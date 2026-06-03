#!/usr/bin/env bash
set -euo pipefail

source "$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)/_common.sh"

case "$(host_os)" in
  mingw* | msys* | cygwin*)
    ;;
  *)
    echo "desktop-windows-pr.sh must run on Windows under Git Bash/MSYS." >&2
    exit 1
    ;;
esac

to_windows_path() {
  if command -v cygpath >/dev/null 2>&1; then
    cygpath -w "$1"
  else
    printf '%s\n' "$1"
  fi
}

prepare_windows_onnxruntime() {
  local ort_env key value

  ort_env="$("${TAURI_DIR}/scripts/provide-windows-onnxruntime.sh")"
  printf '%s\n' "${ort_env}"

  while IFS='=' read -r key value; do
    case "${key}" in
      ORT_LIB_LOCATION | ORT_SKIP_DOWNLOAD | ORT_DYLIB_PATH)
        export "${key}=${value}"
        ;;
    esac
  done <<< "${ort_env}"
}

stage_windows_runtime_dlls() {
  local vc_redist_version wix_cli_version

  vc_redist_version="$(windows_vc_redist_x64_version)"
  export MAPLE_WINDOWS_VC_REDIST_VERSION="${vc_redist_version}"
  export MAPLE_WINDOWS_VC_REDIST_URL
  export MAPLE_WINDOWS_VC_REDIST_SHA256
  MAPLE_WINDOWS_VC_REDIST_URL="$(windows_vc_redist_x64_url_for_version "${vc_redist_version}")"
  MAPLE_WINDOWS_VC_REDIST_SHA256="$(windows_vc_redist_x64_archive_sha256_for_version "${vc_redist_version}")"

  wix_cli_version="$(windows_wix_cli_version)"
  export MAPLE_WINDOWS_WIX_CLI_VERSION="${wix_cli_version}"
  export MAPLE_WINDOWS_WIX_CLI_URL
  export MAPLE_WINDOWS_WIX_CLI_SHA256
  MAPLE_WINDOWS_WIX_CLI_URL="$(windows_wix_cli_url_for_version "${wix_cli_version}")"
  MAPLE_WINDOWS_WIX_CLI_SHA256="$(windows_wix_cli_archive_sha256_for_version "${wix_cli_version}")"

  pwsh -NoLogo -NoProfile -ExecutionPolicy Bypass \
    -File "$(to_windows_path "${TAURI_DIR}/scripts/stage-windows-runtime-dlls.ps1")" \
    -OrtDllPath "$(to_windows_path "${ORT_DYLIB_PATH:?ORT_DYLIB_PATH is required}")" \
    -Destination "$(to_windows_path "${TAURI_DIR}/resources/windows")"
}

print_source_provenance

install_frontend_deps
configure_sccache
use_pr_environment
configure_reproducible_build_metadata
prepare_windows_onnxruntime
stage_windows_runtime_dlls
build_frontend_dist

cd "${FRONTEND_DIR}"

remove_build_tree "${TAURI_DIR}/target/release/bundle/nsis"
bun tauri build --verbose --no-sign --config '{"build":{"beforeBuildCommand":null},"bundle":{"createUpdaterArtifacts":false}}'

repro_dir="${TAURI_DIR}/target/reproducibility"
mkdir -p "${repro_dir}"

windows_artifacts=()
while IFS= read -r -d '' file; do
  windows_artifacts+=("${file}")
done < <(find "${TAURI_DIR}/target/release/bundle/nsis" -type f -name '*.exe' -print0 | LC_ALL=C sort -z)

if [ -f "${TAURI_DIR}/target/release/maple.exe" ]; then
  windows_artifacts+=("${TAURI_DIR}/target/release/maple.exe")
fi

windows_runtime_dlls=()
while IFS= read -r -d '' file; do
  windows_runtime_dlls+=("${file}")
done < <(find "${TAURI_DIR}/resources/windows" -type f -name '*.dll' -print0 | LC_ALL=C sort -z)

if [ "${#windows_artifacts[@]}" -eq 0 ]; then
  echo "windows_artifacts is empty; no Windows .exe artifacts found under ${TAURI_DIR}/target/release." >&2
  exit 1
fi

if [ "${#windows_runtime_dlls[@]}" -eq 0 ]; then
  echo "windows_runtime_dlls is empty; no staged runtime DLLs found under ${TAURI_DIR}/resources/windows." >&2
  exit 1
fi

write_sha256_manifest "${repro_dir}/desktop-pr-windows-final.sha256" "${windows_artifacts[@]}"
write_sha256_manifest "${repro_dir}/desktop-pr-windows-runtime-dlls.sha256" "${windows_runtime_dlls[@]}"

print_file_hashes "${windows_artifacts[@]}"
print_file_hashes "${windows_runtime_dlls[@]}"
verify_frontend_dist_unchanged
