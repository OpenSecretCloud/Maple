use std::path::{Path, PathBuf};
use std::process::Command;

use crate::cli::{human_log, json_print, CliError, JsonOk};

pub fn init(
    cwd: &Path,
    json: bool,
    verbose: bool,
    args: crate::cli::InitArgs,
) -> Result<(), CliError> {
    let include_ios = resolve_toggle(args.ios, args.no_ios, true);
    let include_android = resolve_toggle(args.android, args.no_android, true);
    let include_iced = resolve_toggle(args.iced, args.no_iced, false);
    let include_flake = resolve_toggle(args.flake, args.no_flake, false);
    let include_git = resolve_toggle(args.git, args.no_git, include_flake);
    if !include_ios && !include_android && !include_iced {
        return Err(CliError::user(
            "at least one platform must be enabled (use --ios, --android, or --iced)",
        ));
    }

    let requested = PathBuf::from(&args.name);
    let project_dir_name = requested
        .file_name()
        .and_then(|s| s.to_str())
        .map(|s| s.to_string())
        .ok_or_else(|| CliError::user("project name must be a valid path segment"))?;

    let dest = if requested.is_absolute() {
        requested.clone()
    } else {
        cwd.join(&requested)
    };

    if dest.exists() {
        return Err(CliError::user(format!(
            "destination already exists: {}",
            dest.to_string_lossy()
        )));
    }

    let org = args.org.unwrap_or_else(|| "com.example".to_string());
    validate_org(&org)?;

    let id_segment = java_identifier_segment(&project_dir_name);
    let bundle_id = args
        .bundle_id
        .unwrap_or_else(|| format!("{org}.{id_segment}"));
    let app_id = args.app_id.unwrap_or_else(|| format!("{org}.{id_segment}"));

    validate_bundle_like("bundle id", &bundle_id)?;
    validate_bundle_like("app id", &app_id)?;

    let display_name = display_name(&project_dir_name);

    // Derive Rust crate/lib name from the project name.
    let crate_name = rust_crate_name(&project_dir_name);
    let lib_name = crate_name.replace('-', "_");
    let iced_package = format!("{crate_name}_desktop_iced");

    // Kotlin package path segments from the app_id (e.g., "com.example.myapp" → "com/example/myapp").
    let kotlin_pkg = &app_id;
    let kotlin_pkg_path = kotlin_pkg.replace('.', "/");

    human_log(
        verbose,
        format!(
            "initializing project '{}' at {}",
            project_dir_name,
            dest.to_string_lossy()
        ),
    );

    std::fs::create_dir_all(&dest)
        .map_err(|e| CliError::operational(format!("failed to create destination: {e}")))?;

    // ── Root files ──────────────────────────────────────────────────────
    write_text(&dest.join(".gitignore"), &tpl_gitignore())?;
    write_text(&dest.join("Cargo.toml"), &tpl_workspace_toml(include_iced))?;
    write_text(
        &dest.join("rmp.toml"),
        &tpl_rmp_toml(
            &project_dir_name,
            &org,
            &crate_name,
            &bundle_id,
            &app_id,
            include_ios,
            include_android,
            include_iced,
            &iced_package,
        ),
    )?;
    write_text(
        &dest.join("justfile"),
        &tpl_justfile(
            include_ios,
            include_android,
            include_iced,
            &crate_name,
            &lib_name,
            &iced_package,
        ),
    )?;
    write_text(
        &dest.join("README.md"),
        &tpl_readme(
            &display_name,
            include_ios,
            include_android,
            include_iced,
            include_flake,
        ),
    )?;
    if include_flake {
        write_text(&dest.join("flake.nix"), &tpl_flake_nix())?;
        write_text(&dest.join(".envrc"), &tpl_envrc())?;
    }

    // ── Rust core ───────────────────────────────────────────────────────
    let rust_dir = dest.join("rust");
    std::fs::create_dir_all(rust_dir.join("src"))
        .map_err(|e| CliError::operational(format!("create rust/src: {e}")))?;
    write_text(&rust_dir.join("Cargo.toml"), &tpl_rust_cargo(&crate_name))?;
    write_text(&rust_dir.join("build.rs"), "fn main() {}\n")?;
    write_text(&rust_dir.join("src/lib.rs"), &tpl_rust_lib())?;
    write_text(
        &rust_dir.join("uniffi.toml"),
        &tpl_uniffi_toml(kotlin_pkg, &lib_name),
    )?;

    // ── uniffi-bindgen ──────────────────────────────────────────────────
    let ub_dir = dest.join("uniffi-bindgen");
    std::fs::create_dir_all(ub_dir.join("src"))
        .map_err(|e| CliError::operational(format!("create uniffi-bindgen/src: {e}")))?;
    write_text(&ub_dir.join("Cargo.toml"), &tpl_uniffi_bindgen_cargo())?;
    write_text(
        &ub_dir.join("src/main.rs"),
        "fn main() {\n    uniffi::uniffi_bindgen_main()\n}\n",
    )?;

    // ── iOS ─────────────────────────────────────────────────────────────
    if include_ios {
        let ios_dir = dest.join("ios");
        let src_dir = ios_dir.join("Sources");
        let assets_dir = src_dir.join("Assets.xcassets/AppIcon.appiconset");
        std::fs::create_dir_all(&assets_dir)
            .map_err(|e| CliError::operational(format!("create ios dirs: {e}")))?;
        std::fs::create_dir_all(src_dir.join("Assets.xcassets"))
            .map_err(|e| CliError::operational(format!("create assets dir: {e}")))?;

        write_text(
            &ios_dir.join("project.yml"),
            &tpl_ios_project_yml(&bundle_id, &lib_name),
        )?;
        write_text(
            &ios_dir.join("Info.plist"),
            &tpl_ios_info_plist(&display_name),
        )?;
        write_text(
            &src_dir.join("App.swift"),
            &tpl_ios_app_swift(&display_name),
        )?;
        write_text(&src_dir.join("AppManager.swift"), &tpl_ios_app_manager())?;
        write_text(
            &src_dir.join("ContentView.swift"),
            &tpl_ios_content_view(&display_name),
        )?;
        write_text(
            &assets_dir.join("Contents.json"),
            &tpl_ios_appicon_contents(),
        )?;
        write_text(
            &src_dir.join("Assets.xcassets/Contents.json"),
            "{\"info\":{\"version\":1,\"author\":\"xcode\"}}\n",
        )?;
    }

    // ── Android ─────────────────────────────────────────────────────────
    if include_android {
        let android_dir = dest.join("android");
        let app_dir = android_dir.join("app");
        let main_dir = app_dir.join("src/main");
        let java_dir = main_dir.join(format!("java/{kotlin_pkg_path}"));
        let ui_dir = java_dir.join("ui");
        let theme_dir = ui_dir.join("theme");
        let res_dir = main_dir.join("res");
        let rust_bindings_dir = java_dir.join("rust");
        let gradle_dir = android_dir.join("gradle/wrapper");

        for d in [
            &java_dir,
            &ui_dir,
            &theme_dir,
            &rust_bindings_dir,
            &res_dir.join("values"),
            &res_dir.join("mipmap-hdpi"),
            &gradle_dir,
        ] {
            std::fs::create_dir_all(d)
                .map_err(|e| CliError::operational(format!("create android dirs: {e}")))?;
        }

        write_text(
            &android_dir.join("build.gradle.kts"),
            &tpl_android_root_gradle(),
        )?;
        write_text(
            &android_dir.join("settings.gradle.kts"),
            &tpl_android_settings_gradle(&display_name),
        )?;
        write_text(
            &android_dir.join("gradle.properties"),
            &tpl_android_gradle_properties(),
        )?;
        write_text(
            &app_dir.join("build.gradle.kts"),
            &tpl_android_app_gradle(&app_id, kotlin_pkg, &lib_name),
        )?;
        write_text(
            &main_dir.join("AndroidManifest.xml"),
            &tpl_android_manifest(kotlin_pkg, &display_name),
        )?;
        write_text(
            &java_dir.join("MainActivity.kt"),
            &tpl_android_main_activity(kotlin_pkg, &display_name),
        )?;
        write_text(
            &java_dir.join("AppManager.kt"),
            &tpl_android_app_manager(kotlin_pkg, &lib_name),
        )?;
        write_text(
            &ui_dir.join("MainApp.kt"),
            &tpl_android_main_app(kotlin_pkg, &display_name),
        )?;
        write_text(&theme_dir.join("Theme.kt"), &tpl_android_theme(kotlin_pkg))?;
        write_text(
            &res_dir.join("values/strings.xml"),
            &tpl_android_strings(&display_name),
        )?;
        write_text(&res_dir.join("values/themes.xml"), &tpl_android_themes())?;
        // Placeholder empty Kotlin file so Gradle's ensureUniffiGenerated doesn't fail
        // before bindings are generated.
        write_text(
            &rust_bindings_dir.join(format!("{lib_name}.kt")),
            &tpl_android_placeholder_bindings(kotlin_pkg, &lib_name),
        )?;

        // Minimal gradlew (just enough to exist; users normally get this from wrapper).
        write_gradlew(&android_dir)?;
    }

    // ── Desktop (ICED) ────────────────────────────────────────────────────
    if include_iced {
        let desktop_dir = dest.join("desktop/iced");
        std::fs::create_dir_all(desktop_dir.join("src"))
            .map_err(|e| CliError::operational(format!("create desktop/iced/src: {e}")))?;
        write_text(
            &desktop_dir.join("Cargo.toml"),
            &tpl_desktop_iced_cargo(&iced_package, &crate_name),
        )?;
        write_text(
            &desktop_dir.join("src/main.rs"),
            &tpl_desktop_iced_main(&display_name, &lib_name),
        )?;
    }

    if include_git {
        init_git_repo(&dest)?;
    }

    // ── Done ────────────────────────────────────────────────────────────
    if json {
        let mut platforms: Vec<&str> = vec![];
        if include_ios {
            platforms.push("ios");
        }
        if include_android {
            platforms.push("android");
        }
        if include_iced {
            platforms.push("iced");
        }
        json_print(&JsonOk {
            ok: true,
            data: serde_json::json!({
                "path": dest,
                "project": {
                    "name": project_dir_name,
                    "org": org,
                    "bundle_id": bundle_id,
                    "app_id": app_id,
                    "crate_name": crate_name,
                    "iced_package": iced_package,
                    "flake": include_flake,
                    "git": include_git,
                },
                "platforms": platforms,
            }),
        });
    } else {
        eprintln!("ok: initialized project at {}", dest.to_string_lossy());
        if include_ios {
            eprintln!("  ios bundle id: {bundle_id}");
        }
        if include_android {
            eprintln!("  android app id: {app_id}");
        }
        if include_iced {
            eprintln!("  desktop package: {iced_package}");
        }
        if include_flake {
            eprintln!("  nix shell: flake.nix + .envrc generated (--flake)");
        }
        if include_git {
            eprintln!("  git: initialized local repo + staged scaffold");
        }
        if include_flake {
            eprintln!(
                "  next: cd {} && nix develop -c just doctor",
                dest.to_string_lossy()
            );
        } else {
            eprintln!("  next: cd {} && rmp doctor", dest.to_string_lossy());
        }
    }

    Ok(())
}

