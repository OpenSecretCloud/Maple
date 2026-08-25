# Security review: auth, token handling, event channel

Date: 2026-08-25
Scope: `app/src/backend.rs`, `app/src/main.rs`, `app/src/ui/login.rs`,
`crates/maple-agent/src/maple_api.rs`, `crates/maple-agent/src/open_secret_config.rs`.
Reference: Maple `AGENTS.md`, "Security and privacy invariants".
Method: read-only source review. No code was changed.

Counts: BLOCKER 0, SHOULD-FIX 5, NIT 4.

## Findings

1. **SHOULD-FIX** — `app/src/backend.rs:156`, `app/src/main.rs:47`
   The login path builds `OpenSecretClient` from the raw `MAPLE_API_URL`
   value. `normalize_api_url` (`maple_api.rs:454`) runs only later, inside
   `set_auth` (`maple_api.rs:574`), after the password has already been sent.
   The SDK constructor rejects embedded credentials, query, fragment, and
   non-HTTPS non-loopback hosts, so the HTTPS invariant holds. But the SDK
   accepts a path (`https://host/v1`) and the unspecified address
   `http://0.0.0.0`, which `normalize_api_url` rejects. Result: a bad override
   passes login, then `set_auth` fails with a URL error and the fresh tokens
   are dropped. The user sees "Sign in failed. Check your email and password"
   (`backend.rs:161` maps every SDK error to that text), which is misleading.
   The value is also never validated at startup, so the error only appears
   after the user types a password.
   Fix: expose a `pub fn validate_api_url(&str) -> Result<String, String>`
   from `maple_api` (wrap `normalize_api_url`) and call it in
   `AgentBackend::new` so `main` fails fast. Store the normalized value in
   `AgentBackend.api_url` so login and `set_auth` use the same string.

2. **SHOULD-FIX** — `app/src/ui/chat.rs:402`, `:221`, `:424`
   `ChatScreen.pending_permission` is a single `Option`. A second
   `PermissionRequested` overwrites the first (`:402`). A session switch
   (`set_active_session`, `:221`) and `Finished` for any run (`:424`) clear it.
   `handle_run_event` (`:381`) also drops permission events for sessions that
   are not selected. The runtime has no permission timeout (no timeout in
   `agent.rs` around `pending_permissions`); Goose blocks in
   `handle_confirmation` until `permission_respond`, `cancel_run`
   (`agent.rs:5336`), run end (`:4884`), or `stop_runtime` (`:2214`) resolves
   it. A lost card therefore means the run hangs until the user presses Stop.
   This fails closed (no auto-approve), so it is not a BLOCKER, but it strands
   work and the user gets no explanation.
   Fix: store pending permissions in a `BTreeMap<(session_id, request_id),
   PendingPermission>`, keep entries across session switches, render the
   entries for the selected session, and on `Finished` remove only entries
   whose `run_id` matches. When a permission arrives for a background
   session, show a badge on that session in the sidebar.

3. **SHOULD-FIX** — `app/src/backend.rs:52-61`, `app/src/main.rs:114-123`
   The sink comment says a "full channel" drops events. The channel is
   `unbounded`, so it never fills; `send` fails only when the receiver is
   gone. Two consequences. (a) No backpressure: if the UI executor stalls,
   agent events accumulate without limit. (b) `take_events` is one-shot. If
   the pump future exits (for example `chat.update` fails after the window
   closes or the entity is replaced), the receiver drops and every later
   event, including `PermissionRequested`, is discarded silently. The runtime
   then waits forever for an answer (see finding 2).
   Fix: use a bounded channel (`mpsc::channel(1024)`) with `try_send`; on
   `Full`, drop non-permission events and `log::warn!` the count; on `Closed`
   or for permission events, fall back to a blocking `blocking_send` on a
   dedicated thread or set a "UI disconnected" flag that `start_runtime`
   checks. Also make the pump re-attachable: keep the receiver in the backend
   and let a new `ChatScreen` call `load_session` to reconcile pending
   permission cards from the persisted timeline (`live_timelines` already
   records them with a pending status).

4. **SHOULD-FIX** — `app/src/ui/chat.rs` (no sign-out path), `app/src/backend.rs:178`
   `AgentBackend::logout` exists but nothing calls it. `ChatScreen` has no
   sign-out control and never calls `stop_runtime` or `clear_auth`. Tokens
   stay in `MapleApiAuthState` for the process lifetime. The Maple invariant
   requires secure logout to stop and drain the agent, then clear native
   auth, in that order. Today the only way to drop tokens is to quit.
   Token lifetime is in-memory only. Verified: `MapleApiAuthState` holds an
   `Arc<MapleApiSession>` behind a `Mutex`; the SDK `SessionManager` keeps
   tokens in an `RwLock` (opensecret 3.6.2 `session.rs:24`); no
   `fs::write`/keyring call touches tokens in `maple_api.rs`, `backend.rs`,
   or the SDK. `configure_embedded_goose` (`agent.rs:7987`) removes provider
   env vars so Goose never sees them. Only Goose session transcripts are
   written under `<XDG_CONFIG_HOME|~/.config>/maple-gpui/agent` and
   `<XDG_DATA_HOME|~/.local/share>/maple-gpui/agent`.
   Fix: add `AgentBackend::sign_out(user_id)` that calls
   `service.handle_for_user(user_id).stop()` then `auth.clear_auth(user_id)`,
   wire a "Sign out" button that returns to `LoginScreen`, and add a doc
   comment on `AgentBackend.auth` (`backend.rs:46`) that states tokens are
   memory-only and cleared by `sign_out`.

