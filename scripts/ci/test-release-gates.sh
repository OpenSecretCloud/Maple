#!/usr/bin/env bash
set -euo pipefail

script_dir="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
repo_root="$(cd "${script_dir}/../.." && pwd -P)"
classifier="${script_dir}/classify-app-release.sh"
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
    echo "Unexpected success output:" >&2
    printf '%s\n' "${output}" >&2
    fail "${label}"
  fi
  pass "${label}"
}

make_fixture() {
  local directory="$1"
  local app_version="$2"
  local release_tag="$3"
  local cargo_version="${4:-${app_version}}"

  mkdir -p "${directory}/frontend/src-tauri"
  printf '{"version":"%s"}\n' "${app_version}" > "${directory}/frontend/package.json"
  printf '{"version":"%s"}\n' "${app_version}" > "${directory}/frontend/src-tauri/tauri.conf.json"
  printf '[package]\nname = "maple-fixture"\nversion = "%s"\n\n[dependencies]\n' "${cargo_version}" \
    > "${directory}/frontend/src-tauri/Cargo.toml"
  git -C "${directory}" init -q -b master
  git -C "${directory}" config user.name "Maple release gate tests"
  git -C "${directory}" config user.email "release-gates@example.invalid"
  git -C "${directory}" add \
    frontend/package.json \
    frontend/src-tauri/Cargo.toml \
    frontend/src-tauri/tauri.conf.json
  git -C "${directory}" commit -q -m "fixture"
  git -C "${directory}" tag -a "${release_tag}" -m "${release_tag}"
}

run_classifier() {
  local fixture="$1"
  local tag="$2"
  local sha="$3"
  local draft="$4"
  local prerelease="$5"
  local ref="$6"
  local protected_ref="$7"

  bash "${classifier}" \
    --repo-root "${fixture}" \
    --tag "${tag}" \
    --sha "${sha}" \
    --draft "${draft}" \
    --prerelease "${prerelease}" \
    --ref "${ref}" \
    --protected-ref "${protected_ref}"
}

fixture="${temp_root}/valid"
make_fixture "${fixture}" "1.2.3" "v1.2.3"
sha="$(git -C "${fixture}" rev-parse HEAD)"

expected_stable="$(cat <<EOF
tag=v1.2.3
release_sha=${sha}
version=1.2.3
prerelease=false
stable=true
EOF
)"
actual="$(run_classifier "${fixture}" "v1.2.3" "${sha}" false false refs/tags/v1.2.3 refs/heads/master)"
[ "${actual}" = "${expected_stable}" ] || fail "stable release output did not match"
pass "accepts a stable app release"

expected_prerelease="$(cat <<EOF
tag=v1.2.3
release_sha=${sha}
version=1.2.3
prerelease=true
stable=false
EOF
)"
actual="$(run_classifier "${fixture}" "v1.2.3" "${sha}" false true refs/tags/v1.2.3 refs/heads/master)"
[ "${actual}" = "${expected_prerelease}" ] || fail "GitHub prerelease output did not match"
pass "accepts an exact app tag marked as a GitHub prerelease"

for malformed_tag in 1.2.3 v1.2 v1.2.3-beta.1 v1.2.3+build v01.2.3 V1.2.3; do
  expect_failure "rejects malformed tag ${malformed_tag}" \
    run_classifier "${fixture}" "${malformed_tag}" "${sha}" false false "refs/tags/${malformed_tag}" refs/heads/master
done

uppercase_sha="$(printf '%s' "${sha}" | tr '[:lower:]' '[:upper:]')"
if [ "${uppercase_sha}" = "${sha}" ]; then
  uppercase_sha="A${sha:1}"
fi
expect_failure "rejects an uppercase SHA" \
  run_classifier "${fixture}" v1.2.3 "${uppercase_sha}" false false refs/tags/v1.2.3 refs/heads/master
expect_failure "rejects a short SHA" \
  run_classifier "${fixture}" v1.2.3 "${sha:0:12}" false false refs/tags/v1.2.3 refs/heads/master
expect_failure "rejects draft=true" \
  run_classifier "${fixture}" v1.2.3 "${sha}" true false refs/tags/v1.2.3 refs/heads/master
expect_failure "rejects a malformed draft boolean" \
  run_classifier "${fixture}" v1.2.3 "${sha}" no false refs/tags/v1.2.3 refs/heads/master
expect_failure "rejects a malformed prerelease boolean" \
  run_classifier "${fixture}" v1.2.3 "${sha}" false no refs/tags/v1.2.3 refs/heads/master
