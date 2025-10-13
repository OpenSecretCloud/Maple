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

# Test the frontend (needs local backend running)
test:
    cd tests && bun test 

format:
    cd frontend && bun run format

# Update version across all required files
update-version version:
    #!/usr/bin/env bash
    set -euo pipefail
    
    echo "Updating version to {{version}}..."
    
    # Parse version into components
    IFS='.' read -r major minor patch <<< "{{version}}"
    
    # Calculate Android versionCode: 1 + MMM (minor) + PPP (patch) + 000 (counter reset)
    # Format: 1MMMPPPCCC (10 digits total)
    android_version_code=$(printf "1%03d%03d000" "$minor" "$patch")
    echo "Calculated Android versionCode: $android_version_code"
    
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

# Increment Android versionCode counter (last 3 digits) for Play Store updates
update-android-counter:
    #!/usr/bin/env bash
    set -euo pipefail
    
    # Get current versionCode from tauri.conf.json
    current_code=$(jq -r '.bundle.android.versionCode' frontend/src-tauri/tauri.conf.json)
    
    # Extract base (first 7 digits) and counter (last 3 digits)
    base=$((current_code / 1000))
    counter=$((current_code % 1000))
    
    # Increment counter
    new_counter=$((counter + 1))
    
    # Ensure counter doesn't exceed 999
    if [ $new_counter -gt 999 ]; then
        echo "Error: Counter exceeds maximum (999). Consider bumping the version instead."
        exit 1
    fi
    
    # Calculate new versionCode
    new_code=$((base * 1000 + new_counter))
    
    echo "Updating Android versionCode: $current_code → $new_code"
    
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
