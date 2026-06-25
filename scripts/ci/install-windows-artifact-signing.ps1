$ErrorActionPreference = "Stop"
Set-StrictMode -Version 3.0

$moduleName = "ArtifactSigning"
$moduleVersion = "0.1.8"
$moduleSha256 = "3221344b8c627915d3870f23e80816f31a5d8c2bae1d7c0cdd6c9652f6c4e089"
$moduleUrl = "https://www.powershellgallery.com/api/v2/package/$moduleName/$moduleVersion"

if ([string]::IsNullOrWhiteSpace($env:RUNNER_TEMP)) {
  $baseTemp = [System.IO.Path]::GetTempPath()
} else {
  $baseTemp = $env:RUNNER_TEMP
}

$moduleRoot = Join-Path $baseTemp "maple-powershell-modules"
$moduleDir = Join-Path $moduleRoot "$moduleName/$moduleVersion"
$downloadDir = Join-Path $baseTemp "maple-powershell-downloads"
$packagePath = Join-Path $downloadDir "$moduleName.$moduleVersion.nupkg"

New-Item -ItemType Directory -Force -Path $downloadDir | Out-Null
New-Item -ItemType Directory -Force -Path $moduleRoot | Out-Null

Invoke-WebRequest -Uri $moduleUrl -OutFile $packagePath -TimeoutSec 120

$actualSha256 = (Get-FileHash -Algorithm SHA256 -LiteralPath $packagePath).Hash.ToLowerInvariant()
if ($actualSha256 -ne $moduleSha256) {
  throw "$moduleName $moduleVersion hash mismatch. Expected $moduleSha256 but got $actualSha256."
}

if (Test-Path -LiteralPath $moduleDir) {
  Remove-Item -LiteralPath $moduleDir -Recurse -Force
}
New-Item -ItemType Directory -Force -Path $moduleDir | Out-Null
Expand-Archive -LiteralPath $packagePath -DestinationPath $moduleDir -Force

$manifestPath = Join-Path $moduleDir "$moduleName.psd1"
Import-Module -Name $manifestPath -Force -ErrorAction Stop
$loadedModule = Get-Module $moduleName | Where-Object { $_.Version -eq [version]$moduleVersion } | Select-Object -First 1
if (-not $loadedModule) {
  throw "$moduleName $moduleVersion was not loaded from $manifestPath."
}
if (-not (Get-Command Invoke-ArtifactSigning -ErrorAction SilentlyContinue)) {
  throw "Invoke-ArtifactSigning was not exported by $manifestPath."
}

if (-not [string]::IsNullOrWhiteSpace($env:GITHUB_ENV)) {
  "MAPLE_WINDOWS_ARTIFACT_SIGNING_MODULE_ROOT=$moduleRoot" | Out-File -FilePath $env:GITHUB_ENV -Append -Encoding utf8
  if ([string]::IsNullOrWhiteSpace($env:PSModulePath)) {
    "PSModulePath=$moduleRoot" | Out-File -FilePath $env:GITHUB_ENV -Append -Encoding utf8
  } else {
    "PSModulePath=$moduleRoot;$env:PSModulePath" | Out-File -FilePath $env:GITHUB_ENV -Append -Encoding utf8
  }
}

Write-Host "$moduleName $moduleVersion installed with verified SHA-256 $moduleSha256"
