# Agent Mode Direct Goose Product Plan

This document captures the product direction, UX constraints, and technical
planning decisions for Maple Agent Mode using direct Goose crates. It is a
planning artifact only. It does not start implementation.

## Executive Decision

Maple should build Agent Mode as a first-class product surface, not as normal
chat with extra tools attached.

For the built-in Maple agent, the preferred runtime path is direct Goose through
the Rust crates in the Goose submodule. ACP remains useful later as an optional
adapter for external agents, but it should not be the default interface between
Maple Desktop and its own bundled Goose-powered agent.

The product split is:

```text
Maple Chat
  private conversational AI

Maple Agent Mode
  private local workspace agent
  runs on the user's desktop
  works inside selected filesystem folders
  powered by Maple's local proxy and embedded Goose
```

The user-facing promise is not "Maple has Goose" or "Maple supports ACP." The
promise is:

> Maple can safely act for me inside a selected local project folder, with clear
> visibility into what it is doing.

## Why Agent Mode Is A Good Product Bet

Agent Mode moves Maple beyond generic chat without abandoning Maple's core
identity. Maple already has:

- A desktop app.
- A private AI chat UX.
- A local OpenAI-compatible proxy.
- Account, billing, and model access.
- Existing project/chat organization concepts.
- A privacy-oriented product story.

A local workspace agent fits that base. It gives Maple a high-value capability:
the assistant can inspect, edit, run commands, test, and explain work inside a
user-selected folder on the user's own machine.

This is similar to the ChatGPT/Codex split:

```text
ChatGPT
  general conversation

Codex inside ChatGPT
  execution-oriented workspace agent
```

Maple's version should be:

```text
Maple Chat
  private conversation

Maple Agent Mode
  local private project/workspace agent
```

The important product distinction is that Agent Mode is not merely a model
choice or a chat setting. It is an execution surface.

## Codex UX Reference

The Codex screenshots are useful as a product reference because they show a
clear mental model:

- Codex has its own top-level tab.
- Projects are filesystem folders.
- Sessions are grouped under projects.
- The active run view shows the project and device context.
- The composer is scoped to the active project/session.
- Device chips prepare the UX for remote desktop execution.
- The session list is separate from normal chat recents.

Maple should use the same conceptual split, but not copy Codex's visual
language. Maple should keep its existing styling, layout density, tokens,
typography, dark theme, buttons, menus, dialogs, and chat surface feel.

## Product Principles

### Agent Mode Is A Separate Surface

Agent Mode should have its own sidebar entry and its own pages. It should not be
folded into ordinary Maple chat history.

Normal chat history and agent sessions are different artifacts:

- Chat history is conversation state.
- Agent session history is workspace execution state.

They can look related, but they should not be stored, named, filtered, or routed
as the same product object.

### Greenfield Concepts, Existing Maple Feel

Agent Mode introduces new concepts:

- Agent projects.
- Local filesystem roots.
- Agent sessions.
- Active runs.
- Tool timelines.
- Permission prompts.
- Runtime status.
- Device context.
- Session logs.

Those concepts should be modeled cleanly rather than squeezed into existing
Maple chat objects.

At the same time, Agent Mode should reuse Maple's existing visual system:

- Sidebar patterns from `frontend/src/components/Sidebar.tsx`.
- Chat list grouping patterns from `frontend/src/components/ChatHistoryList.tsx`.
- Project detail layout ideas from `frontend/src/components/ProjectDetailView.tsx`.
- Composer/message styling ideas from `frontend/src/components/UnifiedChat.tsx`.
- Shared primitives from `frontend/src/components/ui/*`.
- Markdown rendering from `frontend/src/components/markdown.tsx`.
- Global styling from `frontend/src/index.css` and `frontend/src/chat.css`.

The rule is:

> Copy Maple patterns first. Invent only where Agent Mode has a genuinely new
> concept.

### Do Not Combine Product Concepts

We should keep these concepts separate:

