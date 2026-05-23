#!/usr/bin/env bash
set -euo pipefail

source "$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)/_common.sh"

print_source_provenance

install_frontend_deps
configure_sccache
use_pr_environment
configure_reproducible_build_metadata
remove_generated_ios_cargo_config
build_frontend_dist

cd "${FRONTEND_DIR}"

case "$(host_os)" in
  linux)
    prepare_linux_onnxruntime
    export APPIMAGE_EXTRACT_AND_RUN="${APPIMAGE_EXTRACT_AND_RUN:-1}"
    export NO_STRIP="${NO_STRIP:-true}"
    bun tauri build --verbose --no-sign --config "$(linux_tauri_pr_config)"
    normalize_linux_desktop_packages

    desktop_artifacts=()
    while IFS= read -r -d '' file; do
      desktop_artifacts+=("${file}")
    done < <(find "${TAURI_DIR}/target/release/bundle" -type f \( -name '*.deb' -o -name '*.rpm' \) -print0 | LC_ALL=C sort -z)
    repro_dir="${TAURI_DIR}/target/reproducibility"
    write_sha256_manifest "${repro_dir}/desktop-pr-linux-final.sha256" "${desktop_artifacts[@]}" "${TAURI_DIR}/target/release/maple"
    print_file_hashes "${desktop_artifacts[@]}"
    print_file_hashes "${TAURI_DIR}/target/release/maple"

    if [ "${MAPLE_TAURI_FAKE_UPDATER_SIGNING:-0}" = "1" ]; then
      generate_fake_tauri_updater_keypair
      fake_public_key="${repro_dir}/desktop-pr-linux-fake-updater.pub"
      cp "${MAPLE_TAURI_UPDATER_PUBLIC_KEY_PATH}" "${fake_public_key}"
      write_sha256_manifest "${repro_dir}/desktop-pr-linux-fake-updater-public-key.sha256" "${fake_public_key}"

      sign_tauri_updater_artifacts "${desktop_artifacts[@]}"

      fake_signature_artifacts=()
      while IFS= read -r -d '' file; do
        fake_signature_artifacts+=("${file}")
      done < <(find "${TAURI_DIR}/target/release/bundle" -type f \( -name '*.deb.sig' -o -name '*.rpm.sig' \) -print0 | LC_ALL=C sort -z)

      if [ "${#fake_signature_artifacts[@]}" -eq 0 ]; then
        echo "No fake Linux updater signature artifacts were created." >&2
        exit 1
      fi

      verify_tauri_updater_signature_files "${fake_signature_artifacts[@]}"
      write_sha256_manifest "${repro_dir}/desktop-pr-linux-fake-signing-final.sha256" "${desktop_artifacts[@]}" "${fake_signature_artifacts[@]}"
      printf 'verified-linux-fake-updater-signatures  %s\n' "${#fake_signature_artifacts[@]}"
    fi

    verify_frontend_dist_unchanged
    ;;
  darwin)
    use_xcode_toolchain
    export MACOSX_DEPLOYMENT_TARGET="13.3"
    export CMAKE_OSX_DEPLOYMENT_TARGET="${MACOSX_DEPLOYMENT_TARGET}"
    export SDKROOT="${SDKROOT:-$(xcrun --sdk macosx --show-sdk-path)}"
    export LIBRARY_PATH="${SDKROOT}/usr/lib${LIBRARY_PATH:+:${LIBRARY_PATH}}"
    export RUSTFLAGS="${RUSTFLAGS:+${RUSTFLAGS} }-Clink-arg=-isysroot -Clink-arg=${SDKROOT}"
    bun tauri build --target universal-apple-darwin --no-sign --config '{"build":{"beforeBuildCommand":null},"bundle":{"createUpdaterArtifacts":false,"macOS":{"minimumSystemVersion":"13.3"}}}'

    desktop_artifacts=()
    while IFS= read -r -d '' file; do
      desktop_artifacts+=("${file}")
    done < <(find "${TAURI_DIR}/target/universal-apple-darwin/release/bundle" -type f -name '*.dmg' -print0 | LC_ALL=C sort -z)

    repro_dir="${TAURI_DIR}/target/reproducibility"
    mkdir -p "${repro_dir}"
    app_dir="${TAURI_DIR}/target/universal-apple-darwin/release/bundle/macos/Maple.app"
    app_archive="${repro_dir}/maple-macos-app.tar.gz"

    print_file_hashes "${desktop_artifacts[@]}"
    scrub_host_metadata_files "${app_dir}"
    print_tree_hash "${app_dir}"
    print_canonical_apple_bundle_hash "${app_dir}" "$(repo_relative_path "${app_dir}")" | tee "${repro_dir}/desktop-pr-macos-app-canonical.sha256"
    rm -f "${app_archive}"
    archive_tree_as_root_tar_gz "$(dirname "${app_dir}")" "${app_archive}"
    write_sha256_manifest "${repro_dir}/desktop-pr-macos-final.sha256" "${desktop_artifacts[@]}" "${app_archive}"
    print_file_hashes "${app_archive}"
    verify_frontend_dist_unchanged
    ;;
  *)
    echo "Unsupported desktop PR build host: $(uname -s)" >&2
    exit 1
    ;;
esac
