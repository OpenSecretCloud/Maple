# Phase 2 implementation validation

Local validation on 2026-10-19 used macOS 27.0.1 / Apple Silicon and the
`agent-remote-p2` managed Local environment. The controller and execution daemon
had independent config/data roots, endpoint keys, proxy ports and debug bundle
identities. The installed production Agent, other development apps and Paseo
were not reconfigured. Test accounts and credentials remained inside the Local
MDE; no production OpenSecret account or backend was used. Live model calls used
the Local backend's configured provider.

## Observed behavior

- Two isolated daemons registered/discovered through the Local OpenSecret's real
  Transport V2 routes, then paired with CPace through public Iroh relays. IP
  transports were explicitly disabled. The focused transport test additionally
  asserted a relay path and pinned peer identities. A separate loopback UDP test
  covered a direct path; this is not evidence of real LAN/NAT traversal.
- Remote task lists matched the execution daemon. Account/password/whole-host
  controls were refused by the remote operation allowlist.
- GLM 5.3 Flash executed Bash on the execution daemon. The controller wrote its
  send request without reading the acknowledgement, then restarted during the
  turn. Recovery queried the execution daemon's original receipt and snapshot.
  The tool's proof file contained exactly one line: the prompt was not replayed.
- A wrong proof consumed the execution window even for a different admitted
  controller. A locally cancelled window could not be used. Disabling remote
  access refused control; re-enabling retained an existing grant. Signing the
  execution account out denied reconnect without signing out the controller.
  Same-account sign-in retained endpoint identity and pairing.
- The native controller opened the execution task, sent a real Bash turn and
  displayed the resulting text/tool summary. Local ACP loaded that same task,
  streamed a continuation, and both desktops displayed it. No ACP-over-Iroh
  adapter or Paseo configuration was involved.
- Remote desktop Stop terminated the Bash process and its recorded child.
  Closing the execution window preserved remote task access. Explicit Quit
  stopped its daemon. The isolated debug bundles preserve their public relay
  settings for a Dock/application relaunch.

Native testing found and corrected a desktop-startup request burst hitting the
initial remote capacity limit, initial host-selector theme/traffic-light spacing,
discovery publishing an empty address before the relay became ready, and the
Integrations settings pane retaining the controller backend after selecting a
remote task. Account/preferences remain local while Integrations uses the
selected execution backend. A disabled execution-only MCP fixture was visible
in both the remote task menu and Integrations pane; the controller configuration
remained empty. Remote capacity remains bounded and separate from local controls, with reserved slots
for cancellation and question answers.

## Computer and project navigation follow-up

The top-level computer toolbar has been replaced by computer/folder groups in the
normal sidebar and project menus. Remote setup lives in Settings. Three isolated
stores on this Mac (local plus two remote execution computers) used the same
folder paths and completed separate GLM Flash tasks through public Iroh relays.
The native desktop showed their independent titles and groups; switching between
computers preserved an unsent composer. Renaming a computer updated its local
label. Taking the third execution daemon offline retained its cached task and
folder rows; restoring the endpoint recovered its conversation without resending
the prompt. This is still same-machine development evidence.

Navigation regressions cover identical task IDs and folder paths on different
computers, keyboard destinations, search across computer/project names, summary
updates without transcript loading, a thousand streamed chunks during catalog
recovery, background unread state and superseded navigation during startup.
The full component checks below were rerun after this navigation change.

## Independent request streams

The pre-review audit found that reserved request slots did not prevent a large
transfer from blocking control messages on the original single ordered stream.
Wire version 2 uses one bounded QUIC stream per request/reply and a separate
ordered event stream, retaining the existing authentication, receipts and recovery.

