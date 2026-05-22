#!/usr/bin/env bash
set -euo pipefail

source "$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)/_common.sh"

print_source_provenance

install_frontend_deps

cd "${FRONTEND_DIR}"
bun run format:check
bun run lint
bun run typecheck
bun run test
