param(
  [Parameter(Mandatory = $true)]
  [string]$File
)

$ArtifactSigningVersion = "0.1.8"

$ErrorActionPreference = "Stop"
Set-StrictMode -Version 3.0

if (-not (Test-Path -LiteralPath $File -PathType Leaf)) {
  throw "Windows artifact to sign was not found: $File"
}

$requiredEnv = @(
  "MAPLE_WINDOWS_ARTIFACT_SIGNING_ENDPOINT",
  "MAPLE_WINDOWS_ARTIFACT_SIGNING_ACCOUNT_NAME",
  "MAPLE_WINDOWS_ARTIFACT_SIGNING_CERTIFICATE_PROFILE_NAME"
)

foreach ($name in $requiredEnv) {
  if ([string]::IsNullOrWhiteSpace([Environment]::GetEnvironmentVariable($name))) {
    throw "$name is required for Windows Artifact Signing."
  }
}

$moduleRoot = $env:MAPLE_WINDOWS_ARTIFACT_SIGNING_MODULE_ROOT
if (-not [string]::IsNullOrWhiteSpace($moduleRoot)) {
  $moduleManifest = Join-Path $moduleRoot "ArtifactSigning/$ArtifactSigningVersion/ArtifactSigning.psd1"
  if (-not (Test-Path -LiteralPath $moduleManifest -PathType Leaf)) {
    throw "ArtifactSigning module manifest was not found: $moduleManifest"
  }
  Import-Module -Name $moduleManifest -Force -ErrorAction Stop
} else {
  Import-Module ArtifactSigning -RequiredVersion $ArtifactSigningVersion -ErrorAction Stop
}

$loadedModule = Get-Module ArtifactSigning | Where-Object { $_.Version -eq [version]$ArtifactSigningVersion } | Select-Object -First 1
if (-not $loadedModule) {
  throw "ArtifactSigning $ArtifactSigningVersion was not loaded."
}

$params = @{
  Endpoint                          = $env:MAPLE_WINDOWS_ARTIFACT_SIGNING_ENDPOINT
  CodeSigningAccountName            = $env:MAPLE_WINDOWS_ARTIFACT_SIGNING_ACCOUNT_NAME
  CertificateProfileName            = $env:MAPLE_WINDOWS_ARTIFACT_SIGNING_CERTIFICATE_PROFILE_NAME
  Files                             = $File
  FileDigest                        = "SHA256"
  TimestampRfc3161                  = "http://timestamp.acs.microsoft.com"
  TimestampDigest                   = "SHA256"
  ExcludeEnvironmentCredential      = $true
  ExcludeWorkloadIdentityCredential = $true
  ExcludeManagedIdentityCredential  = $true
  ExcludeSharedTokenCacheCredential = $true
  ExcludeVisualStudioCredential     = $true
  ExcludeVisualStudioCodeCredential = $true
  ExcludeAzureCliCredential         = $false
  ExcludeAzurePowerShellCredential  = $true
  ExcludeAzureDeveloperCliCredential = $true
  ExcludeInteractiveBrowserCredential = $true
}

Invoke-ArtifactSigning @params

Write-Host ("signed-windows-artifact  {0}" -f (Split-Path -Leaf $File))
