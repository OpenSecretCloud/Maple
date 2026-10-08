#!/usr/bin/env python3
"""Select native iOS validation without Research or SDK-consumer fanout."""

from __future__ import annotations

import argparse
from collections.abc import Iterable
import sys

from hook_change_detection import classify_path


SHARED_INPUTS = frozenset({
    "flake.nix", "flake.lock", "scripts/ci/hook_change_detection.py",
    "scripts/ci/ios_native_change_detection.py", "scripts/ci/test_ios_native.py",
    "apps/maple-agent/app/assets/fonts/Array-Regular.otf",
    "apps/maple-research/frontend/public/fonts/Manrope-VariableFont_wght.ttf",
})


def selected(paths: Iterable[str]) -> bool:
    # SDK publication and upgrading the consumer are independent decisions.
    # Even an active local SDK link is selected through the app manifest lane.
    return any(path in SHARED_INPUTS or "ios_native" in classify_path(path) for path in paths)


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--all", action="store_true")
    args = parser.parse_args()
    if args.all:
        enabled = True
    else:
        paths = (value.decode("utf-8", errors="surrogateescape")
                 for value in sys.stdin.buffer.read().split(b"\0") if value)
        enabled = selected(paths)
    print(f"ios_native={'true' if enabled else 'false'}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
