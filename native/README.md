# Maple 3.0 (Native)

Native cross-platform AI agent chat app built with Rust core + platform-native UIs.

## Architecture

```
native/
├── rust/              # Shared Rust core (maple_core) - all business logic
│   ├── src/lib.rs     # AppState, actions, reducers, SSE streaming, pagination
│   └── uniffi.toml    # FFI bindings config (cloud.opensecret.maple.rust)
├── ios/               # SwiftUI frontend (iOS 26+, liquid glass)
│   ├── Sources/       # ContentView, AppManager, MapleTheme, MapleWordmark
│   └── project.yml    # XcodeGen project (cloud.opensecret.maple)
├── android/           # Jetpack Compose frontend (Material 3)
│   └── app/src/main/java/cloud/opensecret/maple/
├── desktop/iced/      # iced 0.14 desktop frontend
│   └── src/main.rs    # Single-file app with theme.rs
├── rmp.toml           # RMP project config
├── justfile           # Build commands (also accessible from repo root)
└── Cargo.toml         # Workspace: rust, uniffi-bindgen, desktop/iced
```

**Design principle**: All logic lives in `rust/src/lib.rs`. Platforms are thin
renderers that dispatch `AppAction`s and render `AppState`. No business logic
in Swift/Kotlin/Rust-iced code.

See `rmp-architecture-bible.md` for the full RMP framework reference.

## Bundle IDs & Versions

| Platform | Bundle ID | Version |
|----------|-----------|---------|
| iOS | `cloud.opensecret.maple` (debug: `.dev` suffix) | 3.0.0 |
| Android | `cloud.opensecret.maple` (debug: `.dev` suffix) | 3.0.0 (versionCode: 3000000000) |
| Desktop | `cloud.opensecret.maple.desktop` (keyring) | 3.0.0 |

These match Maple 2.0's App Store / Play Store listings for upgrade continuity.
Team ID: `X773Y823TN`.

## Dependencies

- **OpenSecret SDK**: `opensecret = "3.0.0-alpha.0"` from crates.io
- **FFI**: UniFFI 0.31 (generates Swift + Kotlin bindings)
- **Desktop**: iced 0.14 with tokio + canvas features
- **Build tooling**: RMP CLI, Nix flake, XcodeGen

## Quick Start

From the repo root (`maple/`):

```bash
# Run on each platform
just native-run-ios
just native-run-android
just native-run-desktop

# Regenerate FFI bindings after changing rust/src/lib.rs
just native-bindings

# Or from inside native/ directly
just run-ios
just run-android
just run-desktop
```

From OrbStack (Linux container with macOS host):

```bash
mac just -f /path/to/maple/native/justfile run-ios
mac just -f /path/to/maple/native/justfile run-android
# Desktop builds natively on mac:
mac just -f /path/to/maple/native/justfile run-desktop
```

## Key Features

- **Single-agent chat** with SSE streaming via OpenSecret API
- **Session persistence** (keychain on iOS, EncryptedSharedPreferences on Android, keyring on desktop)
- **Cursor-based pagination** (20 messages per page, prefetch within 3 of top)
- **iMessage-style timestamps** with smart grouping (5+ min gap threshold)
- **Settings** with delete agent + sign out
- **OAuth** (GitHub, Google, Apple) + email/password auth
- **Splash screen** with radial gradient and SVG wordmark

## Design System

- **iOS**: Native iOS 26 Liquid Glass (`.glassEffect()`, `GlassEffectContainer`)
- **Android**: Simulated frosted glass (translucent containers, scale animations)
- **Desktop**: Simulated frosted glass (translucent containers, subtle borders/shadows)
- **Brand**: Manrope body font, Array-Bold display font, MPL app icon
- **Colors**: Maple (primary), Pebble (secondary), Bark (tertiary), Grove, Neutral
- **Bubbles**: User = Maple 400→600 gradient, Agent = Pebble-50

## Relationship to Maple 2.0

Maple 2.0 lives in `frontend/` (Tauri + React/TypeScript web app).
Maple 3.0 lives in `native/` (pure native apps).

Both share the same bundle IDs and App Store listings. When 3.0 is ready,
it will replace 2.0 as the shipped product. Until then, both coexist.
