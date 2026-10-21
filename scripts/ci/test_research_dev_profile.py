import copy
import importlib.util
import json
from pathlib import Path
import plistlib
import subprocess
import tempfile
import unittest
from unittest.mock import patch

SPEC = importlib.util.spec_from_file_location("research_dev_profile", Path(__file__).with_name("research-dev-profile.py"))
profile_module = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(profile_module)


class ResearchDevProfileTests(unittest.TestCase):
    def setUp(self):
        self.profile = json.loads(profile_module.PROFILE.read_text())
        self.overlay = json.loads((profile_module.TAURI / "tauri.desktop-dev.conf.json").read_text())

    def test_no_production_identity_updater_or_storage(self):
        profile_module.validate_config(self.profile, self.overlay)
        mutations = [
            lambda o: o.update(identifier="cloud.opensecret.maple"),
            lambda o: o["plugins"]["deep-link"]["desktop"].update(schemes=["cloud.opensecret.maple"]),
            lambda o: o["plugins"]["updater"].update(endpoints=["https://updates.trymaple.ai/latest.json"]),
            lambda o: o["bundle"].update(createUpdaterArtifacts=True),
            lambda o: o["app"]["security"].update(capabilities=["default"]),
        ]
        for mutate in mutations:
            with self.subTest(mutation=mutate):
                invalid = copy.deepcopy(self.overlay)
                mutate(invalid)
                with self.assertRaises(ValueError):
                    profile_module.validate_config(self.profile, invalid)

    def test_checks_actual_package_identity_and_scheme(self):
        with tempfile.TemporaryDirectory() as directory:
            bundle = Path(directory)
            (bundle / "Contents/MacOS").mkdir(parents=True)
            (bundle / "Contents/MacOS/maple").write_bytes(b"fixture executable")
            info = {"CFBundleIdentifier": self.profile["identifier"], "CFBundleName": self.profile["productName"],
                    "CFBundleExecutable": "maple", "CFBundleURLTypes": [{"CFBundleURLSchemes": [self.profile["scheme"]]}]}
            path = bundle / "Contents/Info.plist"
            path.write_bytes(plistlib.dumps(info))
            self.assertEqual(profile_module.verify_bundle(bundle, self.profile)["scheme"], "cloud.opensecret.maple.dev")
            info["CFBundleURLTypes"][0]["CFBundleURLSchemes"] = ["cloud.opensecret.maple"]
            path.write_bytes(plistlib.dumps(info))
            with self.assertRaises(ValueError):
                profile_module.verify_bundle(bundle, self.profile)

    def test_pr_artifact_job_has_no_publication_or_signing_authority(self):
        workflow = json.loads(subprocess.check_output([
            "yq", "-o=json", ".", str(profile_module.ROOT / ".github/workflows/desktop-pr-build.yml")
        ], text=True))
        self.assertEqual(set(workflow["on"]), {"pull_request"})
        self.assertEqual(workflow["permissions"], {"contents": "read"})
        job = workflow["jobs"]["build-research-dev-macos"]
        self.assertEqual(job["needs"], "changes")
        self.assertIn("needs.changes.outputs.macos", job["if"])
        self.assertNotIn("environment", job)
        self.assertNotIn("secrets.", json.dumps(job))
        self.assertTrue(any("./scripts/ci/research-dev-desktop.sh" in step.get("run", "") for step in job["steps"]))
        artifact = next(step["with"] for step in job["steps"] if step.get("uses", "").startswith("actions/upload-artifact@"))
        self.assertEqual(artifact["name"], "maple-research-dev-macos-pr")
        self.assertEqual(artifact["if-no-files-found"], "error")
        self.assertIn("build-profile.json", artifact["path"])
        self.assertIn("maple-research-dev-macos.tar.gz", artifact["path"])

    def test_pr_dev_opt_in_event_selection_and_concurrency(self):
        workflow = json.loads(subprocess.check_output([
            "yq", "-o=json", ".", str(profile_module.ROOT / ".github/workflows/desktop-pr-build.yml")
        ], text=True))
        self.assertEqual(workflow["on"]["pull_request"]["types"], ["opened", "synchronize", "reopened", "labeled"])
        jobs = workflow["jobs"]
        for name, job in jobs.items():
            self.assertEqual(job["name"], name + "${{ github.event.action == 'labeled' && format(' (label {0})', github.event.label.name) || '' }}")
        dev = "build-research-dev-macos"
        native = {"build-macos", "build-linux", "build-windows", dev}
        label = "ci:research-dev-macos"

        def selected(action, added_label="", labels=(), result="success", changed="false", cancelled=False):
            # Evaluate the actual job expressions with the same boolean semantics.
            context = {
                "github.event.action": action,
                "github.event.label.name": added_label,
                "github.event.pull_request.labels.*.name": list(labels),
                "needs.changes.result": result,
                **{f"needs.changes.outputs.{platform}": changed for platform in ("macos", "linux", "windows")},
            }
            selected_jobs = set()
            for name in native | {"changes"}:
                expression = jobs[name]["if"].strip().removeprefix("${{").removesuffix("}}").strip()
                expression = expression.replace("&&", " and ").replace("||", " or ")
                expression = expression.replace("!cancelled()", str(not cancelled)).replace("always()", "True")
                for key, value in context.items():
                    expression = expression.replace(key, repr(value))
                if eval(" ".join(expression.split()), {"__builtins__": {}},
                        {"contains": lambda items, value: value in items}):
                    selected_jobs.add(name)
            return selected_jobs

        for action in ("opened", "synchronize", "reopened"):
            with self.subTest(action=action):
                self.assertEqual(selected(action), {"changes"})
                self.assertEqual(selected(action, labels=[label]), {"changes", dev})
                self.assertEqual(selected(action, changed="true"), {"changes"} | native)
                self.assertEqual(selected(action, result="failure"), {"changes"} | native)
                self.assertEqual(selected(action, changed="true", cancelled=True), {"changes"})
        self.assertEqual(selected("labeled", label, [label], "skipped"), {dev})
        self.assertEqual(selected("labeled", "unrelated", [label], "skipped", "true"), set())
        self.assertEqual(selected("labeled", label, [label], "skipped", cancelled=True), set())

        # Metadata runs cannot cancel source runs or runs for other labels.
        self.assertEqual(workflow["concurrency"]["group"],
                         "${{ github.workflow }}-pr-${{ github.event.pull_request.number }}"
                         "${{ github.event.action == 'labeled' && format('-label-{0}', github.event.label.name) || '' }}")
        self.assertTrue(workflow["concurrency"]["cancel-in-progress"])
        self.assertEqual(jobs[dev]["concurrency"], {
            "group": "research-dev-macos-pr-${{ github.event.pull_request.number }}",
            "cancel-in-progress": True,
        })
        checkout = next(step for step in jobs[dev]["steps"] if step.get("uses", "").startswith("actions/checkout@"))
        self.assertEqual(checkout["with"]["ref"], "${{ github.event.pull_request.head.sha }}")
        self.assertFalse(checkout["with"]["persist-credentials"])

    def test_runtime_audit_rejects_host_dependencies_and_search_paths(self):
        system_loads = "maple:\n\t/usr/lib/libiconv.2.dylib (compatibility version 7.0.0, current version 7.0.0)\n"
        bundle_rpath = "Load command 0\n cmd LC_RPATH\n path @executable_path/../Frameworks (offset 12)\n"
        with patch.object(profile_module.subprocess, "check_output", side_effect=[system_loads, bundle_rpath]):
            profile_module.verify_macos_runtime_paths(Path("fixture/maple"))
        for host_path in ("/nix/store/fixture/lib/libiconv.2.dylib", "/Volumes/runner/lib/libiconv.2.dylib", "/Users/runner/lib/libiconv.2.dylib"):
            with self.subTest(host_path=host_path):
                loads = system_loads.replace("/usr/lib/libiconv.2.dylib", host_path)
                with patch.object(profile_module.subprocess, "check_output", side_effect=[loads, bundle_rpath]):
                    with self.assertRaisesRegex(ValueError, "build-host runtime path"):
                        profile_module.verify_macos_runtime_paths(Path("fixture/maple"))
                commands = bundle_rpath.replace("@executable_path/../Frameworks", str(Path(host_path).parent))
                with patch.object(profile_module.subprocess, "check_output", side_effect=[system_loads, commands]):
                    with self.assertRaisesRegex(ValueError, "build-host runtime path"):
                        profile_module.verify_macos_runtime_paths(Path("fixture/maple"))


if __name__ == "__main__":
    unittest.main()
