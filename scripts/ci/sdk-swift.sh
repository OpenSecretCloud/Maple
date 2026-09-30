#!/usr/bin/env bash
# Swift SDK validation. Requires the host Xcode pin, also used by the Apple shell.
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
package="$repo_root/sdk/swift"
mode="${1:-unit}"
case "$mode" in
  unit|build|ios-build|format|live-attestation|integration) ;;
  *) echo "Usage: sdk-swift.sh {unit|build|ios-build|format|live-attestation|integration}" >&2; exit 2 ;;
esac

if [[ "$(uname -s)" != Darwin ]]; then
  echo "The Swift SDK requires macOS and Xcode 26.5 (CryptoKit and Security.framework)." >&2
  exit 1
fi
xcode_info="$(xcodebuild -version)"
xcode_version="$(awk '/^Xcode / { print $2; exit }' <<< "$xcode_info")"
case "$xcode_version" in
  26.5|26.5.*) ;;
  *) echo "Select Xcode 26.5 before running Swift SDK checks." >&2; exit 1 ;;
esac

# Pin formatting through Xcode, not a host-global swift-format installation.
swift_args=(--package-path "$package")
if [[ -n "${MAPLE_SWIFT_SCRATCH_PATH:-}" ]]; then
  swift_args+=(--scratch-path "$MAPLE_SWIFT_SCRATCH_PATH")
fi
case "$mode" in
  format)
    xcrun swift-format format --configuration "$package/.swift-format" --in-place --recursive \
      "$package/Package.swift" "$package/Sources" "$package/Tests"
    ;;
  build)
    xcrun swift build "${swift_args[@]}" -c release -Xswiftc -warnings-as-errors
    ;;
  ios-build)
    xcrun swift build "${swift_args[@]}" -c release --triple arm64-apple-ios26.0-simulator \
      --sdk "$(xcrun --sdk iphonesimulator --show-sdk-path)" -Xswiftc -warnings-as-errors
    xcrun swift build "${swift_args[@]}" -c release --triple arm64-apple-ios26.0 \
      --sdk "$(xcrun --sdk iphoneos --show-sdk-path)" -Xswiftc -warnings-as-errors
    ;;
  unit)
    xcrun swift-format lint --configuration "$package/.swift-format" --strict --recursive \
      "$package/Package.swift" "$package/Sources" "$package/Tests"
    # An inherited opt-in must never silently turn a pre-commit or PR unit gate
    # into a hosted request or a disposable-account integration run.
    env -u MAPLE_RUN_LIVE_ATTESTATION -u MAPLE_SWIFT_INTEGRATION_URL -u MAPLE_SWIFT_DEV_INTEGRATION \
      -u RUN_LIVE_AI \
      xcrun swift test "${swift_args[@]}" -Xswiftc -warnings-as-errors
    ;;
  live-attestation)
    MAPLE_RUN_LIVE_ATTESTATION=1 xcrun swift test "${swift_args[@]}" --filter LiveAttestationTests
    ;;
  integration)
    : "${MAPLE_SWIFT_INTEGRATION_URL:?Set MAPLE_SWIFT_INTEGRATION_URL to the disposable loopback backend}"
    : "${MAPLE_SWIFT_TEST_EMAIL:?Set the disposable test account email}"
    : "${MAPLE_SWIFT_TEST_PASSWORD:?Set the disposable test account password}"
    : "${MAPLE_SWIFT_TEST_CLIENT_ID:?Set the disposable project client ID}"
    python3 -I -c '
import os, sys
from urllib.parse import urlsplit
try:
    url = urlsplit(os.environ["MAPLE_SWIFT_INTEGRATION_URL"])
    valid = (url.scheme == "http" and url.hostname in {"127.0.0.1", "localhost", "::1"}
             and url.port is not None and 0 < url.port <= 65535 and url.username is None
             and url.password is None and not url.query and not url.fragment and url.path in {"", "/"})
except ValueError:
    valid = False
if not valid:
    sys.exit("SDK integration requires an explicit HTTP loopback backend.")
'
    xcrun swift test "${swift_args[@]}" --filter LocalIntegrationTests
    ;;
esac
