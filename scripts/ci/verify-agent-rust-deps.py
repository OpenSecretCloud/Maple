#!/usr/bin/env python3
"""Reject registry/fork duplicates of the SDK and proxy in Agent's Cargo graph."""

import json
from pathlib import Path
import sys


ROOT = Path(__file__).resolve().parents[2]
DEPENDENCIES = {"maple-sdk": "sdk/rust/Cargo.toml", "maple-proxy": "proxy/Cargo.toml"}


def verify(metadata: dict, root: Path = ROOT) -> None:
    if any(package["name"] == "opensecret" for package in metadata["packages"]):
        raise ValueError("Agent must not resolve the legacy opensecret SDK alongside maple-sdk")
    for name, relative in DEPENDENCIES.items():
        packages = [package for package in metadata["packages"] if package["name"] == name]
        if len(packages) != 1:
            raise ValueError(f"Agent must resolve exactly one {name} crate")
        package = packages[0]
        if package.get("source") is not None or Path(package["manifest_path"]).resolve() != (root / relative).resolve():
            raise ValueError(f"Agent must resolve {name} from the monorepo's {relative}")


def main() -> int:
    if len(sys.argv) != 2:
        raise SystemExit("usage: verify-agent-rust-deps.py CARGO_METADATA_JSON")
    with open(sys.argv[1], encoding="utf-8") as metadata_file:
        metadata = json.load(metadata_file)
    try:
        verify(metadata)
    except (KeyError, ValueError) as error:
        raise SystemExit(str(error)) from None
    print("Maple Agent resolves exactly one in-tree Maple SDK and proxy crate, with no legacy SDK.")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
