"""Attack and regression cases for the trusted publisher, without live writes."""

import copy
import hashlib
import io
import json
import os
from pathlib import Path
import sys
import tempfile
import unittest
from unittest.mock import patch
from urllib.error import HTTPError

sys.path.insert(0, str(Path(__file__).resolve().parent))
import pages_deploy as pages

SHA = "a" * 40
OLD_SHA = "b" * 40
OTHER_SHA = "c" * 40
REPO_ID = 923138240


class MemoryAPI:
    def __init__(self, values):
        self.values = values

    def json(self, path, method="GET", data=None):
        if method != "GET":
            raise AssertionError("Selection must never mutate GitHub")
        return copy.deepcopy(self.values[path])


class ProvenanceTests(unittest.TestCase):
    def setUp(self):
        self.repo = "OpenSecretCloud/Maple"
        self.root = "/repos/" + self.repo
        self.run = {"id": 100, "workflow_id": 10, "path": ".github/workflows/pages-preview-build.yml",
                    "event": "pull_request", "status": "completed", "conclusion": "success",
                    "run_attempt": 2, "head_sha": SHA, "head_branch": "feature", "pull_requests": [],
                    "repository": {"id": REPO_ID}, "head_repository": {"id": REPO_ID}}
        self.pr = {"number": 877, "state": "open", "base": {"ref": "master", "repo": {"id": REPO_ID}},
                   "head": {"sha": SHA, "ref": "feature", "repo": {"id": REPO_ID}}}
        self.artifact = {"id": 300, "name": "maple-pages-preview-100-2", "expired": False,
                         "digest": "sha256:" + "d" * 64, "size_in_bytes": 1000}
        self.values = {self.root: {"id": REPO_ID, "default_branch": "master"},
                       self.root + "/actions/runs/100": self.run,
                       self.root + "/actions/workflows/pages-preview-build.yml": {"id": 10},
                       self.root + f"/commits/{SHA}/pulls?per_page=100": [{"number": 877}],
                       self.root + "/pulls/877": self.pr,
                       self.root + "/git/ref/heads/master": {"object": {"sha": SHA}},
                       self.root + "/actions/runs/100/artifacts?per_page=100": {"total_count": 1, "artifacts": [self.artifact]}}
        self.event = {"workflow_run": copy.deepcopy(self.run)}
        self.gh = pages.GitHub(MemoryAPI(self.values), self.repo, REPO_ID)

    def select(self):
        return pages.select_plan(self.gh, self.event, "preview")

    def test_internal_pr_with_empty_event_pr_list(self):
        plan = self.select()
        self.assertEqual((plan["branch"], plan["profile"], plan["sha"]), ("pr-877", "pr", SHA))
        self.assertEqual(plan["artifact_id"], 300)

    def test_owner_rename_does_not_change_repository_identity(self):
        new_repo = "MaplePrivacyLabs/Maple"
        moved = {key.replace(self.root, "/repos/" + new_repo): value for key, value in self.values.items()}
        gh = pages.GitHub(MemoryAPI(moved), new_repo, REPO_ID)
        self.assertEqual(pages.select_plan(gh, self.event, "preview")["sha"], SHA)

    def test_fork_run_never_eligible_even_when_pr_commit_is_shared(self):
        self.run["head_repository"]["id"] = 99
        with self.assertRaises(pages.Rejected):
            self.select()

    def test_fork_pr_rejected_even_if_workflow_metadata_claims_internal(self):
        self.pr["head"]["repo"]["id"] = 99
        with self.assertRaises(pages.Superseded):
            self.select()

    def test_closed_changed_head_and_wrong_base_prs(self):
        for mutate in (lambda: self.pr.update(state="closed"),
                       lambda: self.pr["head"].update(sha=OTHER_SHA),
                       lambda: self.pr["base"].update(ref="not-master"),
                       lambda: self.pr["base"]["repo"].update(id=99),
                       lambda: self.pr["head"].update(repo=None),
                       lambda: self.pr["head"].update(ref="other-branch")):
            original = copy.deepcopy(self.pr)
            mutate()
            with self.assertRaises(pages.Superseded):
                self.select()
            self.pr.clear()
            self.pr.update(original)

    def test_run_attempt_race(self):
        self.run["run_attempt"] = 3
        with self.assertRaises(pages.Superseded):
            self.select()

    def test_wrong_workflow_identity_path_or_event(self):
        for key, value in (("workflow_id", 123), ("path", ".github/workflows/evil.yml"),
                           ("event", "pull_request_target"), ("conclusion", "failure"), ("status", "in_progress")):
            old = self.run[key]
            self.run[key] = value
            with self.assertRaises(pages.Rejected):
                self.select()
            self.run[key] = old

    def test_trigger_sha_must_agree_with_api(self):
        self.event["workflow_run"]["head_sha"] = OTHER_SHA
        with self.assertRaises(pages.Rejected):
            self.select()

    def test_previous_attempt_artifact_not_selected(self):
        self.artifact["name"] = "maple-pages-preview-100-1"
        with self.assertRaises(pages.Rejected):
            self.select()

    def test_duplicate_expired_and_unsigned_artifacts_rejected(self):
        artifact_list = self.values[self.root + "/actions/runs/100/artifacts?per_page=100"]["artifacts"]
        artifact_list.append(copy.deepcopy(self.artifact))
        with self.assertRaises(pages.Rejected):
            self.select()
        artifact_list.pop()
        self.artifact["expired"] = True
        with self.assertRaises(pages.Rejected):
            self.select()
        self.artifact["expired"] = False
        self.artifact["digest"] = None
        with self.assertRaises(pages.Rejected):
            self.select()

    def test_master_preview_is_dev_and_must_be_current(self):
        self.run.update(event="push", head_branch="master")
        self.event = {"workflow_run": copy.deepcopy(self.run)}
        plan = self.select()
        self.assertEqual((plan["profile"], plan["branch"], plan["pr_number"]), ("pr", "master", None))
        self.values[self.root + "/git/ref/heads/master"]["object"]["sha"] = OTHER_SHA
        with self.assertRaises(pages.Superseded):
            self.select()

    def setup_release(self):
        self.run.update(event="release", head_branch="v3.3.10", path=".github/workflows/release.yml", workflow_id=11)
        self.event = {"workflow_run": copy.deepcopy(self.run)}
        self.release = {"id": 900, "tag_name": "v3.3.10", "draft": False, "prerelease": False,
                        "assets": [{"id": 1, "name": "maple-web-dist.tar.gz", "state": "uploaded", "size": 8000,
                                    "digest": "sha256:" + "d" * 64},
                                   {"id": 2, "name": "web-final.sha256", "state": "uploaded", "size": 130,
                                    "digest": "sha256:" + "e" * 64}]}
        self.values.update({self.root + "/releases/latest": self.release,
                            self.root + "/commits/v3.3.10": {"sha": SHA},
                            self.root + "/actions/workflows/release.yml": {"id": 11},
                            self.root + f"/compare/{SHA}...master": {"status": "ahead"},
                            self.root + "/git/ref/heads/pages-production": {"object": {"sha": OLD_SHA}},
                            self.root + f"/compare/{OLD_SHA}...{SHA}": {"status": "ahead"},
                            self.root + f"/actions/workflows/release.yml/runs?event=release&head_sha={SHA}&per_page=100": {"workflow_runs": [self.run]}})

    def production(self):
        return pages.select_plan(self.gh, self.event, "production")

    def test_release_selects_exact_production_asset(self):
        self.setup_release()
        plan = self.production()
        self.assertEqual((plan["branch"], plan["profile"], plan["sha"]), ("pages-production", "release", SHA))
        self.assertEqual(plan["archive"]["id"], 1)

    def test_current_release_can_be_redeployed_without_a_new_release(self):
        self.setup_release()
        self.values[self.root + "/git/ref/heads/pages-production"]["object"]["sha"] = SHA
        self.event = {}
        self.assertEqual(self.production()["previous_sha"], SHA)

    def test_draft_prerelease_and_malformed_tag_rejected(self):
        self.setup_release()
        for field, value in (("draft", True), ("prerelease", True), ("tag_name", "v3.3.10\n::error::injection")):
            old = self.release[field]
            self.release[field] = value
            with self.assertRaises(pages.Rejected):
                self.production()
            self.release[field] = old

    def test_release_superseded_by_new_tag(self):
        self.setup_release()
        self.event["workflow_run"]["head_branch"] = "v3.3.9"
        with self.assertRaises(pages.Superseded):
            self.production()

    def test_release_run_must_match_tag_sha(self):
        self.setup_release()
        self.run["head_sha"] = OTHER_SHA
        with self.assertRaises(pages.Rejected):
            self.production()

    def test_release_must_be_on_master_and_forward(self):
        self.setup_release()
        for path in (f"/compare/{SHA}...master", f"/compare/{OLD_SHA}...{SHA}"):
            self.values[self.root + path]["status"] = "diverged"
            with self.assertRaises(pages.Rejected):
                self.production()
            self.values[self.root + path]["status"] = "ahead"

    def test_manual_retry_does_not_accept_failed_release(self):
        self.setup_release()
        self.event = {}
        self.run["conclusion"] = "failure"
        with self.assertRaises(pages.Rejected):
            self.production()

    def test_release_digest_and_asset_uniqueness_required(self):
        self.setup_release()
        self.release["assets"].append(copy.deepcopy(self.release["assets"][0]))
        with self.assertRaises(pages.Rejected):
            self.production()
        self.release["assets"].pop()
        self.release["assets"][0]["digest"] = None
        with self.assertRaises(pages.Rejected):
            self.production()


