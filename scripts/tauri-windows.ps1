#Requires -Version 5.1
<#
.SYNOPSIS
  One-command wrapper for `tauri build` / `tauri dev` on Windows.

.DESCRIPTION
  Handles the vcvarsall.bat + ORT env-var dance so the developer doesn't
  need a Developer PowerShell open, doesn't need to remember the ORT helper,
  and doesn't blow up on cargo target dir when the repo lives on a Parallels
  shared folder.

  Steps:
    1. Locate VS Build Tools install via vswhere.
    2. Point CARGO_TARGET_DIR at %USERPROFILE%\maple-cargo-target (overridable
       via -CargoTargetDir or $env:CARGO_TARGET_DIR) -- Parallels shared-folder
       writes don't play with cargo's default target directory.
    3. Source frontend/src-tauri/scripts/provide-windows-onnxruntime.sh via Git
       Bash to fetch + SHA-verify ONNX Runtime, then import the script's
       ORT_LIB_LOCATION / ORT_SKIP_DOWNLOAD / ORT_DYLIB_PATH outputs as
       process env vars (mirrors how desktop-build.yml + desktop-pr-build.yml
       feed those into $GITHUB_ENV).
    4. cmd /c chain vcvarsall.bat <arch> with the tauri command, applying the
       frontend/src-tauri/tauri.windows.conf.json overlay so the Windows-only
       knobs (bun -> npm for beforeBuildCommand / beforeDevCommand) take
       effect.

.PARAMETER Command
  'build' or 'dev'.

.PARAMETER Arch
  vcvarsall arch argument. Common values:
    arm64        -- native ARM64 build on ARM64 host (default)
    arm64_amd64  -- cross-build to x64 from ARM64 host
    x64          -- native x64 build on x64 host

.PARAMETER CargoTargetDir
  Override CARGO_TARGET_DIR. Defaults to %USERPROFILE%\maple-cargo-target.

.PARAMETER SkipOrt
  Skip the ONNX Runtime setup step. The ort crate will fall back to its
  auto-download (slower, unpinned) on the first build.

.EXAMPLE
  powershell -ExecutionPolicy Bypass -File scripts/tauri-windows.ps1 -Command build -Arch arm64

.EXAMPLE
  powershell -ExecutionPolicy Bypass -File scripts/tauri-windows.ps1 -Command dev -Arch arm64_amd64
#>
[CmdletBinding()]
param(
    [Parameter(Mandatory)][ValidateSet('build', 'dev')][string]$Command,
    [string]$Arch = 'arm64',
    [string]$CargoTargetDir = (Join-Path $env:USERPROFILE 'maple-cargo-target'),
    [switch]$SkipOrt
)

$ErrorActionPreference = 'Stop'
$ProgressPreference    = 'SilentlyContinue'
Set-StrictMode -Version Latest

function Write-Section { param([string]$M) Write-Host ""; Write-Host "=== $M ===" -ForegroundColor Cyan }
function Write-Step    { param([string]$M) Write-Host "[..]  $M" -ForegroundColor Yellow }
function Write-Ok      { param([string]$M) Write-Host "[OK]  $M" -ForegroundColor Green }

$RepoRoot     = Split-Path -Parent $PSScriptRoot
$FrontendDir  = Join-Path $RepoRoot 'frontend'
$OrtScript    = Join-Path $RepoRoot 'frontend\src-tauri\scripts\provide-windows-onnxruntime.sh'
$WinOverlay   = 'src-tauri/tauri.windows.conf.json'  # path is relative to frontend/ (tauri --config base)

# ---------- vcvarsall.bat ----------
Write-Section "Locating Visual Studio Build Tools"
$vswhere = Join-Path ${env:ProgramFiles(x86)} 'Microsoft Visual Studio\Installer\vswhere.exe'
if (-not (Test-Path $vswhere)) {
    throw "vswhere.exe not found at $vswhere. Run scripts/setup-windows.ps1 to install VS Build Tools."
}
$vsInstallPath = (& $vswhere -latest -products * `
    -requires Microsoft.VisualStudio.Workload.VCTools `
    -property installationPath 2>$null | Select-Object -First 1)
if ([string]::IsNullOrWhiteSpace($vsInstallPath)) {
    throw "No VS install with VCTools workload found. Run scripts/setup-windows.ps1."
}
$vcvarsall = Join-Path $vsInstallPath 'VC\Auxiliary\Build\vcvarsall.bat'
if (-not (Test-Path $vcvarsall)) {
    throw "vcvarsall.bat not found under $vsInstallPath."
}
Write-Ok "vcvarsall.bat: $vcvarsall"

