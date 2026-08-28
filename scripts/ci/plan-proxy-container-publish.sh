#!/usr/bin/env bash
set -euo pipefail

fail() {
  echo "Proxy container publish plan failed: $*" >&2
  exit 1
}

[ "$#" -eq 3 ] || fail "usage: $0 VERSION PREVIOUS_VERSION TAGS_JSON"

version="$1"
previous_version="$2"
tags_json="$3"
semver_regex='^(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)$'

[[ "${version}" =~ ${semver_regex} ]] || fail "version must be exact canonical X.Y.Z"
[ -z "${previous_version}" ] || [[ "${previous_version}" =~ ${semver_regex} ]] || \
  fail "previous version must be empty or exact canonical X.Y.Z"
[ -f "${tags_json}" ] || fail "tag inventory does not exist: ${tags_json}"
jq -e '.tags | type == "array" and all(.[]; type == "string")' \
  "${tags_json}" >/dev/null || fail "tag inventory must contain a string array"

major="${version%%.*}"
minor="${version%.*}"
highest="$(
  jq -r --arg regex "${semver_regex}" '.tags[] | select(test($regex))' \
    "${tags_json}" | sort -uV | tail -n 1
)"

if [ -z "${previous_version}" ]; then
  printf 'publish=false\nreason=baseline\nproxy_version=%s\nmajor=%s\nminor=%s\nregistry_version=%s\n' \
    "${version}" "${major}" "${minor}" "${highest}"
  exit 0
fi

if [ "${version}" = "${previous_version}" ]; then
  printf 'publish=false\nreason=unchanged\nproxy_version=%s\nmajor=%s\nminor=%s\nregistry_version=%s\n' \
    "${version}" "${major}" "${minor}" "${highest}"
  exit 0
fi

newest_release_version="$(
  printf '%s\n%s\n' "${previous_version}" "${version}" | sort -V | tail -n 1
)"
[ "${newest_release_version}" = "${version}" ] || \
  fail "refusing release rollback from ${previous_version} to ${version}"

if jq -e --arg version "${version}" '.tags | index($version) != null' \
  "${tags_json}" >/dev/null; then
  printf 'publish=false\nreason=already-published\nproxy_version=%s\nmajor=%s\nminor=%s\nregistry_version=%s\n' \
    "${version}" "${major}" "${minor}" "${highest}"
  exit 0
fi

if [ -n "${highest}" ]; then
  newest="$(printf '%s\n%s\n' "${highest}" "${version}" | sort -V | tail -n 1)"
  [ "${newest}" = "${version}" ] || \
    fail "refusing to publish ${version} after newer ${highest}"
fi

printf 'publish=true\nreason=new-version\nproxy_version=%s\nmajor=%s\nminor=%s\nregistry_version=%s\n' \
  "${version}" "${major}" "${minor}" "${highest}"
