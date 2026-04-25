# phf_generator 0.8.0 advisory patch

This is a temporary copy of `phf_generator 0.8.0` used by `Cargo.toml`'s
`[patch.crates-io]`.

Tauri `2.10.3` still resolves `tauri-utils -> kuchikiki -> selectors 0.24 ->
phf_codegen 0.8 -> phf_generator 0.8 -> rand 0.7.3`, which is flagged by
`GHSA-cq8v-f236-94qc`. The only intentional change from upstream
`phf_generator 0.8.0` is bumping its `rand` dependency to patched `0.8.6`.

Remove this vendored patch once upstream Tauri no longer pulls `rand 0.7.x`.
