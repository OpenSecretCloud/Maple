#!/usr/bin/env bash

native_ci_dir="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
native_root="$(cd "$native_ci_dir/../.." && pwd)"
repo_root="$(cd "$native_root/.." && pwd)"

function require_env() {
  local missing=()
  for name in "$@"; do
    if [[ -z "${!name:-}" ]]; then
      missing+=("$name")
    fi
  done

  if (( ${#missing[@]} > 0 )); then
    printf 'Missing required environment variables: %s\n' "${missing[*]}" >&2
    exit 1
  fi
}

function require_command() {
  local missing=()
  for name in "$@"; do
    if ! command -v "$name" >/dev/null 2>&1; then
      missing+=("$name")
    fi
  done

  if (( ${#missing[@]} > 0 )); then
    printf 'Missing required commands: %s\n' "${missing[*]}" >&2
    exit 1
  fi
}

function maple_version() {
  awk -F '"' '/^version = "/ { print $2; exit }' "$native_root/rust/Cargo.toml"
}

function api_url_for_env() {
  case "$1" in
    local) echo "http://0.0.0.0:3000" ;;
    dev) echo "https://enclave.secretgpt.ai" ;;
    prod) echo "https://enclave.trymaple.ai" ;;
    *)
      printf 'Unsupported MAPLE_API_ENV: %s\n' "$1" >&2
      exit 1
      ;;
  esac
}

function ensure_dir() {
  mkdir -p "$1"
}

function tauri_sign_artifact() {
  local artifact="$1"

  if [[ -z "${TAURI_SIGNING_PRIVATE_KEY:-}" || -z "${TAURI_SIGNING_PRIVATE_KEY_PASSWORD:-}" ]]; then
    return 0
  fi

  require_command cargo-tauri

  local key_file
  key_file="$(mktemp)"
  printf '%s' "$TAURI_SIGNING_PRIVATE_KEY" > "$key_file"

  cargo-tauri signer sign \
    --private-key-path "$key_file" \
    --password "$TAURI_SIGNING_PRIVATE_KEY_PASSWORD" \
    "$artifact"

  rm -f "$key_file"

  if [[ ! -f "${artifact}.sig" ]]; then
    printf 'Expected detached signature at %s.sig\n' "$artifact" >&2
    exit 1
  fi
}

function maple_artifact_version() {
  if [[ -n "$maple_prerelease_label" ]]; then
    printf '%s-%s.%s' "$maple_version_value" "$maple_prerelease_label" "$maple_build_number"
  else
    printf '%s' "$maple_version_value"
  fi
}

function maple_deb_version() {
  if [[ -n "$maple_prerelease_label" ]]; then
    printf '%s~%s.%s' "$maple_version_value" "$maple_prerelease_label" "$maple_build_number"
  else
    printf '%s' "$maple_version_value"
  fi
}

run_number="${GITHUB_RUN_NUMBER:-1}"
run_attempt="${GITHUB_RUN_ATTEMPT:-1}"

maple_api_env="${MAPLE_API_ENV:-dev}"
maple_version_value="${MAPLE_VERSION:-$(maple_version)}"
maple_build_number="${MAPLE_BUILD_NUMBER:-$((run_number * 10 + run_attempt))}"
if [[ "${MAPLE_PRERELEASE_LABEL+x}" == "x" ]]; then
  maple_prerelease_label="$MAPLE_PRERELEASE_LABEL"
else
  maple_prerelease_label="beta"
fi
open_secret_api_url="${OPEN_SECRET_API_URL:-$(api_url_for_env "$maple_api_env")}"
ci_build_root="${MAPLE_CI_BUILD_ROOT:-${RUNNER_TEMP:-$native_root/target}/maple-ci}"
