# Goose Embedded Runtime Readiness Feedback

This note is feedback from Maple's Agent Mode proof of concept. Maple embeds
Goose inside the Tauri desktop process and uses Maple's local OpenAI-compatible
proxy as the model provider. The PoC now works, but the path is not clean enough
to ship without either carrying Goose-specific integration debt or getting a
more explicit embedded-runtime surface from Goose.

The scope here is intentionally narrow:

- This is about embedding Goose directly in Maple's Rust/Tauri desktop runtime.
- This is not a critique of ACP as a protocol or of remote ACP interoperability.
- This is not about Maple frontend UX.
- This does not cover broad provider compatibility. The provider-specific fixes
  Maple may need should stay Maple's responsibility unless they expose a concrete
  Goose parser/runtime bug.

## Current Maple Shape

The current PR embeds Goose by depending on a forked submodule:

- Submodule: `ThirdParties/goose`
- Rust dependency: `goose = { path = "../../ThirdParties/goose/crates/goose", default-features = false }`
- Goose runtime entrypoint used by Maple:
  - `goose::acp::server_factory::{AcpServer, AcpServerFactoryConfig}`
  - `goose::acp::transport::create_router`
  - `goose::agents::GoosePlatform`
  - `goose::config::Config::global()`

Maple starts Goose inside Tauri by:

1. Ensuring Maple's local proxy is running.
2. Creating a Maple-owned Goose root under Maple's app config directory.
3. Mutating process environment variables used by Goose.
4. Writing Goose provider/model/key settings through `Config::global()`.
5. Creating an `AcpServer`.
6. Wrapping Goose's router in a local Axum server.
7. Returning the local endpoint to the Maple frontend.

That is viable for a PoC, but the host app is reaching into internal runtime
details, global config, process environment, and a forked source checkout.

## P0: Stable Embeddable Runtime Crate

### Problem

Maple currently imports the parent `goose` crate from a Git submodule and calls
APIs that appear designed primarily for Goose's own desktop/server binaries.
That creates several shipping problems:

- Maple needs a fork/submodule instead of a normal published crate dependency.
- The integration depends on internal module layout staying stable.
- Maple's reproducible builds now depend on a nested source checkout.
- It is unclear which APIs Goose considers supported for external embedders.

### Current Workaround

Maple carries:

```toml
[target.'cfg(any(target_os = "macos", target_os = "windows", target_os = "linux"))'.dependencies]
goose = { path = "../../ThirdParties/goose/crates/goose", default-features = false }
```

### Requested Goose Improvement

Expose and support a small embeddable runtime crate, for example `goose-runtime`
or a GDK crate, with a semver-stable API for:

- Constructing a Goose runtime from host-owned config.
- Starting and stopping the runtime.
- Selecting built-in tool groups.
- Configuring provider/model settings.
- Setting data/config/state/log directories.
- Receiving structured lifecycle and diagnostics events.

This does not need to expose every Goose internal. The important line is that
Maple should be able to depend on a normal crate and avoid reaching through the
same entrypoints used by Goose's own binaries.

## P0: Host-Owned Runtime Configuration

### Problem

The current embedding path is too global. Maple has to configure Goose by
setting process environment variables, mutating `Config::global()`, and writing
Goose config/secrets files:

```rust
std::env::set_var("GOOSE_PATH_ROOT", goose_path_root);
std::env::set_var("GOOSE_DISABLE_KEYRING", "true");
std::env::remove_var("GOOSE_MAX_TOKENS");

let config = goose::config::Config::global();
goose::config::set_active_provider(config, "openai", model)?;
config.set_param("GOOSE_FAST_MODEL", model)?;
config.set_param("GOOSE_MODE", mode)?;
config.set_param("OPENAI_BASE_URL", format!("{maple_proxy_base_url}/v1"))?;
config.set_secret("OPENAI_API_KEY", &proxy_api_key)?;
```

This is risky for an embedded desktop app:

- Process-global environment is hard to test and hard to reason about.
- Two runtimes in one process would conflict.
- Host apps cannot cleanly separate user config from runtime config.
- Secrets flow through Goose's default config path even when the host already
  owns secret storage.
