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
