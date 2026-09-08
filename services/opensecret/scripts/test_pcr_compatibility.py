#!/usr/bin/env python3
"""Offline regression tests using existing public signed-history entries."""

import copy
import importlib.util
import json
from pathlib import Path
import subprocess
import tempfile
import unittest
from unittest import mock


SPEC = importlib.util.spec_from_file_location("pcr_compatibility", Path(__file__).with_name("pcr_compatibility.py"))
pcr = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(pcr)
SOURCE = Path(__file__).resolve().parents[1]


def encode(value):
    return (json.dumps(value, indent=2) + "\n").encode()


def fixture(count=2, current_index=-1):
    blobs = {}
    for environment in ("Dev", "Prod"):
        history_name = f"pcr{environment}History.json"
        history = json.loads((SOURCE / history_name).read_bytes())[:count]
        current = {"HashAlgorithm": "Sha384 { ... }"}
        current.update({field: history[current_index][field] for field in pcr.PCR_FIELDS})
        blobs[history_name] = encode(history)
        blobs[f"pcr{environment}.json"] = encode(current)
    return blobs


class ValidationTests(unittest.TestCase):
    def assert_bad(self, blobs, message):
        with self.assertRaisesRegex(pcr.ValidationError, message):
            pcr.validate_bundle(blobs)

    def test_checked_in_files_verify_with_sdk_public_key(self):
        histories = pcr.validate_bundle(pcr.read_directory(SOURCE))
        self.assertTrue(all(histories.values()))
        maple = SOURCE.parents[1]
        for relative in ("sdk/src/lib/pcr.ts", "sdk/rust/src/pcr.rs"):
            self.assertIn(pcr.PUBLIC_KEY_B64, (maple / relative).read_text())

    def test_append_preserves_prefix(self):
        histories = pcr.validate_extension(fixture(2), fixture(1))
        self.assertEqual(len(histories["pcrDevHistory.json"]), 2)

    def test_current_can_reference_earlier_signed_entry_for_rollback(self):
        pcr.validate_bundle(fixture(2, current_index=0))

    def test_invalid_signature_rejected(self):
        blobs = fixture()
        entries = json.loads(blobs["pcrDevHistory.json"])
        entries[0]["signature"] = "A" * 128
        blobs["pcrDevHistory.json"] = encode(entries)
        self.assert_bad(blobs, "signature is invalid")

    def test_schema_and_encoding_fail_closed(self):
        cases = [
            ("PCR0", "0" * 96, "nonzero"),
            ("PCR1", "A" * 96, "lowercase"),
            ("PCR2", "a", "96"),
            ("timestamp", True, "safe integer"),
            ("timestamp", 0, "safe integer"),
            ("timestamp", 2**53, "safe integer"),
            ("signature", "+" * 127, "encoding"),
            ("signature", "!" * 128, "encoding"),
        ]
        for field, value, message in cases:
            with self.subTest(field=field, value=value):
                blobs = fixture()
                entries = json.loads(blobs["pcrDevHistory.json"])
                entries[0][field] = value
                blobs["pcrDevHistory.json"] = encode(entries)
                self.assert_bad(blobs, message)

    def test_empty_and_nonarray_history_rejected(self):
        for value in ([], {}):
            with self.subTest(kind=type(value), count=len(value)):
                blobs = fixture()
                blobs["pcrDevHistory.json"] = encode(value)
                self.assert_bad(blobs, r"1\.\.2048")

    def test_entry_limit_is_enforced_below_the_byte_limit(self):
        blobs = fixture()
        entry = json.loads(blobs["pcrDevHistory.json"])[0]
        blobs["pcrDevHistory.json"] = json.dumps(
            [entry] * (pcr.MAX_ENTRIES + 1), separators=(",", ":")
        ).encode()
        self.assertLess(len(blobs["pcrDevHistory.json"]), pcr.MAX_BYTES)
        self.assert_bad(blobs, r"1\.\.2048")

    def test_payload_limit_and_malformed_json_rejected(self):
        for data, message in ((b" " * (pcr.MAX_BYTES + 1), "1 MiB"), (b"[", "invalid UTF-8 JSON"), (b"\xff", "invalid UTF-8 JSON")):
            with self.subTest(message=message):
                blobs = fixture()
                blobs["pcrDevHistory.json"] = data
                self.assert_bad(blobs, message)

    def test_duplicate_json_field_rejected(self):
        blobs = fixture()
        blobs["pcrDev.json"] = blobs["pcrDev.json"].replace(b'"HashAlgorithm":', b'"PCR0": "a", "HashAlgorithm":')
        self.assert_bad(blobs, "Duplicate JSON field")

    def test_duplicate_pcr0_rejected(self):
        blobs = fixture()
        entries = json.loads(blobs["pcrDevHistory.json"])
        entries.append(copy.deepcopy(entries[0]))
        blobs["pcrDevHistory.json"] = encode(entries)
        self.assert_bad(blobs, "duplicate PCR0")

    def test_current_must_match_one_history_entry(self):
        blobs = fixture()
        current = json.loads(blobs["pcrDev.json"])
        current["PCR2"] = "1" * 96
        blobs["pcrDev.json"] = encode(current)
        self.assert_bad(blobs, "do not match")

    def test_prefix_rejects_truncation_reordering_and_unsigned_metadata_change(self):
        baseline = fixture(2)
        with self.assertRaisesRegex(pcr.ValidationError, "truncates, reorders, or changes"):
            pcr.validate_extension(fixture(1), baseline)
        for mutate in (lambda entries: entries.reverse(), lambda entries: entries[0].update(timestamp=1)):
            source = fixture(2)
            entries = json.loads(source["pcrDevHistory.json"])
            mutate(entries)
            source["pcrDevHistory.json"] = encode(entries)
            pcr.validate_bundle(source)  # Timestamp/order are not authenticated by PCR0 signatures.
            with self.assertRaisesRegex(pcr.ValidationError, "truncates, reorders, or changes"):
                pcr.validate_extension(source, baseline)

    def test_unknown_fields_rejected(self):
        blobs = fixture()
        entries = json.loads(blobs["pcrDevHistory.json"])
        entries[0]["unexpected"] = True
        blobs["pcrDevHistory.json"] = encode(entries)
        self.assert_bad(blobs, "invalid fields")


