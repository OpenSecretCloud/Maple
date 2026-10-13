# Maple for iOS

Native SwiftUI Maple for iPhone and iPad, targeting iOS 26+. The app uses
`MapleAppCore` for account-owned workflows, `MapleAppUI` for presentation, and
the in-tree Swift `MapleSDK` for OpenSecret authentication and encrypted APIs.
macOS supports package tests only; the app target supports iPhone and iPad.

The development app is displayed as **Maple Native Dev**, with bundle and callback
scheme `cloud.opensecret.maple.native.dev`. Its Keychain service and standard
preferences container are separate from Research. It does not import saved
sign-in or local drafts from another app identity. Research development build
profiles remain independent.

Message rendering uses the local pinned Textual runtime in `Vendor/textual`.
Its bounded Copy export corrections and per-file provenance are documented in
[the rendering guide](docs/rendering.md) and
[the vendor patch notes](Vendor/textual/MAPLE-PATCH.md). Keep that runtime out
of app-wide formatting; update its source and provenance together rather than
editing generated SwiftPM checkouts.

## Build and test

From the Maple repository root, select Xcode 26.5 and use the pinned Apple
shell. No project generator, global Swift tools or signing credentials are
required for simulator builds.

```sh
nix develop --no-update-lock-file .#apple -c bash scripts/ci/ios-native.sh unit
nix develop --no-update-lock-file .#apple -c bash scripts/ci/ios-native.sh simulator-build
```

The checked-in `Maple.xcodeproj` contains the `Maple` scheme. `PR` always uses
fixed public development origins and ignores local overrides. Build success
does not exercise those services or prove authentication.

The independent `ios-native.yml` workflow also runs `cross-build` for Swift
package device/simulator compilation and `device-build` for an unsigned device
app. The `ios_native` component hook runs format lint and deterministic unit
tests only. SDK-only edits do not select consumer packaging; an app manifest
change does. Native app changes never select Research packaging. Use
`scripts/ci/ios-native.sh format` inside the same Apple shell to apply the
Xcode-pinned formatter. `MAPLE_IOS_SWIFT_SCRATCH_PATH` can isolate package builds.

For a local stack, preserve its managed configuration and import only the
public API, billing and flags URLs:

```sh
nix develop --no-update-lock-file .#apple -c python3 apps/maple-ios/scripts/configure-local.py \
  --env-file /absolute/path/to/public-client.env \
  --client-id PUBLIC_APPLICATION_UUID \
  --auth-url http://127.0.0.1:WEB_PORT
nix develop --no-update-lock-file .#apple -c apps/maple-ios/scripts/build.sh build local
```

For an installed simulator app or selected UI tests, choose a simulator from
`xcrun simctl list devices available`, then use its exact UDID:

```sh
MAPLE_IOS_SIMULATOR_ID=SIMULATOR_UDID nix develop --no-update-lock-file .#apple \
  -c apps/maple-ios/scripts/build.sh runtime-build local
```

`runtime-build` runs `build-for-testing`; it builds the app and UI test runner
without installing, launching or executing tests. The deterministic package
`unit` check above does not execute UI tests either. To run one self-contained
composer fixture, use the generated `.xctestrun` file in that build's
`Build/Products` directory and the same simulator:

```sh
nix develop --no-update-lock-file .#apple -c env -u LD -u LDPLUSPLUS -u CC -u CXX \
  xcrun xcodebuild -jobs 2 test-without-building \
  -xctestrun /absolute/path/to/generated.xctestrun \
  -destination 'platform=iOS Simulator,id=SIMULATOR_UDID' \
  -parallel-testing-enabled NO \
  -only-testing:MapleUITests/OwnedComposerUITests/testSoftwareReturnRemainsNewline
```

This example requires the standard software keyboard to be visible and uses
synthetic in-memory text; it does not require account credentials or services.
Rebuild after relevant source changes and record the selected method, artifact,
simulator and authoritative test result.

`build.sh test-simulator local` instead builds and runs the **entire** UI target;
it does not accept a test selector. That target also contains input-delivery
diagnostics and tests with additional setup. `ComposerKeyboardUITests` and the
hardware-key cases in `OwnedComposerUITests` test the available injection path;
missing or stripped key/modifier delivery does not prove app keyboard behavior.
Run those exact diagnostic methods with their native receipt checks, preserving
failures separately from product results.

Real-account UI suites use `MAPLE_RUN_LOCAL_UI=1` in the test runner environment
and need the signed-in local account or consumed private fixture inputs described
in their source. Some create/delete test records or sign out, so select them
against a dedicated local fixture. `ReferenceComparisonUITests` separately uses
`MAPLE_RUN_REFERENCE_UI=1` and a capture descriptor. `BrowserUITests` requires
`MAPLE_LINK_FIXTURE_URL` and a matching local page fixture. Enabling a flag alone
does not supply these prerequisites, and skipped cases do not establish coverage.
Keep credentials out of launch arguments and XCTest typing logs; use ordinary
sign-in or the test's private consumed-input channel.

