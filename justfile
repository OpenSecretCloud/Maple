# Load environment variables from frontend/.env.local
set dotenv-path := "frontend/.env.local"
set dotenv-required := false

# List available commands
default:
    @just --list

# Install frontend dependencies
install:
    cd frontend && bun install

# Start the frontend development server
dev:
    cd frontend && bun run dev

build:
    cd frontend && bun run build

format:
    cd frontend && bun run format

lint:
    cd frontend && bun run lint

# Run Tauri iOS development build (default simulator)
ios-dev:
    cd frontend && bun run tauri ios dev

# Run Tauri iOS development build on specific simulator (e.g., "iPhone 16 Pro iOS 26")
ios-dev-sim simulator:
    cd frontend && bun run tauri ios dev '{{simulator}}'

# Run Tauri iOS development build on physical device (e.g., "Your iPhone")
ios-dev-device device:
    cd frontend && bun run tauri ios dev --device '{{device}}'

# Build and verify ONNX Runtime for iOS (device + simulator) - used by TTS and PDF OCR
ios-build-onnxruntime:
    ./scripts/ci/ios-onnxruntime.sh

# Setup cargo config for iOS ONNX Runtime (run after building ONNX Runtime)
ios-setup-cargo-config:
    cd frontend/src-tauri && ./scripts/setup-ios-cargo-config.sh

# Fix arm64-sim Xcode architecture issue (run if you get arm64-sim errors)
ios-fix-arch:
    #!/usr/bin/env bash
    set -euo pipefail
    cd frontend/src-tauri/gen/apple
    echo "Fixing arm64-sim architecture issue..."
    perl -i -0pe 's/ARCHS = \(\s*arm64,\s*"arm64-sim",\s*\);/ARCHS = arm64;/g' maple.xcodeproj/project.pbxproj
    sed -i '' 's/VALID_ARCHS = "arm64  arm64-sim";/VALID_ARCHS = arm64;/g' maple.xcodeproj/project.pbxproj
    sed -i '' 's/"EXCLUDED_ARCHS\[sdk=iphoneos\*\]" = "arm64-sim x86_64";/"EXCLUDED_ARCHS[sdk=iphoneos*]" = x86_64;/g' maple.xcodeproj/project.pbxproj
    sed -i '' 's/"EXCLUDED_ARCHS\[sdk=iphonesimulator\*\]" = arm64;/"EXCLUDED_ARCHS[sdk=iphonesimulator*]" = "";/g' maple.xcodeproj/project.pbxproj
    echo "Fixed! Verify with: grep -E 'ARCHS =|VALID_ARCHS|EXCLUDED_ARCHS' maple.xcodeproj/project.pbxproj"

# Build Tauri Android release
android-build:
    cd frontend/src-tauri && ./scripts/provide-android-onnxruntime.sh
    cd frontend && bun run tauri android build

# Build Tauri desktop release
desktop-build:
    cd frontend && src-tauri/scripts/run-with-desktop-onnxruntime.sh bun tauri build

# Run Tauri desktop development build, using workspace-local config when available
desktop-dev:
    cd frontend && if [ -f ../.local/tauri-workspace.json ]; then src-tauri/scripts/run-with-desktop-onnxruntime.sh bun tauri dev --config ../.local/tauri-workspace.json; else src-tauri/scripts/run-with-desktop-onnxruntime.sh bun tauri dev; fi

# Build Tauri desktop debug
desktop-build-debug:
    cd frontend && src-tauri/scripts/run-with-desktop-onnxruntime.sh bun tauri build --debug

# Build Tauri desktop release (with CC unset for compatibility)
desktop-build-no-cc:
    cd frontend && unset CC && src-tauri/scripts/run-with-desktop-onnxruntime.sh bun tauri build

# Build Tauri desktop debug (with CC unset for compatibility)
desktop-build-debug-no-cc:
    cd frontend && unset CC && src-tauri/scripts/run-with-desktop-onnxruntime.sh bun tauri build --debug

# Format Rust code
rust-fmt:
    cd frontend/src-tauri && cargo fmt

# Check Rust code compiles
rust-check:
    cd frontend/src-tauri && cargo check

# Run Clippy lints on Rust code
rust-clippy:
    cd frontend/src-tauri && cargo clippy

# Run all Rust checks (fmt check + clippy)
rust-lint:
    cd frontend/src-tauri && cargo fmt --check && cargo clippy -- -D warnings