- The host has to know which global keys to delete, such as `GOOSE_MAX_TOKENS`,
  to avoid stale behavior.

### Current Workaround

Maple creates `config`, `data`, and `state` directories under its own app config
root, points `GOOSE_PATH_ROOT` there, disables keyring use, writes Goose config
values, and locks down file permissions after writing `config.yaml` and
`secrets.yaml`.

### Requested Goose Improvement

Provide a runtime config builder that does not require process-global mutation:

```rust
let runtime = GooseRuntime::builder()
    .path_root(goose_path_root)
    .config_dir(goose_config_dir)
    .data_dir(goose_data_dir)
    .state_dir(goose_state_dir)
    .disable_keyring(true)
    .provider(OpenAiCompatibleProvider {
        base_url: maple_proxy_base_url,
        api_key: SecretString::new(proxy_api_key),
        model,
        fast_model: Some(model),
    })
    .mode(mode)
    .builtins(["developer"])
    .build()?;
```

The exact names do not matter. The important requirement is that an embedded
host can configure Goose with normal Rust values, without environment variables
or global singleton config.

## P0: Upstream the Streaming Tool-Call Delta Fix

### Problem

The PoC exposed a concrete Goose parser bug in OpenAI-compatible streaming tool
calls. Some providers stream multiple `tool_calls` entries for the same index in
one chunk. One entry can carry the tool id/name while another same-index entry
carries only argument fragments.

Before the patch, Goose could drop the argument-only fragment. The remaining
arguments then looked truncated, and Goose surfaced a misleading parse failure
similar to:

```text
The model's response was truncated -- it hit the output token limit while generating this tool call.
```

The tool never executed. The model had actually produced a valid tool call, but
Goose's stream merger lost part of the argument JSON.

### Current Workaround

Maple's Goose submodule points at a fork containing:

- `ThirdParties/goose/crates/goose-provider-types/src/formats/openai.rs`
- commit: `1e03dbd Merge streaming tool call deltas by index`

The patch merges streaming tool-call deltas by `index`, preserving id, function
name, argument fragments, and extra metadata.

### Requested Goose Improvement

Upstream this parser behavior and keep a regression test covering:

- Multiple tool-call deltas with the same `index` in a single stream chunk.
- id/name arriving separately from arguments.
- Argument fragments being appended in arrival order.
- No false "output token limit" error when the merged argument JSON is complete.

This is not a broad provider-compatibility ask. It is a concrete streaming
parser correctness issue that directly affected embedding Goose with Maple's
local OpenAI-compatible proxy.

## P0: Embedding-Friendly Dependency and MSRV Boundary

### Problem

Embedding Goose changed Maple's desktop build surface. Two concrete issues came
up:

- Goose v1.41.0 declared `rmcp = "1.4"` in its workspace, but the embedded build
  did not compile against the current 1.x API without pinning `rmcp = "=1.4.0"`.
- Maple had to align desktop CI around Rust 1.91.1 after Goose entered the
  desktop dependency graph.

Those may be reasonable requirements, but embedders need them to be explicit and
stable. A desktop app with reproducible builds needs to know the exact supported
dependency and toolchain boundary before shipping.

### Current Workaround

Maple pins `rmcp` next to the Goose path dependency:

```toml
rmcp = { version = "=1.4.0", default-features = false }
```

Maple also limits Goose to desktop targets only, so web/iOS/Android do not pull
the embedded runtime.

### Requested Goose Improvement

For embedders, Goose should publish one of:

- A crate with dependency constraints that compile correctly without Goose's
  repository lockfile.
- A documented lockfile/vendor strategy for external hosts.
- A minimal embedded feature set that avoids pulling CLI/TUI/desktop-only
  surfaces when the host only needs the agent runtime.

Also document:

- Supported Rust MSRV.
- Supported desktop platforms.
- Required native libraries or build-time assumptions.
- Which Cargo features are intended for embedded host apps.

## P1: Runtime Lifecycle Handle

### Problem

Maple currently wraps Goose's router in its own Axum server task and implements
readiness, shutdown, and status tracking around it. That is manageable, but the
host app is inventing lifecycle semantics around a runtime it does not fully
own.

