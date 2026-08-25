---
name: release-maple
description: Prepare, publish, monitor, and verify a Maple release from current master. Use when asked to bump a release version, cut or create a GitHub release, verify signed artifacts and updater metadata, monitor downstream publication, or prepare an explicitly authorized App Store, TestFlight, Google Play, or billing-API handoff.
---

# Release Maple

Treat every release step as a production action. Do not use this workflow for
routine validation. Before a write, state the exact repository, version, tag,
commit, external effect, and authority provided by the user.

## Know the triggers

- A push to `master` starts production-shaped desktop, Android, iOS, web,
  frontend, and Rust workflows. The iOS master workflow uploads its verified
  IPA to TestFlight automatically.
- Creating a GitHub Release starts the cross-platform release workflow. A
  successful release workflow starts separate updater-metadata, Pages
  production-branch, and best-effort Zapstore workflows. These sibling
  workflows never gate or change the outcome of the core Maple release.
- GitHub Release creation does not itself submit the release IPA or AAB to
  Apple App Store review or Google Play.

Never push or merge `master`, create a release, retry a workflow, upload to a
store, submit for review, or alter a rollout merely to see whether it works.

## Prepare the version

1. Prepare the bump on a clean focused branch based on current `origin/master`;
   do not switch another worktree to `master` or force its owning worktree off
   that branch. Compare the checked-in version with the latest release:

   ```bash
   git fetch origin master
   git merge-base --is-ancestor origin/master HEAD
   current_version="$(nix develop --no-update-lock-file .#ci -c just get-version | tail -n 1)"
   released_version="$(gh api repos/OpenSecretCloud/Maple/releases/latest --jq '.tag_name | ltrimstr("v")')"
   printf 'current=%s released=%s\n' "$current_version" "$released_version"
   ```

2. If `current_version` is newer, retain it. Never bump again merely because a
   release was requested.
3. If versions are equal, establish the intended next version. Proceed when
   the user names an exact version or patch/minor/major level. If the user
   delegates the choice, use patch; do not infer minor or major from commits.
4. On a focused branch, run exactly one repository helper:

   ```bash
   nix develop --no-update-lock-file .#ci -c just update-version X.Y.Z
   nix develop --no-update-lock-file .#ci -c just bump-patch
   nix develop --no-update-lock-file .#ci -c just bump-minor
   nix develop --no-update-lock-file .#ci -c just bump-major
   ```

5. Review all manifest, Apple project, Android version-code, and
   `frontend/src-tauri/Cargo.lock` changes. Run the applicable Maple validation
   gates and submit the isolated bump through normal review when authorized.
   Do not use `just release`; it creates a local tag before the reviewed GitHub
   flow.
6. After the bump merges, use or create a clean worktree on `master`. If another
   worktree already owns that branch, use its checkout instead of forcing or
   stealing it. Pull with `--ff-only` and wait for every required workflow on
   the merged commit. Release only that commit; preflight verifies it again.

## Run preflight

Run the bundled fail-closed preflight from the repository root:

```bash
preflight="$(.agents/skills/release-maple/scripts/preflight.sh)"
printf '%s\n' "$preflight" | jq .
tag="$(printf '%s' "$preflight" | jq -r .tag)"
previous_tag="$(printf '%s' "$preflight" | jq -r .previous_tag)"
head_sha="$(printf '%s' "$preflight" | jq -r .head_sha)"
```

The script requires a clean current `master`, exact manifest version parity, a
newer version and unused tag, and successful required workflows for the exact
commit. Stop on any failure; correct it through the normal reviewed process.
Never overwrite or move a release tag.

Preview GitHub's generated notes:

```bash
gh api --method POST repos/OpenSecretCloud/Maple/releases/generate-notes \
  -f tag_name="$tag" \
  -f target_commitish="$head_sha" \
  -f previous_tag_name="$previous_tag" | jq -r '.name, .body'
```

Confirm the notes span the intended changes and recheck that `head_sha` is
still `origin/master`. Present the tag, commit, previous tag, and notes to the
user before creating the release unless the current request already gives
unambiguous authority for that exact release.

## Publish once

Create the GitHub Release exactly once. This creates the tag in the same flow:

```bash
gh release create "$tag" \
  --repo OpenSecretCloud/Maple \
  --target "$head_sha" \
  --title "$tag" \
  --generate-notes
```

Do not create or push a local tag first. Record the release URL and confirm the
release and workflow resolve to `head_sha`.

