# maple-gpui

GPUI desktop app for Maple. Workspace crates: `app` (binary `maple-gpui`),
`crates/maple-agent`, `crates/maple-billing`. See `README.md` for the
layout, prerequisites, and command line modes.

## Build, test, run

`just` lists the recipes. `just ci` runs the same checks as CI (format,
clippy with `-D warnings` for every feature set, tests). Run it before a
commit.

```sh
just build     # debug binary
just run       # debug binary with RUST_LOG=warn,maple_gpui=debug
just release   # release binary (fat LTO, one codegen unit)
just headless  # acp and proxy modes only, no window
```

To stop a running app, use `pkill -x maple-gpui` (exact process name).
`pkill -f` with the binary path also matches the shell that runs the
command and kills it.

## Logs and freezes

The app logs to stderr and to `~/.local/share/maple-gpui/logs/maple-gpui.log`
(level `info` by default; `RUST_LOG=debug` for more). Panics are logged
there too. Tail it with `tail -f ~/.local/share/maple-gpui/logs/maple-gpui.log`.

If the app freezes, dump all thread backtraces while it is still hung:

```sh
gdb -p "$(pgrep -x maple-gpui)" -batch -ex "thread apply all bt" > /tmp/maple-hang.txt 2>&1
```

The default release profile strips symbols. `just release-debug` builds a
release binary that keeps line tables for usable backtraces.

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
- Release builds use fat LTO and one codegen unit. Use release builds
  for any performance check; the dev profile is `opt-level = 1`.
