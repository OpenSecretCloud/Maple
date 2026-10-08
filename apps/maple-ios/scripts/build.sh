#!/usr/bin/env bash
set -euo pipefail
app_dir="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
mode="${1:-build}"
profile="${2:-local}"
case "$(xcodebuild -version | awk '/^Xcode / {print $2; exit}')" in
  26.5|26.5.*) ;;
  *) echo "Select Xcode 26.5 before building Maple iOS." >&2; exit 1 ;;
esac

case "$mode" in
  unit)
    xcrun swift-format lint --strict --configuration "$app_dir/.swift-format" --recursive \
      "$app_dir/Package.swift" "$app_dir/App" "$app_dir/Sources" "$app_dir/Tests" "$app_dir/UITests"
    # Nix may export a Command Line Tools SDKROOT. SwiftUI macros need the
    # macOS SDK supplied by the selected Xcode, matching xcrun's compiler.
    swift_args=(--package-path "$app_dir" --sdk "$(xcrun --sdk macosx --show-sdk-path)")
    if [[ -n "${MAPLE_IOS_SWIFT_SCRATCH_PATH:-}" ]]; then
      swift_args+=(--scratch-path "$MAPLE_IOS_SWIFT_SCRATCH_PATH")
    fi
    env -u MAPLE_RUN_LIVE_ATTESTATION -u MAPLE_SWIFT_INTEGRATION_URL -u MAPLE_SWIFT_DEV_INTEGRATION \
      -u MAPLE_IOS_ACCOUNT_LIVE -u MAPLE_IOS_AUTH_LIVE -u RUN_LIVE_AI \
      -u LD -u LDPLUSPLUS -u CC -u CXX xcrun swift test "${swift_args[@]}" -Xswiftc -warnings-as-errors
    exit 0
    ;;
  build|device-build|runtime-build|test-simulator) ;;
  *) echo "Usage: build.sh {unit|build|device-build|runtime-build|test-simulator} [local|pr|release]" >&2; exit 2 ;;
esac
case "$profile" in
  local) configuration=Local ;;
  pr) configuration=PR ;;
  release) configuration=Release ;;
  *) echo "Expected profile local, pr, or release." >&2; exit 2 ;;
esac
if [[ "$profile" != pr && ! -f "$app_dir/Config/Local.generated.xcconfig" ]]; then
  echo "Run configure-local.py with explicit public local service inputs first." >&2
  exit 1
fi
destination='generic/platform=iOS Simulator'
[[ "$mode" == device-build ]] && destination='generic/platform=iOS'
derived="${MAPLE_IOS_DERIVED_DATA:-$app_dir/.build/xcode-$profile}"
build_settings=(CODE_SIGNING_ALLOWED=NO CODE_SIGNING_REQUIRED=NO)
action=build
if [[ "$mode" == runtime-build || "$mode" == test-simulator ]]; then
  if [[ "$profile" != local || ! "${MAPLE_IOS_SIMULATOR_ID:-}" =~ ^[A-Fa-f0-9]{8}-[A-Fa-f0-9]{4}-[A-Fa-f0-9]{4}-[A-Fa-f0-9]{4}-[A-Fa-f0-9]{12}$ ]]; then
    echo "Simulator runtime checks require profile local and an explicit MAPLE_IOS_SIMULATOR_ID." >&2
    exit 2
  fi
  destination="platform=iOS Simulator,id=$MAPLE_IOS_SIMULATOR_ID"
  # Xcode supplies its simulator application identity. No team, certificate,
  # provisioning profile or device/distribution signature is used here.
  build_settings=(CODE_SIGNING_ALLOWED=YES CODE_SIGNING_REQUIRED=YES CODE_SIGN_IDENTITY=- DEVELOPMENT_TEAM=)
  build_settings+=(ONLY_ACTIVE_ARCH=YES SWIFT_OPTIMIZATION_LEVEL=-Onone
    SWIFT_COMPILATION_MODE=singlefile 'SWIFT_ACTIVE_COMPILATION_CONDITIONS=$(inherited) DEBUG')
  action=build-for-testing
  [[ "$mode" == test-simulator ]] && action=test
fi
env -u LD -u LDPLUSPLUS -u CC -u CXX \
  xcodebuild -jobs 2 -project "$app_dir/Maple.xcodeproj" -scheme Maple -configuration "$configuration" \
  -destination "$destination" -derivedDataPath "$derived" \
  "${build_settings[@]}" "$action"
