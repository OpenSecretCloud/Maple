#!/usr/bin/env python3
"""Stage previously generated, verified public notices without Cargo or network."""
import argparse
from pathlib import Path
import shutil

from third_party_notices import COMPONENT, GENERATED_FILES, native_target, output_directory, validate


def stage(component, destination, target, source=None):
    source = source or output_directory(component, target)
    validate(component, source, target)
    destination.mkdir(parents=True, exist_ok=True)
    for name in (*GENERATED_FILES, "manifest.json"):
        shutil.copyfile(source / name, destination / name)
        (destination / name).chmod(0o644)
    keys = component / "crates/maple-agent/src/store/keys"
    wordlists = destination / "wordlists"
    wordlists.mkdir(exist_ok=True)
    for name in ("eff-large-wordlist.txt", "SOURCES.md"):
        shutil.copyfile(keys / name, wordlists / name)
        (wordlists / name).chmod(0o644)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("destination", type=Path)
    parser.add_argument("--component", type=Path, default=COMPONENT)
    parser.add_argument("--target", default=native_target())
    parser.add_argument("--source", type=Path)
    arguments = parser.parse_args()
    stage(arguments.component, arguments.destination, arguments.target, arguments.source)


if __name__ == "__main__":
    main()
