#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "${SCRIPT_DIR}/../.." && pwd)"
FRONTEND_DIR="${REPO_ROOT}/frontend"
TAURI_DIR="${FRONTEND_DIR}/src-tauri"

source "${TAURI_DIR}/scripts/onnxruntime-pins.sh"

export CARGO_TERM_COLOR="${CARGO_TERM_COLOR:-always}"

install_frontend_deps() {
  disable_bun_env_files
  cd "${FRONTEND_DIR}"
  rm -rf node_modules
  bun install --frozen-lockfile --ignore-scripts
}

disable_bun_env_files() {
  if [ -n "${MAPLE_BUN_NO_ENV_FILE:-}" ]; then
    export MAPLE_IGNORE_VITE_ENV_FILES=1
    return 0
  fi

  local real_bun wrapper_dir frontend_dir
  real_bun="$(command -v bun)"
  frontend_dir="${FRONTEND_DIR}"
  wrapper_dir="$(mktemp -d)"

  cat > "${wrapper_dir}/bun" <<EOF
#!/usr/bin/env bash
set -euo pipefail

real_bun="$(printf '%q' "${real_bun}")"
frontend_dir="$(printf '%q' "${frontend_dir}")"
restore_dir=""
moved_env_files=()

restore_frontend_env_files() {
  local name

  if [ -z "\${restore_dir}" ]; then
    return 0
  fi

  for name in "\${moved_env_files[@]}"; do
    if [ -e "\${restore_dir}/\${name}" ] || [ -L "\${restore_dir}/\${name}" ]; then
      mv -- "\${restore_dir}/\${name}" "\${frontend_dir}/\${name}"
    fi
  done

  rmdir "\${restore_dir}" 2>/dev/null || true
}

hide_frontend_env_files() {
  local cwd name

  cwd="\$(pwd -P)"
  case "\${cwd}" in
    "\${frontend_dir}" | "\${frontend_dir}"/*)
      ;;
    *)
      return 0
      ;;
  esac

  restore_dir="\$(mktemp -d "\${frontend_dir}/.maple-bun-env-hide.XXXXXX")"
  for name in \
    .env \
    .env.local \
    .env.development \
    .env.development.local \
    .env.production \
    .env.production.local \
    .env.test \
    .env.test.local; do
    if [ -e "\${frontend_dir}/\${name}" ] || [ -L "\${frontend_dir}/\${name}" ]; then
      mv -- "\${frontend_dir}/\${name}" "\${restore_dir}/\${name}"
      moved_env_files+=("\${name}")
    fi
  done
}

hide_frontend_env_files
trap restore_frontend_env_files EXIT HUP INT TERM

set +e
"\${real_bun}" --no-env-file "\$@"
status="\$?"
set -e
exit "\${status}"
EOF
  chmod +x "${wrapper_dir}/bun"

  export MAPLE_BUN_NO_ENV_FILE=1
  export MAPLE_IGNORE_VITE_ENV_FILES=1
  export PATH="${wrapper_dir}:${PATH}"
}

configure_sccache() {
  if command -v sccache >/dev/null 2>&1; then
    local os socket_root
    os="$(host_os)"
    socket_root="${TMPDIR:-/tmp}"

    export RUSTC_WRAPPER="${RUSTC_WRAPPER:-sccache}"
    export SCCACHE_CACHE_SIZE="${SCCACHE_CACHE_SIZE:-2G}"
    export SCCACHE_SERVER_UDS="${SCCACHE_SERVER_UDS:-${socket_root%/}/maple-sccache-${os}.sock}"
    export CARGO_CACHE_RUSTC_INFO="${CARGO_CACHE_RUSTC_INFO:-0}"

    case "${os}" in
      darwin)
        export SCCACHE_DIR="${SCCACHE_DIR:-${HOME}/Library/Caches/Mozilla.sccache}"
        ;;
      *)
        export SCCACHE_DIR="${SCCACHE_DIR:-${HOME}/.cache/sccache}"
        ;;
    esac
  fi
}

use_pr_environment() {
  local name
  while IFS='=' read -r name _; do
    case "${name}" in
      VITE_*)
        unset "${name}"
        ;;
    esac
  done < <(env)

  export VITE_OPEN_SECRET_API_URL="https://enclave.secretgpt.ai"
  export VITE_MAPLE_BILLING_API_URL="https://billing-dev.opensecret.cloud"
  export VITE_CLIENT_ID="ba5a14b5-d915-47b1-b7b1-afda52bc5fc6"
}

use_release_environment() {
  local name
  while IFS='=' read -r name _; do
    case "${name}" in
      VITE_*)
        unset "${name}"
        ;;
    esac
  done < <(env)

  export VITE_OPEN_SECRET_API_URL="https://enclave.trymaple.ai"
  export VITE_MAPLE_BILLING_API_URL="https://billing.opensecret.cloud"
  export VITE_CLIENT_ID="ba5a14b5-d915-47b1-b7b1-afda52bc5fc6"
}

configure_reproducible_build_metadata() {
  if [ -z "${SOURCE_DATE_EPOCH:-}" ]; then
    SOURCE_DATE_EPOCH="315532800"
    export SOURCE_DATE_EPOCH
  fi

  export ZERO_AR_DATE="${ZERO_AR_DATE:-1}"
  configure_reproducible_rust_paths
  configure_reproducible_native_paths
  echo "SOURCE_DATE_EPOCH=${SOURCE_DATE_EPOCH}"
}

print_source_provenance() {
  if ! command -v git >/dev/null 2>&1; then
    return 0
  fi

  if ! git -C "${REPO_ROOT}" rev-parse --is-inside-work-tree >/dev/null 2>&1; then
    return 0
  fi

  printf 'git-commit  %s\n' "$(git -C "${REPO_ROOT}" rev-parse HEAD)"
  printf 'git-tree  %s\n' "$(git -C "${REPO_ROOT}" rev-parse 'HEAD^{tree}')"

  if ! git -C "${REPO_ROOT}" diff --quiet --ignore-submodules --; then
    echo "git-worktree-dirty  unstaged"
  fi

  if ! git -C "${REPO_ROOT}" diff --cached --quiet --ignore-submodules --; then
    echo "git-worktree-dirty  staged"
  fi
}

append_env_word_once() {
  local var_name="$1"
  local word="$2"
  local current

  current="${!var_name:-}"
  case " ${current} " in
    *" ${word} "*)
      ;;
    *)
      printf -v "${var_name}" '%s' "${current:+${current} }${word}"
      export "${var_name}"
      ;;
  esac
}

append_rustflag_once() {
  local flag="$1"

  append_env_word_once RUSTFLAGS "${flag}"
}

append_rust_remap_path_prefix() {
  local from="$1"
  local to="$2"

  if [ -z "${from}" ] || [ ! -d "${from}" ]; then
    return 0
  fi

  append_rustflag_once "--remap-path-prefix=${from}=${to}"
}

configure_reproducible_rust_paths() {
  local cargo_home rustup_home

  append_rust_remap_path_prefix "${REPO_ROOT}" "/maple"

  cargo_home="${CARGO_HOME:-${HOME:-}/.cargo}"
  append_rust_remap_path_prefix "${cargo_home}" "/cargo"

  rustup_home="${RUSTUP_HOME:-${HOME:-}/.rustup}"
  append_rust_remap_path_prefix "${rustup_home}" "/rustup"
}

append_native_remap_path_prefix() {
  local from="$1"
  local to="$2"
  local flag var_name

  if [ -z "${from}" ] || [ ! -d "${from}" ]; then
    return 0
  fi

  for flag in \
    "-ffile-prefix-map=${from}=${to}" \
    "-fmacro-prefix-map=${from}=${to}" \
    "-fdebug-prefix-map=${from}=${to}"; do
    for var_name in \
      CFLAGS CXXFLAGS OBJCFLAGS OBJCXXFLAGS \
      CMAKE_C_FLAGS CMAKE_CXX_FLAGS CMAKE_OBJC_FLAGS CMAKE_OBJCXX_FLAGS; do
      append_env_word_once "${var_name}" "${flag}"
    done
  done
}

configure_reproducible_native_paths() {
  local cargo_home rustup_home

  append_native_remap_path_prefix "${REPO_ROOT}" "/maple"

  cargo_home="${CARGO_HOME:-${HOME:-}/.cargo}"
  append_native_remap_path_prefix "${cargo_home}" "/cargo"

  rustup_home="${RUSTUP_HOME:-${HOME:-}/.rustup}"
  append_native_remap_path_prefix "${rustup_home}" "/rustup"
}

configure_tauri_updater_signing_key() {
  if [ -n "${TAURI_SIGNING_PRIVATE_KEY:-}" ]; then
    unset TAURI_SIGNING_PRIVATE_KEY_PATH
    return 0
  fi

  if [ -n "${TAURI_SIGNING_PRIVATE_KEY_PATH:-}" ]; then
    if [ ! -f "${TAURI_SIGNING_PRIVATE_KEY_PATH}" ]; then
      echo "TAURI_SIGNING_PRIVATE_KEY_PATH does not exist: ${TAURI_SIGNING_PRIVATE_KEY_PATH}" >&2
      return 1
    fi
    # tauri build requires the key content even though tauri signer sign accepts a path.
    export TAURI_SIGNING_PRIVATE_KEY="$(cat "${TAURI_SIGNING_PRIVATE_KEY_PATH}")"
    unset TAURI_SIGNING_PRIVATE_KEY_PATH
  fi
}

generate_fake_tauri_updater_keypair() {
  local key_dir private_key password

  key_dir="$(mktemp -d)"
  private_key="${key_dir}/tauri-updater.key"
  password="${MAPLE_TAURI_FAKE_UPDATER_PASSWORD:-maple-pr-fake-updater-key}"

  (
    cd "${FRONTEND_DIR}"
    bun tauri signer generate \
      --ci \
      --password "${password}" \
      --write-keys "${private_key}" \
      --force >/dev/null
  )

  unset TAURI_SIGNING_PRIVATE_KEY
  export TAURI_SIGNING_PRIVATE_KEY_PATH="${private_key}"
  export TAURI_SIGNING_PRIVATE_KEY_PASSWORD="${password}"
  export MAPLE_TAURI_UPDATER_PUBLIC_KEY_PATH="${private_key}.pub"

  echo "generated-fake-tauri-updater-keypair  ephemeral"
}

source_date_rfc3339() {
  date -u -d "@${SOURCE_DATE_EPOCH:?SOURCE_DATE_EPOCH is required}" +"%Y-%m-%dT%H:%M:%SZ"
}

is_valid_xcode_developer_dir() {
  local dir="${1:-}"
  [ -n "${dir}" ] \
    && [ -x "${dir}/usr/bin/simctl" ] \
    && [ -x "${dir}/Toolchains/XcodeDefault.xctoolchain/usr/bin/clang" ]
}

xcode_version_for_developer_dir() {
  local dir="${1:?developer dir is required}"
  DEVELOPER_DIR="${dir}" /usr/bin/xcodebuild -version 2>/dev/null | awk '/^Xcode / { print $2; exit }'
}

xcode_version_matches_expected() {
  local version="${1:-}"
  local expected="${2:-}"

  [ "${version}" = "${expected}" ] \
    || [ "${version}" = "${expected}.0" ] \
    || [ "${version}.0" = "${expected}" ]
}

resolve_xcode_developer_dir() {
  local expected="${MAPLE_NIX_XCODE_VERSION:-}"
  local candidate version
  local -a candidates=()

  if [ -n "${DEVELOPER_DIR:-}" ]; then
    candidates+=("${DEVELOPER_DIR}")
  fi

  if [ -n "${expected}" ]; then
    candidates+=(
      "/Applications/Xcode-${expected}.app/Contents/Developer"
      "/Applications/Xcode-${expected}.0.app/Contents/Developer"
      "/Applications/Xcode_${expected}.app/Contents/Developer"
      "/Applications/Xcode_${expected}.0.app/Contents/Developer"
    )
  fi

  candidate="$(xcode-select -p 2>/dev/null || true)"
  if [ -n "${candidate}" ]; then
    candidates+=("${candidate}")
  fi
  candidates+=("/Applications/Xcode.app/Contents/Developer")

  for candidate in "${candidates[@]}"; do
    if ! is_valid_xcode_developer_dir "${candidate}"; then
      continue
    fi

    if [ -n "${expected}" ]; then
      version="$(xcode_version_for_developer_dir "${candidate}")"
      if ! xcode_version_matches_expected "${version}" "${expected}"; then
        continue
      fi
    fi

    printf '%s\n' "${candidate}"
    return 0
  done

  if [ -n "${expected}" ]; then
    echo "Could not find Xcode ${expected}. Install it or select it with xcode-select." >&2
  else
    echo "Could not find a full Xcode installation." >&2
  fi
  return 1
}

use_xcode_toolchain() {
  if [ "$(host_os)" != "darwin" ]; then
    return 0
  fi

  local dev_dir toolchain_bin macos_sdkroot macos_link_flags xcode_info
  dev_dir="$(resolve_xcode_developer_dir)"
  toolchain_bin="${dev_dir}/Toolchains/XcodeDefault.xctoolchain/usr/bin"
  macos_sdkroot="$(DEVELOPER_DIR="${dev_dir}" /usr/bin/xcrun --sdk macosx --show-sdk-path)"
  macos_link_flags="-C link-arg=-isysroot -C link-arg=${macos_sdkroot}"
  xcode_info="$(DEVELOPER_DIR="${dev_dir}" /usr/bin/xcodebuild -version)"

  export DEVELOPER_DIR="${dev_dir}"
  export MAPLE_XCODE_VERSION="$(printf '%s\n' "${xcode_info}" | awk '/^Xcode / { print $2; exit }')"
  export MAPLE_XCODE_BUILD_VERSION="$(printf '%s\n' "${xcode_info}" | awk '/^Build version / { print $3; exit }')"
  export CC="/usr/bin/clang"
  export CXX="/usr/bin/clang++"
  export AR="/usr/bin/ar"
  export RANLIB="/usr/bin/ranlib"
  export PATH="${toolchain_bin}:${PATH}:${dev_dir}/usr/bin:/usr/bin"

  if command -v sw_vers >/dev/null 2>&1; then
    printf 'Using macOS %s build %s\n' "$(sw_vers -productVersion)" "$(sw_vers -buildVersion)"
  fi
  printf 'Using Xcode %s build %s from %s\n' "${MAPLE_XCODE_VERSION}" "${MAPLE_XCODE_BUILD_VERSION}" "${dev_dir}"

  if [ -n "${MAPLE_NIX_LIBICONV:-}" ] && [ -d "${MAPLE_NIX_LIBICONV}/lib" ]; then
    export LIBRARY_PATH="${MAPLE_NIX_LIBICONV}/lib${LIBRARY_PATH:+:${LIBRARY_PATH}}"
  fi

  export CARGO_TARGET_AARCH64_APPLE_DARWIN_LINKER="${CC}"
  export CARGO_TARGET_X86_64_APPLE_DARWIN_LINKER="${CC}"
  export CARGO_TARGET_AARCH64_APPLE_IOS_LINKER="${CC}"
  export CARGO_TARGET_AARCH64_APPLE_IOS_SIM_LINKER="${CC}"
  export CARGO_TARGET_AARCH64_APPLE_DARWIN_RUSTFLAGS="${CARGO_TARGET_AARCH64_APPLE_DARWIN_RUSTFLAGS:+${CARGO_TARGET_AARCH64_APPLE_DARWIN_RUSTFLAGS} }${macos_link_flags}"
  export CARGO_TARGET_X86_64_APPLE_DARWIN_RUSTFLAGS="${CARGO_TARGET_X86_64_APPLE_DARWIN_RUSTFLAGS:+${CARGO_TARGET_X86_64_APPLE_DARWIN_RUSTFLAGS} }${macos_link_flags}"

  export CC_aarch64_apple_darwin="${CC}"
  export CXX_aarch64_apple_darwin="${CXX}"
  export AR_aarch64_apple_darwin="${AR}"
  export RANLIB_aarch64_apple_darwin="${RANLIB}"

  export CC_x86_64_apple_darwin="${CC}"
  export CXX_x86_64_apple_darwin="${CXX}"
  export AR_x86_64_apple_darwin="${AR}"
  export RANLIB_x86_64_apple_darwin="${RANLIB}"

  export CC_aarch64_apple_ios="${CC}"
  export CXX_aarch64_apple_ios="${CXX}"
  export AR_aarch64_apple_ios="${AR}"
  export RANLIB_aarch64_apple_ios="${RANLIB}"

  export CC_aarch64_apple_ios_sim="${CC}"
  export CXX_aarch64_apple_ios_sim="${CXX}"
  export AR_aarch64_apple_ios_sim="${AR}"
  export RANLIB_aarch64_apple_ios_sim="${RANLIB}"

  unset NIX_CC NIX_CC_FOR_BUILD NIX_BINTOOLS NIX_BINTOOLS_FOR_BUILD
  unset NIX_CFLAGS_COMPILE NIX_CFLAGS_COMPILE_FOR_BUILD NIX_CFLAGS_LINK NIX_CFLAGS_LINK_FOR_BUILD
  unset NIX_LDFLAGS NIX_LDFLAGS_FOR_BUILD NIX_LDFLAGS_BEFORE NIX_LDFLAGS_BEFORE_FOR_BUILD
  unset NIX_DONT_SET_RPATH NIX_DONT_SET_RPATH_FOR_BUILD NIX_ENFORCE_NO_NATIVE
  unset NIX_HARDENING_ENABLE NIX_IGNORE_LD_THROUGH_GCC NIX_NO_SELF_RPATH NIXPKGS_CMAKE_PREFIX_PATH
  unset LD CFLAGS CXXFLAGS CPPFLAGS LDFLAGS

  local name
  while IFS='=' read -r name _; do
    case "${name}" in
      NIX_CC_WRAPPER_* | NIX_BINTOOLS_WRAPPER_* | NIX_PKG_CONFIG_WRAPPER_*)
        unset "${name}"
        ;;
    esac
  done < <(env)

  configure_reproducible_native_paths
}

require_ios_simulator_runtime_for_xcode() {
  if [ "$(host_os)" != "darwin" ]; then
    return 0
  fi

  local dev_dir sdk_settings version runtimes_json
  dev_dir="$(resolve_xcode_developer_dir)"
  sdk_settings="${dev_dir}/Platforms/iPhoneSimulator.platform/Developer/SDKs/iPhoneSimulator.sdk/SDKSettings.plist"

  if [ ! -f "${sdk_settings}" ]; then
    echo "iPhoneSimulator SDKSettings.plist is missing: ${sdk_settings}" >&2
    return 1
  fi

  version="$(/usr/libexec/PlistBuddy -c 'Print :Version' "${sdk_settings}")"
  runtimes_json="$(DEVELOPER_DIR="${dev_dir}" /usr/bin/xcrun simctl list runtimes --json)"

  if ! SIMCTL_RUNTIMES_JSON="${runtimes_json}" python3 - "${version}" >/dev/null <<'PY'
import json
import os
import sys

required = sys.argv[1]

def version_tuple(value):
    parts = []
    for part in value.split("."):
        try:
            parts.append(int(part))
        except ValueError:
            parts.append(0)
    while len(parts) < 3:
        parts.append(0)
    return tuple(parts)

required_tuple = version_tuple(required)
for runtime in json.loads(os.environ["SIMCTL_RUNTIMES_JSON"]).get("runtimes", []):
    if not str(runtime.get("name", "")).startswith("iOS"):
        continue
    if runtime.get("isAvailable") is False:
        continue
    if version_tuple(str(runtime.get("version", ""))) >= required_tuple:
        sys.exit(0)
sys.exit(1)
PY
  then
    echo "iOS Simulator runtime ${version} or newer is required for Xcode $(xcode_version_for_developer_dir "${dev_dir}")." >&2
    echo "Install it with: xcodebuild -downloadPlatform iOS" >&2
    DEVELOPER_DIR="${dev_dir}" /usr/bin/xcrun simctl list runtimes >&2 || true
    return 1
  fi
}

host_os() {
  uname -s | tr '[:upper:]' '[:lower:]'
}

linux_ort_arch() {
  case "$(uname -m)" in
    x86_64 | amd64)
      printf '%s\n' "x64"
      ;;
    aarch64 | arm64)
      printf '%s\n' "aarch64"
      ;;
    *)
      echo "Unsupported Linux ONNX Runtime architecture: $(uname -m)" >&2
      return 1
      ;;
  esac
}

prepare_linux_onnxruntime() {
  if [ "$(host_os)" != "linux" ]; then
    return 0
  fi

  local ort_env
  ort_env="$("${TAURI_DIR}/scripts/provide-linux-onnxruntime.sh")"
  printf '%s\n' "${ort_env}"

  local key value
  while IFS='=' read -r key value; do
    case "${key}" in
      ORT_LIB_LOCATION | ORT_SKIP_DOWNLOAD | ORT_DYLIB_PATH)
        export "${key}=${value}"
        ;;
    esac
  done <<< "${ort_env}"
}

sha256_file() {
  if command -v sha256sum >/dev/null 2>&1; then
    sha256sum "$1"
  else
    shasum -a 256 "$1"
  fi
}

sha256_stream() {
  if command -v sha256sum >/dev/null 2>&1; then
    sha256sum
  else
    shasum -a 256
  fi
}

decode_base64_string_to_file() {
  local value="$1"
  local out="$2"

  if printf '%s' "${value}" | base64 --decode > "${out}" 2>/dev/null; then
    return 0
  fi

  printf '%s' "${value}" | base64 -D > "${out}"
}

decode_base64_file_to_file() {
  local in_file="$1"
  local out_file="$2"

  if base64 --decode "${in_file}" > "${out_file}" 2>/dev/null; then
    return 0
  fi

  base64 -D -i "${in_file}" > "${out_file}"
}

encode_base64_file_to_string() {
  base64 < "$1" | tr -d '\n'
}

repo_relative_path() {
  local path="$1"
  printf '%s\n' "${path#${REPO_ROOT}/}"
}

print_file_hashes() {
  local file digest

  for file in "$@"; do
    if [ ! -f "${file}" ]; then
      continue
    fi

    digest="$(sha256_file "${file}" | awk '{ print $1 }')"
    printf 'sha256-file  %s  %s\n' "${digest}" "$(repo_relative_path "${file}")"
  done
}

tree_hash_digest() {
  local dir="$1"

  (
    cd "${dir}"
    find . -type f -print0 \
      | LC_ALL=C sort -z \
      | while IFS= read -r -d '' file; do
          file="${file#./}"
          printf '%s  %s\n' "$(sha256_file "${file}" | awk '{ print $1 }')" "${file}"
        done \
      | sha256_stream \
      | awk '{ print $1 }'
  )
}

print_canonical_ios_app_hash() {
  local bundle="$1"
  local label="${2:-$(repo_relative_path "${bundle}")}"
  local digest

  if [ ! -d "${bundle}" ]; then
    return 0
  fi

  digest="$(python3 "${REPO_ROOT}/scripts/ci/canonical-ios-app-hash.py" "${bundle}")"
  printf 'sha256-ios-app-canonical  %s  %s\n' "${digest}" "${label}"
}

scrub_host_metadata_files() {
  local root="$1"

  if [ ! -d "${root}" ]; then
    return 0
  fi

  find "${root}" \
    \( -name '.DS_Store' -o -name '._*' -o -name 'Thumbs.db' -o -name 'Desktop.ini' \) \
    -type f -print0 \
    | while IFS= read -r -d '' file; do
        rm -f "${file}"
      done
}

strip_apple_debug_symbols() {
  local root="$1"
  local file

  if [ ! -d "${root}" ] || ! command -v strip >/dev/null 2>&1; then
    return 0
  fi

  while IFS= read -r -d '' file; do
    if file "${file}" | grep -q 'Mach-O'; then
      strip -S "${file}"
    fi
  done < <(find "${root}" -type f -print0 | LC_ALL=C sort -z)
}

adhoc_resign_apple_bundle() {
  local bundle="$1"

  if [ ! -d "${bundle}" ] || ! command -v codesign >/dev/null 2>&1; then
    return 0
  fi

  codesign --force --sign - --timestamp=none --generate-entitlement-der "${bundle}"
}

remove_build_tree() {
  local root="$1"

  if [ ! -e "${root}" ]; then
    return 0
  fi

  chmod -R u+w "${root}" 2>/dev/null || true
  rm -rf "${root}"
}

linux_runtime_library_path() {
  local paths_file="${MAPLE_NIX_LINUX_CLOSURE_INFO:-}/store-paths"
  local lib_dirs=()
  local store_path lib_dir

  if [ -f "${paths_file}" ]; then
    while IFS= read -r store_path; do
      for lib_dir in "${store_path}/lib" "${store_path}/lib64"; do
        if [ -d "${lib_dir}" ]; then
          lib_dirs+=("${lib_dir}")
        fi
      done
    done < "${paths_file}"
  fi

  if [ -n "${MAPLE_NIX_GCC_LIB:-}" ] && [ -d "${MAPLE_NIX_GCC_LIB}/lib" ]; then
    lib_dirs+=("${MAPLE_NIX_GCC_LIB}/lib")
  fi

  if [ "${#lib_dirs[@]}" -eq 0 ]; then
    return 0
  fi

  printf '%s\n' "${lib_dirs[@]}" | awk '!seen[$0]++' | paste -sd ':' -
}

prepend_linux_runtime_library_path() {
  local library_path
  library_path="$(linux_runtime_library_path)"

  if [ -n "${library_path}" ]; then
    export LD_LIBRARY_PATH="${library_path}${LD_LIBRARY_PATH:+:${LD_LIBRARY_PATH}}"
  fi
}

linuxdeploy_tools_arch() {
  case "$(uname -m)" in
    x86_64 | amd64)
      printf '%s\n' "x86_64"
      ;;
    aarch64 | arm64)
      printf '%s\n' "aarch64"
      ;;
    *)
      echo "Unsupported Linux AppImage tool architecture: $(uname -m)" >&2
      return 1
      ;;
  esac
}

prepare_tauri_linuxdeploy_tools_cache() {
  if [ "$(host_os)" != "linux" ]; then
    return 0
  fi

  if [ -z "${MAPLE_NIX_TAURI_LINUXDEPLOY_TOOLS:-}" ]; then
    echo "MAPLE_NIX_TAURI_LINUXDEPLOY_TOOLS is required for reproducible AppImage bundling." >&2
    return 1
  fi

  local arch linuxdeploy_arch tools cache wrapper
  arch="$(linuxdeploy_tools_arch)"
  linuxdeploy_arch="${arch}"
  tools="${MAPLE_NIX_TAURI_LINUXDEPLOY_TOOLS}"
  cache="${TAURI_DIR}/target/.tauri"
  wrapper="${cache}/linuxdeploy-plugin-appimage.AppImage"

  mkdir -p "${cache}"
  install -m 0755 "${tools}/AppRun-${arch}" "${cache}/AppRun-${arch}"
  install -m 0755 "${tools}/linuxdeploy-${linuxdeploy_arch}.AppImage" "${cache}/linuxdeploy-${linuxdeploy_arch}.AppImage"
  install -m 0755 "${tools}/linuxdeploy-plugin-appimage.real.AppImage" "${cache}/linuxdeploy-plugin-appimage.real.AppImage"
  install -m 0755 "${tools}/linuxdeploy-plugin-gtk.sh" "${cache}/linuxdeploy-plugin-gtk.sh"
  install -m 0755 "${tools}/linuxdeploy-plugin-gstreamer.sh" "${cache}/linuxdeploy-plugin-gstreamer.sh"

  cat > "${wrapper}" <<'EOF'
#!/usr/bin/env bash
set -euo pipefail

for arg in "$@"; do
  case "${arg}" in
    --plugin-type)
      printf '%s\n' output
      exit 0
      ;;
    --plugin-api-version)
      printf '%s\n' 0
      exit 0
      ;;
  esac
done

appdir=""
previous=""
for arg in "$@"; do
  if [ "${previous}" = "--appdir" ]; then
    appdir="${arg}"
    previous=""
    continue
  fi

  case "${arg}" in
    --appdir=*)
      appdir="${arg#--appdir=}"
      ;;
    --appdir)
      previous="--appdir"
      ;;
  esac
done

if [ -n "${appdir}" ]; then
  rm -f "${appdir}/.DirIcon"
fi

script_dir="$(CDPATH= cd -- "$(dirname -- "$0")" && pwd)"
real_plugin="${script_dir}/linuxdeploy-plugin-appimage.real.AppImage"

if [ ! -x "${real_plugin}" ]; then
  echo "Missing real linuxdeploy AppImage plugin at ${real_plugin}" >&2
  exit 1
fi

APPIMAGE_EXTRACT_AND_RUN=1 exec "${real_plugin}" --appimage-extract-and-run "$@"
EOF
  chmod +x "${wrapper}"

  print_file_hashes \
    "${cache}/AppRun-${arch}" \
    "${cache}/linuxdeploy-${linuxdeploy_arch}.AppImage" \
    "${cache}/linuxdeploy-plugin-appimage.real.AppImage" \
    "${cache}/linuxdeploy-plugin-appimage.AppImage" \
    "${cache}/linuxdeploy-plugin-gtk.sh" \
    "${cache}/linuxdeploy-plugin-gstreamer.sh"
}

print_tree_hash() {
  local dir="$1"
  local label="${2:-$(repo_relative_path "${dir}")}"
  local digest

  if [ ! -d "${dir}" ]; then
    return 0
  fi

  digest="$(tree_hash_digest "${dir}")"

  printf 'sha256-tree  %s  %s\n' "${digest}" "${label}"
}

write_sha256_manifest() {
  local out="$1"
  shift

  mkdir -p "$(dirname "${out}")"
  : > "${out}"

  local file digest
  for file in "$@"; do
    if [ -f "${file}" ]; then
      digest="$(sha256_file "${file}" | awk '{ print $1 }')"
      printf '%s  %s\n' "${digest}" "$(repo_relative_path "${file}")" >> "${out}"
    fi
  done

  test -s "${out}"
  cat "${out}"
}

print_zip_payload_hash() {
  local archive="$1"
  local label="${2:-$(repo_relative_path "${archive}")}"
  local digest

  digest="$(zip_payload_hash_digest "${archive}")"

  printf 'sha256-zip-payload  %s  %s\n' "${digest}" "${label}"
}

zip_payload_hash_digest() {
  local archive="$1"

  unzip -Z1 "${archive}" \
    | LC_ALL=C sort \
    | while IFS= read -r entry; do
        case "${entry}" in
          */)
            continue
            ;;
          META-INF/MANIFEST.MF | META-INF/*.SF | META-INF/*.RSA | META-INF/*.DSA | META-INF/*.EC)
            continue
            ;;
        esac

        printf '%s  %s\n' "$(zip_payload_entry_hash_digest "${archive}" "${entry}")" "${entry}"
      done \
    | sha256_stream \
    | awk '{ print $1 }'
}

zip_payload_entry_hash_digest() {
  local archive="$1"
  local entry="$2"

  case "${entry}" in
    assets/tauri.conf.json | */assets/tauri.conf.json)
      unzip -p "${archive}" "${entry}" | jq -cS . | sha256_stream | awk '{ print $1 }'
      ;;
    *)
      unzip -p "${archive}" "${entry}" | sha256_stream | awk '{ print $1 }'
      ;;
  esac
}

