"""Exercise backend diff selection and enforce unprivileged CI boundaries."""

import functools
import json
import os
from pathlib import Path
import shutil
import subprocess
import tempfile
import tomllib
import unittest

from opensecret_change_detection import OUTPUTS


ROOT = Path(__file__).resolve().parents[2]


@functools.cache
def workflow(name):
    result = subprocess.run(["yq", "-o=json", ".", str(ROOT / ".github/workflows" / name)],
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


class OpenSecretWorkflowBoundaryTests(unittest.TestCase):
    def test_fork_jobs_are_hosted_read_only_and_credential_free(self):
        for name in ("opensecret-ci.yml", "opensecret-change-detection.yml", "sdk-integration.yml"):
            config = workflow(name)
            with self.subTest(workflow=name):
                self.assertEqual(config["permissions"], {"contents": "read"})
                self.assertNotIn("pull_request_target", config["on"])
                self.assertNotIn("workflow_run", config["on"])
                for value in strings(config):
                    self.assertNotRegex(value, r"\bsecrets\b|github\.token|\bGH_TOKEN\b|\bid-token\b")
                for job in config["jobs"].values():
                    self.assertNotIn("environment", job)
                    self.assertIn(job.get("permissions"), (None, {"contents": "read"}))
                    if "uses" in job:
                        self.assertEqual(job["uses"], "./.github/workflows/opensecret-change-detection.yml")
                        self.assertNotIn("secrets", job)
                        continue
                    self.assertEqual(job["runs-on"], "ubuntu-latest")
                    for step in job["steps"]:
                        self.assertNotIn("${{", step.get("run", ""))
                        action = step.get("uses", "")
                        if action:
                            self.assertRegex(action, r"^[A-Za-z0-9_.-]+/[A-Za-z0-9_.-]+@[0-9a-f]{40}$")
                        if action.startswith("actions/checkout@"):
                            self.assertIs(step["with"]["persist-credentials"], False)
                            self.assertNotIn("repository", step["with"])
                            self.assertNotIn("ref", step["with"])
                        if action.startswith("DeterminateSystems/nix-installer-action@"):
                            self.assertEqual(step["with"]["github-token"], "")

    def test_backend_does_not_publish_or_run_privileged_legacy_builds(self):
        config = workflow("opensecret-ci.yml")
        self.assertEqual(set(config["on"]), {"push", "pull_request", "schedule", "workflow_dispatch"})
        self.assertEqual(config["on"]["push"]["branches"], ["master"])
        self.assertEqual(config["on"]["pull_request"]["branches"], ["master"])
        self.assertFalse(any((ROOT / "services/opensecret/.github/workflows").glob("*.yml")))
        for value in strings(config["jobs"]):
            self.assertNotRegex(value, r"eif-|deploy-|stage-|scp-|update-pcr|append-pcr|verify-pcr")
            self.assertNotRegex(value, r"upload-artifact|download-artifact|flakehub-cache|gh release")
        cache = next(step["with"] for step in config["jobs"]["rust"]["steps"]
                     if "rust-cache@" in step.get("uses", ""))
        self.assertEqual(cache["workspaces"], "services/opensecret -> target")
        self.assertEqual(cache["save-if"],
                         "${{ github.event_name == 'push' && github.ref == 'refs/heads/master' }}")

    def test_backend_retains_exact_rust_gates_and_disabled_stateful_shell_hooks(self):
        config = workflow("opensecret-ci.yml")
        self.assertEqual(config["defaults"]["run"]["working-directory"], "services/opensecret")
        self.assertEqual(config["env"]["RUSTFLAGS"], "-D warnings")
        for key in ("OPENSECRET_DEV_POSTGRES", "OPENSECRET_DEV_ENV", "OPENSECRET_DEV_CONTAINERS"):
            self.assertEqual(config["env"][key], "0")
        commands = [step["run"] for step in config["jobs"]["rust"]["steps"] if "run" in step]
        self.assertEqual(commands, [
            "nix develop --no-update-lock-file '.?submodules=1' -c cargo fmt --all -- --check",
            "nix develop --no-update-lock-file '.?submodules=1' -c cargo clippy --locked --all-targets --all-features -- -D warnings",
            "nix develop --no-update-lock-file '.?submodules=1' -c cargo test --locked --all-features",
        ])
        nix_commands = [step["run"] for step in config["jobs"]["nix"]["steps"] if "run" in step]
        self.assertEqual(nix_commands, [
            "nix flake check --no-update-lock-file --print-build-logs '.?submodules=1'",
            "nix build --no-link --no-update-lock-file '.?submodules=1#default'",
        ])
        audit = config["jobs"]["audit"]["steps"][-1]["with"]
        toolchain = tomllib.loads((ROOT / "services/opensecret/rust-toolchain.toml").read_text())
        self.assertEqual(audit["rust-version"], toolchain["toolchain"]["channel"])
        self.assertEqual(audit["manifest-path"], "services/opensecret/Cargo.toml")
        self.assertEqual(audit["command"], "check advisories bans")
        self.assertEqual(audit["arguments"], "--config services/opensecret/deny.toml --all-features --locked")

    def test_pcr_job_only_checks_existing_signed_inputs(self):
        commands = [step["run"] for step in workflow("opensecret-ci.yml")["jobs"]["pcr"]["steps"]
                    if "run" in step]
        self.assertEqual(commands, [
            "nix develop --no-update-lock-file '.?submodules=1' -c python3 scripts/test_pcr_compatibility.py\n"
            "nix develop --no-update-lock-file '.?submodules=1' -c python3 scripts/pcr_compatibility.py check .\n"
        ])

    def test_sdk_integration_uses_checked_out_backend_and_disposable_services(self):
        config = workflow("sdk-integration.yml")
        self.assertEqual(set(config["on"]), {"push", "pull_request", "workflow_dispatch"})
        job = config["jobs"]["sdk-integration"]
        checkouts = [step for step in job["steps"] if "checkout@" in step.get("uses", "")]
        self.assertEqual(len(checkouts), 1)
        self.assertEqual(checkouts[0]["with"]["submodules"], "recursive")
        commands = "\n".join(step.get("run", "") for step in job["steps"])
        self.assertNotIn(".ci/opensecret", commands)
        self.assertNotIn("opensecret-integration-revision", commands)
        self.assertFalse((ROOT / "sdk/opensecret-integration-revision").exists())
        self.assertIn("cd services/opensecret", commands)
        self.assertIn("'./services/opensecret?submodules=1'", commands)
        self.assertIn("services/opensecret/target/debug/opensecret", commands)
        self.assertIn("diesel migration run", commands)
        self.assertIn("sdk/test/integration/bootstrap.sql", commands)
        self.assertIn("bun test src/lib/test/integration/api.test.ts", commands)
        self.assertIn("cargo test --locked --all-features --tests", commands)
        self.assertEqual(config["env"]["APP_MODE"], "local")
        self.assertEqual(config["env"]["OPENAI_API_BASE"], "http://127.0.0.1:9")
        self.assertEqual(job["services"]["postgres"]["env"]["POSTGRES_DB"], "opensecret")

    def test_selector_failures_or_missing_outputs_cannot_skip_validation(self):
        for workflow_name, lanes in (("opensecret-ci.yml", {name: name for name in OUTPUTS if name != "integration"}),
                                     ("sdk-integration.yml", {"sdk-integration": "integration"})):
            config = workflow(workflow_name)
            self.assertNotIn("paths", config["on"]["pull_request"])
            self.assertNotIn("paths", config["on"]["push"])
            for job_name, output in lanes.items():
                condition = config["jobs"][job_name]["if"]
                self.assertIn("always() && !cancelled()", condition)
                self.assertIn("needs.changes.result != 'success'", condition)
                self.assertIn(f"needs.changes.outputs.{output} != 'false'", condition)


class OpenSecretDiffSelectionTests(unittest.TestCase):
    def setUp(self):
        self.temporary = tempfile.TemporaryDirectory()
        self.addCleanup(self.temporary.cleanup)
        self.root = Path(self.temporary.name)
        self.git("init", "-q", "-b", "master")
        self.git("config", "user.email", "ci@example.invalid")
        self.git("config", "user.name", "CI fixture")
        self.git("config", "core.hooksPath", "/dev/null")
        scripts = self.root / "scripts/ci"
        scripts.mkdir(parents=True)
        shutil.copyfile(ROOT / "scripts/ci/opensecret_change_detection.py", scripts / "opensecret_change_detection.py")
        self.base = self.commit_file("README.md", "initial\n")

    def git(self, *arguments):
        result = subprocess.run(["git", *arguments], cwd=self.root, check=True,
                                capture_output=True, text=True)
        return result.stdout.strip()

    def commit_file(self, path, text):
        file = self.root / path
        file.parent.mkdir(parents=True, exist_ok=True)
        file.write_text(text)
        self.git("add", "--", path)
        self.git("commit", "-qm", "fixture change")
        return self.git("rev-parse", "HEAD")

    def select(self, event, base, head):
        step = next(step for step in workflow("opensecret-change-detection.yml")["jobs"]["detect"]["steps"]
                    if step.get("id") == "classify")
        output = self.root / "output"
        output.unlink(missing_ok=True)
        env = {**os.environ, "GITHUB_EVENT_NAME": event, "BASE_SHA": base, "HEAD_SHA": head,
               "GITHUB_OUTPUT": str(output)}
        result = subprocess.run(["bash", "-c", step["run"]], cwd=self.root, env=env,
                                capture_output=True, text=True)
        self.assertEqual(result.returncode, 0, result.stderr)
        return dict(line.split("=", 1) for line in output.read_text().splitlines())

    def expected(self, *selected):
        return {name: "true" if name in selected else "false" for name in OUTPUTS}

    def test_docs_only_push_backend_runtime_push_and_signed_pcr_push(self):
        docs = self.commit_file("services/opensecret/docs/design.md", "design\n")
        self.assertEqual(self.select("push", self.base, docs), self.expected())
        runtime = self.commit_file("services/opensecret/src/main.rs", "fn main() {}\n")
        self.assertEqual(self.select("push", docs, runtime), self.expected("rust", "nix", "integration"))
        pcr = self.commit_file("services/opensecret/pcrDevHistory.json", "[]\n")
        self.assertEqual(self.select("push", runtime, pcr), self.expected("pcr"))

    def test_pull_request_uses_merge_base_instead_of_unrelated_base_changes(self):
        master = self.commit_file("services/opensecret/src/main.rs", "fn main() {}\n")
        self.git("checkout", "-qb", "contributor", self.base)
        docs = self.commit_file("services/opensecret/docs/design.md", "design\n")
        self.assertEqual(self.select("pull_request", master, docs), self.expected())

    def test_deletion_or_rename_out_of_backend_still_selects_contract_checks(self):
        runtime = self.commit_file("services/opensecret/src/main.rs", "fn main() {}\n")
        self.git("mv", "services/opensecret/src/main.rs", "LICENSE")
        self.git("commit", "-qam", "rename fixture")
        self.assertEqual(self.select("push", runtime, self.git("rev-parse", "HEAD")),
                         self.expected("rust", "nix", "integration"))

    def test_missing_history_manual_event_classifier_failure_and_partial_output_fail_safe(self):
        for event, base, head in (("push", "0" * 40, self.base), ("push", "a" * 40, self.base),
                                  ("workflow_dispatch", "", "")):
            with self.subTest(event=event, base=base):
                self.assertEqual(self.select(event, base, head), self.expected(*OUTPUTS))
        classifier = self.root / "scripts/ci/opensecret_change_detection.py"
        classifier.write_text("print('rust=false')\nraise RuntimeError('fixture')\n")
        self.assertEqual(self.select("push", self.base, self.base), self.expected(*OUTPUTS))

    def test_schedule_selects_only_advisory_audit(self):
        self.assertEqual(self.select("schedule", "", ""), self.expected("audit"))


if __name__ == "__main__":
    unittest.main()
