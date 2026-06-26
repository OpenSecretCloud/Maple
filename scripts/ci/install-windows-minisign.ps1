$ErrorActionPreference = "Stop"
Set-StrictMode -Version 3.0

$minisignVersion = "0.12"
$minisignSha256 = "37b600344e20c19314b2e82813db2bfdcc408b77b876f7727889dbd46d539479"
$minisignArchive = "minisign-$minisignVersion-win64.zip"
$minisignUrl = "https://github.com/jedisct1/minisign/releases/download/$minisignVersion/$minisignArchive"

if ([string]::IsNullOrWhiteSpace($env:RUNNER_TEMP)) {
  $baseTemp = [System.IO.Path]::GetTempPath()
} else {
  $baseTemp = $env:RUNNER_TEMP
}

$downloadDir = Join-Path $baseTemp "maple-minisign-downloads"
$extractDir = Join-Path $baseTemp "maple-minisign-$minisignVersion"
$archivePath = Join-Path $downloadDir $minisignArchive

New-Item -ItemType Directory -Force -Path $downloadDir | Out-Null
Invoke-WebRequest -Uri $minisignUrl -OutFile $archivePath -TimeoutSec 120

$actualSha256 = (Get-FileHash -Algorithm SHA256 -LiteralPath $archivePath).Hash.ToLowerInvariant()
if ($actualSha256 -ne $minisignSha256) {
  throw "minisign $minisignVersion hash mismatch. Expected $minisignSha256 but got $actualSha256."
}

if (Test-Path -LiteralPath $extractDir) {
  Remove-Item -LiteralPath $extractDir -Recurse -Force
}
New-Item -ItemType Directory -Force -Path $extractDir | Out-Null
Expand-Archive -LiteralPath $archivePath -DestinationPath $extractDir -Force

$osArchitecture = [System.Runtime.InteropServices.RuntimeInformation]::OSArchitecture.ToString()
$minisignArch = switch ($osArchitecture) {
  "X64" { "x86_64" }
  "Arm64" { "aarch64" }
  default { throw "Unsupported Windows architecture for minisign: $osArchitecture" }
}

$minisignDir = Join-Path (Join-Path $extractDir "minisign-win64") $minisignArch
$minisignPath = Join-Path $minisignDir "minisign.exe"
if (-not (Test-Path -LiteralPath $minisignPath)) {
  throw "minisign.exe was not found at $minisignPath."
}

if (-not [string]::IsNullOrWhiteSpace($env:GITHUB_PATH)) {
  $minisignDir | Out-File -FilePath $env:GITHUB_PATH -Append -Encoding utf8
}

& $minisignPath -v
Write-Host "minisign $minisignVersion installed with verified SHA-256 $minisignSha256"
