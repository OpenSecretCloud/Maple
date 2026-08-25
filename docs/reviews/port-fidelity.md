# Port fidelity review: crates/maple-agent

Date: 2026-08-25
Scope: `crates/maple-agent` (ported) versus
`/home/ben/projects/Maple/frontend/src-tauri` (original).
`agent/macos_login_path.rs` is excluded (macOS-only).

## Method

1. `diff -w` (whitespace-insensitive) of each ported file against the
   original. Every non-whitespace difference is listed below.
2. Token check: each file with an empty `diff -w` was also compared with all
   whitespace removed. `developer_tools.rs`, `shell_permission.rs`,
   `system_prompt.rs`, `transient_mcp.rs`, `web_permission.rs`, and
   `web_tools.rs` are token-identical. `attachments.rs`, `tool_context.rs`,
   and `open_secret_config.rs` differ only on visibility lines.
3. sed damage check: every `pub(crate)` -> `pub` change lands on a
   declaration line (item, method, or `use` re-export). No string literal,
   comment, constant, or logic line changed. `tauri::http` -> `http` changed
   only path prefixes in `provider.rs`; no literal changed. 21 `pub(crate)`
   markers remain in `agent.rs` (module and internal items), so the sed was
   selective, not global.
4. Test parity: `#[test]` and `#[tokio::test]` attribute counts match in every
   file (agent.rs 155, maple_api.rs 11, open_secret_config.rs 1,
   attachments 2, developer_tools 48, provider 38, shell_permission 8,
   system_prompt 3, tool_context 3, transient_mcp 2, web_permission 7,
   web_tools 12). `fn` count in agent.rs is 510 on both sides.
5. `cargo check -p maple-agent --tests` passes. `cargo test -p maple-agent`
   passes: 290 passed, 0 failed.

## Findings

1. NIT — `crates/maple-agent/src/agent.rs:63..5560` vs original
   `src/agent.rs:63..5569`. 72 visibility changes `pub(crate)` -> `pub` on
   the service/event surface (`AgentEventSink`, `AgentRuntimeHandle`,
   `MapleAgentHostResources`, `AgentPathLayout`, `AgentRunHandle`,
   `AgentPermission*`, `AgentRunEvent`, `AgentServiceEvent`,
   `AgentToolContextSpec` re-export, and all handle methods). Rustfmt
   re-joined three signatures onto one line (`handle_for_user`,
   `get_project_trust`, `load_session`). INTENDED. Fix: none. Note: the
   compiler now reports "type X is more private than item Y" for
   `AgentHostEventPolicy`, `AgentToolContextAccess`,
   `AgentTransientMcpServer`, `CreatedAgentSession`,
   `AgentRunCancellation`, and `AgentRunPermissionResponder`. Harmless, but
   either widen those types or narrow the surface-context methods to keep the
   warning list clean.

2. NIT — `crates/maple-agent/src/maple_api.rs:9-41` vs original
   `src/maple_api.rs:10-60`. Removed `use tauri::{AppHandle, Emitter, State}`,
   `AUTH_CHANGED_EVENT`, `MapleApiAuthChanged`, and `TauriAuthEventSink`.
   Added `pub trait MapleApiAuthEventSink` (was private) and
   `pub struct NoopAuthEventSink` whose `auth_changed` is a no-op. INTENDED.
   Fix: none for fidelity. The app crate must supply a real sink if the UI
   needs the "credentials refreshed" signal; the Noop sink drops it.

3. NIT — `crates/maple-agent/src/maple_api.rs:555-561` vs original
   `src/maple_api.rs:574-581`. `set_auth` is now `pub` and takes
   `event_sink: Arc<dyn MapleApiAuthEventSink>` instead of `AppHandle`. The
   body still delegates to `set_auth_with_sink(event_sink, request)`.
   Signature change only, no logic drift. INTENDED. Fix: none.

4. NIT — `crates/maple-agent/src/maple_api.rs:606,620` vs original
   `src/maple_api.rs:626,640`. `session_for` and `clear_auth` changed
   visibility only (`pub(crate)`/private -> `pub`). Bodies identical.
   INTENDED. Fix: none.

5. NIT — `crates/maple-agent/src/maple_api.rs` (end of file) vs original
   `src/maple_api.rs:659-682`. The three `#[tauri::command]` wrappers
   (`maple_api_set_auth`, `maple_api_get_auth`, `maple_api_clear_auth`) are
   deleted. INTENDED. Fix: none.