// ── Helpers ─────────────────────────────────────────────────────────────────

fn resolve_toggle(include_flag: bool, exclude_flag: bool, default_value: bool) -> bool {
    if exclude_flag {
        false
    } else if include_flag {
        true
    } else {
        default_value
    }
}

fn write_text(path: &Path, content: &str) -> Result<(), CliError> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| {
            CliError::operational(format!("failed to create {}: {e}", parent.display()))
        })?;
    }
    std::fs::write(path, content)
        .map_err(|e| CliError::operational(format!("failed to write {}: {e}", path.display())))?;
    Ok(())
}

fn init_git_repo(dest: &Path) -> Result<(), CliError> {
    let init = Command::new("git")
        .args(["init", "-q"])
        .current_dir(dest)
        .status()
        .map_err(|e| CliError::operational(format!("failed to run `git init`: {e}")))?;
    if !init.success() {
        return Err(CliError::operational(
            "failed to initialize git repo (`git init` exited non-zero)",
        ));
    }

    let add = Command::new("git")
        .args(["add", "."])
        .current_dir(dest)
        .status()
        .map_err(|e| CliError::operational(format!("failed to run `git add .`: {e}")))?;
    if !add.success() {
        return Err(CliError::operational(
            "failed to stage scaffold files (`git add .` exited non-zero)",
        ));
    }

    Ok(())
}

fn validate_org(org: &str) -> Result<(), CliError> {
    if org.trim().is_empty() || !org.contains('.') {
        return Err(CliError::user(
            "--org must be reverse-DNS style (for example: com.example)",
        ));
    }
    validate_bundle_like("org", org)
}

