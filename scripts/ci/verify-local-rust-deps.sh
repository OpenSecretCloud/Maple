#!/usr/bin/env bash
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd -P)"
metadata_file="$(mktemp)"
trap 'rm -f "${metadata_file}"' EXIT

cd "${repo_root}"
cargo metadata \
  --locked \
  --manifest-path apps/maple-research/frontend/src-tauri/Cargo.toml \
  --format-version 1 > "${metadata_file}"

# Research and Agent share the same SDK/proxy identity constraints. Cargo's
# --locked resolution above checks that declarations and the lockfile agree.
python3 scripts/ci/verify-agent-rust-deps.py "${metadata_file}"
