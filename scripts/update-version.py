#!/usr/bin/env python3
"""Update Maple's release-version sources consistently."""

import re
import sys
from pathlib import Path


ROOT = Path(__file__).resolve().parents[1]


def replace_once(path: Path, pattern: str, replacement: str) -> None:
    contents = path.read_text()
    updated, count = re.subn(pattern, replacement, contents, flags=re.MULTILINE)
    if count != 1:
        raise SystemExit(f"Expected one version field in {path.relative_to(ROOT)}, found {count}.")
    path.write_text(updated)


def main() -> None:
    if len(sys.argv) != 3:
        raise SystemExit("Usage: update-version.py X.Y.Z ANDROID_VERSION_CODE")

    version, android_version_code = sys.argv[1:]
    if not re.fullmatch(r"[0-9]+\.[0-9]+\.[0-9]+", version):
        raise SystemExit(f"Invalid version: {version}")
    if not re.fullmatch(r"[0-9]+", android_version_code):
        raise SystemExit(f"Invalid Android version code: {android_version_code}")

    replace_once(
        ROOT / "frontend/package.json",
        r'("version": ")[^"]*(")',
        rf"\g<1>{version}\g<2>",
    )
    replace_once(
        ROOT / "frontend/src-tauri/tauri.conf.json",
        r'("version": ")[^"]*(")',
        rf"\g<1>{version}\g<2>",
    )
    replace_once(
        ROOT / "frontend/src-tauri/tauri.conf.json",
        r'("versionCode": )\d+',
        rf"\g<1>{android_version_code}",
    )
    replace_once(
        ROOT / "frontend/src-tauri/Cargo.toml",
        r'^(version = ")[^"]*(")$',
        rf"\g<1>{version}\g<2>",
    )
    replace_once(
        ROOT / "frontend/src-tauri/gen/apple/project.yml",
        r'^(\s*CFBundleShortVersionString: ).*$',
        rf"\g<1>{version}",
    )
    replace_once(
        ROOT / "frontend/src-tauri/gen/apple/project.yml",
        r'^(\s*CFBundleVersion: ).*$',
        rf"\g<1>{version}",
    )
    replace_once(
        ROOT / "frontend/src-tauri/gen/apple/maple_iOS/Info.plist",
        r'(<key>CFBundleShortVersionString</key>\s*<string>)[^<]*(</string>)',
        rf"\g<1>{version}\g<2>",
    )
    replace_once(
        ROOT / "frontend/src-tauri/gen/apple/maple_iOS/Info.plist",
        r'(<key>CFBundleVersion</key>\s*<string>)[^<]*(</string>)',
        rf"\g<1>{version}\g<2>",
    )


if __name__ == "__main__":
    main()
