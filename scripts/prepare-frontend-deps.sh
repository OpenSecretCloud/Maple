#!/usr/bin/env bash
set -euo pipefail

if [ "${MAPLE_FRONTEND_DEPS_PREPARED:-0}" = "1" ]; then
  exit 0
fi

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
frontend_dir="${repo_root}/apps/maple-research/frontend"

sdk_dependency="$(bun --no-env-file -p 'require(process.argv[1]).dependencies["@mapleai/sdk"]' "${frontend_dir}/package.json")"
if [ "${sdk_dependency}" = "file:../../../sdk" ]; then
  "${repo_root}/scripts/prepare-typescript-sdk.sh"
fi

cd "${frontend_dir}"
bun --no-env-file install --frozen-lockfile --ignore-scripts
