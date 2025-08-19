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
        rustToolchain = pkgs.rust-bin.stable."1.85.0".default.override {
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
          ] ++ (if pkgs.stdenv.isDarwin then [
            # macOS-specific dependencies
            pkgs.darwin.apple_sdk.frameworks.WebKit
            pkgs.darwin.apple_sdk.frameworks.AppKit
          ] else []);
          
          # Set environment variables
          shellHook = ''
            echo "Using Rust version: $(rustc --version)"
            echo "Tauri development environment ready"
          '';
        };
      }
    );
}
