# Agent release publisher

This tooling publishes the independent Agent feed. It does not change Research,
create Cloudflare resources, generate keys, create GitHub releases or execute
downloaded applications. Run it from a trusted checkout with the root pinned
`nix develop --no-update-lock-file .#ci` shell (Python, GitHub CLI and minisign).

The workflow separates read-only package verification, bucket reservation,
signing and publication. Reservation/publication use bucket-scoped S3 credentials;
the signing stage receives only a dedicated unencrypted minisign secret-key file.
The public key must already be pinned both in the selected build's source profile
and in the publisher's trusted source profile. An empty key list blocks publication.

```sh
publisher=services/agent-updates/publisher/publisher.py
python3 "$publisher" trust --run-id BUILD_RUN_ID --output trust.json
# Use --linux on trust and --linux linux-artifacts on prepare only when the
# Linux package and installed-update acceptance gates have passed.
python3 "$publisher" prepare --run-id BUILD_RUN_ID --version 0.0.1 \
  --build BUILD_RUN_NUMBER --macos macos-artifacts --notes-file notes.txt \
  --public-key public-key.pub --output bundle
python3 "$publisher" reserve --bundle bundle --public-key bundle/public-key.pub \
  --endpoint https://ACCOUNT.r2.cloudflarestorage.com --bucket AGENT_BUCKET
python3 "$publisher" sign --bundle bundle --public-key bundle/public-key.pub \
  --secret-key /private/temporary/signing-key
python3 "$publisher" publish --bundle bundle --public-key bundle/public-key.pub \
  --endpoint https://ACCOUNT.r2.cloudflarestorage.com --bucket AGENT_BUCKET \
  --base-url https://updates.trymaple.ai
```

`trust` returns the source SHA, selected artifact names, exact run attempt,
publication date and build number as JSON. Only completed trusted master runs of
`agent-desktop-build.yml` qualify. The selected Prod signing/package and downloaded
consumer validation jobs must have succeeded; an aggregate success or skipped job
is insufficient, while an unrelated Dev failure does not block a qualified Prod
package. The latest job inventory retains successful producer jobs when only
failed consumers were rerun. `prepare` repeats this check and verifies every final downloaded file's
GitHub attestation against the exact source SHA, master ref and signer workflow.
Package metadata, public Prod profile, inventory, notarization results and streaming
checksums are validated before copying the binary payloads into the bundle. The
macOS archive's bounded regular `Info.plist` member is read without extraction to
match the selected run's build number, version and application identity. Sparse
archives are rejected; extension headers and cumulative metadata are bounded.

The manifest build number is the selected build run's `run_number`, matching the
packaging workflow's `CFBundleVersion` input. Its application version must match
the selected packages and be a canonical stable SemVer. The default publication
date is the build run's creation date, making preparation deterministic across
retries. `--pub-date` can select another UTC date, but retries must preserve it,
the notes and every other reserved byte. No timestamps are inserted when signing.

`reserve` creates `stable/releases/V/publication-manifest.json` using
`If-None-Match: *`. This private object reserves the exact unsigned bytes before
the signing stage. It also reserves `publication-metadata.json`, an exact hash and
size inventory of the private audit sidecars. Both digests are bound by the
reservation receipt. It recovers an existing complete signed manifest into the
bundle, preserving its envelope on a retry. `sign` requires the reservation
receipt, signs the exact bytes with prehashed minisign and the exact trusted
comment `product=maple-agent channel=stable version=V`, then verifies the signature.

`publish` revalidates the envelope and local payload hashes, confirms the durable
reservation and snapshots the current feed's ETag. It creates immutable payloads,
and the bounded private audit sidecars, verifies each by reading its content back,
then creates the complete signed `stable/releases/V/manifest.json` and promotes
`stable/latest.json` using `If-Match` (or `If-None-Match: *` for the first release).
Existing immutable content is streamed and reused only if its bytes match. A
partially uploaded release resumes without replacing completed objects. A
concurrent pointer change stops promotion. Re-running an already published exact
release verifies the objects, repairs missing objects with create-only writes and
leaves the pointer unchanged; a different envelope at the same or lower version is
rejected. Failed attempts never delete objects or automatically restore a pointer.

The bundle contains raw, provenance-checked build-info, package manifest,
SHA256SUMS and macOS notarization logs under `metadata/`, prefixed by platform,
plus the stable run/source/artifact identity and attestation policy in
`provenance.json`. The current diagnostic run attempt stays in CI's `trust.json`;
it is omitted from durable metadata so a successful verifier-only rerun can reuse
the exact release. A finite filename allowlist and a 4 MiB limit per file
bound this audit inventory. Reservation, signing and publication require exactly
the five macOS sidecars and provenance, plus the three Linux sidecars when Linux
is included; a missing, empty, partial or extra inventory is rejected. These
objects are stored immutably under
`stable/releases/V/metadata/` and verified before the complete manifest appears.
They remain private because the Worker serves only artifacts enumerated in the
signed wire manifest. The publication reservation and metadata inventory are also
private. Missing sidecars can be repaired on an identical retry; differing bytes
are a conflict. Original CI artifacts retain the independently verifiable package
attestations.
R2 credentials use `AWS_ACCESS_KEY_ID` and `AWS_SECRET_ACCESS_KEY`; the publisher
signs every S3 request with SigV4 and includes conditional headers in that signature.
All network requests use HTTPS, fixed timeouts and no implicit redirects. Errors
do not print subprocess output, signing-key paths, request headers or credentials.

Publication finishes by checking the live latest and immutable signed manifest,
signature, full artifact hashes, HEAD lengths, one-byte ranges, and installer
redirect destinations. Repeat that separately with `verify-live --bundle bundle
--public-key bundle/public-key.pub --base-url https://updates.trymaple.ai` after a
transport failure or operational change. If this final check fails after promotion,
inspect the live state and retry the identical bundle; publication may already
have completed. This check does not install or relaunch an application.

Run offline tests:

```sh
nix develop --no-update-lock-file .#ci -c python3 -m unittest discover \
  -s services/agent-updates/publisher -p 'test_*.py'
```

The fake-store tests cover interrupted writes, reservation/envelope reuse,
immutable conflicts, pointer races and already-published recovery. Their synthetic
signatures exercise envelope plumbing without making a cryptographic claim.
The pinned-shell minisign tests verify the shared public wire fixture, generate
disposable offline keys only inside a private temporary directory, sign a reserved
manifest, and exercise verification, tampering and deterministic signing retries.
No production credentials, resources or live release are used by these tests.
