# OpenSecret SDKs

This directory contains the TypeScript/React and Rust clients used by Maple and
OpenSecret's internal applications. Both clients establish attested,
end-to-end encrypted sessions with an OpenSecret backend and expose the API
surface needed by those applications.

The developer/platform API remains part of the TypeScript SDK for internal
OpenSecret workflows. This repository does not maintain or deploy a separate
documentation website; keep behavior documentation close to the exported code
and tests.

## Repository layout

- `src/` — `@opensecret/react`, including the React providers, encrypted API
  client, attestation policy, model/conversation APIs, and internal developer
  platform client.
- `rust/` — the `opensecret` crate used by native clients.
- `docs/PLATFORM.md` — internal developer/platform API notes.
- repository-root `.github/workflows/sdk-*.yml` — path-scoped TypeScript, Rust,
  and supply-chain validation for this directory.

Maple's frontend consumes this TypeScript package through `file:../sdk`.
Desktop Maple and `proxy/` consume `sdk/rust` through versioned path
dependencies; iOS and Android exclude those desktop-only Rust consumers.
Published npm and crates.io packages remain independent compatibility surfaces
for external users.

## Security model

For non-local endpoints, both SDKs require HTTPS, verify AWS Nitro attestation,
and require the complete PCR0/PCR1/PCR2 tuple to match an environment-scoped
active release before completing key exchange. Current release authorization is
published as standard, consistent-snapshot TUF metadata and targets at the one
fixed origin `https://attestations.trymaple.ai/tuf`. The SDK embeds only the TUF
root of trust; it does not fetch GitHub PCR histories or dereference source,
Fulcio, Rekor, or GitHub provenance URLs at runtime.

The browser flow is deliberately split at a clear verification boundary:

```text
release promotion pipeline
  admits configured builders, verifies locked release inputs and Sigstore evidence
  publishes manifest + bundle, then atomically authorizes them with TUF

browser before each new attested session
  verifies TUF root rotation, signatures, versions, expiry, hashes, and lengths
  loads only the selected prod or dev channel (at most two active manifests)
  verifies each exact manifest byte sequence against its Fulcio certificate,
    SCT, Rekor body/inclusion proof/signed checkpoint, and RFC3161 timestamp
  compares PCR0 + PCR1 + PCR2 against one complete authenticated manifest
  performs key exchange only after that tuple matches
```

The browser uses the exact-pinned `@freedomofpress/sigstore-browser` dependency
behind Maple's bounded v0.3 message-signature adapter. The adapter rejects DSSE
and legacy certificate-chain layouts, requires one Rekor entry with an inclusion
proof and signed checkpoint, requires CT/Rekor/TSA thresholds of one, and verifies
the raw manifest bytes rather than parsed or reserialized JSON. It also checks the
full Fulcio path at the authenticated RFC3161 observer time. For Rekor v2, the
absent/null/zero legacy integrated-time field is ignored and the required RFC3161
timestamp is authoritative. TUF supplies current authorization and rollback
protection; the immutable Sigstore record supplies cryptographic provenance and
transparency. Neither is a substitute for the other.

Builder admission remains a promotion concern. `build.builderId`, certificate
SANs, OIDC issuer, repository, workflow, and run URI are authenticated audit
provenance after TUF and Sigstore verification, but the SDK does not match them
against a client-side allowlist. This keeps authorized reproducible builders
fungible and avoids coupling existing SDKs to a repository, CI provider, or
workflow name during a migration.

The Rust SDK has a different capability boundary: it uses the maintained
`sigstore-tuf` and `sigstore-verify` crates to verify both layers locally before
accepting an active PCR tuple. It follows the same authorization split: TUF
selects exact evidence, while Sigstore verifies that evidence cryptographically;
certificate identity is not a second client authorization policy.

Policy targets are isolated per channel, while root, timestamp, snapshot, and
targets rollback high-water marks are shared for the repository. Browser cache
entries use the immutable v4 authority-history schema and versioned keys, so a
stale tab cannot overwrite or erase a newer generation it did not observe; Web
Locks serialize cleanup when available. Pre-root-history v3 cache entries fail
closed rather than silently resetting their trust floors.
An authenticated-observation journal persists newer root or partial metadata
floors before the next download, without activating a partial PCR policy. Before
authorizing a freshly verified Nitro document, the browser rechecks signed
metadata expiry and the already-loaded policy against that durable journal; this
is a non-network currentness check, so a cross-tab revocation cannot be hidden by
the policy object held during document verification.