class PreparationTests(unittest.TestCase):
    def setUp(self):
        self.temporary = tempfile.TemporaryDirectory(prefix="pcr-compatibility-test-")
        self.addCleanup(self.temporary.cleanup)
        root = Path(self.temporary.name)
        self.source = root / "maple"
        self.legacy = root / "opensecret"
        self.new_blobs = fixture(2)
        self.old_blobs = fixture(1)
        self.source_ref = self.repo(self.source, "MaplePrivacyLabs/Maple", self.new_blobs, "services/opensecret/")
        self.legacy_ref = self.repo(self.legacy, "OpenSecretCloud/opensecret", self.old_blobs)

    def command(self, repo, *arguments):
        return subprocess.check_output(
            ["git", "-c", "core.hooksPath=/dev/null", "-c", "commit.gpgsign=false", "-C", str(repo), *arguments],
            stderr=subprocess.DEVNULL,
        ).decode().strip()

    def repo(self, root, remote, blobs, prefix=""):
        root.mkdir()
        self.command(root, "init", "--initial-branch=fixture")
        self.command(root, "config", "user.name", "PCR test fixture")
        self.command(root, "config", "user.email", "pcr-test@example.invalid")
        self.command(root, "remote", "add", "origin", f"https://github.com/{remote}.git")
        for name, data in blobs.items():
            path = root / prefix / name
            path.parent.mkdir(parents=True, exist_ok=True)
            path.write_bytes(data)
        self.command(root, "add", ".")
        self.command(root, "commit", "-m", "Synthetic repository with existing public PCR fixtures")
        ref = self.command(root, "rev-parse", "HEAD")
        self.command(root, "update-ref", "refs/remotes/origin/master", ref)
        return ref

    def prepare(self):
        return pcr.prepare(self.source, self.source_ref, self.legacy, self.legacy_ref)

    def test_dry_run_then_exact_unstaged_copy(self):
        source, baseline, _ = self.prepare()
        self.assertEqual(pcr.read_directory(self.legacy), self.old_blobs)
        pcr.copy_prepared(self.legacy, source, baseline)
        self.assertEqual(pcr.read_directory(self.legacy), self.new_blobs)
        self.assertEqual(self.command(self.legacy, "diff", "--cached", "--name-only"), "")
        self.assertEqual(set(self.command(self.legacy, "diff", "--name-only").splitlines()), set(pcr.FILES))
        self.assertEqual(self.command(self.legacy, "rev-parse", "HEAD"), self.legacy_ref)

    def test_dirty_worktree_and_index_rejected(self):
        (self.legacy / "unrelated.txt").write_text("keep this")
        with self.assertRaisesRegex(pcr.ValidationError, "must be clean"):
            self.prepare()
        self.command(self.legacy, "add", "unrelated.txt")
        with self.assertRaisesRegex(pcr.ValidationError, "must be clean"):
            self.prepare()

    def test_wrong_origin_rejected(self):
        self.command(self.legacy, "remote", "set-url", "origin", "https://github.com/attacker/opensecret.git")
        with self.assertRaisesRegex(pcr.ValidationError, "Expected origin"):
            self.prepare()

    def test_stale_legacy_head_or_tracking_ref_rejected(self):
        (self.legacy / "notice.txt").write_text("public notice")
        self.command(self.legacy, "add", "notice.txt")
        self.command(self.legacy, "commit", "-m", "Advance fixture")
        new_ref = self.command(self.legacy, "rev-parse", "HEAD")
        with self.assertRaisesRegex(pcr.ValidationError, "Legacy HEAD differs"):
            self.prepare()
        with self.assertRaisesRegex(pcr.ValidationError, "origin/master differs"):
            pcr.prepare(self.source, self.source_ref, self.legacy, new_ref)

    def test_mutable_ref_and_nonroot_rejected(self):
        with self.assertRaisesRegex(pcr.ValidationError, "immutable full"):
            pcr.prepare(self.source, "HEAD", self.legacy, self.legacy_ref)
        with self.assertRaisesRegex(pcr.ValidationError, "exact repository worktree root"):
            pcr.prepare(self.source / "services/opensecret", self.source_ref, self.legacy, self.legacy_ref)

    def test_symlink_rejected_even_when_bytes_match(self):
        target = self.legacy / "pcrDev.json"
        target.unlink()
        target.symlink_to(self.source / "services/opensecret/pcrDev.json")
        with self.assertRaisesRegex(pcr.ValidationError, "not a symlink"):
            pcr.read_directory(self.legacy)

    def test_changed_target_between_plan_and_copy_rejected(self):
        source, baseline, _ = self.prepare()
        (self.legacy / "pcrDev.json").write_bytes(b"{}")
        with self.assertRaisesRegex(pcr.ValidationError, "changed after validation"):
            pcr.copy_prepared(self.legacy, source, baseline)

    def test_write_failure_restores_already_copied_files(self):
        source, baseline, _ = self.prepare()
        original = pcr.atomic_write
        calls = 0

        def fail_second(path, data):
            nonlocal calls
            calls += 1
            if calls == 2:
                raise OSError("synthetic write failure")
            original(path, data)

        with mock.patch.object(pcr, "atomic_write", side_effect=fail_second):
            with self.assertRaisesRegex(OSError, "synthetic write failure"):
                pcr.copy_prepared(self.legacy, source, baseline)
        self.assertEqual(pcr.read_directory(self.legacy), self.old_blobs)


if __name__ == "__main__":
    unittest.main()
