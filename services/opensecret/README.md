# OpenSecret

OpenSecret is the open-source Rust backend for confidential AI applications
such as [Maple](https://github.com/OpenSecretCloud/Maple). It owns
authentication, encrypted client sessions and persistence, provider routing,
usage accounting, and OpenAI-shaped/Responses APIs carried inside the
OpenSecret encrypted transport.

Production is designed for AWS Nitro Enclaves. A source checkout or local build
can validate implementation and build invariants, but does not by itself prove
the artifact, PCR, IAM/KMS policy, logging, or network configuration of a
deployed environment.

## Local quick start

The Nix flake pins the Rust toolchain, PostgreSQL, Diesel, native libraries, and
provider dependencies.

Before running migrations, confirm that the PostgreSQL listener and database
belong to this checkout. The development shell may reuse any listener answering
on its configured port; use distinct state and ports for concurrent checkouts.
It may also start `.pgdata` and create `.env` from `.env.sample`. On Linux,
container setup changes user-level state unless disabled. See
[`docs/dev-shell.md`](docs/dev-shell.md) for controls.

```sh
git submodule update --init --recursive
OPENSECRET_DEV_CONTAINERS=0 nix develop --no-update-lock-file
just diesel-migration-run-local
```

Backend startup does not run Diesel schema migrations;
`src/migrations.rs` is separate application-data migration logic.

### Provider credentials and macOS stack

Tinfoil runs in-process; there is no local Tinfoil sidecar. For protected
credential files, the native Continuum proxy, process topology, and Maple
wiring, follow [`docs/local-macos-stack.md`](docs/local-macos-stack.md).

## Configuration and API boundary

`.env.sample` is the supported local-development starting point; current
startup source is authoritative. Keep real credentials in ignored files or
protected environment variables. Billing and feature flags are optional
external HTTP APIs whose administrative credentials remain in the backend;
their server implementations are not part of this repository setup.

Protected routes require OpenSecret attestation/key exchange and encrypted
sessions. “OpenAI-shaped” describes decrypted payloads, not a plaintext
OpenAI-compatible wire endpoint. Use an OpenSecret SDK or Maple for
protected-route integration tests; plain `curl` is suitable only for public
health probes.

Contributor and coding-agent standards live in [`AGENTS.md`](AGENTS.md).
Task-specific development, API, provider, security, and validation workflows
live under [`.agents/skills/`](.agents/skills/).

## Validation

The Rust CI gate is formatting, strict all-target/all-feature Clippy, and locked
all-feature tests. Run it through the pinned shell without starting local
services:

```sh
OPENSECRET_DEV_POSTGRES=0 OPENSECRET_DEV_ENV=0 OPENSECRET_DEV_CONTAINERS=0 \
  nix develop --no-write-lock-file -c cargo fmt --all -- --check
OPENSECRET_DEV_POSTGRES=0 OPENSECRET_DEV_ENV=0 OPENSECRET_DEV_CONTAINERS=0 \
  nix develop --no-write-lock-file -c env RUSTFLAGS='-D warnings' \
  cargo clippy --locked --all-targets --all-features -- -D warnings
OPENSECRET_DEV_POSTGRES=0 OPENSECRET_DEV_ENV=0 OPENSECRET_DEV_CONTAINERS=0 \
  nix develop --no-write-lock-file -c env RUSTFLAGS='-D warnings' \
  cargo test --locked --all-features
```

Default CI does not run ignored database or live-provider tests. Use the
[`validate-opensecret`](.agents/skills/validate-opensecret/SKILL.md) workflow
for disposable PostgreSQL tests, authorized provider checks, encrypted-client
smoke tests, Nix checks, and release-only EIF/PCR evidence. Report those layers
separately.

## Nitro builds and deployment

The supported EIF outputs are `eif-dev`, `eif-preview`, and `eif-prod`, built
with the repository's Nix flake on the appropriate Linux/ARM environment. Build,
PCR comparison, deployment, and live trust verification are distinct evidence.

Routine pull requests do not require EIF/PCR parity. The Nix Reproducible
Builds workflow still builds the development EIF on pull requests, but skips
PCR comparison there. Master pushes and manual `workflow_dispatch` still
verify PCR values against the checked-in references. Before an authorized
dev or prod publish/deployment, use the supported Linux/ARM64 release
builder to review and deliberately update/verify the target measurements;
never update checked-in PCRs solely to clear ordinary pull-request CI.

See [`docs/nitro-deploy.md`](docs/nitro-deploy.md) for operator procedures.
Changing PCR references or KMS policy, copying artifacts, starting or stopping
enclaves, restarting remote services, staging, and deployment require explicit
authorization for the named environment.

## Contributing

Keep changes focused, add regression coverage at the owning boundary, and
follow the relevant repository skill. Before opening a pull request, run every
validation tier reached by the diff and state what remains unverified.
