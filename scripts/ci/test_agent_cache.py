#!/usr/bin/env python3
"""Regression tests for compatible Cargo-target cache boundaries."""

import json
import os
from pathlib import Path
import subprocess
import shutil
import tempfile
import unittest
from unittest.mock import patch

import agent_cache


class AgentCacheTests(unittest.TestCase):
    def setUp(self):
        self.temporary = tempfile.TemporaryDirectory()
        self.addCleanup(self.temporary.cleanup)
        self.root = Path(self.temporary.name)
        for name in agent_cache.CONFIG_FILES:
            path = self.root / name
            path.parent.mkdir(parents=True, exist_ok=True)
            path.write_text("pinned configuration\n", encoding="utf-8")
        manifests = (
            "apps/maple-agent/Cargo.toml", "apps/maple-agent/app/Cargo.toml",
            "apps/maple-agent/crates/core/Cargo.toml", "sdk/rust/Cargo.toml", "proxy/Cargo.toml",
        )
        for name in manifests:
            path = self.root / name
            path.parent.mkdir(parents=True, exist_ok=True)
            path.write_text('[package]\nname = "fixture"\nversion = "1.0.0"\nedition = "2024"\n[features]\ndefault = ["native"]\nnative = []\n', encoding="utf-8")
        self.environment = {"RUSTFLAGS": "-D warnings", "CARGO_PROFILE_DEV_DEBUG": "0", "CARGO_HOME": str(self.root / "cargo-home")}
        self.patches = (
            patch.dict(os.environ, self.environment, clear=True),
            patch.object(agent_cache.platform, "system", return_value="Linux"),
            patch.object(agent_cache.platform, "machine", return_value="x86_64"),
            patch.object(agent_cache, "executable", side_effect=lambda name: f"/nix/store/toolchain/bin/{name}"),
            patch.object(agent_cache, "command", side_effect=lambda args: " ".join(args) + " version 1"),
            patch.object(agent_cache, "native_identity", return_value={"cc": "/nix/store/native/bin/cc", "sdk": "1"}),
            patch.object(agent_cache, "native_build_tools", return_value={"perl": "native", "nasm": None}),
        )
        for selected in self.patches:
            selected.start()
            self.addCleanup(selected.stop)

    def identity(self, profile="ci"):
        return agent_cache.build_identity(profile, self.root)

    def change_file(self, name, suffix="changed\n"):
        path = self.root / name
        path.write_text(path.read_text(encoding="utf-8") + suffix, encoding="utf-8")

    def test_version_only_manifest_and_lock_bump_keeps_compatible_prefix(self):
        before = agent_cache.digest(self.identity())
        for name in ("apps/maple-agent/Cargo.toml", "apps/maple-agent/app/Cargo.toml"):
            path = self.root / name
            path.write_text(path.read_text().replace('version = "1.0.0"', 'version = "1.0.1"'))
        (self.root / "apps/maple-agent/Cargo.lock").write_text("changed local package version")
        self.assertEqual(agent_cache.digest(self.identity()), before)

    def test_workspace_package_version_does_not_change_prefix(self):
        path = self.root / "apps/maple-agent/Cargo.toml"
        path.write_text('[workspace]\nresolver = "2"\n[workspace.package]\nversion = "1.0.0"\nedition = "2024"\n')
        before = agent_cache.digest(self.identity())
        path.write_text(path.read_text().replace('"1.0.0"', '"1.0.1"'))
        self.assertEqual(agent_cache.digest(self.identity()), before)

    def test_provider_configuration_and_helper_invalidate_prefix(self):
        for name in ("apps/maple-agent/dependencies/sqlcipher/provenance.json", "apps/maple-agent/scripts/test-rusqlite-provider.py"):
            with self.subTest(name=name):
                before = agent_cache.digest(self.identity())
                self.change_file(name)
                self.assertNotEqual(agent_cache.digest(self.identity()), before)

    def test_feature_and_profile_changes_invalidate_prefix(self):
        path = self.root / "apps/maple-agent/Cargo.toml"
        before = agent_cache.digest(self.identity())
        self.change_file(str(path.relative_to(self.root)), '\n[profile.dev]\nopt-level = 1\n')
        self.assertNotEqual(agent_cache.digest(self.identity()), before)
        before = agent_cache.digest(self.identity())
        path.write_text(path.read_text().replace('default = ["native"]', 'default = []'))
        self.assertNotEqual(agent_cache.digest(self.identity()), before)

    def test_cargo_config_changes_invalidate_prefix(self):
        before = agent_cache.digest(self.identity())
        path = self.root / "apps/maple-agent/.cargo/config.toml"
        path.parent.mkdir()
        path.write_text('[build]\nrustflags = ["-C", "target-cpu=native"]\n')
        self.assertNotEqual(agent_cache.digest(self.identity()), before)

    def test_target_dependency_features_invalidate_but_versions_do_not(self):
        path = self.root / "apps/maple-agent/Cargo.toml"
        self.change_file(str(path.relative_to(self.root)),
                         '\n[target.\'cfg(windows)\'.dependencies]\n'
                         'provider = { version = "1.0", features = ["static"], default-features = false }\n')
        before = agent_cache.digest(self.identity())
        path.write_text(path.read_text().replace('version = "1.0"', 'version = "1.1"'))
        self.assertEqual(agent_cache.digest(self.identity()), before)
        path.write_text(path.read_text().replace('features = ["static"]', 'features = ["dynamic"]'))
        self.assertNotEqual(agent_cache.digest(self.identity()), before)

    def test_cargo_home_config_contents_invalidate_without_exposing_secrets(self):
        before = agent_cache.digest(self.identity())
        home = Path(self.environment["CARGO_HOME"])
        home.mkdir()
        (home / "config.toml").write_text('[build]\nrustflags = ["private-native-flag"]\n')
        after = self.identity()
        self.assertNotEqual(agent_cache.digest(after), before)
        serialized = json.dumps(after)
        self.assertNotIn("private-native-flag", serialized)
        self.assertNotIn(str(home), serialized)
        self.assertIn("cargo-home/config.toml", after["configuration"]["cargo_configs"]["workspace"])

    def test_ancestor_above_repository_is_included_with_stable_label(self):
        # Keep all test configurations under a disposable parent, never the
        # machine's real home or ancestor .cargo directories.
        with tempfile.TemporaryDirectory() as temporary:
            parent = Path(temporary)
            repository = parent / "checkout"
            shutil.copytree(self.root, repository)
            before = agent_cache.build_identity("ci", repository)
            (parent / ".cargo").mkdir()
            (parent / ".cargo/config").write_text('[build]\ntarget = "changed-native-target"\n')
            after = agent_cache.build_identity("ci", repository)
            self.assertNotEqual(agent_cache.digest(after), agent_cache.digest(before))
            configs = after["configuration"]["cargo_configs"]["workspace"]
            self.assertIn("ancestor-3/.cargo/config", configs)
            self.assertNotIn(str(parent), json.dumps(configs))

    def test_provider_directory_configs_change_prefix_without_temporary_path(self):
        copies = []
        for name in ("provider-first", "provider-second"):
            work = self.root / name
            (work / ".cargo").mkdir(parents=True)
            (work / ".cargo/config.toml").write_text('[build]\nrustflags = ["-Dwarnings"]\n')
            copies.append(work)
        first = agent_cache.build_identity("provider", self.root, copies[0])
        second = agent_cache.build_identity("provider", self.root, copies[1])
        # External suite directories have stable depth labels rather than
        # names of disposable source copies.
        with tempfile.TemporaryDirectory() as temporary:
            external = Path(temporary) / "suite"
            shutil.copytree(copies[0], external)
            external_first = agent_cache.build_identity("provider", self.root, external)
            renamed = external.with_name("renamed-suite")
            external.rename(renamed)
            external_second = agent_cache.build_identity("provider", self.root, renamed)
            self.assertEqual(external_first, external_second)
            self.assertNotIn(str(renamed), json.dumps(external_second["configuration"]["cargo_configs"]))
        self.assertNotEqual(first, second)
        (copies[0] / ".cargo/config.toml").write_text('[build]\nrustflags = ["-Ctarget-cpu=native"]\n')
        self.assertNotEqual(agent_cache.build_identity("provider", self.root, copies[0]), first)

    def test_absent_cargo_home_uses_default_home_without_writing_it(self):
        fixture_home = self.root / "default-home"
        with patch.dict(os.environ, clear=True), patch.object(agent_cache.Path, "home", return_value=fixture_home):
            before = self.identity()
            self.assertFalse(fixture_home.exists())
            (fixture_home / ".cargo").mkdir(parents=True)
            (fixture_home / ".cargo/config.toml").write_text('[build]\nincremental = false\n')
            after = self.identity()
        self.assertNotEqual(agent_cache.digest(after), agent_cache.digest(before))

    def test_actual_compiler_native_abi_flags_and_lane_invalidate(self):
        before = agent_cache.digest(self.identity())
        with patch.object(agent_cache, "command", return_value="actual compiler revision 2"):
            self.assertNotEqual(agent_cache.digest(self.identity()), before)
        with patch.object(agent_cache, "native_identity", return_value={"cc": "changed ABI"}):
            self.assertNotEqual(agent_cache.digest(self.identity()), before)
        with patch.object(agent_cache, "native_build_tools", return_value={"perl": "native", "nasm": "2"}):
            self.assertNotEqual(agent_cache.digest(self.identity()), before)
        with patch.dict(os.environ, {"RUSTFLAGS": "-D warnings -C target-cpu=x86-64-v3"}):
            self.assertNotEqual(agent_cache.digest(self.identity()), before)
        self.assertNotEqual(agent_cache.digest(self.identity("provider")), before)
        self.assertNotEqual(agent_cache.digest(self.identity("release")), before)

    def test_actual_cargo_clippy_selection_changes_linux_ci_prefix(self):
        before = self.identity()
        previous_command = agent_cache.command.side_effect
        with patch.object(agent_cache, "command", side_effect=lambda arguments: "different clippy compiler" if "clippy" in arguments else previous_command(arguments)):
            after = self.identity()
        self.assertEqual(before["rustc"], after["rustc"])
        self.assertNotEqual(before["clippy"], after["clippy"])
        self.assertNotEqual(agent_cache.digest(before), agent_cache.digest(after))

    def test_non_clippy_ci_lanes_do_not_require_clippy(self):
        for system, profile in (("Darwin", "ci"), ("Windows", "ci"), ("Linux", "provider"), ("Linux", "release")):
            with self.subTest(system=system, profile=profile), patch.object(agent_cache.platform, "system", return_value=system):
                agent_cache.command.reset_mock()
                self.assertIsNone(self.identity(profile)["clippy"])
                self.assertFalse(any("clippy" in call.args[0] for call in agent_cache.command.call_args_list))

    def test_release_variants_and_native_environment_changes_invalidate(self):
        before = agent_cache.digest(self.identity("release"))
        for key, value in (("MAPLE_RELEASE_PROFILE", "prod"), ("NIX_CFLAGS_COMPILE", "-isystem /nix/store/changed/include"), ("MACOSX_DEPLOYMENT_TARGET", "15.0")):
            with self.subTest(key=key), patch.dict(os.environ, {key: value}):
                self.assertNotEqual(agent_cache.digest(self.identity("release")), before)

    def test_unrelated_environment_is_not_recorded(self):
        before = self.identity()
        with patch.dict(os.environ, {"GH_TOKEN": "private", "APPLE_ID_PASSWORD": "private", "GITHUB_RUN_ID": "different-run"}):
            self.assertEqual(self.identity(), before)

    def test_unix_rejects_host_toolchain_before_cache_restore(self):
        with patch.object(agent_cache, "executable", return_value="/usr/bin/rustc"):
            with self.assertRaisesRegex(ValueError, "pinned Nix shell"):
                self.identity()

    def test_output_reports_compatibility_and_actual_compiler(self):
        output = self.root / "target/environment.json"
        github_output = self.root / "github-output"
        expected = self.identity()
        with patch.object(agent_cache, "build_identity", return_value=expected), patch.dict(os.environ, {"GITHUB_OUTPUT": str(github_output)}), patch("sys.argv", ["agent_cache.py", "--profile", "ci", "--output", str(output)]):
            agent_cache.main()
        actual = json.loads(output.read_text())
        self.assertEqual(actual["compatibility"], agent_cache.digest(expected))
        self.assertEqual(actual["rustc"], expected["rustc"])
        self.assertEqual(github_output.read_text(), f"compatibility={agent_cache.digest(expected)}\n")


