# Task runner for maple-gpui. Run `just` to list the recipes.

set shell := ["bash", "-euo", "pipefail", "-c"]

export CARGO_TERM_COLOR := "always"

# Share Cargo intermediate artifacts across maple-gpui worktrees. Final
# binaries stay in this checkout's target/. Skip in CI, when disabled, or
# when CARGO_BUILD_BUILD_DIR is already set. rustc host+version keep
# incompatible toolchains in separate caches.
export CARGO_BUILD_BUILD_DIR := `if [ -n "${CARGO_BUILD_BUILD_DIR:-}" ]; then printf "%s" "$CARGO_BUILD_BUILD_DIR"; elif [ -n "${CI:-}" ] || [ "${MAPLE_GPUI_DISABLE_SHARED_CARGO_BUILD_DIR:-0}" = "1" ]; then printf target; elif command -v rustc >/dev/null 2>&1; then printf "%s/.cache/cargo-build/maple-gpui/%s/rust-%s" "$HOME" "$(rustc -vV | awk '/^host:/{print $2}')" "$(rustc --version | awk '{print $2}')"; else printf target; fi`

headless := "--no-default-features --features acp,proxy"

# List the recipes.
default:
    @just --list --unsorted

# Run all the checks that CI runs.
ci: fmt-check clippy test

# Format all code.
fmt:
    cargo fmt --all

# Check that all code is formatted.
fmt-check:
    cargo fmt --all -- --check

# Run clippy for the workspace and for each feature set that CI checks.
clippy:
    cargo clippy --workspace --all-targets --locked -- -D warnings
    cargo clippy -p maple-gpui --all-targets --locked {{headless}} -- -D warnings
    cargo clippy -p maple-gpui --locked --no-default-features --features acp -- -D warnings
    cargo clippy -p maple-gpui --locked --no-default-features --features proxy -- -D warnings

# Build and run the tests for the workspace and for the headless build.
test:
    RUSTFLAGS="-D warnings" cargo build --workspace --all-targets --locked
    RUSTFLAGS="-D warnings" cargo test --workspace --locked
    RUSTFLAGS="-D warnings" cargo test -p maple-gpui --locked {{headless}}

# Build the debug binary.
build:
    cargo build -p maple-gpui

# Stage the debug binary in a stable macOS app bundle. A stable bundle identity
# is required for honest Accessibility and Screen Recording permission tests.
debug-app: build
    ./scripts/macos-debug-app.sh

# Build the release binary.
release:
    cargo build --release -p maple-gpui --locked

# Build a release binary that keeps symbols for backtraces.
release-debug:
    CARGO_PROFILE_RELEASE_STRIP=false CARGO_PROFILE_RELEASE_DEBUG=line-tables-only \
        cargo build --release -p maple-gpui --locked

# Build the headless binary (acp and proxy modes, no window).
headless:
    cargo build --release -p maple-gpui --locked {{headless}}

# Build and run the debug binary with debug logs for the app.
run *ARGS: build
    RUST_LOG=warn,maple_gpui=debug ./target/debug/maple-gpui {{ARGS}}

# Build the release binary and copy it to dist/ with a version, commit, and SHA-256.
dist: release
    #!/usr/bin/env bash
    set -euo pipefail
    version=$(grep -m1 '^version' Cargo.toml | cut -d'"' -f2)
    rev=$(git rev-parse --short HEAD)
    dirty=""
    if [ -n "$(git status --porcelain)" ]; then dirty="-dirty"; fi
    case "$(uname -s)-$(uname -m)" in
        Linux-x86_64) target=linux-x86_64 ;;
        Darwin-arm64) target=macos-aarch64 ;;
        Darwin-x86_64) target=macos-x86_64 ;;
        *) target="$(uname -s | tr '[:upper:]' '[:lower:]')-$(uname -m)" ;;
    esac
    name="maple-gpui-${version}-${rev}${dirty}-${target}"
    mkdir -p dist
    cp target/release/maple-gpui "dist/${name}"
    (cd dist && shasum -a 256 "${name}" | tee "${name}.sha256")

# Remove this checkout's target/ without wiping the shared Cargo cache.
clean-local:
    CARGO_BUILD_BUILD_DIR=target cargo clean

# Remove this checkout's target/ and dist/. Keeps the shared Cargo cache.
clean: clean-local
    rm -rf dist
