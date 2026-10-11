#!/usr/bin/env python3
"""Verify Tauri's package marker; correct it when repacking a DEB payload as RPM."""

import argparse
from pathlib import Path
import sys


# Tauri's bundler patches this fixed-length token in the packaged executable.
PREFIX = b"__TAURI_BUNDLE_TYPE_VAR_"
SUFFIXES = {"deb": b"DEB", "rpm": b"RPM", "appimage": b"APP"}


def verify_bundle_type(path, expected, replacement=None):
    if replacement is not None and (expected, replacement) != ("deb", "rpm"):
        raise ValueError("only the DEB-to-RPM transition is supported")
    if path.is_symlink() or not path.is_file():
        raise ValueError("expected a regular executable file")

    data = path.read_bytes()
    if data.count(PREFIX) != 1:
        raise ValueError("expected exactly one Tauri bundle marker")
    offset = data.index(PREFIX) + len(PREFIX)
    suffix = data[offset:offset + 3]
    if len(suffix) != 3:
        raise ValueError("truncated Tauri bundle marker")
    if suffix not in SUFFIXES.values():
        raise ValueError("unrecognized Tauri bundle marker")
    if suffix != SUFFIXES[expected]:
        raise ValueError(f"expected {expected} bundle marker, found {suffix.decode().lower()}")

    if replacement is not None:
        # Patch only the three-byte suffix, retaining all other bytes and mode.
        with path.open("r+b") as stream:
            stream.seek(offset)
            stream.write(SUFFIXES[replacement])


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--expect", required=True, choices=SUFFIXES)
    parser.add_argument("--set", choices=["rpm"], dest="replacement")
    parser.add_argument("file", type=Path)
    args = parser.parse_args()
    try:
        verify_bundle_type(args.file, args.expect, args.replacement)
    except (OSError, ValueError) as error:
        print(f"Tauri bundle type check failed: {error}", file=sys.stderr)
        return 1
    kind = args.replacement or args.expect
    print(f"verified-tauri-bundle-type {kind} {args.file}")
    return 0


if __name__ == "__main__":
    sys.exit(main())
