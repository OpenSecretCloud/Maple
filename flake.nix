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
          linuxRuntimeInputs = with pkgs; [
            libxcb
            libxkbcommon
            mesa
            vulkan-loader
            wayland
          ];
          linuxBuildInputs = with pkgs; [
            alsa-lib
            fontconfig
            freetype
          ] ++ linuxRuntimeInputs;
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
            ] ++ pkgs.lib.optionals pkgs.stdenv.hostPlatform.isLinux [ pkgs.makeWrapper ];

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

            # GPUI loads the Wayland and Vulkan libraries at runtime, so they are
            # not discovered by ELF dependency scanning. Prefer the NixOS GPU
            # driver link and retain Mesa as a portable fallback elsewhere.
            postFixup = pkgs.lib.optionalString pkgs.stdenv.hostPlatform.isLinux ''
              wrapProgram "$out/bin/maple-gpui" \
                --prefix LD_LIBRARY_PATH : "${pkgs.addDriverRunpath.driverLink}/lib:${pkgs.lib.makeLibraryPath linuxRuntimeInputs}" \
                --suffix VK_ADD_DRIVER_FILES : "${pkgs.addDriverRunpath.driverLink}/share/vulkan/icd.d:${pkgs.mesa}/share/vulkan/icd.d"
            '';

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
          linuxRuntimeInputs = with pkgs; [
            libxcb
            libxkbcommon
            mesa
            vulkan-loader
            wayland
          ];
          linuxBuildInputs = with pkgs; [
            alsa-lib
            fontconfig
            freetype
          ] ++ linuxRuntimeInputs;
          isDarwin = pkgs.stdenv.hostPlatform.isDarwin;
          mkDevShell = if isDarwin then pkgs.mkShellNoCC else pkgs.mkShell;
          xcrun = pkgs.writeShellScriptBin "xcrun" ''
            exec /usr/bin/xcrun "$@"
          '';
        in
        {
          default = mkDevShell {
            packages = with pkgs; [
              clang
              cmake
              pkg-config
              rustToolchain
            ] ++ pkgs.lib.optionals isDarwin [ xcrun ];

            buildInputs =
              [ pkgs.libiconv ]
              ++ pkgs.lib.optionals pkgs.stdenv.hostPlatform.isLinux linuxBuildInputs;

            shellHook = pkgs.lib.optionalString pkgs.stdenv.hostPlatform.isLinux ''
              export LD_LIBRARY_PATH="${pkgs.addDriverRunpath.driverLink}/lib:${pkgs.lib.makeLibraryPath linuxRuntimeInputs}''${LD_LIBRARY_PATH:+:$LD_LIBRARY_PATH}"
              export VK_ADD_DRIVER_FILES="${pkgs.addDriverRunpath.driverLink}/share/vulkan/icd.d:${pkgs.mesa}/share/vulkan/icd.d''${VK_ADD_DRIVER_FILES:+:$VK_ADD_DRIVER_FILES}"
            '' + pkgs.lib.optionalString isDarwin ''
              maple_nix_valid_developer_dir() {
                [ -d "$1" ] \
                  && [ -x "$1/usr/bin/xcodebuild" ] \
                  && DEVELOPER_DIR="$1" /usr/bin/xcrun --sdk macosx --show-sdk-path >/dev/null 2>&1
              }

              maple_nix_developer_dir=""
              if [ -n "''${MAPLE_NIX_XCODE_VERSION:-}" ]; then
                maple_nix_developer_dir="/Applications/Xcode_''${MAPLE_NIX_XCODE_VERSION}.app/Contents/Developer"
                if ! maple_nix_valid_developer_dir "$maple_nix_developer_dir"; then
                  echo "Maple Nix shell: Xcode ''${MAPLE_NIX_XCODE_VERSION} was not found at $maple_nix_developer_dir." >&2
                  echo "Install that version, or unset MAPLE_NIX_XCODE_VERSION and set DEVELOPER_DIR to a full Xcode installation." >&2
                  exit 1
                fi
              elif [ -n "''${DEVELOPER_DIR:-}" ] && maple_nix_valid_developer_dir "$DEVELOPER_DIR"; then
                maple_nix_developer_dir="$DEVELOPER_DIR"
              elif maple_nix_valid_developer_dir "/Applications/Xcode.app/Contents/Developer"; then
                maple_nix_developer_dir="/Applications/Xcode.app/Contents/Developer"
              else
                maple_nix_selected_developer_dir="$(DEVELOPER_DIR= /usr/bin/xcode-select -p 2>/dev/null || true)"
                if maple_nix_valid_developer_dir "$maple_nix_selected_developer_dir"; then
                  maple_nix_developer_dir="$maple_nix_selected_developer_dir"
                else
                  echo "Maple Nix shell: no full Xcode installation was found." >&2
                  echo "Install Xcode, select it with xcode-select, or set MAPLE_NIX_XCODE_VERSION/DEVELOPER_DIR." >&2
                  exit 1
                fi
              fi

              export DEVELOPER_DIR="$maple_nix_developer_dir"
              export SDKROOT="$(/usr/bin/xcrun --sdk macosx --show-sdk-path)"
              # Native Rust dependencies such as aws-lc ask cc-rs to compile
              # against the macOS SDK. Point cc-rs at Xcode's actual compiler
              # so it recognizes the Apple toolchain and supplies -isysroot;
              # nixpkgs' generic clang wrapper cannot infer that SDK boundary.
              export CC="$(/usr/bin/xcrun --find clang)"
              export CXX="$(/usr/bin/xcrun --find clang++)"
              export PATH="${xcrun}/bin:$PATH"

              unset maple_nix_developer_dir maple_nix_selected_developer_dir
              unset -f maple_nix_valid_developer_dir
            '';
          };
        }
      );
    };
}
