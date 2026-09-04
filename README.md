# maple-gpui

Native Maple desktop app. It rebuilds Maple Agent Mode in
[gpui](https://crates.io/crates/gpui) on top of the Maple agent runtime
ported from the Tauri app.

The same binary also runs as an Agent Client Protocol (ACP) agent and as an
OpenAI-compatible proxy. See "Command line" below.

## Layout

```
app/                  The maple-gpui binary. Owns the window, login, chat,
                      settings, notifications, and the backend adapter.
crates/maple-agent/   Maple's transport-neutral agent runtime, extracted from
                      the Tauri app with Tauri removed. Owns embedded Goose,
                      the Maple provider over the OpenSecret SDK, developer
                      tools, permission policy, account-scoped session
                      storage, and the ACP server.
crates/maple-billing/ HTTP client for the Maple billing API.
docs/                 Theme spec measured from the Tauri app.
scripts/              One maintainer helper: screenshot.py takes a desktop
                      screenshot through the xdg portal on GNOME Wayland.
                      Nothing in the build or the app uses it.
```

### Backend / frontend boundary

`app/src/backend.rs` is the only file that imports `maple_agent`. It owns a
private Tokio runtime and exposes an async facade (`AgentBackend`) plus one
event stream. The UI talks to that facade only. This mirrors Maple's own
edge-adapter pattern, so a future process split replaces the facade without
touching UI code.

The runtime was copied from `Maple/frontend/src-tauri/src` (`agent.rs`,
`agent/*`, `maple_api.rs`, `open_secret_config.rs`) and changed only to
remove Tauri:

- `tauri::http` types → the `http` crate
- Tauri event and command adapters dropped; auth state and the agent event
  sink are injectable traits
- public visibility opened on the service surface the app consumes

Goose is pinned to the same aaif-goose fork revision as Maple.

## Features

- Sign in with email and password, or with GitHub, Google, or Apple OAuth.
  The session persists in `auth.json` (mode 0600) so the next launch and
  the `acp` mode skip sign-in. The window opens while the saved session is
  checked, and a check that cannot reach the server keeps the credentials
  for the next launch; only a refusal from the server signs the user out.
- Agent chat with streaming Markdown, tool calls, permission prompts,
  agent questions, image attachments (picker, paste, or drag and drop),
  a per-message Copy button, and a context-window indicator.
- Slash commands in the composer: `/btw` asks a side question the task
  never sees, plus `/compact`, `/new`, `/pin`, `/web`, `/model`, and
  `/help`. The account's skills appear in the same list.
- The task's latest todo list stays pinned above the composer.
- Subagents: the task can give a piece of work to a subagent with the
  `delegate` tool, which runs it in its own context. Known limitation:
  a subagent does not inherit the task's permission mode. Goose runs
  every subagent with all tools approved, so even in Read only mode a
  subagent can run shell commands and edit files without a prompt. The
  fix needs the Goose fork to forward subagent approvals to the parent
  (summon.rs hard-codes Auto because an approval would hang). The
  subagents that work now show above the composer with the tool each one
  runs and how long it has worked. A subagent that runs in the background
  keeps its row after the turn ends, and Maple tells the task when it
  finishes: into the running turn, or into the next one.
- Voice: dictate a message with the microphone button, and read any
  message aloud. Both use Maple's speech models; the voice and speed
  are settings.
- The composer spell checks as you type; right-click a word for
  suggestions or to add it to your dictionary.
- Message queue: Enter during a run queues the message for the next turn,
  Ctrl+Enter (Cmd+Enter) steers it into the current turn. Queued messages
  can be sent now, edited in the composer (the message keeps its place in
  the queue), or removed.
- Sidebar search filters tasks and projects by name; Escape clears it.
- Up and Down in an empty composer recall prompts sent in this window.
- Optional composer-only Vim editing provides Normal, Insert, and characterwise
  Visual modes, Unicode-aware motions and text objects, operators and counts,
  an unnamed register, undo/redo transactions, and structured dot repeat.
  Enable it under General settings; every other text field stays standard.
- Optional application Vim navigation moves a stable semantic selection through
  Chat sidebar and transcript rows, modal choices, and Settings controls. It is
  independent of composer Vim and leaves ordinary text fields unchanged.
- Projects (working directories) with pinned and recent roots, rename,
  open in the file manager, and remove. Projects that provide skills ask
  for a trust decision before their guidance loads.
- Sessions grouped by project, with rename, archive, and restore.
- Settings: General (default permission mode, web tools, appearance,
  tool call details, desktop notifications, tool call summaries, composer Vim,
  application Vim, and the speech voice and speed), Keyboard Shortcuts, System
  prompt, Integrations (detected built-ins and custom MCP servers), Usage (plan
  meter from the billing API plus totals from the Goose ledger), and About.
- Dark and light themes; the default follows the system.
- Billing status from the Maple billing API.
- Desktop notifications when a task finishes, asks a question, or needs
  permission while the window is not focused.
- Release check on launch: a banner links to a newer GitHub release.
  Nothing is downloaded or installed by the app.
- Window size and maximized state persist between launches.

### Integrations preview

On macOS and Linux, Settings > Integrations can set up computer use inside
Maple itself. The embedded CUA runtime uses the pinned Cua Driver Rust SDK; it
does not need a separate daemon, executable, or MCP child process.

On macOS, setup reports and requests Accessibility and Screen Recording for
Maple's own app identity. Grants held by a separately installed CuaDriver app
do not transfer to Maple. On Linux there is no setup step: the desktop portal
asks for consent the first time a task captures the screen or sends input.
Under GNOME on Wayland, install the `winrects@cua` GNOME Shell extension that
ships with the SDK for accurate pointer targeting and the agent cursor.

CUA keeps its native screenshot defaults. Every model receives full
accessibility text plus a bounded projection of exact structured grounding
data such as window IDs and element tokens. Vision models retain the canonical
image blocks; text-only models instead receive a CUA-specific description from
Maple's existing Gemma image helper, with raw screenshot blocks removed before
the primary-model request. Maple owns the task-scoped session lifecycle and
prevents models from mixing standalone CLI or other MCP session identities into
the embedded transport.

Enabling an integration sets a device-local default for new tasks. Existing
tasks keep their frozen integration choice and expose CUA as an independent
per-task switch in the composer. A task that never chose a backend adopts the
device default only when it can actually run it. A detected standalone
CuaDriver, which Maple looks for on macOS only, remains a legacy-compatible
backend until the user explicitly sets up the built-in one;
Maple does not install or update it, start or stop its daemon, or alter another
client's configuration. Custom STDIO and Streamable HTTP MCP servers remain
account configuration that may roam between devices.

The embedded design, migration rules, privacy boundary, and preview limits are
documented in [`docs/embedded-cua.md`](docs/embedded-cua.md).

### Composer Vim preview

Turn on **Vim mode in composer** in General settings. The composer opens in
Normal mode and shows a small `NORMAL`, `INSERT`, or `VISUAL` badge; login,
search, rename, settings, prompt, question, and MCP fields keep their ordinary
editing behavior.

The preview includes `h/j/k/l`, `w/b/e`, `0/$`, `gg/G`, `i/a/I/A/o/O`,
`d/c/y` with motions or `iw`/`aw`, `dd/cc/yy`, counts, `v`, `x`, `p/P`,
`u`, `Ctrl-R`, and structured `.` repeat. Arrow keys also move in Normal and
Visual modes. Escape leaves Insert or Visual for Normal; Enter sends in Insert
or Normal, while Shift-Enter inserts a newline only in Insert.

Composer Vim uses the same customizable shortcut catalog as the rest of the
app. It can be enabled with or without application Vim.

### Application Vim preview

Turn on **Vim navigation across the app** in General settings. Application Vim
owns a stable semantic selection while ordinary inputs retain normal text
editing. Chat remembers transcript selection per task, follows streaming only
while the selection is pinned to the newest row, and resolves sidebar projects
and tasks by stable IDs rather than virtual-list indices. Direct clicks update
the same selection state.

The preview includes `j/k`, `gg/G`, counts, `Enter`, `h/l`, `y`, `/`, `ga`,
`[a`/`]a`, `gi`, and `Ctrl-W h/j/k/l`. `Space s n` starts a task and `Space ,`
opens Settings. The Settings page itself is navigable, including its shortcut
search and editable binding rows. Annotation motions are registered and report
that no annotation source is available in the current app. A root action
palette and which-key display remain follow-up work rather than hidden partial
implementations.

Closing Settings explicitly restores Chat focus because the two screens are
separately mounted. Application Vim returns to its semantic focus proxy;
Standard mode returns to the composer. The Standard-mode handoff is intentional
cross-screen behavior rather than an opt-in Vim side effect.

### Keyboard shortcuts preview

Open **Keyboard Shortcuts** in Settings to search, record, disable, or reset
the 159 bindings Maple ships in this preview. Recording accepts sequences of up to
four strokes; Enter saves, Backspace removes the latest stroke, and Escape
cancels. Exact and prefix collisions are shown before saving, with an explicit
choice to replace the other bindings or keep compatible chords.

The catalog covers the existing GPUI actions plus the typed composer and
application Vim commands. It does not expose raw input behavior such as
composer send, Shift-Enter, slash completion, or login field traversal.
Per-binding changes are stored in `settings.json` under `shortcut_overrides`;
missing entries keep their shipped key and `null` disables that exact binding
slot. Maple validates the complete candidate map before replacing the live one,
so a malformed override cannot leave ordinary text editing half-installed.

## Prerequisites

- Rust stable (edition 2024; 1.94 or newer)
- Linux: `libxkbcommon-dev`, `libxkbcommon-x11-dev`, `libfontconfig-dev`,
  `libfreetype-dev`, `libclang-dev`, `cmake`, ALSA headers for the
  microphone and playback, and a Vulkan loader. Debian/Ubuntu:

  ```sh
  sudo apt install libxkbcommon-dev libxkbcommon-x11-dev \
       libfontconfig-dev libfreetype-dev libclang-dev cmake \
       libasound2-dev mesa-vulkan-drivers
  ```

- macOS and Windows: the Rust toolchain only.

## Build and run

```sh
cargo run -p maple-gpui              # desktop app, dev profile
cargo build --release -p maple-gpui  # release binary in target/release
```

On macOS, use `just debug-app` when testing features that depend on privacy
permissions. It stages the debug binary in a stable, development-only `.app`
identity, discovers and embeds any Swift compatibility libraries required by
native dependencies, signs nested code before sealing the bundle, and prints
the exact bundle path to launch. This requires a full Xcode toolchain but does
not require a Developer ID or produce a release artifact. The default ad hoc
identity changes when Maple is rebuilt, so macOS may require the development
app's privacy grants again. Set `MAPLE_DEBUG_CODESIGN_IDENTITY` to the name or
SHA-1 hash of an Apple Development identity in the local keychain when more
stable grants across rebuilds are useful.

### Nix

The flake provides a release package and a development shell with the latest
stable Rust toolchain pinned by `flake.lock`:

```sh
nix build
nix develop
```

On Apple Silicon macOS, the pure `nix build` package enables GPUI's runtime
Metal shader compilation because Apple does not redistribute the `metal`
compiler with the macOS SDK. To precompile the shaders with the Metal toolchain
from the standard Xcode installation instead, install the optional Xcode
component and build in the development shell:

```sh
xcodebuild -downloadComponent MetalToolchain
nix develop --command cargo build --release -p maple-gpui --locked
```

The development shell uses `/Applications/Xcode.app`. Linux builds use the
Nix-provided ALSA, font, keyboard, Wayland, and Vulkan dependencies.

### Shared Rust build cache

Local Nix shells and `just` recipes use Cargo's separate build directory
(`CARGO_BUILD_BUILD_DIR` / `build.build-dir`) to share Rust intermediate
artifacts across maple-gpui checkouts and git worktrees. Final artifacts
remain in the current checkout under `target/`, so `just run`,
`just dist`, and debugger paths do not change.

The default cache is separated by rustc host triple and compiler version:

```text
$HOME/.cache/cargo-build/maple-gpui/<host-triple>/rust-<version>
```

An existing `CARGO_BUILD_BUILD_DIR` takes precedence. To temporarily restore
Cargo's traditional checkout-local layout, set
`MAPLE_GPUI_DISABLE_SHARED_CARGO_BUILD_DIR=1`. CI does not enable the local
shared cache automatically.

Raw `cargo clean` removes both the checkout's target directory and the
configured shared build directory. To clean only the current checkout
without invalidating other maple-gpui worktrees, run:

```bash
just clean-local
```

Release builds use fat LTO and one codegen unit. Use a release build for any
performance check; the dev profile is `opt-level = 1`.

The `justfile` has recipes for the common tasks. Install
[just](https://github.com/casey/just) and run `just` to list them:

```sh
just ci          # all the checks that CI runs
just debug-app   # stable macOS debug app for privacy-permission testing
just release     # release binary in target/release
just dist        # release binary copied to dist/ with a SHA-256
just run         # debug build with debug logs
just clean-local # this checkout's Cargo artifacts only (keeps the shared cache)
```

## Command line

```
maple-gpui                 Open the desktop app.
maple-gpui acp             Serve the Agent Client Protocol on stdio.
maple-gpui proxy [FLAGS]   Serve an OpenAI-compatible HTTP endpoint.
maple-gpui --version       Print the version.
```

### `maple-gpui acp`

Runs a standalone ACP agent over stdio for editors and ACP clients. It
reuses the sign-in saved by the desktop app and hosts its own runtime, so
the desktop app does not need to run. Logs go to the log file only; stdout
is the ACP channel. If no sign-in is saved, it exits with a message that
tells the user to sign in from the desktop app first.

### `maple-gpui proxy`

```
--host HOST     bind address (default 127.0.0.1, env MAPLE_PROXY_HOST)
--port PORT     bind port (default 8080, env MAPLE_PORT)
--api-key KEY   Maple API key for requests without an Authorization header
                (env MAPLE_API_KEY); not allowed together with --cors
--cors          allow browser origins; every request must then carry its
                own key (env MAPLE_ENABLE_CORS=1)
```

Without `--cors`, the proxy rejects requests that carry browser-only headers
(`Origin`, `Sec-Fetch-Site`) so a web page cannot spend a saved key through
loopback. With `--cors`, a default key is refused for the same reason.

## Build features

The default build has every mode. Cargo features turn modes off, so a
server or CI machine can build the `acp` or `proxy` mode without the gpui
window and its display libraries:

| Feature | What it adds |
| --- | --- |
| `desktop` | The gpui window. Without it the binary is headless. |
| `acp` | `maple-gpui acp` and `maple_agent::acp`. |
| `proxy` | `maple-gpui proxy`. |

```sh
cargo build --release -p maple-gpui --no-default-features --features acp
cargo build --release -p maple-gpui --no-default-features --features proxy
```

A mode that is compiled out exits with status 2 and a message that names
the missing feature.

## Configuration

All settings are environment variables. None are required.

| Variable | Purpose | Default |
| --- | --- | --- |
| `MAPLE_API_URL` | OpenSecret backend. Use `http://127.0.0.1:3000` for a local dev backend. | `https://enclave.trymaple.ai` |
| `MAPLE_BILLING_API_URL` | Maple billing API. | `https://billing.opensecret.cloud` |
| `MAPLE_CLIENT_ID` | OpenSecret client id (UUID). | Maple's id |
| `MAPLE_MODEL` | Model to select at start. | Runtime default |
| `MAPLE_PERMISSION_MODE` | `smart_approve` or `auto`. Overrides the saved setting. | Saved setting |
| `MAPLE_CONTEXT_LIMIT` | Context window size in tokens, when the model catalog does not report one. | Catalog value |
| `GOOSE_SHELL` | Shell for the agent's shell tool. | `bash` (Windows: `cmd`) |
| `MAPLE_UPDATE_REPO` | GitHub `owner/repo` whose releases the launch check reads. | `benthecarman/maple-gpui` |
| `MAPLE_DISABLE_UPDATE_CHECK` | `1` turns the release check off. | unset |
| `RUST_LOG` | Log filter. | `info,goose=warn` |

### File locations

The roots follow the platform, the same way the Tauri app's
`app_config_dir` and `app_local_data_dir` do. `XDG_CONFIG_HOME` and
`XDG_DATA_HOME` override them on every platform.

| Root | Linux | macOS | Windows |
| --- | --- | --- | --- |
| Config | `~/.config/maple-gpui/` | `~/Library/Application Support/maple-gpui/` | `%APPDATA%\maple-gpui\` |
| Local data | `~/.local/share/maple-gpui/` | `~/Library/Application Support/maple-gpui/` | `%LOCALAPPDATA%\maple-gpui\` |

| Path | Content |
| --- | --- |
| `<config>/settings.json` | App settings. |
| `<config>/agent/accounts/<scope>/config.json` | Per-account agent configuration (default root, model, custom MCP servers, project trust). May roam between machines. |
| `<config>/agent/accounts/<scope>/goose/config/` | Goose permission file for the account. |
| `<config>/agent/goose-runtime/` | Goose process configuration. |
| `<local data>/auth.json` | Sign-in credentials (mode 0600). Device-local; never in a roaming profile. |
| `<local data>/agent/accounts/<scope>/integrations.json` | Per-account defaults and validated launch details for integrations detected on this device. |
| `<local data>/agent/accounts/<scope>/goose/data/sessions/sessions.db` | Goose session history and usage ledger (SQLite, WAL). |
| `<local data>/agent/accounts/<scope>/tool_summaries.db` | Model-written one-line summaries of tool calls (SQLite, WAL). |
| `<local data>/agent/accounts/<scope>/attachments/` | Image attachments. |
| `<local data>/agent/acp/accounts/<scope>/config.json` | ACP configuration. |
| `<local data>/logs/maple-gpui.log` | Log file. Panics are logged here too. |

`<scope>` is the SHA-256 of the account's user id. Small JSON files are
written atomically (temp file, sync, rename) with owner-only permissions.

These directories are separate from the Tauri app's directories. The two
apps must not share Goose session storage.

## Tests

```sh
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
cargo fmt --all -- --check
```

CI runs the same three commands on Linux, macOS, and Windows. It also
builds and tests the headless feature sets and a Linux release build.
`just ci` runs the whole set locally. A `v*` tag builds release binaries
for all three platforms and attaches them to a GitHub release.

## Before a release

- `app/src/update.rs` has the default release repository
  (`DEFAULT_REPO`). Point it at the repository that publishes the
  binaries, or set `MAPLE_UPDATE_REPO` at run time.

## Dependencies to watch

`Cargo.toml` patches `opensecret` to a git branch that adds a root workspace
manifest around the in-tree Rust SDK, so the `get_model_catalog()` method is
reachable. Drop the patch when the SDK publishes that method to crates.io.

## License

MIT. See `LICENSE`.
