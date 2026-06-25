#!/usr/bin/env bash
set -euo pipefail

source "$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)/_common.sh"

usage() {
  cat >&2 <<'EOF'
usage: desktop-windows-release.sh <build|bundle|finalize>

build
  Prepare the reproducible Windows release environment, build frontend assets,
  stage pinned runtime DLLs, and compile maple.exe without bundling.

bundle
  Generate the NSIS installer. Tauri patches and signs maple.exe through
  bundle.windows.signCommand during bundling, signs the installer, then restores
  target/release/maple.exe to its original unsigned/unpatched bytes.

finalize
  Verify Authenticode signatures on the NSIS installer, create the final Tauri
  updater signature for the signed installer, and emit release reproducibility
  manifests.
EOF
}

mode="${1:-}"
case "${mode}" in
  build | bundle | finalize)
    ;;
  -h | --help | help)
    usage
    exit 0
    ;;
  *)
    usage
    exit 2
    ;;
esac

require_windows_host "desktop-windows-release.sh"

run_build_phase() {
  local app_exe

  print_source_provenance
  install_frontend_deps
  configure_sccache
  use_release_environment
  configure_reproducible_build_metadata
  prepare_windows_onnxruntime
  stage_windows_runtime_dlls
  build_frontend_dist

  cd "${FRONTEND_DIR}"
  remove_build_tree "${TAURI_DIR}/target/release/bundle/nsis"
  remove_build_tree "${TAURI_DIR}/target/release/bundle/msi"

  bun tauri build --verbose --no-bundle --config "$(windows_tauri_release_build_config)"

  app_exe="$(windows_release_app_exe)"
  if [ ! -f "${app_exe}" ]; then
    echo "Windows app executable was not built: ${app_exe}" >&2
    exit 1
  fi
  print_file_hashes "${app_exe}"
  verify_frontend_dist_unchanged
}

run_bundle_phase() {
  local app_exe setup_exe

  print_source_provenance
  configure_sccache
  use_release_environment
  configure_reproducible_build_metadata

  app_exe="$(windows_release_app_exe)"
  if [ ! -f "${app_exe}" ]; then
    echo "Windows app executable is missing before bundling: ${app_exe}" >&2
    exit 1
  fi

  cd "${FRONTEND_DIR}"
  remove_build_tree "${TAURI_DIR}/target/release/bundle/nsis"
  remove_build_tree "${TAURI_DIR}/target/release/bundle/msi"

  bun tauri bundle --verbose --bundles nsis --config "$(windows_tauri_release_bundle_config)"

  setup_exe="$(windows_release_setup_exe_required)"
  # Tauri restores target/release/maple.exe after bundling. The durable signed
  # artifact at this point is the NSIS installer; installed-payload verification
  # should inspect an extracted installer payload, not the restored build output.
  verify_windows_authenticode_signatures "${setup_exe}"
  print_file_hashes "${setup_exe}"
  verify_frontend_dist_unchanged
}

run_finalize_phase() {
  local setup_exe repro_dir
  local windows_runtime_dlls=()

  print_source_provenance
  configure_reproducible_build_metadata
  configure_tauri_updater_signing_key

  setup_exe="$(windows_release_setup_exe_required)"
  verify_windows_authenticode_signatures "${setup_exe}"

  rm -f "${setup_exe}.sig"
  sign_tauri_updater_artifacts "${setup_exe}"
  verify_tauri_updater_signature_files "${setup_exe}.sig"

  repro_dir="${TAURI_DIR}/target/reproducibility"
  mkdir -p "${repro_dir}"

  while IFS= read -r -d '' file; do
    windows_runtime_dlls+=("${file}")
  done < <(find "${TAURI_DIR}/resources/windows" -type f -name '*.dll' -print0 | LC_ALL=C sort -z)

  if [ "${#windows_runtime_dlls[@]}" -eq 0 ]; then
    echo "windows_runtime_dlls is empty; no staged runtime DLLs found under ${TAURI_DIR}/resources/windows." >&2
    exit 1
  fi

  write_sha256_manifest "${repro_dir}/desktop-release-windows-final.sha256" \
    "${setup_exe}" \
    "${setup_exe}.sig"
  write_sha256_manifest "${repro_dir}/desktop-release-windows-runtime-dlls.sha256" \
    "${windows_runtime_dlls[@]}"

  print_file_hashes "${setup_exe}" "${setup_exe}.sig"
  print_file_hashes "${windows_runtime_dlls[@]}"
  verify_frontend_dist_unchanged
}

case "${mode}" in
  build)
    run_build_phase
    ;;
  bundle)
    run_bundle_phase
    ;;
  finalize)
    run_finalize_phase
    ;;
esac