6. NIT — `crates/maple-agent/src/agent/provider.rs` (prod lines 268-276,
   584-587, 698-728; test lines 1107-1137, 1285-1295, 2092-2135, 2408) vs
   the same lines in the original. `tauri::http::` -> `http::` on 33 path
   prefixes. Both resolve to the `http` 1.x crate. INTENDED. Fix: none.

7. NIT — `crates/maple-agent/src/open_secret_config.rs:13`,
   `agent/attachments.rs:13,20`, `agent/tool_context.rs:17,24` vs the same
   lines in the original. Visibility only. INTENDED. Fix: none.

8. SHOULD-FIX — `crates/maple-agent/Cargo.toml` (no `[target.'cfg(windows)']`
   table) vs original `Cargo.toml:83-88`. `agent/developer_tools.rs:47-48`
   has `#[cfg(windows)] use windows::Win32::System::Threading::CREATE_NO_WINDOW;`
   but the ported crate does not declare the `windows` dependency
   (`windows = { version = "0.62.2", features = ["Win32_System_Threading"] }`).
   Linux and macOS builds are unaffected; a Windows build fails.
   Classification: SUSPECT (dependency drift, not source drift).
   Fix: add the `[target.'cfg(windows)'.dependencies]` entry, or drop the
   Windows branches if the gpui app will never target Windows.

9. NIT — `crates/maple-agent/Cargo.toml` vs original `Cargo.toml`. Runtime
   dependency set and feature flags match: goose and goose-providers use the
   same git rev `f9c7aacc…` with `default-features = false`; rmcp `=3.1.2`
   with `client` + `transport-streamable-http-client-reqwest`; process-wrap
   `=9.1.0` with `tokio1`, `creation-flags`, `job-object`, `process-group`;
   pulldown-cmark 0.13 `default-features = false`; icu_properties 2.1.1;
   chrono `std` only; tokio, tokio-util, reqwest (`stream`) features
   identical. `axum` moved from a normal dependency to `[dev-dependencies]`;
   it is used only in `#[cfg(test)]` code in the ported files, so this is
   correct. `agent-client-protocol`, `tauri-plugin-dialog`, and `keyring` are
   not needed by the ported files. INTENDED. Fix: none.

10. NIT — `Cargo.lock` (gpui) vs `Cargo.lock` (Maple). Resolved versions
    drift within semver for `http` (1.5.0 vs 1.3.1), `reqwest` (0.13.4 vs
    0.13.2), `tokio` (1.53.1 vs 1.52.3), `icu_properties` (2.1.2 vs 2.1.1),
    `rand` (0.8.7 vs 0.8.6), `tempfile` (3.27.0 vs 3.23.0). goose,
    goose-providers, rmcp, opensecret, maple-proxy, process-wrap,
    pulldown-cmark, and chrono resolve to identical versions. INTENDED
    (fresh lock). Fix: none unless a byte-identical dependency graph is a
    goal.

11. NIT (informational) — `Cargo.toml:25-26` (workspace root)
    `[profile.dev] opt-level = 1`. This applies to `cargo test` as well
    (the `test` profile inherits from `dev`). Effects: debug assertions and
    overflow checks stay enabled, so it does not hide panics or overflow
    behavior. It can change timing in the tests that use real timeouts
    (permission deadlines, MCP 30 s bounds, run cancellation) by making the
    code faster than an `opt-level = 0` build; it does not make any test
    weaker than the original, which the Maple repo also builds with the
    default dev profile. No masking risk found. Fix: none.

12. NIT (informational) — `crates/maple-agent/src/agent.rs:10133,10145` and
    related. Test helpers `blocking_transient_mcp`,
    `leased_test_tool_context`, `blocking_mcp_catalog_server`,
    `BlockingMcpCatalogServer`, and prod items `AgentRunHandle::respond`,
    `AgentRunHandle::cancel`, `permission_respond_for_run`,
    `AgentToolContextAccess::{access, revoke, discard_created_if_untouched}`,
    and enum variants `CallingSurface`, `StreamableHttp` now warn as unused.
    In the original they were reached from `agent_acp.rs` / `agent_tauri.rs`,
    which were not ported. The code is byte-identical; only the callers are
    gone. Classification: INTENDED consequence of deleting the Tauri and ACP
    adapters. Fix: none for fidelity; add `#[allow(dead_code)]` or wire the
    callers once the gpui app uses them.

## Summary

- Files compared: 12.
- Differences classified INTENDED: 11 (findings 1-7, 9-12).
- Differences classified SUSPECT: 1 (finding 8, missing `windows`
  dependency).
- BLOCKER: 0. SHOULD-FIX: 1. NIT: 11.
- No deleted logic, changed literal, changed constant, or test-body change
  was found in any ported file.