class NativeEnvironmentTests(unittest.TestCase):
    def test_optional_windows_wrapper_probe_failure_is_fingerprinted(self):
        with tempfile.TemporaryDirectory() as temporary:
            wrapper = Path(temporary) / "pkg-config.BAT"
            wrapper.write_bytes(b"broken preinstalled Perl wrapper")
            with patch.object(agent_cache, "executable", return_value="/native/perl"), \
                    patch.object(agent_cache, "command", return_value="v5.40"), \
                    patch.object(agent_cache.shutil, "which", side_effect=lambda name: str(wrapper) if name == "pkg-config" else None), \
                    patch.object(agent_cache.subprocess, "run", return_value=subprocess.CompletedProcess([], 29, "", "wrapper error")):
                before = agent_cache.native_build_tools({})
                self.assertEqual(before["pkg-config"]["version_probe_exit"], 29)
                self.assertIsNone(before["pkg-config"]["version"])
                self.assertNotIn("wrapper error", json.dumps(before))
                wrapper.write_bytes(b"updated wrapper")
                self.assertNotEqual(agent_cache.native_build_tools({}), before)

    def test_required_perl_probe_still_fails_closed(self):
        with patch.object(agent_cache, "executable", return_value="/native/perl"), \
                patch.object(agent_cache, "command", side_effect=ValueError("required Perl failed")):
            with self.assertRaisesRegex(ValueError, "required Perl failed"):
                agent_cache.native_build_tools({})

    def test_optional_version_probe_timeout_keeps_file_identity(self):
        with tempfile.TemporaryDirectory() as temporary:
            wrapper = Path(temporary) / "pkg-config.BAT"
            wrapper.write_bytes(b"preinstalled wrapper")
            with patch.object(agent_cache, "executable", return_value="/native/perl"), \
                    patch.object(agent_cache, "command", return_value="v5.40"), \
                    patch.object(agent_cache.shutil, "which", side_effect=lambda name: str(wrapper) if name == "pkg-config" else None), \
                    patch.object(agent_cache.subprocess, "run", side_effect=subprocess.TimeoutExpired("optional", 10)):
                tools = agent_cache.native_build_tools({})
                self.assertEqual(tools["pkg-config"]["version_probe_error"], "TimeoutExpired")
                self.assertEqual(len(tools["pkg-config"]["sha256"]), 64)

    def test_windows_requires_selected_toolset_and_sdk(self):
        with self.assertRaisesRegex(ValueError, "MSVC developer environment"):
            agent_cache.native_identity("Windows", {})

    def test_windows_environment_names_are_case_insensitive(self):
        with patch.object(agent_cache, "executable", return_value="/unselected/cl.exe"):
            with self.assertRaisesRegex(ValueError, "outside the selected"):
                agent_cache.native_identity("Windows", {"VCTOOLSINSTALLDIR": "/selected", "WINDOWSSDKDIR": "/sdk", "WINDOWSSDKVERSION": "10.0"})

    def test_openssl_native_perl_and_nasm_absence_are_recorded(self):
        with patch.object(agent_cache, "executable", return_value="/native/perl"), patch.object(agent_cache.shutil, "which", return_value=None), patch.object(agent_cache, "command", return_value="v5.40"):
            tools = agent_cache.native_build_tools({"OPENSSL_SRC_PERL": "/native/perl"})
        self.assertEqual(tools["perl"], {"path": "/native/perl", "version": "v5.40"})
        self.assertIsNone(tools["nasm"])

    def test_windows_rejects_different_cl_on_path(self):
        with patch.object(agent_cache, "executable", return_value="/unselected/cl.exe"):
            with self.assertRaisesRegex(ValueError, "outside the selected"):
                agent_cache.native_identity("Windows", {"VCToolsInstallDir": "/selected", "WindowsSdkDir": "/sdk", "WindowsSDKVersion": "10.0"})

    def test_windows_toolset_binary_and_sdk_changes_are_fingerprinted(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            compiler = root / "toolset/bin/cl.exe"
            compiler.parent.mkdir(parents=True)
            for name in ("cl.exe", "c1.dll", "c1xx.dll", "c2.dll", "link.exe"):
                (compiler.parent / name).write_bytes(name.encode())
            sdk = root / "sdk"
            for version in ("10.0", "11.0"):
                (sdk / "Include" / version).mkdir(parents=True)
                (sdk / "Lib" / version).mkdir(parents=True)
            environment = {"VCToolsInstallDir": str(root / "toolset"), "WindowsSdkDir": str(sdk), "WindowsSDKVersion": "10.0\\"}
            with patch.object(agent_cache, "executable", return_value=str(compiler)), patch.object(agent_cache, "command", return_value="MSVC version"):
                before = agent_cache.native_identity("Windows", environment)
                (compiler.parent / "c2.dll").write_bytes(b"new native backend")
                self.assertNotEqual(agent_cache.native_identity("Windows", environment), before)
                environment["WindowsSDKVersion"] = "11.0\\"
                self.assertEqual(agent_cache.native_identity("Windows", environment)["sdk_version"], "11.0")

    def test_macos_collects_actual_xcode_and_sdk(self):
        with patch.object(agent_cache, "compiler_identity", return_value={"version": "clang"}), patch.object(agent_cache, "command", side_effect=lambda args: " ".join(args)):
            native = agent_cache.native_identity("Darwin", {"CC": "/selected/clang", "CXX": "/selected/clang++"})
        self.assertIn("xcodebuild -version", native["xcode"])
        self.assertIn("--show-sdk-path", native["sdk_path"])
        self.assertIn("--show-sdk-version", native["sdk_version"])


if __name__ == "__main__":
    unittest.main()
