#!/usr/bin/env bash
set -euo pipefail

source "$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)/_common.sh"

print_source_provenance

if [ "$(host_os)" != "linux" ]; then
  echo "Android builds must run on Linux." >&2
  exit 1
fi

case "$(uname -m)" in
  x86_64 | amd64)
    ;;
  *)
    echo "Android release builds currently require an x86_64 Linux host because Google's Linux NDK tools are x86_64-only in this SDK package. Current host: $(uname -m)." >&2
    exit 1
    ;;
esac

if [ -z "${ANDROID_HOME:-}" ] || [ ! -d "${ANDROID_HOME}" ]; then
  echo "ANDROID_HOME is not set. Run through 'nix develop .#android'." >&2
  exit 1
fi

if [ -z "${NDK_HOME:-}" ] || [ ! -d "${NDK_HOME}" ]; then
  if [ -d "${ANDROID_HOME}/ndk-bundle" ]; then
    export NDK_HOME="${ANDROID_HOME}/ndk-bundle"
  else
    NDK_HOME="$(find "${ANDROID_HOME}/ndk" -mindepth 1 -maxdepth 1 -type d 2>/dev/null | sort | tail -n 1 || true)"
    export NDK_HOME
  fi
fi

if [ -z "${NDK_HOME:-}" ] || [ ! -d "${NDK_HOME}/toolchains/llvm/prebuilt" ]; then
  echo "Could not find Android NDK under ANDROID_HOME=${ANDROID_HOME}" >&2
  exit 1
fi

android_signing_enabled=0
keystore_file=""
keystore_properties_file="${TAURI_DIR}/gen/android/keystore.properties"
fake_android_keystore_dir=""

cleanup_android_release() {
  if [ -n "${tmp_toolchain_bin:-}" ]; then
    rm -rf "${tmp_toolchain_bin}"
  fi

  if [ -n "${keystore_file:-}" ]; then
    rm -f "${keystore_file}"
  fi

  if [ -n "${fake_android_keystore_dir:-}" ]; then
    rm -rf "${fake_android_keystore_dir}"
  fi
}
trap cleanup_android_release EXIT

generate_fake_android_signing_env() {
  local fake_keystore fake_alias fake_password

  fake_android_keystore_dir="$(mktemp -d)"
  fake_keystore="${fake_android_keystore_dir}/maple-pr-fake-release.jks"
  fake_alias="maple-pr-fake-release"
  fake_password="maple-pr-fake-release-password"

  keytool -genkeypair \
    -keystore "${fake_keystore}" \
    -storepass "${fake_password}" \
    -keypass "${fake_password}" \
    -alias "${fake_alias}" \
    -keyalg RSA \
    -keysize 2048 \
    -validity 10000 \
    -dname "CN=Maple PR Fake,O=OpenSecret,C=US" \
    -storetype JKS >/dev/null

  ANDROID_KEYSTORE_BASE64="$(base64 < "${fake_keystore}" | tr -d '\n')"
  export ANDROID_KEYSTORE_BASE64
  export ANDROID_KEY_ALIAS="${fake_alias}"
  export ANDROID_KEY_PASSWORD="${fake_password}"

  echo "generated-fake-android-keystore  ephemeral"
}

if [ "${MAPLE_ANDROID_FAKE_SIGNING:-0}" = "1" ]; then
  generate_fake_android_signing_env
fi

if [ -z "${ANDROID_KEYSTORE_BASE64:-}" ] || [ -z "${ANDROID_KEY_ALIAS:-}" ] || [ -z "${ANDROID_KEY_PASSWORD:-}" ]; then
  if [ "${MAPLE_ANDROID_ALLOW_UNSIGNED_RELEASE:-0}" != "1" ]; then
    echo "Android release signing variables are required: ANDROID_KEYSTORE_BASE64, ANDROID_KEY_ALIAS, ANDROID_KEY_PASSWORD." >&2
    exit 1
  fi
else
  android_signing_enabled=1
fi

