# OpenSecret Rust SDK

This source lives under Maple's `sdk/rust/` directory. Desktop Maple and
Maple's in-tree `proxy/` consume it through versioned path dependencies, while
crates.io publishing remains an independent compatibility surface.

Rust SDK for OpenSecret - secure AI API interactions with nitro attestation.

## Features

- 🔐 **Nitro Attestation**: Verify server identity through AWS Nitro Enclaves
- 🔑 **End-to-End Encryption**: All API calls encrypted with session keys
- 👤 **Authentication**: Support for both email-based and guest users
- 🔄 **Token Management**: Automatic token refresh and session management
- ↔️ **Lossless Inference Transport**: Provider-specific request and response fields pass through as raw bytes
- 🛡️ **Secure by Default**: No plaintext data transmission

## Installation

The dynamic TUF/Sigstore implementation documented below is currently
unreleased. The latest crates.io version, 3.6.2, still has the legacy PCR trust
contract and must not be presented as containing these changes. Maple and its
proxy consume this in-tree source through path dependencies while the breaking
SDK release and dependent proxy release are reviewed and published in order.

Add to your `Cargo.toml`:

```toml
[dependencies]
opensecret = "3.6.2"
bytes = "1"
futures = "0.3"
http = "1"
```

The version above is the latest legacy release. Update it to the new major only
after the TUF-enabled SDK has actually been published.

## Quick Start

```rust
use opensecret::{OpenSecretClient, Result};
use uuid::Uuid;

#[tokio::main]
async fn main() -> Result<()> {
    // Initialize client
    let client = OpenSecretClient::new("https://api.opensecret.cloud")?;
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

Production clients verify both AWS Nitro authenticity and an atomic
PCR0/PCR1/PCR2 tuple before key exchange. Before every real attestation
handshake, the SDK refreshes the selected `prod` or `dev` channel from
`https://attestations.trymaple.ai/tuf/`. It verifies TUF root rotation,
signatures, versions, expiry, lengths, and hashes, then locally verifies each
active release's portable Sigstore bundle over the exact manifest bytes. The
Sigstore check includes the Fulcio certificate chain and SCT, Rekor inclusion
proof and signed checkpoint, integrated signing time, artifact signature, and
the TUF-authenticated builder issuer and certificate-identity expression.
This refresh happens before the SDK requests the backend's ephemeral Nitro
attestation key, whose five-minute lifetime must not be consumed by a cold TUF
refresh. Immediately before key exchange the SDK performs a non-network check
against the latest process-wide and durable rollback floors, rechecks signed
metadata expiry, and authorizes the complete PCR tuple.

The convenience constructors recognize only the SDK's exact official origins.
A custom HTTPS origin must use `new_with_attestation_config` (or the API-key
equivalent) with an explicit `TrustedReleaseConfig` containing its repository
URL and bootstrap root. Unknown remote origins never inherit production trust.
Conversely, an official API origin accepts only the matching official channel,
the canonical `https://attestations.trymaple.ai/tuf/` repository, the SDK's
embedded bootstrap root, and a persistent rollback-state path. Explicit custom
configuration cannot replace or disable any part of that official trust domain;
mobile callers should use `official_with_cache_path` to select the durable path.

The cache contains the minimum complete last-known-good TUF generation plus a
monotonic authenticated-observation journal; it is not a list of trusted PCRs.
Default desktop state is stored in the platform's durable application-data
directory, not its purgeable cache or temporary directory. Android and iOS
hosts that cannot expose a home directory must obtain a durable app-data file
path from the platform and create the official manager with
`TrustedReleaseManager::official_with_cache_path`; they can then pass that
manager to `OpenSecretClient::new_with_trusted_release_manager` (or the API-key
equivalent). Deleting this state explicitly resets the local rollback history.
If a refresh authenticates newer root, timestamp, snapshot, targets, or channel
metadata and then fails later, those version/hash/sequence floors are persisted
without activating a partial PCR policy. This prevents a restart from replaying
an older still-unexpired generation. A network/unavailability failure may use
cached authorization only after re-running all TUF and Sigstore verification at
the current time against the greatest observed floors. Each TUF repository HTTP
request has a 15-second total deadline, including its streamed body. The bounded
cold path permits at most 43 sequential repository requests (including the
root-34 absence sentinel), so an enclosing recovery budget must allow up to 645
seconds of repository I/O; the Maple proxy supplies a 15-minute recovery
budget. The
signed timestamp may be valid for at most 48 hours. Invalid signatures,
rollback, expiry, redirects,
schema errors, or digest mismatches fail closed and do not fall back to cached
authorization. Root, timestamp, snapshot, and targets expiry are checked again
against a fresh clock reading after all downloads and Sigstore work, immediately
before the policy is returned, and the minimum signed expiry is checked once
more when the PCR tuple is authorized immediately before key exchange. The
cache also persists the greatest accepted channel sequence and exact channel
digest; a lower sequence or changed channel at the same sequence is rejected
even when newer TUF metadata signs it.
Production and development share one repository-level root and metadata history
so one channel cannot be selectively held on an older root, while their channel
sequence floors remain independent. Custom managers for the same repository
should therefore use the same cache path.

