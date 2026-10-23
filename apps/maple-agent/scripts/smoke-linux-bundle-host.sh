#!/bin/sh
# Check an installed Linux bundle AppRun without an account or credentials.
# Run in the verifier's network-disabled Ubuntu container. Requires coreutils,
# grep and util-linux flock; no Nix or Python is needed inside the container.
set -eu
umask 077
if [ "$#" -ne 1 ] || [ ! -f "$1" ] || [ ! -x "$1" ] || [ -L "$1" ]; then
    echo 'usage: smoke-linux-bundle-host.sh /installed/maple-agent.app/AppRun' >&2
    exit 2
fi
apprun=$(CDPATH= cd -- "$(dirname -- "$1")" && pwd)/$(basename -- "$1")
smoke_timeout=${MAPLE_AGENT_SMOKE_TIMEOUT:-30}
case "$smoke_timeout" in ''|*[!0-9]*|0) exit 2 ;; esac
smoke_path=${PATH:-/usr/bin:/bin}
scratch=$(mktemp -d /tmp/maple-agent-bundle-smoke.XXXXXX)
scratch=$(CDPATH= cd -- "$scratch" && pwd -P)
endpoint=''
owner=''
mkdir "$scratch/home" "$scratch/config" "$scratch/data" "$scratch/cache" "$scratch/tmp"
cd -- "$scratch"

agent() {
    agent_timeout=$smoke_timeout
    # Short cancellation probes must not also expire cold identity discovery.
    if [ "${1:-}" = --build-info ] && [ "$agent_timeout" -lt 10 ]; then
        agent_timeout=10
    fi
    timeout "$agent_timeout" env -i PATH="$smoke_path" HOME="$scratch/home" \
        XDG_CONFIG_HOME="$scratch/config" XDG_DATA_HOME="$scratch/data" \
        XDG_CACHE_HOME="$scratch/cache" TMPDIR="$scratch/tmp" \
        MAPLE_DISABLE_UPDATE_CHECK=1 "$apprun" "$@"
}
locked() {
    [ -f "$owner" ] && ! flock -n "$owner" true
}
fail() { echo "$1" >&2; exit 1; }
diagnostics() {
    [ "${MAPLE_AGENT_SMOKE_DIAGNOSTICS:-0}" = 1 ] || return 0
    # This probe passes no inherited credentials and uses empty private state.
    # Opt-in CI output is bounded and strips escapes while allowing whitespace.
    echo "Isolated signed-out ACP exit status: $result" >&2
    for diagnostic in "$error" "$scratch/data/$namespace/store-v1/logs/maple-agent.log"; do
        [ -f "$diagnostic" ] || continue
        echo 'Bounded isolated startup diagnostics:' >&2
        tail -c 8192 "$diagnostic" | head -c 8192 | LC_ALL=C tr -cd '\11\12\15\40-\176' >&2
        echo >&2
    done
}
cleanup() {
    status=$?
    trap - EXIT INT TERM HUP
    safe=true
    if [ -n "$endpoint" ]; then
        agent host stop >/dev/null 2>&1 || true
        deadline=$(($(date +%s) + smoke_timeout))
        while locked || [ -S "$endpoint/ipc.sock" ]; do
            if [ "$(date +%s)" -ge "$deadline" ]; then safe=false; break; fi
            sleep 0.05
        done
    fi
    if "$safe"; then
        rm -rf -- "$scratch"
        if [ -n "$endpoint" ]; then rm -rf -- "$endpoint"; fi
    else
        echo 'The isolated host did not release ownership; probe state was retained' >&2
        status=1
    fi
    if [ "$status" -eq 0 ]; then
        echo 'Verified Linux bundle AppRun starts one host, preserves the signed-out ACP guard across two clients, and releases ownership on stop'
    fi
    exit "$status"
}
trap cleanup EXIT
trap 'exit 130' INT
trap 'exit 143' TERM
trap 'exit 129' HUP

# These roots contain only controlled ASCII characters. This is the exact JSON
# storage owner identity in host/ipc.rs, without scanning another host's files.
agent --build-info > "$scratch/build-info.json" 2>/dev/null || fail 'Could not read AppRun build identity'
namespace=$(sed -n 's/.*"data_namespace"[[:space:]]*:[[:space:]]*"\([^"]*\)".*/\1/p' "$scratch/build-info.json")
case "$namespace" in maple-agent|maple-agent-dev|maple-agent-prod) ;; *) fail 'Unexpected Agent data namespace' ;; esac
mkdir "$scratch/config/$namespace" "$scratch/data/$namespace"
store="$scratch/data/$namespace/store-v1"
owner="$store/owner.lock"
uid=$(id -u)
identity=$(printf '["%s","%s","%s"]' "$store" "$namespace" "$uid" | sha256sum | cut -c1-24)
candidate="/tmp/maple-agent-$uid/$identity"
[ ! -e "$candidate" ] || fail 'The smoke profile unexpectedly already has an endpoint'
endpoint=$candidate

signed_out_client() {
    number=$1
    output="$scratch/output-$number"
    error="$scratch/error-$number"
    result=0
    # Both fixed unauthenticated gates exercise on-demand spawn and self-launch:
    # a fresh store may require setup or open without a saved sign-in.
    agent acp < /dev/null > "$output" 2> "$error" || result=$?
    if [ "$result" -eq 124 ]; then
        diagnostics
        fail 'The signed-out ACP probe timed out'
    fi
    if [ "$result" -ne 1 ] || ! {
        grep -Fxq 'Maple needs local store setup. Open Maple to continue.' "$error" ||
        grep -Fxq 'Maple ACP requires sign-in. Sign in through Maple Desktop or `maple-agent login` first.' "$error"
    }; then
        diagnostics
        fail 'The ACP probe did not return an expected signed-out guard'
    fi
}
socket_identity() {
    [ -S "$endpoint/ipc.sock" ] || fail 'The isolated host did not retain its IPC socket'
    ls -di "$endpoint/ipc.sock" | awk '{print $1}'
}

signed_out_client 1
inode=$(socket_identity)
locked || fail 'The host did not retain ownership after the signed-out ACP client exited'
signed_out_client 2
[ "$(socket_identity)" = "$inode" ] && locked || fail 'The second ACP client did not reuse the same host'
# The EXIT trap stops this isolated host and checks its lock and socket before
# removing state. A failed stop cannot produce a successful verification.
