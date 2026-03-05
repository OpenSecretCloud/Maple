# Windows Support Progress

## Status: Dev Config Complete - CI/CD Workflow Needed

**Branch:** `windows-init`
**Issue:** #364
**PR:** #369

## Related Documentation

- `docs/windows-research.md` - Consolidated research from Gemini, Claude, and Grok on Tauri v2 Windows distribution (signing, installers, SmartScreen, CI/CD, gotchas)
- `docs/windows-certificate-setup.md` - Step-by-step guide for certificate signing options (Azure Individual, Azure Organization, SSL.com eSigner)
- `docs/azure-assignment-letter.md` - Template for the assignment letter Microsoft requested during identity validation

## Research & Planning (Complete - January 2026)

- [x] Research consolidated (Gemini, Claude, Grok) in `docs/windows-research.md`
- [x] Certificate setup guide written in `docs/windows-certificate-setup.md`
- [x] Key decisions made (see below)

## Azure Account & Identity Validation (Complete - March 4, 2026)

- [x] Azure account created with Pay-As-You-Go subscription
- [x] Trusted Signing Account created (East US region)
- [x] Organization identity validation submitted
- [x] Microsoft requested additional docs (Category 1: assignment letter)
- [x] Assignment letter uploaded to Azure portal (Feb 24, 2026)
- [x] Microsoft identity validation approved (March 4, 2026)

## Azure Signing Infrastructure (Complete - March 5, 2026)

- [x] Certificate Profile created (Public Trust)
- [x] App Registration created in Entra ID ("GitHub Actions - Maple Signing")
- [x] Federated credentials added (org: OpenSecretCloud, repo: Maple, tag: v*)
- [x] "Trusted Signing Certificate Profile Signer" role assigned
- [x] All 6 secrets added to GitHub repo (AZURE_CLIENT_ID, AZURE_TENANT_ID, AZURE_SUBSCRIPTION_ID, AZURE_ENDPOINT, AZURE_ACCOUNT, AZURE_PROFILE)

## App Configuration (Complete - March 5, 2026)

- [x] `tauri.conf.json` - Windows bundle config: NSIS installer, `currentUser` install mode, `embedBootstrapper` for WebView2, `signCommand` for Azure Trusted Signing
- [x] `build.rs` - Windows manifest: PerMonitorV2 DPI awareness, `asInvoker` execution level (no admin prompt)
- [x] Windows icon (.ico) - Already existed with proper multi-layer format
- [x] Platform detection (`src/utils/platform.ts`) - Already handled Windows, no changes needed

## Remaining Work

- [ ] **GitHub Actions workflow for signed Windows builds** - Needs a Windows build job that:
  - Runs on `windows-latest`
  - Authenticates to Azure via OIDC (using federated credentials)
  - Installs the Azure Code Signing Tool (`dotnet tool install --global Azure.CodeSigning.Tool`)
  - Creates a `sign.ps1` adapter script that injects Azure secrets (endpoint, account, profile) and calls the signing tool
  - Runs `tauri build --bundles nsis` (the `signCommand` in tauri.conf.json calls `sign.ps1 %1`)
  - Uploads the signed NSIS installer as a release artifact
  - Reference: `docs/windows-research.md` has full workflow examples
- [ ] **Auto-updater `latest.json` Windows platform entry** - Needs a `windows-x86_64` entry added to the update manifest when the first Windows release is built (the release workflow typically generates the signature `.sig` file and populates this)

## Key Decisions Made

- **Certificate:** Azure Trusted Signing, Organization validation (~$10/mo)
- **Installer:** NSIS (.exe), `currentUser` install mode (installs to `%LOCALAPPDATA%`, no admin required)
- **WebView2:** `embedBootstrapper` (adds ~1.8MB, works offline)
- **CI/CD auth:** OIDC (keyless, no stored secrets needed in GitHub)
- **DPI:** PerMonitorV2 awareness via Windows manifest in `build.rs`
- **Signing flow:** `tauri.conf.json` → `signCommand` → `sign.ps1` → Azure Code Signing Tool
