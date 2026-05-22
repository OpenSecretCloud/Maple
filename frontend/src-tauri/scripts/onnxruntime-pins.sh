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

onnxruntime_windows_arm64_archive_sha256_for_version() {
  case "$1" in
    1.22.0)
      printf '%s\n' "7008f7ff82f8e7de563a22f2b590e08e706a1289eba606b93de2b56edfb1e04b"
      ;;
    *)
      echo "No pinned Windows arm64 ONNX Runtime archive SHA-256 for version '$1'." >&2
      return 1
      ;;
  esac
}

onnxruntime_windows_arm64_dll_sha256_for_version() {
  case "$1" in
    1.22.0)
      printf '%s\n' "79281671a386ed1baab9dbdbb09fe55f99577011472e9526cf9d0b468bb6bcc7"
      ;;
    *)
      echo "No pinned Windows arm64 ONNX Runtime DLL SHA-256 for version '$1'." >&2
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
