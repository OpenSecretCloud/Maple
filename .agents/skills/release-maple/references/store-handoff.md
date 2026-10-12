# Apple and Google store handoff

Use this reference only for an explicitly authorized store handoff under
`$release-maple`. GitHub publication, TestFlight upload, store review, and public
availability are separate results. Keep release-specific identities, receipts,
screenshots, and blockers in the private handoff; do not put credentials or
console account details in repository guidance.

## Select the verified build

- **Apple:** [production Mobile App CI](../../../../.github/workflows/mobile-build.yml)
  builds and uploads its own verified IPA to TestFlight. The
  [GitHub Release workflow](../../../../.github/workflows/release.yml) exports a
  separate IPA and only attaches it to GitHub. Even at the same commit and app
  version, export-assigned build numbers and digests can differ. Match the selected
  TestFlight build to its production bundle/profile, source SHA, verified export,
  and successful upload receipt. Do not select the Dev app or infer identity from
  the displayed version or newest build number alone.
- **Google:** use the exact tag's final `app-universal-release.aab`, not
  `intermediary-bundle.aab`. Confirm its digest in the final Android checksum
  manifest, downloaded-asset proof, package ID, embedded `versionName` and
  `versionCode`, and actual upload-key signature verification. Inspect verification
  results for skips; a workflow pass is not proof that signature tooling ran.

Record the actual provenance checks performed. Attestation generation, platform
signing, downloaded-byte verification, and installed-device behavior are distinct.
For GitHub assets, use the release's generated verification guide for independent
attestation checks. For an existing TestFlight build, use its Mobile App CI run's
export/proof and upload evidence; a check of the separate GitHub IPA does not
verify that TestFlight export.

## Apple: promote production TestFlight to App Store review

1. Open the intended production app and verify the selected TestFlight build has
   completed processing and is eligible for review. Match its source and upload
   evidence before choosing it for the App Store version.
2. Prepare the App Store version and concise What's New text from that build's
   changes. Check inherited review notes for stale claims about earlier updates;
   preserve valid reviewer access instructions and existing contact information
   without copying credentials into logs or the handoff.
3. Inspect required metadata/compliance fields, territory availability, automatic
   versus manual release, and phased rollout. Preserve existing choices unless
   the requested rollout authorizes a change; do not invent compliance answers.
4. **Add for Review** stages the version. Complete **Submit for Review** to send
   the submission, then record its identifier and resulting status. Apple's
   [submission procedure](https://developer.apple.com/help/app-store-connect/manage-submissions-to-app-review/submit-an-app)
   distinguishes these two actions.
5. Report processing, Waiting for Review, approval, release, and public availability
   separately. Automatic release after approval still depends on Apple review;
   submission is not proof that users can download the update.

## Google: choose tracks without duplicate beta maintenance

The normal Maple handoff uses **Internal testing and Production**. Leave a retired
external beta paused; do not recreate it or publish a duplicate beta merely to
deliver normal updates. Open or closed testing is useful when explicitly requested
for a different build before production.

Open/closed testers are eligible for production and receive the highest compatible
version code on their eligible tracks. A matching or newer production bundle can
serve those users without a duplicate beta release. Internal testers receive only
the internal track, so handle that requested publication separately. See Google's
[track eligibility and fallback rules](https://support.google.com/googleplay/android-developer/answer/9845334?hl=en).

If a beta change is requested, inspect the actual Open and Closed track detail
pages, enrolled audience, release codes, status, and countries. Track names such
as Alpha and a partially loaded overview do not establish which track has users.
Choose the existing audience specified by the user; do not create a new external
track just because the requested label is ambiguous.

When retiring an authorized beta, compare production and beta version codes,
device compatibility, and country coverage first. Use **Pause track** to end the
test. The installed app remains; eligible users can receive a newer production
release normally. If beta is ahead or production does not cover those users,
do not promise an automatic downgrade or require everyone to reinstall. A pause
may itself need review: record both the console state and pending publishing
change. Google's [end-test procedure](https://support.google.com/googleplay/android-developer/answer/9845334?hl=en)
documents the installed-app and version-selection behavior.

## Google: prepare and submit the requested changes

1. Upload the verified final AAB once. For other requested tracks, select that same
   version code from the bundle library or promote the existing release; do not
   rebuild or upload a different bundle just to copy it between tracks.
2. Review release notes, package/version code, countries, rollout percentage,
   device support changes, and validation errors/warnings. Policy notices remain
   unresolved until the console confirms their result; source fixes or an accepted
   upload alone do not clear them. An operator's testing waiver does not waive
   store-required checks.
3. Complete the authorized internal publication and verify its actual track
   status. Save production and any authorized external-track changes, including
   retirement, before review submission when feasible.
4. In **Publishing overview**, inspect the exact changes to be submitted. Exclude
   unrelated pending changes. Check **Managed publishing**: off publishes after
   approval; on holds approved changes for a later publication. Preserve that
   setting unless a change is authorized. Use **Send changes for review** and
   verify the intended items appear in the review state. See Google's
   [review and publishing controls](https://support.google.com/googleplay/android-developer/answer/9859654?hl=en).
5. If adding a later change causes a restart-review warning, inspect which changes
   are affected and proceed only within the authorized scope. Record automated
   checks, review, approval, rollout, and public availability separately; a queued
   quick check is not confirmed human review or a live production release.

## Apply billing gates to the actual platform path

Inspect the selected platform route and billing implementation before treating
a products API's version flag as a store blocker. Research currently routes iOS
pricing through StoreKit and Android through its consumption-only page; a legacy
web products response does not prove iOS purchasing is disabled. Follow the
[pricing router](../../../../apps/maple-research/frontend/src/routes/pricing.tsx)
and [Apple catalog](../../../../apps/maple-research/frontend/src/billing/useAppleProducts.ts)
at the build's source commit. If that path uses a version gate, verify its exact
version. Service-side gate edits and deployments retain their separate authority.

Finish the private handoff with artifact/source identities, checks actually run,
destination and rollout choices, submission receipts, current store states,
unresolved policy/review items, and the layers still unverified. Store approval
and installed-device checks cannot be inferred from repository release success.