decode_android_keystore() {
  keystore_file="${RUNNER_TEMP:-${TMPDIR:-/tmp}}/maple-release-keystore.jks"
  decode_base64_string_to_file "${ANDROID_KEYSTORE_BASE64}" "${keystore_file}"
}

install_frontend_deps
configure_sccache
case "${MAPLE_ANDROID_WEB_ENVIRONMENT:-release}" in
  pr)
    use_pr_environment
    ;;
  release)
    use_release_environment
    ;;
  *)
    echo "Unsupported MAPLE_ANDROID_WEB_ENVIRONMENT=${MAPLE_ANDROID_WEB_ENVIRONMENT}. Expected pr or release." >&2
    exit 1
    ;;
esac
configure_reproducible_build_metadata
build_frontend_dist

toolchain_prebuilt="$(
  find "${NDK_HOME}/toolchains/llvm/prebuilt" -mindepth 1 -maxdepth 1 -type d 2>/dev/null \
    | LC_ALL=C sort \
    | head -n 1 || true
)"
if [ -z "${toolchain_prebuilt}" ] || [ ! -d "${toolchain_prebuilt}/bin" ]; then
  echo "Could not find Android NDK LLVM prebuilt toolchain under NDK_HOME=${NDK_HOME}" >&2
  exit 1
fi
export PATH="${toolchain_prebuilt}/bin:${PATH}"

tmp_toolchain_bin="$(mktemp -d)"

ln -sf "${toolchain_prebuilt}/bin/llvm-ranlib" "${tmp_toolchain_bin}/aarch64-linux-android-ranlib"
ln -sf "${toolchain_prebuilt}/bin/llvm-ranlib" "${tmp_toolchain_bin}/armv7a-linux-androideabi-ranlib"
ln -sf "${toolchain_prebuilt}/bin/llvm-ranlib" "${tmp_toolchain_bin}/x86_64-linux-android-ranlib"
ln -sf "${toolchain_prebuilt}/bin/llvm-ranlib" "${tmp_toolchain_bin}/i686-linux-android-ranlib"
export PATH="${tmp_toolchain_bin}:${PATH}"

export AR_aarch64_linux_android="${toolchain_prebuilt}/bin/llvm-ar"
export CC_aarch64_linux_android="${toolchain_prebuilt}/bin/aarch64-linux-android24-clang"
export CXX_aarch64_linux_android="${toolchain_prebuilt}/bin/aarch64-linux-android24-clang++"
export RANLIB_aarch64_linux_android="${toolchain_prebuilt}/bin/llvm-ranlib"

export AR_armv7_linux_androideabi="${toolchain_prebuilt}/bin/llvm-ar"
export CC_armv7_linux_androideabi="${toolchain_prebuilt}/bin/armv7a-linux-androideabi24-clang"
export CXX_armv7_linux_androideabi="${toolchain_prebuilt}/bin/armv7a-linux-androideabi24-clang++"
export RANLIB_armv7_linux_androideabi="${toolchain_prebuilt}/bin/llvm-ranlib"

export AR_x86_64_linux_android="${toolchain_prebuilt}/bin/llvm-ar"
export CC_x86_64_linux_android="${toolchain_prebuilt}/bin/x86_64-linux-android24-clang"
export CXX_x86_64_linux_android="${toolchain_prebuilt}/bin/x86_64-linux-android24-clang++"
export RANLIB_x86_64_linux_android="${toolchain_prebuilt}/bin/llvm-ranlib"

export AR_i686_linux_android="${toolchain_prebuilt}/bin/llvm-ar"
export CC_i686_linux_android="${toolchain_prebuilt}/bin/i686-linux-android24-clang"
export CXX_i686_linux_android="${toolchain_prebuilt}/bin/i686-linux-android24-clang++"
export RANLIB_i686_linux_android="${toolchain_prebuilt}/bin/llvm-ranlib"

