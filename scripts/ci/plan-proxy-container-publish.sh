#!/usr/bin/env bash
set -euo pipefail

fail() {
  echo "Proxy container publish plan failed: $*" >&2
  exit 1
}

[ "$#" -eq 4 ] || fail "usage: $0 VERSION PREVIOUS_VERSION UNBACKFILLED_BASELINE TAGS_JSON"

version="$1"
previous_version="$2"
unbackfilled_baseline="$3"
tags_json="$4"
semver_regex='^(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)$'

[[ "${version}" =~ ${semver_regex} ]] || fail "version must be exact canonical X.Y.Z"
[ -z "${previous_version}" ] || [[ "${previous_version}" =~ ${semver_regex} ]] || \
  fail "previous version must be empty or exact canonical X.Y.Z"
[[ "${unbackfilled_baseline}" =~ ${semver_regex} ]] || \
  fail "unbackfilled baseline must be exact canonical X.Y.Z"
[ -f "${tags_json}" ] || fail "tag inventory does not exist: ${tags_json}"
jq -e '.tags | type == "array" and all(.[]; type == "string")' \
  "${tags_json}" >/dev/null || fail "tag inventory must contain a string array"

major="${version%%.*}"
minor="${version%.*}"
highest="$(
  jq -r --arg regex "${semver_regex}" '.tags[] | select(test($regex))' \
    "${tags_json}" | sort -uV | tail -n 1
)"

if [ -n "${highest}" ]; then
  newest="$(printf '%s\n%s\n' "${highest}" "${version}" | sort -V | tail -n 1)"
  [ "${newest}" = "${version}" ] || \
    fail "refusing to publish ${version} after newer ${highest}"
fi

if [ -n "${previous_version}" ]; then
  newest_release_version="$(
    printf '%s\n%s\n' "${previous_version}" "${version}" | sort -V | tail -n 1
  )"
  [ "${newest_release_version}" = "${version}" ] || \
    fail "refusing release rollback from ${previous_version} to ${version}"
fi

emit_plan() {
  local publish="$1"
  local reconcile="$2"
  local reason="$3"
  printf 'publish=%s\nreconcile=%s\nreason=%s\nproxy_version=%s\nmajor=%s\nminor=%s\nregistry_version=%s\n' \
    "${publish}" "${reconcile}" "${reason}" "${version}" "${major}" "${minor}" "${highest}"
}

if jq -e --arg version "${version}" '.tags | index($version) != null' \
  "${tags_json}" >/dev/null; then
  emit_plan false true already-published
  exit 0
fi

# 0.3.3 was intentionally absorbed without a GHCR backfill. Keep that policy
# explicit so a later unchanged Maple release cannot accidentally publish it.
if [ "${version}" = "${unbackfilled_baseline}" ]; then
  emit_plan false false baseline
  exit 0
fi

[ -n "${previous_version}" ] || \
  fail "previous version is missing for non-baseline ${version}"

if [ "${version}" = "${previous_version}" ]; then
  emit_plan true false missing-version
else
  emit_plan true false new-version
fi
