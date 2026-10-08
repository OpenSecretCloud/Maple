"""Native iOS lane isolation, profile selection and unprivileged CI contracts."""

import functools
import json
import os
from pathlib import Path
import subprocess
import tempfile
import unittest

from ios_native_change_detection import selected


ROOT = Path(__file__).resolve().parents[2]


@functools.cache
def workflow():
    result = subprocess.run(["yq", "-o=json", ".", str(ROOT / ".github/workflows/ios-native.yml")],
                            check=True, capture_output=True, text=True)
    return json.loads(result.stdout)


def strings(value):
    if isinstance(value, str):
        yield value
    elif isinstance(value, dict):
        for key, child in value.items():
            yield str(key)
            yield from strings(child)
    elif isinstance(value, list):
        for child in value:
            yield from strings(child)


class NativeSelectionTests(unittest.TestCase):
    def test_app_and_consumed_build_inputs_select_native(self):
        for path in (
            "apps/maple-ios/Package.swift", "apps/maple-ios/Package.resolved",
            "apps/maple-ios/Sources/MapleAppCore/Chat/ChatRuntime.swift",
            "apps/maple-ios/Maple.xcodeproj/project.pbxproj", "apps/maple-ios/Config/PR.xcconfig",
            ".github/workflows/ios-native.yml", "scripts/ci/ios-native.sh", "flake.nix", "flake.lock",
            "scripts/ci/ios_native_change_detection.py", "scripts/ci/hook_change_detection.py",
            "apps/maple-agent/app/assets/fonts/Array-Regular.otf",
            "apps/maple-research/frontend/public/fonts/Manrope-VariableFont_wght.ttf",
        ):
            with self.subTest(path=path):
                self.assertTrue(selected([path]))

    def test_sdk_backend_other_apps_and_docs_do_not_package_native(self):
        for path in (
            "sdk/swift/Package.swift", "sdk/swift/Sources/MapleSDK/MapleClient.swift",
            "services/opensecret/src/web/responses/handlers.rs", "apps/maple-research/frontend/package.json",
            "apps/maple-research/frontend/src/App.tsx", "apps/maple-agent/app/src/main.rs",
            "apps/maple-auth/src/App.tsx", "apps/maple-ios/README.md", "apps/maple-ios/AGENTS.md",
            "apps/maple-ios/.githooks/pre-commit", "docs/sdk-publishing.md",
        ):
            with self.subTest(path=path):
                self.assertFalse(selected([path]))
        self.assertFalse(selected([]))
        self.assertTrue(selected(["sdk/swift/Package.swift", "apps/maple-ios/Package.swift"]))

    def test_unknown_root_and_invalid_paths_fail_open_to_validation(self):
        for path in ("new-component/code.swift", "/absolute", "../outside", ""):
            self.assertTrue(selected([path]))


class NativeWorkflowTests(unittest.TestCase):
    def test_workflow_cannot_publish_sign_or_receive_credentials(self):
        config = workflow()
        self.assertEqual(config["permissions"], {"contents": "read"})
        self.assertEqual(set(config["on"]), {"push", "pull_request", "workflow_dispatch"})
        self.assertEqual(config["on"]["push"]["branches"], ["master"])
        self.assertEqual(config["on"]["pull_request"]["branches"], ["master"])
        self.assertEqual(config["concurrency"]["group"], "ios-native-ci-${{ github.ref }}")
        for text in strings(config):
            self.assertNotRegex(text, r"\bsecrets\b|github\.token|\bGH_TOKEN\b|id-token|git (?:tag|push)|gh release|fastlane|notarytool|altool|upload-artifact|CODE_SIGN_IDENTITY")
        for job in config["jobs"].values():
            self.assertNotIn("environment", job)
            self.assertNotIn("permissions", job)
            for step in job["steps"]:
                self.assertNotIn("${{", step.get("run", ""))
                action = step.get("uses", "")
                if action:
                    self.assertRegex(action, r"^[A-Za-z0-9_.-]+/[A-Za-z0-9_.-]+@[0-9a-f]{40}$")
                if action.startswith("actions/checkout@"):
                    self.assertIs(step["with"]["persist-credentials"], False)

    def test_failed_selection_still_runs_unsigned_validation(self):
        job = workflow()["jobs"]["ios-native"]
        self.assertIn("always() && !cancelled()", job["if"])
        self.assertIn("needs.changes.result != 'success'", job["if"])
        self.assertIn("needs.changes.outputs.ios_native != 'false'", job["if"])
        self.assertEqual(job["runs-on"], "macos-26-xlarge")
        commands = "\n".join(step.get("run", "") for step in job["steps"])
        self.assertIn("/Applications/Xcode_26.5.app", commands)
        for mode in ("unit", "cross-build", "simulator-build", "device-build"):
            self.assertIn(".#apple -c bash scripts/ci/ios-native.sh " + mode, commands)
        self.assertNotIn("release", commands)
        self.assertNotIn("integration", commands)


