#!/usr/bin/env bash
set -euo pipefail

ORT_VERSION="${ORT_VERSION:-1.22.0}"
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
source "${SCRIPT_DIR}/onnxruntime-pins.sh"

TAURI_DIR="$(cd "${SCRIPT_DIR}/.." && pwd)"
ORT_ROOT="${TAURI_DIR}/onnxruntime-linux"
ORT_DIR="${ORT_ROOT}/onnxruntime-linux-x64-${ORT_VERSION}"
ORT_ARCHIVE="onnxruntime-linux-x64-${ORT_VERSION}.tgz"
ORT_URL="https://github.com/microsoft/onnxruntime/releases/download/v${ORT_VERSION}/${ORT_ARCHIVE}"
ORT_DYLIB="${ORT_DIR}/lib/libonnxruntime.so.${ORT_VERSION}"
ORT_ARCHIVE_SHA256="$(onnxruntime_linux_x64_archive_sha256_for_version "${ORT_VERSION}")"
ORT_DYLIB_SHA256="$(onnxruntime_linux_x64_dylib_sha256_for_version "${ORT_VERSION}")"

sha256_file() {
  local path="$1"

  if command -v sha256sum >/dev/null 2>&1; then
    sha256sum "${path}" | awk '{print $1}'
  elif command -v shasum >/dev/null 2>&1; then
    shasum -a 256 "${path}" | awk '{print $1}'
  elif command -v openssl >/dev/null 2>&1; then
    openssl dgst -sha256 -r "${path}" | awk '{print $1}'
  else
    echo "No SHA-256 tool found. Install sha256sum, shasum, or openssl." >&2
    return 1
  fi
}

verify_sha256() {
  local label="$1"
  local path="$2"
  local expected="$3"
  local actual

  actual="$(sha256_file "${path}")"
  if [ "${actual}" != "${expected}" ]; then
    echo "${label} SHA-256 mismatch for ${path}" >&2
    echo "expected: ${expected}" >&2
    echo "actual:   ${actual}" >&2
    return 1
  fi
}

if [ ! -f "${ORT_DYLIB}" ]; then
  rm -rf "${ORT_ROOT}"
  mkdir -p "${ORT_ROOT}"
  archive_path="${ORT_ROOT}/${ORT_ARCHIVE}"

  curl -fL --retry 5 --retry-delay 2 --retry-all-errors \
    "${ORT_URL}" \
    --output "${archive_path}"

  verify_sha256 "ONNX Runtime archive" "${archive_path}" "${ORT_ARCHIVE_SHA256}"
  tar -xzf "${archive_path}" -C "${ORT_ROOT}"
  rm -f "${archive_path}"
fi

verify_sha256 "ONNX Runtime shared library" "${ORT_DYLIB}" "${ORT_DYLIB_SHA256}"

echo "ORT_LIB_LOCATION=${ORT_DIR}"
echo "ORT_SKIP_DOWNLOAD=true"
echo "ORT_DYLIB_PATH=${ORT_DYLIB}"
