//! The account's MCP servers: their settings, normalized and checked before
//! they are saved (names, commands, endpoints, environment variables and
//! headers), and the servers each task runs, over rmcp, by Pi's conventions.

use std::collections::{BTreeMap, HashSet};
use std::path::{Path, PathBuf};

use super::config::{
    account_local_data_dir_path, load_agent_config_inner, normalize_project_root, path_string,
    save_agent_config_inner, write_device_local_json_file,
};
use super::integrations::{is_cua_identity, validate_new_mcp_integration_collisions};
use super::{
    AgentMcpKeyValue, AgentMcpServer, AgentMcpTransport, AgentPathLayout, AgentRuntimeHandle,
};

mod connection;
#[cfg(test)]
pub(super) mod fake_server;
mod task;
mod tool;

pub(super) use task::{
    MAX_IDLE_TASKS_WITH_SERVERS, STARTUP_WAIT, TaskMcp, read_saved_servers, servers_for_new_task,
    set_chosen_servers, task_servers,
};

const MAX_MCP_SERVER_NAME_CHARS: usize = 64;

/// Explicit new-task choices, keyed by canonical project root and server key.
/// Missing choices inherit the catalog's current default.
type ProjectMcpDefaults = BTreeMap<String, BTreeMap<String, bool>>;

fn project_mcp_defaults_path(paths: &AgentPathLayout, user_id: &str) -> Result<PathBuf, String> {
    account_local_data_dir_path(paths, user_id)
        .map(|directory| directory.join("project_mcp_defaults.json"))
        .map_err(|error| error.to_string())
}

fn read_project_mcp_defaults(path: &Path) -> Result<ProjectMcpDefaults, String> {
    match std::fs::read(path) {
        Ok(bytes) => serde_json::from_slice(&bytes)
            .map_err(|error| format!("Failed to read project MCP defaults: {error}")),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(BTreeMap::new()),
        Err(error) => Err(format!("Failed to read project MCP defaults: {error}")),
    }
}

/// Apply only this project's explicit switches over the live catalog defaults.
pub(super) fn servers_with_project_defaults(
    paths: &AgentPathLayout,
    user_id: &str,
    project_root: &Path,
    mut servers: Vec<AgentMcpServer>,
) -> Result<Vec<AgentMcpServer>, String> {
    let defaults = read_project_mcp_defaults(&project_mcp_defaults_path(paths, user_id)?)?;
    if let Some(choices) = defaults.get(&path_string(project_root)) {
        for server in &mut servers {
            if !is_cua_identity(&server.name)
                && let Some(enabled) = choices.get(&name_to_key(&server.name))
            {
                server.enabled = *enabled;
            }
        }
    }
    Ok(servers)
}

/// Keys of Maple's own tool groups, which a server cannot take.
const RESERVED_KEYS: [&str; 2] = ["developer", "maple-skills-extension"];

/// Variables a server's environment cannot set: they change how programs and
/// libraries are found and loaded.
const DISALLOWED_ENVIRONMENT: [&str; 31] = [
    "PATH",
    "PATHEXT",
    "SystemRoot",
    "windir",
    "LD_LIBRARY_PATH",
    "LD_PRELOAD",
    "LD_AUDIT",
    "LD_DEBUG",
    "LD_BIND_NOW",
    "LD_ASSUME_KERNEL",
    "DYLD_LIBRARY_PATH",
    "DYLD_INSERT_LIBRARIES",
    "DYLD_FRAMEWORK_PATH",
    "PYTHONPATH",
    "PYTHONHOME",
    "NODE_OPTIONS",
    "RUBYOPT",
    "GEM_PATH",
    "GEM_HOME",
    "CLASSPATH",
    "GO111MODULE",
    "GOROOT",
    "APPINIT_DLLS",
    "SESSIONNAME",
    "ComSpec",
    "TEMP",
    "TMP",
    "LOCALAPPDATA",
    "USERPROFILE",
    "HOMEDRIVE",
    "HOMEPATH",
];

/// A server name as a key: lowercase letters, digits, `_` and `-`; spaces
/// are dropped and anything else becomes `_`.
pub(super) fn name_to_key(name: &str) -> String {
    name.chars()
        .filter(|c| !c.is_whitespace())
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '_' || c == '-' {
                c.to_ascii_lowercase()
            } else {
                '_'
            }
        })
        .collect()
}

