use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use walkdir::WalkDir;

use crate::cli::{human_log, json_print, CliError, JsonOk};
use crate::config::{load_rmp_toml, RmpToml};
use crate::util::{discover_xcode_dev_dir, run_capture, which};

const BINDGEN_HASH_FILE_PREFIX: &str = "target/.rmp-bindgen-hash";

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum BuildProfile {
    Debug,
    Release,
}

impl BuildProfile {
    fn cargo_dir(self) -> &'static str {
        match self {
            Self::Debug => "debug",
            Self::Release => "release",
        }
    }

    fn cargo_release_arg(self) -> Option<&'static str> {
        match self {
            Self::Debug => None,
            Self::Release => Some("--release"),
        }
    }

    fn display(self) -> &'static str {
        match self {
            Self::Debug => "debug",
            Self::Release => "release",
        }
    }
}

pub fn build_swift_for_run(
    root: &Path,
    rust_target: &str,
    profile: BuildProfile,
    verbose: bool,
) -> Result<(), CliError> {
    let cfg = load_rmp_toml(root)?;
    let _ = cfg
        .ios
        .as_ref()
        .ok_or_else(|| CliError::user("rmp.toml missing [ios] section"))?;
    let core_pkg = cfg.core.crate_.clone();
    let core_lib = core_pkg.replace('-', "_");
    generate_ios_sources(root, &cfg, profile, true, verbose)?;
    build_ios_xcframework(root, &core_lib, &core_pkg, &[rust_target], profile, verbose)
}

pub fn build_kotlin_for_run(
    root: &Path,
    abi: &str,
    profile: BuildProfile,
    verbose: bool,
) -> Result<(), CliError> {
    let cfg = load_rmp_toml(root)?;
    let _ = cfg
        .android
        .as_ref()
        .ok_or_else(|| CliError::user("rmp.toml missing [android] section"))?;
    let core_pkg = cfg.core.crate_.clone();
    generate_android_sources(root, &cfg, profile, true, verbose)?;
    build_android_so(root, &core_pkg, &[abi], profile, verbose)
}

fn default_ios_targets() -> [&'static str; 2] {
    ["aarch64-apple-ios", "aarch64-apple-ios-sim"]
}

fn default_android_abis() -> [&'static str; 3] {
    ["arm64-v8a", "armeabi-v7a", "x86_64"]
}

pub fn bindings(
    root: &Path,
    json: bool,
    verbose: bool,
    args: crate::cli::BindingsArgs,
) -> Result<(), CliError> {
    let cfg = load_rmp_toml(root)?;
    let core_pkg = cfg.core.crate_.clone();
    let core_lib = core_pkg.replace('-', "_");

    let ios = cfg.ios.clone();
    let android = cfg.android.clone();
    let profile = BuildProfile::Release;

    match args.target {
        crate::cli::BindingsTarget::Swift => {
            let _ = ios.ok_or_else(|| CliError::user("rmp.toml missing [ios] section"))?;
            if args.clean {
                clean_ios(root, verbose)?;
            }
            if args.check {
                check_ios_sources(root, &cfg, verbose)?;
            } else {
                generate_ios_sources(root, &cfg, profile, false, verbose)?;
            }
            build_ios_xcframework(
                root,
                &core_lib,
                &core_pkg,
                &default_ios_targets(),
                profile,
                verbose,
            )?;
        }
        crate::cli::BindingsTarget::Kotlin => {
            let _ = android.ok_or_else(|| CliError::user("rmp.toml missing [android] section"))?;
            if args.clean {
                clean_android(root, verbose)?;
            }
            if args.check {
                check_android_sources(root, &cfg, verbose)?;
            } else {
                generate_android_sources(root, &cfg, profile, false, verbose)?;
            }
            build_android_so(root, &core_pkg, &default_android_abis(), profile, verbose)?;
        }
        crate::cli::BindingsTarget::All => {
            let _ = ios.ok_or_else(|| CliError::user("rmp.toml missing [ios] section"))?;
            let _ = android.ok_or_else(|| CliError::user("rmp.toml missing [android] section"))?;
            if args.clean {
                clean_ios(root, verbose)?;
                clean_android(root, verbose)?;
            }
            if args.check {
                check_ios_sources(root, &cfg, verbose)?;
                check_android_sources(root, &cfg, verbose)?;
            } else {
                generate_ios_sources(root, &cfg, profile, false, verbose)?;
                generate_android_sources(root, &cfg, profile, false, verbose)?;
            }
            build_ios_xcframework(
                root,
                &core_lib,
                &core_pkg,
                &default_ios_targets(),
                profile,
                verbose,
            )?;
            build_android_so(root, &core_pkg, &default_android_abis(), profile, verbose)?;
        }
    }

    if json {
        json_print(&JsonOk {
            ok: true,
            data: serde_json::json!({}),
        });
    }

    Ok(())
}