fn validate_bundle_like(label: &str, value: &str) -> Result<(), CliError> {
    if value.trim().is_empty() || !value.contains('.') {
        return Err(CliError::user(format!(
            "{label} must be dot-separated (for example: com.example.app)",
        )));
    }
    for seg in value.split('.') {
        if seg.is_empty()
            || !seg
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-')
        {
            return Err(CliError::user(format!(
                "{label} has invalid segment `{seg}` in `{value}`",
            )));
        }
    }
    Ok(())
}

fn java_identifier_segment(input: &str) -> String {
    let mut out = String::new();
    for c in input.chars() {
        if c.is_ascii_alphanumeric() {
            out.push(c.to_ascii_lowercase());
        }
    }
    if out.is_empty() {
        "app".to_string()
    } else if out.chars().next().unwrap().is_ascii_digit() {
        format!("app{out}")
    } else {
        out
    }
}

fn rust_crate_name(input: &str) -> String {
    let mut out = String::new();
    for c in input.chars() {
        if c.is_ascii_alphanumeric() || c == '-' || c == '_' {
            out.push(c.to_ascii_lowercase());
        } else if c == ' ' {
            out.push('_');
        }
    }
    if out.is_empty() {
        "app_core".to_string()
    } else {
        // Ensure it ends with _core for clarity.
        if !out.ends_with("_core") && !out.ends_with("-core") {
            out.push_str("_core");
        }
        out
    }
}

fn display_name(input: &str) -> String {
    let mut parts: Vec<String> = vec![];
    for tok in input
        .split(|c: char| !c.is_ascii_alphanumeric())
        .filter(|s| !s.is_empty())
    {
        let mut chars = tok.chars();
        if let Some(first) = chars.next() {
            let mut part = String::new();
            part.push(first.to_ascii_uppercase());
            for ch in chars {
                part.push(ch.to_ascii_lowercase());
            }
            parts.push(part);
        }
    }
    if parts.is_empty() {
        "App".to_string()
    } else {
        parts.join(" ")
    }
}

// ── Template functions ──────────────────────────────────────────────────────

