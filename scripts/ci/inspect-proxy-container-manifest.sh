#!/usr/bin/env bash
set -euo pipefail

fail() {
  echo "Proxy container inspection failed: $*" >&2
  exit 1
}

[ "$#" -ge 5 ] && [ "$#" -le 6 ] || \
  fail "usage: $0 REGISTRY IMAGE_NAME VERSION EXPECTED_LABEL_VERSION EXPECTED_SOURCE [SOURCE_DIGEST_DIR]"

registry="$1"
image_name="$2"
version="$3"
expected_label_version="$4"
expected_source="$5"
source_digest_dir="${6:-}"
semver_regex='^(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)$'
digest_regex='^sha256:[0-9a-f]{64}$'

[[ "${registry}" =~ ^[a-z0-9.-]+(:[0-9]+)?$ ]] || fail "registry is invalid"
[[ "${image_name}" =~ ^[a-z0-9._/-]+$ ]] || fail "image name is invalid"
[[ "${version}" =~ ${semver_regex} ]] || fail "version must be exact canonical X.Y.Z"
[ -n "${expected_label_version}" ] || fail "expected label version is empty"
[ -n "${expected_source}" ] || fail "expected source is empty"
[ -z "${source_digest_dir}" ] || [ -d "${source_digest_dir}" ] || \
  fail "source digest directory does not exist: ${source_digest_dir}"

temp_dir="$(mktemp -d)"
trap 'rm -rf "${temp_dir}"' EXIT HUP INT TERM

registry_token="$(
  curl --fail --silent --show-error \
    "https://${registry}/token?scope=repository:${image_name}:pull" | jq -er '.token'
)" || fail "could not obtain an anonymous registry token"

accept_manifest='application/vnd.oci.image.index.v1+json, application/vnd.docker.distribution.manifest.list.v2+json, application/vnd.oci.image.manifest.v1+json, application/vnd.docker.distribution.manifest.v2+json'

registry_manifest() {
  local reference="$1"
  local output="$2"
  curl --fail --silent --show-error --location \
    --header "Authorization: Bearer ${registry_token}" \
    --header "Accept: ${accept_manifest}" \
    "https://${registry}/v2/${image_name}/manifests/${reference}" >"${output}" || \
    fail "could not fetch manifest ${reference}"
}

registry_blob() {
  local digest="$1"
  local output="$2"
  curl --fail --silent --show-error --location \
    --header "Authorization: Bearer ${registry_token}" \
    "https://${registry}/v2/${image_name}/blobs/${digest}" >"${output}" || \
    fail "could not fetch blob ${digest}"
}

remote_digest() {
  local reference="$1"
  local headers="${temp_dir}/headers-${reference//\//_}"
  local status
  status="$(
    curl --silent --show-error --head --output /dev/null \
      --write-out '%{http_code}' \
      --dump-header "${headers}" \
      --header "Authorization: Bearer ${registry_token}" \
      --header "Accept: ${accept_manifest}" \
      "https://${registry}/v2/${image_name}/manifests/${reference}"
  )"
  [ "${status}" = "200" ] || fail "manifest ${reference} returned HTTP ${status}"
  local digest
  digest="$(
    awk 'tolower($1) == "docker-content-digest:" { gsub("\\r", "", $2); print $2 }' \
      "${headers}"
  )"
  [[ "${digest}" =~ ${digest_regex} ]] || fail "manifest ${reference} returned an invalid digest"
  printf '%s\n' "${digest}"
}

index_json="${temp_dir}/index.json"
registry_manifest "${version}" "${index_json}"
manifest_digest="$(remote_digest "${version}")"

jq -e '
  (.mediaType == "application/vnd.oci.image.index.v1+json" or
   .mediaType == "application/vnd.docker.distribution.manifest.list.v2+json") and
  (.manifests | type == "array" and length == 4) and
  ([.manifests[] | select(.platform.os == "linux" and .platform.architecture == "amd64")] | length == 1) and
  ([.manifests[] | select(.platform.os == "linux" and .platform.architecture == "arm64")] | length == 1) and
  ([.manifests[] | select(
      .platform.os == "unknown" and
      .platform.architecture == "unknown" and
      .annotations["vnd.docker.reference.type"] == "attestation-manifest"
    )] | length == 2)
' "${index_json}" >/dev/null || fail "exact tag must contain two runtime and two attestation manifests"

