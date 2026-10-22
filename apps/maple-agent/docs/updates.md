# Agent updates

Packaged Prod builds use the Agent-only signed update service at
`https://updates.trymaple.ai/agent/stable/latest.json`. Dev and unpackaged
builds do not check that service. The production public-key list is intentionally
empty until reviewed owner setup; those builds show **Updates not configured**
and make no updater requests. Environment variables cannot supply a feed or
signing key. `MAPLE_DISABLE_UPDATE_CHECK=1` can only disable checks.

This feature does not change Research's updater, update Worker, publisher,
release selection, signing configuration, or download paths. Agent publication
does not create GitHub Releases or tags and cannot change the repository's
latest Release.

## User behavior

The desktop checks at startup and about hourly, with jitter. Automatic download
is enabled by default and can be disabled in Settings. An authenticated update
is downloaded in the background, but replacement requires **Restart to Update**
and confirmation that active work will stop. **Later** hides the banner while
keeping the prepared update available in Settings. About, the native application
menu, and the tray provide update access.

The installer holds the per-application lock while staging a verified bundle
beside the installed application and while that update is ready. It checks the
fresh signed offer again and holds the profile's existing startup and host ownership locks from host stop
through replacement. It checks the payload and staged tree again before macOS's
atomic bundle exchange. A previous bundle remains in the staging directory until
the new installed version starts successfully and validates its receipt. This
is crash recovery retention, not automatic rollback or permission to install an
older signed version.

After replacement commits, the old process retires its backend and enters
**Restart required** before asking the platform to relaunch. If the restart
helper cannot start, **Restart Maple** retries only that committed application
path; Quit remains available so the user can reopen the installed update.
Checks, downloads, a second installation and new backend work cannot resume in
the old process. A relaunch failure never triggers an automatic downgrade.

Read-only locations, disk images, translocated bundles and accounts lacking
replacement permission offer a manual download before active tasks stop. The
installation lock uses one shared inode for the canonical application, readable
by other Mac accounts; separate profile locks do not replace that exclusion.
Permissions are rechecked before host shutdown and replacement can still fail
safely if the filesystem changes afterward.

Linux currently offers manual installation: the AppImage self-launch
repair is a prerequisite, and does not by itself prove safe replacement and
relaunch. Enabling Linux automatic installation requires the packaged gates
below. Unsupported architectures do not install another architecture's payload.

## Wire and trust contract

The response is an envelope with two fields: `manifest` is the exact JSON string
that was signed, and `signature` is a four-line, prehashed minisign signature.
The authenticated trusted comment is
`product=maple-agent channel=stable version=<version>`. Clients verify a compiled
Agent public key before interpreting the manifest. Research keys are not used.

The `maple-agent-update-v1` manifest identifies `maple-agent`, `stable`, a
canonical stable SemVer, a positive decimal build number, the source commit,
UTC publication date, bounded plain-text notes, updater artifacts, and manual
downloads. macOS arm64 uses `.app.tar.gz` for installation and `.dmg` for manual
download. Optional Linux x86_64 entries reference the same `.AppImage` in both
inventories. Each object has a signed path, byte size, and SHA-256.

Paths stay under `/agent/stable/releases/<version>/<filename>`. Metadata is
bounded to 128 KiB, manifest text to 64 KiB, signature text to 4 KiB, release notes
to 16 KiB, and each artifact to 4 GiB. Download redirects admit only the two
compiled HTTPS origins `updates.trymaple.ai` and `updates.maple.ai`, on port 443;
requests have timeouts and no application cookies,
credentials, referrer, or inherited proxy. HTTP content decoding is disabled so
signed sizes and resume offsets refer to the same bytes. A partial response must
match the requested range, and the complete file is hashed before preparation.

Updater state is private to the compiled profile. It records the highest verified
version and rejects older offers. A moved application keeps that high-water mark
and clears its old preparation binding. Restored preparations are authenticated
and rehashed; saved paths are not authority. Before installation a withdrawn or
changed offer clears preparation. A newer offer immediately starts the normal
check/download path. A malicious service can withhold a release; signed metadata
has no expiry, and this implementation does not claim freeze-attack prevention.

