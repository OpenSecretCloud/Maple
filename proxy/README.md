# 🍁 Maple Proxy

A lightweight proxy for Maple/OpenSecret's OpenAI-compatible inference
endpoints, with the security and privacy benefits of Trusted Execution
Environment (TEE) processing.

## 🚀 Features

- **OpenAI-Compatible Surface** - Models, chat completions, and embeddings endpoints
- **Attested TEE Transport** - The OpenSecret SDK establishes an attested,
  encrypted channel before inference requests are forwarded
- **Lossless Chat Parameters** - Provider-specific request fields pass through unchanged
- **Streaming and Non-Streaming** - Supports both chat completion response modes
- **Flexible Authentication** - Environment variables or per-request API keys
- **Familiar Clients** - Point compatible OpenAI clients at the proxy base URL
- **Lightweight** - Minimal overhead, maximum performance
- **Optional CORS** - Browser mode requires per-request keys and deployment review

## 📦 Installation

### As a Binary

Maple's current release workflow builds native proxy archives for Linux x86_64,
Linux ARM64, Apple Silicon macOS, and Windows x86_64 and attaches them to the
ordinary Maple app Release. Maple v3.3.9 completed the first post-integration
publication and verification of all four archives plus their checksum manifest.

After that release, verify the assets are present and use the stable download
URLs, for example:

```bash
curl -LO https://github.com/OpenSecretCloud/Maple/releases/latest/download/maple-proxy-linux-x86_64.tar.gz
curl -LO https://github.com/OpenSecretCloud/Maple/releases/latest/download/maple-proxy-release-final.sha256
sha256sum --check --ignore-missing maple-proxy-release-final.sha256
```

There is no separate proxy GitHub Release or proxy release tag. To build from
source now:

```bash
git clone https://github.com/OpenSecretCloud/Maple.git
cd Maple/proxy
cargo build --locked --release
```

### As a Library

Add to your `Cargo.toml`:

```toml
[dependencies]
maple-proxy = "0.3.2"
```

Crates.io publishing remains separate from Maple application releases; the
example above uses the latest published crate version.

That published 0.3.2 crate predates the in-tree dynamic TUF/Sigstore trust
path documented below. The new behavior is currently unreleased and must ship
only after the breaking Rust SDK release, with a breaking proxy version such as
0.4.0. Do not infer the behavior below from installing 0.3.2.

## ⚙️ Configuration

Set environment variables or use command-line arguments:

```bash
# Environment Variables
export MAPLE_HOST=127.0.0.1                    # Server host (default: 127.0.0.1)
export MAPLE_PORT=8080                         # Server port (default: 8080)
export MAPLE_BACKEND_URL=https://enclave.trymaple.ai   # Maple backend URL
export MAPLE_API_KEY=your-maple-api-key        # Optional for trusted, non-browser clients only
export MAPLE_DEBUG=true                        # Enable debug logging
export MAPLE_ENABLE_CORS=false                 # Default; see browser warning below
export MAPLE_REQUEST_TIMEOUT_SECS=300          # Response-start/non-streaming timeout
export MAPLE_STREAM_IDLE_TIMEOUT_SECS=300      # Streaming idle timeout between chunks
```

Initial attestation has its own timeout. Within each inference call, SDK-owned
session and authentication recovery share one cumulative recovery budget. Both
use a 15-minute minimum so a cold, bounded TUF root-rotation sequence is not cut
off by a shorter inference timeout. `MAPLE_REQUEST_TIMEOUT_SECS` applies
independently to each actual inference attempt through response headers and any
buffered non-streaming body; values above 15 minutes also extend the attestation
and recovery caps. After streaming headers arrive, the separate stream idle
timeout governs each response chunk.

Or use CLI arguments:
```bash
cargo run --locked -- --host 0.0.0.0 --port 8080 --backend-url https://enclave.trymaple.ai
```

For an unsigned local backend, use `just run-local`. That recipe alone enables
the explicitly named `insecure-local-mock-attestation` Cargo feature. Generic,
release, Docker, and embedded Maple builds leave the feature disabled.

The packaged proxy automatically selects attestation policy only for the exact
official Maple/OpenSecret origins. An arbitrary remote HTTPS backend needs a
custom library integration that supplies its own `TrustedReleaseConfig` and
bootstrap root; the proxy CLI does not yet expose that trust configuration.
Unknown remote origins therefore fail closed instead of inheriting Maple
production policy.

## 🛠️ Usage