class ProcessAndTransportTests(unittest.TestCase):
    def test_wrangler_rejects_ancestor_configuration(self):
        with tempfile.TemporaryDirectory() as tmp:
            parent = Path(tmp)
            cwd = parent / "nested" / "runner"
            cwd.mkdir(parents=True)
            # Isolate test from the host's own ancestors; only our fixture matters.
            for name in ("wrangler.toml", "wrangler.json", "wrangler.jsonc", "package.json", ".env", ".dev.vars"):
                config = parent / name
                config.write_text("fixture")
                with self.assertRaises(pages.Rejected):
                    pages.require_clean_wrangler_ancestors(cwd)
                config.unlink()
            (parent / ".wrangler").mkdir()
            with self.assertRaises(pages.Rejected):
                pages.require_clean_wrangler_ancestors(cwd)

    def test_child_environment_drops_every_other_credential_and_runtime_override(self):
        injected = {"PATH": "/trusted/bin", "GH_TOKEN": "fake-gh-secret", "GITHUB_TOKEN": "other-fake",
                    "NODE_OPTIONS": "--require=/evil", "BUN_OPTIONS": "--preload=/evil", "PYTHONPATH": "/evil",
                    "HOME": "/attacker", "HTTPS_PROXY": "https://attacker.invalid", "BWS_ACCESS_TOKEN": "fake-bws",
                    "GITHUB_ENV": "/commands", "GITHUB_OUTPUT": "/commands", "CLOUDFLARE_API_BASE_URL": "https://attacker.invalid"}
        with tempfile.TemporaryDirectory() as tmp, patch.dict(os.environ, injected, clear=True):
            result = pages.wrangler_environment("a" * 32, "fake-cf-canary", Path(tmp))
            self.assertEqual(result["CLOUDFLARE_API_TOKEN"], "fake-cf-canary")
            for key in injected.keys() - {"PATH", "HOME"}:
                self.assertNotIn(key, result)
            self.assertNotEqual(result["HOME"], "/attacker")
            self.assertNotIn("fake-gh-secret", json.dumps(result))

    def test_cf_project_identity_and_automatic_build_gate(self):
        project = {"success": True, "result": {"name": "maple", "subdomain": pages.SUBDOMAIN,
                   "production_branch": "pages-production", "source": {"config": {"production_deployments_enabled": False}}}}
        api = MemoryAPI({"/accounts/abc/pages/projects/maple": project})
        pages.cloudflare_project(api, "abc", "production")
        project["result"]["source"]["config"]["production_deployments_enabled"] = True
        with self.assertRaises(pages.Rejected):
            pages.cloudflare_project(api, "abc", "production")
        # Preview pilots can coexist until the operator turns off native previews.
        pages.cloudflare_project(api, "abc", "preview")
        project["result"]["subdomain"] = "wrong.pages.dev"
        with self.assertRaises(pages.Rejected):
            pages.cloudflare_project(api, "abc", "preview")

    def test_artifact_redirect_never_receives_github_authorization(self):
        requests = []

        class Opener:
            def open(self, request, timeout):
                requests.append(request)
                if len(requests) == 1:
                    raise HTTPError(request.full_url, 302, "redirect", {"Location": "https://storage.example/artifact?signature=fake"}, None)
                return io.BytesIO(b"archive")

        api = pages.API("https://api.github.com", "fake-gh-canary")
        api.opener = Opener()
        with tempfile.TemporaryDirectory() as tmp:
            api.download("/repos/owner/repo/actions/artifacts/12/zip", Path(tmp) / "a.zip",
                         expected_size=7, expected_digest=hashlib.sha256(b"archive").hexdigest(),
                         accept="application/vnd.github+json")
        self.assertEqual(requests[0].get_header("Authorization"), "Bearer fake-gh-canary")
        self.assertEqual(requests[0].get_header("Accept"), "application/vnd.github+json")
        self.assertIsNone(requests[1].get_header("Authorization"))

    def test_release_asset_requests_binary_representation(self):
        api = pages.API("https://api.github.com", "fake-gh-canary")
        with tempfile.TemporaryDirectory() as tmp, patch.object(api, "request", return_value=io.BytesIO(b"archive")) as request:
            api.download("/repos/owner/repo/releases/assets/12", Path(tmp) / "archive")
        request.assert_called_once_with("/repos/owner/repo/releases/assets/12", accept="application/octet-stream")

    def test_preview_waits_for_deploy_success_not_an_earlier_stage(self):
        result = {"deployment_id": "a" * 36, "url": "https://12345678.maple-ca8.pages.dev"}
        plan = {"target": "preview", "sha": SHA, "branch": "pr-12"}
        deployment = {"environment": "preview", "project_name": "maple", "url": result["url"],
                      "deployment_trigger": {"metadata": {"commit_hash": SHA, "branch": "pr-12"}}}
        api = pages.API("https://api.cloudflare.com/client/v4", "fake-cf-canary")
        stages = [{"name": "build", "status": "success"}, {"name": "deploy", "status": "active"},
                  {"name": "deploy", "status": "success"}]
        replies = [{"success": True, "result": {**deployment, "latest_stage": stage}} for stage in stages]
        with patch.object(api, "json", side_effect=replies) as read, patch.object(pages.time, "sleep") as sleep:
            pages.verify_deployment(api, "a" * 32, plan, result)
        self.assertEqual(read.call_count, 3)
        self.assertEqual(sleep.call_count, 2)

    def test_non_https_redirect_rejected_without_second_request(self):
        class Opener:
            def open(self, request, timeout):
                raise HTTPError(request.full_url, 302, "redirect", {"Location": "http://attacker.invalid/artifact"}, None)

        api = pages.API("https://api.github.com", "fake-gh-canary")
        api.opener = Opener()
        with tempfile.TemporaryDirectory() as tmp, self.assertRaises(pages.Rejected):
            api.download("/artifact", Path(tmp) / "a.zip")

    def test_download_bounds_size_and_digest(self):
        class Opener:
            def open(self, request, timeout):
                return io.BytesIO(b"archive")

        for kwargs in ({"limit": 6}, {"expected_size": 8}, {"expected_digest": "0" * 64}):
            api = pages.API("https://api.github.com", "fake-gh-canary")
            api.opener = Opener()
            with tempfile.TemporaryDirectory() as tmp, self.assertRaises(pages.Rejected):
                api.download("/artifact", Path(tmp) / "a.zip", **kwargs)

    def test_identifiers_cannot_inject_api_paths_or_output(self):
        for value in (True, -1, "12\n::error::injected", "1/../../secrets"):
            with self.assertRaises(pages.Rejected):
                pages.number(value)
        for value in (SHA + "\n::error::injected", "../../master", "$(env)"):
            with self.assertRaises(pages.Rejected):
                pages.sha(value)


if __name__ == "__main__":
    unittest.main()
