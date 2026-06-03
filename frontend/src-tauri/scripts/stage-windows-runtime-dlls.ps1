param(
  [Parameter(Mandatory = $true)]
  [string] $OrtDllPath,

  [Parameter(Mandatory = $true)]
  [string] $Destination
)

$ErrorActionPreference = "Stop"

$dllNames = @(
  "VCRUNTIME140.dll",
  "VCRUNTIME140_1.dll",
  "MSVCP140.dll",
  "MSVCP140_1.dll"
)

function Require-Env {
  param([Parameter(Mandatory = $true)][string] $Name)

  $value = [Environment]::GetEnvironmentVariable($Name)
  if ([string]::IsNullOrWhiteSpace($value)) {
    throw "Missing required environment variable: $Name"
  }

  return $value
}

function Get-Sha256 {
  param([Parameter(Mandatory = $true)][string] $Path)

  return (Get-FileHash -LiteralPath $Path -Algorithm SHA256).Hash.ToLowerInvariant()
}

function Assert-Sha256 {
  param(
    [Parameter(Mandatory = $true)][string] $Label,
    [Parameter(Mandatory = $true)][string] $Path,
    [Parameter(Mandatory = $true)][string] $Expected
  )

  $actual = Get-Sha256 -Path $Path
  if ($actual -ne $Expected.ToLowerInvariant()) {
    throw "$Label SHA-256 mismatch for ${Path}. expected=$Expected actual=$actual"
  }
}

function Invoke-Download {
  param(
    [Parameter(Mandatory = $true)][string] $Url,
    [Parameter(Mandatory = $true)][string] $OutFile
  )

  for ($attempt = 1; $attempt -le 5; $attempt++) {
    try {
      Invoke-WebRequest -Uri $Url -OutFile $OutFile
      return
    } catch {
      if ($attempt -eq 5) {
        throw
      }
      Start-Sleep -Seconds (2 * $attempt)
    }
  }
}

function Invoke-CheckedProcess {
  param(
    [Parameter(Mandatory = $true)][string] $FilePath,
    [Parameter(Mandatory = $true)][string] $Arguments,
    [int[]] $AllowedExitCodes = @(0, 3010)
  )

  $process = Start-Process -FilePath $FilePath -ArgumentList $Arguments -NoNewWindow -Wait -PassThru
  if ($AllowedExitCodes -notcontains $process.ExitCode) {
    throw "Command failed with exit code $($process.ExitCode): $FilePath $Arguments"
  }
}

function Get-PayloadKind {
  param([Parameter(Mandatory = $true)][string] $Path)

  $bytes = New-Object byte[] 8
  $stream = [IO.File]::OpenRead($Path)
  try {
    $read = $stream.Read($bytes, 0, $bytes.Length)
  } finally {
    $stream.Dispose()
  }

  if (($read -ge 4) -and ([Text.Encoding]::ASCII.GetString($bytes, 0, 4) -eq "MSCF")) {
    return "cab"
  }

  if (
    ($read -ge 8) -and
    ($bytes[0] -eq 0xd0) -and
    ($bytes[1] -eq 0xcf) -and
    ($bytes[2] -eq 0x11) -and
    ($bytes[3] -eq 0xe0) -and
    ($bytes[4] -eq 0xa1) -and
    ($bytes[5] -eq 0xb1) -and
    ($bytes[6] -eq 0x1a) -and
    ($bytes[7] -eq 0xe1)
  ) {
    return "msi"
  }

  if (($read -ge 2) -and ($bytes[0] -eq 0x4d) -and ($bytes[1] -eq 0x5a)) {
    return "exe"
  }

  return "unknown"
}

function Copy-PayloadWithExtension {
  param(
    [Parameter(Mandatory = $true)][string] $Path,
    [Parameter(Mandatory = $true)][string] $Extension,
    [Parameter(Mandatory = $true)][string] $OutDir,
    [Parameter(Mandatory = $true)][int] $Index
  )

  if ([IO.Path]::GetExtension($Path) -ieq $Extension) {
    return $Path
  }

  $target = Join-Path $OutDir ("payload-{0}{1}" -f $Index, $Extension)
  Copy-Item -LiteralPath $Path -Destination $target -Force
  return $target
}