expect_failure "rejects a release ref that does not match the tag" \
  run_classifier "${fixture}" v1.2.3 "${sha}" false false refs/tags/v9.9.9 refs/heads/master
expect_failure "rejects a malformed protected ref" \
  run_classifier "${fixture}" v1.2.3 "${sha}" false false refs/tags/v1.2.3 'master^{commit}'

printf 'new head\n' > "${fixture}/README.md"
git -C "${fixture}" add README.md
git -C "${fixture}" commit -q -m "move head"
new_head_sha="$(git -C "${fixture}" rev-parse HEAD)"
expect_failure "rejects a tag that does not peel to the release SHA" \
  run_classifier "${fixture}" v1.2.3 "${new_head_sha}" false false refs/tags/v1.2.3 refs/heads/master
expect_failure "rejects a checked-out HEAD that does not match the release SHA" \
  run_classifier "${fixture}" v1.2.3 "${sha}" false false refs/tags/v1.2.3 refs/heads/master

non_ancestor_fixture="${temp_root}/non-ancestor"
make_fixture "${non_ancestor_fixture}" "1.2.3" "v1.2.3"
tree="$(git -C "${non_ancestor_fixture}" rev-parse 'HEAD^{tree}')"
side_sha="$(printf 'unrelated release\n' | git -C "${non_ancestor_fixture}" commit-tree "${tree}")"
git -C "${non_ancestor_fixture}" update-ref refs/tags/v1.2.3 "${side_sha}"
git -C "${non_ancestor_fixture}" checkout -q --detach "${side_sha}"
expect_failure "rejects a release SHA outside the protected branch" \
  run_classifier "${non_ancestor_fixture}" v1.2.3 "${side_sha}" false false refs/tags/v1.2.3 refs/heads/master

version_fixture="${temp_root}/version-mismatch"
make_fixture "${version_fixture}" "9.9.9" "v1.2.3"
version_sha="$(git -C "${version_fixture}" rev-parse HEAD)"
expect_failure "rejects an app manifest version mismatch" \
  run_classifier "${version_fixture}" v1.2.3 "${version_sha}" false false refs/tags/v1.2.3 refs/heads/master

cargo_fixture="${temp_root}/cargo-version-mismatch"
make_fixture "${cargo_fixture}" "1.2.3" "v1.2.3" "9.9.9"
cargo_sha="$(git -C "${cargo_fixture}" rev-parse HEAD)"
expect_failure "rejects a Cargo package version mismatch" \
  run_classifier "${cargo_fixture}" v1.2.3 "${cargo_sha}" false false refs/tags/v1.2.3 refs/heads/master

expect_failure "rejects a missing required argument" \
  bash "${classifier}" --repo-root "${fixture}"
expect_failure "rejects an unknown argument" \
  bash "${classifier}" --unknown value

release_json="${temp_root}/release.json"
pages_production_json="${temp_root}/pages-production.json"
yq -o=json '.' "${repo_root}/.github/workflows/release.yml" > "${release_json}"
yq -o=json '.' "${repo_root}/.github/workflows/pages-production.yml" > "${pages_production_json}"

python3 - "${release_json}" "${pages_production_json}" <<'PY'
import json
import re
import sys


def check(condition, message):
    if not condition:
        raise AssertionError(message)


def needs(job):
    value = job.get("needs", [])
    return [value] if isinstance(value, str) else value


def secret_names(value):
    found = set()
    if isinstance(value, dict):
        for child in value.values():
            found.update(secret_names(child))
    elif isinstance(value, list):
        for child in value:
            found.update(secret_names(child))
    elif isinstance(value, str):
        dot_pattern = r"secrets\.([A-Za-z0-9_]+)"
        bracket_pattern = r"secrets\[['\"]([A-Za-z0-9_]+)['\"]\]"
        found.update(re.findall(dot_pattern, value))
        found.update(re.findall(bracket_pattern, value))
        remaining = re.sub(dot_pattern, "", value)
        remaining = re.sub(bracket_pattern, "", remaining)
        if re.search(r"\bsecrets\b", remaining):
            found.add("*")
    return found


with open(sys.argv[1], encoding="utf-8") as handle:
    release = json.load(handle)

with open(sys.argv[2], encoding="utf-8") as handle:
    pages_production = json.load(handle)

