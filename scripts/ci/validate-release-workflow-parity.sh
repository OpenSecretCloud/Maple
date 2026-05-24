#!/usr/bin/env bash
set -euo pipefail

script_dir="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
repo_root="$(cd "${script_dir}/../.." && pwd)"

require_contains() {
  local file="$1"
  local needle="$2"
  local description="$3"

  if ! grep -Fq -- "${needle}" "${repo_root}/${file}"; then
    echo "Missing release workflow parity invariant: ${description}" >&2
    echo "file=${file}" >&2
    echo "needle=${needle}" >&2
    return 1
  fi
}

require_master_and_release() {
  local master_file="$1"
  local release_needle="$2"
  local description="$3"
  shift 3

  for master_needle in "$@"; do
    require_contains "${master_file}" "${master_needle}" "master ${description}"
  done
  require_contains ".github/workflows/release.yml" "${release_needle}" "release ${description}"
}

require_master_and_release \
  ".github/workflows/desktop-build.yml" \
  "nix develop .#\${{ matrix.nix_shell }} -c ./scripts/ci/desktop-release.sh" \
  "desktop release build script" \
  "nix develop .#ci -c ./scripts/ci/desktop-release.sh"
require_contains ".github/workflows/desktop-build.yml" "desktop-release-macos-final.sha256" "master macOS desktop final proof"
require_contains ".github/workflows/desktop-build.yml" "desktop-release-linux-final.sha256" "master Linux desktop final proof"
require_contains ".github/workflows/release.yml" "desktop-release-\${{ matrix.artifact_prefix }}-final.sha256" "release desktop matrix final proof"
require_contains ".github/workflows/release.yml" "-path '*/reproducibility/desktop-release-*.sha256'" "release desktop proof upload"
require_master_and_release \
  ".github/workflows/desktop-build.yml" \
  "nix develop .#ci -c ./scripts/ci/verify-release-artifacts.sh artifacts linux macos" \
  "desktop release verifier targets" \
  "nix develop .#ci -c ./scripts/ci/verify-release-artifacts.sh artifacts linux macos"

require_master_and_release \
  ".github/workflows/android-build.yml" \
  "nix develop .#android -c ./scripts/ci/android-release.sh" \
  "Android release build script" \
  "nix develop .#android -c ./scripts/ci/android-release.sh"
require_master_and_release \
  ".github/workflows/android-build.yml" \
  "MAPLE_REQUIRE_ANDROID_SIGNATURE_VERIFICATION: \"1\"" \
  "Android signature verification enforcement" \
  "MAPLE_REQUIRE_ANDROID_SIGNATURE_VERIFICATION: \"1\""
require_contains ".github/workflows/android-build.yml" "android-release-final.sha256" "master Android final proof"
require_contains ".github/workflows/release.yml" "android-release-final.sha256" "release Android final proof"
require_contains ".github/workflows/release.yml" "-path '*/reproducibility/android-release-*.sha256'" "release Android proof upload"
require_master_and_release \
  ".github/workflows/android-build.yml" \
  "nix develop .#android -c ./scripts/ci/verify-release-artifacts.sh artifacts android" \
  "Android release verifier target" \
  "nix develop .#android -c ./scripts/ci/verify-release-artifacts.sh artifacts android"

require_master_and_release \
  ".github/workflows/mobile-build.yml" \
  "nix develop .#apple -c ./scripts/ci/ios-release.sh" \
  "iOS release build script" \
  "nix develop .#apple -c ./scripts/ci/ios-release.sh"
require_master_and_release \
  ".github/workflows/mobile-build.yml" \
  "MAPLE_ENFORCE_IOS_SIGNED_REPRODUCIBILITY: \"1\"" \
  "iOS signed reproducibility enforcement" \
  "MAPLE_ENFORCE_IOS_SIGNED_REPRODUCIBILITY: \"1\""
require_contains ".github/workflows/mobile-build.yml" "ios-release-final.sha256" "master iOS final proof"
require_contains ".github/workflows/mobile-build.yml" "ios-release-*.txt" "master iOS diff proof upload"
require_contains ".github/workflows/release.yml" "ios-release-final.sha256" "release iOS final proof"
require_contains ".github/workflows/release.yml" "-path '*/reproducibility/ios-release-*.txt'" "release iOS diff proof upload"
require_contains ".github/workflows/mobile-build.yml" "nix develop .#ci -c ./scripts/ci/verify-release-artifacts.sh artifacts ios" "master iOS release verifier target"
require_contains ".github/workflows/release.yml" "nix develop .#ci -c ./scripts/ci/verify-release-artifacts.sh artifacts linux macos ios web latest-json" "release final verifier target set"

require_master_and_release \
  ".github/workflows/web-build.yml" \
  "nix develop .#ci -c ./scripts/ci/web.sh" \
  "web release build script" \
  "nix develop .#ci -c ./scripts/ci/web.sh"
require_master_and_release \
  ".github/workflows/web-build.yml" \
  "MAPLE_WEB_ENVIRONMENT: release" \
  "web release environment" \
  "MAPLE_WEB_ENVIRONMENT: release"
require_master_and_release \
  ".github/workflows/web-build.yml" \
  "web-final.sha256" \
  "web final proof" \
  "web-final.sha256"
require_contains ".github/workflows/web-build.yml" "nix develop .#ci -c ./scripts/ci/verify-release-artifacts.sh artifacts web" "master web release verifier target"

require_contains ".github/workflows/release.yml" "needs: verify-desktop-release-artifacts" "latest.json waits for verified desktop artifacts"
require_contains ".github/workflows/release.yml" "nix develop .#ci -c ./scripts/ci/latest-json.sh artifacts latest.json" "release latest.json generator"
require_contains ".github/workflows/release.yml" "latest-json-final.sha256" "release latest.json proof"
require_contains ".github/workflows/release.yml" "subject-checksums: latest-json-artifacts.sha256" "release latest.json attestation"
require_contains ".github/workflows/release.yml" "- verify-android-release-artifacts" "release final verifier waits for Android verifier"
require_contains ".github/workflows/release.yml" "- build-ios" "release final verifier waits for iOS build"
require_contains ".github/workflows/release.yml" "- build-web" "release final verifier waits for web build"
require_contains ".github/workflows/release.yml" "- update-latest-json" "release final verifier waits for latest.json"

echo "release workflow parity invariants verified"
