# Agent Client Protocol Research

Research snapshot: 2026-07-08.

This is the running Maple ACP research document. The near-term product goal is
Maple as an ACP client that can talk to multiple ACP agent servers running on
the user's computer. The separate idea of embedding Goose into Maple as an ACP
server remains useful context, but this document focuses on client-side protocol
support and compatibility with external ACP agents.

## Executive Takeaways

- The current stable ACP wire protocol is `protocolVersion: 1`. The official
  repository also contains v2 schemas and docs, but the README still names v1 as
  stable. Maple should implement v1 first and keep the transport/message model
  versioned so v2 can be added without reshaping the product.
- Stdio JSON-RPC is the common denominator. ACP's stable transport model is a
  client-launched subprocess using newline-delimited UTF-8 JSON-RPC messages on
  stdin/stdout. Streamable HTTP exists as a draft and Goose has serious support
  for HTTP/WebSocket, but Maple should not require it for v1 compatibility.
- Capabilities drive correctness. Maple should not call optional methods or send
  optional content until the agent advertises support, and should advertise only
  client capabilities that the UI/runtime can actually honor.
- The three target servers are all practically v1 today:
  - Goose: v1 schema/runtime plus a large `_goose/unstable/*` app extension
    surface and draft HTTP/WebSocket transport.
  - OpenCode: v1 over stdio, clean implementation, config options for model,
    effort, and mode, plus unstable fork/model helpers.
  - claude-agent-acp: v1 over stdio, broad session lifecycle support, terminal
    auth, logout, additional directories, form/url elicitation adapters, and
    Claude-specific `_meta` pass-through.
- The common UX surface Maple should build first is: initialize/auth, session
  create/list/load/resume/close where supported, prompt/cancel, streaming
  transcript updates, tool-call rendering, permission prompts, config options,
  slash-command updates, image/resource prompt content, MCP server handoff, and
  extension metadata preservation.
- The implementation location question matters mostly for transport. A browser
  frontend cannot directly own stdio subprocesses, local filesystem mediation,
  or process lifetime. If Maple supports local ACP agents by stdio, the Tauri
  side is the natural process/transport boundary, with the frontend consuming a
  typed event/request API. A frontend-only path becomes plausible for agents
  exposed over local HTTP/WebSocket, but that should be additive.

## Source Inventory

Official protocol:

- Repository: https://github.com/agentclientprotocol/agent-client-protocol
- Local snapshot: `/Users/admin/repos/ThirdParties/agent-client-protocol`
- Snapshot commit: `dbe946b docs: update registry agents (#1628)`
- Key files:
  - `README.md`
  - `schema/v1/meta.json`
  - `schema/v1/meta.unstable.json`
  - `schema/v2/meta.json`
  - `schema/v2/meta.unstable.json`
  - `docs/protocol/v1/*.mdx`
  - `docs/protocol/v2/*.mdx`

Goose:

- Repository: https://github.com/aaif-goose/goose
- Local snapshot: `/Users/admin/repos/ThirdParties/goose-upstream`
- Snapshot commit: `6d69936 chore: Remove stale crates/goose-server and update docs (#10224)`
- Also present: `/Users/admin/repos/goose-v1.39.0` is a dirty existing checkout
  and was not modified.
- Key files:
  - `crates/goose/src/acp/server.rs`
  - `crates/goose/src/acp/server/dispatch.rs`
  - `crates/goose/src/acp/server/custom_dispatch.rs`
  - `crates/goose/src/acp/transport/mod.rs`
  - `crates/goose/src/acp/provider.rs`
  - `crates/goose/src/acp/response_builder.rs`
  - `crates/goose-sdk-types/src/custom_requests.rs`
  - `crates/goose-sdk-types/src/custom_notifications.rs`
  - `ui/desktop/src/acp/*`
  - `documentation/docs/guides/acp-clients.md`
  - `documentation/docs/guides/acp-providers.md`

OpenCode:

- Repository: https://github.com/anomalyco/opencode
- Local snapshot: `/Users/admin/repos/ThirdParties/opencode`
- Snapshot commit: `95013d293 fix(app): keep session routes within layouts (#35842)`
- Package version: `opencode` `1.17.15`
- ACP SDK dependency: `@agentclientprotocol/sdk` `0.21.0`
- Key files:
  - `packages/opencode/src/cli/cmd/acp.ts`
  - `packages/opencode/src/acp/service.ts`
  - `packages/opencode/src/acp/agent.ts`
  - `packages/opencode/src/acp/event.ts`
  - `packages/opencode/src/acp/permission.ts`
  - `packages/opencode/src/acp/config-option.ts`
  - `packages/opencode/src/acp/content.ts`
  - `packages/opencode/src/acp/tool.ts`
  - `packages/opencode/test/cli/acp/*`
  - `packages/opencode/test/acp/*`

Claude Agent ACP:

- Repository: https://github.com/agentclientprotocol/claude-agent-acp
- Local snapshot: `/Users/admin/repos/ThirdParties/claude-agent-acp`
- Snapshot commit: `32b9350 fix: Use SDK guards for elicitation validation (#850)`
- Package version: `@agentclientprotocol/claude-agent-acp` `0.57.0`
- ACP SDK dependency: `@agentclientprotocol/sdk` `1.2.1`
- Claude SDK dependency: `@anthropic-ai/claude-agent-sdk` `0.3.202`
- Key files:
  - `src/acp-agent.ts`
  - `src/tools.ts`
  - `src/elicitation.ts`
  - `src/settings.ts`
  - `src/tests/*`

Maple proof-of-concept context:

