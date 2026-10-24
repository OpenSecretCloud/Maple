"""Trust boundary tests, independent of GitHub, credentials and signing keys."""

import base64
from copy import deepcopy
import json
import io
from pathlib import Path
import tempfile
import unittest
from unittest.mock import patch

import publisher as p


class TrustTests(unittest.TestCase):
    def setUp(self):
        self.run = {
            "repository": {"full_name": p.REPOSITORY},
            "head_repository": {"full_name": p.REPOSITORY},
            "path": p.WORKFLOW, "head_branch": "master", "event": "push",
            "status": "completed", "conclusion": "failure", "head_sha": "a" * 40,
            "id": 12, "run_attempt": 2, "run_number": 42,
            "created_at": "2026-10-21T01:02:03Z",
        }
        self.jobs = {"total_count": 5, "jobs": [
            {"name": name, "status": "completed", "conclusion": "success"}
            for name in ("Signed macOS (prod)", "Verify downloaded macOS (prod)",
                         "Linux bundle (prod)", "Attest Linux packages (prod)",
                         "Verify downloaded Linux (prod)")
        ]}

    def test_actual_prod_consumers_gate_even_if_dev_failed(self):
        self.assertEqual(p.validate_run(self.run, self.jobs, True), self.run)

    def test_skipped_failed_missing_duplicate_and_incomplete_jobs_fail(self):
        cases = []
        for conclusion in ("skipped", "failure", "cancelled", "neutral", None):
            jobs = deepcopy(self.jobs)
            jobs["jobs"][1]["conclusion"] = conclusion
            cases.append(jobs)
        missing = deepcopy(self.jobs)
        missing["jobs"].pop(1)
        missing["total_count"] -= 1
        cases.append(missing)
        duplicate = deepcopy(self.jobs)
        duplicate["jobs"].append(deepcopy(duplicate["jobs"][1]))
        duplicate["total_count"] += 1
        cases.append(duplicate)
        incomplete = deepcopy(self.jobs)
        incomplete["total_count"] += 1
        cases.append(incomplete)
        for jobs in cases:
            with self.subTest(jobs=jobs), self.assertRaises(p.PublishError):
                p.validate_run(self.run, jobs)

    def test_linux_is_required_only_when_requested(self):
        self.jobs["jobs"][4]["conclusion"] = "failure"
        p.validate_run(self.run, self.jobs, False)
        with self.assertRaises(p.PublishError):
            p.validate_run(self.run, self.jobs, True)

    def test_fork_pull_request_branch_and_other_workflow_fail(self):
        replacements = {
            "repository": {"full_name": "elsewhere/Maple"},
            "head_repository": {"full_name": "elsewhere/Maple"},
            "event": "pull_request", "head_branch": "feature",
            "path": ".github/workflows/other.yml", "head_sha": "A" * 40,
            "status": "in_progress", "run_attempt": 0,
        }
        for key, value in replacements.items():
            run = {**self.run, key: value}
            with self.subTest(key=key), self.assertRaises(p.PublishError):
                p.validate_run(run, self.jobs)

    def test_trust_uses_latest_jobs_including_successful_prior_attempt_and_final_artifact(self):
        artifacts = {"total_count": 1, "artifacts": [
            {"name": "maple-agent-prod-macos-aarch64-12", "expired": False}
        ]}
        with patch.object(p, "gh_json", side_effect=[self.run, self.jobs, artifacts]) as api:
            proof = p.trust_run("12")
        self.assertEqual(proof["build"], "42")
        self.assertIn("/jobs?filter=latest", api.call_args_list[1].args[0])
        self.assertEqual(proof["run_attempt"], 2)
        self.assertEqual(proof["artifacts"]["macos"], "maple-agent-prod-macos-aarch64-12")
        artifacts["artifacts"][0]["name"] += "-unsigned"
        with patch.object(p, "gh_json", side_effect=[self.run, self.jobs, artifacts]), self.assertRaises(p.PublishError):
            p.trust_run("12")

    def test_durable_audit_ignores_diagnostic_attempt_but_binds_build_identity(self):
        proof = {"run_id": 12, "run_attempt": 1, "source_sha": "a" * 40,
                 "build": "42", "pub_date": "2026-10-21T01:02:03Z",
                 "artifacts": {"macos": "maple-agent-prod-macos-aarch64-12"}}
        original = p.durable_provenance(proof)
        self.assertEqual(original, p.durable_provenance({**proof, "run_attempt": 2}))
        self.assertNotEqual(original, p.durable_provenance({**proof, "source_sha": "b" * 40}))
        self.assertNotIn("run_attempt", json.loads(original))

    def test_stream_hash_reads_only_expected_size_plus_one_on_oversized_response(self):
        stream = io.BytesIO(b"oversized response body")
        with self.assertRaisesRegex(p.PublishError, "oversized"):
            p.hash_stream(stream, maximum=3)
        self.assertEqual(stream.tell(), 4)


