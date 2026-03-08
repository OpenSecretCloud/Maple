#!/usr/bin/env bash
set -euo pipefail

source "$(cd "$(dirname "$0")" && pwd)/common.sh"

require_env APPLE_API_KEY APPLE_API_ISSUER APPLE_API_KEY_PATH APPLE_SIGNING_IDENTITY
require_command cargo lipo codesign iconutil sips xcrun hdiutil spctl cargo-tauri

macos_build_root="$ci_build_root/desktop-macos"
app_bundle="$macos_build_root/Maple.app"
app_contents="$app_bundle/Contents"
artifact_version="${maple_version_value}-beta.${maple_build_number}"
artifact_prefix="Maple-${artifact_version}-macos-universal"
icon_source="$native_root/ios/Sources/Assets.xcassets/AppIcon.appiconset/icon_1024.png"
iconset_dir="$macos_build_root/Maple.iconset"
app_zip="$macos_build_root/${artifact_prefix}.app.zip"
dmg_path="$macos_build_root/${artifact_prefix}.dmg"

ensure_dir "$macos_build_root"
rm -rf "$app_bundle" "$iconset_dir" "$app_zip" "$dmg_path" "$macos_build_root/dmg"

openssl_prefix="$(brew --prefix openssl@3)"
export PKG_CONFIG_PATH="$openssl_prefix/lib/pkgconfig"
export OPENSSL_DIR="$openssl_prefix"
export OPEN_SECRET_API_URL="$open_secret_api_url"

cargo build --manifest-path "$native_root/Cargo.toml" -p maple_desktop_iced --release --target aarch64-apple-darwin
cargo build --manifest-path "$native_root/Cargo.toml" -p maple_desktop_iced --release --target x86_64-apple-darwin

mkdir -p "$app_contents/MacOS" "$app_contents/Resources"

lipo -create \
  "$native_root/target/aarch64-apple-darwin/release/maple_desktop_iced" \
  "$native_root/target/x86_64-apple-darwin/release/maple_desktop_iced" \
  -output "$app_contents/MacOS/Maple"

chmod 755 "$app_contents/MacOS/Maple"

mkdir -p "$iconset_dir"
for size in 16 32 128 256 512; do
  sips -z "$size" "$size" "$icon_source" --out "$iconset_dir/icon_${size}x${size}.png" >/dev/null
  sips -z "$((size * 2))" "$((size * 2))" "$icon_source" --out "$iconset_dir/icon_${size}x${size}@2x.png" >/dev/null
done
cp "$icon_source" "$iconset_dir/icon_512x512@2x.png"
iconutil -c icns "$iconset_dir" -o "$app_contents/Resources/Maple.icns"

cat > "$app_contents/Info.plist" <<EOF
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
  <key>CFBundleDevelopmentRegion</key>
  <string>en</string>
  <key>CFBundleExecutable</key>
  <string>Maple</string>
  <key>CFBundleIconFile</key>
  <string>Maple.icns</string>
  <key>CFBundleIdentifier</key>
  <string>cloud.opensecret.maple.desktop</string>
  <key>CFBundleInfoDictionaryVersion</key>
  <string>6.0</string>
  <key>CFBundleName</key>
  <string>Maple</string>
  <key>CFBundlePackageType</key>
  <string>APPL</string>
  <key>CFBundleShortVersionString</key>
  <string>${maple_version_value}</string>
  <key>CFBundleVersion</key>
  <string>${maple_build_number}</string>
  <key>LSApplicationCategoryType</key>
  <string>public.app-category.productivity</string>
  <key>LSMinimumSystemVersion</key>
  <string>13.0</string>
  <key>NSHighResolutionCapable</key>
  <true/>
</dict>
</plist>
EOF

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
