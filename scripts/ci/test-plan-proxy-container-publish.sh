#!/usr/bin/env bash
set -euo pipefail

script_dir="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
planner="${script_dir}/plan-proxy-container-publish.sh"
temp_dir="$(mktemp -d)"
trap 'rm -rf "${temp_dir}"' EXIT HUP INT TERM

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

tags="${temp_dir}/tags.json"
cat >"${tags}" <<'JSON'
{"tags":["latest","master","sha-7b89422","0","0.3","0.3.1","0.3.2"]}
JSON

expected='publish=true
reconcile=false
reason=new-version
proxy_version=0.3.3
major=0
minor=0.3
registry_version=0.3.2'
actual="$(bash "${planner}" 0.3.3 0.3.2 0.3.0 "${tags}")"
[ "${actual}" = "${expected}" ] || fail "new version publish plan did not match"
pass "publishes a strictly newer exact version"

expected='publish=false
reconcile=true
reason=already-published
proxy_version=0.3.2
major=0
minor=0.3
registry_version=0.3.2'
actual="$(bash "${planner}" 0.3.2 0.3.1 0.3.0 "${tags}")"
[ "${actual}" = "${expected}" ] || fail "existing version plan did not match"
pass "reconciles an existing exact version without rebuilding it"

printf '{"tags":["latest","0.3.2"]}\n' >"${tags}"
expected='publish=true
reconcile=false
reason=missing-version
proxy_version=0.3.3
major=0
minor=0.3
registry_version=0.3.2'
actual="$(bash "${planner}" 0.3.3 0.3.3 0.3.0 "${tags}")"
[ "${actual}" = "${expected}" ] || fail "missing unchanged version plan did not match"
pass "recovers a missing non-baseline exact version"

expected='publish=false
reconcile=false
reason=baseline
proxy_version=0.3.3
major=0
minor=0.3
registry_version=0.3.2'
actual="$(bash "${planner}" 0.3.3 0.3.3 0.3.3 "${tags}")"
[ "${actual}" = "${expected}" ] || fail "unchanged baseline plan did not match"
pass "does not backfill the explicit baseline on a later release"

actual="$(bash "${planner}" 0.3.3 '' 0.3.3 "${tags}")"
[ "${actual}" = "${expected}" ] || fail "baseline release plan did not match"
pass "does not publish the first in-tree baseline"

expect_failure "rejects a release version rollback" bash "${planner}" 0.3.0 0.3.1 0.3.3 "${tags}"
expect_failure "rejects a registry version rollback" bash "${planner}" 0.3.1 0.3.0 0.3.3 "${tags}"
expect_failure "rejects a non-canonical version" bash "${planner}" 00.3.3 0.3.2 0.3.3 "${tags}"
expect_failure "rejects a non-canonical baseline" bash "${planner}" 0.3.3 0.3.2 v0.3.3 "${tags}"

printf '{"tags":"latest"}\n' >"${tags}"
expect_failure "rejects a malformed registry response" bash "${planner}" 0.3.3 0.3.2 0.3.3 "${tags}"

printf '{"tags":[]}\n' >"${tags}"
expected='publish=true
reconcile=false
reason=new-version
proxy_version=1.0.0
major=1
minor=1.0
registry_version='
actual="$(bash "${planner}" 1.0.0 0.9.0 0.3.3 "${tags}")"
[ "${actual}" = "${expected}" ] || fail "empty registry plan did not match"
pass "supports the first exact version in an empty registry"

expect_failure "rejects a missing previous non-baseline version" \
  bash "${planner}" 1.0.0 '' 0.3.3 "${tags}"

printf '1..%d\n' "${passed}"