impl AgentMcpServer {
    /// The stable catalog identity used to match a server's display name with
    /// saved project and task choices.
    pub fn key_for_name(name: &str) -> String {
        name_to_key(name)
    }
}

/// A command line's words. Quotes keep spaces in a word; a single quote opens
/// only at the start of one.
pub(super) fn split_command(command: &str) -> Result<Vec<String>, String> {
    let mut words = Vec::new();
    let mut current = String::new();
    let (mut in_double, mut in_single) = (false, false);
    for c in command.chars() {
        match c {
            '"' if !in_single => in_double = !in_double,
            '\'' if !in_double && (in_single || current.is_empty()) => in_single = !in_single,
            c if c.is_whitespace() && !in_double && !in_single => {
                if !current.is_empty() {
                    words.push(std::mem::take(&mut current));
                }
            }
            c => current.push(c),
        }
    }
    if in_double || in_single {
        return Err("Unmatched quote in command".to_string());
    }
    if !current.is_empty() {
        words.push(current);
    }
    Ok(words)
}

fn validate_key_values(
    entries: &mut [AgentMcpKeyValue],
    server_name: &str,
    label: &str,
    case_insensitive: bool,
) -> Result<(), String> {
    let mut keys = HashSet::new();
    for entry in entries {
        entry.key = entry.key.trim().to_string();
        if entry.key.is_empty() {
            return Err(format!(
                "MCP server '{server_name}' has an empty {label} name"
            ));
        }
        if label == "HTTP header" && entry.key.chars().any(char::is_whitespace) {
            return Err(format!(
                "MCP server '{server_name}' HTTP header names cannot contain whitespace"
            ));
        }
        let comparison = if case_insensitive {
            entry.key.to_ascii_lowercase()
        } else {
            entry.key.clone()
        };
        if !keys.insert(comparison) {
            return Err(format!(
                "MCP server '{server_name}' has a duplicate {label} named {}",
                entry.key
            ));
        }
    }
    Ok(())
}

/// The servers trimmed and checked: unique, usable names; a command or an
/// endpoint; a timeout; and environment variables and headers that are named,
/// distinct, and do not change how programs are loaded.
pub(super) fn normalize_mcp_servers(
    mut servers: Vec<AgentMcpServer>,
) -> Result<Vec<AgentMcpServer>, String> {
    let mut names = HashSet::new();
    for server in &mut servers {
        server.name = server.name.trim().to_string();
        server.description = server.description.trim().to_string();
        if server.name.is_empty() {
            return Err("MCP server name cannot be empty".to_string());
        }
        if server.name.chars().count() > MAX_MCP_SERVER_NAME_CHARS {
            return Err(format!(
                "MCP server name '{}' must be 64 characters or fewer",
                server.name
            ));
        }
        let key = name_to_key(&server.name);
        if key.is_empty() {
            return Err(format!(
                "MCP server name '{}' must contain a letter, number, underscore, or hyphen",
                server.name
            ));
        }
        if RESERVED_KEYS.contains(&key.as_str()) {
            return Err(format!(
                "The MCP server name '{}' is reserved by Maple",
                server.name
            ));
        }
        if !names.insert(key) {
            return Err(format!(
                "MCP server name '{}' conflicts with another configured server",
                server.name
            ));
        }
        if server.timeout_seconds == 0 {
            return Err(format!(
                "MCP server '{}' must have a timeout greater than zero",
                server.name
            ));
        }

        let environment = match &mut server.transport {
            AgentMcpTransport::Stdio {
                command,
                environment,
            } => {
                *command = command.trim().to_string();
                if command.is_empty() {
                    return Err(format!("MCP server '{}' requires a command", server.name));
                }
                let words = split_command(command).map_err(|error| {
                    format!(
                        "MCP server '{}' has an invalid command: {error}",
                        server.name
                    )
                })?;
                if words.first().is_none_or(String::is_empty) {
                    return Err(format!(
                        "MCP server '{}' requires an executable",
                        server.name
                    ));
                }
                validate_key_values(environment, &server.name, "environment variable", false)?;
                environment
            }
            AgentMcpTransport::StreamableHttp {
                url,
                environment,
                headers,
            } => {
                *url = url.trim().to_string();
                if url.is_empty() {
                    return Err(format!(
                        "MCP server '{}' requires an endpoint URL",
                        server.name
                    ));
                }
                validate_key_values(environment, &server.name, "environment variable", false)?;
                validate_key_values(headers, &server.name, "HTTP header", true)?;
                environment
            }
        };
        if let Some(entry) = environment.iter().find(|entry| {
            DISALLOWED_ENVIRONMENT
                .iter()
                .any(|disallowed| disallowed.eq_ignore_ascii_case(&entry.key))
        }) {
            return Err(format!(
                "MCP server '{}' cannot override the environment variable {}",
                server.name, entry.key
            ));
        }
    }
    Ok(servers)
}