normalize_zip_json_entry() {
  local archive="$1"
  local entry="$2"
  local tmp

  if ! unzip -Z1 "${archive}" | grep -Fx -- "${entry}" >/dev/null; then
    return 0
  fi

  command -v zip >/dev/null 2>&1 || {
    echo "zip is required to normalize ${entry} in ${archive}. Run through the flake CI shell." >&2
    return 1
  }

  tmp="$(mktemp -d)"
  mkdir -p "${tmp}/$(dirname "${entry}")"
  unzip -p "${archive}" "${entry}" | jq -cS . > "${tmp}/${entry}"
  touch_tree_to_source_date_epoch "${tmp}"

  (
    cd "${tmp}"
    zip -X -q "${archive}" "${entry}"
  )

  rm -rf "${tmp}"
}

normalize_android_zip_metadata() {
  local archive="$1"

  normalize_zip_json_entry "${archive}" "assets/tauri.conf.json"
  normalize_zip_json_entry "${archive}" "base/assets/tauri.conf.json"
}

remove_apple_signing_metadata() {
  local root="$1"

  scrub_host_metadata_files "${root}"

  while IFS= read -r -d '' dir; do
    rm -rf "${dir}"
  done < <(find "${root}" -type d -name '_CodeSignature' -print0)

  find "${root}" \
    \( -name 'CodeResources' \
      -o -name 'embedded.mobileprovision' \
      -o -name 'archived-expanded-entitlements.xcent' \) \
    -type f -delete

  if command -v codesign >/dev/null 2>&1; then
    while IFS= read -r -d '' bundle; do
      codesign --remove-signature "${bundle}" >/dev/null 2>&1 || true
    done < <(find "${root}" -type d \( -name '*.app' -o -name '*.appex' -o -name '*.framework' -o -name '*.xpc' \) -print0 | LC_ALL=C sort -z -r)

    while IFS= read -r -d '' file; do
      if file "${file}" | grep -q 'Mach-O'; then
        codesign --remove-signature "${file}" >/dev/null 2>&1 || true
      fi
    done < <(find "${root}" -type f -print0 | LC_ALL=C sort -z)
  fi

  while IFS= read -r -d '' dir; do
    rm -rf "${dir}"
  done < <(find "${root}" -type d -name '_CodeSignature' -print0)

  scrub_host_metadata_files "${root}"
}

