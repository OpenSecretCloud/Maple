# Packaging / release review

Date: 2026-08-25. Commit: 8d6b0f8. Scope: manifests, README, examples,
release build, xkbcommon workaround, .gitignore. No source file was changed.

## Findings

1. **SHOULD-FIX** — `app/Cargo.toml:15,17,18` — `serde`, `anyhow`, and `log`
   are declared but no file under `app/src` or `app/examples` references them
   (`serde_json` is used; `serde` itself is not; logging goes only through
   `env_logger::init()`). Fix: delete the three lines. If you plan to add
   `log::` calls soon, keep `log` and add the calls; do not keep the other two.

2. **SHOULD-FIX** — `app/Cargo.toml:23` — `x11rb` is a normal dependency but
   only `app/examples/xsend_input.rs` uses it. It is compiled into every
   build of the binary. Fix: move it to `[dev-dependencies]` (examples can
   use dev-dependencies) with the same `features = ["xtest"]`.

3. **SHOULD-FIX** — `crates/maple-agent/Cargo.toml:26` — `chrono` is declared
   but no file under `crates/maple-agent/src` references it. Fix: delete the
   line, then `cargo check --workspace`.

4. **SHOULD-FIX** — `Cargo.toml` (workspace) — no `[profile.release]`. The
   gpui 0.2.2 crate ships no profile recommendation (its README and
   Cargo.toml have none). Zed, gpui's home project, uses `lto = "thin"`,
   `codegen-units = 1`, and `debug = "limited"`. Measured on this machine:
   default release = 134,619,360 bytes, not stripped; `strip` alone brings it
   to 104,605,936 bytes. Fix: add
   ```toml
   [profile.release]
   lto = "thin"
   codegen-units = 1
   strip = true
   ```
   and re-measure. Expect a longer build (currently 2m12s) and a smaller
   binary. Do not set `panic = "abort"`: Goose and rmcp rely on unwinding in
   places.

5. **SHOULD-FIX** — `app/Cargo.toml:1-5` — no `description`. `maple-agent`
   has one; the binary crate does not. Also no `publish = false`, so a stray
   `cargo publish` is not blocked. Fix: add
   `description = "Native Maple desktop app (gpui)"` and `publish = false`.
   Consider `publish = false` on `maple-agent` too: it depends on a git
   fork of Goose, so crates.io would reject it anyway.

6. **NIT** — `app/Cargo.toml:3` uses `edition = "2024"`;
   `Cargo.toml:6` sets the workspace edition to 2021 and `maple-agent`
   inherits it. Two editions in one small workspace is a surprise for the
   next reader. Fix: pick one. 2024 is fine for both if `maple-agent` still
   compiles (the ported code was written for 2021; expect `unsafe extern`,
   `gen`-keyword, and `impl Trait` capture changes to surface).

7. **NIT** — `Cargo.toml:5-8` `[workspace.package]` has no `repository`,
   `authors`, or `rust-version`. Fix: add `repository` once the GitHub URL
   exists, and `rust-version` set to the toolchain you verify with, so a
   too-old `cargo` fails early with a clear message.

8. **NIT** — `Cargo.toml:23-24` `[profile.dev] opt-level = 1` is a global
   choice that slows incremental dev builds. It is common for gpui apps, so
   keep it, but add a one-line comment that says why (gpui and Goose are too
   slow at opt-level 0).

9. **SHOULD-FIX** — `README.md:6` — "Sprint 1 scope (agreed with the boss)"
   is internal chatter that does not belong in a public README. Fix: replace
   with "Current scope: Desktop Agent Mode only."

10. **SHOULD-FIX** — `README.md:45-49` — no Linux build prerequisites. A
    clean machine needs `libxkbcommon-dev`, `libxkbcommon-x11-dev`,
    `libxcb-*`, `libwayland-dev`, `libvulkan-dev`, `libfontconfig1-dev`,
    plus `cmake`/`clang` for gpui's native pieces, and a Vulkan-capable GPU
    or `lavapipe` for headless. Fix: add a "Build prerequisites (Linux)"
    section with the apt line. Without it the first external clone fails at
    link time, which is exactly the `libxkbcommon-x11` failure that needed
    the RUSTFLAGS workaround earlier.

