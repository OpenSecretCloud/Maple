#!/bin/bash
# Setup ONNX Runtime for iOS builds
# This script downloads pre-built ONNX Runtime xcframework from HuggingFace
# or builds from source if needed.

set -e

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
TAURI_DIR="$(dirname "$SCRIPT_DIR")"
ORT_VERSION="${ORT_VERSION:-1.20.1}"
ORT_DIR="$TAURI_DIR/onnxruntime-ios"
XCFRAMEWORK_DIR="$ORT_DIR/onnxruntime.xcframework"

echo "Setting up ONNX Runtime $ORT_VERSION for iOS..."
echo "Target directory: $ORT_DIR"

# Check if already downloaded
if [ -d "$XCFRAMEWORK_DIR" ]; then
    echo "ONNX Runtime xcframework already exists at $XCFRAMEWORK_DIR"
    echo "To re-download, remove the directory first: rm -rf $ORT_DIR"
    exit 0
fi

# Create directory
mkdir -p "$ORT_DIR"

# Download pre-built xcframework from HuggingFace
# Repository: https://huggingface.co/csukuangfj/ios-onnxruntime
HF_BASE_URL="https://huggingface.co/csukuangfj/ios-onnxruntime/resolve/main"

echo "Downloading ONNX Runtime $ORT_VERSION xcframework from HuggingFace..."

# Download the xcframework directory structure
# The structure is:
# onnxruntime.xcframework/
#   Info.plist
#   Headers/
#     cpu_provider_factory.h
#     onnxruntime_c_api.h
#     onnxruntime_cxx_api.h
#     onnxruntime_cxx_inline.h
#   ios-arm64/
#     onnxruntime.a
#   ios-arm64_x86_64-simulator/
#     onnxruntime.a

mkdir -p "$XCFRAMEWORK_DIR/Headers"
mkdir -p "$XCFRAMEWORK_DIR/ios-arm64"
mkdir -p "$XCFRAMEWORK_DIR/ios-arm64_x86_64-simulator"

echo "Downloading Info.plist..."
curl -L -o "$XCFRAMEWORK_DIR/Info.plist" \
    "$HF_BASE_URL/$ORT_VERSION/onnxruntime.xcframework/Info.plist"

echo "Downloading headers..."
for header in cpu_provider_factory.h onnxruntime_c_api.h onnxruntime_cxx_api.h onnxruntime_cxx_inline.h; do
    curl -L -o "$XCFRAMEWORK_DIR/Headers/$header" \
        "$HF_BASE_URL/$ORT_VERSION/onnxruntime.xcframework/Headers/$header"
done

echo "Downloading iOS arm64 static library (this may take a while)..."
curl -L -o "$XCFRAMEWORK_DIR/ios-arm64/onnxruntime.a" \
    "$HF_BASE_URL/$ORT_VERSION/onnxruntime.xcframework/ios-arm64/onnxruntime.a"

echo "Downloading iOS simulator static library (this may take a while)..."
curl -L -o "$XCFRAMEWORK_DIR/ios-arm64_x86_64-simulator/onnxruntime.a" \
    "$HF_BASE_URL/$ORT_VERSION/onnxruntime.xcframework/ios-arm64_x86_64-simulator/onnxruntime.a"

echo ""
echo "ONNX Runtime xcframework downloaded successfully!"
echo "Location: $XCFRAMEWORK_DIR"
echo ""
echo "Contents:"
ls -la "$XCFRAMEWORK_DIR"
echo ""
echo "Static library sizes:"
ls -lh "$XCFRAMEWORK_DIR/ios-arm64/onnxruntime.a"
ls -lh "$XCFRAMEWORK_DIR/ios-arm64_x86_64-simulator/onnxruntime.a"