- Maple conversation project: existing server-side chat organization.
- Agent project: local filesystem folder.
- Maple chat conversation: normal AI chat session.
- Agent session: local workspace execution timeline.
- Agent run: one prompt execution inside an agent session.
- Runtime adapter: Goose direct now, ACP later.

The UI may reuse list rows, headers, dialogs, and composer styling, but the data
model should keep these objects distinct.

### Maple Owns The UX And Trust Layer

Goose should be the harness/runtime. Maple should own the product surface around
it:

- Project picker.
- Device/runtime status.
- Model and effort selection.
- Session browser.
- Timeline rendering.
- Tool-call display.
- Permission prompts.
- Logs and diagnostics access.
- Stop/cancel/retry/resume controls.
- Future encrypted sync and remote control.

If Maple exposes Goose too directly, the product becomes a Goose skin. That is
not the desired end state. Maple should use Goose as a strong embedded agent
engine while keeping Maple's own app model.

## Information Architecture

The proposed top-level structure is:

```text
Maple
  New Chat
  Search
  Agent Mode
  Projects
  Recents
```

Agent Mode should route to a separate agent area:

```text
Agent Mode
  Agent home / project browser
  Agent project detail
  Agent session detail
```

### Agent Home / Project Browser

Purpose: let the user pick where the agent should work.

Core elements:

- Header: "Agent Mode".
- Runtime status: stopped, starting, running, failed.
- Device identity: local desktop in V1.
- Project list grouped by filesystem roots.
- Session list under each project.
- Search/filter for agent sessions.
- New project/folder picker.
- New agent session action.

V1 can be simple. It does not need remote devices or sync, but the shape should
not block them.

### Agent Project Detail

Purpose: show all agent sessions for one local folder.

Core elements:

- Project name derived from folder basename.
- Full root path.
- Runtime/model status.
- Recent sessions.
- New session composer.
- Folder actions: reveal in Finder, change folder, remove from list.
- Optional project metadata: last opened, last run, runtime adapter.

This is analogous to Maple's `ProjectDetailView`, but it is local and
filesystem-backed rather than server-backed chat-project data.

### Agent Session Detail

Purpose: show the active execution timeline and prompt composer.

Core elements:

- Header with session title, project root, runtime status, model/effort.
- Timeline of user prompts, assistant messages, thinking, tool calls, tool
  results, errors, permission requests, usage, and final answers.
- Composer scoped to the project root.
- Stop/cancel button during active runs.
- Retry/resume affordance after failures.
- Logs/diagnostics entry point.

The active session view should feel like Maple's chat page, but the content is
an execution timeline, not a pure conversation transcript.

## V1 Product Scope

V1 should stay narrow:

- Desktop only.
- Local machine only.
- One selected filesystem project root per session.
- Goose direct runtime only.
- Maple proxy auto-started.
- Local proxy API key auto-created if missing.
- No remote mobile control.
- No encrypted cloud session sync.
- No external ACP runtime selection.
- No broad "control my whole computer" promise.

The first reliable user workflow should be:

1. User opens Maple Desktop.
2. User selects Agent Mode.
3. User picks a project folder.
4. Maple initializes local proxy and embedded Goose.
5. User asks the agent to work in that folder.
6. Maple shows the run timeline with thinking, tools, results, and errors.
7. User can stop the run.
8. User can inspect what happened.
9. User can resume or start another session later.

This is enough to validate the product.

## Later Product Direction

The V1 architecture should leave room for:

- Remote mobile-to-desktop control.
- Enclave-mediated device relay.
- Encrypted session backup/sync.
- External ACP agents such as Codex or Claude Code.
- Multiple local desktop devices.
- Scheduled/background tasks.
- Subagents and automation if Goose exposes them cleanly.

The data model should include device/runtime fields early, even if V1 only has
one local desktop.

## Data Model

Maple should define its own frontend/runtime-facing data model. These are Maple
objects, not Goose objects.