### Current Workaround

Maple:

- Finds an available local port.
- Creates a token.
- Builds a local URL.
- Spawns `axum::serve(listener, router)`.
- Polls `/status` until ready.
- Stores its own `shutdown_tx`.
- Tracks runtime status separately for the frontend.

### Requested Goose Improvement

Expose a typed embedded runtime handle:

```rust
let handle = runtime.start().await?;
handle.wait_until_ready().await?;
handle.status().await?;
handle.stop().await?;
handle.cancel_session(session_id).await?;
```

The host can still choose whether to expose ACP, a local socket, or a direct
Rust API. The embedded runtime should own lifecycle semantics and provide typed
errors when startup fails.

## P1: Host-Visible Diagnostics Hooks

### Problem

When the initial tool-call failures happened, Maple did not have enough
host-visible evidence from inside Goose to distinguish:

- A model output issue.
- A provider/proxy cap.
- A Goose stream parser bug.
- A malformed tool-call argument.

Maple eventually added proxy-side raw LLM logging and better error-body logging
to investigate. That was useful, but it is outside Goose. For an embedded
runtime, the host app needs diagnostics hooks near the point where Goose sends a
provider request, consumes the stream, parses tool calls, and decides whether to
execute a tool.

### Current Workaround

Maple writes several logs:

- Tauri app logs.
- Goose runtime logs.
- Maple proxy raw request/response logs.
- Maple Agent Mode session JSONL.

That gives enough evidence now, but it required Maple-specific logging around
Goose instead of a structured embedded diagnostics API.

### Requested Goose Improvement

Expose opt-in diagnostics hooks with redaction controls:

- Provider request metadata and redacted request bodies.
- Raw stream chunks before normalization, when enabled.
- Normalized stream events.
- Tool-call parse success/failure details.
- Tool name, tool id, arguments, and parser error context.
- Finish reason and upstream usage details when available.
- Retry attempts and retry reasons.

This should not require changing ACP. It can be a Rust callback, event channel,
or tracing layer attached to the embedded runtime.

## P1: Runtime Tool and Permission Policy Callback

### Problem

Maple can rely on Goose defaults for the PoC, but a shippable embedded desktop
agent needs a host-owned policy boundary. The host app should be able to decide
which tools are enabled and when to ask the user before running a tool.

Some of this can be represented over ACP, but the embedded Goose concern is
lower-level: Maple needs a Rust-side way to apply policy before Goose executes
tools.

### Current Workaround

Maple selects Goose's `developer` builtins and configures Goose mode. More
specific permission behavior remains Goose-owned.

### Requested Goose Improvement

Expose a host policy callback before tool execution:

```rust
runtime.set_tool_policy(|request| async move {
    match request.tool_name.as_str() {
        "shell" if request.is_destructive() => ToolDecision::AskUser,
        "text_editor" => ToolDecision::Allow,
        _ => ToolDecision::Default,
    }
});
```

The callback should include:

- Tool name and id.
- Parsed arguments.
- Working directory.
- Session id.
- Whether Goose believes the operation is read-only or mutating, if known.
- A way to allow, deny, or defer to a host UI prompt.

## P1: Documented Storage Ownership

### Problem

This is not asking Goose to solve ACP session sync. Maple may build its own
remote backup/sync layer later.

The embedded runtime issue is simpler: Maple needs to know what Goose stores
under `config`, `data`, and `state`, which files are stable, which are cache,
which contain secrets, and what can be safely deleted, migrated, exported, or
backed up.

### Current Workaround

Maple creates a Goose root under its own app config directory and separately
keeps Agent Mode session JSONL for the UI. Goose still owns its own config,
data, and state files beneath that root.

### Requested Goose Improvement

Document storage ownership for embedded hosts:

- Which directories Goose needs.
- Which files contain secrets.
- Which files are durable session state.
- Which files are cache/logs.
- How to safely delete a session.
- How to migrate or compact state across Goose versions.
- Whether a host can export/import a Goose session without relying on ACP.

If Goose wants embedders to treat the whole root as opaque, that is acceptable,
but it should be documented with backup/delete expectations.

