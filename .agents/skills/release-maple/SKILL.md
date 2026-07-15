---
name: release-maple
description: Publish and verify a Maple GitHub release from master. Use when asked to cut, create, publish, monitor, or finish a Maple release, including checking whether the version was bumped after the previous release, generating GitHub release notes, creating the release tag, monitoring release CI, retrying genuinely transient failures, verifying artifacts, and confirming Zapstore publication.
---

# Release Maple

Run this workflow from the `OpenSecretCloud/Maple` repository. Treat publishing a release as a production action: report the intended tag and commit before creating it, then stay with all workflows until they succeed or a real defect is identified.

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
     -f target_commitish=master \
     -f previous_tag_name="$previous_tag" | jq -r '.name, .body'
   ```

4. Confirm the notes span the intended changes and the reported commit is still `origin/master`.

## Publish

Create and publish the release exactly once. This creates the tag, matching the standard GitHub release GUI flow:

```bash
gh release create "$tag" \
  --repo OpenSecretCloud/Maple \
  --target master \
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
