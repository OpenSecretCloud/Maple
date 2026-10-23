#!/usr/bin/env python3
"""Check compiler-probe forwarding independently of the ambient Rustup list."""

import contextlib
import io
import json
import os
from pathlib import Path
import shutil
import subprocess
import sys
import tempfile
import unittest
from unittest.mock import patch

import agent_cache_command as wrapper


class AgentCacheCommandTests(unittest.TestCase):
    def test_only_exact_optional_inventory_is_empty_success_without_tool_access(self):
        stdout, stderr = io.StringIO(), io.StringIO()
        with patch.object(wrapper.subprocess, "run", side_effect=AssertionError("tool accessed")), \
                patch.object(wrapper.platform, "system", side_effect=AssertionError("platform accessed")), \
                contextlib.redirect_stdout(stdout), contextlib.redirect_stderr(stderr):
            self.assertEqual(wrapper.main(["rustup", "toolchain", "list", "--quiet"]), 0)
        self.assertEqual(stdout.getvalue(), "")
        self.assertEqual(stderr.getvalue(), "")

    def test_unexpected_rustup_probes_fail_without_running_or_faking_versions(self):
        probes = (
            ["rustup", "toolchain", "list"],
            ["rustup", "toolchain", "list", "--quiet", "--verbose"],
            ["rustup", "--version"],
            ["rustup", "run", "stable", "rustc", "-vV"],
            ["/host/bin/rustup", "toolchain", "list", "--quiet"],
            [r"C:\Users\runner\.cargo\bin\rustup.exe", "toolchain", "list", "--quiet"],
            ["rustup.exe", "toolchain", "list", "--quiet"],
        )
        for probe in probes:
            stdout, stderr = io.StringIO(), io.StringIO()
            with self.subTest(probe=probe), patch.object(wrapper.subprocess, "run") as run, \
                    contextlib.redirect_stdout(stdout), contextlib.redirect_stderr(stderr):
                self.assertEqual(wrapper.main(probe), 2)
                run.assert_not_called()
            self.assertEqual(stdout.getvalue(), "")
            self.assertIn("Unexpected Rustup probe", stderr.getvalue())

    def test_unix_forwards_real_probes_through_exact_component_shell(self):
        component = Path("/work/Maple checkout with spaces/apps/maple-agent")
        for system in ("Darwin", "Linux"):
            for probe in (["rustc", "-vV"],
                          ["cargo", "metadata", "--all-features", "--format-version", "1"],
                          ["cargo", "metadata", "--all-features", "--format-version", "1", "--no-deps"]):
                with self.subTest(system=system, probe=probe), \
                        patch.object(wrapper.platform, "system", return_value=system), \
                        patch.object(wrapper, "COMPONENT", component), \
                        patch.object(wrapper.subprocess, "run") as run:
                    run.return_value.returncode = 0
                    self.assertEqual(wrapper.main(probe), 0)
                    run.assert_called_once_with(
                        ["nix", "develop", "--no-update-lock-file", str(component), "-c", *probe],
                        shell=False, check=False,
                    )

    def test_windows_forwards_native_selected_executable_and_arguments_unchanged(self):
        probe = [r"C:\Rust toolchain\bin\cargo.exe", "metadata", "--manifest-path",
                 r"D:\Maple checkout\Cargo.toml", "--format-version", "1",
                 "literal 'quotes'", 'literal "quotes"', "$(no expansion)", "x;y"]
        with patch.object(wrapper.platform, "system", return_value="Windows"), \
                patch.object(wrapper.subprocess, "run") as run:
            run.return_value.returncode = 0
            self.assertEqual(wrapper.main(probe), 0)
            run.assert_called_once_with(probe, shell=False, check=False)

    def test_failed_probe_exit_is_not_replaced_with_a_successful_identity(self):
        with patch.object(wrapper.platform, "system", return_value="Linux"), \
                patch.object(wrapper.subprocess, "run") as run:
            run.return_value.returncode = 37
            self.assertEqual(wrapper.main(["rustc", "-vV"]), 37)

    def test_missing_probe_or_shell_fails_closed(self):
        stdout, stderr = io.StringIO(), io.StringIO()
        with patch.object(wrapper.platform, "system", return_value="Windows"), \
                patch.object(wrapper.subprocess, "run", side_effect=FileNotFoundError("missing rustc")), \
                contextlib.redirect_stdout(stdout), contextlib.redirect_stderr(stderr):
            self.assertEqual(wrapper.main(["rustc", "-vV"]), 127)
        self.assertEqual(stdout.getvalue(), "")
        self.assertIn("missing rustc", stderr.getvalue())

    def test_empty_command_fails_without_executing_anything(self):
        with patch.object(wrapper.subprocess, "run") as run, contextlib.redirect_stderr(io.StringIO()):
            self.assertEqual(wrapper.main([]), 2)
            run.assert_not_called()


