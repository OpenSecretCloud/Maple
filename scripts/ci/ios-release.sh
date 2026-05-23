#!/usr/bin/env bash
set -euo pipefail

source "$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)/_common.sh"

print_source_provenance

if [ "$(host_os)" != "darwin" ]; then
  echo "iOS builds must run on macOS." >&2
  exit 1
fi

command -v xcodebuild >/dev/null 2>&1 || {
  echo "xcodebuild is required for iOS builds." >&2
  exit 1
}

use_xcode_toolchain
install_frontend_deps
use_release_environment
configure_reproducible_build_metadata
build_frontend_dist

if [ -n "${APPLE_API_PRIVATE_KEY:-}" ] && [ -n "${APPLE_API_KEY:-}" ] && [ -z "${APPLE_API_KEY_PATH:-}" ]; then
  mkdir -p "${HOME}/.private_keys"
  APPLE_API_KEY_PATH="${HOME}/.private_keys/AuthKey_${APPLE_API_KEY}.p8"
  decode_base64_string_to_file "${APPLE_API_PRIVATE_KEY}" "${APPLE_API_KEY_PATH}"
  chmod 600 "${APPLE_API_KEY_PATH}"
  export APPLE_API_KEY_PATH
fi

if [ -z "${APPLE_API_ISSUER:-}" ] || [ -z "${APPLE_API_KEY:-}" ] || [ -z "${APPLE_API_KEY_PATH:-}" ] || [ -z "${APPLE_DEVELOPMENT_TEAM:-}" ]; then
  echo "iOS release signing variables are required: APPLE_API_ISSUER, APPLE_API_KEY, APPLE_API_KEY_PATH or APPLE_API_PRIVATE_KEY, APPLE_DEVELOPMENT_TEAM." >&2
  exit 1
fi

ios_project_state_dir=""
restore_ios_build_state() {
  remove_generated_ios_cargo_config

  if [ -n "${ios_project_state_dir}" ] && [ -d "${ios_project_state_dir}" ]; then
    if [ -f "${ios_project_state_dir}/Info.plist" ]; then
      cp "${ios_project_state_dir}/Info.plist" "${TAURI_DIR}/gen/apple/maple_iOS/Info.plist"
    fi
    if [ -f "${ios_project_state_dir}/maple_iOS.entitlements" ]; then
      cp "${ios_project_state_dir}/maple_iOS.entitlements" "${TAURI_DIR}/gen/apple/maple_iOS/maple_iOS.entitlements"
    fi
    rm -rf "${ios_project_state_dir}"
  fi
}

cd "${TAURI_DIR}"
ios_project_state_dir="$(mktemp -d)"
cp "${TAURI_DIR}/gen/apple/maple_iOS/Info.plist" "${ios_project_state_dir}/Info.plist"
cp "${TAURI_DIR}/gen/apple/maple_iOS/maple_iOS.entitlements" "${ios_project_state_dir}/maple_iOS.entitlements"
trap restore_ios_build_state EXIT

repro_dir="${TAURI_DIR}/target/reproducibility"
mkdir -p "${repro_dir}"

verify_ios_onnxruntime_manifest
print_ios_onnxruntime_hashes
write_ios_onnxruntime_reproducibility_manifest "${repro_dir}/ios-onnxruntime-final.sha256"
./scripts/setup-ios-cargo-config.sh

export ORT_LIB_LOCATION="${TAURI_DIR}/onnxruntime-ios/onnxruntime.xcframework/ios-arm64"
export ORT_SKIP_DOWNLOAD="true"
export IPHONEOS_DEPLOYMENT_TARGET="${IPHONEOS_DEPLOYMENT_TARGET:-16.0}"

ios_build_config='{"build":{"beforeBuildCommand":null}}'

remove_ios_release_outputs() {
  local artifact

  remove_build_tree "${TAURI_DIR}/gen/apple/build/arm64"
  remove_build_tree "${TAURI_DIR}/gen/apple/build/maple_iOS.xcarchive"
  remove_build_tree "${TAURI_DIR}/gen/apple/build/Payload"

  if [ -d "${TAURI_DIR}/gen/apple/build" ]; then
    while IFS= read -r -d '' artifact; do
      remove_build_tree "${artifact}"
    done < <(find "${TAURI_DIR}/gen/apple/build" -maxdepth 1 -type f -name '*.ipa' -print0 | LC_ALL=C sort -z)
  fi
}

build_ios_release() {
  cd "${FRONTEND_DIR}"
  bun tauri ios build --target aarch64 --ci --config "${ios_build_config}" "$@"
}

find_ios_release_app() {
  local app root

  for root in \
    "${TAURI_DIR}/gen/apple/build/maple_iOS.xcarchive/Products/Applications" \
    "${TAURI_DIR}/gen/apple/build/arm64" \
    "${TAURI_DIR}/gen/apple/build/Payload"; do
    if [ ! -d "${root}" ]; then
      continue
    fi

    app="$(find "${root}" -mindepth 1 -maxdepth 1 -type d -name '*.app' 2>/dev/null | LC_ALL=C sort | head -n 1)"
    if [ -n "${app}" ]; then
      printf '%s\n' "${app}"
      return 0
    fi
  done
}

write_ios_canonical_app_file_manifest() {
  local app="$1"
  local out="$2"

  python3 "${REPO_ROOT}/scripts/ci/canonical-ios-app-hash.py" --manifest "${app}" > "${out}"
}

