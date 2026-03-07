use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::thread;
use std::time::{Duration, Instant};

use crate::bindings;
use crate::bindings::BuildProfile;
use crate::cli::{human_log, json_print, CliError, JsonOk};
use crate::config::load_rmp_toml;
use crate::util::{discover_xcode_dev_dir, run_capture};

pub fn run(
    root: &Path,
    json: bool,
    verbose: bool,
    args: crate::cli::RunArgs,
) -> Result<(), CliError> {
    match args.platform {
        crate::cli::RunPlatform::Ios => run_ios(root, json, verbose, args.ios, args.release),
        crate::cli::RunPlatform::Android => {
            run_android(root, json, verbose, args.android, args.release)
        }
        crate::cli::RunPlatform::Iced => run_iced(root, json, verbose, args.release),
    }
}

fn run_ios(
    root: &Path,
    json: bool,
    verbose: bool,
    args: crate::cli::RunIosArgs,
    release: bool,
) -> Result<(), CliError> {
    let cfg = load_rmp_toml(root)?;
    let ios = cfg
        .ios
        .ok_or_else(|| CliError::user("rmp.toml missing [ios] section"))?;
    let dev_dir = discover_xcode_dev_dir()?;
    let profile = build_profile(release);
    let (rust_target, xcode_arch) = ios_sim_target_for_host()?;

    let udid = ensure_ios_simulator(&dev_dir, args.udid.as_deref(), verbose)?;

    // Build bindings + xcframework for a single simulator arch.
    bindings::build_swift_for_run(root, rust_target, profile, verbose)?;

    // Generate Xcode project.
    human_log(verbose, "xcodegen generate");
    let status = Command::new("xcodegen")
        .current_dir(root.join("ios"))
        .arg("generate")
        .stdout(Stdio::inherit())
        .stderr(Stdio::inherit())
        .status()
        .map_err(|e| CliError::operational(format!("failed to run xcodegen: {e}")))?;
    if !status.success() {
        return Err(CliError::operational("xcodegen generate failed"));
    }

    let bundle_id = format!("{}.dev", ios.bundle_id);
    let xcode_name = read_xcode_project_name(root).unwrap_or_else(|| "App".to_string());
    let xcode_scheme = ios.scheme.clone().unwrap_or_else(|| xcode_name.clone());
    let xcode_config = if release { "Release" } else { "Debug" };

    let xcode_project_path = root.join(format!("ios/{xcode_name}.xcodeproj"));

    // Build simulator .app.
    human_log(
        verbose,
        format!("xcodebuild ({xcode_config}, iphonesimulator, arch={xcode_arch})"),
    );
    let mut cmd = Command::new("/usr/bin/xcrun");
    cmd.env("DEVELOPER_DIR", &dev_dir)
        .env_remove("LD")
        .env_remove("CC")
        .env_remove("CXX")
        .arg("xcodebuild")
        .arg("-project")
        .arg(&xcode_project_path)
        .arg("-scheme")
        .arg(&xcode_scheme)
        .arg("-destination")
        .arg(format!("id={udid}"))
        .arg("-configuration")
        .arg(xcode_config)
        .arg("-sdk")
        .arg("iphonesimulator")
        .arg("build")
        .arg(format!("ARCHS={xcode_arch}"))
        .arg("ONLY_ACTIVE_ARCH=YES")
        .arg("CODE_SIGNING_ALLOWED=NO")
        .arg(format!("PRODUCT_BUNDLE_IDENTIFIER={bundle_id}"));

    let status = cmd
        .stdout(Stdio::inherit())
        .stderr(Stdio::inherit())
        .status()
        .map_err(|e| CliError::operational(format!("failed to run xcodebuild: {e}")))?;
    if !status.success() {
        return Err(CliError::operational("xcodebuild failed"));
    }

    let app_path = resolve_ios_app_path(
        &dev_dir,
        &xcode_project_path,
        &xcode_scheme,
        &udid,
        xcode_config,
        xcode_arch,
    )?;
    if !app_path.is_dir() {
        return Err(CliError::operational(format!(
            "missing built app at {}",
            app_path.to_string_lossy()
        )));
    }

    // Install.
    human_log(verbose, format!("simctl install (udid={udid})"));
    let status = Command::new("/usr/bin/xcrun")
        .env("DEVELOPER_DIR", &dev_dir)
        .arg("simctl")
        .arg("install")
        .arg(&udid)
        .arg(&app_path)
        .stdout(Stdio::inherit())
        .stderr(Stdio::inherit())
        .status()
        .map_err(|e| CliError::operational(format!("failed to run simctl install: {e}")))?;
    if !status.success() {
        return Err(CliError::operational("simctl install failed"));
    }

    // Launch.
    let _ = Command::new("/usr/bin/xcrun")
        .env("DEVELOPER_DIR", &dev_dir)
        .arg("simctl")
        .arg("terminate")
        .arg(&udid)
        .arg(&bundle_id)
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status();

    human_log(verbose, "simctl launch");
    let status = Command::new("/usr/bin/xcrun")
        .env("DEVELOPER_DIR", &dev_dir)
        .arg("simctl")
        .arg("launch")
        .arg(&udid)
        .arg(&bundle_id)
        .stdout(Stdio::inherit())
        .stderr(Stdio::inherit())
        .status()
        .map_err(|e| CliError::operational(format!("failed to run simctl launch: {e}")))?;
    if !status.success() {
        return Err(CliError::operational("simctl launch failed"));
    }

    let _ = Command::new("open").arg("-a").arg("Simulator").status();

    if json {
        json_print(&JsonOk {
            ok: true,
            data: serde_json::json!({"platform":"ios","kind":"simulator","udid":udid,"bundle_id":bundle_id}),
        });
    } else {
        eprintln!("ok: ios app launched (simulator)");
    }
    Ok(())
}

