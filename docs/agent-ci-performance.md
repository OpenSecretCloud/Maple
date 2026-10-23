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

## macOS cache retention and repeated application compilation

The [master macOS workspace job at `dbb8e397`](https://github.com/MaplePrivacyLabs/Maple/actions/runs/39915101523/job/120286893796)
passed in **18m31s** after restoring its exact target cache. Its measured build
phase was **8m26s** and its test phase **4m42s**. Native dependencies were reused.
The [later `c9cf8522` job](https://github.com/MaplePrivacyLabs/Maple/actions/runs/39919954389/job/120302912965)
passed in **50m08s**, with a cache miss and **33m21s** measured compilation. Its
SQLCipher/OpenSSL native build alone occupied **13m23s**, overlapping other
compilation. Source revisions and hosted runners differ; these are observed
phases, not a controlled hardware benchmark or additive estimates of savings.

The two environment identities differed only in the complete Agent `justfile`
hash. Three Linux packaging-test recipe additions had invalidated macOS and
Windows even though those workspace jobs invoke Cargo directly. Configuration
fingerprints now follow the command owner:

- Linux workspace CI retains the entire `justfile`, including recipe variables
  and dependencies. Direct macOS/Windows workspace builds do not hash it.
- Nix flake inputs apply to macOS/Linux; the Windows native-setup helper applies
  to Windows. Provider and release helpers apply to their respective profiles.
- Actual Rust/Cargo, compiler/native binaries, SDK, flags, Cargo configuration,
  manifest features/profiles and common SQLCipher provenance checks remain
  part of the compatibility boundary. Exact dependency keys and successful
  master-only cache writes retain their existing policies.

Changing this fingerprint policy intentionally creates a new cache generation.
Its first successful master run must seed it; a cold first PR is not evidence
that the new scope failed. Keep sufficient storage for overlapping native-cache
generations. The read-only audit found a configured **10 GiB** GitHub cap and
**10.085 GiB** of entries, including an obsolete **2.42 GiB** Windows generation.
That establishes quota pressure, not the cause of every missing entry. Blacksmith
Desktop/Linux caches use a separate backend and are outside that GitHub total.

The app build script also watched Git reference files when they did not exist.
Cargo treats missing watched inputs as dirty, so an unchanged build followed by
test could rerun the script and recompile the app test binary. The warm macOS
sample repeated **3m18s** of app test compilation; the later cold sample repeated
**5m22s**. Hosted timings identify the repeated units; they did not capture the
original Cargo fingerprint reason.

The production Git metadata helper now watches existing Git pointers, using the
nearest existing parent for an absent active branch reference. That parent is
needed to detect a packed branch advancing into a newly created loose ref.
Simply ignoring all missing references can embed an old commit SHA. Offline
fixtures import the production helper and verify unchanged build/test freshness
plus full/short SHA correctness across detached, loose, packed and linked
worktree transitions. The existing dirty-suffix refresh policy remains intact.
The platform CI jobs and component checks run those fixtures with their selected
toolchain. Quantify hosted savings only after the retained Cargo reports confirm
that the repeated application unit is fresh and tests still execute.

## Pull-request package profiles

Agent desktop pull requests build only the Dev profile on macOS and Linux.
The unsigned macOS packager and Linux downloaded-package verifier use the same
Dev-only selection, including fork and stacked PRs. Master pushes and manual
master dispatches retain Dev and Prod builds, signing, attestation and package
verification. The master change selector still requires all four Dev/Prod
downloaded verifiers before considering outstanding changes covered.

This removes four runnable PR jobs (Prod macOS binary, unsigned macOS package,
Linux AppImage and downloaded Linux verifier), reducing desktop jobs from eight
to four. It does not remove platform workspace/provider tests or the separate
plain Linux release build. The job-count reduction saves their runner work;
the slowest remaining job still determines PR wall time.
