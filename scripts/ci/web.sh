#!/usr/bin/env bash
set -euo pipefail

source "$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)/_common.sh"

print_source_provenance

install_frontend_deps

case "${MAPLE_WEB_ENVIRONMENT:-pr}" in
  pr)
    use_pr_environment
    ;;
  release)
    use_release_environment
    ;;
  *)
    echo "Unsupported MAPLE_WEB_ENVIRONMENT=${MAPLE_WEB_ENVIRONMENT}. Expected pr or release." >&2
    exit 1
    ;;
esac

configure_reproducible_build_metadata
build_frontend_dist

repro_dir="${TAURI_DIR}/target/reproducibility"
mkdir -p "${repro_dir}"

web_archive="${repro_dir}/maple-web-dist.tar.gz"
archive_tree_as_root_tar_gz "${FRONTEND_DIR}/dist" "${web_archive}"
write_sha256_manifest "${repro_dir}/web-final.sha256" "${web_archive}"
print_file_hashes "${web_archive}"
