# OpenSecret Rust SDK

Rust SDK for OpenSecret - secure AI API interactions with nitro attestation.

## Features

- 🔐 **Nitro Attestation**: Verify server identity through AWS Nitro Enclaves
- 🔑 **End-to-End Encryption**: All API calls encrypted with session keys
- 👤 **Authentication**: Support for both email-based and guest users
- 🔄 **Token Management**: Automatic token refresh and session management
- ↔️ **Lossless Inference Transport**: Provider-specific request and response fields pass through as raw bytes
- 🛡️ **Secure by Default**: No plaintext data transmission

## Installation

Add to your `Cargo.toml`:

```toml
[dependencies]
opensecret = "3.4.0"
bytes = "1"
futures = "0.3"
http = "1"
```

## Quick Start

```rust
use opensecret::{OpenSecretClient, Pcr0TrustPolicy, Result};
use uuid::Uuid;

#[tokio::main]
async fn main() -> Result<()> {
    // Initialize client
    let client = OpenSecretClient::new("https://api.opensecret.com")?;
    let client_id = Uuid::parse_str("your-client-id")?;

    // Establish secure session
    client.perform_attestation_handshake().await?;

    // Register and login
    let response = client.register(
        "user@example.com".to_string(),
        "password".to_string(),
        client_id,
        Some("John Doe".to_string())
    ).await?;

    println!("Logged in as: {}", response.id);
    Ok(())
}
```

Production clients verify both the AWS Nitro attestation and the enclave's
PCR0 deployment identity. `OpenSecretClient::new` uses pinned official PCR0
values and OpenSecret's signed production and development histories. Custom
deployments can add a static allowlist without replacing official trust:

```rust
let policy = Pcr0TrustPolicy::official().with_additional_pcr0s([
    "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef",
])?;
let client = OpenSecretClient::new_with_pcr0_trust_policy(
    "https://api.opensecret.cloud",
    policy,
)?;
```

Use `Pcr0TrustPolicy::from_static_allowlist(...)` to disable remote history and
trust only an explicit custom set. Remote entries are size/time bounded and
must verify against the SDK's hardcoded OpenSecret P-384 signing key. Exact
localhost, loopback, and unspecified-address development endpoints continue to
use mock attestation; Android also supports the exact emulator alias
`10.0.2.2`. Other endpoints must use HTTPS.

## Inference APIs

`send_inference_request` is the lossless inference API. The caller owns the
HTTP method, route, query, headers, body bytes, and response parsing; the SDK
owns attestation, authentication, encryption, retrying an expired session, and
response decryption. It does not parse or rewrite inference parameters such as
`stream`:

```rust
use bytes::Bytes;
use futures::StreamExt;
use http::Request;

let request = Request::post("/v1/chat/completions")
    .header("x-provider-option", "value")
    .body(Bytes::from_static(br#"{
        "model": "provider-model",
        "messages": [{"role": "user", "content": "Hello"}],
        "provider_option": {"enabled": true}
    }"#))
    .expect("valid inference request");

let response = client.send_inference_request(request).await?;
let status = response.status();
let mut body = response.into_body();
while let Some(chunk) = body.next().await {
    let decrypted_bytes = chunk?;
    // Parse or forward the bytes in the calling application.
}
```

The transport accepts only the SDK's explicitly allowed inference routes;
it cannot be used to bypass JWT-only account or conversation APIs. The existing
typed model, embedding, and chat-completion helpers remain available as
compatibility wrappers over the same transport.

The SDK manages transport credentials and framing. Caller-provided `Host`,
`Authorization`, `x-session-id`, `Content-Length`, `Content-Type`,
`Content-Encoding`, `Accept-Encoding`, `Content-MD5`, `Digest`, hop-by-hop, and
`Connection`-listed headers are not forwarded; other headers are preserved.

## Authentication

### User Registration

Register with email:
```rust
let response = client.register(
    email,
    password,
    client_id,
    Some(name)  // Optional
).await?;
```

Register as guest (no email):
```rust
let response = client.register_guest(
    password,
    client_id
).await?;
```

### Login

Login with email:
```rust
let response = client.login(
    email,
    password,
    client_id
).await?;
```

Login with user ID (guests only):
```rust
let response = client.login_with_id(
    user_id,
    password,
    client_id
).await?;
```

### Token Management

Tokens are automatically stored after login/registration. You can:

```rust
// Get one coherent access/refresh pair snapshot
let tokens = client.get_tokens()?;

// Individual reads remain available when a coherent pair is not required
let access_token = client.get_access_token()?;
let refresh_token = client.get_refresh_token()?;

// Refresh tokens
client.refresh_token().await?;

// Logout (clears session and tokens)
client.logout().await?;
```

## Session Management

Every API call requires an encrypted session:

1. **Attestation Handshake**: Establishes trust and exchanges encryption keys
2. **Encrypted Communication**: All subsequent calls use the session key
3. **Token Authentication**: Protected endpoints require valid access tokens

```rust
// Required before any API calls
client.perform_attestation_handshake().await?;

// Check session status
if let Some(session_id) = client.get_session_id()? {
    println!("Active session: {}", session_id);
}
```

## Error Handling

The SDK uses a custom `Error` type with detailed error variants:

```rust
use opensecret::Error;

match client.login(email, password, client_id).await {
    Ok(response) => println!("Success!"),
    Err(Error::Authentication(msg)) => println!("Auth failed: {}", msg),
    Err(Error::Api { status, message }) => println!("API error {}: {}", status, message),
    Err(e) => println!("Other error: {}", e),
}
```

## Testing

The SDK reads configuration from `.env.local` in the parent directory (OpenSecret-SDK root), matching the TypeScript SDK setup.

Required environment variables in `.env.local`:
```bash
VITE_OPEN_SECRET_API_URL=http://localhost:3000
VITE_TEST_CLIENT_ID=your-client-id-uuid
```

Run tests:
```bash
# All tests (requires running server on localhost:3000)
cargo test

# With output
cargo test -- --nocapture

# Specific test
cargo test test_login_signup_flow -- --nocapture
```

## Examples

See the `examples/` directory for complete examples:

```bash
# Basic authentication flow
cargo run --example auth_example
```

## Security Considerations

1. **Always verify attestation** in production environments
2. **Store tokens securely** - the SDK keeps them in memory only
3. **Use HTTPS** for all production API calls
4. **Rotate tokens regularly** using the refresh mechanism
5. **Clear sessions** after use with `logout()`

## License

MIT