print_canonical_apple_bundle_hash() {
  local bundle="$1"
  local label="${2:-$(repo_relative_path "${bundle}")}"
  local digest

  if [ ! -d "${bundle}" ]; then
    return 0
  fi

  digest="$(canonical_apple_bundle_hash_from_path_digest "${bundle}")"

  printf 'sha256-apple-unsigned-tree  %s  %s\n' "${digest}" "${label}"
}

canonical_apple_bundle_hash_from_path_digest() {
  local bundle="$1"
  local tmp digest

  tmp="$(mktemp -d)"
  cp -a "${bundle}" "${tmp}/bundle"
  remove_apple_signing_metadata "${tmp}/bundle"
  digest="$(canonical_apple_bundle_hash_digest "${tmp}/bundle")"
  rm -rf "${tmp}"

  printf '%s\n' "${digest}"
}

canonical_apple_bundle_hash_digest() {
  local bundle="$1"
  python3 "${REPO_ROOT}/scripts/ci/canonical-ios-app-hash.py" "${bundle}"
}

find_canonical_app_bundle_under_dir() {
  local root="$1"
  find "${root}" -type d -name '*.app' -print 2>/dev/null | LC_ALL=C sort | head -n 1
}

print_canonical_app_tar_payload_hash() {
  local archive="$1"
  local label="${2:-$(repo_relative_path "${archive}")}"
  local tmp app digest

  if [ ! -f "${archive}" ]; then
    return 0
  fi

  tmp="$(mktemp -d)"
  tar -xzf "${archive}" -C "${tmp}"

  app="$(find_canonical_app_bundle_under_dir "${tmp}")"
  if [ -z "${app}" ]; then
    echo "Could not find a *.app bundle in ${archive}" >&2
    rm -rf "${tmp}"
    return 1
  fi

  digest="$(canonical_apple_bundle_hash_from_path_digest "${app}")"
  rm -rf "${tmp}"

  printf 'sha256-macos-app-tar-canonical  %s  %s::*.app\n' "${digest}" "${label}"
}