pub(crate) fn ensure_ios_simulator(
    dev_dir: &Path,
    explicit_udid: Option<&str>,
    verbose: bool,
) -> Result<String, CliError> {
    if let Some(u) = explicit_udid {
        // Validate exists.
        let mut cmd = Command::new("/usr/bin/xcrun");
        cmd.env("DEVELOPER_DIR", dev_dir)
            .arg("simctl")
            .arg("list")
            .arg("devices");
        let out = run_capture(cmd)?;
        if !out.status.success() {
            return Err(CliError::operational("failed to list simulators"));
        }
        let s = String::from_utf8_lossy(&out.stdout);
        if !s.contains(u) {
            return Err(CliError::user(format!(
                "requested simulator udid not found: {u}"
            )));
        }
        boot_sim(dev_dir, u, verbose)?;
        return Ok(u.to_string());
    }

    // Ensure at least one runtime exists.
    let mut cmd = Command::new("/usr/bin/xcrun");
    cmd.env("DEVELOPER_DIR", dev_dir)
        .arg("simctl")
        .arg("list")
        .arg("-j")
        .arg("runtimes");
    let out = run_capture(cmd)?;
    if !out.status.success() {
        return Err(CliError::operational("failed to list simulator runtimes"));
    }
    let j: serde_json::Value = serde_json::from_slice(&out.stdout)
        .map_err(|e| CliError::operational(format!("failed to parse runtimes JSON: {e}")))?;
    let mut runtimes: Vec<(u32, u32, String)> = vec![];
    for rt in j
        .get("runtimes")
        .and_then(|v| v.as_array())
        .into_iter()
        .flatten()
    {
        let name = rt.get("name").and_then(|v| v.as_str()).unwrap_or("");
        let ident = rt.get("identifier").and_then(|v| v.as_str()).unwrap_or("");
        let avail = rt
            .get("isAvailable")
            .and_then(|v| v.as_bool())
            .unwrap_or(true);
        if !name.starts_with("iOS ") || !avail {
            continue;
        }
        // ident ends with iOS-18-6 style; parse.
        if let Some((maj, min)) = parse_ios_runtime_ident(ident) {
            runtimes.push((maj, min, ident.to_string()));
        }
    }
    runtimes.sort();
    let runtime_id = runtimes
        .last()
        .map(|t| t.2.clone())
        .ok_or_else(|| CliError::operational("no iOS simulator runtimes installed"))?;

    let device_type_id = pick_device_type_id(dev_dir, "iPhone 15")?;

    let device_name = "RMP iPhone 15";
    let udid = match find_simulator_udid_by_name_and_runtime(dev_dir, device_name, &runtime_id)? {
        Some(u) => u,
        None => create_simulator(dev_dir, device_name, &device_type_id, &runtime_id)?,
    };

    boot_sim(dev_dir, &udid, verbose)?;
    Ok(udid)
}

