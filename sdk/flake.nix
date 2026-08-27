{
  description = "OpenSecret SDK - TypeScript and Rust development environment";

  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixos-unstable";
    # Keep Bun aligned with package.json and CI without advancing the SDK's
    # older Node/Rust/system package set. Update all three pins together.
    bun-nixpkgs.url = "github:NixOS/nixpkgs/5912c1772a44e31bf1c63c0390b90501e5026886";
    # Keep Sigstore's newer Node requirement isolated from the SDK toolchain.
    sigstore-nixpkgs.url = "github:NixOS/nixpkgs/241313f4e8e508cb9b13278c2b0fa25b9ca27163";
    flake-utils.url = "github:numtide/flake-utils";
    rust-overlay = {
      url = "github:oxalica/rust-overlay";
      inputs.nixpkgs.follows = "nixpkgs";
    };
  };

  outputs = { self, nixpkgs, bun-nixpkgs, sigstore-nixpkgs, flake-utils, rust-overlay }:
    flake-utils.lib.eachDefaultSystem (system:
      let
        overlays = [ rust-overlay.overlays.default ];
        pkgs = import nixpkgs { inherit system overlays; };
        bunPkgs = import bun-nixpkgs { inherit system; };
        sigstorePkgs = import sigstore-nixpkgs { inherit system; };
        sdkBun = assert bunPkgs.bun.version == "1.3.5"; bunPkgs.bun;

        cosignPlatforms = {
          x86_64-linux = "linux-amd64";
          aarch64-linux = "linux-arm64";
          x86_64-darwin = "darwin-amd64";
          aarch64-darwin = "darwin-arm64";
        };
        cosignHashes = {
          x86_64-linux = "sha256-92Iu088i5V4a5jd8CAl5/3eiLamYHBHfIiouREmR588=";
          aarch64-linux = "sha256-kOeuC139YPIIFrUsASrd9/wFXrzHvqTOgcQoyoUYwwI=";
          x86_64-darwin = "sha256-rNGA+LAVviUkDKM6vuih5WTrZc3xo87kclRW0tzrfaY=";
          aarch64-darwin = "sha256-3sHD+AIyCxnC+88tx7z7PyWOHBgaBGwjoaB0vfky8Qo=";
        };
        cosign_3_1_2 = pkgs.stdenvNoCC.mkDerivation {
          pname = "cosign";
          version = "3.1.2";
          src = pkgs.fetchurl {
            url = "https://github.com/sigstore/cosign/releases/download/v3.1.2/cosign-${cosignPlatforms.${system}}";
            hash = cosignHashes.${system};
          };
          dontUnpack = true;
          installPhase = ''
            install -Dm755 "$src" "$out/bin/cosign"
          '';
        };

        # Try to use rust-toolchain.toml if it exists, otherwise use stable
        rust = if builtins.pathExists ./rust-toolchain.toml
          then pkgs.rust-bin.fromRustupToolchainFile ./rust-toolchain.toml
          else pkgs.rust-bin.stable.latest.default;

        commonInputs = with pkgs; [
          # TypeScript/JavaScript tooling
          sdkBun
          sigstorePkgs.nodejs
          cosign_3_1_2
          nodePackages.typescript
          nodePackages.typescript-language-server

          # Rust tooling
          rust
          rust-analyzer
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
          darwin.apple_sdk.frameworks.Security
          darwin.apple_sdk.frameworks.SystemConfiguration
        ];

        linuxOnlyInputs = with pkgs; [
          gcc
        ];

        allInputs = commonInputs
          ++ pkgs.lib.optionals pkgs.stdenv.isDarwin darwinOnlyInputs
          ++ pkgs.lib.optionals pkgs.stdenv.isLinux linuxOnlyInputs;
      in
      {
        devShells.default = pkgs.mkShell {
          packages = allInputs;

          shellHook = ''
            echo "OpenSecret SDK Development Environment"
            echo "----------------------------------------"
            echo "TypeScript/Bun tools available"
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
            ''}
          '';
        };
      }
    );
}
