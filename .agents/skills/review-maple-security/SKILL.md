---
name: review-maple-security
description: Review Maple changes and current behavior across authentication, account isolation, local persistence, Tauri IPC and capabilities, OAuth and deep links, the Local OpenAI Proxy, Agent Mode tools and permissions, MCP, ACP, streaming concurrency, secrets, logging, and configurable API endpoints. Use for security audits, threat modeling, security-sensitive code review, regression assessment, or implementation planning when a Maple change crosses a browser, native, filesystem, process, local-network, account, or OpenSecret API trust boundary.
---

# Review Maple Security

Review the current source, not remembered findings. Separate policy, consent, containment, and server authorization. Treat Maple as an open-source client of OpenSecret; discuss billing and feature flags only through their public API/configuration contracts.

Default to read-only investigation when asked to review, audit, diagnose, or threat-model. Do not implement, commit, push, open issues, or make incident claims unless the user separately authorizes the relevant action and the evidence supports it.

## Establish the Review Baseline

1. Read `AGENTS.md`, the requested diff or files, nearby tests, and applicable docs.
2. Record `git status --short --branch`, `git rev-parse HEAD`, the comparison base, and whether the checkout has unrelated changes. Never clean or rewrite them.
3. Re-check current command and dependency sources: `justfile`, `frontend/package.json`, `frontend/src-tauri/Cargo.toml`, lockfiles, `scripts/ci/`, and `.github/workflows/`.
4. Identify whether the request is:

   - a read-only audit of current behavior;
   - a review of a bounded diff;
   - a proposed security design; or
   - validation of an implemented fix.

5. State the assets, attacker position, trust boundaries, and deployment assumptions before assigning severity.

Do not infer deployment state from repository source. Do not infer runtime behavior from a passing build. Do not infer authorization from hidden UI or a feature flag.

## Use an Evidence Taxonomy

Label every material claim with the strongest evidence actually obtained:

- **Source-confirmed:** current checked-out code directly implements the behavior.
- **Test-confirmed:** a focused or full automated test exercised the behavior.
- **Runtime-confirmed:** the exact web or native application demonstrated it locally.
- **Deployment-confirmed:** the relevant deployed service, policy, or artifact was inspected.
- **Inferred risk:** the consequence follows from confirmed primitives, but the full exploit path was not demonstrated.
- **Unverified:** a required dependency, platform, deployment, or attacker precondition was not inspected.

Call something an incident, compromise, or production exposure only with live evidence. A source-confirmed risky primitive is not proof that it was exploited or deployed insecurely.

For each finding, report:

1. Severity and confidence.
2. Exact source or runtime evidence.
3. Required attacker access and preconditions.
4. User, confidentiality, integrity, availability, or billing consequence.
5. Existing mitigations and why they do or do not close the path.
6. The narrowest responsible fix boundary: Maple frontend, Maple native Rust, OpenSecret backend, or coordinated API change.
7. Required regression and smoke evidence.

Omit speculative findings that cannot survive this structure. Record important
unknowns as task-local unverified boundaries. Keep findings in the current
task's review output or another destination the user explicitly authorizes.
Keep only durable security standards and review methodology in this skill and
`AGENTS.md`.

## Map the Trust Boundaries

Trace data and authority across every boundary touched by the change:

```text
user/browser or WebView
  -> React state, storage, and OpenSecret SDK
  -> Tauri IPC and plugin capabilities
  -> native Rust state, filesystem, keyring, processes, and listeners
  -> OpenSecret API and other configured public API endpoints

Agent prompt or remote content
  -> Maple permission policy
  -> Goose tool request
  -> Maple developer tool, MCP process/server, or ACP caller
  -> local files, shell, network, and credentials
```

Treat renderer input, model output, MCP output, ACP input, remote content, persisted JSON, URLs, paths, and environment values as untrusted at their receiving boundary. Validate again where authority is exercised.

## Review Authentication and Account Lifecycles

Inspect these source areas when authentication, user state, native providers, logout, deletion, or account switching is relevant:

- `frontend/src/services/mapleApiAuthService.ts`
- `frontend/src/services/agentAuthLifecycle.ts`
- `frontend/src/services/agentOperationFence.ts`
- `frontend/src/services/agentRuntimeService.ts`
- `frontend/src-tauri/src/maple_api.rs`
- `frontend/src/components/RootRuntimeLayout.tsx`
- every logout, guest-upgrade, verification, and account-deletion path

