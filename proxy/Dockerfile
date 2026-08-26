# Build the proxy and its in-tree Rust SDK dependency together.
FROM docker.io/library/rust:1.89.0-bookworm AS builder
WORKDIR /app

# Install build dependencies
RUN apt-get update && apt-get install -y \
    pkg-config \
    libssl-dev \
    && rm -rf /var/lib/apt/lists/*

# Copy source code and build the application
COPY proxy/Cargo.toml proxy/Cargo.lock ./proxy/
COPY proxy/src ./proxy/src
COPY sdk/rust/Cargo.toml ./sdk/rust/
COPY sdk/rust/src ./sdk/rust/src
COPY sdk/rust/assets ./sdk/rust/assets
WORKDIR /app/proxy
RUN cargo build --locked --release --bin maple-proxy

# Runtime stage - minimal image for production
FROM docker.io/debian:bookworm-slim AS runtime

# Install runtime dependencies
RUN apt-get update && apt-get install -y \
    ca-certificates \
    libssl3 \
    curl \
    && rm -rf /var/lib/apt/lists/*

# Create non-root user for security
RUN useradd -m -u 1001 -s /bin/bash maple

WORKDIR /app

# Copy the binary from builder
COPY --from=builder /app/proxy/target/release/maple-proxy /usr/local/bin/maple-proxy

# Set ownership
RUN chown -R maple:maple /app

# Switch to non-root user
USER maple

# Production environment variables
ENV MAPLE_HOST=0.0.0.0 \
    MAPLE_PORT=8080 \
    MAPLE_BACKEND_URL=https://enclave.trymaple.ai \
    MAPLE_PCR0_ENVIRONMENT=production \
    MAPLE_DEBUG=false \
    MAPLE_ENABLE_CORS=true \
    MAPLE_REQUEST_TIMEOUT_SECS=300 \
    MAPLE_STREAM_IDLE_TIMEOUT_SECS=300 \
    RUST_LOG=info

# Expose the port
EXPOSE 8080

# Health check (curl needs to be installed)
HEALTHCHECK --interval=30s --timeout=3s --start-period=5s --retries=3 \
    CMD curl -f http://localhost:8080/health || exit 1

# Run the binary
ENTRYPOINT ["/usr/local/bin/maple-proxy"]
