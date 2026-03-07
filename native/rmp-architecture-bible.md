---
summary: Comprehensive guide for building Rust Multi-Platform apps — philosophy, architecture, FFI, platform layers, build system, patterns, and scaffolding
read_when:
  - starting a new RMP project from scratch
  - deciding whether code belongs in Rust or native (iOS/Android/Desktop)
  - understanding the unidirectional data flow and actor model patterns
  - adding a new feature, screen, or platform capability bridge
  - onboarding to the Pika codebase or any RMP codebase
---

# RMP Architecture Bible

> A comprehensive guide for building Rust Multi-Platform applications targeting iOS, Android, and Desktop (Linux/macOS/Windows) with maximally shared Rust components and thin native UI layers.

## Intent

This document exists so that **any developer with an idea for any application** can build it using the RMP paradigm and have it target iOS/Android/Desktop with as much shared Rust as possible. The only platform-specific work should be the light native UI layer (SwiftUI, Jetpack Compose, iced, etc.) and bounded platform capability bridges.

The RMP model is a sustainable and correct way to build multi-platform apps, especially given that many SDKs and core libraries already exist in Rust. Rather than writing business logic three times (or using a lowest-common-denominator cross-platform framework), RMP puts Rust at the center and lets each platform do what it does best: render native UI.

**Reference implementation:** The Pika messaging app (`sledtools/pika`) is used throughout this document as the primary example. Pika is a real, battle-tested app -- but it is also alpha software that does not perfectly follow its own philosophy everywhere. Where Pika drifts from the ideal, this bible calls it out. **This document is the stricter standard.**

---

## Table of Contents

