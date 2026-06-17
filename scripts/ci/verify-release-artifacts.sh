#!/usr/bin/env bash
set -euo pipefail

source "$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)/_common.sh"

usage() {
  cat >&2 <<'EOF'
usage: verify-release-artifacts.sh <artifacts-dir> [all|present|linux|macos|windows|android|ios|web|latest-json ...]

Verifies downloaded release artifacts against their reproducibility proof files.
The verifier recomputes final file hashes, canonical signed payload hashes, and
Tauri updater signatures where the current host has the required platform tools.

The all target requires every release proof class. The present target verifies
only the proof classes found in the artifact directory, which is useful for
partial PR artifact bundles.

Set MAPLE_VERIFY_ALLOW_PLATFORM_SKIPS=1 to skip host-specific container checks,
such as DMG or IPA canonicalization on non-macOS hosts and Linux installer
metadata checks on non-Linux hosts.
EOF
}

artifacts_dir="${1:-}"
if [ -z "${artifacts_dir}" ] || [ "${artifacts_dir}" = "-h" ] || [ "${artifacts_dir}" = "--help" ]; then
  usage
  exit 2
fi
shift || true

if [ ! -d "${artifacts_dir}" ]; then
  echo "Artifacts directory does not exist: ${artifacts_dir}" >&2
  exit 1
fi

if [ "$#" -eq 0 ]; then
  set -- all
fi

allow_platform_skips="${MAPLE_VERIFY_ALLOW_PLATFORM_SKIPS:-0}"

proof_file_optional() {
  local name="$1"
  find "${artifacts_dir}" -type f -name "${name}" | LC_ALL=C sort | head -n 1
}

proof_file_required() {
  local name="$1"
  local file

  file="$(proof_file_optional "${name}")"
  if [ -z "${file}" ]; then
    echo "Missing release proof file: ${name}" >&2
    return 1
  fi

  printf '%s\n' "${file}"
}

manifest_label_path() {
  local label="$1"
  label="${label%%::*}"
  printf '%s\n' "${label#./}"
}

artifact_for_label() {
  local label="$1"
  local path base found

  path="$(manifest_label_path "${label}")"

  if [ -f "${artifacts_dir}/${path}" ]; then
    printf '%s\n' "${artifacts_dir}/${path}"
    return 0
  fi

  found="$(find "${artifacts_dir}" -type f -path "*/${path}" | LC_ALL=C sort | head -n 1)"
  if [ -n "${found}" ]; then
    printf '%s\n' "${found}"
    return 0
  fi

  base="$(basename "${path}")"
  found="$(find "${artifacts_dir}" -type f -name "${base}" | LC_ALL=C sort | head -n 1)"
  if [ -z "${found}" ]; then
    echo "Missing artifact for proof label: ${label}" >&2
    return 1
  fi

  printf '%s\n' "${found}"
}

manifest_single_digest() {
  local manifest="$1"
  awk 'NF >= 3 && $1 ~ /^sha256-/ { print $2; exit }' "${manifest}"
}

manifest_digests() {
  local manifest="$1"
  awk 'NF >= 3 && $1 ~ /^sha256-/ { print $2 }' "${manifest}"
}

verify_file_manifest() {
  local manifest="$1"
  local digest label file actual

  while read -r digest label _; do
    [ -n "${digest:-}" ] || continue

    if ! [[ "${digest}" =~ ^[0-9a-fA-F]{64}$ ]]; then
      echo "Invalid file manifest line in ${manifest}: ${digest} ${label:-}" >&2
      return 1
    fi

    file="$(artifact_for_label "${label}")"
    actual="$(sha256_file "${file}" | awk '{ print $1 }')"
    if [ "${actual}" != "${digest}" ]; then
      echo "File hash mismatch for ${label}." >&2
      echo "expected=${digest}" >&2
      echo "actual=${actual}" >&2
      return 1
    fi

    printf 'verified-file-hash  %s  %s\n' "${actual}" "${label}"
  done < "${manifest}"
}