## Monitor release CI

Find and watch the new `Release` run:

```bash
gh run list --repo OpenSecretCloud/Maple --workflow Release --event release \
  --commit "$head_sha" --limit 10 \
  --json databaseId,displayTitle,headSha,status,conclusion,url

gh run watch RELEASE_RUN_ID \
  --repo OpenSecretCloud/Maple --exit-status --compact
```

Stay with every platform build, signature/canonical proof, artifact upload,
updater `latest.json`, aggregate verification, and verification-guide step.
Packaging success alone is not runtime smoke; inspect the workflow's actual
verification and attestation results.

After `Release` succeeds, inspect the two required publication handoffs. Do
not rerun the core Release to repair either sibling:

```bash
gh run list --repo OpenSecretCloud/Maple --workflow 'Publish updater metadata' \
  --commit "$head_sha" --limit 10 \
  --json databaseId,status,conclusion,headSha,createdAt,url

gh run list --repo OpenSecretCloud/Maple --workflow 'Promote Pages production' \
  --commit "$head_sha" --limit 10 \
  --json databaseId,status,conclusion,headSha,createdAt,url
```

The updater workflow must publish the verified `latest.json` before reporting
the desktop updater control plane current. The Pages workflow advances the
machine-owned `pages-production` ref to the exact stable release SHA; its
success proves the ref mutation, not Cloudflare's external build or live-site
result. Verify the corresponding Cloudflare Pages check/deployment separately
before reporting Maple web production current. A failure in either sibling is
reported and repaired in that workflow without changing the completed release
artifacts.

On failure, read the failed logs before acting:

```bash
gh run view RELEASE_RUN_ID --repo OpenSecretCloud/Maple --log-failed
```

Retry only a terminal failure proven to be transient infrastructure trouble:

```bash
gh run rerun RELEASE_RUN_ID --repo OpenSecretCloud/Maple --failed
```

Do not classify version/proof mismatches, deterministic builds, signing
failures, missing credentials, or integrity checks as transient. Do not delete
or recreate a published release without separate explicit direction.

## Verify the release and optionally inspect Zapstore

Verify the published release and its assets:

```bash
gh release view "$tag" --repo OpenSecretCloud/Maple \
  --json tagName,name,isDraft,isPrerelease,publishedAt,targetCommitish,url,assets
```

Zapstore starts only after `Release` succeeds and is strictly best effort. Its
queued, running, skipped, or failed state must not delay release completion,
trigger a release retry, or be reported as a Maple release failure. Inspect it
only when Zapstore status is specifically useful:

```bash
gh run list --repo OpenSecretCloud/Maple --workflow 'Publish to Zapstore' \
  --commit "$head_sha" --limit 10 \
  --json databaseId,status,conclusion,headSha,createdAt,url
```

Do not retry or repair Zapstore as part of the Maple release flow. A separate
explicit request may authorize investigating or retrying Zapstore itself.

Do not call the core repository release complete while its required `Release`
workflow is queued or running. Report updater publication and Pages production
as separate downstream states. Zapstore is not a required release workflow.

## Store and API handoff

Apple and Google actions remain manual production operations. Do not open a
store console, choose a track, add testers, upload a build, answer compliance
questions, submit for review, release an approved version, or change a rollout
without explicit authorization for that exact action.

For an authorized handoff:

1. Identify the artifact from the exact tag and commit.
2. Verify its digest, platform signature, application/bundle ID, visible
   version, build/version code, and repository release proof.
3. Record the destination application, tester group or release track,
   countries/audience, rollout choice, and any review/compliance state before
   submission.
4. After the store reports a result, distinguish upload, processing, testing,
   review, approval, rollout, and public availability. Do not infer one state
   from another.
5. If the approved client version is gated by a configured billing API, verify
   that API recognizes the exact `vX.Y.Z` version. Any service-side version-gate
   change or deployment is outside this repository and requires its own
   reviewed workflow and authority.

Keep time-specific build numbers, review outcomes, blockers, and rollout facts
in the release handoff or issue that owns them, not in this evergreen skill.

## Report

Report the version, tag, exact commit, release URL, main workflow URL and
attempt count, artifact verification result, any required-workflow retry and
supporting evidence, authorized store/API actions, and every boundary that
remains unverified. If Zapstore was inspected, report its status separately as
non-gating. Separate repository release completion from store distribution and
live application availability.
