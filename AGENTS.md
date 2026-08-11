# Maple agent guide

This file applies to the entire repository. It defines the durable rules an
agent should know before changing Maple. Task-specific procedures live under
`.agents/skills/`; load the matching skill before doing that work.

## Start here

1. Confirm the checkout, branch, and worktree state. Preserve unrelated user
   changes and do not switch branches or rewrite history in a dirty checkout.
2. Read the relevant source and tests before proposing placement. Do not treat
   historical design documents as stronger evidence than current code.
3. Before creating configuration or starting or stopping services, determine
   whether an external development environment owns this checkout's ignored
   environment files, local Tauri overlay, ports, or processes. Preserve those
   resources and follow that environment's lifecycle instructions. Standalone
   setup and startup examples apply only when no external orchestrator owns the
   affected resource.
4. Enter the pinned environment with `nix develop`. Install dependencies with
   `just install` and enable the repository hook with `./setup-hooks.sh`.
5. If `frontend/.env.local` is absent, create it with
   `test -e frontend/.env.local || cp frontend/.env.example frontend/.env.local`.
   Never overwrite an existing file: it may be externally managed or contain
   checkout-specific backend and application configuration. Record the
   selected OpenSecret, feature-flags, and billing endpoints in smoke-test
   evidence; never put secrets in `VITE_*` variables.
6. Choose the runtime you actually need: `just dev` is web-only;
   `just desktop-dev` is the Tauri application and is required for Agent Mode
   and other native behavior.

## Product and runtime map

Maple is a React/Vite application packaged with Tauri for desktop and mobile.
OpenSecret is its required backend. Keep these runtime paths distinct:

- Research chat: React -> OpenAI JavaScript client ->
  `@opensecret/react` encrypted custom fetch -> OpenSecret Responses and
  Conversations APIs.
- Desktop Agent Mode: React -> Maple-owned Tauri commands/events ->
  `MapleAgentService` -> embedded pinned Goose -> `MapleProvider` -> Rust
  OpenSecret SDK -> `/v1/chat/completions`.
- ACP: an external client edge over a protected local socket into the same
  Maple Agent service. ACP is not Agent Mode's internal abstraction.
- Local proxy: a separate user-facing OpenAI-compatible relay. Research chat
  and Agent Mode do not internally route through it.

The browser client and native Agent Mode use independently pinned OpenSecret
SDKs. Do not assume their versions, transports, retries, or API coverage are
identical. A backend contract change that Maple consumes needs compatibility
checks for every affected client path.

## Code ownership and placement

- `frontend/src/routes`, `components`, `contexts`, and `state` own routing,
  presentation, account-scoped UI state, drafts, and interaction behavior.
- `frontend/src/services` owns browser-side API/Tauri bridges and lifecycle
  orchestration. It is not a place to reimplement backend authorization.
- `frontend/src-tauri/src` owns privileged device behavior: filesystem and
  process access, native networking, local listeners, deep links, PDF/OCR,
  credential-bearing native clients, and OS integration.
- `frontend/src-tauri/src/agent.rs` and `agent/` own the transport-neutral Agent
  runtime, provider adapter, developer tools, permissions, trusted project
  skills, and system-prompt policy. Keep public TypeScript contracts
  Maple-owned; do not leak Goose, RMCP, or ACP types through the Tauri API.
- OpenSecret owns authentication and authorization truth, encrypted
  persistence, provider credentials and routing, model canonicalization,
  protected-route enforcement, and inference-usage capture. Maple may present
  billing and flags API state, but it is not authorization or accounting truth.

Prefer the narrowest existing layer. If a change crosses React, Tauri, an SDK,
and OpenSecret, write down the contract at each boundary before editing.

For every added or changed privileged Tauri command:

1. Put the local privileged effect in `frontend/src-tauri/src` behind a narrow
   Rust command. Treat its renderer arguments as untrusted and enforce account,
   canonical path, allowed-root, file-type, size, and lifecycle constraints at
   the native authority boundary as applicable.
2. Register the command in every intended platform's `generate_handler!` list,
   preserve the surrounding `cfg` gates, and verify it is not exposed on an
   unintended target.
3. Put the typed renderer bridge in `frontend/src/services` and use an explicit
   platform guard. Components should consume that bridge instead of growing a
   second native contract.
