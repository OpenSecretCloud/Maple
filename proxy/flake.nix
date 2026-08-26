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
          pkg-config
          openssl
          zlib
          gcc
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

              # Podman configuration
              export CONTAINERS_CONF=$HOME/.config/containers/containers.conf
              export CONTAINERS_POLICY=$HOME/.config/containers/policy.json
              mkdir -p $HOME/.config/containers
              echo '{"default":[{"type":"insecureAcceptAnything"}]}' > $CONTAINERS_POLICY

              # Create a basic containers.conf if it doesn't exist
              if [ ! -f $CONTAINERS_CONF ]; then
                echo "[engine]
              cgroup_manager = \"cgroupfs\"
              events_logger = \"file\"
              runtime = \"crun\"

              [storage]
              driver = \"vfs\"" > $CONTAINERS_CONF
              fi

              # Ensure correct permissions
              chmod 600 $CONTAINERS_POLICY $CONTAINERS_CONF 2>/dev/null || true
            ''}
          '';
        };
      }
    );
}