fn parse_ios_runtime_ident(ident: &str) -> Option<(u32, u32)> {
    // com.apple.CoreSimulator.SimRuntime.iOS-18-6
    let tail = ident.rsplit('.').next().unwrap_or("");
    let mut it = tail.split('-').skip_while(|s| *s != "iOS").skip(1);
    let maj = it.next()?.parse().ok()?;
    let min = it.next()?.parse().ok()?;
    Some((maj, min))
}

fn pick_device_type_id(dev_dir: &Path, prefer: &str) -> Result<String, CliError> {
    let mut cmd = Command::new("/usr/bin/xcrun");
    cmd.env("DEVELOPER_DIR", dev_dir)
        .arg("simctl")
        .arg("list")
        .arg("devicetypes");
    let out = run_capture(cmd)?;
    if !out.status.success() {
        return Err(CliError::operational("failed to list sim device types"));
    }
    let s = String::from_utf8_lossy(&out.stdout);
    let mut first_iphone: Option<String> = None;
    for ln in s.lines() {
        if let Some((name, rest)) = ln.split_once('(') {
            let name = name.trim();
            let id = rest.trim_end_matches(')').trim();
            if first_iphone.is_none() && name.contains("iPhone") {
                first_iphone = Some(id.to_string());
            }
            if name.contains(prefer) {
                return Ok(id.to_string());
            }
        }
    }
    first_iphone.ok_or_else(|| CliError::operational("no iPhone simulator device types found"))
}

fn find_simulator_udid_by_name_and_runtime(
    dev_dir: &Path,
    name: &str,
    runtime_id: &str,
) -> Result<Option<String>, CliError> {
    let mut cmd = Command::new("/usr/bin/xcrun");
    cmd.env("DEVELOPER_DIR", dev_dir)
        .arg("simctl")
        .arg("list")
        .arg("-j")
        .arg("devices");
    let out = run_capture(cmd)?;
    if !out.status.success() {
        return Err(CliError::operational("failed to list simulators"));
    }
    let j: serde_json::Value = serde_json::from_slice(&out.stdout)
        .map_err(|e| CliError::operational(format!("failed to parse simulator JSON: {e}")))?;
    let Some(runtime_devices) = j.get("devices").and_then(|v| v.get(runtime_id)) else {
        return Ok(None);
    };
    for dev in runtime_devices.as_array().into_iter().flatten() {
        let dev_name = dev.get("name").and_then(|v| v.as_str()).unwrap_or("");
        let udid = dev.get("udid").and_then(|v| v.as_str()).unwrap_or("");
        let available = dev
            .get("isAvailable")
            .and_then(|v| v.as_bool())
            .unwrap_or(true);
        if available && dev_name == name && udid.len() >= 25 {
            return Ok(Some(udid.to_string()));
        }
    }
    Ok(None)
}

fn create_simulator(
    dev_dir: &Path,
    name: &str,
    device_type_id: &str,
    runtime_id: &str,
) -> Result<String, CliError> {
    let out = Command::new("/usr/bin/xcrun")
        .env("DEVELOPER_DIR", dev_dir)
        .arg("simctl")
        .arg("create")
        .arg(name)
        .arg(device_type_id)
        .arg(runtime_id)
        .output()
        .map_err(|e| CliError::operational(format!("simctl create failed: {e}")))?;
    if !out.status.success() {
        return Err(CliError::operational("simctl create failed"));
    }
    Ok(String::from_utf8_lossy(&out.stdout).trim().to_string())
}

fn boot_sim(dev_dir: &Path, udid: &str, verbose: bool) -> Result<(), CliError> {
    let _ = Command::new("/usr/bin/xcrun")
        .env("DEVELOPER_DIR", dev_dir)
        .arg("simctl")
        .arg("boot")
        .arg(udid)
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status();

    // Avoid an unbounded `simctl bootstatus -b` wait in CI; poll with timeout.
    human_log(verbose, "waiting for simulator boot");
    let start = Instant::now();
    while start.elapsed() < Duration::from_secs(180) {
        if simulator_is_booted(dev_dir, udid)? {
            return Ok(());
        }
        thread::sleep(Duration::from_secs(1));
    }

    Err(CliError::operational(format!(
        "simulator did not boot in time: {udid}"
    )))
}

