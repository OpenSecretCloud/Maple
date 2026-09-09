#!/usr/bin/env python3
"""Release comparisons follow the SDK selected by each standalone proxy lockfile."""

from pathlib import Path
import subprocess
import sys
import tempfile
import unittest


HELPER = Path(__file__).with_name("proxy_runtime_diff.py").resolve()


class ProxyRuntimeDiffTests(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.addCleanup(self.temp.cleanup)
        self.root = Path(self.temp.name)
        self.git("init", "-q")
        self.git("config", "user.name", "Proxy release tests")
        self.git("config", "user.email", "proxy-tests@example.invalid")
        self.write("proxy/Cargo.toml", '[package]\nname = "maple-proxy"\nversion = "0.3.4"\n')
        self.write("proxy/src/main.rs", "fn main() {}\n")
        self.write("proxy/Dockerfile", "FROM fixture\n")
        self.write("sdk/rust/src/lib.rs", "// SDK source\n")

    def git(self, *args):
        return subprocess.run(
            ["git", *args], cwd=self.root, check=True, capture_output=True, text=True
        ).stdout.strip()

    def write(self, name, content):
        path = self.root / name
        path.parent.mkdir(parents=True, exist_ok=True)
        path.write_text(content, encoding="utf-8")

    def sdk_lock(self, local, name="maple-sdk"):
        source = "" if local else 'source = "registry+https://github.com/rust-lang/crates.io-index"\n'
        self.write(
            "proxy/Cargo.lock",
            f'version = 4\n[[package]]\nname = "{name}"\nversion = "3.6.2"\n{source}',
        )

    def commit(self):
        self.git("add", ".")
        self.git("commit", "-qm", "fixture")
        return self.git("rev-parse", "HEAD")

    def compare(self, before, after):
        return subprocess.run(
            [sys.executable, "-I", str(HELPER), before, after],
            cwd=self.root, capture_output=True, text=True,
        ).returncode

    def test_pinned_proxy_ignores_unconsumed_sdk_source(self):
        self.sdk_lock(local=False)
        before = self.commit()
        self.write("sdk/rust/src/lib.rs", "// Unreleased SDK change\n")
        after = self.commit()
        self.assertEqual(self.compare(before, after), 0)

    def test_local_sdk_changes_count_for_old_and_new_package_names(self):
        for name in ("opensecret", "maple-sdk"):
            with self.subTest(name=name):
                self.sdk_lock(local=True, name=name)
                before = self.commit()
                self.write("sdk/rust/src/lib.rs", f"// {name} change\n")
                after = self.commit()
                self.assertEqual(self.compare(before, after), 1)

    def test_source_mode_transitions_count_in_both_directions(self):
        self.sdk_lock(local=True)
        local = self.commit()
        self.sdk_lock(local=False)
        pinned = self.commit()
        self.assertEqual(self.compare(local, pinned), 1)
        self.assertEqual(self.compare(pinned, local), 1)

    def test_pinned_proxy_still_tracks_its_own_build_inputs(self):
        self.sdk_lock(local=False)
        before = self.commit()
        for path in ("proxy/Cargo.toml", "proxy/Cargo.lock", "proxy/Dockerfile", "proxy/src/main.rs"):
            with self.subTest(path=path):
                original = (self.root / path).read_text(encoding="utf-8")
                self.write(path, original + "\n# changed\n")
                after = self.commit()
                self.assertEqual(self.compare(before, after), 1)
                self.write(path, original)
                before = self.commit()

    def test_missing_or_ambiguous_sdk_and_invalid_refs_fail_as_errors(self):
        self.sdk_lock(local=False)
        before = self.commit()
        self.assertEqual(self.compare(before, "missing-revision"), 2)
        for content in (
            "version = 4\n",
            "invalid toml",
            '[[package]]\nname = "maple-sdk"\n[[package]]\nname = "opensecret"\n',
        ):
            with self.subTest(content=content):
                self.write("proxy/Cargo.lock", content)
                after = self.commit()
                self.assertEqual(self.compare(before, after), 2)
                self.assertEqual(self.compare(after, before), 2)


if __name__ == "__main__":
    unittest.main()
