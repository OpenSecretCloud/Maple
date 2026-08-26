#!/usr/bin/env bash
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
sdk_dir="${repo_root}/sdk"

cd "${sdk_dir}"
bun --no-env-file install --frozen-lockfile --ignore-scripts
rm -rf dist
bun --no-env-file run build

test -f dist/opensecret-react.es.js
test -f dist/opensecret-react.umd.js
test -f dist/index.d.ts
