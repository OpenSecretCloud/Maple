use std::path::Path;
use std::process::Command;

use serde::Serialize;

use crate::cli::{human_log, json_print, CliError, DeviceStartPlatform, DevicesStartArgs, JsonOk};
use crate::config::load_rmp_toml;
use crate::run::{ensure_android_emulator, ensure_ios_simulator};
use crate::util::{discover_xcode_dev_dir, run_capture};

#[derive(Serialize)]
struct DevicesJson {
    devices: Vec<DeviceItem>,
}

#[derive(Serialize)]
struct DeviceItem {
    id: String,
    platform: String, // ios|android
    kind: String,     // device|simulator|emulator
    name: Option<String>,
    os: Option<String>,
    boot_state: Option<String>, // booted|shutdown (simulators/emulators)
    connection_state: Option<String>, // connected|... (devices)
}

#[derive(Serialize)]
struct DeviceStartJson {
    platform: String,
    kind: String,
    id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    avd: Option<String>,
}

#[derive(Debug)]
struct IosDev {
    udid: String,
    name: String,
    os: Option<String>,
}

fn ios_connected_devices(dev_dir: &Path) -> Result<Vec<IosDev>, CliError> {
    let mut cmd = Command::new("/usr/bin/xcrun");
    cmd.env("DEVELOPER_DIR", dev_dir)
        .arg("xctrace")
        .arg("list")
        .arg("devices");
    let out = run_capture(cmd)?;
    if !out.status.success() {
        return Err(CliError::operational(
            "failed to list iOS devices (xcrun xctrace list devices)",
        ));
    }
    let s = String::from_utf8_lossy(&out.stdout);
    let mut in_devices = false;
    let mut res = vec![];
    for line in s.lines() {
        let ln = line.trim();
        if ln == "== Devices ==" {
            in_devices = true;
            continue;
        }
        if ln == "== Simulators ==" {
            in_devices = false;
            continue;
        }
        if !in_devices {
            continue;
        }
        if !(ln.contains("iPhone") || ln.contains("iPad")) {
            continue;
        }
        // Format: "<name> (<os>) (<udid>)"
        let Some(i1) = ln.rfind(" (") else { continue };
        let Some(i0) = ln[..i1].rfind(" (") else {
            continue;
        };
        let name = ln[..i0].to_string();
        let mut os = ln[i0 + 2..i1].to_string();
        os = os.trim_end_matches(')').to_string();
        let udid = ln[i1 + 2..].trim_end_matches(')').to_string();
        if udid.len() < 25 {
            continue;
        }
        res.push(IosDev {
            udid,
            name,
            os: Some(os),
        });
    }
    res.sort_by(|a, b| (a.name.as_str(), a.udid.as_str()).cmp(&(b.name.as_str(), b.udid.as_str())));
    Ok(res)
}

#[derive(Debug)]
struct IosSim {
    udid: String,
    name: String,
    state: String, // Booted|Shutdown
    os: Option<String>,
}

fn ios_simulators(dev_dir: &Path) -> Result<Vec<IosSim>, CliError> {
    let mut cmd = Command::new("/usr/bin/xcrun");
    cmd.env("DEVELOPER_DIR", dev_dir)
        .arg("simctl")
        .arg("list")
        .arg("-j")
        .arg("devices")
        .arg("available");
    let out = run_capture(cmd)?;
    if !out.status.success() {
        return Err(CliError::operational(
            "failed to list iOS simulators (xcrun simctl list -j devices available)",
        ));
    }
    let j: serde_json::Value = serde_json::from_slice(&out.stdout)
        .map_err(|e| CliError::operational(format!("failed to parse simctl JSON output: {e}")))?;
    let mut sims: Vec<IosSim> = vec![];
    let devices = j
        .get("devices")
        .and_then(|v| v.as_object())
        .cloned()
        .unwrap_or_default();
    for (runtime, devs) in devices {
        if !runtime.contains("iOS") {
            continue;
        }
        let Some(list) = devs.as_array() else {
            continue;
        };
        for d in list {
            if d.get("isAvailable").and_then(|v| v.as_bool()) == Some(false) {
                continue;
            }
            let udid = d
                .get("udid")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string();
            let name = d
                .get("name")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string();
            let state = d
                .get("state")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string();
            let os = d
                .get("osVersion")
                .and_then(|v| v.as_str())
                .map(|s| s.to_string());
            if udid.is_empty() || name.is_empty() {
                continue;
            }
            sims.push(IosSim {
                udid,
                name,
                state,
                os,
            });
        }
    }
    sims.sort_by(|a, b| {
        (a.state != "Booted", a.name.as_str(), a.udid.as_str()).cmp(&(
            b.state != "Booted",
            b.name.as_str(),
            b.udid.as_str(),
        ))
    });
    Ok(sims)
}

#[derive(Debug)]
struct AndroidTarget {
    serial: String,
    model: Option<String>,
    product: Option<String>,
}