macOS installation also requires the expected bundle ID
`cloud.opensecret.maple.agent`, Developer ID team `X773Y823TN`, signed version and
build, hardened code-signing identity, and a bundle tree matching the downloaded
archive. Extraction bounds entries, expanded bytes and path lengths; it rejects
path traversal, hard links, special files, duplicate paths and escaping or cyclic
symlinks while allowing internal framework links. No downloaded executable runs
before replacement.

## Owner setup before activation

Implementation PRs do not create infrastructure, production signing keys,
GitHub environments, routes, credentials, releases, or production publications.
Before enabling the feed, prepare and review these concrete operations:

1. Create two dedicated Agent minisign key pairs, retain the backup offline, and
   commit only the two reviewed public keys in `release-profiles.json`. Rebuild
   the initial `0.0.1` package with those pins. Test keys must never become Prod
   pins. Key rotation is a separately reviewed release operation.
2. Reuse the existing `desktop-signing` environment and its five Apple
   signing/notarization secrets for the trusted master Agent packaging job. No
   new app-signing environment, Apple credentials or BWS migration is required;
   keep the existing environment's credentials and policies unchanged.
3. Provision the Agent R2 bucket and Worker. Bind only the Agent bucket. Create
   the two HTTPS routes for `/agent/stable/*` and
   `/download/agent/stable/*` as an owner operation. Keep routes out of routine
   Worker deployment configuration so deploy tokens do not need route-write
   authority. Configure HTTP-to-HTTPS handling in the zone.
4. Reuse the existing `updates-production` approval environment and BWS access
   credential for Agent reservation, update signing and publication. Configure
   the Agent signing-key and bucket-item IDs documented by the service without
   changing the existing environment's policies or Research credentials. The
   workflow maps BWS outputs only into the consuming step; shared BWS grants and
   environment access remain a trusted workflow boundary. Publication must verify that the
   selected artifact's embedded pins and trusted master both contain the chosen
   public key; an environment variable is not a substitute for compiled trust.
5. Deploy with the narrowly scoped existing-Worker token and confirm the R2
   binding can be retained. Verify Agent activation leaves Research working,
   and a later normal Research publication leaves Agent routes working.

The service and publisher's own README documents exact commands and recovery.
Artifact builds remain separate from publication. Only final verified Prod
packages from the trusted desktop workflow may be published; selector jobs and
unsigned PR packages are not release evidence. Immutable version objects are
create-only and pointer promotion is conditional, so retries cannot overwrite
an existing version or race a later release.

## Validation and release gates

Run Agent checks through its pinned environment:

```sh
cd apps/maple-agent
nix develop --no-update-lock-file -c just ci
```

Workflow and configuration changes also require the root pinned
`nix flake check --no-update-lock-file`. Focused core and installer tests cover
signature, schema, anti-rollback, interrupted downloads, saved-state tampering,
archive/path attacks, staging integrity, replacement receipts and lock exclusion.
Prod macOS packaging also runs the real installer archive scanner against each
produced unsigned PR or signed master `.app.tar.gz`, binding the scanned bytes to
the package checksum before final upload/attestation. BSDtar host metadata is
explicitly omitted during creation; signed file contents and existing native
signature/notarization checks remain intact.
They do not prove Developer ID distribution, deployed service behavior, or a
successful real package update.

Before invites, publish an unadvertised signed `0.0.1`, then `0.0.2`, and run the
actual installed application's check, download, confirmation, host stop, swap
and relaunch through the production service. Verify active host clients cannot
start during installation, the new version works, and a failed or interrupted
attempt preserves a runnable installation. Keep version, build, source SHA,
workflow run, manifest, artifact hashes, OS, install location and observed result
with the rehearsal evidence. The invitation date slips if these gates fail.

For Linux, first verify AppImage GUI, host, ACP and reopen behavior on the target
Ubuntu environment, including spaces in the path and the supported extraction
mode. Then verify a real AppImage N-to-N+1 replacement and relaunch with host
exclusion and failure recovery. Until that evidence exists, Linux automatic
installation stays disabled and Linux does not count as passing the preview's
updater gate. Do not infer package proof from a launcher fixture or Rust unit test.

Recovery must preserve the signed high-water mark. Fix a bad offer by publishing
a new, higher version; do not rewrite an immutable version or lower the pointer
and expect clients to downgrade. Keep the previous installation until the new
startup checkpoint succeeds. If a disk image, permissions, signature, host stop,
or network check prevents installation, use the verified manual download path
or retry without weakening validation.
