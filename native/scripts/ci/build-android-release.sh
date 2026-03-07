#!/usr/bin/env bash
set -euo pipefail

source "$(cd "$(dirname "$0")" && pwd)/common.sh"

require_command cargo

if [[ ! -x "$native_root/android/gradlew" ]]; then
  echo "Missing executable Gradle wrapper at $native_root/android/gradlew" >&2
  exit 1
fi

android_build_root="$ci_build_root/android"
android_version_name="${MAPLE_ANDROID_VERSION_NAME:-${maple_version_value}-beta.${maple_build_number}}"
android_version_code="${MAPLE_ANDROID_VERSION_CODE:-$((300000000 + maple_build_number))}"

ensure_dir "$android_build_root"

if [[ -n "${ANDROID_SDK_ROOT:-}" ]]; then
  cat > "$native_root/android/local.properties" <<EOF
sdk.dir=${ANDROID_SDK_ROOT}
EOF
elif [[ -n "${ANDROID_HOME:-}" ]]; then
  cat > "$native_root/android/local.properties" <<EOF
sdk.dir=${ANDROID_HOME}
EOF
fi

export OPEN_SECRET_API_URL="$open_secret_api_url"
export MAPLE_ANDROID_VERSION_NAME="$android_version_name"
export MAPLE_ANDROID_VERSION_CODE="$android_version_code"

(
  cd "$native_root"
  cargo run --manifest-path "$native_root/rmp-cli/Cargo.toml" -- bindings kotlin
)

(
  cd "$native_root/android"
  ./gradlew --no-daemon clean :app:assembleRelease :app:bundleRelease
)

apk_path="$native_root/android/app/build/outputs/apk/release/app-release.apk"
aab_path="$native_root/android/app/build/outputs/bundle/release/app-release.aab"

if [[ ! -f "$apk_path" || ! -f "$aab_path" ]]; then
  echo "Expected signed Android artifacts were not produced" >&2
  exit 1
fi

cp "$apk_path" "$android_build_root/Maple-${android_version_name}.apk"
cp "$aab_path" "$android_build_root/Maple-${android_version_name}.aab"

printf 'Android APK: %s\n' "$android_build_root/Maple-${android_version_name}.apk"
printf 'Android AAB: %s\n' "$android_build_root/Maple-${android_version_name}.aab"
