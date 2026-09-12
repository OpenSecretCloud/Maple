#!/usr/bin/env python3
"""Check the SDK and embedded proxy identities in Research and Agent graphs."""

import json
from pathlib import Path
import sys


ROOT = Path(__file__).resolve().parents[2]
DEPENDENCIES = {"maple-sdk": "sdk/rust/Cargo.toml", "maple-proxy": "proxy/Cargo.toml"}
CRATES_IO = "registry+https://github.com/rust-lang/crates.io-index"


def verify(metadata: dict, root: Path = ROOT) -> None:
    if any(package["name"] == "opensecret" for package in metadata["packages"]):
        raise ValueError("Maple must not resolve the legacy opensecret SDK alongside maple-sdk")
    for name, relative in DEPENDENCIES.items():
        packages = [package for package in metadata["packages"] if package["name"] == name]
        if len(packages) != 1:
            raise ValueError(f"Maple must resolve exactly one {name} crate")
        package = packages[0]
        if name == "maple-sdk" and package.get("source") == CRATES_IO:
            continue
        if package.get("source") is not None or Path(package["manifest_path"]).resolve() != (root / relative).resolve():
            allowed = f"the monorepo's {relative}"
            if name == "maple-sdk":
                allowed += " or crates.io"
            raise ValueError(f"Maple must resolve {name} from {allowed}")


def main() -> int:
    if len(sys.argv) != 2:
        raise SystemExit("usage: verify-agent-rust-deps.py CARGO_METADATA_JSON")
    with open(sys.argv[1], encoding="utf-8") as metadata_file:
        metadata = json.load(metadata_file)
    try:
        verify(metadata)
    except (KeyError, ValueError) as error:
        raise SystemExit(str(error)) from None
    sdk = next(package for package in metadata["packages"] if package["name"] == "maple-sdk")
    source = "crates.io" if sdk.get("source") == CRATES_IO else "local sdk/rust"
    print(f"Maple resolves maple-sdk {sdk['version']} from {source}, with one local proxy and no legacy SDK.")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