- [Part I: Philosophy and Core Principles](#part-i-philosophy-and-core-principles)
- [Part II: The Rust Core](#part-ii-the-rust-core)
- [Part III: The FFI Boundary (UniFFI)](#part-iii-the-ffi-boundary-uniffi)
- [Part IV: Platform Layers](#part-iv-platform-layers)
- [Part V: Build System and Cross-Compilation](#part-v-build-system-and-cross-compilation)
- [Part VI: Patterns and Recipes](#part-vi-patterns-and-recipes)
- [Part VII: Testing Strategy](#part-vii-testing-strategy)
- [Part VIII: From Zero to Running App](#part-viii-from-zero-to-running-app)
- [Part IX: Migration and Refactoring](#part-ix-migration-and-refactoring)
- [Appendix: Research Scratchpad](#appendix-research-scratchpad)

---

## Part I: Philosophy and Core Principles

### 1.1 The UX Invariant

RMP must never produce a second-class experience compared to a true native app. "Cross-platform purity" does not override user experience quality. If a Rust-first implementation would result in a worse user experience than a native one, the native approach wins -- but only for the rendering and OS-integration surface, never for business logic.

What this means in practice:

- **Navigation must feel native.** iOS users expect swipe-back gestures and `NavigationStack` transitions. Android users expect predictive back and Material motion. A todo app should use `NavigationStack` on iOS and `AnimatedContent` on Android -- not a shared cross-platform navigation abstraction that feels wrong on both.
- **System integration must be seamless.** A fitness tracker's background GPS tracking must use `CLLocationManager` on iOS and `FusedLocationProviderClient` on Android. A photo editor must use the platform's native file picker. These are not things you approximate.
- **Accessibility must be platform-native.** VoiceOver semantics on iOS and TalkBack semantics on Android differ. The native UI layer handles this; Rust provides the data.
- **Performance must meet platform expectations.** 60fps scrolling, instant touch response, smooth transitions. The Rust core must not block the UI thread, ever.

The invariant is simple: if a user cannot tell that Rust is involved, the architecture is working correctly.

### 1.2 What Rust Owns vs. What Native Owns

**Rust owns:**
- State machines and policy decisions (what screen to show, what happens when a button is tapped)
- Protocol, transport, and cryptographic behavior
- Long-lived application state (`AppState` and all actor-internal derivation)
- Cross-platform invariants and error semantics
- Business logic, validation, formatting, and domain rules
- Persistence (databases, file storage, caching)
- Networking (API calls, WebSocket connections, relay management)

**Native owns:**
- Rendering and UX affordances (SwiftUI views, Compose screens, iced widgets)
- Platform capability execution (audio routing, push notification surfaces, URL scheme handling, camera capture)
- Short-lived handles to OS resources (audio sessions, location managers, Bluetooth connections)
- Secure credential storage (iOS Keychain, Android EncryptedSharedPreferences)

**The preference order is strict:**
1. Rust implementation that preserves native-quality UX (always try this first)
2. Native capability bridge if and only if required for native-quality UX (see Section 1.4)

**The golden rule: native must NOT own app business logic.** If you find yourself writing an `if` statement in Swift or Kotlin that decides what the app should *do* (not how it should *look*), that logic belongs in Rust.

Examples of the boundary:
- **Correct:** Rust computes `display_timestamp: String` for a chat message; Swift/Kotlin just render it.
- **Wrong:** Swift formats the timestamp with `DateFormatter`; Kotlin formats it with `SimpleDateFormat`. Now you have two implementations to maintain, and they will diverge.
- **Correct:** Rust decides `can_send_message: bool` based on auth state, connectivity, and group membership; the native button reads this flag.
- **Wrong:** Kotlin checks `if (state.auth is AuthState.LoggedIn && state.currentChat != null)` in the Compose layer.

### 1.3 Unidirectional Data Flow (Elm Architecture)

RMP applications follow The Elm Architecture (TEA), also known as Model-View-Update. This is the same pattern used by Elm, Redux, iced, and Ratatui (see [Ratatui's TEA documentation](https://ratatui.rs/concepts/application-patterns/the-elm-architecture/) for an excellent introduction to the concept).

The pattern has three components:

1. **Model** (`AppState`) -- a single struct containing all data the UI needs to render.
2. **Message** (`AppAction`) -- an enum of every user intent or lifecycle event.
3. **Update** (`AppCore::handle_message`) -- a function that takes the current state and a message, and produces a new state.

The data flow is strictly one-directional:

```
User taps button
    → Native UI dispatches AppAction (fire-and-forget, never blocks)
    → Rust actor thread receives action via channel
    → handle_message() mutates AppState, increments rev
    → AppUpdate::FullState(state) sent to platform via AppReconciler callback
    → Native AppManager receives update on background thread
    → Hops to main/UI thread
    → Replaces state (mutableStateOf on Android, @Observable property on iOS)
    → UI framework detects change and re-renders
```

**Why this pattern?**
- **No data races.** A single actor thread owns all mutable state. No locks, no concurrent mutation, no race conditions.
- **Predictable debugging.** Every state change is caused by an `AppAction` or `InternalEvent`. You can log every action and reproduce any state.
- **Platform agnostic.** The update function doesn't know or care whether the UI is SwiftUI, Compose, iced, or a CLI. It produces state; the platform renders it.
- **Testable.** The core can be tested without any platform dependencies. Feed actions in, assert state out.

**Tradeoffs:**
- Full state snapshots are sent on every change (the MVP approach). This is simple but means every field of `AppState` is cloned and sent across the FFI boundary, even if only one field changed. This works for apps with moderate state complexity. For apps with very large state trees (thousands of list items, heavy media metadata), consider granular update variants as the app matures.
- `dispatch()` is fire-and-forget. There is no return value. The platform cannot know synchronously whether an action succeeded. Results come back as state changes or side-effect updates.

### 1.4 The Capability Bridge Pattern

A capability bridge is a bounded lifecycle where Rust leases a single responsibility to native runtime code while keeping policy and state ownership in Rust.

**The contract shape:**

1. **Rust opens the window** -- Rust decides that a platform capability is needed (e.g., "start recording audio," "request GPS coordinates," "show file picker").
2. **Native executes** -- The platform performs the side effect using OS APIs. It holds transient handles (audio sessions, location managers, camera captures) but makes no policy decisions.
3. **Native reports back** -- Raw data flows back to Rust via typed callback interfaces. Coordinates, audio samples, file paths -- not decisions.
4. **Rust decides** -- State updates, retries, fallbacks, error handling, and user-visible outcomes are all determined by Rust.
5. **Rust closes the window** -- Deterministic teardown. Native releases OS resources.

**Guardrails:**
- **No policy forks.** Native code never decides "should we retry?" or "is this error recoverable?" It reports the error; Rust decides.
- **Bounded native state.** Native holds only transient buffers and OS handles. No caches, no derived state, no business logic.
- **Idempotent lifecycle.** Start/stop/restart must be safe. The bridge must handle interruptions gracefully (app backgrounding, permission revocation, hardware disconnection).
- **Typed contracts.** Callback interfaces are defined in Rust with UniFFI annotations. Native code implements them. The contract is versioned and type-checked at compile time.

**Example -- a location bridge for a fitness tracker:**
```rust
#[uniffi::export(callback_interface)]
pub trait LocationProvider: Send + Sync + 'static {
    fn start_tracking(&self);
    fn stop_tracking(&self);
}

// Rust calls start_tracking() when user starts a workout.
// Native starts CLLocationManager / FusedLocationProvider.
// Native calls back with coordinates via a separate channel.
// Rust decides: store this point, discard noise, end workout, etc.
```

**Pika examples:**
- `ExternalSignerBridge` -- Rust asks native to open a URL or sign a Nostr event. Native executes the OS intent/URL scheme. Rust processes the result.
- `VideoFrameReceiver` -- Rust pushes decoded video frames to native at ~30fps. Native renders them. Rust decides frame rate, codec, and quality.
- `CallAudioSessionCoordinator` (iOS) -- Rust owns the call state machine. Native configures `AVAudioSession` modes based on call type (voice vs. video). Native never decides whether a call should start or end.

### 1.5 Decision Framework: Should This Be in Rust or Native?

Before adding logic to the native layer, answer these five questions:

1. **Can a Rust-first implementation match true native UX quality?** If yes, put it in Rust. Timestamp formatting, string validation, list sorting, business rules -- all of these can be done in Rust without UX degradation.
2. **Does it require an OS API that only exists on the platform?** Camera capture, push notification registration, Keychain/Keystore access, audio session routing -- these must be native. But the *logic around them* (when to start recording, what to do with the token, what credentials to store) stays in Rust.
3. **Is the native state purely transient?** The native side should hold only OS handles and buffers that have no meaning outside the current operation. If you're tempted to cache, sort, filter, or derive state on the native side, that logic belongs in Rust.
4. **Does Rust still decide policy and user-visible outcomes?** The native layer reports data; Rust makes decisions. If your Kotlin code contains `if/else` branches that determine app behavior (not just visual layout), move that logic to Rust.
5. **Can you test it without a device?** Rust core logic can be tested with `cargo test`. Native capability bridges are thin enough to mock. If your logic can only be tested on a simulator, it's probably too complex for the native layer.

**Examples across app types:**

| Feature | Rust | Native |
|---------|------|--------|
| **Todo app:** Toggle completion | Rust mutates item, recalculates counts, emits state | Native re-renders the checkbox |
| **Todo app:** Swipe-to-delete gesture | Rust handles `DeleteTodo { id }`, removes item | Native provides the swipe gesture UX |
| **Fitness tracker:** Compute pace/distance | Rust processes GPS coordinates, calculates metrics | Native provides `CLLocationManager` bridge |
| **Fitness tracker:** Background tracking | Rust decides tracking state | Native manages `BGAppRefreshTask` / `WorkManager` |
| **Photo editor:** Apply blur filter | Rust applies filter using `image` crate | Native displays the result as `UIImage` / `Bitmap` |
| **Photo editor:** Show file picker | Rust receives file path after selection | Native presents `UIDocumentPickerViewController` / `Intent.ACTION_OPEN_DOCUMENT` |
| **Messaging app:** Parse message content | Rust parses markdown, extracts mentions, formats timestamps | Native renders styled text |
| **Messaging app:** Camera for QR scan | Rust processes the decoded QR string | Native manages `AVCaptureSession` / CameraX |

### 1.6 Pika as Reference Implementation

Pika is used throughout this document as the primary example. It is a real app shipping to users -- an MLS-encrypted messenger over Nostr with iOS (SwiftUI), Android (Jetpack Compose), Desktop (iced), and CLI targets, all sharing a single Rust core. This makes it a credible, battle-tested reference.

But Pika is also alpha software. It does not perfectly follow its own stated philosophy everywhere. **This bible is the stricter standard.** Where Pika deviates, this document calls it out as a known drift, not as the correct approach.

**Known deviations (as of February 2026):**

- **Duplicated formatting logic.** Timestamp formatting, chat summary display strings, and peer key validation were implemented independently in both Swift and Kotlin instead of living in Rust. As of this draft, timestamp/chat-preview formatting is still native on both mobile platforms; lowering it to Rust fields (e.g., `display_timestamp`, `last_message_preview`) remains an active migration target.
- **Business logic in ViewState derivation.** Some iOS `ViewState` mapping functions contain conditional logic that should be computed in Rust and exposed as state fields.
- **Navigation leaks.** Platform code occasionally manages navigation state alongside Rust's `Router` instead of treating the Router as the sole source of truth.
- **God module.** `core/mod.rs` is 4,600+ lines -- the main actor file that handles all actions. This should be split by domain (chat, calls, profiles, auth) with each domain module handling its own action subset.
- **No Android ViewModels.** Compose screens read directly from `AppManager.state` without a ViewModel indirection layer. Phase 2 of the Android parity plan addresses this.

The lesson: even the team that designed the architecture drifts under shipping pressure. The bible exists to make the standard explicit so drift can be identified and corrected systematically.

---

## Part II: The Rust Core

### 2.1 Project Structure and Workspace Layout

An RMP app uses a Cargo workspace with the core logic in a single crate that compiles to three library types:

```
my-app/
├── Cargo.toml              # Workspace root
├── rmp.toml                # RMP project configuration
├── rust/                   # Core crate
│   ├── Cargo.toml          # crate-type = ["cdylib", "staticlib", "rlib"]
│   ├── src/
│   │   ├── lib.rs          # FfiApp, UniFFI scaffolding, callback interfaces
│   │   ├── state.rs        # AppState and all FFI-visible types
│   │   ├── actions.rs      # AppAction enum
│   │   ├── updates.rs      # AppUpdate enum + internal message types
│   │   └── core/           # Actor implementation and business logic
│   └── uniffi.toml         # Kotlin package configuration
├── uniffi-bindgen/         # Standalone binding generator binary
│   ├── Cargo.toml
│   └── src/main.rs         # calls uniffi::uniffi_bindgen_main()
├── ios/                    # iOS app (SwiftUI)
├── android/                # Android app (Jetpack Compose)
├── desktop/iced/           # Desktop app (iced, default rmp init scaffold)
└── crates/my-app-desktop/  # Desktop app in monorepo layout (Pika-style)
```

Both desktop layouts are valid. `rmp init --iced` scaffolds `desktop/iced/`; larger monorepos often relocate the app into `crates/<name>-desktop/` and wire it through the workspace.

The three library types serve different consumers:
- **`cdylib`** -- C-compatible dynamic library. Used by `uniffi-bindgen` to generate Swift and Kotlin bindings on the host, and as the `.so` loaded by JNA on Android.
- **`staticlib`** -- Static archive (`.a`). Linked into the iOS `.xcframework` for static linking.
- **`rlib`** -- Standard Rust library. Used by the desktop app and CLI as a direct Rust dependency (no FFI overhead), and by integration tests.

Pin protocol-critical dependencies at the workspace level to prevent version skew:
```toml
[workspace.dependencies]
mdk-core = { git = "...", rev = "d9f3727..." }  # MLS library -- must match everywhere
```

**Pika example:** The workspace has 18 member crates including the core (`rust/`), desktop (`crates/pika-desktop/`), CLI (`cli/`), notification extension (`crates/pika-nse/`), and the scaffolding tool (`crates/rmp-cli/`). Workspace-level pinning of `mdk-core` ensures the MLS wire format is identical across all consumers.

### 2.2 The Actor Model (AppCore)

The RMP core runs on a **dedicated OS thread** (the "actor thread") that owns all mutable state. This thread runs a synchronous event loop reading from a `flume` channel. A separate **tokio runtime** handles async I/O (networking, timers), feeding results back through the same channel.

```
┌──────────────┐    dispatch()     ┌─────────────────────────────┐
│  Native UI   │ ───────────────→  │  flume channel (CoreMsg)    │
│  (any thread)│                   └──────────┬──────────────────┘
└──────────────┘                              │
                                              ▼
                                   ┌─────────────────────────────┐
                                   │  Actor Thread (std::thread)  │
                                   │  while let Ok(msg) = rx.recv│
                                   │    core.handle_message(msg) │
                                   │                             │
                                   │  ┌────────────────────────┐ │
                                   │  │ tokio runtime (2 thds) │ │
                                   │  │ - network I/O          │ │
                                   │  │ - timers               │ │
                                   │  │ → sends InternalEvent   │ │
                                   │  │   back via channel     │ │
                                   │  └────────────────────────┘ │
                                   └──────────┬──────────────────┘
                                              │ emit AppUpdate
                                              ▼
                                   ┌─────────────────────────────┐
                                   │  Listener Thread            │
                                   │  reconciler.reconcile(upd)  │
                                   └─────────────────────────────┘
```

The actor thread is a plain `std::thread::spawn`, not a tokio task. This keeps the message loop deterministic and avoids executor coupling. The tokio runtime (typically 2 worker threads) is owned by the actor and used exclusively for I/O-bound work.

**Why this pattern?**
- **No data races.** The actor is the sole writer of `AppState`. No locks needed for state mutation.
- **Predictable ordering.** Messages are processed sequentially. Action A always completes before Action B starts.
- **Async without complexity.** Network requests run on tokio's thread pool, but their results are funneled back through the channel and processed synchronously by the actor. The actor never awaits anything.

**Pika implementation:** `FfiApp::new()` creates two `flume::unbounded` channels (`core_tx`/`core_rx` for inbound, `update_tx`/`update_rx` for outbound), spawns the actor thread, and stores the channels in the `FfiApp` struct. The actor creates a `tokio::runtime::Builder::new_multi_thread().worker_threads(2)` runtime for async I/O.

### 2.3 AppState Design

`AppState` is the single, serializable state tree that fully describes the UI at any point in time. It is annotated with `#[derive(uniffi::Record)]` so it crosses the FFI boundary as a value type (copied, not referenced).

**Structural principles:**

- **Flat and complete.** `AppState` contains everything the UI needs to render every screen. If a native view needs a computed value, compute it in Rust and add it as a field. Never derive business logic on the native side.
- **Monotonic revision.** `rev: u64` increments on every state change. Platforms use this to detect stale updates and skip redundant re-renders.
- **Navigation as state.** A `Router` field contains the current screen stack, making navigation declarative and Rust-driven.
- **Busy flags.** A `BusyState` record contains boolean flags for in-flight async operations (creating account, sending message, etc.). This lets the UI show spinners without native-side timing heuristics.
- **Ephemeral messages.** A `toast: Option<String>` field carries transient user-visible messages (errors, confirmations). The platform displays and then dispatches `ClearToast`.
- **Raw + display field pairs for user text.** Keep protocol-raw text and Rust-computed display text as separate fields when needed (e.g., `content` + `display_content`). Native renders display fields, while raw fields remain available for copy/share/debug flows.

**Shared state access.** `AppState` is held in an `Arc<RwLock<AppState>>`. The actor thread writes to it after every mutation. The platform can read a synchronous snapshot at any time via `FfiApp::state()`, which is useful for initial hydration before the update stream starts.

**Example for a todo app:**
```rust
#[derive(uniffi::Record, Clone, Debug)]
pub struct AppState {
    pub rev: u64,
    pub router: Router,
    pub items: Vec<TodoItem>,
    pub active_count: u32,      // computed in Rust, not by native
    pub busy: BusyState,
    pub toast: Option<String>,
}
```

**Pika example:** `AppState` contains `router`, `auth`, `my_profile`, `busy`, `chat_list` (Vec of summaries), `current_chat` (detailed view of the open chat), `follow_list`, `peer_profile`, `active_call`, `call_timeline`, and `toast`. Every nested type (`ChatSummary`, `ChatMessage`, `CallState`, etc.) is also a `uniffi::Record`.

### 2.4 AppAction: The Action Catalog

`AppAction` is a flat enum of every user-initiated intent and lifecycle event. It is annotated with `#[derive(uniffi::Enum)]` so platforms can construct and dispatch it.

**Design rules:**
- **Flat, not nested.** UniFFI works best with flat enums. Keep it as a single enum with descriptive variant names, organized by domain in comments.
- **Fire-and-forget.** `dispatch(action)` enqueues the action via the `flume` channel and returns immediately. It never blocks. There is no return value -- results come back as state changes.
- **Imperative, not declarative.** Actions describe what the user *wants to do* (`AddTodo { text }`, `ToggleTodo { id }`), not what the state *should become* (`SetTodos { items }`).
- **Secret-safe.** Implement a `tag()` method that returns a log-safe string representation. Never log secrets (keys, tokens, passwords) in action tags.

**Example for a todo app:**
```rust
#[derive(uniffi::Enum, Clone, Debug)]
pub enum AppAction {
    // Navigation
    PushScreen { screen: Screen },
    PopScreen,
    // Todos
    AddTodo { text: String },
    ToggleTodo { id: u64 },
    DeleteTodo { id: u64 },
    ClearCompleted,
    // UI
    ClearToast,
    // Lifecycle
    Foregrounded,
}
```

**Pika example:** `AppAction` has ~50 variants covering Auth (CreateAccount, Login, Logout), Navigation (PushScreen, UpdateScreenStack), Chat (SendMessage, OpenChat, LoadOlderMessages), Calling (StartCall, AcceptCall, ToggleMute), Group Chat (CreateGroupChat, AddGroupMembers), Hypernote (HypernoteAction), UI (ClearToast), Lifecycle (Foregrounded, ReloadConfig), Push (SetPushToken), and Follow List (FollowUser, UnfollowUser).

### 2.5 AppUpdate: The Update Stream

`AppUpdate` is the outbound enum sent from the Rust core to the platform. The primary variant is `FullState(AppState)` -- a complete snapshot sent on every state change.

**Side-effect variants** carry ephemeral data that should not live in `AppState`. In Pika today, these include:

- `AccountCreated { rev, nsec, pubkey, npub }` -- generated local account secret (`nsec`) must be persisted natively, never in Rust state.
- `BunkerSessionDescriptor { rev, bunker_uri, client_nsec }` -- bunker-session credentials must be persisted natively, never in Rust state.

```rust
#[derive(uniffi::Enum, Clone, Debug)]
pub enum AppUpdate {
    FullState(AppState),
    AccountCreated { rev: u64, nsec: String, pubkey: String, npub: String },
    BunkerSessionDescriptor { rev: u64, bunker_uri: String, client_nsec: String },
}
```

Side-effect variants carry a `rev` field so platforms can maintain ordering. The native `apply()` method handles side effects (e.g., saving credentials) *before* checking the rev guard, ensuring credentials are never lost even if the rev check would skip the update.

Design rule: if an auth/capability flow produces a secret that native must persist, introduce a dedicated side-effect `AppUpdate` variant rather than embedding that value in `FullState`.

**Internal message types** (not FFI-visible):
- `CoreMsg` -- the envelope wrapping both `AppAction` (from platform) and `InternalEvent` (from async tasks). Uses `Box<InternalEvent>` to avoid bloating the enum.
- `InternalEvent` -- a large enum of async completion events (network results, profile fetches, media uploads, subscription recomputations). These stay internal because they reference protocol-specific types that should never cross the FFI boundary.

### 2.6 Module Organization

The core crate follows a two-level organization: the FFI boundary layer at the top and the actor implementation in a subdirectory.

**Top-level modules (`rust/src/`):**

| File | Responsibility |
|------|----------------|
| `lib.rs` | FFI entry point. `FfiApp` struct, `dispatch()`, `listen_for_updates()`, `state()`, callback interfaces, `uniffi::setup_scaffolding!()` |
| `state.rs` | All FFI-visible state types (`AppState`, `Router`, `Screen`, domain-specific records) |
| `actions.rs` | `AppAction` enum |
| `updates.rs` | `AppUpdate` enum (FFI-visible) + `CoreMsg` and `InternalEvent` (internal) |
| `route_projection.rs` | Pure functions mapping `Router` to platform-specific navigation models |

**Actor subdirectory (`rust/src/core/`):**

| File | Responsibility |
|------|----------------|
| `mod.rs` | `AppCore` struct, `handle_message()`, `handle_action()`, `handle_internal()`, state emission |
| `storage.rs` | Persistence refresh and paging (`refresh_all_from_storage`, message/chat reconstruction) |
| `session.rs` | Session lifecycle (login/restore, relay connections, subscriptions) |
| `config.rs` | App configuration loading/defaults (`pika_config.json`) |
| `profile.rs` | Profile fetch/save/update orchestration |
| `profile_db.rs` | Profile database access layer |
| `profile_pics.rs` | Profile-picture cache helpers |
| `chat_media.rs` | Media upload/download/encryption orchestration |
| `chat_media_db.rs` | Media attachment database access layer |
| `push.rs` | Push subscription sync management |
| `call_control.rs` | Call state machine + call signaling orchestration |
| `call_runtime.rs` | MoQ runtime/media transport integration |
| `interop.rs` | Interop and protocol normalization helpers |

**Guidance:**
- Keep the actor file (`core/mod.rs`) as an orchestrator. When it grows beyond ~1,000 lines, split domain logic into sub-modules that the actor calls. Each sub-module handles its own subset of actions and returns state mutations.
- Domain modules should use `pub(super)` visibility -- they are helpers for the actor, not independent subsystems.
- **Pika drift note:** Pika's `core/mod.rs` is 4,600+ lines. This is recognized as a deviation that should be refactored into domain-specific sub-modules.

### 2.7 Internal Events vs. FFI-Visible Types

The FFI boundary is defined by UniFFI annotations. Everything without these annotations stays internal to the Rust core.

**FFI-visible (crosses to Swift/Kotlin):**
- `#[derive(uniffi::Record)]` -- value types (`AppState`, domain records)
- `#[derive(uniffi::Enum)]` -- enums (`AppAction`, `AppUpdate`, `Screen`)
- `#[derive(uniffi::Object)]` -- reference types (`FfiApp`)
- `#[uniffi::export(callback_interface)]` -- protocols (`AppReconciler`, capability bridges)

**Internal-only (never crosses FFI):**
- `CoreMsg`, `InternalEvent` -- internal message routing
- `AppCore` -- the actor struct and all its fields
- Domain-specific types (pending sends, outbox entries, protocol objects, database records)
- Configuration structs, session handles, connection objects

**Why this separation matters:**
- **API stability.** Internal types can change freely without regenerating bindings or updating platform code.
- **Binary size.** Every FFI-visible type generates scaffolding code in both Swift and Kotlin. Keeping the boundary thin reduces generated code size.
- **Security.** Protocol objects, encryption keys, and raw network data stay in Rust. The platform only sees sanitized, display-ready values.

### 2.8 Async Runtime Integration

The actor thread owns a `tokio::runtime::Runtime` for async I/O. The integration pattern:

```rust
// Inside AppCore
let runtime = tokio::runtime::Builder::new_multi_thread()
    .worker_threads(2)
    .enable_time()
    .enable_io()
    .build()
    .expect("tokio runtime");
```

Async tasks are spawned from the actor and communicate results back via the same `flume` channel:

```rust
// In an action handler:
let tx = self.core_sender.clone();
self.runtime.spawn(async move {
    let result = fetch_something().await;
    let _ = tx.send(CoreMsg::Internal(Box::new(
        InternalEvent::FetchCompleted { result }
    )));
});
```

The actor continues processing other messages while the async task runs. When the result arrives, it is handled synchronously in `handle_internal()`, maintaining the single-writer invariant.

**Key principles:**
- The actor loop uses blocking `flume::Receiver::recv()`, not async. It does not depend on tokio for its own execution.
- Async tasks never directly mutate `AppCore` state. They send `InternalEvent` messages.
- 2 worker threads is sufficient for most mobile apps. Adjust based on concurrent network request patterns.
- Use `tokio::time::timeout` for network operations and `tokio::sync::Semaphore` for rate-limiting (e.g., concurrent image downloads).

### 2.9 Persistence Layer

RMP apps typically use SQLite for local persistence, with optional encryption via SQLCipher.

**The recommended pattern:**
- **Primary database** (per-user, encrypted) -- domain data. For Pika, this is the MDK (MLS) database holding messages, groups, and MLS state. Encryption keys live in the platform keychain (iOS) or Android Keystore.
- **Cache databases** (shared, unencrypted) -- public or non-sensitive cached data. For Pika, profile metadata and media attachment records live in separate unencrypted SQLite databases.
- **JSON files** -- configuration, non-structured state. For Pika, relay config and call timeline events are stored as JSON.

Database connections are held as `Option<Connection>` on the `AppCore` struct for graceful degradation -- if a cache database fails to open, the app continues without it.

The actor provides a `refresh_all_from_storage()` method that rebuilds the full `AppState` from disk. This is called after login, after receiving new messages, and whenever the UI state needs to be consistent with the database.

**Pika example:** Three databases -- `mdk.sqlite3` (encrypted, per-identity), `profiles.sqlite3` (unencrypted, shared), `chat_media.sqlite3` (unencrypted, shared). The MDK database uses `bundled-sqlcipher` (or `bundled-sqlcipher-vendored-openssl` on Android where the NDK lacks OpenSSL).

### 2.10 Error Handling Across the FFI Boundary

RMP apps do not propagate `Result` types across the FFI boundary. Instead, errors are communicated as state changes.

**The pattern:**
- `FfiApp::dispatch()` is infallible -- it enqueues a message and returns. It cannot fail.
- `FfiApp::new()` returns `Arc<Self>` -- it panics only on catastrophic failure (tokio runtime creation). This is acceptable because the app cannot function without the core.
- Operational errors become **toast messages**: `self.state.toast = Some("Login failed: invalid key".into())`
- Long-running operation errors clear **BusyState flags**: the spinner stops, indicating failure.
- Per-item errors use **domain-specific state**: `MessageDeliveryState::Failed { reason }` for individual message failures.

Inside the actor, `anyhow::Result` is used pervasively. Errors are caught at the boundary and converted to user-visible messages using `{e:#}` (anyhow's full error chain formatter).

```rust
// Typical error handling pattern:
self.set_busy(|b| b.logging_in = true);
match self.start_session(keys) {
    Ok(()) => { /* state updated, busy cleared in success path */ },
    Err(e) => {
        self.clear_busy();
        self.toast(format!("Login failed: {e:#}"));
    }
}
```

**Why no FFI-crossing errors?** It simplifies platform code dramatically. Native `dispatch()` calls never need try/catch. All error presentation flows through the same `toast` and `busy` state that the UI already observes.

### 2.11 Route Projection: Platform-Specific Navigation

The core stores navigation as a platform-agnostic `Router`:

```rust
#[derive(uniffi::Record, Clone, Debug)]
pub struct Router {
    pub default_screen: Screen,
    pub screen_stack: Vec<Screen>,
}
```

**Route projection** functions transform this into platform-specific navigation models without duplicating state:

- **`project_mobile(state) -> MobileRouteState`** -- simple stack: root screen + stack produces active screen + `can_pop` flag.
- **`project_desktop(state) -> DesktopRouteState`** -- shell mode (Login/Main) + selected chat + detail pane (peer profile, group info) + modal (active call).

These projections are pure functions with no side effects. They exist as Rust helpers -- mobile platforms currently read `Router` directly from `AppState` and apply their own navigation, while the desktop app uses `project_desktop()`.

The key principle: **Rust owns navigation state.** When the user taps "Open Chat," the native layer dispatches `AppAction::PushScreen { screen: Screen::Chat { chat_id } }`. Rust updates the `Router`, emits state, and the native `NavigationStack` (iOS) or `AnimatedContent` (Android) reacts to the new screen stack. Platform-initiated navigation (swipe-back) dispatches `UpdateScreenStack` back to Rust.

---

## Part III: The FFI Boundary (UniFFI)

### 3.1 UniFFI Overview and Why It Was Chosen

[UniFFI](https://mozilla.github.io/uniffi-rs/) (version 0.31.x) is Mozilla's tool for generating foreign-language bindings from Rust. It produces idiomatic Swift and Kotlin code from annotated Rust types, handling all marshaling, memory management, and thread safety.

**Proc-macro approach.** RMP uses UniFFI's proc-macro mode exclusively -- no `.udl` files. Types are annotated directly in Rust source with `#[derive(uniffi::Record)]`, `#[derive(uniffi::Enum)]`, etc. This keeps the type definitions in one place and catches errors at compile time.

**Why UniFFI over alternatives:**
- **vs. raw FFI / `cbindgen`:** Raw C FFI requires manual memory management, manual type marshaling, and manual error handling on both sides. UniFFI generates all of this.
- **vs. `swift-bridge`:** Only targets Swift. UniFFI generates both Swift and Kotlin from the same annotations.
- **vs. Diplomat:** Similar goals but less mature ecosystem. UniFFI has Mozilla backing and is used in production (Firefox, Application Services).
- **vs. `pyo3`/`napi`:** Language-specific. UniFFI is designed for the mobile multi-target use case.

The binding generator is a standalone binary crate (`uniffi-bindgen/`) that calls `uniffi::uniffi_bindgen_main()`. It reads the compiled `cdylib` to extract type metadata and generates platform code.

### 3.2 Type Mapping: Rust to Swift/Kotlin

UniFFI maps Rust types to idiomatic platform types:

| Rust Annotation | Swift | Kotlin |
|----------------|-------|--------|
| `#[derive(uniffi::Record)]` | `struct Foo: Equatable, Hashable` | `data class Foo` |
| `#[derive(uniffi::Enum)]` | `enum Foo` (with associated values) | `sealed class Foo` (with subclasses) |
| `#[derive(uniffi::Object)]` | `class Foo` (handle-based, reference type) | `class Foo: Disposable, AutoCloseable` |
| `#[uniffi::export(callback_interface)]` | `protocol Foo: AnyObject, Sendable` | `interface Foo` |

**Primitive mapping:**

| Rust | Swift | Kotlin |
|------|-------|--------|
| `String` | `String` | `String` |
| `u64` | `UInt64` | `ULong` |
| `bool` | `Bool` | `Boolean` |
| `Vec<u8>` | `Data` | `ByteArray` |
| `Vec<T>` | `[T]` | `List<T>` |
| `Option<T>` | `T?` | `T?` |
| `HashMap<K,V>` | `[K:V]` | `Map<K,V>` |

**Serialization:** Records are serialized into `RustBuffer` (big-endian byte buffer) for crossing the FFI. Objects use opaque `UInt64` handles. This means every `dispatch()` call serializes the `AppAction`, and every `reconcile()` call serializes the `AppUpdate`. For `FullState`, the entire `AppState` tree is serialized.

**Auto-generated conformances (critical for platform code):**
- **Swift:** UniFFI Records and Enums automatically conform to `Equatable` and `Hashable`. Do **not** add manual conformances (e.g., `extension MyEnum: @retroactive Hashable`) -- the compiler will error on the duplicate. This means UniFFI enums can be used directly as `NavigationStack` path elements, dictionary keys, and `ForEach` identifiers without any additional Swift code.
- **Kotlin:** Records become `data class` (automatic `equals`/`hashCode`). Enums become `sealed class` with subclass-based matching.

**Gotchas:**
- UniFFI enums cannot use `Box<T>` indirection for large variants. If one variant is much larger than others, the entire enum is sized to the largest variant.
- Recursive types are not supported. `AppState` and its nested types form a tree, not a graph.
- Custom error types require `#[derive(uniffi::Error)]` and special handling. RMP avoids this by not returning errors across FFI (see Section 2.10).

**Pika scale:** The generated Swift bindings are ~4,500 lines; Kotlin bindings are ~6,000 lines. This covers 23 Records, 8 Enums, 1 Object, and 3 callback interfaces.

### 3.3 The FfiApp Object Pattern

`FfiApp` is the single entry point for all platform interaction with the Rust core. It is a `uniffi::Object` (reference type) created once at app startup.

```rust
#[derive(uniffi::Object)]
pub struct FfiApp {
    core_tx: Sender<CoreMsg>,
    update_rx: Receiver<AppUpdate>,
    listening: AtomicBool,
    shared_state: Arc<RwLock<AppState>>,
    // ... capability bridge handles
}
```

**Constructor:** `FfiApp::new(data_dir: String) -> Arc<Self>`. The constructor creates channels, spawns the actor thread, and returns an `Arc` (UniFFI requirement for Object types). This is the complete wiring -- channels, actor spawn, shared state:

```rust
#[uniffi::export]
impl FfiApp {
    #[uniffi::constructor]
    pub fn new(data_dir: String) -> Arc<Self> {
        let (update_tx, update_rx) = flume::unbounded();
        let (core_tx, core_rx) = flume::unbounded::<CoreMsg>();
        let shared_state = Arc::new(RwLock::new(AppState::empty()));

        // Spawn the actor thread -- the single owner of all mutable state.
        let shared_for_core = shared_state.clone();
        thread::spawn(move || {
            let mut state = AppState::empty();
            let mut rev: u64 = 0;

            // Emit initial state so platforms have something to display immediately.
            let snapshot = state.clone();
            if let Ok(mut g) = shared_for_core.write() { *g = snapshot.clone(); }
            let _ = update_tx.send(AppUpdate::FullState(snapshot));

            // Actor loop: process messages sequentially, forever.
            while let Ok(msg) = core_rx.recv() {
                match msg {
                    CoreMsg::Action(action) => {
                        // Handle action, mutate state...
                        rev += 1;
                        state.rev = rev;
                        // ... domain-specific logic here ...

                        // Write to shared state and emit update.
                        let snapshot = state.clone();
                        if let Ok(mut g) = shared_for_core.write() {
                            *g = snapshot.clone();
                        }
                        let _ = update_tx.send(AppUpdate::FullState(snapshot));
                    }
                }
            }
        });

        Arc::new(Self {
            core_tx,
            update_rx,
            listening: AtomicBool::new(false),
            shared_state,
        })
    }
}
```

The pattern has three moving parts: (1) `core_tx`/`core_rx` for inbound actions, (2) `update_tx`/`update_rx` for outbound state updates, (3) `shared_state` for synchronous reads via `state()`. The actor thread writes to both `shared_state` and `update_tx` on every mutation. Production apps add a `keychain_group` parameter, logging initialization, and `tokio::runtime` for async I/O (see Section 2.8), but the core wiring is identical.

**Exported methods:**

| Method | Purpose | Thread Safety |
|--------|---------|---------------|
| `new(data_dir, keychain_group)` | Constructor; spawns actor thread | Called once at startup |
| `state() -> AppState` | Synchronous snapshot from `RwLock` | Read lock; safe from any thread |
| `dispatch(action: AppAction)` | Non-blocking action send | `flume::send` is lock-free; never blocks |
| `listen_for_updates(reconciler)` | Start listener thread for state updates | `AtomicBool` CAS ensures single listener |
| `set_video_frame_receiver(receiver)` | Set platform video callback | Write lock on `Arc<RwLock<Option<...>>>` |
| `send_video_frame(payload)` | Send camera frame to core | Via channel to actor |
| `set_external_signer_bridge(bridge)` | Set external signer callback | Write lock |

**Why a single object?** Keeps the API surface minimal. The platform creates one `FfiApp`, stores it in the `AppManager`, and uses it for everything. Multiple service objects would require coordination, lifecycle management, and shared state -- all of which are handled internally by the actor.

**Poison lock handling:** All `RwLock` accesses use `poison.into_inner()` fallback rather than panicking. If the actor thread panics, the platform can still read the last known state.

### 3.4 Callback Interfaces: Native -> Rust Communication

Callback interfaces let Rust call into platform code. They are defined as Rust traits with `#[uniffi::export(callback_interface)]`, which generates a Swift protocol or Kotlin interface.

**The essential callback -- `AppReconciler`:**
```rust
#[uniffi::export(callback_interface)]
pub trait AppReconciler: Send + Sync + 'static {
    fn reconcile(&self, update: AppUpdate);
}
```

The platform's `AppManager` implements this trait. The Rust side spawns a dedicated listener thread that drains the update channel and forwards each update to the reconciler:

```rust
pub fn listen_for_updates(&self, reconciler: Box<dyn AppReconciler>) {
    // CAS guard: only one listener thread allowed (a second would split messages).
    if self
        .listening
        .compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst)
        .is_err()
    {
        return;
    }

    let rx = self.update_rx.clone();
    thread::spawn(move || {
        while let Ok(update) = rx.recv() {
            reconciler.reconcile(update);
        }
    });
}
```

This is the bridge between the actor's `update_tx.send()` and the platform's `reconcile()` callback. The `AtomicBool` CAS ensures only one thread drains the channel -- calling `listen_for_updates` a second time is a no-op, preventing messages from being split across multiple consumers.

**Thread safety is critical.** Callback interfaces must be `Send + Sync + 'static`. Rust calls them from background threads. The platform must handle the thread hop:
- **iOS:** `nonisolated func reconcile(update:)` dispatches to `@MainActor` via `Task { @MainActor in self?.apply(update:) }`.
- **Android:** `reconcile(update)` posts to `Handler(Looper.getMainLooper())`.

**Capability bridge callbacks** follow the same pattern. Pika defines:
- `VideoFrameReceiver` -- Rust pushes decoded video frames at ~30fps.
- `ExternalSignerBridge` -- Rust asks native to perform cryptographic operations via an external signer app (7 methods for NIP-46/Amber signing, encryption, decryption).

Under the hood, UniFFI wraps callback interfaces as boxed trait objects with reference counting. On Swift, they become protocol objects. On Kotlin, they become JNA callback instances. The marshaling overhead is minimal for infrequent calls but should be considered for high-frequency callbacks (like video frames).

### 3.5 Binding Generation Pipeline

The full pipeline from Rust source to generated bindings:

```
Rust source (proc-macro annotations)
    → cargo build --target <host> (produces libmy_core.{dylib|so})
    → uniffi-bindgen generate --library <lib> --language {swift|kotlin}
    → generated bindings (Swift .swift + .h + .modulemap, or Kotlin .kt)
```

**Configuration (`uniffi.toml`):**
```toml
[bindings.kotlin]
package_name = "com.example.myapp.rust"
cdylib_name = "my_app_core"
```

**Swift generation:** Produces three files -- `my_core.swift` (all types and wrappers), `my_coreFFI.h` (C header), `my_coreFFI.modulemap` (module map for Swift imports).

**Kotlin generation:** Produces a single file -- `my_core.kt` (all types, JNA bindings, and wrappers).

**Generated bindings should be checked into git.** This avoids requiring a host build step on every developer's machine and makes CI simpler. Regenerate when Rust FFI types change.

**Pika example:** The justfile provides `ios-gen-swift` (generates Swift + NSE bindings, strips trailing whitespace via Python) and `gen-kotlin` (generates Kotlin with `--no-format`). Both depend on `rust-build-host` to produce the host `cdylib` first.

### 3.6 Binary Size Considerations

Every FFI-visible type generates serialization and deserialization code in both the Rust scaffolding and the platform bindings. Minimizing the FFI surface directly reduces binary size.

**Current state (Pika, no optimizations applied):**
- No `[profile.release]` overrides (Rust defaults: `opt-level=3`, no LTO, 16 codegen units, no stripping)
- Heavy dependencies: `nostr-sdk`, `tokio`, `rusqlite` (bundled SQLCipher), `rustls`, `reqwest`

**Recommended optimizations for release builds:**
```toml
[profile.release]
lto = true          # Full cross-crate LTO (10-30% size reduction)
codegen-units = 1   # Better optimization at cost of compile time
strip = true        # Remove debug symbols
panic = "abort"     # Smaller binary, no unwind tables
```

**General guidance:**
- Only expose what the UI needs over FFI. Internal types add zero to binary size.
- `lto = true` is the single biggest win for release binary size.
- Android ships separate `.so` files per ABI (arm64-v8a, armeabi-v7a, x86_64). Consider shipping only arm64 for production if your user base supports it.
- iOS static libraries are linked into the app binary. Size shows up in the IPA.

---

## Part IV: Platform Layers

### 4.1 iOS (SwiftUI)

#### 4.1.1 Project Structure

The canonical iOS project layout for an RMP app:

```
ios/
├── project.yml                # XcodeGen project definition (generates .xcodeproj)
├── Info.plist                 # App configuration
├── Sources/                   # Swift source files
│   ├── AppManager.swift       # Bridge: owns FfiApp, implements AppReconciler
│   ├── App.swift              # @main SwiftUI entry point
│   ├── ContentView.swift      # Root view with NavigationStack
│   ├── ViewState.swift        # Thin derived view-state structs
│   └── Views/                 # Screen-level SwiftUI views
├── Bindings/                  # UniFFI-generated Swift (checked into git)
│   ├── my_core.swift
│   ├── my_coreFFI.h
│   └── my_coreFFI.modulemap
├── Frameworks/                # Pre-built xcframeworks
│   └── MyCore.xcframework
└── Tests/ + UITests/
```

XcodeGen generates the `.xcodeproj` from `project.yml`, keeping the project file out of version control and avoiding merge conflicts. The xcframework is linked as a non-embedded framework dependency (static linking).

**Pika additions beyond the basics:** Notification Service Extension (`NotificationService/`), a second xcframework for the NSE crate (`PikaNSE.xcframework`), NSE-specific bindings (`NSEBindings/`), entitlements for App Groups and Keychain sharing, and call-related views (video pipeline, camera capture, decoder/renderer).

#### 4.1.2 AppManager: The Bridge Class

`AppManager` is the single class that bridges Rust and SwiftUI. It owns the `FfiApp` instance, holds the current `AppState`, implements the `AppReconciler` callback, and provides `dispatch()` to forward actions.

```swift
@MainActor
@Observable
final class AppManager: AppReconciler {
    let core: AppCore          // Protocol wrapping FfiApp
    var state: AppState        // Updated via reconciler
    private var lastRevApplied: UInt64

    init() {
        let dataDir = /* resolve app data directory */
        let core = FfiApp(dataDir: dataDir)
        self.core = core
        let initial = core.state()
        self.state = initial
        self.lastRevApplied = initial.rev
        core.listenForUpdates(reconciler: self)
    }

    func dispatch(_ action: AppAction) {
        core.dispatch(action: action)
    }
}
```

**Protocol abstraction for testability:** Define an `AppCore` protocol that `FfiApp` conforms to via an extension. This enables test doubles and SwiftUI previews without a running Rust core.

```swift
protocol AppCore: AnyObject, Sendable {
    func dispatch(action: AppAction)
    func listenForUpdates(reconciler: AppReconciler)
    func state() -> AppState
}
extension FfiApp: AppCore {}
```

#### 4.1.2.1 Production boot flow (critical)

The minimal constructor above is intentionally simplified. A production iOS app should add a deterministic boot sequence:

1. Resolve the data directory (prefer App Group container when available).
2. Run one-time migration from app-private storage to App Group storage (for NSE sharing).
3. Ensure a default config file exists (`pika_config.json`) and fill missing keys without clobbering user overrides.
4. Create `FfiApp`.
5. Load stored auth mode from secure storage and dispatch the correct restore action:
   - `.restoreSession(nsec:)`
   - `.restoreSessionBunker(bunkerUri:clientNsec:)`
6. Expose an `isRestoringSession` UI flag and show a loading surface until Rust settles on logged-in or logged-out state.

This startup contract is architecture-level guidance, not product-specific behavior.

#### 4.1.3 State Observation with @Observable

RMP uses Swift 5.9's `@Observable` macro (Observation framework), not the older `ObservableObject`/`@Published` pattern.

- `@Observable` on `AppManager` makes every stored property automatically observable by SwiftUI.
- When `state` is replaced in `apply(update:)`, any view that accessed `state` (or its sub-properties) automatically re-renders.
- No `@Published` wrappers needed. No `objectWillChange` publisher.

The reconciler callback is `nonisolated` (called from Rust's background thread) and hops to `@MainActor`:

```swift
nonisolated func reconcile(update: AppUpdate) {
    Task { @MainActor [weak self] in
        self?.apply(update: update)
    }
}

private func apply(update: AppUpdate) {
    switch update {
    case .fullState(let s):
        if s.rev <= lastRevApplied { return }  // stale guard
        lastRevApplied = s.rev
        state = s
    case .accountCreated(let rev, let nsec, _, _):
        authStore.saveNsec(nsec)  // side effect: persist credential
        if rev <= lastRevApplied { return }
        lastRevApplied = rev
    }
}
```

In the app entry point, `AppManager` is stored with `@State` to ensure it lives for the lifetime of the scene:

```swift
@main struct MyApp: App {
    @State private var manager = AppManager()
    var body: some Scene {
        WindowGroup { ContentView(manager: manager) }
    }
}
```

When passing the manager into child views, use `@Bindable var manager: AppManager` in the receiving view so Observation bindings work correctly with `@Observable`.

#### 4.1.4 Navigation: Rust-Driven NavigationStack

SwiftUI's `NavigationStack` is driven by Rust's `Router.screenStack`:

```swift
@State private var navPath: [Screen] = []

NavigationStack(path: $navPath) {
    rootView()
        .navigationDestination(for: Screen.self) { screen in
            screenView(for: screen)
        }
}
.onAppear { navPath = manager.state.router.screenStack }
.onChange(of: manager.state.router.screenStack) { _, new in
    navPath = new  // Rust pushed/popped a screen
}
.onChange(of: navPath) { old, new in
    guard new != manager.state.router.screenStack else { return }
    if new.count < old.count {
        // User swiped back -- report to Rust
        manager.dispatch(.updateScreenStack(stack: new))
    }
}
```

The sync is bidirectional: Rust drives the stack via state updates, and platform-initiated pops (swipe-back) are reported back to Rust. The `guard` prevents feedback loops.

#### 4.1.5 ViewState Derivation

Thin Swift structs slice `AppState` into screen-specific shapes via pure derivation functions:

```swift
struct ChatListViewState: Equatable {
    let chats: [ChatSummary]
    let myNpub: String?
    let myProfile: MyProfileState
}

func chatListState(from state: AppState) -> ChatListViewState {
    ChatListViewState(
        chats: state.chatList,
        myNpub: /* extract from auth */,
        myProfile: state.myProfile
    )
}
```

Views receive their ViewState, not raw `AppState`. This keeps views focused and makes them `Equatable`-diffable. **ViewState derivation must be trivially mechanical** -- if it contains business logic (filtering, sorting, conditional computation), that logic belongs in Rust.

#### 4.1.6 Platform Capabilities (Push, Audio, Camera, Keychain)

Each iOS capability is a thin native shim that reports to Rust or executes Rust commands:

- **Push notifications:** `PushNotificationManager` calls `UIApplication.shared.registerForRemoteNotifications()`. The resulting device token is dispatched to Rust via `AppAction.setPushToken`. All subscription management happens in Rust.
- **Audio session:** `CallAudioSessionCoordinator` configures `AVAudioSession` based on `CallState` (voice chat mode for voice calls, video chat mode with speaker for video calls). It reads Rust state and configures the OS; it never decides whether to start or stop a call.
- **Camera capture:** `VideoCaptureManager` captures frames via `AVCaptureSession` and sends them to Rust via `core.sendVideoFrame(payload:)`. Frame rate and resolution decisions are made in Rust.
- **Keychain:** `KeychainNsecStore` stores credentials in the iOS Keychain with `kSecAttrAccessibleAfterFirstUnlockThisDeviceOnly` protection. Shared access group enables the NSE to read credentials for push decryption.
- **External signer:** `IOSExternalSignerBridge` opens URLs for NostrConnect flows. On iOS, only `openUrl` is functional; the full NIP-55 signing interface (used on Android for Amber) returns `.signerUnavailable`.

#### 4.1.7 Notification Service Extension (NSE)

For apps that need to process push notifications before display (decrypting content, fetching additional data), a separate Rust crate provides the heavy lifting within the NSE's constrained environment.

**Why a separate crate:** NSE extensions have strict memory limits (~24MB) and short execution windows (~30 seconds). A stripped-down Rust crate with only the necessary dependencies (crypto, storage) keeps the footprint manageable.

**Pika implementation:** `crates/pika-nse/` provides `decrypt_push_notification()` which opens the MLS database, processes the Nostr event, and returns structured content (message text, sender info, call invite). The NSE reads credentials from the shared Keychain and data from the shared App Group container. This produces a separate `PikaNSE.xcframework` with its own UniFFI bindings.

#### 4.1.8 XcodeGen and Project Configuration

`project.yml` defines the Xcode project declaratively:

```yaml
name: App
options:
  bundleIdPrefix: com.example.myapp
  deploymentTarget:
    iOS: "17.0"
targets:
  App:
    type: application
    platform: iOS
    sources:
      - path: Sources
      - path: Bindings
        excludes: ["*.h", "*.modulemap"]
    dependencies:
      - framework: Frameworks/MyCore.xcframework
        embed: false
```

Benefits of XcodeGen over manual `.xcodeproj`: no merge conflicts, declarative and reviewable, easy to regenerate, consistent across team members. The `.xcodeproj` stays out of git.

### 4.2 Android (Jetpack Compose)

#### 4.2.1 Project Structure

The canonical Android project layout for an RMP app:

```
android/
├── build.gradle.kts               # Root Gradle build
├── settings.gradle.kts            # Plugin management
├── gradle.properties              # JVM/Android flags
└── app/
    ├── build.gradle.kts           # App module: dependencies, compose, NDK
    └── src/main/
        ├── AndroidManifest.xml
        ├── java/<package>/
        │   ├── MainActivity.kt       # ComponentActivity entry point
        │   ├── AppManager.kt         # Bridge: owns FfiApp, implements AppReconciler
        │   ├── rust/
        │   │   └── my_core.kt        # UniFFI-generated Kotlin (checked into git)
        │   └── ui/
        │       ├── MainApp.kt        # Root @Composable
        │       ├── theme/Theme.kt    # Material3 theme
        │       └── screens/          # Screen composables
        └── jniLibs/
            ├── arm64-v8a/libmy_core.so
            ├── armeabi-v7a/libmy_core.so
            └── x86_64/libmy_core.so
```

#### 4.2.2 AppManager: The Bridge Class

Android's `AppManager` mirrors iOS's pattern but uses Compose's `mutableStateOf` for reactivity:

```kotlin
class AppManager private constructor(context: Context) : AppReconciler {
    private val rust: FfiApp
    private val mainHandler = Handler(Looper.getMainLooper())
    private var lastRevApplied: ULong = 0UL

    var state: AppState by mutableStateOf(/* initial empty state */)
        private set

    init {
        rust = FfiApp(dataDir = context.filesDir.absolutePath)
        val initial = rust.state()
        state = initial
        lastRevApplied = initial.rev
        rust.listenForUpdates(this)
    }

    fun dispatch(action: AppAction) = rust.dispatch(action)

    override fun reconcile(update: AppUpdate) {
        mainHandler.post {
            when (update) {
                is AppUpdate.FullState -> {
                    if (update.v1.rev <= lastRevApplied) return@post
                    lastRevApplied = update.v1.rev
                    state = update.v1
                }
                // handle side-effect variants
            }
        }
    }

    companion object {
        @Volatile private var instance: AppManager? = null
        fun getInstance(context: Context): AppManager =
            instance ?: synchronized(this) {
                instance ?: AppManager(context.applicationContext).also { instance = it }
            }
    }
}
```

The singleton pattern ensures `AppManager` survives Activity configuration changes. `mutableStateOf` makes `state` observable to the Compose runtime -- any composable reading `manager.state` recomposes automatically when state is replaced.

#### 4.2.2.1 Production boot flow (critical)

As on iOS, production Android startup should do more than instantiate `FfiApp`:

1. Ensure default config exists (`pika_config.json`) before Rust bootstraps.
2. Create `FfiApp` and start update listener.
3. Restore session from secure storage with mode-specific actions:
   - `RestoreSession(nsec)`
   - `RestoreSessionExternalSigner(pubkey, signerPackage, currentUser)`
   - `RestoreSessionBunker(bunkerUri, clientNsec)`
4. Keep side-effect update handling (`AccountCreated`, `BunkerSessionDescriptor`) before stale-rev checks.

This ensures startup behavior is deterministic and auth mode handling does not drift by platform.

#### 4.2.3 State Observation with Compose mutableStateOf

The Compose integration works identically to iOS's `@Observable`:

1. Rust calls `reconcile()` on a background thread.
2. `reconcile()` posts to the main thread via `Handler(Looper.getMainLooper())`.
3. On the main thread, `state = update.v1` triggers Compose's snapshot system.
4. Any `@Composable` that read `manager.state` (or any sub-property) recomposes.

The `rev`-based stale guard (`if (update.v1.rev <= lastRevApplied) return@post`) prevents out-of-order updates from overwriting newer state.

#### 4.2.4 Navigation: Rust-Driven Compose Navigation

RMP apps on Android use `AnimatedContent` driven by Rust's `Router`, not Jetpack Navigation:

```kotlin
@Composable
fun MainApp(manager: AppManager) {
    val router = manager.state.router
    val current = router.screenStack.lastOrNull() ?: router.defaultScreen

    BackHandler(enabled = router.screenStack.isNotEmpty()) {
        manager.dispatch(AppAction.UpdateScreenStack(
            stack = router.screenStack.dropLast(1)
        ))
    }

    AnimatedContent(targetState = current) { screen ->
        when (screen) {
            is Screen.ChatList -> ChatListScreen(manager)
            is Screen.Chat -> ChatScreen(manager, screen.chatId)
            // ...
        }
    }
}
```

Back presses dispatch `UpdateScreenStack` to Rust (popping the last screen). Navigation is fully Rust-driven -- the native layer never decides which screen to show.

#### 4.2.5 JNA and Library Loading

UniFFI Kotlin bindings use JNA (Java Native Access) to call into Rust, not raw JNI. The `.so` files in `jniLibs/` are automatically loaded by JNA at runtime.

For Android-specific Rust features that need the NDK context (e.g., Android Keystore-backed keyring), a raw JNI function is needed:

```kotlin
object Keyring {
    init { System.loadLibrary("my_core") }
    @JvmStatic external fun init(context: Context)
}
```

This must be called in `MainActivity.onCreate()` before creating `AppManager`, because the Rust core needs NDK context for encrypted database initialization.

#### 4.2.6 Platform Capabilities (Keyring, Audio, Signer, Secure Storage)

- **Secure storage:** `SecureAuthStore` wraps `EncryptedSharedPreferences` (AndroidX Security Crypto) with `AES256_GCM` encryption. Stores credentials by auth mode (local nsec, external signer, bunker).
- **Audio focus:** `AndroidAudioFocusManager` acquires `AUDIOFOCUS_GAIN_TRANSIENT_EXCLUSIVE` during calls and releases on hang-up. Simpler than iOS (no voice/video mode distinction).
- **External signer (Amber):** `AmberIntentBridge` uses `ActivityResultLauncher` for intent-based IPC with the Amber signer app. `AmberSignerClient` implements the full NIP-55 protocol. This is Android-specific -- iOS uses NostrConnect URLs instead.

#### 4.2.7 Gradle Configuration and Dependencies

Key Gradle setup for an RMP Android app:

```kotlin
// app/build.gradle.kts
android {
    compileSdk = 35
    defaultConfig {
        minSdk = 26
        targetSdk = 35
        ndkVersion = "28.2.13676358"
    }
    buildFeatures { compose = true }
    composeOptions { kotlinCompilerExtensionVersion = "1.5.14" }
    sourceSets {
        getByName("main") { jniLibs.srcDirs("src/main/jniLibs") }
    }
}

dependencies {
    implementation("net.java.dev.jna:jna:5.18.1@aar")  // Required for UniFFI
    implementation(platform("androidx.compose:compose-bom:2024.06.00"))
    implementation("androidx.compose.material3:material3")
    // ... other Compose and AndroidX deps
}

// Pre-build check: fail early if bindings don't exist
tasks.register("ensureUniffiGenerated") {
    doLast {
        if (!file("src/main/java/<package>/rust/my_core.kt").exists()) {
            throw GradleException("Missing UniFFI bindings. Run `rmp bindings kotlin`.")
        }
    }
}
tasks.named("preBuild") { dependsOn("ensureUniffiGenerated") }
```

### 4.3 Desktop (iced)

#### 4.3.1 Direct Rust Dependency (No FFI)

The desktop app imports the core crate as a regular Rust path dependency:

```toml
[dependencies]
pika_core = { path = "../../rust" }
```

No UniFFI, no code generation, no serialization overhead. The desktop app calls `FfiApp::new()`, `FfiApp::state()`, and `FfiApp::dispatch()` directly via Rust method calls on `Arc`-wrapped types. This is the fastest integration path.

**Implication for API design:** Because the core crate must serve both UniFFI consumers (iOS, Android) and direct Rust consumers (desktop, CLI), all FFI-visible types must also be usable as regular Rust types. The `uniffi::Record` and `uniffi::Enum` derives are additive -- they don't prevent normal Rust usage.

#### 4.3.2 iced Elm Architecture and the Two Nested Loops

The desktop app uses [iced](https://iced.rs/) (v0.14), a Rust-native GUI framework that itself follows TEA. This creates an architecture of **two nested Elm loops**.

The **outer loop** is iced's: `DesktopApp` implements `new()`, `update()`, `view()`, `subscription()`. The **inner loop** is the Rust core's: `AppCore` processes `CoreMsg` messages on its actor thread and emits `AppUpdate` notifications.

```
┌─────────────────────────────────────────────────────────┐
│  iced runtime (outer loop)                              │
│                                                         │
│   DesktopApp::view()                                    │
│        │                                                │
│        ▼                                                │
│   Element<Message>  ──user interaction──▶  Message       │
│        ▲                                    │           │
│        │                                    ▼           │
│   DesktopApp::update()                                  │
│        │                                                │
│        ├── UI-only state mutations (screen transitions) │
│        └── manager.dispatch(AppAction::...)             │
│                       │                                 │
│  ┌────────────────────▼────────────────────────────┐    │
│  │  AppCore actor (inner loop)                     │    │
│  │  CoreMsg → handle_message → mutate AppState     │    │
│  │  → AppUpdate notification via flume channel     │    │
│  └────────────────────┬────────────────────────────┘    │
│                       │                                 │
│   Subscription::run_with() polls flume::Receiver        │
│        │                                                │
│        ▼                                                │
│   Message::CoreUpdated → sync_from_manager()            │
│   reads latest AppState, triggers re-render             │
└─────────────────────────────────────────────────────────┘
```

The `AppManager` wraps `FfiApp` and exposes `subscribe_updates() -> flume::Receiver<()>`. iced polls this via `Subscription::run_with()`. When `Message::CoreUpdated` arrives, `sync_from_manager()` reads the latest `AppState` snapshot and stores it, triggering a re-render.

**iced 0.14 API notes (critical for implementation):**

- **`Subscription::run_with()` requires `Hash`.** The data parameter (`AppManager`) must implement `Hash`. Derive it manually by hashing the `Arc` pointer:
  ```rust
  impl Hash for AppManager {
      fn hash<H: Hasher>(&self, state: &mut H) {
          Arc::as_ptr(&self.ffi).hash(state);
      }
  }
  ```
- **`Space` API.** There is no `horizontal_space()` free function or `Space::with_width()`. Use `iced::widget::Space::new().width(Fill)` for horizontal expansion and `Space::new().height(Fill)` for vertical.
- **`Rule` API.** There is no `Rule::vertical()` constructor. For vertical dividers, use a styled container: `container(column![]).width(1).height(Fill).style(...)`.
- **Theme closures.** The `.theme()` method on `iced::application` requires a method reference (e.g., `App::theme`) rather than a closure like `|_| Theme::Dark`, due to lifetime requirements on the `Fn` bound.

#### 4.3.2.1 Desktop AppManager internals (production pattern)

In production desktop apps, `AppManager` is typically more than a thin proxy. Pika uses an explicit inner model + subscriber fan-out pattern:

```rust
struct Inner {
    core: Arc<FfiApp>,
    model: RwLock<ManagerModel>,
    subscribers: Mutex<Vec<Sender<()>>>,
    // data_dir, credential store, ...
}

struct ManagerModel {
    state: AppState,
    last_rev_applied: u64,
    is_restoring_session: bool,
    pending_login_nsec: Option<String>,
}
```

Key behavior:
- A reconciler callback receives `AppUpdate` values and forwards them into a channel.
- A background thread applies updates into `ManagerModel` with `rev` stale guards.
- Secret-bearing side effects (`AccountCreated`) are persisted **before** stale checks.
- On change, `notify_subscribers()` emits `()` to all active update subscribers.
- Desktop credential persistence uses file storage with strict permissions (`0600` on Unix).

**Boot state as a top-level enum.** Instead of wrapping the manager in `Option<AppManager>` and scattering `if let Some(manager)` checks everywhere, model boot failure as a variant of the top-level app:

```rust
enum DesktopApp {
    BootError { error: String },
    Loaded {
        manager: AppManager,
        screen: Screen,
        state: AppState,
        // ...
    },
}
```

Every method (`update`, `view`, `subscription`) matches on the variant first. `BootError` renders an error message and ignores all input. This eliminates an entire category of `Option` unwrapping and makes the impossible state (running without a manager) unrepresentable.

#### 4.3.3 The State / Message / Event Module Pattern

This is the canonical pattern for structuring **stateful** iced view and screen modules. It was designed by an iced contributor and is documented in `crates/pika-desktop/iced-view-pattern.md`. In practice, this is the primary pattern, with lightweight variants for simpler modules.

```rust
// Stateful modules define these three types.
// No prefixing needed -- use scoping rules at call sites (e.g. chat_rail::Message).

pub struct State {
    // All state owned by this module
}

#[derive(Debug, Clone)]
pub enum Message {
    // Triggered by view() interactions (button presses, text input, etc.)
}

pub enum Event {
    // Raised to the parent -- "something happened that I can't handle myself"
}

impl State {
    pub fn new(/* minimal init data */) -> Self { /* ... */ }

    pub fn update(&mut self, message: Message) -> Option<Event> {
        match message {
            // Handle UI state internally, return None
            // When something needs parent action, return Some(Event::...)
        }
    }

    pub fn view(&self, /* read-only context */) -> Element<Message> {
        // Pure rendering, immutable &self
    }
}
```

When a module needs to kick off an async UI operation (for example, an iced file picker), return an optional `Task` alongside the `Event`:

```rust
pub fn update(&mut self, message: Message) -> (Option<Event>, Option<Task<Message>>)
```

Valid module variants in a real iced app:
- **Full triad (`State` + `Message` + `Event`)**: `conversation`, `my_profile`, `new_chat`, `new_group_chat`, `group_info`
- **`Message` + `Event` (no local state)**: `peer_profile`
- **Message-only stateless view modules**: `chat_rail`, `toast`, `call_banner`, `call_screen`
- **Pure render helpers**: `avatar`, `message_bubble`, `empty_state`

**Key principles:**

1. **Messages go down, Events (and optional Tasks) bubble up.** The parent calls `child.update(msg)` and inspects the returned `Event`/`Task` to decide what to do. The child never reaches up to mutate parent state or call `manager.dispatch()` directly.

2. **Scoped naming.** Every module uses the names `State`, `Message`, `Event` without prefixes. At call sites, Rust's module system provides the namespace: `conversation::Message`, `chat_rail::Message`, `login::Event`. This eliminates naming stutters like `ConversationMessage` or `ChatRailState`.

3. **`view()` takes `&self`, not `&mut self`.** Views are pure functions of state. Side effects only happen through Messages.

#### 4.3.4 Event Bubbling: The Logout Flow

The event bubbling pattern is most clearly illustrated by the logout flow, which spans three levels of the module hierarchy:

```
my_profile::view()
  └── button("Logout").on_press(Message::Logout)
        │
        ▼
my_profile::update(Message::Logout)
  └── returns Some(Event::Logout)
        │
        ▼
home::update(Message::MyProfile(msg))
  └── matches Event::Logout → returns Some(home::Event::Logout)
        │
        ▼
DesktopApp::update(Message::Home(msg))
  └── matches home::Event::Logout
        ├── manager.logout()
        ├── avatar_cache.borrow_mut().clear()
        └── *screen = Screen::Login(screen::login::State::new())
```

At no point does `my_profile` know about `AppManager` or screen transitions. It only knows that logout was requested and communicates this upward. The top level -- which owns the manager and the screen enum -- is the only place that performs the destructive session teardown and screen swap.

This pattern replaces the alternative of passing `&mut AppManager` down through every module, which creates tight coupling and makes modules impossible to test in isolation.

#### 4.3.5 Screen Modules vs. View Modules

The module hierarchy has two tiers:

```
src/
├── main.rs              # DesktopApp: top-level iced application
├── screen/
│   ├── home.rs          # Authenticated experience (composes view modules)
│   └── login.rs         # Unauthenticated login form
└── views/
    ├── chat_rail.rs     # Left sidebar with chat list
    ├── conversation.rs  # Message thread + input
    ├── call_screen.rs   # Voice/video call UI
    ├── my_profile.rs    # Profile editor overlay
    ├── peer_profile.rs  # Other user's profile
    ├── new_chat.rs      # Start DM form
    ├── new_group_chat.rs# Group creation form
    ├── group_info.rs    # Group settings
    ├── toast.rs         # Notification bar
    └── ...
```

**Screen modules** (`screen/home.rs`, `screen/login.rs`) are top-level composition units. They own the full-screen layout, compose multiple view modules together, and are the primary routing targets. The top-level `Screen` enum switches between them:

```rust
enum Screen {
    Home(Box<screen::home::State>),   // Boxed because it's large
    Login(screen::login::State),
}
```

Screen transitions happen at the `DesktopApp` level in response to auth state changes in `sync_from_manager()` or events like `Event::Logout`.

**View modules** (`views/conversation.rs`, `views/chat_rail.rs`, etc.) are reusable components owned by a screen. The home screen composes them:

```rust
// home.rs owns child view states
pub struct State {
    pane: Pane,                              // Active overlay/panel
    conversation: views::conversation::State,
    group_info: Option<views::group_info::State>,
    // ...
}

// home.rs delegates messages to the right child
pub fn update(&mut self, message: Message, ...) -> Option<Event> {
    match message {
        Message::Conversation(msg) => {
            let (event, task) = self.conversation.update(msg);
            // Handle conversation events (SendMessage, ReactToMessage, etc.)
        }
        Message::ChatRail(msg) => {
            // Handle chat selection, overlay toggles
        }
        // ...
    }
}
```

The home screen's `view()` method maps each child's output into its own message namespace:

```rust
// Each child view's Message type is wrapped via .map()
let rail = views::chat_rail::view(&state.chat_list, ...)
    .map(Message::ChatRail);

let center = self.conversation.view(chat, ...)
    .map(Message::Conversation);
```

This `.map()` wrapping is how iced maintains type-safe message routing through the component tree.

**When to use a screen vs. a view:**

- If it replaces the entire window content and corresponds to an auth state or major navigation boundary, it is a **screen**.
- If it fills a panel, overlay, or section within a screen and can be composed alongside other views, it is a **view**.
- Views can nest (a conversation view contains message bubble views), but screens do not nest.

#### 4.3.6 The Home Screen as Composition Root

The home screen (`screen/home.rs`) deserves special attention because it demonstrates the full pattern at scale. It owns:

- A **Pane enum** for mutually exclusive overlays (new chat form, new group form, profile editor, or empty)
- A **conversation state** that is always present (for the active chat)
- An **optional group_info state** (shown only when viewing group settings)
- **Optimistic selection** tracking (for instant chat switching before the core confirms)

Its `update()` method is a large match that delegates to child modules and translates their Events:

```rust
Message::Conversation(msg) => {
    let (event, conv_task) = self.conversation.update(msg);
    if let Some(event) = event {
        match event {
            conversation::Event::SendMessage { content, reply_to_message_id } => {
                manager.dispatch(AppAction::SendMessage { ... });
            }
            conversation::Event::ShowGroupInfo => {
                self.group_info = Some(views::group_info::State::new(...));
                manager.dispatch(AppAction::PushScreen { ... });
            }
            // ... 10+ other event variants
        }
    }

    if let Some(task) = conv_task {
        return Some(Event::Task(task.map(Message::Conversation)));
    }
}
```

The home screen also receives a `sync_from_update()` call whenever the core state changes, which it uses to:
- Close overlays when async operations complete (e.g., `creating_chat` goes from `true` to `false`)
- Reconcile optimistic chat selection with the authoritative route
- Auto-dismiss the call screen when a call ends
- Refilter follow lists for open overlays

In practice, this sync method should diff previous and latest core snapshots explicitly:

```rust
pub fn sync_from_update(
    &mut self,
    old_state: &AppState,
    new_state: &AppState,
    manager: &AppManager,
    cached_profiles: &[FollowListEntry],
)
```

This is the desktop equivalent of the mobile `AppManager` reconciliation, but structured as explicit Rust state machine transitions rather than reactive UI framework bindings.

#### 4.3.6.1 Task Propagation and Window Event Routing

Top-level iced `update()` returns `Task<Message>`. Child tasks must be mapped upward through each message namespace:

- `conversation::State::update()` may return `Task<conversation::Message>`
- `home::State::update()` maps it to `Task<home::Message>` via `.map(Message::Conversation)`
- `DesktopApp::update()` maps it again to `Task<crate::Message>` via `.map(Message::Home)` and returns it to iced

Desktop apps should also route window-level events into the message tree. Pika subscribes via `iced::event::listen()`, maps to `Message::WindowEvent`, and translates file-hover/file-drop events into conversation messages (`FileHovered`, `FilesDropped`, `FilesHoveredLeft`).

#### 4.3.7 Contrast with Mobile Platform Layers

The desktop pattern differs from iOS and Android in a fundamental way:

| Aspect | Mobile (iOS/Android) | Desktop (iced) |
|--------|---------------------|----------------|
| Core integration | UniFFI bridge (cross-language FFI serialization) | Direct Rust dependency (zero-cost) |
| State observation | Callback interface (`AppReconciler`) pushes state | Subscription polls `flume::Receiver` |
| UI state | Derived in the native layer (`ViewState`) | Modules own local state alongside `AppState` |
| Component model | SwiftUI/Compose declarative views | iced State/Message/Event modules |
| Message routing | Framework handles (SwiftUI bindings, Compose state) | Explicit `.map()` wrapping through module tree |
| Event bubbling | Not needed (flat reconciler callback) | Required (hierarchical State/Message/Event) |

The key insight: mobile platforms have flat state observation (one `AppReconciler` callback updates the entire UI), while desktop has hierarchical state ownership (each module owns its local state and communicates via events). This hierarchy is necessary because iced has no equivalent to SwiftUI's `@Observable` or Compose's `mutableStateOf` -- state flow must be explicit.

#### 4.3.8 Platform-Specific Desktop Features

Desktop has capabilities that don't apply to mobile:

- **Video pipeline (all Rust):** `nokhwa` for camera capture, `openh264` for H.264 encode/decode, custom wgpu shaders for GPU-accelerated video rendering with aspect-ratio-correct letterboxing. No platform APIs needed -- everything is Rust.
- **Audio:** Handled within the core/MoQ layer using `cpal` for capture/playback.
- **Fonts:** 6 TTF fonts bundled at compile time via `include_bytes!()` (Geist family, Noto Color Emoji, Lucide icons). No system font dependency ensures consistent rendering across Linux/macOS/Windows.
- **Design system:** A `PikaTheme` struct defines design tokens (surfaces, semantic colors, spacing scale, typography) with a Signal-inspired dark theme.
- **macOS release:** Universal binary (arm64 + x86_64 via `lipo`), `.app` bundle with `Info.plist`, `.dmg` packaging. A safety check verifies no `/nix/store` paths leaked into the release binary.

### 4.4 CLI (pikachat)

#### 4.4.1 CLI as a Platform Target

A CLI can consume the Rust core as a direct dependency (like desktop), providing a headless interface for testing, automation, and bot processes.

**Two architectural approaches exist:**

1. **Core-based CLI** -- Uses `FfiApp` directly. Dispatches actions, reads state, runs commands. This is the simplest path and shares 100% of the business logic with mobile/desktop.

2. **Protocol-based CLI** -- Builds on the raw protocol libraries directly (e.g., `mdk-core` + `nostr-sdk`), bypassing the `FfiApp`/`AppState` layer entirely. This is appropriate when the CLI needs capabilities beyond the app's state model (agent provisioning, interop testing, daemon mode).

**Pika example:** `pikachat` takes the second approach -- it uses `mdk-core` and `nostr-sdk` directly with 15+ subcommands including agent management and bot processes. This is a deliberate choice because the CLI's capabilities (daemon mode, provider management, scenario testing) extend far beyond the mobile app's state model. For most RMP apps, the first approach (core-based) is recommended.

---

## Part V: Build System and Cross-Compilation

### 5.1 Workspace and Toolchain Setup

An RMP project needs:
- **Rust toolchain** with cross-compilation targets for all platforms: `aarch64-apple-ios`, `aarch64-apple-ios-sim`, `x86_64-apple-ios`, `aarch64-linux-android`, `armv7-linux-androideabi`, `x86_64-linux-android`.
- **Xcode** (macOS only) with iOS simulator runtimes installed. The Xcode toolchain's clang is used as the C compiler for iOS cross-compilation.
- **Android SDK + NDK** (version 28.2.x recommended) with build tools, platform APIs, and emulator.
- **cargo-ndk** for Android cross-compilation.
- **xcodegen** for iOS project generation from `project.yml`.
- **JDK 17** for Gradle.

**Nix is recommended but optional.** A Nix flake can provision the entire toolchain reproducibly, ensuring every developer and CI runner has the same environment. Without Nix, each tool must be installed manually.

**Pika approach:** No `rust-toolchain.toml` -- the toolchain is 100% Nix-managed via `rust-overlay`. The flake provides a single `rustToolchain` with all six cross-compilation targets pre-installed plus `rust-src` and `rust-analyzer`.

### 5.2 iOS Build Pipeline

The iOS build has five stages:

**1. Build for host** (`rust-build-host`)
```bash
cargo build -p my_core --release  # produces libmy_core.dylib on macOS
```
The host build is needed because `uniffi-bindgen` reads type metadata from the compiled library.

**2. Generate Swift bindings** (`ios-gen-swift`)
```bash
cargo run -p uniffi-bindgen -- generate \
  --library target/release/libmy_core.dylib \
  --language swift \
  --out-dir ios/Bindings \
  --config rust/uniffi.toml
```
Outputs: `my_core.swift`, `my_coreFFI.h`, `my_coreFFI.modulemap`.

**3. Cross-compile for iOS** (`ios-rust`)

This is the most complex step. The Rust compiler must use Xcode's clang as the linker, with the correct SDK root and minimum version flags:

```bash
# For device (aarch64-apple-ios):
env -u SDKROOT -u MACOSX_DEPLOYMENT_TARGET \
  CC="$XCODE_TOOLCHAIN/clang" \
  SDKROOT="$(xcrun --sdk iphoneos --show-sdk-path)" \
  RUSTFLAGS="-C linker=$CC -C link-arg=-miphoneos-version-min=17.0" \
  cargo build -p my_core --lib --target aarch64-apple-ios --release

# For simulator (aarch64-apple-ios-sim):
env -u SDKROOT -u MACOSX_DEPLOYMENT_TARGET \
  CC="$XCODE_TOOLCHAIN/clang" \
  SDKROOT="$(xcrun --sdk iphonesimulator --show-sdk-path)" \
  RUSTFLAGS="-C linker=$CC -C link-arg=-mios-simulator-version-min=17.0" \
  cargo build -p my_core --lib --target aarch64-apple-ios-sim --release
```

**Critical -- Nix environment variable isolation:** When building inside a Nix shell, the shell hook sets `CC`, `CXX`, `AR`, `RANLIB`, `SDKROOT`, `MACOSX_DEPLOYMENT_TARGET`, `LIBRARY_PATH`, and `NIX_LDFLAGS` to Nix's apple-sdk paths. These **completely break** iOS cross-compilation because they point to the macOS SDK, not the iOS SDK. You must unset **all** of them and explicitly point to the system Xcode:

```bash
# The complete env var isolation for iOS cross-compilation inside Nix:
XCODE_DEV="/Applications/Xcode.app/Contents/Developer"
TOOLCHAIN_BIN="$XCODE_DEV/Toolchains/XcodeDefault.xctoolchain/usr/bin"
IOS_SDK="$XCODE_DEV/Platforms/iPhoneOS.platform/Developer/SDKs/iPhoneOS.sdk"
SIM_SDK="$XCODE_DEV/Platforms/iPhoneSimulator.platform/Developer/SDKs/iPhoneSimulator.sdk"

# Device build:
env -u SDKROOT -u MACOSX_DEPLOYMENT_TARGET -u CC -u CXX -u AR -u RANLIB -u LIBRARY_PATH \
  DEVELOPER_DIR="$XCODE_DEV" \
  SDKROOT="$IOS_SDK" \
  CC="$TOOLCHAIN_BIN/clang" \
  RUSTFLAGS="-C linker=$TOOLCHAIN_BIN/clang -C link-arg=-miphoneos-version-min=17.0 -C link-arg=-isysroot -C link-arg=$IOS_SDK" \
  cargo build -p my_core --lib --target aarch64-apple-ios --release
```

Note the explicit `-C link-arg=-isysroot -C link-arg=$IOS_SDK` -- without this, the linker may pick up the wrong sysroot from Nix's environment.

**`xcodebuild` must run outside the Nix shell** (or with `NIX_LDFLAGS` unset). Nix injects linker flags like `-objc_abi_version '-Xlinker'` via `NIX_LDFLAGS` that are not valid for the iOS linker and cause the link step to fail with cryptic errors. The simplest approach:

```bash
# Option 1: Run xcodebuild outside Nix entirely (recommended):
xcodebuild build -project ios/App.xcodeproj -scheme App ...

# Option 2: Unset all Nix linker pollution:
env -u NIX_LDFLAGS -u NIX_CFLAGS_COMPILE -u SDKROOT -u MACOSX_DEPLOYMENT_TARGET \
  -u CC -u CXX -u AR -u RANLIB -u LIBRARY_PATH \
  DEVELOPER_DIR="/Applications/Xcode.app/Contents/Developer" \
  xcodebuild build -project ios/App.xcodeproj -scheme App ...
```

**4. Create xcframework** (`ios-xcframework`)
```bash
# Stage the headers from UniFFI-generated bindings:
mkdir -p staging/headers
cp ios/Bindings/my_coreFFI.h staging/headers/
cp ios/Bindings/my_coreFFI.modulemap staging/headers/module.modulemap

xcodebuild -create-xcframework \
  -library target/aarch64-apple-ios/release/libmy_core.a -headers staging/headers \
  -library target/aarch64-apple-ios-sim/release/libmy_core.a -headers staging/headers \
  -output ios/Frameworks/MyCore.xcframework

rm -rf staging
```

**5. Generate Xcode project** (`ios-xcodeproj`)
```bash
cd ios && xcodegen generate
```

### 5.3 Android Build Pipeline

**1. Build for host** (same as iOS step 1)

**2. Generate Kotlin bindings** (`gen-kotlin`)
```bash
cargo run -p uniffi-bindgen -- generate \
  --library target/release/libmy_core.so \
  --language kotlin \
  --out-dir android/app/src/main/java \
  --no-format \
  --config rust/uniffi.toml
```

**3. Cross-compile for Android** (`android-rust`)
```bash
cargo ndk -o android/app/src/main/jniLibs -P 26 \
  -t arm64-v8a -t armeabi-v7a -t x86_64 \
  build -p my_core --release
```
`cargo-ndk` handles NDK toolchain selection, sysroot configuration, and output placement. `-P 26` sets minimum API level 26 (Android 8.0).

**4. Build APK** (`android-assemble`)
```bash
cd android && ./gradlew :app:assembleDebug
```

**Android-specific Rust dependencies:** SQLCipher on Android requires `bundled-sqlcipher-vendored-openssl` because the NDK does not include OpenSSL. This is handled via Cargo feature flags or conditional dependencies.

### 5.4 Desktop Build Pipeline

**Development:** Simply `cargo run -p my-desktop`. On macOS, you may need `cargo-with-xcode` wrapper to unset Nix environment variables and force the Xcode toolchain.

**macOS release:** Build universal binary (arm64 + x86_64), create `.app` bundle with `Info.plist`, package as `.dmg`. Verify no `/nix/store` paths leaked into the binary via `otool -L`.

**Linux:** Requires X11/Wayland and Vulkan/Mesa runtime libraries. A Nix flake can provision these; otherwise they must be installed system-wide.

**Windows:** Not yet implemented in Pika. The iced framework supports Windows, and Rust cross-compilation to `x86_64-pc-windows-msvc` is mature. This is an open area for the RMP ecosystem.

### 5.5 The justfile: Central Build Orchestrator

The `justfile` (using [just](https://github.com/casey/just)) is the central entry point for all build, test, and release operations. Organize recipes by category:

| Category | Naming Convention | Examples |
|----------|-------------------|----------|
| Platform builds | `ios-*`, `android-*`, `desktop-*` | `ios-rust`, `android-assemble`, `run-desktop` |
| Binding generation | `gen-*`, `*-gen-*` | `gen-kotlin`, `ios-gen-swift` |
| Running | `run-*` | `run-ios`, `run-android`, `run-desktop` |
| Testing | `test`, `*-ui-test`, `*-e2e-*` | `test`, `ios-ui-test`, `android-ui-e2e-local` |
| CI lanes | `pre-commit`, `pre-merge-*`, `nightly-*` | `pre-merge-pika`, `nightly` |
| Release | `release`, `*-release` | `android-release`, `release VERSION` |
| Utilities | `doctor-*`, `fmt`, `clippy` | `doctor-ios`, `fmt`, `clippy` |

Composite recipes chain dependencies: `android-assemble: gen-kotlin android-rust android-local-properties`.

**Recommended composite recipes for common workflows:**

```just
# Full iOS pipeline: host build → Swift bindings → cross-compile → xcframework → xcodegen
ios-full: rust-build-host ios-gen-swift ios-rust ios-xcframework ios-xcodeproj

# Full Android pipeline: host build → Kotlin bindings → cross-compile → Gradle assemble
android-full: rust-build-host gen-kotlin android-rust android-assemble

# Quick iteration: rebuild Rust + regenerate bindings (no platform build)
rebind: rust-build-host ios-gen-swift gen-kotlin
```

The `ios-full` recipe is particularly valuable because the iOS pipeline has 5 sequential steps with environment variable isolation requirements (see Section 5.2). Running them individually is error-prone; a composite recipe ensures the correct order and can embed the `env -u` invocations.

**Pika scale:** dozens of recipes in a ~1,300-line justfile.

### 5.6 Nix Flake: Reproducible Dev Environment

A Nix flake provides fully reproducible developer environments:

```nix
devShells.default = {
  # Rust toolchain with all cross-compilation targets
  # Android SDK + NDK
  # JDK 17
  # cargo-ndk, xcodegen, gradle
  # just, git, curl, python3
  # Platform-specific: Xcode wrapper (macOS), X11/Vulkan libs (Linux)
};
```

The flake ensures every developer and CI runner has identical tool versions. Shell hooks can auto-configure `ANDROID_HOME`, `ANDROID_NDK_HOME`, `JAVA_HOME`, write `android/local.properties`, and set up AVDs.

**The `rmp` wrapper script.** The flake includes a `writeShellScriptBin "rmp"` wrapper that delegates to `cargo run -p rmp-cli` from a source checkout pointed to by `$RMP_REPO`. The wrapper must handle two layouts:

1. **Workspace layout:** `$RMP_REPO/Cargo.toml` is a workspace that contains `rmp-cli` as a member. Use `cargo run --manifest-path "$RMP_REPO/Cargo.toml" -p rmp-cli`.
2. **Standalone layout:** `$RMP_REPO/rmp-cli/Cargo.toml` exists but there is no parent workspace. Use `cargo run --manifest-path "$RMP_REPO/rmp-cli/Cargo.toml"`.

The wrapper should check both paths and fail with a clear error if neither exists.

**`RMP_REPO` auto-detection.** The shell hook should attempt to locate `rmp-cli` in the parent directory of the project (the common monorepo layout where the scaffolded project sits next to the CLI source). Use `cd .. && pwd` rather than `BASH_SOURCE` (which is unreliable in Nix shell contexts):

```bash
if [ -z "''${RMP_REPO:-}" ]; then
  _parent="$(cd .. 2>/dev/null && pwd)"
  if [ -f "$_parent/rmp-cli/Cargo.toml" ]; then
    export RMP_REPO="$_parent"
  fi
fi
```

**Multiple shells for different workflows:**
- `default` -- Full development (all platforms)
- `rmp` -- Minimal for RMP scaffolding and development
- `worker-wasm` -- WebAssembly target (if applicable)
- `infra` -- Infrastructure management

### 5.7 The rmp.toml Configuration File

`rmp.toml` is the single source of truth for project-level configuration:

```toml
[project]
name = "my-app"
org = "com.example"

[core]
crate = "my_app_core"
bindings = "uniffi"

[ios]
bundle_id = "com.example.myapp"
scheme = "MyApp"

[android]
app_id = "com.example.myapp"
avd_name = "my_app_api35"

# Optional:
# [desktop]
# targets = ["iced"]
# [desktop.iced]
# package = "my_app_desktop_iced"
```

Pika currently keeps desktop outside `rmp.toml` (`crates/pika-desktop`, wired by workspace + justfile). Fresh `rmp init --iced` scaffolds should include the `[desktop]` section above.

The `rmp` CLI discovers this file by walking up from the current directory. Every subcommand except `init` requires it. It maps the project name to crate names, platform identifiers, and build targets.

---

## Part VI: Patterns and Recipes

### 6.1 Adding a New Feature (End-to-End Walkthrough)

Every new feature follows the same six steps, regardless of app domain:

**Step 1: Add state fields to `AppState`.**
What does the user need to see? Add fields for it.
```rust
// Example: adding a "dark mode" toggle to any app
pub struct AppState {
    pub rev: u64,
    pub dark_mode: bool,  // new
    // ...
}
```

**Step 2: Add action variants to `AppAction`.**
What can the user do? Each interaction is a variant.
```rust
pub enum AppAction {
    ToggleDarkMode,  // new
    // ...
}
```

**Step 3: Handle the action in `AppCore`.**
What happens when they do it? Mutate state and emit.
```rust
AppAction::ToggleDarkMode => {
    self.state.dark_mode = !self.state.dark_mode;
    self.emit_state();
}
```

**Step 4: Regenerate bindings.**
```bash
rmp bindings all  # or: just ios-gen-swift && just gen-kotlin
```

**Step 5: Consume in platform UI.**
- **iOS:** `Toggle(isOn: .constant(manager.state.darkMode))` with `manager.dispatch(.toggleDarkMode)` on change.
- **Android:** `Switch(checked = manager.state.darkMode, onCheckedChange = { manager.dispatch(AppAction.ToggleDarkMode) })`
- **Desktop:** Read `state.dark_mode` in iced's `view()` to select the theme.

**Step 6: Add a capability bridge if needed.**
Dark mode doesn't need one. If the feature required a platform API (e.g., biometrics for a "lock app" feature), define a callback interface (see Section 6.3).

**More complex example (Pika: adding typing indicators):**
1. Add `typing_members: Vec<TypingMember>` to `ChatViewState`.
2. Add `AppAction::TypingStarted` (dispatched when the user types).
3. In the actor: send a Nostr typing event, track incoming typing events, add/remove from `typing_members` with a timeout.
4. Regenerate bindings.
5. iOS/Android: render "Alice is typing..." below the message list.

### 6.2 Adding a New Screen

1. **Add a `Screen` variant in Rust:**
   ```rust
   pub enum Screen {
       Settings,  // new
       // ...
   }
   ```

2. **Add navigation actions:**
   ```rust
   pub enum AppAction {
       PushScreen { screen: Screen },
       // PopScreen is handled via UpdateScreenStack
   }
   ```

3. **Handle navigation in the actor:**
   ```rust
   AppAction::PushScreen { screen } => {
       self.state.router.screen_stack.push(screen);
       self.emit_state();
   }
   ```

4. **Create the SwiftUI view:** `SettingsView.swift` reading from `manager.state`. Add the case to `screenView(for:)` in `ContentView.swift`.

5. **Create the Compose screen:** `SettingsScreen.kt` reading from `manager.state`. Add the case to the `when(screen)` block in `MainApp.kt`.

6. **Create the iced view:** Add a `settings` view module. Handle in the screen routing.

7. **Wire up:** The existing `NavigationStack`/`AnimatedContent` routing picks up new `Screen` variants automatically through the `when`/`switch` dispatch.

### 6.3 Adding a Platform Capability Bridge

When your app needs a platform API, define the contract in Rust and implement it natively.

**Example: a location bridge for a fitness tracker.**

**1. Define the Rust-side contract:**
```rust
#[uniffi::export(callback_interface)]
pub trait LocationProvider: Send + Sync + 'static {
    fn start_tracking(&self);
    fn stop_tracking(&self);
}
```

**2. Add a reporting channel.** The bridge *reports* data; it doesn't return it from method calls. Define an action for incoming data:
```rust
pub enum AppAction {
    LocationUpdate { lat: f64, lon: f64, accuracy: f64 },
    // ...
}
```

**3. Implement in Swift:**
```swift
class IOSLocationProvider: LocationProvider, CLLocationManagerDelegate {
    let manager: AppManager
    private let clManager = CLLocationManager()

    func startTracking() { clManager.startUpdatingLocation() }
    func stopTracking() { clManager.stopUpdatingLocation() }

    func locationManager(_ manager: CLLocationManager, didUpdateLocations locations: [CLLocation]) {
        let loc = locations.last!
        self.manager.dispatch(.locationUpdate(lat: loc.coordinate.latitude, ...))
    }
}
```

**4. Implement in Kotlin:** Same pattern with `FusedLocationProviderClient`.

**5. Desktop:** Stub or mock if the capability doesn't apply. Or implement using a Rust GPS library if available.

**The guardrail:** If you find yourself adding `if/else` logic to a bridge implementation, stop. Bridges report raw data. Rust makes all decisions.

### 6.4 Managing State Granularity

**Start with full snapshots.** `AppUpdate::FullState(AppState)` on every change is the correct starting point. It is simple, correct, and avoids partial-state consistency bugs. The `rev` counter handles ordering.

**When to consider granular updates:**
- Profiling shows that serialization/deserialization of `AppState` is a measurable bottleneck.
- Your state tree contains large collections (thousands of items) that change infrequently.
- A specific feature sends high-frequency updates (e.g., timer ticks, audio levels).

**How to evolve:**
```rust
pub enum AppUpdate {
    FullState(AppState),
    TimerTick { elapsed_secs: u64 },        // lightweight, high-frequency
    ItemUpdated { id: u64, item: TodoItem }, // targeted update
}
```

Platform `reconcile()` handlers must grow to handle each variant. The complexity cost is real -- only add granular variants when profiling justifies it.

### 6.5 Handling Platform-Specific Behavior

**Route projection** (Section 2.11) maps the same `Router` state to different navigation models per platform. Mobile gets a stack; desktop gets a shell with sidebar and detail pane.

**Conditional compilation** in Rust handles platform-specific logging, keyring initialization, and dependency selection:
```rust
#[cfg(target_os = "ios")]
use apple_native_keyring_store::protected::Store;

#[cfg(target_os = "android")]
use android_native_keyring_store::Store;
```

**Platform identification** at runtime: pass a platform string to `FfiApp::new()` or add a `platform: String` field to the constructor. The actor can use this for platform-specific behavior (e.g., different retry strategies, platform-specific feature flags).

**Feature flags in AppState:** Add `developer_mode: bool` or similar flags to `AppState` that the actor controls and the UI reads. This keeps feature gating in Rust.

### 6.6 Secure Credential Storage

**The rule: Rust never persists secrets.** Private keys, passwords, and tokens are stored natively using each platform's secure storage.

**The pattern:**
1. Rust generates or receives auth/session secrets.
2. Rust emits secret-bearing side-effect updates (e.g., `AccountCreated`, `BunkerSessionDescriptor`), never embedding those secrets in `FullState`.
3. Native persists those secrets in platform secure storage.
4. On cold launch, native restores auth by loading stored mode and dispatching the matching action (`RestoreSession`, `RestoreSessionExternalSigner`, or `RestoreSessionBunker`).
5. Rust reconstructs session state and emits normal `FullState` updates.
6. Rust may keep secrets in memory for active session use, but does not write them to disk.

**Startup restoration UX contract:** expose an explicit "restoring session" loading state during step 4→5 so the UI does not briefly flash the logged-out screen.

**Platform implementations:**
- **iOS:** `KeychainAuthStore` + `KeychainNsecStore` using `kSecClassGenericPassword` with `kSecAttrAccessibleAfterFirstUnlockThisDeviceOnly`. Use a shared access group for App Group sharing (e.g., with NSE).
- **Android:** `SecureAuthStore` using `EncryptedSharedPreferences` with `AES256_GCM` encryption backed by Android Keystore; supports local nsec, external signer, and bunker modes.
- **Desktop:** File-based storage with restricted permissions (`0600` on Unix). Less secure than mobile but acceptable for desktop environments.

### 6.7 Push Notifications

Push notifications in an RMP app have a split architecture:

**Registration:** Native-only. iOS registers with APNs, Android registers with FCM. The resulting device token is dispatched to Rust via `AppAction::SetPushToken`. Rust handles server-side subscription management.

**Display:** Handled by the OS for basic notifications. For rich notifications requiring decryption (like encrypted messaging apps), a background processing component is needed.

**Background processing (iOS NSE):** A separate Rust crate provides decryption within the `UNNotificationServiceExtension`'s constrained environment. The NSE shares data with the main app via App Group containers and Keychain access groups. This crate has minimal dependencies to stay within the NSE's ~24MB memory limit.

**Background processing (Android):** FCM data messages can trigger processing in `FirebaseMessagingService`. Depending on the app's needs, this can call into the Rust core directly or use a separate lightweight Rust crate.

### 6.8 Real-Time Media (Audio/Video Calls)

Real-time media is the quintessential capability bridge in action:

- **Call state machine:** Entirely in Rust. `CallState` (with `CallStatus` enum: Offering, Ringing, Connecting, Active, Ended) is part of `AppState`. Actions like `StartCall`, `AcceptCall`, `EndCall`, `ToggleMute` are dispatched from native. Rust decides all transitions.
- **Audio routing:** Native capability bridge. iOS configures `AVAudioSession` (voice chat mode vs. video chat mode, speaker vs. earpiece). Android acquires `AudioFocus`. Desktop uses `cpal` directly.
- **Video capture:** Native provides camera frames via `sendVideoFrame(payload)`. Rust encodes and transmits.
- **Video receive:** Rust decodes received frames and pushes them to native via `VideoFrameReceiver.onVideoFrame()` at ~30fps. Native renders them.
- **Video rendering:** iOS uses `VideoToolbox` for hardware decoding. Android uses `MediaCodec`. Desktop uses `openh264` + custom wgpu shaders (all Rust, no platform API needed).

The native layer handles OS-level media APIs. Rust owns the call state machine, codec selection, network transport, and all policy decisions (mute, camera on/off, hang up).

### 6.9 Anti-Patterns and Common Drifts

These are the ways RMP discipline breaks down in practice. Each is named, explained, and corrected.

**Anti-pattern 1: Duplicated Formatting Logic**
- **Symptom:** Both Swift and Kotlin have their own timestamp formatting, display name derivation, or message preview generation.
- **Why it happens:** Writing a 5-line Swift extension is faster than adding a Rust field, regenerating bindings, and updating both platforms.
- **The fix:** Add a pre-formatted field to `AppState` (e.g., `display_timestamp: String`). Rust does the work once. Native renders.
- **Pika example:** Timestamp formatting and chat summary strings are still duplicated across iOS and Kotlin in the current codebase. Lowering these into Rust fields (`display_timestamp`, `last_message_preview`) is the intended migration path.

**Anti-pattern 2: Business Logic in ViewState Derivation**
- **Symptom:** Native ViewState mapping functions contain conditional logic, filtering, sorting, or validation.
- **Why it happens:** It feels like "presentation logic," but it's actually business logic in disguise.
- **The fix:** If derivation does anything beyond field renaming or type conversion, it belongs in Rust. ViewState derivation must be trivially mechanical.

**Anti-pattern 3: Navigation Logic Leaking to Native**
- **Symptom:** Native code decides which screen to show based on state conditions, or manages its own navigation stack alongside Rust's Router.
- **Why it happens:** Platform navigation APIs (`NavigationStack`, `NavHost`) have opinions, and it is tempting to use them idiomatically.
- **The fix:** Rust Router is the single source of truth. Native navigation is driven by Router state, never by native-side conditionals. Platform-initiated pops dispatch back to Rust.

**Anti-pattern 4: God Module**
- **Symptom:** The core actor file grows to thousands of lines as every feature adds match arms.
- **Why it happens:** Path of least resistance -- add a few lines to the existing match block.
- **The fix:** Split `handle_message()` by domain. Each domain module handles its own action subset and returns state mutations. The actor orchestrates; it does not implement.

**Anti-pattern 5: Native-Side State Caching**
- **Symptom:** Native code caches derived values from `AppState` and manages invalidation.
- **Why it happens:** Performance concerns with full-state snapshots.
- **The fix:** If caching is needed, do it in Rust. Native should treat every `AppUpdate` as the complete, current truth. If performance is an issue, move to granular updates (Section 6.4) -- not native caching.

**Anti-pattern 6: Capability Bridge Scope Creep**
- **Symptom:** A callback interface that started as "report audio level" now carries policy decisions like "should we mute."
- **Why it happens:** Easier to add a boolean to the existing callback than to round-trip through Rust.
- **The fix:** Bridges report data. Rust makes decisions. If you are adding decision logic to a bridge, extract it back to Rust and have the bridge report raw inputs.

---

## Part VII: Testing Strategy

### 7.1 Rust Core Unit Tests

The Rust core is the most testable part of the stack. Because the actor processes actions and produces state, tests are straightforward:

```rust
#[test]
fn test_add_todo() {
    let (update_tx, update_rx) = flume::unbounded();
    let (core_tx, core_rx) = flume::unbounded();
    // Create AppCore, send AddTodo action, assert state.items contains the new item
}
```

**Patterns:**
- Test actions by feeding `CoreMsg::Action` into the actor and asserting the resulting `AppState`.
- Test async flows by spawning the actor in a test, sending actions, and collecting `AppUpdate` messages from the update channel.
- Mock platform capabilities by implementing callback interfaces with test doubles.
- The `rlib` crate type enables `use my_core::*` in integration tests without going through FFI.

**Pika example:** Route projection has 7 unit tests covering login overrides, stack navigation, detail pane priority, and fallback behavior. Storage refresh functions are tested against in-memory SQLite databases.

### 7.2 Platform UI Tests

**iOS:** XCUITest with simulators. The `AppCore` protocol abstraction (Section 4.1.2) enables preview factories and test doubles that return pre-built `AppState` values without running the Rust core.

**Android:** Compose testing with `@Composable` test rules. Since all screens read from `AppManager.state`, tests can create an `AppManager` with mock state and assert composable output.

**Desktop:** `cargo test -p my-desktop` tests the manager and UI wiring. iced widgets can be tested for correct output given specific state.

**Across all platforms:** Because the UI is a pure function of `AppState`, most UI tests reduce to: set up a specific `AppState`, render the view, assert the output. Business logic testing belongs in the Rust layer (Section 7.1).

### 7.3 End-to-End Tests

**Local E2E (deterministic):** Spin up local infrastructure (e.g., a local relay, a local bot), run the app on a simulator/emulator, and execute a scripted flow. This tests the full stack without network variability.

**Public E2E (nondeterministic):** Run against production infrastructure. Useful for integration validation but inherently flaky due to network conditions.

**Interop tests:** For apps that communicate with other clients, cross-app compatibility tests verify protocol compliance.

**Pika example:** Local E2E spins up a Nostr relay + a Rust bot, runs the app on iOS simulator or Android emulator, and executes chat flows. Nightly lanes run extended E2E suites including call tests over MoQ relays.

### 7.4 CI/CD Pipeline

**Layered pre-merge lanes** test components independently:
- `pre-merge-core` -- fmt + clippy + Rust tests
- `pre-merge-android` -- Gradle build + instrumented tests
- `pre-merge-ios` -- Xcode build + UI tests (macOS runners)
- `pre-merge-desktop` -- cargo check/test
- `pre-merge-rmp` -- scaffold QA (multiple `rmp init` variants)

**Nightly lanes** run extended tests requiring hardware or network:
- iOS simulator E2E
- Android emulator E2E
- Desktop launch smoke test (with `xvfb-run` on Linux)
- Public relay integration tests

**RMP scaffold QA:** Test multiple scaffold configurations (iOS-only, Android-only, with-iced, with-flake, etc.) to ensure the scaffolding tool works across all combinations.

**Release pipeline:** Version validation, signed builds (encrypted keystores via `age`), platform-specific packaging, distribution (app stores, Zapstore, DMG).

---

## Part VIII: From Zero to Running App

### 8.1 The rmp CLI Tool

The `rmp` CLI scaffolds new RMP projects and manages their lifecycle:

| Command | Purpose | Needs `rmp.toml`? |
|---------|---------|-------------------|
| `rmp init <name>` | Scaffold a new project | No (creates it) |
| `rmp doctor` | Check toolchain prerequisites | Yes |
| `rmp devices list` | List simulators/emulators | Yes |
| `rmp devices start <platform>` | Start a target device | Yes |
| `rmp bindings <target>` | Generate UniFFI bindings + build artifacts | Yes |
| `rmp run <platform>` | Build, install, and launch | Yes |

**Key flags for `rmp init`:**
- `--org <org>` -- reverse-DNS organization prefix (default: `com.example`)
- `--ios / --no-ios` -- include/exclude iOS (default: included)
- `--android / --no-android` -- include/exclude Android (default: included)
- `--iced / --no-iced` -- include/exclude iced desktop (default: excluded)
- `--flake` -- generate a Nix `flake.nix` dev shell
- `--git` -- initialize a git repo and stage files

### 8.2 Anatomy of a Scaffolded Project

`rmp init my-app --org com.acme` generates a complete, working project:

```
my-app/
├── rmp.toml                # Project config
├── Cargo.toml              # Workspace (rust/ + uniffi-bindgen/ + optional desktop/)
├── justfile                # Convenience recipes (doctor, bindings, run-ios, run-android)
├── rust/
│   ├── Cargo.toml          # crate-type = ["cdylib", "staticlib", "rlib"]
│   ├── uniffi.toml         # Kotlin package config
│   └── src/lib.rs          # Starter FfiApp + AppState + AppAction + AppUpdate
├── uniffi-bindgen/         # Binding generator binary
├── ios/                    # SwiftUI app
│   ├── project.yml         # XcodeGen definition
│   ├── Info.plist
│   └── Sources/
│       ├── App.swift           # @main entry point
│       ├── AppManager.swift    # @Observable bridge class
│       └── ContentView.swift   # Starter UI
├── android/                # Jetpack Compose app
│   ├── build.gradle.kts
│   ├── settings.gradle.kts
│   └── app/src/main/java/com/acme/myapp/
│       ├── MainActivity.kt
│       ├── AppManager.kt       # Singleton bridge class
│       └── ui/MainApp.kt       # Starter Compose UI
└── desktop/iced/           # (only with --iced)
    └── src/main.rs         # iced app using core directly
```

As projects mature, many teams move the desktop app to `crates/<name>-desktop/` to match monorepo conventions; the architecture and data flow stay the same.

**The starter `rust/src/lib.rs`** includes a fully working implementation with:
- `AppState { rev, greeting }` -- a single string field
- `AppAction::SetName { name }` -- updates the greeting
- `AppUpdate::FullState(AppState)` -- full snapshot on every change
- `FfiApp` with constructor, `state()`, `dispatch()`, `listen_for_updates()`
- `AppReconciler` callback interface
- Actor thread with `flume` channels and `rev` counter

The scaffold compiles and runs immediately on all platforms. The "hello world" demo shows bidirectional communication: the user enters a name on the native UI, it dispatches `SetName` to Rust, Rust updates the greeting, and the UI re-renders.

### 8.3 From Scaffold to Real App

After the scaffold runs:

1. **Define your domain state.** Replace `greeting: String` with your actual data model in `AppState`. Add nested `uniffi::Record` types for complex domain objects.

2. **Define your actions.** Replace `SetName` with your actual user intents in `AppAction`. Cover all screens and interactions.

3. **Build the core loop.** Expand the `match` block in the actor thread (or extract `AppCore` for larger apps). Handle each action, mutate state, emit updates.

4. **Regenerate bindings.** `rmp bindings all` after every Rust change.

5. **Build your screens.** Each platform gets its own screen implementations that read from `AppState` and dispatch `AppAction`. Start with one platform and replicate to the others.

6. **Add capabilities as needed.** Define callback interfaces for platform APIs, implement on each platform, inject into `FfiApp`.

7. **Set up CI.** Add pre-merge lanes (Section 7.4) to catch regressions on each platform.

### 8.4 Prerequisites and Environment Setup

**Universal:**
- Rust toolchain (stable) with cross-compilation targets
- `just` (task runner)
- `cargo-ndk` (Android cross-compilation)

**iOS (macOS only):**
- Xcode with iOS simulator runtimes
- `xcodegen`

**Android:**
- Android SDK + NDK (28.2.x)
- JDK 17
- Gradle
- An AVD (Android Virtual Device) for testing

**Desktop:**
- Linux: X11/Wayland + Vulkan/Mesa libraries
- macOS: Xcode command line tools
- Windows: MSVC toolchain (for future support)

**Recommended:** Install everything via a Nix flake (`rmp init --flake`). This gives you a reproducible environment with one command: `nix develop`.

`rmp doctor` checks for the minimum prerequisites and reports what is missing.

### 8.5 Designing Your Domain

This is the most important step before writing code. Map your app idea onto the RMP primitives.

**Step 1: Define your state.** Walk through every screen of your app and list every piece of data it displays. That is your `AppState`.

```rust
// Todo app
pub struct AppState {
    pub rev: u64,
    pub router: Router,
    pub items: Vec<TodoItem>,
    pub active_count: u32,
    pub completed_count: u32,
    pub current_filter: Filter,
    pub editing: Option<u64>,
    pub busy: BusyState,
    pub toast: Option<String>,
}

// Fitness tracker
pub struct AppState {
    pub rev: u64,
    pub router: Router,
    pub current_session: Option<WorkoutSession>,
    pub history: Vec<WorkoutSummary>,
    pub weekly_stats: WeeklyStats,
    pub active_timer_secs: u64,
    pub busy: BusyState,
    pub toast: Option<String>,
}

// Photo editor
pub struct AppState {
    pub rev: u64,
    pub router: Router,
    pub layers: Vec<Layer>,
    pub selected_layer_id: Option<u64>,
    pub active_tool: Tool,
    pub can_undo: bool,
    pub can_redo: bool,
    pub processing: bool,
    pub export_progress: Option<f32>,
    pub toast: Option<String>,
}
```

**Step 2: Define your actions.** What can the user *do*? Be specific and imperative.

```rust
// Todo app
pub enum AppAction {
    AddTodo { text: String },
    ToggleTodo { id: u64 },
    DeleteTodo { id: u64 },
    SetFilter { filter: Filter },
    ClearCompleted,
    // lifecycle
    Foregrounded,
    ClearToast,
}
```

**Step 3: Identify capability bridges.** What platform APIs does your app need?

| App | Capability | Bridge Pattern |
|-----|-----------|----------------|
| Todo app | None needed | -- |
| Fitness tracker | GPS | `LocationProvider` callback → `LocationUpdate` action |
| Fitness tracker | Background tracking | `BackgroundTaskBridge` → iOS BGTask / Android WorkManager |
| Photo editor | File picker | `FilePickerBridge` → native picker → `FileSelected` action |
| Photo editor | Photo library | `PhotoLibraryBridge` → native gallery → `PhotoSelected` action |
| Any app | Push notifications | `PushNotificationManager` → native APNs/FCM → `SetPushToken` action |

**Step 4: Draw the state flow.** For each screen, trace the loop:
```
User taps "Add Todo"
  → dispatch(AddTodo { text: "Buy milk" })
  → Rust: items.push(TodoItem { id: next_id, text, done: false })
  → Rust: active_count += 1
  → emit FullState
  → iOS/Android: list re-renders with new item
```

If you cannot draw this loop cleanly for every interaction, your state design needs work.

**Step 5: Decide what stays native.** Apply the Section 1.5 checklist. The default is always "put it in Rust." Only go native when the checklist forces you to.

**This exercise produces three artifacts before you write any code:**
1. An `AppState` struct (even as pseudocode)
2. An `AppAction` enum (even as a list on paper)
3. A capability bridge inventory (even as bullet points)

With these three artifacts, you can `rmp init`, replace the starter code, and start building.

---

## Part IX: Migration and Refactoring

### 9.1 Lowering Logic from Native to Rust

When you discover duplicated logic across platforms (and you will), the migration pattern is:

1. **Add the Rust field.** Add a pre-computed field to `AppState` or the relevant nested type. Compute it in the actor when the underlying data changes.
2. **Update native to use it.** Replace the native computation with a read from `state.newField`. Both platforms benefit simultaneously.
3. **Remove native logic.** Delete the Swift extension, Kotlin utility function, or ViewState computation that is now redundant.
4. **Regenerate bindings.** `rmp bindings all`.

**Priority order for lowering (highest first):**
1. **String formatting** (timestamps, display names, message previews) -- duplicated across all platforms, easy to move, immediate consistency win.
2. **Validation** (key validation, input normalization) -- business logic that should never live in the UI layer.
3. **Computed aggregates** (unread counts, active item counts, statistics) -- if native code computes these, they will diverge.
4. **State machines** (recording state, upload progress, timer state) -- policy logic that belongs in the actor.
5. **Feature flags and developer mode** -- if native code gates features, the gates will diverge.

### 9.2 The Platform Parity Blueprint

When one platform is ahead of another (common when iOS ships first and Android catches up), use this three-phase approach:

**Phase 1: Lower logic to Rust.**
Audit both platforms for duplicated parsing, validation, and formatting. Move everything that is not a platform capability into Rust. This phase improves *both* platforms, not just the lagging one. The iOS app gets cleaner too.

**Phase 2: Native UI polish.**
Now that business logic is shared, focus the lagging platform's native layer on UX quality. Apply platform design guidelines (Material Design 3 for Android, Human Interface Guidelines for iOS). Add ViewModel layers if needed. Address accessibility, transitions, edge-to-edge layout, adaptive layout for tablets/foldables.

**Phase 3: Add missing features.**
With shared Rust logic in place, adding features to the lagging platform is mostly native UI work. Each feature is: add a Compose/SwiftUI screen, wire it to existing Rust state and actions. This is dramatically faster than Phase 1 because the Rust foundation is already there.

**Pika example:** Android was behind iOS. Phase 1 identified 12 pieces of logic to lower to Rust (message segments, timestamps, display strings, key validation, toast auto-dismiss, etc.). Phase 2 planned Material Design 3 adoption, ViewModel introduction, and accessibility improvements. Phase 3 listed 15+ missing features (typing indicators, reactions, media attachments, voice messages) that become straightforward once Phases 1-2 are done.

### 9.3 Evolving the State Model

As your app grows, `AppState` will evolve. Handle this carefully:

**Adding fields:** New fields with default values are backward-compatible. The actor initializes them in `AppState::empty()` or equivalent. Native code that doesn't know about the field ignores it (after binding regeneration, it will be available).

**Removing or renaming fields:** This is a breaking change. Regenerate bindings, update all platform code, and ship simultaneously.

**State versioning:** For apps with persistent state, consider a `state_version: u64` field in your configuration or database schema. When the app starts with an older schema, the actor can migrate before emitting the first `AppUpdate`.

**Granularity evolution:** When profiling shows full snapshots are a bottleneck:
1. Add specific `AppUpdate` variants for high-frequency changes (e.g., `TimerTick`, `ScrollPositionChanged`).
2. Keep `FullState` as the fallback for infrequent changes.
3. Platform `reconcile()` methods handle each variant type independently.
4. Never remove `FullState` -- it remains the catch-all and the simplest correctness guarantee.

**FFI contract versioning:** UniFFI does not have built-in versioning. If you need to support multiple native app versions against different Rust core versions, maintain backward compatibility in `AppState` (add fields, don't remove them) or version the `FfiApp` constructor.

---

## Appendix: Research Scratchpad

### Raw Findings and Open Questions

**Pika Reference Implementation Structure:**
```
pika/
├── rust/                  # Core crate: AppState, AppAction, AppUpdate, AppCore actor
│   ├── src/
│   │   ├── lib.rs         # FfiApp (UniFFI Object), callback interfaces, scaffolding
│   │   ├── state.rs       # All FFI-visible state types
│   │   ├── actions.rs     # AppAction enum (~50 variants)
│   │   ├── updates.rs     # AppUpdate + internal CoreMsg/InternalEvent
│   │   ├── route_projection.rs  # Mobile vs desktop navigation projection
│   │   ├── external_signer.rs   # Callback interface for external signers
│   │   ├── mdk_support.rs       # MLS library integration
│   │   ├── logging.rs           # Platform-specific logging (oslog, paranoid-android)
│   │   └── core/                # AppCore actor + domain modules
│   ├── Cargo.toml         # crate-type = ["cdylib", "staticlib", "rlib"]
│   └── uniffi.toml        # Kotlin package config
├── uniffi-bindgen/        # Standalone binary for binding generation
├── ios/
│   ├── Sources/           # SwiftUI app (AppManager, ContentView, screens)
│   ├── Bindings/          # UniFFI-generated Swift (checked into git)
│   ├── Frameworks/        # PikaCore.xcframework, PikaNSE.xcframework
│   └── project.yml        # XcodeGen
├── android/
│   └── app/src/main/java/
│       ├── <package>/     # Kotlin app (AppManager, screens, bridges)
│       └── <package>/rust/ # UniFFI-generated Kotlin (checked into git)
├── crates/
│   ├── pika-desktop/      # iced desktop app (direct Rust dep, no FFI)
│   ├── rmp-cli/           # Scaffolding tool
│   ├── pika-nse/          # Notification Service Extension crate
│   └── ...
├── rmp.toml               # RMP project config
├── justfile               # Build orchestrator
└── flake.nix              # Nix dev environment
```

**Key Architectural Decisions:**
- UniFFI proc-macros only (no UDL files) — simpler, type-checked at compile time
- Full state snapshots over granular diffs — MVP tradeoff for simplicity
- Single FfiApp object as entry point — clean API surface
- Generated bindings checked into git — builds don't require host compilation step every time
- Separate Rust crate for NSE — memory/lifecycle constraints of notification extensions
- No ViewModels on Android (current design) — Rust owns all state, Compose reads directly
- Desktop skips FFI entirely — pure Rust-to-Rust, fastest path

**Data Flow Diagram:**
```
┌─────────────────────────────────────────────────────────────┐
│                     NATIVE UI LAYER                         │
│  ┌──────────┐  ┌──────────────┐  ┌────────────────────┐    │
│  │ SwiftUI  │  │ Compose      │  │ iced (direct Rust) │    │
│  │ Views    │  │ Screens      │  │ Views              │    │
│  └────┬─────┘  └──────┬───────┘  └────────┬───────────┘    │
│       │               │                    │                │
│  ┌────▼─────┐  ┌──────▼───────┐  ┌────────▼───────────┐    │
│  │ AppMgr   │  │ AppMgr       │  │ AppMgr             │    │
│  │ @Observable│ │ mutableState │  │ (direct)           │    │
│  └────┬─────┘  └──────┬───────┘  └────────┬───────────┘    │
│       │dispatch()      │dispatch()          │dispatch()     │
├───────┼────────────────┼────────────────────┼───────────────┤
│       │   UniFFI       │   UniFFI           │  Direct       │
│       │   (Swift)      │   (Kotlin/JNA)     │  Rust call    │
├───────┼────────────────┼────────────────────┼───────────────┤
│       ▼                ▼                    ▼               │
│  ┌──────────────────────────────────────────────────────┐   │
│  │                    FfiApp                             │   │
│  │  ┌────────────────────────────────────────────────┐  │   │
│  │  │  flume channel (CoreMsg)                       │  │   │
│  │  └──────────────────┬─────────────────────────────┘  │   │
│  │                     ▼                                │   │
│  │  ┌────────────────────────────────────────────────┐  │   │
│  │  │  AppCore (single-threaded actor)               │  │   │
│  │  │  - handle_message()                            │  │   │
│  │  │  - mutate AppState                             │  │   │
│  │  │  - emit AppUpdate                              │  │   │
│  │  └──────────────────┬─────────────────────────────┘  │   │
│  │                     │                                │   │
│  │  ┌──────────────────▼─────────────────────────────┐  │   │
│  │  │  Arc<RwLock<AppState>>                         │  │   │
│  │  │  (shared_state for sync reads)                 │  │   │
│  │  └────────────────────────────────────────────────┘  │   │
│  │                     │                                │   │
│  │  ┌──────────────────▼─────────────────────────────┐  │   │
│  │  │  update_tx -> AppReconciler.reconcile(update)  │  │   │
│  │  │  (callback to native, invoked on bg thread)    │  │   │
│  │  └────────────────────────────────────────────────┘  │   │
│  └──────────────────────────────────────────────────────┘   │
│                     RUST CORE                               │
└─────────────────────────────────────────────────────────────┘
```

**Build Pipeline Per Platform:**
| Platform | Bindings | Rust Artifact | Platform Build | Final Output |
|---|---|---|---|---|
| iOS | UniFFI -> Swift + C header | Static lib (.a) per target | xcodebuild -create-xcframework -> xcodegen -> xcodebuild | .app / .ipa |
| Android | UniFFI -> Kotlin | Shared lib (.so) via cargo-ndk | Gradle assembleDebug/Release | .apk |
| Desktop | None (direct Rust dep) | Native binary | cargo run/build | Binary / .app+.dmg (macOS) |
| CLI | None (direct Rust dep) | Native binary | cargo run/build | Binary |

**Things That Belong in Rust (Examples from Pika):**
- Message content parsing/formatting (ContentSegment enum)
- Peer key validation/normalization
- Timestamp formatting (`display_timestamp`) *(planned migration target; currently still formatted natively on mobile)*
- Chat list display strings (`display_name`, `subtitle`, `last_message_preview`) *(planned migration target; currently still partially native-derived)*
- First-unread-message tracking
- Toast auto-dismiss timers
- Voice recording state machine (but audio capture stays native)
- Call duration display formatting
- Developer mode flag
- All MLS/crypto operations
- All Nostr protocol operations
- All networking and relay management
- All persistence (SQLite/SQLCipher)
- Navigation state (Router with screen_stack)

**Things That Must Stay Native (Examples from Pika):**
- Audio session routing (AVAudioSession / AudioManager)
- Video capture/decode (VideoToolbox / MediaCodec)
- Push notification lifecycle (NSE / FirebaseMessagingService)
- QR code scanning (camera APIs)
- Keychain / EncryptedSharedPreferences
- External signer intent handling (Amber on Android)
- Haptic feedback
- System share sheet
- Clipboard access

**Key Dependencies for an RMP Project:**
- `uniffi` (0.31.x) — FFI binding generation
- `flume` — MPSC channels for actor message passing
- `tokio` — Async runtime for I/O
- `rusqlite` + `libsqlite3-sys` (bundled-sqlcipher) — Encrypted storage
- `tracing` — Structured logging
- `tracing-oslog` (iOS) / `paranoid-android` (Android) — Platform logging
- `serde` + `serde_json` — Serialization
- `cargo-ndk` — Android cross-compilation tool
- `xcodegen` — iOS project generation
- JNA 5.18.x (Android) — Java Native Access for UniFFI

**Open Research Questions:**
- [ ] How should Windows desktop builds work? (Currently no Windows target in the project)
- [ ] What's the best approach for Linux desktop distribution? (AppImage, Flatpak, etc.)
- [ ] How to handle platform-specific UI testing for the Rust layer?
- [ ] What's the performance ceiling of full-state snapshots? When does granular become necessary?
- [ ] How to handle background processing differently per platform? (iOS BGAppRefreshTask, Android WorkManager, Desktop always-on)
- [ ] What's the recommended approach for platform-specific deep linking through the Rust router?
- [ ] How should accessibility semantics be handled across the FFI boundary?
- [ ] What's the story for WebAssembly as a target? (pikachat-wasm crate exists but is scaffold status)
- [ ] How to handle platform-specific permissions (camera, microphone, contacts) through the capability bridge?
- [ ] What's the recommended testing strategy for the FFI boundary itself?
- [ ] How should feature flags work across the Rust/native boundary?
- [ ] What's the upgrade/migration story for AppState schema changes across app versions?
- [ ] How to handle platform-specific analytics/telemetry through the capability bridge?
- [ ] What about hot reload / fast iteration during development? (Rust compile times)
- [ ] How should platform-specific assets (icons, images, colors) be coordinated with Rust state?
