#!/usr/bin/env bash
set -euo pipefail

readonly expected_repo="OpenSecretCloud/Maple"
readonly remote_url="https://github.com/OpenSecretCloud/Maple.git"

fail() {
  printf 'release preflight: %s\n' "$*" >&2
  exit 1
}

for command_name in git gh jq python3; do
  command -v "${command_name}" >/dev/null 2>&1 || fail "missing required command: ${command_name}"
done

repo_root="$(git rev-parse --show-toplevel 2>/dev/null)" || fail "run from the Maple repository"
cd "${repo_root}"

repo="$(gh repo view --json nameWithOwner --jq .nameWithOwner)"
[[ "${repo}" == "${expected_repo}" ]] || fail "expected ${expected_repo}, found ${repo}"

branch="$(git branch --show-current)"
[[ "${branch}" == "master" ]] || fail "release from master, not ${branch:-detached HEAD}"

if [[ -n "$(git status --porcelain --untracked-files=no)" ]]; then
  fail "tracked worktree changes are present"
fi

git fetch --no-tags "${remote_url}" refs/heads/master:refs/remotes/origin/master >/dev/null

head_sha="$(git rev-parse HEAD)"
remote_sha="$(git rev-parse refs/remotes/origin/master)"
[[ "${head_sha}" == "${remote_sha}" ]] || fail "HEAD ${head_sha} does not match origin/master ${remote_sha}"

package_version="$(jq -er .version frontend/package.json)"
tauri_version="$(jq -er .version frontend/src-tauri/tauri.conf.json)"
cargo_version="$(awk '
  /^\[package\]$/ { in_package = 1; next }
  /^\[/ && in_package { exit }
  in_package && /^version[[:space:]]*=/ {
    value = $0
    sub(/^[^=]*=[[:space:]]*"/, "", value)
    sub(/"[[:space:]]*$/, "", value)
    print value
    exit
  }
' frontend/src-tauri/Cargo.toml)"

[[ -n "${cargo_version}" ]] || fail "could not read Cargo package version"
[[ "${package_version}" == "${tauri_version}" ]] || fail "package.json ${package_version} != tauri.conf.json ${tauri_version}"
[[ "${package_version}" == "${cargo_version}" ]] || fail "package.json ${package_version} != Cargo.toml ${cargo_version}"

tag="v${package_version}"
validated_version="$(./scripts/ci/validate-release-version.sh "${tag}")"
[[ "${validated_version}" == "${package_version}" ]] || fail "release validator returned ${validated_version}"

previous_tag="$(gh api "repos/${repo}/releases/latest" --jq .tag_name)"
[[ -n "${previous_tag}" ]] || fail "could not determine the latest published release"
[[ "${tag}" != "${previous_tag}" ]] || fail "${tag} is already the latest published release"

previous_version="$(gh api \
  -H 'Accept: application/vnd.github.raw+json' \
  "repos/${repo}/contents/frontend/package.json?ref=${previous_tag}" | jq -er .version)"

python3 - "${previous_version}" "${package_version}" <<'PY'
import re
import sys


def semver(value: str):
    match = re.fullmatch(
        r"(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)"
        r"(?:-([0-9A-Za-z.-]+))?(?:\+[0-9A-Za-z.-]+)?",
        value,
    )
    if not match:
        raise SystemExit(f"release preflight: invalid semantic version: {value}")
    core = tuple(int(match.group(index)) for index in range(1, 4))
    prerelease = match.group(4)
    identifiers = prerelease.split(".") if prerelease else None
    return core, identifiers


def compare(left, right):
    if left[0] != right[0]:
        return (left[0] > right[0]) - (left[0] < right[0])
    left_pre, right_pre = left[1], right[1]
    if left_pre is None or right_pre is None:
        return (left_pre is None) - (right_pre is None)
    for left_id, right_id in zip(left_pre, right_pre):
        if left_id == right_id:
            continue
        left_num, right_num = left_id.isdigit(), right_id.isdigit()
        if left_num and right_num:
            return (int(left_id) > int(right_id)) - (int(left_id) < int(right_id))
        if left_num != right_num:
            return -1 if left_num else 1
        return (left_id > right_id) - (left_id < right_id)
    return (len(left_pre) > len(right_pre)) - (len(left_pre) < len(right_pre))


previous, current = map(semver, sys.argv[1:3])
if compare(current, previous) <= 0:
    raise SystemExit(
        f"release preflight: current version {sys.argv[2]} is not newer than {sys.argv[1]}"
    )
PY

compare_status="$(gh api "repos/${repo}/compare/${previous_tag}...${head_sha}" --jq .status)"
[[ "${compare_status}" == "ahead" ]] || fail "master is not ahead of ${previous_tag} (status: ${compare_status})"

if gh release view "${tag}" --repo "${repo}" >/dev/null 2>&1; then
  fail "GitHub release ${tag} already exists"
fi

if git ls-remote --exit-code --tags "${remote_url}" "refs/tags/${tag}" "refs/tags/${tag}^{}" >/dev/null 2>&1; then
  fail "remote tag ${tag} already exists"
fi

runs="$(gh run list --repo "${repo}" --commit "${head_sha}" --limit 50 \
  --json workflowName,status,conclusion,createdAt,url)"

required_workflows=(
  "Frontend Tests"
  "Rust Unit Tests"
  "Web App Build"
  "Desktop App CI"
  "Android App CI"
  "Mobile App CI"
  "CodeQL"
)

for workflow in "${required_workflows[@]}"; do
  latest="$(printf '%s' "${runs}" | jq -c --arg workflow "${workflow}" \
    '[.[] | select(.workflowName == $workflow)] | sort_by(.createdAt) | reverse | .[0] // empty')"
  [[ -n "${latest}" ]] || fail "no ${workflow} run found for ${head_sha}"
  run_status="$(printf '%s' "${latest}" | jq -r .status)"
  run_conclusion="$(printf '%s' "${latest}" | jq -r .conclusion)"
  [[ "${run_status}" == "completed" && "${run_conclusion}" == "success" ]] || \
    fail "${workflow} is ${run_status}/${run_conclusion} for ${head_sha}"
done

jq -n \
  --arg repo "${repo}" \
  --arg branch "${branch}" \
  --arg head_sha "${head_sha}" \
  --arg previous_tag "${previous_tag}" \
  --arg previous_version "${previous_version}" \
  --arg version "${package_version}" \
  --arg tag "${tag}" \
  '{
    repo: $repo,
    branch: $branch,
    head_sha: $head_sha,
    previous_tag: $previous_tag,
    previous_version: $previous_version,
    version: $version,
    tag: $tag,
    ci: "passed"
  }'
