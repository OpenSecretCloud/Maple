# Maple Agent update delivery

Independent Agent Worker and R2 publisher. Research's `services/updates/`,
`maple-updates` Worker, domain mapping, publishing workflow and installed clients
are unchanged. This code creates no Cloudflare resources and starts no releases.
See [publisher tooling](publisher/README.md) for exact commands and retry behavior.

The checked-in bucket binding is deliberately a placeholder. There is no deploy
script or automatic deployment workflow. `workers_dev` and preview URLs are off;
routine configuration must never contain `route`, `routes` or a Custom Domain.

## Local verification

From the monorepo root:

```sh
nix develop --no-update-lock-file .#ci -c ./scripts/ci/agent-updates.sh
```

This runs schema and Worker tests, real workerd/R2 fixture requests, publisher
failure/retry tests and a deploy dry run. Fixtures use fake signatures: they
prove delivery and parsing, while publisher tests and the client own pinned-key
authentication. None of these tests reads a production bucket or executes a
downloaded app. Source checks do not establish Cloudflare permissions or a
successful packaged update.

## Public contract

Only GET and HEAD are supported. Metadata and errors use `no-store` at both
browser and CDN layers. No origin fetch is made on failure.

| Path                                     | Behavior                                                                         |
| ---------------------------------------- | -------------------------------------------------------------------------------- |
| `/agent/stable/latest.json`              | Exact signed envelope from `stable/latest.json`                                  |
| `/agent/stable/releases/V/manifest.json` | Immutable signed envelope                                                        |
| `/agent/stable/releases/V/FILE`          | Signed, enumerated artifact; GET/HEAD, single byte ranges, ETag, immutable cache |
| `/download/agent/stable/macos`           | Uncached 302 to the current DMG                                                  |
| `/download/agent/stable/linux-appimage`  | Uncached 302 only when Linux is present                                          |

Missing objects/targets return 404. Malformed metadata, mismatched stored sizes
and bucket failures return bounded 503 responses. The Worker checks schema and
minisign structure; it has no signing key. Clients must authenticate exact
manifest bytes with their pinned keys before trusting fields or hashes.

The envelope has exactly `manifest` (JSON text) and `signature` (minisign text).
The manifest has `format: maple-agent-update-v1`, `product: maple-agent`,
`channel: stable`, canonical stable SemVer `version`, positive decimal-string
`build` (up to 18 digits), lowercase `source_sha`, UTC second-resolution
`pub_date`, plain-text `notes`, `artifacts`, and `downloads`.

`artifacts.darwin-aarch64` is the `.app.tar.gz`; `downloads.macos` is the DMG.
Optional `artifacts.linux-x86_64` and `downloads.linux-appimage` must describe the
same `.AppImage`. Each artifact contains exactly `path`, positive `size` (at most
4 GiB) and lowercase SHA-256 `sha256`. Paths must start
`/agent/stable/releases/<version>/` and end in one ASCII filename using letters,
digits, `.`, `_`, `-`; traversal and encoded separators are rejected. Supporting
build-info and provenance objects in R2 are private, not public artifact routes.

Limits: 128 KiB envelope, 64 KiB manifest, 4 KiB minisign signature, 16 KiB notes.
Signature format is prehashed minisign Ed25519 with exact trusted comment
`product=maple-agent channel=stable version=<version>`. No expiry is claimed.

## Publication and authority

`agent-updates-publish.yml` is manual, trusted-master-only, and serialized with
operator pointer changes through `agent-updates-production`. It selects an exact
completed trusted-master `agent-desktop-build.yml` run, requires the successful
Prod producers and final download verifiers for every included platform (not
merely overall workflow success), verifies every final file's
GitHub attestation against master and the source commit, and validates build
profile, version, inventory and hashes before receiving signing/bucket secrets.
Unrelated Dev or omitted-Linux failures do not block a verified macOS-only
publication. Downloaded executable/package content is never executed in
credentialed jobs.

Publication uses create-only immutable objects, reuses identical bytes, resumes
missing objects and rejects differing bytes for a reserved version. The complete
signed manifest is created after payload verification. Only the latest pointer
is mutable, using an atomic compare-and-swap. A retry of the identical promoted
release verifies it as a no-op, including after a failed live verification.
Never delete a reserved release to make a conflicting retry pass. Publish a new
version or resolve the exact failure with the owner.

The owner must prepare and approve these inputs before activating delivery:

- A private existing Agent bucket, no `r2.dev`/bucket custom domain, retention
  and immutable-object overwrite/delete policy protecting release prefixes.
- Separate Agent-only macOS package signing, update-signing and R2 publication
  environments with required reviewers and master-only branch policy. Signing
  uses new Agent primary/offline backup keys; never Research's key.
- Bucket-scoped S3 Object Read & Write credentials, an approved account/bucket,
  public primary key and exact HTTPS origin. R2 credentials cannot deploy Workers.
- An existing Agent Worker and scoped Worker Editor deployment credential;
  confirm a pinned code deployment with the R2 binding preserves Routes. No
  Route or Custom Domain management is part of routine code deployment.
- Exactly `https://updates.trymaple.ai/agent/stable/*` and
  `https://updates.trymaple.ai/download/agent/stable/*`, separately owner-managed.
  Confirm zone/account/proxied DNS, HTTP redirects and edge allowlisting first.

Workflow setup is explicit and initially absent:

| Scope                                            | Required input                                                                |
| ------------------------------------------------ | ----------------------------------------------------------------------------- |
| Repository variables                             | `AGENT_UPDATES_ENABLED=true`, `AGENT_UPDATE_PUBLIC_KEY` (public minisign key) |
| `agent-updates-production` environment variables | `AGENT_R2_ACCOUNT_ID`, `AGENT_R2_BUCKET`                                      |
| `agent-updates-production` secrets               | `AGENT_R2_ACCESS_KEY_ID`, `AGENT_R2_SECRET_ACCESS_KEY`                        |
| `agent-update-signing` secret                    | `AGENT_UPDATE_SIGNING_KEY` (dedicated unencrypted minisign key-file contents) |

The public key must also be pinned in both the chosen build's Prod source profile
and the publisher's trusted master profile. An empty source key list fails closed;
setting workflow variables cannot replace those pins. Production verification uses
the fixed `https://updates.trymaple.ai` origin. No Worker deployment token belongs
in any publication environment. Protect both environments with required reviewers
and master-only branch policy before activation.

Before/after route activation compare Research's metadata bytes and all six
GET/HEAD download redirects, accounting for legitimate Research releases. In an
isolated rehearsal verify Research-style Worker redeploy/domain reattachment
preserves Agent routes; recheck after the next authorized Research publication.
Do not create a Research release for this check.

An unadvertised signed `0.0.1` to `0.0.2` rehearsal through real packages, feed,
keys and UI is required before invitations. Include Linux only after its own
packaged launch/update/relaunch gates pass. This service does not prove those
client gates or authorize a deploy/publication.
The separately authorized bucket rehearsal must also demonstrate identical
retries under its actual release-prefix lock policy, differing-byte refusal,
missing-object repair and conditional pointer-race rejection. Mock storage and
request-signature tests do not establish those live R2 properties.

Pointer rollback changes discovery, not installed apps. Install-time client
revalidation is required. Freeze/replay protection is limited in v1; a compromised
accepted key may require manual trusted reinstall. Keep the offline backup
separate and document key retirement/manual recovery before production signing.
