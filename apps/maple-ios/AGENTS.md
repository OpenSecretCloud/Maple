# Maple native iOS

Read the monorepo [guide](../../AGENTS.md) first. This component is the native
SwiftUI application for iPhone and iPad, with iOS 26 as its minimum. The
development bundle and callback scheme are `cloud.opensecret.maple.native.dev`.
The icon name is `Maple Native Dev`. Preserve its separate Keychain service and
app container; do not reuse Research identities or import another app's state.
Production identity, provisioning, distribution and uploads are separate work.

## Ownership

- `App/` is the thin iOS application entry point.
- `Sources/MapleAppCore/` owns testable account workflows and injected service,
  persistence, clock and identifier boundaries. It must not depend on SwiftUI.
- `Sources/MapleAppUI/` owns SwiftUI presentation and narrowly justified UIKit
  adapters. Guard iOS-only files so the core package can be tested on macOS.
- `Tests/MapleAppCoreTests/` tests behavior and ownership transitions.
- `Maple.xcodeproj/` is checked-in source, not generated output. It uses the
  local Swift package in this directory; that package links `../../sdk/swift`.
- `Config/`, `Resources/` and `scripts/` own application identity, explicit
  build profiles, bundled assets and repeatable unsigned builds.

Use the Swift SDK for all OpenSecret requests. Do not duplicate transport,
attestation, credential renewal or server authorization in the app. Billing
and flags are independent HTTP API boundaries; never depend on their server
source trees. Account identity and a revocable generation must fence every
asynchronous result and private storage write, including the same account
signing in again.

Use Observation and Swift concurrency, with main-actor UI models and one owner
for each mutable state. Ongoing chat, queue and playback tasks belong to the
account session independently of navigation. Keep CPU-heavy work off the UI
actor. Local drafts and queue storage require Complete Protection, excluded
backups and account cleanup; microphone audio remains in memory.

## Configuration and privacy

Preserve externally managed environment files. `scripts/configure-local.py`
reads only three allowlisted public endpoint values from an explicit env file,
plus explicit public client ID and auth origin arguments. It writes the
ignored `Config/Local.generated.xcconfig`; it never reads account credentials
or sources the env file. Local endpoints must be explicit HTTP loopback
origins. The PR profile is fixed and does not include that local file.

Keep credentials in the SDK secure store. Never log tokens, raw deep links,
private messages, decrypted responses, raw audio or credential-bearing URLs.
Treat renderer input, files, URL callbacks and persisted data as untrusted.
Use `$review-maple-security` for changes to these boundaries and
`$validate-maple` for simulator/application evidence.

Reuse the existing repository font files through the declared build inputs.
Do not convert or modify Array, or add another raw Array copy to this public
component. Keep font notices in the bundled `FontLicenses` directory.

## Checks

Run from the Maple root in the pinned `.#apple` shell with host Xcode 26.5:

```sh
nix develop --no-update-lock-file .#apple -c apps/maple-ios/scripts/build.sh unit
nix develop --no-update-lock-file .#apple -c apps/maple-ios/scripts/build.sh build pr
```

Use `build local` for a deliberately configured loopback stack. `build release`
is an optimized unsigned development artifact using those local inputs; it is
not a production or store lane. Neither build command launches the app.

For simulator installation and UI tests, use `runtime-build local` or
`test-simulator local` with an explicit `MAPLE_IOS_SIMULATOR_ID`, as documented
in the [README](README.md#build-and-test). These modes use Xcode's ad-hoc
simulator identity for Keychain access and debug package compilation; they need
no developer team, certificate or provisioning profile. Keep the unsigned PR
compile lane separate from runtime evidence.

The pinned Textual runtime in `Vendor/textual` has documented Copy export
patches and per-file provenance. Follow [its patch notes](Vendor/textual/MAPLE-PATCH.md)
when changing it; do not reformat the vendored runtime or patch SwiftPM checkouts.

For a runtime claim, record the exact bundle path, identifier and compiled
public endpoint profile, simulator UDID/runtime, test scope and observed
behavior. Test on iPhone and a resizable single iPad window. Complete the
simulator portion before handing real device boundaries off separately. Do
not install global substitute tools, use production signing, upload, deploy or repurpose the
existing Research release lanes during component development.

Keep app-owned strings in catalogs with context, including accessibility and
permission strings. English is the launch language; test expanded strings,
RTL, Dynamic Type, VoiceOver semantics and multilingual fallback. No language
picker, telemetry, purchasing, new guest signup or Mac app target belongs here.