fn simulator_is_booted(dev_dir: &Path, udid: &str) -> Result<bool, CliError> {
    let mut cmd = Command::new("/usr/bin/xcrun");
    cmd.env("DEVELOPER_DIR", dev_dir)
        .arg("simctl")
        .arg("list")
        .arg("devices");
    let out = run_capture(cmd)?;
    if !out.status.success() {
        return Err(CliError::operational("failed to list simulators"));
    }
    let s = String::from_utf8_lossy(&out.stdout);
    for line in s.lines() {
        if line.contains(udid) && line.contains("(Booted)") {
            return Ok(true);
        }
    }
    Ok(false)
}

fn run_android(
    root: &Path,
    json: bool,
    verbose: bool,
    args: crate::cli::RunAndroidArgs,
    release: bool,
) -> Result<(), CliError> {
    let cfg = load_rmp_toml(root)?;
    let android = cfg
        .android
        .ok_or_else(|| CliError::user("rmp.toml missing [android] section"))?;
    let app_id = android.app_id;
    let avd = args
        .avd
        .or(android.avd_name)
        .unwrap_or_else(|| "rmp_api35".into());

    let serial = ensure_android_emulator(root, &avd, args.serial.as_deref(), verbose)?;
    let abi = detect_android_abi(&serial, verbose)?;
    let profile = build_profile(release);

    // Build bindings (kotlin + .so) for the connected ABI only.
    bindings::build_kotlin_for_run(root, &abi, profile, verbose)?;

    // Assemble debug APK.
    human_log(verbose, "gradle assembleDebug");
    let ci = is_ci();
    let mut cmd = Command::new("./gradlew");
    cmd.current_dir(root.join("android"))
        .arg(":app:assembleDebug");
    if ci {
        // CI stability + debuggability.
        cmd.arg("--no-daemon")
            .arg("--console=plain")
            .arg("--stacktrace");
    } else if verbose {
        cmd.arg("--console=plain");
    }
    let status = cmd
        .stdout(Stdio::inherit())
        .stderr(Stdio::inherit())
        .status()
        .map_err(|e| CliError::operational(format!("failed to run gradlew: {e}")))?;
    if !status.success() {
        return Err(CliError::operational("gradle assembleDebug failed"));
    }

    let apk = root.join("android/app/build/outputs/apk/debug/app-debug.apk");
    if !apk.is_file() {
        return Err(CliError::operational(format!(
            "expected apk not found: {}",
            apk.to_string_lossy()
        )));
    }

    let pkg = format!("{app_id}.dev");

    // Install.
    human_log(verbose, format!("adb install (serial={serial})"));
    let status = Command::new("adb")
        .arg("-s")
        .arg(&serial)
        .arg("install")
        .arg("-r")
        .arg(&apk)
        .stdout(Stdio::inherit())
        .stderr(Stdio::inherit())
        .status()
        .map_err(|e| CliError::operational(format!("failed to run adb install: {e}")))?;
    if !status.success() {
        return Err(CliError::operational("adb install failed"));
    }

    if let Some(rev) = args.adb_reverse {
        setup_adb_reverse(&serial, &rev, verbose)?;
    }

    launch_android(&serial, &pkg, verbose)?;

    if json {
        json_print(&JsonOk {
            ok: true,
            data: serde_json::json!({"platform":"android","kind":"emulator","serial":serial,"app_id":pkg}),
        });
    } else {
        eprintln!("ok: android app launched");
    }
    Ok(())
}

