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
ios_info_plist_present=0
ios_entitlements_present=0
restore_ios_build_state() {
  remove_generated_ios_cargo_config
  restore_desktop_goose_cargo_dependencies

  if [ -n "${ios_project_state_dir}" ] && [ -d "${ios_project_state_dir}" ]; then
    if [ -f "${ios_project_state_dir}/Info.plist" ]; then
      cp "${ios_project_state_dir}/Info.plist" "${TAURI_DIR}/gen/apple/maple_iOS/Info.plist"
    elif [ "${ios_info_plist_present}" = "0" ]; then
      rm -f "${TAURI_DIR}/gen/apple/maple_iOS/Info.plist"
    fi
    if [ -f "${ios_project_state_dir}/maple_iOS.entitlements" ]; then
      cp "${ios_project_state_dir}/maple_iOS.entitlements" "${TAURI_DIR}/gen/apple/maple_iOS/maple_iOS.entitlements"
    elif [ "${ios_entitlements_present}" = "0" ]; then
      rm -f "${TAURI_DIR}/gen/apple/maple_iOS/maple_iOS.entitlements"
    fi
    rm -rf "${ios_project_state_dir}"
  fi
}

cd "${TAURI_DIR}"
ios_project_state_dir="$(mktemp -d)"
trap restore_ios_build_state EXIT
if [ -f "${TAURI_DIR}/gen/apple/maple_iOS/Info.plist" ]; then
  ios_info_plist_present=1
  cp "${TAURI_DIR}/gen/apple/maple_iOS/Info.plist" "${ios_project_state_dir}/Info.plist"
fi
if [ -f "${TAURI_DIR}/gen/apple/maple_iOS/maple_iOS.entitlements" ]; then
  ios_entitlements_present=1
  cp "${TAURI_DIR}/gen/apple/maple_iOS/maple_iOS.entitlements" "${ios_project_state_dir}/maple_iOS.entitlements"
fi

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
  disable_desktop_goose_cargo_dependencies
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
  local tmp

  tmp="$(mktemp -d)"
  cp -a "${app}" "${tmp}/app"
  remove_apple_signing_metadata "${tmp}/app"
  python3 "${REPO_ROOT}/scripts/ci/canonical-ios-app-hash.py" --manifest "${tmp}/app" > "${out}"
  rm -rf "${tmp}"
}

write_ios_ipa_payload_canonical_file_manifest() {
  local ipa="$1"
  local out="$2"
  local tmp app

  tmp="$(mktemp -d)"
  unzip -qq "${ipa}" -d "${tmp}"
  app="$(find "${tmp}/Payload" -mindepth 1 -maxdepth 1 -type d -name '*.app' 2>/dev/null | LC_ALL=C sort | head -n 1)"
  if [ -z "${app}" ]; then
    rm -rf "${tmp}"
    echo "Could not find a Payload/*.app bundle in ${ipa}" >&2
    return 1
  fi

  remove_apple_signing_metadata "${app}"
  write_ios_canonical_app_file_manifest "${app}" "${out}"
  rm -rf "${tmp}"
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
  if [ ! -s "${out}" ]; then
    rm -f "${out}"
    printf 'verified-ios-canonical-file-manifest-no-diff  %s  %s\n' "$(basename "${unsigned_manifest}")" "$(basename "${signed_manifest}")"
  fi
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
signed_app_canonical_hash="$(print_canonical_ios_app_hash "${signed_app}" "$(repo_relative_path "${signed_app}")" | tee "${repro_dir}/ios-release-archive-app-canonical.sha256" | awk '{ print $2 }')"
cat "${repro_dir}/ios-release-archive-app-canonical.sha256"
write_ios_canonical_app_file_manifest "${signed_app}" "${repro_dir}/ios-release-archive-app-canonical-files.sha256"
write_ios_canonical_app_manifest_diff \
  "${repro_dir}/ios-release-unsigned-app-canonical-files.sha256" \
  "${repro_dir}/ios-release-archive-app-canonical-files.sha256" \
  "${repro_dir}/ios-release-archive-app-vs-unsigned-canonical.diff.txt"

if [ "${signed_app_canonical_hash}" != "${unsigned_app_canonical_hash}" ]; then
  echo "diagnostic-ios-archive-app-canonical-mismatch  archived iOS app does not strip back to the unsigned app tree." >&2
  echo "unsigned=${unsigned_app_canonical_hash}" >&2
  echo "archive_canonical=${signed_app_canonical_hash}" >&2
  echo "Canonical iOS archive app file manifest diff diagnostic:" >&2
  if [ -s "${repro_dir}/ios-release-archive-app-vs-unsigned-canonical.diff.txt" ]; then
    sed -n '1,80p' "${repro_dir}/ios-release-archive-app-vs-unsigned-canonical.diff.txt" >&2
  else
    echo "No canonical iOS archive app file manifest diff was produced." >&2
  fi
else
  printf 'verified-ios-archive-app  %s  %s\n' "${signed_app_canonical_hash}" "$(repo_relative_path "${signed_app}")"
fi

ios_artifacts=()
while IFS= read -r -d '' file; do
  ios_artifacts+=("${file}")
done < <(find "${TAURI_DIR}/gen/apple/build" -type f -name '*.ipa' -print0 | LC_ALL=C sort -z)

write_sha256_manifest "${repro_dir}/ios-release-final.sha256" "${ios_artifacts[@]}"
print_file_hashes "${ios_artifacts[@]}"

if [ "${#ios_artifacts[@]}" -ne 1 ]; then
  echo "Expected exactly one iOS IPA artifact, found ${#ios_artifacts[@]}." >&2
  exit 1
fi

: > "${repro_dir}/ios-release-canonical-payload.sha256"
for artifact in "${ios_artifacts[@]}"; do
  payload_file_manifest="${repro_dir}/ios-release-ipa-payload-canonical-files.sha256"
  payload_diff="${repro_dir}/ios-release-ipa-vs-unsigned-canonical.diff.txt"

  ipa_canonical_hash="$(print_canonical_ipa_payload_hash "${artifact}" "$(repo_relative_path "${artifact}")" | tee -a "${repro_dir}/ios-release-canonical-payload.sha256" | awk '{ print $2 }')"
  write_ios_ipa_payload_canonical_file_manifest "${artifact}" "${payload_file_manifest}"
  write_ios_canonical_app_manifest_diff \
    "${repro_dir}/ios-release-unsigned-app-canonical-files.sha256" \
    "${payload_file_manifest}" \
    "${payload_diff}"

  if [ "${ipa_canonical_hash}" != "${unsigned_app_canonical_hash}" ]; then
    echo "warning-ios-exported-payload-canonical-mismatch  exported iOS IPA payload does not strip back to the unsigned app build product." >&2
    echo "unsigned=${unsigned_app_canonical_hash}" >&2
    echo "ipa_payload=${ipa_canonical_hash}" >&2
    echo "Canonical iOS IPA payload file manifest diff diagnostic:" >&2
    if [ -s "${payload_diff}" ]; then
      sed -n '1,80p' "${payload_diff}" >&2
    else
      echo "No canonical iOS IPA payload file manifest diff was produced." >&2
    fi
    if [ "${MAPLE_ENFORCE_IOS_SIGNED_REPRODUCIBILITY:-0}" = "1" ]; then
      exit 1
    fi
    continue
  fi

  printf 'verified-ios-exported-payload  %s  %s\n' "${ipa_canonical_hash}" "$(repo_relative_path "${artifact}")"
done

verify_frontend_dist_unchanged
