#!/usr/bin/env python3
"""Hermetic regressions for generated-notice scope and offline packaging."""
import importlib.util
import json
import os
from pathlib import Path
import shutil
import subprocess
import sys
import tempfile
import tomllib
import unittest
from unittest import mock

from third_party_notices import output_directory, sha256, validate

SCRIPTS = Path(__file__).resolve().parent


def load_script(name):
    spec = importlib.util.spec_from_file_location(name.replace("-", "_"), SCRIPTS / f"{name}.py")
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


generator = load_script("generate-third-party-notices")
stager = load_script("stage-third-party-notices")
TARGET = "x86_64-unknown-linux-gnu"
SQLCIPHER_VERSION = "fixture-provider-version"
SQLCIPHER_LICENSE = "Copyright (c) 2025, ZETETIC LLC\nSynthetic provider license fixture.\n"
INVENTORY = {"licenses": [{"id": "MIT", "name": "MIT License", "text": "Fixture <license>",
                            "used_by": [{"crate": {"name": "fixture", "version": "1.0.0"}}]},
                           {"id": "BSD-3-Clause", "name": "BSD 3-Clause License", "text": SQLCIPHER_LICENSE,
                            "used_by": [{"crate": {"name": "sqlcipher-src", "version": SQLCIPHER_VERSION}}]}]}


def notice_fixture(component, target=TARGET):
    """Small synthetic package inputs; never copy a production dependency inventory."""
    for name, text in {
        "Cargo.toml": "[workspace]\n", "Cargo.lock": "version = 4\n",
        "app/Cargo.toml": '[package]\nname = "maple-agent-app"\n',
        "about.toml": 'accepted = ["MIT"]\n[cua-driver-contract.clarify]\nlicense = "MIT"\n'
                      '[[cua-driver-contract.clarify.files]]\npath = "../../../../../LICENSE.md"\n'
                      'checksum = "fixture"\n',
        "crates/maple-agent/src/store/keys/SOURCES.md": "EFF source attribution fixture\n",
        "crates/maple-agent/src/store/keys/eff-large-wordlist.txt": "11111 fixture\n",
        "app/assets/dict/LICENSE": "Spelling license fixture\n",
        "dependencies/sqlcipher/provenance.json": json.dumps({"packages": {"sqlcipher-src": {
            "version": SQLCIPHER_VERSION,
            "files_sha256": {"sqlcipher/LICENSE-sqlcipher": sha256(SQLCIPHER_LICENSE.encode())},
        }}}) + "\n",
        "dependencies/licenses/CUA-LICENSE.md": "MIT grant fixture\n",
    }.items():
        path = component / name
        path.parent.mkdir(parents=True, exist_ok=True)
        path.write_text(text)
    (component / "scripts").mkdir(exist_ok=True)
    for name in ("generate-third-party-notices.py", "third_party_notices.py", "stage-third-party-notices.py"):
        shutil.copyfile(SCRIPTS / name, component / "scripts" / name)
    destination = output_directory(component, target)
    generator.generate(component, destination, target, inventory=INVENTORY)
    return destination


