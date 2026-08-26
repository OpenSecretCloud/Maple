# Justfile for Maple Proxy
# Development commands for the OpenAI-compatible proxy server

# Load environment variables from .env file
set dotenv-load

# Set the container runtime (docker or podman)
container := env_var_or_default("CONTAINER_RUNTIME", "podman")

# Default command - show available commands
default:
    @just --list

# Set up development environment
setup:
    @echo "🔧 Setting up development environment..."
    @bash setup-hooks.sh
    @cargo check --locked --all-features
    @echo "✅ Development environment ready"

# Format code with rustfmt
format:
    @echo "🎨 Formatting code..."
    @cargo fmt
    @echo "✅ Code formatted"

# Alias for format
fmt: format

# Run clippy lints
lint:
    @echo "🔍 Running clippy lints..."
    @cargo clippy --locked --all-targets --all-features -- -D warnings
    @echo "✅ Lints passed"

# Alias for lint
clippy: lint

# Run all tests
test:
    @echo "🧪 Running tests..."
    @cargo test --locked --all-features
    @echo "✅ Tests passed"

# Run all checks (format, lint, test)
check: format lint test
    @echo "✅ All checks passed"

# Run the development server
run:
    @echo "🚀 Starting Maple Proxy server..."
    @echo "📝 Loading configuration from .env"
    @cargo run --locked

# Run with custom backend URL (preserves .env variables)
run-with-backend url:
    @echo "🚀 Starting Maple Proxy server..."
    @echo "🔗 Backend: {{url}}"
    @echo "📝 Loading other configs from .env"
    @bash -c 'set -a; source .env 2>/dev/null; set +a; MAPLE_BACKEND_URL={{url}} cargo run --locked'

# Run pointing to local backend
run-local:
    @just run-with-backend "http://localhost:3000"

# Run pointing to production backend
run-prod:
    @just run-with-backend "https://enclave.trymaple.ai"

# Build debug binary
build:
    @echo "🔨 Building debug binary..."
    @cargo build --locked
    @echo "✅ Debug binary built at target/debug/maple-proxy"

# Build release binary
release:
    @echo "📦 Building release binary..."
    @cargo build --locked --release
    @echo "✅ Release binary built at target/release/maple-proxy"

# Build for all targets (used in CI)
build-all:
    @echo "📦 Building for all targets..."
    @cargo build --locked --all-targets --all-features
    @echo "✅ All targets built"

# Clean build artifacts
clean:
    @echo "🧹 Cleaning build artifacts..."
    @cargo clean
    @echo "✅ Build artifacts cleaned"

# Update dependencies
update:
    @echo "📦 Updating dependencies..."
    @cargo update
    @echo "✅ Dependencies updated"

# Install to ~/.cargo/bin
install:
    @echo "📥 Installing maple-proxy..."
    @cargo install --locked --path .
    @echo "✅ Installed to ~/.cargo/bin/maple-proxy"

# Uninstall from ~/.cargo/bin
uninstall:
    @echo "📤 Uninstalling maple-proxy..."
    @cargo uninstall maple-proxy
    @echo "✅ Uninstalled"

# Run with verbose logging
debug:
    @echo "🐛 Starting with debug logging..."
    RUST_LOG=debug MAPLE_DEBUG=true cargo run --locked

# Check for security vulnerabilities
audit:
    @echo "🔒 Running security audit..."
    @cargo audit
    @echo "✅ Security audit complete"

# Generate documentation
doc:
    @echo "📚 Generating documentation..."
    @cargo doc --locked --no-deps --all-features --open
    @echo "✅ Documentation generated"

# Show code coverage (requires cargo-tarpaulin)
coverage:
    @echo "📊 Generating code coverage..."
    @cargo metadata --locked --format-version 1 >/dev/null
    @cargo tarpaulin --verbose --all-features --workspace --timeout 120 --out html
    @echo "✅ Coverage report generated at tarpaulin-report.html"

# Watch for changes and run tests
watch:
    @echo "👁️ Watching for changes..."
    @cargo watch -x 'test --locked'

# Create a new git commit with conventional commit message
commit message:
    @git add -A
    @git commit -m "{{message}}"
    @echo "✅ Changes committed"

# Quick test with curl
test-curl:
    @echo "🧪 Testing with curl..."
    @curl -N http://localhost:8080/v1/chat/completions \
        -H "Content-Type: application/json" \
        -d '{"model": "llama3-3-70b", "messages": [{"role": "user", "content": "Say hello"}], "stream": true}'

# Show environment info
env:
    @echo "🌍 Environment Info"
    @echo "==================="
    @echo "Rust version: $(rustc --version)"
    @echo "Cargo version: $(cargo --version)"
    @echo "Just version: $(just --version)"
    @echo "Container runtime: {{container}} $({{container}} --version 2>/dev/null | head -1 || echo 'not installed')"
    @echo ""
    @echo "📋 Environment Variables:"
    @echo "MAPLE_HOST: ${MAPLE_HOST:-127.0.0.1}"
    @echo "MAPLE_PORT: ${MAPLE_PORT:-8080}"
    @echo "MAPLE_BACKEND_URL: ${MAPLE_BACKEND_URL:-https://enclave.trymaple.ai}"
    @echo "MAPLE_PCR0_ENVIRONMENT: ${MAPLE_PCR0_ENVIRONMENT:-production}"
    @echo "MAPLE_API_KEY: ${MAPLE_API_KEY:-[not set]}"
    @echo "MAPLE_DEBUG: ${MAPLE_DEBUG:-false}"
    @echo "MAPLE_ENABLE_CORS: ${MAPLE_ENABLE_CORS:-false}"
    @echo "MAPLE_REQUEST_TIMEOUT_SECS: ${MAPLE_REQUEST_TIMEOUT_SECS:-300}"
    @echo "MAPLE_STREAM_IDLE_TIMEOUT_SECS: ${MAPLE_STREAM_IDLE_TIMEOUT_SECS:-300}"