verify_proof_file_hash() {
  local manifest="$1"
  local digest label file actual

  while read -r digest label _; do
    [ -n "${digest:-}" ] || continue

    file="$(artifact_for_label "${label}")"
    actual="$(sha256_file "${file}" | awk '{ print $1 }')"
    if [ "${actual}" != "${digest}" ]; then
      echo "Proof file hash mismatch for ${label}." >&2
      echo "expected=${digest}" >&2
      echo "actual=${actual}" >&2
      return 1
    fi

    printf 'verified-proof-file-hash  %s  %s\n' "${actual}" "${label}"
  done < "${manifest}"
}

verify_zip_payload_manifest() {
  local manifest="$1"
  local kind digest label file actual

  while read -r kind digest label _; do
    [ -n "${kind:-}" ] || continue

    if [ "${kind}" != "sha256-zip-payload" ]; then
      echo "Invalid ZIP payload manifest line in ${manifest}: ${kind} ${digest:-} ${label:-}" >&2
      return 1
    fi

    file="$(artifact_for_label "${label}")"
    actual="$(zip_payload_hash_digest "${file}")"
    if [ "${actual}" != "${digest}" ]; then
      echo "ZIP payload hash mismatch for ${label}." >&2
      echo "expected=${digest}" >&2
      echo "actual=${actual}" >&2
      return 1
    fi

    printf 'verified-zip-payload  %s  %s\n' "${actual}" "${label}"
  done < "${manifest}"
}

android_payload_key() {
  local label="$1"
  local base

  base="$(basename "$(manifest_label_path "${label}")")"
  case "${base}" in
    *-unsigned.apk)
      base="${base%-unsigned.apk}.apk"
      ;;
  esac

  printf '%s\n' "${base}"
}

verify_android_payload_equivalence() {
  local unsigned_manifest="$1"
  local signed_manifest="$2"
  local kind digest label key
  declare -A unsigned_by_key=()

  while read -r kind digest label _; do
    [ -n "${kind:-}" ] || continue
    key="$(android_payload_key "${label}")"
    unsigned_by_key["${key}"]="${digest}"
  done < "${unsigned_manifest}"

  while read -r kind digest label _; do
    [ -n "${kind:-}" ] || continue
    key="$(android_payload_key "${label}")"
    if [ -z "${unsigned_by_key[${key}]:-}" ]; then
      echo "Missing unsigned Android payload proof for ${key}." >&2
      return 1
    fi

    if [ "${digest}" != "${unsigned_by_key[${key}]}" ]; then
      echo "Signed Android payload does not match unsigned payload for ${key}." >&2
      echo "unsigned=${unsigned_by_key[${key}]}" >&2
      echo "signed=${digest}" >&2
      return 1
    fi

    printf 'verified-android-payload-equivalence  %s  %s\n' "${digest}" "${key}"
  done < "${signed_manifest}"
}

verify_android_signatures_optional() {
  local artifact require_signatures

  require_signatures="${MAPLE_REQUIRE_ANDROID_SIGNATURE_VERIFICATION:-0}"

  while IFS= read -r -d '' artifact; do
    case "${artifact}" in
      *.apk)
        if command -v apksigner >/dev/null 2>&1; then
          apksigner verify --verbose "${artifact}" >/dev/null
          printf 'verified-android-apk-signature  %s\n' "$(basename "${artifact}")"
        elif [ "${require_signatures}" = "1" ]; then
          echo "apksigner is required to verify Android APK signatures." >&2
          return 1
        else
          printf 'skipped-android-apk-signature  missing-apksigner  %s\n' "$(basename "${artifact}")"
        fi
        ;;
      *.aab)
        if command -v jarsigner >/dev/null 2>&1; then
          jarsigner -verify "${artifact}" >/dev/null
          printf 'verified-android-aab-signature  %s\n' "$(basename "${artifact}")"
        elif [ "${require_signatures}" = "1" ]; then
          echo "jarsigner is required to verify Android AAB signatures." >&2
          return 1
        else
          printf 'skipped-android-aab-signature  missing-jarsigner  %s\n' "$(basename "${artifact}")"
        fi
        ;;
    esac
  done < <(find "${artifacts_dir}" -type f \( -name '*.apk' -o -name '*.aab' \) -print0 | LC_ALL=C sort -z)
}