release_jobs = release["jobs"]
classifier_id = "classify-app-release"
check(classifier_id in release_jobs, "Release workflow must have classify-app-release job")
classifier = release_jobs[classifier_id]
classifier_permissions = classifier.get("permissions", {})
check(
    classifier_permissions == {"contents": "read"},
    "Release classifier must have only contents: read permission",
)
check(not secret_names(classifier), "Release classifier must not receive secrets")
for output in ("tag", "release_sha", "version", "prerelease", "stable"):
    check(output in classifier.get("outputs", {}), f"Release classifier must emit {output}")

android_attestation_steps = [
    step
    for step in release_jobs.get("build-android", {}).get("steps", [])
    if step.get("name") == "Attest Android release artifacts"
]
check(len(android_attestation_steps) == 1, "Android release artifacts must be attested exactly once")
check(
    android_attestation_steps[0].get("continue-on-error") is True,
    "Android attestation must remain best effort",
)

classifier_checkouts = [
    step
    for step in classifier.get("steps", [])
    if str(step.get("uses", "")).startswith("actions/checkout@")
]
check(len(classifier_checkouts) == 1, "Release classifier must have exactly one checkout")
classifier_checkout = classifier_checkouts[0].get("with", {})
check(classifier_checkout.get("ref") == "${{ github.sha }}", "Release classifier must checkout github.sha")
check(classifier_checkout.get("fetch-depth") in (0, "0"), "Release classifier must fetch complete history")
check(classifier_checkout.get("persist-credentials") is False, "Release classifier must not persist credentials")

release_ref = "${{ needs.classify-app-release.outputs.release_sha }}"
for job_id, job in release_jobs.items():
    if job_id == classifier_id:
        continue
    check(classifier_id in needs(job), f"Release job {job_id} must directly need {classifier_id}")
    for step in job.get("steps", []):
        if str(step.get("uses", "")).startswith("actions/checkout@"):
            checkout = step.get("with", {})
            check(checkout.get("ref") == release_ref, f"Release checkout in {job_id} must pin classifier SHA")
            check(checkout.get("persist-credentials") is False, f"Release checkout in {job_id} must not persist credentials")

check(
    pages_production.get("permissions") == {"contents": "read"},
    "Pages production workflow must default to contents: read",
)
pages_on = pages_production.get("on", {})
check("workflow_dispatch" in pages_on, "Pages production workflow must support manual retry")
pages_workflow_run = pages_on.get("workflow_run", {})
check(
    pages_workflow_run.get("workflows") == ["Release"]
    and pages_workflow_run.get("types") == ["completed"],
    "Pages production workflow must follow completed Release workflows",
)
check(
    pages_production.get("concurrency")
    == {"group": "pages-production", "cancel-in-progress": False},
    "Pages production workflow must serialize promotions without cancellation",
)

pages_jobs = pages_production.get("jobs", {})
check(set(pages_jobs) == {"promote"}, "Pages production workflow must have one promotion job")
pages_job = pages_jobs["promote"]
check(
    pages_job.get("permissions") == {"contents": "write"},
    "Pages production promotion must have only contents: write permission",
)
check(not secret_names(pages_production), "Pages production workflow must not receive secrets")
check("environment" not in pages_job, "Pages production promotion must not require a secret-bearing environment")

pages_if = str(pages_job.get("if", ""))
for required_gate in (
    "workflow_run.conclusion == 'success'",
    "workflow_run.event == 'release'",
    "workflow_run.path == '.github/workflows/release.yml'",
    "workflow_run.head_repository.full_name == github.repository",
):
    check(required_gate in pages_if, f"Pages production job is missing gate: {required_gate}")

pages_steps = pages_job.get("steps", [])
check(len(pages_steps) == 1, "Pages production promotion must have one non-checkout step")
pages_step = pages_steps[0]
check("uses" not in pages_step, "Pages production promotion must not run a third-party action")
pages_env = pages_step.get("env", {})
check(
    pages_env.get("PAGES_PRODUCTION_BRANCH") == "pages-production",
    "Pages production branch must be hardcoded",
)
check(pages_env.get("GH_TOKEN") == "${{ github.token }}", "Pages promotion must use github.token")

pages_run = str(pages_step.get("run", ""))
for required_control in (
    "repos/${REPOSITORY}/releases/latest",
    "repos/${REPOSITORY}/compare/${release_sha}...master",
    "repos/${REPOSITORY}/compare/${current_sha}...${release_sha}",
    "repos/${REPOSITORY}/git/refs/heads/${PAGES_PRODUCTION_BRANCH}",
    '-F force=true',
):
    check(required_control in pages_run, f"Pages production step is missing control: {required_control}")

PY
pass "workflow release-gate topology is fail closed with isolated downstream publishers"

printf '1..%d\n' "${passed}"