### Using as a Binary

#### Start the Server

```bash
cargo run --locked
```

You should see:
```
🚀 Maple Proxy Server started successfully!
📋 Available endpoints:
   GET  /health              - Health check
   GET  /v1/models           - List available models
   POST /v1/chat/completions - Create chat completions (streaming & non-streaming)
   POST /v1/embeddings       - Create embeddings
```

### API Endpoints

#### List Models
```bash
curl http://localhost:8080/v1/models \
  -H "Authorization: Bearer YOUR_MAPLE_API_KEY"
```

#### Chat Completions
```bash
curl -N http://localhost:8080/v1/chat/completions \
  -H "Authorization: Bearer YOUR_MAPLE_API_KEY" \
  -H "Content-Type: application/json" \
  -d '{
    "model": "llama3-3-70b",
    "messages": [
      {"role": "user", "content": "Write a haiku about technology"}
    ],
    "stream": true
  }'
```

Set `stream` to `true` for Server-Sent Events or `false` for one JSON response.
Additional provider-specific JSON fields are forwarded without being parsed or
rewritten by the proxy or Rust SDK.

#### Embeddings
```bash
curl http://localhost:8080/v1/embeddings \
  -H "Authorization: Bearer YOUR_MAPLE_API_KEY" \
  -H "Content-Type: application/json" \
  -d '{
    "model": "nomic-embed-text",
    "input": "Generate an embedding for this text"
  }'
```

### Using as a Library

You can also embed Maple Proxy in your own Rust application:

```rust
use maple_proxy::{Config, create_app};
use tokio::net::TcpListener;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Initialize tracing
    tracing_subscriber::fmt::init();

    // Create config programmatically
    let config = Config::new(
        "127.0.0.1".to_string(),
        8081,  // Custom port
        "https://enclave.trymaple.ai".to_string(),
    )
    .with_api_key("your-api-key-here".to_string())
    .with_debug(true)
    .with_cors(true);

    // Create the app
    let app = create_app(config.clone());

    // Start the server
    let addr = config.socket_addr()?;
    let listener = TcpListener::bind(addr).await?;
    
    println!("Maple proxy server running on http://{}", addr);
    
    axum::serve(listener, app).await?;

    Ok(())
}
```

Run the example:
```bash
cargo run --locked --example library_usage
```

## 💻 Client Examples

### Python (OpenAI Library)

```python
import openai

client = openai.OpenAI(
    api_key="YOUR_MAPLE_API_KEY",
    base_url="http://localhost:8080/v1"
)

# Streaming chat completion
stream = client.chat.completions.create(
    model="llama3-3-70b",
    messages=[{"role": "user", "content": "Hello, world!"}],
    stream=True
)

for chunk in stream:
    if chunk.choices[0].delta.content is not None:
        print(chunk.choices[0].delta.content, end="")
```

### JavaScript/Node.js

```javascript
import OpenAI from 'openai';

const openai = new OpenAI({
  apiKey: 'YOUR_MAPLE_API_KEY',
  baseURL: 'http://localhost:8080/v1',
});

const stream = await openai.chat.completions.create({
  model: 'llama3-3-70b',
  messages: [{ role: 'user', content: 'Hello!' }],
  stream: true,
});

for await (const chunk of stream) {
  process.stdout.write(chunk.choices[0]?.delta?.content || '');
}
```

### cURL

```bash
# Health check
curl http://localhost:8080/health

# List models
curl http://localhost:8080/v1/models \
  -H "Authorization: Bearer YOUR_MAPLE_API_KEY"

# Streaming chat completion
curl -N http://localhost:8080/v1/chat/completions \
  -H "Authorization: Bearer YOUR_MAPLE_API_KEY" \
  -H "Content-Type: application/json" \
  -d '{
    "model": "llama3-3-70b",
    "messages": [{"role": "user", "content": "Tell me a joke"}],
    "stream": true
  }'

# Embeddings
curl http://localhost:8080/v1/embeddings \
  -H "Authorization: Bearer YOUR_MAPLE_API_KEY" \
  -H "Content-Type: application/json" \
  -d '{
    "model": "nomic-embed-text",
    "input": "Generate an embedding for this text"
  }'
```

## 🔐 Authentication

Maple Proxy supports two authentication methods:

### 1. Environment Variable (Default)
Set `MAPLE_API_KEY` - all requests will use this key by default:
```bash
export MAPLE_API_KEY=your-maple-api-key
cargo run --locked
```

