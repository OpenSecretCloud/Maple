---
name: validate-opensecret
description: Validate OpenSecret changes with focused Rust tests, exact Rust CI parity, disposable PostgreSQL migration and ignored-test proof, separately authorized provider checks, encrypted SDK or Maple smoke tests, Nix checks, and release-only EIF/PCR evidence. Use before claiming backend work complete or when reviewing whether test evidence matches a changed API, provider, persistence, security, build, or deployment boundary.
---

# Validate OpenSecret

## Select evidence from the diff

Read `AGENTS.md`, inspect the complete worktree, and select the union of the
applicable tiers. A higher tier supplements rather than replaces lower tiers.

| Change | Required evidence |
| --- | --- |
| Documentation only | Verify every changed path, command, variable, link, and behavioral claim. |
| Rust behavior or dependency | Focused tests, then Tier 1. |
| Auth, encryption, persistence, reset, or SQL migration | Tier 1 plus Tier 2 when database state is involved. |
| HTTP, middleware, SSE, Responses, or client contract | Tier 1 plus Tier 4; include affected SDK/Maple paths. |
| Provider, model, routing, headers, usage, or attestation | Tier 1 plus focused provider tests; add authorized Tiers 3 and 4 when the claim reaches them. |
| Nix, entrypoint, kernel, or packaging | Tier 1 when Rust is affected plus applicable current-host Tier 5 checks. |
| Authorized dev or prod publish/deployment | Tier 5 release EIF/PCR evidence on the supported Linux/ARM64 builder. |

Keep credentials and user data out of commands, logs, fixtures, and tracked
files. Never blanket-run `cargo test --locked -- --ignored`: ignored tests mix
disposable-database mutation with a credentialed live-provider test.

## Tier 0: iterate narrowly

Disable stateful shell hooks for pure checks. For example:

```sh
OPENSECRET_DEV_POSTGRES=0 OPENSECRET_DEV_ENV=0 OPENSECRET_DEV_CONTAINERS=0 \
  nix develop --no-write-lock-file -c \
  cargo test --locked --all-features provider_client::tests
```

Choose a filter from the owning code. Focused success is iteration evidence,
not completion evidence.

## Tier 1: reproduce Rust CI

Initialize submodules when building or testing, then run the exact inner gates
from `.github/workflows/rust.yml` through the pinned, side-effect-disabled Nix
environment:

```sh
git submodule update --init --recursive

OPENSECRET_DEV_POSTGRES=0 OPENSECRET_DEV_ENV=0 OPENSECRET_DEV_CONTAINERS=0 \
  nix develop --no-write-lock-file -c cargo fmt --all -- --check

OPENSECRET_DEV_POSTGRES=0 OPENSECRET_DEV_ENV=0 OPENSECRET_DEV_CONTAINERS=0 \
  nix develop --no-write-lock-file -c env RUSTFLAGS='-D warnings' \
  cargo clippy --locked --all-targets --all-features -- -D warnings

OPENSECRET_DEV_POSTGRES=0 OPENSECRET_DEV_ENV=0 OPENSECRET_DEV_CONTAINERS=0 \
  nix develop --no-write-lock-file -c env RUSTFLAGS='-D warnings' \
  cargo test --locked --all-features
```

Report passed, failed, and ignored counts. Default CI has no PostgreSQL service
and does not execute ignored tests. Do not substitute an aggregate recipe
unless its checked-in definition preserves the same targets, features,
lockfile, and warning policy.

## Tier 2: prove migrations and database-backed tests

For persistence changes that do not add a migration, run the bundled helper
from the repository root. It disables the default shell hooks, creates an
isolated loopback PostgreSQL cluster and database, verifies their identity and
empty schema, runs the full migration chain, discovers the selected ignored-test
counts, runs each subset serially with visible output, fails on a skip or count
mismatch, and cleans only its guarded temporary data directory:

```sh
OPENSECRET_DEV_POSTGRES=0 OPENSECRET_DEV_ENV=0 OPENSECRET_DEV_CONTAINERS=0 \
  nix develop --no-write-lock-file -c bash \
  ./.agents/skills/validate-opensecret/scripts/disposable_db_tests.sh
```

When the diff adds a new, unreleased latest reversible migration, use
`--redo-latest` instead. It includes the same validation and also exercises the
latest down/up cycle inside that disposable lifecycle:

```sh
OPENSECRET_DEV_POSTGRES=0 OPENSECRET_DEV_ENV=0 OPENSECRET_DEV_CONTAINERS=0 \
  nix develop --no-write-lock-file -c bash \
  ./.agents/skills/validate-opensecret/scripts/disposable_db_tests.sh --redo-latest
```