Require these invariants:

- Verify token ownership against the backend before publishing native credentials for an expected user.
- Serialize native credential installation, refresh reconciliation, and clearing. Fence late refreshes with immutable client identity and generation.
- Bind long-lived native handles and in-flight operations to an opaque account scope plus revocable generation; never silently rebind them to a new account.
- Include user identity in query/cache keys and remount account-owned providers on transition.
- Drain old-account Agent, ACP, proxy, timers, requests, controllers, and object URLs before activating a new account.
- Retain failed cleanup targets so rapid A -> B -> C transitions cannot skip A or B.
- Perform required local credential cleanup before losing the authenticated context needed to identify the owner. Make rollback or fail-closed behavior explicit when cleanup fails.
- Avoid new direct `os.signOut()` calls. Route sign-out, guest conversion, verification transitions, and deletion through one reviewed lifecycle sequence.
- Treat `localStorage` and `sessionStorage` as sensitive client storage, not a native trust anchor or secure vault.

Exercise success, cancellation, failure, timeout, token refresh during validation, wrong-user tokens, repeated logout, rapid account switching, and app exit/update. Check every `await` for stale account ownership before subsequent mutation.

## Review Local Persistence and Secrets

Inspect where state is stored, who can read it, when it roams, and how precisely it is deleted.

- Key user-sensitive Agent config, sessions, recent roots, trust decisions, caches, and runtime data by account scope.
- Keep device/path-specific state in app-local-data when roaming it could affect another device.
- Treat a hashed user ID as namespacing, not authorization or encryption.
- Treat Unix mode `0600` files and `0700` directories as defense in depth, not encrypted storage.
- Treat persisted MCP environment values, HTTP headers, proxy keys, and other
  credentials as sensitive. Verify storage, fallback, and deletion guarantees
  from the implementation; do not infer encryption from file permissions or
  opaque serialization.
- Prefer secure platform credential storage where the product contract requires it. Specify fallback and hard-clear behavior; do not silently report success while a stale credential remains.
- Write credential-bearing files through a restrictive same-directory temporary file, sync it, and atomically replace the destination. Avoid write-then-chmod windows.
- Scope history clearing, session deletion, and account deletion to the exact target. Preserve unrelated account data and unrelated app configuration.
- Re-check canonicalization, symlink behavior, traversal, file type, file size, and deletion boundaries for every renderer- or agent-controlled path.

Never include real tokens, passwords, prompts, private paths, encrypted user seeds, MCP secrets, or full credential-bearing URLs in tests, logs, screenshots, or reports. Use explicit non-secret fixtures.

## Review Tauri and Native Authority

Inspect:

- `frontend/src-tauri/src/lib.rs`
- `frontend/src-tauri/tauri.conf.json`
- `frontend/src-tauri/capabilities/*.json`
- every added or changed `#[tauri::command]`
- every intended platform-specific `generate_handler!` registration and its
  `cfg` gates
- plugin dependency and initialization changes
- renderer `@tauri-apps/*` imports and raw `plugin:*` calls
- the typed frontend service, caller, and native implementation together

Require native validation for all renderer-controlled accounts, paths, URLs, ports, sizes, enum values, identifiers, and lifecycle capabilities. Frontend validation is user experience, not enforcement.

Ask whether the change:

- expands the invoke handler, plugin set, CSP, `connect-src`, opener allowlist, filesystem scope, dialog access, deep-link handling, or mobile capabilities;
- exposes secrets in command responses, errors, emitted events, logs, argv, or process environments;
- permits stale or cross-account native handles;
- holds lifecycle locks across the right async boundaries and awaits shutdown before restart;
- preserves desktop/mobile compile gates;
- validates native URLs and sockets rather than trusting a frontend-created configuration; and
- changes a privileged behavior without exact-app runtime proof.

Keep security-sensitive local effects in native Rust: process launch, MCP connection, filesystem mutation, local listeners, credential persistence, deep-link ingestion, updater/restart, and account-bound native sessions. Minimize capabilities rather than compensating with renderer checks.

Application-defined Rust commands and plugin commands are separate authority
paths. Under the repository's default Tauri build configuration,
`generate_handler!` registration exposes a custom command to local WebViews
independently of plugin capability entries. A filesystem plugin scope constrains
filesystem plugin calls; it does not constrain `std::fs`, Tokio filesystem, or
another local effect inside a custom Rust command. Require custom commands to
canonicalize and bound paths, verify account and lifecycle ownership, reject
unintended file types and sizes, and sanitize results in Rust. Add plugin
permissions only when the renderer needs that plugin API, and never cite a
plugin scope as proof that a custom command is contained.

