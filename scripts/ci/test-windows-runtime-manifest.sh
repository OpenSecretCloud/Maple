#!/usr/bin/env bash
set -euo pipefail

script_dir="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
temp_root="$(mktemp -d)"
trap 'rm -rf "${temp_root}"' EXIT HUP INT TERM

# Loading the verifier with an empty optional artifact set makes its real
# runtime-manifest validator available without needing a signed NSIS fixture.
source "${script_dir}/verify-release-artifacts.sh" "${temp_root}" present

passed=0
digest="aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"
legacy_prefix="frontend/src-tauri/resources/windows/"
research_prefix="apps/maple-research/frontend/src-tauri/resources/windows/"

pass() {
  passed=$((passed + 1))
  printf 'ok %d - %s\n' "${passed}" "$1"
}

expect_failure() {
  local description="$1"
  local manifest="$2"
  local expected_message="$3"
  local output

  if output="$(verify_windows_runtime_manifest "${manifest}" 2>&1)"; then
    printf 'not ok - %s\n%s\n' "${description}" "${output}" >&2
    exit 1
  fi
  if [[ "${output}" != *"${expected_message}"* ]]; then
    printf 'not ok - %s failed for the wrong reason\n%s\n' "${description}" "${output}" >&2
    exit 1
  fi
  pass "${description}"
}

make_manifest() {
  local prefix="$1"
  local name
  for name in MSVCP140.dll MSVCP140_1.dll onnxruntime.dll VCRUNTIME140.dll VCRUNTIME140_1.dll; do
    printf '%s  %s%s\n' "${digest}" "${prefix}" "${name}"
  done
}

make_manifest "${legacy_prefix}" > "${temp_root}/legacy.sha256"
verify_windows_runtime_manifest "${temp_root}/legacy.sha256" >/dev/null
pass "accepts the complete historical DLL proof inventory"

make_manifest "${research_prefix}" > "${temp_root}/research.sha256"
verify_windows_runtime_manifest "${temp_root}/research.sha256" >/dev/null
pass "accepts the complete relocated Research DLL proof inventory"

make_manifest "./${research_prefix}" > "${temp_root}/relative.sha256"
verify_windows_runtime_manifest "${temp_root}/relative.sha256" >/dev/null
pass "retains support for explicitly relative proof labels"

{
  head -n 4 "${temp_root}/legacy.sha256"
  tail -n 1 "${temp_root}/research.sha256"
} > "${temp_root}/mixed.sha256"
expect_failure "rejects mixed old and new paths despite a complete inventory" \
  "${temp_root}/mixed.sha256" "Mixed Windows runtime DLL proof layouts"

{
  cat "${temp_root}/research.sha256"
  head -n 1 "${temp_root}/research.sha256"
} > "${temp_root}/duplicate.sha256"
expect_failure "rejects a duplicate DLL proof" \
  "${temp_root}/duplicate.sha256" "Duplicate Windows runtime DLL proof"

head -n 4 "${temp_root}/research.sha256" > "${temp_root}/missing.sha256"
expect_failure "rejects an incomplete DLL inventory" \
  "${temp_root}/missing.sha256" "Missing Windows runtime DLL proof"

make_manifest "arbitrary/${research_prefix}" > "${temp_root}/wrong-root.sha256"
expect_failure "rejects a known DLL under an unrecognized root" \
  "${temp_root}/wrong-root.sha256" "Unexpected Windows runtime DLL proof"

make_manifest "${research_prefix}../" > "${temp_root}/traversal.sha256"
expect_failure "rejects traversal after a valid prefix" \
  "${temp_root}/traversal.sha256" "Unexpected Windows runtime DLL proof"

cp "${temp_root}/research.sha256" "${temp_root}/extra.sha256"
printf '%s  %sunexpected.dll\n' "${digest}" "${research_prefix}" >> "${temp_root}/extra.sha256"
expect_failure "rejects an additional DLL outside the fixed inventory" \
  "${temp_root}/extra.sha256" "Unexpected Windows runtime DLL proof"

printf '%s  %s\n' "${digest}" "${research_prefix}" > "${temp_root}/empty-name.sha256"
expect_failure "rejects a directory without a DLL name" \
  "${temp_root}/empty-name.sha256" "Unexpected Windows runtime DLL proof"

printf 'invalid-hash  %sonnxruntime.dll\n' "${research_prefix}" > "${temp_root}/invalid-hash.sha256"
expect_failure "rejects an invalid digest" \
  "${temp_root}/invalid-hash.sha256" "Invalid Windows runtime manifest line"

printf '1..%d\n' "${passed}"
