#!/usr/bin/env bash
set -euo pipefail

usage() {
  cat >&2 <<'EOF'
usage: proxy-release.sh <output-dir> <archive-name>

Builds maple-proxy for the current native runner, checks its --version output,
and packages the binary under the stable release asset name for that runner.
EOF
}

output_dir="${1:-}"
archive_name="${2:-}"
if [ -z "${output_dir}" ] || [ -z "${archive_name}" ] || [ "$#" -ne 2 ]; then
  usage
  exit 2
fi

script_dir="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
repo_root="$(cd "${script_dir}/../.." && pwd -P)"
manifest="${repo_root}/proxy/Cargo.toml"
target_dir="${repo_root}/proxy/target"
host_os="$(uname -s)"
host_arch="$(uname -m)"

case "${host_os}:${host_arch}" in
  Linux:x86_64)
    expected_archive="maple-proxy-linux-x86_64.tar.gz"
    binary_name="maple-proxy"
    ;;
  Linux:aarch64 | Linux:arm64)
    expected_archive="maple-proxy-linux-aarch64.tar.gz"
    binary_name="maple-proxy"
    ;;
  Darwin:arm64 | Darwin:aarch64)
    expected_archive="maple-proxy-macos-aarch64.tar.gz"
    binary_name="maple-proxy"
    ;;
  MINGW*:x86_64 | MSYS*:x86_64 | CYGWIN*:x86_64)
    expected_archive="maple-proxy-windows-x86_64.zip"
    binary_name="maple-proxy.exe"
    ;;
  *)
    echo "Unsupported maple-proxy release host: ${host_os} ${host_arch}" >&2
    exit 1
    ;;
esac

if [ "${archive_name}" != "${expected_archive}" ]; then
  echo "Archive ${archive_name} does not match native host ${host_os} ${host_arch}; expected ${expected_archive}." >&2
  exit 1
fi

proxy_version="$(awk '
  /^\[package\]$/ { in_package = 1; next }
  /^\[/ && in_package { exit }
  in_package && /^version[[:space:]]*=/ {
    value = $0
    sub(/^[^=]*=[[:space:]]*"/, "", value)
    sub(/"[[:space:]]*$/, "", value)
    print value
    exit
  }
' "${manifest}")"
if [ -z "${proxy_version}" ]; then
  echo "Could not read maple-proxy package version." >&2
  exit 1
fi

cargo build --locked --manifest-path "${manifest}" --release --bin maple-proxy

binary="${target_dir}/release/${binary_name}"
if [ ! -f "${binary}" ]; then
  echo "Built maple-proxy binary is missing: ${binary}" >&2
  exit 1
fi

actual_version="$("${binary}" --version)"
expected_version="maple-proxy ${proxy_version}"
if [ "${actual_version}" != "${expected_version}" ]; then
  echo "Unexpected maple-proxy version output." >&2
  echo "expected=${expected_version}" >&2
  echo "actual=${actual_version}" >&2
  exit 1
fi

mkdir -p "${output_dir}"
output_dir="$(cd "${output_dir}" && pwd -P)"
archive="${output_dir}/${archive_name}"
stage_dir="$(mktemp -d)"
trap 'rm -rf "${stage_dir}"' EXIT HUP INT TERM
cp "${binary}" "${stage_dir}/${binary_name}"

case "${archive_name}" in
  *.tar.gz)
    tar -C "${stage_dir}" -czf "${archive}" "${binary_name}"
    ;;
  *.zip)
    command -v 7z >/dev/null 2>&1 || {
      echo "7z is required to package the Windows proxy binary." >&2
      exit 1
    }
    (
      cd "${stage_dir}"
      7z a -tzip "${archive}" "${binary_name}" >/dev/null
    )
    ;;
  *)
    echo "Unsupported proxy archive format: ${archive_name}" >&2
    exit 1
    ;;
esac

printf 'built-proxy-release-asset  %s  %s\n' "${actual_version}" "${archive}"