11. **NIT** — `README.md:57-59` — the data-dir claim is correct but
    incomplete: `app/src/backend.rs:66-78` honours `XDG_CONFIG_HOME` and
    `XDG_DATA_HOME` before falling back to `~/.config` and `~/.local/share`.
    Fix: say "`$XDG_CONFIG_HOME/maple-gpui` (default `~/.config/maple-gpui`)"
    and the same for data.

12. **NIT** — `README.md:65-70` — the test section says
    `cargo test -p maple-agent` but not what to expect. Verified today:
    `cargo test -p maple-agent -- --list` reports **290** tests;
    `maple-gpui` has 0. If you state a count anywhere, use 290. Also add
    `cargo check --workspace --all-targets` so the examples get compiled in
    CI; plain `cargo check --workspace` skips them.

13. **NIT** — `README.md` does not mention `app/examples/`. Both examples
    compile under `cargo check --all-targets -p maple-gpui` (verified, zero
    errors). They are test tooling for the Xvfb display, not product code.
    Recommendation: **keep them in the repo** — they are small (82 and 130
    lines), they compile in CI as examples, and they document the
    headless-QA procedure — but add a short "Headless QA" section to the
    README that says what `gpui_hello` and `xsend_input` are for and how
    to run them (`cargo run -p maple-gpui --example xsend_input -- click X Y`).
    Moving them to a `tests-support/` dir would lose the free compile check
    that `--all-targets` gives. Do finding 2 so `x11rb` stops being a
    product dependency.

14. **NIT** — release build emits 34 warnings (22 in `maple-agent`, 12 in
    `maple-gpui`). None are release-only; the same set appears in dev.
    Most are `dead_code` (unused methods on `AgentBackend`, unused theme
    constants, `heading` assigned but never read in `ui/markdown.rs`) and
    `private_interfaces` in `maple-agent`. Not a packaging blocker, but a
    clean `cargo build --release` output is a cheap release-quality signal.
    Fix: address or `#[allow]` with a reason before the first tag.

15. **NIT** — `cargo build` warns: "proc-macro-error2 v2.0.1 ... will be
    rejected by a future version of Rust". This is a transitive dependency
    (via the Goose fork). Nothing to do here except track it; a future
    toolchain bump will turn it into an error and require a Goose rev bump.

16. **NIT** — xkbcommon workaround: `~/.local/lib/libxkbcommon-x11.so ->
    /usr/lib/x86_64-linux-gnu/libxkbcommon-x11.so.0` still exists.
    Verified it is now **harmless**: `libxkbcommon-x11-dev` is installed,
    the release build succeeds with `RUSTFLAGS` unset, `~/.local/lib` is
    not in `LD_LIBRARY_PATH`, and `ldd target/release/maple-gpui` resolves
    `libxkbcommon-x11.so.0` to `/usr/lib`. No file in the repo mentions the
    workaround (grep for `xkbcommon`, `RUSTFLAGS`, `.local/lib`,
    `LIBRARY_PATH` hits only `Cargo.lock`), and the README does not mention
    it. Fix: `rm ~/.local/lib/libxkbcommon-x11.so` so it cannot shadow a
    future system lib if someone later adds `-L ~/.local/lib`.

17. **NIT** — `.gitignore` covers `/target`, `.env`, `.env.*`,
    `.papercuts.jsonl` (all verified with `git check-ignore`).
    `Cargo.lock` is tracked (`git ls-files` lists it) — correct for a
    binary workspace; keep it. Suggested additions before pushing:
    `*.png` under a QA output dir if screenshots land in-repo,
    `.DS_Store`, `*.swp`, `.idea/`, `.vscode/` (or rely on a global
    gitignore), and `/docs/reviews/*.tmp` if reviewers write scratch.
    Nothing sensitive is currently untracked-but-present except
    `.papercuts.jsonl`, which is ignored.

## Release build facts

- Command: `cargo build --release -p maple-gpui` with `RUSTFLAGS` unset.
- Result: success, exit 0. Wall time: **132 s** (2m12s, warm registry,
  cold release target).
- Binary: `target/release/maple-gpui`, **134,619,360 bytes (129 MiB)**,
  ELF x86-64 PIE, dynamically linked, **not stripped**.
- `strip` on a copy: 104,605,936 bytes (100 MiB). LTO not measured.
- Warnings: 34 total, identical to the dev profile; none release-only.
- `cargo check --all-targets -p maple-gpui`: both examples compile.
- Test inventory: `maple-agent` 290 tests, `maple-gpui` 0.
