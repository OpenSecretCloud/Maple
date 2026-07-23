# Maple AI Frontend

Uses [bun](https://bun.sh/) for development and [Tauri](https://tauri.app/) for desktop app builds.

## Prerequisites

1. Install [Bun](https://bun.sh/):
```bash
curl -fsSL https://bun.sh/install | bash
```

2. Install Rust and its dependencies:
```bash
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
```

3. Install system dependencies:

### macOS
```bash
# Install Xcode Command Line Tools
xcode-select --install

# Install additional dependencies via Homebrew
brew install openssl@3
```

### Linux (Ubuntu/Debian)
```bash
sudo apt update
sudo apt install libwebkit2gtk-4.1-dev \
    build-essential \
    curl \
    wget \
    file \
    libssl-dev \
    libgtk-3-dev \
    libayatana-appindicator3-dev \
    librsvg2-dev
```

4. Add required Rust targets for universal macOS builds:
```bash
rustup target add aarch64-apple-darwin x86_64-apple-darwin
```

## Setup

1. Clone the repository and run the setup script:
```bash
./setup-hooks.sh
```

This configures Git to use the project's pre-commit hook. Managed
`opensecret-workspaces` checkouts enable the same hook automatically. The hook
checks formatting, builds the frontend, runs frontend tests, and runs Rust tests
when Tauri files are staged. When Nix is installed, the hook enters the pinned
CI development shell automatically, including from macOS GUI environments that
do not inherit the user's shell `PATH`. Without Nix, it uses compatible Bun and
Cargo tools already available on `PATH`.

## Development

### Using Just Commands

This project uses [just](https://github.com/casey/just) for common development tasks:

```bash
# List all available commands
just

# Install dependencies
just install

# Start development server
just dev

# Build the project
just build

# Format code
just format

# Run tests
just test

# Get current version
just get-version
```

### Manual Development

1. Install dependencies:
```bash
bun install
```

2. Start the development server:
```bash
# For web development only
bun run dev

# For desktop app development
bun tauri dev
```

Expects a `VITE_OPEN_SECRET_API_URL` environment variable to be set. For local
OpenSecret development, copy `frontend/.env.example` to `frontend/.env.local`
and point it at the local API:

```bash
VITE_OPEN_SECRET_API_URL=http://127.0.0.1:3000
VITE_OPEN_SECRET_ATTESTATION_ENVIRONMENT=dev
```

The public OpenSecret client id defaults to Maple's project id
`ba5a14b5-d915-47b1-b7b1-afda52bc5fc6`, so `VITE_CLIENT_ID` is only needed to
override the default. (See `.env.example`.) The attestation environment defaults
to `prod`. Selecting `dev` changes the authorized tagged-release policy; it does
not disable verification for a remote server. The SDK's local development bypass
only applies when an HTTP `VITE_OPEN_SECRET_API_URL` parses to an exact supported
local-loopback host; lookalike hostnames do not qualify.

## Building

### Desktop Builds

Use the `just` commands for desktop builds:

```bash
# Release build
just desktop-build

# Debug build
just desktop-build-debug

# If you encounter CC-related errors in a Nix shell, use the -no-cc variants:
just desktop-build-no-cc
just desktop-build-debug-no-cc
```

Or use `bun tauri build` directly:
```bash
# Standard build
bun tauri build

# For universal macOS build (Apple Silicon + Intel)
bun tauri build --target universal-apple-darwin
```

#### Linux: ONNX Runtime Setup

Linux builds bundle ONNX Runtime 1.23.2 for local TTS and PDF OCR. `just desktop-build` provisions it automatically. Before invoking `bun tauri build` directly, download the pinned shared library:

```bash
cd frontend/src-tauri
./scripts/provide-linux-onnxruntime.sh
```

This downloads the pinned ONNX Runtime release, verifies its SHA-256 checksum, and extracts it to `frontend/src-tauri/onnxruntime-linux/` (which is gitignored). The script is idempotent — it skips the download if the library already exists.

> **Note:** CI workflows and the desktop `just` recipes call this script automatically. Run it manually only when invoking the lower-level Tauri commands directly.

#### Linux: Running in Headless/Virtual Display Environments

If you're running the built Maple binary in a headless environment (e.g., CI, virtual display with Xvfb), WebKit may fail to render content. Set these environment variables before launching:

```bash
export WEBKIT_DISABLE_COMPOSITING_MODE=1
export WEBKIT_DISABLE_DMABUF_RENDERER=1
DISPLAY=:0 ./frontend/src-tauri/target/debug/maple
```

These are set automatically when using `nix develop` (via `flake.nix`).

If you need a new set of icons:

```
bun run tauri icon [path/to/png]
```

## Releases

### Setting up Signing Keys

#### Tauri Updater Signing
1. Generate a new signing key:
```bash
cargo tauri signer generate
```
This will create the tauri public and private key.

2. Add the public key to `src-tauri/tauri.conf.json` in the `updater.pubkey` field
3. Add the private key to GitHub Actions secrets:
   - Go to repository Settings → Secrets and variables → Actions
   - Create a new secret named `TAURI_SIGNING_PRIVATE_KEY`
   - Create another secret named `TAURI_SIGNING_PRIVATE_KEY_PASSWORD` if your key has a password
   - Paste the private key from the tauri command.

#### Apple Developer Certificate (for macOS builds)
For proper macOS builds and notarization, you need to set up the following GitHub secrets:

1. `APPLE_CERTIFICATE` - Base64-encoded p12 certificate
   ```bash
   base64 -i YourCertificate.p12 | pbcopy  # Copies to clipboard
   ```

2. `APPLE_CERTIFICATE_PASSWORD` - Password for the certificate
3. `KEYCHAIN_PASSWORD` - Password for the temporary keychain (can be any secure password)
4. `APPLE_ID` - Your Apple Developer account email
5. `APPLE_PASSWORD` - Your Apple Developer account password or app-specific password
6. `APPLE_TEAM_ID` - Your Apple Developer team ID

#### Azure Artifact Signing (for Windows builds)
Windows release builds use Microsoft Azure Artifact Signing through GitHub
Actions OIDC. No Azure client secret is required.

Add these GitHub Actions secrets:

| GitHub secret | Source |
|---------------|--------|
| `AZURE_CLIENT_ID` | Microsoft Entra app registration "Application (client) ID". |
| `AZURE_TENANT_ID` | Microsoft Entra "Directory (tenant) ID". |
| `AZURE_SUBSCRIPTION_ID` | Azure subscription ID that contains the Artifact Signing account. |
| `AZURE_ARTIFACT_SIGNING_ENDPOINT` | Artifact Signing account endpoint. |
| `AZURE_ARTIFACT_SIGNING_ACCOUNT_NAME` | Artifact Signing account name. |
| `AZURE_ARTIFACT_SIGNING_CERTIFICATE_PROFILE_NAME` | Certificate profile name under the Artifact Signing account. |
| `AZURE_ARTIFACT_SIGNING_EXPECTED_SUBJECT` | Expected signer certificate Subject DN for that certificate profile. |

The Entra application must also have a federated credential for this GitHub
environment:

```text
repo:OpenSecretCloud/Maple:environment:windows-signing
```

In the Azure portal, add it under Microsoft Entra ID -> App registrations ->
the CI application -> Certificates & secrets -> Federated credentials. Use:

- Organization: `OpenSecretCloud`
- Repository: `Maple`
- Entity type: `Environment`
- Environment name: `windows-signing`
- Audience: `api://AzureADTokenExchange`

If the portal asks for raw values instead of GitHub-specific fields, use:

- Issuer: `https://token.actions.githubusercontent.com`
- Subject: `repo:OpenSecretCloud/Maple:environment:windows-signing`
- Audience: `api://AzureADTokenExchange`

Assign the same Entra application/service principal the `Artifact Signing
Certificate Profile Signer` role on the Artifact Signing account, resource
group, or subscription. The role assignment is what authorizes the OIDC-auth'd
GitHub runner to sign with the certificate profile.

The expected signer subject should match the X.509 signer certificate subject
reported by `Get-AuthenticodeSignature`. It is usually visible on the Artifact
Signing certificate profile as the Subject DN derived from the completed identity
validation, for example `CN=Example Corp, O=Example Corp, L=City, S=State, C=US`.
Do not pin the leaf certificate thumbprint for this check; Artifact Signing
manages certificate lifecycle and can issue rotated certificates for the same
profile identity.

The Windows release job builds `maple.exe` first, Authenticode-signs it, bundles
the NSIS installer from that signed executable, Authenticode-signs the installer,
then creates the Tauri updater `.sig` for the final signed installer bytes. This
ordering is required because Authenticode signing changes the file being signed;
the Tauri updater signature must be generated after the final Windows installer
signature is applied.

Windows release verification currently checks the final signed installer bytes,
the final Tauri updater signature, and the pinned runtime DLL proofs. It does not
yet try to canonicalize Authenticode-signed Windows binaries back to an unsigned
baseline.

### To Create a Release

#### Version Management
Use the provided `just` commands to manage version updates:

```bash
# Bump patch version (e.g., 1.0.0 → 1.0.1)
just bump-patch

# Bump minor version (e.g., 1.0.0 → 1.1.0)
just bump-minor

# Bump major version (e.g., 1.0.0 → 2.0.0)
just bump-major

# Set a specific version
just update-version 1.2.3

# Create a release with automatic git tag
just release 1.2.3
```

These commands automatically update all necessary files:
- `frontend/package.json`
- `frontend/src-tauri/tauri.conf.json`
- `frontend/src-tauri/Cargo.toml`
- `frontend/src-tauri/gen/apple/project.yml`
- `frontend/src-tauri/gen/apple/maple_iOS/Info.plist`
- `Cargo.lock` (via cargo check)

Android `versionCode` is internal and increments by one for each Play Store upload.
Use `just update-android-counter` for another internal/test build with the same visible version.

#### Creating a GitHub Release
1. Use one of the version commands above to update the version
2. Create a new release in GitHub:
   - Go to Releases → Draft a new release
   - Create a new tag (e.g., `v0.1.0`)
   - Set a release title and description
   - Publish the release

The GitHub Actions workflow will automatically:
- Build the app for all platforms
- Sign the builds
- Upload the artifacts to the release
- Create and upload `latest.json` for auto-updates

## OpenSecret enclave release authorization

Maple does not carry a hand-maintained PCR allowlist. Before key exchange, the
OpenSecret SDK verifies the AWS Nitro attestation document and requires its full
PCR0/PCR1/PCR2 measurement tuple to match an OpenSecret release authorized for
the configured environment.

Each SDK release embeds a generated snapshot of authorized measurements. The
snapshot updater treats the tagged OpenSecret release manifest and Cosign bundle
as untrusted input, pins the expected repository, workflow, OIDC issuer, and tag
identity, and verifies the signature and Rekor transparency-log evidence before
generating the snapshot. The app performs no GitHub, Sigstore, or Rekor network
lookup during a runtime handshake.

Sigstore evidence shows that the pinned release workflow signed the measurements
and that the event was entered in the transparency log. It does not prove that
the source is safe, that a Nix build is reproducible, or that an authorized
release is the newest one. Rebuilding from source remains an independent
reproducibility check, and freshness, rollback, and revocation need separate
policy.

No tagged Sigstore release assets or SDK version containing their generated
snapshot have been published yet. This source change is stacked on an unreleased
`@opensecret/react` API that adds the explicit attestation environment and
enforces the release check before key exchange. The package remains pinned to
the real published `3.2.1` release in this branch: do not merge or release this
integration until a signed backend release exists, the SDK snapshot is generated
and reviewed, that SDK is published, and this exact dependency is bumped.

Maple also embeds `maple-proxy` for its local OpenAI-compatible transport. This
integration branch pins the exact reviewed proxy commit with default features
disabled; that commit pins the same secured Rust SDK. Its snapshot is still
empty, so the pin must be updated to a snapshot-bearing commit and later to the
published release. Updating only the TypeScript package would leave the
embedded proxy on the legacy Rust attestation behavior.

## iOS Development

Run in emulator:

```bash
dotenv -e .env.local -- bun run tauri ios dev 'iPhone 16 Pro'
```

Run on a connected phone:

```bash
dotenv -e .env.local -- bun run tauri ios build
```

### Ignoring Local XCode Project Changes

To prevent committing automatic changes to the XCode project file during local development:

```bash
# Tell Git to ignore changes to the file
git update-index --assume-unchanged frontend/src-tauri/gen/apple/maple.xcodeproj/project.pbxproj

# When you need to commit changes to this file, use:
git update-index --no-assume-unchanged frontend/src-tauri/gen/apple/maple.xcodeproj/project.pbxproj
```
