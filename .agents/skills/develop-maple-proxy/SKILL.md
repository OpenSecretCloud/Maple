---
name: develop-maple-proxy
description: Develop and review the maple-proxy Rust crate, binary, container, and OpenAI-compatible HTTP behavior under Maple's proxy directory. Use for proxy APIs, configuration, authentication, CORS, attested OpenSecret transport, tests, container builds, package versions, or an explicitly authorized crates.io or GHCR publishing handoff; use develop-maple for the Tauri lifecycle wrapper alone.
---

# Develop Maple Proxy

Work from the `OpenSecretCloud/Maple` repository root; the component source is
under `proxy/`. Read the repository-root `AGENTS.md`, `proxy/README.md`,
affected source and tests, and the root
`.github/workflows/proxy-*.yml` files relevant to the change.

The source is part of Maple but keeps distinct public package and runtime
boundaries:

- `proxy/` builds the `maple-proxy` crate and binary.
- `proxy/Cargo.toml` consumes the in-tree OpenSecret Rust SDK at `../sdk/rust`
  with a registry version retained for Cargo publishing.
- desktop Maple consumes `../../proxy` and `../../sdk/rust` from
  `frontend/src-tauri/Cargo.toml`; iOS and Android do not compile the proxy.
- `frontend/src-tauri/src/proxy.rs` owns Maple's account-scoped listener,
  configuration, key storage, and lifecycle around the library. Do not move
  that application behavior into the reusable crate incidentally.

Do not commit, push, open a PR, publish, tag, release, or change live
infrastructure unless the user authorizes that action.

## Keep validation and routing aligned

Run the credential-free proxy checks through its pinned shell:

```sh
nix develop --no-update-lock-file ./proxy -c bash -lc '
  cd proxy
  cargo fmt --all -- --check
  cargo clippy --locked --all-targets --all-features -- -D warnings
  cargo test --locked --all-features
  RUSTDOCFLAGS="-D warnings" cargo doc --locked --no-deps --all-features
  cargo machete
'
```

For Rust SDK or dependency-wiring changes, also prove the application resolves
one local SDK and one local proxy:

```sh
nix develop --no-update-lock-file .#ci -c \
  ./scripts/ci/verify-local-rust-deps.sh
```

Root workflows own proxy Rust, daily supply-chain, non-publishing container,
and native-release rehearsal checks. `proxy/src/**`, `proxy/Cargo.toml`, and
unknown proxy build inputs are desktop application inputs; tests, examples,
docs, the standalone lockfile, and container-only files do not by themselves
route expensive Maple app builds. `sdk/rust` runtime changes affect both proxy
checks and desktop Maple. When a new input changes either graph, update
`scripts/ci/change_detection.py`, its table-driven tests, and workflow paths in
the same change.

## Exercise the right runtime

Run the standalone proxy on a checkout-specific loopback port with an explicit
backend and PCR0 environment. Keep API keys in ignored local configuration or
the invoking process; never print or commit them. Exercise `/health`,
`/v1/models`, streaming and non-streaming chat, embeddings, invalid
authentication, timeout, and cancellation only as relevant to the change.

Container builds require the Maple repository root as context because the
Dockerfile copies both `proxy/` and `sdk/rust`:

```sh
docker build -f proxy/Dockerfile -t maple-proxy:dev .
```

Use the configured container runtime from `proxy/justfile` when Docker is not
the intended local engine. A successful image build is not proof that GHCR was
published or that the service works against a live enclave.

For authentication, CORS, bind exposure, saved-key behavior, backend URL or
PCR0 selection, request forwarding, logging, or timeout changes, load
`$review-maple-security`. Preserve the core loopback and CORS-off defaults;
treat container CORS-on configuration as a separate exposed mode. A configured
default API key is for private/originless clients; browser-facing CORS mode must
require each request's bearer key rather than spending the saved default. Never
assume protections in the Tauri wrapper also exist in the standalone router.
Never log any portion of an API key. Treat raw OpenAI request and response
bodies as untrusted and potentially sensitive.

## Preserve publishing boundaries

Maple's current GitHub Release workflow builds, checksums, attests, uploads,
and re-verifies four native proxy archives. The first post-integration release
is still the live publication canary. Never create a proxy GitHub tag or
Release; a proxy-only binary fix ships through a normal Maple patch release.

Crates.io publishing remains separately versioned and manual. On an authorized
publish, inspect the exact package first:

```sh
cargo package --locked --manifest-path proxy/Cargo.toml
```

If the proxy references a new `opensecret` version, publish that SDK crate
first. Do not publish either crate from Maple's application Release workflow.
The current root proxy container workflow builds without pushing; adding or
running a GHCR publisher is a separate production action requiring explicit
repository, version, image namespace, tag, and credential authority.

## Report

State the proxy behavior and public contract changed, SDK/application boundary,
exact checks and runtime evidence, container or package inspection performed,
version/publisher state left unchanged or deliberately updated, and every
platform, live backend, registry, or release boundary not exercised.
