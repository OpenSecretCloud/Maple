#Requires -Version 5.1
<#
.SYNOPSIS
  Install Windows prerequisites for building Maple.

.DESCRIPTION
  Idempotent bootstrap for a fresh Win10/11 (x64 or ARM64) dev machine.
  Installs Visual Studio Build Tools 2022 with the exact MSVC + Clang +
  Windows SDK components the Rust/Tauri toolchain needs (clang-cl is required
  by `ring` and `aws-lc-sys` on aarch64-pc-windows-msvc), plus Node.js LTS
  (bun has no Win-ARM binary), rustup, the VC++ runtime redistributables,
  standalone LLVM, Git for Windows (provides Git Bash, used by
  scripts/tauri-windows.ps1 to invoke the ONNX Runtime helper and by the
  helper itself for curl/sha256sum/unzip/cygpath), and just (justfile
  runner for the `just windows-build` / `just windows-dev` recipes).
  Detects a missing frontend/.env.local and writes a working template
  pointing at the production enclave URL.

  Safe to re-run. Every step checks for prior installation first.

.PARAMETER SkipVsBuildTools
  Skip the Visual Studio Build Tools step. Use when full VS or VS Build Tools
  is already installed and the required components have been verified by hand.

.PARAMETER VsInstallerArgs
  Extra args appended to vs_BuildTools.exe (e.g. '--quiet' for unattended
  CI runs that don't need the installer UI -- note the epic-1 finding that
  `--quiet` swallows exit codes on modify-existing-install paths, so the
  default here uses `--passive` instead).

.EXAMPLE
  powershell -ExecutionPolicy Bypass -File scripts/setup-windows.ps1
#>
[CmdletBinding()]
param(
    [switch]$SkipVsBuildTools,
    [string[]]$VsInstallerArgs = @(),
    # Keep in lockstep with the toolchain pinned in
    # .github/workflows/desktop-build.yml (dtolnay/rust-toolchain) so local
    # dev builds and CI builds use the same compiler.
    [string]$RustToolchain = '1.95.0'
)

$ErrorActionPreference = 'Stop'
$ProgressPreference    = 'SilentlyContinue'
Set-StrictMode -Version Latest

# ---------- helpers ----------
function Write-Section { param([string]$M) Write-Host ""; Write-Host "=== $M ===" -ForegroundColor Cyan }
function Write-Step    { param([string]$M) Write-Host "[..]  $M" -ForegroundColor Yellow }
function Write-Ok      { param([string]$M) Write-Host "[OK]  $M" -ForegroundColor Green }
function Write-Skip2   { param([string]$M) Write-Host "[--]  $M" -ForegroundColor DarkGray }
function Write-Warn2   { param([string]$M) Write-Host "[!!]  $M" -ForegroundColor Magenta }

function Test-IsAdmin {
    $id = [System.Security.Principal.WindowsIdentity]::GetCurrent()
    $pr = New-Object System.Security.Principal.WindowsPrincipal($id)
    return $pr.IsInRole([System.Security.Principal.WindowsBuiltInRole]::Administrator)
}

function Get-HostArch {
    switch ($env:PROCESSOR_ARCHITECTURE) {
        'AMD64' { return 'x64' }
        'ARM64' { return 'arm64' }
        'x86'   { return 'x86' }
        default { return $env:PROCESSOR_ARCHITECTURE }
    }
}

function Test-WingetAvailable {
    return [bool](Get-Command winget -ErrorAction SilentlyContinue)
}

function Test-WingetPackage {
    param([Parameter(Mandatory)][string]$Id)
    $null = & winget list --id $Id --exact --accept-source-agreements --disable-interactivity 2>$null
    return ($LASTEXITCODE -eq 0)
}

function Install-WingetPackage {
    param(
        [Parameter(Mandatory)][string]$Id,
        [Parameter(Mandatory)][string]$Description
    )
    Write-Step "winget: $Description ($Id)"
    if (Test-WingetPackage -Id $Id) {
        Write-Skip2 "$Description already installed"
        return
    }
    & winget install --id $Id --exact `
        --accept-package-agreements --accept-source-agreements `
        --disable-interactivity --silent
    if ($LASTEXITCODE -ne 0) {
        throw "winget install failed for $Id (exit $LASTEXITCODE)"
    }
    Write-Ok "$Description installed"
}

