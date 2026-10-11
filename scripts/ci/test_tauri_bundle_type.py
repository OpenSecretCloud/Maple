"""Fail-closed package identity checks and the bounded DEB-to-RPM patch."""

from pathlib import Path
import stat
import subprocess
import sys
import tempfile
import unittest


HELPER = Path(__file__).with_name("tauri_bundle_type.py")
PREFIX = b"__TAURI_BUNDLE_TYPE_VAR_"


class TauriBundleTypeTests(unittest.TestCase):
    def setUp(self):
        directory = tempfile.TemporaryDirectory()
        self.addCleanup(directory.cleanup)
        self.directory = Path(directory.name)
        self.executable = self.directory / "maple"

    def run_helper(self, *arguments):
        return subprocess.run(
            [sys.executable, "-I", str(HELPER), *arguments, str(self.executable)],
            capture_output=True, text=True, check=False,
        )

    def write_executable(self, data):
        self.executable.write_bytes(data)
        self.executable.chmod(0o751)

    def test_each_supported_kind_verifies_without_writing(self):
        for kind, suffix in (("deb", b"DEB"), ("rpm", b"RPM"), ("appimage", b"APP")):
            with self.subTest(kind=kind):
                data = b"\x7fELF\x00before" + PREFIX + suffix + b"\x00after\xff"
                self.write_executable(data)
                before = self.executable.stat()
                result = self.run_helper("--expect", kind)
                self.assertEqual(result.returncode, 0, result.stderr)
                self.assertEqual(self.executable.read_bytes(), data)
                after = self.executable.stat()
                self.assertEqual(after.st_mtime_ns, before.st_mtime_ns)
                self.assertEqual(stat.S_IMODE(after.st_mode), 0o751)

    def test_patch_changes_only_marker_suffix_and_preserves_mode(self):
        before = b"\x7fELF\x00before" + PREFIX + b"DEB\x00after\xff"
        expected = b"\x7fELF\x00before" + PREFIX + b"RPM\x00after\xff"
        self.write_executable(before)
        result = self.run_helper("--expect", "deb", "--set", "rpm")
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertEqual(self.executable.read_bytes(), expected)
        self.assertEqual(len(expected), len(before))
        self.assertEqual(stat.S_IMODE(self.executable.stat().st_mode), 0o751)
        self.assertEqual(self.run_helper("--expect", "rpm").returncode, 0)
        self.assertNotEqual(self.run_helper("--expect", "deb").returncode, 0)
        self.assertEqual(self.executable.read_bytes(), expected)

    def test_invalid_source_markers_never_mutate_file(self):
        for data in (
            b"no marker",
            PREFIX,
            PREFIX + b"DE",
            PREFIX + b"UNK",
            PREFIX + b"XYZ",
            PREFIX + b"RPM",
            PREFIX + b"APP",
            PREFIX + b"DEB\x00" + PREFIX + b"DEB",
            PREFIX + b"DEB\x00" + PREFIX + b"RPM",
            PREFIX + b"DEB\x00" + PREFIX,
        ):
            with self.subTest(data=data):
                self.write_executable(data)
                result = self.run_helper("--expect", "deb", "--set", "rpm")
                self.assertNotEqual(result.returncode, 0)
                self.assertIn("Tauri bundle type check failed", result.stderr)
                self.assertEqual(self.executable.read_bytes(), data)
                self.assertEqual(stat.S_IMODE(self.executable.stat().st_mode), 0o751)

    def test_invalid_transition_and_missing_expected_kind_do_not_write(self):
        data = PREFIX + b"RPM"
        for arguments in (
            ("--expect", "rpm", "--set", "rpm"),
            ("--expect", "appimage", "--set", "rpm"),
            ("--set", "rpm"),
        ):
            with self.subTest(arguments=arguments):
                self.write_executable(data)
                self.assertNotEqual(self.run_helper(*arguments).returncode, 0)
                self.assertEqual(self.executable.read_bytes(), data)

    def test_missing_nonregular_and_symlink_files_are_rejected(self):
        self.assertNotEqual(self.run_helper("--expect", "deb").returncode, 0)
        self.executable.mkdir()
        self.assertNotEqual(self.run_helper("--expect", "deb").returncode, 0)
        self.executable.rmdir()
        original = self.directory / "original"
        data = PREFIX + b"DEB"
        original.write_bytes(data)
        self.executable.symlink_to(original)
        result = self.run_helper("--expect", "deb", "--set", "rpm")
        self.assertNotEqual(result.returncode, 0)
        self.assertEqual(original.read_bytes(), data)
        self.assertTrue(self.executable.is_symlink())


if __name__ == "__main__":
    unittest.main()