fn android_targets() -> Result<Vec<AndroidTarget>, CliError> {
    let mut cmd = Command::new("adb");
    cmd.arg("devices").arg("-l");
    let out = run_capture(cmd)?;
    if !out.status.success() {
        return Err(CliError::operational(
            "failed to list android devices (adb devices -l)",
        ));
    }
    let s = String::from_utf8_lossy(&out.stdout);
    let mut res: Vec<AndroidTarget> = vec![];
    for line in s.lines() {
        let ln = line.trim();
        if ln.is_empty() || ln.starts_with("List of devices") {
            continue;
        }
        let parts: Vec<&str> = ln.split_whitespace().collect();
        if parts.len() < 2 {
            continue;
        }
        let serial = parts[0].to_string();
        let state = parts[1];
        if state != "device" {
            continue;
        }
        let mut model: Option<String> = None;
        let mut product: Option<String> = None;
        for tok in parts.iter().skip(2) {
            if let Some((k, v)) = tok.split_once(':') {
                match k {
                    "model" => model = Some(v.to_string()),
                    "product" => product = Some(v.to_string()),
                    _ => {}
                }
            }
        }
        res.push(AndroidTarget {
            serial,
            model,
            product,
        });
    }
    res.sort_by(|a, b| {
        (!a.serial.starts_with("emulator-"), a.serial.as_str())
            .cmp(&(!b.serial.starts_with("emulator-"), b.serial.as_str()))
    });
    Ok(res)
}

pub fn devices_list(root: &Path, json: bool, verbose: bool) -> Result<(), CliError> {
    let _cfg = load_rmp_toml(root)?;
    let dev_dir = discover_xcode_dev_dir()?;

    let ios_devs = ios_connected_devices(&dev_dir)?;
    let ios_sims = ios_simulators(&dev_dir)?;
    let android = android_targets()?;

    let mut out: Vec<DeviceItem> = vec![];
    for d in ios_devs {
        out.push(DeviceItem {
            id: d.udid,
            platform: "ios".into(),
            kind: "device".into(),
            name: Some(d.name),
            os: d.os,
            boot_state: None,
            connection_state: Some("connected".into()),
        });
    }
    for s in ios_sims {
        out.push(DeviceItem {
            id: s.udid,
            platform: "ios".into(),
            kind: "simulator".into(),
            name: Some(s.name),
            os: s.os,
            boot_state: Some(s.state.to_lowercase()),
            connection_state: None,
        });
    }
    for a in android {
        let is_emulator = a.serial.starts_with("emulator-");
        let kind = if is_emulator { "emulator" } else { "device" };
        out.push(DeviceItem {
            id: a.serial,
            platform: "android".into(),
            kind: kind.into(),
            name: a.model.or(a.product),
            os: None,
            boot_state: if is_emulator {
                Some("booted".into())
            } else {
                None
            },
            connection_state: if is_emulator {
                None
            } else {
                Some("connected".into())
            },
        });
    }

    out.sort_by(|a, b| {
        (
            a.platform.as_str(),
            a.kind.as_str(),
            a.name.as_deref().unwrap_or(""),
            a.id.as_str(),
        )
            .cmp(&(
                b.platform.as_str(),
                b.kind.as_str(),
                b.name.as_deref().unwrap_or(""),
                b.id.as_str(),
            ))
    });
    human_log(verbose, format!("found {} devices", out.len()));

    if json {
        json_print(&JsonOk {
            ok: true,
            data: DevicesJson { devices: out },
        });
    } else {
        eprintln!("Devices:");
        for d in &out {
            let extra = match (&d.name, &d.os, &d.boot_state) {
                (Some(n), Some(os), Some(bs)) => format!("{n} ({os}) [{bs}]"),
                (Some(n), Some(os), None) => format!("{n} ({os})"),
                (Some(n), None, Some(bs)) => format!("{n} [{bs}]"),
                (Some(n), None, None) => n.clone(),
                _ => "".into(),
            };
            eprintln!(
                "  {} {}: {}{}",
                d.platform,
                d.kind,
                d.id,
                if extra.is_empty() {
                    "".into()
                } else {
                    format!("  {extra}")
                }
            );
        }
    }

    Ok(())
}

pub fn devices_start(
    root: &Path,
    json: bool,
    verbose: bool,
    args: DevicesStartArgs,
) -> Result<(), CliError> {
    let cfg = load_rmp_toml(root)?;
    match args.platform {
        DeviceStartPlatform::Android => {
            let android = cfg
                .android
                .ok_or_else(|| CliError::user("rmp.toml missing [android] section"))?;
            let avd = args
                .android
                .avd
                .or(android.avd_name)
                .unwrap_or_else(|| "rmp_api35".into());
            let serial =
                ensure_android_emulator(root, &avd, args.android.serial.as_deref(), verbose)?;

            if json {
                json_print(&JsonOk {
                    ok: true,
                    data: DeviceStartJson {
                        platform: "android".into(),
                        kind: if serial.starts_with("emulator-") {
                            "emulator".into()
                        } else {
                            "device".into()
                        },
                        id: serial,
                        avd: Some(avd),
                    },
                });
            } else {
                eprintln!("ok: android target ready");
            }
        }
        DeviceStartPlatform::Ios => {
            let _ios = cfg
                .ios
                .ok_or_else(|| CliError::user("rmp.toml missing [ios] section"))?;
            let dev_dir = discover_xcode_dev_dir()?;
            let udid = ensure_ios_simulator(&dev_dir, args.ios.udid.as_deref(), verbose)?;
            let _ = Command::new("open").arg("-a").arg("Simulator").status();

            if json {
                json_print(&JsonOk {
                    ok: true,
                    data: DeviceStartJson {
                        platform: "ios".into(),
                        kind: "simulator".into(),
                        id: udid,
                        avd: None,
                    },
                });
            } else {
                eprintln!("ok: ios simulator ready");
            }
        }
    }
    Ok(())
}