The helper proves an empty-database migration and the selected local synthetic
database suites. It does not prove an OAuth provider flow, encrypted client
transport, or a data conversion from representative old rows.

For a data migration, separately build an upgrade-shaped disposable database
with representative pre-change rows and verify restart, rollback, and retry
behavior. A user-key ciphertext change needs versioned dual-read/new-write and
authenticated lazy rewrite; SQL/startup cannot prove re-encryption without the
owning user key.

## Tier 3: isolate live-provider proof

Run a live check only with explicit authorization for the credential, network
egress, likely cost, and named provider. Keep it separate from default tests.
For the checked-in Tinfoil boundary test, configure the normal protected secret
source and run:

```sh
OPENSECRET_DEV_POSTGRES=0 OPENSECRET_DEV_ENV=0 OPENSECRET_DEV_CONTAINERS=0 \
  nix develop --no-write-lock-file -c cargo test --locked --all-features \
  provider_client::tests::live_tinfoil_models_and_completions_match_the_legacy_api_contract \
  -- --ignored --exact
```

This proves only the named provider boundary at that time. Follow
`docs/tinfoil-rust-sdk-parity.md` for its contract and evidence controls. If a
provider has no live harness, report live behavior unverified rather than
borrowing another provider's result.

## Tier 4: smoke the encrypted application

Health probes are preliminary only:

```sh
curl --fail --silent --show-error http://127.0.0.1:3000/health-check
curl --fail --silent --show-error http://127.0.0.1:3000/health-check-extended
```

The first is process liveness; the second checks Tinfoil model connectivity.
Neither proves PostgreSQL, auth, encryption, persistence, routing, billing,
flags, or a user flow.

Exercise protected routes through the SDK under a selected Maple checkout's
`sdk/` directory or through the corresponding pinned Maple application client:

1. Start an isolated migrated backend with the authorized external services.
2. Exercise the exact changed success and failure paths with route-appropriate
   auth and a live encrypted session.
3. Verify persistence after reload/re-entry when data changes.
4. For streams, verify decrypted order, cancellation/disconnect behavior,
   usage when promised, and one terminal condition.
5. Inspect bounded logs for accidental sensitive content.

In the selected Maple revision, follow its checked-in `AGENTS.md` and matching
SDK or application validation skill when present. Otherwise derive commands
from that revision's repository-native docs and build metadata. Read
`frontend/package.json` to determine whether the browser client consumes a
published TypeScript version or the in-tree `file:../sdk` package. The native
path remains on the Rust crate pinned in `frontend/src-tauri/Cargo.toml` until
the coordinated proxy/Rust switch. Test browser Research and native Agent paths
independently when both consume the change. An unavailable client checkout
leaves that layer unverified.

Configure billing or feature-flag API URLs/keys only when their public backend
outcome is in scope. Treat them as external HTTP dependencies and test the
changed success, denial, timeout, and unavailable behavior.

## Tier 5: validate Nix and release artifacts

For current-host flake or packaging changes:

```sh
nix flake show --all-systems --no-write-lock-file
nix flake check --no-write-lock-file --print-build-logs
nix build --no-link --no-write-lock-file .#default
```

EIF construction, PCR comparison, and reference/history updates are
release-only work. The Nix Reproducible Builds workflow builds the
development EIF on pull requests but skips PCR comparison there. Master
pushes and `workflow_dispatch` still compare against checked-in
references. Ordinary pull-request completion does not update PCR
references. If a master or release build succeeds and then fails only
PCR comparison, treat that as deferred release work and do not copy or
sign CI values just to make the job green. Treat an EIF build failure
separately.

Immediately before an authorized dev or prod publish/deployment, use the
supported Linux/ARM64 release builder and the operator runbook in
`docs/nitro-deploy.md` to build the exact target, review its measurements, and
deliberately update and verify the appropriate references and history. PCR
mutation, signing, artifact transfer, KMS changes, enclave lifecycle, staging,
and deployment require explicit authorization.

## Report without overclaiming

Record the commit and dirty state, host, exact commands, test/ignored/skip
counts, disposable database lifecycle, external authorization category, client
configuration, and every unrun or unavailable layer. For release evidence,
also record the target artifact and PCR source.

Use narrow labels: **static/unit**, **disposable DB**, **live provider**, or
**local encrypted full stack**. Use **Linux/Nitro/PCR** or **deployed** only for
authorized release/deployment evidence.
Failed, skipped, ignored, interrupted, timing-dependent, and unavailable checks
remain exactly that; do not turn partial evidence into “fully tested” or
“production ready.”
