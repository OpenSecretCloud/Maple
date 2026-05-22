#!/usr/bin/env bash
set -euo pipefail

source "$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)/_common.sh"

print_source_provenance

configure_sccache
prepare_linux_onnxruntime

cd "${TAURI_DIR}"
cargo test --all-targets
