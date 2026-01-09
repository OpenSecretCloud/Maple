#!/usr/bin/env bash

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
TAURI_DIR="$(cd "${SCRIPT_DIR}/.." && pwd)"

ORT_VERSION="${1:-1.22.2}"
IOS_DEPLOYMENT_TARGET="${IOS_DEPLOYMENT_TARGET:-13.0}"

BUILD_ROOT="${TAURI_DIR}/onnxruntime-build"
SRC_DIR="${BUILD_ROOT}/onnxruntime"
OUT_DIR="${TAURI_DIR}/onnxruntime-ios"
XC_DIR="${OUT_DIR}/onnxruntime.xcframework"

if [ -d "${XC_DIR}" ]; then
  echo "ONNX Runtime xcframework already exists at ${XC_DIR}"
  exit 0
fi

mkdir -p "${BUILD_ROOT}"
mkdir -p "${OUT_DIR}"

for tool in git python3 cmake xcodebuild lipo; do
  if ! command -v "${tool}" >/dev/null 2>&1; then
    echo "Error: required tool '${tool}' not found"
    exit 1
  fi
done

if [ ! -d "${SRC_DIR}" ]; then
  git clone --depth 1 --branch "v${ORT_VERSION}" --recursive \
    https://github.com/microsoft/onnxruntime.git "${SRC_DIR}"
else
  cd "${SRC_DIR}"
  git fetch --tags
  git checkout "v${ORT_VERSION}"
  git submodule update --init --recursive
fi

cd "${SRC_DIR}"

build_one() {
  local sysroot="$1"
  local arch="$2"
  local build_dir="$3"

  ./build.sh \
    --config Release \
    --use_xcode \
    --ios \
    --apple_sysroot "${sysroot}" \
    --osx_arch "${arch}" \
    --apple_deploy_target "${IOS_DEPLOYMENT_TARGET}" \
    --build_dir "${build_dir}" \
    --parallel \
    --skip_tests \
    --compile_no_warning_as_error
}

echo "Building ONNX Runtime v${ORT_VERSION} for iOS (deployment target ${IOS_DEPLOYMENT_TARGET})"

DEVICE_BUILD_DIR="${BUILD_ROOT}/build-iphoneos-arm64"
SIM_ARM64_BUILD_DIR="${BUILD_ROOT}/build-iphonesimulator-arm64"
SIM_X64_BUILD_DIR="${BUILD_ROOT}/build-iphonesimulator-x86_64"

build_one "iphoneos" "arm64" "${DEVICE_BUILD_DIR}"
build_one "iphonesimulator" "arm64" "${SIM_ARM64_BUILD_DIR}"
build_one "iphonesimulator" "x86_64" "${SIM_X64_BUILD_DIR}"

DEVICE_LIB="$(find "${DEVICE_BUILD_DIR}" -name "libonnxruntime.a" -type f | head -n 1 || true)"
SIM_ARM64_LIB="$(find "${SIM_ARM64_BUILD_DIR}" -name "libonnxruntime.a" -type f | head -n 1 || true)"
SIM_X64_LIB="$(find "${SIM_X64_BUILD_DIR}" -name "libonnxruntime.a" -type f | head -n 1 || true)"

if [ -z "${DEVICE_LIB}" ] || [ -z "${SIM_ARM64_LIB}" ] || [ -z "${SIM_X64_LIB}" ]; then
  echo "Error: failed to locate libonnxruntime.a outputs"
  echo "  DEVICE_LIB=${DEVICE_LIB}"
  echo "  SIM_ARM64_LIB=${SIM_ARM64_LIB}"
  echo "  SIM_X64_LIB=${SIM_X64_LIB}"
  exit 1
fi

HEADERS_DIR="${OUT_DIR}/Headers"
mkdir -p "${HEADERS_DIR}"

cp -f "${SRC_DIR}/include/onnxruntime/core/session/onnxruntime_c_api.h" "${HEADERS_DIR}/"
cp -f "${SRC_DIR}/include/onnxruntime/core/session/onnxruntime_cxx_api.h" "${HEADERS_DIR}/"
cp -f "${SRC_DIR}/include/onnxruntime/core/session/onnxruntime_cxx_inline.h" "${HEADERS_DIR}/"
cp -f "${SRC_DIR}/include/onnxruntime/core/providers/cpu/cpu_provider_factory.h" "${HEADERS_DIR}/"

SIM_UNIVERSAL_LIB="${OUT_DIR}/libonnxruntime-simulator-universal.a"
lipo -create "${SIM_ARM64_LIB}" "${SIM_X64_LIB}" -output "${SIM_UNIVERSAL_LIB}"

rm -rf "${XC_DIR}"
xcodebuild -create-xcframework \
  -library "${DEVICE_LIB}" -headers "${HEADERS_DIR}" \
  -library "${SIM_UNIVERSAL_LIB}" -headers "${HEADERS_DIR}" \
  -output "${XC_DIR}"

echo "Created ${XC_DIR}"