android_page_size_flags="-C link-arg=-Wl,-z,max-page-size=16384"
export CARGO_TARGET_AARCH64_LINUX_ANDROID_RUSTFLAGS="${CARGO_TARGET_AARCH64_LINUX_ANDROID_RUSTFLAGS:-${android_page_size_flags}}"
export CARGO_TARGET_ARMV7_LINUX_ANDROIDEABI_RUSTFLAGS="${CARGO_TARGET_ARMV7_LINUX_ANDROIDEABI_RUSTFLAGS:-${android_page_size_flags}}"
export CARGO_TARGET_I686_LINUX_ANDROID_RUSTFLAGS="${CARGO_TARGET_I686_LINUX_ANDROID_RUSTFLAGS:-${android_page_size_flags}}"
export CARGO_TARGET_X86_64_LINUX_ANDROID_RUSTFLAGS="${CARGO_TARGET_X86_64_LINUX_ANDROID_RUSTFLAGS:-${android_page_size_flags}}"

version="$(jq -r '.version' "${TAURI_DIR}/tauri.conf.json")"
version_code="$(jq -r '.bundle.android.versionCode' "${TAURI_DIR}/tauri.conf.json")"
mkdir -p "${TAURI_DIR}/gen/android/app"
cat > "${TAURI_DIR}/gen/android/app/tauri.properties" <<EOF
// THIS IS AN AUTOGENERATED FILE. DO NOT EDIT THIS FILE DIRECTLY.
tauri.android.versionName=${version}
tauri.android.versionCode=${version_code}
EOF

repro_dir="${TAURI_DIR}/target/reproducibility"
mkdir -p "${repro_dir}"

android_build_config='{"build":{"beforeBuildCommand":null}}'

remove_android_outputs() {
  remove_build_tree "${TAURI_DIR}/gen/android/app/build/outputs/apk"
  remove_build_tree "${TAURI_DIR}/gen/android/app/build/outputs/bundle"
}

build_android_release_outputs() {
  cd "${FRONTEND_DIR}"
  bun tauri android build --apk --aab --ci --config "${android_build_config}"
}

build_android_unsigned_release_outputs() {
  ORG_GRADLE_PROJECT_mapleUnsignedRelease=true build_android_release_outputs
}

collect_android_artifacts() {
  find "${TAURI_DIR}/gen/android/app/build/outputs" -type f \( -name '*.apk' -o -name '*.aab' \) -print0 \
    | LC_ALL=C sort -z
}

android_artifact_compare_key() {
  local artifact="$1"
  local base
  base="$(basename "${artifact}")"
  case "${base}" in
    *-unsigned.apk)
      base="${base%-unsigned.apk}.apk"
      ;;
  esac
  printf '%s\n' "${base}"
}

android_build_tool() {
  local tool="$1"
  local found

  if command -v "${tool}" >/dev/null 2>&1; then
    command -v "${tool}"
    return 0
  fi

  found="$(
    find "${ANDROID_HOME}/build-tools" -mindepth 2 -maxdepth 4 -type f \
      \( -name "${tool}" -o -name "${tool}.jar" \) 2>/dev/null \
      | LC_ALL=C sort -V \
      | tail -n 1 || true
  )"
  if [ -n "${found}" ]; then
    printf '%s\n' "${found}"
    return 0
  fi

  echo "Could not find Android build tool: ${tool}" >&2
  return 1
}

signed_android_apk_path() {
  local artifact="$1"

  case "${artifact}" in
    *-unsigned.apk)
      printf '%s.apk\n' "${artifact%-unsigned.apk}"
      ;;
    *)
      printf '%s\n' "${artifact}"
      ;;
  esac
}