4. Inspect `frontend/src-tauri/Cargo.toml`, plugin initialization,
   `frontend/src-tauri/tauri.conf.json`, CSP, and
   `frontend/src-tauri/capabilities/*.json` separately. Plugin permissions and
   application-defined Rust commands are different authority paths. With the repository's default
   Tauri build configuration, `generate_handler!` registration is the exposure
   boundary for a custom command used by local WebViews; plugin capability
   entries do not narrow that access. Filesystem plugin scopes constrain
   filesystem plugin calls; they do not constrain `std::fs`, Tokio filesystem,
   or other local effects performed by a custom Rust command.
   Enforce a custom command's scope in Rust, and do not broaden a plugin
   permission unless the renderer directly needs that plugin API.
5. Add focused tests for native validation and the typed caller. Where no
   checked-in React-to-IPC integration test exists, manually exercise the exact
   desktop app from UI entry point through IPC to the native effect, including
   a representative rejection case.

## Security and privacy invariants

- Treat the WebView and all Tauri command arguments as untrusted. Revalidate
  account identity, paths, URLs, sizes, ports, enum values, and lifecycle
  ownership in Rust before a privileged effect.
- Never log access or refresh tokens, API keys, raw deep-link URLs, prompts,
  response contents, MCP headers/environments, or decrypted backend payloads.
  Sanitize native errors before emitting them to the renderer.
- Credential-bearing backend URLs must be HTTPS, except explicit loopback HTTP
  in development. Reject embedded credentials, unexpected paths, query
  strings, and fragments.
- Preserve account isolation. Every user-sensitive runtime, cache, file,
  pending operation, event, and query key needs an account owner or opaque
  account scope. After every `await`, late work must prove it still owns the
  current account/session/run before publishing state.
- Account transition and secure logout must stop and drain Agent/ACP, stop and
  scrub the local proxy, clear billing session tokens, clear native auth, and
  dispose account-owned UI state in the established order. Do not add a direct
  sign-out shortcut that bypasses cleanup.
- Project roots grant context and trust; they are not filesystem containment.
  Read-only Agent mode is a consent policy, not an OS sandbox.
- Permission grants are one-use capabilities bound to the exact account,
  session, run, calling surface, request, and payload. Unknown, duplicate,
  late, cancelled, or cross-surface responses fail closed.
- Shell/web permission classifiers fail closed. Cancellation, revocation,
  timeout, and output overflow must terminate spawned process groups and
  revoke credential-bearing tool contexts before another launch.
- STDIO MCP is intentional arbitrary local-code execution. Saving a definition
  is inert; enabling it launches the executable. Never pass its command through
  a shell. Treat persisted MCP configuration as sensitive; verify storage,
  fallback, and deletion guarantees from the implementation.
- Keep local-proxy defaults loopback-only, CORS-off, and auto-start-off. Browser
  reachability and saved-key fallback must remain mutually exclusive.
- Model output and remote Markdown are untrusted. Preserve sanitization,
  external-link confirmation, CSP, Tauri capability, and opener restrictions.
- `VITE_*` is public build-time configuration. Provider credentials and
  administrative billing/flags credentials never belong in Maple.

For auth, proxy, Agent tooling, native capabilities, filesystem or process
access, deep links, or persistence, load `$review-maple-security` before
changing code. Keep resulting findings in the task's review output or another
explicitly authorized destination. Keep only durable security standards and
review methodology in this guide and its skills.

## Development conventions

- Use the versions and platform dependencies pinned by `flake.nix`; do not add
  an alternate toolchain bootstrap to project docs or CI.
- Use Bun from `frontend/`; use Cargo from `frontend/src-tauri/`.
- Prefer repository desktop recipes because they provision the pinned ONNX
  Runtime. `just desktop-dev` applies an active local Tauri config overlay;
  standard desktop build recipes retain the standard application identity.
  Use `just desktop-build-debug-overlay` when an unsigned, overlay-configured
  package is required.
- Use `just clean-local`. Raw `cargo clean` may erase a shared Nix Cargo build
  directory used by other checkouts.
- Follow existing React, TypeScript, Rust, error, test, and accessibility
  patterns in the nearest code. Do not perform drive-by migrations.
- Preserve explicit cancellation and ownership tokens in concurrent code.
  Navigation, the currently selected chat, and component lifetime are not
  sufficient ownership proofs for background streams.
- Clean SSE iterator EOF without the protocol's terminal event is truncation,
  not success. Retry cleanup removes only state created by that attempt.
- A Goose dependency bump requires re-diffing Maple's system prompt and
  reviewing the intentional prompt-drift test.

Do not edit generated files by hand, including
`frontend/src/routeTree.gen.ts`. Regenerate platform projects or lockfiles with
their repository workflow, inspect all generated deltas, and never hide them
with `git update-index --assume-unchanged`.