function Test-PortableExecutableMachineAmd64 {
  param([Parameter(Mandatory = $true)][string] $Path)

  $stream = $null
  $reader = $null
  try {
    $stream = [IO.File]::OpenRead($Path)
    if ($stream.Length -lt 64) {
      return $false
    }

    $reader = New-Object IO.BinaryReader($stream)
    if ($reader.ReadUInt16() -ne 0x5A4D) {
      return $false
    }

    $stream.Position = 0x3C
    $peOffset = $reader.ReadInt32()
    if (($peOffset -lt 0) -or ($stream.Length -lt ($peOffset + 6))) {
      return $false
    }

    $stream.Position = $peOffset
    if ($reader.ReadUInt32() -ne 0x00004550) {
      return $false
    }

    return $reader.ReadUInt16() -eq 0x8664
  } catch {
    return $false
  } finally {
    if ($null -ne $reader) {
      $reader.Dispose()
    } elseif ($null -ne $stream) {
      $stream.Dispose()
    }
  }
}

function Expand-VcRedist {
  param(
    [Parameter(Mandatory = $true)][string] $ExePath,
    [Parameter(Mandatory = $true)][string] $WixExe,
    [Parameter(Mandatory = $true)][string] $OutDir
  )

  $burnDir = Join-Path $OutDir "burn"
  $layoutDir = Join-Path $OutDir "layout"
  $adminDir = Join-Path $OutDir "admin"
  $cabDir = Join-Path $OutDir "cab"
  $payloadDir = Join-Path $OutDir "payloads"

  Remove-Item -LiteralPath $burnDir, $layoutDir, $adminDir, $cabDir, $payloadDir -Recurse -Force -ErrorAction SilentlyContinue
  New-Item -ItemType Directory -Force -Path $burnDir, $layoutDir, $adminDir, $cabDir, $payloadDir | Out-Null

  Invoke-CheckedProcess `
    -FilePath $WixExe `
    -Arguments "burn extract `"$ExePath`" -o `"$burnDir`""

  Invoke-CheckedProcess `
    -FilePath $ExePath `
    -Arguments "/layout `"$layoutDir`" /quiet /norestart"

  $packageRoots = @($burnDir, $layoutDir)

  $payloads = @(Get-ChildItem -LiteralPath $packageRoots -Recurse -File |
      Sort-Object FullName |
      ForEach-Object {
        $kind = Get-PayloadKind -Path $_.FullName
        Write-Host ("vc-redist-payload  {0}  {1}  len={2}" -f $kind, $_.FullName, $_.Length)
        [PSCustomObject]@{
          File = $_
          Kind = $kind
        }
      })

  $cabIndex = 0
  foreach ($payload in ($payloads | Where-Object { $_.Kind -eq "cab" })) {
    $cabIndex += 1
    $cabPath = Copy-PayloadWithExtension `
      -Path $payload.File.FullName `
      -Extension ".cab" `
      -OutDir $payloadDir `
      -Index $cabIndex
    $targetDir = Join-Path $cabDir ("payload-{0}" -f $cabIndex)
    New-Item -ItemType Directory -Force -Path $targetDir | Out-Null
    & "$env:WINDIR\System32\expand.exe" -F:* $cabPath $targetDir | Out-Null
    if ($LASTEXITCODE -ne 0) {
      throw "Failed to expand $cabPath"
    }
  }

  $msiIndex = 0
  foreach ($payload in ($payloads | Where-Object { $_.Kind -eq "msi" })) {
    $msiIndex += 1
    $msiPath = Copy-PayloadWithExtension `
      -Path $payload.File.FullName `
      -Extension ".msi" `
      -OutDir $payloadDir `
      -Index $msiIndex
    $targetDir = Join-Path $adminDir ("payload-{0}" -f $msiIndex)
    New-Item -ItemType Directory -Force -Path $targetDir | Out-Null
    try {
      Invoke-CheckedProcess `
        -FilePath "msiexec.exe" `
        -Arguments "/a `"$msiPath`" TARGETDIR=`"$targetDir`" /qn /norestart"
    } catch {
      Write-Host ("vc-redist-msi-admin-extract-skipped  {0}  {1}" -f $msiPath, $_.Exception.Message)
    }
  }

  return @($burnDir, $layoutDir, $adminDir, $cabDir)
}