print_canonical_dmg_app_hash() {
  local dmg="$1"
  local label="${2:-$(repo_relative_path "${dmg}")}"
  local tmp mount app digest

  if [ ! -f "${dmg}" ]; then
    return 0
  fi

  if [ "$(host_os)" != "darwin" ]; then
    echo "DMG canonicalization requires macOS: ${dmg}" >&2
    return 1
  fi

  tmp="$(mktemp -d)"
  mount="${tmp}/mount"
  mkdir -p "${mount}"

  if ! hdiutil attach -nobrowse -readonly -mountpoint "${mount}" "${dmg}" >/dev/null; then
    rm -rf "${tmp}"
    return 1
  fi

  app="$(find_canonical_app_bundle_under_dir "${mount}")"
  if [ -z "${app}" ]; then
    hdiutil detach "${mount}" -quiet || hdiutil detach "${mount}" -force -quiet || true
    rm -rf "${tmp}"
    echo "Could not find a *.app bundle in ${dmg}" >&2
    return 1
  fi

  digest="$(canonical_apple_bundle_hash_from_path_digest "${app}")"
  hdiutil detach "${mount}" -quiet || hdiutil detach "${mount}" -force -quiet || true
  rm -rf "${tmp}"

  printf 'sha256-macos-dmg-canonical  %s  %s::*.app\n' "${digest}" "${label}"
}