# ---------- CARGO_TARGET_DIR ----------
if (-not (Test-Path $CargoTargetDir)) {
    New-Item -ItemType Directory -Path $CargoTargetDir -Force | Out-Null
}
$env:CARGO_TARGET_DIR = $CargoTargetDir
Write-Ok "CARGO_TARGET_DIR=$CargoTargetDir"

# ---------- ONNX Runtime ----------
if ($SkipOrt) {
    Write-Step 'Skipping ONNX Runtime setup (-SkipOrt). The ort crate will auto-download.'
} else {
    Write-Section "Provisioning ONNX Runtime"
    $bashCandidates = @(
        (Join-Path $env:ProgramFiles 'Git\bin\bash.exe'),
        (Join-Path ${env:ProgramFiles(x86)} 'Git\bin\bash.exe')
    )
    $bashExe = $bashCandidates | Where-Object { Test-Path $_ } | Select-Object -First 1
    if (-not $bashExe) {
        throw "Git Bash not found (tried: $($bashCandidates -join ', ')). Install Git for Windows, or re-run with -SkipOrt to use the ort crate's auto-download."
    }
    if (-not (Test-Path $OrtScript)) {
        throw "ORT helper not found at $OrtScript."
    }
    # Map vcvarsall arch (host[_target]) to the *final binary* target arch
    # the ort crate needs to link against. arm64_amd64 = arm64 host cross to
    # amd64, so target is x64; amd64_arm64 = amd64 host cross to arm64.
    $ortTargetArch = if ($Arch -match '_amd64$' -or $Arch -in @('x64', 'amd64')) {
        'x64'
    } elseif ($Arch -match '_arm64$' -or $Arch -eq 'arm64') {
        'arm64'
    } else {
        Write-Warning "Could not map vcvarsall arch '$Arch' to an ORT target; defaulting to x64. Pass an explicit -Arch if this is wrong."
        'x64'
    }
    Write-Step "ORT_TARGET_ARCH=$ortTargetArch (derived from -Arch $Arch)"
    Write-Step "bash $OrtScript"
    $ortEnvLines = & $bashExe -c "ORT_TARGET_ARCH=$ortTargetArch ./frontend/src-tauri/scripts/provide-windows-onnxruntime.sh"
    if ($LASTEXITCODE -ne 0) {
        throw "provide-windows-onnxruntime.sh failed (exit $LASTEXITCODE)."
    }
    foreach ($line in $ortEnvLines) {
        if ($line -match '^([A-Z0-9_]+)=(.+)$') {
            [System.Environment]::SetEnvironmentVariable($Matches[1], $Matches[2], 'Process')
            Write-Ok "$($Matches[1])=$($Matches[2])"
        }
    }
}

# ---------- npm dependencies ----------
# Tauri's beforeDevCommand / beforeBuildCommand run `npm run dev` / `npm run
# build`, but neither tauri nor those scripts install npm deps. On a fresh
# clone `npx tauri` fails with "could not determine executable to run" because
# @tauri-apps/cli isn't in node_modules. Bootstrap if missing.
$NodeModules = Join-Path $FrontendDir 'node_modules'
if (-not (Test-Path $NodeModules)) {
    Write-Section "Installing npm dependencies (first run)"
    Push-Location $FrontendDir
    try {
        & npm install
        if ($LASTEXITCODE -ne 0) {
            throw "npm install failed (exit $LASTEXITCODE)."
        }
    } finally {
        Pop-Location
    }
    Write-Ok 'npm install complete'
} else {
    Write-Step "Skipping npm install (frontend/node_modules exists; run 'npm install' in frontend/ manually if package.json changed)"
}

# ---------- tauri command ----------
Write-Section "Running tauri $Command ($Arch)"
$tauriCmd = switch ($Command) {
    'build' { "npx tauri build --config $WinOverlay" }
    'dev'   { "npx tauri dev --config $WinOverlay" }
}

# vcvarsall sets MSVC env (INCLUDE / LIB / PATH bits) inside the cmd.exe that
# invokes it; those vars don't survive back into PowerShell. Chain everything
# in one cmd /c so the tauri build inherits the vcvars-set environment.
$chained = '"' + $vcvarsall + '" ' + $Arch + ' && ' + $tauriCmd
Write-Step "cmd /c $chained"

Push-Location $FrontendDir
try {
    & cmd /c $chained
    $exit = $LASTEXITCODE
} finally {
    Pop-Location
}

exit $exit