```ts
type AgentRuntimeKind = "goose-direct" | "acp";

interface AgentDevice {
  id: string;
  displayName: string;
  kind: "local-desktop" | "remote-desktop";
  status: "available" | "offline" | "unknown";
}

interface AgentProject {
  id: string;
  deviceId: string;
  displayName: string;
  rootPath: string;
  runtimeKind: AgentRuntimeKind;
  createdAt: string;
  updatedAt: string;
  lastOpenedAt?: string;
}

interface AgentSession {
  id: string;
  projectId: string;
  deviceId: string;
  runtimeKind: AgentRuntimeKind;
  title: string;
  status: "idle" | "running" | "cancelled" | "failed";
  createdAt: string;
  updatedAt: string;
  lastRunAt?: string;
}

interface AgentRun {
  id: string;
  sessionId: string;
  status: "running" | "completed" | "cancelled" | "failed";
  startedAt: string;
  completedAt?: string;
}
```

The timeline should also be Maple-owned:

```ts
type AgentTimelineItem =
  | AgentUserMessageItem
  | AgentAssistantMessageItem
  | AgentThinkingItem
  | AgentToolCallItem
  | AgentPermissionRequestItem
  | AgentErrorItem
  | AgentUsageItem;
```

Goose events should be mapped into this model on the Tauri side or in a thin
frontend adapter. The rest of the UI should not depend on Goose internals.

## Runtime Boundary

The desired architecture is:

```text
Maple frontend
  <=> Tauri commands and emits
    <=> Maple AgentRuntime trait
      <=> GooseDirectRuntime
        <=> goose::agents::Agent
```

The frontend speaks Maple concepts:

- start runtime
- create project
- list projects
- create session
- list sessions
- load session
- send prompt
- cancel run
- respond to permission request
- open logs

The direct Goose adapter handles:

- Goose config/root setup.
- Provider setup for Maple proxy.
- SessionManager calls.
- AgentConfig and Agent construction.
- Developer/platform tool setup.
- Agent::reply stream consumption.
- Goose event to Maple event conversion.
- Cancellation tokens.
- Permission confirmation routing.
- Diagnostics/log collection.

This keeps the door open for:

```text
Maple AgentRuntime
  GooseDirectRuntime
  AcpRuntime
  FutureRuntime
```

ACP support can come back as an adapter without changing the Agent Mode UI.

## Goose Internal API Research Plan

Because the current published `goose-sdk` crate does not yet expose the full
agent runtime, this experiment uses the Goose submodule directly. We should
research the internal APIs deliberately before wiring UI.

### Agent Loop

Research:

- `goose::agents::Agent`
- `goose::agents::AgentConfig`
- `goose::agents::SessionConfig`
- `goose::agents::AgentEvent`
- `Agent::reply`
- `Agent::update_provider`
- `Agent::update_goose_mode`
- `Agent::list_tools`
- `Agent::add_extension`
- `Agent::handle_confirmation`

Questions:

- Can one `Agent` safely serve multiple sessions?
- Should Maple create one agent per session or one runtime agent per project?
- Which calls rely on `Config::global()`?
- How does cancellation behave mid-tool versus mid-model stream?

### Sessions

Research:

- `goose::session::SessionManager`
- session create/list/load/delete/export/import
- session usage totals
- session truncation
- session naming
- session working directory behavior

Questions:

- Can Maple use Goose session storage as the source of truth for Agent Mode?
- Which fields are stable enough to show in the UI?
- How should Maple map local folders to Goose sessions?
- How should Maple handle deleted or moved project folders?

### Provider Configuration

Research:

- `goose::providers::*`
- OpenAI-compatible provider construction.
- provider registry helpers.
- model config helpers.
- request logging hooks.
- `Config::global()` dependencies.

Questions:

- Can Maple configure the local proxy without writing Goose secrets/config?
- Can Maple pass provider credentials directly?
- Can model/effort be changed per session?
- Can Maple record raw request/response diagnostics in a supported way?

### Developer Tools And Platform Extensions

Research:

- `goose::agents::platform_extensions::*`
- developer tools implementation.
- in-process platform extension registration.
- working-directory context.
- tool metadata and permission metadata.

Questions:

