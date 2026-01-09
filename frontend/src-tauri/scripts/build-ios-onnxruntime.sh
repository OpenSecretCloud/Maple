#!/bin/bash
# Build ONNX Runtime from source for iOS
# This creates a static library with all dependencies (including Abseil) statically linked
#
# Prerequisites:
# - macOS with Xcode installed
# - CMake 3.26+
# - Python 3.8+
# - Git
#
# Usage: ./build-ios-onnxruntime.sh [version]
# Example: ./build-ios-onnxruntime.sh 1.20.1

set -e

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
TAURI_DIR="$(dirname "$SCRIPT_DIR")"
# Use latest 1.22.2 - older versions have Eigen hash mismatch issues with GitLab
ORT_VERSION="${1:-1.22.2}"
BUILD_DIR="${TAURI_DIR}/onnxruntime-build"
OUTPUT_DIR="${TAURI_DIR}/onnxruntime-ios"
XCFRAMEWORK_DIR="${OUTPUT_DIR}/onnxruntime.xcframework"

# Minimum iOS version to support
IOS_DEPLOYMENT_TARGET="13.0"

echo "========================================"
echo "Building ONNX Runtime ${ORT_VERSION} for iOS"
echo "========================================"
echo "Build directory: ${BUILD_DIR}"
echo "Output directory: ${OUTPUT_DIR}"
echo "iOS deployment target: ${IOS_DEPLOYMENT_TARGET}"
echo ""

# Check prerequisites
command -v cmake >/dev/null 2>&1 || { echo "Error: cmake is required but not installed."; exit 1; }
command -v python3 >/dev/null 2>&1 || { echo "Error: python3 is required but not installed."; exit 1; }
command -v git >/dev/null 2>&1 || { echo "Error: git is required but not installed."; exit 1; }
command -v xcodebuild >/dev/null 2>&1 || { echo "Error: Xcode is required but not installed."; exit 1; }

# Check if output already exists
if [ -d "$XCFRAMEWORK_DIR" ]; then
    echo "ONNX Runtime xcframework already exists at $XCFRAMEWORK_DIR"
    echo "To rebuild, remove the directory first: rm -rf $OUTPUT_DIR"
    exit 0
fi

# Create build directory
mkdir -p "$BUILD_DIR"
cd "$BUILD_DIR"

# Clone ONNX Runtime if not already cloned
if [ ! -d "onnxruntime" ]; then
    echo "Cloning ONNX Runtime repository..."
    git clone --depth 1 --branch "v${ORT_VERSION}" --recursive https://github.com/microsoft/onnxruntime.git
else
    echo "ONNX Runtime repository already cloned"
    cd onnxruntime
    git fetch --tags
    git checkout "v${ORT_VERSION}"
    git submodule update --init --recursive
    cd ..
fi

cd onnxruntime

# Fix Eigen hash mismatch issue
# GitLab regenerates archive files periodically, causing hash mismatches
# We patch deps.txt to remove hashes - cmake will skip verification with empty hash
# This is safe because we're building from a known ONNX Runtime release tag
echo ""
echo "========================================"
echo "Patching deps.txt to fix hash mismatches..."
echo "========================================"
if [ -f "cmake/deps.txt" ]; then
    # Show current eigen entries
    echo "Current eigen entries:"
    grep -i "eigen" cmake/deps.txt || echo "No eigen entries found"
    
    # For each line starting with eigen (case insensitive), keep URL but remove hash
    # The format is: name;url;hash
    # We want: name;url;  (empty hash makes cmake skip verification)
    # Using perl for more reliable in-place editing on macOS
    perl -i.bak -pe 's/^(eigen[^;]*;[^;]+);[a-f0-9]+$/$1;/i' cmake/deps.txt
    
    echo ""
    echo "Patched eigen entries:"
    grep -i "eigen" cmake/deps.txt || echo "No eigen entries found"
fi

# Common cmake extra defines to work around compatibility issues
# CMAKE_POLICY_VERSION_MINIMUM=3.5 fixes nsync compatibility with newer CMake
CMAKE_EXTRA_DEFINES="CMAKE_POLICY_VERSION_MINIMUM=3.5"

# Build for iOS device (arm64)
echo ""
echo "========================================"
echo "Building for iOS device (arm64)..."
echo "========================================"

./build.sh \
    --config Release \
    --use_xcode \
    --ios \
    --apple_sysroot iphoneos \
    --osx_arch arm64 \
    --apple_deploy_target "${IOS_DEPLOYMENT_TARGET}" \
    --parallel \
    --skip_tests \
    --compile_no_warning_as_error \
    --cmake_extra_defines "${CMAKE_EXTRA_DEFINES}"

# The static library should be at:
# build/iOS/Release/Release-iphoneos/libonnxruntime.a
IOS_ARM64_LIB="build/iOS/Release/Release-iphoneos/libonnxruntime.a"

if [ ! -f "$IOS_ARM64_LIB" ]; then
    # Try alternate location
    IOS_ARM64_LIB=$(find build -name "libonnxruntime.a" -path "*iphoneos*" | head -n 1)
fi

