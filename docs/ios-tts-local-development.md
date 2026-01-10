# iOS TTS Local Development Guide

This guide explains how to build and test the iOS TTS (Text-to-Speech) feature locally using the iOS Simulator or a physical device.

## Overview

The iOS TTS feature uses ONNX Runtime to run Kokoro TTS models. ONNX Runtime must be built from source for iOS because:
1. Pre-built binaries from HuggingFace are missing Abseil symbols
2. We need both device (arm64) and simulator (arm64) builds
3. The simulator build requires a workaround for a libiconv linking bug

## Prerequisites

- macOS with Xcode installed (16.x+)
- CMake 3.26+
- Python 3.8+
- Git
- Nix (recommended) or manually install Rust toolchain

## Quick Start

### 1. Build ONNX Runtime

```bash
just ios-build-onnxruntime
```

This builds ONNX Runtime for both device and simulator (~5-10 minutes) and automatically generates the cargo config.

The output will be in `frontend/src-tauri/onnxruntime-ios/onnxruntime.xcframework/`.

### 2. Regenerate Cargo Config (if needed)

If you move the project or need to regenerate the cargo config:

```bash
just ios-setup-cargo-config
```

This creates `frontend/src-tauri/.cargo/config.toml` with the correct absolute paths for your machine.

### 3. Fix arm64-sim Xcode Issue (if needed)

If you see this error:
```
clang: error: version '-sim' in target triple 'arm64-apple-ios13.0-simulator-sim' is invalid
```

See [troubleshooting-ios-build.md](./troubleshooting-ios-build.md) for details.

Quick fix:
```bash
just ios-fix-arch
```

### 4. Run on Simulator

```bash
# Boot simulator first
xcrun simctl boot "iPhone 16 Pro"

# Run the app
just ios-dev-sim "iPhone 16 Pro"
```

### 5. Run on Physical Device

```bash
just ios-dev
```

Note: If you have a device connected (even wirelessly), `just ios-dev` may deploy to it instead of the simulator. Use `just ios-dev-sim` to explicitly target the simulator.

## Troubleshooting

### Vite Server Not Reachable

The iOS simulator needs to connect to your development server. Ensure `frontend/vite.config.ts` has:

```typescript
server: {
  host: "0.0.0.0",
  port: 5173,
  strictPort: true
}
```

### Missing Abseil Symbols

If you see linker errors like:
```
Undefined symbols for architecture arm64:
  "_AbslInternalSpinLockDelay_lts_20240722"
```

This means the ONNX Runtime library wasn't built from source. Pre-built binaries are missing these symbols. Run `./scripts/build-ios-onnxruntime-all.sh` to build from source.

### Simulator Build Fails with libiconv Error

If the simulator build fails with:
```
ld: building for 'iOS-simulator', but linking in dylib built for 'iOS'
```

This is fixed by adding `CMAKE_FIND_ROOT_PATH_MODE_LIBRARY=NEVER` to the cmake flags. The `build-ios-onnxruntime-all.sh` script already includes this fix.

### Cargo Not Finding Library

1. Ensure `.cargo/config.toml` uses absolute paths
2. Clean and rebuild: `cd frontend/src-tauri && cargo clean`
3. Verify the library exists: `ls -la onnxruntime-ios/onnxruntime.xcframework/ios-arm64-simulator/`

## Architecture Notes

### Why Build from Source?

1. **Abseil symbols**: ONNX Runtime depends on Abseil (Google's C++ library). Pre-built binaries don't include these statically linked.

2. **Simulator support**: Pre-built libraries often only include device builds.

3. **Version compatibility**: Building from source ensures compatibility with our ort-sys Rust crate version.

### Build Artifacts

After building, you'll have:
```
frontend/src-tauri/
├── onnxruntime-build/          # Build directory (can be deleted after build)
│   └── onnxruntime/            # ONNX Runtime source
└── onnxruntime-ios/            # Output directory
    └── onnxruntime.xcframework/
        ├── Headers/
        ├── Info.plist
        ├── ios-arm64/
        │   └── libonnxruntime.a     # Device library (~69MB)
        └── ios-arm64-simulator/
            └── libonnxruntime.a     # Simulator library (~69MB)
```

### .cargo/config.toml

The cargo config tells the Rust `ort-sys` crate where to find the ONNX Runtime library. The keys are:
- `[target.aarch64-apple-ios.onnxruntime]` - Device builds
- `[target.aarch64-apple-ios-sim.onnxruntime]` - Simulator builds (ARM64 Mac)
- `[target.x86_64-apple-ios.onnxruntime]` - Simulator builds (Intel Mac)

### CI/CD

The CI workflow (`mobile-build.yml`) builds ONNX Runtime from source and caches it. The cache key includes the version number, so updating `ORT_VERSION` will trigger a rebuild.

## Cleaning Up

To free disk space after testing:

```bash
# Remove build directory (keeps the built xcframework)
rm -rf frontend/src-tauri/onnxruntime-build

# Remove everything (requires rebuilding)
rm -rf frontend/src-tauri/onnxruntime-build frontend/src-tauri/onnxruntime-ios
```

## Related Documentation

- [troubleshooting-ios-build.md](./troubleshooting-ios-build.md) - arm64-sim architecture fix
- [tts-research.md](./tts-research.md) - TTS implementation details
