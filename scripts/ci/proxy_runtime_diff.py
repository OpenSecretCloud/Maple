#!/usr/bin/env python3
"""Compare proxy container inputs; exit like git diff --quiet (0/1/2).

The committed standalone lockfile selects the SDK actually compiled by Docker.
Include in-tree SDK source when either revision uses it, including a Cargo patch.
"""

import subprocess
import sys
import tomllib


PROXY_INPUTS = [
    "proxy/Dockerfile",
    "proxy/Cargo.toml",
    "proxy/Cargo.lock",
    "proxy/src",
]
SDK_INPUTS = ["sdk/rust/Cargo.toml", "sdk/rust/src", "sdk/rust/assets"]


def git_output(*args):
    return subprocess.run(
        ["git", *args], check=True, capture_output=True, text=True
    ).stdout


def uses_local_sdk(sha):
    lock = tomllib.loads(git_output("show", f"{sha}:proxy/Cargo.lock"))
    sdk_packages = [
        package
        for package in lock.get("package", [])
        if package.get("name") in ("maple-sdk", "opensecret")
    ]
    if len(sdk_packages) != 1:
        raise ValueError("proxy lockfile must identify exactly one Maple SDK")
    return "source" not in sdk_packages[0]


def runtime_diff(from_ref, to_ref):
    revisions = [
        git_output("rev-parse", "--verify", "--end-of-options", f"{ref}^{{commit}}").strip()
        for ref in (from_ref, to_ref)
    ]
    # Inspect both revisions before comparing, including local/registry transitions.
    local_sdk = [uses_local_sdk(sha) for sha in revisions]
    paths = PROXY_INPUTS + (SDK_INPUTS if any(local_sdk) else [])
    result = subprocess.run(["git", "diff", "--quiet", *revisions, "--", *paths])
    return result.returncode if result.returncode in (0, 1) else 2


def main():
    if len(sys.argv) != 3:
        print("usage: proxy_runtime_diff.py FROM_REF TO_REF", file=sys.stderr)
        return 2
    try:
        return runtime_diff(*sys.argv[1:])
    except (OSError, subprocess.CalledProcessError, ValueError, TypeError, AttributeError) as exc:
        print(f"Could not compare proxy container runtime inputs: {exc}", file=sys.stderr)
        return 2


if __name__ == "__main__":
    sys.exit(main())
