#!/usr/bin/env bash
set -euo pipefail

script_dir="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"

if [ -n "${MAPLE_RELEASE_REPO_ROOT:-}" ]; then
  if ! repo_root="$(cd "${MAPLE_RELEASE_REPO_ROOT}" && pwd -P)"; then
    echo "MAPLE_RELEASE_REPO_ROOT is not an accessible directory: ${MAPLE_RELEASE_REPO_ROOT}" >&2
    exit 1
  fi
else
  repo_root="$(cd "${script_dir}/../.." && pwd -P)"
fi

frontend_dir="${repo_root}/frontend"
tauri_dir="${frontend_dir}/src-tauri"

release_tag="${1:-${RELEASE_TAG:-${GITHUB_REF_NAME:-}}}"
package_version="$(jq -er '.version' "${frontend_dir}/package.json")"
tauri_version="$(jq -er '.version' "${tauri_dir}/tauri.conf.json")"
cargo_version="$(awk '
  /^\[package\]$/ { in_package = 1; next }
  /^\[/ && in_package { exit }
  in_package && /^version[[:space:]]*=/ {
    value = $0
    sub(/^[^=]*=[[:space:]]*"/, "", value)
    sub(/"[[:space:]]*$/, "", value)
    print value
    exit
  }
' "${tauri_dir}/Cargo.toml")"

if [ -z "${cargo_version}" ]; then
  echo "Could not read the Cargo package version." >&2
  exit 1
fi

if [ "${package_version}" != "${tauri_version}" ]; then
  echo "frontend/package.json version does not match frontend/src-tauri/tauri.conf.json." >&2
  echo "package=${package_version}" >&2
  echo "tauri=${tauri_version}" >&2
  exit 1
fi

if [ "${package_version}" != "${cargo_version}" ]; then
  echo "frontend/package.json version does not match frontend/src-tauri/Cargo.toml." >&2
  echo "package=${package_version}" >&2
  echo "cargo=${cargo_version}" >&2
  exit 1
fi

if [ -z "${release_tag}" ]; then
  printf '%s\n' "${tauri_version}"
  exit 0
fi

if [[ ! "${release_tag}" =~ ^v(0|[1-9][0-9]*)[.](0|[1-9][0-9]*)[.](0|[1-9][0-9]*)$ ]]; then
  echo "Refusing to build unexpected release tag: ${release_tag}" >&2
  exit 1
fi

release_version="${release_tag#v}"
if [ "${release_version}" != "${tauri_version}" ]; then
  echo "Release tag version does not match app version." >&2
  echo "tag=${release_tag}" >&2
  echo "tag_version=${release_version}" >&2
  echo "app_version=${tauri_version}" >&2
  exit 1
fi

printf '%s\n' "${release_version}"