Rollback floors retain bounded cumulative full-authority provenance and replay
every authenticated root transition in sequence. A direct metadata floor resets
only when its old key material cannot meet the replacement role's threshold. A
snapshot or targets descriptor may reset when either its signing parent or its
referenced child authority is fully replaced; only targets-authority replacement
resets a channel sequence. An overlap rotation conservatively widens surviving
provenance because TUF signatures are detachable, so staged overlap is not a
compromise-recovery mechanism. Recovery requires fresh non-overlapping keys (and
may also require rotating the parent that authenticated a poisoned descriptor).
Duplicate aliases, moving retired key material between online roles, and later
reauthorizing retired timestamp, snapshot, or targets keys are rejected: release
operations must never reuse retired online-role key material.

The v1 client contract keeps the original embedded root and follows every
numbered remote root update in order, with at most 32 rotations over the entire
embedded-root trust epoch (root v33 for the official root-v1 client). At that
ceiling, the browser probes root v34 only as a non-persisted sentinel and fails
on every retry while it exists. Only an exact v34 `404` permits authorization at
the ceiling; transient or ambiguous sentinel failures cannot select cached
policy. SDK releases must not replace the bootstrap: persisted state is bound
to the exact embedded-root epoch, and any mismatch fails closed before network
access. Even an immediate embedded-root successor would be an unauthenticated
out-of-band reanchor that could omit authority history. A future bootstrap
replacement requires an explicit, authenticated bridge-history migration in
both SDKs; it is not an ordinary SDK asset update.

Offline root key material must be disjoint from every online role for the
repository's lifetime: a later root cannot move a former offline key online or
promote previously-online material into the root role. The timestamp, snapshot,
and targets roles may intentionally share the same online key material, as the
initial threshold-1 deployment does. Repository tooling must enforce both this
custody boundary and lifetime online-key non-reuse across the immutable root
history; clients persist and enforce both histories across every root epoch they
authenticate.

For a new remote session, the browser completes the potentially slow TUF refresh
before creating the nonce or requesting the backend's ephemeral attestation
document. This preserves the backend pending secret's five-minute lifetime. It
then verifies the Nitro document, performs the expiry/currentness and complete
PCR0/PCR1/PCR2 authorization above, and immediately proceeds to key exchange.
An already verified cached session does not trigger this refresh.

Manifest, bundle, and Sigstore trusted-root bodies are retained in the immutable
generation cache. A cached generation is usable only after all three exact bodies
are hash-checked against TUF and the bundle is cryptographically reverified. A
refresh may fall back only to such a fully reverified, unexpired last-known-good
generation after an explicit retryable
HTTP status (`404`, `408`, `429`, or `5xx`), timeout, or response interruption
after headers. Ambiguous pre-response Fetch rejection fails closed because
browsers do not distinguish an offline failure from a redirect blocked by
`redirect: "error"`. Cryptographic, rollback, redirect, schema, size, or
integrity failures never select cached policy. Timestamp validity is capped at
48 hours.

The publisher must produce the shared browser/native TUF v1 profile: Ed25519
top-level roles, threshold signatures, `consistent_snapshot: true`, sequential
root rotation, and SHA-256 length/hash descriptors. Browser limits are 64 KiB
root, 32 KiB timestamp, 128 KiB snapshot, 256 KiB targets, 128 KiB channel,
and manifest targets, 512 KiB Sigstore trusted root, and 2 MiB
portable bundle. Publication must enforce the smallest client limit. The
checked-in generated root is intentionally an unbootstrapped fail-closed
placeholder until the production TUF repository is created and reviewed.

`@freedomofpress/sigstore-browser` 0.1.14 filters Fulcio certificate authorities
against the wall clock while loading a root. Maple first selects the one Rekor
key whose log ID matches the one required bundle entry, so overlapping Rekor
rotations do not depend on array order; that key must still be current when the
library loads it. The library does not expose a safe override for the remaining
current-time Fulcio filter. A sufficiently old release can therefore become
locally unverifiable after Sigstore retires trust material even while Maple TUF
still lists it; the SDK fails closed instead of weakening authenticated validity
periods. Promotion should replace such an active release before its Sigstore
trust material ages out.