verify_android() {
  local final_manifest unsigned_manifest signed_manifest

  final_manifest="$(proof_file_required android-release-final.sha256)"
  unsigned_manifest="$(proof_file_required android-release-unsigned-canonical-payload.sha256)"
  signed_manifest="$(proof_file_required android-release-canonical-payload.sha256)"

  verify_file_manifest "${final_manifest}"
  verify_zip_payload_manifest "${signed_manifest}"
  verify_android_payload_equivalence "${unsigned_manifest}" "${signed_manifest}"
  verify_android_signatures_optional
}

verify_tauri_signatures_in_artifacts() {
  local fake_pub signature artifact

  fake_pub="$(proof_file_optional desktop-pr-linux-fake-updater.pub)"
  if [ -n "${fake_pub}" ]; then
    export MAPLE_TAURI_UPDATER_PUBLIC_KEY_PATH="${fake_pub}"
  fi

  while IFS= read -r -d '' signature; do
    artifact="${signature%.sig}"
    verify_tauri_updater_signature "${artifact}" "${signature}" "$(basename "${artifact}")"
  done < <(find "${artifacts_dir}" -type f -name '*.sig' -print0 | LC_ALL=C sort -z)
}

verify_linux_installer_metadata() {
  local appimage deb
  local -a appimages=()
  local -a debs=()

  if [ "$(host_os)" != "linux" ]; then
    if [ "${allow_platform_skips}" = "1" ]; then
      printf 'skipped-linux-installer-metadata  requires-linux\n'
      return 0
    fi
    echo "Linux installer metadata verification requires Linux." >&2
    return 1
  fi

  while IFS= read -r -d '' appimage; do
    appimages+=("${appimage}")
  done < <(find "${artifacts_dir}" -type f -name 'Maple_*.AppImage' -print0 | LC_ALL=C sort -z)

  while IFS= read -r -d '' deb; do
    debs+=("${deb}")
  done < <(find "${artifacts_dir}" -type f -name '*.deb' -print0 | LC_ALL=C sort -z)

  if [ "${#appimages[@]}" -eq 0 ]; then
    echo "No Linux Maple AppImage artifact found for installer metadata verification." >&2
    return 1
  fi

  if [ "${#debs[@]}" -eq 0 ]; then
    echo "No Linux .deb artifact found for installer metadata verification." >&2
    return 1
  fi

  for appimage in "${appimages[@]}"; do
    verify_linux_appimage_executable_metadata "${appimage}"
  done

  for deb in "${debs[@]}"; do
    verify_linux_deb_package_executable_metadata "${deb}"
  done
}

verify_linux_manifest() {
  local final_manifest="$1"
  local fake_pub_manifest

  fake_pub_manifest="$(proof_file_optional desktop-pr-linux-fake-updater-public-key.sha256)"
  if [ -n "${fake_pub_manifest}" ]; then
    verify_proof_file_hash "${fake_pub_manifest}"
  fi

  verify_file_manifest "${final_manifest}"
  verify_linux_installer_metadata
  verify_tauri_signatures_in_artifacts
}

verify_linux() {
  local final_manifest

  final_manifest="$(proof_file_required desktop-release-linux-final.sha256)"
  verify_linux_manifest "${final_manifest}"
}

verify_linux_present() {
  local final_manifest

  final_manifest="$(proof_file_optional desktop-release-linux-final.sha256)"
  if [ -z "${final_manifest}" ]; then
    final_manifest="$(proof_file_required desktop-pr-linux-fake-signing-final.sha256)"
  fi

  verify_linux_manifest "${final_manifest}"
}

verify_windows() {
  local final_manifest runtime_manifest

  final_manifest="$(proof_file_optional desktop-release-windows-final.sha256)"
  runtime_manifest="$(proof_file_optional desktop-release-windows-runtime-dlls.sha256)"
  if [ -z "${final_manifest}" ]; then
    final_manifest="$(proof_file_required desktop-pr-windows-final.sha256)"
    runtime_manifest="$(proof_file_required desktop-pr-windows-runtime-dlls.sha256)"
  fi

  verify_file_manifest "${final_manifest}"
  if [ -n "${runtime_manifest}" ]; then
    verify_file_manifest "${runtime_manifest}"
  fi
  verify_tauri_signatures_in_artifacts
}