class PinTests(unittest.TestCase):
    def setUp(self):
        self.key = base64.b64encode(b"Ed" + b"a" * 40).decode("ascii")
        self.profile = {"prod": {"update_feed_url": p.FEED_URL, "update_public_keys": [self.key]}}

    def test_key_must_be_in_both_source_and_trusted_profiles(self):
        p.validate_pins(self.key, self.profile, deepcopy(self.profile))
        for side in (0, 1):
            for change in ([], [self.key + "x"]):
                profiles = [deepcopy(self.profile), deepcopy(self.profile)]
                profiles[side]["prod"]["update_public_keys"] = change
                with self.subTest(side=side, change=change), self.assertRaises(p.PublishError):
                    p.validate_pins(self.key, *profiles)

    def test_feed_mismatch_and_missing_pins_fail(self):
        for bad in ({"prod": {}}, {"prod": {"update_feed_url": "https://elsewhere.invalid", "update_public_keys": [self.key]}}):
            with self.assertRaises(p.PublishError):
                p.validate_pins(self.key, self.profile, bad)

    def test_feed_migration_requires_the_canonical_host_in_both_trust_profiles(self):
        profile = deepcopy(self.profile)
        profile["prod"]["linux_update_feed_url"] = p.LINUX_FEED_URL
        # A new publisher must not prepare an old-feed binary as though the
        # hostname move were compiled into it, or accept stale trusted pins.
        for side in (0, 1):
            for field, legacy in (
                ("update_feed_url", "https://updates.trymaple.ai/agent/stable/latest.json"),
                ("linux_update_feed_url", "https://updates.trymaple.ai/agent/stable/linux/latest.json"),
            ):
                profiles = [deepcopy(profile), deepcopy(profile)]
                profiles[side]["prod"][field] = legacy
                with self.subTest(side=side, field=field), self.assertRaisesRegex(p.PublishError, "update feed"):
                    p.validate_pins(self.key, *profiles, linux=True)

    def test_checked_in_profile_matches_publisher_feed_pins(self):
        profile = p.decode_json(p.read_bytes(p.ROOT / "apps/maple-agent/release-profiles.json", p.MAX_MANIFEST))
        p.validate_pins(profile["prod"]["update_public_keys"][0], profile, deepcopy(profile), linux=True)

    def test_linux_feed_is_pinned_on_both_profiles_only_when_linux_selected(self):
        # Historical Mac-only producers need no Linux profile field.
        p.validate_pins(self.key, self.profile, deepcopy(self.profile), False)
        profile = deepcopy(self.profile)
        profile["prod"]["linux_update_feed_url"] = p.LINUX_FEED_URL
        p.validate_pins(self.key, profile, deepcopy(profile), True)
        for side in (0, 1):
            profiles = [deepcopy(profile), deepcopy(profile)]
            profiles[side]["prod"]["linux_update_feed_url"] = p.FEED_URL
            with self.subTest(side=side), self.assertRaisesRegex(p.PublishError, "Linux update feed"):
                p.validate_pins(self.key, *profiles, linux=True)

    def test_source_profile_is_read_at_exact_source_sha(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            (root / "apps/maple-agent").mkdir(parents=True)
            (root / "apps/maple-agent/release-profiles.json").write_bytes(p.encode_json(self.profile))
            key = root / "public-key.pub"
            key.write_text("untrusted comment: test\n" + self.key + "\n")
            response = {"encoding": "base64", "content": base64.b64encode(p.encode_json(self.profile)).decode()}
            with patch.object(p, "ROOT", root), patch.object(p, "gh_json", return_value=response) as api:
                self.assertEqual(p.check_source_pins(key, "b" * 40), self.key)
            self.assertTrue(api.call_args.args[0].endswith("?ref=" + "b" * 40))

    def test_public_key_format_is_bounded_and_algorithm_checked(self):
        with tempfile.TemporaryDirectory() as temporary:
            key = Path(temporary) / "key.pub"
            key.write_text(self.key + "\n")
            self.assertEqual(p.public_key_text(key), self.key)
            for value in ("invalid", self.key + "\n" + self.key, base64.b64encode(b"ED" + b"a" * 40).decode(), "x" * (p.MAX_SIGNATURE + 1)):
                key.write_text(value)
                with self.subTest(value=value[:20]), self.assertRaises(p.PublishError):
                    p.public_key_text(key)

    def test_existing_pointer_accepts_primary_when_new_release_selects_backup(self):
        backup = base64.b64encode(b"Ed" + b"b" * 40).decode("ascii")
        profile = deepcopy(self.profile)
        profile["prod"]["update_public_keys"].append(backup)
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            (root / "apps/maple-agent").mkdir(parents=True)
            (root / "apps/maple-agent/release-profiles.json").write_bytes(p.encode_json(profile))
            selected = root / "selected.pub"
            selected.write_text(backup + "\n")
            attempted = []

            def verify(data, path):
                candidate = p.public_key_text(path)
                attempted.append(candidate)
                if candidate == self.key:
                    return {"version": "1.0.0"}
                raise p.PublishError("fixture signature mismatch")

            with patch.object(p, "ROOT", root), patch.object(p, "minisign_verify", side_effect=verify):
                self.assertEqual(p.verify_current(b"signed-by-primary", selected), {"version": "1.0.0"})
            self.assertEqual(attempted, [self.key])
            profile["prod"]["update_public_keys"] = [backup]
            (root / "apps/maple-agent/release-profiles.json").write_bytes(p.encode_json(profile))
            with patch.object(p, "ROOT", root), patch.object(p, "minisign_verify", side_effect=verify), self.assertRaisesRegex(p.PublishError, "not trusted"):
                p.verify_current(b"signed-by-retired-primary", selected)


if __name__ == "__main__":
    unittest.main()