### 2. Per-Request Authorization Header
Override the default key or provide one if not set:
```bash
curl -H "Authorization: Bearer different-api-key" ...
```

## 🌐 CORS Support

The standalone proxy does not inherit Maple's Tauri wrapper protections. If a
browser must call it, do not configure `MAPLE_API_KEY`; require every request
to carry its own bearer key and review the network exposure separately before
enabling CORS:

```bash
unset MAPLE_API_KEY
export MAPLE_ENABLE_CORS=true
cargo run --locked
```

## 🐳 Docker Deployment

### Pre-built Image

The currently published GHCR `latest` image is the independently released
standalone proxy 0.3.2. The first Maple-owned publication deliberately does not
backfill the in-tree 0.3.3 version. After the next proxy version change ships in
a successful stable Maple Release, the release-following publisher updates the
same `ghcr.io/opensecretcloud/maple-proxy` package automatically.

To run the legacy image deliberately:

```bash
# Pull the latest image
docker pull ghcr.io/opensecretcloud/maple-proxy:latest

# Run with your API key
docker run -p 8080:8080 \
  -e MAPLE_BACKEND_URL=https://enclave.trymaple.ai \
  -e MAPLE_REQUEST_TIMEOUT_SECS=300 \
  -e MAPLE_STREAM_IDLE_TIMEOUT_SECS=300 \
  ghcr.io/opensecretcloud/maple-proxy:latest
```

### Build from Source

```bash
# Build the image locally
just docker-build

# Run the container
just docker-run
```

### Production Docker Setup

1. **Option A: Use the published image from GHCR**
```bash
# In your docker-compose.yml, use:
image: ghcr.io/opensecretcloud/maple-proxy:latest
```

2. **Option B: Build your own image**
```bash
docker build -f Dockerfile -t maple-proxy:latest ..
```

3. **Run with docker-compose:**
```bash
# Copy the example environment file
cp .env.example .env

# Edit .env with your configuration
vim .env

# Start the service
docker-compose up -d
```

### 🔒 Security Note for Public Deployments

When deploying Maple Proxy on a public network:

- **DO NOT** set `MAPLE_API_KEY` in the container environment
- Instead, require clients to pass their API key with each request:

```python
# Client-side authentication for public proxy
client = OpenAI(
    base_url="https://your-proxy.example.com/v1",
    api_key="user-specific-maple-api-key"  # Each user provides their own key
)
```

This keeps API keys out of the shared container configuration and allows each
client to supply its own credential. Review proxy logs and browser policy
before treating a public deployment as safe.

### Docker Commands

```bash
# Build image
just docker-build

# Run interactively
just docker-run

# Run in background
just docker-run-detached

# View logs
just docker-logs

# Stop container
just docker-stop

# Use docker-compose
just compose-up
just compose-logs
just compose-down
```

### Container Configuration

The Docker image:
- Uses multi-stage builds for minimal size (~130MB)
- Runs as non-root user for security
- Includes health checks
- Supports both x86_64 and ARM architectures

### Environment Variables for Docker

```yaml
# docker-compose.yml environment section
environment:
  - MAPLE_BACKEND_URL=https://enclave.trymaple.ai  # Production backend
  - MAPLE_ENABLE_CORS=true                         # Enable for web apps
  - MAPLE_REQUEST_TIMEOUT_SECS=300                 # Response-start/non-streaming timeout
  - MAPLE_STREAM_IDLE_TIMEOUT_SECS=300             # Streaming idle timeout
  - RUST_LOG=info                                  # Logging level
  # - MAPLE_API_KEY=xxx                            # Only for private deployments!
```

## 🔧 Development

### Docker Images & CI/CD

