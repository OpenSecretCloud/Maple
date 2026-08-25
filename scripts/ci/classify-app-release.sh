#!/usr/bin/env bash
set -euo pipefail

script_dir="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"

fail() {
  echo "$*" >&2
  exit 1
}

require_value() {
  local option="$1"
  local value="${2:-}"

  if [ -z "${value}" ] || [[ "${value}" == --* ]]; then
    fail "${option} requires a value."
  fi
}

repo_root=""
tag=""
release_sha=""
draft=""
prerelease=""
release_ref=""
protected_ref=""

while [ "$#" -gt 0 ]; do
  case "$1" in
    --repo-root)
      require_value "$1" "${2:-}"
      [ -z "${repo_root}" ] || fail "--repo-root may only be specified once."
      repo_root="$2"
      shift 2
      ;;
    --tag)
      require_value "$1" "${2:-}"
      [ -z "${tag}" ] || fail "--tag may only be specified once."
      tag="$2"
      shift 2
      ;;
    --sha)
      require_value "$1" "${2:-}"
      [ -z "${release_sha}" ] || fail "--sha may only be specified once."
      release_sha="$2"
      shift 2
      ;;
    --draft)
      require_value "$1" "${2:-}"
      [ -z "${draft}" ] || fail "--draft may only be specified once."
      draft="$2"
      shift 2
      ;;
    --prerelease)
      require_value "$1" "${2:-}"
      [ -z "${prerelease}" ] || fail "--prerelease may only be specified once."
      prerelease="$2"
      shift 2
      ;;
    --ref)
      require_value "$1" "${2:-}"
      [ -z "${release_ref}" ] || fail "--ref may only be specified once."
      release_ref="$2"
      shift 2
      ;;
    --protected-ref)
      require_value "$1" "${2:-}"
      [ -z "${protected_ref}" ] || fail "--protected-ref may only be specified once."
      protected_ref="$2"
      shift 2
      ;;
    *)
      fail "Unknown argument: $1"
      ;;
  esac
done

[ -n "${repo_root}" ] || fail "--repo-root is required."
[ -n "${tag}" ] || fail "--tag is required."
[ -n "${release_sha}" ] || fail "--sha is required."
[ -n "${draft}" ] || fail "--draft is required."
[ -n "${prerelease}" ] || fail "--prerelease is required."
[ -n "${release_ref}" ] || fail "--ref is required."
[ -n "${protected_ref}" ] || fail "--protected-ref is required."

if ! repo_root="$(cd "${repo_root}" && pwd -P)"; then
  fail "--repo-root is not an accessible directory."
fi

if [[ ! "${tag}" =~ ^v(0|[1-9][0-9]*)[.](0|[1-9][0-9]*)[.](0|[1-9][0-9]*)$ ]]; then
  fail "Refusing unexpected app release tag: ${tag}"
fi

if [[ ! "${release_sha}" =~ ^[0-9a-f]{40}$ ]]; then
  fail "Release SHA must be exactly 40 lowercase hexadecimal characters."
fi

if [ "${draft}" != "false" ]; then
  fail "Release must not be a draft."
fi

case "${prerelease}" in
  true | false)
    ;;
  *)
    fail "Prerelease must be either true or false."
    ;;
esac

if [ "${release_ref}" != "refs/tags/${tag}" ]; then
  fail "Release ref does not match the release tag."
fi

if [[ ! "${protected_ref}" =~ ^refs/(heads|remotes)/[0-9A-Za-z._/-]+$ ]] || \
  ! git check-ref-format "${protected_ref}" >/dev/null 2>&1; then
  fail "Protected ref has an unexpected format."
fi

if ! git -C "${repo_root}" rev-parse --is-inside-work-tree >/dev/null 2>&1; then
  fail "Repository root is not a Git worktree: ${repo_root}"
fi

if ! head_sha="$(git -C "${repo_root}" rev-parse --verify 'HEAD^{commit}' 2>/dev/null)"; then
  fail "Unable to resolve the checked-out HEAD commit."
fi
if [ "${head_sha}" != "${release_sha}" ]; then
  fail "Checked-out HEAD does not match the release SHA."
fi

if ! tag_sha="$(git -C "${repo_root}" rev-parse --verify "refs/tags/${tag}^{commit}" 2>/dev/null)"; then
  fail "Unable to resolve release tag ${tag}."
fi
if [ "${tag_sha}" != "${release_sha}" ]; then
  fail "Release tag ${tag} does not resolve to the release SHA."
fi

if ! git -C "${repo_root}" rev-parse --verify "${protected_ref}^{commit}" >/dev/null 2>&1; then
  fail "Unable to resolve protected ref ${protected_ref}."
fi
if ! git -C "${repo_root}" merge-base --is-ancestor "${release_sha}" "${protected_ref}"; then
  fail "Release SHA is not reachable from protected ref ${protected_ref}."
fi

version="$(
  MAPLE_RELEASE_REPO_ROOT="${repo_root}" \
    bash "${script_dir}/validate-release-version.sh" "${tag}"
)"

stable=true
if [ "${prerelease}" = "true" ]; then
  stable=false
fi

printf 'tag=%s\n' "${tag}"
printf 'release_sha=%s\n' "${release_sha}"
printf 'version=%s\n' "${version}"
printf 'prerelease=%s\n' "${prerelease}"
printf 'stable=%s\n' "${stable}"