fn run_iced(root: &Path, json: bool, verbose: bool, release: bool) -> Result<(), CliError> {
    let cfg = load_rmp_toml(root)?;
    let desktop = cfg
        .desktop
        .ok_or_else(|| CliError::user("rmp.toml missing [desktop] section"))?;

    if !desktop
        .targets
        .iter()
        .any(|t| t.eq_ignore_ascii_case("iced"))
    {
        return Err(CliError::user(
            "desktop target `iced` is not enabled in rmp.toml ([desktop].targets)",
        ));
    }

    let package = desktop
        .iced
        .and_then(|i| i.package)
        .filter(|s| !s.trim().is_empty())
        .unwrap_or_else(|| default_iced_package_name(&cfg.project.name));

    human_log(verbose, format!("cargo run -p {package}"));
    let mut cmd = Command::new("cargo");
    cmd.current_dir(root).arg("run").arg("-p").arg(&package);
    if release {
        cmd.arg("--release");
    }
    let status = cmd
        .stdout(Stdio::inherit())
        .stderr(Stdio::inherit())
        .status()
        .map_err(|e| CliError::operational(format!("failed to run cargo: {e}")))?;
    if !status.success() {
        return Err(CliError::operational(format!(
            "cargo run failed for desktop package `{package}`"
        )));
    }

    if json {
        json_print(&JsonOk {
            ok: true,
            data: serde_json::json!({"platform":"iced","package":package}),
        });
    } else {
        eprintln!("ok: iced app exited");
    }

    Ok(())
}

pub(crate) fn ensure_android_emulator(
    root: &Path,
    avd: &str,
    explicit_serial: Option<&str>,
    verbose: bool,
) -> Result<String, CliError> {
    let allow_headless = android_allow_headless();

    if let Some(s) = explicit_serial {
        if !adb_serial_exists(s)? {
            return Err(CliError::user(format!(
                "requested android serial not connected: {s}"
            )));
        }
        if !allow_headless && s.starts_with("emulator-") {
            let avd_name = emulator_avd_name(s).unwrap_or_else(|| avd.to_string());
            if emulator_is_headless_only(&avd_name)? {
                if avd_exists(&avd_name)? {
                    human_log(
                        verbose,
                        format!("emulator is headless (avd={avd_name}); restarting with GUI"),
                    );
                    kill_emulator(s, verbose)?;
                    return start_emulator_and_wait(root, &avd_name, verbose);
                }
                human_log(
                    verbose,
                    format!(
                        "emulator is headless (avd={avd_name}) but AVD is not available locally; keeping existing emulator"
                    ),
                );
            }
        }
        return Ok(s.to_string());
    }

    if let Some(s) = pick_any_emulator_serial()? {
        if !allow_headless {
            let avd_name = emulator_avd_name(&s).unwrap_or_else(|| avd.to_string());
            if emulator_is_headless_only(&avd_name)? {
                if avd_exists(&avd_name)? {
                    human_log(
                        verbose,
                        format!("emulator is headless (avd={avd_name}); restarting with GUI"),
                    );
                    kill_emulator(&s, verbose)?;
                    return start_emulator_and_wait(root, &avd_name, verbose);
                }
                human_log(
                    verbose,
                    format!(
                        "emulator is headless (avd={avd_name}) but AVD is not available locally; keeping existing emulator"
                    ),
                );
            }
        }
        human_log(
            verbose,
            format!("ok: android emulator already connected ({s})"),
        );
        return Ok(s);
    }

    let _ = Command::new("adb")
        .arg("start-server")
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status();

    // Ensure AVD exists.
    let mut cmd = Command::new("emulator");
    cmd.arg("-list-avds");
    let out = run_capture(cmd)?;
    if !out.status.success() {
        return Err(CliError::operational(
            "failed to list AVDs (emulator -list-avds)",
        ));
    }
    let list = String::from_utf8_lossy(&out.stdout);
    if !list.lines().any(|l| l.trim() == avd) {
        return Err(CliError::user(format!(
            "android AVD not found: {avd} (create it, then re-run)"
        )));
    }

    start_emulator_and_wait(root, avd, verbose)
}

fn start_emulator_and_wait(root: &Path, avd: &str, verbose: bool) -> Result<String, CliError> {
    human_log(verbose, format!("starting android emulator: {avd}"));
    let allow_headless = android_allow_headless();
    let log_path = root.join("emulator.log");
    let log = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&log_path)
        .map_err(|e| CliError::operational(format!("failed to open emulator log: {e}")))?;
    let log2 = log
        .try_clone()
        .map_err(|e| CliError::operational(format!("failed to clone emulator log handle: {e}")))?;

    let mut child = Command::new("emulator");
    child
        .arg("-avd")
        .arg(avd)
        .arg("-no-snapshot")
        .arg("-no-audio")
        .arg("-no-boot-anim")
        .arg("-gpu")
        .arg("swiftshader_indirect");
    if allow_headless {
        child.arg("-no-window");
    }
    child.stdout(Stdio::from(log)).stderr(Stdio::from(log2));

    let _ = child
        .spawn()
        .map_err(|e| CliError::operational(format!("failed to start emulator: {e}")))?;

    // Wait for boot.
    let start = Instant::now();
    while start.elapsed() < Duration::from_secs(180) {
        if let Some(serial) = pick_any_emulator_serial()? {
            let boot = adb_shell(&serial, &["getprop", "sys.boot_completed"])?;
            if boot.trim() == "1" {
                human_log(verbose, format!("ok: android emulator booted ({serial})"));
                return Ok(serial);
            }
        }
        thread::sleep(Duration::from_secs(1));
    }

    Err(CliError::operational(
        "android emulator did not boot in time (see emulator.log)",
    ))
}