fn host_cdylib_path(
    root: &Path,
    core_lib: &str,
    profile: BuildProfile,
) -> Result<PathBuf, CliError> {
    let target = root.join(format!("target/{}", profile.cargo_dir()));
    let candidates = [
        target.join(format!("lib{core_lib}.dylib")),
        target.join(format!("lib{core_lib}.so")),
        target.join(format!("{core_lib}.dll")),
    ];
    for p in candidates {
        if p.is_file() {
            return Ok(p);
        }
    }
    Err(CliError::operational(format!(
        "missing built host cdylib (expected target/{}/lib{core_lib}.*)",
        profile.cargo_dir()
    )))
}

fn cargo_build_host(
    root: &Path,
    core_pkg: &str,
    profile: BuildProfile,
    verbose: bool,
) -> Result<(), CliError> {
    if which("cargo").is_none() {
        return Err(CliError::operational("missing `cargo` on PATH"));
    }
    human_log(
        verbose,
        format!("cargo build -p {core_pkg} (host, {})", profile.display()),
    );
    let mut cmd = Command::new("cargo");
    cmd.current_dir(root).arg("build").arg("-p").arg(core_pkg);
    if let Some(flag) = profile.cargo_release_arg() {
        cmd.arg(flag);
    }
    let status = cmd
        .stdout(Stdio::inherit())
        .stderr(Stdio::inherit())
        .status()
        .map_err(|e| CliError::operational(format!("failed to run cargo: {e}")))?;
    if !status.success() {
        return Err(CliError::operational("cargo build failed"));
    }
    Ok(())
}

fn generate_ios_sources(
    root: &Path,
    cfg: &RmpToml,
    profile: BuildProfile,
    use_cache: bool,
    verbose: bool,
) -> Result<(), CliError> {
    let core_pkg = cfg.core.crate_.as_str();
    let core_lib = cfg.core.crate_.replace('-', "_");
    let out_dir = root.join("ios/Bindings");
    std::fs::create_dir_all(&out_dir)
        .map_err(|e| CliError::operational(format!("failed to create ios/Bindings: {e}")))?;
    if use_cache && bindgen_cache_hit(root, swift_bindings_present(root)?, "swift", verbose)? {
        return Ok(());
    }
    cargo_build_host(root, core_pkg, profile, verbose)?;
    let lib = host_cdylib_path(root, &core_lib, profile)?;

    human_log(verbose, "uniffi-bindgen generate (swift)");
    let status = Command::new("cargo")
        .current_dir(root)
        .arg("run")
        .arg("-q")
        .arg("-p")
        .arg("uniffi-bindgen")
        .arg("--")
        .arg("generate")
        .arg("--library")
        .arg(lib)
        .arg("--language")
        .arg("swift")
        .arg("--out-dir")
        .arg(&out_dir)
        .arg("--config")
        .arg(uniffi_toml_path(root)?)
        .stdout(Stdio::inherit())
        .stderr(Stdio::inherit())
        .status()
        .map_err(|e| CliError::operational(format!("failed to run uniffi-bindgen: {e}")))?;
    if !status.success() {
        return Err(CliError::operational("uniffi swift generation failed"));
    }
    if use_cache {
        write_bindgen_hash(root, "swift")?;
    }
    Ok(())
}