impl AgentRuntimeHandle {
    /// The account's custom MCP catalog with this project's new-task defaults.
    /// Servers without an explicit project choice inherit the Settings default.
    pub async fn list_project_mcp_servers(
        &self,
        project_root: String,
    ) -> Result<Vec<AgentMcpServer>, String> {
        self.verify_generation().await?;
        let project_root = normalize_project_root(Path::new(&project_root))?;
        let _settings = self.lock_settings().await;
        self.verify_generation().await?;
        let config = load_agent_config_inner(self.paths(), &self.user_id)
            .map_err(|error| error.to_string())?;
        servers_with_project_defaults(
            self.paths(),
            &self.user_id,
            &project_root,
            normalize_mcp_servers(config.mcp_servers)?,
        )
    }

    /// Pin one custom server's new-task default on this device. This does not
    /// connect the server or alter any existing task's choices.
    pub async fn set_project_mcp_server_enabled(
        &self,
        project_root: String,
        name: String,
        enabled: bool,
    ) -> Result<(), String> {
        self.verify_generation().await?;
        self.ensure_accepting_new_work()?;
        let project_root = normalize_project_root(Path::new(&project_root))?;
        let name = name.trim();
        let key = name_to_key(name);
        if key.is_empty() || RESERVED_KEYS.contains(&key.as_str()) || is_cua_identity(name) {
            return Err("That MCP server cannot be changed".to_string());
        }
        let _settings = self.lock_settings().await;
        self.verify_generation().await?;
        self.ensure_accepting_new_work()?;
        let config = load_agent_config_inner(self.paths(), &self.user_id)
            .map_err(|error| error.to_string())?;
        let servers = normalize_mcp_servers(config.mcp_servers)?;
        if !servers
            .iter()
            .any(|server| name_to_key(&server.name) == key)
        {
            return Err(format!("MCP server '{name}' is no longer configured"));
        }
        let path = project_mcp_defaults_path(self.paths(), &self.user_id)?;
        let mut defaults = read_project_mcp_defaults(&path)?;
        defaults
            .entry(path_string(&project_root))
            .or_default()
            .insert(key, enabled);
        write_device_local_json_file(&path, &defaults)
            .map_err(|error| format!("Failed to save project MCP defaults: {error}"))
    }

    /// The account's saved MCP servers, checked as a save checks them.
    pub async fn list_mcp_servers(&self) -> Result<Vec<AgentMcpServer>, String> {
        self.verify_generation().await?;
        let _settings = self.lock_settings().await;
        let config = load_agent_config_inner(self.paths(), &self.user_id)
            .map_err(|error| error.to_string())?;
        normalize_mcp_servers(config.mcp_servers)
    }

    /// Check and save the account's MCP servers, in place of the saved ones.
    pub async fn save_mcp_servers(
        &self,
        servers: Vec<AgentMcpServer>,
    ) -> Result<Vec<AgentMcpServer>, String> {
        self.verify_generation().await?;
        self.ensure_accepting_new_work()?;
        let servers = normalize_mcp_servers(servers)?;
        let _settings = self.lock_settings().await;
        let mut config = load_agent_config_inner(self.paths(), &self.user_id)
            .map_err(|error| error.to_string())?;
        validate_new_mcp_integration_collisions(&config.mcp_servers, &servers)?;
        config.mcp_servers = servers.clone();
        save_agent_config_inner(self.paths(), &self.user_id, &config)
            .map_err(|error| error.to_string())?;
        Ok(servers)
    }
}

#[cfg(test)]
mod tests;
