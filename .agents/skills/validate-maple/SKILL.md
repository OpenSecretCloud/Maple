---
name: validate-maple
description: Validate Maple changes with risk-tiered frontend, Rust, web, desktop, mobile, integration, and exact-application checks. Use when testing, smoke-testing, reproducing, preparing a handoff, or deciding what evidence a Maple change requires, especially for Tauri IPC, Agent Mode, the local proxy, authentication, deep links, PDF/OCR, mobile behavior, CI, build tooling, or release artifacts.
---

# Validate Maple

Prove the behavior that changed. Treat automated checks, successful builds, and observed runtime behavior as different evidence. Never call a build or unit-test pass an end-to-end test.

## Establish scope and identity

1. Read `AGENTS.md`, the relevant source, nearby tests, and the applicable workflow or script before choosing commands.
2. Inspect `git status`, the diff, and the base commit. Preserve unrelated user changes.
3. Classify risk by behavior, not file path. Code under `frontend/src/` can invoke Tauri commands and require native validation even though PR change detection may classify it as web-only.
4. Identify every boundary the change crosses: browser, Tauri IPC, Rust, local process, filesystem, keyring, OpenSecret API, configurable billing or feature-flag API endpoint, operating system, or packaged resource.
5. Record the untested boundaries before running checks. Add proof as it is obtained.

Use the repository's pinned Nix environments. Do not install substitute global tools merely to make a check pass.

## Choose the validation tier

Use the highest tier triggered by the change. Add lower-tier checks rather than replacing them.

### Tier 0: documentation and inert metadata

Use for prose, comments, or metadata that cannot affect a build or runtime.

- Review the rendered or consumed result, links, examples, and commands.
- Run `nix flake check` when changing the flake, GitHub Actions, or release metadata.
- Escalate to the owning tier when documentation changes an executable example, environment contract, or generated input.

### Tier 1: isolated frontend behavior

Use for pure React, TypeScript, CSS, state, and browser behavior with no native or service boundary.

- Run focused Bun tests while iterating.
- Run the complete frontend CI script before handoff.
- Build the PR-configured web artifact.
- Smoke the changed state in a real browser. Cover loading, empty, success, error, and disabled states when relevant.
- Test keyboard operation, focus, accessible labels, light/dark themes, and narrow/wide layouts when the change can affect them.

### Tier 2: API, account, persistence, and streaming integration

Use when behavior depends on an OpenSecret server, authentication, account state, conversation persistence, billing API endpoint, feature-flag API endpoint, or streamed responses.

- Complete Tier 1.
- Use a local OpenSecret backend or an explicitly configured dev server.
- Configure billing and feature flags only as external API endpoints when the scenario needs them; otherwise leave optional integrations out of scope and say so.
- Use disposable accounts and non-sensitive fixtures.
- Exercise login, logout, account switching, request start, partial stream, cancellation, failure, reload, and restored history as applicable.
- Verify that late responses from an old account or session cannot mutate the new one.
- Inspect logs for failures without copying tokens, secrets, prompts, or private content into the report.

### Tier 3: native desktop and packaged behavior

Use for Tauri IPC, Rust, Agent Mode, MCP, local proxy, keyring, filesystem/dialogs, deep links, updater behavior, PDF/OCR, process lifecycle, or packaged resources.

- Complete the relevant lower tiers.
- Run Rust format/lint and locked tests.
- Build on every affected desktop operating system. Do not treat one host as proof for another.
- Launch the exact executable or application bundle produced for the test.
- Exercise the native boundary through the UI, then corroborate it with process, listener, filesystem, and sanitized-log evidence as applicable.
- Re-test cancellation, restart, logout, and account switching when a long-lived process or listener is involved.

For a custom Tauri command, trace the Rust declaration, every intended
platform-specific `generate_handler!` registration and `cfg` gate, the typed
frontend service, and the caller. Review plugin dependency, initialization, and
capability changes as a separate authority path. Under the repository's default
Tauri build configuration, registering a custom command exposes it to local
WebViews independently of plugin capability entries. Filesystem plugin scopes
apply to filesystem plugin calls; they do not constrain filesystem access
performed inside an application-defined Rust command. Test that command's
native account, path, type, size, and lifecycle enforcement directly. If no
checked-in React-to-IPC integration test covers the path, an exact-app manual
smoke from the user action through the native result is required.