- PR: https://github.com/OpenSecretCloud/Maple/pull/606
- PR #606 is a useful proof that Maple can host/bridge Goose, but it should not
  be treated as the architecture for this v1 client-support work. The PR starts
  from a desktop Agent Mode route backed by ACP, then moves toward embedded
  Goose runtime startup from Tauri. Its own notes say Maple still talks ACP over
  a local WebSocket while Tauri starts Goose in-process. That is v2/embedding
  context, not the core interoperability target here.

## ACP Protocol State

### Versioning

ACP has artifact versions and wire protocol versions. The official README is
explicit that crate/schema artifact versions do not determine wire
compatibility. The negotiated `protocolVersion` in `initialize` does.

Current stable wire version: `1`.

Practical implication for Maple:

- Offer v1 first.
- Store negotiated protocol version per agent connection.
- Do not hard-code schema artifact/package versions as protocol semantics.
- Expect v2 shape changes later, especially prompt lifecycle and session state.

### Transport

Stable v1 transport:

- JSON-RPC 2.0 messages encoded as UTF-8.
- Client launches the agent as a subprocess.
- Client writes JSON-RPC requests/notifications/responses to agent stdin.
- Agent writes JSON-RPC to stdout.
- Messages are newline-delimited and must not include embedded newlines.
- Agent stderr is logging only.
- Agent stdout must contain only valid ACP messages.

Draft/additional transport:

- Streamable HTTP is documented as a draft/in-progress proposal.
- Goose implements a serious HTTP/WebSocket transport at `/acp` with CORS,
  optional token auth, `acp-connection-id`, and `acp-session-id` headers.
- Maple should treat HTTP/WebSocket as an optional transport adapter, not as the
  baseline ACP path.

### Stable v1 Method Surface

Stable v1 agent methods from `schema/v1/meta.json`:

- `initialize`
- `authenticate`
- `session/new`
- `session/load`
- `session/set_mode`
- `session/set_config_option`
- `session/prompt`
- `session/cancel`
- `session/list`
- `session/delete`
- `session/resume`
- `session/close`
- `logout`

Stable v1 client methods from `schema/v1/meta.json`:

- `session/request_permission`
- `session/update`
- `fs/write_text_file`
- `fs/read_text_file`
- `terminal/create`
- `terminal/output`
- `terminal/release`
- `terminal/wait_for_exit`
- `terminal/kill`

Protocol method:

- `$/cancel_request`

Not all listed methods are baseline. The baseline agent methods are
`initialize`, `authenticate`, `session/new`, and `session/prompt`; session
cancel/update are expected for prompt lifecycle. Optional methods must be gated
by capabilities.

### Stable v1 Capabilities

Client capabilities worth Maple considering:

- `fs.readTextFile`
- `fs.writeTextFile`
- `terminal`
- `session.configOptions.boolean`
- Custom `_meta` capabilities, only for extensions Maple can actually service.

Agent capabilities Maple must honor:

- `loadSession`: enables `session/load`.
- `promptCapabilities.image`: image prompt content.
- `promptCapabilities.audio`: audio prompt content.
- `promptCapabilities.embeddedContext`: embedded `resource` prompt content.
- `mcpCapabilities.http`: HTTP MCP server handoff.
- `mcpCapabilities.sse`: SSE MCP server handoff. SSE is deprecated in MCP but
  appears in current ACP agents.
- `auth.logout`: enables `logout`.
- `sessionCapabilities.delete`: enables `session/delete`.
- `sessionCapabilities.additionalDirectories`: enables additional workspace
  roots in session lifecycle requests.
- `sessionCapabilities.resume`: enables `session/resume`.
- `sessionCapabilities.close`: enables `session/close`.
- `sessionCapabilities.list`: enables `session/list`.

Baseline prompt content is text and resource links. Image, audio, and embedded
resources need prompt capability checks.

### v1 Session Lifecycle

`session/new`:

- Client sends absolute `cwd` and MCP servers.
- Agent returns `sessionId`, and may return modes/config options.

`session/load`:

- Requires `loadSession`.
- Restores a session and replays history using `session/update` notifications
  before the method response.
- Maple must handle replay interleaved with normal session update parsing.

`session/resume`:

- Requires `sessionCapabilities.resume`.
- Restores without replaying history.
- Response may include current mode/model/config option state.

`session/close`:

- Requires `sessionCapabilities.close`.
- Cancels active work and frees resources for an active session.

`session/list` and `session/delete`:

- Capability gated.
- `session/list` is paginated by cursor in the schema and returns `SessionInfo`.

Additional directories:

- Capability gated.
- Intended for additional workspace roots beyond the primary `cwd`.
- Maple should preserve the distinction between primary root and additional
  roots rather than flattening them into one project path.

### v1 Prompt Lifecycle

In v1, `session/prompt` lasts for the full turn. The agent streams progress via
`session/update` notifications and eventually responds to `session/prompt` with
a `stopReason`.

Stop reasons include:

- `end_turn`
- `max_tokens`
- `max_turn_requests`
- `refusal`
- `cancelled`

Maple must support:

- Agent/user/thought message chunks.
- Tool calls and tool call updates.
- Plan updates.
- Available slash-command updates.
- Mode/config option updates.
- Usage updates where agents emit them.
- Optional `messageId` in v1. Some agents send it reliably; clients should not
  require it in v1. v2 requires message IDs.

### Cancellation

There are two cancellation concepts:

- Feature-level `session/cancel`: client tells agent to cancel active session
  work. Pending permission requests must be answered with `cancelled`.
- Protocol-level `$/cancel_request`: either side may ask to cancel an in-flight
  JSON-RPC request. The callee may return a normal partial/cancellation result
  or JSON-RPC error `-32800`.

For Maple:

