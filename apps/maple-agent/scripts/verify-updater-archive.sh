#!/usr/bin/env bash
# Read actual produced package data with the trusted installer scanner.
set -euo pipefail
if [[ $# -lt 1 ]]; then
    echo "usage: verify-updater-archive.sh ARTIFACT_DIRECTORY [--unsigned] [--profile dev|prod] [--platform PLATFORM]" >&2
    exit 2
fi
component="$(cd "$(dirname "$0")/.." && pwd)"
directory="$(cd "$1" && pwd)"
unsigned=()
profile=prod
profile_set=false
platform_options=()
shift
while [[ $# -gt 0 ]]; do
    case "$1" in
        --unsigned)
            [[ ${#unsigned[@]} == 0 ]] || exit 2
            unsigned=(--unsigned)
            shift
            ;;
        --profile)
            [[ "$profile_set" == false && $# -ge 2 && ( "$2" == dev || "$2" == prod ) ]] || exit 2
            profile="$2"
            profile_set=true
            shift 2
            ;;
        --platform)
            [[ ${#platform_options[@]} == 0 && $# -ge 2 && ( "$2" == macos-aarch64 || "$2" == linux-x86_64 ) ]] || exit 2
            platform_options=(--platform "$2")
            shift 2
            ;;
        *) echo "unknown archive verification option: $1" >&2; exit 2 ;;
    esac
done
scanner="$component/target/debug/examples/validate-release-archive"
[[ -x "$scanner" && ! -L "$scanner" ]] || { echo "build the trusted updater archive scanner first" >&2; exit 1; }
manifest="$(python3 "$component/scripts/release-info.py" verify "$profile" "$directory" \
    --source-sha "$(git -C "$component" rev-parse HEAD)" "${unsigned[@]}" "${platform_options[@]}")"
platform="$(python3 -c 'import json,sys; print(json.loads(sys.argv[1])["platform"])' "$manifest")"
scanner_options=()
if [[ "$platform" == linux-x86_64 ]]; then
    package="$(python3 -c 'import json,sys; print(json.loads(sys.argv[1])["package"])' "$manifest")"
    archives=("$directory/$package")
    scanner_options=(--linux-bundle --profile "$profile")
else
    [[ "$profile" == prod ]] || { echo "macOS updater scanner supports only Prod packages" >&2; exit 2; }
    archives=("$directory"/*.app.tar.gz)
fi
[[ ${#archives[@]} == 1 && -f "${archives[0]}" && ! -L "${archives[0]}" ]] || { echo "expected exactly one regular app archive" >&2; exit 1; }
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
"$scanner" "${scanner_options[@]}" "$archive" "$expected"
