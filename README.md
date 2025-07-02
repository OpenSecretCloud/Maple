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

This will configure git to use the project's pre-commit hooks, which run `bun run build` before each commit.

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

Expects a `VITE_OPEN_SECRET_API_URL` environment variable to be set. (See `.env.example`)

## Building

To build the desktop application:
```bash
# Standard build
bun tauri build

# For universal macOS build (Apple Silicon + Intel)
bun tauri build --target universal-apple-darwin
```

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

## Updating PCR0 values

If there's a new version of the enclave pushed to staging or prod, append the new PCR0 value to the `pcr0Values` or `pcr0DevValues` arrays in `frontend/src/app.tsx`.

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