function Get-VsWherePath {
    $candidate = Join-Path ${env:ProgramFiles(x86)} 'Microsoft Visual Studio\Installer\vswhere.exe'
    if (Test-Path $candidate) { return $candidate }
    return $null
}

function Test-VsComponent {
    param([Parameter(Mandatory)][string]$Component)
    $vsw = Get-VsWherePath
    if (-not $vsw) { return $false }
    $found = & $vsw -products * -requires $Component -property installationPath 2>$null
    return -not [string]::IsNullOrWhiteSpace($found)
}

function Get-VsBuildToolsInstallPath {
    $vsw = Get-VsWherePath
    if (-not $vsw) { return $null }
    $path = & $vsw -products Microsoft.VisualStudio.Product.BuildTools `
        -property installationPath 2>$null | Select-Object -First 1
    if ([string]::IsNullOrWhiteSpace($path)) { return $null }
    return $path
}

function Install-VsBuildTools {
    param(
        [Parameter(Mandatory)][string[]]$Components,
        [string[]]$ExtraArgs = @()
    )

    $missing = @($Components | Where-Object { -not (Test-VsComponent $_) })
    if ($missing.Count -eq 0) {
        Write-Skip2 'VS Build Tools components already present'
        return
    }
    Write-Step ("VS Build Tools: {0} component(s) missing:" -f $missing.Count)
    $missing | ForEach-Object { Write-Host "       - $_" }

    $installer = Join-Path $env:TEMP 'vs_BuildTools.exe'
    Write-Step 'Downloading vs_BuildTools.exe'
    Invoke-WebRequest -UseBasicParsing `
        -Uri 'https://aka.ms/vs/17/release/vs_BuildTools.exe' `
        -OutFile $installer

    # NOTE: --passive (not --quiet). winget + `--quiet --override` for the
    # BuildTools bootstrapper has been observed to swallow exit codes when
    # adding components to an existing install. --passive shows a progress
    # bar but still propagates exit codes.
    #
    # NOTE: build the command line as a single string with explicit quoting.
    # Start-Process -ArgumentList <array> in PowerShell 5.1 does NOT reliably
    # quote elements containing spaces, so --installPath "C:\Program Files
    # (x86)\..." would be sent through unquoted and the bootstrapper would
    # parse the space-broken fragments as orphan args (exit 1, no useful
    # message). Single-string ArgumentList + explicit quoting works.
    $existingInstall = Get-VsBuildToolsInstallPath
    $addParts = ($missing | ForEach-Object { "--add $_" }) -join ' '

    if ($existingInstall) {
        # Modify path: only --add the missing components. Don't include
        # already-present ones or --includeRecommended (the bootstrapper has
        # been observed to fail with exit 1 when handed redundant --add args
        # on top of an existing install).
        Write-Step "Modifying existing BuildTools install at: $existingInstall"
        $cmdLine = 'modify --installPath "{0}" --wait --norestart --nocache --passive {1}' -f $existingInstall, $addParts
    } else {
        # Fresh install: --add every requested component; recommended bits OK.
        Write-Step 'Installing BuildTools fresh'
        $allAddParts = ($Components | ForEach-Object { "--add $_" }) -join ' '
        $cmdLine = '--wait --norestart --nocache --passive {0} --includeRecommended' -f $allAddParts
    }
    if ($ExtraArgs) { $cmdLine = "$cmdLine $($ExtraArgs -join ' ')" }

    Write-Step "vs_BuildTools.exe $cmdLine"
    $proc = Start-Process -FilePath $installer -ArgumentList $cmdLine -Wait -PassThru
    # 0 = success; 3010 = success, reboot required; 1602/1605 = user cancel/not installed
    if (@(0, 3010) -notcontains $proc.ExitCode) {
        $logGlob = Join-Path $env:TEMP 'dd_*.log'
        Write-Warn2 "vs_BuildTools.exe failed (exit $($proc.ExitCode))."
        Write-Warn2 "Inspect the latest installer log:"
        Write-Warn2 "  Get-ChildItem '$logGlob' | Sort-Object LastWriteTime -Desc | Select-Object -First 3"
        throw "vs_BuildTools.exe failed (exit $($proc.ExitCode))"
    }

    # Verify on disk -- the issue spec calls this out: "finished install" from
    # the installer doesn't always mean the components landed.
    $stillMissing = @($Components | Where-Object { -not (Test-VsComponent $_) })
    if ($stillMissing.Count -gt 0) {
        Write-Warn2 'These components are still missing after install:'
        $stillMissing | ForEach-Object { Write-Host "       - $_" }
        throw 'VS Build Tools install did not produce the required components.'
    }
    Write-Ok 'VS Build Tools components installed'
}

function Add-CargoBinToPath {
    # winget-installed rustup lives at %USERPROFILE%\.cargo\bin but PATH won't
    # update in the current PowerShell session. Add it so rustup steps can run
    # without forcing the user to open a new shell.
    $cargoBin = Join-Path $env:USERPROFILE '.cargo\bin'
    if (Test-Path (Join-Path $cargoBin 'rustup.exe')) {
        if (-not ($env:PATH -split ';' | Where-Object { $_ -eq $cargoBin })) {
            $env:PATH = "$cargoBin;$env:PATH"
        }
    }
}

function Set-RustToolchain {
    param(
        [Parameter(Mandatory)][string]$Toolchain,
        [string]$ExtraTarget
    )

    Add-CargoBinToPath
    if (-not (Get-Command rustup -ErrorAction SilentlyContinue)) {
        throw 'rustup not on PATH after install. Open a new PowerShell window and re-run this script.'
    }

    $installedToolchains = @((& rustup toolchain list) -split "`r?`n" | Where-Object { $_ })
    if (-not ($installedToolchains | Where-Object { $_ -like "$Toolchain-*" })) {
        Write-Step "rustup toolchain install $Toolchain"
        & rustup toolchain install $Toolchain --profile minimal --no-self-update | Out-Null
        if ($LASTEXITCODE -ne 0) { throw "rustup toolchain install $Toolchain failed" }
    } else {
        Write-Skip2 "Toolchain $Toolchain already installed"
    }

    Write-Step "rustup default $Toolchain"
    & rustup default $Toolchain | Out-Null
    if ($LASTEXITCODE -ne 0) { throw "rustup default $Toolchain failed" }

    if ($ExtraTarget) {
        $installed = @((& rustup target list --installed) -split "`r?`n" | Where-Object { $_ })
        if ($installed -contains $ExtraTarget) {
            Write-Skip2 "rustup target $ExtraTarget already installed"
        } else {
            Write-Step "rustup target add $ExtraTarget"
            & rustup target add $ExtraTarget | Out-Null
            if ($LASTEXITCODE -ne 0) { throw "rustup target add $ExtraTarget failed" }
            Write-Ok "rustup target $ExtraTarget added"
        }
    }
    Write-Ok "Rust toolchain $Toolchain ready"
}

function Repair-EnvLocalBom {
    # PowerShell 5.1's Out-File / Set-Content / > redirect (and old Notepad)
    # default to UTF-8 *with* BOM. just's dotenv parser then fails to parse
    # the file with a confusing error pointing at "line index: 0" -- it's the
    # BOM bytes. Strip them in place if found; this never destroys content
    # (BOM is metadata, not data) and unblocks the dev loop.
    param([Parameter(Mandatory)][string]$Path)
    $bytes = [System.IO.File]::ReadAllBytes($Path)
    if ($bytes.Length -ge 3 -and $bytes[0] -eq 0xEF -and $bytes[1] -eq 0xBB -and $bytes[2] -eq 0xBF) {
        Write-Step "Stripping UTF-8 BOM from $Path (just's dotenv parser chokes on BOM)"
        $stripped = $bytes[3..($bytes.Length - 1)]
        [System.IO.File]::WriteAllBytes($Path, $stripped)
        Write-Ok 'BOM stripped'
    }
}

function Set-EnvLocalTemplate {
    param([Parameter(Mandatory)][string]$RepoRoot)
    $envLocal = Join-Path $RepoRoot 'frontend\.env.local'
    if (Test-Path $envLocal) {
        Repair-EnvLocalBom -Path $envLocal
        Write-Skip2 'frontend/.env.local already exists'
        return
    }
    Write-Step 'Writing frontend/.env.local template (prod enclave URL)'
    $content = @'
# Maple local dev env. Vite bakes these values into the bundle at BUILD time,
# not runtime -- rerun the dev server or rebuild after editing. A missing or
# empty VITE_OPEN_SECRET_API_URL produces a silent white-screen on launch.
VITE_OPEN_SECRET_API_URL=https://enclave.trymaple.ai
VITE_CLIENT_ID=ba5a14b5-d915-47b1-b7b1-afda52bc5fc6
# VITE_MAPLE_BILLING_API_URL=https://billing.opensecret.cloud
# VITE_DEV_MODEL_OVERRIDE=gpt-4o
'@
    # Write UTF-8 without BOM -- Vite chokes on a BOM in .env files.
    [System.IO.File]::WriteAllText(
        $envLocal,
        $content,
        (New-Object System.Text.UTF8Encoding($false))
    )
    Write-Ok "Created frontend/.env.local (edit to point at a non-prod backend)"
}

# ---------- preflight ----------
Write-Section 'Preflight'
$hostArch = Get-HostArch
Write-Host "Host arch:    $hostArch"
Write-Host "PowerShell:   $($PSVersionTable.PSVersion)"
try {
    $osCaption = (Get-CimInstance -ClassName Win32_OperatingSystem -ErrorAction Stop).Caption
    Write-Host "OS:           $osCaption"
} catch {
    Write-Host "OS:           (unknown -- Get-CimInstance failed)"
}

if (-not (Test-IsAdmin)) {
    Write-Warn2 'Not running as Administrator. VS Build Tools install will UAC-prompt.'
}

if (-not (Test-WingetAvailable)) {
    throw "winget not found. Install 'App Installer' from the Microsoft Store, then re-run."
}

# npm.ps1 (and many other tool shims) are blocked under the default Restricted
# / AllSigned execution policy. Relax to RemoteSigned for the current user --
# but only if the EFFECTIVE policy (across all scopes) isn't already permissive.
# Common case: script invoked with `powershell -ExecutionPolicy Bypass` sets the
# Process scope to Bypass, which overrides CurrentUser. Naively setting
# CurrentUser anyway emits a non-terminating "overridden by a more specific
# scope" error that $ErrorActionPreference='Stop' turns into a script-killer.
$permissive = @('RemoteSigned', 'Unrestricted', 'Bypass')
$effective  = Get-ExecutionPolicy
if ($permissive -contains $effective) {
    Write-Skip2 "ExecutionPolicy already permissive (effective: $effective)"
} else {
    $userEp = Get-ExecutionPolicy -Scope CurrentUser
    Write-Step "Setting CurrentUser ExecutionPolicy: $userEp -> RemoteSigned"
    try {
        Set-ExecutionPolicy -Scope CurrentUser -ExecutionPolicy RemoteSigned -Force -ErrorAction Stop
        Write-Ok 'ExecutionPolicy updated'
    } catch {
        # Group policy or another scope can block the set. Effective policy is
        # what matters at runtime; warn and continue rather than abort the whole
        # bootstrap over a shim-permission concern.
        Write-Warn2 "Could not update CurrentUser ExecutionPolicy ($($_.Exception.Message.Trim())). Continuing -- if npm/just shims later fail with a policy error, re-run this script from an elevated shell or set the policy manually."
    }
}

# ---------- winget packages ----------
Write-Section 'winget packages'

$packages = @(
    @{ Id = 'Microsoft.VCRedist.2015+.x64';   Desc = 'VC++ 2015+ Redistributable (x64)' }
)
if ($hostArch -eq 'arm64') {
    # rollup's native module ships a prebuilt for ARM64 that links against the
    # ARM64 VC++ redistributable. x64-only redist is not enough on ARM hosts.
    $packages += @{ Id = 'Microsoft.VCRedist.2015+.arm64'; Desc = 'VC++ 2015+ Redistributable (ARM64)' }
}
$packages += @{ Id = 'OpenJS.NodeJS.LTS'; Desc = 'Node.js LTS (bun has no Win-ARM binary)' }
$packages += @{ Id = 'Rustlang.Rustup';   Desc = 'rustup (Rust toolchain manager)' }
# LLVM.LLVM is a backstop: the VS clang-cl component lives under the VS install
# and only resolves via Developer PowerShell / vcvars; this gives clang on PATH
# in any shell.
$packages += @{ Id = 'LLVM.LLVM';         Desc = 'LLVM / Clang (standalone)' }
# Git for Windows pulls in `git.exe`, Git Bash (`bash.exe`), and the bundled
# unix tools (curl, sha256sum, unzip, awk, cygpath) that
# scripts/tauri-windows.ps1 and provide-windows-onnxruntime.sh both rely on.
$packages += @{ Id = 'Git.Git';           Desc = 'Git for Windows (provides git + Git Bash)' }
# just is the recipe runner the README + docs/windows-build.md document for
# `just windows-build` / `just windows-dev`. Without it the wrappers can only
# be invoked via the underlying PowerShell script directly.
$packages += @{ Id = 'Casey.Just';        Desc = 'just (justfile runner)' }

foreach ($p in $packages) { Install-WingetPackage -Id $p.Id -Description $p.Desc }

# ---------- VS Build Tools ----------
Write-Section 'Visual Studio Build Tools 2022'

# The component IDs below were validated against PR 1's manual Windows smoke.
# - VC.Llvm.Clang is the one most likely to be forgotten; `ring` 0.17 on
#   aarch64-pc-windows-msvc needs clang for ARM64 asm.
# - VC.Tools.ARM64 is required on ARM hosts and harmless elsewhere; the
#   VCTools workload alone installs x64 cross-compilers only.
$vsComponents = @(
    'Microsoft.VisualStudio.Workload.VCTools',
    'Microsoft.VisualStudio.Component.VC.Tools.x86.x64',
    'Microsoft.VisualStudio.Component.VC.Tools.ARM64',
    'Microsoft.VisualStudio.Component.VC.Llvm.Clang',
    'Microsoft.VisualStudio.Component.Windows11SDK.22621'
)

if ($SkipVsBuildTools) {
    Write-Skip2 'VS Build Tools step skipped (-SkipVsBuildTools)'
} else {
    Install-VsBuildTools -Components $vsComponents -ExtraArgs $VsInstallerArgs
}

# ---------- Rust toolchain ----------
Write-Section 'Rust toolchain'
# On ARM64 hosts, default host target is aarch64-pc-windows-msvc; add x64 so
# the same machine can also cross-compile the x86_64 Tauri bundle.
$extraTarget = if ($hostArch -eq 'arm64') { 'x86_64-pc-windows-msvc' } else { $null }
try {
    Set-RustToolchain -Toolchain $RustToolchain -ExtraTarget $extraTarget
} catch {
    Write-Warn2 $_.Exception.Message
    Write-Warn2 'Skipping Rust step. Open a new PowerShell window so PATH picks up rustup, then re-run.'
}

# ---------- .env.local template ----------
Write-Section 'frontend/.env.local'
$repoRoot = Split-Path -Parent $PSScriptRoot
Set-EnvLocalTemplate -RepoRoot $repoRoot

# ---------- summary ----------
Write-Section 'Next steps'
@'

Setup complete. Close this PowerShell window and open a fresh one so PATH
picks up the freshly-installed git, just, and the updated Rust default.

Then from the repo root, build / dev via the just recipes (they wrap
vcvarsall, the ONNX Runtime helper, and the tauri.windows.conf.json
overlay -- no need to open Developer PowerShell first):

  just windows-dev                  # Tauri dev server (Vite hot-reload)
  just windows-build                # native ARM64 release (default)
  just windows-build x64            # native x64
  just windows-build arm64_amd64    # x64 cross-build from ARM host

Sanity checks first:
  rustc --version    # rustc 1.95.0
  node --version     # v22.x.x or later
  just --list        # should include windows-build / windows-dev

Edit frontend/.env.local if you need a non-prod backend. Vite bakes env
values at build time -- restart the dev server after edits. Save as
UTF-8 *without* BOM; PowerShell 5.1's Out-File / Set-Content / > redirect
all add a BOM by default and just's dotenv parser fails on it. Use VS Code
(no BOM by default), or in PS 7+ use `Set-Content -Encoding utf8NoBOM`.
Re-running this script will strip a BOM from an existing .env.local in
place if one is found.

Full guide: docs/windows-build.md

'@ | Write-Host