The new transport tests use actual Iroh endpoints with the production flow-control
configuration. They leave an upload incomplete and an image reply larger than its
receive window unread, fill ordinary admission capacity, and verify that Stop and
question answers still complete. Finishing the upload admits the original send
once. Additional coverage closes an incomplete transfer before admission, rejects
oversized urgent bodies before transmission, and refuses a mismatched reply ID.
These are transport tests, not evidence of latency across a real slow network.

A fresh isolated headless build of this correction also repeated the Local
registration, pairing and control flow with IP transports disabled. GLM 5.3 Flash
completed its tool turn after the controller restarted with an unread send
acknowledgement; the original receipt and exactly one proof-file write were
verified. Revocation refused reconnect. All test-owned daemons were stopped, and
the user's existing desktop builds remained untouched.

## Automated checks

- Full Agent `just ci`: all four Clippy configurations, workspace build/tests and
  headless tests. Runtime: 271 passed / 2 ignored; desktop app: 494 passed /
  1 ignored; headless app: 86 passed / 1 ignored. The ignored app test contacts
  public relays and is run explicitly for this validation, not silently counted
  as ordinary offline CI coverage. Existing Pi/runtime workspace suites pass too.
- OpenSecret all-feature Clippy and unit tests: 817 passed / 32 ignored.
  Disposable PostgreSQL checks explicitly ran 21 AEAD tamper tests, 2 OAuth and
  8 callback binding tests, including valid foreign-row/lookup-column swaps,
  repeated-window expiry and relay key-lease rejection. Migration down/up passed.
- Rust SDK format, all-feature Clippy, 119 unit tests / 1 ignored, and documentation
  with warnings denied passed. Client/service registration fixture bytes match.
- Root and Agent Nix flake checks/evaluation passed on the current platform.
- The backend's configured advisory and incident-ban checks passed. Its policy
  does not configure a license allowlist; an exploratory unrestricted
  `cargo deny check` rejects ordinary MIT/Apache dependencies for that reason.
  No security policy was changed to suppress a finding.
- All eleven tests/vectors at the pinned upstream CPace revision passed. See
  [qualification and evidence limits](remote-security.md); this is not a claim
  that the complete Maple implementation received an external security audit.

At the audit baseline `3bd0c4a3`, GitHub Actions ran actual Linux, macOS and Windows
Agent tests, unsigned Dev/Prod macOS packaging and downloaded Linux artifact
verification successfully. Signed macOS jobs intentionally skipped. Those results
do not validate subsequent transport edits; those have the local checks above
and require their own hosted CI results.

## Open implementation findings

These remain review/rollout work, rather than newly accepted design exceptions:

- Disabling incoming remote access closes control connections but leaves an
  initialized networking endpoint alive until account/daemon teardown. Closing
  unused networking must preserve any outgoing controller role.
- User/project deletion cascades to enrollment records; destructive password
  reset does not yet remove them. Expired device/admission rows are removed on
  that account's later writes, while its latest window tombstones are retained.
  Reset cleanup and idle-record retention need resolution before hosted rollout.
- Enrollment has row caps, leases and payload bounds, but no dedicated
  publish/list/admission request-rate enforcement yet. Row caps do not bound
  request traffic.

Cancel/disable is authoritative on the device even if discovery briefly retains
stale pairing readiness until refresh/expiry. That is a UI freshness limitation;
it does not permit another code attempt or grant control.

## Remaining trial/release evidence

Public relay validation does not exercise the prepared Maple relay admission
callback in a deployed relay. The callback has unit/disposable-DB coverage; actual
Maple Dev relay/TLS/credential configuration, DNS and enclave deployment are
separate authorized rollout work. No service or production release was deployed.

Windows/Linux interactive status-tray behavior still needs platform testing;
builds and automated tests are covered by the baseline CI above. Two physical machines behind independent networks,
sleep/wake recovery, and signed macOS permissions/CUA remain distinct trial gates.
This same-machine debug exercise does not establish those results. A VM bridged
address did not establish the direct test path; the explicit loopback and forced
public-relay paths are the evidence reported above.