write_ios_canonical_app_manifest_diff() {
  local unsigned_manifest="$1"
  local signed_manifest="$2"
  local out="$3"
  local unsigned_by_path signed_by_path

  unsigned_by_path="$(mktemp)"
  signed_by_path="$(mktemp)"
  sed -E 's/^([0-9a-f]{64})  (.*)$/\2  \1/' "${unsigned_manifest}" | LC_ALL=C sort > "${unsigned_by_path}"
  sed -E 's/^([0-9a-f]{64})  (.*)$/\2  \1/' "${signed_manifest}" | LC_ALL=C sort > "${signed_by_path}"
  comm -3 "${unsigned_by_path}" "${signed_by_path}" > "${out}"
  rm -f "${unsigned_by_path}" "${signed_by_path}"
}

remove_ios_release_outputs
build_ios_release --no-sign --archive-only

unsigned_app="$(find_ios_release_app)"
if [ -z "${unsigned_app}" ]; then
  echo "Could not find unsigned iOS app build product under ${TAURI_DIR}/gen/apple/build." >&2
  exit 1
fi
unsigned_app_canonical_hash="$(print_canonical_ios_app_hash "${unsigned_app}" "$(repo_relative_path "${unsigned_app}")" | tee "${repro_dir}/ios-release-unsigned-app-canonical.sha256" | awk '{ print $2 }')"
cat "${repro_dir}/ios-release-unsigned-app-canonical.sha256"
write_ios_canonical_app_file_manifest "${unsigned_app}" "${repro_dir}/ios-release-unsigned-app-canonical-files.sha256"

remove_ios_release_outputs
build_ios_release --export-method app-store-connect

signed_app="$(find_ios_release_app)"
if [ -z "${signed_app}" ]; then
  echo "Could not find signed iOS app build product under ${TAURI_DIR}/gen/apple/build." >&2
  exit 1
fi
signed_app_canonical_hash="$(print_canonical_ios_app_hash "${signed_app}" "$(repo_relative_path "${signed_app}")" | tee "${repro_dir}/ios-release-signed-app-canonical.sha256" | awk '{ print $2 }')"
cat "${repro_dir}/ios-release-signed-app-canonical.sha256"
write_ios_canonical_app_file_manifest "${signed_app}" "${repro_dir}/ios-release-signed-app-canonical-files.sha256"
write_ios_canonical_app_manifest_diff \
  "${repro_dir}/ios-release-unsigned-app-canonical-files.sha256" \
  "${repro_dir}/ios-release-signed-app-canonical-files.sha256" \
  "${repro_dir}/ios-release-signed-vs-unsigned-canonical.diff.txt"

if [ "${signed_app_canonical_hash}" != "${unsigned_app_canonical_hash}" ]; then
  echo "warning-ios-signed-app-canonical-mismatch  signed iOS app does not strip back to the unsigned app tree." >&2
  echo "unsigned=${unsigned_app_canonical_hash}" >&2
  echo "signed_canonical=${signed_app_canonical_hash}" >&2
  if [ -s "${repro_dir}/ios-release-signed-vs-unsigned-canonical.diff.txt" ]; then
    echo "First canonical iOS file manifest differences:" >&2
    sed -n '1,80p' "${repro_dir}/ios-release-signed-vs-unsigned-canonical.diff.txt" >&2
  fi
  if [ "${MAPLE_ENFORCE_IOS_SIGNED_REPRODUCIBILITY:-0}" = "1" ]; then
    exit 1
  fi
else
  printf 'verified-ios-signed-app  %s  %s\n' "${signed_app_canonical_hash}" "$(repo_relative_path "${signed_app}")"
fi

ios_artifacts=()
while IFS= read -r -d '' file; do
  ios_artifacts+=("${file}")
done < <(find "${TAURI_DIR}/gen/apple/build" -type f -name '*.ipa' -print0 | LC_ALL=C sort -z)

write_sha256_manifest "${repro_dir}/ios-release-final.sha256" "${ios_artifacts[@]}"
print_file_hashes "${ios_artifacts[@]}"

: > "${repro_dir}/ios-release-canonical-payload.sha256"
for artifact in "${ios_artifacts[@]}"; do
  ipa_canonical_hash="$(print_canonical_ipa_payload_hash "${artifact}" "$(repo_relative_path "${artifact}")" | tee -a "${repro_dir}/ios-release-canonical-payload.sha256" | awk '{ print $2 }')"

  if [ -n "${signed_app_canonical_hash}" ]; then
    if [ "${ipa_canonical_hash}" != "${signed_app_canonical_hash}" ]; then
      echo "warning-ios-exported-payload-canonical-mismatch  exported iOS IPA payload does not strip back to the signed app build product." >&2
      echo "signed_app=${signed_app_canonical_hash}" >&2
      echo "ipa_payload=${ipa_canonical_hash}" >&2
      if [ "${MAPLE_ENFORCE_IOS_SIGNED_REPRODUCIBILITY:-0}" = "1" ]; then
        exit 1
      fi
      continue
    fi

    printf 'verified-ios-exported-payload  %s  %s\n' "${ipa_canonical_hash}" "$(repo_relative_path "${artifact}")"
  fi
done

verify_frontend_dist_unchanged