Require focused native-validation and typed-caller tests. When no checked-in
React-to-IPC integration test covers a privileged path, require a manual smoke
through the exact application, real UI entry point, intended command, native
effect, and a representative rejection case.

Treat untrusted Markdown and external URLs as a renderer boundary. Preserve sanitization before trusted transforms, suppress remote images unless intentionally supported, confirm external navigation, and review any sanitizer schema, raw HTML, KaTeX/highlight, CSP, or opener change together.

## Review Agent Mode, Tools, MCP, and ACP

Read the current implementation and docs together:

- `frontend/src-tauri/src/agent.rs`
- `frontend/src-tauri/src/agent/`
- `frontend/src-tauri/src/agent_acp.rs`
- `frontend/src-tauri/src/agent_host.rs`
- `frontend/src-tauri/src/agent_tauri.rs`
- `docs/agent-mode-mcp.md`
- `docs/agent-mode-acp.md`

Keep these concepts separate:

- **Permission policy:** which tool calls Maple or a caller consents to.
- **Containment:** which resources an allowed tool can actually access.
- **Credential scope:** which secrets an allowed process receives and for how long.
- **Surface ownership:** whether Desktop or an ACP caller owns events, approvals, and cancellation.

Do not describe Read-only/SmartApprove, allowed project roots, Unix socket
permissions, or process-group cleanup as an operating-system sandbox. A
project-root grant is not per-tool filesystem confinement; enforce and test
containment separately when it is part of the required policy.

Require these invariants:

- Keep Goose routed through action-required boundaries so Maple applies current policy to every tool call.
- Treat classifier input as untrusted. Fail closed on ambiguity, provider failure, timeout, invalid structured output, cancellation, secret reads, scripts, builds/tests, network access, or possible mutation.
- Bind every permission response to exact account generation, session, run, calling surface, request ID, and payload. Make IDs one-shot; reject unknown, duplicate, reused, late, cross-surface, disconnected, and cancelled responses.
- Apply a more restrictive mode before fallible asynchronous setup or persistence so one last permissive action cannot slip through.
- Keep Desktop and ACP events, approvals, status, and cancellation isolated. Do not make ACP runs actionable through the Desktop Tauri projection.
- Bound event queues and surface overflow as truncation/error; a slow client must not block runtime cleanup.
- Allowlist and bound tool-context environment keys and values. Scrub same-named ambient variables, linearize revocation with process launch, and revoke before future spawn.
- Treat STDIO MCP as arbitrary local code execution. Preserve executable-plus-argument tokenization without a shell. Saving may remain inert; enabling or connecting is the execution boundary.
- Freeze the complete MCP executable/endpoint/environment/header definition into the session snapshot. Do not retroactively send changed credentials to old sessions.
- Keep project skill discovery disabled until the canonical project root has an explicit trust decision. Fail closed when trust or the root cannot be loaded.
- Treat a same-user ACP endpoint as a local trust boundary, not independent application authentication. Explain which same-user processes can connect and what credentials/tools they can reach.

Preserve intended power features when they are product choices. Assess whether
their consent and containment match the documented threat model rather than
assuming intentional execution authority violates the design.

## Review the Local OpenAI Proxy

Inspect `frontend/src-tauri/src/proxy.rs`, `frontend/src/services/proxyService.ts`, the settings UI, all startup/logout callers, and tests as one boundary.

Require these defaults unless an explicit product decision changes them:

- bind to loopback;
- disable CORS;
- disable auto-start; and
- keep browser reachability separate from Maple's saved credential.

When CORS is disabled, verify browser-controlled `Origin` and `Sec-Fetch-Site` requests are rejected before a saved key can be spent. Omitting CORS response headers alone is insufficient. When CORS is enabled for browser compatibility, require every inference request to provide its own bearer key and remove saved-key fallback.

Review host, port, CORS mode, saved credential, auto-start, owner account, and backend URL as one security object. Validate bind hosts and endpoints natively. Allow plaintext HTTP only for explicit loopback development; reject embedded URL credentials and unexpected path, query, or fragment components unless the API contract requires them.

Serialize start, save, stop, reset, and auto-start. Await listener teardown before rebinding. Fence API-key creation and delayed startup with account identity plus auth generation so logout/reset cannot be followed by a stale completion that restarts an old account's proxy. Scrub the local listener and credential before best-effort remote key revocation.

