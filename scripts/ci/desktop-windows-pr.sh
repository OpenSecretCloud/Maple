#!/usr/bin/env bash
set -euo pipefail

source "$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)/_common.sh"

require_windows_host "desktop-windows-pr.sh"

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
