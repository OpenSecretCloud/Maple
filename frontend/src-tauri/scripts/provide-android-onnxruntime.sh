#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
TAURI_DIR="$(cd "${SCRIPT_DIR}/.." && pwd)"

source "${SCRIPT_DIR}/onnxruntime-android-pins.sh"

ORT_ANDROID_VERSION="${ORT_ANDROID_VERSION:-$(onnxruntime_android_version)}"
ORT_ANDROID_ROOT="${TAURI_DIR}/onnxruntime-android"
ORT_ANDROID_AAR="${ORT_ANDROID_ROOT}/onnxruntime-android-${ORT_ANDROID_VERSION}.aar"
ORT_ANDROID_AAR_URL="$(onnxruntime_android_aar_url_for_version "${ORT_ANDROID_VERSION}")"
ORT_ANDROID_AAR_SHA256="$(onnxruntime_android_aar_sha256_for_version "${ORT_ANDROID_VERSION}")"
ORT_ANDROID_JNI_LIBS_DIR="${TAURI_DIR}/gen/android/app/src/main/jniLibs"
download_tmp=""
stage_tmp=""

cleanup() {
  if [ -n "${download_tmp}" ]; then
    rm -f "${download_tmp}"
  fi
  if [ -n "${stage_tmp}" ]; then
    rm -f "${stage_tmp}"
  fi
}
trap cleanup EXIT

sha256_digest() {
  local path="$1"

  if command -v sha256sum >/dev/null 2>&1; then
    sha256sum "${path}" | awk '{ print $1 }'
  elif command -v shasum >/dev/null 2>&1; then
    shasum -a 256 "${path}" | awk '{ print $1 }'
  elif command -v openssl >/dev/null 2>&1; then
    openssl dgst -sha256 -r "${path}" | awk '{ print $1 }'
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

  actual="$(sha256_digest "${path}")"
  if [ "${actual}" != "${expected}" ]; then
    echo "${label} SHA-256 mismatch for ${path}" >&2
    echo "expected: ${expected}" >&2
    echo "actual:   ${actual}" >&2
    return 1
  fi
}

mkdir -p "${ORT_ANDROID_ROOT}"

if [ ! -f "${ORT_ANDROID_AAR}" ]; then
  download_tmp="$(mktemp "${ORT_ANDROID_ROOT}/.onnxruntime-android-${ORT_ANDROID_VERSION}.aar.XXXXXX")"
  curl -fL --retry 5 --retry-delay 2 --retry-all-errors \
    "${ORT_ANDROID_AAR_URL}" \
    --output "${download_tmp}"
  verify_sha256 "ONNX Runtime Android AAR" "${download_tmp}" "${ORT_ANDROID_AAR_SHA256}"
  mv "${download_tmp}" "${ORT_ANDROID_AAR}"
  download_tmp=""
fi

verify_sha256 "ONNX Runtime Android AAR" "${ORT_ANDROID_AAR}" "${ORT_ANDROID_AAR_SHA256}"

while IFS= read -r abi; do
  source_entry="jni/${abi}/libonnxruntime.so"
  destination_dir="${ORT_ANDROID_JNI_LIBS_DIR}/${abi}"
  destination="${destination_dir}/libonnxruntime.so"
  expected="$(onnxruntime_android_lib_sha256_for_version "${ORT_ANDROID_VERSION}" "${abi}")"
  entry_count="$(unzip -Z1 "${ORT_ANDROID_AAR}" | grep -Fxc -- "${source_entry}" || true)"

  if [ "${entry_count}" != "1" ]; then
    echo "Expected exactly one ${source_entry} in ${ORT_ANDROID_AAR}; found ${entry_count}." >&2
    exit 1
  fi

  mkdir -p "${destination_dir}"
  if [ -f "${destination}" ] && [ "$(sha256_digest "${destination}")" = "${expected}" ]; then
    continue
  fi

  stage_tmp="$(mktemp "${destination}.XXXXXX")"
  if ! unzip -p "${ORT_ANDROID_AAR}" "${source_entry}" > "${stage_tmp}"; then
    rm -f "${stage_tmp}"
    exit 1
  fi
  verify_sha256 "ONNX Runtime Android ${abi} library" "${stage_tmp}" "${expected}"
  chmod 0755 "${stage_tmp}"
  mv "${stage_tmp}" "${destination}"
  stage_tmp=""
done < <(onnxruntime_android_abis)

while IFS= read -r abi; do
  library="${ORT_ANDROID_JNI_LIBS_DIR}/${abi}/libonnxruntime.so"
  expected="$(onnxruntime_android_lib_sha256_for_version "${ORT_ANDROID_VERSION}" "${abi}")"
  verify_sha256 "ONNX Runtime Android ${abi} library" "${library}" "${expected}"
done < <(onnxruntime_android_abis)

printf 'ORT_ANDROID_VERSION=%s\n' "${ORT_ANDROID_VERSION}"
printf 'ORT_ANDROID_AAR_PATH=%s\n' "${ORT_ANDROID_AAR}"
printf 'ORT_ANDROID_JNI_LIBS_DIR=%s\n' "${ORT_ANDROID_JNI_LIBS_DIR}"