fn generate_android_sources(
    root: &Path,
    cfg: &RmpToml,
    profile: BuildProfile,
    use_cache: bool,
    verbose: bool,
) -> Result<(), CliError> {
    let core_pkg = cfg.core.crate_.as_str();
    let core_lib = cfg.core.crate_.replace('-', "_");
    let out_dir = root.join("android/app/src/main/java");
    std::fs::create_dir_all(&out_dir)
        .map_err(|e| CliError::operational(format!("failed to create android java dir: {e}")))?;
    if use_cache && bindgen_cache_hit(root, kotlin_bindings_present(root)?, "kotlin", verbose)? {
        return Ok(());
    }
    cargo_build_host(root, core_pkg, profile, verbose)?;
    let lib = host_cdylib_path(root, &core_lib, profile)?;

    human_log(verbose, "uniffi-bindgen generate (kotlin)");
    let status = Command::new("cargo")
        .current_dir(root)
        .arg("run")
        .arg("-q")
        .arg("-p")
        .arg("uniffi-bindgen")
        .arg("--")
        .arg("generate")
        .arg("--library")
        .arg(lib)
        .arg("--language")
        .arg("kotlin")
        .arg("--out-dir")
        .arg(&out_dir)
        .arg("--no-format")
        .arg("--config")
        .arg(uniffi_toml_path(root)?)
        .stdout(Stdio::inherit())
        .stderr(Stdio::inherit())
        .status()
        .map_err(|e| CliError::operational(format!("failed to run uniffi-bindgen: {e}")))?;
    if !status.success() {
        return Err(CliError::operational("uniffi kotlin generation failed"));
    }
    if use_cache {
        write_bindgen_hash(root, "kotlin")?;
    }
    Ok(())
}

fn clean_ios(root: &Path, verbose: bool) -> Result<(), CliError> {
    human_log(verbose, "clean ios bindings + build outputs");
    let bindings = root.join("ios/Bindings");
    if bindings.is_dir() {
        for ent in std::fs::read_dir(&bindings).map_err(|e| {
            CliError::operational(format!("failed to read {}: {e}", bindings.display()))
        })? {
            let ent =
                ent.map_err(|e| CliError::operational(format!("failed to read dir entry: {e}")))?;
            let p = ent.path();
            if !p.is_file() {
                continue;
            }
            let name = p.file_name().and_then(|v| v.to_str()).unwrap_or("");
            if name == ".gitkeep" {
                continue;
            }
            if name.ends_with(".swift")
                || name.ends_with("FFI.h")
                || name.ends_with("FFI.modulemap")
            {
                let _ = std::fs::remove_file(p);
            }
        }
    }
    let _ = std::fs::remove_dir_all(root.join("ios/.build"));
    let _ = std::fs::remove_dir_all(root.join("ios/Frameworks"));
    let _ = std::fs::remove_file(bindgen_hash_path(root, "swift"));
    Ok(())
}

fn clean_android(root: &Path, verbose: bool) -> Result<(), CliError> {
    human_log(verbose, "clean android bindings + jniLibs");
    let _ = std::fs::remove_dir_all(kotlin_bindings_dir(root)?);
    let _ = std::fs::remove_dir_all(root.join("android/app/src/main/jniLibs"));
    let _ = std::fs::remove_file(bindgen_hash_path(root, "kotlin"));
    Ok(())
}

fn check_ios_sources(root: &Path, cfg: &RmpToml, verbose: bool) -> Result<(), CliError> {
    use tempfile::TempDir;

    let core_pkg = cfg.core.crate_.as_str();
    let core_lib = cfg.core.crate_.replace('-', "_");
    cargo_build_host(root, core_pkg, BuildProfile::Release, verbose)?;
    let lib = host_cdylib_path(root, &core_lib, BuildProfile::Release)?;
    let tmp = TempDir::new().map_err(|e| CliError::operational(format!("tempdir: {e}")))?;
    let out_dir = tmp.path().join("Bindings");
    std::fs::create_dir_all(&out_dir)
        .map_err(|e| CliError::operational(format!("failed to create temp out dir: {e}")))?;

    let status = Command::new("cargo")
        .current_dir(root)
        .arg("run")
        .arg("-q")
        .arg("-p")
        .arg("uniffi-bindgen")
        .arg("--")
        .arg("generate")
        .arg("--library")
        .arg(lib)
        .arg("--language")
        .arg("swift")
        .arg("--out-dir")
        .arg(&out_dir)
        .arg("--config")
        .arg(uniffi_toml_path(root)?)
        .stdout(Stdio::inherit())
        .stderr(Stdio::inherit())
        .status()
        .map_err(|e| CliError::operational(format!("failed to run uniffi-bindgen: {e}")))?;
    if !status.success() {
        return Err(CliError::operational("uniffi swift generation failed"));
    }

    let want = out_dir;
    let have = root.join("ios/Bindings");
    diff_dir(&have, &want, &[".gitkeep"])?;
    Ok(())
}

