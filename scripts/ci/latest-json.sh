#!/usr/bin/env bash
set -euo pipefail

source "$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)/_common.sh"

print_source_provenance

artifacts_dir="${1:-artifacts}"
out="${2:-latest.json}"
release_tag="${RELEASE_TAG:-${GITHUB_REF_NAME:-}}"

if [ -z "${release_tag}" ]; then
  echo "RELEASE_TAG or GITHUB_REF_NAME is required." >&2
  exit 1
fi

if [ ! -d "${artifacts_dir}" ]; then
  echo "Artifacts directory does not exist: ${artifacts_dir}" >&2
  exit 1
fi

configure_reproducible_build_metadata
pub_date="${MAPLE_LATEST_JSON_PUB_DATE:-$(source_date_rfc3339)}"

repository_metadata="${REPO_ROOT}/repo.meta.json"
github_owner="$(jq -er '.github.owner | select(type == "string" and length > 0)' "${repository_metadata}")"
github_repository="$(jq -er '.github.repository | select(type == "string" and length > 0)' "${repository_metadata}")"
github_repository_url="https://github.com/${github_owner}/${github_repository}"

find_one_artifact() {
  local pattern="$1"
  local file
  local matches=()

  while IFS= read -r -d '' file; do
    matches+=("${file}")
  done < <(find "${artifacts_dir}" -type f -name "${pattern}" -print0 | LC_ALL=C sort -z)

  if [ "${#matches[@]}" -eq 0 ]; then
    echo "Could not find artifact matching ${pattern} in ${artifacts_dir}" >&2
    return 1
  fi

  if [ "${#matches[@]}" -ne 1 ]; then
    echo "Expected exactly one artifact matching ${pattern} in ${artifacts_dir}, found ${#matches[@]}:" >&2
    printf '  %s\n' "${matches[@]}" >&2
    return 1
  fi

  printf '%s\n' "${matches[0]}"
}

macos_bundle="$(find_one_artifact '*.app.tar.gz')"
macos_sig="$(find_one_artifact '*.app.tar.gz.sig')"
linux_appimage_bundle="$(find_one_artifact 'Maple_*_amd64.AppImage')"
linux_appimage_sig="$(find_one_artifact 'Maple_*_amd64.AppImage.sig')"
linux_deb_bundle="$(find_one_artifact 'Maple_*_amd64.deb')"
linux_deb_sig="$(find_one_artifact 'Maple_*_amd64.deb.sig')"
linux_rpm_bundle="$(find_one_artifact 'Maple-*.x86_64.rpm')"
linux_rpm_sig="$(find_one_artifact 'Maple-*.x86_64.rpm.sig')"
windows_bundle_basename="$(windows_release_setup_exe_basename_for_version "${release_tag#v}")"
windows_bundle="$(find_one_artifact "${windows_bundle_basename}")"
windows_sig="$(find_one_artifact "${windows_bundle_basename}.sig")"

verify_tauri_updater_signature "${macos_bundle}" "${macos_sig}" "$(basename "${macos_bundle}")"
verify_tauri_updater_signature "${linux_appimage_bundle}" "${linux_appimage_sig}" "$(basename "${linux_appimage_bundle}")"
verify_tauri_updater_signature "${linux_deb_bundle}" "${linux_deb_sig}" "$(basename "${linux_deb_bundle}")"
verify_tauri_updater_signature "${linux_rpm_bundle}" "${linux_rpm_sig}" "$(basename "${linux_rpm_bundle}")"
verify_tauri_updater_signature "${windows_bundle}" "${windows_sig}" "$(basename "${windows_bundle}")"

macos_sig_content="$(cat "${macos_sig}")"
linux_appimage_sig_content="$(cat "${linux_appimage_sig}")"
linux_deb_sig_content="$(cat "${linux_deb_sig}")"
linux_rpm_sig_content="$(cat "${linux_rpm_sig}")"
windows_sig_content="$(cat "${windows_sig}")"
macos_url="${github_repository_url}/releases/download/${release_tag}/$(basename "${macos_bundle}")"
linux_appimage_url="${github_repository_url}/releases/download/${release_tag}/$(basename "${linux_appimage_bundle}")"
linux_deb_url="${github_repository_url}/releases/download/${release_tag}/$(basename "${linux_deb_bundle}")"
linux_rpm_url="${github_repository_url}/releases/download/${release_tag}/$(basename "${linux_rpm_bundle}")"
windows_url="${github_repository_url}/releases/download/${release_tag}/$(basename "${windows_bundle}")"

tmp="$(mktemp)"
jq -S -n \
  --arg version "${release_tag#v}" \
  --arg notes "See the release notes at ${github_repository_url}/releases/tag/${release_tag}" \
  --arg pub_date "${pub_date}" \
  --arg macos_sig "${macos_sig_content}" \
  --arg linux_appimage_sig "${linux_appimage_sig_content}" \
  --arg linux_deb_sig "${linux_deb_sig_content}" \
  --arg linux_rpm_sig "${linux_rpm_sig_content}" \
  --arg windows_sig "${windows_sig_content}" \
  --arg macos_url "${macos_url}" \
  --arg linux_appimage_url "${linux_appimage_url}" \
  --arg linux_deb_url "${linux_deb_url}" \
  --arg linux_rpm_url "${linux_rpm_url}" \
  --arg windows_url "${windows_url}" \
  '{
    notes: $notes,
    platforms: {
      "darwin-aarch64": {
        signature: $macos_sig,
        url: $macos_url
      },
      "darwin-x86_64": {
        signature: $macos_sig,
        url: $macos_url
      },
      "linux-x86_64": {
        signature: $linux_appimage_sig,
        url: $linux_appimage_url
      },
      "linux-x86_64-appimage": {
        signature: $linux_appimage_sig,
        url: $linux_appimage_url
      },
      "linux-x86_64-deb": {
        signature: $linux_deb_sig,
        url: $linux_deb_url
      },
      "linux-x86_64-rpm": {
        signature: $linux_rpm_sig,
        url: $linux_rpm_url
      },
      "windows-x86_64": {
        signature: $windows_sig,
        url: $windows_url
      }
    },
    pub_date: $pub_date,
    version: $version
  }' > "${tmp}"

