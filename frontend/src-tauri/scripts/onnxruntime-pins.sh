#!/usr/bin/env bash

onnxruntime_linux_x64_archive_sha256_for_version() {
  case "$1" in
    1.22.0)
      printf '%s\n' "8344d55f93d5bc5021ce342db50f62079daf39aaafb5d311a451846228be49b3"
      ;;
    *)
      echo "No pinned Linux x64 ONNX Runtime archive SHA-256 for version '$1'." >&2
      return 1
      ;;
  esac
}

onnxruntime_linux_aarch64_archive_sha256_for_version() {
  case "$1" in
    1.22.0)
      printf '%s\n' "bb76395092d150b52c7092dc6b8f2fe4d80f0f3bf0416d2f269193e347e24702"
      ;;
    *)
      echo "No pinned Linux aarch64 ONNX Runtime archive SHA-256 for version '$1'." >&2
      return 1
      ;;
  esac
}

onnxruntime_linux_x64_dylib_sha256_for_version() {
  case "$1" in
    1.22.0)
      printf '%s\n' "3da6146e14e7b8aaec625dde11d6114c7457c87a5f93d744897da8781e35c673"
      ;;
    *)
      echo "No pinned Linux x64 ONNX Runtime shared-library SHA-256 for version '$1'." >&2
      return 1
      ;;
  esac
}

onnxruntime_linux_aarch64_dylib_sha256_for_version() {
  case "$1" in
    1.22.0)
      printf '%s\n' "0afd69a0ae38c5099fd0e8604dda398ac43dee67cd9c6394b5142b19e82528de"
      ;;
    *)
      echo "No pinned Linux aarch64 ONNX Runtime shared-library SHA-256 for version '$1'." >&2
      return 1
      ;;
  esac
}

onnxruntime_windows_x64_archive_sha256_for_version() {
  case "$1" in
    1.22.0)
      printf '%s\n' "174c616efc0271194488642a72f1a514e01487da4dfe84c49296d66e40ebe0da"
      ;;
    *)
      echo "No pinned Windows x64 ONNX Runtime archive SHA-256 for version '$1'." >&2
      return 1
      ;;
  esac
}

onnxruntime_windows_x64_dll_sha256_for_version() {
  case "$1" in
    1.22.0)
      printf '%s\n' "579b636403983254346a5c1d80bd28f1519cd1e284cd204f8d4ff41f8d711559"
      ;;
    *)
      echo "No pinned Windows x64 ONNX Runtime DLL SHA-256 for version '$1'." >&2
      return 1
      ;;
  esac
}

windows_vc_redist_x64_version() {
  printf '%s\n' "14.44.35211"
}

windows_vc_redist_x64_url_for_version() {
  case "$1" in
    14.44.35211)
      printf '%s\n' "https://download.visualstudio.microsoft.com/download/pr/7ebf5fdb-36dc-4145-b0a0-90d3d5990a61/CC0FF0EB1DC3F5188AE6300FAEF32BF5BEEBA4BDD6E8E445A9184072096B713B/VC_redist.x64.exe"
      ;;
    *)
      echo "No pinned Windows x64 VC++ Redistributable URL for version '$1'." >&2
      return 1
      ;;
  esac
}

windows_vc_redist_x64_archive_sha256_for_version() {
  case "$1" in
    14.44.35211)
      printf '%s\n' "cc0ff0eb1dc3f5188ae6300faef32bf5beeba4bdd6e8e445a9184072096b713b"
      ;;
    *)
      echo "No pinned Windows x64 VC++ Redistributable SHA-256 for version '$1'." >&2
      return 1
      ;;
  esac
}

windows_wix_cli_version() {
  printf '%s\n' "6.0.2"
}

windows_wix_cli_url_for_version() {
  case "$1" in
    6.0.2)
      printf '%s\n' "https://www.nuget.org/api/v2/package/wix/6.0.2"
      ;;
    *)
      echo "No pinned WiX CLI NuGet package URL for version '$1'." >&2
      return 1
      ;;
  esac
}

windows_wix_cli_archive_sha256_for_version() {
  case "$1" in
    6.0.2)
      printf '%s\n' "13caed0aa86898c9952eb8ba82c6ac6b43d1575bb731ac848e5edf5490a10428"
      ;;
    *)
      echo "No pinned WiX CLI NuGet package SHA-256 for version '$1'." >&2
      return 1
      ;;
  esac
}

onnxruntime_ios_commit_for_version() {
  case "$1" in
    1.22.2)
      printf '%s\n' "5630b081cd25e4eccc7516a652ff956e51676794"
      ;;
    *)
      echo "No pinned ONNX Runtime iOS source commit for version '$1'." >&2
      return 1
      ;;
  esac
}

onnxruntime_ios_device_lib_sha256_for_version() {
  local xcode_build="${2:-${MAPLE_XCODE_BUILD_VERSION:-17F42}}"

  case "$1:${xcode_build}" in
    1.22.2:17F42)
      printf '%s\n' "d202f35d0567b0f8d5cf14192ab6034dd17be481af58153c5eedbefcb9084fc7"
      ;;
    1.22.2:17F5022i)
      printf '%s\n' "d202f35d0567b0f8d5cf14192ab6034dd17be481af58153c5eedbefcb9084fc7"
      ;;
    *)
      echo "No pinned ONNX Runtime iOS device library SHA-256 for version '$1' and Xcode build '${xcode_build}'." >&2
      return 1
      ;;
  esac
}

onnxruntime_ios_simulator_lib_sha256_for_version() {
  local xcode_build="${2:-${MAPLE_XCODE_BUILD_VERSION:-17F42}}"

  case "$1:${xcode_build}" in
    1.22.2:17F42)
      printf '%s\n' "a24992c2e26049eb8b1aaf90e7dbf03fe24ed140e7b365b5c2880e3f0953baa9"
      ;;
    1.22.2:17F5022i)
      printf '%s\n' "a24992c2e26049eb8b1aaf90e7dbf03fe24ed140e7b365b5c2880e3f0953baa9"
      ;;
    *)
      echo "No pinned ONNX Runtime iOS simulator library SHA-256 for version '$1' and Xcode build '${xcode_build}'." >&2
      return 1
      ;;
  esac
}

onnxruntime_ios_xcframework_sha256_for_version() {
  local xcode_build="${2:-${MAPLE_XCODE_BUILD_VERSION:-17F42}}"

  case "$1:${xcode_build}" in
    1.22.2:17F42)
      printf '%s\n' "718e3c6e70702b82e87bc3ea1b035b00ac8451130f5922d682eaf7885c648c12"
      ;;
    1.22.2:17F5022i)
      printf '%s\n' "718e3c6e70702b82e87bc3ea1b035b00ac8451130f5922d682eaf7885c648c12"
      ;;
    *)
      echo "No pinned ONNX Runtime iOS xcframework SHA-256 for version '$1' and Xcode build '${xcode_build}'." >&2
      return 1
      ;;
  esac
}