fn check_android_sources(root: &Path, cfg: &RmpToml, verbose: bool) -> Result<(), CliError> {
    use tempfile::TempDir;

    let core_pkg = cfg.core.crate_.as_str();
    let core_lib = cfg.core.crate_.replace('-', "_");
    cargo_build_host(root, core_pkg, BuildProfile::Release, verbose)?;
    let lib = host_cdylib_path(root, &core_lib, BuildProfile::Release)?;
    let tmp = TempDir::new().map_err(|e| CliError::operational(format!("tempdir: {e}")))?;
    let out_dir = tmp.path().join("java");
    std::fs::create_dir_all(&out_dir)
        .map_err(|e| CliError::operational(format!("failed to create temp out dir: {e}")))?;

    let status = Command::new("cargo")
        .current_dir(root)
        .arg("run")
        .arg("-q")
        .arg("-p")
        .arg("uniffi-bindgen")
        .arg("--")
        .arg("generate")
        .arg("--library")
        .arg(lib)
        .arg("--language")
        .arg("kotlin")
        .arg("--out-dir")
        .arg(&out_dir)
        .arg("--no-format")
        .arg("--config")
        .arg(uniffi_toml_path(root)?)
        .stdout(Stdio::inherit())
        .stderr(Stdio::inherit())
        .status()
        .map_err(|e| CliError::operational(format!("failed to run uniffi-bindgen: {e}")))?;
    if !status.success() {
        return Err(CliError::operational("uniffi kotlin generation failed"));
    }

    let rel = kotlin_bindings_relpath(root)?;
    let want = out_dir.join(&rel);
    let have = root.join("android/app/src/main/java").join(&rel);
    diff_dir(&have, &want, &[])?;
    Ok(())
}

fn diff_dir(have: &Path, want: &Path, ignore: &[&str]) -> Result<(), CliError> {
    let mut mismatches: Vec<String> = vec![];

    let mut want_files: Vec<PathBuf> = vec![];
    for ent in WalkDir::new(want).into_iter().flatten() {
        if !ent.file_type().is_file() {
            continue;
        }
        let rel = ent.path().strip_prefix(want).unwrap().to_path_buf();
        if ignore.iter().any(|i| Path::new(i) == rel) {
            continue;
        }
        want_files.push(rel);
    }

    let mut have_files: Vec<PathBuf> = vec![];
    for ent in WalkDir::new(have).into_iter().flatten() {
        if !ent.file_type().is_file() {
            continue;
        }
        let rel = ent.path().strip_prefix(have).unwrap().to_path_buf();
        if ignore.iter().any(|i| Path::new(i) == rel) {
            continue;
        }
        have_files.push(rel);
    }

    want_files.sort();
    have_files.sort();

    for rel in want_files.iter() {
        let w = std::fs::read(want.join(rel)).unwrap_or_default();
        let h = std::fs::read(have.join(rel)).unwrap_or_default();
        if w != h {
            mismatches.push(rel.to_string_lossy().to_string());
        }
    }
    for rel in have_files.iter() {
        if !want_files.contains(rel) {
            mismatches.push(format!("extra: {}", rel.to_string_lossy()));
        }
    }

    if !mismatches.is_empty() {
        return Err(
            CliError::operational("bindings --check failed (generated outputs differ)")
                .with_detail("files", serde_json::json!(mismatches)),
        );
    }
    Ok(())
}

