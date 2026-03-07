use std::path::PathBuf;
use std::process::{Command, Output, Stdio};

use crate::cli::CliError;

pub fn which(bin: &str) -> Option<PathBuf> {
    let path = std::env::var_os("PATH")?;
    for dir in std::env::split_paths(&path) {
        let p = dir.join(bin);
        if p.is_file() {
            return Some(p);
        }
        #[cfg(windows)]
        {
            let p = dir.join(format!("{bin}.exe"));
            if p.is_file() {
                return Some(p);
            }
        }
    }
    None
}

pub fn run_capture(mut cmd: Command) -> Result<Output, CliError> {
    let out = cmd
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .output()
        .map_err(|e| CliError::operational(format!("failed to spawn process: {e}")))?;
    Ok(out)
}

pub fn discover_xcode_dev_dir() -> Result<PathBuf, CliError> {
    if let Ok(v) = std::env::var("DEVELOPER_DIR") {
        let p = PathBuf::from(v);
        if (p.join("usr/bin/xcrun").exists() || p.join("usr/bin/simctl").exists())
            && developer_dir_supports_iphoneos(&p)
        {
            return Ok(p);
        }
    }
    let apps = std::fs::read_dir("/Applications")
        .map_err(|e| CliError::operational(format!("failed to read /Applications: {e}")))?;
    let mut candidates: Vec<PathBuf> = vec![];
    for ent in apps.flatten() {
        let name = ent.file_name();
        let name = name.to_string_lossy();
        if !name.starts_with("Xcode") || !name.ends_with(".app") {
            continue;
        }
        let dev = ent.path().join("Contents/Developer");
        if dev.is_dir() {
            candidates.push(dev);
        }
    }
    candidates.sort();
    for dev in candidates.into_iter().rev() {
        if developer_dir_supports_iphoneos(&dev) {
            return Ok(dev);
        }
    }
    Err(CliError::operational(
        "Xcode with iPhoneOS SDK not found under /Applications",
    ))
}

fn developer_dir_supports_iphoneos(dev_dir: &PathBuf) -> bool {
    let out = Command::new("/usr/bin/xcrun")
        .env("DEVELOPER_DIR", dev_dir)
        .arg("--sdk")
        .arg("iphoneos")
        .arg("--show-sdk-path")
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status();
    matches!(out, Ok(status) if status.success())
}

// (reserved) write_file_atomic: will be useful once `rmp init` lands.
