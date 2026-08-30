{
  description = "Native Maple desktop app built with GPUI";

  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixpkgs-unstable";

    rust-overlay = {
      url = "github:oxalica/rust-overlay";
      inputs.nixpkgs.follows = "nixpkgs";
    };
  };

  outputs =
    {
      nixpkgs,
      rust-overlay,
      ...
    }:
    let
      supportedSystems = [
        "aarch64-darwin"
        "aarch64-linux"
        "x86_64-linux"
      ];
      forAllSystems = nixpkgs.lib.genAttrs supportedSystems;
    in
    {
      packages = forAllSystems (
        system:
        let
          pkgs = import nixpkgs {
            inherit system;
            overlays = [ (import rust-overlay) ];
          };
          rustToolchain = pkgs.rust-bin.stable.latest.default.override {
            extensions = [
              "clippy"
              "rustfmt"
            ];
          };
          rustPlatform = pkgs.makeRustPlatform {
            cargo = rustToolchain;
            rustc = rustToolchain;
          };
          isDarwin = pkgs.stdenv.hostPlatform.isDarwin;
          linuxBuildInputs = with pkgs; [
            alsa-lib
            fontconfig
            freetype
            libxkbcommon
            vulkan-loader
            wayland
          ];
        in
        {
          default = rustPlatform.buildRustPackage {
            pname = "maple-gpui";
            version = "0.1.0";

            src = pkgs.lib.fileset.toSource {
              root = ./.;
              fileset = pkgs.lib.fileset.unions [
                ./Cargo.lock
                ./Cargo.toml
                ./app
                ./crates
              ];
            };

            cargoLock = {
              lockFile = ./Cargo.lock;
              outputHashes = {
                "goose-1.47.0" = "sha256-+sowkBtUbpBPAgi1Tn1WSgIac2yzCWsXcsh96Pp5VSY=";
                "opensecret-3.6.2" = "sha256-v1vBeVj5xrRQovm9oKmEkMSmUtcHE+4M7SJO8LYsYOs=";
              };
            };

            nativeBuildInputs = with pkgs; [
              clang
              cmake
              pkg-config
            ];

            buildInputs =
              [ pkgs.libiconv ]
              ++ pkgs.lib.optionals pkgs.stdenv.hostPlatform.isLinux linuxBuildInputs;

            # Apple's Metal compiler is not part of the redistributable SDK.
            # The pure Darwin package embeds the shader source and compiles it
            # at runtime. The dev shell below uses Xcode to precompile shaders.
            cargoBuildFlags =
              [
                "-p"
                "maple-gpui"
              ]
              ++ pkgs.lib.optionals isDarwin [
                "--features"
                "gpui/runtime_shaders"
              ];

            # The upstream CI runs the complete workspace and feature matrix.
            # Keep the package derivation focused on producing the release binary.
            doCheck = false;

            meta = {
              description = "Native Maple desktop app built with GPUI";
              homepage = "https://github.com/benthecarman/maple-gpui";
              license = pkgs.lib.licenses.mit;
              mainProgram = "maple-gpui";
              platforms = supportedSystems;
            };
          };
        }
      );

      devShells = forAllSystems (
        system:
        let
          pkgs = import nixpkgs {
            inherit system;
            overlays = [ (import rust-overlay) ];
          };
          rustToolchain = pkgs.rust-bin.stable.latest.default.override {
            extensions = [
              "clippy"
              "rustfmt"
            ];
          };
          linuxBuildInputs = with pkgs; [
            alsa-lib
            fontconfig
            freetype
            libxkbcommon
            vulkan-loader
            wayland
          ];
          isDarwin = pkgs.stdenv.hostPlatform.isDarwin;
          xcrun = pkgs.writeShellScriptBin "xcrun" ''
            exec /usr/bin/xcrun "$@"
          '';
        in
        {
          default = pkgs.mkShell {
            packages = with pkgs; [
              clang
              cmake
              pkg-config
              rustToolchain
            ] ++ pkgs.lib.optionals isDarwin [ xcrun ];

            buildInputs =
              [ pkgs.libiconv ]
              ++ pkgs.lib.optionals pkgs.stdenv.hostPlatform.isLinux linuxBuildInputs;

            shellHook = pkgs.lib.optionalString isDarwin ''
              export DEVELOPER_DIR="/Applications/Xcode.app/Contents/Developer"
              export PATH="${xcrun}/bin:$PATH"
            '';
          };
        }
      );
    };
}
