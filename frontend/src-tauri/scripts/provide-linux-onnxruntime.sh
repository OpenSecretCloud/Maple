#!/usr/bin/env bash
set -euo pipefail

ORT_VERSION="${ORT_VERSION:-1.22.0}"
TAURI_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
ORT_ROOT="${TAURI_DIR}/onnxruntime-linux"
ORT_DIR="${ORT_ROOT}/onnxruntime-linux-x64-${ORT_VERSION}"

if [ ! -f "${ORT_DIR}/lib/libonnxruntime.so" ]; then
  rm -rf "${ORT_ROOT}"
  mkdir -p "${ORT_ROOT}"
  curl -fL --retry 5 --retry-delay 2 --retry-all-errors \
    "https://github.com/microsoft/onnxruntime/releases/download/v${ORT_VERSION}/onnxruntime-linux-x64-${ORT_VERSION}.tgz" \
    | tar -xz -C "${ORT_ROOT}"
fi

echo "ORT_LIB_LOCATION=${ORT_DIR}"
echo "ORT_SKIP_DOWNLOAD=true"
echo "ORT_DYLIB_PATH=${ORT_DIR}/lib/libonnxruntime.so.1.22.0"