Rollback floors retain bounded full-authority provenance and replay every
authenticated root transition in sequence. A metadata floor is cleared only
when the old key material cannot meet the replacement role's threshold; an
overlap rotation conservatively widens the floor's provenance because TUF
signatures can be detached. Consequently, additive or staged overlap is not a
compromise-recovery mechanism in this v1 profile. Recovery from a compromised
online role must use fresh, non-overlapping key custody (and any parent role
needed to replace a poisoned child descriptor). Duplicate aliases for one
cryptographic key, reassigning previously seen key material to another online
role, and later reauthorizing a retired online key are rejected. Release
operations must never move or reuse retired timestamp, snapshot, or targets
keys. Offline root-role key material must be completely disjoint from the
timestamp, snapshot, and targets roles for the repository's entire observed
lifetime: a key that has ever appeared offline may never move online, and a key
that has ever appeared online may never become a root key. The three online
roles may share one protected key in the initial deployment. Cache schema v4
persists this bounded custody ledger; older unshipped draft cache schemas are
rejected rather than migrated without the missing history.

The supported v1 client line keeps its embedded bootstrap at signed root version
1 and traverses every numbered remote root in order (the repository retains them
all, through root 33 / 32 rotations). The official constructors machine-check
that the embedded root is self-authenticating and signed version 1; explicit
`TrustedReleaseConfig` custom repositories may intentionally bootstrap at a
different version and are bounded at bootstrap version + 32. The SDK applies
that absolute span again while journaling and loading durable state, so a
dependency that adopts the 33rd transition before reporting its iteration
limit cannot make that extra root trusted on a retry. Replacing the official
embedded root or skipping an
intermediate root is forbidden until an explicit authenticated bridge-history
migration exists. A missing intermediate root therefore fails closed rather
than discarding persisted rollback history.

An authenticated channel with no active releases is a valid emergency
revoke-all generation. It is committed to the cache and advances the channel
high-water mark, but its empty policy rejects every PCR tuple. An outage cannot
fall back past that revocation to an older active generation.

The checked-in generated TUF root is intentionally a v1 unpublished placeholder
until the production repository is bootstrapped; it is the only official-root
sentinel exception. In that staging state, real
handshakes return `UnreleasedAttestationPolicy`; there is no GitHub PCR-history
fallback. Exact localhost, loopback, and unspecified-address development
endpoints use mock attestation only when the `mock-attestation` feature is
enabled; Android also supports the exact emulator alias `10.0.2.2`. Other
endpoints require HTTPS.

The authenticated builder target also carries `workflowName` and
`workflowTrigger`. Release promotion validates those and the GitHub certificate
workflow-ref and workflow-SHA extension claims. The Rust verifier currently
enforces the cryptographically bound certificate identity, issuer, repository
linkage, signature, and log proofs; `sigstore-verify` does not expose those
GitHub-specific extensions for an additional runtime comparison.

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

The SDK reads configuration from `.env.local` in the parent Maple `sdk/`
directory, matching the TypeScript SDK setup.

Required environment variables in `.env.local`:
```bash
VITE_OPEN_SECRET_API_URL=http://localhost:3000
VITE_TEST_CLIENT_ID=your-client-id-uuid
```

Official hosted origins select their fixed `prod` or `dev` channel. Custom
origins require an explicit `TrustedReleaseConfig`; an environment variable
cannot silently change an official origin's trust domain.

Run tests:
```bash
# Hermetic library tests (no server required)
cargo test --locked --lib

# Local integration tests (requires a server on localhost:3000 and explicitly
# enables the mock-attestation bypass for that loopback origin)
cargo test --locked --features mock-attestation

# With output
cargo test --locked --features mock-attestation -- --nocapture

# Specific test
cargo test --locked --features mock-attestation test_login_signup_flow -- --nocapture
```

## Examples

See the `examples/` directory for complete examples:

```bash
# Basic authentication flow
cargo run --locked --example auth_example
```

## Security Considerations

1. **Always verify attestation** in production environments
2. **Store tokens securely** - the SDK keeps them in memory only
3. **Use HTTPS** for all production API calls
4. **Rotate tokens regularly** using the refresh mechanism
5. **Clear sessions** after use with `logout()`

## License

MIT
