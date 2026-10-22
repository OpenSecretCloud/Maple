# Agent desktop builds

`Maple Agent Desktop Builds` builds both hosted profiles when Agent inputs change
on protected `master`, and always on a manual dispatch from `master`. It compares
the current source with the last successful ancestor run that actually completed
all four Dev/Prod macOS/Linux downloaded-package verifiers. Failed, in-flight and
selector-only runs do not advance that baseline, so a later unrelated push still
builds outstanding Agent changes. Missing history, API failures or no verified
baseline in the last 100 successful master runs select a full build. PRs continue
to select unsigned previews from their whole merge-base diff.

Unrelated master commits have no new Agent artifacts. Existing downloads keep
their original source SHA and provenance; they are never relabeled as a newer
commit. A manual dispatch from `master` forces fresh packages, including when
the previous run's 30-day artifacts have expired. macOS ARM64 builds are
Developer ID signed, notarized, and stapled. Linux x86_64 builds are AppImages
with a bundled runtime; CI also checks each downloaded AppImage's command-line
startup in an Ubuntu 24.04 container with no Nix installation and no network
access. Each master run completes independently; newer PR updates cancel older
previews.

| Profile | Application | macOS bundle ID | Configuration/data namespace | Update delivery |
| --- | --- | --- | --- | --- |
| Dev | Maple Agent Dev | `cloud.opensecret.maple.agent.dev` | `maple-agent-dev` | Disabled; master Actions artifacts |
| Prod | Maple Agent | `cloud.opensecret.maple.agent` | `maple-agent-prod` | Signed Agent feed, after production keys are configured |

The binary embeds its profile, hosted API/client/billing settings, and PCR
environment. `--build-info` prints the public build identity as JSON without
opening the application or reading a saved account. Package verification checks
that identity against the requested profile and the package manifest. Packaged
Dev and Prod use their own state directories and do not adopt legacy Agent or
Research state. The development launcher retains its existing workspace
configuration and uses a separate debug app identity.

macOS compiles each profile in a job without signing credentials and uploads
its prebuilt binary. Packaging runs on a fresh runner that downloads that
binary and checks its embedded profile and source commit against the run's
checkout before the signing step receives Apple credentials. It reads a dedicated
Mach-O JSON section without executing artifact code, including before signing.
No app binary runs anywhere on the signing runner. Packaging does not restore a
Cargo cache or compile the application. For Prod, the packaging runner first
compiles a small updater-only scanner from its trusted checkout, before the step
receiving Apple credentials. After packaging, that tool reads the actual
`.app.tar.gz` through the installer's archive scanner, without extracting or
executing the app, and checks its SHA-256 against the package checksums before
final artifact upload or attestation. The same gate covers unsigned Prod PR
packages. Dev uses a different bundle root and has no production updater.
PR previews use the same prebuilt verification on runners without signing
credentials. Runtime metadata, duplicate
Swift runtime, and real GPUI window/first-render checks run only in credential-free
verification. GUI startup uses empty temporary state and does not sign in.

## Download and install

Open the latest successful `Maple Agent Desktop Builds` run with completed
package verifiers on GitHub Actions; a selector-only run has no packages.
Its artifacts include the profile, platform, and run ID in their names:

- `maple-agent-dev-macos-aarch64-RUN`
- `maple-agent-prod-macos-aarch64-RUN`
- `maple-agent-dev-linux-x86_64-RUN`
- `maple-agent-prod-linux-x86_64-RUN`

Artifact names stay stable when failed jobs are retried, so verification can
reuse a successful profile's package from an earlier attempt. A rerun producer
replaces only its own profile/platform artifact after packaging verification;
the replacement has a new artifact ID and download link. Every package must
still match the run's source commit and pass the same native verification.
Internal macOS binary artifacts use `macos-aarch64-prebuilt` in their names.
They retain the same run/profile identity across retries, so a packaging-only
retry can reuse a successful compilation while that internal artifact is retained.

