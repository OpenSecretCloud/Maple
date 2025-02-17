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
xcode-select --install
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
bun tauri build
```

## Releases

### Setting up Signing Keys
1. Generate a new signing key:
```bash
cargo tauri signer generate
```
This will create the tauri public and private key.


2. Add the public key to `src-tauri/tauri.conf.json` in the `updater.pubkey` field
3. Add the private key to GitHub Actions secrets:
   - Go to repository Settings → Secrets and variables → Actions
   - Create a new secret named `TAURI_SIGNING_PRIVATE_KEY`
   - Paste the private key from the tauri command.

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
