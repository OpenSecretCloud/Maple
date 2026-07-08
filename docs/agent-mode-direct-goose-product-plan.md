# Agent Mode Direct Goose Product Plan

This document records the product direction and implementation boundary for the
direct Goose Agent Mode experiment in Maple.

It is not a final shipping commitment. It is the working decision record for the
next experiment after the Goose-over-ACP proof of concept.

## Inputs

This plan is based on:

- the working Maple Desktop + Goose + ACP proof of concept;
- the direct-Goose workspace created at `/Users/admin/workspaces/goose-sdk/maple`;
- the Codex screenshots showing a separate execution surface inside ChatGPT,
  with filesystem projects, device/runtime context, and session lists grouped
  under projects;
- the Goose/GDK architecture direction from Spiral's July 7, 2026 post,
  [Spiral x Goose: The Best is Yet to Honk](https://spiralxyz.substack.com/p/spiral-x-goose-the-best-is-yet-to);
- the current state of Goose's published `goose-sdk` crates and internal parent
  `goose` crate APIs.

## Final Decision

Maple should build Agent Mode as a first-class Maple product surface, not as a
mode inside normal chat.

For Maple's built-in desktop agent, the next experiment should use direct
embedded Goose through Tauri-side Rust code.

ACP should stay in the broader Agent Mode architecture, but not as the default
path between Maple Desktop and its own built-in Goose runtime.

The intended split is:

```text
V1 built-in Agent Mode
  Maple UI
    -> Maple-owned Tauri commands/events
      -> direct embedded Goose
        -> Maple local OpenAI-compatible proxy
          -> Maple models

Later external Agent Mode
  Maple UI
    -> Maple-owned Tauri commands/events
      -> ACP adapter
        -> Codex / Claude Code / external Goose / other ACP runtime
```

The frontend should speak Maple Agent Mode concepts only. It should not depend
on Goose Rust types, ACP protocol objects, local runtime URLs, WebSockets, or
Goose config files.

## Discussion Decisions

The discussion settled these product and architecture points:

- Agent Mode should be a separate top-level Maple surface, similar in product
  shape to Codex inside ChatGPT.
- Normal Maple Chat and Agent Mode should not share the same chat/session
  concepts. They can share visual components, but they are different products.
- Agent projects are filesystem folders. This should be a first-class concept,
  not hidden inside a prompt or model setting.
- Agent sessions belong under projects and should eventually be browsable,
  searchable, resumable, and distinguishable from normal Maple chat history.
- The direct Goose path is the best fit for Maple's built-in agent because
  Maple owns the desktop process, local proxy, folder picker, permissions, and
  UI.
- ACP remains strategically important, but it should be an adapter for external
  runtimes rather than the default internal bridge to Maple's own embedded
  Goose runtime.
- The TypeScript frontend should talk to a Maple-owned Agent Mode command/event
  API. Tauri should decide whether the active runtime is direct Goose, ACP, or
  something else.
- The first direct-Goose branch is an experiment and proof point, not a final
  shipping commitment.
- Before treating the integration as production architecture, Maple should
  research the Goose internal crates and the Goose reference client in detail.

## Detailed Discussion Notes

The important product shift is that Agent Mode should be treated as a sibling
product inside Maple, not as a variant of the existing chat screen.

The Codex screenshots are useful because they show this separation clearly:

- Codex has its own navigation entry.
- Codex has projects that are really filesystem folders.
- Codex has session/chat lists that belong to the agent surface.
- Codex shows device/runtime context because execution happens somewhere
  concrete.
- Codex uses a chat composer, but the surrounding product is not ordinary chat.

That maps well to Maple:

```text
Maple
  Chat
    normal private AI conversation
    server-side Maple tools
    existing Maple chat history

  Agent Mode
    local filesystem projects
    local desktop runtime
    Goose sessions
    tool execution timeline
    permission decisions
    diagnostics and logs
```

The conclusion was not that Maple should copy Codex. The conclusion was that
Codex validates the product shape: a specialized agent tab inside a broader AI
chat product can be understandable if projects, sessions, runtime status, and
execution history are kept separate from normal chat.

Maple should keep its own visual language. The implementation can copy or adapt
existing Maple components, but it should not merge the concepts:

- reuse sidebar/list/composer/dialog/markdown primitives where they fit;
- copy-paste existing Maple component patterns if that keeps styling consistent;
- build new Agent Mode pages and state models;
- do not store agent sessions as normal Maple conversations;
- do not make normal Maple projects secretly mean local filesystem folders;
- do not expose Goose or ACP nouns as primary user-facing concepts.

The user should feel that Agent Mode is Maple acting inside a chosen local
folder, not a Goose demo embedded in Maple.

## Product Thesis

Agent Mode is Maple's local workspace execution surface.

Normal Maple Chat remains the place for private conversation, explanation,
server-side tools, and general AI chat. Agent Mode is the place where a user
selects a local folder and asks Maple to inspect, edit, run, test, debug, and
explain work on that machine.

The closest product analogy is Codex inside ChatGPT:

- same parent product;
- separate top-level surface;
- separate project and session model;
- execution-oriented timeline;
- local or device-aware runtime state;
- chat-shaped composer, but not ordinary chat history.

For Maple, the mental model should be:

```text
Maple Chat
  private AI conversation

Maple Agent Mode
  private local workspace execution
```

The user-facing promise should not be "Maple supports Goose" or "Maple supports
ACP." The promise should be:

> Maple can safely act for me inside a selected local project folder, with clear
> visibility into what it is doing.

## Product Positioning

The closest analogy is "Codex inside ChatGPT":

- same parent product;
- separate navigation entry;
- separate project/session list;
- separate runtime and execution model;
- shared account, theme, and product shell;
- chat-shaped interaction where that is natural;
- execution timeline where plain chat is not enough.

For Maple, this means Agent Mode can live next to existing chat without
competing with it. Chat remains optimized for conversation and private hosted
AI. Agent Mode is optimized for local project work.

The product should avoid two failure modes:

- making Agent Mode feel like a thin Goose control panel;
- making Agent Mode feel like a risky hidden toggle inside normal chat.

The right target is a Maple-native workspace agent.

## Why Agent Mode Should Be Separate

Agent Mode introduces concepts that do not fit cleanly into normal chat:

- selected local filesystem folders;
- active desktop runtime status;
- resumable agent sessions;
- tool execution timelines;
- tool permissions;
- shell/file output;
- cancellation;
- local logs and diagnostics;
- possible future device routing and remote control.

These concepts deserve a separate surface because they change the user's trust
model. A normal chat message is conversational. An agent run may inspect files,
execute commands, request permission, modify a workspace, and continue over
multiple tool calls.

Agent Mode should therefore have its own navigation item, project/session list,
composer, runtime status, and timeline. It should not be a model selector or
toggle inside the existing Maple chat page.

## Product Objects

The Maple product model should use Maple-owned objects:

- Agent project: a selected filesystem folder.
- Agent session: a resumable thread of work scoped to an agent project.
- Agent run: one user request and the agent/tool activity that follows.
- Timeline item: assistant text, thinking, tool call, tool result, permission,
  error, or usage event.
- Permission request: a user decision point for a tool action.
- Runtime: the local or external agent backend that executes the session.
- Device: later, the desktop machine capable of executing a session.

These objects should remain stable even if the runtime changes from direct
Goose to ACP, or if Goose later moves the needed APIs into a stable GDK crate.

## Project And Session Model

Agent projects should be local filesystem folders. The folder is the trust and
execution boundary. It tells the user where Maple may inspect files, run
commands, and eventually make edits.

Maple should represent projects with Maple-owned metadata:

- display name;
- filesystem path;
- last opened time;
- runtime/device association;
- recent sessions;
- optional user pinning or favorites later.

Goose can own the detailed execution transcript if its session APIs provide the
right lifecycle operations. Maple should still keep a lightweight index for the
UI so the product is not forced to scrape session files just to render a project
browser.

The likely split is:

```text
Goose session store
  full agent transcript
  tool calls/results
  runtime-resumable context

Maple agent index
  project list
  session list metadata
  titles
  last activity
  selected runtime/device
  local UI preferences
```

If Goose's direct APIs can list, load, title, delete, and resume sessions cleanly,
Maple should use them. If they are incomplete, Maple should wrap them behind its
own agent-session API and keep the missing metadata locally.

Normal Maple chat history should not be the source of truth for Agent Mode.
Agent sessions are execution records, not chat conversations.

## Runtime Strategy

For Maple's built-in desktop agent, direct Goose is the default runtime choice.
The runtime should be embedded and owned by Tauri:

```text
TypeScript UI
  calls Maple Agent Mode commands
  listens to Maple Agent Mode events

Tauri Agent Runtime
  owns Goose construction
  owns sessions
  owns local proxy initialization
  owns cancellation
  owns permission routing
  maps Goose events into Maple timeline events
```

The TypeScript side should never care whether the active runtime is direct
Goose, ACP, or a future adapter. This keeps Maple's product surface stable while
the runtime layer evolves.

The previous ACP PoC remains valuable as a comparison point. It proved that
Goose can work with Maple's local proxy and that the timeline UI direction is
viable. The direct-Goose experiment should now prove whether embedded control is
cleaner for startup, config, permissions, session access, and diagnostics.

## Permission And Tool Policy

Agent Mode needs an explicit permission model because the product changes from
conversation to local action.

The default policy should be conservative:

- show the user what tool/action is about to run;
- include the selected project folder context;
- support allow once, deny, and eventually allow for session/project;
- preserve enough input details for debugging;
- record permission decisions in the timeline/logs;
- make cancellation available while a run is active.

Direct Goose should be evaluated on whether it can pause at the right boundary,
surface a structured confirmation request, and resume deterministically after
Maple answers.

Maple should own the permission UX. Goose can provide the underlying policy and
confirmation hooks, but the user should experience it as a Maple decision point,
not a Goose terminal prompt or hidden runtime event.

## Timeline And Event Model

The frontend should render a Maple timeline, not raw Goose messages.

The timeline should preserve execution order across:

- assistant text;
- thinking;
- tool requests;
- permission requests;
- tool results;
- errors;
- usage/runtime metadata.

This matters because debugging an agent run depends on sequence. A user should
be able to answer:

- what did the agent decide to do;
- what did it ask permission for;
- what actually executed;
- what came back;
- what failed;
- what final answer used that information.

Direct Goose events should be mapped into stable Maple events at the Tauri
boundary. If Goose later changes event shapes, Maple should update the adapter,
not the UI product model.

## V1 Scope

The first direct-Goose Agent Mode experiment should be intentionally local and
desktop-only.

In scope:

- Maple Desktop only.
- User chooses a local project folder.
- Tauri starts and owns the embedded Goose runtime.
- Tauri starts Maple's local proxy when needed.
- Tauri creates or reuses a local proxy API key automatically.
- Goose uses Maple's local OpenAI-compatible proxy and Maple's selected model.
- Goose developer tools can inspect and operate in the selected folder.
- The UI shows thinking, assistant text, tool calls, tool results, permission
  prompts, failures, runtime status, and enough diagnostics to debug failures.
- Sessions can be listed, loaded, and resumed if Goose's session APIs make that
  practical.

Out of scope for V1:

- mobile remote control;
- remote session sync;
- enclave-backed agent state;
- multiple desktop device coordination;
- external ACP runtimes;
- Goose configuration screens for normal users;
- web, iOS, or Android Agent Mode;
- merging agent sessions into normal Maple chat history.

## UX Direction

The Codex screenshots are useful as a structural reference, not as a visual
style reference.

Useful structural ideas:

- Agent Mode has its own top-level tab.
- Projects are folders.
- Sessions/chats are grouped under projects.
- Runtime/device context is visible.
- The composer is scoped to the active project/session.
- Search and session navigation are part of the agent surface.

Things Maple should not copy:

- Codex's exact visual design;
- Codex's exact navigation layout;
- Codex's session storage assumptions;
- a generic developer-dashboard look.

Maple Agent Mode should look and feel like Maple. It should reuse Maple's
existing visual system where possible:

- sidebar patterns;
- row/list treatments;
- composer style;
- dialogs and dropdowns;
- buttons and badges;
- markdown rendering;
- dark/light theme behavior;
- spacing, typography, and interaction rhythm.

At the same time, Agent Mode should not overload existing chat objects. It is
mostly a greenfield product area with reused Maple components.

## Maple UI Constraints

The product should borrow structure from Codex and Goose Desktop, but the visual
language should stay Maple-native.

Keep:

- Maple's existing sidebar density, typography, spacing, and color behavior.
- Maple's existing chat composer feel where it fits the agent workflow.
- Maple's existing button, badge, dropdown, dialog, and markdown treatments.
- Maple's existing light/dark theme rules.
- Maple's existing project-related visual vocabulary where it is useful.

Do not combine:

- normal Maple chat sessions and agent sessions;
- server-side Maple tools and local Goose/developer tools;
- Maple cloud projects and local filesystem agent projects, unless a later
  product design deliberately links them;
- Maple chat model selection and Agent Mode runtime configuration;
- normal chat message history and local execution timelines.

Do not copy:

- Codex's exact visual style;
- Goose Desktop's exact UI;
- generic IDE or developer-dashboard patterns that clash with Maple.

The intended result is a new Maple section that feels like it belongs in the
same app, while clearly signaling a different trust and execution model.

## Suggested Page Shape

A useful eventual information architecture is:

```text
Agent Mode
  Home / project browser
    device/runtime filters
    filesystem projects
    sessions grouped under projects
    search

  Project session view
    project folder header
    runtime status
    session title
    timeline
    permission prompts
    composer
    model/mode controls

  Diagnostics / logs
    runtime logs
    LLM request/response logs
    tool call records
    session JSONL
```

V1 can open directly into the active project/session view if that is faster, but
the data model should not block an eventual project/session browser.

## Timeline Model

The timeline is the core Agent Mode artifact. It should make the agent's work
legible.

Timeline item types should include:

- user message;
- assistant message;
- thinking block, collapsed by default;
- tool call;
- tool result;
- permission request;
- error;
- usage/runtime metadata.

Tool calls should appear in execution order, not grouped at the bottom. A tool
row should show:

- tool name;
- human-readable summary;
- status;
- input;
- output;
- failure details;
- raw diagnostic payload where available.

Thinking should be available but not visually dominant. It should be collapsed
by default and expandable for debugging or power users.

## Runtime Boundary

Tauri should own the runtime lifecycle.

Responsibilities on the Tauri side:

- start and stop embedded Goose;
- configure Goose for the selected project root;
- configure Goose to use Maple's local proxy;
- ensure the local proxy is running;
- ensure a local proxy API key exists;
- create/list/load/delete sessions;
- send prompts;
- cancel active runs;
- route permission requests and responses;
- map Goose events into Maple events;
- write runtime, session, tool, and LLM logs.

Responsibilities on the TypeScript side:

- render Maple Agent Mode UI;
- call Maple-owned Tauri commands;
- listen for Maple-owned events;
- maintain optimistic UI state where appropriate;
- keep normal web, iOS, and Android builds unaware of desktop-only runtime APIs.

TypeScript should not:

- open a WebSocket directly to a local Goose server for the built-in runtime;
- know the local Goose port or URL;
- read or write Goose config files;
- depend on Goose Rust type names;
- depend on ACP event shapes for the built-in path.

## Direct Goose Versus ACP

Direct Goose is the better default for Maple's built-in Agent Mode because Maple
owns the desktop process, local proxy, selected folder, and UI. Embedding Goose
lets Maple control startup, configuration, permissions, logging, cancellation,
and event mapping without supervising a separate first-party daemon.

ACP remains valuable when Maple does not own the runtime:

- Codex;
- Claude Code;
- a user-installed Goose daemon;
- future ACP-compatible tools;
- possible remote/runtime interoperability cases.

The product boundary should therefore be:

```text
Maple Agent Mode API/events
  Runtime adapter interface
    Direct Goose adapter
    ACP adapter later
```

Both adapters should feed the same Maple timeline and session model.

## Goose And GDK Direction

Spiral's July 7, 2026 Goose/GDK announcement supports this direction. The post
describes Goose evolving from a single application into a development platform
for many agentic applications. It also describes two integration paths:

- ACP for daemon/process-style integrations.
- A lower-level Rust API for embedded, fine-grained control.

That maps directly onto Maple's desired architecture:

```text
Built-in Maple Agent Mode
  use embedded Rust/GDK-style control

External agent support
  use ACP
```

The announcement also frames the Goose desktop app as a reference client. For
Maple, that means the Goose reference client is useful for API and interaction
research, but it should not become Maple's visual design.

## Goose Reference Client Research

Goose's reference desktop/client implementation should be treated as a live
example of how the Goose team expects GDK capabilities to be composed.

Research it for:

- how it constructs and owns agents;
- how it configures providers and models;
- how it initializes sessions and resumes them;
- how it maps Goose messages into UI events;
- how it represents tool calls, tool results, and failures;
- how it handles permissions and confirmations;
- how it stores and retrieves session history;
- how it exposes logs and diagnostics;
- how it separates runtime concerns from UI state.

Do not treat it as:

- Maple's design system;
- Maple's information architecture;
- the final answer for Maple's session model;
- a reason to expose Goose-specific nouns to Maple users.

The practical goal is to learn the cleanest internal Goose/GDK entrypoints and
then hide them behind Maple's runtime adapter.

The reference client should answer product and API questions at the same time.
Concrete things to inspect:

- What is the first object created when a user starts a new agent session?
- Does the client treat folder/project selection as a Goose concept, an app
  concept, or both?
- Which API owns the session list shown in the UI?
- Does the UI reconstruct timelines from stored sessions, live events, or both?
- How are tool request rows correlated with tool result rows?
- How are failed tool calls represented when the tool never executes?
- Where are permission prompts generated, and what object resumes the run?
- Are provider/model settings written to global Goose config, per-session config,
  or passed directly into agent construction?
- Which logs are emitted by Goose automatically, and which are app-level logs?
- Which parts look like stable GDK direction versus reference-app glue?

Maple should document the answers as implementation notes before committing to a
shipping integration. The goal is not to reverse engineer Goose Desktop; it is
to avoid building against the wrong internal layer if the Goose team is already
moving that layer into GDK.

## Current Goose API Reality

The current published `goose-sdk` crate does not yet appear to expose the full
agent/session/tool runtime Maple needs.

The usable direct runtime surface is in the parent `goose` crate, including
internal APIs around:

- `goose::agents::Agent`;
- `goose::agents::AgentConfig`;
- `goose::agents::AgentEvent`;
- `goose::agents::SessionConfig`;
- `goose::session::SessionManager`;
- `goose::config::permission::PermissionManager`;
- providers and OpenAI-compatible provider configuration;
- platform extensions and developer tools.

For the experiment, using the Goose submodule and parent crate is acceptable.
For a real shipping integration, Maple should push toward a stable GDK/Rust API
surface that covers the same needs.

The important constraint is isolation: direct Goose calls should live behind a
Maple-owned Rust adapter so future Goose/GDK API changes do not leak into the
frontend or product model.

## Goose Direct API Areas To Validate

The direct experiment should explicitly validate the following Goose areas.

### Agent Construction

Questions:

- Can Maple construct an agent without relying on the Goose binary?
- Can Maple pass provider/model/session/tool configuration directly?
- Which configuration still has to be written to Goose global files?
- Can multiple sessions be active or resumable without global runtime conflicts?

Desired outcome:

```text
Maple Tauri creates a Goose agent in process with explicit config, explicit
project root, explicit provider, and explicit session identity.
```

### Provider And Proxy Setup

Questions:

- Can Goose use Maple's local OpenAI-compatible proxy through direct provider
  construction?
- Can the API key be supplied by Maple without writing it into Goose secrets?
- Can model selection be changed per session or per run?
- Which provider options are only available through config files today?

Desired outcome:

```text
Maple owns proxy startup and local API key creation. Goose receives a normal
OpenAI-compatible provider config pointed at localhost.
```

### Session Storage And Retrieval

Questions:

- Can Goose create, list, load, resume, delete, import, and export sessions
  through public Rust APIs?
- Does the session API expose enough metadata for a project/session browser?
- Can Maple choose the session root directory?
- Can Maple safely build a lightweight index on top without racing Goose?

Desired outcome:

```text
Goose owns resumable execution transcripts. Maple owns the product index and UI
metadata.
```

### Tool Lifecycle

Questions:

- Which direct event identifies a tool request?
- Which event identifies the matching tool result?
- Are tool-call IDs stable enough to coalesce request/result rows?
- Are failed tool-call parses represented before execution?
- Are raw inputs/outputs available for diagnostics?

Desired outcome:

```text
Maple can show one ordered row per tool action with status, summary, input,
output, and failure details.
```

### Permissions

Questions:

- Can Goose pause a pending tool action and wait for Maple's answer?
- Can Maple distinguish allow-once, deny, and future broader grants?
- Are permission requests structured enough for a user-facing UI?
- Can permission decisions be logged with session/run/tool IDs?

Desired outcome:

```text
Maple owns the permission prompt. Goose owns enforcement and resumes only after
Maple responds.
```

### Cancellation

Questions:

- Does cancellation stop only the active run, or the whole agent/session?
- Are in-flight tool calls cancellable?
- Does cancellation leave the session in a resumable state?
- What events are emitted after cancellation?

Desired outcome:

```text
The user can stop a run from Maple UI and then continue using the same session.
```

### Logs And Diagnostics

Questions:

- Where does Goose write runtime logs by default?
- Can Maple choose log locations under its app config/log directory?
- Can raw provider request/response logs be enabled without patching Goose?
- Can tool failures be correlated to provider responses and session events?

Desired outcome:

```text
When something breaks, Maple can point a developer or power user to local logs
that explain whether the failure came from proxy startup, provider output,
Goose parsing, tool execution, permission denial, or UI mapping.
```

## Goose Internal API Research Plan

Before treating direct Goose as production architecture, inspect and document
the exact APIs Maple uses in these areas:

- agent construction and lifecycle;
- provider setup;
- model selection;
- local proxy configuration;
- session creation, listing, loading, deletion, import, and export;
- event streaming;
- message/content mapping;
- tool call lifecycle;
- built-in developer tools;
- permission manager and confirmation flow;
- cancellation;
- context management;
- memory hooks;
- logging and tracing;
- error surfaces.

For each area, record:

- the specific Goose modules/types/functions used;
- whether the API is public, semi-public, or internal;
- whether it touches Goose global config or global state;
- whether Maple can scope it to an app-owned config directory;
- the Maple command/event shape that hides the detail;
- the GDK ask that would make the integration cleaner.

This research should be aimed at producing clear, actionable feedback for the
Goose team.

## Configuration Principles

The default Maple-powered Agent Mode path should not expose Goose configuration
ceremony to normal users.

Entering Agent Mode should:

- auto-start the Maple proxy if needed;
- auto-create a local proxy API key if needed;
- configure Goose to use the local Maple proxy;
- select the requested Maple model;
- scope execution to the selected project folder;
- choose Maple-owned config, session, and log paths.

Advanced users can eventually configure external runtimes or ACP providers, but
that should be separate from the default built-in Maple runtime.

## Logs And Diagnostics

The ACP PoC showed that diagnostics are a product requirement, not a developer
nice-to-have.

Agent Mode should make it possible to debug:

- runtime startup;
- proxy startup;
- local API key creation;
- raw LLM requests;
- raw LLM streaming responses;
- tool call parsing;
- tool execution;
- permission decisions;
- Goose session events;
- frontend event mapping.

Logs should be local, discoverable, and tied to session/run IDs where possible.
If a tool call fails before execution, the UI and logs should make that clear.
If a model emits malformed tool-call arguments, the raw upstream response should
be recoverable.

## Remote Control Later

The direct-Goose V1 should not include remote mobile control, but it should not
paint Maple into a corner.

A later version could add:

- encrypted session metadata sync through the enclave;
- desktop device registration;
- mobile-to-desktop message relay;
- mobile readback of assistant/tool progress;
- remote permission prompts;
- remote cancellation.

Possible future approaches:

- encrypted KV/S3-style session backup;
- dedicated backend tables for agent sessions/runs;
- polling from desktop and mobile through the enclave;
- an authenticated SSE/WebSocket relay between devices.

This is deliberately out of scope for V1. The only V1 requirement is that the
local product model can later be synchronized or mirrored.

## Implementation Principles

The experiment should follow these principles:

- Keep Agent Mode separate from normal Maple chat.
- Reuse Maple components and style, not chat data structures.
- Keep desktop-only code behind platform guards.
- Keep Goose behind a Rust adapter.
- Keep TypeScript runtime-agnostic.
- Auto-initialize the proxy and local proxy API key.
- Prefer direct Goose for the built-in runtime.
- Preserve ACP as a future external runtime adapter.
- Use the Goose submodule only because the stable GDK surface is not ready yet.
- Document every Goose internal API dependency.
- Do not copy Codex or Goose Desktop visually.
- Do not expose Goose config ceremony in the default flow.

## V1 Acceptance Criteria

A successful first experiment should prove:

- Agent Mode is visible only on Maple Desktop.
- A user can select a local project folder.
- Maple starts the local proxy automatically.
- Maple creates or reuses a local proxy API key automatically.
- Tauri starts embedded Goose without an external Goose binary.
- Goose uses Maple's local proxy and model.
- A user can send a prompt from the Maple UI.
- Goose can inspect the selected project through developer tools.
- Tool calls appear in order with useful summaries.
- Permission prompts are shown in Maple UI and can be answered there.
- Thinking is collapsed by default but inspectable.
- The final assistant answer renders in Maple style.
- Runtime, session, tool, and LLM logs are available locally.
- Web, iOS, and Android builds do not load or render Agent Mode.

## Open Questions

These should be answered during the direct-Goose experiment:

- Should clicking Agent Mode open a home/project browser or the last active
  project session?
- How should Maple derive session titles?
- Should Goose session storage be the source of truth, or should Maple maintain
  its own session index?
- Which Goose APIs still require global config writes?
- Can provider setup avoid persistent Goose secrets/config files?
- Can developer tools be enabled fully in process without any Goose binary path?
- What permission policy should be the default for Maple users?
- What exact API surface should Maple ask the Goose/GDK team to stabilize?
- How should diagnostics be exposed in the UI without overwhelming normal users?

## Conclusion

Agent Mode should be Maple's local project/workspace agent: a separate desktop
surface, powered by embedded Goose, presented through Maple-owned UX, state
objects, permissions, logs, and runtime controls.

Direct Goose is the right default path for the built-in Maple agent because it
matches the product need for embedded control. ACP is still important, but as a
future adapter for external runtimes rather than the core path for Maple's own
desktop agent.

The immediate goal is to validate the product boundary:

```text
Maple UX
  -> Maple Agent Mode API/events
    -> direct Goose adapter
      -> Goose runtime
        -> Maple proxy
```

If that boundary feels clean, the long-term work is to replace internal Goose
submodule calls with the stable GDK/Rust API as it becomes available.

The ACP PoC should stay available for comparison and future external-runtime
work. The direct-Goose workspace should now be used to explore the embedded GDK
path, Goose internal APIs, and Maple-native Agent Mode product surface without
starting from the assumption that ACP is the internal bridge.

The UI work should be treated as mostly greenfield product development inside
Maple's existing design language. Reuse Maple components and patterns where
they help, but keep projects, sessions, timelines, permissions, and runtime
state as Agent Mode concepts.
