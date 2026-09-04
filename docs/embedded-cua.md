# Embedded CUA developer preview

Maple can expose the Cua Driver tool catalog to its embedded Goose agent
without launching a Cua Driver daemon or an MCP child process. The macOS build
pins Cua's `cua-driver-sdk` source at an immutable commit directly on top of
release `0.23.2` and creates the native runtime inside the Maple process. The
source delta fixes Cua's macOS SDK link declarations for Xcode 26.5 and routes
self-process window restoration through AppKit's main queue. The latter avoids
an upstream `invoke_menu` crash that only exists when CUA is embedded inside
the application whose window it restores.

This is an explicitly enabled developer preview. It is not an installer, a
general integration marketplace, or a sandbox for computer-use actions.

## Architecture

There is one lazily-created CUA runtime for the Maple process. Each enabled
desktop task receives its own standard-mode trusted CUA session and a native
Goose MCP client. Its public and transport identities are stable, opaque hashes
of the account scope and Maple task ID, so replacing the ephemeral client at a
run boundary reconnects to the same isolated lifecycle without colliding across
accounts. Maple starts or revives that lifecycle before exposing tools; the
model cannot name, end, or enumerate sessions. The client uses the SDK's
canonical `list_tools_json` and `call_tool` surfaces, so Maple does not maintain
a second copy of CUA's tool definitions or run an in-process network protocol.

Goose still owns tool discovery, namespacing, dispatch, and Maple's normal
approval routing. Maple marks every CUA operation as sensitive for approval
purposes, including screenshots and accessibility reads. The user's existing
Maple permission mode therefore remains the place to choose between per-call
approval and allow-all behavior.

CUA's action schemas and screenshot defaults remain canonical. Maple adapts the
general-purpose catalog only at the bound-session boundary: it hides CUA's six
lifecycle tools and removes caller-controlled `session` and reserved arguments
before the SDK injects Maple's trusted identity. Embedded-specific instructions
tell the model to use this tool surface directly, never mix CLI or other MCP
tokens into it, observe before acting, and treat controlled-app content as
untrusted data.

Every primary model receives the original accessibility text and a bounded,
image-free projection of CUA's structured grounding fields. This is necessary
because Goose's current OpenAI formatter otherwise preserves that field without
showing its exact window IDs, element tokens, coordinate frames, and refusal
details to the primary model. Vision-capable models also retain the canonical
raw image blocks. For a text-only primary model, Maple instead sends each
returned screenshot through the same fixed, tool-free Gemma perception helper
used by `read_image`, removes the raw image blocks, and appends a factual
CUA-specific description. The original non-image content, structured metadata,
protocol metadata, and error state remain intact. The helper is instructed to
report controls, state, layout, coordinate-space-aware approximate positions,
and discrepancies from the accessibility tree without selecting actions or
inventing element identifiers. Helper usage is recorded outside the primary
context ledger.

The native client is ephemeral. Goose must not serialize a Rust object as an
ordinary MCP transport, so its extension snapshot excludes the client. Maple
stores only versioned logical task metadata: whether CUA is enabled and whether
that task uses the embedded or legacy external backend. On cold load or a new
turn Maple recreates its trusted connection and rejoins the stable account/task
CUA lifecycle.

Embedded CUA is attached only while a user task is controlled by the desktop
app. ACP and other headless surfaces do not inherit the host's interactive
desktop authority.

## Setup and existing installations

Settings > Integrations reports Maple's own Accessibility and Screen Recording
status. **Set up CUA** invokes Cua's in-process macOS permission helpers on the
UI thread only after a direct user action. If macOS does not raise a prompt,
Maple opens the next missing Privacy & Security pane, Accessibility first and
then Screen Recording. Those grants belong to Maple's app identity;
permissions previously granted to `CuaDriver.app`, Codex, a terminal, or any
other host do not transfer.

Maple continues to detect a compatible standalone `CuaDriver.app` for migration
from the first integrations preview. Existing tasks and version-1 device
settings retain that external backend. A successful explicit setup switches
the default for future tasks to embedded CUA without rewriting historical task
snapshots. Maple never installs, updates, launches, or reconfigures the
standalone driver.

Enabling the card changes the default for newly-created tasks. Existing tasks
retain an independent switch in the composer. Changing tools while a task is
running or leased to another agent surface remains disallowed.

## macOS development bundle

Raw command-line binaries do not provide the stable application identity that
macOS privacy controls require. On macOS, stage the debug executable in the
repository's fixed development bundle with:

```sh
just debug-app
```

The command prints the exact `.app` path. Launch that bundle with `open` and
keep the same path and bundle identifier when validating permission changes.
The final Maple binary carries an app-relative `Contents/Frameworks` lookup
path. The staging script uses Xcode's `swift-stdlib-tool` to scan that binary,
embeds the complete set of required Swift compatibility libraries, signs the
nested libraries, and then seals the complete bundle. This keeps Swift runtime
packaging correct when native dependencies change instead of maintaining a
hard-coded library list.

By default the script uses a local ad hoc signature, so it needs no Developer
ID and does not produce a release artifact. Because rebuilding changes the ad
hoc identity, macOS may require those grants again after the executable
changes. Developers with an Apple Development identity in their keychain can
set `MAPLE_DEBUG_CODESIGN_IDENTITY` to its name or SHA-1 hash for a more stable
development identity. Release signing, hardened runtime, and notarization
remain separate distribution concerns.

## Preview limits

- macOS is the first embedded platform; other platforms keep the integration
  hidden until their packaging and permission UX are implemented.
- CUA's Rust packages are not published to crates.io and its portable tool
  contract is still marked experimental, so Maple pins an exact source commit.
  The pin currently includes narrow Xcode 26.5 linker and embedded-host
  main-thread fixes that should move back to an upstream release once
  available.
- Direct embedding is not process isolation. A CUA crash or native defect can
  affect Maple, and Maple approval does not provide OS-level containment.
- The CUA cursor overlay needs a main-thread AppKit host adapter and is not
  wired in this preview.
- Cancellation is best effort after an operating-system input event has been
  delivered; an already-completed external side effect cannot be rolled back.
- Text-only models receive mediated visual perception rather than native
  vision. This preserves CUA's visual fallback for canvas and weak-accessibility
  applications, but it adds one bounded helper request per returned screenshot
  and may be less precise than a vision-capable primary model on difficult
  pixel-only tasks. Models can still request CUA's tree-only fast path when a
  screenshot is unnecessary.
- Production distribution still needs a signed/notarized `.app` pipeline with
  a stable production bundle identifier and the same privacy usage strings.
