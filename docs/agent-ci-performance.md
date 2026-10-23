# Agent CI performance audit

This change addresses [issue #1224](https://github.com/MaplePrivacyLabs/Maple/issues/1224).
The baseline below comes from completed Actions traces collected October 22,
2026 (Pacific time), before the cache and job changes. Runs use different
revisions and runner conditions; these are observations, not a controlled
performance benchmark.

| Agent CI run | Workflow elapsed | Windows workspace build/test | Windows provider suite |
| --- | ---: | ---: | ---: |
| [39885038207](https://github.com/MaplePrivacyLabs/Maple/actions/runs/39885038207) | 67m36s | 45m00s, cache miss | 18m16s |
| [39890827847](https://github.com/MaplePrivacyLabs/Maple/actions/runs/39890827847) | 71m02s | 47m01s, cache miss | 19m23s |
| [39876263031](https://github.com/MaplePrivacyLabs/Maple/actions/runs/39876263031) | 52m13s | 29m29s, target cache restored | 19m25s |
| [39842844293](https://github.com/MaplePrivacyLabs/Maple/actions/runs/39842844293) | 77m52s | 48m03s, cache miss | 20m11s |

Windows controlled the critical path. In the baseline its workspace build
reported 33m07s development compilation and 6m03s test compilation; the
provider reported 18m00s compilation and about 11 seconds executing tests.
Linux additionally built an optimized binary for 19m52s after its other checks.
Job creation-to-start delays were 3–9 seconds across 15 sampled platform jobs.
The evidence identifies compilation and serial scheduling costs; it does not
identify CPU, memory or disk saturation.

Local validation also found Cargo selecting a home-directory Rustup Clippy
1.97 proxy while the Nix shell selected Rust/Cargo 1.98. The component shell
now pins `RUSTUP_TOOLCHAIN` to its Nix toolchain, covering external Cargo
subcommands such as Clippy and rustfmt. Linux CI fingerprints Clippy's actual
version as well. This local observation is not a measurement of hosted Clippy
selection or the time saved by the correction.

Cache behavior was mixed: all baseline platform lookups missed, while later
runs restored some workspace targets. All twelve Desktop build lookups across
[39885037989](https://github.com/MaplePrivacyLabs/Maple/actions/runs/39885037989),
[39891392302](https://github.com/MaplePrivacyLabs/Maple/actions/runs/39891392302)
and [39876520067](https://github.com/MaplePrivacyLabs/Maple/actions/runs/39876520067)
missed. Nix installer-cache hits are separate from Cargo target-cache reuse.
The old user key put the full Cargo.lock/flake digest in both exact and restore
keys, preventing compatible fallback after a version change. The upstream
provider suite compiled into a disposable target outside that workspace cache.

In [cancelled master run 39891392236](https://github.com/MaplePrivacyLabs/Maple/actions/runs/39891392236/job/120211837972),
Windows completed 45m20s of workspace build/test, then ran the provider for
12m14s before cancellation. Its workspace cache-save post step never ran.
Independent provider jobs let completed workspace jobs save earlier without
changing master cancellation or signed Desktop concurrency policies.

The implementation preserves the workspace feature matrix, upstream provider
features and provenance checks, static linkage, source regeneration, notices,
optimized binary, and all package/signing/download validations. Compiler/native
compatibility and dependency identity are separate cache boundaries; provider
test artifacts have a dedicated exact-graph cache. Signing, publication and
credential boundaries are unchanged.

## Validation after a trusted cache seed

New cache namespaces are initially cold. PRs cannot seed them because writes
remain restricted to successful master pushes. Do not weaken this rule to
benchmark a PR. Before claiming issue acceptance:

1. Retain the first successful trusted run's three-platform workspace/provider
   and Desktop build logs and `maple-agent-*-metrics-*` artifacts. Record exact
   revision, actual compiler/native identity, cache keys/hit state, action
   restore/save bytes and durations, phase durations and target sizes.
2. Compare a subsequent run using that same dependency/toolchain configuration,
   including a version-only application bump. Confirm compatible restore keys
   and Cargo's reuse separately from total workflow wall time. A PR after a
   trusted seed can demonstrate read-only restore.
3. In unprivileged validation branches, change provider revision, resolved
   upstream test-only lock, helper, toolchain/native ABI, and flags/features one
   boundary at a time. Check that the matching compatibility or exact provider
   key changes and incompatible artifacts are not restored. Local regression
   tests cover fingerprint changes; hosted invalidation remains separate proof.
4. Report wall time, sum of job durations (runner elapsed compute), cache
   transfer/archive bytes, storage usage and failed/cancelled runs for cold and
   warm cases. Keep selector-only runs out of build comparisons. Distinguish
   assertion failures and artifact-service errors from capacity problems.

Moving the baseline Windows provider off the serial path removes an 18-minute
dependency edge; it does not establish an 18-minute end-to-end saving. Extra
jobs duplicate setup/metadata work and maintain additional target caches. Cache
restore/save overhead, storage eviction, test-only resolution changes, and the
new slowest job determine the result. Dev/Prod packages and the plain CI binary
have different build inputs; their outputs have not been proved interchangeable.
Runner upgrades and further scheduling experiments should follow these results.
