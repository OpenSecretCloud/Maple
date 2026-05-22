#!/usr/bin/env bash
set -euo pipefail

source "$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)/_common.sh"

print_source_provenance

if [ "$(host_os)" != "darwin" ]; then
  echo "iOS ONNX Runtime builds must run on macOS." >&2
  exit 1
fi

command -v xcodebuild >/dev/null 2>&1 || {
  echo "xcodebuild is required for iOS ONNX Runtime builds." >&2
  exit 1
}

use_xcode_toolchain
configure_reproducible_build_metadata

onnxruntime_version="$(ios_onnxruntime_version)"

if [ -f "$(ios_onnxruntime_manifest_file)" ]; then
  echo "Using cached iOS ONNX Runtime artifact."
  if verify_ios_onnxruntime_manifest; then
    print_ios_onnxruntime_hashes
    repro_dir="${TAURI_DIR}/target/reproducibility"
    mkdir -p "${repro_dir}"
    write_ios_onnxruntime_reproducibility_manifest "${repro_dir}/ios-onnxruntime-final.sha256"
    exit 0
  fi

  echo "Cached iOS ONNX Runtime artifact does not match pinned ${onnxruntime_version} hashes; rebuilding." >&2
  rm -rf "${TAURI_DIR}/onnxruntime-ios"
fi

if [ -d "$(ios_onnxruntime_xcframework_dir)" ]; then
  echo "Found iOS ONNX Runtime artifact without a manifest; rebuilding for hash verification."
  rm -rf "${TAURI_DIR}/onnxruntime-ios"
fi

cd "${TAURI_DIR}"
./scripts/build-ios-onnxruntime-all.sh "${onnxruntime_version}"
write_ios_onnxruntime_manifest
verify_ios_onnxruntime_manifest
print_ios_onnxruntime_hashes

repro_dir="${TAURI_DIR}/target/reproducibility"
mkdir -p "${repro_dir}"
write_ios_onnxruntime_reproducibility_manifest "${repro_dir}/ios-onnxruntime-final.sha256"
