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

## Development

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

Expects a `VITE_OPEN_SECRET_API_URL` environment variable to be set. (See `.env.example`)

## Building

To build the desktop application:
```bash
# Standard build
bun tauri build

# For universal macOS build (Apple Silicon + Intel)
bun tauri build --target universal-apple-darwin
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

### Creating a Release
1. Update the version in `src-tauri/tauri.conf.json`
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

## Updating PCR0 values

If there's a new version of the enclave pushed to staging or prod, append the new PCR0 value to the `pcr0Values` or `pcr0DevValues` arrays in `frontend/src/app.tsx`.
