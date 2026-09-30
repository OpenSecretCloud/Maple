# Maple Swift SDK

`MapleSDK` is a Swift 6 package for iOS 26 and macOS 15 or later. It uses
CryptoKit, Security.framework, URLSession and Swift concurrency. The package
has no external dependencies and no UI dependency. It implements Transport V2
and the authentication, account, Responses, conversations, projects, models,
instructions and audio contracts used by Maple Research.

## Development

Select Xcode 26.5. Its bundled `swift-format` supplies the formatter; use the
root Apple Nix shell for the surrounding pinned command-line environment:

```sh
nix develop --no-update-lock-file .#apple -c bash scripts/ci/sdk-swift.sh unit
nix develop --no-update-lock-file .#apple -c bash scripts/ci/sdk-swift.sh build
nix develop --no-update-lock-file .#apple -c bash scripts/ci/sdk-swift.sh ios-build
```

Run these from the repository root. `unit` checks formatting and runs
deterministic tests with hosted/local integration opt-ins cleared. `build`
builds the release package; `ios-build` compiles the library for arm64 iOS 26
and its simulator without signing. All compile with warnings treated as errors.
Use `bash scripts/ci/sdk-swift.sh format` to apply the checked-in formatting
policy. Set `MAPLE_SWIFT_SCRATCH_PATH` to isolate build products when running
concurrent checks. Tests use the real historical Nitro document shared with
the TypeScript and Rust implementations, synthetic protocol fixtures, and
in-memory credential stores; deterministic success does not establish live
deployment behavior or physical-device Keychain behavior.

An explicitly authorized development-enclave trust check is separate:

```sh
nix develop --no-update-lock-file .#apple -c bash scripts/ci/sdk-swift.sh live-attestation
```

It creates an anonymous session, verifies Nitro and development PCR policy,
fetches the signed PCR history, and checks clock rejection. It does not sign
in, invoke a model, or save live captures. The full encrypted transport's
development checks use `MAPLE_SWIFT_DEV_INTEGRATION=1` and remain opt-in.

For a disposable, already-running local OpenSecret backend, set
`MAPLE_SWIFT_INTEGRATION_URL` to its exact HTTP loopback URL with a port and
set `MAPLE_SWIFT_TEST_EMAIL`, `MAPLE_SWIFT_TEST_PASSWORD`, and
`MAPLE_SWIFT_TEST_CLIENT_ID`. The optional `MAPLE_SWIFT_TEST_INVITE_CODE`
supports disposable account creation. Then run:

```sh
nix develop --no-update-lock-file .#apple -c bash scripts/ci/sdk-swift.sh integration
```

These tests can mutate the named disposable account. Use the owning local
environment's service lifecycle; the check script never starts or stops it.
On macOS, `MAPLE_SWIFT_MAIL_FIXTURE_HELPER` can name an absolute executable
that returns a code actually delivered by that disposable backend. It receives
the ceremony (`verification`, `password-reset` or `account-deletion`) and test
email as two arguments, then returns only the code on standard output. The
mail-ceremony case is skipped when the helper is absent; stored code hashes
are not a substitute for delivered-mail evidence.
The separate CI-only fixture bootstrap creates its own PostgreSQL directory,
migrates the in-tree backend, runs the local integration suite, and stops
only its own processes. Provider-spending cases require the separate
`RUN_LIVE_AI=1` opt-in and explicitly configured development provider access.
Local mock attestation establishes protocol behavior, not Nitro trust.

## Using the package

During monorepo development, add `sdk/swift` as a local Swift package
dependency and link its `MapleSDK` product. Keep a `MapleClient` for the
application's lifetime. Clients using the default Keychain store share a
process-wide credential authority for each Keychain service and canonical API
origin. They also share an encrypted transport when their exact endpoint and
complete trust policy match:

```swift
let origin = "https://api.example.com"
let client = try MapleClient(
    apiURL: origin,
    clientID: "00000000-0000-0000-0000-000000000000",
    policy: PCRTrustPolicy(environment: .production)
)
// After sign-in, capture once for this account session:
let api = try await client.accountAPI()
```