Final packages are retained for 30 days; internal prebuilt binaries for 7 days.
macOS packages contain a DMG and application
archive; drag the chosen Agent app into Applications. Both Agent profiles can
coexist with Maple Research. Linux packages contain an AppImage; make it
executable and launch it. Each artifact also contains public build information,
a package manifest, and checksums.

Native BSDtar creation explicitly disables xattrs, file flags, ACLs and macOS
metadata, while preserving signed file bytes, public modes and symlink targets.
`COPYFILE_DISABLE` alone does not suppress all PAX host metadata. Normal native
format support remains enabled for required long paths; unsupported metadata is
rejected by the real installer scanner. Existing extracted-archive codesign,
notarization, Gatekeeper and DMG comparison checks remain required.

Every final master package file, including its public metadata and checksums,
receives GitHub build provenance. The trusted macOS signing job mints provenance
after packaging. Linux compilation uses a read-only token; a separate trusted
job downloads and attests the final Linux artifacts without checking out or
executing repository code or launching artifacts. Fresh verifiers check the
downloaded files against this workflow, `master`, and the exact source commit.
For a downloaded file, verify the provenance with the GitHub CLI:

```sh
gh attestation verify FILE -R MaplePrivacyLabs/Maple \
  --signer-workflow MaplePrivacyLabs/Maple/.github/workflows/agent-desktop-build.yml \
  --source-ref refs/heads/master --source-digest SOURCE_SHA \
  --deny-self-hosted-runners
```

The source SHA is recorded in `build-info.json`. Provenance authenticates the
build origin; Linux checksums alone do not provide that authentication.

Pull requests produce unsigned macOS previews and Linux packaging evidence on
fresh hosted runners. Their macOS artifacts include `unsigned` in the name and
cannot satisfy the signed-artifact verifier. PR previews are not attested.

The macOS GUI smoke establishes window creation, entry into the first render,
and process survival; it does not capture a presented frame or verify login, chat,
microphone or capture permissions. Linux command-line checks do not exercise
X11/Wayland or host GPU drivers; those remain beta runtime checks. Dev checkout
and portal requests use the Dev billing API, and pricing/payment return links
use `app-dev.maple.ai`, which is protected by Cloudflare Access. Prod uses
`app.maple.ai`. Profile changes require checkout success/cancellation and portal
return validation; package metadata alone does not prove those browser flows.

## Packaging commands

From `apps/maple-agent`, using the pinned component shell:

```sh
nix develop --no-update-lock-file . -c ./scripts/build-release.sh dev
nix develop --no-update-lock-file . -c ./scripts/package-release.sh dev --unsigned
nix develop --no-update-lock-file . -c ./scripts/verify-release.sh dev dist/dev --unsigned
```

For a produced Prod updater archive, compile the trusted data-only scanner and
validate the exact package directory (add `--unsigned` for PR packages):

```sh
nix develop --no-update-lock-file . -c cargo build --locked -p maple-agent-updater --example validate-release-archive
nix develop --no-update-lock-file . -c ./scripts/verify-updater-archive.sh dist/prod --unsigned
```

Use `prod` for the production service profile. Official macOS signing is run
only by the trusted CI job using the separate `agent-desktop-signing` GitHub
environment. Application compilation runs on a separate runner; the trusted
updater scanner compiles before the job step receiving Apple credentials. Linux
packaging needs no Apple credentials.

## Distribution and update boundaries

These builds remain Actions downloads and do not advance an updater feed.
The independent Agent publisher promotes only verified Prod artifacts into the
Agent R2 bucket. It creates no GitHub Release/tag and does not redeploy Research.
The client accepts only its signed Agent product/channel/target manifest, with
keys embedded in the selected Prod profile. Empty production keys disable update
traffic until owner setup is complete. See [updater activation](updates.md).

Configure `agent-desktop-signing` and its master-only protection before merging
the workflow's environment change. Keep Research's signing environment, updater
key, publication workflow and compatibility URLs intact. No Research install is
replaced, overwritten or migrated by Agent packaging or publication.
