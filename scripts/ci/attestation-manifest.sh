#!/usr/bin/env bash
set -euo pipefail

source "$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)/_common.sh"

out="${1:-}"
final_manifest="${2:-}"

if [ -z "${out}" ] || [ -z "${final_manifest}" ]; then
  echo "usage: attestation-manifest.sh <out.sha256> <final-artifacts.sha256> [proof-file.sha256 ...]" >&2
  exit 2
fi
shift 2

if [ ! -f "${final_manifest}" ]; then
  echo "Final artifact manifest does not exist: ${final_manifest}" >&2
  exit 1
fi

validate_sha256sum_manifest() {
  local manifest="$1"

  awk '
    NF < 2 || $1 !~ /^[0-9a-fA-F]{64}$/ {
      printf("Invalid sha256sum manifest line in %s: %s\n", FILENAME, $0) > "/dev/stderr"
      exit 1
    }
  ' "${manifest}"
}

mkdir -p "$(dirname "${out}")"
validate_sha256sum_manifest "${final_manifest}"
cp "${final_manifest}" "${out}"

proof_files=()
for file in "$@"; do
  if [ -f "${file}" ]; then
    proof_files+=("${file}")
  fi
done

if [ "${#proof_files[@]}" -gt 0 ]; then
  printf '%s\0' "${proof_files[@]}" \
    | LC_ALL=C sort -z \
    | while IFS= read -r -d '' file; do
        digest="$(sha256_file "${file}" | awk '{ print $1 }')"
        printf '%s  %s\n' "${digest}" "$(repo_relative_path "${file}")"
      done >> "${out}"
fi

# actions/attest@v4 splits checksum files on the runner platform EOL.
# Windows runners need CRLF here even when Git Bash generated the file.
case "$(host_os)" in
  mingw* | msys* | cygwin*)
    tmp="$(mktemp)"
    awk '{ sub(/\r$/, ""); printf "%s\r\n", $0 }' "${out}" > "${tmp}"
    mv "${tmp}" "${out}"
    ;;
esac

cat "${out}"
