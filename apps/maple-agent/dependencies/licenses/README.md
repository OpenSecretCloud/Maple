# Source license inputs

`CUA-LICENSE.md` is the unmodified repository-level MIT grant for the CUA crates
at the revision selected in `Cargo.lock`:

- Source: <https://github.com/trycua/cua/blob/2dbc1c2cfbee5d25c9cea2dc37db80b98d4f9b00/LICENSE.md>
- SHA-256: `c0779290c1d4783169aa3dbfb55feb505e563ef8a004bbf55298ceffcfbda8d9`

Cargo/Nix vendors individual Git crates without their ancestor repository's
license file. The notice generator uses this small, hash-checked source input
so generation remains offline in both checkouts and vendored builds. It is not
a generated dependency inventory. Recheck the source and clarification checksum
when updating the CUA revision. The Inter font retains its own OFL input within
the upstream `cursor-overlay` crate.
