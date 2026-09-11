#!/usr/bin/env bash
# Build and compare only. Never copy, sign, or publish approval files.
set -euo pipefail

if [[ $# -ne 1 ]]; then
  echo "Usage: check_opensecret_eif.sh dev|prod" >&2
  exit 2
fi
case "$1" in
  dev) reference=pcrDev.json ;;
  prod) reference=pcrProd.json ;;
  *) echo "Expected dev or prod." >&2; exit 2 ;;
esac
mode=$1

if [[ "$(uname -s)" != Linux || "$(uname -m)" != aarch64 ]]; then
  echo "EIF approval checks require a Linux ARM64 runner." >&2
  exit 1
fi

repo_root=$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/../.." && pwd -P)
cd "$repo_root/services/opensecret"
if [[ ! -f "$reference" || ! -s "$reference" || -L "$reference" ]]; then
  echo "Missing regular approved measurement file: $reference" >&2
  exit 1
fi

# Direct Nix invocation avoids just's dotenv loader and development shell hooks.
# A fresh temporary link never replaces an operator's existing result symlink.
output_dir=$(mktemp -d "${TMPDIR:-/tmp}/opensecret-eif-${mode}.XXXXXX")
trap 'rm -rf -- "$output_dir"' EXIT
nix build --no-update-lock-file --print-build-logs \
  --out-link "$output_dir/result" ".?submodules=1#eif-$mode"

if [[ ! -f "$output_dir/result/image.eif" || ! -s "$output_dir/result/image.eif" ||
      ! -f "$output_dir/result/pcr.json" || ! -s "$output_dir/result/pcr.json" ]]; then
  echo "EIF build did not produce image.eif and pcr.json." >&2
  exit 1
fi

if ! diff -u -- "$reference" "$output_dir/result/pcr.json"; then
  echo "EIF/PCR approval mismatch ($mode): this build does not match $reference." >&2
  echo "Review the measurements through the manual approval process; CI will not update or sign them." >&2
  exit 1
fi
echo "EIF/PCR approval match ($mode). This does not authorize deployment."
