---
name: release-maple
description: Prepare, publish, and verify a Maple GitHub release from master. Use when asked to cut, create, publish, monitor, or finish a Maple release, including preparing a missing version bump through the pinned Nix and just workflow, generating GitHub release notes, creating the release tag, monitoring release CI, retrying genuinely transient failures, verifying artifacts, and confirming Zapstore publication.
---

# Release Maple

Run this workflow from the `OpenSecretCloud/Maple` repository. Treat publishing a release as a production action: report the intended tag and commit before creating it, then stay with all workflows until they succeed or a real defect is identified.

## Prepare Version

1. Start from a clean, current `master`. Compare `just get-version` with the latest published release:

   ```bash
   git switch master
   git pull --ff-only origin master
   current_version="$(nix develop .#ci -c just get-version | tail -n 1)"
   released_version="$(gh api repos/OpenSecretCloud/Maple/releases/latest --jq '.tag_name | ltrimstr("v")')"
   printf 'current=%s released=%s\n' "$current_version" "$released_version"
   ```

2. If `current_version` is newer than `released_version`, keep the existing version. Never bump it again merely because a release was requested.
3. If they are equal, prepare the intended next version through the repository helpers. Proceed directly only when the user's current request explicitly names the version or patch/minor/major level. Otherwise ask which version to stage and recommend a patch bump. If the user explicitly delegates the choice, use patch; never infer a minor or major bump from the commit list. After the choice is established, run exactly one of:

   ```bash
   nix develop .#ci -c just update-version X.Y.Z
   nix develop .#ci -c just bump-patch
   nix develop .#ci -c just bump-minor
   nix develop .#ci -c just bump-major
   ```

4. Review the generated manifest and lockfile diff, then commit it on a focused branch, push it, and open a PR. Do not use `just release`; it creates a local tag before the reviewed GitHub release flow.
5. Wait for required PR checks, merge the bump, switch back to `master`, pull with `--ff-only`, and wait for every required master workflow to succeed. Then continue with preflight. Never release from the bump branch or before the merged commit's CI is green.

## Preflight

1. Run the bundled preflight from the repository root:

   ```bash
   preflight="$(.agents/skills/release-maple/scripts/preflight.sh)"
   printf '%s\n' "$preflight" | jq .
   tag="$(printf '%s' "$preflight" | jq -r .tag)"
   previous_tag="$(printf '%s' "$preflight" | jq -r .previous_tag)"
   head_sha="$(printf '%s' "$preflight" | jq -r .head_sha)"
   ```

2. Stop if preflight fails. Fix the branch/version/CI issue through the normal PR process before releasing. Never overwrite a tag or release.
3. Preview GitHub's generated title and notes:

   ```bash
   gh api --method POST repos/OpenSecretCloud/Maple/releases/generate-notes \
     -f tag_name="$tag" \
     -f target_commitish="$head_sha" \
     -f previous_tag_name="$previous_tag" | jq -r '.name, .body'
   ```

4. Confirm the notes span the intended changes and the reported commit is still `origin/master`.

## Publish

Create and publish the release exactly once. This creates the tag, matching the standard GitHub release GUI flow:

```bash
gh release create "$tag" \
  --repo OpenSecretCloud/Maple \
  --target "$head_sha" \
  --title "$tag" \
  --generate-notes
```

Do not create or push a separate local tag first. Record the release URL and confirm the resulting release and workflow point to `head_sha`.

## Monitor Release CI

1. Find the new `Release` run for the tag and commit:

   ```bash
   gh run list --repo OpenSecretCloud/Maple --workflow Release --event release \
     --commit "$head_sha" --limit 10 \
     --json databaseId,displayTitle,headSha,status,conclusion,url
   ```

2. Watch it through every platform build, artifact upload, reproducibility verifier, `latest.json`, full release verification, and verification-guide publication:

   ```bash
   gh run watch RELEASE_RUN_ID --repo OpenSecretCloud/Maple --exit-status --compact
   ```

3. If it fails, inspect the failed job logs before acting:

   ```bash
   gh run view RELEASE_RUN_ID --repo OpenSecretCloud/Maple --log-failed
   ```

4. Retry only failures proven to be transient infrastructure problems, such as interrupted cache/network downloads. Wait for the run to become terminal, then use:

   ```bash
   gh run rerun RELEASE_RUN_ID --repo OpenSecretCloud/Maple --failed
   ```

5. Do not retry version mismatches, proof mismatches, signing failures, missing credentials, or deterministic build failures as if they were transient. Diagnose and report them. Do not delete or recreate the published release without explicit user direction.
6. Continue watching the same run until its latest attempt concludes `success`. A failed attempt may create a skipped Zapstore workflow; this is expected and is not the final Zapstore result.

## Verify Zapstore

`Publish to Zapstore` starts only after the `Release` workflow completes successfully. Find the newest non-skipped workflow for `head_sha`, then watch it through `Verify and install zsp` and `Publish to Zapstore`:

```bash
gh run list --repo OpenSecretCloud/Maple --workflow 'Publish to Zapstore' \
  --commit "$head_sha" --limit 10 \
  --json databaseId,status,conclusion,headSha,createdAt,url

gh run watch ZAPSTORE_RUN_ID --repo OpenSecretCloud/Maple --exit-status --compact
```

Treat pinned Go/zsp verification failures as integrity failures unless logs clearly show transient transport trouble.

## Final Verification

Confirm the release is published, targets the intended commit/tag, and has artifacts:

```bash
gh release view "$tag" --repo OpenSecretCloud/Maple \
  --json tagName,name,isDraft,isPrerelease,publishedAt,targetCommitish,url,assets
```

Report the release URL, tag, commit SHA, main workflow URL and attempt count, Zapstore workflow URL, any retry and its evidence, and final success. Do not call the release complete while any required workflow is queued or running.