sign_android_artifact() {
  local artifact="$1"
  local signed tmp apksigner
  local -a apksigner_cmd

  case "${artifact}" in
    *.apk)
      apksigner="$(android_build_tool apksigner)"
      case "${apksigner}" in
        *.jar)
          apksigner_cmd=(java -jar "${apksigner}")
          ;;
        *)
          apksigner_cmd=("${apksigner}")
          ;;
      esac
      signed="$(signed_android_apk_path "${artifact}")"
      tmp="${signed}.tmp"
      rm -f "${tmp}" "${signed}"
      "${apksigner_cmd[@]}" sign \
        --ks "${keystore_file}" \
        --ks-key-alias "${ANDROID_KEY_ALIAS}" \
        --ks-pass "pass:${ANDROID_KEY_PASSWORD}" \
        --key-pass "pass:${ANDROID_KEY_PASSWORD}" \
        --out "${tmp}" \
        "${artifact}"
      mv "${tmp}" "${signed}"
      if [ "${signed}" != "${artifact}" ]; then
        rm -f "${artifact}"
      fi
      signed_android_artifacts+=("${signed}")
      ;;
    *.aab)
      signed="${artifact}.signed"
      rm -f "${signed}"
      jarsigner \
        -keystore "${keystore_file}" \
        -storepass "${ANDROID_KEY_PASSWORD}" \
        -keypass "${ANDROID_KEY_PASSWORD}" \
        -sigalg SHA256withRSA \
        -digestalg SHA-256 \
        -signedjar "${signed}" \
        "${artifact}" \
        "${ANDROID_KEY_ALIAS}" >/dev/null
      mv "${signed}" "${artifact}"
      signed_android_artifacts+=("${artifact}")
      ;;
    *)
      echo "Unsupported Android artifact for signing: ${artifact}" >&2
      return 1
      ;;
  esac
}

sign_android_artifacts() {
  local artifact

  signed_android_artifacts=()
  decode_android_keystore
  for artifact in "$@"; do
    sign_android_artifact "${artifact}"
  done
}

write_android_payload_manifest() {
  local out="$1"
  shift

  : > "${out}"
  for artifact in "$@"; do
    print_zip_payload_hash "${artifact}" "$(repo_relative_path "${artifact}")" | tee -a "${out}"
  done
  test -s "${out}"
}

declare -A unsigned_payload_by_key=()
unsigned_artifacts=()
signed_android_artifacts=()

rm -f "${keystore_properties_file}"
remove_android_outputs
build_android_unsigned_release_outputs

while IFS= read -r -d '' file; do
  normalize_android_zip_metadata "${file}"
  unsigned_artifacts+=("${file}")
done < <(collect_android_artifacts)

write_android_payload_manifest "${repro_dir}/android-release-unsigned-canonical-payload.sha256" "${unsigned_artifacts[@]}"

if [ "${android_signing_enabled}" = "1" ] && [ "${MAPLE_ANDROID_COMPARE_UNSIGNED_RELEASE:-1}" = "1" ]; then
  for artifact in "${unsigned_artifacts[@]}"; do
    unsigned_payload_by_key["$(android_artifact_compare_key "${artifact}")"]="$(zip_payload_hash_digest "${artifact}")"
  done
fi

if [ "${android_signing_enabled}" = "1" ]; then
  sign_android_artifacts "${unsigned_artifacts[@]}"
  android_artifacts=("${signed_android_artifacts[@]}")
else
  android_artifacts=("${unsigned_artifacts[@]}")
fi

write_sha256_manifest "${repro_dir}/android-release-final.sha256" "${android_artifacts[@]}"
print_file_hashes "${android_artifacts[@]}"

write_android_payload_manifest "${repro_dir}/android-release-canonical-payload.sha256" "${android_artifacts[@]}"

if [ "${#unsigned_payload_by_key[@]}" -gt 0 ]; then
  for artifact in "${android_artifacts[@]}"; do
    key="$(android_artifact_compare_key "${artifact}")"
    signed_payload="$(zip_payload_hash_digest "${artifact}")"
    unsigned_payload="${unsigned_payload_by_key[${key}]:-}"

    if [ -z "${unsigned_payload}" ]; then
      echo "Missing unsigned Android payload baseline for ${key}." >&2
      exit 1
    fi

    if [ "${signed_payload}" != "${unsigned_payload}" ]; then
      echo "Signed Android artifact does not strip back to the unsigned payload for ${key}." >&2
      echo "unsigned=${unsigned_payload}" >&2
      echo "signed_canonical=${signed_payload}" >&2
      exit 1
    fi

    printf 'verified-android-signed-payload  %s  %s\n' "${signed_payload}" "$(repo_relative_path "${artifact}")"
  done
fi

verify_frontend_dist_unchanged