mv "${tmp}" "${out}"
jq -e '
  (.version | type == "string" and length > 0)
  and (.platforms."darwin-aarch64".url | startswith("https://"))
  and (.platforms."darwin-aarch64".signature | type == "string" and length > 0)
  and (.platforms."darwin-x86_64".url | startswith("https://"))
  and (.platforms."darwin-x86_64".signature | type == "string" and length > 0)
  and (.platforms."linux-x86_64".url | startswith("https://"))
  and (.platforms."linux-x86_64".signature | type == "string" and length > 0)
  and (.platforms."linux-x86_64-appimage".url | startswith("https://") and endswith(".AppImage"))
  and (.platforms."linux-x86_64-appimage".signature | type == "string" and length > 0)
  and (.platforms."linux-x86_64-deb".url | startswith("https://") and endswith(".deb"))
  and (.platforms."linux-x86_64-deb".signature | type == "string" and length > 0)
  and (.platforms."linux-x86_64-rpm".url | startswith("https://") and endswith(".rpm"))
  and (.platforms."linux-x86_64-rpm".signature | type == "string" and length > 0)
  and (.platforms."linux-x86_64" == .platforms."linux-x86_64-appimage")
  and (.platforms."windows-x86_64".url | startswith("https://"))
  and (.platforms."windows-x86_64".signature | type == "string" and length > 0)
' "${out}" >/dev/null

repro_dir="${TAURI_DIR}/target/reproducibility"
mkdir -p "${repro_dir}"
write_sha256_manifest \
  "${repro_dir}/latest-json-final.sha256" \
  "${out}" \
  "${macos_sig}" \
  "${linux_appimage_sig}" \
  "${linux_deb_sig}" \
  "${linux_rpm_sig}" \
  "${windows_sig}"
print_file_hashes \
  "${out}" \
  "${macos_sig}" \
  "${linux_appimage_sig}" \
  "${linux_deb_sig}" \
  "${linux_rpm_sig}" \
  "${windows_sig}"