image_revision=""
runtime_digests=()
for architecture in amd64 arm64; do
  runtime_digest="$(
    jq -er --arg architecture "${architecture}" '
      [.manifests[] | select(
        .platform.os == "linux" and .platform.architecture == $architecture
      )] | if length == 1 then .[0].digest else error("runtime descriptor count") end
    ' "${index_json}"
  )"
  [[ "${runtime_digest}" =~ ${digest_regex} ]] || fail "${architecture} runtime digest is invalid"
  runtime_digests+=("${runtime_digest}")

  runtime_manifest="${temp_dir}/runtime-${architecture}.json"
  registry_manifest "${runtime_digest}" "${runtime_manifest}"
  config_digest="$(jq -er '.config.digest' "${runtime_manifest}")"
  [[ "${config_digest}" =~ ${digest_regex} ]] || fail "${architecture} config digest is invalid"

  config_json="${temp_dir}/config-${architecture}.json"
  registry_blob "${config_digest}" "${config_json}"
  jq -e \
    --arg architecture "${architecture}" \
    --arg version "${expected_label_version}" \
    --arg source "${expected_source}" '
      .os == "linux" and
      .architecture == $architecture and
      .config.Labels["org.opencontainers.image.version"] == $version and
      .config.Labels["org.opencontainers.image.source"] == $source
    ' "${config_json}" >/dev/null || \
    fail "${architecture} runtime labels or platform do not match the release"

  revision="$(jq -er '.config.Labels["org.opencontainers.image.revision"]' "${config_json}")"
  [[ "${revision}" =~ ^[0-9a-f]{40}$ ]] || fail "${architecture} revision label is invalid"
  if [ -z "${image_revision}" ]; then
    image_revision="${revision}"
  fi
  [ "${revision}" = "${image_revision}" ] || fail "runtime images have different revision labels"

  attestation_digest="$(
    jq -er --arg runtime_digest "${runtime_digest}" '
      [.manifests[] | select(
        .platform.os == "unknown" and
        .platform.architecture == "unknown" and
        .annotations["vnd.docker.reference.type"] == "attestation-manifest" and
        .annotations["vnd.docker.reference.digest"] == $runtime_digest
      )] | if length == 1 then .[0].digest else error("attestation descriptor count") end
    ' "${index_json}"
  )"
  [[ "${attestation_digest}" =~ ${digest_regex} ]] || fail "${architecture} attestation digest is invalid"

  attestation_manifest="${temp_dir}/attestation-${architecture}.json"
  registry_manifest "${attestation_digest}" "${attestation_manifest}"
  jq -e '
    .config.mediaType == "application/vnd.oci.empty.v1+json" and
    ([.layers[] | select(
      .mediaType == "application/vnd.in-toto+json" and
      .annotations["in-toto.io/predicate-type"] == "https://slsa.dev/provenance/v1"
    )] | length == 1)
  ' "${attestation_manifest}" >/dev/null || \
    fail "${architecture} image is missing its SLSA provenance attestation"
done

if [ -n "${source_digest_dir}" ]; then
  source_files=()
  while IFS= read -r source_file; do
    source_files+=("${source_file}")
  done < <(find "${source_digest_dir}" -maxdepth 1 -type f -print | sort)
  [ "${#source_files[@]}" -eq 2 ] || fail "expected exactly two build source digests"

  expected_children="${temp_dir}/expected-children"
  : >"${expected_children}"
  for source_file in "${source_files[@]}"; do
    source_digest="sha256:$(basename "${source_file}")"
    [[ "${source_digest}" =~ ${digest_regex} ]] || fail "build source digest is invalid"
    source_manifest="${temp_dir}/source-$(basename "${source_file}").json"
    registry_manifest "${source_digest}" "${source_manifest}"
    source_media_type="$(jq -er '.mediaType' "${source_manifest}")"
    case "${source_media_type}" in
      application/vnd.oci.image.index.v1+json|application/vnd.docker.distribution.manifest.list.v2+json)
        jq -er '.manifests[].digest' "${source_manifest}" >>"${expected_children}"
        ;;
      application/vnd.oci.image.manifest.v1+json|application/vnd.docker.distribution.manifest.v2+json)
        printf '%s\n' "${source_digest}" >>"${expected_children}"
        ;;
      *) fail "build source has unsupported media type ${source_media_type}" ;;
    esac
  done

  jq -er '.manifests[].digest' "${index_json}" | sort -u >"${temp_dir}/published-children"
  sort -u "${expected_children}" >"${temp_dir}/expected-children-sorted"
  cmp -s "${temp_dir}/expected-children-sorted" "${temp_dir}/published-children" || \
    fail "published manifest children do not match the two build outputs"
fi

printf 'manifest_digest=%s\n' "${manifest_digest}"
printf 'image_revision=%s\n' "${image_revision}"
printf 'amd64_digest=%s\n' "${runtime_digests[0]}"
printf 'arm64_digest=%s\n' "${runtime_digests[1]}"