fn build_ios_xcframework(
    root: &Path,
    core_lib: &str,
    core_pkg: &str,
    targets: &[&str],
    profile: BuildProfile,
    verbose: bool,
) -> Result<(), CliError> {
    if targets.is_empty() {
        return Err(CliError::user("no iOS Rust target selected"));
    }
    let dev_dir = discover_xcode_dev_dir()?;

    // Build Rust static libs for the selected iOS targets.
    build_ios_staticlibs(root, &dev_dir, core_pkg, targets, profile, verbose)?;

    // Ensure headers exist from UniFFI swift generation.
    let bindings_dir = root.join("ios/Bindings");
    let hdr_name = format!("{core_lib}FFI.h");
    let mm_name = format!("{core_lib}FFI.modulemap");
    let hdr = bindings_dir.join(&hdr_name);
    let mm = bindings_dir.join(&mm_name);
    if !hdr.is_file() || !mm.is_file() {
        return Err(CliError::operational(
            "missing ios/Bindings headers; run `rmp bindings swift` first",
        ));
    }

    // Assemble xcframework.
    let build_dir = root.join("ios/.build");
    let headers_dir = build_dir.join(format!("headers/{core_lib}FFI"));
    let frameworks_dir = root.join("ios/Frameworks");
    let _ = std::fs::remove_dir_all(&build_dir);
    let _ = std::fs::remove_dir_all(&frameworks_dir);
    std::fs::create_dir_all(&headers_dir)
        .map_err(|e| CliError::operational(format!("failed to create ios/.build: {e}")))?;
    std::fs::create_dir_all(&frameworks_dir)
        .map_err(|e| CliError::operational(format!("failed to create ios/Frameworks: {e}")))?;

    std::fs::copy(&hdr, headers_dir.join(&hdr_name))
        .map_err(|e| CliError::operational(format!("copy header: {e}")))?;
    std::fs::copy(&mm, headers_dir.join("module.modulemap"))
        .map_err(|e| CliError::operational(format!("copy modulemap: {e}")))?;

    let mut selected: Vec<&str> = Vec::with_capacity(targets.len());
    for target in targets {
        if !selected.contains(target) {
            selected.push(*target);
        }
    }

    let mut libraries: Vec<PathBuf> = vec![];
    let has_sim_arm64 = selected.contains(&"aarch64-apple-ios-sim");
    let has_sim_x86_64 = selected.contains(&"x86_64-apple-ios");
    if has_sim_arm64 && has_sim_x86_64 {
        let sim_a = build_dir.join(format!("lib{core_lib}_sim.a"));
        let status = Command::new("/usr/bin/xcrun")
            .env("DEVELOPER_DIR", &dev_dir)
            .arg("lipo")
            .arg("-create")
            .arg(ios_staticlib_path(
                root,
                "aarch64-apple-ios-sim",
                core_lib,
                profile,
            ))
            .arg(ios_staticlib_path(
                root,
                "x86_64-apple-ios",
                core_lib,
                profile,
            ))
            .arg("-output")
            .arg(&sim_a)
            .stdout(Stdio::inherit())
            .stderr(Stdio::inherit())
            .status()
            .map_err(|e| CliError::operational(format!("failed to run lipo: {e}")))?;
        if !status.success() {
            return Err(CliError::operational("lipo failed"));
        }
        libraries.push(sim_a);
        selected.retain(|t| *t != "aarch64-apple-ios-sim" && *t != "x86_64-apple-ios");
    }
    for target in selected {
        libraries.push(ios_staticlib_path(root, target, core_lib, profile));
    }

    let xcf_name = pascal_case(core_lib);
    let out_xcf = frameworks_dir.join(format!("{xcf_name}.xcframework"));
    let mut cmd = Command::new("/usr/bin/xcrun");
    cmd.env("DEVELOPER_DIR", &dev_dir)
        .arg("xcodebuild")
        .arg("-create-xcframework");
    for lib in &libraries {
        cmd.arg("-library")
            .arg(lib)
            .arg("-headers")
            .arg(&headers_dir);
    }
    cmd.arg("-output").arg(&out_xcf);
    let status = cmd
        .stdout(Stdio::inherit())
        .stderr(Stdio::inherit())
        .status()
        .map_err(|e| {
            CliError::operational(format!("failed to run xcodebuild -create-xcframework: {e}"))
        })?;
    if !status.success() {
        return Err(CliError::operational(
            "xcodebuild -create-xcframework failed",
        ));
    }

    Ok(())
}

