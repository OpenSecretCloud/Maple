These are public test fixtures, signed with the deterministic test-only seed
`[7; 32]` also used by `src/tests.rs`. This key is public knowledge and must never
be pinned in an application profile or used to sign a production release.

`manifest.json` contains the exact bytes authenticated by `manifest.minisig`.
`signed-envelope.json` embeds the same bytes and signature for client, Worker
and publisher contract tests. The artifact bytes are the ASCII string
`signed payload fixture bytes`; they are not an application archive.