print_canonical_ipa_payload_hash() {
  local ipa="$1"
  local label="${2:-$(repo_relative_path "${ipa}")}"
  local tmp app digest

  if [ ! -f "${ipa}" ]; then
    return 0
  fi

  tmp="$(mktemp -d)"
  unzip -qq "${ipa}" -d "${tmp}"

  app="$(find "${tmp}/Payload" -mindepth 1 -maxdepth 1 -type d -name '*.app' 2>/dev/null | LC_ALL=C sort | head -n 1)"
  if [ -z "${app}" ]; then
    echo "Could not find a Payload/*.app bundle in ${ipa}" >&2
    rm -rf "${tmp}"
    return 1
  fi

  remove_apple_signing_metadata "${app}"
  digest="$(canonical_apple_bundle_hash_digest "${app}")"
  rm -rf "${tmp}"

  printf 'sha256-ios-unsigned-app-tree  %s  %s::Payload/*.app\n' "${digest}" "${label}"
}

tauri_updater_public_key_file() {
  local out="$1"
  local encoded decoded

  if [ -n "${MAPLE_TAURI_UPDATER_PUBLIC_KEY_PATH:-}" ]; then
    if [ ! -f "${MAPLE_TAURI_UPDATER_PUBLIC_KEY_PATH}" ]; then
      echo "MAPLE_TAURI_UPDATER_PUBLIC_KEY_PATH does not exist: ${MAPLE_TAURI_UPDATER_PUBLIC_KEY_PATH}" >&2
      return 1
    fi

    cp "${MAPLE_TAURI_UPDATER_PUBLIC_KEY_PATH}" "${out}"
  elif [ -n "${MAPLE_TAURI_UPDATER_PUBLIC_KEY:-}" ]; then
    printf '%s' "${MAPLE_TAURI_UPDATER_PUBLIC_KEY}" > "${out}"
  else
    encoded="$(jq -er '.plugins.updater.pubkey' "${TAURI_DIR}/tauri.conf.json")"
    decode_base64_string_to_file "${encoded}" "${out}"
  fi

  if grep -q 'minisign public key' "${out}"; then
    return 0
  fi

  decoded="$(mktemp)"
  if decode_base64_file_to_file "${out}" "${decoded}" && grep -q 'minisign public key' "${decoded}"; then
    mv "${decoded}" "${out}"
    return 0
  fi
  rm -f "${decoded}"

  echo "Configured Tauri updater public key is not a minisign public key." >&2
  return 1
}

tauri_updater_public_key_config_value() {
  local tmp encoded

  if [ -n "${MAPLE_TAURI_UPDATER_PUBLIC_KEY_PATH:-}" ] || [ -n "${MAPLE_TAURI_UPDATER_PUBLIC_KEY:-}" ]; then
    tmp="$(mktemp)"
    tauri_updater_public_key_file "${tmp}" || {
      rm -f "${tmp}"
      return 1
    }
    encoded="$(encode_base64_file_to_string "${tmp}")"
    rm -f "${tmp}"
    printf '%s\n' "${encoded}"
    return 0
  fi

  jq -er '.plugins.updater.pubkey' "${TAURI_DIR}/tauri.conf.json"
}

verify_tauri_updater_signature() {
  local artifact="$1"
  local signature="$2"
  local label="${3:-$(repo_relative_path "${artifact}")}"
  local tmp pubkey decoded_signature

  if [ ! -f "${artifact}" ]; then
    echo "Missing updater artifact for signature verification: ${artifact}" >&2
    return 1
  fi

  if [ ! -f "${signature}" ]; then
    echo "Missing updater signature for ${artifact}: ${signature}" >&2
    return 1
  fi

  command -v minisign >/dev/null 2>&1 || {
    echo "minisign is required to verify Tauri updater signatures. Run through the flake CI shell." >&2
    return 1
  }

  tmp="$(mktemp -d)"
  pubkey="${tmp}/tauri-updater.pub"
  decoded_signature="${tmp}/artifact.minisig"

  tauri_updater_public_key_file "${pubkey}"
  decode_base64_file_to_file "${signature}" "${decoded_signature}"

  if ! minisign -Vm "${artifact}" -p "${pubkey}" -x "${decoded_signature}" -q; then
    rm -rf "${tmp}"
    return 1
  fi
  rm -rf "${tmp}"

  printf 'verified-tauri-updater-signature  %s  %s\n' "${label}" "$(repo_relative_path "${signature}")"
}

sign_tauri_updater_artifact() {
  local artifact="$1"

  if [ ! -f "${artifact}" ]; then
    echo "Missing updater artifact to sign: ${artifact}" >&2
    return 1
  fi

  configure_tauri_updater_signing_key

  if [ -z "${TAURI_SIGNING_PRIVATE_KEY:-}" ] && [ -z "${TAURI_SIGNING_PRIVATE_KEY_PATH:-}" ]; then
    echo "TAURI_SIGNING_PRIVATE_KEY or TAURI_SIGNING_PRIVATE_KEY_PATH is required to sign ${artifact}." >&2
    return 1
  fi

  (
    cd "${FRONTEND_DIR}"
    bun tauri signer sign "${artifact}" >/dev/null
  )
}

sign_tauri_updater_artifacts() {
  local artifact

  for artifact in "$@"; do
    sign_tauri_updater_artifact "${artifact}"
  done
}

verify_tauri_updater_signature_files() {
  local signature artifact

  for signature in "$@"; do
    case "${signature}" in
      *.sig)
        ;;
      *)
        continue
        ;;
    esac

    if [ ! -f "${signature}" ]; then
      continue
    fi

    artifact="${signature%.sig}"
    verify_tauri_updater_signature "${artifact}" "${signature}" "$(repo_relative_path "${artifact}")"
  done
}