fn tpl_gitignore() -> String {
    r#"/target
.direnv/
.DS_Store
*.swp
*.swo
flake.lock
staging/
ios/Bindings/
ios/Frameworks/
ios/.build/
ios/*.xcodeproj
android/app/src/main/jniLibs/
android/.gradle/
android/build/
android/app/build/
android/local.properties
"#
    .to_string()
}

fn tpl_workspace_toml(include_iced: bool) -> String {
    let mut members = vec!["  \"rust\",", "  \"uniffi-bindgen\","];
    if include_iced {
        members.push("  \"desktop/iced\",");
    }
    format!(
        "[workspace]\nresolver = \"2\"\nmembers = [\n{}\n]\n",
        members.join("\n")
    )
}

fn tpl_rmp_toml(
    project_name: &str,
    org: &str,
    crate_name: &str,
    bundle_id: &str,
    app_id: &str,
    include_ios: bool,
    include_android: bool,
    include_iced: bool,
    iced_package: &str,
) -> String {
    let mut out = format!(
        r#"[project]
name = "{project_name}"
org = "{org}"

[core]
crate = "{crate_name}"
bindings = "uniffi"
"#
    );

    if include_ios {
        out.push_str(&format!(
            r#"
[ios]
bundle_id = "{bundle_id}"
"#
        ));
    }

    if include_android {
        out.push_str(&format!(
            r#"
[android]
app_id = "{app_id}"
"#
        ));
    }

    if include_iced {
        out.push_str(&format!(
            r#"
[desktop]
targets = ["iced"]

[desktop.iced]
package = "{iced_package}"
"#
        ));
    }

    out
}

fn tpl_justfile(
    include_ios: bool,
    include_android: bool,
    include_iced: bool,
    crate_name: &str,
    lib_name: &str,
    iced_package: &str,
) -> String {
    let xcf_name = pascal_case(lib_name);
    let header = format!(
        "set shell := [\"bash\", \"-c\"]\n\
         \n\
         CORE_CRATE := \"{crate_name}\"\n\
         LIB_NAME := \"{lib_name}\"\n\
         XCF_NAME := \"{xcf_name}\"\n\
         ICED_PACKAGE := \"{iced_package}\"\n",
    );
    let mut out = header;
    out.push_str(
        r#"
default:
  @just --list

doctor:
  rmp doctor

# Build Rust core for the host (needed for uniffi-bindgen).
rust-build-host:
  cargo build -p {{CORE_CRATE}} --release

bindings:
  rmp bindings all
"#,
    );

    if include_ios {
        out.push_str(
            r#"
# ── iOS ──────────────────────────────────────────────────────────────────────

XCODE_DEV := "/Applications/Xcode.app/Contents/Developer"
TOOLCHAIN_BIN := XCODE_DEV / "Toolchains/XcodeDefault.xctoolchain/usr/bin"

run-ios:
  rmp run ios

ios-gen-swift: rust-build-host
  rmp bindings swift

# Cross-compile Rust for iOS device and simulator (arm64).
ios-rust:
  #!/usr/bin/env bash
  set -e
  IOS_SDK="{{XCODE_DEV}}/Platforms/iPhoneOS.platform/Developer/SDKs/iPhoneOS.sdk"
  SIM_SDK="{{XCODE_DEV}}/Platforms/iPhoneSimulator.platform/Developer/SDKs/iPhoneSimulator.sdk"
  for pair in "aarch64-apple-ios $IOS_SDK -miphoneos-version-min=17.0" \
              "aarch64-apple-ios-sim $SIM_SDK -mios-simulator-version-min=17.0"; do
    set -- $pair; TARGET=$1; SDK=$2; VFLAG=$3
    env -u SDKROOT -u MACOSX_DEPLOYMENT_TARGET -u CC -u CXX -u AR -u RANLIB -u LIBRARY_PATH \
      DEVELOPER_DIR="{{XCODE_DEV}}" SDKROOT="$SDK" CC="{{TOOLCHAIN_BIN}}/clang" \
      RUSTFLAGS="-C linker={{TOOLCHAIN_BIN}}/clang -C link-arg=$VFLAG -C link-arg=-isysroot -C link-arg=$SDK" \
      cargo build -p {{CORE_CRATE}} --lib --target "$TARGET" --release
  done

# Package static libs into an xcframework.
ios-xcframework:
  #!/usr/bin/env bash
  set -e
  rm -rf ios/Frameworks/{{XCF_NAME}}.xcframework staging
  mkdir -p staging/headers
  cp ios/Bindings/{{LIB_NAME}}FFI.h staging/headers/
  cp ios/Bindings/{{LIB_NAME}}FFI.modulemap staging/headers/module.modulemap
  xcodebuild -create-xcframework \
    -library target/aarch64-apple-ios/release/lib{{LIB_NAME}}.a -headers staging/headers \
    -library target/aarch64-apple-ios-sim/release/lib{{LIB_NAME}}.a -headers staging/headers \
    -output ios/Frameworks/{{XCF_NAME}}.xcframework
  rm -rf staging

ios-xcodeproj:
  cd ios && xcodegen generate

# Build the iOS app for simulator (run outside Nix shell if linking fails).
ios-build:
  #!/usr/bin/env bash
  set -e
  env -u NIX_LDFLAGS -u NIX_CFLAGS_COMPILE -u SDKROOT -u MACOSX_DEPLOYMENT_TARGET \
    -u CC -u CXX -u AR -u RANLIB -u LIBRARY_PATH \
    DEVELOPER_DIR="{{XCODE_DEV}}" \
    xcodebuild build \
      -project ios/App.xcodeproj -scheme App \
      -destination "generic/platform=iOS Simulator" \
      -configuration Debug CODE_SIGNING_ALLOWED=NO ARCHS=arm64 ONLY_ACTIVE_ARCH=YES

# Full iOS pipeline: host build → bindings → cross-compile → xcframework → xcodegen → build.
ios-full: ios-gen-swift ios-rust ios-xcframework ios-xcodeproj ios-build
"#,
        );
    }

    if include_android {
        out.push_str(
            r#"
# ── Android ──────────────────────────────────────────────────────────────────

run-android:
  rmp run android

gen-kotlin: rust-build-host
  rmp bindings kotlin

# Cross-compile Rust for Android ABIs.
android-rust:
  cargo ndk -o android/app/src/main/jniLibs -P 26 \
    -t arm64-v8a -t armeabi-v7a -t x86_64 \
    build -p {{CORE_CRATE}} --release

android-assemble:
  cd android && ./gradlew :app:assembleDebug

# Full Android pipeline: host build → bindings → cross-compile → assemble.
android-full: gen-kotlin android-rust android-assemble
"#,
        );
    }

    if include_iced {
        out.push_str(
            r#"
# ── Desktop (iced) ───────────────────────────────────────────────────────────

run-iced:
  cargo run -p {{ICED_PACKAGE}}
"#,
        );
    }

    out.push_str(
        r#"
# ── Utilities ────────────────────────────────────────────────────────────────

# Rebuild Rust + regenerate bindings for all platforms (no platform build).
rebind: rust-build-host bindings
"#,
    );

    out
}

fn tpl_readme(
    display_name: &str,
    include_ios: bool,
    include_android: bool,
    include_iced: bool,
    include_flake: bool,
) -> String {
    let mut s = if include_flake {
        format!(
            r#"# {display_name}

A cross-platform app built with [RMP](https://github.com/nickthecook/rmp) (Rust Multiplatform).

## Quick Start

```bash
nix develop -c just doctor
nix develop -c just bindings
"#
        )
    } else {
        format!(
            r#"# {display_name}

A cross-platform app built with [RMP](https://github.com/nickthecook/rmp) (Rust Multiplatform).

## Quick Start

```bash
rmp doctor
rmp bindings all
"#
        )
    };
    if include_ios {
        if include_flake {
            s.push_str("nix develop -c just run-ios\n");
        } else {
            s.push_str("rmp run ios\n");
        }
    }
    if include_android {
        if include_flake {
            s.push_str("nix develop -c just run-android\n");
        } else {
            s.push_str("rmp run android\n");
        }
    }
    if include_iced {
        if include_flake {
            s.push_str("nix develop -c just run-iced\n");
        } else {
            s.push_str("rmp run iced\n");
        }
    }
    s.push_str("```\n");
    if include_flake {
        s.push_str("\nOptional: enable direnv (`.envrc` is included and uses `use flake`).\n");
    }
    s
}

fn tpl_envrc() -> String {
    "use flake\n".to_string()
}

fn tpl_flake_nix() -> String {
    r#"{
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

        androidSdk = android-nixpkgs.sdk.${system} (sdkPkgs: with sdkPkgs; [
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
        ]);

        rmp = pkgs.writeShellScriptBin "rmp" ''
          set -euo pipefail
          if [ -z "''${RMP_REPO:-}" ]; then
            echo "error: set RMP_REPO to a checkout containing the rmp-cli crate" >&2
            exit 2
          fi
          # Support both workspace layout ($RMP_REPO/Cargo.toml with -p rmp-cli)
          # and standalone layout ($RMP_REPO/rmp-cli/Cargo.toml).
          if [ -f "$RMP_REPO/Cargo.toml" ] && grep -q "rmp-cli" "$RMP_REPO/Cargo.toml" 2>/dev/null; then
            exec cargo run --manifest-path "$RMP_REPO/Cargo.toml" -p rmp-cli -- "$@"
          elif [ -f "$RMP_REPO/rmp-cli/Cargo.toml" ]; then
            exec cargo run --manifest-path "$RMP_REPO/rmp-cli/Cargo.toml" -- "$@"
          else
            echo "error: RMP_REPO=$RMP_REPO does not contain rmp-cli (checked workspace and standalone layouts)" >&2
            exit 2
          fi
        '';

        shell = pkgs.mkShell {
          buildInputs = pkgs.lib.optionals pkgs.stdenv.isDarwin [
            pkgs.libiconv
          ];

          packages = [
            rustToolchain
            androidSdk
            pkgs.just
            pkgs.nodejs_22
            pkgs.python3
            pkgs.curl
            pkgs.git
            pkgs.gradle
            rmp
          ] ++ pkgs.lib.optionals pkgs.stdenv.isDarwin [
            pkgs.xcodegen
          ];

          shellHook = ''
            export IN_NIX_SHELL=1
            export ANDROID_HOME=${androidSdk}/share/android-sdk
            export ANDROID_SDK_ROOT=${androidSdk}/share/android-sdk
            export ANDROID_NDK_HOME="$ANDROID_HOME/ndk/28.2.13676358"
            export PATH=$ANDROID_HOME/emulator:$ANDROID_HOME/platform-tools:$ANDROID_HOME/cmdline-tools/latest/bin:$PATH

            # Auto-detect rmp-cli in parent directory (common monorepo layout).
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

            mkdir -p android
            cat > android/local.properties <<EOF
            sdk.dir=$ANDROID_HOME
EOF

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
"#
    .to_string()
}

fn tpl_rust_cargo(crate_name: &str) -> String {
    format!(
        r#"[package]
name = "{crate_name}"
version = "0.1.0"
edition = "2021"

[lib]
crate-type = ["cdylib", "staticlib", "rlib"]

[dependencies]
flume = "0.11"
uniffi = "0.31.0"
"#
    )
}

fn tpl_rust_lib() -> String {
    r#"use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, RwLock};
use std::thread;

use flume::{Receiver, Sender};

uniffi::setup_scaffolding!();

// ── State ───────────────────────────────────────────────────────────────────

#[derive(uniffi::Record, Clone, Debug)]
pub struct AppState {
    pub rev: u64,
    pub greeting: String,
}

impl AppState {
    fn empty() -> Self {
        Self {
            rev: 0,
            greeting: "Hello from Rust!".to_string(),
        }
    }
}

// ── Actions & Updates ───────────────────────────────────────────────────────

#[derive(uniffi::Enum, Clone, Debug)]
pub enum AppAction {
    SetName { name: String },
}

#[derive(uniffi::Enum, Clone, Debug)]
pub enum AppUpdate {
    FullState(AppState),
}

// ── Callback interface ──────────────────────────────────────────────────────

#[uniffi::export(callback_interface)]
pub trait AppReconciler: Send + Sync + 'static {
    fn reconcile(&self, update: AppUpdate);
}

// ── FFI entry point ─────────────────────────────────────────────────────────

enum CoreMsg {
    Action(AppAction),
}

#[derive(uniffi::Object)]
pub struct FfiApp {
    core_tx: Sender<CoreMsg>,
    update_rx: Receiver<AppUpdate>,
    listening: AtomicBool,
    shared_state: Arc<RwLock<AppState>>,
}

#[uniffi::export]
impl FfiApp {
    #[uniffi::constructor]
    pub fn new(data_dir: String) -> Arc<Self> {
        let _ = data_dir; // reserved for future use

        let (update_tx, update_rx) = flume::unbounded();
        let (core_tx, core_rx) = flume::unbounded::<CoreMsg>();
        let shared_state = Arc::new(RwLock::new(AppState::empty()));

        let shared_for_core = shared_state.clone();
        thread::spawn(move || {
            let mut state = AppState::empty();
            let mut rev: u64 = 0;

            // Emit initial state.
            {
                let snapshot = state.clone();
                match shared_for_core.write() {
                    Ok(mut g) => *g = snapshot.clone(),
                    Err(p) => *p.into_inner() = snapshot.clone(),
                }
                let _ = update_tx.send(AppUpdate::FullState(snapshot));
            }

            while let Ok(msg) = core_rx.recv() {
                match msg {
                    CoreMsg::Action(action) => match action {
                        AppAction::SetName { name } => {
                            rev += 1;
                            state.rev = rev;
                            if name.trim().is_empty() {
                                state.greeting = "Hello from Rust!".to_string();
                            } else {
                                state.greeting = format!("Hello, {}!", name.trim());
                            }
                            let snapshot = state.clone();
                            match shared_for_core.write() {
                                Ok(mut g) => *g = snapshot.clone(),
                                Err(p) => *p.into_inner() = snapshot.clone(),
                            }
                            let _ = update_tx.send(AppUpdate::FullState(snapshot));
                        }
                    },
                }
            }
        });

        Arc::new(Self {
            core_tx,
            update_rx,
            listening: AtomicBool::new(false),
            shared_state,
        })
    }

    pub fn state(&self) -> AppState {
        match self.shared_state.read() {
            Ok(g) => g.clone(),
            Err(poison) => poison.into_inner().clone(),
        }
    }

    pub fn dispatch(&self, action: AppAction) {
        let _ = self.core_tx.send(CoreMsg::Action(action));
    }

    pub fn listen_for_updates(&self, reconciler: Box<dyn AppReconciler>) {
        if self
            .listening
            .compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst)
            .is_err()
        {
            return;
        }

        let rx = self.update_rx.clone();
        thread::spawn(move || {
            while let Ok(update) = rx.recv() {
                reconciler.reconcile(update);
            }
        });
    }
}
"#
    .to_string()
}

fn tpl_uniffi_toml(kotlin_pkg: &str, lib_name: &str) -> String {
    format!(
        r#"[bindings.kotlin]
package_name = "{kotlin_pkg}.rust"
cdylib_name = "{lib_name}"
"#
    )
}

fn tpl_uniffi_bindgen_cargo() -> String {
    r#"[package]
name = "uniffi-bindgen"
version = "0.1.0"
edition = "2021"
publish = false

[dependencies]
uniffi = { version = "0.31.0", features = ["cli"] }
"#
    .to_string()
}

// ── Desktop (ICED) templates ───────────────────────────────────────────────

fn tpl_desktop_iced_cargo(package: &str, core_crate: &str) -> String {
    format!(
        r#"[package]
name = "{package}"
version = "0.1.0"
edition = "2021"
publish = false

[dependencies]
{core_crate} = {{ path = "../../rust" }}
iced = "0.14"
flume = "0.11"
dirs-next = "2"
"#
    )
}

fn tpl_desktop_iced_main(display_name: &str, core_lib: &str) -> String {
    format!(
        r#"use iced::Center;
use iced::widget::{{button, column, container, text, text_input}};
use iced::{{Element, Fill, Subscription, Task, Theme}};
use std::sync::Arc;
use {core_lib}::{{AppAction, AppState, FfiApp}};

fn main() -> iced::Result {{
    iced::application(App::new, App::update, App::view)
        .title("{display_name}")
        .subscription(App::subscription)
        .run()
}}

// ── AppManager ──────────────────────────────────────────────────────────────

#[derive(Clone)]
struct AppManager {{
    ffi: Arc<FfiApp>,
    update_rx: flume::Receiver<()>,
}}

impl AppManager {{
    fn new() -> Result<Self, String> {{
        let data_dir = dirs_next::data_dir()
            .unwrap_or_else(|| std::path::PathBuf::from("."))
            .join("{display_name}")
            .to_string_lossy()
            .to_string();
        let _ = std::fs::create_dir_all(&data_dir);

        let ffi = FfiApp::new(data_dir);
        let (notify_tx, update_rx) = flume::unbounded();
        ffi.listen_for_updates(Box::new(DesktopReconciler {{ tx: notify_tx }}));

        Ok(Self {{ ffi, update_rx }})
    }}

    fn state(&self) -> AppState {{
        self.ffi.state()
    }}

    fn dispatch(&self, action: AppAction) {{
        self.ffi.dispatch(action);
    }}

    fn subscribe_updates(&self) -> flume::Receiver<()> {{
        self.update_rx.clone()
    }}
}}

struct DesktopReconciler {{
    tx: flume::Sender<()>,
}}

impl {core_lib}::AppReconciler for DesktopReconciler {{
    fn reconcile(&self, _update: {core_lib}::AppUpdate) {{
        let _ = self.tx.send(());
    }}
}}

fn manager_update_stream(manager: &AppManager) -> impl iced::futures::Stream<Item = ()> {{
    let rx = manager.subscribe_updates();
    iced::futures::stream::unfold(rx, |rx| async move {{
        match rx.recv_async().await {{
            Ok(()) => Some(((), rx)),
            Err(_) => None,
        }}
    }})
}}

// ── App ─────────────────────────────────────────────────────────────────────

enum App {{
    BootError {{ error: String }},
    Loaded {{
        manager: AppManager,
        state: AppState,
        name_input: String,
    }},
}}

#[derive(Debug, Clone)]
enum Message {{
    CoreUpdated,
    NameChanged(String),
    Apply,
}}

impl App {{
    fn new() -> (Self, Task<Message>) {{
        let app = match AppManager::new() {{
            Ok(manager) => {{
                let state = manager.state();
                Self::Loaded {{
                    manager,
                    state,
                    name_input: String::new(),
                }}
            }}
            Err(error) => Self::BootError {{ error }},
        }};
        (app, Task::none())
    }}

    fn subscription(&self) -> Subscription<Message> {{
        match self {{
            App::BootError {{ .. }} => Subscription::none(),
            App::Loaded {{ manager, .. }} => {{
                Subscription::run_with(manager.clone(), manager_update_stream)
                    .map(|_| Message::CoreUpdated)
            }}
        }}
    }}

    fn update(&mut self, message: Message) -> Task<Message> {{
        match self {{
            App::BootError {{ .. }} => {{}}
            App::Loaded {{
                manager,
                state,
                name_input,
            }} => match message {{
                Message::CoreUpdated => {{
                    let latest = manager.state();
                    if latest.rev > state.rev {{
                        *state = latest;
                    }}
                }}
                Message::NameChanged(name) => {{
                    *name_input = name;
                }}
                Message::Apply => {{
                    manager.dispatch(AppAction::SetName {{
                        name: name_input.clone(),
                    }});
                }}
            }},
        }}
        Task::none()
    }}

    fn view(&self) -> Element<'_, Message> {{
        match self {{
            App::BootError {{ error }} => container(
                column![
                    text("{display_name}").size(24),
                    text(error).color([0.8, 0.2, 0.2]),
                ]
                .spacing(12),
            )
            .center_x(Fill)
            .center_y(Fill)
            .into(),
            App::Loaded {{
                state, name_input, ..
            }} => container(
                column![
                    text("{display_name}").size(24),
                    text(&state.greeting).size(20),
                    text_input("Enter a name", name_input)
                        .on_input(Message::NameChanged)
                        .on_submit(Message::Apply),
                    button("Apply").on_press(Message::Apply),
                ]
                .padding(24)
                .spacing(12)
                .align_x(Center),
            )
            .center_x(Fill)
            .center_y(Fill)
            .into()
        }}
    }}
}}
"#
    )
}

// ── iOS templates ───────────────────────────────────────────────────────────

fn tpl_ios_project_yml(bundle_id: &str, lib_name: &str) -> String {
    // The Xcode project and target are always called "App" — neutral, no renaming needed.
    // The xcframework name derives from the lib name using PascalCase.
    let xcf_name = pascal_case(lib_name);
    format!(
        r#"name: App
options:
  bundleIdPrefix: {bundle_id}
  deploymentTarget:
    iOS: "17.0"
  createIntermediateGroups: true

settings:
  base:
    PRODUCT_BUNDLE_IDENTIFIER: {bundle_id}
    MARKETING_VERSION: 0.1.0
    CURRENT_PROJECT_VERSION: 1
    SWIFT_VERSION: 5.0
  configs:
    Debug:
      PRODUCT_BUNDLE_IDENTIFIER: {bundle_id}.dev

targets:
  App:
    type: application
    platform: iOS
    sources:
      - path: Sources
      - path: Bindings
        excludes:
          - "*.h"
          - "*.modulemap"
    settings:
      base:
        INFOPLIST_FILE: Info.plist
        ASSETCATALOG_COMPILER_APPICON_NAME: AppIcon
    dependencies:
      - framework: Frameworks/{xcf_name}.xcframework
        embed: false

schemes:
  App:
    build:
      targets:
        App: all
"#
    )
}

fn tpl_ios_info_plist(display_name: &str) -> String {
    format!(
        r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
	<key>CFBundleDevelopmentRegion</key>
	<string>en</string>
	<key>CFBundleDisplayName</key>
	<string>{display_name}</string>
	<key>CFBundleExecutable</key>
	<string>$(EXECUTABLE_NAME)</string>
	<key>CFBundleIdentifier</key>
	<string>$(PRODUCT_BUNDLE_IDENTIFIER)</string>
	<key>CFBundleName</key>
	<string>$(PRODUCT_NAME)</string>
	<key>CFBundlePackageType</key>
	<string>APPL</string>
	<key>CFBundleShortVersionString</key>
	<string>$(MARKETING_VERSION)</string>
	<key>CFBundleVersion</key>
	<string>$(CURRENT_PROJECT_VERSION)</string>
	<key>UILaunchScreen</key>
	<dict/>
	<key>UISupportedInterfaceOrientations</key>
	<array>
		<string>UIInterfaceOrientationPortrait</string>
	</array>
</dict>
</plist>
"#
    )
}

fn tpl_ios_app_swift(display_name: &str) -> String {
    format!(
        r#"import SwiftUI

@main
struct {entry_name}: App {{
    @State private var manager = AppManager()

    var body: some Scene {{
        WindowGroup {{
            ContentView(manager: manager)
        }}
    }}
}}
"#,
        entry_name = swift_app_entry_name(display_name),
    )
}

fn tpl_ios_app_manager() -> String {
    r#"import Foundation
import Observation

@MainActor
@Observable
final class AppManager: AppReconciler {
    let rust: FfiApp
    var state: AppState
    private var lastRevApplied: UInt64

    init() {
        let fm = FileManager.default
        let dataDirUrl = fm.urls(for: .applicationSupportDirectory, in: .userDomainMask).first!
        let dataDir = dataDirUrl.path
        try? fm.createDirectory(at: dataDirUrl, withIntermediateDirectories: true)

        let rust = FfiApp(dataDir: dataDir)
        self.rust = rust

        let initial = rust.state()
        self.state = initial
        self.lastRevApplied = initial.rev

        rust.listenForUpdates(reconciler: self)
    }

    nonisolated func reconcile(update: AppUpdate) {
        Task { @MainActor [weak self] in
            self?.apply(update: update)
        }
    }

    private func apply(update: AppUpdate) {
        switch update {
        case .fullState(let s):
            if s.rev <= lastRevApplied { return }
            lastRevApplied = s.rev
            state = s
        }
    }

    func dispatch(_ action: AppAction) {
        rust.dispatch(action: action)
    }
}
"#
    .to_string()
}

fn tpl_ios_content_view(display_name: &str) -> String {
    format!(
        r#"import SwiftUI

struct ContentView: View {{
    @Bindable var manager: AppManager
    @State private var nameInput = ""

    var body: some View {{
        VStack(spacing: 24) {{
            Text("{display_name}")
                .font(.largeTitle.weight(.semibold))

            Text(manager.state.greeting)
                .font(.title3)

            TextField("Enter your name", text: $nameInput)
                .textFieldStyle(.roundedBorder)
                .onSubmit {{
                    manager.dispatch(.setName(name: nameInput))
                }}

            Button("Greet") {{
                manager.dispatch(.setName(name: nameInput))
            }}
            .buttonStyle(.borderedProminent)
        }}
        .padding(20)
    }}
}}
"#
    )
}

fn tpl_ios_appicon_contents() -> String {
    r#"{
  "images" : [
    {
      "idiom" : "universal",
      "platform" : "ios",
      "size" : "1024x1024"
    }
  ],
  "info" : {
    "author" : "xcode",
    "version" : 1
  }
}
"#
    .to_string()
}

// ── Android templates ───────────────────────────────────────────────────────

fn tpl_android_root_gradle() -> String {
    r#"plugins {
    id("com.android.application") version "8.5.1" apply false
    id("org.jetbrains.kotlin.android") version "1.9.24" apply false
}
"#
    .to_string()
}

fn tpl_android_settings_gradle(display_name: &str) -> String {
    format!(
        r#"pluginManagement {{
    repositories {{
        google()
        mavenCentral()
        gradlePluginPortal()
    }}
}}
dependencyResolutionManagement {{
    repositoriesMode.set(RepositoriesMode.FAIL_ON_PROJECT_REPOS)
    repositories {{
        google()
        mavenCentral()
    }}
}}
rootProject.name = "{display_name}"
include(":app")
"#
    )
}

fn tpl_android_gradle_properties() -> String {
    r#"android.useAndroidX=true
kotlin.code.style=official
org.gradle.jvmargs=-Xmx2048m
"#
    .to_string()
}

fn tpl_android_app_gradle(app_id: &str, kotlin_pkg: &str, lib_name: &str) -> String {
    format!(
        r#"plugins {{
    id("com.android.application")
    id("org.jetbrains.kotlin.android")
}}

android {{
    namespace = "{kotlin_pkg}"
    compileSdk = 35
    ndkVersion = "28.2.13676358"

    defaultConfig {{
        applicationId = "{app_id}"
        minSdk = 26
        targetSdk = 35
        versionCode = 1
        versionName = "0.1.0"
    }}

    buildTypes {{
        debug {{
            applicationIdSuffix = ".dev"
            versionNameSuffix = "-dev"
        }}
        release {{
            isMinifyEnabled = false
            proguardFiles(
                getDefaultProguardFile("proguard-android-optimize.txt"),
            )
        }}
    }}

    buildFeatures {{
        compose = true
    }}

    composeOptions {{
        kotlinCompilerExtensionVersion = "1.5.14"
    }}

    compileOptions {{
        sourceCompatibility = JavaVersion.VERSION_17
        targetCompatibility = JavaVersion.VERSION_17
    }}

    kotlinOptions {{
        jvmTarget = "17"
    }}

    packaging {{
        resources.excludes.addAll(
            listOf("/META-INF/{{AL2.0,LGPL2.1}}", "META-INF/DEPENDENCIES"),
        )
    }}

    sourceSets {{
        getByName("main") {{
            jniLibs.srcDirs("src/main/jniLibs")
        }}
    }}
}}

tasks.register("ensureUniffiGenerated") {{
    doLast {{
        val out = file("src/main/java/{pkg_path}/rust/{lib_name}.kt")
        if (!out.exists()) {{
            throw GradleException("Missing UniFFI Kotlin bindings. Run `rmp bindings kotlin` first.")
        }}
    }}
}}

tasks.named("preBuild") {{
    dependsOn("ensureUniffiGenerated")
}}

dependencies {{
    val composeBom = platform("androidx.compose:compose-bom:2024.06.00")
    implementation(composeBom)

    implementation("androidx.core:core-ktx:1.13.1")
    implementation("androidx.activity:activity-compose:1.9.0")
    implementation("androidx.lifecycle:lifecycle-runtime-ktx:2.8.3")

    implementation("androidx.compose.ui:ui")
    implementation("androidx.compose.ui:ui-tooling-preview")
    implementation("androidx.compose.material3:material3")

    debugImplementation("androidx.compose.ui:ui-tooling")

    // UniFFI JNA
    implementation("net.java.dev.jna:jna:5.14.0@aar")
}}
"#,
        pkg_path = kotlin_pkg.replace('.', "/"),
    )
}

fn tpl_android_manifest(_kotlin_pkg: &str, display_name: &str) -> String {
    format!(
        r#"<?xml version="1.0" encoding="utf-8"?>
<manifest xmlns:android="http://schemas.android.com/apk/res/android">

    <uses-permission android:name="android.permission.INTERNET" />

    <application
        android:allowBackup="true"
        android:label="{display_name}"
        android:supportsRtl="true"
        android:theme="@style/Theme.App">

        <activity
            android:name=".MainActivity"
            android:exported="true">
            <intent-filter>
                <action android:name="android.intent.action.MAIN" />
                <category android:name="android.intent.category.DEFAULT" />
                <category android:name="android.intent.category.LAUNCHER" />
            </intent-filter>
        </activity>

    </application>

</manifest>
"#
    )
}

fn tpl_android_main_activity(kotlin_pkg: &str, _display_name: &str) -> String {
    format!(
        r#"package {kotlin_pkg}

import android.os.Bundle
import androidx.activity.ComponentActivity
import androidx.activity.compose.setContent
import {kotlin_pkg}.ui.MainApp
import {kotlin_pkg}.ui.theme.AppTheme

class MainActivity : ComponentActivity() {{
    private lateinit var manager: AppManager

    override fun onCreate(savedInstanceState: Bundle?) {{
        super.onCreate(savedInstanceState)
        manager = AppManager.getInstance(applicationContext)
        setContent {{
            AppTheme {{
                MainApp(manager = manager)
            }}
        }}
    }}
}}
"#
    )
}

fn tpl_android_app_manager(kotlin_pkg: &str, _lib_name: &str) -> String {
    format!(
        r#"package {kotlin_pkg}

import android.content.Context
import android.os.Handler
import android.os.Looper
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.setValue
import {kotlin_pkg}.rust.AppAction
import {kotlin_pkg}.rust.AppReconciler
import {kotlin_pkg}.rust.AppState
import {kotlin_pkg}.rust.AppUpdate
import {kotlin_pkg}.rust.FfiApp

class AppManager private constructor(context: Context) : AppReconciler {{
    private val mainHandler = Handler(Looper.getMainLooper())
    private val rust: FfiApp
    private var lastRevApplied: ULong = 0UL

    // NOTE: This placeholder is immediately replaced by the Rust core's
    // initial state snapshot in init{{}}. If you change AppState fields in
    // Rust, update this placeholder to match or the Kotlin code will not
    // compile -- this is intentional, ensuring the types stay in sync.
    var state: AppState by mutableStateOf(
        AppState(rev = 0UL, greeting = ""),
    )
        private set

    init {{
        val dataDir = context.filesDir.absolutePath
        rust = FfiApp(dataDir)
        val initial = rust.state()
        state = initial
        lastRevApplied = initial.rev
        rust.listenForUpdates(this)
    }}

    fun dispatch(action: AppAction) {{
        rust.dispatch(action)
    }}

    override fun reconcile(update: AppUpdate) {{
        mainHandler.post {{
            when (update) {{
                is AppUpdate.FullState -> {{
                    if (update.v1.rev <= lastRevApplied) return@post
                    lastRevApplied = update.v1.rev
                    state = update.v1
                }}
            }}
        }}
    }}

    companion object {{
        @Volatile
        private var instance: AppManager? = null

        fun getInstance(context: Context): AppManager =
            instance ?: synchronized(this) {{
                instance ?: AppManager(context.applicationContext).also {{ instance = it }}
            }}
    }}
}}
"#
    )
}

fn tpl_android_main_app(kotlin_pkg: &str, display_name: &str) -> String {
    format!(
        r#"package {kotlin_pkg}.ui

import androidx.compose.foundation.layout.*
import androidx.compose.material3.*
import androidx.compose.runtime.*
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.unit.dp
import {kotlin_pkg}.AppManager
import {kotlin_pkg}.rust.AppAction

@OptIn(ExperimentalMaterial3Api::class)
@Composable
fun MainApp(manager: AppManager) {{
    var nameInput by remember {{ mutableStateOf("") }}
    val state = manager.state

    Scaffold(
        topBar = {{
            TopAppBar(title = {{ Text("{display_name}") }})
        }},
    ) {{ padding ->
        Column(
            modifier = Modifier
                .fillMaxSize()
                .padding(padding)
                .padding(20.dp),
            horizontalAlignment = Alignment.CenterHorizontally,
            verticalArrangement = Arrangement.spacedBy(16.dp),
        ) {{
            Text(
                state.greeting,
                style = MaterialTheme.typography.headlineMedium,
            )

            OutlinedTextField(
                value = nameInput,
                onValueChange = {{ nameInput = it }},
                label = {{ Text("Enter your name") }},
                modifier = Modifier.fillMaxWidth(),
                singleLine = true,
            )

            Button(
                onClick = {{ manager.dispatch(AppAction.SetName(nameInput)) }},
                modifier = Modifier.fillMaxWidth(),
            ) {{
                Text("Greet")
            }}
        }}
    }}
}}
"#
    )
}

fn tpl_android_theme(kotlin_pkg: &str) -> String {
    format!(
        r#"package {kotlin_pkg}.ui.theme

import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.lightColorScheme
import androidx.compose.runtime.Composable

private val LightColors = lightColorScheme()

@Composable
fun AppTheme(content: @Composable () -> Unit) {{
    MaterialTheme(
        colorScheme = LightColors,
        content = content,
    )
}}
"#
    )
}

fn tpl_android_strings(display_name: &str) -> String {
    format!(
        r#"<resources>
    <string name="app_name">{display_name}</string>
</resources>
"#
    )
}

fn tpl_android_themes() -> String {
    r#"<resources>
    <style name="Theme.App" parent="android:Theme.Material.Light.NoActionBar" />
</resources>
"#
    .to_string()
}

fn tpl_android_placeholder_bindings(kotlin_pkg: &str, _lib_name: &str) -> String {
    // This is a placeholder that will be overwritten by `rmp bindings kotlin`.
    // It exists so Gradle's `ensureUniffiGenerated` task doesn't block the first build.
    format!(
        r#"// Placeholder — will be replaced by `rmp bindings kotlin`.
package {kotlin_pkg}.rust
"#
    )
}

fn write_gradlew(android_dir: &Path) -> Result<(), CliError> {
    // Gradle wrapper script — minimal version that delegates to system Gradle.
    // Real projects should run `gradle wrapper` to get the full wrapper.
    let gradlew = android_dir.join("gradlew");
    write_text(
        &gradlew,
        r#"#!/bin/sh
exec gradle "$@"
"#,
    )?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = std::fs::set_permissions(&gradlew, std::fs::Permissions::from_mode(0o755));
    }
    Ok(())
}

/// Convert a snake_case lib name to PascalCase (e.g., "my_app_core" → "MyAppCore").
fn pascal_case(s: &str) -> String {
    s.split('_')
        .filter(|seg| !seg.is_empty())
        .map(|seg| {
            let mut c = seg.chars();
            match c.next() {
                Some(first) => {
                    let mut part = first.to_uppercase().to_string();
                    part.extend(c);
                    part
                }
                None => String::new(),
            }
        })
        .collect()
}

fn swift_app_entry_name(display_name: &str) -> String {
    // Turn "My App" into "MyAppApp" (SwiftUI convention).
    let cleaned: String = display_name
        .chars()
        .filter(|c| c.is_ascii_alphanumeric())
        .collect();
    if cleaned.is_empty() {
        "MainApp".to_string()
    } else {
        format!("{}App", cleaned)
    }
}
