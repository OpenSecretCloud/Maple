#!/usr/bin/env python3
"""Unit tests for the monorepo CI path classifier."""

import unittest

from change_detection import OUTPUTS, classify_paths


def enabled(*names: str) -> dict[str, bool]:
    selected = set(names)
    return {name: name in selected for name in OUTPUTS}


class ChangeDetectionTests(unittest.TestCase):
    def assert_routes(self, paths: list[str], *names: str) -> None:
        self.assertEqual(classify_paths(paths), enabled(*names))

    def test_documentation_and_independent_components_skip_maple_app_builds(self) -> None:
        self.assert_routes(["README.md", "docs/monorepo-plan.md"])
        self.assert_routes(["sdk/src/lib/index.ts", "sdk/rust/src/client.rs"])
        self.assert_routes(["updates/src/index.ts"])
        self.assert_routes([".githooks/pre-commit", "justfile", "zapstore.yaml"])

    def test_renderer_changes_only_mark_the_frontend_lane(self) -> None:
        self.assert_routes(["frontend/src/routes/index.tsx"], "frontend")
        self.assert_routes(["frontend/public/favicon.svg"], "frontend")

    def test_frontend_dependency_changes_affect_every_packaged_app(self) -> None:
        self.assert_routes(
            ["frontend/package.json"],
            "frontend",
            "macos",
            "linux",
            "windows",
            "ios",
            "android",
        )

    def test_shared_native_changes_affect_every_native_target(self) -> None:
        self.assert_routes(
            ["frontend/src-tauri/src/lib.rs"],
            "macos",
            "linux",
            "windows",
            "ios",
            "android",
        )

    def test_platform_owned_paths_only_mark_their_platform(self) -> None:
        self.assert_routes(["frontend/src-tauri/gen/android/build.gradle.kts"], "android")
        self.assert_routes(["frontend/src-tauri/icons/android/mipmap-hdpi/ic_launcher.png"], "android")
        self.assert_routes(["frontend/src-tauri/gen/apple/project.yml"], "ios")
        self.assert_routes(["frontend/src-tauri/resources/windows/install-dlls.nsh"], "windows")
        self.assert_routes(["frontend/src-tauri/tauri.macos.conf.json"], "macos")

    def test_ios_onnx_inputs_mark_the_cache_warmer(self) -> None:
        self.assert_routes(
            ["frontend/src-tauri/scripts/onnxruntime-pins.sh"],
            "macos",
            "linux",
            "windows",
            "ios",
            "android",
            "ios_onnx",
        )

    def test_mixed_changes_union_their_routes(self) -> None:
        self.assert_routes(
            ["sdk/src/lib/index.ts", "frontend/src/routes/index.tsx", "frontend/src-tauri/gen/android/build.gradle.kts"],
            "frontend",
            "android",
        )

    def test_unclassified_root_paths_fail_safe(self) -> None:
        self.assert_routes(["new-build-system/input.toml"], *OUTPUTS)
        self.assert_routes(["../outside"], *OUTPUTS)


if __name__ == "__main__":
    unittest.main()
