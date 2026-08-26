#!/usr/bin/env python3
"""Classify changed repository paths for Maple's expensive app build jobs."""

from __future__ import annotations

import argparse
import sys
from collections.abc import Iterable


OUTPUTS = ("frontend", "macos", "linux", "windows", "ios", "android", "ios_onnx")
NATIVE_PLATFORMS = frozenset({"macos", "linux", "windows", "ios", "android"})
DESKTOP_PLATFORMS = frozenset({"macos", "linux", "windows"})
ALL_OUTPUTS = frozenset(OUTPUTS)

INERT_ROOT_FILES = frozenset(
    {
        ".gitignore",
        ".repo_ignore",
        "AGENTS.md",
        "deny.toml",
        "justfile",
        "LICENSE",
        "README.md",
        "repo.meta.json",
        "setup-hooks.sh",
        "zapstore.yaml",
    }
)
INERT_PREFIXES = (".agents/", ".githooks/", "docs/", "updates/")
PURE_FRONTEND_PREFIXES = ("frontend/public/", "frontend/src/")
PURE_FRONTEND_FILES = frozenset({"frontend/icon.svg", "frontend/index.html"})
SDK_FRONTEND_PREFIXES = ("sdk/src/",)
SDK_TEST_PREFIXES = ("sdk/src/lib/test/",)
SDK_FRONTEND_FILES = frozenset(
    {
        "sdk/bun.lock",
        "sdk/bunfig.toml",
        "sdk/package.json",
        "sdk/tsconfig.build.json",
        "sdk/tsconfig.json",
        "sdk/vite.config.ts",
    }
)

IOS_ONNX_INPUTS = frozenset(
    {
        "flake.lock",
        "flake.nix",
        "scripts/ci/_common.sh",
        "scripts/ci/ios-onnxruntime.sh",
        "frontend/src-tauri/scripts/build-ios-onnxruntime-all.sh",
        "frontend/src-tauri/scripts/canonicalize-static-archive.py",
        "frontend/src-tauri/scripts/onnxruntime-pins.sh",
    }
)

WORKFLOW_ROUTES = {
    ".github/workflows/android-build.yml": frozenset({"android"}),
    ".github/workflows/android-pr-build.yml": frozenset({"android"}),
    ".github/workflows/app-change-detection.yml": NATIVE_PLATFORMS,
    ".github/workflows/desktop-build.yml": DESKTOP_PLATFORMS,
    ".github/workflows/desktop-pr-build.yml": DESKTOP_PLATFORMS,
    ".github/workflows/mobile-build.yml": frozenset({"ios", "ios_onnx"}),
    ".github/workflows/mobile-pr-build.yml": frozenset({"ios"}),
}

CI_SCRIPT_ROUTES = {
    "scripts/ci/_common.sh": frozenset({"frontend", *NATIVE_PLATFORMS, "ios_onnx"}),
    "scripts/ci/android-pr.sh": frozenset({"android"}),
    "scripts/ci/android-release.sh": frozenset({"android"}),
    "scripts/ci/attestation-manifest.sh": NATIVE_PLATFORMS,
    "scripts/ci/canonical-ios-app-hash.py": frozenset({"ios"}),
    "scripts/ci/canonical-windows-nsis-payload-hash.py": frozenset({"windows"}),
    "scripts/ci/desktop-pr.sh": DESKTOP_PLATFORMS,
    "scripts/ci/desktop-release.sh": DESKTOP_PLATFORMS,
    "scripts/ci/desktop-windows-pr.sh": frozenset({"windows"}),
    "scripts/ci/desktop-windows-release.sh": frozenset({"windows"}),
    "scripts/ci/frontend.sh": frozenset({"frontend"}),
    "scripts/ci/install-windows-artifact-signing.ps1": frozenset({"windows"}),
    "scripts/ci/install-windows-minisign.ps1": frozenset({"windows"}),
    "scripts/ci/ios-onnxruntime.sh": frozenset({"ios", "ios_onnx"}),
    "scripts/ci/ios-pr.sh": frozenset({"ios"}),
    "scripts/ci/ios-release.sh": frozenset({"ios"}),
    "scripts/ci/verify-release-artifacts.sh": NATIVE_PLATFORMS,
    "scripts/ci/web.sh": frozenset({"frontend"}),
    "scripts/ci/windows-artifact-sign.ps1": frozenset({"windows"}),
}