The source lives under [`proxy/`](https://github.com/OpenSecretCloud/Maple/tree/master/proxy)
in the Maple repository. Root, path-scoped workflows run locked Rust checks,
supply-chain policy, and non-publishing AMD64/ARM64 container builds for proxy
changes. After every successful stable Maple Release, a separate serialized
publisher compares the checked-in proxy version with the previous stable Maple
Release and rejects changed container inputs without a version bump. Version
`0.3.3` remains the explicit unbackfilled migration baseline. A new or missing
later version publishes native AMD64/ARM64 images with per-platform provenance,
verifies their exact build digests and release labels, then reconciles the minor,
major, and `latest` aliases. Existing exact versions are verified without being
overwritten, and manual dispatch safely retries verification or alias repair.
Maple must have Actions write access to the existing organization-scoped GHCR
package; local recipes intentionally cannot publish to it.

`proxy/Cargo.lock`, `sdk/rust/Cargo.lock`, and
`frontend/src-tauri/Cargo.lock` remain separate lockfiles. Runtime dependency
changes must keep the path graph and all affected locks coherent. Docker builds
must use the Maple repository root as context so both `proxy/` and `sdk/rust/`
are available. GitHub Release archives, crates.io, and GHCR are three separate
publication paths; completing one does not update the others.

**Local Development (Justfile)**
```bash
# For local testing and debugging
just docker-build        # Build locally
just docker-run          # Test locally
```

### Build from Source
```bash
cargo build --locked
```

### Run with Debug Logging
```bash
export MAPLE_DEBUG=true
cargo run --locked
```

### Run Tests
```bash
cargo test --locked
```

## 📊 Supported Models

Maple Proxy supports all models available in the Maple/OpenSecret platform, including:
- `llama3-3-70b` - Llama 3.3 70B parameter model
- `nomic-embed-text` - Embedding model for `/v1/embeddings`
- And many others - check `/v1/models` endpoint for current list

## 🔍 Troubleshooting

### Common Issues

**"No API key provided"**
- Set `MAPLE_API_KEY` environment variable or provide `Authorization: Bearer <key>` header

**"Failed to establish secure connection"**
- Check your `MAPLE_BACKEND_URL` is correct
- Ensure your API key is valid
- Check network connectivity

**Connection refused**
- Make sure the server is running on the specified host/port
- Check firewall settings

### Debug Mode

Enable debug logging for detailed information:
```bash
export MAPLE_DEBUG=true
cargo run --locked
```

## 🏗️ Architecture

```
┌─────────────────┐    ┌─────────────────┐    ┌─────────────────┐
│   OpenAI Client │───▶│   Maple Proxy   │───▶│  Maple Backend  │
│   (Python/JS)   │    │   (localhost)   │    │      (TEE)      │
└─────────────────┘    └─────────────────┘    └─────────────────┘
```

1. **Client** makes standard OpenAI API calls to localhost
2. **Maple Proxy** handles authentication and asks the OpenSecret SDK to
   establish the TEE channel
3. **OpenSecret SDK** authenticates and authorizes the enclave before accepting
   its key and completing key exchange
4. **Requests** are encrypted and forwarded to Maple's TEE infrastructure
5. **Responses** are streamed back to the client in OpenAI format

### TEE release authorization

Sigstore/Rekor release authorization belongs in the OpenSecret SDK rather than
Maple Proxy. For each non-local backend, the SDK:

1. refreshes signed release policy from `https://attestations.trymaple.ai/tuf`,
   verifies its TUF chain and each selected portable Sigstore bundle locally;
2. creates a fresh nonce, requests the AWS Nitro attestation document, and
   verifies its certificate chain, nonce, and signature;
3. extracts the complete PCR0/PCR1/PCR2 measurement tuple;
4. rechecks the held policy against current local rollback state and expiry,
   then compares the tuple with one complete active release manifest; and
5. accepts the enclave public key and performs key exchange only after that
   atomic tuple is authorized.

Maple Proxy continues to call `perform_attestation_handshake`; it neither
maintains a second PCR allowlist nor implements a separate Sigstore verifier.
Keeping this policy in the SDK gives every Rust SDK consumer the same
fail-closed authorization boundary before application data is sent.

Runtime policy requests go only to `attestations.trymaple.ai`; the SDK does not
contact GitHub, Fulcio, Rekor, or Sigstore's TUF service during a handshake.
It starts from its embedded Maple TUF root, verifies current expiring policy and
the exact immutable manifest/bundle bytes, and performs the Sigstore verification
offline with TUF-authenticated trust roots and builder policy.

Sigstore makes a release statement and its signing identity tamper-evident in
an append-only transparency log. It does **not** prove that an artifact was
reproducibly built or decide whether a historical release is still current.
Reproducibility remains a separate Nix rebuild/compare property; TUF supplies
current authorization, bounded freshness, explicit rollback, and revocation.

> **Integration status:** the embedded TUF root is intentionally an unconfigured,
> fail-closed placeholder until production bootstrap is reviewed. Remote
> handshakes therefore fail closed in this draft branch; no release or policy is
> published by this change.

## 📝 License

MIT License - see LICENSE file for details.

## 🤝 Contributing

Contributions welcome! Please feel free to submit a Pull Request.
