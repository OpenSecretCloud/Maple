//! Per-account ACP limits in the host-owned store. Previous hand-edited
//! configuration files are never inspected or imported.

use crate::store::{AccountId, Store};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
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
const ACP_SETTINGS_KEY: &str = "acp";

fn config_account(store: &Store, user_id: &str) -> Result<AccountId, String> {
    crate::maple_api::account_scope(user_id)?;
    store
        .account_id(user_id)
        .map_err(|error| error.to_string())?
        .ok_or_else(|| "ACP configuration requires the active signed-in account".to_string())
}

/// Load only this store's account row, using defaults for a new account.
/// No prior or intermediate JSON file is a fallback.
pub fn load_acp_config(store: &Store, user_id: &str) -> Result<AgentAcpConfig, String> {
    let account = config_account(store, user_id)?;
    match store
        .account_setting(account, ACP_SETTINGS_KEY)
        .map_err(|error| error.to_string())?
    {
        Some(value) => serde_json::from_value(value)
            .map_err(|_| "Stored ACP configuration is invalid".to_string())
            .and_then(normalize_config),
        None => Ok(AgentAcpConfig::default()),
    }
}

/// Validate the entire replacement before a durable settings transaction.
pub fn save_acp_config(
    store: &Store,
    user_id: &str,
    config: AgentAcpConfig,
    now_ms: i64,
) -> Result<AgentAcpConfig, String> {
    let account = config_account(store, user_id)?;
    let config = normalize_config(config)?;
    let value = serde_json::to_value(&config)
        .map_err(|_| "Could not encode ACP configuration".to_string())?;
    store
        .set_account_setting(account, ACP_SETTINGS_KEY, &value, now_ms)
        .map_err(|error| error.to_string())?;
    Ok(config)
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
