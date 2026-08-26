#!/usr/bin/env bash
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd -P)"
metadata_file="$(mktemp)"
trap 'rm -f "${metadata_file}"' EXIT

cd "${repo_root}"
cargo metadata \
  --locked \
  --manifest-path frontend/src-tauri/Cargo.toml \
  --format-version 1 > "${metadata_file}"

jq -e --arg root "${repo_root}" '
  ([.packages[] | select(.name == "opensecret")] | length) == 1 and
  ([.packages[] | select(.name == "maple-proxy")] | length) == 1 and
  ([.packages[] | select(.name == "opensecret")][0].source == null) and
  ([.packages[] | select(.name == "opensecret")][0].manifest_path == ($root + "/sdk/rust/Cargo.toml")) and
  ([.packages[] | select(.name == "maple-proxy")][0].source == null) and
  ([.packages[] | select(.name == "maple-proxy")][0].manifest_path == ($root + "/proxy/Cargo.toml"))
' "${metadata_file}" > /dev/null

echo "Maple resolves exactly one in-tree OpenSecret SDK and proxy crate."
