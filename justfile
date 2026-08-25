# Build a fresh npm tarball and publish that exact artifact.
publish-npm:
    nix develop --no-update-lock-file -c bun install --frozen-lockfile --ignore-scripts
    nix develop --no-update-lock-file -c bun run build
    nix develop --no-update-lock-file -c bun pm pack --filename opensecret-react-publish.tgz
    nix develop --no-update-lock-file -c npm publish ./opensecret-react-publish.tgz --access public

# Publish the Rust crate using the committed lockfile.
publish-cargo:
    nix develop --no-update-lock-file -c cargo publish --locked --manifest-path rust/Cargo.toml