- Track pending prompt request, pending permission requests, and pending client
  fs/terminal requests by session.
- On user cancel, send `session/cancel`, mark local UI cancelling, and respond
  to all pending permission requests as `cancelled`.
- Continue accepting late `session/update` notifications until the prompt
  response/terminal idle state arrives.

### Permissions

The agent calls client `session/request_permission` with:

- `sessionId`
- `toolCall` context
- available options

The client responds with:

- `selected` plus `optionId`, or
- `cancelled`

Maple should never treat unknown/missing outcomes as approval. For v1, the
permission request's `toolCall` is a `ToolCallUpdate`; many agents emit the
referenced tool call before asking permission, but Maple should be prepared for
the permission request to carry the first visible tool-call details.

### Tool Calls

v1 tool updates:

- `tool_call` creates/surfaces a tool call.
- `tool_call_update` modifies it.
- `toolCallId` is the stable key within the session.
- `kind` values help UI icons and grouping: `read`, `edit`, `delete`, `move`,
  `search`, `execute`, `think`, `fetch`, `other`.
- `status` values: `pending`, `in_progress`, `completed`, `failed`.
- Content can include regular ACP content blocks and diffs.
- v1 diff content uses `path`, `oldText`, and `newText`.

Maple should model tool calls as keyed entities with patch/update semantics,
not as append-only transcript text.

### Session Config Options

Session config options are now the preferred v1 surface for session-level
settings. They replace/soft-deprecate legacy `modes` for many UIs.

Important details:

- Agents may return `configOptions` from session setup methods.
- Config option `id` is sent back as `configId` in `session/set_config_option`.
- `select` is baseline.
- `boolean` requires client capability `session.configOptions.boolean: {}`.
- Categories are semantic UI hints only:
  - `mode`
  - `model`
  - `model_config`
  - `thought_level`
- Unknown categories or option types must degrade gracefully.
- Option order is meaningful and should be preserved.

Common observed config IDs:

- `mode`
- `model`
- `effort`
- `thinking_effort`
- `provider`
- `agent`
- `fast`

Maple should build generic config-option rendering and only add agent-specific
polish where useful.

### Slash Commands

Agents can emit `available_commands_update` via `session/update`. Clients should
replace the session's cached command list with the payload. OpenCode, Goose, and
claude-agent-acp all use slash-command-style updates.

Sending a slash command is just a normal prompt whose text begins with `/`.
Agents decide whether to route it as a command or ordinary text.

### Extensibility

ACP extension mechanisms:

- `_meta` fields on requests/responses/updates.
- Custom methods prefixed with `_`.
- Custom capabilities in initialize.

Maple should:

- Preserve unknown `_meta` in stored traces where possible.
- Namescape any Maple-specific custom capability under `_meta.maple`.
- Add per-agent extension adapters only after the stable core works.
- Never require a custom extension for basic chat/tool/session workflows.

### v1 Unstable Surface

`schema/v1/meta.unstable.json` adds:

Agent methods:

- `providers/list`
- `providers/set`
- `providers/disable`
- `mcp/message`
- `session/fork`
- `nes/start`, `nes/suggest`, `nes/accept`, `nes/reject`, `nes/close`
- `document/didOpen`, `document/didChange`, `document/didClose`,
  `document/didSave`, `document/didFocus`

Client methods:

- `mcp/connect`
- `mcp/message`
- `mcp/disconnect`
- `elicitation/create`
- `elicitation/complete`

Observed use:

- OpenCode and claude-agent-acp both implement `session/fork`.
- Claude and Goose both care about elicitation-like flows.
- Goose has far more private `_goose/unstable/*` methods than the official
  unstable ACP surface.

### v2 Differences to Design Around

v2 is not the stable target yet, but it changes enough to influence Maple's
internal abstractions:

- Auth methods rename from `authenticate`/`logout` to `auth/login` and
  `auth/logout`.
- `session/list`, `session/resume`, and `session/close` are baseline under the
  v2 session capability surface.
- `session/load` goes away; `session/resume` can optionally replay from start.
- `session/prompt` returns as soon as the prompt is accepted. Turn completion is
  reported by `session/update` `state_update: idle` with `stopReason`.
- Message updates become explicit upserts keyed by required `messageId`.
- Tool call updates have clearer patch semantics and can stream content chunks.
- Stable v2 client methods are only `session/request_permission` and
  `session/update`; v1 client fs/terminal methods are not in v2 stable.
- Selected enum/tagged-union values can have custom/future fallbacks. `_` values
  are custom, unknown non-underscore values are future ACP variants.
- v2 follows JSON-RPC batch behavior. The docs advise care around batching
  lifecycle-sensitive messages.

Maple should therefore separate:

- transport envelope
- protocol version adapter
- session state machine
- transcript/tool-call/config-option rendering
- per-agent extension adapters

## Compatibility Matrix

