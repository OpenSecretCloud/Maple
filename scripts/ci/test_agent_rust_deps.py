"""The Agent import must not reintroduce a second SDK or a registry proxy."""

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
    def test_local_graph_passes_and_misdirected_or_duplicate_crates_fail(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            graph = {"packages": [
                {"name": name, "source": None, "manifest_path": str(root / manifest)}
                for name, manifest in verifier.DEPENDENCIES.items()
            ]}
            verifier.verify(graph, root)
            for name in verifier.DEPENDENCIES:
                for alteration in ("duplicate", "missing", "registry", "fork", "other_checkout"):
                    with self.subTest(name=name, alteration=alteration):
                        changed = copy.deepcopy(graph)
                        package = next(p for p in changed["packages"] if p["name"] == name)
                        if alteration == "duplicate":
                            changed["packages"].append(copy.deepcopy(package))
                        elif alteration == "missing":
                            changed["packages"].remove(package)
                        elif alteration == "registry":
                            package["source"] = "registry+https://github.com/rust-lang/crates.io-index"
                        elif alteration == "fork":
                            package["source"] = "git+https://github.com/example/fork"
                        else:
                            package["manifest_path"] = str(root / "other" / "Cargo.toml")
                        with self.assertRaises(ValueError):
                            verifier.verify(changed, root)


if __name__ == "__main__":
    unittest.main()
