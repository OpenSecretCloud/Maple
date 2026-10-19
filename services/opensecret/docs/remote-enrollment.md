# Maple remote enrollment and relay admission

The service supplies account-private discovery and admission, not device grants.
`GET/POST /v1/remote/devices`, `POST /v1/remote/admissions`, and
`POST /v1/remote/admissions/check` require JWT authorization and Transport V2.
The Rust SDK provides the corresponding typed methods. Registration additionally
proves possession of the endpoint Ed25519 key over a versioned fixed-schema
message; client/service share `sdk/rust/src/remote_registration_v1.json`.

Device leases expire after 15 minutes; active daemons refresh every minute. A
pairing window lasts at most five minutes and repeated registration of the same
window does not extend its saved expiry. Admission handles are random 256-bit
values, stored under their SHA-256 identifier, bound to both endpoints and W,
and expire within two minutes or the window's remaining lifetime. Both endpoints
must have current authenticated registrations in the same channel. The execution
daemon separately validates its own monotonic window and one-attempt budget.

`remote_enrollment_records` is shared PostgreSQL state, not enclave memory.
Writes serialize only the account's enrollment rows and cap devices/windows at
64 and admissions at 128. Window tombstones are retained to avoid a repeated W
extending its expiry after cleanup. Records contain no code, password verifier,
PAKE message or control grant. Encryption derives a dedicated key/domain from
the enclave root; canonical AAD binds the expected authenticated account,
project, deployment, record kind and record identity. Authorization uses encrypted
expiry and endpoint fields, not plaintext indexes. Copying another user's valid
ciphertext together with rewritten lookup columns fails authentication. Deletion
or rollback can still deny service; local endpoint/grant/window authority prevents
cloud state from silently enrolling a controller.

## Stock relay callback

`POST /v1/remote/relay-access` is deliberately outside client Transport V2/JWT.
It accepts only a dedicated relay-to-service bearer credential and the
relay-authenticated endpoint in **`X-Iroh-NodeId`**, the actual header emitted by
`iroh-relay 1.3.0` (that version's nearby documentation names a different header).
A current encrypted device lease for that exact key allows admission. Success is
HTTP 200 with the literal body `true`; all denial paths fail closed. No credential
means all relay admission is denied. Clients never receive the relay credential.

For a future deployment, provision `maple_remote_relay_auth_v1` through the
existing KMS-encrypted enclave-secret mechanism. Its authenticated plaintext is:

```json
{"purpose":"maple-remote-relay-admission-v1","deployment":"dev","token":"<64 random hexadecimal characters>"}
```

Use a distinct production credential with `deployment: "prod"`. Purpose and
deployment are checked inside the decrypted envelope so an attacker cannot move
another valid KMS secret under this database key and have it accepted. Missing
configuration leaves the route disabled; invalid configured credentials fail
startup. Local-only tests can supply `MAPLE_REMOTE_RELAY_AUTH_TOKEN` instead.
Do not provision secrets, DNS or infrastructure as part of an ordinary code PR.

## Prepared relay configuration, not a deployment

Use stock `iroh-relay = 1.3.0`. Choose a Maple-owned Dev hostname/certificate and
OpenSecret URL during the authorized infrastructure rollout. Example access and
receive-bandwidth settings, to combine with that server's TLS/listener config:

```toml
[access.http]
url = "https://<dev-opensecret-host>/v1/remote/relay-access"

[limits.client.rx]
bytes_per_second = 10485760
max_burst_bytes = 20971520
```

Inject the matching credential through the relay's
`IROH_RELAY_HTTP_BEARER_TOKEN`, not TOML checked into Git. Bind metrics privately.
Apply connection/resource limits in the deployment's existing ingress/service
manager. In 1.3.0, `accept_conn_limit` and `accept_conn_burst` are documented in
source as **not implemented**; do not claim those settings protect admission.
The client receive-byte limiter is implemented. Validate actual access failures,
lease refresh and capacity with the exact deployed executable before trial.
Do not advertise Maple's relay as public infrastructure.

No new admission is allowed while the check service is unavailable. Existing
peer grants remain local, and established connections/direct paths can continue;
a network change requiring new relay admission may remain disconnected until
service recovers. There is no custom cache or relay fallback that weakens this.

## Evidence boundaries

Unit tests cover binding and credentials; disposable-DB tests cover migrations,
repeated windows, account/key substitution, valid foreign-row swaps and relay
lease rejection. Two isolated Local daemons additionally exercise real Transport
V2 registration and direct PAKE over public Iroh relays. That public-relay test
does not validate a deployed Maple relay, its TLS/operator setup, or production
enclave/KMS configuration. Those require the separate rollout gate.
