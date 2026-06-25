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
linux_bundle="$(find_one_artifact '*.AppImage')"
linux_sig="$(find_one_artifact '*.AppImage.sig')"
windows_bundle_basename="$(windows_release_setup_exe_basename_for_version "${release_tag#v}")"
windows_bundle="$(find_one_artifact "${windows_bundle_basename}")"
windows_sig="$(find_one_artifact "${windows_bundle_basename}.sig")"

verify_tauri_updater_signature "${macos_bundle}" "${macos_sig}" "$(basename "${macos_bundle}")"
verify_tauri_updater_signature "${linux_bundle}" "${linux_sig}" "$(basename "${linux_bundle}")"
verify_tauri_updater_signature "${windows_bundle}" "${windows_sig}" "$(basename "${windows_bundle}")"

macos_sig_content="$(cat "${macos_sig}")"
linux_sig_content="$(cat "${linux_sig}")"
windows_sig_content="$(cat "${windows_sig}")"
macos_url="https://github.com/OpenSecretCloud/Maple/releases/download/${release_tag}/$(basename "${macos_bundle}")"
linux_url="https://github.com/OpenSecretCloud/Maple/releases/download/${release_tag}/$(basename "${linux_bundle}")"
windows_url="https://github.com/OpenSecretCloud/Maple/releases/download/${release_tag}/$(basename "${windows_bundle}")"

tmp="$(mktemp)"
jq -S -n \
  --arg version "${release_tag#v}" \
  --arg notes "See the release notes at https://github.com/OpenSecretCloud/Maple/releases/tag/${release_tag}" \
  --arg pub_date "${pub_date}" \
  --arg macos_sig "${macos_sig_content}" \
  --arg linux_sig "${linux_sig_content}" \
  --arg windows_sig "${windows_sig_content}" \
  --arg macos_url "${macos_url}" \
  --arg linux_url "${linux_url}" \
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
        signature: $linux_sig,
        url: $linux_url
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
  and (.platforms."windows-x86_64".url | startswith("https://"))
  and (.platforms."windows-x86_64".signature | type == "string" and length > 0)
' "${out}" >/dev/null

repro_dir="${TAURI_DIR}/target/reproducibility"
mkdir -p "${repro_dir}"
write_sha256_manifest "${repro_dir}/latest-json-final.sha256" "${out}" "${macos_sig}" "${linux_sig}" "${windows_sig}"
print_file_hashes "${out}" "${macos_sig}" "${linux_sig}" "${windows_sig}"
