#!/usr/bin/env python3
"""Exercise RPM staging identity and packaging failures without native builds."""

import os
from pathlib import Path
import subprocess
import tempfile
import unittest


ROOT = Path(__file__).resolve().parents[2]


class RpmPackagingTests(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.addCleanup(self.temp.cleanup)
        self.root = Path(self.temp.name)
        self.tauri = self.root / "tauri"
        self.deb = self.tauri / "target/release/bundle/deb/Maple.deb"
        self.deb.parent.mkdir(parents=True)
        self.deb.write_bytes(b"fixture\0__TAURI_BUNDLE_TYPE_VAR_DEB\0payload")
        (self.tauri / "tauri.conf.json").write_text('{"version":"1.2.3"}')
        self.rpm = self.tauri / "target/release/bundle/rpm/Maple-1.2.3-1.x86_64.rpm"
        self.bin = self.root / "bin"
        self.bin.mkdir()

    def run_shell(self, commands):
        env = dict(os.environ, PATH=f"{self.bin}:{os.environ['PATH']}",
                   FIXTURE_TAURI=str(self.tauri), SOURCE_DATE_EPOCH="1234567890")
        return subprocess.run(
            ["bash", "-c", 'source scripts/ci/_common.sh\n'
             'TAURI_DIR="$FIXTURE_TAURI"\n'
             'host_os() { echo linux; }\n'
             'rpm_arch() { echo x86_64; }\n' + commands],
            cwd=ROOT, env=env, capture_output=True, text=True,
        )

    def test_normalization_preserves_deb_and_marks_only_rpm_copy(self):
        original = self.deb.read_bytes()
        result = self.run_shell('''
normalize_deb_package() { :; }
extract_deb_payload() {
  mkdir -p "$2/usr/bin"
  cp "$1" "$2/usr/bin/maple"
}
build_rpm_package_from_payload() { cp "$2/usr/bin/maple" "$1"; }
normalize_linux_desktop_packages
''')
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertEqual(self.deb.read_bytes(), original)
        self.assertEqual(self.rpm.read_bytes(), original.replace(b"VAR_DEB", b"VAR_RPM"))

    def test_invalid_staging_identity_never_builds_rpm(self):
        self.deb.write_bytes(b"missing bundle identity")
        result = self.run_shell('''
normalize_deb_package() { :; }
extract_deb_payload() {
  mkdir -p "$2/usr/bin"
  cp "$1" "$2/usr/bin/maple"
}
build_rpm_package_from_payload() { touch "$1"; }
normalize_linux_desktop_packages
''')
        self.assertNotEqual(result.returncode, 0)
        self.assertFalse(self.rpm.exists())

    def test_failed_extraction_is_not_hidden_by_cleanup(self):
        result = self.run_shell('''
ar() { return 19; }
if extract_deb_payload "${TAURI_DIR}/target/release/bundle/deb/Maple.deb" "$TAURI_DIR/payload"; then
  exit 0
else
  exit "$?"
fi
''')
        self.assertEqual(result.returncode, 19)

    def fake_rpmbuild(self, exit_code):
        tool = self.bin / "rpmbuild"
        tool.write_text(f'''#!/usr/bin/env bash
set -eu
while [ "$#" -gt 0 ]; do
  if [ "$1" = --define ]; then
    case "$2" in "_topdir "*) topdir="${{2#_topdir }}" ;; esac
    shift
  fi
  shift
done
mkdir -p "$topdir/RPMS/x86_64"
printf 'partial RPM' > "$topdir/RPMS/x86_64/maple-1.2.3-1.x86_64.rpm"
exit {exit_code}
''')
        tool.chmod(0o755)

    def run_factory(self, extra=""):
        return self.run_shell(extra + '''
mkdir -p "$TAURI_DIR/payload/usr/bin" "$TAURI_DIR/target/release/bundle/rpm"
printf 'fixture' > "$TAURI_DIR/payload/usr/bin/maple"
if build_rpm_package_from_payload "$TAURI_DIR/target/release/bundle/rpm/Maple-1.2.3-1.x86_64.rpm" "$TAURI_DIR/payload"; then
  exit 0
else
  exit "$?"
fi
''')

    def test_failed_rpmbuild_cannot_publish_partial_output(self):
        self.fake_rpmbuild(19)
        result = self.run_factory()
        self.assertNotEqual(result.returncode, 0, result.stderr)
        self.assertFalse(self.rpm.exists())

    def test_failed_final_move_is_not_hidden_by_cleanup(self):
        self.fake_rpmbuild(0)
        result = self.run_factory('mv() { return 19; }\n')
        self.assertNotEqual(result.returncode, 0, result.stderr)
        self.assertFalse(self.rpm.exists())


if __name__ == "__main__":
    unittest.main()
