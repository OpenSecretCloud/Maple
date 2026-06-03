# Windows bundled runtime DLLs

`maple.exe` links **ONNX Runtime 1.22.0** with a load-time (compile-time
dynamic) import of `onnxruntime.dll`. On a clean Windows install, the Windows
loader would otherwise bind that import to the OS-shipped Windows-ML
`C:\Windows\System32\onnxruntime.dll` (a forwarder to `onnxruntime_x64.dll`,
currently **v1.17.x**). The version mismatch lets the app launch but **hangs**
the first time TTS calls `Session::builder()` (see `src/tts.rs`).

To force the correct runtime, we ship these DLLs **next to `maple.exe`**. The
executable's own directory is searched **before** `System32` in the Windows DLL
search order, so our 1.22.0 `onnxruntime.dll` wins over the OS one.

`onnxruntime.dll` 1.22.0 in turn depends on the MSVC C++ runtime
(`VCRUNTIME140.dll`, `VCRUNTIME140_1.dll`, `MSVCP140.dll`, `MSVCP140_1.dll`),
which is **not** present on a fresh Windows install. Rather than running
`vc_redist.x64.exe` (which needs admin/UAC and clashes with our per-user
install), we deploy those four redistributable DLLs **app-local** — the same
mechanism as `onnxruntime.dll`. (`maple.exe` itself only needs the Universal
CRT `api-ms-win-crt-*`, which is part of Windows 10+.)

Deployment is done by an NSIS install hook, `install-dlls.nsh`, registered via
`bundle.windows.nsis.installerHooks` in `tauri.windows.conf.json`. Its
`NSIS_HOOK_POSTINSTALL` `File`-copies each DLL from here into `$INSTDIR` (next
to `maple.exe`); `NSIS_HOOK_PREUNINSTALL` removes them again.

We do **not** use `bundle.resources` for this: in Tauri 2.11 a `resources` map
supplied via the platform config gets the DLLs into the installer payload but
not into the NSIS install file-list, so they ride along in the `.exe` yet never
land in the install dir (verified empirically — only `maple.exe` +
`uninstall.exe` were installed). The hook bypasses that plumbing.

## Files (5)

| File                  | Source                                              |
|-----------------------|-----------------------------------------------------|
| `onnxruntime.dll`     | Microsoft ONNX Runtime 1.22.0 win-x64 release        |
| `VCRUNTIME140.dll`    | MSVC 2015–2022 x64 redistributable                   |
| `VCRUNTIME140_1.dll`  | MSVC 2015–2022 x64 redistributable                   |
| `MSVCP140.dll`        | MSVC 2015–2022 x64 redistributable                   |
| `MSVCP140_1.dll`      | MSVC 2015–2022 x64 redistributable                   |

These are gitignored (large redistributables with their own provenance) and
must be staged here **before `bun tauri build`** on Windows.

## CI staging

The Windows PR workflow stages these automatically through
`scripts/ci/desktop-windows-pr.sh` before the Tauri build. That script uses:

- SHA-verified ONNX Runtime from `scripts/provide-windows-onnxruntime.sh`.
- A SHA-verified, versioned Microsoft `VC_redist.x64.exe` URL pinned in
  `frontend/src-tauri/scripts/onnxruntime-pins.sh`.
- A SHA-verified WiX CLI NuGet package, used only to extract the VC++ redist
  bootstrapper payload reproducibly.

The build emits `target/reproducibility/desktop-pr-windows-*.sha256` proof
manifests, and CI verifies those manifests from uploaded artifacts.

For a **local** Windows build, run `scripts/provide-windows-onnxruntime.sh`
first (it exports `ORT_DYLIB_PATH`), then run the same staging helper:

```powershell
$env:MAPLE_WINDOWS_VC_REDIST_VERSION = "14.44.35211"
$env:MAPLE_WINDOWS_VC_REDIST_URL = "<pinned URL from frontend/src-tauri/scripts/onnxruntime-pins.sh>"
$env:MAPLE_WINDOWS_VC_REDIST_SHA256 = "<pinned SHA-256 from frontend/src-tauri/scripts/onnxruntime-pins.sh>"
.\frontend\src-tauri\scripts\stage-windows-runtime-dlls.ps1 `
  -OrtDllPath "$env:ORT_DYLIB_PATH" `
  -Destination .\frontend\src-tauri\resources\windows
```

The hook reads these files at makensis compile time, so they must be present
here before the build runs (the CI staging step guarantees that); a missing
file fails the build loudly rather than shipping a broken installer.

## Verify after a CI build

Confirm the DLLs land **next to `maple.exe`** in the installed app
(`%LOCALAPPDATA%\Maple\`), not in a `resources\` subfolder — only then does the
search-order override work. The running process should load
`...\AppData\Local\Maple\onnxruntime.dll  ver 1.22.0` (check via
`(Get-Process maple).Modules`), not the System32 copy.