| Surface | Goose | OpenCode | claude-agent-acp |
| --- | --- | --- | --- |
| Stable protocol | v1 schema/runtime in source | v1 | v1 |
| Main transport | stdio via `goose acp`; HTTP/WebSocket via `/acp` in `goose serve` | stdio via `opencode acp` | stdio via `claude-agent-acp` |
| Draft HTTP/WebSocket | Yes, strong support | Not for ACP external transport in current code | No, stdio wrapper |
| `initialize` | Echoes requested protocol version using v1 schema types; advertises `goose` info | Returns `protocolVersion: 1` | Returns `protocolVersion: 1` |
| Auth | `goose-provider` auth method; configure provider | `opencode-login`; optional terminal-auth metadata | Terminal auth methods, gateway auth, logout |
| `session/new` | Yes | Yes | Yes |
| `session/load` | Yes, `loadSession: true` | Yes, `loadSession: true` | Yes, `loadSession: true` |
| `session/resume` | Not found in server dispatch | Yes | Yes |
| `session/list` | Advertised | Advertised | Advertised |
| `session/close` | Advertised | Advertised | Advertised |
| `session/delete` | Private `_goose` delete/session management, not stable advertised | Not implemented/advertised | Advertised and implemented |
| `session/fork` | Handler exists; not stable advertised in observed initialize | Advertised/implemented as unstable | Advertised/implemented as unstable |
| Additional directories | Not advertised in observed initialize | Not observed | Advertised/implemented |
| Prompt images | Advertised | Advertised | Advertised |
| Embedded resources | Advertised | Advertised | Advertised |
| Audio prompt content | Explicit false | Not advertised | Not advertised |
| MCP servers | HTTP advertised; stdio handled; SSE conversion appears in tests | HTTP and SSE advertised; stdio/remote mapped | HTTP and SSE advertised; stdio/http/sse mapped |
| Client fs | Reads client fs caps; public docs mention file ops | Uses client `writeTextFile` for proposed edits if available | Mostly agent/SDK side; additional dirs important |
| Client terminal methods | Reads client terminal cap; public docs mention terminal support | No direct ACP terminal method support observed | Uses terminal-like `_meta`, not standard terminal methods |
| Config options | Provider, mode, model, thinking effort | Model, effort, mode | Mode, model, effort, fast, agent |
| Boolean config | Unknown in server; desktop client advertises elicitation only | No boolean options observed | Yes for `fast` when client advertises support; select fallback otherwise |
| Slash commands | Yes | Yes | Yes |
| Elicitation | Goose desktop advertises form; server supports form elicitation | Not observed | Form/url unstable client methods, heavily used |
| Private extensions | Large `_goose/unstable/*` namespace | Small `_meta["terminal-auth"]`; unstable fork/model | `_meta.claudeCode`, gateway auth, terminal output metadata |

## Goose Details

### Server Capabilities

In `crates/goose/src/acp/server.rs`, Goose initialize:

- Stores client fs and terminal capabilities.
- Reads `_meta.goose` custom client capabilities.
- Reads form elicitation support.
- Advertises:
  - `loadSession: true`
  - session list and close capabilities
  - prompt image true
  - prompt audio false
  - embedded context true
  - MCP HTTP true
  - agent info name `goose`
  - auth method `goose-provider` titled `Configure Provider`

The dispatch layer handles:

- `initialize`
- `authenticate`
- `session/new`
- `session/load`
- `session/prompt`
- `session/cancel`
- `session/set_config_option`
- `session/set_mode`
- `session/list`
- `session/close`
- `session/fork`
- otherwise custom requests

Observed mismatch: `session/fork` has a handler but was not advertised in the
initialization capability builder I inspected. Maple should use capabilities as
truth and treat unadvertised methods as unavailable unless a Goose-specific
adapter intentionally probes them.

### Goose Transports

Goose supports stdio for ACP and also a standalone ACP HTTP/WebSocket router:

- ACP path: `/acp`
- Methods: POST/GET/DELETE through the upstream ACP HTTP server.
- Headers: `acp-connection-id`, `acp-session-id`
- CORS: loopback/file-origin aware, with optional exact origins.
- Optional token auth on ACP routes.
- Auxiliary routes include `/health`, `/status`, and MCP app proxy routes.

For Maple v1 external-agent support, stdio is still the baseline. Goose HTTP is
a useful optional transport for desktop/webview setups where a local service is
already running.

### Goose as an ACP Client/Provider

`crates/goose/src/acp/provider.rs` maps other ACP agents into Goose providers.
This is relevant because it proves a client implementation has to be generic:

- Goose can launch an ACP subprocess provider.
- It creates an ACP session eagerly during provider connect.
- It applies ACP config options such as mode/model.
- It translates ACP tool calls, permission requests, message chunks, usage, and
  context window updates into Goose provider concepts.

Goose provider configs include:

- command, args, env, env_remove
- work_dir
- MCP servers
- session mode mapping
- session config option values
- model config option id

### Goose Config Options

Goose builds config options for:

- `provider`
- `mode`
- `model`
- `thinking_effort`

Mode options seen in tests:

- `auto`
- `approve`
- `smart_approve`
- `chat`

### Goose Slash Commands

Goose sends `available_commands_update` from its slash-command registry. Command
metadata includes command type (`Builtin`, `Recipe`, `Skill`) and optional source
path.

### Goose Custom Notifications

Goose defines `_goose/unstable/session/update`, a parallel notification to
standard ACP `session/update`, with variants:

- `usage_update`
- `status_message`
- `message_usage`

The server still emits standard ACP usage updates for backward compatibility
while known clients migrate to `_goose/unstable/session/update`.

Goose also defines `_goose/unstable/session/recipe/request-params`, used by the
desktop app when recipes require parameter input.

### Goose Custom Methods

Goose has a broad private app-control namespace. These are not required for
basic ACP chat, but Maple may encounter them if it tries to reproduce Goose
Desktop behavior.

Representative `_goose/unstable/*` groups:

- Session extensions:
  - `_goose/unstable/session/extensions/add`
  - `_goose/unstable/session/extensions/remove`
  - `_goose/unstable/session/extensions/list`
- Tool/resource/app operations:
  - `_goose/unstable/tools/list`
  - `_goose/unstable/tools/permissions/set`
  - `_goose/unstable/tools/call`
  - `_goose/unstable/resources/read`
  - `_goose/unstable/apps/list`
  - `_goose/unstable/apps/export`
  - `_goose/unstable/apps/import`