# Update version across all required files
update-version version:
    #!/usr/bin/env bash
    set -euo pipefail
    
    echo "Updating version to {{version}}..."
    
    # Parse version into components
    IFS='.' read -r major minor patch <<< "{{version}}"
    
    # Android versionCode is not user-visible. It only has to increase for
    # every uploaded Play Store build and must stay <= 2100000000.
    current_android_version_code=$(jq -r '.bundle.android.versionCode' frontend/src-tauri/tauri.conf.json)
    android_version_code=$((current_android_version_code + 1))
    if [ "$android_version_code" -gt 2100000000 ]; then
        echo "Error: Android versionCode $android_version_code exceeds Google Play maximum 2100000000."
        exit 1
    fi
    echo "Calculated Android versionCode: $android_version_code (previous + 1)"
    
    # Update package.json
    sed -i 's/"version": "[^"]*"/"version": "{{version}}"/' frontend/package.json
    
    # Update tauri.conf.json version
    sed -i 's/"version": "[^"]*"/"version": "{{version}}"/' frontend/src-tauri/tauri.conf.json
    
    # Update tauri.conf.json Android versionCode
    sed -i "s/\"versionCode\": [0-9]*/\"versionCode\": $android_version_code/" frontend/src-tauri/tauri.conf.json
    
    # Update Cargo.toml
    sed -i 's/^version = "[^"]*"/version = "{{version}}"/' frontend/src-tauri/Cargo.toml
    
    # Update project.yml
    sed -i 's/CFBundleShortVersionString: .*/CFBundleShortVersionString: {{version}}/' frontend/src-tauri/gen/apple/project.yml
    sed -i 's/CFBundleVersion: .*/CFBundleVersion: {{version}}/' frontend/src-tauri/gen/apple/project.yml
    
    # Update Info.plist
    sed -i '/<key>CFBundleShortVersionString<\/key>/{n;s/<string>[^<]*<\/string>/<string>{{version}}<\/string>/;}' frontend/src-tauri/gen/apple/maple_iOS/Info.plist
    sed -i '/<key>CFBundleVersion<\/key>/{n;s/<string>[^<]*<\/string>/<string>{{version}}<\/string>/;}' frontend/src-tauri/gen/apple/maple_iOS/Info.plist
    
    # Run cargo check to update Cargo.lock
    echo "Running cargo check to update Cargo.lock..."
    cd frontend/src-tauri && cargo check
    
    echo "Version updated to {{version}} with Android versionCode $android_version_code in all files!"

# Get current version from package.json
get-version:
    @jq -r '.version' frontend/package.json

# Bump version by patch (0.0.1)
bump-patch:
    #!/usr/bin/env bash
    set -euo pipefail
    
    current=$(just get-version)
    IFS='.' read -r major minor patch <<< "$current"
    new_version="$major.$minor.$((patch + 1))"
    
    just update-version "$new_version"

# Bump version by minor (0.1.0)
bump-minor:
    #!/usr/bin/env bash
    set -euo pipefail
    
    current=$(just get-version)
    IFS='.' read -r major minor patch <<< "$current"
    new_version="$major.$((minor + 1)).0"
    
    just update-version "$new_version"

# Bump version by major (1.0.0)
bump-major:
    #!/usr/bin/env bash
    set -euo pipefail
    
    current=$(just get-version)
    IFS='.' read -r major minor patch <<< "$current"
    new_version="$((major + 1)).0.0"
    
    just update-version "$new_version"

# Increment Android versionCode for another Play Store upload with the same visible version
update-android-counter:
    #!/usr/bin/env bash
    set -euo pipefail
    
    # Get current versionCode from tauri.conf.json
    current_code=$(jq -r '.bundle.android.versionCode' frontend/src-tauri/tauri.conf.json)
    
    # Increment by one. This value is internal to Android/Play Store and is not user-visible.
    new_code=$((current_code + 1))
    
    # Ensure versionCode stays within Google Play's maximum.
    if [ "$new_code" -gt 2100000000 ]; then
        echo "Error: Android versionCode $new_code exceeds Google Play maximum 2100000000."
        exit 1
    fi
    
    echo "Updating Android versionCode: $current_code -> $new_code"
    
    # Update tauri.conf.json Android versionCode
    sed -i "s/\"versionCode\": $current_code/\"versionCode\": $new_code/" frontend/src-tauri/tauri.conf.json
    
    echo "Android versionCode updated to $new_code"

# Create a new release (updates version and creates git tag)
release version:
    just update-version {{version}}
    git add -A
    git commit -m "chore: bump version to {{version}}"
    git tag -a "v{{version}}" -m "Release v{{version}}"
    echo "Release v{{version}} created! Don't forget to push tags: git push && git push --tags"