verify_canonical_apple_manifest() {
  local manifest="$1"
  local expected_digest="${2:-}"
  local kind digest label artifact actual

  while read -r kind digest label _; do
    [ -n "${kind:-}" ] || continue
    artifact="$(artifact_for_label "${label}")"

    case "${kind}" in
      sha256-macos-app-tar-canonical)
        actual="$(print_canonical_app_tar_payload_hash "${artifact}" "${label}" | awk '{ print $2 }')"
        ;;
      sha256-macos-dmg-canonical)
        if [ "$(host_os)" != "darwin" ]; then
          if [ "${allow_platform_skips}" = "1" ]; then
            printf 'skipped-macos-dmg-canonical  requires-darwin  %s\n' "${label}"
            continue
          fi
          echo "DMG canonical verification requires macOS: ${label}" >&2
          return 1
        fi
        actual="$(print_canonical_dmg_app_hash "${artifact}" "${label}" | awk '{ print $2 }')"
        ;;
      sha256-ios-unsigned-app-tree)
        if [ "$(host_os)" != "darwin" ]; then
          if [ "${allow_platform_skips}" = "1" ]; then
            printf 'skipped-ios-ipa-canonical  requires-darwin  %s\n' "${label}"
            continue
          fi
          echo "iOS IPA canonical verification requires macOS: ${label}" >&2
          return 1
        fi
        actual="$(print_canonical_ipa_payload_hash "${artifact}" "${label}" | awk '{ print $2 }')"
        ;;
      *)
        echo "Unsupported Apple canonical proof kind in ${manifest}: ${kind}" >&2
        return 1
        ;;
    esac

    if [ "${actual}" != "${digest}" ]; then
      echo "Apple canonical payload mismatch for ${label}." >&2
      echo "expected=${digest}" >&2
      echo "actual=${actual}" >&2
      return 1
    fi

    if [ -n "${expected_digest}" ] && [ "${actual}" != "${expected_digest}" ]; then
      echo "Apple canonical payload does not match unsigned baseline for ${label}." >&2
      echo "unsigned=${expected_digest}" >&2
      echo "actual=${actual}" >&2
      return 1
    fi

    printf 'verified-apple-canonical-payload  %s  %s\n' "${actual}" "${label}"
  done < "${manifest}"
}

verify_macos_app_signature() {
  local app="$1"
  local label="$2"

  codesign --verify --deep --strict --verbose=2 "${app}" >/dev/null
  spctl -a -vvv -t exec "${app}" >/dev/null
  printf 'verified-macos-app-signature  %s\n' "${label}"
}

verify_macos_signatures() {
  local artifact tmp mount app

  if [ "$(host_os)" != "darwin" ]; then
    if [ "${allow_platform_skips}" = "1" ]; then
      printf 'skipped-macos-signature-verification  requires-darwin\n'
      return 0
    fi
    echo "macOS signature verification requires macOS." >&2
    return 1
  fi

  while IFS= read -r -d '' artifact; do
    tmp="$(mktemp -d)"
    mount="${tmp}/mnt"
    mkdir -p "${mount}"

    if ! hdiutil attach -nobrowse -readonly -mountpoint "${mount}" "${artifact}" >/dev/null; then
      rm -rf "${tmp}"
      echo "Failed to mount DMG for signature verification: ${artifact}" >&2
      return 1
    fi

    app="$(find_canonical_app_bundle_under_dir "${mount}")"
    if [ -z "${app}" ]; then
      hdiutil detach "${mount}" -quiet >/dev/null 2>&1 || true
      rm -rf "${tmp}"
      echo "Could not find a *.app bundle in ${artifact}" >&2
      return 1
    fi

    if ! verify_macos_app_signature "${app}" "$(basename "${artifact}")::*.app"; then
      hdiutil detach "${mount}" -quiet >/dev/null 2>&1 || true
      rm -rf "${tmp}"
      return 1
    fi

    hdiutil detach "${mount}" -quiet >/dev/null
    rm -rf "${tmp}"
  done < <(find "${artifacts_dir}" -type f -name '*.dmg' -print0 | LC_ALL=C sort -z)

  while IFS= read -r -d '' artifact; do
    tmp="$(mktemp -d)"
    tar -xzf "${artifact}" -C "${tmp}"
    app="$(find_canonical_app_bundle_under_dir "${tmp}")"
    if [ -z "${app}" ]; then
      rm -rf "${tmp}"
      echo "Could not find a *.app bundle in ${artifact}" >&2
      return 1
    fi

    verify_macos_app_signature "${app}" "$(basename "${artifact}")::*.app"
    rm -rf "${tmp}"
  done < <(find "${artifacts_dir}" -type f -name '*.app.tar.gz' -print0 | LC_ALL=C sort -z)
}

