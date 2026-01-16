#!/bin/bash
# Build ONNX Runtime from source for iOS (device + simulator)
# This creates a static library with all dependencies (including Abseil) statically linked
#
# Prerequisites:
# - macOS with Xcode installed
# - CMake 3.26+
# - Python 3.8+
# - Git
#
# Usage: ./build-ios-onnxruntime-all.sh [version]
# Example: ./build-ios-onnxruntime-all.sh 1.22.2

set -e

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
TAURI_DIR="$(dirname "$SCRIPT_DIR")"
ORT_VERSION="${1:-1.22.2}"
BUILD_DIR="${TAURI_DIR}/onnxruntime-build"
OUTPUT_DIR="${TAURI_DIR}/onnxruntime-ios"
XCFRAMEWORK_DIR="${OUTPUT_DIR}/onnxruntime.xcframework"

IOS_DEPLOYMENT_TARGET="13.0"

echo "========================================"
echo "Building ONNX Runtime ${ORT_VERSION} for iOS"
echo "(Device + Simulator)"
echo "========================================"
echo "Build directory: ${BUILD_DIR}"
echo "Output directory: ${OUTPUT_DIR}"
echo ""

# Check prerequisites
command -v cmake >/dev/null 2>&1 || { echo "Error: cmake is required"; exit 1; }
command -v python3 >/dev/null 2>&1 || { echo "Error: python3 is required"; exit 1; }
command -v git >/dev/null 2>&1 || { echo "Error: git is required"; exit 1; }
command -v xcodebuild >/dev/null 2>&1 || { echo "Error: Xcode is required"; exit 1; }

# Check if output already exists
if [ -d "$XCFRAMEWORK_DIR" ] && [ -f "${XCFRAMEWORK_DIR}/ios-arm64/libonnxruntime.a" ] && [ -f "${XCFRAMEWORK_DIR}/ios-arm64-simulator/libonnxruntime.a" ]; then
    echo "ONNX Runtime xcframework already exists with both device and simulator libraries"
    echo "To rebuild, remove: rm -rf $OUTPUT_DIR"
    exit 0
fi

mkdir -p "$BUILD_DIR"
cd "$BUILD_DIR"

# Clone ONNX Runtime
clone_with_retry() {
    local max_attempts=3
    local attempt=1
    while [ $attempt -le $max_attempts ]; do
        echo "Attempt $attempt of $max_attempts..."
        if git clone --depth 1 --branch "v${ORT_VERSION}" --recursive https://github.com/microsoft/onnxruntime.git; then
            return 0
        fi
        sleep 10
        attempt=$((attempt + 1))
    done
    return 1
}

if [ ! -d "onnxruntime" ]; then
    echo "Cloning ONNX Runtime repository..."
    clone_with_retry
fi

cd onnxruntime

# Common cmake defines
CMAKE_EXTRA_DEFINES="CMAKE_POLICY_VERSION_MINIMUM=3.5"

# Function to build and combine libraries
build_and_combine() {
    local SYSROOT=$1
    local OUTPUT_SUFFIX=$2
    local EXTRA_CMAKE_DEFINES=$3
    
    echo ""
    echo "========================================"
    echo "Building for ${SYSROOT} (arm64)..."
    echo "========================================"
    
    # Clean previous build for this target
    rm -rf "build/iOS/Release"
    
    ./build.sh \
        --build_dir build/iOS \
        --config Release \
        --use_xcode \
        --ios \
        --apple_sysroot "${SYSROOT}" \
        --osx_arch arm64 \
        --apple_deploy_target "${IOS_DEPLOYMENT_TARGET}" \
        --parallel \
        --skip_tests \
        --compile_no_warning_as_error \
        --cmake_extra_defines "${CMAKE_EXTRA_DEFINES} ${EXTRA_CMAKE_DEFINES}"
    
    # Find and combine libraries
    local BUILD_OUTPUT_DIR="build/iOS/Release/Release-${SYSROOT}"
    local COMBINED_LIB="${BUILD_OUTPUT_DIR}/libonnxruntime_combined.a"
    
    echo "Combining static libraries..."
    local LIBS=$(find build/iOS/Release -name "*.a" -path "*Release-${SYSROOT}*" -type f | grep -v "gtest\|gmock" | sort -u)
    
    if [ -z "$LIBS" ]; then
        echo "Error: No libraries found for ${SYSROOT}"
        return 1
    fi
    
    libtool -static -o "$COMBINED_LIB" $LIBS 2>&1 | grep -v "warning duplicate member" || true
    
    if [ ! -f "$COMBINED_LIB" ]; then
        echo "Error: Failed to create combined library"
        return 1
    fi
    
    echo "Created: $COMBINED_LIB ($(du -h "$COMBINED_LIB" | cut -f1))"
    
    # Copy to output
    mkdir -p "${XCFRAMEWORK_DIR}/${OUTPUT_SUFFIX}"
    cp "$COMBINED_LIB" "${XCFRAMEWORK_DIR}/${OUTPUT_SUFFIX}/libonnxruntime.a"
}

