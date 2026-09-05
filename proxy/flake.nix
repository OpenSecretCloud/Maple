{
  description = "Maple Proxy - Rust development environment";

  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixos-unstable";
    flake-utils.url = "github:numtide/flake-utils";
    rust-overlay = {
      url = "github:oxalica/rust-overlay";
      inputs.nixpkgs.follows = "nixpkgs";
    };
  };

  outputs =
    {
      self,
      nixpkgs,
      flake-utils,
      rust-overlay,
    }:
    flake-utils.lib.eachDefaultSystem (
      system:
      let
        overlays = [ rust-overlay.overlays.default ];
        pkgs = import nixpkgs { inherit system overlays; };

        # Try to use rust-toolchain.toml if it exists, otherwise use stable
        rust =
          if builtins.pathExists ./rust-toolchain.toml then
            pkgs.rust-bin.fromRustupToolchainFile ./rust-toolchain.toml
          else
            pkgs.rust-bin.stable.latest.default;

        commonInputs = with pkgs; [
          # Rust tooling
          rust
          rust-analyzer
          cargo-machete
          pkg-config
          openssl
          zlib
          clang
          libclang

          # Useful tools
          jq
          just
        ];

        darwinOnlyInputs = with pkgs; [
          libiconv
          apple-sdk
        ];

        linuxOnlyInputs = with pkgs; [
          gcc

          # Container runtime for Docker compatibility
          podman
          conmon
          slirp4netns
          fuse-overlayfs
        ];

        allInputs =
          commonInputs
          ++ pkgs.lib.optionals pkgs.stdenv.isDarwin darwinOnlyInputs
          ++ pkgs.lib.optionals pkgs.stdenv.isLinux linuxOnlyInputs;
      in
      {
        devShells.default = pkgs.mkShell {
          packages = allInputs;

          shellHook = ''
            echo "Maple Proxy Development Environment"
            echo "-----------------------------------"
            echo "Rust toolchain: $(rustc --version)"
            echo ""

            # Set up Rust environment variables
            export LIBCLANG_PATH=${pkgs.libclang.lib}/lib/
            export LD_LIBRARY_PATH=${pkgs.openssl}/lib:$LD_LIBRARY_PATH
            export PKG_CONFIG_PATH=${pkgs.openssl.dev}/lib/pkgconfig

            ${pkgs.lib.optionalString pkgs.stdenv.isDarwin ''
              # macOS-specific setup
              export RUST_BACKTRACE=1
            ''}

            ${pkgs.lib.optionalString pkgs.stdenv.isLinux ''
              # Linux-specific setup
              export RUST_BACKTRACE=1

              # Podman as Docker replacement
              alias docker='podman'
              echo "Using 'podman' as an alias for 'docker'"
              echo "You can now use 'docker' commands, which will be executed by podman"
            ''}
          '';
        };
      }
    );
}