These runtime modes use Xcode's ad-hoc simulator identity, which provides the
application entitlement required by Keychain. They need no developer team,
certificate or provisioning profile. Compile-only `build` and `device-build`
remain unsigned. The runtime modes also select debug compilation for the
local Swift packages and only the selected simulator architecture.

The env file must provide `VITE_OPEN_SECRET_API_URL`,
`VITE_MAPLE_BILLING_API_URL` and `VITE_OS_FLAGS_BASE_URL`. Every local URL must
be an HTTP loopback origin with an explicit port. The client UUID is public
application configuration, not an account credential. The script never
sources the file or imports unrelated keys. Account credentials are entered
through normal sign-in and kept in the SDK secure store.

`Local` is a debug build. `Release` is an optimized development build using
the same explicit local configuration; run `build release` to inspect it.
Both remain `cloud.opensecret.maple.native.dev`. `device-build` compiles an unsigned
device artifact; installation/provisioning and physical validation remain
separate. These commands do not publish or upload anything.

By default artifacts are in `.build/xcode-<profile>/Build/Products/` under
this directory. `MAPLE_IOS_DERIVED_DATA` can choose another build-output path.
Read `CFBundleIdentifier` and the `MAPLE_*` public settings from the built
`Maple.app/Info.plist` from `runtime-build`, then use `xcrun simctl install SIMULATOR_UDID APP_PATH`
and `xcrun simctl launch SIMULATOR_UDID cloud.opensecret.maple.native.dev` to target
that exact build. Simulator runtime evidence and physical-device evidence
must be recorded separately.

The main menu remembers explicitly expanded projects in an account-scoped,
protected local file excluded from backups. These presentation choices survive
relaunch and sign-out, never sync to the server, and are removed after account
deletion. Search can reveal project contents without changing the saved choice.

On compact layouts, a retained `UINavigationController` owns the full-screen
Main Menu and Chat routes, their root navigation bar, and the standard
interactive back transition. UIKit controls drag progress, flick completion
and cancellation. The admission gate limits this transition to an owned
physical-left-edge contact so transcript and nested code/table scrolling keep
their gestures. Library keeps its SwiftUI stack for Settings and deeper
destinations; that stack takes over the bar while a destination is open.
Root bar publications are fenced by the account/controller lease and the
current chat owner, and retiring a lease removes their controls.

## Account status and catalog

Billing status, team seats and the authenticated model catalog have independent
loading/failure states. A missing or failed plan is never Free. Only a known
Free guest is restricted as a Free guest; read-only plan, quota and seat
messages do not provide purchasing or management actions. Quick/Powerful
remain server-owned aliases; catalog failures keep the saved model sendable.
Disabled, deprecated and non-chat models cannot
be selected; every non-Free catalog tier requires a paid plan for selection.
Web search starts enabled and follows its saved preference, and read-aloud
blocks only a known Free plan; the server authorizes every request.

Billing uses a captured account SDK API to mint a third-party token for the
configured billing origin. It caches that token only in memory and remints
once on HTTP 401; it never signs the SDK out for a billing failure. Retiring an
account invalidates pending billing and catalog results, including a later
sign-in to the same principal.

The opt-in `LocalAccountIntegrationTests` exercise disposable local Free and
Pro accounts through the actual billing and catalog adapters. They require
`MAPLE_IOS_ACCOUNT_LIVE=1`, public loopback origins in
`MAPLE_IOS_ACCOUNT_API`, `BILLING`, `FLAGS` and `AUTH` (each with the
`MAPLE_IOS_ACCOUNT_` prefix), a public `MAPLE_IOS_ACCOUNT_CLIENT_ID`, and the
`MAPLE_IOS_ACCOUNT_PRO_EMAIL`/`PASSWORD` and `FREE_EMAIL`/`PASSWORD` fixture
values in a private child environment. Run the Swift test filter directly
inside the Apple shell; the normal unit/CI recipe deliberately removes those
opt-ins. These read-only checks neither provision accounts nor make purchases.

## Local persistence

`ProtectedChatStore` keeps unsent drafts and recovery identities in one atomic
account manifest. It does not cache server transcripts. Image bytes live in
immutable content-addressed files inside the same account namespace; attachment
IDs cannot overwrite another draft's bytes. The manifest remains small when a
draft includes a supported 20 MiB image. Retained work is bounded to ten images
per draft, 640 retained images and 256 MiB of image bytes per account. Shared image
identities with unchanged bytes count once across draft/edit/recovery references. Metadata
has a separate 16 MiB limit; a failed admission preserves the previous snapshot.

