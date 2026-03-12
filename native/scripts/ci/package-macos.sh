#!/usr/bin/env bash
set -euo pipefail

source "$(cd "$(dirname "$0")" && pwd)/common.sh"

require_env APPLE_API_KEY APPLE_API_ISSUER APPLE_API_KEY_PATH APPLE_SIGNING_IDENTITY
require_command cargo codesign ditto hdiutil spctl xcodebuild xcodegen xcrun

macos_build_root="$ci_build_root/desktop-macos"
derived_data="$macos_build_root/DerivedData"
built_app="$derived_data/Build/Products/Release/Maple.app"
app_bundle="$macos_build_root/Maple.app"
artifact_version="$(maple_artifact_version)"
artifact_prefix="Maple-${artifact_version}-macos-universal"
app_zip="$macos_build_root/${artifact_prefix}.app.zip"
dmg_path="$macos_build_root/${artifact_prefix}.dmg"

ensure_dir "$macos_build_root"
rm -rf "$app_bundle" "$app_zip" "$dmg_path" "$macos_build_root/dmg" "$derived_data"

export OPEN_SECRET_API_URL="$open_secret_api_url"

(
  cd "$native_root"
  RMP_SWIFT_UNIVERSAL_MACOS=1 cargo run --manifest-path "$native_root/rmp-cli/Cargo.toml" -- bindings swift
)

(
  cd "$native_root/ios"
  xcodegen generate
)

env -u NIX_LDFLAGS -u LD -u CC -u CXX -u AR -u RANLIB \
  xcrun xcodebuild \
    -project "$native_root/ios/App.xcodeproj" \
    -scheme AppMac \
    -configuration Release \
    -derivedDataPath "$derived_data" \
    -destination generic/platform=macOS \
    ARCHS="arm64 x86_64" \
    ONLY_ACTIVE_ARCH=NO \
    CODE_SIGNING_ALLOWED=NO \
    CODE_SIGNING_REQUIRED=NO \
    CODE_SIGN_IDENTITY="" \
    MARKETING_VERSION="$maple_version_value" \
    CURRENT_PROJECT_VERSION="$maple_build_number" \
    OPEN_SECRET_API_URL="$open_secret_api_url" \
    COMPILER_INDEX_STORE_ENABLE=NO \
    clean build

if [[ ! -d "$built_app" ]]; then
  echo "Failed to locate built AppMac bundle at $built_app" >&2
  exit 1
fi

cp -R "$built_app" "$app_bundle"

codesign --force --deep --options runtime --timestamp --sign "$APPLE_SIGNING_IDENTITY" "$app_bundle"
codesign --verify --deep --strict --verbose=2 "$app_bundle"

ditto -c -k --keepParent "$app_bundle" "$app_zip"

xcrun notarytool submit "$app_zip" \
  --key "$APPLE_API_KEY_PATH" \
  --key-id "$APPLE_API_KEY" \
  --issuer "$APPLE_API_ISSUER" \
  --wait

xcrun stapler staple "$app_bundle"

dmg_stage="$macos_build_root/dmg"
mkdir -p "$dmg_stage"
cp -R "$app_bundle" "$dmg_stage/"
ln -s /Applications "$dmg_stage/Applications"

hdiutil create -volname "Maple" -srcfolder "$dmg_stage" -ov -format UDZO "$dmg_path"

codesign --force --timestamp --sign "$APPLE_SIGNING_IDENTITY" "$dmg_path"

xcrun notarytool submit "$dmg_path" \
  --key "$APPLE_API_KEY_PATH" \
  --key-id "$APPLE_API_KEY" \
  --issuer "$APPLE_API_ISSUER" \
  --wait

xcrun stapler staple "$dmg_path"
spctl --assess --type open --context context:primary-signature "$dmg_path"

tauri_sign_artifact "$app_zip"
tauri_sign_artifact "$dmg_path"

printf 'macOS ZIP: %s\n' "$app_zip"
printf 'macOS DMG: %s\n' "$dmg_path"
