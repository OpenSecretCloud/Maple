"""Both SDK modes must preserve one SDK identity and the local embedded proxy."""

import copy
import importlib.util
from pathlib import Path
import tempfile
import unittest


spec = importlib.util.spec_from_file_location(
    "verify_agent_rust_deps", Path(__file__).with_name("verify-agent-rust-deps.py")
)
verifier = importlib.util.module_from_spec(spec)
spec.loader.exec_module(verifier)


class AgentRustDependencyTests(unittest.TestCase):
    def graph(self, root, registry=False):
        graph = {"packages": [
            {"name": name, "version": "3.6.2" if name == "maple-sdk" else "0.3.4",
             "source": None, "manifest_path": str(root / manifest)}
            for name, manifest in verifier.DEPENDENCIES.items()
        ]}
        if registry:
            graph["packages"][0].update(
                source=verifier.CRATES_IO,
                manifest_path=str(root / "registry" / "maple-sdk-3.6.2" / "Cargo.toml"),
            )
        return graph

    def test_local_and_published_graphs_pass(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            for registry in (False, True):
                with self.subTest(registry=registry):
                    verifier.verify(self.graph(root, registry), root)

    def test_consumer_version_need_not_equal_current_sdk_source(self):
        root = Path("/maple")
        graph = self.graph(root, registry=True)
        # Cargo --locked validates the consumer's requirement. This guard must
        # not require consumers to upgrade when monorepo SDK development moves.
        graph["packages"][0]["version"] = "3.6.1"
        verifier.verify(graph, root)

    def test_legacy_sdk_fails(self):
        root = Path("/maple")
        for registry in (False, True):
            legacy = self.graph(root, registry)
            legacy["packages"].append({
                "name": "opensecret",
                "source": verifier.CRATES_IO,
                "manifest_path": str(root / "registry" / "opensecret" / "Cargo.toml"),
            })
            with self.assertRaisesRegex(ValueError, "legacy opensecret SDK"):
                verifier.verify(legacy, root)

    def test_misdirected_missing_or_duplicate_crates_fail(self):
        root = Path("/maple")
        for registry in (False, True):
            graph = self.graph(root, registry)
            for name in verifier.DEPENDENCIES:
                for alteration in ("duplicate", "missing", "other_registry", "fork", "other_checkout"):
                    with self.subTest(registry=registry, name=name, alteration=alteration):
                        changed = copy.deepcopy(graph)
                        package = next(p for p in changed["packages"] if p["name"] == name)
                        if alteration == "duplicate":
                            changed["packages"].append(copy.deepcopy(package))
                        elif alteration == "missing":
                            changed["packages"].remove(package)
                        elif alteration == "other_registry":
                            package["source"] = "registry+https://example.com/crates.io-index"
                        elif alteration == "fork":
                            package["source"] = "git+https://github.com/example/fork"
                        else:
                            package["source"] = None
                            package["manifest_path"] = str(root / "other" / "Cargo.toml")
                        with self.assertRaises(ValueError):
                            verifier.verify(changed, root)

    def test_registry_proxy_fails(self):
        root = Path("/maple")
        graph = self.graph(root, registry=True)
        graph["packages"][1]["source"] = verifier.CRATES_IO
        with self.assertRaisesRegex(ValueError, "monorepo's proxy/Cargo.toml"):
            verifier.verify(graph, root)

    def test_mixed_local_and_registry_sdk_identities_fail(self):
        root = Path("/maple")
        graph = self.graph(root)
        graph["packages"].append(self.graph(root, registry=True)["packages"][0])
        with self.assertRaisesRegex(ValueError, "exactly one maple-sdk"):
            verifier.verify(graph, root)


if __name__ == "__main__":
    unittest.main()