@unittest.skipIf(os.name == "nt", "Unix Nix executable fixture")
class CommandLineTests(unittest.TestCase):
    def setUp(self):
        self.temporary = tempfile.TemporaryDirectory(prefix="Agent cache command ")
        self.addCleanup(self.temporary.cleanup)
        self.root = Path(self.temporary.name).resolve()
        self.script = self.root / "Maple checkout with spaces/scripts/ci/agent_cache_command.py"
        self.script.parent.mkdir(parents=True)
        shutil.copyfile(wrapper.__file__, self.script)
        self.component = self.script.parents[2] / "apps/maple-agent"
        self.tools = self.root / "tools with spaces"
        self.tools.mkdir()
        self.receipt = self.root / "arguments.json"
        self.rustup_access = self.root / "rustup-access"
        self.env = dict(os.environ, PATH=str(self.tools) + os.pathsep + os.environ.get("PATH", ""),
                        CACHE_PROBE_RECEIPT=str(self.receipt), CACHE_RUSTUP_ACCESS=str(self.rustup_access))
        self.tool("nix", '''import json, os, sys
expected = ["develop", "--no-update-lock-file", ''' + repr(str(self.component)) + ''', "-c"]
assert sys.argv[1:5] == expected, sys.argv
with open(os.environ["CACHE_PROBE_RECEIPT"], "w") as receipt:
    json.dump(sys.argv[1:], receipt)
os.execvp(sys.argv[5], sys.argv[5:])
''')

    def tool(self, name, source):
        path = self.tools / name
        path.write_text(f"#!{sys.executable}\n" + source, encoding="utf-8")
        path.chmod(0o755)

    def invoke(self, *arguments):
        return subprocess.run([sys.executable, str(self.script), *arguments], env=self.env,
                              capture_output=True, text=True, check=False)

    def test_cli_preserves_space_paths_literal_arguments_streams_and_failure(self):
        marker = self.root / "must not execute"
        arguments = ["path with spaces", '"quoted value"', r"C:\some path\Cargo.toml",
                     "$(touch forbidden)", f"; touch {marker}", "a&b", ""]
        probe = [sys.executable, "-c",
                 "import json,sys; print(json.dumps(sys.argv[1:])); "
                 "print('actual probe stderr',file=sys.stderr); sys.exit(23)", *arguments]
        result = self.invoke(*probe)
        self.assertEqual(result.returncode, 23, result.stderr)
        self.assertEqual(json.loads(result.stdout), arguments)
        self.assertEqual(result.stderr, "actual probe stderr\n")
        self.assertEqual(json.loads(self.receipt.read_text()),
                         ["develop", "--no-update-lock-file", str(self.component), "-c", *probe])
        self.assertFalse(marker.exists())

    def test_ambient_inventory_drift_never_runs_rustup_or_changes_real_compiler_probe(self):
        self.tool("rustc", "import sys; assert sys.argv[1:] == ['-vV']; print('actual pinned compiler identity')\n")
        outputs = []
        for inventory in ("stable-host-1", "stable-host-2\\nnightly-host"):
            self.tool("rustup", "import os; from pathlib import Path; "
                      "Path(os.environ['CACHE_RUSTUP_ACCESS']).write_text('accessed'); "
                      f"print({inventory!r}); raise SystemExit(91)\n")
            inventory_result = self.invoke("rustup", "toolchain", "list", "--quiet")
            self.assertEqual((inventory_result.returncode, inventory_result.stdout, inventory_result.stderr), (0, "", ""))
            probe = self.invoke("rustc", "-vV")
            self.assertEqual(probe.returncode, 0, probe.stderr)
            outputs.append(probe.stdout)
        self.assertEqual(outputs, ["actual pinned compiler identity\n"] * 2)
        self.assertFalse(self.rustup_access.exists())


if __name__ == "__main__":
    unittest.main()
