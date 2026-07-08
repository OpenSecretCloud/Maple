# Goose Direct SDK Experiment Prep

This workspace is for a direct Goose integration experiment that does not route
Maple's built-in Agent Mode through ACP. The existing `goose` workspace should
stay around for comparison against the ACP PoC.

No Maple runtime wiring has been started in this branch yet. This note records
the prep state and the direct-integration surface we found in upstream Goose.

## Workspace State

- Workspace: `/Users/admin/workspaces/goose-sdk`
- Maple worktree: `/Users/admin/workspaces/goose-sdk/maple`
- Maple branch: `codex-goose-sdk-maple`
- Goose submodule path: `ThirdParties/goose`
- Goose remote: `https://github.com/aaif-goose/goose.git`
- Goose commit: `ce53dd5526c6935e538e81e47614a9b30cd2a137`

The submodule intentionally points at upstream Goose, not the prior Maple Goose
fork. That keeps this experiment focused on the current Goose/GDK direction and
lets us identify which patches are still needed.

## What `goose-sdk` Currently Provides

Upstream Goose now has these SDK crates:

- `crates/goose-sdk`
- `crates/goose-sdk-types`

Current `goose-sdk` scope appears narrow:

- Default feature re-exports shared Goose/ACP custom wire types.
- `uniffi` feature exposes Python/Kotlin bindings.
- The UniFFI surface currently constructs declarative providers from JSON and
  streams provider completions.
- Tools are not exposed over the UniFFI provider stream path.
- The included ACP example still spawns `goose acp` as a child process.

That means `goose-sdk` is useful context, but it is not yet the full embedded
agent/session/tool runtime Maple wants.

## Direct Runtime Surface Available Today

The full in-process agent runtime is still in the parent `goose` crate:

- `goose::agents::Agent`
- `goose::agents::AgentConfig`
- `goose::agents::AgentEvent`
- `goose::agents::SessionConfig`
- `goose::session::SessionManager`
- `goose::config::permission::PermissionManager`
- `goose::providers::*`
- `goose::agents::platform_extensions::*`

The key direct agent call is:

```rust
agent.reply(user_message, session_config, cancel_token).await?
```

It returns a stream of `AgentEvent`:

- `Message(Message)`
- `Usage(ProviderUsage)`
- `MessageUsage { message_id, usage }`
- `McpNotification((request_id, ServerNotification))`
- `HistoryReplaced(Conversation)`

This is close to what Maple wants for a Tauri-backed event stream. Tauri can map
these events into a Maple-owned frontend event model rather than exposing Goose
types directly to TypeScript.

## Likely Experiment Shape

The clean experiment should add a Maple-owned Rust adapter boundary first:

```text
Maple frontend
  <=> Tauri commands + emits
    <=> Maple AgentRuntime trait
      <=> GooseDirectRuntime
        <=> goose::agents::Agent
```

The frontend should not depend on Goose, ACP, or provider-specific event shapes.
It should receive Maple events like:

- session created
- assistant message delta/message
- thinking update
- tool call started
- tool call updated
- tool call completed/failed
- permission requested
- usage updated
- history replaced
- run cancelled
- run failed

Suggested Tauri commands for the experiment:

- `agent_runtime_status`
- `agent_runtime_start`
- `agent_session_create`
- `agent_session_list`
- `agent_session_load`
- `agent_session_delete`
- `agent_prompt_send`
- `agent_prompt_cancel`
- `agent_permission_respond`

The first pass should stay desktop-only and behind the same platform guards as
the ACP PoC.

## Expected Direct Integration Steps

1. Add desktop-only Goose dependencies in `frontend/src-tauri/Cargo.toml`.
2. Initialize a Maple-owned Goose root under Maple's app config directory.
3. Start or ensure Maple's local proxy and local proxy API key.
4. Configure Goose provider/model state for the Maple proxy.
5. Build a `SessionManager` and `PermissionManager` rooted in Maple-owned dirs.
6. Build an `AgentConfig` and `Agent::with_config`.
7. Create/load Goose sessions through `SessionManager`.
8. Attach developer/platform tools or builtins directly.
9. Send user prompts through `Agent::reply`.
10. Convert `AgentEvent` and `MessageContent` into Maple events.
11. Store cancellation tokens per active prompt run.
12. Route permission requests back through Maple UI and into Goose confirmation.

## Open Questions Before Coding

### Provider Configuration

The current direct APIs still lean on Goose global config and provider registry
helpers in several places. We need to verify whether Maple can construct an
OpenAI-compatible provider for the local proxy without writing Goose global
config/secrets files.

If not, direct mode will still need the same host-owned config improvement we
identified in the embedded readiness feedback.

### Built-In Developer Tools

Goose has in-process platform extension support and developer tools appear to
support direct working-directory context. We need to decide whether the direct
experiment should:

- add the developer tools as platform extensions directly, or
- use Goose's existing extension/builtin loading helpers.

The goal should be no external Goose binary and no `goose mcp developer`
subprocess for the built-in Maple path.

### Session Ownership

`SessionManager` gives direct create/list/load/delete/export/import methods.
That is a better fit for Maple than discovering state over ACP, but Maple still
needs a frontend-facing session model that hides Goose's storage format.

### Permission UX

Goose exposes permission manager and confirmation plumbing. The experiment needs
to prove that Maple can pause a tool call, emit a native permission request to
the frontend, and resume once the user allows or denies it.

### Event Mapping

`AgentEvent::Message` can contain mixed content: assistant text, thinking, tool
requests, tool responses, system notifications, action-required content, and
errors. The experiment should build a deterministic mapper from Goose messages
to Maple's timeline model before spending time on UI polish.

### Global State

Even with direct `Agent::with_config`, some Goose paths still use global config
or global state. The experiment should keep a list of each global touchpoint so
we can separate Maple-side cleanup from Goose-side GDK asks.

## Non-Goals For The Prep Phase

- No Tauri command wiring yet.
- No frontend changes yet.
- No provider config mutation yet.
- No Goose patches yet.
- No PR creation yet.

## Initial Read

Direct Goose still looks like the better first-party product path for Maple's
built-in Agent Mode, but the currently published `goose-sdk` crate is not the
full GDK described in the architecture direction. For a near-term experiment,
Maple likely needs to depend on the parent `goose` crate directly and wrap it
behind a Maple-owned runtime adapter. That keeps the frontend architecture valid
even if Goose later promotes the full runtime API into `goose-sdk` or a separate
GDK crate.