- Can Maple enable developer tools without spawning the Goose binary?
- Can tools be scoped strictly to the selected project root?
- Can Maple hide or disable tools per mode?
- Which tools should V1 expose by default?

### Permissions

Research:

- `goose::config::permission::PermissionManager`
- `goose::permission::*`
- `Agent::handle_confirmation`
- action-required/elicitation content.
- permission routing in providers/tools.

Questions:

- Can Maple pause a tool call and show native permission UI?
- What data does Maple receive before tool execution?
- Can Maple distinguish read-only from mutating operations?
- Can Maple persist user allow/deny decisions per project?

### Event Mapping

Research:

- `AgentEvent`
- `Message`
- `MessageContent`
- `ToolRequest`
- `ToolResponse`
- `Thinking`
- `SystemNotification`
- `ActionRequired`
- `HistoryReplaced`
- `McpNotification`

Questions:

- Which events are deltas and which are full messages?
- How do we keep tool calls in correct order?
- How should thinking be grouped and collapsed?
- How are errors represented?
- What should be persisted by Maple versus read back from Goose?

### Context And Compaction

Research:

- Goose context management APIs.
- compaction behavior.
- model context usage.
- session truncation.

Questions:

- Can Maple show useful context/token usage?
- Can Maple expose compaction events?
- Can users recover or inspect pre-compaction history?

### Logs And Diagnostics

Research:

- Goose tracing/logging setup.
- request logs.
- diagnostics generation.
- session diagnostics.
- parser/provider error surfaces.

Questions:

- Can Maple configure log directories directly?
- Can Maple collect one diagnostic bundle per failed run?
- Can Maple expose raw provider logs when debug mode is enabled?
- Which logs might contain secrets and need redaction?

### Goose Desktop Reference

Reference paths:

- `ThirdParties/goose/ui/desktop/src`
- `ThirdParties/goose/ui/sdk/src`
- `ThirdParties/goose/ui/text/src/tui.tsx`
- `ThirdParties/goose/ui/text/src/toolcall.tsx`
- `ThirdParties/goose/ui/desktop/src/sessions.ts`

Use Goose Desktop as inspiration for:

- session creation/resume behavior.
- tool-call timeline behavior.
- extension configuration flows.
- provider/model configuration.
- diagnostics and backend status handling.

Do not import Goose Desktop's visual style into Maple. The reference value is
API usage and interaction modeling.

## Maple UI Reuse Plan

Agent Mode should be new files and pages, but existing Maple components should
guide the implementation.

Likely new surfaces:

- `AgentModePage`
- `AgentProjectBrowser`
- `AgentProjectDetail`
- `AgentSessionView`
- `AgentTimeline`
- `AgentComposer`
- `AgentToolCallRow`
- `AgentThinkingBlock`
- `AgentPermissionDialog`
- `AgentRuntimeStatusBadge`
- `AgentProjectPicker`

Likely reused patterns/components:

- `Sidebar` for top-level navigation.
- `ChatHistoryList` for grouped lists and row actions.
- `ProjectDetailView` for project detail layout ideas.
- `UnifiedChat` for composer and message-flow references.
- `ModelSelector` for model selection behavior.
- `Button`, `Dialog`, `DropdownMenu`, `Tooltip`, `Badge`, `Textarea`,
  `Select`, `Tabs`, and other shared UI primitives.

Styling guidance:

- Preserve Maple spacing, border radius, dark theme, and typography.
- Keep tool rows compact and readable.
- Use icons for tool actions and controls.
- Do not use marketing-page composition.
- Do not create a separate Goose/Codex-looking visual system.
- Avoid nesting cards inside cards.
- Keep the main session timeline scannable.
- Treat errors and permission prompts as first-class timeline events, not
  anonymous red banners.

## Timeline UX

The timeline is the core Agent Mode artifact. It should make the agent's work
auditable.

Recommended item order:

1. User message.
2. Thinking block, collapsed by default once there is meaningful output.
3. Tool call row, in chronological order.
4. Tool result summary.
5. Assistant message.
6. Error or permission request exactly where it happened.

