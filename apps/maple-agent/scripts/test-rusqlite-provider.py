#!/usr/bin/env python3
"""Run released rusqlite's own tests against Maple's static SQLCipher provider.

The default invocation prepares and tests a disposable source copy. CI may use
--phase prepare, restore a cache keyed by dependency-fingerprint, then use
--phase run with the same explicit temporary work and target directories.
"""

import argparse
from contextlib import ExitStack
import hashlib
import json
import os
from pathlib import Path
import shutil
import subprocess
import sys
import tempfile


HELPER = Path(__file__).resolve()
COMPONENT = HELPER.parents[1]
FEATURES = "bundled-sqlcipher,modern-full"
REGISTRY = "registry+https://github.com/rust-lang/crates.io-index"
SELECTED_METADATA = "maple-provider-metadata.json"
PREPARED_STATE = "maple-provider-state.json"


def sha256(path):
    return hashlib.sha256(path.read_bytes()).hexdigest()


def verifier():
    return COMPONENT / "dependencies/sqlcipher/verify_sources.py"


def verify_sources(metadata_path):
    subprocess.run(
        [sys.executable, str(verifier()), "--metadata", str(metadata_path)], check=True,
    )


def released_rusqlite(metadata_path):
    metadata = json.loads(metadata_path.read_text(encoding="utf-8"))
    packages = [package for package in metadata["packages"] if package["name"] == "rusqlite"]
    if (
        len(packages) != 1 or packages[0]["version"] != "0.40.2"
        or packages[0]["source"] != REGISTRY
    ):
        raise ValueError("expected the released rusqlite 0.40.2 registry source")
    return packages[0]


def suite_environment(target):
    # The shell normally shares Agent build intermediates between worktrees.
    # This upstream test-only graph must never inherit shipping artifacts.
    environment = os.environ.copy()
    environment["CARGO_TARGET_DIR"] = str(target)
    environment["CARGO_BUILD_BUILD_DIR"] = str(target / "build")
    return environment


def check_directories(work, target):
    if work.is_relative_to(COMPONENT) or target.is_relative_to(COMPONENT):
        raise ValueError("provider work and target directories must be outside the shipping component")
    if work == target or work.is_relative_to(target) or target.is_relative_to(work):
        raise ValueError("provider work and target directories must be separate")


def prepared_metadata(work):
    metadata = json.loads((work / SELECTED_METADATA).read_text(encoding="utf-8"))
    if Path(metadata["workspace_root"]).resolve() != work:
        raise ValueError("prepared metadata does not belong to the provider work directory")
    roots = [package for package in metadata["packages"] if package["name"] == "rusqlite"]
    if (
        len(roots) != 1 or roots[0]["version"] != "0.40.2"
        or roots[0].get("source") is not None
        or Path(roots[0]["manifest_path"]).resolve() != work / "Cargo.toml"
    ):
        raise ValueError("prepared metadata must select the copied released rusqlite suite")
    return metadata


def dependency_fingerprint(work, upstream):
    selected = prepared_metadata(work)
    # Metadata paths are deliberately excluded: equivalent clean runners must
    # share a key. The resolved lock, manifest and selected sources bind the
    # test-only graph; the caller separately binds the compiler/native ABI.
    identity = {
        "format": 1,
        "manifest": sha256(work / "Cargo.toml"),
        "lock": sha256(work / "Cargo.lock"),
        "helper": sha256(HELPER),
        "verifier": sha256(verifier()),
        "provenance": sha256(COMPONENT / "dependencies/sqlcipher/provenance.json"),
        "features": FEATURES,
        "upstream": {key: upstream[key] for key in ("name", "version", "source")},
        "selected": sorted(
            (package["name"], package["version"], package.get("source") or "workspace")
            for package in selected["packages"]
        ),
    }
    return hashlib.sha256(json.dumps(identity, sort_keys=True).encode("utf-8")).hexdigest()