class NativeRecipeTests(unittest.TestCase):
    def run_recipe(self, mode, extra=None):
        with tempfile.TemporaryDirectory() as folder:
            temporary = Path(folder)
            trace = temporary / "trace"
            environment_trace = ('printf "account=%s auth=%s sdk=%s live=%s provider=%s vite=%s profile=%s api=%s ld=%s\\n" '
                                 '"${MAPLE_IOS_ACCOUNT_LIVE:-}" "${MAPLE_IOS_AUTH_LIVE:-}" '
                                 '"${MAPLE_SWIFT_INTEGRATION_URL:-}" "${MAPLE_RUN_LIVE_ATTESTATION:-}" '
                                 '"${RUN_LIVE_AI:-}" "${VITE_OPEN_SECRET_API_URL:-}" '
                                 '"${MAPLE_PROFILE:-}" "${MAPLE_API_URL:-}" "${LD:-}" >> "$TRACE"\n')
            for name, source in {
                "uname": '#!/bin/sh\necho "${FAKE_SYSTEM:-Darwin}"\n',
                "xcodebuild": ('#!/bin/sh\nif [ "$1" = -version ]; then echo "Xcode ${FAKE_XCODE:-26.5}"; exit 0; fi\n'
                               'printf "%s\\n" "$*" >> "$TRACE"\n' + environment_trace),
                "xcrun": ('#!/bin/sh\nprintf "%s\\n" "$*" >> "$TRACE"\n' + environment_trace +
                          'if [ "$1" = --sdk ]; then echo /synthetic-sdk; fi\n'),
            }.items():
                path = temporary / name
                path.write_text(source)
                path.chmod(0o755)
            env = {**os.environ, "PATH": str(temporary) + os.pathsep + os.environ.get("PATH", ""), "TRACE": str(trace)}
            env.update(extra or {})
            result = subprocess.run(["bash", str(ROOT / "scripts/ci/ios-native.sh"), mode],
                                    env=env, capture_output=True, text=True)
            return result, trace.read_text() if trace.exists() else ""

    def test_unit_never_inherits_live_opt_ins_or_linker_overrides(self):
        extra = {name: "synthetic-untrusted-value" for name in (
            "MAPLE_IOS_ACCOUNT_LIVE", "MAPLE_IOS_AUTH_LIVE", "MAPLE_SWIFT_INTEGRATION_URL",
            "MAPLE_RUN_LIVE_ATTESTATION", "RUN_LIVE_AI", "VITE_OPEN_SECRET_API_URL",
            "MAPLE_PROFILE", "MAPLE_API_URL", "LD", "SDKROOT",
        )}
        result, trace = self.run_recipe("unit", extra)
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertIn("swift-format lint", trace)
        swift_test = next(line for line in trace.splitlines() if line.startswith("swift test "))
        self.assertIn("--sdk macosx --show-sdk-path", trace)
        self.assertIn("--sdk /synthetic-sdk", swift_test)
        self.assertNotIn("synthetic-untrusted-value", trace)
        self.assertNotIn("xcodebuild", trace)

    def test_packaging_always_selects_pr_and_disables_signing(self):
        for mode, destination in (("simulator-build", "generic/platform=iOS Simulator"), ("device-build", "generic/platform=iOS")):
            with self.subTest(mode=mode):
                result, trace = self.run_recipe(mode, {"MAPLE_PROFILE": "local", "MAPLE_API_URL": "http://127.0.0.1:9999"})
                self.assertEqual(result.returncode, 0, result.stderr)
                self.assertIn("-configuration PR", trace)
                self.assertIn("-destination " + destination, trace)
                self.assertIn("CODE_SIGNING_ALLOWED=NO CODE_SIGNING_REQUIRED=NO build", trace)
                self.assertNotIn("127.0.0.1:9999", trace)
                self.assertNotIn("archive", trace)

    def test_cross_build_compiles_both_ios_platforms_with_warning_errors(self):
        result, trace = self.run_recipe("cross-build")
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertIn("--triple arm64-apple-ios26.0-simulator", trace)
        self.assertIn("--triple arm64-apple-ios26.0 ", trace)
        self.assertIn("-Xswiftc -warnings-as-errors", trace)

    def test_unrecognized_mode_or_toolchain_fails_before_build(self):
        for mode, extra in (("release", {}), ("unit", {"FAKE_SYSTEM": "Linux"}), ("unit", {"FAKE_XCODE": "26.4"})):
            result, trace = self.run_recipe(mode, extra)
            self.assertNotEqual(result.returncode, 0)
            self.assertEqual(trace, "")

    def test_pr_configuration_cannot_include_local_overrides(self):
        profile = (ROOT / "apps/maple-ios/Config/PR.xcconfig").read_text()
        self.assertNotIn("Local", profile)
        self.assertIn('MAPLE_PROFILE = development', profile)
        base = (ROOT / "apps/maple-ios/Config/Base.xcconfig").read_text()
        self.assertIn('PRODUCT_BUNDLE_IDENTIFIER = cloud.opensecret.maple.native.dev', base)


if __name__ == "__main__":
    unittest.main()
