#!/usr/bin/env bash
set -euo pipefail

source "$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)/_common.sh"

release_tag="${1:-${RELEASE_TAG:-${GITHUB_REF_NAME:-}}}"
package_version="$(jq -er '.version' "${FRONTEND_DIR}/package.json")"
tauri_version="$(jq -er '.version' "${TAURI_DIR}/tauri.conf.json")"

if [ "${package_version}" != "${tauri_version}" ]; then
  echo "frontend/package.json version does not match frontend/src-tauri/tauri.conf.json." >&2
  echo "package=${package_version}" >&2
  echo "tauri=${tauri_version}" >&2
  exit 1
fi

if [ -z "${release_tag}" ]; then
  printf '%s\n' "${tauri_version}"
  exit 0
fi

if [[ ! "${release_tag}" =~ ^v?[0-9]+[.][0-9]+[.][0-9]+(-[0-9A-Za-z.-]+)?([+][0-9A-Za-z.-]+)?$ ]]; then
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