Treat non-loopback service exposure as a separate feature requiring explicit authentication, transport security, network threat modeling, and billing-abuse analysis. A warning label alone is not native enforcement.

## Review OAuth and Deep Links

Inspect both sides of every flow:

- OAuth initiation and SDK callback handling;
- `frontend/src/routes/auth.$provider.callback.tsx`;
- `frontend/src/components/AppleAuthProvider.tsx`;
- `frontend/src/components/DeepLinkHandler.tsx`;
- native deep-link registration, forwarding, and single-instance handling.

Require exact scheme, authority, path, provider, parameter, and redirect validation. Bind completion atomically to a pending native-generated state, provider, PKCE verifier, expiry, and one-time use. Reject unsolicited, duplicate, stale, malformed, or cross-provider callbacks.

Never log raw callback URLs, single-instance argv, codes, access tokens, refresh tokens, state, nonce, or sensitive query strings. Prefer a short-lived one-use authorization code exchanged by the intended native client over bearer tokens in a custom URI. Treat custom schemes as interceptable on platforms where handler ownership is not cryptographically verified; prefer verified App/Universal Links when redesigning the flow.

Validate internal redirects independently and reject absolute, scheme-relative, backslash-confused, or normalized traversal forms. Do not treat safe navigation as proof that the authentication handoff itself is bound correctly.

## Review Endpoints and Attested Transport

Inspect the browser and native OpenSecret clients independently:

- `frontend/src/app.tsx`
- `frontend/src/ai/OpenAIContext.tsx`
- `frontend/src/services/mapleApiAuthService.ts`
- `frontend/src-tauri/src/maple_api.rs`
- `frontend/src-tauri/src/agent/provider.rs`
- the pinned JavaScript and Rust OpenSecret SDK versions and lockfiles

Preserve the OpenSecret SDK's encrypted and attested transport. Do not replace `aiCustomFetch` or the native SDK client with raw browser/native fetch merely to make a request work. Avoid layering automatic client retries over Maple operations that may have server-side effects; keep retry ownership explicit.

Review development and production enclave URLs and PCR/attestation allowlists together. Reject an endpoint that is not HTTPS unless it is an explicit loopback development address. Reject URL credentials and unexpected paths, queries, or fragments at the native authority boundary. Treat source-configured PCR values as policy, not proof that a live enclave currently matches; perform live attestation before making deployment claims.

Do not assume the browser and native SDK pins have identical features, request schemas, refresh behavior, attestation behavior, or error handling. For an OpenSecret API contract change, validate both clients. Keep decrypted upstream errors and response bodies out of logs and user-facing native errors; expose bounded categories unless safe detail is deliberately part of the public contract.

When backend confirmation is needed, inspect the current open-source OpenSecret repository and follow its `AGENTS.md` and relevant skills. Do not infer its authorization, transport, or deployment behavior solely from Maple's caller.

## Review Secrets, Logs, and Errors

Search both sides of a changed boundary for `console.*`, Rust `log::*`, string interpolation, Tauri events, structured errors, telemetry, clipboard flows, URLs, process arguments, and child environments.

- Log operation type, bounded identifiers, status, duration, and safe error categories—not tokens, API keys, auth codes, nonces, credential-bearing URLs, prompts, documents, model output, headers, environment values, or decrypted provider bodies.
- Assume error objects can contain request URLs, response bodies, headers, or SDK internals. Sanitize at the boundary before logging or returning them.
- Never forward a secret from renderer storage into argv or a process-global environment. Give an approved child only the exact bounded values needed for its lifetime.
- Avoid echoing sensitive native config back to an untrusted renderer when a redacted status or opaque handle is sufficient.
- Keep clipboard and external-navigation actions explicit and user initiated when content may contain credentials or private data.
- Test both expected failures and logging failures. Inspect captured logs and emitted events for absence of fixture secrets, not merely for presence of an error message.

Use unique fake canary values in tests so accidental propagation is detectable without exposing real credentials.

## Review Streaming and Concurrency

Inspect `ChatRuntimeContext`, `chatRuntimeStore`, `UnifiedChat`, stream coalescing, retry recovery, Agent event routing, and any affected async service.

Require every request, stream, timer, retry, load, cancellation, and delayed callback to capture immutable owner identity plus generation/run token. Re-check ownership after every `await` before mutation.

