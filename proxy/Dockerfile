# Build stage with cargo-chef for dependency caching
FROM docker.io/lukemathwalker/cargo-chef:latest-rust-1 AS chef
WORKDIR /app

# Plan stage - prepare dependency list for caching
FROM chef AS planner
COPY proxy/Cargo.toml proxy/Cargo.lock ./proxy/
COPY proxy/src ./proxy/src
COPY sdk/rust/Cargo.toml ./sdk/rust/
COPY sdk/rust/src ./sdk/rust/src
COPY sdk/rust/assets ./sdk/rust/assets
WORKDIR /app/proxy
RUN cargo chef prepare --recipe-path recipe.json

# Build stage - build dependencies separately for caching
FROM chef AS builder
WORKDIR /app/proxy

# Install build dependencies
RUN apt-get update && apt-get install -y \
    pkg-config \
    libssl-dev \
    && rm -rf /var/lib/apt/lists/*

# Copy and build dependencies (cached if unchanged)
COPY --from=planner /app/proxy/recipe.json recipe.json
RUN cargo chef cook --locked --release --recipe-path recipe.json

# Copy source code and build the application
COPY proxy/Cargo.toml proxy/Cargo.lock ./
COPY proxy/src ./src
COPY sdk/rust/Cargo.toml /app/sdk/rust/Cargo.toml
COPY sdk/rust/src /app/sdk/rust/src
COPY sdk/rust/assets /app/sdk/rust/assets
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
COPY --from=builder /app/target/release/maple-proxy /usr/local/bin/maple-proxy

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
