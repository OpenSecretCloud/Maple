#!/usr/bin/env python3
"""Run rust-cache probes in the Agent build environment.

The action already passes tokenized arguments through cmd-format. Suppress only
its optional ambient Rustup inventory: that inventory does not select the
compiler used by these jobs. Actual Rust/Cargo probes must execute normally.
"""

from __future__ import annotations

from pathlib import Path
import platform
import subprocess
import sys


COMPONENT = Path(__file__).resolve().parents[2] / "apps/maple-agent"
OPTIONAL_TOOLCHAIN_LIST = ("rustup", "toolchain", "list", "--quiet")


def main(arguments: list[str] | None = None) -> int:
    arguments = list(sys.argv[1:] if arguments is None else arguments)
    if tuple(arguments) == OPTIONAL_TOOLCHAIN_LIST:
        return 0
    if not arguments:
        print("Agent cache command requires a probe executable", file=sys.stderr)
        return 2
    executable = arguments[0].replace("\\", "/").rsplit("/", 1)[-1].lower()
    if executable in ("rustup", "rustup.exe"):
        print("Unexpected Rustup probe in Agent cache command", file=sys.stderr)
        return 2
    command = arguments if platform.system() == "Windows" else [
        "nix", "develop", "--no-update-lock-file", str(COMPONENT), "-c", *arguments,
    ]
    try:
        # Inherit both output streams. No joining, reparsing or shell expansion:
        # paths with spaces and every argument retain the action's tokenization.
        return subprocess.run(command, shell=False, check=False).returncode
    except OSError as error:
        print(f"Cannot run Agent cache probe: {error}", file=sys.stderr)
        return 127


if __name__ == "__main__":
    raise SystemExit(main())