Mock attestation is limited to exact loopback development endpoints (plus the
documented Android emulator alias in the Rust SDK). Do not weaken attestation,
trusted-release validation, or encrypted transport to accommodate a caller.

The SDKs use operating-system or Web Crypto randomness for keys, nonces, and
session material. Never substitute deterministic or convenience randomness in
production paths.

## TypeScript/React SDK

> **Unreleased breaking change:** npm 3.5.2 is the legacy PCR-file client. The
> dynamic TUF/Sigstore client below must ship as npm 4.0.0 (or another new major)
> only after the production TUF root and initial policy are published. Do not
> publish this draft under the current package version.

Install the package:

```sh
bun add @opensecret/react
```

Wrap the application with `OpenSecretProvider` and supply the backend URL and
client ID:

```tsx
import { OpenSecretProvider } from "@opensecret/react";
import type { ReactNode } from "react";

export function AppProviders({ children }: { children: ReactNode }) {
  return (
    <OpenSecretProvider
      apiUrl="https://api.example.com"
      clientId="00000000-0000-0000-0000-000000000000"
      pcrConfig={{ environment: "prod" }}
    >
      {children}
    </OpenSecretProvider>
  );
}
```

Use `useOpenSecret` for authentication, encrypted application APIs,
conversations, inference, and account operations. Internal developer tooling
uses `OpenSecretDeveloper` and `useOpenSecretDeveloper`; preserve that surface
when changing the public exports.

### Development

Use the pinned Nix shell and Bun version:

```sh
nix develop --no-update-lock-file
bun install --frozen-lockfile --ignore-scripts
bun run format:check
bun run build
bun test --timeout 30000
```

Integration tests read the variables documented in `.env.example`. Monorepo CI
checks out the exact OpenSecret commit pinned in
`opensecret-integration-revision`, migrates disposable PostgreSQL, starts that
backend on loopback, and creates disposable SDK fixtures. It does not depend on
the hosted development service or stored test-account credentials.

Tests that spend model/provider capacity are opt-in with `RUN_LIVE_AI=1` and
are not part of the deterministic pull-request gate. When intentionally
advancing the integration backend, update the pinned full commit SHA and let
the local integration workflow prove both SDK implementations against it.

Inspect the publishable npm artifact with:

```sh
bun run pack
```

Only `dist/` is included in the package.

First bootstrap and publish the backend TUF repository, replace the placeholder
SDK root, and then release this TypeScript SDK major before updating consumers.

Publish a freshly built npm artifact with:

```sh
just publish-npm
```

## Rust SDK

Add the crate to a Rust application:

```toml
[dependencies]
opensecret = "3"
```

The published 3.x crate still uses the legacy PCR trust contract. The dynamic
TUF/Sigstore implementation in this monorepo is unreleased and requires a new
major SDK release before external consumers should rely on the behavior
described here. See `rust/README.md` for the explicit activation warning and
release ordering.

The primary entry point is `OpenSecretClient`. See `rust/README.md` for native
client examples and transport details.

Run the Rust validation from the `sdk/` directory:

```sh
nix develop --no-update-lock-file -c bash -lc '
  cd rust
  cargo fmt --all -- --check
  cargo clippy --locked --all-targets --all-features -- -D warnings
  cargo test --locked --all-features
  cargo doc --locked --no-deps --all-features
'
```

Integration tests use the variables documented in `rust/.env.example` and are
separate from the default local validation path.

Publish the locked Rust crate with:

```sh
just publish-cargo
```

## Change discipline

- Keep the TypeScript and Rust attestation policies aligned intentionally;
  neither SDK's passing tests prove parity with the other.
- Treat API compatibility, authentication state, encrypted retry behavior, and
  trusted-release policy changes as security-sensitive.
- Update source comments and focused tests with behavior changes instead of
  regenerating a standalone documentation site.
- Validate the built npm package and Rust crate boundary before publishing a
  release.

## License

MIT