fn kill_emulator(serial: &str, verbose: bool) -> Result<(), CliError> {
    human_log(verbose, format!("killing emulator: {serial}"));
    let _ = Command::new("adb")
        .arg("-s")
        .arg(serial)
        .arg("emu")
        .arg("kill")
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status();

    let start = Instant::now();
    while start.elapsed() < Duration::from_secs(30) {
        if !adb_serial_exists(serial)? {
            return Ok(());
        }
        thread::sleep(Duration::from_millis(500));
    }
    Err(CliError::operational(format!(
        "timed out waiting for emulator {serial} to exit"
    )))
}

fn emulator_avd_name(serial: &str) -> Option<String> {
    let mut cmd = Command::new("adb");
    cmd.arg("-s").arg(serial).arg("emu").arg("avd").arg("name");
    let out = run_capture(cmd).ok()?;
    if !out.status.success() {
        return None;
    }
    let s = String::from_utf8_lossy(&out.stdout);
    let avd = s.lines().last()?.trim();
    if avd.is_empty() || avd.eq_ignore_ascii_case("ok") {
        return None;
    }
    Some(avd.to_string())
}

fn emulator_is_headless_only(avd: &str) -> Result<bool, CliError> {
    // Mirrors previous shell behavior:
    // headless qemu exists AND no emulator frontend process exists for this AVD.
    let mut cmd = Command::new("ps");
    cmd.arg("ax").arg("-o").arg("command=");
    let out = run_capture(cmd)?;
    if !out.status.success() {
        return Err(CliError::operational(
            "failed to inspect emulator processes",
        ));
    }

    let needle = format!("-avd {avd}");
    let mut has_headless_qemu = false;
    let mut has_frontend = false;
    let s = String::from_utf8_lossy(&out.stdout);
    for line in s.lines() {
        if !line.contains(&needle) {
            continue;
        }
        if line.contains("qemu-system") && line.contains("headless") {
            has_headless_qemu = true;
        }
        if line.contains("/emulator") || line.starts_with("emulator ") {
            has_frontend = true;
        }
    }
    Ok(has_headless_qemu && !has_frontend)
}

fn android_allow_headless() -> bool {
    if std::env::var("RMP_ANDROID_ALLOW_HEADLESS").ok().as_deref() == Some("1") {
        return true;
    }
    is_ci()
}

fn is_ci() -> bool {
    std::env::var("CI")
        .ok()
        .map(|v| !v.trim().is_empty())
        .unwrap_or(false)
}

fn build_profile(release: bool) -> BuildProfile {
    if release {
        BuildProfile::Release
    } else {
        BuildProfile::Debug
    }
}

fn default_iced_package_name(project_name: &str) -> String {
    let mut out = String::new();
    for c in project_name.chars() {
        if c.is_ascii_alphanumeric() || c == '_' || c == '-' {
            out.push(c.to_ascii_lowercase());
        }
    }
    if out.is_empty() {
        out.push_str("app");
    }
    format!("{out}_desktop_iced")
}

fn ios_sim_target_for_host() -> Result<(&'static str, &'static str), CliError> {
    match std::env::consts::ARCH {
        "aarch64" => Ok(("aarch64-apple-ios-sim", "arm64")),
        "x86_64" => Ok(("x86_64-apple-ios", "x86_64")),
        arch => Err(CliError::operational(format!(
            "unsupported host arch for iOS simulator builds: {arch}"
        ))),
    }
}