def prepare(metadata_path, work, target):
    check_directories(work, target)
    # Fail before preparing a graph or emitting any cache key if production
    # provider sources no longer match the reviewed provenance.
    verify_sources(metadata_path)
    upstream = released_rusqlite(metadata_path)
    source = Path(upstream["manifest_path"]).parent.resolve()
    if work == source or work.is_relative_to(source):
        raise ValueError("the provider work directory must be a separate source copy")
    if work.exists() and (not work.is_dir() or any(work.iterdir())):
        raise ValueError("prepare requires a new or empty temporary work directory")
    provenance = json.loads(
        (COMPONENT / "dependencies/sqlcipher/provenance.json").read_text(encoding="utf-8")
    )
    provider = provenance["packages"]["libsqlite3-sys"]
    shutil.copytree(source, work, dirs_exist_ok=True)
    manifest = work / "Cargo.toml"
    contents = manifest.read_text(encoding="utf-8")
    contents += (
        "\n[workspace]\n\n[patch.crates-io]\n"
        f"libsqlite3-sys = {{ git = {json.dumps(provider['repository'])}, "
        f"rev = {json.dumps(provider['revision'])} }}\n"
        "\n[dev-dependencies.libsqlite3-sys]\n"
        f"version = {json.dumps('=' + provider['version'])}\n"
        'features = ["bundled-sqlcipher-openssl4"]\n'
    )
    manifest.write_text(contents, encoding="utf-8")
    # Resolve the upstream suite's additional test dependencies before cache
    # restore. Never key this cache from the shipping workspace's Cargo.lock.
    selected = subprocess.run(
        ["cargo", "metadata", "--format-version", "1", "--features", FEATURES],
        cwd=work, env=suite_environment(target), text=True, encoding="utf-8",
        capture_output=True, check=True,
    )
    selected_path = work / SELECTED_METADATA
    selected_path.write_text(selected.stdout, encoding="utf-8")
    prepared_metadata(work)
    verify_sources(selected_path)
    fingerprint = dependency_fingerprint(work, upstream)
    (work / PREPARED_STATE).write_text(json.dumps({
        "format": 1,
        "dependency-fingerprint": fingerprint,
        "metadata-sha256": sha256(selected_path),
        "target-dir": str(target),
    }, sort_keys=True) + "\n", encoding="utf-8")
    return fingerprint


def run_prepared(metadata_path, work, target):
    check_directories(work, target)
    verify_sources(metadata_path)
    upstream = released_rusqlite(metadata_path)
    state = json.loads((work / PREPARED_STATE).read_text(encoding="utf-8"))
    if state["format"] != 1 or state["target-dir"] != str(target):
        raise ValueError("prepared provider state has a different format or target directory")
    if state["metadata-sha256"] != sha256(work / SELECTED_METADATA):
        raise ValueError("prepared provider metadata changed after cache preparation")
    prepared_metadata(work)
    verify_sources(work / SELECTED_METADATA)
    if state["dependency-fingerprint"] != dependency_fingerprint(work, upstream):
        raise ValueError("prepared provider dependencies changed after cache preparation")
    subprocess.run(
        ["cargo", "test", "--locked", "--features", FEATURES, "--timings"],
        cwd=work, env=suite_environment(target), check=True,
    )


def test(metadata_path):
    """Keep the original single-metadata invocation disposable."""
    with tempfile.TemporaryDirectory(prefix="maple-rusqlite-provider-") as temporary:
        root = Path(temporary)
        prepare(metadata_path, root / "rusqlite", root / "target")
        run_prepared(metadata_path, root / "rusqlite", root / "target")


def main(argv=None):
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("metadata", type=Path)
    parser.add_argument("--phase", choices=("all", "prepare", "run"), default="all")
    parser.add_argument("--work-dir", type=Path, help="temporary copy of the upstream suite")
    parser.add_argument("--target-dir", type=Path, help="dedicated provider-only Cargo cache")
    parser.add_argument("--github-output", type=Path, help="append dependency-fingerprint after verification")
    arguments = parser.parse_args(argv)
    if arguments.phase != "all" and (arguments.work_dir is None or arguments.target_dir is None):
        parser.error("separate phases require --work-dir and --target-dir")
    if arguments.phase == "run" and arguments.github_output is not None:
        parser.error("cache fingerprints are emitted only during preparation")
    with ExitStack() as stack:
        temporary = None
        if arguments.work_dir is None or arguments.target_dir is None:
            temporary = Path(stack.enter_context(tempfile.TemporaryDirectory(prefix="maple-rusqlite-provider-")))
        work = (arguments.work_dir or temporary / "rusqlite").resolve()
        target = (arguments.target_dir or temporary / "target").resolve()
        metadata_path = arguments.metadata.resolve()
        if arguments.phase != "run":
            fingerprint = prepare(metadata_path, work, target)
            if arguments.github_output is not None:
                with arguments.github_output.open("a", encoding="utf-8") as output:
                    output.write(f"dependency-fingerprint={fingerprint}\n")
            print(f"rusqlite provider dependency fingerprint: {fingerprint}")
        if arguments.phase != "prepare":
            run_prepared(metadata_path, work, target)


if __name__ == "__main__":
    try:
        main()
    except (OSError, ValueError, KeyError, subprocess.CalledProcessError) as error:
        print(f"rusqlite provider tests failed: {error}", file=sys.stderr)
        sys.exit(1)