# Build Docker image
docker-build:
    @echo "🐳 Building Docker image with {{container}}..."
    @{{container}} build -t maple-proxy:latest .
    @echo "✅ Docker image built: maple-proxy:latest"

# Run Docker container
docker-run:
    @echo "🚀 Running Docker container with {{container}}..."
    @{{container}} run --rm -it \
        -p ${MAPLE_PORT:-8080}:8080 \
        -e MAPLE_API_KEY=${MAPLE_API_KEY} \
        -e MAPLE_BACKEND_URL=${MAPLE_BACKEND_URL:-https://enclave.trymaple.ai} \
        -e MAPLE_PCR0_ENVIRONMENT=${MAPLE_PCR0_ENVIRONMENT:-production} \
        -e MAPLE_DEBUG=${MAPLE_DEBUG:-false} \
        -e MAPLE_ENABLE_CORS=${MAPLE_ENABLE_CORS:-true} \
        -e MAPLE_REQUEST_TIMEOUT_SECS=${MAPLE_REQUEST_TIMEOUT_SECS:-300} \
        -e MAPLE_STREAM_IDLE_TIMEOUT_SECS=${MAPLE_STREAM_IDLE_TIMEOUT_SECS:-300} \
        maple-proxy:latest

# Run Docker container in detached mode
docker-run-detached:
    @echo "🚀 Running Docker container in background with {{container}}..."
    @{{container}} run -d \
        --name maple-proxy \
        -p ${MAPLE_PORT:-8080}:8080 \
        -e MAPLE_API_KEY=${MAPLE_API_KEY} \
        -e MAPLE_BACKEND_URL=${MAPLE_BACKEND_URL:-https://enclave.trymaple.ai} \
        -e MAPLE_PCR0_ENVIRONMENT=${MAPLE_PCR0_ENVIRONMENT:-production} \
        -e MAPLE_DEBUG=${MAPLE_DEBUG:-false} \
        -e MAPLE_ENABLE_CORS=${MAPLE_ENABLE_CORS:-true} \
        -e MAPLE_REQUEST_TIMEOUT_SECS=${MAPLE_REQUEST_TIMEOUT_SECS:-300} \
        -e MAPLE_STREAM_IDLE_TIMEOUT_SECS=${MAPLE_STREAM_IDLE_TIMEOUT_SECS:-300} \
        maple-proxy:latest
    @echo "✅ Container started. Use 'just docker-stop' to stop it."

# Stop Docker container
docker-stop:
    @echo "🛑 Stopping Docker container..."
    @{{container}} stop maple-proxy 2>/dev/null || echo "Container not running"
    @{{container}} rm maple-proxy 2>/dev/null || true
    @echo "✅ Container stopped"

# View Docker logs
docker-logs:
    @{{container}} logs -f maple-proxy 2>/dev/null || echo "Container not running"

# Run with docker-compose
compose-up:
    @echo "🚀 Starting services with docker-compose..."
    @{{container}}-compose up -d
    @echo "✅ Services started. Use 'just compose-down' to stop."

# Stop docker-compose services
compose-down:
    @echo "🛑 Stopping services..."
    @{{container}}-compose down
    @echo "✅ Services stopped"

# View docker-compose logs
compose-logs:
    @{{container}}-compose logs -f

# Clean Docker images
docker-clean:
    @echo "🧹 Cleaning Docker images..."
    @{{container}} rmi maple-proxy:latest 2>/dev/null || true
    @echo "✅ Docker images cleaned"

# Push to GitHub Container Registry
ghcr-push tag="latest":
    @echo "📦 Pushing to GitHub Container Registry..."
    @{{container}} tag maple-proxy:latest ghcr.io/opensecretcloud/maple-proxy:{{tag}}
    @{{container}} push ghcr.io/opensecretcloud/maple-proxy:{{tag}}
    @echo "✅ Pushed to ghcr.io/opensecretcloud/maple-proxy:{{tag}}"

# Build and push to GHCR
ghcr-build-push tag="latest":
    @echo "🐳 Building and pushing to GHCR..."
    @{{container}} build -t ghcr.io/opensecretcloud/maple-proxy:{{tag}} .
    @{{container}} push ghcr.io/opensecretcloud/maple-proxy:{{tag}}
    @echo "✅ Image available at ghcr.io/opensecretcloud/maple-proxy:{{tag}}"

# Login to GitHub Container Registry (requires PAT token)
ghcr-login:
    @echo "🔐 Logging in to GitHub Container Registry..."
    @echo "Please ensure you have a GitHub Personal Access Token with 'write:packages' scope"
    @echo "${GITHUB_TOKEN}" | {{container}} login ghcr.io -u ${GITHUB_USER} --password-stdin
    @echo "✅ Logged in to ghcr.io"

# Pull from GitHub Container Registry
ghcr-pull tag="latest":
    @echo "📥 Pulling from GitHub Container Registry..."
    @{{container}} pull ghcr.io/opensecretcloud/maple-proxy:{{tag}}
    @echo "✅ Pulled ghcr.io/opensecretcloud/maple-proxy:{{tag}}"

# Compatibility alias for the complete repository check.
check-all: check
