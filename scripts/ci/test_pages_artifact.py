#!/usr/bin/env python3
"""Adversarial archive tests; fixtures contain no credentials or production data."""

import contextlib
import gzip
import hashlib
import io
import json
import os
from pathlib import Path
import stat
import struct
import sys
import tarfile
import tempfile
import unittest
from unittest.mock import patch
import warnings
import zipfile

import pages_artifact as artifact


SHA = "a" * 40
RUN_ID = 12345
ATTEMPT = 2
CANARY = "UNTRUSTED_TEST_CANARY\n::error::injected"


def tar_bytes(entries: list[tuple[str, bytes | None]], **options) -> bytes:
    result = io.BytesIO()
    with tarfile.open(fileobj=result, mode="w:gz", format=tarfile.GNU_FORMAT) as bundle:
        for name, contents in entries:
            member = tarfile.TarInfo(name)
            member.mode = 0o7777
            member.uid = member.gid = 65534
            member.mtime = 1234567890
            if contents is None:
                member.type = tarfile.DIRTYPE
            else:
                member.size = len(contents)
            for key, value in options.items():
                setattr(member, key, value)
            bundle.addfile(member, None if contents is None else io.BytesIO(contents))
    return result.getvalue()


class PagesArtifactTests(unittest.TestCase):
    def setUp(self) -> None:
        self.temporary = tempfile.TemporaryDirectory()
        self.addCleanup(self.temporary.cleanup)
        self.root = Path(self.temporary.name)
        self.archive = self.root / artifact.ARCHIVE_NAME
        self.destination = self.root / "dist"
        self.preview = self.root / "preview.zip"
        self.output_archive = self.root / "downloaded.tar.gz"
        self.write_archive(
            [
                ("./assets/", None),
                ("./assets/index-a123.js", b'console.log("fixture")'),
                ("./assets/index-b123.css", b"body { color: #000; }"),
                ("./favicon.svg", b"<svg></svg>"),
                ("./index.html", b'<script src="/assets/index-a123.js"></script>'),
            ]
        )

    def write_archive(self, entries: list[tuple[str, bytes | None]], **options) -> str:
        self.archive.write_bytes(tar_bytes(entries, **options))
        return self.digest()

    def digest(self) -> str:
        return hashlib.sha256(self.archive.read_bytes()).hexdigest()

    def manifest(self) -> dict:
        return artifact.pack_manifest(self.archive, "pr", SHA, RUN_ID, ATTEMPT)

    def write_zip(self, entries=None, manifest=None, archive=None) -> None:
        if entries is None:
            entries = [
                (artifact.ARCHIVE_NAME, self.archive.read_bytes() if archive is None else archive),
                (artifact.MANIFEST_NAME, json.dumps(self.manifest() if manifest is None else manifest).encode()),
            ]
        with warnings.catch_warnings():
            warnings.simplefilter("ignore", UserWarning)
            with zipfile.ZipFile(self.preview, "w", compression=zipfile.ZIP_DEFLATED) as bundle:
                for name, data in entries:
                    bundle.writestr(name, data)

    def read_zip(self, **kwargs) -> dict:
        return artifact.read_preview_zip(
            self.preview,
            kwargs.get("sha", SHA),
            kwargs.get("run_id", RUN_ID),
            kwargs.get("attempt", ATTEMPT),
            self.output_archive,
        )

    def assert_rejected(self, operation) -> None:
        with self.assertRaises(artifact.ArtifactError) as rejected:
            operation()
        self.assertNotIn(CANARY, str(rejected.exception))
        self.assertNotIn("\n", str(rejected.exception))
        self.assertFalse(self.destination.exists())
        self.assertFalse(self.output_archive.exists())
        self.assertEqual(list(self.root.glob(".pages-*")), [])

    def test_manifest_fields_and_release_profile(self) -> None:
        manifest = artifact.pack_manifest(self.archive, "release", SHA, RUN_ID, ATTEMPT)
        self.assertEqual(set(manifest), artifact.MANIFEST_FIELDS)
        self.assertEqual(manifest["profile"], "release")
        self.assertEqual(manifest["archive_sha256"], self.digest())

    def test_manifest_strict_types_and_unknown_fields(self) -> None:
        mutations = [
            ("schema_version", True), ("schema_version", 2),
            ("profile", "production"), ("profile", CANARY), ("profile", []),
            ("source_sha", SHA.upper()), ("source_sha", "a" * 39),
            ("archive_sha256", "g" * 64), ("archive_sha256", CANARY),
            ("run_id", True), ("run_id", "12345"), ("run_id", 0),
            ("run_attempt", 0), ("run_attempt", 2.0), ("unknown", CANARY),
        ]
        for key, value in mutations:
            with self.subTest(key=key, value=value):
                manifest = self.manifest()
                manifest[key] = value
                self.assert_rejected(lambda: artifact.validate_manifest(manifest))
        manifest = self.manifest()
        del manifest["source_sha"]
        self.assert_rejected(lambda: artifact.validate_manifest(manifest))

    def test_manifest_creation_bounds_archive(self) -> None:
        with patch.object(artifact, "MAX_ARCHIVE_BYTES", 4):
            self.assert_rejected(self.manifest)

    def test_valid_github_zip_and_real_shaped_static_archive(self) -> None:
        self.write_zip()
        manifest = self.read_zip()
        self.assertEqual(self.output_archive.read_bytes(), self.archive.read_bytes())
        hashes = artifact.extract_static(self.output_archive, self.destination, manifest["archive_sha256"])
        self.assertEqual(set(hashes), {"index.html", "favicon.svg", "assets/index-a123.js", "assets/index-b123.css"})
        for path, digest in hashes.items():
            output = self.destination / path
            self.assertEqual(hashlib.sha256(output.read_bytes()).hexdigest(), digest)
            self.assertEqual(stat.S_IMODE(output.stat().st_mode), 0o644)
        self.assertEqual(stat.S_IMODE(self.destination.stat().st_mode), 0o755)
        self.assertEqual(stat.S_IMODE((self.destination / "assets").stat().st_mode), 0o755)

    def test_zip_requires_matching_profile_sha_run_and_attempt(self) -> None:
        for key, value in [("profile", "release"), ("source_sha", "b" * 40), ("run_id", 12346), ("run_attempt", 1)]:
            with self.subTest(field=key):
                manifest = self.manifest()
                manifest[key] = value
                self.write_zip(manifest=manifest)
                self.assert_rejected(self.read_zip)

    def test_zip_rejects_invalid_expected_identity(self) -> None:
        self.write_zip()
        for arguments in ({"sha": CANARY}, {"run_id": True}, {"attempt": 0}):
            with self.subTest(arguments=arguments):
                self.assert_rejected(lambda: self.read_zip(**arguments))

    def test_zip_digest_is_checked_before_publishing(self) -> None:
        self.write_zip(archive=b"different bytes")
        self.assert_rejected(self.read_zip)

    def test_zip_rejects_traversal_extra_missing_and_duplicate_entries(self) -> None:
        payload = json.dumps(self.manifest()).encode()
        for names in (
            ["../maple-web-dist.tar.gz", artifact.MANIFEST_NAME],
            ["/maple-web-dist.tar.gz", artifact.MANIFEST_NAME],
            ["sub/maple-web-dist.tar.gz", artifact.MANIFEST_NAME],
            [artifact.ARCHIVE_NAME, artifact.ARCHIVE_NAME],
            [artifact.ARCHIVE_NAME],
            [artifact.ARCHIVE_NAME, artifact.MANIFEST_NAME, "extra"],
            [artifact.ARCHIVE_NAME, "bad\n::error::injected"],
        ):
            with self.subTest(names=names):
                self.write_zip([(name, payload) for name in names])
                self.assert_rejected(self.read_zip)

    def test_zip_rejects_symlinks_directories_and_special_files(self) -> None:
        for unix_type in (stat.S_IFLNK, stat.S_IFDIR, stat.S_IFIFO, stat.S_IFCHR):
            with self.subTest(unix_type=unix_type):
                member = zipfile.ZipInfo(artifact.ARCHIVE_NAME)
                member.create_system = 3
                member.external_attr = (unix_type | 0o755) << 16
                self.write_zip([(member, b"ignored"), (artifact.MANIFEST_NAME, json.dumps(self.manifest()).encode())])
                self.assert_rejected(self.read_zip)

    def test_zip_rejects_dos_directory_attribute(self) -> None:
        member = zipfile.ZipInfo(artifact.ARCHIVE_NAME)
        member.external_attr = 0x10
        self.write_zip([(member, b"ignored"), (artifact.MANIFEST_NAME, json.dumps(self.manifest()).encode())])
        self.assert_rejected(self.read_zip)

    def test_zip_rejects_duplicate_manifest_fields_and_invalid_json(self) -> None:
        for data in (b'{"profile":"pr","profile":"release"}', b"\xff", b"[", b"null"):
            with self.subTest(data=data):
                self.write_zip([(artifact.ARCHIVE_NAME, self.archive.read_bytes()), (artifact.MANIFEST_NAME, data)])
                self.assert_rejected(self.read_zip)

    def test_zip_manifest_and_archive_size_limits(self) -> None:
        self.write_zip()
        for limit in ("MAX_MANIFEST_BYTES", "MAX_ARCHIVE_BYTES"):
            with self.subTest(limit=limit), patch.object(artifact, limit, 4):
                self.assert_rejected(self.read_zip)

    def test_zip_entry_count_checked_before_parser_allocates_entries(self) -> None:
        self.write_zip()
        contents = bytearray(self.preview.read_bytes())
        end = contents.rfind(b"PK\x05\x06")
        struct.pack_into("<HH", contents, end + 8, 65000, 65000)
        self.preview.write_bytes(contents)
        with patch.object(artifact.zipfile, "ZipFile", side_effect=AssertionError("Parser must not run")):
            self.assert_rejected(self.read_zip)

    def test_zip_never_overwrites_an_existing_output(self) -> None:
        self.write_zip()
        self.output_archive.write_bytes(b"keep")
        with self.assertRaises(artifact.ArtifactError):
            self.read_zip()
        self.assertEqual(self.output_archive.read_bytes(), b"keep")

    def test_tar_path_traversal_and_log_injection(self) -> None:
        for name in (
            "../outside", "/outside", "./../outside", "a/../../outside", "a//b", "a/./b",
            "a\\b", "C:/outside", "a\nb", "a\rb", "a\x1bb", "a\u202eb", CANARY,
            "././index.html", ".env", "a/.git/config", "a/.well-known/file",
            "assets/\u0065\u0301.txt",
        ):
            with self.subTest(name=name):
                digest = self.write_archive([("index.html", b"safe"), (name, b"bad")])
                self.assert_rejected(lambda: artifact.extract_static(self.archive, self.destination, digest))
        self.assertFalse((self.root / "outside").exists())

    def test_tar_rejects_runtime_and_configuration_files_at_any_depth(self) -> None:
        for component in artifact.RESERVED_COMPONENTS:
            for name in (component, "assets/" + component.upper(), "a/" + component + "/payload"):
                with self.subTest(name=name):
                    digest = self.write_archive([("index.html", b"safe"), (name, b"bad")])
                    self.assert_rejected(lambda: artifact.extract_static(self.archive, self.destination, digest))

    def test_tar_preserves_public_root_mobile_association_documents(self) -> None:
        documents = {
            ".well-known/apple-app-site-association": b'{"applinks":{"details":[]}}',
            ".well-known/assetlinks.json": b"[]",
        }
        digest = self.write_archive(
            [("index.html", b"safe"), ("./.well-known/", None)]
            + [("./" + name, contents) for name, contents in documents.items()]
        )
        hashes = artifact.extract_static(self.archive, self.destination, digest)
        for name, contents in documents.items():
            self.assertEqual((self.destination / name).read_bytes(), contents)
            self.assertEqual(hashes[name], hashlib.sha256(contents).hexdigest())

    def test_well_known_exception_cannot_expose_hidden_or_runtime_files(self) -> None:
        forbidden = [
            ".well-known", ".WELL-KNOWN/assetlinks.json", "a/.well-known/assetlinks.json",
            ".well-known/.env", ".well-known/.git/config",
            ".well-known/a/.well-known/assetlinks.json",
        ] + [".well-known/" + name for name in artifact.RESERVED_COMPONENTS]
        for name in forbidden:
            with self.subTest(name=name):
                digest = self.write_archive([("index.html", b"safe"), (name, b"bad")])
                self.assert_rejected(lambda: artifact.extract_static(self.archive, self.destination, digest))

    def test_tar_rejects_links_devices_fifo_and_sparse(self) -> None:
        for member_type in (tarfile.SYMTYPE, tarfile.LNKTYPE, tarfile.CHRTYPE, tarfile.BLKTYPE, tarfile.FIFOTYPE, tarfile.GNUTYPE_SPARSE):
            with self.subTest(member_type=member_type):
                digest = self.write_archive([("index.html", b"")], type=member_type, linkname="/outside")
                self.assert_rejected(lambda: artifact.extract_static(self.archive, self.destination, digest))

    def test_tar_rejects_duplicate_and_case_conflicting_paths(self) -> None:
        for first, second in (
            ("index.html", "./index.html"),
            ("assets/a.js", "assets/a.js"),
            ("assets/a.js", "assets/A.js"),
            ("ASSETS/a.js", "assets/b.js"),
            ("assets", "assets/a.js"),
            ("assets/a.js", "assets"),
        ):
            with self.subTest(paths=(first, second)):
                digest = self.write_archive([(first, b"safe"), (second, b"bad")])
                self.assert_rejected(lambda: artifact.extract_static(self.archive, self.destination, digest))

    def test_tar_rejects_duplicate_directories(self) -> None:
        digest = self.write_archive([("assets/", None), ("./assets/", None), ("index.html", b"safe")])
        self.assert_rejected(lambda: artifact.extract_static(self.archive, self.destination, digest))

    def test_tar_requires_root_index_html(self) -> None:
        digest = self.write_archive([("assets/index.html", b"safe")])
        self.assert_rejected(lambda: artifact.extract_static(self.archive, self.destination, digest))

    def test_tar_bounds_compressed_input_and_each_file(self) -> None:
        for limit in ("MAX_ARCHIVE_BYTES", "MAX_FILE_BYTES"):
            with self.subTest(limit=limit), patch.object(artifact, limit, 4):
                self.assert_rejected(lambda: artifact.extract_static(self.archive, self.destination, self.digest()))

    def test_tar_bounds_entry_count(self) -> None:
        with patch.object(artifact, "MAX_ENTRIES", 2):
            self.assert_rejected(lambda: artifact.extract_static(self.archive, self.destination, self.digest()))

    def test_tar_bounds_decompression_and_metadata(self) -> None:
        digest = self.write_archive([("index.html", b"a" * 8000)])
        with patch.object(artifact, "MAX_EXPANDED_BYTES", 4096):
            self.assert_rejected(lambda: artifact.extract_static(self.archive, self.destination, digest))
        digest = self.write_archive([("index.html", b"safe"), ("a" * 200, b"safe")])
        with patch.object(artifact, "MAX_EXPANDED_BYTES", 1536):
            self.assert_rejected(lambda: artifact.extract_static(self.archive, self.destination, digest))

    def test_tar_bounds_compressed_data_after_tar_end(self) -> None:
        self.archive.write_bytes(self.archive.read_bytes() + gzip.compress(b"a" * 100_000))
        with patch.object(artifact, "MAX_EXPANDED_BYTES", 20_000):
            self.assert_rejected(lambda: artifact.extract_static(self.archive, self.destination, self.digest()))

    def test_tar_invalid_gzip_and_wrong_digest(self) -> None:
        self.assert_rejected(lambda: artifact.extract_static(self.archive, self.destination, "0" * 64))
        self.archive.write_bytes(b"not gzip " + CANARY.encode())
        self.assert_rejected(lambda: artifact.extract_static(self.archive, self.destination, self.digest()))

    def test_tar_truncated_gzip_is_rejected(self) -> None:
        self.archive.write_bytes(self.archive.read_bytes()[:-4])
        self.assert_rejected(lambda: artifact.extract_static(self.archive, self.destination, self.digest()))

    def test_tar_rejects_symlink_input_and_preserves_destination(self) -> None:
        link = self.root / "linked.tar.gz"
        link.symlink_to(self.archive)
        self.assert_rejected(lambda: artifact.extract_static(link, self.destination, self.digest()))
        self.destination.mkdir()
        (self.destination / "existing").write_bytes(b"keep")
        with self.assertRaises(artifact.ArtifactError):
            artifact.extract_static(self.archive, self.destination, self.digest())
        self.assertEqual((self.destination / "existing").read_bytes(), b"keep")

    def test_tar_accepts_empty_existing_destination(self) -> None:
        self.destination.mkdir()
        artifact.extract_static(self.archive, self.destination, self.digest())
        self.assertTrue((self.destination / "index.html").is_file())

    def test_tar_rejects_symlink_destination(self) -> None:
        outside = self.root / "outside"
        outside.mkdir()
        self.destination.symlink_to(outside, target_is_directory=True)
        with self.assertRaises(artifact.ArtifactError):
            artifact.extract_static(self.archive, self.destination, self.digest())
        self.assertEqual(list(outside.iterdir()), [])

    def test_nested_archive_is_inert_static_bytes(self) -> None:
        nested = tar_bytes([("../outside", b"bad")])
        digest = self.write_archive([("index.html", b"safe"), ("assets/example.tar.gz", nested)])
        hashes = artifact.extract_static(self.archive, self.destination, digest)
        self.assertEqual(hashes["assets/example.tar.gz"], hashlib.sha256(nested).hexdigest())
        self.assertFalse((self.root / "outside").exists())

    def test_manifest_cli_and_safe_errors(self) -> None:
        output = self.root / "manifest.json"
        arguments = ["pages_artifact.py", "manifest", "--archive", str(self.archive), "--profile", "pr", "--sha", SHA, "--run-id", str(RUN_ID), "--run-attempt", str(ATTEMPT), "--output", str(output)]
        with patch.object(sys, "argv", arguments):
            self.assertEqual(artifact.main(), 0)
        self.assertEqual(json.loads(output.read_text()), self.manifest())
        for flag in ("--sha", "--profile", "--run-id", "--archive"):
            with self.subTest(flag=flag):
                invalid = arguments.copy()
                invalid[invalid.index(flag) + 1] = CANARY
                captured = io.StringIO()
                with patch.object(sys, "argv", invalid), contextlib.redirect_stderr(captured), contextlib.redirect_stdout(captured):
                    try:
                        result = artifact.main()
                    except SystemExit as error:
                        result = error.code
                self.assertNotEqual(result, 0)
                self.assertNotIn("UNTRUSTED_TEST_CANARY", captured.getvalue())
                self.assertNotIn("::error::injected", captured.getvalue())


if __name__ == "__main__":
    unittest.main()
