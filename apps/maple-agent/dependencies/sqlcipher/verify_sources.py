#!/usr/bin/env python3
"""Verify the pinned Git provider and registry source hashes without building code."""

import argparse
import hashlib
import json
from pathlib import Path
import sys
import tomllib


ROOT = Path(__file__).resolve().parent
PROVENANCE = json.loads((ROOT / "provenance.json").read_text(encoding="utf-8"))


def require(condition, message):
    if not condition:
        raise ValueError(message)


def sha256(path):
    return hashlib.sha256(path.read_bytes()).hexdigest()


def fork_source():
    package = PROVENANCE["packages"]["libsqlite3-sys"]
    return f"git+{package['repository']}?rev={package['revision']}#{package['revision']}"


def selected_package(metadata, lock, name, expected_source):
    expected = PROVENANCE["packages"][name]
    matches = [package for package in metadata["packages"] if package["name"] == name]
    require(
        len(matches) == 1 and matches[0]["version"] == expected["version"],
        f"expected one {name} {expected['version']} in metadata",
    )
    package = matches[0]
    require(package.get("source") == expected_source, f"{name} must resolve to the reviewed source")
    # A root package's lockfile can contain another version solely for an
    # inactive optional feature. Metadata above defines the selected graph.
    locked = [
        item for item in lock["package"]
        if item["name"] == name and item["version"] == expected["version"]
    ]
    require(
        len(locked) == 1
        and locked[0]["version"] == expected["version"]
        and locked[0].get("source") == expected_source,
        f"{name} lock entry differs from the reviewed source",
    )
    directory = Path(package["manifest_path"]).parent
    for relative, digest in expected["files_sha256"].items():
        require(sha256(directory / relative) == digest, f"{name} source differs: {relative}")
    return directory, locked[0]


def source_packages(metadata_path):
    metadata = json.loads(metadata_path.read_text(encoding="utf-8"))
    lock = tomllib.loads((Path(metadata["workspace_root"]) / "Cargo.lock").read_text(encoding="utf-8"))
    selected_package(metadata, lock, "libsqlite3-sys", fork_source())
    result = {}
    for name in ("sqlcipher-src", "openssl-src"):
        directory, locked = selected_package(
            metadata, lock, name, "registry+https://github.com/rust-lang/crates.io-index"
        )
        expected = PROVENANCE["packages"][name]
        require(locked.get("checksum") == expected["archive_sha256"], f"{name} checksum differs")
        result[name] = directory
    return result


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--metadata", type=Path, required=True,
                        help="cargo metadata --locked --format-version 1 output")
    arguments = parser.parse_args()
    packages = source_packages(arguments.metadata)
    print(json.dumps({
        "git_provider_revision": PROVENANCE["packages"]["libsqlite3-sys"]["revision"],
        "registry_source_hashes_match": sorted(packages),
    }, sort_keys=True))


if __name__ == "__main__":
    try:
        main()
    except (OSError, ValueError, KeyError) as error:
        print(f"source verification failed: {error}", file=sys.stderr)
        sys.exit(1)