- Session control:
  - `_goose/unstable/session/working-dir/update`
  - `_goose/unstable/session/system-prompt/set`
  - `_goose/unstable/session/steer`
  - `_goose/unstable/session/info`
  - `_goose/unstable/session/conversation/truncate`
  - `_goose/unstable/session/project/update`
  - `_goose/unstable/session/rename`
  - `_goose/unstable/session/archive`
  - `_goose/unstable/session/unarchive`
  - `_goose/unstable/session/export`
  - `_goose/unstable/session/import`
  - `_goose/unstable/session/share/nostr`
- Provider/config:
  - `_goose/unstable/providers/list`
  - `_goose/unstable/providers/supported-models/list`
  - `_goose/unstable/providers/catalog/list`
  - `_goose/unstable/providers/setup/catalog/list`
  - `_goose/unstable/providers/catalog/template`
  - `_goose/unstable/providers/custom/create/read/update/delete`
  - `_goose/unstable/providers/inventory/refresh`
  - `_goose/unstable/providers/config/read/status/save/delete/authenticate`
  - `_goose/unstable/providers/secrets/list/delete`
  - `_goose/unstable/providers/canonical-model-info`
  - `_goose/unstable/config/read/upsert/remove/read-all`
  - `_goose/unstable/defaults/read/save/clear`
  - `_goose/unstable/preferences/read/save/remove`
- Prompts/extensions/onboarding:
  - `_goose/unstable/config/prompts/list/get/save/reset`
  - `_goose/unstable/config/extensions/list/add/remove/set-enabled`
  - `_goose/unstable/extensions/available`
  - `_goose/unstable/onboarding/import/scan`
  - `_goose/unstable/onboarding/import/apply`
- Recipes and schedules:
  - `_goose/unstable/recipes/encode/decode/scan/list/save/parse/delete`
  - `_goose/unstable/recipes/schedule`
  - `_goose/unstable/recipes/slash-command`
  - `_goose/unstable/recipes/to-yaml`
  - `_goose/unstable/schedules/list/create/delete/update/run-now`
  - `_goose/unstable/schedules/sessions/list`
  - `_goose/unstable/schedules/pause/unpause`
  - `_goose/unstable/schedules/running-job/kill/inspect`
- Sources, dictation, local inference:
  - `_goose/unstable/sources/create/list/update/delete/export/import`
  - `_goose/unstable/dictation/*`
  - `_goose/unstable/local-inference/*`
  - `_goose/unstable/agent-mentions/list`
  - `_goose/unstable/slash-commands/list`

Maple should not depend on this namespace for v1 ACP interoperability. A future
Goose-enhanced mode could optionally use a small subset, likely session info,
working-directory updates, provider list/config, and richer usage/status.

### Goose Desktop Client Capabilities

Goose Desktop initializes with:

- `protocolVersion` from ACP SDK.
- `clientCapabilities.elicitation.form`.
- `_meta.goose.mcpHostCapabilities`.
- `_meta.goose.customNotifications: true`.
- `_meta.goose.recipeParameterRequests: true`.

Maple should use this as a pattern, not a default. Only advertise similar custom
capabilities once Maple has the matching UI and callbacks.

## OpenCode Details

### Launch and Transport

`opencode acp` starts OpenCode's internal HTTP server, creates an SDK client to
that local server, and bridges ACP over stdio with `AgentSideConnection` and
`ndJsonStream`.

External ACP clients still see stdio JSON-RPC. The internal HTTP server is an
implementation detail.

### Capabilities

OpenCode initialize returns:

- `protocolVersion: 1`
- `agentCapabilities.loadSession: true`
- `mcpCapabilities.http: true`
- `mcpCapabilities.sse: true`
- `promptCapabilities.embeddedContext: true`
- `promptCapabilities.image: true`
- `sessionCapabilities.close: {}`
- `sessionCapabilities.fork: {}`
- `sessionCapabilities.list: {}`
- `sessionCapabilities.resume: {}`
- `agentInfo.name: "OpenCode"`

It does not advertise:

- `auth.logout`
- `sessionCapabilities.delete`
- `sessionCapabilities.additionalDirectories`
- audio prompt content
- boolean config options

### Auth

Auth method:

- `opencode-login`
- description: run `opencode auth login` in terminal
- if client advertises `_meta["terminal-auth"] === true`, OpenCode adds:
  - command: `opencode`
  - args: `["auth", "login"]`
  - label: `OpenCode Login`

`authenticate` accepts only `opencode-login`, returns `{}`, and rejects unknown
methods as invalid params without leaking secrets. Tests cover this.

### Session Lifecycle

OpenCode supports:

- `session/new`
- `session/load`
- `session/list`
- `session/resume`
- `session/close`
- `session/fork`

Behavior:

- `session/new` snapshots the working directory, chooses default model/variant
  and mode, creates an internal OpenCode session, registers MCP servers, sends
  available commands, and returns `sessionId` plus `configOptions`.
- `session/load` restores from stored messages, registers MCP servers, sends
  available commands, replays messages, and returns config options.
- `session/resume` restores model/variant/mode from the last 20 messages and
  does not replay history.
- `session/list` merges stored sessions with live ACP-created sessions, sorts by
  updated time descending, and paginates with a timestamp cursor.
- `session/close` removes live ACP state and aborts the backing OpenCode
  session.
- `session/cancel` aborts the backing OpenCode session.
- `session/fork` calls internal session fork, restores config from the forked
  history, registers MCP, sends commands, replays fork messages, and returns the
  new session id/config options.

### Prompt Handling

OpenCode prompt flow:

