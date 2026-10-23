The Linux v2 fixture uses the same public deterministic test-only key and
ASCII payload as the existing v1 fixtures. It is not an application archive.
Its feed is `/agent/stable/linux/latest.json`; immutable signed metadata lives
at `/agent/stable/releases/0.0.2/linux-manifest.json`.

`legacy-v1-validator.rs` preserves the pre-Linux validator from commit
`264560b0` byte for byte. Tests compile it independently to ensure newly emitted
macOS v1 metadata remains readable by the installed client. Existing v1 fixtures
remain unchanged.