fn build_ios_staticlibs(
    root: &Path,
    dev_dir: &Path,
    core_pkg: &str,
    targets: &[&str],
    profile: BuildProfile,
    verbose: bool,
) -> Result<(), CliError> {
    if which("cargo").is_none() {
        return Err(CliError::operational("missing `cargo` on PATH"));
    }
    if targets.is_empty() {
        return Err(CliError::user("no iOS Rust target selected"));
    }

    // Replicate `just ios-rust` env behavior.
    let toolchain_bin = dev_dir.join("Toolchains/XcodeDefault.xctoolchain/usr/bin");
    let cc = toolchain_bin.join("clang");
    let cxx = toolchain_bin.join("clang++");
    let ar = toolchain_bin.join("ar");
    let ranlib = toolchain_bin.join("ranlib");
    let ios_min = "17.0";

    // sdk roots
    let sdk_ios = {
        let mut cmd = Command::new("/usr/bin/xcrun");
        cmd.env("DEVELOPER_DIR", dev_dir)
            .arg("--sdk")
            .arg("iphoneos")
            .arg("--show-sdk-path");
        let out = run_capture(cmd)?;
        if !out.status.success() {
            return Err(CliError::operational(
                "xcrun --sdk iphoneos --show-sdk-path failed",
            ));
        }
        String::from_utf8_lossy(&out.stdout).trim().to_string()
    };
    let sdk_sim = {
        let mut cmd = Command::new("/usr/bin/xcrun");
        cmd.env("DEVELOPER_DIR", dev_dir)
            .arg("--sdk")
            .arg("iphonesimulator")
            .arg("--show-sdk-path");
        let out = run_capture(cmd)?;
        if !out.status.success() {
            return Err(CliError::operational(
                "xcrun --sdk iphonesimulator --show-sdk-path failed",
            ));
        }
        String::from_utf8_lossy(&out.stdout).trim().to_string()
    };

    human_log(
        verbose,
        format!("cargo build (iOS staticlib, profile={})", profile.display()),
    );
    for target in targets {
        let (sdkroot, min_flag) = match *target {
            "aarch64-apple-ios" => (sdk_ios.as_str(), "-miphoneos-version-min="),
            "aarch64-apple-ios-sim" | "x86_64-apple-ios" => {
                (sdk_sim.as_str(), "-mios-simulator-version-min=")
            }
            _ => {
                return Err(CliError::user(format!(
                    "unsupported iOS Rust target: {target}"
                )))
            }
        };
        let mut cmd = Command::new("cargo");
        cmd.current_dir(root)
            .arg("build")
            .arg("-p")
            .arg(core_pkg)
            .arg("--lib")
            .arg("--target")
            .arg(target);
        if let Some(flag) = profile.cargo_release_arg() {
            cmd.arg(flag);
        }

        // Clean out Nix/toolchain vars that break iOS builds.
        for k in [
            "LIBRARY_PATH",
            "SDKROOT",
            "MACOSX_DEPLOYMENT_TARGET",
            "CC",
            "CXX",
            "AR",
            "RANLIB",
            "LD",
        ] {
            cmd.env_remove(k);
        }

        cmd.env("DEVELOPER_DIR", dev_dir)
            .env("CC", &cc)
            .env("CXX", &cxx)
            .env("AR", &ar)
            .env("RANLIB", &ranlib)
            .env("SDKROOT", sdkroot)
            .env("IPHONEOS_DEPLOYMENT_TARGET", ios_min)
            .env(
                "RUSTFLAGS",
                format!(
                    "-C linker={} -C link-arg={}{}",
                    cc.to_string_lossy(),
                    min_flag,
                    ios_min
                ),
            );

        // Ensure linker is clang for relevant targets.
        match *target {
            "aarch64-apple-ios" => {
                cmd.env("CARGO_TARGET_AARCH64_APPLE_IOS_LINKER", &cc);
            }
            "aarch64-apple-ios-sim" => {
                cmd.env("CARGO_TARGET_AARCH64_APPLE_IOS_SIM_LINKER", &cc);
            }
            "x86_64-apple-ios" => {
                cmd.env("CARGO_TARGET_X86_64_APPLE_IOS_LINKER", &cc);
            }
            _ => {}
        }

        let status = cmd
            .stdout(Stdio::inherit())
            .stderr(Stdio::inherit())
            .status()
            .map_err(|e| CliError::operational(format!("failed to run cargo for {target}: {e}")))?;
        if !status.success() {
            return Err(CliError::operational(format!(
                "cargo build failed for target {target}"
            )));
        }
    }

    Ok(())
}

