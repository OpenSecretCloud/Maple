#!/usr/bin/env bash
set -euo pipefail

source "$(cd "$(dirname "$0")" && pwd)/common.sh"

require_env APPLE_API_KEY APPLE_API_ISSUER APPLE_API_KEY_PATH APPLE_TEAM_ID
require_command cargo xcodebuild xcodegen xcrun

ios_build_root="$ci_build_root/ios"
archive_path="$ios_build_root/Maple.xcarchive"
export_path="$ios_build_root/export"
export_options_plist="$ios_build_root/ExportOptions.plist"

ensure_dir "$ios_build_root"
rm -rf "$archive_path" "$export_path"

cat > "$export_options_plist" <<EOF
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
  <key>destination</key>
  <string>export</string>
  <key>manageAppVersionAndBuildNumber</key>
  <false/>
  <key>method</key>
  <string>app-store-connect</string>
  <key>signingStyle</key>
  <string>automatic</string>
  <key>teamID</key>
  <string>${APPLE_TEAM_ID}</string>
  <key>uploadSymbols</key>
  <true/>
</dict>
</plist>
EOF

export OPEN_SECRET_API_URL="$open_secret_api_url"

(
  cd "$native_root"
  cargo run --manifest-path "$native_root/rmp-cli/Cargo.toml" -- bindings swift
)

(
  cd "$native_root/ios"
  xcodegen generate
)

# Match Maple 2.0/Tauri CI: exportArchive performs the App Store signing.
# If native iOS gains entitlements, revisit this unsigned archive flow.
env -u NIX_LDFLAGS -u LD -u CC -u CXX \
  xcrun xcodebuild \
    -project "$native_root/ios/App.xcodeproj" \
    -scheme App \
    -configuration Release \
    -destination generic/platform=iOS \
    -archivePath "$archive_path" \
    -allowProvisioningUpdates \
    -authenticationKeyPath "$APPLE_API_KEY_PATH" \
    -authenticationKeyID "$APPLE_API_KEY" \
    -authenticationKeyIssuerID "$APPLE_API_ISSUER" \
    clean archive \
    CODE_SIGNING_REQUIRED=NO \
    CODE_SIGNING_ALLOWED=NO \
    CODE_SIGN_IDENTITY="" \
    CODE_SIGN_ENTITLEMENTS="" \
    DEVELOPMENT_TEAM="$APPLE_TEAM_ID" \
    PRODUCT_BUNDLE_IDENTIFIER=cloud.opensecret.maple \
    MARKETING_VERSION="$maple_version_value" \
    CURRENT_PROJECT_VERSION="$maple_build_number" \
    OPEN_SECRET_API_URL="$open_secret_api_url" \
    COMPILER_INDEX_STORE_ENABLE=NO

env -u NIX_LDFLAGS -u LD -u CC -u CXX \
  xcrun xcodebuild \
    -exportArchive \
    -archivePath "$archive_path" \
    -exportPath "$export_path" \
    -exportOptionsPlist "$export_options_plist" \
    -allowProvisioningUpdates \
    -authenticationKeyPath "$APPLE_API_KEY_PATH" \
    -authenticationKeyID "$APPLE_API_KEY" \
    -authenticationKeyIssuerID "$APPLE_API_ISSUER"

ipa_path="$(find "$export_path" -name '*.ipa' -print -quit)"

if [[ -z "$ipa_path" ]]; then
  echo "Failed to locate exported IPA" >&2
  exit 1
fi

private_keys_dir="$HOME/.private_keys"
mkdir -p "$private_keys_dir"
install -m 600 "$APPLE_API_KEY_PATH" "$private_keys_dir/AuthKey_${APPLE_API_KEY}.p8"

upload_log="$(mktemp)"
set +e
xcrun altool --upload-app --type ios \
  --file "$ipa_path" \
  --apiKey "$APPLE_API_KEY" \
  --apiIssuer "$APPLE_API_ISSUER" 2>&1 | tee "$upload_log"
upload_status=${PIPESTATUS[0]}
set -e

if (( upload_status != 0 )) || grep -Eq 'UPLOAD FAILED|Validation failed|Failed to upload package' "$upload_log"; then
  cat "$upload_log" >&2
  rm -f "$upload_log"
  exit 1
fi

if ! grep -q 'No errors uploading archive' "$upload_log"; then
  cat "$upload_log" >&2
  rm -f "$upload_log"
  echo "Could not confirm successful TestFlight upload" >&2
  exit 1
fi

rm -f "$upload_log"

printf 'IPA exported to %s\n' "$ipa_path"