fn avd_exists(avd: &str) -> Result<bool, CliError> {
    let mut cmd = Command::new("emulator");
    cmd.arg("-list-avds");
    let out = run_capture(cmd)?;
    if !out.status.success() {
        return Err(CliError::operational(
            "failed to list AVDs (emulator -list-avds)",
        ));
    }
    let s = String::from_utf8_lossy(&out.stdout);
    Ok(s.lines().any(|l| l.trim() == avd))
}

fn adb_serial_exists(serial: &str) -> Result<bool, CliError> {
    let mut cmd = Command::new("adb");
    cmd.arg("devices");
    let out = run_capture(cmd)?;
    if !out.status.success() {
        return Err(CliError::operational("adb devices failed"));
    }
    let s = String::from_utf8_lossy(&out.stdout);
    Ok(s.lines()
        .skip(1)
        .any(|l| l.split_whitespace().next() == Some(serial)))
}

fn pick_any_emulator_serial() -> Result<Option<String>, CliError> {
    let mut cmd = Command::new("adb");
    cmd.arg("devices");
    let out = run_capture(cmd)?;
    if !out.status.success() {
        return Err(CliError::operational("adb devices failed"));
    }
    let s = String::from_utf8_lossy(&out.stdout);
    for ln in s.lines().skip(1) {
        let parts: Vec<&str> = ln.split_whitespace().collect();
        if parts.len() < 2 {
            continue;
        }
        if parts[1] != "device" {
            continue;
        }
        if parts[0].starts_with("emulator-") {
            return Ok(Some(parts[0].to_string()));
        }
    }
    Ok(None)
}

fn adb_shell(serial: &str, args: &[&str]) -> Result<String, CliError> {
    let mut cmd = Command::new("adb");
    cmd.arg("-s").arg(serial).arg("shell").args(args);
    let out = run_capture(cmd)?;
    if !out.status.success() {
        return Err(CliError::operational("adb shell failed"));
    }
    Ok(String::from_utf8_lossy(&out.stdout).replace('\r', ""))
}

fn detect_android_abi(serial: &str, verbose: bool) -> Result<String, CliError> {
    let raw = adb_shell(serial, &["getprop", "ro.product.cpu.abi"])?;
    let raw = raw.lines().next().unwrap_or("").trim();
    if raw.is_empty() {
        return Err(CliError::operational(
            "failed to detect Android ABI (getprop ro.product.cpu.abi returned empty)",
        ));
    }
    let abi = normalize_android_abi(raw).unwrap_or(raw).to_string();
    human_log(
        verbose,
        format!("android ABI detected: {raw} (using {abi})"),
    );
    Ok(abi)
}

fn normalize_android_abi(raw: &str) -> Option<&'static str> {
    match raw {
        "arm64-v8a" | "aarch64" => Some("arm64-v8a"),
        "armeabi-v7a" | "armeabi" => Some("armeabi-v7a"),
        "x86_64" => Some("x86_64"),
        "x86" => Some("x86"),
        _ => None,
    }
}

fn setup_adb_reverse(serial: &str, spec: &str, verbose: bool) -> Result<(), CliError> {
    for item in spec.split(',').map(|s| s.trim()).filter(|s| !s.is_empty()) {
        let (dev_port, host_port) = if let Some((a, b)) = item.split_once(':') {
            (a.trim(), b.trim())
        } else {
            (item, item)
        };
        human_log(
            verbose,
            format!("adb reverse tcp:{dev_port} -> tcp:{host_port}"),
        );
        let status = Command::new("adb")
            .arg("-s")
            .arg(serial)
            .arg("reverse")
            .arg(format!("tcp:{dev_port}"))
            .arg(format!("tcp:{host_port}"))
            .stdout(Stdio::inherit())
            .stderr(Stdio::inherit())
            .status()
            .map_err(|e| CliError::operational(format!("failed to run adb reverse: {e}")))?;
        if !status.success() {
            return Err(CliError::operational("adb reverse failed"));
        }
    }
    Ok(())
}

