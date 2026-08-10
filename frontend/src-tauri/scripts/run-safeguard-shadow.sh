#!/usr/bin/env bash
set -euo pipefail

if [[ -n "${TINFOIL_API_KEY+x}" ]]; then
  echo "Refusing an inherited TINFOIL_API_KEY; unset it and use the secure prompt." >&2
  exit 2
fi

if [[ ! -t 0 ]]; then
  echo "The safeguard runner requires an interactive terminal for the API-key prompt." >&2
  exit 2
fi

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"

case "$(uname -s)" in
  Darwin)
    provider="${SCRIPT_DIR}/provide-macos-onnxruntime.sh"
    binary_name="maple"
    ;;
  Linux)
    provider="${SCRIPT_DIR}/provide-linux-onnxruntime.sh"
    binary_name="maple"
    ;;
  MINGW* | MSYS* | CYGWIN*)
    provider="${SCRIPT_DIR}/provide-windows-onnxruntime.sh"
    binary_name="maple.exe"
    ;;
  *)
    echo "Unsupported desktop platform: $(uname -s)" >&2
    exit 1
    ;;
esac

frontend_dir="$(cd "${SCRIPT_DIR}/../.." && pwd)"
if [[ ! -x "${frontend_dir}/node_modules/.bin/tauri" ]]; then
  echo "Frontend dependencies are unavailable; run 'nix develop -c just install' first." >&2
  exit 2
fi
build_command=(bun tauri build --debug --no-bundle)
if [[ -f "${frontend_dir}/../.local/tauri-workspace.json" ]]; then
  build_command+=(--config ../.local/tauri-workspace.json)
fi

# Tauri copies the final artifact back to this checkout even though the Nix
# shell shares Rust intermediates. Building here and using the managed-workspace
# config avoids launching another checkout's binary or production app identity.
(
  cd "${frontend_dir}"
  "${SCRIPT_DIR}/run-with-desktop-onnxruntime.sh" "${build_command[@]}"
)

maple_binary="${SCRIPT_DIR}/../target/debug/${binary_name}"
if [[ ! -x "${maple_binary}" ]]; then
  echo "The safeguard runner did not produce the expected checkout-local debug binary." >&2
  exit 2
fi

# Complete all provisioning before reading the secret so no build hook or
# helper subprocess can inherit it. After the prompt this shell only exports
# the key and immediately replaces itself with Maple.
ort_env="$("${provider}")"
ort_dylib_path="$(printf '%s\n' "${ort_env}" | sed -n 's/^ORT_DYLIB_PATH=//p')"
if [[ -z "${ort_dylib_path}" ]]; then
  echo "The ONNX Runtime provider did not return ORT_DYLIB_PATH." >&2
  exit 1
fi

IFS= read -r -s -p "Tinfoil API key: " safeguard_key
printf '\n'
if [[ -z "${safeguard_key}" ]]; then
  echo "A nonblank Tinfoil API key is required." >&2
  exit 2
fi

export ORT_DYLIB_PATH="${ort_dylib_path}"
export MAPLE_SAFEGUARD_SHADOW=1
export TINFOIL_API_KEY="${safeguard_key}"
unset safeguard_key
exec "${maple_binary}"
