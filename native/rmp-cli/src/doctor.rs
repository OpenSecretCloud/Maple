use std::path::Path;
use std::process::Command;

use crate::cli::{human_log, json_print, CliError, JsonOk};
use crate::config::load_rmp_toml;
use crate::util::{discover_xcode_dev_dir, run_capture, which};

#[derive(serde::Serialize)]
struct DoctorJson {
    in_nix_shell: bool,
    xcode_developer_dir: Option<String>,
    android_home: Option<String>,
    desktop_targets: Vec<String>,
}

pub fn doctor(root: &Path, json: bool, verbose: bool) -> Result<(), CliError> {
    let cfg = load_rmp_toml(root)?;

    let in_nix = std::env::var("IN_NIX_SHELL")
        .ok()
        .filter(|v| !v.is_empty())
        .is_some();
    if !in_nix && !json {
        human_log(
            verbose,
            "note: IN_NIX_SHELL not set; recommended: run via `nix develop` for a consistent toolchain",
        );
    }

    if which("cargo").is_none() {
        return Err(CliError::operational(
            "missing `cargo` on PATH (run inside nix develop)",
        ));
    }
    if which("rustc").is_none() {
        return Err(CliError::operational(
            "missing `rustc` on PATH (run inside nix develop)",
        ));
    }

    let mut xcode_developer_dir = None;
    if cfg.ios.is_some() {
        // iOS: Xcode + sim runtimes.
        let dev_dir = discover_xcode_dev_dir()?;
        let mut cmd = Command::new("/usr/bin/xcrun");
        cmd.env("DEVELOPER_DIR", &dev_dir)
            .arg("simctl")
            .arg("list")
            .arg("runtimes");
        let out = run_capture(cmd)?;
        if !out.status.success() {
            return Err(CliError::operational(
                "failed to run `xcrun simctl list runtimes` (check Xcode install)",
            ));
        }
        let runtimes = String::from_utf8_lossy(&out.stdout);
        if runtimes.lines().count() <= 1 {
            return Err(CliError::operational(
                "no iOS simulator runtimes installed (simctl list runtimes is empty)",
            ));
        }
        xcode_developer_dir = Some(dev_dir.to_string_lossy().to_string());
    }

    let mut android_home = None;
    if cfg.android.is_some() {
        // Android: adb/emulator + ANDROID_HOME best-effort.
        if which("adb").is_none() {
            return Err(CliError::operational(
                "missing `adb` on PATH (run inside nix develop)",
            ));
        }
        if which("emulator").is_none() {
            return Err(CliError::operational(
                "missing `emulator` on PATH (run inside nix develop)",
            ));
        }
        android_home = std::env::var("ANDROID_HOME").ok().filter(|s| !s.is_empty());
        if android_home.is_none() && !json {
            human_log(
                verbose,
                "note: ANDROID_HOME not set (may be set by nix develop)",
            );
        }
    }

    let desktop_targets = cfg
        .desktop
        .as_ref()
        .map(|d| d.targets.clone())
        .unwrap_or_default();
    let desktop_iced_enabled = desktop_targets
        .iter()
        .any(|t| t.eq_ignore_ascii_case("iced"));
    if desktop_iced_enabled && cfg!(target_os = "linux") && !json {
        eprintln!(
            "note: ICED on Linux may require Wayland/X11 runtime libs (for example libxkbcommon, libwayland-client, libX11)"
        );
    }

    if json {
        json_print(&JsonOk {
            ok: true,
            data: DoctorJson {
                in_nix_shell: in_nix,
                xcode_developer_dir,
                android_home,
                desktop_targets,
            },
        });
    } else {
        eprintln!("ok: doctor checks passed");
    }

    Ok(())
}