resolve_bwrap_visible_tool() {
  local candidate resolved

  for candidate in "$@"; do
    if [ -z "${candidate}" ]; then
      continue
    fi

    resolved="$(readlink -f "${candidate}" 2>/dev/null || printf '%s\n' "${candidate}")"
    case "${resolved}" in
      /usr/* | /bin/*)
        continue
        ;;
    esac

    printf '%s\n' "${resolved}"
    return 0
  done

  return 1
}

resolve_bwrap_visible_command() {
  local candidate name

  for name in "$@"; do
    while IFS= read -r candidate; do
      if resolve_bwrap_visible_tool "${candidate}"; then
        return 0
      fi
    done < <(type -a -P "${name}" 2>/dev/null || true)
  done

  return 1
}

run_with_nix_usr_bin() {
  if [ "$(host_os)" != "linux" ]; then
    "$@"
    return $?
  fi

  if [ -z "${MAPLE_NIX_LINUX_CLOSURE_INFO:-}" ] && [ -x /usr/bin/xdg-mime ] && [ -x /bin/bash ]; then
    "$@"
    return $?
  fi

  command -v bwrap >/dev/null 2>&1 || {
    echo "bubblewrap is required to provide Nix xdg-utils at /usr/bin for AppImage bundling." >&2
    return 1
  }

  local bash_path bin_dir tool_bin usr_root tool tool_path
  bash_path="$(command -v bash)"
  bin_dir="$(mktemp -d)"
  tool_bin="$(mktemp -d)"
  usr_root="$(mktemp -d)"
  mkdir -p "${usr_root}/bin" "${usr_root}/lib" "${usr_root}/share/glib-2.0"

  for tool in bash sh; do
    tool_path="$(command -v "${tool}" 2>/dev/null || true)"
    if [ -n "${tool_path}" ]; then
      ln -s "${tool_path}" "${bin_dir}/${tool}"
    fi
  done

  for tool in env xdg-mime xdg-open update-desktop-database; do
    tool_path="$(command -v "${tool}" 2>/dev/null || true)"
    if [ -n "${tool_path}" ]; then
      ln -s "${tool_path}" "${usr_root}/bin/${tool}"
    fi
  done

  local paths_file="${MAPLE_NIX_LINUX_CLOSURE_INFO:-}/store-paths"
  local store_path lib_dir lib_file
  if [ -f "${paths_file}" ]; then
    while IFS= read -r store_path; do
      for lib_dir in "${store_path}/lib" "${store_path}/lib64"; do
        if [ ! -d "${lib_dir}" ]; then
          continue
        fi

        while IFS= read -r -d '' lib_file; do
          case "${lib_file}" in
            *-gdb.py | *.debug)
              continue
              ;;
          esac
          ln -sf "$(readlink -f "${lib_file}")" "${usr_root}/lib/$(basename "${lib_file}")"
        done < <(find "${lib_dir}" -maxdepth 1 \( -type f -o -type l \) -name '*.so*' -print0 | LC_ALL=C sort -z)
      done
    done < "${paths_file}"
  fi

  if [ -n "${MAPLE_NIX_GTK_LIB:-}" ] && [ -d "${MAPLE_NIX_GTK_LIB}/gtk-3.0" ]; then
    cp -a "${MAPLE_NIX_GTK_LIB}/gtk-3.0" "${usr_root}/lib/gtk-3.0"
    chmod -R u+w "${usr_root}/lib/gtk-3.0" 2>/dev/null || true
  fi

  if [ -n "${MAPLE_NIX_GLIB_SCHEMAS:-}" ] && [ -d "${MAPLE_NIX_GLIB_SCHEMAS}" ]; then
    mkdir -p "${usr_root}/share/glib-2.0/schemas"
    cp -a "${MAPLE_NIX_GLIB_SCHEMAS}/." "${usr_root}/share/glib-2.0/schemas/"
    chmod -R u+w "${usr_root}/share/glib-2.0/schemas" 2>/dev/null || true
  fi

  if [ -n "${MAPLE_NIX_GDK_PIXBUF_BINARYDIR:-}" ] && [ -d "${MAPLE_NIX_GDK_PIXBUF_BINARYDIR}" ]; then
    mkdir -p "${usr_root}/lib/gdk-pixbuf-2.0/2.10.0"
    cp -a "${MAPLE_NIX_GDK_PIXBUF_BINARYDIR}/." "${usr_root}/lib/gdk-pixbuf-2.0/2.10.0/"
    chmod -R u+w "${usr_root}/lib/gdk-pixbuf-2.0/2.10.0" 2>/dev/null || true
  fi

  local real_pkg_config
  real_pkg_config="$(resolve_bwrap_visible_command pkg-config pkgconf || true)"
  if [ -n "${real_pkg_config}" ]; then
    cat > "${tool_bin}/pkgconf" <<EOF
#!${bash_path}
set -euo pipefail
if [ "\${1:-}" = "--variable=schemasdir" ] && [ "\${2:-}" = "gio-2.0" ] && [ -n "\${MAPLE_NIX_GLIB_SCHEMAS:-}" ]; then
  printf '%s\n' "/usr/share/glib-2.0/schemas"
  exit 0
fi
if [ "\${1:-}" = "--variable=exec_prefix" ] && [ "\${2:-}" = "gtk+-3.0" ] && [ -n "\${MAPLE_NIX_GTK_LIB:-}" ]; then
  printf '%s\n' "/usr"
  exit 0
fi
if [ "\${1:-}" = "--variable=libdir" ] && [ "\${2:-}" = "gtk+-3.0" ] && [ -n "\${MAPLE_NIX_GTK_LIB:-}" ]; then
  printf '%s\n' "/usr/lib"
  exit 0
fi
case "\${2:-}" in
  gobject-2.0 | gio-2.0 | librsvg-2.0 | pango | pangocairo | pangoft2)
    if [ "\${1:-}" = "--variable=libdir" ] && [ -n "\${MAPLE_NIX_LINUX_CLOSURE_INFO:-}" ]; then
      printf '%s\n' "/usr/lib"
      exit 0
    fi
    ;;
esac
if [ "\${2:-}" = "gdk-pixbuf-2.0" ] && [ -n "\${MAPLE_NIX_GDK_PIXBUF_BINARYDIR:-}" ]; then
  case "\${1:-}" in
    --variable=libdir)
      printf '%s\n' "/usr/lib"
      exit 0
      ;;
    --variable=gdk_pixbuf_binarydir)
      printf '%s\n' "/usr/lib/gdk-pixbuf-2.0/2.10.0"
      exit 0
      ;;
    --variable=gdk_pixbuf_cache_file)
      printf '%s\n' "/usr/lib/gdk-pixbuf-2.0/2.10.0/loaders.cache"
      exit 0
      ;;
    --variable=gdk_pixbuf_moduledir)
      printf '%s\n' "/usr/lib/gdk-pixbuf-2.0/2.10.0/loaders"
      exit 0
      ;;
  esac
fi
exec "${real_pkg_config}" "\$@"
EOF
    chmod +x "${tool_bin}/pkgconf"
    ln -s "${tool_bin}/pkgconf" "${tool_bin}/pkg-config"
  fi

  local real_patchelf
  real_patchelf="$(resolve_bwrap_visible_command patchelf || true)"
  if [ -n "${real_patchelf}" ]; then
    cat > "${tool_bin}/patchelf" <<EOF
#!${bash_path}
for arg in "\$@"; do
  if [ -f "\${arg}" ]; then
    chmod u+w "\${arg}" 2>/dev/null || true
  fi
done
exec "${real_patchelf}" "\$@"
EOF
    chmod +x "${tool_bin}/patchelf"
  fi

  local status
  set +e
  GDK_PIXBUF_MODULEDIR="${GDK_PIXBUF_MODULEDIR:-/usr/lib/gdk-pixbuf-2.0/2.10.0/loaders}" \
    PATH="${tool_bin}:${PATH}" \
    bwrap --dev-bind / / --bind "${bin_dir}" /bin --bind "${usr_root}" /usr -- "$@"
  status=$?
  set -e
  rm -rf "${bin_dir}"
  rm -rf "${tool_bin}"
  rm -rf "${usr_root}"
  return "${status}"
}

build_frontend_dist() {
  disable_bun_env_files
  cd "${FRONTEND_DIR}"
  rm -rf dist
  bun run build
  scrub_host_metadata_files "${FRONTEND_DIR}/dist"
  MAPLE_FRONTEND_DIST_TREE_SHA256="$(tree_hash_digest "${FRONTEND_DIR}/dist")"
  export MAPLE_FRONTEND_DIST_TREE_SHA256
  printf 'sha256-tree  %s  %s\n' "${MAPLE_FRONTEND_DIST_TREE_SHA256}" "frontend/dist"
}

verify_frontend_dist_unchanged() {
  local digest

  if [ ! -d "${FRONTEND_DIR}/dist" ]; then
    echo "Missing frontend dist at ${FRONTEND_DIR}/dist" >&2
    return 1
  fi

  digest="$(tree_hash_digest "${FRONTEND_DIR}/dist")"
  if [ -n "${MAPLE_FRONTEND_DIST_TREE_SHA256:-}" ] && [ "${digest}" != "${MAPLE_FRONTEND_DIST_TREE_SHA256}" ]; then
    echo "frontend/dist changed during native packaging." >&2
    echo "before=${MAPLE_FRONTEND_DIST_TREE_SHA256}" >&2
    echo "after=${digest}" >&2
    return 1
  fi

  printf 'sha256-tree  %s  %s\n' "${digest}" "frontend/dist"
}

ios_onnxruntime_xcframework_dir() {
  printf '%s\n' "${TAURI_DIR}/onnxruntime-ios/onnxruntime.xcframework"
}

ios_onnxruntime_manifest_file() {
  printf '%s\n' "${TAURI_DIR}/onnxruntime-ios/onnxruntime.xcframework.sha256"
}

ios_onnxruntime_version() {
  printf '%s\n' "${IOS_ONNXRUNTIME_VERSION:-1.22.2}"
}

require_ios_onnxruntime_files() {
  local xcframework
  xcframework="$(ios_onnxruntime_xcframework_dir)"

  local -a required_files=(
    "${xcframework}/Info.plist"
    "${xcframework}/Headers/onnxruntime_c_api.h"
    "${xcframework}/ios-arm64/libonnxruntime.a"
    "${xcframework}/ios-arm64-simulator/libonnxruntime.a"
  )

  local file
  for file in "${required_files[@]}"; do
    if [ ! -f "${file}" ]; then
      echo "Missing iOS ONNX Runtime artifact file: ${file}" >&2
      return 1
    fi
  done
}

write_ios_onnxruntime_manifest() {
  require_ios_onnxruntime_files

  local xcframework manifest
  xcframework="$(ios_onnxruntime_xcframework_dir)"
  manifest="$(ios_onnxruntime_manifest_file)"

  mkdir -p "$(dirname "${manifest}")"
  (
    cd "${xcframework}"
    find . -type f -print0 \
      | LC_ALL=C sort -z \
      | while IFS= read -r -d '' file; do
          file="${file#./}"
          printf '%s  %s\n' "$(sha256_file "${file}" | awk '{ print $1 }')" "${file}"
        done
  ) > "${manifest}"

  print_file_hashes "${manifest}"
}

verify_ios_onnxruntime_manifest() {
  require_ios_onnxruntime_files

  local xcframework manifest
  xcframework="$(ios_onnxruntime_xcframework_dir)"
  manifest="$(ios_onnxruntime_manifest_file)"

  if [ ! -f "${manifest}" ]; then
    echo "Missing iOS ONNX Runtime manifest: ${manifest}" >&2
    echo "Run scripts/ci/ios-onnxruntime.sh before the iOS app build." >&2
    return 1
  fi

  (
    cd "${xcframework}"
    if command -v sha256sum >/dev/null 2>&1; then
      sha256sum --check "${manifest}"
    else
      shasum -a 256 --check "${manifest}"
    fi
  )

  verify_ios_onnxruntime_pinned_hashes
}

verify_ios_onnxruntime_pinned_hashes() {
  local version xcode_build xcframework manifest file expected actual status

  version="$(ios_onnxruntime_version)"
  xcode_build="${MAPLE_XCODE_BUILD_VERSION:-17F42}"
  xcframework="$(ios_onnxruntime_xcframework_dir)"
  manifest="$(ios_onnxruntime_manifest_file)"
  status=0

  file="${xcframework}/ios-arm64/libonnxruntime.a"
  expected="$(onnxruntime_ios_device_lib_sha256_for_version "${version}" "${xcode_build}")"
  actual="$(sha256_file "${file}" | awk '{ print $1 }')"
  if [ "${actual}" != "${expected}" ]; then
    echo "iOS ONNX Runtime device library hash mismatch for ${version} with Xcode build ${xcode_build}." >&2
    echo "expected=${expected}" >&2
    echo "actual=${actual}" >&2
    status=1
  fi

  file="${xcframework}/ios-arm64-simulator/libonnxruntime.a"
  expected="$(onnxruntime_ios_simulator_lib_sha256_for_version "${version}" "${xcode_build}")"
  actual="$(sha256_file "${file}" | awk '{ print $1 }')"
  if [ "${actual}" != "${expected}" ]; then
    echo "iOS ONNX Runtime simulator library hash mismatch for ${version} with Xcode build ${xcode_build}." >&2
    echo "expected=${expected}" >&2
    echo "actual=${actual}" >&2
    status=1
  fi

  expected="$(onnxruntime_ios_xcframework_sha256_for_version "${version}" "${xcode_build}")"
  actual="$(sha256_file "${manifest}" | awk '{ print $1 }')"
  if [ "${actual}" != "${expected}" ]; then
    echo "iOS ONNX Runtime manifest hash mismatch for ${version} with Xcode build ${xcode_build}." >&2
    echo "expected=${expected}" >&2
    echo "actual=${actual}" >&2
    status=1
  fi

  actual="$(tree_hash_digest "${xcframework}")"
  if [ "${actual}" != "${expected}" ]; then
    echo "iOS ONNX Runtime xcframework tree hash mismatch for ${version} with Xcode build ${xcode_build}." >&2
    echo "expected=${expected}" >&2
    echo "actual=${actual}" >&2
    status=1
  fi

  return "${status}"
}

print_ios_onnxruntime_hashes() {
  local xcframework
  xcframework="$(ios_onnxruntime_xcframework_dir)"

  print_file_hashes \
    "${xcframework}/ios-arm64/libonnxruntime.a" \
    "${xcframework}/ios-arm64-simulator/libonnxruntime.a" \
    "$(ios_onnxruntime_manifest_file)"
  print_tree_hash "${xcframework}" "frontend/src-tauri/onnxruntime-ios/onnxruntime.xcframework"
}

write_ios_onnxruntime_reproducibility_manifest() {
  local out="$1"
  local xcframework
  xcframework="$(ios_onnxruntime_xcframework_dir)"

  write_sha256_manifest \
    "${out}" \
    "${xcframework}/ios-arm64/libonnxruntime.a" \
    "${xcframework}/ios-arm64-simulator/libonnxruntime.a" \
    "$(ios_onnxruntime_manifest_file)"
}

remove_generated_ios_cargo_config() {
  local config_file="${TAURI_DIR}/.cargo/config.toml"

  if [ ! -f "${config_file}" ]; then
    return 0
  fi

  if grep -q "Generated by: scripts/setup-ios-cargo-config.sh" "${config_file}"; then
    rm -f "${config_file}"
    rmdir "${TAURI_DIR}/.cargo" 2>/dev/null || true
  fi
}

archive_tree_as_root_tar_gz() {
  local root="$1"
  local out="$2"
  local tar_cmd="${MAPLE_NIX_GNUTAR:-tar}"
  local gzip_cmd="${MAPLE_NIX_GZIP:-gzip}"

  if [ ! -x "${tar_cmd}" ] && command -v gtar >/dev/null 2>&1; then
    tar_cmd="gtar"
  fi
  if [ ! -x "${tar_cmd}" ]; then
    tar_cmd="tar"
  fi
  if [ ! -x "${gzip_cmd}" ]; then
    gzip_cmd="gzip"
  fi

  (
    cd "${root}"
    find . -mindepth 1 -print0 \
      | LC_ALL=C sort -z \
      | "${tar_cmd}" --null --no-recursion \
          --mtime="@${SOURCE_DATE_EPOCH}" \
          --owner=0 \
          --group=0 \
          --numeric-owner \
          -cf - \
          -T -
  ) | "${gzip_cmd}" -n > "${out}"
}

normalize_deb_package() {
  local deb="$1"
  local tmp
  tmp="$(mktemp -d)"

  (
    cd "${tmp}"
    ar x "${deb}"
    mkdir control data
    tar -xzf control.tar.gz -C control
    tar -xzf data.tar.gz -C data
    archive_tree_as_root_tar_gz "${tmp}/control" "${tmp}/control.tar.gz"
    archive_tree_as_root_tar_gz "${tmp}/data" "${tmp}/data.tar.gz"
    printf '2.0\n' > debian-binary
    ar crD normalized.deb debian-binary control.tar.gz data.tar.gz
  )

  mv "${tmp}/normalized.deb" "${deb}"
  rm -rf "${tmp}"
}

extract_deb_payload() {
  local deb="$1"
  local payload="$2"
  local tmp
  tmp="$(mktemp -d)"

  (
    cd "${tmp}"
    ar x "${deb}"
    mkdir -p "${payload}"
    tar -xzf data.tar.gz -C "${payload}"
  )

  rm -rf "${tmp}"
}

touch_tree_to_source_date_epoch() {
  local root="$1"
  find "${root}" -exec touch -h -d "@${SOURCE_DATE_EPOCH}" {} +
}

rpm_arch() {
  case "$(uname -m)" in
    x86_64 | amd64)
      printf '%s\n' "x86_64"
      ;;
    aarch64 | arm64)
      printf '%s\n' "aarch64"
      ;;
    *)
      echo "Unsupported Linux RPM architecture: $(uname -m)" >&2
      return 1
      ;;
  esac
}

rebuild_rpm_package_from_payload() {
  local rpm="$1"
  local payload="$2"
  local topdir spec built_rpm
  local version release arch

  command -v rpmbuild >/dev/null 2>&1 || {
    echo "rpmbuild is required to normalize RPM packages. Run through the flake CI shell." >&2
    return 1
  }

  version="$(jq -r '.version' "${TAURI_DIR}/tauri.conf.json")"
  release="$(jq -r '.bundle.linux.rpm.release // "1"' "${TAURI_DIR}/tauri.conf.json")"
  arch="$(rpm_arch)"
  topdir="$(mktemp -d)"
  spec="${topdir}/SPECS/maple.spec"

  touch_tree_to_source_date_epoch "${payload}"
  mkdir -p "${topdir}/BUILD" "${topdir}/BUILDROOT" "${topdir}/RPMS" "${topdir}/SOURCES" "${topdir}/SPECS" "${topdir}/SRPMS"

  cat > "${spec}" <<EOF
Name: maple
Epoch: 0
Version: ${version}
Release: ${release}
Summary: Maple AI
License: MIT
BuildArch: ${arch}
AutoReqProv: no

%description
Maple AI

%prep

%build

%install
rm -rf %{buildroot}
mkdir -p %{buildroot}
cp -a ${payload}/. %{buildroot}/

%files
%defattr(-,root,root,-)
EOF

  (
    cd "${payload}"
    find . \( -type f -o -type l \) -print \
      | LC_ALL=C sort \
      | sed 's#^\./#/#'
  ) >> "${spec}"

  SOURCE_DATE_EPOCH="${SOURCE_DATE_EPOCH}" rpmbuild -bb "${spec}" \
    --define "_topdir ${topdir}" \
    --define "_dbpath ${topdir}/rpmdb" \
    --define "_buildhost (none)" \
    --define "use_source_date_epoch_as_buildtime 1" \
    --define "_build_name_fmt %{NAME}-%{VERSION}-%{RELEASE}.%{ARCH}.rpm" \
    --define "_binary_payload w9.gzdio"

  built_rpm="$(find "${topdir}/RPMS" -type f -name '*.rpm' | LC_ALL=C sort | head -n 1)"
  if [ -z "${built_rpm}" ]; then
    echo "rpmbuild did not produce an RPM." >&2
    rm -rf "${topdir}"
    return 1
  fi

  mv "${built_rpm}" "${rpm}"
  rm -rf "${topdir}"
}

normalize_linux_desktop_packages() {
  if [ "$(host_os)" != "linux" ]; then
    return 0
  fi

  local deb rpm payload
  local -a debs=()
  local -a rpms=()

  while IFS= read -r -d '' deb; do
    debs+=("${deb}")
  done < <(find "${TAURI_DIR}/target/release/bundle/deb" -type f -name '*.deb' -print0 2>/dev/null | LC_ALL=C sort -z)

  while IFS= read -r -d '' rpm; do
    rpms+=("${rpm}")
  done < <(find "${TAURI_DIR}/target/release/bundle/rpm" -type f -name '*.rpm' -print0 2>/dev/null | LC_ALL=C sort -z)

  for deb in "${debs[@]}"; do
    normalize_deb_package "${deb}"
  done

  if [ "${#rpms[@]}" -gt 0 ]; then
    if [ "${#debs[@]}" -eq 0 ]; then
      echo "Cannot normalize RPM packages without a Debian payload source." >&2
      return 1
    fi

    payload="$(mktemp -d)"
    extract_deb_payload "${debs[0]}" "${payload}"
    for rpm in "${rpms[@]}"; do
      rebuild_rpm_package_from_payload "${rpm}" "${payload}"
    done
    rm -rf "${payload}"
  fi
}

linux_tauri_pr_config() {
  local ort_version="${ORT_VERSION:-1.22.0}"
  local ort_arch
  ort_arch="$(linux_ort_arch)"
  local ort_rel="./onnxruntime-linux/onnxruntime-linux-${ort_arch}-${ort_version}/lib/libonnxruntime.so.${ort_version}"

  jq -cn --arg ort "${ort_rel}" '{
    build: {
      beforeBuildCommand: null
    },
    bundle: {
      createUpdaterArtifacts: false,
      targets: ["deb", "rpm"],
      linux: {
        appimage: {
          bundleMediaFramework: true,
          files: {
            "/usr/lib/maple/libonnxruntime.so": $ort
          }
        },
        deb: {
          files: {
            "/usr/lib/maple/libonnxruntime.so": $ort
          }
        },
        rpm: {
          files: {
            "/usr/lib/maple/libonnxruntime.so": $ort
          }
        }
      }
    }
  }'
}

linux_tauri_release_config() {
  local ort_version="${ORT_VERSION:-1.22.0}"
  local ort_arch updater_pubkey
  ort_arch="$(linux_ort_arch)"
  local ort_rel="./onnxruntime-linux/onnxruntime-linux-${ort_arch}-${ort_version}/lib/libonnxruntime.so.${ort_version}"
  updater_pubkey="$(tauri_updater_public_key_config_value)"

	  jq -cn --arg ort "${ort_rel}" --arg updaterPubkey "${updater_pubkey}" '{
	    build: {
	      beforeBuildCommand: null
	    },
	    plugins: {
	      updater: {
	        pubkey: $updaterPubkey
	      }
	    },
	    bundle: {
	      createUpdaterArtifacts: true,
	      useLocalToolsDir: true,
	      targets: ["appimage", "deb", "rpm"],
	      linux: {
	        appimage: {
          bundleMediaFramework: true,
          files: {
            "/usr/lib/maple/libonnxruntime.so": $ort
          }
        },
        deb: {
          files: {
            "/usr/lib/maple/libonnxruntime.so": $ort
          }
        },
        rpm: {
          files: {
            "/usr/lib/maple/libonnxruntime.so": $ort
          }
        }
      }
    }
  }'
}

import_apple_developer_certificate() {
  if [ "$(host_os)" != "darwin" ] || [ -z "${APPLE_CERTIFICATE:-}" ]; then
    return 0
  fi

  if [ -z "${APPLE_CERTIFICATE_PASSWORD:-}" ]; then
    echo "APPLE_CERTIFICATE_PASSWORD is required when APPLE_CERTIFICATE is set." >&2
    return 1
  fi

  local temp_dir cert_file keychain keychain_password cert_info cert_id
  temp_dir="${RUNNER_TEMP:-${TMPDIR:-/tmp}}"
  cert_file="${temp_dir%/}/maple-apple-certificate.p12"
  keychain="${temp_dir%/}/maple-build.keychain-db"
  keychain_password="${KEYCHAIN_PASSWORD:-$(openssl rand -hex 24)}"

  decode_base64_string_to_file "${APPLE_CERTIFICATE}" "${cert_file}"

  security create-keychain -p "${keychain_password}" "${keychain}"
  security default-keychain -s "${keychain}"
  security unlock-keychain -p "${keychain_password}" "${keychain}"
  security set-keychain-settings -t 3600 -u "${keychain}"
  security import "${cert_file}" -k "${keychain}" -P "${APPLE_CERTIFICATE_PASSWORD}" -T /usr/bin/codesign -T /usr/bin/productbuild
  security set-key-partition-list -S apple-tool:,apple:,codesign: -s -k "${keychain_password}" "${keychain}"

  cert_info="$(security find-identity -v -p codesigning "${keychain}" | grep "Developer ID Application" | head -n 1 || true)"
  if [ -z "${cert_info}" ]; then
    echo "No Developer ID Application identity found in imported Apple certificate." >&2
    security find-identity -v -p codesigning "${keychain}" >&2 || true
    return 1
  fi

  cert_id="$(printf '%s\n' "${cert_info}" | awk -F'"' '{ print $2 }')"
  export APPLE_SIGNING_IDENTITY="${APPLE_SIGNING_IDENTITY:-${cert_id}}"
  echo "Imported Apple Developer ID certificate."
}