fn build_android_so(
    root: &Path,
    core_pkg: &str,
    abis: &[&str],
    profile: BuildProfile,
    verbose: bool,
) -> Result<(), CliError> {
    if abis.is_empty() {
        return Err(CliError::user("no Android ABI selected"));
    }
    // Ensure local.properties so Gradle doesn't require Android Studio.
    let sdk = std::env::var("ANDROID_HOME")
        .ok()
        .or_else(|| std::env::var("ANDROID_SDK_ROOT").ok())
        .unwrap_or_default();
    if !sdk.is_empty() {
        let lp = root.join("android/local.properties");
        let _ = std::fs::create_dir_all(root.join("android"));
        let contents = format!("sdk.dir={}\n", sdk);
        let _ = std::fs::write(lp, contents);
    }

    let out_dir = root.join("android/app/src/main/jniLibs");
    // Clear old slices first so the package exactly matches the requested ABI set.
    let _ = std::fs::remove_dir_all(&out_dir);
    std::fs::create_dir_all(&out_dir)
        .map_err(|e| CliError::operational(format!("failed to create jniLibs dir: {e}")))?;

    human_log(
        verbose,
        format!(
            "cargo ndk build (android .so, profile={}, abis={})",
            profile.display(),
            abis.join(",")
        ),
    );
    let mut cmd = Command::new("cargo");
    cmd.current_dir(root)
        .arg("ndk")
        .arg("-o")
        .arg(&out_dir)
        .arg("-P")
        .arg("26");
    for abi in abis {
        cmd.arg("-t").arg(abi);
    }
    cmd.arg("build").arg("-p").arg(core_pkg);
    if let Some(flag) = profile.cargo_release_arg() {
        cmd.arg(flag);
    }
    let status = cmd
        .stdout(Stdio::inherit())
        .stderr(Stdio::inherit())
        .status()
        .map_err(|e| CliError::operational(format!("failed to run cargo-ndk: {e}")))?;
    if !status.success() {
        return Err(CliError::operational("cargo ndk build failed"));
    }

    Ok(())
}

fn ios_staticlib_path(root: &Path, target: &str, core_lib: &str, profile: BuildProfile) -> PathBuf {
    root.join(format!(
        "target/{}/{}/lib{core_lib}.a",
        target,
        profile.cargo_dir()
    ))
}

fn swift_bindings_present(root: &Path) -> Result<bool, CliError> {
    let dir = root.join("ios/Bindings");
    if !dir.is_dir() {
        return Ok(false);
    }
    let mut has_swift = false;
    let mut has_header = false;
    let mut has_modulemap = false;
    for ent in std::fs::read_dir(&dir)
        .map_err(|e| CliError::operational(format!("failed to read {}: {e}", dir.display())))?
    {
        let ent =
            ent.map_err(|e| CliError::operational(format!("failed to read dir entry: {e}")))?;
        let p = ent.path();
        if !p.is_file() {
            continue;
        }
        let name = p.file_name().and_then(|v| v.to_str()).unwrap_or("");
        if name.ends_with(".swift") {
            has_swift = true;
        } else if name.ends_with("FFI.h") {
            has_header = true;
        } else if name.ends_with("FFI.modulemap") {
            has_modulemap = true;
        }
    }
    Ok(has_swift && has_header && has_modulemap)
}

fn kotlin_bindings_present(root: &Path) -> Result<bool, CliError> {
    let dir = kotlin_bindings_dir(root)?;
    if !dir.is_dir() {
        return Ok(false);
    }
    for ent in WalkDir::new(&dir).into_iter().flatten() {
        if ent.file_type().is_file()
            && ent
                .path()
                .extension()
                .and_then(|v| v.to_str())
                .is_some_and(|ext| ext == "kt")
        {
            return Ok(true);
        }
    }
    Ok(false)
}

fn bindgen_cache_hit(
    root: &Path,
    outputs_present: bool,
    key: &str,
    verbose: bool,
) -> Result<bool, CliError> {
    if !outputs_present {
        return Ok(false);
    }
    let hash = compute_bindgen_inputs_hash(root)?;
    let cache_path = bindgen_hash_path(root, key);
    let cached = std::fs::read_to_string(cache_path).ok();
    let cached = cached.as_deref().map(str::trim);
    if cached == Some(hash.as_str()) {
        human_log(
            verbose,
            "bindgen cache hit; skipping host build + UniFFI generation",
        );
        return Ok(true);
    }
    Ok(false)
}