- Converts ACP content blocks to OpenCode prompt parts.
- Detects slash commands by joining text parts and checking for leading `/`.
- Normal prompt calls internal `session.prompt`.
- Known slash command calls internal `session.command`.
- `/compact` calls `session.summarize`.
- Sends usage update after prompt/command paths.
- Maps errors to ACP stop reasons:
  - normal -> `end_turn`
  - `MessageAbortedError` -> `cancelled`
  - `MessageOutputLengthError` -> `max_tokens`
  - `ContentFilterError` -> `refusal`
  - auth errors -> ACP auth-required style error

### Content Support

OpenCode input mapping:

- Text -> text part, preserving audience annotations.
- Image base64/data/http(s) URI -> file part.
- `resource_link` -> file URL part for `file://`, `zed://` path translation, or
  text fallback.
- Embedded text resource -> text with `[uri]` prefix; file URL with `#L` line
  hash includes path and line.
- Embedded binary resource -> data URL file part if MIME type exists.
- Audio currently maps to empty/no-op.

Replay/output mapping:

- Text -> `agent_message_chunk` or `user_message_chunk`.
- Reasoning -> `agent_thought_chunk`.
- File data URLs become images/resources/resource links.

### Tool Calls and Permissions

OpenCode subscribes to global events:

- `permission.asked`
- `message.part.updated`
- `message.part.delta`

Tool event mapping:

- First tool part -> `tool_call` with pending status.
- Running -> `tool_call_update` with in-progress status.
- Completed -> `tool_call_update` completed with output/content/rawOutput.
- Error -> `tool_call_update` failed with error content/rawOutput.
- Bash/shell output is rendered as tool content, not standard ACP terminal
  methods.

Permission flow:

- If client lacks `requestPermission`, OpenCode rejects.
- Otherwise it sends `session/request_permission`.
- Options are `once`, `always`, `reject`.
- For edit permissions, OpenCode can call client `writeTextFile` with a proposed
  patched file if available.

### Config Options

OpenCode config options:

- `model`
  - category `model`
  - select option values like `provider/model`
  - can encode variants as `provider/model/variant` when configured
- `effort`
  - category `thought_level`
  - select from model variants
- `mode`
  - category `mode`
  - select from available primary/non-hidden agents/modes

It also implements unstable `setSessionModel`, but the generic
`session/set_config_option` path is the path Maple should use.

## claude-agent-acp Details

### Launch and Transport

`claude-agent-acp` is an ACP stdio agent backed by the official Claude Agent SDK.
It wires methods through the TypeScript ACP SDK and uses `ndJsonStream` over
process stdin/stdout.

### Capabilities

Initialize returns:

- `protocolVersion: 1`
- `_meta.claudeCode.promptQueueing: true`
- `promptCapabilities.image: true`
- `promptCapabilities.embeddedContext: true`
- `mcpCapabilities.http: true`
- `mcpCapabilities.sse: true`
- `auth.logout: {}`
- `loadSession: true`
- `sessionCapabilities.additionalDirectories: {}`
- `sessionCapabilities.close: {}`
- `sessionCapabilities.delete: {}`
- `sessionCapabilities.fork: {}`
- `sessionCapabilities.list: {}`
- `sessionCapabilities.resume: {}`
- `agentInfo.name: "@agentclientprotocol/claude-agent-acp"`
- `agentInfo.title: "Claude Agent"`

It does not advertise audio prompt support.

### Auth

Auth methods are capability/environment dependent:

- Terminal auth methods are offered only when the client advertises
  `clientCapabilities.auth.terminal === true` or
  `clientCapabilities._meta["terminal-auth"] === true`.
- In remote environments, it offers a terminal method that runs Claude login in
  a terminal/TUI-compatible path.
- In local environments, it can offer Claude subscription and Anthropic Console
  login paths.
- If `_meta["terminal-auth"]` is supported, auth methods include a command/args
  payload the client can run.
- If client advertises `auth._meta.gateway === true`, it offers `gateway` and
  `gateway-bedrock` auth methods with `_meta.gateway.protocol`.

`authenticate` currently handles gateway methods. Terminal login methods are
handled externally by running the advertised command. `logout` clears gateway
credentials and runs `claude auth logout`.

### Session Lifecycle

Claude wrapper supports:

- `session/new`
- `session/load`
- `session/resume`
- `session/list`
- `session/delete`
- `session/close`
- `session/fork`
- `session/set_mode`
- `session/set_config_option`
- `session/prompt`
- `session/cancel`
- `logout`

Important behavior:

- Validates `cwd` is absolute and exists before creating a session.
- Computes a stable session fingerprint from `cwd` and sorted MCP servers.
- `loadSession` and `resumeSession` reuse an existing active query if the
  fingerprint matches, otherwise tear it down and recreate it.
- `loadSession` replays history; `resumeSession` does not.
- `deleteSession` tears down active state before deleting the SDK session.
- `closeSession` cancels in-flight work, closes the SDK query/subprocess, and
  removes active session state.
- `session/fork` creates a new session id and starts Claude with resume/fork
  options.

### Prompt and Streaming

The wrapper has a long-lived per-session consumer of the Claude SDK stream:

- `prompt()` enqueues a turn and pushes an SDK user message.
- Consumer forwards messages as ACP `session/update` notifications.
- Prompt queueing is explicit in `_meta.claudeCode.promptQueueing`.
- Consumer handles background/between-turn output, not only output while a
  single `session/prompt` request is awaiting.
- It tracks message IDs, streamed content, stop reasons, usage, session title
  updates, cancellation, and query stream closure.

### Cancellation

Cancellation has extra hardening:

- Calls SDK `query.interrupt()`.
- Arms a force-cancel timer so a wedged SDK stream eventually resolves the
  active prompt as `cancelled`.
