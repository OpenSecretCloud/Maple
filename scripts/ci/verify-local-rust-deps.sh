#!/usr/bin/env bash
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd -P)"
metadata_file="$(mktemp)"
local_mock_metadata_file="$(mktemp)"
trap 'rm -f "${metadata_file}" "${local_mock_metadata_file}"' EXIT

cd "${repo_root}"
cargo metadata \
  --locked \
  --manifest-path frontend/src-tauri/Cargo.toml \
  --format-version 1 > "${metadata_file}"
cargo metadata \
  --locked \
  --manifest-path frontend/src-tauri/Cargo.toml \
  --features insecure-local-mock-attestation \
  --format-version 1 > "${local_mock_metadata_file}"

jq -e --arg root "${repo_root}" '
  ([.packages[] | select(.name == "opensecret")] | length) == 1 and
  ([.packages[] | select(.name == "maple-proxy")] | length) == 1 and
  ([.packages[] | select(.name == "opensecret")][0].source == null) and
  ([.packages[] | select(.name == "opensecret")][0].manifest_path == ($root + "/sdk/rust/Cargo.toml")) and
  ([.packages[] | select(.name == "maple-proxy")][0].source == null) and
  ([.packages[] | select(.name == "maple-proxy")][0].manifest_path == ($root + "/proxy/Cargo.toml"))
' "${metadata_file}" > /dev/null

opensecret_id="$(jq -er '[.packages[] | select(.name == "opensecret" and .source == null)][0].id' "${metadata_file}")"
proxy_id="$(jq -er '[.packages[] | select(.name == "maple-proxy" and .source == null)][0].id' "${metadata_file}")"

jq -e \
  --arg opensecret_id "${opensecret_id}" \
  --arg proxy_id "${proxy_id}" '
    ([.resolve.nodes[] | select(.id == $opensecret_id)][0].features | index("mock-attestation")) == null and
    ([.resolve.nodes[] | select(.id == $proxy_id)][0].features | index("insecure-local-mock-attestation")) == null
  ' "${metadata_file}" > /dev/null

jq -e \
  --arg opensecret_id "${opensecret_id}" \
  --arg proxy_id "${proxy_id}" '
    ([.resolve.nodes[] | select(.id == $opensecret_id)][0].features | index("mock-attestation")) != null and
    ([.resolve.nodes[] | select(.id == $proxy_id)][0].features | index("insecure-local-mock-attestation")) != null
  ' "${local_mock_metadata_file}" > /dev/null

echo "Maple resolves exactly one in-tree OpenSecret SDK and proxy crate."
echo "Default builds exclude mock attestation; the explicit local-development feature enables it."
