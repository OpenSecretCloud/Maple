#!/usr/bin/env python3
"""Run released rusqlite's own tests against Maple's static SQLCipher provider."""

import argparse
import json
from pathlib import Path
import shutil
import subprocess
import sys
import tempfile


COMPONENT = Path(__file__).resolve().parents[1]


def test(metadata_path):
    verifier = COMPONENT / "dependencies/sqlcipher/verify_sources.py"
    subprocess.run([sys.executable, str(verifier), "--metadata", str(metadata_path)], check=True)
    metadata = json.loads(metadata_path.read_text(encoding="utf-8"))
    packages = [package for package in metadata["packages"] if package["name"] == "rusqlite"]
    if (
        len(packages) != 1 or packages[0]["version"] != "0.40.2"
        or packages[0]["source"] != "registry+https://github.com/rust-lang/crates.io-index"
    ):
        raise ValueError("expected the released rusqlite 0.40.2 registry source")
    source = Path(packages[0]["manifest_path"]).parent
    provenance = json.loads((COMPONENT / "dependencies/sqlcipher/provenance.json").read_text(encoding="utf-8"))
    provider = provenance["packages"]["libsqlite3-sys"]
    with tempfile.TemporaryDirectory(prefix="maple-rusqlite-provider-") as temporary:
        work = Path(temporary) / "rusqlite"
        shutil.copytree(source, work)
        manifest = work / "Cargo.toml"
        contents = manifest.read_text(encoding="utf-8")
        contents += (
            "\n[workspace]\n\n[patch.crates-io]\n"
            f"libsqlite3-sys = {{ git = {json.dumps(provider['repository'])}, "
            f"rev = {json.dumps(provider['revision'])} }}\n"
            "\n[dev-dependencies.libsqlite3-sys]\n"
            'version = "=0.38.2"\nfeatures = ["bundled-sqlcipher-openssl4"]\n'
        )
        manifest.write_text(contents, encoding="utf-8")
        # This disposable upstream suite resolves its test-only dependencies.
        # Production sources/versions remain exact through the reviewed Git pin.
        # The shipping workspace and its lockfile are never modified here.
        selected = subprocess.run(
            ["cargo", "metadata", "--format-version", "1"],
            cwd=work, text=True, encoding="utf-8", capture_output=True, check=True,
        )
        selected_metadata = Path(temporary) / "metadata.json"
        selected_metadata.write_text(selected.stdout, encoding="utf-8")
        subprocess.run(
            [sys.executable, str(verifier), "--metadata", str(selected_metadata)], check=True,
        )
        subprocess.run(
            ["cargo", "test", "--locked", "--features", "bundled-sqlcipher,modern-full"],
            cwd=work, check=True,
        )


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("metadata", type=Path)
    arguments = parser.parse_args()
    test(arguments.metadata.resolve())


if __name__ == "__main__":
    try:
        main()
    except (OSError, ValueError, KeyError, subprocess.CalledProcessError) as error:
        print(f"rusqlite provider tests failed: {error}", file=sys.stderr)
        sys.exit(1)