## P1: First-Class OpenAI-Compatible Provider Config

### Problem

Maple's model path is intentionally simple: the desktop app ships a local
OpenAI-compatible proxy, and Goose should call that proxy. Today Maple gets
there by configuring Goose's `openai` provider name and writing
`OPENAI_BASE_URL` / `OPENAI_API_KEY`.

That works, but it is awkward for an embedded app because the host already owns
the local proxy lifecycle and credential generation.

### Current Workaround

Maple starts the proxy, creates/loads a local proxy API key, and writes that key
into Goose's config as `OPENAI_API_KEY`.

### Requested Goose Improvement

Allow embedded hosts to pass a provider object directly:

```rust
ProviderConfig::OpenAiCompatible {
    base_url,
    api_key,
    model,
    headers,
    timeout,
}
```

This should avoid requiring env-style key names or writing provider secrets to a
Goose secrets file when the host already has a secret manager.

## P2: Clearer Parser Error Wording

### Problem

The tool-call parser error said the model hit an output token limit. In the case
Maple debugged, that was not the real root cause. The root cause was lost
streamed argument fragments before JSON parsing.

### Requested Goose Improvement

When Goose reports tool-call truncation, include the evidence that led to that
conclusion:

- Upstream finish reason, if known.
- Whether the JSON was incomplete at end of stream.
- Number of received argument characters.
- Tool id/name/index.
- Last argument fragment.

If the upstream finish reason is not a length/token-limit reason, prefer wording
like "tool-call arguments ended before valid JSON was complete" instead of
asserting a token limit.

## P2: Minimal Embedded Example

### Problem

Most of the integration time was spent discovering the right entrypoint,
configuration keys, directory layout, and startup lifecycle.

### Requested Goose Improvement

Add a small embedded Rust example that:

- Starts Goose from another Rust process.
- Uses an OpenAI-compatible local endpoint.
- Sets model, mode, config/data/state dirs, and builtins.
- Installs a diagnostics hook.
- Starts and stops cleanly.

A Tauri-specific example would be useful, but a plain Rust example is enough if
the runtime API is explicit.

## Workarounds Maple Should Remove Before Shipping

These are the concrete PoC compromises Maple should not carry long term:

- Forked Goose submodule in `ThirdParties/goose`.
- Path dependency on `../../ThirdParties/goose/crates/goose`.
- Direct imports from Goose modules that are not clearly an embedded public API.
- `rmcp = "=1.4.0"` pin required by embedding outside Goose's lockfile.
- Process-global `GOOSE_PATH_ROOT`, `GOOSE_DISABLE_KEYRING`, and
  `GOOSE_MAX_TOKENS` mutation.
- Runtime writes to Goose `config.yaml` and `secrets.yaml` for provider config.
- Host-side deletion of stale Goose config keys.
- Host-wrapped Axum server lifecycle and readiness polling.
- Maple proxy-side raw LLM logging as the only way to inspect provider stream
  failures before Goose normalization.

## Suggested Upstream Patch Order

1. Upstream the OpenAI-compatible streaming tool-call merge fix and regression
   test.
2. Document the current best supported embedded entrypoint, even if it is not
   final GDK shape yet.
3. Add a host-owned runtime config builder that avoids env/global config.
4. Publish or stabilize a minimal embedded runtime crate/feature set.
5. Add diagnostics hooks for provider requests, raw stream chunks, parser
   failures, and tool execution decisions.
6. Document Goose storage directories and lifecycle expectations for embedded
   hosts.

## Success Criteria For Maple

Maple can seriously consider shipping embedded Goose when:

- Goose can be consumed as a normal crate without a forked submodule.
- Maple can configure Goose with Rust values instead of global env/config
  mutation.
- Maple can start, stop, and observe Goose through a typed runtime handle.
- The streaming tool-call parser fix is upstream.
- The desktop build footprint, MSRV, and features are documented and stable.
- Maple can attach host diagnostics and tool policy hooks without patching Goose.

At that point Maple can keep Goose as the built-in agent harness while still
leaving ACP support for external harnesses as a separate integration layer.
