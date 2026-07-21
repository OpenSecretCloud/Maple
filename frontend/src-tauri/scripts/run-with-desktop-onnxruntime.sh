#!/usr/bin/env bash
set -euo pipefail

if [ "$#" -eq 0 ]; then
  echo "Usage: $0 <command> [args...]" >&2
  exit 2
fi

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"

case "$(uname -s)" in
  Darwin)
    provider="${SCRIPT_DIR}/provide-macos-onnxruntime.sh"
    ;;
  Linux)
    provider="${SCRIPT_DIR}/provide-linux-onnxruntime.sh"
    ;;
  MINGW* | MSYS* | CYGWIN*)
    provider="${SCRIPT_DIR}/provide-windows-onnxruntime.sh"
    ;;
  *)
    echo "Unsupported desktop platform for ONNX Runtime provisioning: $(uname -s)" >&2
    exit 1
    ;;
esac

ort_env="$("${provider}")"
ort_dylib_path="$(printf '%s\n' "${ort_env}" | sed -n 's/^ORT_DYLIB_PATH=//p')"
if [ -z "${ort_dylib_path}" ]; then
  echo "The ONNX Runtime provider did not return ORT_DYLIB_PATH." >&2
  exit 1
fi

export ORT_DYLIB_PATH="${ort_dylib_path}"
exec "$@"