class NoticeTests(unittest.TestCase):
    def setUp(self):
        self.temporary = tempfile.TemporaryDirectory()
        self.addCleanup(self.temporary.cleanup)
        self.component = Path(self.temporary.name) / "component"
        self.notices = notice_fixture(self.component)

    def test_sqlcipher_clarification_selects_the_pinned_compiled_provider_license(self):
        component = SCRIPTS.parent
        config = tomllib.loads((component / "about.toml").read_text())
        provenance = json.loads((component / "dependencies/sqlcipher/provenance.json").read_text())
        provider = config["sqlcipher-src"]["clarify"]
        compiled = [file for file in provider["files"] if file["license"] == "BSD-3-Clause"]
        self.assertEqual(len(compiled), 1)
        self.assertEqual(compiled[0]["path"], "sqlcipher/LICENSE-sqlcipher")
        self.assertEqual(compiled[0]["checksum"], provenance["packages"]["sqlcipher-src"]["files_sha256"][compiled[0]["path"]])
        self.assertNotIn("sqlcipher/LICENSE", [file["path"] for file in config["libsqlite3-sys"]["clarify"]["files"]])

    def test_generated_inventory_requires_the_pinned_license_attributed_to_the_provider(self):
        for change in ("missing", "placeholder", "wrong_package", "wrong_version", "duplicate"):
            inventory = json.loads(json.dumps(INVENTORY))
            provider = inventory["licenses"][1]
            if change == "missing":
                inventory["licenses"].pop()
            elif change == "placeholder":
                provider["text"] = "Copyright (c) <year> <owner>.\n"
            elif change == "wrong_package":
                provider["used_by"][0]["crate"]["name"] = "libsqlite3-sys"
            elif change == "wrong_version":
                provider["used_by"][0]["crate"]["version"] = "outdated-provider"
            else:
                inventory["licenses"].append(provider)
            with self.subTest(change=change), self.assertRaisesRegex(ValueError, "SQLCipher notices"):
                generator.generate(self.component, self.notices, TARGET, inventory=inventory)
        inventory = json.loads(json.dumps(INVENTORY))
        inventory["licenses"][1]["text"] = SQLCIPHER_LICENSE.replace("\n", "\r\n").rstrip()
        generator.generate(self.component, self.notices, TARGET, inventory=inventory)
        text = (self.notices / "THIRD_PARTY_NOTICES.txt").read_text()
        self.assertIn(f"Applies to: sqlcipher-src {SQLCIPHER_VERSION}", text)
        self.assertIn("Copyright (c) 2025, ZETETIC LLC", text)
        validate(self.component, self.notices, TARGET)

    def test_release_builder_fetches_all_locked_targets_before_offline_inventory(self):
        bash = shutil.which("bash")
        self.assertIsNotNone(bash, "The release fixture requires Bash on PATH")
        shutil.copyfile(SCRIPTS / "build-release.sh", self.component / "scripts/build-release.sh")
        commands = self.component / "commands.jsonl"
        tools = self.component / "tools"
        tools.mkdir()
        cargo = tools / "cargo"
        cargo.write_text(f"#!{sys.executable}\n"
                         "import json, os, pathlib, sys\n"
                         "args = sys.argv[1:]\n"
                         "log = pathlib.Path(os.environ['NOTICE_COMMAND_LOG'])\n"
                         "with log.open('a') as output: output.write(json.dumps(args) + '\\n')\n"
                         "if args[0] == 'fetch':\n"
                         "    assert args == ['fetch', '--locked']\n"
                         "    pathlib.Path('graph-fetched').touch()\n"
                         "else:\n"
                         "    assert pathlib.Path('graph-fetched').exists()\n")
        cargo.chmod(0o755)
        uname = tools / "uname"
        uname.write_text(f'#!{bash}\nprintf "%s\\n" "$NOTICE_TEST_OS"\n')
        uname.chmod(0o755)
        generator = self.component / "scripts/generate-third-party-notices.py"
        generator.write_text("import json, os, pathlib\n"
                             "assert pathlib.Path('graph-fetched').exists()\n"
                             "with open(os.environ['NOTICE_COMMAND_LOG'], 'a') as output:\n"
                             "    output.write(json.dumps(['offline-inventory']) + '\\n')\n")
        binary = self.component / "target/release/maple-agent"
        binary.parent.mkdir(parents=True)
        binary.write_text(f'#!{bash}\n' + 'printf \'{"profile":"%s"}\\n\' "$MAPLE_RELEASE_PROFILE"\n')
        binary.chmod(0o755)
        for platform, profile in (("Darwin", "dev"), ("Linux", "prod")):
            with self.subTest(platform=platform):
                commands.unlink(missing_ok=True)
                (self.component / "graph-fetched").unlink(missing_ok=True)
                environment = {**os.environ, "PATH": str(tools) + os.pathsep + os.environ["PATH"],
                               "NOTICE_TEST_OS": platform, "NOTICE_COMMAND_LOG": str(commands)}
                result = subprocess.run([bash, str(self.component / "scripts/build-release.sh"), profile],
                                        env=environment, capture_output=True, text=True, timeout=20)
                self.assertEqual(result.returncode, 0,
                                 f"stdout:\n{result.stdout}\nstderr:\n{result.stderr}")
                self.assertEqual([json.loads(line) for line in commands.read_text().splitlines()], [
                    ["fetch", "--locked"],
                    ["build", "--release", "-p", "maple-agent-app", "--locked"],
                    ["offline-inventory"],
                ])

    def test_inventory_command_selects_application_target_and_actual_features(self):
        def inventory(command, **kwargs):
            self.assertEqual(kwargs["cwd"], self.component)
            self.assertIn("--locked", command)
            self.assertIn("--offline", command)
            self.assertNotIn("--workspace", command)
            self.assertNotIn("--all-features", command)
            self.assertEqual(command[command.index("--manifest-path") + 1], str(self.component / "app/Cargo.toml"))
            self.assertEqual(command[command.index("--target") + 1], TARGET)
            config = Path(command[command.index("--config") + 1]).read_text()
            self.assertNotIn('path = "../../../../../LICENSE.md"', config)
            self.assertIn(str(self.component / "dependencies/licenses/CUA-LICENSE.md"), config)
            self.assertIn("--no-default-features", command)
            self.assertEqual(command[command.index("--features") + 1], "acp proxy")
            Path(command[command.index("--output-file") + 1]).write_text(json.dumps(INVENTORY))
        with mock.patch.object(generator.subprocess, "run", side_effect=inventory) as run:
            generator.generate(self.component, self.notices, TARGET, False, ("acp", "proxy"))
        run.assert_called_once()
        validate(self.component, self.notices, TARGET, False, ("acp", "proxy"))

    def test_packaging_needs_no_cargo_or_network_and_keeps_public_attribution(self):
        destination = self.component / "package/licenses"
        with mock.patch.object(subprocess, "run", side_effect=AssertionError("No process allowed")):
            stager.stage(self.component, destination, TARGET)
        self.assertEqual(sorted(path.name for path in (destination / "wordlists").iterdir()),
                         ["SOURCES.md", "eff-large-wordlist.txt"])
        self.assertIn("Fixture &lt;license&gt;", (destination / "THIRD_PARTY_NOTICES.html").read_text())
        self.assertEqual((destination / "THIRD_PARTY_NOTICES.html").stat().st_mode & 0o777, 0o644)

    def test_changed_inputs_and_target_or_feature_mismatch_refuse_staging(self):
        for target, default, features in [("aarch64-apple-darwin", True, ()), (TARGET, False, ()),
                                          (TARGET, True, ("acp",))]:
            with self.subTest(target=target, default=default, features=features), self.assertRaises(ValueError):
                validate(self.component, self.notices, target, default, features)
        (self.component / "Cargo.lock").write_text("changed lock\n")
        with self.assertRaisesRegex(ValueError, "source inputs"):
            stager.stage(self.component, self.component / "package", TARGET)

    def test_missing_tampered_and_symlinked_notice_output_refuses_staging(self):
        notice = self.notices / "THIRD_PARTY_NOTICES.html"
        notice.write_text("modified")
        with self.assertRaisesRegex(ValueError, "modified"):
            validate(self.component, self.notices, TARGET)
        notice.unlink()
        with self.assertRaisesRegex(ValueError, "Missing"):
            validate(self.component, self.notices, TARGET)
        other = self.notices / "other.html"
        other.write_text("public")
        notice.symlink_to(other)
        with self.assertRaisesRegex(ValueError, "modified"):
            validate(self.component, self.notices, TARGET)

    def test_missing_manifest_refuses_packaging(self):
        (self.notices / "manifest.json").unlink()
        with self.assertRaisesRegex(ValueError, "before packaging"):
            stager.stage(self.component, self.component / "package", TARGET)


if __name__ == "__main__":
    unittest.main()
