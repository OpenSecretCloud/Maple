# Windows development

End-to-end guide for building and running Maple on Windows 10/11 (x64 or
ARM64). Goal: clone → setup → first build in ~30 minutes on a clean VM.

> Most ARM64 dev was done on Apple Silicon Parallels VMs (Win11 ARM64).
> Snapdragon X laptops use the same toolchain. Native x64 is also
> supported.

## Prerequisites

A fresh Win10/11 install needs three things: the right Visual Studio Build
Tools components, a Rust toolchain pinned to the same version CI uses, and
a `frontend/.env.local` with the backend URL. The bootstrap script handles
all of it.

```powershell
# In repo root, from an elevated PowerShell (Administrator):
powershell -ExecutionPolicy Bypass -File scripts/setup-windows.ps1
```

The script is idempotent — safe to re-run if something fails mid-way or you
want to verify state.

What it installs:

- **VS Build Tools 2022** with the exact components the Rust/Tauri chain
  needs: `VC.Tools.x86.x64`, `VC.Tools.ARM64`, `VC.Llvm.Clang`,
  `Windows11SDK.22621`. The Clang component is the one most likely to be
  forgotten; `ring` 0.17 on `aarch64-pc-windows-msvc` needs clang for
  ARM64 assembly and `cl.exe` alone can't compile it.
- **Node.js LTS** via winget (`bun` has no Windows ARM64 binary; the
  `tauri.windows.conf.json` overlay swaps `bun` for `npm` automatically).
- **rustup** + Rust toolchain pinned to **1.95.0** (matches the CI pin in
  `.github/workflows/desktop-build.yml`). Override with
  `-RustToolchain 1.96.0` if you need a different version.
- **VC++ 2015+ Redistributable** for both x64 and ARM64 (rollup's native
  module links against the ARM64 redist on ARM hosts).
- **LLVM/Clang** standalone as a backstop for the VS Clang component.
- **Git for Windows** — `git.exe` for the clone, plus Git Bash
  (`bash.exe` + the bundled `curl` / `sha256sum` / `unzip` / `cygpath`
  unix tools that `scripts/tauri-windows.ps1` and the ONNX Runtime helper
  both rely on).
