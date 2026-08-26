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

Screenshots capture the full desktop. The sidebar plan card is in the bottom
left of the app window.

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
