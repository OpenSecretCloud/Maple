# Maple-owned Pages deployments

Maple can publish prebuilt static assets to the existing Cloudflare Pages project
`maple` (`maple-ca8.pages.dev`). Production remains `trymaple.ai`; its production
branch remains `pages-production`. This does not move DNS, the `www` marketing
site, or the updater Worker. The new path does not need a Cloudflare GitHub App.
Cloudflare supports Wrangler uploads to an existing Git-integrated project after
automatic builds are disabled; this does not convert the project's type.
See [Cloudflare's Git integration guidance](https://developers.cloudflare.com/pages/configuration/git-integration/#disable-automatic-deployments).

Merging these workflows does not enable publication. Both repository variables
`MAPLE_PAGES_PREVIEW_ENABLED` and `MAPLE_PAGES_PRODUCTION_ENABLED` must equal the
literal string `true` to enable their respective jobs. Missing/false variables
leave the existing Cloudflare integration and legacy production promoter in use.
Changing flags, secrets, Cloudflare settings, or production needs an authorized
operator action; this guide is preparation, not evidence of a completed cutover.

## Build and destination contract

| Source | Configuration profile | Destination |
| --- | --- | --- |
| Open internal PR targeting `master` | `pr` | `pr-N` Pages preview |
| Current `master` push | `pr` | `master` Pages preview |
| Latest stable Maple release with successful `Release` run | `release` | `pages-production` / `trymaple.ai` |

`Pages preview build` calls `scripts/ci/web.sh` with
`MAPLE_WEB_ENVIRONMENT=pr` for both PRs and master. It builds the PR head SHA,
not a production-configured master artifact. A separate unprivileged checkout
at the workflow/merge SHA supplies manifest tooling for older PR heads.
Path filters cover the existing web build inputs and the Pages CI files.
Fork PRs retain ordinary CI but receive no hosted preview. Pushes to other
branches without a qualifying PR do not get automatic previews from this path.

The fixed build settings come from `scripts/ci/_common.sh`:

| Setting | Preview | Production |
| --- | --- | --- |
| OpenSecret API | `https://enclave.secretgpt.ai` | `https://enclave.trymaple.ai` |
| PCR environment | `development` | `production` |
| Flags | `https://flags-dev.opensecret.cloud` | `https://flags.opensecret.cloud` |
| Billing | `https://billing-dev.opensecret.cloud` | `https://billing.opensecret.cloud` |

Both profiles retain the same public client ID. Vite embeds these public values
at build time; deployment does not substitute Cloudflare dashboard variables.
Production downloads the existing `maple-web-dist.tar.gz` and `web-final.sha256`
from the successful stable GitHub release, checks GitHub digests and checksums,
and uploads those bytes without rebuilding. A master push alone cannot publish
production. The existing master verification artifact is unchanged.

## Credential and artifact boundaries

The preview build has only `contents: read`, no protected environment, no CF
secrets, and no restored caches. It uploads exactly the archive and its manifest
under a name containing the GitHub run ID and attempt. `Publish Pages` executes
only its trusted default-branch checkout; it never executes PR scripts, installs
PR dependencies, or loads PR Wrangler configuration. Checkout credentials are
not persisted and external actions are pinned to commit SHAs. This separation
addresses the [GitHub Security Lab privileged PR execution pattern](https://securitylab.github.com/resources/github-actions-preventing-pwn-requests/).

The publisher rechecks repository IDs, workflow ID/path, event, successful run,
run attempt, source SHA, and the current open internal PR head or master head.
Production additionally checks the exact stable tag, successful release build,
reachability from master, latest-release identity, and forward-only production
ref movement. Superseded sources fail closed, including rerun attempts.

Artifacts remain untrusted data. ZIP/tar parsing bounds compressed and expanded
bytes, individual file sizes, entries, and paths. It rejects links, special
files, traversal, duplicates, ambiguous spelling, hidden paths (except the root
`.well-known` directory used for mobile app links), Functions,
`_worker.js`, Wrangler/package configuration, `_routes.json`, `_headers`, and
`_redirects`. Only static files are extracted, including a required `index.html`.
The publisher rechecks their hashes immediately before upload.

Pinned Wrangler dependencies are installed from trusted `services/updates/bun.lock`
with lifecycle scripts disabled, before CF credentials enter the final step.
Wrangler runs outside the checkout and artifact tree, with no artifact bundling,
an isolated home, and an allowlisted child environment. It receives CF credentials
but no GitHub/BWS token, runner command files, inherited Node options, or proxy
configuration. Its raw output is suppressed and structured results are checked.

A producer can fabricate a manifest alongside an artifact. Hashes and provenance
prove consistency and authorized origin, not that PR JavaScript is honest or
uses the declared endpoints. Internal PR review, Access, development accounts,
and browser smoke remain necessary. Do not enter production credentials into
an unreviewed preview. The publisher's token boundary is separate from browser
trust in the application being previewed.

## Operator prerequisites

1. Create GitHub environments `pages-preview` and `pages-production`. Restrict
   each to the exact **branch** `master` using selected deployment branches/tags;
   allow no tags, PR refs, or wildcard branches. Protect master review and writes.
   Configure any required reviewers before placing credentials in the environment.
2. Create dedicated CI tokens with **Account → Cloudflare Pages → Edit**, limited
   to the intended CF account. Do not grant DNS, Workers Scripts, or Access
   permissions, and do not reuse the machine's broader BWS operations token.
   Set environment secrets `CLOUDFLARE_API_TOKEN` and `CLOUDFLARE_ACCOUNT_ID` in
   each environment. [Cloudflare's CI credential guide](https://developers.cloudflare.com/pages/how-to/use-direct-upload-with-continuous-integration/#generate-an-api-token)
3. Pages Edit is account-scoped, not restricted to one project or preview branch.
   Separate preview/production tokens help rotation and auditing but do not create
   CF project isolation. The trusted publisher enforces its target and branch.
4. Verify project identity, production branch, current successful production
   deployment ID/SHA, and Access coverage for `*.maple-ca8.pages.dev`. Record the
   previous successful production deployment ID in the cutover evidence.
5. Review branch-protection requirements that expect the Cloudflare App check.
   The new path reports GitHub deployments, workflow results, and a PR preview
   comment; it does not impersonate the old Cloudflare App check.

## Staged activation before the repository transfer

1. Run offline validation, then merge the reviewed implementation with both flags
   absent/false. Retain evidence of the current production deployment.
2. Enable only `MAPLE_PAGES_PREVIEW_ENABLED=true`. Trigger a current internal PR
   build and a qualifying master build. Inspect the immutable deployment URLs
   reported by `Publish Pages`; native previews can still exist during the pilot.
   Verify Access requires the intended identity, assets load, login/chat work,
   and browser requests select development API, flags, billing, and PCR history.
3. In an authorized quiet window, pause releases and wait for existing production
   writers to finish. In the existing Pages project's branch controls, disable
   automatic production deployments and set automatic preview deployments to
   None. Keep `pages-production` as the production branch and preserve domains.
4. Set `MAPLE_PAGES_PRODUCTION_ENABLED=true`. This disables the legacy
   `Promote Pages production` job; both paths share the `pages-production`
   concurrency group. `Publish Pages` refuses a production upload if Cloudflare
   still reports automatic production deployments enabled.
5. Manually dispatch `Publish Pages` **from master** to publish the current stable
   release's existing artifact. No new Release or native rebuild is required.
   Verify deployment SHA, active canonical deployment ID, production ref, and
   browser production settings/PCR history; exercise login and chat.
6. After this evidence passes, amend the organization-move runbook in its owning
   repository to replace its earlier replacement-project/domain-cutover plan.
   Transfer separately, then verify the new organization's runner/environment
   access and publication. This PR does not edit that external runbook or transfer.

## Verification and recovery

CI verifies the Cloudflare deployment's successful stage, environment, project,
branch, URL, and commit; production also verifies the active canonical deployment.
It then advances the production ref without force and reports GitHub status.
That is deployment-state evidence, not an authenticated application smoke test.
Access-protected previews require a permitted browser; an automated HTTP 403
alone does not establish a broken application.

Both publisher jobs keep their protected environments but set
`environment.deployment: false`. This prevents GitHub's automatic job-completion
record for the publisher's master checkout from superseding the deployment
reported for the actual artifact SHA. The explicit deployment status owns the
production/preview URL. Branch policies, reviewers, wait timers, and secrets still
apply; custom deployment-protection GitHub Apps are incompatible with this mode
and make the job fail. See [GitHub's environment-without-deployment rules](https://docs.github.com/en/actions/how-tos/deploy/configure-and-manage-deployments/control-deployments#using-environments-without-deployments).

GitHub and Cloudflare do not provide an atomic transaction here. If the source
changes during upload, or later ref/status reporting fails, the new deployment
can already be active even though the workflow fails. Inspect Cloudflare's actual
deployment ID, commit and canonical state before retrying. Publish the newly
eligible source, or use the recorded previous successful **production** deployment
as an explicitly authorized [Cloudflare rollback target](https://developers.cloudflare.com/pages/configuration/rollbacks/).
Preview deployments cannot be production rollback targets. Never force-rewind
`pages-production`, recreate a Release, or change DNS to repair publication.

For a hold, disable the appropriate owned-publisher flag and inspect/drain any
already running job; a flag change does not cancel an upload in progress. Keep
native builds disabled while investigating. Disabling the production flag
reactivates the legacy branch promoter, so native builds must remain off until
an intentional handback. Never reenable native publishing while the owned
publisher is enabled. A CF rollback does not change the GitHub release or ref.

Offline checks from the repository root (use the host system for a local check):

```bash
nix build --no-update-lock-file --no-link --print-build-logs .#checks.x86_64-linux.pages
nix flake check --no-update-lock-file
MAPLE_WEB_ENVIRONMENT=pr nix develop --no-update-lock-file .#ci -c ./scripts/ci/web.sh
```
