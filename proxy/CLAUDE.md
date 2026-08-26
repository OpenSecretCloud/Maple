# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Project Overview

Maple Proxy is a lightweight OpenAI-compatible proxy server that forwards requests to Maple/OpenSecret's TEE (Trusted Execution Environment) infrastructure. It acts as a translation layer between OpenAI client libraries and the OpenSecret backend, enabling secure AI processing in trusted enclaves.

## Common Development Commands

### Build and Run
- `just run` - Start the development server (loads config from .env)
- `just run-local` - Run pointing to local backend (http://localhost:3000)
- `just run-prod` - Run pointing to production backend (https://enclave.trymaple.ai)
- `just build` - Build debug binary
- `just release` - Build optimized release binary
- `cargo run --locked` - Run directly with cargo

### Testing and Quality
- `just test` - Run all tests
- `just fmt` or `just format` - Format code with rustfmt
- `just lint` or `just clippy` - Run clippy lints with strict warnings
- `just check` - Run format, lint, and test in sequence

### Docker Operations
- `just docker-build` - Build Docker image locally
- `just docker-run` - Run container interactively
- `just docker-run-detached` - Run container in background
- `just compose-up` - Start with docker-compose
- `just compose-down` - Stop docker-compose services

## Architecture

### Core Components

1. **main.rs** - Entry point that initializes the server with configuration and starts the Axum web server on the configured host/port.

2. **lib.rs** - Library root that exports the main `create_app` function, which builds the Axum router with:
   - Health check endpoints (/, /health)
   - OpenAI-compatible endpoints (/v1/models, /v1/chat/completions)
   - Optional CORS support
   - Request tracing

3. **config.rs** - Configuration management using clap for CLI args and environment variables:
   - Server settings (host, port)
   - Backend URL configuration
   - API key management
   - Debug and CORS flags
   - OpenAI-compatible error types

4. **proxy.rs** - Core proxy logic that:
   - Extracts API keys from Authorization headers or falls back to default
   - Creates OpenSecret client and performs attestation handshake
   - Forwards requests to the TEE backend
   - Handles streaming responses for chat completions
   - Transforms responses to OpenAI format

### Request Flow

1. Client sends OpenAI-compatible request to proxy
2. Proxy extracts API key (from header or default config)
3. Creates OpenSecret client and performs TEE attestation
4. Forwards request to Maple backend (enclave.trymaple.ai or configured URL)
5. Streams response back to client in OpenAI format

### Authentication

The proxy supports two authentication modes:
- **Default API Key**: Set via `MAPLE_API_KEY` environment variable
- **Per-Request**: Clients provide `Authorization: Bearer <key>` header

For public deployments, avoid setting default API key to require per-request authentication.

## Configuration

Environment variables (can be set in .env file):
- `MAPLE_HOST` - Server bind address (default: 127.0.0.1)
- `MAPLE_PORT` - Server port (default: 8080)
- `MAPLE_BACKEND_URL` - OpenSecret backend URL (default: https://enclave.trymaple.ai)
- `MAPLE_API_KEY` - Default API key (optional)
- `MAPLE_DEBUG` - Enable debug logging
- `MAPLE_ENABLE_CORS` - Enable CORS for web clients
- `MAPLE_REQUEST_TIMEOUT_SECS` - Backend request timeout in seconds (default: 300)
- `MAPLE_STREAM_IDLE_TIMEOUT_SECS` - Streaming idle timeout in seconds (default: 300)

## Testing

Tests are located in `tests/` directory. Currently includes:
- `health_test.rs` - Tests for health check endpoints

Run tests with `just test` or `cargo test --locked`.

## Dependencies

Key dependencies:
- **opensecret** - Official OpenSecret SDK for TEE communication
- **axum** - Web framework for the HTTP server
- **tokio** - Async runtime
- **tower/tower-http** - Middleware for CORS and tracing
- **clap** - CLI argument parsing
- **dotenvy** - .env file support