Credential authorities have monotonic revisions and account generations.
Capture a snapshot before authentication, then install the complete returned
access/refresh pair against that snapshot before publishing the account.
`MapleClient` handles ordinary sign-in installation. Use its `accountAPI()`
after sign-in to capture both principal and immutable account generation;
principal identity alone cannot fence same-account logout and sign-in.
Refresh coalescing belongs to the shared authority, including when transports
use different endpoint paths or trust policies. Custom `KeychainSecureStore`
service names isolate credential namespaces. Other custom stores create
independent clients; reuse one client when they access the same credentials.
Advanced callers constructing transports directly must explicitly share one
`CredentialAuthority` for each credential namespace and origin. These
process-local ownership checks do not coordinate separate app processes or
extensions. JWT claims are unverified ownership/expiry hints; backend signature
validation and authorization remain authoritative.

The default store uses non-synchronizing, device-only Keychain entries.
Credentials and attested sessions are never stored in preferences. Session
cache identity includes the exact endpoint and complete PCR policy. Account
owners must cancel their work on logout and clear authority even if remote
revocation fails. A failed durable credential cleanup blocks continued use.
Cancel the task consuming a response to cancel its stream work. Typed stream
termination cancels its producer, and credential generations reject stale
response reads. Applications still own their operation lifecycle.

## Trust and transport

The public attestation verifier has no mock switch. It verifies the
byte-exact nonce, COSE ES384 signature and complete X.509 path against only
the bundled AWS Nitro root; system trust anchors and network chain fetching
are disabled. Security.framework validates path constraints at the signed
document timestamp, and every certificate is additionally checked against
the device clock, with 300 seconds of `notBefore` leeway and strict
`notAfter`. The signed timestamp explains clock failures and cannot replace
the device clock. `AttestationClockSkewError` preserves actionable clock
guidance; other errors contain bounded categories, not decrypted content.

`PCRTrustPolicy` defaults to production and never crosses environment trust
roots. Unknown PCR0 requires a valid signature under the pinned OpenSecret
P-384 history key. History responses have a five-second deadline, 1 MiB and
2,048-entry limits, and cannot redirect. Only PCR0 is signed in the existing
history protocol; timestamp, PCR1 and PCR2 are not authenticated freshness or
identity assertions. Additional roots and URLs are explicit configuration.

Only an exact HTTP loopback transport endpoint can select the internal local
mock parser. HTTPS endpoints always require real Nitro verification and PCR
trust before deriving session traffic keys. The handshake also binds the
client public key, server X25519 key and computed session ID. No V1 fallback
exists. Authenticated sequence, size and terminal checks reject tampering,
truncation and data after the terminal record.

The single resend rule follows the other SDKs: only an outer HTTP 400 with
the exact versioned `session_not_found` or `request_decryption_failed`
contract can establish a new verified session and resend once. Network
failure, application errors, decryption failures and partial streams never
trigger replay. The outer hint is unauthenticated and cannot guarantee
cross-session at-most-once execution; application idempotency remains
necessary where required. Native handoff and OAuth grants retain their
original session and credential ownership fences.

Public request header names are normalized to lowercase before envelope
validation. Case-colliding dictionary keys are rejected, including duplicate
inference-budget controls; the internal wire format requires lowercase names.
The inference send budget remains local and is never forwarded as a header.

## Version and distribution

`VERSION` is the independent Swift SDK version; it does not track Maple's
application version or the TypeScript/Rust package versions. Reserve
`maple-sdk-swift/vX.Y.Z` for Swift SDK source checkpoint tags. No tag or release
is created by development or CI. These namespaced monorepo tags are not
directly consumable SwiftPM semantic versions: SwiftPM requires a manifest
at the package repository root. Before a separately authorized publication,
prepare a package-root distribution or registry archive from `sdk/swift`,
with its own semantic version and immutable source provenance. Until then,
consumers use the local package and record the Maple revision.

Do not create a Maple app release or move the GitHub latest-release pointer
to publish this SDK. Follow the shared [consumer policy](../../docs/sdk-publishing.md#consumer-version-policy).
SDK publication, consumer pin updates, app signing and store submission are
separate actions.