5. **SHOULD-FIX** — `app/src/backend.rs:36-41`, `app/src/ui/login.rs:11`
   `AuthSession` derives `Debug` and embeds `MapleApiAuthSnapshot`, which
   derives `Debug` and `Serialize` and holds `access_token` and
   `refresh_token`. `LoginSucceeded(AuthSession)` broadcasts the tokens on
   the gpui event bus; `main.rs:87` only reads `user_id` and the rest is
   dropped. No `{:?}` or log call touches these types today (grep of
   `app/src` and `maple_api.rs` found none), so this is not a live leak. It
   is the exact regression class the Tauri code guarded against, and one
   `log::debug!("{event:?}")` would reintroduce it.
   Fix: make `AuthSession { user_id, revision, native_instance_id }` and do
   not carry the snapshot out of `backend.rs`. If the snapshot is needed
   later, add a manual `Debug` impl that prints `<redacted>` for both tokens.

6. **NIT** — `app/src/ui/login.rs:51`, `app/src/ui/chat.rs:69`
   `format!("{error}")` on a Tokio `JoinError` is shown to the user. Since
   Tokio 1.38 the `Display` output includes the panic payload string. A
   panic message from a backend task is not sanitized. No current panic path
   carries a secret, but the text is not user-facing quality either.
   Fix: map join errors to a fixed "Backend task failed" string and
   `log::error!` the join error category only.

7. **NIT** — `app/src/backend.rs:65-81`
   When `HOME` and `XDG_*` are unset or relative, the config and data roots
   fall back to `.`. Agent state, including Goose session transcripts with
   prompts, is then written into the current working directory, which is
   also the default project root (`backend.rs:89`).
   Fix: return `Err` from `AgentBackend::new` when no absolute home or XDG
   root exists.

8. **NIT** — `app/src/main.rs:47`
   `MAPLE_API_URL` is honoured in release builds. The login screen shows the
   value (`login.rs:99`), so it is not hidden, but an operator-set env var
   can silently point a release build at a loopback dev server with mock
   attestation (the SDK allows `http://localhost`).
   Fix: accept the override only under `cfg!(debug_assertions)` or an
   explicit `--dev-api-url` flag, and show a visible "development server"
   banner when it is active.

9. **NIT** — `app/src/ui/chat.rs:162-185`
   `select_session` applies the loaded session whenever its future resolves.
   Two quick clicks (A, then B) can end with A displayed if A's load resolves
   last. This breaks the "late work must prove it still owns the current
   session before publishing state" rule. It is a UI consistency bug, not a
   cross-account leak: `handle_for_user` re-checks the account scope on every
   call, and `user_id` is fixed per `ChatScreen`.
   Fix: capture a request counter or the target `session_id` and discard
   results that do not match `self.selected_session` at apply time.

## Checks with no finding

- **Token logging (Q1).** `backend.rs` maps every login error to a fixed
  string. `map_sdk_error` (`maple_api.rs:432`) logs only an error category.
  The three `log::warn!` calls in `maple_api.rs` (`:260`, `:288`, `:316`)
  print `{error}` where `error` is a `String` produced by `map_sdk_error` or
  a fixed literal, so no token can reach the log. The SDK has no `println!`
  or token-bearing log line. `env_logger` is on, but no app-layer log macro
  exists at all. No regression from the Tauri sanitization was found.
- **Persistence (Q2).** See finding 4: tokens are memory-only. The only
  disk writes under the app roots are Goose session and config files.
- **URL validation (Q4).** Every path into `build_client` inside
  `maple_api.rs` (`:575`) goes through `normalize_api_url`. The gap is only
  the app-layer login path (finding 1).
- **Panic paths (Q5).** No `unwrap`, `expect`, slice indexing, or byte
  slicing on untrusted data in `app/src`. `tool_summary` formats
  `serde_json::Value` with `Display`, which cannot panic. The two `unwrap`
  calls in `text_input.rs:660-662` act on layout state the element built
  itself in `prepaint`. `main.rs:49`, `:76`, `:98` are startup `expect`s on
  local configuration, not on remote input.
- **`spawn` facade (Q6).** `AgentBackend::spawn` requires `Send + 'static`
  and runs the future on the private Tokio runtime, so the UI executor never
  blocks on it and a future cannot borrow UI state. Every service call
  re-derives the account scope from `user_id` (`agent.rs:1525`), so a future
  that outlives a future logout would fail closed with "belongs to a
  different signed-in account". Today no logout exists (finding 4), so the
  leak question is moot until one is added; finding 9 covers the intra-user
  late-apply case.
