#!/bin/sh
# Run from an extracted, verified release bundle. No sudo and no network access.
set -eu
bundle=$(CDPATH= cd -- "$(dirname -- "$0")" && pwd -P)
if ! command -v python3 >/dev/null 2>&1; then
    echo "Python 3 is required for initial installation only." >&2
    exit 1
fi
exec python3 "$bundle/usr/share/maple-agent/install.py" --bundle "$bundle" "$@"