verify_ios_signatures() {
  local artifact tmp app

  if [ "$(host_os)" != "darwin" ]; then
    if [ "${allow_platform_skips}" = "1" ]; then
      printf 'skipped-ios-signature-verification  requires-darwin\n'
      return 0
    fi
    echo "iOS signature verification requires macOS." >&2
    return 1
  fi

  while IFS= read -r -d '' artifact; do
    tmp="$(mktemp -d)"
    unzip -qq "${artifact}" -d "${tmp}"
    app="$(find_canonical_app_bundle_under_dir "${tmp}/Payload")"
    if [ -z "${app}" ]; then
      rm -rf "${tmp}"
      echo "Could not find a Payload/*.app bundle in ${artifact}" >&2
      return 1
    fi

    codesign --verify --deep --strict --verbose=2 "${app}" >/dev/null
    printf 'verified-ios-app-signature  %s::Payload/*.app\n' "$(basename "${artifact}")"
    rm -rf "${tmp}"
  done < <(find "${artifacts_dir}" -type f -name '*.ipa' -print0 | LC_ALL=C sort -z)
}

verify_macos() {
  local final_manifest unsigned_manifest signed_manifest container_manifest
  local unsigned_digest signed_digest

  final_manifest="$(proof_file_required desktop-release-macos-final.sha256)"
  unsigned_manifest="$(proof_file_required desktop-release-macos-unsigned.sha256)"
  signed_manifest="$(proof_file_required desktop-release-macos-signed-canonical.sha256)"
  container_manifest="$(proof_file_required desktop-release-macos-container-canonical.sha256)"

  verify_file_manifest "${final_manifest}"

  unsigned_digest="$(manifest_single_digest "${unsigned_manifest}")"
  signed_digest="$(manifest_single_digest "${signed_manifest}")"
  if [ -z "${unsigned_digest}" ] || [ "${unsigned_digest}" != "${signed_digest}" ]; then
    echo "macOS signed app canonical proof does not match unsigned proof." >&2
    echo "unsigned=${unsigned_digest:-missing}" >&2
    echo "signed=${signed_digest:-missing}" >&2
    return 1
  fi
  printf 'verified-macos-signed-app-proof  %s\n' "${signed_digest}"

  verify_canonical_apple_manifest "${container_manifest}" "${unsigned_digest}"
  verify_macos_signatures
  verify_tauri_signatures_in_artifacts
}

verify_ios() {
  local final_manifest unsigned_manifest archive_manifest payload_manifest
  local unsigned_digest archive_digest payload_digest payload_seen payload_mismatch

  final_manifest="$(proof_file_required ios-release-final.sha256)"
  unsigned_manifest="$(proof_file_required ios-release-unsigned-app-canonical.sha256)"
  archive_manifest="$(proof_file_optional ios-release-archive-app-canonical.sha256)"
  if [ -z "${archive_manifest}" ]; then
    archive_manifest="$(proof_file_optional ios-release-signed-app-canonical.sha256)"
  fi
  payload_manifest="$(proof_file_required ios-release-canonical-payload.sha256)"

  verify_file_manifest "${final_manifest}"

  unsigned_digest="$(manifest_single_digest "${unsigned_manifest}")"
  if [ -z "${unsigned_digest}" ]; then
    echo "iOS unsigned app canonical proof is missing." >&2
    echo "unsigned=${unsigned_digest:-missing}" >&2
    return 1
  fi

  if [ -n "${archive_manifest}" ]; then
    archive_digest="$(manifest_single_digest "${archive_manifest}")"
    if [ -z "${archive_digest}" ]; then
      echo "iOS archive app canonical diagnostic proof is empty." >&2
      return 1
    fi
    if [ "${unsigned_digest}" != "${archive_digest}" ]; then
      printf 'diagnostic-ios-archive-app-proof-mismatch  unsigned=%s  archive=%s\n' "${unsigned_digest}" "${archive_digest}"
    else
      printf 'verified-ios-archive-app-proof  %s\n' "${archive_digest}"
    fi
  fi

  verify_canonical_apple_manifest "${payload_manifest}"
  payload_seen=0
  payload_mismatch=0
  while IFS= read -r payload_digest; do
    [ -n "${payload_digest}" ] || continue
    payload_seen=1
    if [ "${payload_digest}" = "${unsigned_digest}" ]; then
      printf 'verified-ios-exported-payload-proof  %s\n' "${payload_digest}"
    else
      payload_mismatch=1
      echo "iOS IPA payload canonical proof does not match unsigned app proof." >&2
      echo "unsigned=${unsigned_digest:-missing}" >&2
      echo "payload=${payload_digest}" >&2
    fi
  done < <(manifest_digests "${payload_manifest}")
  if [ "${payload_seen}" -eq 0 ]; then
    echo "iOS IPA payload canonical proof is missing." >&2
    return 1
  fi
  if [ "${payload_mismatch}" -ne 0 ]; then
    if [ "${MAPLE_ENFORCE_IOS_SIGNED_REPRODUCIBILITY:-0}" = "1" ]; then
      return 1
    fi
    printf 'warning-ios-exported-payload-proof-mismatch  unsigned=%s\n' "${unsigned_digest}"
  fi
  verify_ios_signatures
}