- Treat the selected chat as a projection, not the owner of background work.
- Flush attempt-owned buffered deltas while ownership is valid, clear ownership, then abort.
- Treat EOF without an explicit terminal event as truncation, not success.
- Remove only attempt-owned optimistic/response items during retry recovery.
- Reconcile ambiguous failures against server state before restoring input or resending.
- Rekey draft-to-conversation state atomically; never replace an active destination or strand destination-owned attachments, recording, composer state, or object URLs.
- Bind shared scroll and view state to a projection key/lease so delayed A -> B -> A work cannot mutate the wrong view.
- Dispose account-owned stores, controllers, timers, and object URLs on account transition.

## Choose Maple or OpenSecret

Keep in the open-source OpenSecret backend:

- token validation and account identity;
- authorization for persisted data and account-owned resources;
- entitlements, usage limits, billing enforcement, and API-key permissions;
- provider/model routing and server-authoritative response semantics; and
- one-time auth-code exchange or other server-enforced protocol guarantees.

Keep in Maple:

- presentation, accessibility, navigation, drafts, and client orchestration;
- account-keyed client caches and safe projection of server state;
- Tauri lifecycle and explicitly local OS integrations; and
- native enforcement for local files, processes, credentials, IPC, listeners, Agent tools, MCP, and ACP.

Do not use billing status, feature flags, hidden routes, or disabled controls as
authorization. Configure billing and feature flags as external dev/prod HTTP
API endpoints when a test needs them and assess only their public contracts.
Treat every `VITE_*` value as public build-time configuration, never a secret.

For coordinated changes, make the backend contract secure and backward-compatible first, then update Maple through the pinned OpenSecret SDK or a reviewed API adapter. Avoid duplicating backend authorization or provider behavior in the renderer.

## Validate in Proportion to Risk

Use `$validate-maple` for the complete test and exact-application workflow. During a security review, select focused tests first, then run the repository's CI-equivalent suites before declaring an implementation ready:

```bash
nix develop .#ci -c ./scripts/ci/frontend.sh
nix develop .#ci -c ./scripts/ci/rust.sh
```

Run dependency advisory checks only with available, trusted tooling and without changing lockfiles. Separate advisory presence, production reachability, exploitability, and accepted exceptions. Report when an audit tool or advisory database could not be refreshed.

Add boundary-specific proof:

- **Auth/account:** wrong-user credentials, refresh during validation, A -> B -> C, cleanup failure/retry, logout with pending operations, deletion ordering, restart.
- **Deep link:** exact valid callback, wrong scheme/authority/path/provider, missing or stale state, duplicate/replay, malformed parameters, safe redirect, and no credential logging.
- **Proxy:** CORS-off `Origin` POST and `Sec-Fetch-Site` GET rejection; originless local success; CORS-on preflight with `Authorization`; no saved fallback; non-loopback rejection or explicit secure mode; invalid backend endpoints; start/stop/restart; auto-start; offline logout; delayed-start race.
- **Agent permissions:** classifier ambiguity/failure, Auto -> Read-only mid-run, unknown/duplicate/reused/late responses, cross-surface attempts, cancellation/disconnect, event overflow.
- **MCP/ACP:** inert save versus execution, deterministic STDIO and Streamable HTTP fixture, enable/disable, failed connection, immutable session snapshot, secret revocation, same-user client boundary, caller disconnect.
- **Persistence:** account A/B isolation, precise history/data deletion, roaming versus device-local behavior, restrictive permissions, keyring hard-clear failure, symlink/path cases.
- **Streaming:** simultaneous A/B runs, rapid A -> B -> A, hidden completion, cancel with pending deltas, ambiguous retry, draft rekey collision, account teardown, stale timers and loads.

For native behavior, test the exact checkout-built binary or bundle identifier and verify its configured backend URL, executable path, proxy port, and listener PID before automation. Do not target an application only by the display name `Maple`. Distinguish unit tests, build success, runtime smoke, platform coverage, and deployment checks in the final report.

## Deliver the Review

Lead with actionable findings ordered by severity. Use exact file/function references and concise attack narratives. Then list:

- confirmed protections that materially constrain the threat;
- unverified platforms or deployment assumptions;
- validation run and results;
- current worktree/base state; and
- the smallest recommended next action.

If no confirmed findings remain, say so plainly and report the boundaries not
verified for this task. Never convert an intended power feature, dependency
warning, or source-only hypothesis into a production incident claim.
