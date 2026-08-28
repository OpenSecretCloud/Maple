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
scripts/              Developer helpers (screenshot on GNOME Wayland).
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
  the `acp` mode skip sign-in.
- Agent chat with streaming Markdown, tool calls, permission prompts,
  agent questions, image attachments (picker, paste, or drag and drop),
  a per-message Copy button, and a context-window indicator.
- Message queue: Enter during a run queues the message for the next turn,
  Ctrl+Enter (Cmd+Enter) steers it into the current turn. Queued messages
  can be sent now, edited in the composer (the message keeps its place in
  the queue), or removed.
- Sidebar search filters tasks and projects by name; Escape clears it.
- Up and Down in an empty composer recall prompts sent in this window.
- Projects (working directories) with pinned and recent roots, rename,
  open in the file manager, and remove. Projects that provide skills ask
  for a trust decision before their guidance loads.
- Sessions grouped by project, with rename, archive, and restore.
- Settings: general (permission mode, web tools, tool details,
  notifications, appearance), system prompt, MCP servers, usage totals
  from the Goose ledger, and about.
- Dark and light themes; the default follows the system.
- Billing status from the Maple billing API.
- Desktop notifications when the agent needs a decision.
- Release check on launch: a banner links to a newer GitHub release.
  Nothing is downloaded or installed by the app.
- Window size and maximized state persist between launches.

## Prerequisites

- Rust stable (edition 2024; 1.94 or newer)
- Linux: `libxkbcommon-dev`, `libxkbcommon-x11-dev`, `libfontconfig-dev`,
  `libfreetype-dev`, `libclang-dev`, `cmake`, and a Vulkan loader.
  Debian/Ubuntu:

  ```sh
  sudo apt install libxkbcommon-dev libxkbcommon-x11-dev \
       libfontconfig-dev libfreetype-dev libclang-dev cmake \
       mesa-vulkan-drivers
  ```

- macOS and Windows: the Rust toolchain only.

## Build and run

```sh
cargo run -p maple-gpui              # desktop app, dev profile
cargo build --release -p maple-gpui  # release binary in target/release
```

Release builds use fat LTO and one codegen unit. Use a release build for any
performance check; the dev profile is `opt-level = 1`.

The `justfile` has recipes for the common tasks. Install
[just](https://github.com/casey/just) and run `just` to list them:

```sh
just ci        # all the checks that CI runs
just release   # release binary in target/release
just dist      # release binary copied to dist/ with a SHA-256
just run       # debug build with debug logs
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
--port PORT     bind port (default 8080, env MAPLE_PROXY_PORT)
--api-key KEY   Maple API key for requests without an Authorization header
                (env MAPLE_API_KEY); not allowed together with --cors
--cors          allow browser origins; every request must then carry its
                own key
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
| `RUST_LOG` | Log filter. | `info` |

### File locations

| Path | Content |
| --- | --- |
| `$XDG_CONFIG_HOME/maple-gpui/` (`~/.config/maple-gpui/`) | `auth.json`, `settings.json`, and per-account Goose state under `agent/accounts/<scope>/` |
| `$XDG_DATA_HOME/maple-gpui/logs/maple-gpui.log` (`~/.local/share/maple-gpui/logs/`) | Log file. Panics are logged here too. |

These directories are separate from the Tauri app's directories. The two
apps must not share Goose session storage.

## Tests

```sh
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
cargo fmt --all -- --check
```

CI runs the same three commands on Linux, macOS, and Windows, plus a Linux
release build. `just ci` runs the checks locally. A `v*` tag builds release
binaries for all three platforms and attaches them to a GitHub release.

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