## Validation is proportional evidence

Start focused, then run the complete gate for the layers changed:

```bash
# Frontend format, lint, typecheck, and Bun tests
nix develop .#ci -c ./scripts/ci/frontend.sh

# Locked Rust tests for all targets (with Linux ONNX provisioning)
nix develop .#ci -c ./scripts/ci/rust.sh

# Rust formatting and strict Clippy when Rust changed
nix develop .#ci -c just rust-lint

# PR-shaped web artifact when web build/config changed
MAPLE_WEB_ENVIRONMENT=pr nix develop .#ci -c ./scripts/ci/web.sh

# Flake/toolchain/workflow metadata; this does not run application tests
nix flake check
```

The pre-commit hook is useful but is not full CI parity. Unit tests, a web
build, a native package build, and a GUI smoke test are different evidence.
This repository has no general checked-in browser, packaged-app, or
React-to-Tauri-command integration harness; never claim those checks prove
runtime integration. A privileged IPC change requires a manual exact-app smoke
through the UI, command, native validation, and resulting local effect.

PR artifact scripts deliberately ignore local `.env*` files and compile fixed
PR endpoints. They prove PR packaging, not a configured local-backend runtime.
For the latter, preserve the checkout's `frontend/.env.local`, use
`just desktop-dev`, and record its active overlay, compiled endpoints, exact
executable or application identifier, and backend identity.

For a smoke test, state exactly what was exercised and record the commit SHA,
configuration profile, compiled API/flags/billing endpoints, executable or
`.app` path and bundle identifier, backend identity, account class, and any
unverified boundary. Web smoke does not cover Tauri or Agent Mode. Native build
jobs produce packages but do not launch them. Use `$validate-maple` for the
change-to-evidence matrix and full-stack smoke procedure.

## External services and full-stack work

- Run OpenSecret using that repository's own instructions and migrations, then
  point `VITE_OPEN_SECRET_API_URL` to it. Do not copy backend secrets into
  Maple.
- Feature flags and billing are independent API clients. Configure their dev
  or production URLs as appropriate. A working chat does not prove that a
  flag-gated or billing-gated feature works.
- Keep local OAuth/verification callbacks on the expected Vite origin unless
  deliberately changing and testing the backend callback contract.
- Use deterministic unique-marker prompts and assert exact results where
  possible. Do not commit test identities, tokens, response captures, or other
  live-service evidence.

## Review and authority

Review for behavior, accessibility, failure recovery, account isolation,
security boundaries, and honest test coverage—not only for compilation. Cite
source evidence for security claims and distinguish code-confirmed facts from
deployment configuration or live-environment observations.

Routine development never authorizes releases, signing, store submission,
deployment, or changes to live services. Never push to `master` as a validation
step: a master push starts production-shaped signed builds and can upload iOS
artifacts to TestFlight. Creating a GitHub Release triggers release builds and
downstream publication. Use `$release-maple` only when the user explicitly
requests release work, and report the tag and commit before publishing.

## Skills

- `$develop-maple`: setup, placement, implementation loop, and common web,
  desktop, mobile, and backend-integration work.
- `$validate-maple`: focused tests, CI parity, exact-app smoke, full-stack
  smoke, and evidence reporting.
- `$change-maple-agent-mode`: Agent Mode, ACP, Goose/provider, permissions,
  tools, MCP, trust, cancellation, and lifecycle work.
- `$review-maple-security`: security-sensitive design, implementation, or
  review across auth, accounts, native capabilities, proxy, deep links,
  persistence, Agent Mode, and rendering.
- `$release-maple`: version preparation, release preflight, publication, and
  release-workflow verification. Production authority is required.

## Maintaining this guidance

Treat this guide and the repository skills as living operational documentation,
not infallible rules. Re-check prescriptive language against the current source,
tooling, and architecture. If guidance appears stale, materially wrong,
unnecessarily absolute, or repeatedly creates development friction, surface the
mismatch and confirm the intended correction with the user before changing it.
Do not churn guidance for stylistic preferences or isolated nits; do narrow
words such as "always" or "never" when they claim more than the invariant
actually requires.

Update the relevant guide or skill in the same branch when a material change
adds, removes, or rearchitects a workflow, ownership boundary, validation path,
or recurring development procedure. If you discover unrelated drift, keep the
current task scoped and propose a standalone change with evidence and reasoning.
Add a new skill when a genuinely reusable workflow does not fit an existing one;
otherwise prefer improving or consolidating current guidance. Ground every
update in current source or executed workflow experience, keep it concise, and
validate every command or path it prescribes.
