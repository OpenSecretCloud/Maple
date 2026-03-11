{
  description = "RMP app dev environment";

  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixpkgs-unstable";
    flake-utils.url = "github:numtide/flake-utils";
    rust-overlay = {
      url = "github:oxalica/rust-overlay";
      inputs.nixpkgs.follows = "nixpkgs";
    };
    android-nixpkgs = {
      url = "github:tadfisher/android-nixpkgs";
      inputs.nixpkgs.follows = "nixpkgs";
    };
  };

  outputs = { nixpkgs, flake-utils, rust-overlay, android-nixpkgs, ... }:
    flake-utils.lib.eachDefaultSystem (system:
      let
        pkgs = import nixpkgs {
          inherit system;
          overlays = [ (import rust-overlay) ];
          config.allowUnfree = true;
          config.android_sdk.accept_license = true;
        };

        rustToolchain = pkgs.rust-bin.stable.latest.default.override {
          extensions = [ "rust-src" "rust-analyzer" ];
          targets = [
            "aarch64-linux-android"
            "armv7-linux-androideabi"
            "x86_64-linux-android"
            "aarch64-apple-ios"
            "aarch64-apple-ios-sim"
            "x86_64-apple-ios"
          ];
        };

        androidSdk =
          if builtins.hasAttr system android-nixpkgs.sdk then
            android-nixpkgs.sdk.${system} (sdkPkgs: with sdkPkgs; [
              cmdline-tools-latest
              platform-tools
              build-tools-34-0-0
              build-tools-35-0-0
              platforms-android-34
              platforms-android-35
              ndk-28-2-13676358
              emulator
              (if pkgs.stdenv.isDarwin
               then system-images-android-35-google-apis-arm64-v8a
               else system-images-android-35-google-apis-x86-64)
            ])
          else
            null;

        rmp = pkgs.writeShellScriptBin "rmp" ''
          set -euo pipefail
          if [ -z "''${RMP_REPO:-}" ]; then
            echo "error: set RMP_REPO to a checkout containing the rmp-cli crate" >&2
            exit 2
          fi
          # Support both workspace layout ($RMP_REPO/Cargo.toml with -p rmp-cli)
          # and standalone layout ($RMP_REPO/rmp-cli/Cargo.toml).
          if [ -f "$RMP_REPO/Cargo.toml" ]; then
            exec cargo run --manifest-path "$RMP_REPO/Cargo.toml" -p rmp-cli -- "$@"
          elif [ -f "$RMP_REPO/rmp-cli/Cargo.toml" ]; then
            exec cargo run --manifest-path "$RMP_REPO/rmp-cli/Cargo.toml" -- "$@"
          else
            echo "error: RMP_REPO=$RMP_REPO does not contain rmp-cli" >&2
            exit 2
          fi
        '';

        shell = pkgs.mkShell {
          buildInputs = pkgs.lib.optionals pkgs.stdenv.isDarwin [
            pkgs.libiconv
          ];

          packages = [
            rustToolchain
            pkgs.just
            pkgs.nodejs_22
            pkgs.python3
            pkgs.curl
            pkgs.file
            pkgs.git
            pkgs.gradle
            pkgs.patchelf
            pkgs.pkg-config
            pkgs.adwaita-icon-theme
            pkgs.fontconfig
            pkgs.dconf
            pkgs.cairo
            pkgs.gdk-pixbuf
            pkgs.gdk-pixbuf.dev
            pkgs.glib
            pkgs.glib.dev
            pkgs.glib-networking
            pkgs.gsettings-desktop-schemas
            pkgs.graphene
            pkgs.gtk4
            pkgs.hicolor-icon-theme
            pkgs.isocodes
            pkgs.libxkbcommon
            pkgs.pango
            pkgs.shared-mime-info
            pkgs.wayland
            pkgs.xkeyboard-config
            pkgs.libx11
            rmp
          ] ++ pkgs.lib.optionals (androidSdk != null) [
            androidSdk
          ] ++ pkgs.lib.optionals pkgs.stdenv.isDarwin [
            pkgs.xcodegen
          ];

          shellHook = ''
            export IN_NIX_SHELL=1

            if [ -n "${if androidSdk != null then "1" else ""}" ]; then
              export ANDROID_HOME=${if androidSdk != null then "${androidSdk}/share/android-sdk" else ""}
              export ANDROID_SDK_ROOT="$ANDROID_HOME"
              export ANDROID_NDK_HOME="$ANDROID_HOME/ndk/28.2.13676358"
              export PATH=$ANDROID_HOME/emulator:$ANDROID_HOME/platform-tools:$ANDROID_HOME/cmdline-tools/latest/bin:$PATH
            fi

            # Auto-detect rmp-cli in parent directory
            if [ -z "''${RMP_REPO:-}" ]; then
              _parent="$(cd .. 2>/dev/null && pwd)"
              if [ -f "$_parent/rmp-cli/Cargo.toml" ]; then
                export RMP_REPO="$_parent"
              fi
            fi

            if [ "$(uname -s)" = "Darwin" ]; then
              if [ -n "''${DEVELOPER_DIR:-}" ] && [ -x "''${DEVELOPER_DIR}/usr/bin/simctl" ]; then
                DEV_DIR="$DEVELOPER_DIR"
              else
                DEV_DIR="$(xcode-select -p 2>/dev/null || true)"
              fi
              if [ -n "$DEV_DIR" ] && [ -d "$DEV_DIR/Toolchains/XcodeDefault.xctoolchain/usr/bin" ]; then
                export DEVELOPER_DIR="$DEV_DIR"
                TOOLCHAIN_BIN="$DEV_DIR/Toolchains/XcodeDefault.xctoolchain/usr/bin"
                export CC="$TOOLCHAIN_BIN/clang"
                export CXX="$TOOLCHAIN_BIN/clang++"
                export AR="$TOOLCHAIN_BIN/ar"
                export RANLIB="$TOOLCHAIN_BIN/ranlib"
              fi
            fi

            if [ -n "''${ANDROID_HOME:-}" ]; then
              mkdir -p android
              cat > android/local.properties <<EOF
              sdk.dir=$ANDROID_HOME
EOF
            fi

            echo ""
            echo "RMP app dev environment ready"
            echo "  Rust: $(rustc --version)"
            echo "  RMP repo: ''${RMP_REPO:-(not set)}"
            echo ""
          '';
        };
      in {
        devShells.default = shell;
      }
    );
}
