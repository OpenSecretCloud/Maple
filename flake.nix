{
  description = "Cross-platform Bun and Tauri development environment";

  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixos-unstable";
    flake-utils.url = "github:numtide/flake-utils";
    rust-overlay.url = "github:oxalica/rust-overlay";
  };

  outputs = { self, nixpkgs, flake-utils, rust-overlay }:
    flake-utils.lib.eachDefaultSystem (system:
      let
        overlays = [ (import rust-overlay) ];
        pkgs = import nixpkgs {
          inherit system overlays;
        };
        
        # Use specific rust version required by Tauri
        rustToolchain = pkgs.rust-bin.stable."1.88.0".default.override {
          extensions = [ "rust-src" ];
        };
      in
      {
        devShells.default = pkgs.mkShell {
          packages = [
            pkgs.bun
            pkgs.just
            pkgs.jq
            rustToolchain
            
            # Basic build dependencies
            pkgs.pkg-config
            pkgs.openssl
            
            # GTK dependencies for Tauri
            pkgs.atk
            pkgs.gtk3
            pkgs.glib
            pkgs.pango
            pkgs.cairo
            pkgs.gdk-pixbuf
            pkgs.libsoup_3
            pkgs.webkitgtk_4_1
          ] ++ (if pkgs.stdenv.isLinux then [
            # Mesa with software rendering support for environments without a GPU
            # (e.g., VMs, CI, containers). Provides libEGL_mesa.so and swrast DRI
            # drivers needed by WebKitGTK to initialize EGL.
            pkgs.mesa
          ] else []) ++ (if pkgs.stdenv.isDarwin then [
            # macOS-specific dependencies
            pkgs.darwin.apple_sdk.frameworks.WebKit
            pkgs.darwin.apple_sdk.frameworks.AppKit
          ] else []);
          
          # Set environment variables
          shellHook = ''
            echo "Using Rust version: $(rustc --version)"
            echo "Tauri development environment ready"
          ''
          # On Linux, configure Mesa EGL for software rendering so WebKitGTK works
          # in environments without a GPU (VMs, containers, headless servers).
          + pkgs.lib.optionalString pkgs.stdenv.isLinux ''
            export __EGL_VENDOR_LIBRARY_FILENAMES=${pkgs.mesa}/share/glvnd/egl_vendor.d/50_mesa.json
            export LIBGL_DRIVERS_PATH=${pkgs.mesa}/lib/dri
            export LIBGL_ALWAYS_SOFTWARE=1
            export WEBKIT_DISABLE_COMPOSITING_MODE=1
            export WEBKIT_DISABLE_DMABUF_RENDERER=1
          '';
        };
      }
    );
}