def _native_path_routes(path: str) -> frozenset[str]:
    android_prefixes = (
        "frontend/src-tauri/gen/android/",
        "frontend/src-tauri/icons/android/",
        "frontend/src-tauri/capabilities/mobile-android.json",
    )
    ios_prefixes = (
        "frontend/src-tauri/gen/apple/",
        "frontend/src-tauri/icons/ios/",
        "frontend/src-tauri/onnxruntime-ios/",
        "frontend/src-tauri/capabilities/mobile-ios.json",
    )
    windows_prefixes = ("frontend/src-tauri/resources/windows/",)

    if path.startswith(android_prefixes):
        return frozenset({"android"})
    if path.startswith(ios_prefixes):
        return frozenset({"ios"})
    if path.startswith(windows_prefixes):
        return frozenset({"windows"})

    if path in {
        "frontend/src-tauri/Entitlements.plist",
        "frontend/src-tauri/Info.plist",
        "frontend/src-tauri/apple-sign-in-info.md",
    }:
        return frozenset({"macos", "ios"})
    if path == "frontend/src-tauri/tauri.macos.conf.json":
        return frozenset({"macos"})
    if path == "frontend/src-tauri/tauri.windows.conf.json":
        return frozenset({"windows"})

    if path.startswith("frontend/src-tauri/scripts/"):
        filename = path.rsplit("/", 1)[-1]
        if "android" in filename:
            return frozenset({"android"})
        if "ios" in filename or "apple" in filename or filename == "canonicalize-static-archive.py":
            routes = {"ios"}
            if path in IOS_ONNX_INPUTS:
                routes.add("ios_onnx")
            return frozenset(routes)
        if "windows" in filename:
            return frozenset({"windows"})
        if "linux" in filename:
            return frozenset({"linux"})
        if "macos" in filename:
            return frozenset({"macos"})
        if filename == "run-with-desktop-onnxruntime.sh":
            return DESKTOP_PLATFORMS
        if filename == "onnxruntime-pins.sh":
            return frozenset({*NATIVE_PLATFORMS, "ios_onnx"})

    # Unknown files inside the native application remain fail-safe for every target.
    return NATIVE_PLATFORMS


def classify_path(path: str) -> frozenset[str]:
    """Return the app build lanes affected by one repository-relative path."""

    if not path or path.startswith("/") or ".." in path.split("/"):
        return ALL_OUTPUTS
    if path in INERT_ROOT_FILES or path.startswith(INERT_PREFIXES):
        return frozenset()
    if path.startswith(SDK_TEST_PREFIXES):
        return frozenset()
    if path in SDK_FRONTEND_FILES or path.startswith(SDK_FRONTEND_PREFIXES):
        return frozenset({"frontend"})
    if path.startswith("sdk/"):
        return frozenset()
    if path.startswith("proxy/"):
        # Proxy has its own path-scoped checks until the native app starts
        # consuming the in-tree crate in the follow-up dependency switch.
        return frozenset()
    if path in PURE_FRONTEND_FILES or path.startswith(PURE_FRONTEND_PREFIXES):
        return frozenset({"frontend"})
    if path.startswith("frontend/src-tauri/"):
        return _native_path_routes(path)
    if path.startswith("frontend/"):
        # Dependency and build-configuration changes can affect every packaged app.
        return frozenset({"frontend", *NATIVE_PLATFORMS})
    if path in {"flake.lock", "flake.nix"}:
        return ALL_OUTPUTS
    if path in WORKFLOW_ROUTES:
        return WORKFLOW_ROUTES[path]
    if path.startswith(".github/workflows/"):
        return frozenset()
    if path in CI_SCRIPT_ROUTES:
        return CI_SCRIPT_ROUTES[path]
    if path.startswith("scripts/ci/"):
        return frozenset()

    # A new, unclassified root is potentially an application build input.
    return ALL_OUTPUTS


def classify_paths(paths: Iterable[str]) -> dict[str, bool]:
    routes: set[str] = set()
    for path in paths:
        routes.update(classify_path(path))
    return {name: name in routes for name in OUTPUTS}


def _read_null_delimited_paths() -> list[str]:
    raw_paths = sys.stdin.buffer.read().split(b"\0")
    return [path.decode("utf-8", errors="surrogateescape") for path in raw_paths if path]


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument(
        "--all",
        action="store_true",
        help="Conservatively enable every output without reading paths",
    )
    args = parser.parse_args()

    result = {name: True for name in OUTPUTS} if args.all else classify_paths(_read_null_delimited_paths())
    for name in OUTPUTS:
        print(f"{name}={'true' if result[name] else 'false'}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
