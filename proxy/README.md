# 🍁 Maple Proxy

A lightweight proxy for Maple/OpenSecret's OpenAI-compatible inference
endpoints, with the security and privacy benefits of Trusted Execution
Environment (TEE) processing.

## 🚀 Features

- **OpenAI-Compatible Surface** - Models, chat completions, and embeddings endpoints
- **Secure TEE Processing** - All requests processed in secure enclaves
- **Lossless Chat Parameters** - Provider-specific request fields pass through unchanged
- **Streaming and Non-Streaming** - Supports both chat completion response modes
- **Flexible Authentication** - Environment variables or per-request API keys
- **Familiar Clients** - Point compatible OpenAI clients at the proxy base URL
- **Lightweight** - Minimal overhead, maximum performance
- **CORS Support** - Ready for web applications

## 📦 Installation

### As a Binary

Every Maple GitHub Release includes native proxy archives for Linux x86_64,
Linux ARM64, Apple Silicon macOS, and Windows x86_64. The stable download URLs
use the ordinary Maple release, for example:

```bash
curl -LO https://github.com/OpenSecretCloud/Maple/releases/latest/download/maple-proxy-linux-x86_64.tar.gz
curl -LO https://github.com/OpenSecretCloud/Maple/releases/latest/download/maple-proxy-release-final.sha256
sha256sum --check --ignore-missing maple-proxy-release-final.sha256
```

There is no separate proxy GitHub Release or proxy release tag. To build from
source instead:

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

## ⚙️ Configuration

Set environment variables or use command-line arguments:

```bash
# Environment Variables
export MAPLE_HOST=127.0.0.1                    # Server host (default: 127.0.0.1)
export MAPLE_PORT=8080                         # Server port (default: 8080)
export MAPLE_BACKEND_URL=http://localhost:3000         # Maple backend URL (prod: https://enclave.trymaple.ai)
export MAPLE_PCR0_ENVIRONMENT=production       # PCR0 trust roots: production (default) or development
export MAPLE_API_KEY=your-maple-api-key        # Default API key (optional)
export MAPLE_DEBUG=true                        # Enable debug logging
export MAPLE_ENABLE_CORS=true                  # Enable CORS
export MAPLE_REQUEST_TIMEOUT_SECS=300          # Backend request timeout
export MAPLE_STREAM_IDLE_TIMEOUT_SECS=300      # Streaming idle timeout between chunks
```

Or use CLI arguments:
```bash
cargo run --locked -- --host 0.0.0.0 --port 8080 --backend-url https://enclave.trymaple.ai

# Development enclaves must be selected explicitly
cargo run --locked -- --backend-url https://enclave.secretgpt.ai --pcr0-environment development
```

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
use maple_proxy::{Config, Pcr0Environment, create_app};
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
    .with_pcr0_environment(Pcr0Environment::Production)
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

Enable CORS for web applications:
```bash
export MAPLE_ENABLE_CORS=true
cargo run --locked
```

## 🐳 Docker Deployment

### Quick Start with Pre-built Image

Pull and run the official image from GitHub Container Registry:

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

1. **Option A: Use pre-built image from GHCR**
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

This ensures:
- Users' API keys remain private
- Multiple users can share the same proxy instance
- No API keys are exposed in container configurations

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
- Optimizes dependency caching with cargo-chef
- Supports both x86_64 and ARM architectures

### Environment Variables for Docker

```yaml
# docker-compose.yml environment section
environment:
  - MAPLE_BACKEND_URL=https://enclave.trymaple.ai  # Production backend
  - MAPLE_ENABLE_CORS=true                         # Enable for web apps
  - MAPLE_REQUEST_TIMEOUT_SECS=300                 # Backend request timeout
  - MAPLE_STREAM_IDLE_TIMEOUT_SECS=300             # Streaming idle timeout
  - RUST_LOG=info                                  # Logging level
  # - MAPLE_API_KEY=xxx                            # Only for private deployments!
```

## 🔧 Development

### Docker Images & CI/CD

The source lives under [`proxy/`](https://github.com/OpenSecretCloud/Maple/tree/master/proxy)
in the Maple repository. Root, path-scoped workflows run locked Rust checks,
supply-chain policy, and non-publishing AMD64/ARM64 container builds for proxy
changes. Production publishing is intentionally handled separately from these
CI checks.

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
2. **Maple Proxy** handles authentication and TEE handshake
3. **Requests** are securely forwarded to Maple's TEE infrastructure
4. **Responses** are streamed back to the client in OpenAI format

## 📝 License

MIT License - see LICENSE file for details.

## 🤝 Contributing

Contributions welcome! Please feel free to submit a Pull Request.
