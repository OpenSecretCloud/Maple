"""Source-selection regressions, using small generated package/lock fixtures."""

import copy
import hashlib
import json
from pathlib import Path
import tempfile
import unittest
from unittest.mock import patch

import verify_sources


class SourceSelectionTests(unittest.TestCase):
    def setUp(self):
        temporary = tempfile.TemporaryDirectory(prefix="maple-source-selection-")
        self.addCleanup(temporary.cleanup)
        self.root = Path(temporary.name)
        provenance = copy.deepcopy(verify_sources.PROVENANCE)
        self.metadata = {"workspace_root": str(self.root), "packages": []}
        self.locked = []
        for name, package in provenance["packages"].items():
            directory = self.root / name
            directory.mkdir()
            source = b"small source fixture\n"
            (directory / "source.txt").write_bytes(source)
            package["files_sha256"] = {"source.txt": hashlib.sha256(source).hexdigest()}
            identity = {
                "name": name,
                "version": package["version"],
                "source": (verify_sources.fork_source() if name == "libsqlite3-sys" else
                           "registry+https://github.com/rust-lang/crates.io-index"),
            }
            self.metadata["packages"].append({**identity, "manifest_path": str(directory / "Cargo.toml")})
            self.locked.append({**identity, **({"checksum": package["archive_sha256"]}
                                             if "archive_sha256" in package else {})})
        # rusqlite's own unselected vendored-OpenSSL feature can retain this
        # old source in its lockfile without selecting it for the test build.
        self.active_openssl = next(package for package in self.locked if package["name"] == "openssl-src")
        self.active_metadata_openssl = next(
            package for package in self.metadata["packages"] if package["name"] == "openssl-src"
        )
        self.locked.append({
            "name": "openssl-src", "version": "300.6.1+3.6.3",
            "source": "registry+https://github.com/rust-lang/crates.io-index",
            "checksum": "0" * 64,
        })
        substitution = patch.object(verify_sources, "PROVENANCE", provenance)
        substitution.start()
        self.addCleanup(substitution.stop)

    def verify(self):
        metadata = self.root / "metadata.json"
        metadata.write_text(json.dumps(self.metadata, ensure_ascii=False), encoding="utf-8")
        (self.root / "Cargo.lock").write_text("\n".join(
            "[[package]]\n" + "\n".join(f"{key} = {json.dumps(value)}" for key, value in package.items())
            for package in self.locked
        ), encoding="utf-8")
        return verify_sources.source_packages(metadata)

    def test_inactive_locked_version_is_allowed(self):
        self.assertEqual(set(self.verify()), {"sqlcipher-src", "openssl-src"})

    def test_utf8_metadata_with_windows_default_encoding(self):
        self.metadata["description"] = "Password metadata: Ё / 密码"
        original = Path.read_text

        def windows_default(path, *arguments, **keywords):
            if not arguments and keywords.get("encoding") is None:
                keywords["encoding"] = "cp1252"
            return original(path, *arguments, **keywords)

        with patch.object(Path, "read_text", windows_default):
            self.assertEqual(set(self.verify()), {"sqlcipher-src", "openssl-src"})

    def test_selected_locked_source_is_required(self):
        self.active_openssl["source"] = "git+https://example.invalid/provider"
        with self.assertRaisesRegex(ValueError, "openssl-src lock entry differs"):
            self.verify()

    def test_selected_archive_checksum_is_required(self):
        self.active_openssl["checksum"] = "0" * 64
        with self.assertRaisesRegex(ValueError, "openssl-src checksum differs"):
            self.verify()

    def test_multiple_selected_versions_are_rejected(self):
        self.metadata["packages"].append({**self.active_metadata_openssl, "version": "300.6.1+3.6.3"})
        with self.assertRaisesRegex(ValueError, "expected one openssl-src"):
            self.verify()


if __name__ == "__main__":
    unittest.main()
