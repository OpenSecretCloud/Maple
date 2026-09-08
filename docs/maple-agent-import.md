# Maple Agent import

Maple Agent is the GPUI desktop-v2 prototype under `apps/maple-agent/`.
Research remains the shipped React/Tauri application under
`apps/maple-research/`. SDK and proxy source are shared; the two application
runtimes, state directories, build graphs, and update discovery stay distinct.

## Preserved source boundary

The source is `benthecarman/maple-gpui` master at
`6bedcf938b41629fca375ec96caaf5b8ddccaf67`, captured September 8, 2026.
It contains 338 reachable commits and 161 files at the imported tip.

The unmodified import commit is
`b9530b0f1131be219371778e14180340cbfa7694`. Its first parent is Maple's
layout merge `abfa8685534c359949b1a8d1ef0c4c79e036261c`; its second parent is
the source master above. The imported `apps/maple-agent` tree exactly equals
the original root tree `feaa3bc67584d881aa5c85d742302afc52a620b4`.
Dependency, tooling, and update adaptations follow that separate boundary.

Merge this import PR with a normal merge commit. Squashing or rebasing away
the import merge would discard the preserved upstream ancestry.

## Integration

- Use the existing local `opensecret` and `maple-proxy` packages. The GPUI SDK
  fork is replaced by the canonical catalog API and its boolean capability
  contract. Package renaming and registry publishing remain separate work.
- Keep the GPUI component's Cargo/Nix environment and internal `maple-gpui`
  binary name. Root commands, CI, and agent guidance route to the component.
  Linux exposes pure Nix packages; macOS uses the Nix development shell plus
  full Xcode, because the pinned pure Swift/SDK combination cannot build CUA.
- Root CI replaces the inactive nested workflows. PR builds use read-only
  tokens and no signing/deployment credentials. Shared Rust runtime changes
  select both consuming applications.
- OpenSecret Workspaces provides `bin/maple-agent`, private environment and
  isolated config/data. Legacy GPUI checkouts retain their existing launcher
  and state. Both use the workspace proxy reservation; only one proxy process
  may own that port.
- Agent discovers only stable `maple-agent-vX.Y.Z` releases and displays a
  link. It cannot mistake Research's `vX.Y.Z` releases for Agent updates.
  Incomplete or failed release scans suppress the banner.

The Agent release namespace is reserved, with no publisher activated here.
Before official distribution, establish packaging/signing and a dedicated
publisher. Every future Agent release must use `make_latest: false` so it
does not move Research's global GitHub latest pointer. This import does not
create releases or change existing Research signing, updater URLs, or Pages.

## Ongoing upstream work

The initial import includes master only. These source PRs remain separate
follow-up work; this import does not merge, close, or rewrite them:

| PR | Branch and dependency | Captured head | Follow-up |
| --- | --- | --- | --- |
| [31](https://github.com/benthecarman/maple-gpui/pull/31) | `terminal/spike-libghostty`, from master | `e0a0326652015f7903dacef580dfed209c88f540` | Replay first terminal slice if continued |
| [34](https://github.com/benthecarman/maple-gpui/pull/34) | `terminal/product-pane`, on 31 | `fbeb45d08c4b9f40b2da8809e8ee0f13aef54ec0` | Replay after 31 |
| [37](https://github.com/benthecarman/maple-gpui/pull/37) | `terminal/tabs`, on 34 | `6e2c674714308448d5bf83336088c68b850350a3` | Replay after 34 |
| [38](https://github.com/benthecarman/maple-gpui/pull/38) | `terminal/supervisor`, on 37 | `2745300a852009cdaeaf2e582d81875b16a4d68a` | Replay after 37 |
| [69](https://github.com/benthecarman/maple-gpui/pull/69) | Draft `feature/cpython-codemode` | `e087b1af4eb0e4f77c036b9fff2f7904a2e9c4f0` | Separate focused port |
| [2](https://github.com/benthecarman/maple-gpui/pull/2) | Draft `programmable-harness` | `653b95946bf391cc82998bcc6ca22fe40da577e9` | Historical conflicting draft; reassess individual slices against current master |

After merging the import, direct new Agent work here and recheck each active
branch's latest head with its author before replaying. Prefix the intended
delta under `apps/maple-agent/` and review it against current code; do not
blindly merge an old root-layout branch. Keep the source repository and open
branches available until each item's disposition is recorded. Repository
archival or an upstream deprecation notice is a separate cutover step.