verify_web() {
  local final_manifest

  final_manifest="$(proof_file_required web-final.sha256)"
  verify_file_manifest "${final_manifest}"
}

verify_latest_json() {
  local final_manifest latest_json
  local platform url signature basename artifact sig_file sig_content

  final_manifest="$(proof_file_required latest-json-final.sha256)"
  verify_file_manifest "${final_manifest}"

  latest_json="$(artifact_for_label latest.json)"
  jq -e '
    (.version | type == "string" and length > 0)
    and (.pub_date | type == "string" and length > 0)
    and (.platforms | type == "object")
  ' "${latest_json}" >/dev/null

  for platform in darwin-aarch64 darwin-x86_64 linux-x86_64; do
    url="$(jq -er --arg platform "${platform}" '.platforms[$platform].url' "${latest_json}")"
    signature="$(jq -er --arg platform "${platform}" '.platforms[$platform].signature' "${latest_json}")"
    basename="$(basename "${url}")"
    artifact="$(artifact_for_label "${basename}")"
    sig_file="$(artifact_for_label "${basename}.sig")"
    sig_content="$(cat "${sig_file}")"

    if [ "${signature}" != "${sig_content}" ]; then
      echo "latest.json signature does not match sidecar for ${platform}." >&2
      return 1
    fi

    verify_tauri_updater_signature "${artifact}" "${sig_file}" "latest.json:${platform}:${basename}"
    printf 'verified-latest-json-signature-entry  %s  %s\n' "${platform}" "${basename}.sig"
  done
}

target_present() {
  local pattern="$1"
  [ -n "$(proof_file_optional "${pattern}")" ]
}

verify_present() {
  if target_present desktop-release-linux-final.sha256 || target_present desktop-pr-linux-fake-signing-final.sha256; then
    verify_linux_present
  fi
  target_present desktop-release-macos-final.sha256 && verify_macos
  if target_present desktop-release-windows-final.sha256 || target_present desktop-pr-windows-final.sha256; then
    verify_windows
  fi
  target_present android-release-final.sha256 && verify_android
  target_present ios-release-final.sha256 && verify_ios
  target_present web-final.sha256 && verify_web
  target_present latest-json-final.sha256 && verify_latest_json
  return 0
}

verify_all() {
  verify_linux
  verify_macos
  verify_android
  verify_ios
  verify_web
  verify_latest_json
}

for target in "$@"; do
  case "${target}" in
    all)
      verify_all
      ;;
    present)
      verify_present
      ;;
    linux)
      verify_linux
      ;;
    macos)
      verify_macos
      ;;
    windows)
      verify_windows
      ;;
    android)
      verify_android
      ;;
    ios)
      verify_ios
      ;;
    web)
      verify_web
      ;;
    latest-json)
      verify_latest_json
      ;;
    *)
      usage
      exit 2
      ;;
  esac
done
