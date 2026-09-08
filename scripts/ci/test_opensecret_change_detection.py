"""Backend, client-compatibility, and application-packaging routing regressions."""

from pathlib import Path
import subprocess
import sys
import unittest

from agent_change_detection import affects_agent
from change_detection import classify_path as research_routes
from opensecret_change_detection import OUTPUTS, classify_paths


class OpenSecretChangeDetectionTests(unittest.TestCase):
    def assert_routes(self, paths, *selected):
        self.assertEqual(classify_paths(paths), {name: name in selected for name in OUTPUTS})

    def test_backend_runtime_and_migrations_select_compatibility_without_packaging_apps(self):
        for relative in ("src/main.rs", "src/web/session.rs", "tests/contracts.rs",
                         "migrations/2026/up.sql", ".cargo/config.toml", "build.rs"):
            path = "services/opensecret/" + relative
            with self.subTest(path=path):
                expected = ("rust", "integration") if relative.startswith(("tests/", "migrations/")) else ("rust", "nix", "integration")
                self.assert_routes([path], *expected)
                self.assertEqual(research_routes(path), frozenset())
                self.assertFalse(affects_agent(path))

    def test_backend_dependency_and_toolchain_inputs_select_all_backend_checks(self):
        for relative in ("Cargo.toml", "Cargo.lock", "rust-toolchain.toml", "flake.nix", "flake.lock"):
            path = "services/opensecret/" + relative
            with self.subTest(path=path):
                self.assert_routes([path], *OUTPUTS)
                self.assertEqual(research_routes(path), frozenset())
                self.assertFalse(affects_agent(path))

    def test_backend_flake_shell_test_selects_nix_without_rust_or_app_builds(self):
        path = "services/opensecret/tests/entrypoint_entropy_preflight.sh"
        self.assert_routes([path], "nix")
        self.assertEqual(research_routes(path), frozenset())
        self.assertFalse(affects_agent(path))

    def test_backend_nix_only_inputs_and_dependency_policy_keep_independent_checks(self):
        for relative in ("nix/kernel-upstream.nix", "entrypoint.sh", "continuum-proxy",
                         "nitro-toolkit", "nitro-toolkit/init/main.c", "privatemode-public"):
            self.assert_routes(["services/opensecret/" + relative], "nix")
        self.assert_routes(["services/opensecret/deny.toml"], "audit")
        self.assert_routes(["services/opensecret/.env.sample"], "integration")

    def test_signed_pcr_and_operator_documentation_changes_do_not_rebuild_clients(self):
        for relative in ("pcrDev.json", "pcrDevHistory.json", "pcrProd.json", "pcrProdHistory.json",
                         "pcrPreview.json", "pcrPreviewHistory.json", "pcr_sign.js", "pcr_verify.js",
                         "scripts/pcr_compatibility.py", "scripts/test_pcr_compatibility.py"):
            path = "services/opensecret/" + relative
            with self.subTest(path=path):
                self.assert_routes([path], "pcr")
                self.assertEqual(research_routes(path), frozenset())
                self.assertFalse(affects_agent(path))

    def test_sdk_changes_select_compatibility_and_retain_existing_app_routing(self):
        for path in ("sdk/src/lib/client.ts", "sdk/src/lib/test/integration/api.test.ts",
                     "sdk/rust/src/client.rs", "sdk/rust/Cargo.lock", "sdk/test/integration/bootstrap.sql",
                     "sdk/flake.nix", "sdk/bun.lock", "sdk/package.json"):
            with self.subTest(path=path):
                self.assert_routes([path], "integration")
        self.assertEqual(research_routes("sdk/src/lib/client.ts"), frozenset({"frontend"}))
        self.assertTrue(affects_agent("sdk/rust/src/client.rs"))

    def test_independent_components_and_docs_skip_backend_checks(self):
        for path in ("apps/maple-research/frontend/src/main.tsx", "apps/maple-agent/app/src/main.rs",
                     "proxy/src/proxy.rs", "services/updates/src/index.ts", "sdk/README.md",
                     "docs/monorepo-plan.md", "README.md", "flake.nix", "flake.lock",
                     "services/opensecret/README.md", "services/opensecret/AGENTS.md",
                     "services/opensecret/docs/nitro-deploy.md", "services/opensecret/.agents/skills/example.md",
                     ".github/workflows/agent-ci.yml", "scripts/ci/change_detection.py"):
            with self.subTest(path=path):
                self.assert_routes([path])

    def test_submodules_and_selector_changes_select_all_backend_checks(self):
        for path in (".gitmodules", ".github/workflows/opensecret-change-detection.yml",
                     "scripts/ci/opensecret_change_detection.py"):
            self.assert_routes([path], *OUTPUTS)
        self.assertFalse(affects_agent(".gitmodules"))
        self.assertEqual(research_routes(".gitmodules"), frozenset())
        self.assert_routes([".github/workflows/opensecret-ci.yml"], "rust", "nix", "audit", "pcr")
        self.assert_routes([".github/workflows/sdk-integration.yml"], "integration")

    def test_unknown_inputs_invalid_paths_and_empty_diff(self):
        for path in ("unknown-root.toml", "services/opensecret/new-build-input", "", "/tmp/path",
                     "services/opensecret/../updates/src/main.ts"):
            self.assert_routes([path], *OUTPUTS)
        self.assert_routes([])
        self.assert_routes(["README.md", "services/opensecret/src/main.rs"], "rust", "nix", "integration")

    def test_null_delimited_cli_and_explicit_fallback(self):
        script = Path(__file__).with_name("opensecret_change_detection.py")
        for arguments, paths, selected in (
            ([], b"README.md\0services/opensecret/src/name with\nnewline.rs\0", {"rust", "nix", "integration"}),
            ([], b"README.md\0services/opensecret/docs/note with spaces.md\0", set()),
            (["--all"], b"", set(OUTPUTS)),
        ):
            result = subprocess.run([sys.executable, str(script), *arguments], input=paths,
                                    check=True, capture_output=True)
            expected = "".join(f"{name}={'true' if name in selected else 'false'}\n" for name in OUTPUTS)
            self.assertEqual(result.stdout.decode(), expected)


if __name__ == "__main__":
    unittest.main()
