#!/usr/bin/env bash

set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
entrypoint_path="${ENTRYPOINT_UNDER_TEST:-$repo_root/entrypoint.sh}"
work_dir="$(mktemp -d)"
trap 'rm -rf "$work_dir"' EXIT

sed -n \
    '/^# BEGIN ENCLAVE_ENTROPY_PREFLIGHT$/,/^# END ENCLAVE_ENTROPY_PREFLIGHT$/p' \
    "$entrypoint_path" > "$work_dir/preflight.sh"

# The production entrypoint defines this before the extracted function.
log() {
    printf '%s\n' "$*" >/dev/null
}

# shellcheck source=/dev/null
source "$work_dir/preflight.sh"

gate_line="$(grep -nF 'verify_enclave_entropy_for_mode "$APP_MODE" || exit 1' "$entrypoint_path" | cut -d: -f1)"
log_forwarder_line="$(grep -nF 'exec > >(log_forwarder) 2>&1' "$entrypoint_path" | cut -d: -f1)"
if [ -z "$gate_line" ] || [ -z "$log_forwarder_line" ] || [ "$gate_line" -ge "$log_forwarder_line" ]; then
    echo "expected the entropy preflight to run before VSOCK log forwarding" >&2
    exit 1
fi

pass_getrandom="$work_dir/pass-getrandom"
cat > "$pass_getrandom" <<'EOF'
#!/usr/bin/env bash
exit 0
EOF
chmod +x "$pass_getrandom"

fail_getrandom="$work_dir/fail-getrandom"
cat > "$fail_getrandom" <<'EOF'
#!/usr/bin/env bash
exit 1
EOF
chmod +x "$fail_getrandom"

rng_current="$work_dir/rng_current"
printf 'nsm-hwrng\n' > "$rng_current"

verify_enclave_entropy_readiness /dev/null "$rng_current" "$pass_getrandom"
verify_enclave_entropy_for_mode local "$work_dir/not-a-device" "$work_dir/missing-rng-current" "$fail_getrandom"

if verify_enclave_entropy_for_mode dev "$work_dir/not-a-device" "$rng_current" "$pass_getrandom"; then
    echo "expected dev mode to enforce the entropy preflight" >&2
    exit 1
fi

if verify_enclave_entropy_readiness "$work_dir/not-a-device" "$rng_current" "$pass_getrandom"; then
    echo "expected a missing NSM device to fail" >&2
    exit 1
fi

printf 'virtio-rng\n' > "$rng_current"
if verify_enclave_entropy_readiness /dev/null "$rng_current" "$pass_getrandom"; then
    echo "expected an unexpected rng_current value to fail" >&2
    exit 1
fi

if verify_enclave_entropy_readiness /dev/null "$work_dir/missing-rng-current" "$pass_getrandom"; then
    echo "expected a missing rng_current file to fail" >&2
    exit 1
fi

printf 'nsm-hwrng\n' > "$rng_current"
if verify_enclave_entropy_readiness /dev/null "$rng_current" "$fail_getrandom"; then
    echo "expected a failed getrandom check to fail" >&2
    exit 1
fi

echo "entrypoint entropy preflight tests passed"
