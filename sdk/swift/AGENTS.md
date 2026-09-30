# Swift SDK contributor guide

Read the repository-root guide, `sdk/README.md`, this package's `README.md`,
and the `develop-opensecret-sdk` skill before changing the Swift SDK.

Keep this package independent of SwiftUI and application workflow state.
`API/` owns typed Research-used contracts; `Transport/` owns Transport V2
framing, cryptography and native handoff; `Credentials/` owns atomic durable
credential authority; `Trust/` owns Nitro and PCR policy. Compare changed
contracts with both `sdk/src/lib` and `sdk/rust/src`, and inspect the in-tree
OpenSecret implementation when wire or authentication behavior changes.
Record intentional reference differences in task evidence.

Preserve exact HTTPS/loopback enforcement, pinned Nitro/PCR trust, handshake
key binding, non-contributory-key rejection, authenticated stream framing,
bounded recovery, and native handoff one-use ownership. Never turn malformed
attestation into a local mock or replace encrypted requests with raw HTTP.
Keep errors and logs free of credential-bearing URLs, tokens, session keys,
raw attestation, prompts, replies, audio and decrypted responses.

`MapleClient` instances using `KeychainSecureStore` share a process-wide
`CredentialAuthority` keyed by Keychain service and canonical API origin.
Transports additionally key their sharing by exact endpoint and complete PCR
policy; refresh coalescing stays in the authority across those transports.
Preserve this registry and its synchronous construction lock. Other custom
stores intentionally create independent clients; callers must reuse one
client, or explicitly share an authority when constructing low-level
transports over the same credentials. No process-local actor coordinates
separate app processes or extensions. Preserve revision compare-and-swap,
generation fencing, atomic access/refresh pairs, principal consistency and
fail-closed persistence. Cancellation does not replace ownership checks.
Device-only Keychain storage has no plaintext fallback; physical-device
behavior remains a separate check.

Use Xcode 26.5 and the root `.#apple` shell. The checked-in `.swift-format`
configuration and Xcode's formatter are the source of formatting policy.
Run `scripts/ci/sdk-swift.sh unit`, `build` and `ios-build` from the root; use focused
Swift tests while iterating. Run `integration` against an explicitly
configured disposable loopback service for affected API/protocol changes,
and `live-attestation` for real development-enclave trust proof when that
environment is authorized. Never run the CI-only backend bootstrap against
a developer-managed workspace. Never represent mocks or compilation as live
Nitro, hosted OAuth, native Apple acquisition or physical-device evidence.

Changing the workflow, classifier or CI scripts also requires their Python
regression tests and root `nix flake check --no-update-lock-file`. The SDK's
lane and hook stay independent of Research packaging. New tests belong to
the relevant boundary and should exercise failure, cancellation or ownership
behavior rather than only encoding the implementation's output.

Keep the Swift version in `VERSION` independent of app versions. Local
packages are supported during development. Package publication, checkpoint
tags, remote distribution, signing and app releases require their own
authorization; ordinary checks create none of them. Keep public guidance
self-contained and keep operational context and live captures outside this
repository.
