#!/usr/bin/env bash
set -euo pipefail

ORT_VERSION="${ORT_VERSION:-1.23.2}"
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
source "${SCRIPT_DIR}/onnxruntime-pins.sh"

if [ "$(uname -s)" != "Darwin" ]; then
  echo "macOS ONNX Runtime provisioning must run on macOS." >&2
  exit 1
fi

TAURI_DIR="$(cd "${SCRIPT_DIR}/.." && pwd)"
ORT_ROOT="${TAURI_DIR}/onnxruntime-macos"
ORT_DIR="${ORT_ROOT}/onnxruntime-osx-universal2-${ORT_VERSION}"
ORT_ARCHIVE="onnxruntime-osx-universal2-${ORT_VERSION}.tgz"
ORT_URL="https://github.com/microsoft/onnxruntime/releases/download/v${ORT_VERSION}/${ORT_ARCHIVE}"
ORT_DYLIB="${ORT_DIR}/lib/libonnxruntime.${ORT_VERSION}.dylib"
ORT_ARCHIVE_SHA256="$(onnxruntime_macos_universal2_archive_sha256_for_version "${ORT_VERSION}")"
ORT_DYLIB_SHA256="$(onnxruntime_macos_universal2_dylib_sha256_for_version "${ORT_VERSION}")"

sha256_file() {
  shasum -a 256 "$1" | awk '{print $1}'
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

architectures="$(lipo -archs "${ORT_DYLIB}")"
for required_architecture in arm64 x86_64; do
  case " ${architectures} " in
    *" ${required_architecture} "*) ;;
    *)
      echo "Expected a universal arm64+x86_64 ONNX Runtime dylib, got: ${architectures}" >&2
      exit 1
      ;;
  esac
done

echo "ORT_LIB_LOCATION=${ORT_DIR}"
echo "ORT_SKIP_DOWNLOAD=true"
echo "ORT_DYLIB_PATH=${ORT_DYLIB}"