Queue storage includes the original item and position, the working editor, and
the held ordinary draft in the same manifest. Up to sixteen items may wait in
one chat; the account retains up to sixty-four queued turns, including a popped
item that is actively sending. If that active item is provably rejected when all
sixteen waiting slots are filled, a persisted recovery marker retains it as a
seventeenth head item until explicitly resumed or removed. New admission stays
limited to sixteen; recovery never drops accepted work. A queued baseline is limited to 32 KiB of UTF-8
text. Working editor text can exceed that limit while the user revises it; an
unsuccessful queue Save must preserve that work and the original item.

Files and directories are excluded from backups and use Complete Protection on
iOS, with restrictive Unix permissions as an additional measure. The runtime
must report protected-data availability: lock defers writes without deleting
saved work. Account retirement revokes the generation before cleanup and blocks
a new lease until cleanup succeeds. A new sign-in gets a new generation even
when the principal is unchanged.

Version 2 manifests reference blobs by SHA256 and byte count. Version 1 inline
images migrate on the next successful save. Missing/corrupt files and unknown
versions fail closed without replacing saved work. Writes publish protected
image files before atomically replacing the manifest. Old and new image sets
can coexist during that transaction; retained limits apply to the committed
state. Orphan collection runs before another write and on activation, with
bounded directory scans, and sign-out requires complete owned-file cleanup.

New Chat moves retain a stable storage-owner identity and tombstone so delayed
saves cannot recreate an old draft. This identity survives composer swaps and
queue editing while a move is awaiting persistence. Replaying a completed move preserves newer destination edits and
any fresh New Chat draft. Storage tests exercise these cases, interruption at
both publication boundaries, restart, locked-file deferral, and symlink/hardlink
rejection. Actual iOS protection while a physical device is locked remains a
separate device validation boundary.

## Resources and localization

Shared UI catalogs live in `Sources/MapleAppUI/Resources`; Audio, Dictation and
Settings also have catalogs in their feature directories. `Package.swift`
explicitly lists each processed catalog, so add new catalogs there as well as
their source files. System permission and app name translations live in
`Resources/InfoPlist.xcstrings`. UI package code uses its resource bundle.
Keep complete localized messages and translator context; format dates, numbers
and plurals for the active locale. Verify expanded/RTL layouts and dynamic text
without translating user content. Catalog presence and compilation do not prove
export/import, layout, selection, or accessibility behavior.

Library and Settings helper keys are fixed app-owned strings maintained in
their feature catalogs. Keep these catalog entries marked with manual
extraction state: the helper intentionally creates its localized key only at
the explicit feature-table lookup. A LocalizedStringKey parameter or property
can make Xcode extract a literal into the default table before the helper
selects its feature table. Do not use these helpers for user content or
interpolated messages; use an explicit table-aware localized message for
interpolation and plurals. When adding a helper key, add its complete English
entry and translator context to the matching catalog.

Export and import localizations in a disposable copy first. Review catalog
changes for stale required entries, new duplicate default-table keys and
preserved translator comments, then compile and inspect the imported language.
Record the configuration actually used by Xcode extraction; export success
does not establish the app's runtime endpoint profile.

The build embeds existing native Array and Manrope assets directly from their
tracked repository locations. Array is used for the New Chat hero and Manrope
for UI/chat. Geist Mono Regular is bundled unchanged for code and metadata;
its source, version and file hash are recorded in `Resources/FontLicenses/GeistMono-PROVENANCE.json`.
Chat font choices and system script fallback remain available. The native composer
retains UIKit’s preferred body font for system text accessibility. Font files are not
converted or downloaded at runtime. License notices are bundled under `FontLicenses`.

The shared brand styles use Maple 500 for primary fills and Neutral 900 for
readable labels. Small controls use Maple 900 in light mode and Maple 500 in dark
mode. Neutral and Pebble surfaces keep accent use limited to actions and focus.
Primary actions use capsules, a 44-point minimum target and a 200ms press animation;
Reduce Motion disables the scale animation. Semantic errors retain their contrast
adaptation independently of the brand accent.

Software license notices and Textual provenance are copied into the UI resource
bundle under `ThirdPartyNotices`. Keep the verbatim notices, source revisions
and hashes in step with dependency changes. KaTeX and math-font notices remain
in their existing resource directories, and font notices remain in
`FontLicenses`. Inspect the produced app bundle to verify those resources are
present; source files alone do not establish packaging.

The app uses a single resizable iPad window. Mac Catalyst, Designed for iPad
on Mac, and visionOS compatibility are disabled in the project. Final App
Store availability, production identifiers, associated domains, entitlements,
privacy declarations and release ownership require a separate review.
