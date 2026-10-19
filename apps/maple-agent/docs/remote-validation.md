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
initial remote capacity limit, missing host-selector theme/traffic-light spacing,
discovery publishing an empty address before the relay became ready, and the
Integrations settings pane retaining the controller backend after selecting a
remote task. Account/preferences remain local while Integrations uses the
selected execution backend. A disabled execution-only MCP fixture was visible
in both the remote task menu and Integrations pane; the controller configuration
remained empty. Remote capacity remains bounded and separate from local controls, with reserved slots
for cancellation and question answers.

## Automated checks

- Full Agent `just ci`: all four Clippy configurations, workspace build/tests and
  headless tests. Runtime: 271 passed / 2 ignored; desktop app: 478 passed /
  1 ignored; headless app: 82 passed / 1 ignored. The ignored app test contacts
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

## Remaining trial/release evidence

Public relay validation does not exercise the prepared Maple relay admission
callback in a deployed relay. The callback has unit/disposable-DB coverage; actual
Maple Dev relay/TLS/credential configuration, DNS and enclave deployment are
separate authorized rollout work. No service or production release was deployed.

Windows/Linux native builds, status-tray interaction and named-pipe behavior must
be checked on those platforms. Two physical machines behind independent networks,
sleep/wake recovery, and signed macOS permissions/CUA remain distinct trial gates.
This same-machine debug exercise does not establish those results. A VM bridged
address did not establish the direct test path; the explicit loopback and forced
public-relay paths are the evidence reported above.