function Find-RequiredDll {
  param(
    [Parameter(Mandatory = $true)][string] $Name,
    [Parameter(Mandatory = $true)][string[]] $Roots
  )

  $files = @()
  foreach ($root in $Roots) {
    $files += @(Get-ChildItem -LiteralPath $root -Recurse -File -ErrorAction SilentlyContinue)
  }

  $x64Files = @($files | Where-Object { Test-PortableExecutableMachineAmd64 -Path $_.FullName })

  $match = $x64Files |
    Where-Object { $_.Name -ieq $Name } |
    Sort-Object FullName |
    Select-Object -First 1

  if ($null -ne $match) {
    return $match.FullName
  }

  foreach ($file in ($x64Files | Sort-Object FullName)) {
    $versionInfo = $null
    try {
      $versionInfo = $file.VersionInfo
    } catch {
      continue
    }

    if ($null -eq $versionInfo) {
      continue
    }

    if (
      ($versionInfo.OriginalFilename -ieq $Name) -or
      ($versionInfo.InternalName -ieq $Name) -or
      ($versionInfo.FileDescription -ieq $Name)
    ) {
      Write-Host ("resolved-windows-runtime-dll  {0}  {1}" -f $Name, $file.FullName)
      return $file.FullName
    }
  }

  Write-Host "available-vc-redist-payload-files:"
  $files |
    Sort-Object FullName |
    Select-Object -First 200 |
    ForEach-Object {
      $originalFilename = ""
      $internalName = ""
      try {
        $originalFilename = $_.VersionInfo.OriginalFilename
        $internalName = $_.VersionInfo.InternalName
      } catch {
      }
      Write-Host ("  {0}  len={1}  original={2}  internal={3}" -f $_.FullName, $_.Length, $originalFilename, $internalName)
    }

  throw "Could not find $Name in extracted VC++ Redistributable payload roots: $($Roots -join '; ')"
}

function Get-WixExe {
  param([Parameter(Mandatory = $true)][string] $CacheRoot)

  $version = Require-Env -Name "MAPLE_WINDOWS_WIX_CLI_VERSION"
  $url = Require-Env -Name "MAPLE_WINDOWS_WIX_CLI_URL"
  $sha256 = Require-Env -Name "MAPLE_WINDOWS_WIX_CLI_SHA256"
  $wixDir = Join-Path $CacheRoot "wix-$version"
  $wixPackage = Join-Path $wixDir "wix.$version.nupkg"
  $wixExtracted = Join-Path $wixDir "pkg"
  $wixExe = Join-Path $wixExtracted "tools\net6.0\any\wix.exe"

  New-Item -ItemType Directory -Force -Path $wixDir | Out-Null

  if (!(Test-Path -LiteralPath $wixPackage)) {
    Invoke-Download -Url $url -OutFile $wixPackage
  }
  Assert-Sha256 -Label "WiX CLI $version NuGet package" -Path $wixPackage -Expected $sha256

  if (!(Test-Path -LiteralPath $wixExe)) {
    Remove-Item -LiteralPath $wixExtracted -Recurse -Force -ErrorAction SilentlyContinue
    Expand-Archive -LiteralPath $wixPackage -DestinationPath $wixExtracted -Force
  }

  if (!(Test-Path -LiteralPath $wixExe)) {
    throw "WiX executable was not found after extracting $wixPackage"
  }

  return $wixExe
}

$vcRedistVersion = Require-Env -Name "MAPLE_WINDOWS_VC_REDIST_VERSION"
$vcRedistUrl = Require-Env -Name "MAPLE_WINDOWS_VC_REDIST_URL"
$vcRedistSha256 = Require-Env -Name "MAPLE_WINDOWS_VC_REDIST_SHA256"

$tauriDir = (Resolve-Path (Join-Path $PSScriptRoot "..")).Path
$cacheDir = Join-Path $tauriDir "target\windows-runtime"
$redistDir = Join-Path $cacheDir "vc-redist-$vcRedistVersion"
$redistExe = Join-Path $redistDir "VC_redist.x64.exe"

New-Item -ItemType Directory -Force -Path $Destination, $redistDir | Out-Null
Get-ChildItem -LiteralPath $Destination -Filter "*.dll" -File -ErrorAction SilentlyContinue |
  Remove-Item -Force

if (!(Test-Path -LiteralPath $redistExe)) {
  Invoke-Download -Url $vcRedistUrl -OutFile $redistExe
}

Assert-Sha256 -Label "VC++ Redistributable $vcRedistVersion" -Path $redistExe -Expected $vcRedistSha256

$wixExe = Get-WixExe -CacheRoot $cacheDir
$payloadRoots = Expand-VcRedist -ExePath $redistExe -WixExe $wixExe -OutDir $redistDir

Copy-Item -LiteralPath $OrtDllPath -Destination (Join-Path $Destination "onnxruntime.dll") -Force

foreach ($dllName in $dllNames) {
  $source = Find-RequiredDll -Name $dllName -Roots $payloadRoots
  Copy-Item -LiteralPath $source -Destination (Join-Path $Destination $dllName) -Force
}

Get-ChildItem -LiteralPath $Destination -Filter "*.dll" -File |
  Sort-Object Name |
  ForEach-Object {
    $hash = Get-Sha256 -Path $_.FullName
    Write-Host ("sha256-windows-runtime-dll  {0}  {1}" -f $hash, $_.Name)
  }
