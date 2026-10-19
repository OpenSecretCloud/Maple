#!/usr/bin/env bash
# Exercise the optional credential boundary without AWS, VSOCK or real secrets.
set -euo pipefail

component_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
entrypoint_path="${ENTRYPOINT_UNDER_TEST:-$component_root/entrypoint.sh}"
work_dir="$(mktemp -d)"
trap 'rm -rf "$work_dir"' EXIT
sed -n '/^# BEGIN OPTIONAL_MAPLE_INFERENCE_CREDENTIAL$/,/^# END OPTIONAL_MAPLE_INFERENCE_CREDENTIAL$/p' \
    "$entrypoint_path" > "$work_dir/credential.sh"
# shellcheck source=/dev/null
source "$work_dir/credential.sh"

log() { printf '%s\n' "$*" >> "$work_dir/log"; }
vsock_request() {
    [ "$1" = 3 ] && [ "$2" = 8003 ]
    printf '%s' "$3" > "$work_dir/request"
    if [ "$fixture" = missing ]; then return 1; fi
    printf '%s' "$fixture_response"
}
python3() {
    [ "$1" = /app/vsock_helper.py ]
    shift
    vsock_request "$@"
}
timeout() {
    [ "$1" = -k ] && [ "$2" = 2 ] && [ "$3" = 15 ]
    shift 3
    if [ "$fixture" = timeout ]; then return 124; fi
    "$@"
}
kmstool_enclave_cli() {
    if [ "$fixture" = decrypt_failure ]; then return 1; fi
    printf 'PLAINTEXT: %s\n' "$encoded"
}

region=synthetic-region
access_key_id=synthetic-access
secret_access_key=synthetic-secret
session_token=synthetic-session
TINFOIL_API_KEY=synthetic-shared-key
fixture_response='{"response_type":"secret","response_value":"{\"api_key\":\"synthetic-ciphertext\"}"}'
encoded="$(printf '%s' synthetic-dedicated-key | base64 | tr -d '\n')"
fixture=success

for mode in prod preview dev custom; do
    APP_MODE="$mode" ENV_NAME=synthetic-preview
    get_maple_inference_api_key_secret >/dev/null
    case "$mode" in
        prod) expected=maple_inference_prod_api_key ;;
        preview) expected=maple_inference_preview1_api_key ;;
        dev) expected=maple_inference_dev_api_key ;;
        custom) expected=maple_inference_synthetic-preview_api_key ;;
    esac
    [ "$(jq -r .key_name "$work_dir/request")" = "$expected" ]
done
APP_MODE=custom ENV_NAME=""
if get_maple_inference_api_key_secret >/dev/null; then
    echo "custom mode accepted an absent environment name" >&2
    exit 1
fi

APP_MODE=dev ENV_NAME=""
for fixture in missing invalid_response timeout decrypt_failure invalid_base64 empty success; do
    # A host-inherited or shared key must never survive a failed optional load.
    MAPLE_INFERENCE_API_KEY=synthetic-host-inherited-key
    fixture_response='{"response_type":"secret","response_value":"{\"api_key\":\"synthetic-ciphertext\"}"}'
    encoded="$(printf '%s' synthetic-dedicated-key | base64 | tr -d '\n')"
    case "$fixture" in
        invalid_response) fixture_response='{"response_type":"error","response_value":"missing"}' ;;
        invalid_base64) encoded='not-base64!' ;;
        empty) encoded='' ;;
    esac
    load_optional_maple_inference_api_key
    if [ "$fixture" = success ]; then
        [ "$MAPLE_INFERENCE_API_KEY" = synthetic-dedicated-key ]
    else
        [ -z "$MAPLE_INFERENCE_API_KEY" ]
    fi
    [ "$TINFOIL_API_KEY" = synthetic-shared-key ]
done

if grep -Eq 'synthetic-(shared|dedicated|host-inherited)-key|synthetic-ciphertext' "$work_dir/log"; then
    echo "optional credential helper logged credential material" >&2
    exit 1
fi
echo "optional Maple inference credential tests passed"
