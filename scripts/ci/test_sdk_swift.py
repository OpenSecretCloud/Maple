"""Swift SDK build, CI and local-service isolation boundaries."""

import functools
import json
import os
from pathlib import Path
import subprocess
import tempfile
import unittest


ROOT = Path(__file__).resolve().parents[2]


@functools.cache
def workflow():
    result = subprocess.run(
        ["yq", "-o=json", ".", str(ROOT / ".github/workflows/sdk-swift.yml")],
        check=True, capture_output=True, text=True,
    )
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


class SwiftSDKWorkflowTests(unittest.TestCase):
    def test_ci_cannot_publish_or_receive_privileged_credentials(self):
        config = workflow()
        self.assertEqual(config["permissions"], {"contents": "read"})
        self.assertEqual(set(config["on"]), {"push", "pull_request", "workflow_dispatch"})
        self.assertEqual(config["on"]["push"]["branches"], ["master"])
        self.assertEqual(config["on"]["pull_request"]["branches"], ["master"])
        self.assertEqual(config["concurrency"]["group"], "sdk-swift-ci-${{ github.ref }}")
        for text in strings(config):
            self.assertNotRegex(text, r"\bsecrets\b|github\.token|\bGH_TOKEN\b|id-token|git (?:tag|push)|gh release|fastlane|notarytool|altool")
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

    def test_missing_change_selection_fails_open_to_validation(self):
        condition = workflow()["jobs"]["sdk-swift"]["if"]
        self.assertIn("always() && !cancelled()", condition)
        self.assertIn("needs.changes.result != 'success'", condition)
        self.assertIn("needs.changes.outputs.sdk_swift != 'false'", condition)

    def test_lane_builds_and_tests_package_and_local_backend_with_pinned_tools(self):
        job = workflow()["jobs"]["sdk-swift"]
        self.assertEqual(job["runs-on"], "macos-26-xlarge")
        self.assertEqual(job["env"]["OPENSECRET_DEV_ENV"], "0")
        self.assertEqual(job["env"]["OPENSECRET_DEV_POSTGRES"], "0")
        commands = "\n".join(step.get("run", "") for step in job["steps"])
        self.assertIn("/Applications/Xcode_26.5.app", commands)
        self.assertIn(".#apple -c bash scripts/ci/sdk-swift.sh unit", commands)
        self.assertIn(".#apple -c bash scripts/ci/sdk-swift.sh build", commands)
        self.assertIn(".#apple -c bash scripts/ci/sdk-swift.sh ios-build", commands)
        self.assertIn("'./services/opensecret?submodules=1' -c bash scripts/ci/sdk-swift-backend.sh", commands)
        self.assertNotIn("live-attestation", commands)


class SwiftSDKScriptTests(unittest.TestCase):
    def run_script(self, mode, extra=None):
        with tempfile.TemporaryDirectory() as folder:
            temporary = Path(folder)
            for name, source in {
                "uname": "#!/bin/sh\necho Darwin\n",
                "xcodebuild": "#!/bin/sh\necho 'Xcode 26.5'\n",
                "xcrun": "#!/bin/sh\nprintf '%s\\n' \"$*\" >> \"$TRACE\"\nprintf 'integration=%s dev=%s live=%s provider=%s\\n' \"${MAPLE_SWIFT_INTEGRATION_URL:-}\" \"${MAPLE_SWIFT_DEV_INTEGRATION:-}\" \"${MAPLE_RUN_LIVE_ATTESTATION:-}\" \"${RUN_LIVE_AI:-}\" >> \"$TRACE\"\n",
            }.items():
                executable = temporary / name
                executable.write_text(source)
                executable.chmod(0o755)
            env = {key: value for key, value in os.environ.items() if not key.startswith("MAPLE_SWIFT_")}
            env.update({"PATH": str(temporary) + os.pathsep + env.get("PATH", ""), "TRACE": str(temporary / "trace")})
            env.update(extra or {})
            result = subprocess.run(["bash", str(ROOT / "scripts/ci/sdk-swift.sh"), mode], env=env, capture_output=True, text=True)
            trace = (temporary / "trace").read_text() if (temporary / "trace").exists() else ""
            return result, trace

    def test_unit_scrubs_all_network_opt_ins_before_swift_test(self):
        result, trace = self.run_script("unit", {"MAPLE_SWIFT_INTEGRATION_URL": "http://localhost:3000",
            "MAPLE_SWIFT_DEV_INTEGRATION": "1", "MAPLE_RUN_LIVE_ATTESTATION": "1", "RUN_LIVE_AI": "1"})
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertIn("swift-format lint", trace)
        self.assertIn("swift test", trace)
        self.assertEqual(trace.splitlines()[-1], "integration= dev= live= provider=")

    def test_integration_requires_explicit_fixture_configuration(self):
        result, trace = self.run_script("integration")
        self.assertNotEqual(result.returncode, 0)
        self.assertNotIn("swift test", trace)
        env = {"MAPLE_SWIFT_TEST_EMAIL": "synthetic@local.test", "MAPLE_SWIFT_TEST_PASSWORD": "synthetic",
               "MAPLE_SWIFT_TEST_CLIENT_ID": "synthetic"}
        for endpoint in ("https://example.test", "http://localhost.evil.test:3000", "http://localhost:3000@evil.test",
                         "http://127.0.0.1:3000/?x=1", "http://user@127.0.0.1:3000", "http://127.0.0.1:99999"):
            with self.subTest(endpoint=endpoint):
                result, trace = self.run_script("integration", {**env, "MAPLE_SWIFT_INTEGRATION_URL": endpoint})
                self.assertNotEqual(result.returncode, 0)
                self.assertNotIn("swift test", trace)
        result, trace = self.run_script("integration", {**env, "MAPLE_SWIFT_INTEGRATION_URL": "http://127.0.0.1:3000"})
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertIn("--filter LocalIntegrationTests", trace)

    def test_fixture_bootstrap_refuses_developer_workspaces(self):
        env = {**os.environ, "GITHUB_ACTIONS": "false"}
        result = subprocess.run(["bash", str(ROOT / "scripts/ci/sdk-swift-backend.sh")], env=env, capture_output=True, text=True)
        self.assertEqual(result.returncode, 2)
        self.assertIn("disposable CI runners only", result.stderr)


if __name__ == "__main__":
    unittest.main()