Tool call rows should show:

- tool display title.
- status: queued, running, completed, failed, cancelled.
- concise input summary.
- expandable raw input/output.
- working directory if relevant.
- file paths touched if available.
- duration.
- error details if failed.

Thinking should be available but not noisy:

- collapse by default after completion.
- show active thinking while running if available.
- do not render token fragments as separate messages.

Errors should be actionable:

- title that names the failing layer.
- short explanation.
- expandable raw details.
- link/button to logs when useful.

## Trust And Permission Model

The trust model should be explicit in the UX.

The user should always know:

- which folder the agent is working in.
- whether the runtime is local or remote.
- which model/effort is selected.
- whether the proxy is running.
- whether a run is active.
- which tools are enabled.
- whether a tool is read-only or mutating.
- what command/file operation is about to run.
- how to stop the agent.

Suggested V1 defaults:

- Require an explicit project folder.
- Scope developer tools to that folder where possible.
- Prefer ask-first behavior for risky shell/file operations.
- Always show tool calls in the timeline.
- Always expose logs for failed runs.

## Remote Future

Remote is not V1, but the model should leave room for it.

The likely future shape:

```text
Maple Mobile
  sends prompt/session intent to enclave store or relay

Maple Desktop
  receives intent
  executes against local Goose runtime
  writes back session/run events

Maple Mobile
  reads assistant/tool/run updates
```

The desktop remains the execution environment. The enclave should route or store
encrypted session state/events, not execute against the user's filesystem.

The V1 data model should include `deviceId` and `runtimeKind` so mobile remote
control does not require a full migration later.

## Non-Goals

For the first direct-Goose experiment:

- Do not replace Maple Chat.
- Do not merge Maple chat projects with agent filesystem projects.
- Do not build remote mobile control.
- Do not build encrypted cloud sync.
- Do not build external ACP runtime selection.
- Do not expose Goose internals to TypeScript.
- Do not copy Goose Desktop styling.
- Do not make Agent Mode available on web, iOS, or Android.
- Do not ship submodule/internal APIs as final architecture without a follow-up
  decision.

## V1 Acceptance Criteria

The first shippable-quality local experiment should prove:

- Agent Mode has its own top-level Maple surface.
- User can add/select a filesystem project folder.
- User can create/load/list agent sessions for that folder.
- Maple initializes proxy/API key automatically.
- Tauri starts a direct embedded Goose runtime without ACP.
- User can send a prompt.
- Goose can read files and run developer tools in the selected folder.
- Timeline shows user message, thinking, tool calls, tool results, assistant
  output, usage, and errors in order.
- User can cancel an active run.
- Failed runs expose useful diagnostics.
- Frontend only consumes Maple-owned commands/events.
- Existing Maple styling is preserved.

## Questions To Resolve Before Coding

1. Should V1 have an Agent Mode home page, or should clicking Agent Mode open
   the last active project/session?
2. Should the first project picker live in the page body, the composer, or both?
3. Should agent sessions be persisted only in Goose storage, or should Maple
   maintain a small local index for project/session browsing?
4. What are the default tool permissions for V1?
5. Should model/effort live per session, per project, or globally?
6. How much raw tool input/output should be visible by default?
7. What exact event model should Tauri emit to the frontend?
8. What Goose global config touchpoints are acceptable for the experiment?
9. What Goose-side API gaps should be shown to the Goose team before we code
   around them permanently?

## Conclusion

The right product direction is:

> Agent Mode is Maple's local project/workspace agent, powered by embedded
> Goose, presented as a separate Maple surface with Maple-owned UX, state
> mapping, trust controls, and diagnostics.

The direct Goose crate experiment should validate this product shape while
keeping the implementation isolated behind a Maple runtime boundary. The goal is
not simply to make Goose respond from inside Tauri. The goal is to prove that
Maple can offer a native, private, inspectable local agent experience that feels
like Maple and can later grow into remote mobile control and external-agent
interop without rewriting the frontend.
