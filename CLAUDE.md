# maple-gpui

GPUI desktop app for Maple. Workspace crates: `app` (binary `maple-gpui`),
`crates/maple-agent`, `crates/maple-billing`.

## Run the app from the shell

The dev machine runs GNOME on Wayland. The shell that runs Claude Code has no
display variables, so set them:

```sh
export WAYLAND_DISPLAY=wayland-0 XDG_RUNTIME_DIR=/run/user/$(id -u) \
  DBUS_SESSION_BUS_ADDRESS=unix:path=/run/user/$(id -u)/bus
cargo build -p maple-gpui
RUST_LOG=warn,maple_gpui=debug ./target/debug/maple-gpui 2>/tmp/maple-run.log &
```

Wait for the window with `timeout 12 tail -f /dev/null` (not `sleep`).

To stop the app, use `pkill -x maple-gpui` (exact process name). Do not use
`pkill -f` or `pgrep -f` with the binary path: the pattern also matches the
shell that runs the command and kills it (exit code 144).

## Take a screenshot

Use the xdg desktop portal. Other tools do not work on this machine:
`grim` (no wlr-screencopy on GNOME), `import -window root` (X11 auth is
rejected), `gdbus ... org.gnome.Shell.Screenshot` (GNOME 41+ allow-list),
and `gnome-screenshot` (falls back to X11 and hangs).

```sh
scripts/screenshot.py /tmp/shot.png
```

Then read `/tmp/shot.png`. The first request opens a permission dialog on the
desktop; the user must click Share. A second request while one is pending
times out, so wait for the first to finish or be cancelled.

Screenshots capture the full desktop. The app window contains the sidebar
and chat pane.

## Ship a test build

When a screenshot is not possible, ship a release build for the user to test:

```sh
cargo build --release -p maple-gpui
name=maple-gpui-$(grep -m1 '^version' Cargo.toml | cut -d'"' -f2)-$(git rev-parse --short HEAD)-linux-x86_64
upload-thing put target/release/maple-gpui --private --name "$name"
```

Upload the raw binary. Do not tar or zip it. Add `-dirty` to the name when
the tree has uncommitted changes. Give the user the URL and the SHA-256.

## Logs and freezes

The app logs to stderr and to `~/.local/share/maple-gpui/logs/maple-gpui.log`
(level `info` by default; `RUST_LOG=debug` for more). Panics are logged
there too. Tail it with `tail -f ~/.local/share/maple-gpui/logs/maple-gpui.log`.

If the app freezes, dump all thread backtraces while it is still hung:

```sh
gdb -p "$(pgrep -x maple-gpui)" -batch -ex "thread apply all bt" > /tmp/maple-hang.txt 2>&1
```

The default release profile strips symbols. For a test build with usable
backtraces:

```sh
CARGO_PROFILE_RELEASE_STRIP=false CARGO_PROFILE_RELEASE_DEBUG=line-tables-only \
  cargo build --release -p maple-gpui
```

## Performance

This app must feel instant. Treat frame time and UI-thread stalls as bugs.

- Render functions run on every `cx.notify()`. Do not parse, sort, group,
  or clone collections inside `render_*`. Precompute when state changes
  (see `rebuild_project_groups`, `MarkdownCache`) and read it in render.
- The transcript renders through `gpui::list` with `ListState`. Keep it
  that way: never emit all timeline items as plain children. When an item
  changes in place, call `list_state.splice(ix..ix + 1, 1)` so its cached
  height is re-measured — except the newest item, which must not be
  spliced on streaming updates. A splice marks the item unmeasured (0 px)
  until the next paint; a wheel event in that window clamps back to the
  bottom and re-pins the view, so streaming would block scrolling up
  (regression test: `test_streaming_chunk_keeps_wheel_scrolling_up`).
- Never block the UI thread. File dialogs, file reads, and SQLite go
  through `tokio::task::spawn_blocking` or `AgentBackend::spawn`.
- Batch events. The backend pump drains the channel and applies a batch
  in one update; `apply_service_event` returns whether anything visible
  changed so a batch with no visible change does not re-render.
- Poll only when needed, and only `cx.notify()` when a value changed.
  Prefer pushed events over timers.
- Reuse handles: entities (`cx.new`), SQLite connections, and shaped text
  are created once and cached, not per frame or per call.
- Release builds use fat LTO and one codegen unit. Ship release builds
  for any performance check; the dev profile is `opt-level = 1`.