fn write_bindgen_hash(root: &Path, key: &str) -> Result<(), CliError> {
    let hash = compute_bindgen_inputs_hash(root)?;
    let path = bindgen_hash_path(root, key);
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| {
            CliError::operational(format!("failed to create {}: {e}", parent.display()))
        })?;
    }
    std::fs::write(&path, format!("{hash}\n"))
        .map_err(|e| CliError::operational(format!("failed to write {}: {e}", path.display())))?;
    Ok(())
}

fn bindgen_hash_path(root: &Path, key: &str) -> PathBuf {
    root.join(format!("{BINDGEN_HASH_FILE_PREFIX}-{key}"))
}

fn compute_bindgen_inputs_hash(root: &Path) -> Result<String, CliError> {
    let mut files: Vec<PathBuf> = vec![];
    let src_dir = root.join("rust/src");
    for ent in WalkDir::new(&src_dir).into_iter().flatten() {
        if ent.file_type().is_file() {
            files.push(ent.path().to_path_buf());
        }
    }
    files.push(uniffi_toml_path(root)?);
    for extra in [
        root.join("Cargo.toml"),
        root.join("Cargo.lock"),
        root.join("rust/Cargo.toml"),
        root.join("rust/Cargo.lock"),
    ] {
        if extra.is_file() {
            files.push(extra);
        }
    }
    files.sort();

    let mut hash: u64 = 0xcbf29ce484222325;
    for file in files {
        let rel = file
            .strip_prefix(root)
            .unwrap_or(file.as_path())
            .to_string_lossy();
        fnv1a_update(&mut hash, rel.as_bytes());
        fnv1a_update(&mut hash, &[0u8]);
        let data = std::fs::read(&file).map_err(|e| {
            CliError::operational(format!(
                "failed to read bindgen input {}: {e}",
                file.display()
            ))
        })?;
        fnv1a_update(&mut hash, &data);
        fnv1a_update(&mut hash, &[0u8]);
    }
    Ok(format!("{hash:016x}"))
}

fn fnv1a_update(hash: &mut u64, data: &[u8]) {
    const FNV_PRIME: u64 = 0x0000_0100_0000_01b3;
    for b in data {
        *hash ^= u64::from(*b);
        *hash = hash.wrapping_mul(FNV_PRIME);
    }
}

fn kotlin_bindings_dir(root: &Path) -> Result<PathBuf, CliError> {
    Ok(root
        .join("android/app/src/main/java")
        .join(kotlin_bindings_relpath(root)?))
}

fn kotlin_bindings_relpath(root: &Path) -> Result<PathBuf, CliError> {
    let uniffi = uniffi_toml_path(root)?;
    let s = std::fs::read_to_string(&uniffi)
        .map_err(|e| CliError::operational(format!("failed to read {}: {e}", uniffi.display())))?;
    let v: toml::Value = toml::from_str(&s)
        .map_err(|e| CliError::user(format!("failed to parse rust/uniffi.toml: {e}")))?;
    let pkg = v
        .get("bindings")
        .and_then(|x| x.get("kotlin"))
        .and_then(|x| x.get("package_name"))
        .and_then(|x| x.as_str())
        .ok_or_else(|| CliError::user("rust/uniffi.toml missing [bindings.kotlin].package_name"))?;
    if pkg.trim().is_empty() {
        return Err(CliError::user(
            "rust/uniffi.toml has empty [bindings.kotlin].package_name",
        ));
    }
    Ok(PathBuf::from(pkg.replace('.', "/")))
}

fn uniffi_toml_path(root: &Path) -> Result<PathBuf, CliError> {
    let primary = root.join("rust/uniffi.toml");
    if primary.is_file() {
        return Ok(primary);
    }
    let fallback = root.join("uniffi.toml");
    if fallback.is_file() {
        return Ok(fallback);
    }
    Err(CliError::user(
        "missing UniFFI config (expected rust/uniffi.toml or uniffi.toml)",
    ))
}

/// Convert a snake_case lib name to PascalCase (e.g., "pika_core" → "PikaCore").
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