### Tier 4: mobile, release, signing, and distribution

Use for iOS, Android, signing, updater metadata, installers, entitlements, or distribution behavior.

- Complete the relevant lower tiers.
- Build and run on the affected simulator, emulator, or physical device; a compiled archive alone is not runtime proof.
- Validate lifecycle changes such as background/foreground, picker or microphone permissions, and OAuth or payment return links when touched.
- Treat fake signing as build-path evidence only.
- Route publishing, signing, versioning, release creation, and distribution work to `$release-maple`. Do not push or merge merely to trigger release workflows.

## Run canonical automated checks

Run commands from the repository root unless the command changes directory explicitly.

### Focused frontend test

```bash
cd frontend
bun --no-env-file test src/path/to/changed.test.ts
```

Use `--no-env-file` so local secrets and endpoint overrides do not silently affect unit tests.

### Complete frontend checks

```bash
nix develop .#ci -c ./scripts/ci/frontend.sh
```

This script installs locked frontend dependencies and runs formatting, linting, typechecking, and Bun tests. It removes `frontend/node_modules` and ignores local `.env*` files while it runs. Commit or preserve relevant local work before invoking it. It does **not** build the application.

### Web production build

```bash
MAPLE_WEB_ENVIRONMENT=pr nix develop .#ci -c ./scripts/ci/web.sh
```

Use `pr` for contributor validation. Record the compiled OpenSecret, billing, and feature-flag endpoint configuration when those endpoints affect the scenario. A successful web build proves bundling, not browser behavior.

### Rust checks

```bash
nix develop .#ci -c just rust-lint
nix develop .#ci -c ./scripts/ci/rust.sh
```

The lint recipe runs `cargo fmt --check` and Clippy with warnings denied. The CI script provisions Linux ONNX Runtime when needed and runs `cargo test --all-targets --locked`. Neither command launches Maple.

### Repository configuration checks

```bash
nix flake check
```

This validates pinned tool versions, GitHub Actions syntax, and release metadata. Run it for changes to `flake.nix`, `flake.lock`, workflows, CI scripts, or release configuration. It is not a substitute for product tests.

Do not cite the pre-commit hook as complete proof: it omits frontend lint and Rust format/Clippy, and its Rust tests depend on staged file patterns.

## Build the affected platform

Use these commands to mirror PR artifact builds. Then launch and smoke the result separately.

These scripts deliberately hide local `.env*` files and compile fixed PR
endpoint profiles. Their artifacts are PR packaging evidence; they do not prove
that Maple works with the OpenSecret backend configured in
`frontend/.env.local`.

### macOS desktop

```bash
nix develop .#ci -c ./scripts/ci/desktop-pr.sh
```

### Linux desktop

```bash
MAPLE_TAURI_FAKE_UPDATER_SIGNING=1 nix develop .#desktop-linux -c ./scripts/ci/desktop-pr.sh
```

### Windows desktop

Run on real Windows from Git Bash/MSYS:

```bash
./scripts/ci/desktop-windows-pr.sh
```

### Android

Run on x86_64 Linux:

```bash
MAPLE_ANDROID_FAKE_SIGNING=1 MAPLE_ANDROID_WEB_ENVIRONMENT=pr nix develop .#android -c ./scripts/ci/android-release.sh
```

### iOS

Run on macOS with the supported Xcode toolchain:

```bash
nix develop .#apple -c ./scripts/ci/ios-onnxruntime.sh
nix develop .#apple -c ./scripts/ci/ios-pr.sh
```

Treat PR artifact workflows as compile/package evidence. They do not launch the built app. Treat artifact attestations as provenance evidence only when the attestation step actually succeeds.

## Smoke a configured local backend

Preserve existing configuration. Create a local environment file only when it
is absent:

```bash
test -e frontend/.env.local || cp frontend/.env.example frontend/.env.local
```

