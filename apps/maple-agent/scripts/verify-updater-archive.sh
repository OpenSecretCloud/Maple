#!/usr/bin/env bash
# Read actual produced Prod package data with the trusted installer scanner.
set -euo pipefail
if [[ $# -lt 1 || $# -gt 2 || ( $# == 2 && "$2" != --unsigned ) ]]; then
    echo "usage: verify-updater-archive.sh ARTIFACT_DIRECTORY [--unsigned]" >&2
    exit 2
fi
component="$(cd "$(dirname "$0")/.." && pwd)"
directory="$(cd "$1" && pwd)"
unsigned=()
[[ $# == 1 ]] || unsigned=(--unsigned)
scanner="$component/target/debug/examples/validate-release-archive"
[[ -x "$scanner" && ! -L "$scanner" ]] || { echo "build the trusted updater archive scanner first" >&2; exit 1; }
python3 "$component/scripts/release-info.py" verify prod "$directory" \
    --source-sha "$(git -C "$component" rev-parse HEAD)" "${unsigned[@]}" >/dev/null
archives=("$directory"/*.app.tar.gz)
[[ ${#archives[@]} == 1 && -f "${archives[0]}" && ! -L "${archives[0]}" ]] || { echo "expected exactly one regular Prod app archive" >&2; exit 1; }
archive="${archives[0]}"
expected="$(python3 - "$directory/SHA256SUMS" "$(basename "$archive")" <<'PY'
import re,sys
lines=open(sys.argv[1], encoding="ascii").read().splitlines()
matches=[line.split("  ",1)[0] for line in lines if line.endswith("  "+sys.argv[2])]
if len(matches) != 1 or not re.fullmatch(r"[0-9a-f]{64}", matches[0]):
    raise SystemExit("archive must have one package checksum")
print(matches[0])
PY
)"
"$scanner" "$archive" "$expected"
