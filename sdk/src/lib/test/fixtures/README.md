# Sigstore browser test fixtures

`sigstore-production-root.json` is test-only trust material copied from
`tinfoilsh/tinfoil-go/verifier/client/trusted_root.json` at tinfoil-go commit
`074ab5154777cbf1126c7230985433580c7c29d5`. The source file SHA-256 is
`6494e21ea73fa7ee769f85f57d5a3e6a08725eae1e38c755fc3517c9e6bc0b66`.
Prettier changes JSON whitespace in this repository; the copied fixture SHA-256
is `84d95b8389e45dc35f9d22f2a2f30d3f427644ad348c97e3b9f43f49efcb02ad`,
and both files have the same sorted compact-JSON SHA-256
`05a094edfa2e8eb7d1b4b37244e28ffb6d86b8b86ce3eb122b644b4335ae4dc1`.

The positive portable bundle is shared with the Rust SDK at
`sdk/rust/tests/fixtures/cosign-v3-blob.sigstore.json`; its SHA-256 is
`ed70e4cadbe916b31d1c9fe913f6ae8d799b5cc3336b104ec839f65cd16befdd`
and it signs the exact bytes `test content for cosign\n`.

The shared `rekor-v2-*` fixtures live beside the Rust fixture at
`sdk/rust/tests/fixtures/`. They are the official `rekor2-happy-path`
conformance case copied byte-for-byte from `sigstore/sigstore-conformance`
commit `9a5d9e9c5171eb56df7821e342eda7122f764b43`. Their upstream paths are
`test/assets/bundle-verify/rekor2-happy-path/{bundle.sigstore.json,trusted_root.json}`
and `test/assets/bundle-verify/a.txt`. The raw fixture SHA-256 values are:

- artifact: `a0cfc71271d6e278e57cd332ff957c3f7043fdda354c4cbb190a30d56efa01bf`
- bundle: `3a5ce62cee2653969be846a41e8332eae82c633cdfafe6db48b10e60939518dd`
- trusted root: `ed6a9cf4e7c2e3297a4b5974fce0d17132f03c63512029d7aa3a402b43acab49`

The bundle and root use a `.fixture` suffix so repository formatting cannot
change the official raw bytes. This case proves the canonical Rekor v2 wire
form, which omits `integratedTime` and relies on its RFC3161 timestamp.

These fixtures are never imported by production SDK source or included as a
runtime trust root. Tests read them locally and perform no network requests.
