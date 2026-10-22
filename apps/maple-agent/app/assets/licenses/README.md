# Third-party notices

Full notices are generated build output, not checked-in source. They cover the
locked `maple-agent-app` dependency graph for one target and the selected Cargo
features, including native-source build dependencies and EFF/spelling attribution.
Development dependencies, benchmark tools and unrelated workspace applications
are excluded. The EFF list and its source attribution accompany binary packages.

Regenerate from the Agent's pinned Nix shell:

```sh
just build
just notices
just notices --check
```

Generation is locked and offline; fetch the selected dependencies with the normal
Cargo build first. It writes `target/third-party-notices/<target-triple>/`, which
is ignored by Git. `--target`, `--no-default-features` and `--features` select the
same graph as the intended binary. Ordinary Cargo builds/tests, including Windows,
do not need cargo-about or generated notices. An unpackaged desktop binary opens
the small source/attribution page in `development.html`.

`build-release.sh` generates complete notices on the unprivileged build worker.
The macOS prebuilt artifact carries both binary and notice directory. Packaging
verifies a manifest of input/output hashes and refuses missing, modified or stale
notices, without Cargo or network access on the signing worker. Native packages
install the HTML/text alongside the executable; About opens that packaged HTML.
The Nix package generates notices offline from its vendored graph during its
build and stages the same payload. No license fetching runs in `build.rs`.

`about.toml` contains hash-checked source-license clarifications, including the
MIT Rust wrapper, BSD-3-Clause SQLCipher, public-domain SQLite and Apache-2.0
OpenSSL C source. It deliberately retains build dependencies. CUA source uses
its repository-level MIT grant; its Inter font retains OFL-1.1.

The inventory also records existing GPL-3.0-or-later Zed logging/tracing crates.
Including their text does not resolve or waive any distribution obligation;
review the pinned dependency's terms when distributing Maple Agent. This file
and the generation allowlist are a provenance inventory, not legal approval.