Never replace an existing `frontend/.env.local`; it may be externally managed
or contain checkout-specific endpoints and application identity. Start the
selected OpenSecret backend by
[its public repository guide](https://github.com/OpenSecretCloud/opensecret),
including required migrations, then launch Maple from this checkout with:

```bash
just desktop-dev
```

`just desktop-dev` consumes the configured development endpoints and applies
the active `.local/tauri-workspace.json` overlay when present. It is the path
for a configured local-backend desktop smoke; it is not evidence about a fixed
PR package. Record the exact identity fields below, use a disposable account,
and exercise the relevant backend operation.

For privileged IPC, manually trigger the real React user action, observe the
Tauri command and native validation, verify the exact filesystem/process/native
effect, and exercise a representative denied input. This manual exact-app smoke
is required when no checked-in integration test crosses React, IPC, and Rust.

## Prove exact application identity

Before any desktop GUI smoke test, record:

1. Commit SHA, build profile, build command, and active Tauri configuration overlay.
2. Compiled OpenSecret, billing, and feature-flag endpoint configuration.
3. Exact executable or `.app` path.
4. Actual bundle/application identifier. The standard identifier is `cloud.opensecret.maple`, but an overlay can change it.
5. Dev-server URL and owning PID. The standard Tauri dev URL is port `5173`, but an overlay can change it.
6. Native application PID and, when relevant, local proxy port plus listener-owning PID.
7. Disposable account and test-data scope.

Target the recorded identifier and path. Never select or terminate an app only by the display name `Maple`; multiple checkouts can share it. If an overlay is active, use it consistently for build, launch, automation, and cleanup.

Distinguish these targets:

- A browser tab proves web behavior only.
- A raw `tauri dev` executable can prove native development behavior but not packaged resources, signing, updater, installer, or bundle registration.
- A packaged application proves only the scenarios actually observed after launching that exact artifact.

If debug packaging emits an application and then exits nonzero because `TAURI_SIGNING_PRIVATE_KEY` is absent, report the packaging failure. You may separately smoke the emitted application, but do not call the build successful.

Clean up only the exact PIDs, listeners, temporary accounts, and artifacts created by the test. Never kill processes by generic app name or perform broad cleanup.

## Smoke critical native surfaces

Choose only scenarios relevant to the change, but cross every changed boundary.

### Agent Mode, MCP, and local proxy

- Run Agent Mode in the desktop app; it is unavailable in the web build.
- For presentation-only Agent Mode changes, exercise the exact changed states,
  interactions, accessibility, focus, theme, and layout in the native app. Do
  not add file writes, shell execution, account switching, or lifecycle
  manipulation merely to populate the UI.
- For behavioral changes, verify the applicable start, intermediate state,
  permission, cancellation, completion, restart, and shutdown boundaries. Run
  `$change-maple-agent-mode` for the proportional lifecycle matrix.
- Confirm process and listener ownership before and after cancellation, logout,
  account switch, and app exit when those long-lived boundaries are affected.
- For MCP, follow `docs/agent-mode-mcp.md`: use the pinned Everything server, send a unique marker, verify server request, arguments, result, and final answer, then disable or stop the server and verify a clear failure.
- Verify that stale sessions cannot cross account boundaries when session or
  account ownership changed.

### PDF and OCR

- Test a text PDF, scanned PDF, mixed PDF, malformed or locked input, and applicable size/page limits.
- Exercise cold and warm model-cache paths when OCR behavior changes.
- Run the ignored model-backed test only when its external model prerequisites are available, following `docs/pdf-ocr.md`; label it separately from the default Rust suite.
- Verify cancellation and recovery from extraction failures through the exact app.

### Deep links and native services

- Invoke the real link against the recorded bundle identifier; do not merely paste its payload into an internal route.
- Verify cold start and already-running behavior where relevant.
- Exercise real dialogs, filesystem access, keyring, updater, microphone, and packaged resources on each affected platform.

## Report evidence without inflation

Write the handoff in five buckets:

1. **Automated:** exact command, platform, and pass/fail result.
2. **Artifact:** exact build command and produced artifact; state that it was not runtime evidence unless launched.
3. **Runtime:** exact app identity, platform, endpoint configuration, account/data scope, and scenarios observed.
4. **Integration:** backend or external API endpoint used and the boundary exercised, without secrets.
5. **Untested or blocked:** platform, boundary, ignored test, signing path, or failure not covered.

Call a result “end-to-end” only when the real user entry point and every relevant production boundary were exercised. Otherwise name the narrower evidence: unit, render, typecheck, build, package, browser smoke, native smoke, or integration smoke.

For security-sensitive changes, run `$review-maple-security` in addition to this skill. Keep validation and security review distinct: passing tests do not establish that a design is safe.
