{
  description = "Reproducible Maple web, Tauri desktop, Android, and Apple build environments";

  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixos-unstable";
    flake-utils.url = "github:numtide/flake-utils";
    rust-overlay.url = "github:oxalica/rust-overlay";
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
        overlays = [ (import rust-overlay) ];
        pkgs = import nixpkgs {
          inherit system overlays;
          config = {
            allowUnfree = true;
            android_sdk.accept_license = true;
          };
        };
        lib = pkgs.lib;
        supportsAndroidHost = system == "x86_64-linux";

        versions = {
          bun = "1.3.5";
          rust = "1.88.0";
          jdk = "21";
          xcode = "26.5";
          android = {
            compileSdk = "35";
            platforms = [
              "34"
              "35"
              "36"
            ];
            buildTools = [
              "35.0.0"
              "34.0.0"
            ];
            ndk = "27.2.12479018";
          };
        };

        androidRustTargets = [
          "aarch64-linux-android"
          "armv7-linux-androideabi"
          "i686-linux-android"
          "x86_64-linux-android"
        ];
        appleRustTargets = [
          "aarch64-apple-darwin"
          "x86_64-apple-darwin"
          "aarch64-apple-ios"
          "aarch64-apple-ios-sim"
        ];
        hostRustTarget =
          if pkgs.stdenv.hostPlatform.config == "arm64-apple-darwin" then
            "aarch64-apple-darwin"
          else
            pkgs.stdenv.hostPlatform.config;
        nixRustTargets = lib.unique (
          [ hostRustTarget ]
          ++ lib.optionals supportsAndroidHost androidRustTargets
          ++ lib.optionals pkgs.stdenv.isDarwin appleRustTargets
        );

        rustToolchain = pkgs.rust-bin.stable.${versions.rust}.default.override {
          extensions = [
            "rust-src"
            "rustfmt"
            "clippy"
          ];
          targets =
            lib.optionals supportsAndroidHost androidRustTargets
            ++ lib.optionals pkgs.stdenv.isDarwin appleRustTargets;
        };

        rustupShim = pkgs.writeShellScriptBin "rustup" ''
          set -euo pipefail

          target_list="''${MAPLE_NIX_RUST_TARGETS:-}"

          list_installed() {
            local target
            for target in ''${target_list}; do
              printf '%s\n' "''${target}"
            done
          }

          has_target() {
            local requested="''${1:?target is required}"
            local target
            for target in ''${target_list}; do
              if [ "''${target}" = "''${requested}" ]; then
                return 0
              fi
            done
            return 1
          }

          case "''${1:-}" in
            target)
              shift
              case "''${1:-}" in
                list)
                  shift
                  if [ "''${1:-}" = "--installed" ]; then
                    list_installed
                    exit 0
                  fi

                  list_installed | while IFS= read -r target; do
                    printf '%s (installed)\n' "''${target}"
                  done
                  exit 0
                  ;;
                add)
                  shift
                  requested_targets=()
                  while [ "''${#}" -gt 0 ]; do
                    case "''${1}" in
                      --toolchain | -t)
                        shift 2
                        ;;
                      --*)
                        shift
                        ;;
                      *)
                        requested_targets+=("''${1}")
                        shift
                        ;;
                    esac
                  done

                  for target in "''${requested_targets[@]}"; do
                    if ! has_target "''${target}"; then
                      echo "rustup is shimmed by the Nix shell and target ''${target} is not provided by this flake." >&2
                      exit 1
                    fi
                  done
                  exit 0
                  ;;
              esac
              ;;
            show)
              if [ "''${2:-}" = "active-toolchain" ]; then
                printf 'nix-%s (default)\n' "''${MAPLE_NIX_RUST_VERSION:-unknown}"
                exit 0
              fi
              ;;
          esac

          echo "rustup is shimmed by the Maple Nix shell; use rustc/cargo from the flake toolchain." >&2
          exit 1
        '';

        jdk = pkgs.jdk21;

        commonPackages = with pkgs; [
          bash
          bun
          coreutils
          curl
          file
          findutils
          gawk
          git
          gnumake
          gnused
          gnutar
          gzip
          jq
          just
          minisign
          openssl
          pkg-config
          python3
          sccache
          unzip
          zip
          actionlint
          rustToolchain
        ];
        ciPackages = [ rustupShim ] ++ commonPackages;

        linuxTauriPackages =
          with pkgs;
          lib.optionals stdenv.isLinux [
            atk
            bubblewrap
            cairo
            gdk-pixbuf
            glib
            glib-networking
            gst_all_1.gstreamer
            gst_all_1.gst-libav
            gst_all_1.gst-plugins-bad
            gst_all_1.gst-plugins-base
            gst_all_1.gst-plugins-good
            gtk3
            librsvg
            libsoup_3
            mesa
            pango
            patchelf
            desktop-file-utils
            rpm
            webkitgtk_4_1
            xdg-utils
          ];

        linuxRuntimeClosure =
          if pkgs.stdenv.isLinux then
            pkgs.closureInfo {
              rootPaths = linuxTauriPackages ++ [ pkgs.stdenv.cc.cc.lib ];
            }
          else
            null;

        linuxTauriToolsArch =
          if pkgs.stdenv.hostPlatform.isx86_64 then
            "x86_64"
          else if pkgs.stdenv.hostPlatform.isAarch64 then
            "aarch64"
          else
            null;

        linuxTauriToolHashes = {
          appRun = {
            aarch64 = "sha256-By8XwIlahcSQKC/lOVxQB+X8ddpyflU7O4+2gP6xFXg=";
            x86_64 = "sha256-8wFApDoKWeRtshve/fdJuenyxpRukq+rus+YuK5z+08=";
          };
          linuxdeploy = {
            aarch64 = "sha256-sStcxXvQkh4fmNc/WKo2RQO8Gif1S3pp/ShwvOf6L1U=";
            x86_64 = "sha256-52K+qFyOsNSzUI1G5cHwN/cX0PkwOuO0qvyLBJkfoe8=";
          };
          appimagePlugin = {
            aarch64 = "sha256-Ak4f3LJchgv9hSN5I6lO0VubrAwCqQ/xCpCcsIdmxfU=";
            x86_64 = "sha256-Egjmp7HiZG4/sAbeqQC3K9hI7IYyS5l5t8lD8hHGacg=";
          };
          gtkPlugin = "sha256-yzefmwcz6a2fi9ePjC+gOK7yR4Uju31MjmT/ah6jUBo=";
          gstreamerPlugin = "sha256-wQe0nYTtv/xqsibtEAfgYmpPeqLDo2t3gr72I1HUnpQ=";
        };

        tauriLinuxdeployTools =
          if pkgs.stdenv.isLinux && linuxTauriToolsArch != null then
            let
              arch = linuxTauriToolsArch;
              linuxdeployArch = if arch == "i686" then "i386" else arch;
              appRun = pkgs.fetchurl {
                url = "https://github.com/tauri-apps/binary-releases/releases/download/apprun-old/AppRun-${arch}";
                hash = linuxTauriToolHashes.appRun.${arch};
              };
              linuxdeploy = pkgs.fetchurl {
                url = "https://github.com/tauri-apps/binary-releases/releases/download/linuxdeploy/linuxdeploy-${linuxdeployArch}.AppImage";
                hash = linuxTauriToolHashes.linuxdeploy.${linuxdeployArch};
              };
              appimagePlugin = pkgs.fetchurl {
                url = "https://github.com/linuxdeploy/linuxdeploy-plugin-appimage/releases/download/continuous/linuxdeploy-plugin-appimage-${arch}.AppImage";
                hash = linuxTauriToolHashes.appimagePlugin.${arch};
              };
              gtkPlugin = pkgs.fetchurl {
                url = "https://raw.githubusercontent.com/tauri-apps/linuxdeploy-plugin-gtk/master/linuxdeploy-plugin-gtk.sh";
                hash = linuxTauriToolHashes.gtkPlugin;
              };
              gstreamerPlugin = pkgs.fetchurl {
                url = "https://raw.githubusercontent.com/tauri-apps/linuxdeploy-plugin-gstreamer/master/linuxdeploy-plugin-gstreamer.sh";
                hash = linuxTauriToolHashes.gstreamerPlugin;
              };
            in
            pkgs.runCommand "maple-tauri-linuxdeploy-tools-${arch}" { } ''
              mkdir -p "$out"
              install -m 0755 ${appRun} "$out/AppRun-${arch}"
              install -m 0755 ${linuxdeploy} "$out/linuxdeploy-${linuxdeployArch}.AppImage"
              install -m 0755 ${appimagePlugin} "$out/linuxdeploy-plugin-appimage.real.AppImage"
              install -m 0755 ${gtkPlugin} "$out/linuxdeploy-plugin-gtk.sh"
              install -m 0755 ${gstreamerPlugin} "$out/linuxdeploy-plugin-gstreamer.sh"
            ''
          else
            null;

        gstreamerPlugins =
          pkgs.symlinkJoin {
            name = "maple-gstreamer-plugins";
            paths = with pkgs.gst_all_1; [
              gstreamer.out
              gst-libav
              gst-plugins-bad
              gst-plugins-base
              gst-plugins-good
            ];
          };

        darwinPackages =
          with pkgs;
          lib.optionals stdenv.isDarwin [
            cmake
            libiconv
            python3
          ];

        androidComposition =
          if supportsAndroidHost then
            pkgs.androidenv.composeAndroidPackages {
              platformVersions = versions.android.platforms;
              buildToolsVersions = versions.android.buildTools;
              ndkVersions = [ versions.android.ndk ];
              includeNDK = true;
              includeEmulator = false;
              includeSystemImages = false;
              abiVersions = [
                "arm64-v8a"
                "armeabi-v7a"
                "x86"
                "x86_64"
              ];
            }
          else
            null;

        androidPackages = lib.optionals pkgs.stdenv.isLinux [
          pkgs.apksigner
          jdk
        ] ++ lib.optionals supportsAndroidHost [ androidComposition.androidsdk ];

        mkShellForHost = if pkgs.stdenv.isDarwin && pkgs ? mkShellNoCC then pkgs.mkShellNoCC else pkgs.mkShell;

        linuxShellHook = lib.optionalString pkgs.stdenv.isLinux ''
          export MAPLE_NIX_GCC_LIB=${pkgs.stdenv.cc.cc.lib}
          export MAPLE_NIX_GDK_PIXBUF_BINARYDIR=${pkgs.gdk-pixbuf}/lib/gdk-pixbuf-2.0/2.10.0
          export MAPLE_NIX_GDK_PIXBUF_MODULEDIR=${pkgs.gdk-pixbuf}/lib/gdk-pixbuf-2.0/2.10.0/loaders
          export MAPLE_NIX_GLIB_SCHEMAS=${pkgs.glib.dev}/share/glib-2.0/schemas
          export MAPLE_NIX_GTK_LIB=${pkgs.gtk3}/lib
          export MAPLE_NIX_LINUX_CLOSURE_INFO=${linuxRuntimeClosure}
          ${lib.optionalString (tauriLinuxdeployTools != null) "export MAPLE_NIX_TAURI_LINUXDEPLOY_TOOLS=${tauriLinuxdeployTools}"}
          ${lib.optionalString (linuxTauriToolsArch != null) "export MAPLE_NIX_TAURI_LINUXDEPLOY_ARCH=${linuxTauriToolsArch}"}
          export GSTREAMER_PLUGINS_DIR=${gstreamerPlugins}/lib/gstreamer-1.0
          export GSTREAMER_HELPERS_DIR=${pkgs.gst_all_1.gstreamer.out}/libexec/gstreamer-1.0
          export __EGL_VENDOR_LIBRARY_FILENAMES=${pkgs.mesa}/share/glvnd/egl_vendor.d/50_mesa.json
          export LIBGL_DRIVERS_PATH=${pkgs.mesa}/lib/dri
          export LIBGL_ALWAYS_SOFTWARE=1
          export WEBKIT_DISABLE_COMPOSITING_MODE=1
          export WEBKIT_DISABLE_DMABUF_RENDERER=1
          export GIO_MODULE_DIR=${pkgs.glib-networking}/lib/gio/modules
        '';

        androidShellHook = lib.optionalString supportsAndroidHost ''
          export JAVA_HOME=${jdk}
          export ANDROID_HOME=${androidComposition.androidsdk}/libexec/android-sdk
          export ANDROID_SDK_ROOT="$ANDROID_HOME"

          if [ -d "$ANDROID_HOME/ndk/${versions.android.ndk}" ]; then
            export NDK_HOME="$ANDROID_HOME/ndk/${versions.android.ndk}"
          elif [ -d "$ANDROID_HOME/ndk-bundle" ]; then
            export NDK_HOME="$ANDROID_HOME/ndk-bundle"
          fi
        '' + lib.optionalString (pkgs.stdenv.isLinux && !supportsAndroidHost) ''
          export MAPLE_ANDROID_UNSUPPORTED_HOST=1
          echo "Android PR builds require x86_64-linux; current Nix system is ${system}."
        '';

        commonShellHook = ''
          export MAPLE_NIX_BUN_VERSION=${versions.bun}
          export MAPLE_NIX_RUST_VERSION=${versions.rust}
          export MAPLE_NIX_RUST_TARGETS="${lib.concatStringsSep " " nixRustTargets}"
          export MAPLE_NIX_JDK_VERSION=${versions.jdk}
          export MAPLE_NIX_XCODE_VERSION=${versions.xcode}
          export MAPLE_NIX_GNUTAR=${pkgs.gnutar}/bin/tar
          export MAPLE_NIX_GZIP=${pkgs.gzip}/bin/gzip
          ${lib.optionalString pkgs.stdenv.isDarwin "export MAPLE_NIX_LIBICONV=${pkgs.libiconv}"}
          export CARGO_TERM_COLOR=always
          export RUST_BACKTRACE=1
          echo "Maple Nix toolchain: bun $(bun --version), $(rustc --version)"
        '';

        pathShellHook = packages: ''
          export PATH="${lib.makeBinPath packages}:$PATH"
        '';

        mkNixApp =
          name: shell: script:
          (flake-utils.lib.mkApp {
            drv = pkgs.writeShellApplication {
              inherit name;
              runtimeInputs = [ pkgs.nix ];
              text = ''
                exec nix develop ".#${shell}" -c ${script} "$@"
              '';
            };
          }) // {
            meta.description = "Run ${script} through nix develop .#${shell}";
          };
      in
      {
        devShells = {
          default =
            let
              shellPackages = commonPackages ++ [ jdk ] ++ linuxTauriPackages ++ darwinPackages;
            in
            mkShellForHost {
              packages = shellPackages;
              shellHook = pathShellHook shellPackages + commonShellHook + linuxShellHook;
            };

          ci =
            let
              shellPackages = ciPackages ++ [ jdk ] ++ linuxTauriPackages ++ darwinPackages;
            in
            mkShellForHost {
              packages = shellPackages;
              shellHook = pathShellHook shellPackages + commonShellHook + linuxShellHook;
            };
        }
        // lib.optionalAttrs pkgs.stdenv.isLinux {
          android =
            let
              shellPackages = ciPackages ++ androidPackages;
            in
            mkShellForHost {
              packages = shellPackages;
              shellHook = pathShellHook shellPackages + commonShellHook + androidShellHook;
            };
        }
        // lib.optionalAttrs pkgs.stdenv.isDarwin {
          apple =
            let
              shellPackages = ciPackages ++ [ jdk ] ++ darwinPackages;
            in
            mkShellForHost {
              packages = shellPackages;
              shellHook = pathShellHook shellPackages + commonShellHook;
            };
        };

        checks = {
          toolchain = pkgs.runCommand "maple-toolchain-check" { nativeBuildInputs = commonPackages ++ [ jdk ]; } ''
            test "$(bun --version)" = "${versions.bun}"
            rustc --version | grep -F "rustc ${versions.rust}"
            java -version 2>&1 | grep -F 'version "${versions.jdk}.'
            touch "$out"
          '';

          workflows = pkgs.runCommand "maple-github-workflows-check" {
            nativeBuildInputs = [ pkgs.actionlint ];
            src = ./.github/workflows;
          } ''
            cd "$src"
            actionlint \
              android-pr-build.yml \
              android-build.yml \
              desktop-pr-build.yml \
              desktop-build.yml \
              frontend-tests.yml \
              mobile-pr-build.yml \
              mobile-build.yml \
              release.yml \
              rust-tests.yml \
              web-build.yml \
              zapstore-publish.yml
            touch "$out"
          '';
        };

        apps = {
          ci-frontend = mkNixApp "maple-ci-frontend" "ci" "./scripts/ci/frontend.sh";
          ci-rust = mkNixApp "maple-ci-rust" "ci" "./scripts/ci/rust.sh";
          ci-desktop-pr = mkNixApp "maple-ci-desktop-pr" "ci" "./scripts/ci/desktop-pr.sh";
          ci-desktop-release = mkNixApp "maple-ci-desktop-release" "ci" "./scripts/ci/desktop-release.sh";
          ci-signed-desktop-release-rehearsal = mkNixApp "maple-ci-signed-desktop-release-rehearsal" "ci" "./scripts/ci/signed-release-rehearsal.sh desktop";
          ci-latest-json = mkNixApp "maple-ci-latest-json" "ci" "./scripts/ci/latest-json.sh";
          ci-verify-release-artifacts = mkNixApp "maple-ci-verify-release-artifacts" "ci" "./scripts/ci/verify-release-artifacts.sh";
        }
        // lib.optionalAttrs pkgs.stdenv.isLinux {
          ci-android-pr = mkNixApp "maple-ci-android-pr" "android" "./scripts/ci/android-pr.sh";
          ci-android-release = mkNixApp "maple-ci-android-release" "android" "./scripts/ci/android-release.sh";
          ci-signed-android-release-rehearsal = mkNixApp "maple-ci-signed-android-release-rehearsal" "android" "./scripts/ci/signed-release-rehearsal.sh android";
        }
        // lib.optionalAttrs pkgs.stdenv.isDarwin {
          ci-ios-onnxruntime = mkNixApp "maple-ci-ios-onnxruntime" "apple" "./scripts/ci/ios-onnxruntime.sh";
          ci-ios-pr = mkNixApp "maple-ci-ios-pr" "apple" "./scripts/ci/ios-pr.sh";
          ci-ios-release = mkNixApp "maple-ci-ios-release" "apple" "./scripts/ci/ios-release.sh";
          ci-signed-ios-release-rehearsal = mkNixApp "maple-ci-signed-ios-release-rehearsal" "apple" "./scripts/ci/signed-release-rehearsal.sh ios";
        };
      }
    );
}