- **just** — the recipe runner for `just windows-build` / `just windows-dev`.
- A `frontend/.env.local` template pointing at the production enclave
  (see [.env.local handling](#envlocal-handling) below).

After setup, **open a new PowerShell** so PATH picks up the new tools.
Sanity check:

```powershell
rustc --version       # rustc 1.95.0
node --version        # v22.x.x or later (LTS)
cargo --version       # cargo 1.95.0
```

## First build

The build dance (vcvarsall + cargo target dir + ORT setup) is wrapped in
two `just` recipes that hide all of it:

```powershell
# Native ARM64 build (default; produces an ARM64 NSIS installer):
just windows-build

# Cross-build to x64 from an ARM64 host:
just windows-build arm64_amd64

# Native x64 build:
just windows-build x64
```

The recipe resolves to `scripts/tauri-windows.ps1`, which:

1. Locates `vcvarsall.bat` via `vswhere`.
2. Sets `CARGO_TARGET_DIR` to `%USERPROFILE%\maple-cargo-target` — off the
   source tree so Parallels shared-folder writes don't corrupt cargo
   metadata. Override with `-CargoTargetDir`.
3. Runs `frontend/src-tauri/scripts/provide-windows-onnxruntime.sh`
   through Git Bash to fetch + SHA-verify ONNX Runtime, then imports the
   `ORT_LIB_LOCATION` / `ORT_SKIP_DOWNLOAD` / `ORT_DYLIB_PATH` env vars.
   Mirrors how `desktop-build.yml` feeds those into `$GITHUB_ENV`. Skip
   with `-SkipOrt` to fall back to the `ort` crate's auto-download.
4. Chains `vcvarsall.bat <arch>` and the `tauri build` invocation in a
   single `cmd /c` so the MSVC env survives into the build.
5. Applies the `tauri.windows.conf.json` overlay so `npm` replaces `bun`
   in `beforeBuildCommand` / `beforeDevCommand`.

The built `.exe` lands under `%USERPROFILE%\maple-cargo-target\release\bundle\nsis\`.

## Iteration loop

Use `just windows-dev` for the hot-reload loop — same arch handling, runs
`tauri dev` instead of `tauri build`. Vite reloads frontend changes
instantly; Rust changes trigger an incremental cargo rebuild.

```powershell
just windows-dev
```

Major iteration unlock vs. rebuilding the full installer every change.

## `.env.local` handling

Vite reads `frontend/.env.local` at **build time** and bakes the values
into the bundle. A missing file produces a silent white-screen on launch
(captured for ~30 min of devtools debugging during PR 1 manual smoke).

The bootstrap script creates `frontend/.env.local` automatically if absent.
For non-Windows hosts (or if you want a non-prod backend), copy the
template:

```bash
cp frontend/.env.local.template frontend/.env.local
# edit if pointing at a non-prod backend
```

> Reminder: Vite bakes env vars at build time. **Restart the dev server
> after editing `.env.local`** or values won't propagate.

## Gotchas

| Gotcha | Why it bites | Fix |
|---|---|---|
| `bun` doesn't ship a Windows ARM64 binary | `beforeBuildCommand: "bun run build"` fails on ARM hosts | `tauri.windows.conf.json` overlay swaps in `npm` (auto-applied by `just windows-*`) |
| `ring` 0.17 build fails on `aarch64-pc-windows-msvc` with `cl.exe` errors | needs clang for ARM64 asm; `cl.exe` can't compile it | Install `VC.Llvm.Clang` component (setup script does it) |
| `aws-lc-sys` build fails | same root cause | same fix |
| Cargo target dir corruption on Parallels VM | shared-folder writes don't play with cargo metadata | `tauri-windows.ps1` points `CARGO_TARGET_DIR` at `%USERPROFILE%\maple-cargo-target` |
| Silent white-screen on launch | empty / missing `frontend/.env.local` | Copy `frontend/.env.local.template` |
| `just` errors with `Error parsing line: '﻿VITE_...'` at line index 0 | UTF-8 BOM at start of `frontend/.env.local` (PS 5.1's `Out-File` / `Set-Content` / `>` redirect and old Notepad all add one by default) | Re-run `scripts/setup-windows.ps1` — it strips the BOM in place; or save the file in VS Code (no BOM by default), or in PS 7+ use `Set-Content -Encoding utf8NoBOM` |
| `vcvarsall.bat arm64` vs `arm64_amd64` | wrong arch produces wrong-bitness binary that fails to load | Use `just windows-build arm64` for native, `arm64_amd64` to cross to x64 |
| `winget install --quiet --override` swallows exit codes for VS BuildTools modify | bootstrapper reports success even when components didn't land | Setup script uses `--passive` and verifies on disk via `vswhere` after install |

## CI vs local parity

Local `just windows-build` and the CI `build-windows` job
(`.github/workflows/desktop-build.yml` for master push,
`desktop-pr-build.yml` for PRs) share these pins:

- Rust **1.95.0** (`-RustToolchain` param in setup script;
  `dtolnay/rust-toolchain` action with `toolchain: 1.95.0` in CI)
- ONNX Runtime **1.22.0** with SHA-verified archive + DLL fetch via
  `frontend/src-tauri/scripts/provide-windows-onnxruntime.sh` (same
  helper for both)
- Tauri config: identical `tauri.conf.json` + Windows overlay applied via
  `--config`

CI builds an unsigned NSIS x64 installer. Local builds default to ARM64
native; pass `x64` for x64-native parity with CI artifacts.

## Code signing

Out of scope for now. Tracked under PR 7 (MPLR-goufxxvn) — Authenticode
signing requires certificate provisioning and is gated on a code-signing
cert being purchased.

## Native ARM64 shipping decision

CI currently produces x64 artifacts only. ARM64 hosts run those under
Microsoft's x64 emulation (works, slower than native). Whether to ship a
native ARM64 build alongside x64 is tracked as a spike under
MPLR-xhfehqft.
