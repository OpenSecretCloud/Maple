#!/usr/bin/env bash
set -euo pipefail

script_dir="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
verifier="${script_dir}/verify-release-artifacts.sh"
temp_root="$(mktemp -d)"
trap 'rm -rf "${temp_root}"' EXIT HUP INT TERM

passed=0

pass() {
  passed=$((passed + 1))
  printf 'ok %d - %s\n' "${passed}" "$1"
}

fail() {
  echo "not ok - $*" >&2
  exit 1
}

expect_failure() {
  local label="$1"
  shift

  if output="$("$@" 2>&1)"; then
    printf '%s\n' "${output}" >&2
    fail "${label}"
  fi
  pass "${label}"
}

make_fixture() {
  local directory="$1"
  local stage="${directory}/stage"

  mkdir -p "${directory}" "${stage}"
  printf '#!/usr/bin/env sh\nprintf "maple-proxy fixture\\n"\n' > "${stage}/maple-proxy"
  printf 'fixture windows binary\n' > "${stage}/maple-proxy.exe"

  tar -C "${stage}" -czf "${directory}/maple-proxy-linux-aarch64.tar.gz" maple-proxy
  tar -C "${stage}" -czf "${directory}/maple-proxy-linux-x86_64.tar.gz" maple-proxy
  tar -C "${stage}" -czf "${directory}/maple-proxy-macos-aarch64.tar.gz" maple-proxy
  (
    cd "${stage}"
    zip -q "${directory}/maple-proxy-windows-x86_64.zip" maple-proxy.exe
  )
  (
    cd "${directory}"
    sha256sum \
      maple-proxy-linux-aarch64.tar.gz \
      maple-proxy-linux-x86_64.tar.gz \
      maple-proxy-macos-aarch64.tar.gz \
      maple-proxy-windows-x86_64.zip \
      > maple-proxy-release-final.sha256
  )
  rm -rf "${stage}"
}

valid="${temp_root}/valid"
make_fixture "${valid}"
"${verifier}" "${valid}" proxy >/dev/null
pass "accepts the complete proxy release asset set"

missing="${temp_root}/missing"
cp -R "${valid}" "${missing}"
rm "${missing}/maple-proxy-linux-aarch64.tar.gz"
expect_failure "rejects a missing proxy release asset" "${verifier}" "${missing}" proxy

tampered="${temp_root}/tampered"
cp -R "${valid}" "${tampered}"
printf 'tampered\n' >> "${tampered}/maple-proxy-macos-aarch64.tar.gz"
expect_failure "rejects a proxy release hash mismatch" "${verifier}" "${tampered}" proxy

extra_member="${temp_root}/extra-member"
cp -R "${valid}" "${extra_member}"
mkdir -p "${extra_member}/stage"
printf 'proxy\n' > "${extra_member}/stage/maple-proxy"
printf 'unexpected\n' > "${extra_member}/stage/README.txt"
tar -C "${extra_member}/stage" -czf "${extra_member}/maple-proxy-linux-x86_64.tar.gz" maple-proxy README.txt
(
  cd "${extra_member}"
  sha256sum \
    maple-proxy-linux-aarch64.tar.gz \
    maple-proxy-linux-x86_64.tar.gz \
    maple-proxy-macos-aarch64.tar.gz \
    maple-proxy-windows-x86_64.zip \
    > maple-proxy-release-final.sha256
)
rm -rf "${extra_member}/stage"
expect_failure "rejects an archive with extra members" "${verifier}" "${extra_member}" proxy

unexpected="${temp_root}/unexpected"
cp -R "${valid}" "${unexpected}"
cp "${unexpected}/maple-proxy-linux-x86_64.tar.gz" "${unexpected}/maple-proxy-linux-riscv64.tar.gz"
expect_failure "rejects an unexpected proxy release archive" "${verifier}" "${unexpected}" proxy

printf '1..%d\n' "${passed}"
