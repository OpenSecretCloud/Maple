//! Per-account ACP configuration: the on-disk shape, where it lives, and
//! how it is loaded.

use crate::maple_api::account_scope;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::sync::atomic::AtomicUsize;

/// Upper bound on concurrent ACP connections one account may hold open.
pub(super) const MAX_ACP_CONNECTIONS: usize = 8;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct AgentAcpConfig {
    #[serde(default)]
    pub allowed_project_roots: Vec<String>,
    #[serde(default = "default_max_connections")]
    pub max_connections: usize,
}

fn default_max_connections() -> usize {
    8
}

impl Default for AgentAcpConfig {
    fn default() -> Self {
        Self {
            allowed_project_roots: Vec::new(),
            max_connections: default_max_connections(),
        }
    }
}
#[derive(Default)]
pub(super) struct AgentAcpStats {
    pub(super) active_sessions: AtomicUsize,
    pub(super) active_runs: AtomicUsize,
}
/// Per-account ACP configuration from under `local_data_root`, or the
/// defaults when there is none.
pub fn load_acp_config(local_data_root: &Path, user_id: &str) -> Result<AgentAcpConfig, String> {
    load_config(local_data_root, user_id)
}
pub(super) fn config_path(local_data_root: &Path, user_id: &str) -> Result<PathBuf, String> {
    let scope = account_scope(user_id)
        .map_err(|_| "Maple ACP configuration requires an authenticated user".to_string())?;
    config_path_for_scope(local_data_root, &scope)
}

pub(super) fn config_path_for_scope(
    local_data_root: &Path,
    scope: &str,
) -> Result<PathBuf, String> {
    Ok(acp_accounts_root(local_data_root)?
        .join(scope)
        .join("config.json"))
}

pub(super) fn acp_accounts_root(local_data_root: &Path) -> Result<PathBuf, String> {
    Ok(local_data_root.join("acp").join("accounts"))
}

pub(super) fn load_config(local_data_root: &Path, user_id: &str) -> Result<AgentAcpConfig, String> {
    let path = config_path(local_data_root, user_id)?;
    match std::fs::read(&path) {
        Ok(bytes) => serde_json::from_slice(&bytes)
            .map_err(|error| format!("Failed to parse Maple ACP configuration: {error}"))
            .and_then(normalize_config),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(AgentAcpConfig::default()),
        Err(error) => Err(format!("Failed to read Maple ACP configuration: {error}")),
    }
}

/// Clamps the connection limit and keeps each project root once, refusing a
/// relative one.
pub(super) fn normalize_config(mut config: AgentAcpConfig) -> Result<AgentAcpConfig, String> {
    config.max_connections = config.max_connections.clamp(1, MAX_ACP_CONNECTIONS);
    let mut roots = Vec::new();
    for root in config.allowed_project_roots {
        let root = root.trim();
        if root.is_empty() {
            continue;
        }
        let path = PathBuf::from(root);
        if !path.is_absolute() {
            return Err("ACP allowed project roots must be absolute paths".to_string());
        }
        let root = path.to_string_lossy().into_owned();
        if !roots.contains(&root) {
            roots.push(root);
        }
    }
    config.allowed_project_roots = roots;
    Ok(config)
}