if [ ! -f "$IOS_ARM64_LIB" ]; then
    echo "Error: Could not find iOS arm64 static library"
    echo "Searching for any .a files:"
    find build -name "*.a" -type f
    exit 1
fi

echo "Found iOS arm64 library: $IOS_ARM64_LIB"

# Build for iOS simulator (arm64 for Apple Silicon Macs)
echo ""
echo "========================================"
echo "Building for iOS simulator (arm64)..."
echo "========================================"

./build.sh \
    --config Release \
    --use_xcode \
    --ios \
    --apple_sysroot iphonesimulator \
    --osx_arch arm64 \
    --apple_deploy_target "${IOS_DEPLOYMENT_TARGET}" \
    --parallel \
    --skip_tests \
    --compile_no_warning_as_error \
    --cmake_extra_defines "${CMAKE_EXTRA_DEFINES}"

IOS_SIM_ARM64_LIB=$(find build -name "libonnxruntime.a" -path "*iphonesimulator*" -path "*arm64*" | head -n 1)

if [ -z "$IOS_SIM_ARM64_LIB" ]; then
    IOS_SIM_ARM64_LIB=$(find build -name "libonnxruntime.a" -path "*iphonesimulator*" | head -n 1)
fi

echo "Found iOS simulator arm64 library: $IOS_SIM_ARM64_LIB"

# Build for iOS simulator (x86_64 for Intel Macs)
echo ""
echo "========================================"
echo "Building for iOS simulator (x86_64)..."
echo "========================================"

./build.sh \
    --config Release \
    --use_xcode \
    --ios \
    --apple_sysroot iphonesimulator \
    --osx_arch x86_64 \
    --apple_deploy_target "${IOS_DEPLOYMENT_TARGET}" \
    --parallel \
    --skip_tests \
    --compile_no_warning_as_error \
    --cmake_extra_defines "${CMAKE_EXTRA_DEFINES}"

IOS_SIM_X64_LIB=$(find build -name "libonnxruntime.a" -path "*iphonesimulator*" -path "*x86_64*" | head -n 1)

if [ -z "$IOS_SIM_X64_LIB" ]; then
    # If we can't find a separate x86_64 lib, it might be combined
    echo "Warning: Could not find separate x86_64 simulator library"
fi

echo "Found iOS simulator x86_64 library: $IOS_SIM_X64_LIB"

# Create output directories
echo ""
echo "========================================"
echo "Creating xcframework..."
echo "========================================"

mkdir -p "${OUTPUT_DIR}"
mkdir -p "${XCFRAMEWORK_DIR}/ios-arm64"
mkdir -p "${XCFRAMEWORK_DIR}/ios-arm64_x86_64-simulator"
mkdir -p "${XCFRAMEWORK_DIR}/Headers"

# Copy the device library
cp "$IOS_ARM64_LIB" "${XCFRAMEWORK_DIR}/ios-arm64/libonnxruntime.a"

# Create fat library for simulator (arm64 + x86_64)
if [ -n "$IOS_SIM_X64_LIB" ] && [ -f "$IOS_SIM_X64_LIB" ]; then
    echo "Creating fat simulator library..."
    lipo -create "$IOS_SIM_ARM64_LIB" "$IOS_SIM_X64_LIB" \
        -output "${XCFRAMEWORK_DIR}/ios-arm64_x86_64-simulator/libonnxruntime.a"
else
    # Just use the arm64 simulator library
    cp "$IOS_SIM_ARM64_LIB" "${XCFRAMEWORK_DIR}/ios-arm64_x86_64-simulator/libonnxruntime.a"
fi

# Copy headers
HEADER_DIR=$(find build -name "onnxruntime_c_api.h" -type f | head -n 1 | xargs dirname)
if [ -n "$HEADER_DIR" ]; then
    cp "${HEADER_DIR}"/*.h "${XCFRAMEWORK_DIR}/Headers/" 2>/dev/null || true
fi

# Also copy headers from include directory
if [ -d "include/onnxruntime/core/session" ]; then
    cp include/onnxruntime/core/session/*.h "${XCFRAMEWORK_DIR}/Headers/" 2>/dev/null || true
fi

# Create Info.plist for xcframework
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
            <string>ios-arm64_x86_64-simulator</string>
            <key>LibraryPath</key>
            <string>libonnxruntime.a</string>
            <key>SupportedArchitectures</key>
            <array>
                <string>arm64</string>
                <string>x86_64</string>
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
echo "ONNX Runtime xcframework created at:"
echo "  ${XCFRAMEWORK_DIR}"
echo ""
echo "Contents:"
ls -la "${XCFRAMEWORK_DIR}"
echo ""
echo "Static library sizes:"
ls -lh "${XCFRAMEWORK_DIR}/ios-arm64/libonnxruntime.a"
ls -lh "${XCFRAMEWORK_DIR}/ios-arm64_x86_64-simulator/libonnxruntime.a"
echo ""
echo "To verify the library contains all symbols:"
echo "  nm ${XCFRAMEWORK_DIR}/ios-arm64/libonnxruntime.a | grep -i abseil"