- On teardown, aborts the consumer wake signal immediately and closes the query.

This is a good reminder for Maple: local UI state must tolerate agents that do
not stop cleanly or emit final events immediately.

### Content, Tool Calls, and Plans

Content mapping:

- Text -> agent/user message chunks.
- Images -> ACP image content.
- Thinking -> `agent_thought_chunk`.
- Claude `TodoWrite` and Task tools become ACP `plan` updates.
- `tool_use`, `server_tool_use`, and `mcp_tool_use` become `tool_call` or
  `tool_call_update`.
- Tool results become completed/failed updates with raw output and content.
- Edit/write hooks can produce richer diff updates from structured patches.

Tool-call details:

- The wrapper tracks emitted tool call IDs so permission flow and streaming flow
  do not duplicate the same `tool_call`.
- Permission prompts can eagerly emit the tool call before asking permission.
- `_meta.claudeCode.toolName`, `toolResponse`, and `parentToolUseId` carry
  Claude-specific details.

### Permissions

Permission options:

- `allow_always`
- `allow_once`
- `reject_once`
- special plan-mode exit options that can switch mode

Special behavior:

- `ExitPlanMode` permission can update ACP current mode/config option.
- `bypassPermissions` mode auto-allows.
- `AskUserQuestion` is handled as an ACP form elicitation if the client supports
  form elicitation.

### Elicitation

The wrapper bridges the Claude SDK's elicitation/dialog callbacks to ACP
unstable elicitation methods:

- MCP-originated elicitations can be forwarded as ACP form or URL elicitations
  only when the client advertises support.
- `AskUserQuestion` requires form elicitation and is disabled when the client
  lacks form support.
- Refusal fallback consent prompts are rendered as form elicitations.
- URL elicitations use `elicitation/complete` when the underlying MCP flow
  finishes server-side.

Maple should not advertise `elicitation.form` or `elicitation.url` until it can
render and complete those flows correctly. If Maple does support them, Claude
compatibility improves materially.

### Terminal Output Metadata

The wrapper supports terminal-like rendering through `_meta`, not the standard
v1 `terminal/*` client methods:

- Client advertises `_meta["terminal_output"] === true`.
- Bash tool call starts with `_meta.terminal_info`.
- Output can stream as `_meta.terminal_output`.
- Final status can include `_meta.terminal_exit`.
- The comments say this matches a `codex-acp` terminal-output lifecycle.

Maple can initially render this as tool-call content if it does not opt into
terminal-output metadata. A richer terminal widget can come later behind the
capability.

### Config Options

Stable config IDs:

- `mode`
- `model`
- `effort`
- `agent`
- `fast`

Behavior:

- `mode`: Claude permission/session mode. Values include `auto`, `default`,
  `acceptEdits`, `bypassPermissions`, `dontAsk`, and `plan` depending on model
  and settings.
- `model`: AI model selector with alias/fuzzy resolution and allowlist handling.
- `effort`: surfaced only when current model supports effort levels.
- `fast`: surfaced only when the current model supports fast mode. Uses boolean
  config option if the client advertised support, otherwise falls back to select
  values `on`/`off`.
- `agent`: custom main-thread agent persona selector, omitted when no custom
  agents exist.

### Claude-specific Metadata

Observed `_meta` surfaces:

- `_meta.claudeCode.promptQueueing`
- `_meta.claudeCode.options` in session creation for SDK pass-through settings
  such as env, model settings, tools, hooks, and extra args.
- `_meta.claudeCode.emitRawSDKMessages`
- `_meta.systemPrompt`
- legacy `_meta.additionalRoots`
- `_meta.terminal_info`, `_meta.terminal_output`, `_meta.terminal_exit`
- `_meta.gateway`
- `_meta["terminal-auth"]`

Maple should preserve but not depend on these for baseline ACP.

## Shared Functionality Across Target Agents

The shared ACP surface Maple should prioritize:

- Launch/configure an agent connection.
- `initialize` with client info and carefully chosen capabilities.
- Render auth methods and run terminal-auth command metadata where advertised.
- Create new sessions with absolute cwd and MCP servers.
- Load/resume/list/close sessions when capability allows.
- Prompt with text, resource links, images, and embedded resources according to
  prompt capabilities.
- Receive and render `session/update`:
  - agent/user/thought message chunks
  - tool calls and tool call updates
  - plans
  - usage updates
  - available commands
  - mode/config option updates
  - session info updates where present
- Send cancellation and settle pending permission prompts as cancelled.
- Render permission prompts from `session/request_permission`.
- Render config options generically and call `session/set_config_option`.
- Keep per-session slash commands updated.
- Preserve `_meta` and unknown fields for debugging/trace compatibility.

## Differences Maple Must Abstract

Protocol version:

- All targets are v1 today, but v2 docs/schemas are active. Keep a versioned
  adapter boundary.

Transport:

- OpenCode and Claude are stdio only.
- Goose supports stdio and draft HTTP/WebSocket.

Session replay:

- `session/load` replays history before responding.
- `session/resume` does not replay.
- Some agents return config state from both paths.

Message IDs:

- v1 message IDs are not universally required.
- v2 message IDs are required and have upsert semantics.

Config options:

- IDs and categories overlap but are not identical.
- Boolean option support must be negotiated.
- Agents may update config options during a turn.

Permissions:

- OpenCode has simple allow once/always/reject.
- Claude has plan-mode and mode-switching permission flows.
- Goose can use custom richer app flows.

Terminal behavior:

- ACP v1 has standard terminal methods, but target agents also use tool content
  or private terminal metadata.
