use std::path::{Path, PathBuf};

use crate::cli::CliError;

pub fn find_workspace_root(start: &Path) -> Result<PathBuf, CliError> {
    let mut cur = start
        .canonicalize()
        .map_err(|e| CliError::operational(format!("failed to resolve cwd: {e}")))?;
    loop {
        if cur.join("rmp.toml").is_file() {
            return Ok(cur);
        }
        if !cur.pop() {
            break;
        }
    }
    Err(CliError::user(
        "could not find rmp.toml (searches current dir and parents)",
    ))
}

#[allow(dead_code)]
#[derive(serde::Deserialize, Debug, Clone)]
pub struct RmpToml {
    pub project: RmpProject,
    pub core: RmpCore,
    pub ios: Option<RmpIos>,
    pub android: Option<RmpAndroid>,
    pub desktop: Option<RmpDesktop>,
}

#[allow(dead_code)]
#[derive(serde::Deserialize, Debug, Clone)]
pub struct RmpProject {
    pub name: String,
    pub org: String,
}

#[allow(dead_code)]
#[derive(serde::Deserialize, Debug, Clone)]
pub struct RmpCore {
    #[serde(rename = "crate")]
    pub crate_: String,
    pub bindings: String,
}

#[allow(dead_code)]
#[derive(serde::Deserialize, Debug, Clone)]
pub struct RmpIos {
    pub bundle_id: String,
    pub scheme: Option<String>,
}

#[allow(dead_code)]
#[derive(serde::Deserialize, Debug, Clone)]
pub struct RmpAndroid {
    pub app_id: String,
    pub avd_name: Option<String>,
}

#[allow(dead_code)]
#[derive(serde::Deserialize, Debug, Clone)]
pub struct RmpDesktop {
    #[serde(default)]
    pub targets: Vec<String>,
    pub iced: Option<RmpDesktopIced>,
}

#[allow(dead_code)]
#[derive(serde::Deserialize, Debug, Clone)]
pub struct RmpDesktopIced {
    pub package: Option<String>,
}

pub fn load_rmp_toml(root: &Path) -> Result<RmpToml, CliError> {
    let p = root.join("rmp.toml");
    let s = std::fs::read_to_string(&p)
        .map_err(|e| CliError::operational(format!("failed to read rmp.toml: {e}")))?;
    let cfg: RmpToml =
        toml::from_str(&s).map_err(|e| CliError::user(format!("failed to parse rmp.toml: {e}")))?;
    Ok(cfg)
}
