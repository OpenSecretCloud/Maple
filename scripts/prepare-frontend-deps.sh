#!/usr/bin/env bash
set -euo pipefail

if [ "${MAPLE_FRONTEND_DEPS_PREPARED:-0}" = "1" ]; then
  exit 0
fi

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"

"${repo_root}/scripts/prepare-typescript-sdk.sh"

cd "${repo_root}/frontend"
bun --no-env-file install --frozen-lockfile --ignore-scripts
