#!/usr/bin/env python3
"""Generate package notices offline from one locked application's target graph."""
import argparse
import html
import json
from pathlib import Path
import subprocess
import tempfile

from third_party_notices import (
    COMPONENT, GENERATED_FILES, configuration, native_target, output_directory,
    sha256, validate,
)


def render(inventory, component, target, default_features, features):
    feature_description = ", ".join(features) or "none"
    blocks = [
        "Maple Work — Third-party notices\n",
        "This distribution includes software and data under the following licenses.\n"
        f"Application: maple-agent-app; target: {target}.\n"
        f"Default features: {default_features}; additional features: {feature_description}.\n"
        "The inventory excludes development dependencies and other workspace applications.\n"
        "It includes build dependencies that supply compiled native sources.\n"
        "These notices do not change the license of any component.\n",
        "PASSWORD GENERATION WORDLIST\n\n"
        + (component / "crates/maple-agent/src/store/keys/SOURCES.md").read_text(encoding="utf-8"),
        "SPELLING DICTIONARY\n\n" + (component / "app/assets/dict/LICENSE").read_text(encoding="utf-8"),
        "COMPILED DATABASE AND CRYPTOGRAPHY\n\n"
        "SQLCipher 4.19.0, SQLite 3.53.4; OpenSSL 4.0.3.\n"
        "SQLCipher: https://github.com/sqlcipher/sqlcipher/tree/v4.19.0\n"
        "OpenSSL: https://github.com/openssl/openssl/tree/openssl-4.0.3\n"
        "Rust wrapper patch: https://github.com/MaplePrivacyLabs/rusqlite\n"
        "Exact source pins and verification commands are recorded in\n"
        "dependencies/sqlcipher/provenance.json in the source distribution.\n",
    ]
    for license in sorted(inventory["licenses"], key=lambda value: (value["id"], value["text"])):
        packages = sorted({f"{item['crate']['name']} {item['crate']['version']}"
                           for item in license["used_by"]})
        blocks.append(f"{license['name']} ({license['id']})\n\n"
                      + "Applies to: " + ", ".join(packages) + "\n\n" + license["text"])
    text = ("\n" + "=" * 78 + "\n\n").join(blocks).rstrip() + "\n"
    document = ('<!doctype html><html lang="en"><meta charset="utf-8">'
                '<meta name="viewport" content="width=device-width">'
                '<title>Maple Work third-party notices</title>'
                '<style>body{max-width:76rem;margin:2rem auto;padding:0 1rem;'
                'font:15px/1.5 system-ui}pre{white-space:pre-wrap;overflow-wrap:anywhere}</style>'
                '<h1>Third-party notices</h1><pre>' + html.escape(text) + '</pre></html>\n')
    return {"THIRD_PARTY_NOTICES.txt": text.encode(), "THIRD_PARTY_NOTICES.html": document.encode()}


def verify_sqlcipher_attribution(inventory, component):
    """Require the actual pinned provider license, not a generic SPDX template."""
    provenance = json.loads((component / "dependencies/sqlcipher/provenance.json").read_text(encoding="utf-8"))
    package = provenance["packages"]["sqlcipher-src"]
    expected = package["files_sha256"]["sqlcipher/LICENSE-sqlcipher"]
    attributed = [license for license in inventory["licenses"]
                  if license["id"] == "BSD-3-Clause"
                  and any(item["crate"]["name"] == "sqlcipher-src"
                          and item["crate"]["version"] == package["version"]
                          for item in license["used_by"])]
    # Cargo-about may omit the final newline or use native line endings. All
    # copyright, conditions and disclaimer bytes must otherwise match the pin.
    if len(attributed) != 1 or sha256(
        (attributed[0]["text"].replace("\r\n", "\n").rstrip() + "\n").encode()
    ) != expected:
        raise ValueError("SQLCipher notices must attribute the pinned LICENSE-sqlcipher to sqlcipher-src")


def generate(component, destination, target, default_features=True, features=(), inventory=None):
    expected = configuration(component, target, default_features, features)
    with tempfile.TemporaryDirectory(prefix="maple-notices-") as temporary:
        if inventory is None:
            inventory_path = Path(temporary) / "inventory.json"
            # Cargo/Nix vendors Git crates separately from their repository root.
            # Keep the exact upstream grant as a small source input, rather than
            # requiring its checkout root or a network license fetch at build time.
            config_path = Path(temporary) / "about.toml"
            cua_license = component / "dependencies/licenses/CUA-LICENSE.md"
            config = (component / "about.toml").read_text(encoding="utf-8").replace(
                'path = "../../../../../LICENSE.md"', f"path = {json.dumps(str(cua_license))}")
            config_path.write_text(config, encoding="utf-8")
            command = ["cargo", "about", "generate", "--locked", "--offline", "--fail",
                       "--manifest-path", str(component / "app/Cargo.toml"),
                       "--config", str(config_path), "--target", target,
                       "--format", "json", "--output-file", str(inventory_path)]
            if not default_features:
                command.append("--no-default-features")
            if features:
                command.extend(["--features", " ".join(features)])
            subprocess.run(command, cwd=component, check=True)
            inventory = json.loads(inventory_path.read_text(encoding="utf-8"))
        verify_sqlcipher_attribution(inventory, component)
        outputs = render(inventory, component, target, default_features, features)
    assert set(outputs) == set(GENERATED_FILES)
    destination.mkdir(parents=True, exist_ok=True)
    for name, contents in outputs.items():
        (destination / name).write_bytes(contents)
    expected["outputs"] = {name: sha256(contents) for name, contents in outputs.items()}
    (destination / "manifest.json").write_text(json.dumps(expected, indent=2, sort_keys=True) + "\n", encoding="utf-8")
    validate(component, destination, target, default_features, features)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--target", default=native_target())
    parser.add_argument("--output", type=Path)
    parser.add_argument("--no-default-features", action="store_true")
    parser.add_argument("--features", default="", help="Comma-separated extra application features")
    parser.add_argument("--check", action="store_true", help="Validate already generated notices without Cargo")
    arguments = parser.parse_args()
    features = tuple(sorted(filter(None, arguments.features.split(","))))
    destination = arguments.output or output_directory(COMPONENT, arguments.target)
    if arguments.check:
        validate(COMPONENT, destination, arguments.target, not arguments.no_default_features, features)
    else:
        generate(COMPONENT, destination, arguments.target, not arguments.no_default_features, features)
    print(destination)


if __name__ == "__main__":
    main()