# Create output directories
mkdir -p "${OUTPUT_DIR}"
mkdir -p "${XCFRAMEWORK_DIR}/Headers"

# Build for device
build_and_combine "iphoneos" "ios-arm64" ""

# Build for simulator
# CMAKE_FIND_ROOT_PATH_MODE_LIBRARY=NEVER fixes the libiconv linking bug
build_and_combine "iphonesimulator" "ios-arm64-simulator" "CMAKE_FIND_ROOT_PATH_MODE_LIBRARY=NEVER"

# Copy headers
HEADER_DIR=$(find build -name "onnxruntime_c_api.h" -type f | head -n 1 | xargs dirname 2>/dev/null)
if [ -n "$HEADER_DIR" ]; then
    cp "${HEADER_DIR}"/*.h "${XCFRAMEWORK_DIR}/Headers/" 2>/dev/null || true
fi
if [ -d "include/onnxruntime/core/session" ]; then
    cp include/onnxruntime/core/session/*.h "${XCFRAMEWORK_DIR}/Headers/" 2>/dev/null || true
fi

# Create Info.plist
cat > "${XCFRAMEWORK_DIR}/Info.plist" << 'PLIST'
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
    <key>AvailableLibraries</key>
    <array>
        <dict>
            <key>HeadersPath</key>
            <string>Headers</string>
            <key>LibraryIdentifier</key>
            <string>ios-arm64</string>
            <key>LibraryPath</key>
            <string>libonnxruntime.a</string>
            <key>SupportedArchitectures</key>
            <array>
                <string>arm64</string>
            </array>
            <key>SupportedPlatform</key>
            <string>ios</string>
        </dict>
        <dict>
            <key>HeadersPath</key>
            <string>Headers</string>
            <key>LibraryIdentifier</key>
            <string>ios-arm64-simulator</string>
            <key>LibraryPath</key>
            <string>libonnxruntime.a</string>
            <key>SupportedArchitectures</key>
            <array>
                <string>arm64</string>
            </array>
            <key>SupportedPlatform</key>
            <string>ios</string>
            <key>SupportedPlatformVariant</key>
            <string>simulator</string>
        </dict>
    </array>
    <key>CFBundlePackageType</key>
    <string>XFWK</string>
    <key>XCFrameworkFormatVersion</key>
    <string>1.0</string>
</dict>
</plist>
PLIST

echo ""
echo "========================================"
echo "Build complete!"
echo "========================================"
echo ""
echo "xcframework: ${XCFRAMEWORK_DIR}"
echo ""
echo "Libraries:"
ls -lh "${XCFRAMEWORK_DIR}/ios-arm64/libonnxruntime.a"
ls -lh "${XCFRAMEWORK_DIR}/ios-arm64-simulator/libonnxruntime.a"
# Generate cargo config
echo ""
echo "Generating .cargo/config.toml..."
"${SCRIPT_DIR}/setup-ios-cargo-config.sh"

echo ""
echo "Next steps:"
echo "1. Fix arm64-sim issue if needed (see docs/troubleshooting-ios-build.md)"
echo "2. Run: just ios-dev-sim 'iPhone 16 Pro'"
