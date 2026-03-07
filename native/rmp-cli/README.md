# rmp-cli

`rmp-cli` is the Rust Multiplatform orchestration CLI used for scaffolding and running RMP app templates.

## Docs

- RMP architecture and ownership model: [`../../docs/rmp.md`](../../docs/rmp.md)
- RMP CI lanes and nightly checks: [`../../docs/rmp-ci.md`](../../docs/rmp-ci.md)
- Repo architecture overview: [`../../docs/architecture.md`](../../docs/architecture.md)

## Purpose

The CLI is intentionally operational:

- scaffold project layouts (`rmp init`)
- run health checks (`rmp doctor`)
- generate/bind platform glue (`rmp bindings ...`)
- run target apps (`rmp run ios|android|iced`)

Business logic and state for generated apps should stay in Rust core crates; platform code should follow the adapter-window model in `docs/rmp.md`.
