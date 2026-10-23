#!/usr/bin/env python3
"""Mocked upstream-suite preparation, provenance and cache-isolation regressions."""

from contextlib import redirect_stdout
import importlib.util
import io
import json
import os
from pathlib import Path
import subprocess
import tempfile
import unittest
from unittest import mock


spec = importlib.util.spec_from_file_location(
    "rusqlite_provider", Path(__file__).with_name("test-rusqlite-provider.py")
)
provider = importlib.util.module_from_spec(spec)
spec.loader.exec_module(provider)


class ProviderSuiteTests(unittest.TestCase):
    def setUp(self):
        self.temporary = tempfile.TemporaryDirectory()
        self.addCleanup(self.temporary.cleanup)
        self.root = Path(self.temporary.name).resolve()
        self.component = self.root / "agent"
        self.source = self.root / "registry" / "rusqlite-0.40.2"
        self.source.mkdir(parents=True)
        (self.source / "Cargo.toml").write_text('[package]\nname = "rusqlite"\nversion = "0.40.2"\n')
        (self.source / "Cargo.lock").write_text("# Original upstream lock\n")
        self.original_manifest = (self.source / "Cargo.toml").read_bytes()
        self.original_lock = (self.source / "Cargo.lock").read_bytes()
        self.sqlcipher = self.component / "dependencies" / "sqlcipher"
        self.sqlcipher.mkdir(parents=True)
        (self.sqlcipher / "verify_sources.py").write_text("# Reviewed verifier\n")
        self.provenance_path = self.sqlcipher / "provenance.json"
        self.provenance_path.write_text(json.dumps({"packages": {"libsqlite3-sys": {
            "repository": "https://github.com/MaplePrivacyLabs/rusqlite.git",
            "revision": "a" * 40,
            "version": "0.38.2",
        }}}))
        self.helper = self.root / "test-rusqlite-provider.py"
        self.helper.write_bytes(provider.HELPER.read_bytes())
        self.metadata = self.root / "production-metadata.json"
        self.upstream = {
            "name": "rusqlite", "version": "0.40.2", "source": provider.REGISTRY,
            "manifest_path": str(self.source / "Cargo.toml"),
        }
        self.metadata.write_text(json.dumps({"packages": [self.upstream]}))
        self.shipping_lock = self.component / "Cargo.lock"
        self.shipping_lock.write_text("# Shipping graph must not change\n")
        self.work = self.root / "prepared"
        self.target = self.root / "provider-target"
        self.github_output = self.root / "github-output"
        self.commands = []
        self.fail_verification = None
        self.verifications = 0
        self.resolved_lock = '# Resolved test-only graph\nversion = 4\n'
        for patcher in (
            mock.patch.object(provider, "COMPONENT", self.component),
            mock.patch.object(provider, "HELPER", self.helper),
            mock.patch.object(provider.subprocess, "run", side_effect=self.run_command),
        ):
            patcher.start()
            self.addCleanup(patcher.stop)

    def run_command(self, command, **kwargs):
        self.commands.append((command, kwargs))
        if command[0] != "cargo":
            self.verifications += 1
            if self.verifications == self.fail_verification:
                raise subprocess.CalledProcessError(1, command)
            return subprocess.CompletedProcess(command, 0)
        if command[1] == "metadata":
            work = kwargs["cwd"]
            (work / "Cargo.lock").write_text(self.resolved_lock)
            selected = {
                "workspace_root": str(work),
                "packages": [
                    dict(self.upstream, source=None, manifest_path=str(work / "Cargo.toml")),
                    {"name": "libsqlite3-sys", "version": "0.38.2", "source": "git+reviewed-provider"},
                ],
            }
            return subprocess.CompletedProcess(command, 0, stdout=json.dumps(selected))
        self.assertEqual(command[1], "test")
        return subprocess.CompletedProcess(command, 0)

    def prepare(self, work=None, target=None):
        return provider.prepare(self.metadata, work or self.work, target or self.target)

    def cargo_commands(self):
        return [(command, kwargs) for command, kwargs in self.commands if command[0] == "cargo"]

    def assert_no_tests(self):
        self.assertFalse(any(command[:2] == ["cargo", "test"] for command, _ in self.commands))

    def test_prepare_resolves_test_graph_then_verifies_before_emitting_key(self):
        with redirect_stdout(io.StringIO()):
            provider.main([
                str(self.metadata), "--phase", "prepare", "--work-dir", str(self.work),
                "--target-dir", str(self.target), "--github-output", str(self.github_output),
            ])
        self.assertEqual(self.verifications, 2)
        self.assertEqual([command[0][1] for command in self.cargo_commands()], ["metadata"])
        self.assertEqual(self.cargo_commands()[0][0], [
            "cargo", "metadata", "--format-version", "1", "--features", provider.FEATURES,
        ])
        state = json.loads((self.work / provider.PREPARED_STATE).read_text())
        fingerprint = state["dependency-fingerprint"]
        self.assertRegex(fingerprint, "^[a-f0-9]{64}$")
        self.assertEqual(self.github_output.read_text(), f"dependency-fingerprint={fingerprint}\n")
        self.assertEqual((self.work / "Cargo.lock").read_text(), self.resolved_lock)
        self.assertTrue((self.work / provider.SELECTED_METADATA).is_file())
        manifest = (self.work / "Cargo.toml").read_text()
        self.assertIn('features = ["bundled-sqlcipher-openssl4"]', manifest)
        self.assertIn('version = "=0.38.2"', manifest)
        self.assertIn('rev = "' + "a" * 40 + '"', manifest)
        self.assertEqual((self.source / "Cargo.toml").read_bytes(), self.original_manifest)
        self.assertEqual((self.source / "Cargo.lock").read_bytes(), self.original_lock)
        self.assertEqual(self.shipping_lock.read_text(), "# Shipping graph must not change\n")

    def test_run_rechecks_both_sources_and_uses_locked_exact_features_and_isolated_targets(self):
        with mock.patch.dict(os.environ, {
            "CARGO_TARGET_DIR": "/shipping-target", "CARGO_BUILD_BUILD_DIR": "/shared-agent-builds",
        }):
            self.prepare()
            provider.run_prepared(self.metadata, self.work, self.target)
        self.assertEqual(self.verifications, 4)
        self.assertEqual(len(self.cargo_commands()), 2)
        self.assertEqual(self.cargo_commands()[1][0], [
            "cargo", "test", "--locked", "--features", "bundled-sqlcipher,modern-full", "--timings",
        ])
        for _, options in self.cargo_commands():
            self.assertEqual(options["cwd"], self.work)
            self.assertEqual(options["env"]["CARGO_TARGET_DIR"], str(self.target))
            self.assertEqual(options["env"]["CARGO_BUILD_BUILD_DIR"], str(self.target / "build"))

    def test_production_provenance_failure_stops_before_resolution_or_cache_key(self):
        self.fail_verification = 1
        with self.assertRaises(subprocess.CalledProcessError):
            provider.main([
                str(self.metadata), "--phase", "prepare", "--work-dir", str(self.work),
                "--target-dir", str(self.target), "--github-output", str(self.github_output),
            ])
        self.assertEqual(self.cargo_commands(), [])
        self.assertFalse(self.work.exists())
        self.assertFalse(self.github_output.exists())

    def test_test_graph_provenance_failure_stops_before_testing_or_cache_key(self):
        self.fail_verification = 2
        with self.assertRaises(subprocess.CalledProcessError):
            provider.main([
                str(self.metadata), "--phase", "prepare", "--work-dir", str(self.work),
                "--target-dir", str(self.target), "--github-output", str(self.github_output),
            ])
        self.assert_no_tests()
        self.assertFalse((self.work / provider.PREPARED_STATE).exists())
        self.assertFalse(self.github_output.exists())

    def test_run_stops_on_either_provenance_failure_before_using_cached_target(self):
        self.prepare()
        for failure in (1, 2):
            with self.subTest(verification=failure):
                self.verifications = 0
                self.fail_verification = failure
                with self.assertRaises(subprocess.CalledProcessError):
                    provider.run_prepared(self.metadata, self.work, self.target)
                self.assert_no_tests()

    def test_graph_manifest_helper_verifier_and_provider_changes_invalidate_fingerprint(self):
        fingerprint = self.prepare()
        for path in (
            self.work / "Cargo.lock", self.work / "Cargo.toml", self.helper,
            self.sqlcipher / "verify_sources.py", self.provenance_path,
        ):
            with self.subTest(path=path.name):
                original = path.read_bytes()
                path.write_bytes(original + b"\n# changed\n")
                self.assertNotEqual(provider.dependency_fingerprint(self.work, self.upstream), fingerprint)
                with self.assertRaisesRegex(ValueError, "dependencies changed"):
                    provider.run_prepared(self.metadata, self.work, self.target)
                self.assert_no_tests()
                path.write_bytes(original)
        with mock.patch.object(provider, "FEATURES", "bundled-sqlcipher"):
            self.assertNotEqual(provider.dependency_fingerprint(self.work, self.upstream), fingerprint)
        for key in ("version", "source"):
            with self.subTest(upstream=key):
                changed = dict(self.upstream, **{key: "changed"})
                self.assertNotEqual(provider.dependency_fingerprint(self.work, changed), fingerprint)

    def test_newly_resolved_test_dependency_changes_the_cache_key(self):
        first = self.prepare()
        self.resolved_lock += '\n[[package]]\nname = "test-only"\nversion = "2.0.0"\n'
        second = self.prepare(self.root / "second", self.root / "second-target")
        self.assertNotEqual(first, second)

    def test_reviewed_provider_revision_change_changes_cache_key(self):
        first = self.prepare()
        provenance = json.loads(self.provenance_path.read_text())
        provenance["packages"]["libsqlite3-sys"]["revision"] = "b" * 40
        self.provenance_path.write_text(json.dumps(provenance))
        second_work = self.root / "second"
        second = self.prepare(second_work, self.root / "second-target")
        self.assertNotEqual(first, second)
        self.assertIn('rev = "' + "b" * 40 + '"', (second_work / "Cargo.toml").read_text())

    def test_equivalent_graphs_on_different_temporary_paths_share_fingerprint(self):
        first = self.prepare()
        second = self.prepare(self.root / "second", self.root / "second-target")
        self.assertEqual(first, second)

    def test_run_rejects_changed_metadata_or_target(self):
        self.prepare()
        selected = self.work / provider.SELECTED_METADATA
        selected.write_text(selected.read_text() + "\n")
        with self.assertRaisesRegex(ValueError, "metadata changed"):
            provider.run_prepared(self.metadata, self.work, self.target)
        with self.assertRaisesRegex(ValueError, "target directory"):
            provider.run_prepared(self.metadata, self.work, self.root / "other-target")
        self.assert_no_tests()

    def test_rejects_shipping_and_overlapping_directories_before_running_tools(self):
        for work, target in (
            (self.component / "target" / "suite", self.target),
            (self.work, self.component / "target"),
            (self.work, self.work),
            (self.work, self.work / "target"),
            (self.target / "source", self.target),
        ):
            with self.subTest(work=work, target=target), self.assertRaises(ValueError):
                self.prepare(work, target)
        self.assertEqual(self.commands, [])

    def test_prepare_refuses_to_overwrite_an_existing_work_directory(self):
        self.work.mkdir()
        marker = self.work / "keep"
        marker.write_text("existing work")
        with self.assertRaisesRegex(ValueError, "new or empty"):
            self.prepare()
        self.assertEqual(marker.read_text(), "existing work")
        self.assertEqual(self.cargo_commands(), [])

    def test_rejects_unreleased_upstream_source_before_preparation(self):
        self.metadata.write_text(json.dumps({"packages": [dict(self.upstream, source="git+unreviewed")]}))
        with self.assertRaisesRegex(ValueError, "released rusqlite"):
            self.prepare()
        self.assertEqual(self.cargo_commands(), [])
        self.assertFalse(self.work.exists())

    def test_single_metadata_invocation_remains_disposable_and_runs_complete_suite(self):
        with redirect_stdout(io.StringIO()):
            provider.main([str(self.metadata)])
        self.assertEqual([command[1] for command, _ in self.cargo_commands()], ["metadata", "test"])
        self.assertEqual(self.verifications, 4)
        work = self.cargo_commands()[0][1]["cwd"]
        self.assertFalse(work.exists())
        self.assertEqual((self.source / "Cargo.toml").read_bytes(), self.original_manifest)
        self.assertEqual((self.source / "Cargo.lock").read_bytes(), self.original_lock)


if __name__ == "__main__":
    unittest.main()
