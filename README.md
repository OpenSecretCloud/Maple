# maple-gpui

Native Maple desktop app: the Maple Agent Mode flow rebuilt in
[gpui](https://crates.io/crates/gpui) over the unchanged Maple agent runtime.

Scope: Desktop Agent Mode only. ACP, proxy, and daemon work stay out of the
GUI; see "Architecture" below for where they will attach.

## Layout

```
crates/maple-agent/   Backend: Maple's transport-neutral agent runtime,
                      extracted from ../Maple's src-tauri with Tauri removed.
                      Owns embedded Goose, the Maple provider over the
                      OpenSecret SDK, developer tools, permission policy,
                      and account-scoped session storage.
app/                  Frontend: gpui binary. Owns the window, login screen,
                      chat UI, and the backend adapter.
docs/                 Reference material: theme spec, flow checklist, gpui
                      cheat sheet, and the review reports that shaped the
                      current code.
```

### Backend / frontend boundary

`app/src/backend.rs` is the only file that imports `maple_agent`. It owns a
private Tokio runtime, exposes an async facade (`AgentBackend`) plus one
event stream, and converts nothing else. The UI talks to that facade only.
This mirrors Maple's own edge-adapter pattern (`agent_tauri.rs` and
`agent_acp.rs` are sibling adapters over `MapleAgentService`), so a future
process split — GUI talking to a CLI/daemon over a socket, or ACP/proxy as
separate pieces — replaces the facade without touching UI code.

The runtime itself was copied from `../Maple/frontend/src-tauri/src`
(`agent.rs`, `agent/*`, `maple_api.rs`, `open_secret_config.rs`) and changed
only to remove Tauri:

- `tauri::http` types → the `http` crate (`agent/provider.rs`)
- Tauri event/command adapters dropped; auth state and the agent event sink
  are injectable traits
- public visibility opened on the service surface the app consumes

Goose is pinned to the same aaif-goose fork revision as Maple. The port was
diff-audited file by file; the runtime passes its full 290-test suite.

## Prerequisites

- Rust 1.94+ (stable)
- Linux: `libxkbcommon-dev`, `libxkbcommon-x11-dev`, `libfontconfig-dev`,
  `libfreetype-dev`, and a Vulkan loader (any ICD; Lavapipe works for
  headless testing). Debian/Ubuntu:

  ```sh
  sudo apt install libxkbcommon-dev libxkbcommon-x11-dev \
       libfontconfig-dev libfreetype-dev mesa-vulkan-drivers
  ```

## Running

```bash
cargo run -p maple-gpui
```

Configuration:

- `MAPLE_API_URL` — OpenSecret backend. Defaults to
  `https://enclave.trymaple.ai`; point it at `http://127.0.0.1:3000` for a
  local OpenSecret dev backend.
- The agent's project root is the working directory you launch from.
- App data lives under `~/.config/maple-gpui` and `~/.local/share/maple-gpui`
  (separate from the Tauri app's directories; the two apps must not share
  Goose session storage).

Sign in with your Maple email and password (OpenSecret `/login`). Tokens are
kept in memory only — Sign out or quit drops them, and you sign in again on
the next launch.

## Headless QA

The `app/examples/` directory contains the tooling used to verify the UI
without a display:

- `gpui_hello` — upstream gpui example used as a rendering control.
- `xsend_input` — XTEST input injector (clicks and ASCII typing) for
  driving the app under `Xvfb`.

Typical session:

```sh
Xvfb :99 -screen 0 1440x900x24 &
DISPLAY=:99 VK_ICD_FILENAMES=/usr/share/vulkan/icd.d/lvp_icd.json \
  cargo run -p maple-gpui &
DISPLAY=:99 ./target/debug/examples/xsend_input click 728 572
DISPLAY=:99 import -window root /tmp/shot.png
```

## Tests

```bash
cargo test -p maple-agent   # the ported runtime suite from Maple (290 tests)
cargo check --workspace
```
