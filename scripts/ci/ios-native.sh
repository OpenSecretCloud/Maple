#!/usr/bin/env bash
# Unsigned native app checks; host Xcode supplies Swift and swift-format.
set -euo pipefail
repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
app_dir="$repo_root/apps/maple-ios"
mode="${1:-unit}"
case "$mode" in
  unit|format|simulator-build|device-build|cross-build) ;;
  *) echo "Usage: ios-native.sh {unit|format|simulator-build|device-build|cross-build}" >&2; exit 2 ;;
esac
if [[ "$(uname -s)" != Darwin ]]; then
  echo "Maple iOS checks require macOS and Xcode 26.5." >&2
  exit 1
fi
case "$(xcodebuild -version | awk '/^Xcode / {print $2; exit}')" in
  26.5|26.5.*) ;;
  *) echo "Select Xcode 26.5 before running Maple iOS checks." >&2; exit 1 ;;
esac

# Never turn hermetic checks into developer-local service or credential access.
# Only variable names are read here; values are neither printed nor sourced.
while IFS= read -r variable; do
  case "$variable" in
    VITE_*|MAPLE_SWIFT_*|MAPLE_IOS_ACCOUNT_*|MAPLE_IOS_AUTH_*|MAPLE_RUN_LIVE_ATTESTATION|RUN_LIVE_AI|MAPLE_PROFILE|MAPLE_API_URL|MAPLE_BILLING_URL|MAPLE_FLAGS_URL|MAPLE_AUTH_URL|MAPLE_CLIENT_ID|MAPLE_CALLBACK_SCHEME)
      unset "$variable" ;;
  esac
done < <(compgen -v)
unset LD LDPLUSPLUS CC CXX

case "$mode" in
  unit) bash "$app_dir/scripts/build.sh" unit ;;
  format)
    xcrun swift-format format --configuration "$app_dir/.swift-format" --in-place --recursive \
      "$app_dir/Package.swift" "$app_dir/App" "$app_dir/Sources" "$app_dir/Tests" "$app_dir/UITests"
    ;;
  simulator-build) bash "$app_dir/scripts/build.sh" build pr ;;
  device-build) bash "$app_dir/scripts/build.sh" device-build pr ;;
  cross-build)
    args=(--package-path "$app_dir" -c release -Xswiftc -warnings-as-errors)
    if [[ -n "${MAPLE_IOS_SWIFT_SCRATCH_PATH:-}" ]]; then
      args+=(--scratch-path "$MAPLE_IOS_SWIFT_SCRATCH_PATH")
    fi
    xcrun swift build "${args[@]}" --triple arm64-apple-ios26.0-simulator \
      --sdk "$(xcrun --sdk iphonesimulator --show-sdk-path)"
    xcrun swift build "${args[@]}" --triple arm64-apple-ios26.0 \
      --sdk "$(xcrun --sdk iphoneos --show-sdk-path)"
    ;;
esac
