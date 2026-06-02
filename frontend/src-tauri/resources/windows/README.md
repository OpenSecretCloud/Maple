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

The Windows CI workflows (`desktop-pr-build.yml`, `desktop-build.yml`) stage
these automatically in the **"Stage Windows runtime DLLs for bundling"** step,
which runs after "Provide ONNX Runtime (Windows)" and before the Tauri build.

For a **local** Windows build, run `scripts/provide-windows-onnxruntime.sh`
first (it exports `ORT_DYLIB_PATH`), then stage the same five files:

1. **`onnxruntime.dll`** — from the SHA-verified ONNX Runtime download:

   ```bash
   cp "$ORT_DYLIB_PATH" frontend/src-tauri/resources/windows/onnxruntime.dll
   ```

2. **The 4 MSVC CRT DLLs** — from the Visual Studio redist on the machine,
   located via `vswhere` so it's independent of the VS year/edition (the
   `^Microsoft\.VC\d+\.CRT$` filter avoids the neighbouring `DebugCRT`/`OPENMP`
   folders); falls back to `System32`:

   ```powershell
   $crtDlls = 'VCRUNTIME140.dll','VCRUNTIME140_1.dll','MSVCP140.dll','MSVCP140_1.dll'
   $candidates = @()
   $vswhere = "${env:ProgramFiles(x86)}\Microsoft Visual Studio\Installer\vswhere.exe"
   if (Test-Path $vswhere) {
     $vs = & $vswhere -latest -products * -property installationPath
     if ($vs) {
       $candidates += Get-ChildItem (Join-Path $vs 'VC\Redist\MSVC\*\x64') -Directory -ErrorAction SilentlyContinue |
                      Where-Object { $_.Name -match '^Microsoft\.VC\d+\.CRT$' } | ForEach-Object FullName
     }
   }
   $candidates += "$env:WINDIR\System32"
   $src = $candidates | Where-Object { $d = $_; -not ($crtDlls | Where-Object { -not (Test-Path (Join-Path $d $_)) }) } | Select-Object -First 1
   $crtDlls | ForEach-Object { Copy-Item (Join-Path $src $_) frontend\src-tauri\resources\windows\ }
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
