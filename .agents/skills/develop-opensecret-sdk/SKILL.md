---
name: develop-opensecret-sdk
description: Develop and review the OpenSecret TypeScript/React and Rust SDKs under Maple's sdk directory. Use for SDK API, authentication, attestation, encrypted transport, tests, the pinned OpenSecret integration revision, package contents, versions, or an explicitly authorized npm or crates.io publishing handoff; use develop-maple for application-only work.
---

# Develop the OpenSecret SDK

Work from `OpenSecretCloud/Maple/sdk`. Read the repository-root `AGENTS.md`,
`sdk/README.md`, the affected implementation and tests, and the root
`.github/workflows/sdk-*.yml` files relevant to the change.

The SDK source is part of the Maple repository, but its TypeScript and Rust
package boundaries remain independently versioned and publishable:

- `src/` builds `@opensecret/react` for browser and React consumers.
- `rust/` builds the `opensecret` crate for native consumers.
- `frontend/package.json` is authoritative for whether Maple's browser client
  consumes a published TypeScript version or the in-tree `file:../sdk` package.
- Maple's native client continues to consume the published Rust crate pinned in
  `frontend/src-tauri/Cargo.toml` until the proxy and Rust consumers switch
  together.

Do not commit, push, open a PR, publish, or alter Maple's application dependency
wiring unless the user authorizes that action.

## Keep protocol ownership clear

OpenSecret owns authentication and authorization truth, public HTTP semantics,
provider policy, persistence, and usage accounting. The SDKs own client-side
attestation, encrypted sessions, typed contracts, authentication state, and
safe transport adaptation. Maple owns application presentation and local
device behavior.

For a public contract change, inspect the exact OpenSecret backend revision and
both SDK implementations when they expose the affected behavior. Preserve
old-client/new-server and new-client/old-server compatibility where clients can
update independently. Do not weaken HTTPS, PCR validation, attestation, key
exchange, randomness, retry safety, or sanitized errors to accommodate a
caller.

## Develop and validate

Use the SDK's pinned Nix shell. For TypeScript/React work:

```sh
nix develop --no-update-lock-file -c bun install --frozen-lockfile --ignore-scripts
nix develop --no-update-lock-file -c bun run format:check
nix develop --no-update-lock-file -c bun run build
nix develop --no-update-lock-file -c bun test --timeout 30000
```

For Rust work:

```sh
nix develop --no-update-lock-file -c bash -lc '
  cd rust
  cargo fmt --all -- --check
  cargo clippy --locked --all-targets --all-features -- -D warnings
  cargo test --locked --all-features
  cargo doc --locked --no-deps --all-features
'
```

Run focused tests while iterating, then match the root path-scoped workflows:
`sdk-typescript.yml`, `sdk-rust.yml`, and `sdk-supply-chain.yml` as applicable.
When the change reaches backend behavior, authentication, or the encrypted wire
contract, also match `sdk-integration.yml`. It checks out the full commit SHA in
`opensecret-integration-revision`, starts disposable PostgreSQL and OpenSecret,
and tests both SDK implementations without a hosted development server.

Advance `opensecret-integration-revision` only to an inspected OpenSecret commit
whose compatibility the change intends to establish. Provider-spending tests
remain opt-in through `RUN_LIVE_AI=1` and require explicit credential, egress,
and cost authorization.

Before handoff, inspect package boundaries as applicable:

```sh
bun run pack
cargo package --locked --manifest-path rust/Cargo.toml
```

These commands validate package contents; they do not publish them or prove a
Maple application consumes the result.

## Publishing boundary

SDK publishing is separate from the Maple application release workflow.
`just publish-npm` and `just publish-cargo` are external production mutations;
run either only with explicit authority for the exact package, version, registry,
and source commit. Verify versions, clean state, tests, and the built package
before publishing, then report the immutable registry result. Do not create a
Maple GitHub Release merely to publish an SDK.

## Report

State which SDK changed, the backend/API compatibility boundary, exact commands
and results, package inspection performed, the Maple dependency pins left
unchanged or deliberately updated, and every client, platform, live provider,
or publishing boundary not exercised.