- Maple should first render tool content robustly, then add a terminal-specific
  adapter if needed.

Elicitation:

- Claude depends on form/url elicitation for some important user-question and
  MCP flows.
- Goose Desktop advertises form elicitation.
- OpenCode did not show elicitation use in current source.

Private extensions:

- Goose has a large `_goose/unstable/*` app API.
- Claude has rich `_meta.claudeCode` and terminal/gateway metadata.
- OpenCode is relatively minimal.

## Maple Client Capability Plan

Conservative initial initialize request:

```json
{
  "protocolVersion": 1,
  "clientCapabilities": {
    "session": {
      "configOptions": {
        "boolean": {}
      }
    },
    "_meta": {
      "terminal-auth": true
    }
  },
  "clientInfo": {
    "name": "maple",
    "title": "Maple",
    "version": "..."
  }
}
```

Add only when implemented:

- `fs.readTextFile`: if Maple can safely answer file reads in the selected
  project roots.
- `fs.writeTextFile`: if Maple can mediate writes and update files safely.
- `terminal`: if Maple can create, stream, wait, release, and kill terminals.
- `elicitation.form`: if Maple can present forms and return structured results.
- `elicitation.url`: if Maple can present external/browser URL elicitations and
  send completion notifications.
- `_meta["terminal_output"]`: if Maple can render terminal output metadata from
  Claude-like agents.
- `_meta.goose.customNotifications`: if Maple builds a Goose-specific adapter
  for `_goose/unstable/session/update`.
- `_meta.goose.recipeParameterRequests`: only if Maple supports Goose recipe
  parameter prompts.

## Maple Architecture Notes

These are requirements, not a final implementation decision.

Stdio local agents:

- Tauri/Rust side should likely own:
  - subprocess launch
  - stdin/stdout/stderr
  - JSON-RPC framing
  - process lifetime
  - cancellation/kill
  - filesystem and terminal methods if advertised
  - secure env/path handling
- Frontend should likely own:
  - session UI state
  - transcript rendering
  - permission dialogs
  - config controls
  - slash-command palette
  - user-facing auth flows

HTTP/WebSocket agents:

- Frontend can potentially connect directly if CORS/auth allow it.
- Tauri may still be useful as a local proxy/secret boundary.
- Goose's `/acp` router makes this path attractive for embedded Goose and local
  service modes, but not for OpenCode/Claude stdio compatibility.

Multiple agent servers:

- Model as a connection registry:
  - display name
  - command/args/env/cwd
  - transport kind
  - negotiated protocol version
  - capabilities
  - auth methods
  - connection status
  - sessions
- Sessions are scoped by agent connection. `sessionId` is not globally unique
  across agents.

Trace/debugging:

- Store raw JSON-RPC envelopes in a bounded debug log, with redaction.
- Store negotiated capabilities and extension metadata.
- Expose "copy diagnostics" for compatibility issues.

## Compatibility Test Plan

Future implementation should include:

- A fake ACP v1 server fixture that can script:
  - initialize/auth
  - session/new
  - prompt streaming
  - permission request
  - tool call lifecycle
  - config option updates
  - cancellation
  - load replay
  - resume no-replay
- Golden transcript fixtures for v1 update parsing.
- Real-agent smoke tests:
  - `goose acp`
  - `opencode acp`
  - `claude-agent-acp`
- Per-agent manual test checklist:
  - initialize/auth
  - create session
  - send plain prompt
  - send image prompt if supported
  - send embedded resource if supported
  - tool call rendering
  - edit permission prompt
  - cancel running turn
  - list/load/resume/close
  - switch model/mode/effort config option
  - slash command list and execution
  - MCP server handoff
  - extension metadata preserved in debug log

## Open Questions

- Should Maple advertise form elicitation in v1? It materially improves Claude
  support but requires a real form UI and stable response mapping.
- Should Maple initially advertise filesystem write support? It enables richer
  edit preview/apply behavior, but it is a security and UX commitment.
- Should Maple run terminal-auth commands itself, or show a copyable command and
  terminal launcher first?
- How much Goose-specific `_goose/unstable/*` support belongs in Maple v1 versus
  a later Goose-enhanced mode?
- Should Maple expose one global ACP activity view or per-agent/per-session
  traces?
- How should Maple reconcile v1 optional message IDs with v2-required message
  IDs in its internal transcript model?
- What is the right policy for additional roots and MCP server definitions when
  a Maple project spans multiple local directories?

## Running Research Log

2026-07-08:

- Created managed workspace `acp-support` with Maple, maple-billing-server, and
  opensecret.
- Cloned official ACP repo to `/Users/admin/repos/ThirdParties/agent-client-protocol`.
- Cloned claude-agent-acp to `/Users/admin/repos/ThirdParties/claude-agent-acp`.
- Refreshed OpenCode at `/Users/admin/repos/ThirdParties/opencode`.
- Cloned fresh Goose upstream at `/Users/admin/repos/ThirdParties/goose-upstream`.
- Verified official ACP README still says stable protocol version is `1`.
- Extracted v1/v2 stable and unstable method lists from schema metadata.
- Read v1/v2 docs for initialization, transports, session setup, prompt
  lifecycle, config options, tool calls, permissions, and cancellation.
- Inspected Goose ACP server, transport, provider/client adapter, response
  builder, and custom request/notification definitions.
- Inspected OpenCode ACP CLI bridge, service, event, permission, content, tool,
  config option helpers, and tests.
- Inspected claude-agent-acp initialize/auth/session/prompt/config/permission/
  elicitation/terminal-output code and tests.
- Reviewed Maple PR #606 for proof-of-concept context and explicitly kept it out
  of the v1 protocol-support architecture decision.
