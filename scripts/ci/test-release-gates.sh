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
zapstore_json="${temp_root}/zapstore.json"
yq -o=json '.' "${repo_root}/.github/workflows/release.yml" > "${release_json}"
yq -o=json '.' "${repo_root}/.github/workflows/zapstore-publish.yml" > "${zapstore_json}"

python3 - "${release_json}" "${zapstore_json}" <<'PY'
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
    android_attestation_steps[0].get("continue-on-error") is not True,
    "Android attestation must succeed before the Release can wake Zapstore",
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

with open(sys.argv[2], encoding="utf-8") as handle:
    zapstore = json.load(handle)

zapstore_jobs = zapstore["jobs"]
check("classify" in zapstore_jobs, "Zapstore workflow must have classify job")
check("publish" in zapstore_jobs, "Zapstore workflow must have publish job")
zapstore_classifier = zapstore_jobs["classify"]
check(not secret_names(zapstore_classifier), "Zapstore classifier must not receive secrets")
check(
    zapstore_classifier.get("permissions", {}) == {"actions": "read", "contents": "read"},
    "Zapstore classifier must have only actions: read and contents: read permissions",
)
check(
    not any(
        str(step.get("uses", "")).startswith("actions/checkout@")
        for step in zapstore_classifier.get("steps", [])
    ),
    "Zapstore classifier must not checkout or execute repository content",
)
zapstore_classifier_run = "\n".join(
    str(step.get("run", "")) for step in zapstore_classifier.get("steps", [])
)
check(
    ".run_attempt == $run_attempt" not in zapstore_classifier_run,
    "Zapstore classifier must allow a successful classifier retained by --failed reruns",
)
publish = zapstore_jobs["publish"]
check("classify" in needs(publish), "Zapstore publish must directly need classify")
check(
    publish.get("permissions", {}) == {"attestations": "read", "contents": "read"},
    "Zapstore publish must have only attestations: read and contents: read permissions",
)
publish_metadata = {key: value for key, value in publish.items() if key != "steps"}
check(
    not (secret_names(publish_metadata) - {"GITHUB_TOKEN"}),
    "Zapstore custom secret must not be exposed at job scope",
)
concurrency = publish.get("concurrency", {})
check(concurrency.get("group") == "zapstore-production", "Zapstore publish concurrency group must be hard-coded")
check(concurrency.get("cancel-in-progress") is False, "Zapstore publish must not cancel an in-progress publication")

steps = publish.get("steps", [])
checkout_steps = [step for step in steps if str(step.get("uses", "")).startswith("actions/checkout@")]
check(len(checkout_steps) == 1, "Zapstore publish must have exactly one checkout")
checkout = checkout_steps[0].get("with", {})
check(
    checkout.get("ref") == "${{ needs.classify.outputs.release_sha }}",
    "Zapstore checkout must pin the classified upstream SHA",
)
check(checkout.get("persist-credentials") is False, "Zapstore checkout must not persist credentials")

attestation_steps = [step for step in steps if step.get("name") == "Verify APK release provenance"]
check(len(attestation_steps) == 1, "Zapstore must verify APK release provenance exactly once")
attestation_index = steps.index(attestation_steps[0])
attestation_run = str(attestation_steps[0].get("run", ""))
for required_fragment in (
    "gh attestation verify app-universal-release.apk",
    "--signer-workflow",
    "--signer-digest",
    "--source-digest",
    "--source-ref",
    "--deny-self-hosted-runners",
):
    check(required_fragment in attestation_run, f"Zapstore APK verification must use {required_fragment}")

publish_indexes = [index for index, step in enumerate(steps) if step.get("name") == "Publish to Zapstore"]
check(len(publish_indexes) == 1, "Zapstore workflow must have one final Publish to Zapstore step")
publish_index = publish_indexes[0]
check(attestation_index < publish_index, "APK provenance verification must precede Zapstore publish")
publish_step = steps[publish_index]
check(publish_index == len(steps) - 1, "Publish to Zapstore must be the final step")
check(publish_index > 0, "Publish to Zapstore must follow a current-release recheck")
recheck = steps[publish_index - 1]
recheck_name = str(recheck.get("name", "")).lower()
check(
    "release" in recheck_name and any(word in recheck_name for word in ("recheck", "current", "confirm")),
    "Current-release recheck must immediately precede publish",
)
check("releases/latest" in str(recheck.get("run", "")), "Current-release recheck must query releases/latest")
recheck_id = recheck.get("id")
check(isinstance(recheck_id, str) and recheck_id, "Current-release recheck must expose a step output")
expected_publish_condition = f"steps.{recheck_id}.outputs.current == 'true'"
check(
    publish_step.get("if") == expected_publish_condition,
    "Zapstore publish must be conditional on the immediately preceding current-release recheck: "
    f"expected {expected_publish_condition!r}, got {publish_step.get('if')!r}",
)
zsp_indexes = [index for index, step in enumerate(steps) if "zsp publish" in str(step.get("run", ""))]
check(zsp_indexes == [publish_index], "zsp publish must run only in the final secret-bearing step")

for index, step in enumerate(steps):
    custom_secrets = secret_names(step) - {"GITHUB_TOKEN"}
    if index == publish_index:
        check(custom_secrets == {"ZAPSTORE_SIGN_WITH"}, "Final publish must receive only ZAPSTORE_SIGN_WITH")
    else:
        check(not custom_secrets, f"Custom secret exposed before final publish step {index}")
PY
pass "workflow release-gate topology is fail closed"

printf '1..%d\n' "${passed}"