fn launch_android(serial: &str, pkg: &str, verbose: bool) -> Result<(), CliError> {
    human_log(verbose, format!("launching {pkg}"));

    let resolved = adb_shell(
        serial,
        &[
            "cmd",
            "package",
            "resolve-activity",
            "--brief",
            "-a",
            "android.intent.action.MAIN",
            "-c",
            "android.intent.category.LAUNCHER",
            pkg,
        ],
    )
    .unwrap_or_default();
    let resolved = resolved.lines().last().unwrap_or("").trim().to_string();

    if resolved.contains('/') {
        let _ = Command::new("adb")
            .arg("-s")
            .arg(serial)
            .arg("shell")
            .arg("am")
            .arg("start")
            .arg("-W")
            .arg("-n")
            .arg(resolved)
            .stdout(Stdio::inherit())
            .stderr(Stdio::inherit())
            .status()
            .map_err(|e| CliError::operational(format!("failed to run adb am start: {e}")))?;
    } else {
        let _ = Command::new("adb")
            .arg("-s")
            .arg(serial)
            .arg("shell")
            .arg("am")
            .arg("start")
            .arg("-W")
            .arg("-a")
            .arg("android.intent.action.MAIN")
            .arg("-c")
            .arg("android.intent.category.LAUNCHER")
            .arg("-n")
            .arg(format!("{pkg}/.MainActivity"))
            .stdout(Stdio::inherit())
            .stderr(Stdio::inherit())
            .status()
            .map_err(|e| CliError::operational(format!("failed to run adb am start: {e}")))?;
    }

    for _ in 0..20 {
        let out = Command::new("adb")
            .arg("-s")
            .arg(serial)
            .arg("shell")
            .arg("pidof")
            .arg(pkg)
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .output();
        if let Ok(out) = out {
            if out.status.success() && !String::from_utf8_lossy(&out.stdout).trim().is_empty() {
                return Ok(());
            }
        }
        thread::sleep(Duration::from_millis(500));
    }

    Err(CliError::operational(format!(
        "app did not appear to start (pidof {pkg} empty)"
    )))
}

fn resolve_ios_app_path(
    dev_dir: &Path,
    xcode_project_path: &Path,
    xcode_scheme: &str,
    udid: &str,
    xcode_config: &str,
    xcode_arch: &str,
) -> Result<PathBuf, CliError> {
    let mut cmd = Command::new("/usr/bin/xcrun");
    cmd.env("DEVELOPER_DIR", dev_dir)
        .env_remove("LD")
        .env_remove("CC")
        .env_remove("CXX")
        .arg("xcodebuild")
        .arg("-project")
        .arg(xcode_project_path)
        .arg("-scheme")
        .arg(xcode_scheme)
        .arg("-destination")
        .arg(format!("id={udid}"))
        .arg("-configuration")
        .arg(xcode_config)
        .arg("-sdk")
        .arg("iphonesimulator")
        .arg(format!("ARCHS={xcode_arch}"))
        .arg("ONLY_ACTIVE_ARCH=YES")
        .arg("-showBuildSettings");
    let out = run_capture(cmd)?;
    if !out.status.success() {
        return Err(CliError::operational(
            "xcodebuild -showBuildSettings failed",
        ));
    }

    let stdout = String::from_utf8_lossy(&out.stdout);
    let mut target_build_dir: Option<String> = None;
    let mut full_product_name: Option<String> = None;
    for line in stdout.lines() {
        let line = line.trim();
        if let Some(v) = line.strip_prefix("TARGET_BUILD_DIR = ") {
            target_build_dir = Some(v.trim().to_string());
            continue;
        }
        if let Some(v) = line.strip_prefix("FULL_PRODUCT_NAME = ") {
            full_product_name = Some(v.trim().to_string());
        }
    }

    let target_build_dir = target_build_dir.ok_or_else(|| {
        CliError::operational("xcodebuild -showBuildSettings missing TARGET_BUILD_DIR")
    })?;
    let full_product_name = full_product_name.ok_or_else(|| {
        CliError::operational("xcodebuild -showBuildSettings missing FULL_PRODUCT_NAME")
    })?;

    Ok(PathBuf::from(target_build_dir).join(full_product_name))
}

/// Read the `name:` field from `ios/project.yml` to derive the Xcode project/target/app name.
fn read_xcode_project_name(root: &Path) -> Option<String> {
    let yml_path = root.join("ios/project.yml");
    let content = std::fs::read_to_string(&yml_path).ok()?;
    for line in content.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with("name:") {
            let name = trimmed.strip_prefix("name:")?.trim();
            // Strip optional quotes.
            let name = name.trim_matches('"').trim_matches('\'').trim();
            if !name.is_empty() {
                return Some(name.to_string());
            }
        }
    }
    None
}
