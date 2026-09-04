//! Maple-curated, device-local integrations.
//!
//! This module owns the product-level catalog, validation, device-local
//! default, and migration between an installed external MCP server and a
//! Maple-hosted implementation. A task freezes that backend choice when it is
//! created; account defaults never rewrite existing tasks.

use super::*;
use std::collections::HashSet;
#[cfg(target_os = "macos")]
use std::process::Stdio;
#[cfg(target_os = "macos")]
use std::time::Duration;

pub(super) const CUA_DRIVER_INTEGRATION_ID: &str = "cua-driver";
/// The name Goose shows for the extension and that Maple uses when it has to
/// talk about the integration in an error.
pub(super) const CUA_DRIVER_NAME: &str = "Computer use (CUA)";
pub(super) const CUA_DRIVER_MCP_NAME: &str = "cua-driver";
pub(super) const CUA_DRIVER_DESCRIPTION: &str =
    "Let models view and control desktop applications using CUA built into Maple.";
/// What the Integrations page shows. The catalog owns this copy so the page
/// renders what it is given instead of keeping a second set of strings.
const CUA_DRIVER_CARD_NAME: &str = "Cua";
const CUA_DRIVER_CARD_DESCRIPTION: &str = "Let Maple see and control apps on this computer.";
const CUA_EXTERNAL_MCP_DESCRIPTION: &str =
    "Control desktop applications through the locally installed Cua Driver.";
#[cfg(target_os = "macos")]
const CUA_DRIVER_MACOS_BINARY: &str = "/Applications/CuaDriver.app/Contents/MacOS/cua-driver";
const INTEGRATIONS_FILE_NAME: &str = "integrations.json";
const INTEGRATIONS_FILE_VERSION: u32 = 2;
const LEGACY_INTEGRATIONS_FILE_VERSION: u32 = 1;
#[cfg(any(target_os = "macos", test))]
const CUA_MANIFEST_SCHEMA_VERSION: &str = "1";
#[cfg(target_os = "macos")]
const CUA_PROBE_TIMEOUT: Duration = Duration::from_secs(3);
#[cfg(target_os = "macos")]
const MAX_CUA_MANIFEST_BYTES: usize = 64 * 1024;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(super) struct StoredIntegrationRegistry {
    version: u32,
    #[serde(default)]
    integrations: Vec<StoredIntegration>,
}

impl Default for StoredIntegrationRegistry {
    fn default() -> Self {
        Self {
            version: INTEGRATIONS_FILE_VERSION,
            integrations: Vec::new(),
        }
    }
}

impl StoredIntegrationRegistry {
    fn cua(&self) -> Option<&StoredIntegration> {
        self.integrations
            .iter()
            .find(|entry| entry.id == CUA_DRIVER_INTEGRATION_ID)
    }

    fn cua_mut(&mut self) -> Option<&mut StoredIntegration> {
        self.integrations
            .iter_mut()
            .find(|entry| entry.id == CUA_DRIVER_INTEGRATION_ID)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct StoredIntegration {
    id: String,
    enabled: bool,
    backend: AgentIntegrationBackend,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    external_server: Option<AgentMcpServer>,
}

#[derive(Debug, Clone, Deserialize)]
struct LegacyStoredIntegrationRegistry {
    version: u32,
    #[serde(default)]
    integrations: Vec<LegacyStoredIntegration>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
struct LegacyStoredIntegration {
    id: String,
    server: AgentMcpServer,
}

#[cfg(any(target_os = "macos", test))]
#[derive(Debug, Deserialize)]
struct CuaManifest {
    schema_version: String,
    binary_path: String,
    binary_version: String,
    mcp_invocation: CuaMcpInvocation,
}

#[cfg(any(target_os = "macos", test))]
#[derive(Debug, Deserialize)]
struct CuaMcpInvocation {
    command: String,
    args: Vec<String>,
}

#[derive(Debug)]
pub(super) struct CuaDetection {
    availability: AgentIntegrationAvailability,
    permissions: Option<AgentIntegrationPermissions>,
    standalone_version: Option<String>,
    detail: Option<String>,
    external_server: Option<AgentMcpServer>,
}

impl CuaDetection {
    #[cfg(any(not(embedded_cua), test))]
    fn not_detected() -> Self {
        Self {
            availability: AgentIntegrationAvailability::NotDetected,
            permissions: None,
            standalone_version: None,
            detail: Some("Built-in CUA is not available on this operating system yet.".to_string()),
            external_server: None,
        }
    }

    fn embedded_ready(&self) -> bool {
        self.permissions
            .as_ref()
            .is_some_and(AgentIntegrationPermissions::ready)
    }

    fn public(&self, stored: Option<&StoredIntegration>) -> AgentIntegration {
        AgentIntegration {
            id: CUA_DRIVER_INTEGRATION_ID.to_string(),
            name: CUA_DRIVER_CARD_NAME.to_string(),
            description: CUA_DRIVER_CARD_DESCRIPTION.to_string(),
            availability: self.availability,
            backend: stored.map(|entry| entry.backend),
            version: embedded_cua_version(),
            standalone_version: self.standalone_version.clone(),
            permissions: self.permissions.clone(),
            enabled_for_new_tasks: stored.is_some_and(|entry| entry.enabled),
            detail: self.detail.clone(),
        }
    }
}

#[cfg(embedded_cua)]
fn embedded_cua_version() -> Option<String> {
    Some(super::cua::EMBEDDED_CUA_VERSION.to_string())
}

#[cfg(not(embedded_cua))]
fn embedded_cua_version() -> Option<String> {
    None
}

pub(super) async fn detect_integrations() -> CuaDetection {
    detect_cua_driver().await
}

/// The only integration Maple curates today. Every entry point that accepts an
/// integration id checks it here so they cannot disagree.
pub(super) fn require_known_integration(id: &str) -> Result<(), String> {
    if id.trim() == CUA_DRIVER_INTEGRATION_ID {
        return Ok(());
    }
    Err(format!("Unknown integration '{}'", id.trim()))
}

pub(super) fn project_integrations(
    paths: &AgentPathLayout,
    user_id: &str,
    detection: &CuaDetection,
) -> Result<Vec<AgentIntegration>, String> {
    let stored = load_stored_integrations(paths, user_id)?;
    Ok(vec![detection.public(stored.cua())])
}

pub(super) fn set_integration_default(
    paths: &AgentPathLayout,
    user_id: &str,
    request: &AgentSetIntegrationEnabledRequest,
    detection: &CuaDetection,
) -> Result<Vec<AgentIntegration>, String> {
    require_known_integration(&request.id)?;

    let custom = normalize_mcp_servers(
        load_agent_config_inner(paths, user_id)
            .map_err(|error| format!("Failed to load MCP servers: {error}"))?
            .mcp_servers,
    )?;
    let mut stored = load_stored_integrations(paths, user_id)?;

    if request.enabled {
        ensure_no_custom_integration_collision(&custom, CUA_DRIVER_MCP_NAME)?;
        match stored.cua_mut() {
            Some(entry) => {
                match entry.backend {
                    AgentIntegrationBackend::Embedded if !detection.embedded_ready() => {
                        return Err(
                            "Set up Maple's Accessibility and Screen Recording permissions before enabling built-in CUA"
                                .to_string(),
                        );
                    }
                    AgentIntegrationBackend::External => {
                        let server = detection.external_server.clone().ok_or_else(|| {
                            "The standalone CuaDriver selected by this setting is no longer available. Set up built-in CUA instead."
                                .to_string()
                        })?;
                        entry.external_server = Some(server);
                    }
                    AgentIntegrationBackend::Embedded => {}
                }
                entry.enabled = true;
            }
            None if detection.embedded_ready() => {
                stored.integrations.push(StoredIntegration {
                    id: CUA_DRIVER_INTEGRATION_ID.to_string(),
                    enabled: true,
                    backend: AgentIntegrationBackend::Embedded,
                    external_server: detection.external_server.clone(),
                });
            }
            None => {
                let server = detection.external_server.clone().ok_or_else(|| {
                    "Set up Maple's Accessibility and Screen Recording permissions before enabling CUA"
                        .to_string()
                })?;
                stored.integrations.push(StoredIntegration {
                    id: CUA_DRIVER_INTEGRATION_ID.to_string(),
                    enabled: true,
                    backend: AgentIntegrationBackend::External,
                    external_server: Some(server),
                });
            }
        }
    } else if let Some(entry) = stored.cua_mut() {
        entry.enabled = false;
    }

    save_stored_integrations(paths, user_id, &stored)?;
    Ok(vec![detection.public(stored.cua())])
}

/// Switch the new-task default to Maple's embedded backend only after the OS
/// reports both grants. An incomplete setup never silently takes a working
/// external backend away from the user.
pub(super) fn select_embedded_integration_backend(
    paths: &AgentPathLayout,
    user_id: &str,
    detection: &CuaDetection,
) -> Result<Vec<AgentIntegration>, String> {
    if !detection.embedded_ready() {
        return project_integrations(paths, user_id, detection);
    }
    let custom = normalize_mcp_servers(
        load_agent_config_inner(paths, user_id)
            .map_err(|error| format!("Failed to load MCP servers: {error}"))?
            .mcp_servers,
    )?;
    ensure_no_custom_integration_collision(&custom, CUA_DRIVER_MCP_NAME)?;
    let mut stored = load_stored_integrations(paths, user_id)?;
    match stored.cua_mut() {
        Some(entry) => {
            entry.backend = AgentIntegrationBackend::Embedded;
            if detection.external_server.is_some() {
                entry.external_server = detection.external_server.clone();
            }
        }
        None => stored.integrations.push(StoredIntegration {
            id: CUA_DRIVER_INTEGRATION_ID.to_string(),
            enabled: false,
            backend: AgentIntegrationBackend::Embedded,
            external_server: detection.external_server.clone(),
        }),
    }
    save_stored_integrations(paths, user_id, &stored)?;
    project_integrations(paths, user_id, detection)
}

pub(super) fn effective_mcp_servers(
    stored: &StoredIntegrationRegistry,
    custom: Vec<AgentMcpServer>,
) -> Result<Vec<AgentMcpServer>, String> {
    let mut servers = custom;
    for entry in &stored.integrations {
        let Some(server) = entry.external_server.as_ref() else {
            continue;
        };
        // The integration owns this extension key. A custom server that a
        // previous release let the user save under the same name is shadowed
        // rather than merged: merging would make normalization fail and lock
        // the account out of every task, and silently keeping the custom one
        // would let it be replaced later without the user being told.
        servers.retain(|candidate| !is_cua_key(&candidate.name));
        let mut server = server.clone();
        // Keep the concrete external definition available to old tasks,
        // but only select it by default while External owns the global
        // default. Embedded sessions are represented in Maple metadata.
        server.enabled = entry.enabled && entry.backend == AgentIntegrationBackend::External;
        servers.push(server);
    }
    normalize_mcp_servers(servers)
}

/// Whether a configured server name addresses the curated CUA integration.
pub(super) fn is_cua_key(name: &str) -> bool {
    goose::config::extensions::name_to_key(name.trim()) == CUA_DRIVER_MCP_NAME
}

/// Read the device-local registry for a path that must keep working.
///
/// Task creation and the composer MCP menu must not fail because an optional
/// device-local file was written by a newer build or damaged. Those paths get
/// an empty registry and a log line instead of an error. None of them writes
/// the file, so the stored choice is never clobbered and the Integrations page
/// still reports the real problem.
pub(super) fn stored_integrations_for_read(
    paths: &AgentPathLayout,
    user_id: &str,
) -> StoredIntegrationRegistry {
    match load_stored_integrations(paths, user_id) {
        Ok(registry) => registry,
        Err(error) => {
            log::warn!("Ignoring unusable device-local integration settings: {error}");
            StoredIntegrationRegistry::default()
        }
    }
}

/// Freeze the device default into a newly-created task. Explicit server names
/// override the enabled-by-default bit, matching custom MCP selection. The
/// backend itself always comes from the Maple-managed registry and cannot be
/// supplied by an external caller.
pub(super) fn cua_state_for_new_session(
    stored: &StoredIntegrationRegistry,
    requested_names: Option<&[String]>,
    allow_embedded: bool,
) -> Result<Option<CuaSessionState>, String> {
    let Some(entry) = stored.cua() else {
        return Ok(None);
    };
    let explicitly_requested =
        requested_names.map(|names| names.iter().any(|name| is_cua_key(name)));
    let enabled = explicitly_requested.unwrap_or(entry.enabled);
    if entry.backend == AgentIntegrationBackend::Embedded && !allow_embedded {
        if explicitly_requested == Some(true) {
            return Err(
                "Built-in CUA is available only to tasks running in the Maple desktop app"
                    .to_string(),
            );
        }
        return Ok(None);
    }
    Ok(Some(CuaSessionState {
        backend: entry.backend,
        enabled,
    }))
}

pub(super) fn requested_mcp_names_without_embedded_cua(
    requested_names: Option<&[String]>,
    cua_state: Option<CuaSessionState>,
) -> Option<Vec<String>> {
    requested_names.map(|names| {
        names
            .iter()
            .filter(|name| {
                cua_state.is_none_or(|state| {
                    state.backend != AgentIntegrationBackend::Embedded || !is_cua_key(name)
                })
            })
            .cloned()
            .collect()
    })
}

/// Which backend a task uses, in the order the answer becomes authoritative.
///
/// A task that recorded its own choice keeps it. A task that predates that
/// metadata but holds the concrete external extension is unambiguous. Only a
/// task that never expressed a choice falls back to the device default, and
/// only when it could actually run that default: a task outside the desktop
/// app never adopts the embedded backend, because it cannot use it.
pub(super) fn session_cua_backend(
    stored: &StoredIntegrationRegistry,
    session: &Session,
) -> Option<AgentIntegrationBackend> {
    if let Some(state) = session_cua_state(session) {
        return Some(state.backend);
    }
    if session_mcp_extension_keys(session).contains(CUA_DRIVER_MCP_NAME) {
        // Tasks created by the external-driver PR predate Maple's logical
        // metadata. Their concrete persisted stdio extension is unambiguous.
        return Some(AgentIntegrationBackend::External);
    }
    let backend = stored.cua().map(|entry| entry.backend)?;
    if backend == AgentIntegrationBackend::Embedded && session.session_type != SessionType::User {
        return None;
    }
    Some(backend)
}

pub(super) fn project_session_mcp_servers(
    stored: &StoredIntegrationRegistry,
    configured: &[AgentMcpServer],
    session: &Session,
) -> Result<Vec<AgentSessionMcpServer>, String> {
    let mut servers = session_mcp_servers(configured, session);
    servers.retain(|server| !is_cua_key(&server.name));

    let state = session_cua_state(session);
    let active_external = session_mcp_extension_keys(session).contains(CUA_DRIVER_MCP_NAME);
    let Some(backend) = session_cua_backend(stored, session) else {
        return Ok(servers);
    };
    let stored = stored.cua();
    let enabled = match backend {
        AgentIntegrationBackend::Embedded => state.is_some_and(|state| state.enabled),
        AgentIntegrationBackend::External => active_external,
    };
    let available = match backend {
        AgentIntegrationBackend::Embedded => embedded_cua_ready(),
        AgentIntegrationBackend::External => {
            active_external || stored.is_some_and(|entry| entry.external_server.is_some())
        }
    };
    servers.push(AgentSessionMcpServer {
        name: CUA_DRIVER_MCP_NAME.to_string(),
        description: CUA_DRIVER_DESCRIPTION.to_string(),
        transport: match backend {
            AgentIntegrationBackend::Embedded => "embedded",
            AgentIntegrationBackend::External => "stdio",
        }
        .to_string(),
        enabled,
        available,
    });
    Ok(servers)
}

#[cfg(embedded_cua)]
fn embedded_cua_ready() -> bool {
    super::cua::embedded_cua_permission_status().ready()
}

#[cfg(not(embedded_cua))]
fn embedded_cua_ready() -> bool {
    false
}

/// Reject a *newly* introduced custom server that would shadow the curated
/// integration.
///
/// A name an earlier release accepted stays saveable, so one legacy entry
/// cannot make every unrelated MCP edit fail. It is still shadowed at
/// selection time by [`effective_mcp_servers`], and enabling the integration
/// still refuses outright while it exists.
pub(super) fn validate_new_mcp_integration_collisions(
    previous: &[AgentMcpServer],
    next: &[AgentMcpServer],
) -> Result<(), String> {
    let existing = previous
        .iter()
        .map(|server| goose::config::extensions::name_to_key(&server.name))
        .collect::<HashSet<_>>();
    let added = next
        .iter()
        .filter(|server| !existing.contains(&goose::config::extensions::name_to_key(&server.name)))
        .cloned()
        .collect::<Vec<_>>();
    ensure_no_custom_integration_collision(&added, CUA_DRIVER_MCP_NAME)
}

fn ensure_no_custom_integration_collision(
    custom: &[AgentMcpServer],
    integration_name: &str,
) -> Result<(), String> {
    // Treat the human-readable and conventional config spellings as one
    // product identity even though Goose preserves '-' and '_' in its lower
    // level extension key. Otherwise a custom server could shadow Maple's
    // built-in Computer use integration under its historical MCP name.
    let integration_keys = [
        integration_name,
        CUA_DRIVER_NAME,
        "Cua Driver",
        "cua_driver",
    ]
    .into_iter()
    .map(goose::config::extensions::name_to_key)
    .collect::<HashSet<_>>();
    if let Some(server) = custom.iter().find(|server| {
        integration_keys.contains(&goose::config::extensions::name_to_key(&server.name))
    }) {
        return Err(format!(
            "Custom MCP server '{}' conflicts with the {CUA_DRIVER_NAME} integration. Rename or remove the custom server before enabling the integration.",
            server.name
        ));
    }
    Ok(())
}

fn integrations_path(paths: &AgentPathLayout, user_id: &str) -> Result<PathBuf, String> {
    Ok(account_local_data_dir_path(paths, user_id)
        .map_err(|error| error.to_string())?
        .join(INTEGRATIONS_FILE_NAME))
}

fn load_stored_integrations(
    paths: &AgentPathLayout,
    user_id: &str,
) -> Result<StoredIntegrationRegistry, String> {
    let path = integrations_path(paths, user_id)?;
    if !path
        .try_exists()
        .map_err(|error| format!("Failed to inspect device-local integration settings: {error}"))?
    {
        return Ok(StoredIntegrationRegistry::default());
    }
    let contents = fs::read_to_string(&path)
        .map_err(|error| format!("Failed to read device-local integration settings: {error}"))?;
    let value: Value = serde_json::from_str(&contents)
        .map_err(|error| format!("Failed to parse device-local integration settings: {error}"))?;
    let version = value
        .get("version")
        .and_then(Value::as_u64)
        .ok_or_else(|| "Device-local integration settings have no version".to_string())?;
    let registry = match u32::try_from(version) {
        Ok(INTEGRATIONS_FILE_VERSION) => serde_json::from_str(&contents).map_err(|error| {
            format!("Failed to parse device-local integration settings: {error}")
        })?,
        Ok(LEGACY_INTEGRATIONS_FILE_VERSION) => {
            let legacy: LegacyStoredIntegrationRegistry =
                serde_json::from_str(&contents).map_err(|error| {
                    format!("Failed to parse legacy device-local integration settings: {error}")
                })?;
            migrate_legacy_registry(legacy)?
        }
        _ => {
            return Err(format!(
                "Unsupported device-local integration settings version {version}"
            ));
        }
    };
    validate_stored_registry(registry)
}

fn migrate_legacy_registry(
    registry: LegacyStoredIntegrationRegistry,
) -> Result<StoredIntegrationRegistry, String> {
    if registry.version != LEGACY_INTEGRATIONS_FILE_VERSION {
        return Err(format!(
            "Unsupported legacy integration settings version {}",
            registry.version
        ));
    }
    let integrations = registry
        .integrations
        .into_iter()
        .map(|entry| {
            let enabled = entry.server.enabled;
            StoredIntegration {
                id: entry.id,
                enabled,
                backend: AgentIntegrationBackend::External,
                external_server: Some(entry.server),
            }
        })
        .collect();
    Ok(StoredIntegrationRegistry {
        version: INTEGRATIONS_FILE_VERSION,
        integrations,
    })
}

fn validate_stored_registry(
    registry: StoredIntegrationRegistry,
) -> Result<StoredIntegrationRegistry, String> {
    if registry.version != INTEGRATIONS_FILE_VERSION {
        return Err(format!(
            "Unsupported device-local integration settings version {}",
            registry.version
        ));
    }
    let mut ids = HashSet::new();
    for entry in &registry.integrations {
        if entry.id != CUA_DRIVER_INTEGRATION_ID || !ids.insert(entry.id.as_str()) {
            return Err(
                "Device-local integration settings contain an unknown or duplicate integration"
                    .to_string(),
            );
        }
        if entry.backend == AgentIntegrationBackend::External && entry.external_server.is_none() {
            return Err(
                "Device-local external Cua Driver settings have no server definition".to_string(),
            );
        }
        if let Some(server) = entry.external_server.as_ref() {
            validate_stored_cua_server(server)?;
        }
    }
    Ok(registry)
}

fn validate_stored_cua_server(server: &AgentMcpServer) -> Result<(), String> {
    if server.name != CUA_DRIVER_MCP_NAME
        || server.description != CUA_EXTERNAL_MCP_DESCRIPTION
        || server.timeout_seconds != DEFAULT_MCP_TIMEOUT_SECONDS
    {
        return Err("Device-local Cua Driver settings are invalid".to_string());
    }
    let AgentMcpTransport::Stdio {
        command,
        environment,
    } = &server.transport
    else {
        return Err("Device-local Cua Driver transport is invalid".to_string());
    };
    if !environment.is_empty() {
        return Err("Device-local Cua Driver environment must be empty".to_string());
    }
    let parts = split_mcp_command(command, CUA_DRIVER_MCP_NAME)?;
    if parts.len() != 2 || parts[1] != "mcp" || !Path::new(&parts[0]).is_absolute() {
        return Err("Device-local Cua Driver command is invalid".to_string());
    }
    Ok(())
}

fn save_stored_integrations(
    paths: &AgentPathLayout,
    user_id: &str,
    registry: &StoredIntegrationRegistry,
) -> Result<(), String> {
    validate_stored_registry(registry.clone())?;
    write_device_local_json_file(&integrations_path(paths, user_id)?, registry)
        .map_err(|error| format!("Failed to save device-local integration settings: {error}"))
}

async fn detect_cua_driver() -> CuaDetection {
    #[cfg(not(embedded_cua))]
    {
        CuaDetection::not_detected()
    }

    #[cfg(embedded_cua)]
    {
        let permissions = super::cua::embedded_cua_permission_status();
        // Only macOS ships a standalone CuaDriver application at a path Maple
        // knows. Everywhere else the built-in runtime is the only backend, so
        // there is nothing to discover and no foreign executable to run.
        let (standalone_version, external_server, standalone_error) =
            detect_standalone_driver().await;
        let embedded_ready = permissions.ready();
        let availability = if embedded_ready || external_server.is_some() {
            AgentIntegrationAvailability::Available
        } else {
            AgentIntegrationAvailability::SetupRequired
        };
        let detail = standalone_error.map(|error| {
            format!(
                "Maple could not verify the standalone CuaDriver installation: {error}. Built-in CUA can still be set up."
            )
        });
        CuaDetection {
            availability,
            permissions: Some(permissions),
            standalone_version,
            detail,
            external_server,
        }
    }
}

/// Discover a separately installed CuaDriver application, if this platform has
/// one at a path Maple knows.
#[cfg(target_os = "macos")]
async fn detect_standalone_driver() -> (Option<String>, Option<AgentMcpServer>, Option<String>) {
    let candidate = PathBuf::from(CUA_DRIVER_MACOS_BINARY);
    let exists = match candidate.try_exists() {
        Ok(exists) => exists,
        Err(error) => {
            return (
                None,
                None,
                Some(format!(
                    "could not inspect the standalone application: {error}"
                )),
            );
        }
    };
    if !exists {
        return (None, None, None);
    }
    if let Err(error) = ensure_standalone_binary_is_protected(&candidate) {
        return (None, None, Some(error));
    }
    match probe_cua_manifest(&candidate).await {
        Ok((version, server)) => (Some(version), Some(server), None),
        Err(error) => (None, None, Some(error)),
    }
}

#[cfg(all(embedded_cua, not(target_os = "macos")))]
async fn detect_standalone_driver() -> (Option<String>, Option<AgentMcpServer>, Option<String>) {
    (None, None, None)
}

/// Refuse to execute a driver that any other account can rewrite.
///
/// Maple runs this binary to read its manifest whenever the Integrations page
/// opens, so a group- or world-writable file at the expected path would let a
/// second account choose the code Maple runs. Ownership is deliberately not
/// checked: a normal drag-install leaves the application owned by the user who
/// installed it.
#[cfg(target_os = "macos")]
fn ensure_standalone_binary_is_protected(path: &Path) -> Result<(), String> {
    use std::os::unix::fs::PermissionsExt;

    let metadata = std::fs::metadata(path)
        .map_err(|error| format!("could not inspect the standalone application: {error}"))?;
    if metadata.permissions().mode() & 0o022 != 0 {
        return Err(
            "the standalone application is writable by other accounts and was not run".to_string(),
        );
    }
    Ok(())
}

#[cfg(target_os = "macos")]
async fn probe_cua_manifest(path: &Path) -> Result<(String, AgentMcpServer), String> {
    let canonical = path
        .canonicalize()
        .map_err(|error| format!("could not resolve the executable: {error}"))?;
    let mut command = tokio::process::Command::new(&canonical);
    command
        .arg("manifest")
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .kill_on_drop(true);
    let mut child = command
        .spawn()
        .map_err(|error| format!("could not start the manifest probe: {error}"))?;
    let stdout = child
        .stdout
        .take()
        .ok_or_else(|| "could not capture the manifest".to_string())?;
    let mut reader = tokio::spawn(super::bounded_process::read_bounded_stdout(
        stdout,
        MAX_CUA_MANIFEST_BYTES,
        "the Cua Driver manifest",
    ));

    let status = match tokio::time::timeout(CUA_PROBE_TIMEOUT, child.wait()).await {
        Ok(Ok(status)) => status,
        Ok(Err(error)) => {
            reader.abort();
            return Err(format!("could not wait for the manifest probe: {error}"));
        }
        Err(_) => {
            reader.abort();
            let _ = child.kill().await;
            let _ = child.wait().await;
            return Err("manifest probe timed out".to_string());
        }
    };
    let bytes = match tokio::time::timeout(CUA_PROBE_TIMEOUT, &mut reader).await {
        Ok(Ok(result)) => result?,
        Ok(Err(error)) => return Err(format!("could not collect the manifest: {error}")),
        Err(_) => {
            reader.abort();
            return Err("manifest output did not close".to_string());
        }
    };
    if !status.success() {
        return Err(format!("manifest probe exited with {status}"));
    }
    parse_cua_manifest(&canonical, &bytes)
}

#[cfg(any(target_os = "macos", test))]
fn parse_cua_manifest(
    detected_binary: &Path,
    bytes: &[u8],
) -> Result<(String, AgentMcpServer), String> {
    let manifest: CuaManifest = serde_json::from_slice(bytes)
        .map_err(|error| format!("manifest is not valid JSON: {error}"))?;
    if manifest.schema_version != CUA_MANIFEST_SCHEMA_VERSION {
        return Err(format!(
            "manifest schema {} is not supported",
            manifest.schema_version
        ));
    }
    let version = manifest.binary_version.trim();
    if version.is_empty() {
        return Err("manifest has no binary version".to_string());
    }
    if manifest.mcp_invocation.args != ["mcp"] {
        return Err("manifest does not advertise the expected MCP entrypoint".to_string());
    }

    let declared_binary = canonical_manifest_path(&manifest.binary_path, "binary path")?;
    let command_binary = canonical_manifest_path(&manifest.mcp_invocation.command, "MCP command")?;
    if declared_binary != detected_binary || command_binary != detected_binary {
        return Err("manifest executable does not match the detected application".to_string());
    }

    let verified_binary = detected_binary
        .to_str()
        .ok_or_else(|| "detected Cua Driver path is not valid UTF-8".to_string())?;
    let command = join_mcp_command(std::iter::once(verified_binary).chain(["mcp"]))?;
    let server = AgentMcpServer {
        name: CUA_DRIVER_MCP_NAME.to_string(),
        description: CUA_EXTERNAL_MCP_DESCRIPTION.to_string(),
        enabled: false,
        timeout_seconds: DEFAULT_MCP_TIMEOUT_SECONDS,
        transport: AgentMcpTransport::Stdio {
            command,
            environment: Vec::new(),
        },
    };
    validate_stored_cua_server(&server)?;
    Ok((version.to_string(), server))
}

#[cfg(any(target_os = "macos", test))]
fn canonical_manifest_path(path: &str, label: &str) -> Result<PathBuf, String> {
    let path = Path::new(path);
    if !path.is_absolute() {
        return Err(format!("manifest {label} is not absolute"));
    }
    path.canonicalize()
        .map_err(|error| format!("could not resolve manifest {label}: {error}"))
}

#[cfg(any(target_os = "macos", test))]
fn join_mcp_command<'a>(parts: impl IntoIterator<Item = &'a str>) -> Result<String, String> {
    parts
        .into_iter()
        .map(|part| {
            if part.is_empty() || part.contains(['\0', '"']) {
                return Err("manifest MCP arguments contain unsupported characters".to_string());
            }
            if part.chars().any(char::is_whitespace) {
                Ok(format!("\"{part}\""))
            } else {
                Ok(part.to_string())
            }
        })
        .collect::<Result<Vec<_>, _>>()
        .map(|parts| parts.join(" "))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn stored_cua_server(root: &Path, enabled: bool) -> AgentMcpServer {
        let binary = root.join("cua-driver");
        let command = join_mcp_command([binary.to_str().expect("UTF-8 test path"), "mcp"])
            .expect("valid test command");
        AgentMcpServer {
            name: CUA_DRIVER_MCP_NAME.to_string(),
            description: CUA_EXTERNAL_MCP_DESCRIPTION.to_string(),
            enabled,
            timeout_seconds: DEFAULT_MCP_TIMEOUT_SECONDS,
            transport: AgentMcpTransport::Stdio {
                command,
                environment: Vec::new(),
            },
        }
    }

    fn manifest(binary: &Path, schema: &str, args: &[&str]) -> Vec<u8> {
        serde_json::to_vec(&json!({
            "schema_version": schema,
            "binary_path": binary,
            "binary_version": "0.21.1-test",
            "mcp_invocation": {
                "command": binary,
                "args": args,
            }
        }))
        .unwrap()
    }

    #[test]
    fn manifest_builds_an_ordinary_disabled_mcp_server() {
        let temporary = tempfile::tempdir().unwrap();
        let binary = temporary.path().join("cua driver");
        fs::write(&binary, b"fixture").unwrap();
        let binary = binary.canonicalize().unwrap();

        let (version, server) =
            parse_cua_manifest(&binary, &manifest(&binary, "1", &["mcp"])).unwrap();

        assert_eq!(version, "0.21.1-test");
        assert!(!server.enabled);
        assert_eq!(server.name, CUA_DRIVER_MCP_NAME);
        assert_eq!(
            server.transport,
            AgentMcpTransport::Stdio {
                command: format!("\"{}\" mcp", binary.display()),
                environment: Vec::new(),
            }
        );
    }

    #[test]
    fn manifest_rejects_unknown_schema_and_non_mcp_invocation() {
        let temporary = tempfile::tempdir().unwrap();
        let binary = temporary.path().join("cua-driver");
        fs::write(&binary, b"fixture").unwrap();
        let binary = binary.canonicalize().unwrap();

        assert!(
            parse_cua_manifest(&binary, &manifest(&binary, "2", &["mcp"]))
                .unwrap_err()
                .contains("schema")
        );
        assert!(
            parse_cua_manifest(&binary, &manifest(&binary, "1", &["serve"]))
                .unwrap_err()
                .contains("MCP entrypoint")
        );
    }

    #[test]
    fn manifest_rejects_a_different_or_relative_binary() {
        let temporary = tempfile::tempdir().unwrap();
        let detected = temporary.path().join("cua-driver");
        let different = temporary.path().join("different");
        fs::write(&detected, b"fixture").unwrap();
        fs::write(&different, b"fixture").unwrap();
        let detected = detected.canonicalize().unwrap();
        let different = different.canonicalize().unwrap();

        assert!(
            parse_cua_manifest(&detected, &manifest(&different, "1", &["mcp"]))
                .unwrap_err()
                .contains("does not match")
        );
        let relative = br#"{
            "schema_version":"1",
            "binary_path":"cua-driver",
            "binary_version":"test",
            "mcp_invocation":{"command":"cua-driver","args":["mcp"]}
        }"#;
        assert!(
            parse_cua_manifest(&detected, relative)
                .unwrap_err()
                .contains("not absolute")
        );
    }

    #[test]
    fn stored_integrations_are_device_local_and_join_the_effective_registry() {
        let temporary = tempfile::tempdir().unwrap();
        let first = AgentPathLayout::from_app_roots(
            temporary.path().join("shared-config"),
            temporary.path().join("first-device"),
        );
        let second = AgentPathLayout::from_app_roots(
            temporary.path().join("shared-config"),
            temporary.path().join("second-device"),
        );
        let user = "cua-device-local@example.com";
        let server = stored_cua_server(temporary.path(), true);
        save_stored_integrations(
            &first,
            user,
            &StoredIntegrationRegistry {
                version: INTEGRATIONS_FILE_VERSION,
                integrations: vec![StoredIntegration {
                    id: CUA_DRIVER_INTEGRATION_ID.to_string(),
                    enabled: true,
                    backend: AgentIntegrationBackend::External,
                    external_server: Some(server.clone()),
                }],
            },
        )
        .unwrap();

        assert_eq!(
            effective_mcp_servers(&stored_integrations_for_read(&first, user), Vec::new()).unwrap(),
            vec![server]
        );
        assert!(
            effective_mcp_servers(&stored_integrations_for_read(&second, user), Vec::new())
                .unwrap()
                .is_empty()
        );
    }

    #[test]
    fn custom_server_collision_is_explicit() {
        let custom = AgentMcpServer {
            name: "Cua Driver".to_string(),
            description: String::new(),
            enabled: false,
            timeout_seconds: DEFAULT_MCP_TIMEOUT_SECONDS,
            transport: AgentMcpTransport::Stdio {
                command: "elsewhere mcp".to_string(),
                environment: Vec::new(),
            },
        };
        assert!(
            ensure_no_custom_integration_collision(&[custom], CUA_DRIVER_MCP_NAME)
                .unwrap_err()
                .contains("Rename or remove")
        );
    }

    fn http_server(name: &str) -> AgentMcpServer {
        AgentMcpServer {
            name: name.to_string(),
            description: String::new(),
            enabled: true,
            timeout_seconds: DEFAULT_MCP_TIMEOUT_SECONDS,
            transport: AgentMcpTransport::StreamableHttp {
                url: "https://example.com/mcp".to_string(),
                environment: Vec::new(),
                headers: Vec::new(),
            },
        }
    }

    #[test]
    fn an_unusable_registry_file_does_not_block_tasks() {
        let temporary = tempfile::tempdir().unwrap();
        let paths = AgentPathLayout::from_app_roots(
            temporary.path().join("config"),
            temporary.path().join("local-data"),
        );
        let user = "cua-unusable@example.com";
        let path = integrations_path(&paths, user).unwrap();
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        // A file written by a newer build. Task creation must survive it.
        std::fs::write(&path, br#"{"version": 99, "integrations": []}"#).unwrap();

        assert!(load_stored_integrations(&paths, user).is_err());
        let stored = stored_integrations_for_read(&paths, user);
        assert!(stored.cua().is_none());
        assert_eq!(
            effective_mcp_servers(&stored, vec![http_server("Docs")]).unwrap(),
            vec![http_server("Docs")]
        );
        // The unusable file is still there for Settings to report, not clobbered.
        assert!(path.exists());
    }

    #[test]
    fn a_legacy_cua_named_server_does_not_block_unrelated_saves() {
        let legacy = http_server("Cua Driver");
        // Saving the same list again, or adding an unrelated server, succeeds.
        assert!(
            validate_new_mcp_integration_collisions(
                std::slice::from_ref(&legacy),
                &[legacy.clone(), http_server("Docs")],
            )
            .is_ok()
        );
        // Introducing the colliding name for the first time still fails.
        assert!(
            validate_new_mcp_integration_collisions(
                &[http_server("Docs")],
                std::slice::from_ref(&legacy),
            )
            .unwrap_err()
            .contains("Rename or remove")
        );
    }

    #[test]
    fn the_integration_shadows_a_custom_server_that_shares_its_key() {
        let temporary = tempfile::tempdir().unwrap();
        let managed = stored_cua_server(temporary.path(), true);
        let stored = StoredIntegrationRegistry {
            version: INTEGRATIONS_FILE_VERSION,
            integrations: vec![StoredIntegration {
                id: CUA_DRIVER_INTEGRATION_ID.to_string(),
                enabled: true,
                backend: AgentIntegrationBackend::External,
                external_server: Some(managed.clone()),
            }],
        };

        // A legacy custom server under the same key is replaced rather than
        // merged: merging would make normalization reject the whole account.
        let effective = effective_mcp_servers(
            &stored,
            vec![http_server("cua-driver"), http_server("Docs")],
        )
        .unwrap();
        assert_eq!(effective.len(), 2);
        assert!(effective.iter().any(|server| server.name == "Docs"));
        assert!(effective.iter().any(|server| {
            server.name == CUA_DRIVER_MCP_NAME
                && matches!(server.transport, AgentMcpTransport::Stdio { .. })
        }));
    }

    #[test]
    fn enabling_and_disabling_preserve_custom_servers_and_the_managed_entry() {
        let temporary = tempfile::tempdir().unwrap();
        let paths = AgentPathLayout::from_app_roots(
            temporary.path().join("config"),
            temporary.path().join("local-data"),
        );
        let user = "cua-toggle@example.com";
        let custom = AgentMcpServer {
            name: "Docs".to_string(),
            description: String::new(),
            enabled: true,
            timeout_seconds: DEFAULT_MCP_TIMEOUT_SECONDS,
            transport: AgentMcpTransport::StreamableHttp {
                url: "https://example.com/mcp".to_string(),
                environment: Vec::new(),
                headers: Vec::new(),
            },
        };
        save_agent_config_inner(
            &paths,
            user,
            &AgentConfig {
                mcp_servers: vec![custom.clone()],
                ..AgentConfig::default()
            },
        )
        .unwrap();
        let managed = stored_cua_server(temporary.path(), false);
        let detection = CuaDetection {
            availability: AgentIntegrationAvailability::Available,
            permissions: None,
            standalone_version: Some("test".to_string()),
            detail: None,
            external_server: Some(managed),
        };

        let enabled = set_integration_default(
            &paths,
            user,
            &AgentSetIntegrationEnabledRequest {
                id: CUA_DRIVER_INTEGRATION_ID.to_string(),
                enabled: true,
            },
            &detection,
        )
        .unwrap();
        assert!(enabled[0].enabled_for_new_tasks);
        let effective = effective_mcp_servers(
            &stored_integrations_for_read(&paths, user),
            load_agent_config_inner(&paths, user).unwrap().mcp_servers,
        )
        .unwrap();
        assert_eq!(effective.len(), 2);
        assert_eq!(effective[0], custom);
        assert!(effective[1].enabled);

        let disabled = set_integration_default(
            &paths,
            user,
            &AgentSetIntegrationEnabledRequest {
                id: CUA_DRIVER_INTEGRATION_ID.to_string(),
                enabled: false,
            },
            &CuaDetection::not_detected(),
        )
        .unwrap();
        assert!(!disabled[0].enabled_for_new_tasks);
        let effective = effective_mcp_servers(
            &stored_integrations_for_read(&paths, user),
            load_agent_config_inner(&paths, user).unwrap().mcp_servers,
        )
        .unwrap();
        assert_eq!(effective.len(), 2);
        assert_eq!(effective[0], custom);
        assert!(!effective[1].enabled);
    }

    #[test]
    fn version_one_registry_migrates_to_the_external_backend_without_switching() {
        let temporary = tempfile::tempdir().unwrap();
        let paths = AgentPathLayout::from_app_roots(
            temporary.path().join("config"),
            temporary.path().join("local-data"),
        );
        let user = "cua-migration@example.com";
        let server = stored_cua_server(temporary.path(), true);
        let path = integrations_path(&paths, user).unwrap();
        write_device_local_json_file(
            &path,
            &json!({
                "version": 1,
                "integrations": [{
                    "id": CUA_DRIVER_INTEGRATION_ID,
                    "server": server,
                }],
            }),
        )
        .unwrap();

        let migrated = load_stored_integrations(&paths, user).unwrap();
        assert_eq!(migrated.version, INTEGRATIONS_FILE_VERSION);
        assert_eq!(migrated.integrations.len(), 1);
        assert!(migrated.integrations[0].enabled);
        assert_eq!(
            migrated.integrations[0].backend,
            AgentIntegrationBackend::External
        );
        assert!(migrated.integrations[0].external_server.is_some());
    }

    #[test]
    fn explicit_setup_switches_only_new_tasks_to_embedded() {
        let temporary = tempfile::tempdir().unwrap();
        let paths = AgentPathLayout::from_app_roots(
            temporary.path().join("config"),
            temporary.path().join("local-data"),
        );
        let user = "cua-embedded@example.com";
        let external = stored_cua_server(temporary.path(), true);
        save_stored_integrations(
            &paths,
            user,
            &StoredIntegrationRegistry {
                version: INTEGRATIONS_FILE_VERSION,
                integrations: vec![StoredIntegration {
                    id: CUA_DRIVER_INTEGRATION_ID.to_string(),
                    enabled: true,
                    backend: AgentIntegrationBackend::External,
                    external_server: Some(external),
                }],
            },
        )
        .unwrap();
        let detection = CuaDetection {
            availability: AgentIntegrationAvailability::Available,
            permissions: Some(AgentIntegrationPermissions::none_required()),
            standalone_version: Some("test".to_string()),
            detail: None,
            external_server: None,
        };

        let projected = select_embedded_integration_backend(&paths, user, &detection).unwrap();
        assert_eq!(
            projected[0].backend,
            Some(AgentIntegrationBackend::Embedded)
        );
        assert!(projected[0].enabled_for_new_tasks);
        let stored = stored_integrations_for_read(&paths, user);
        assert_eq!(
            cua_state_for_new_session(&stored, None, true).unwrap(),
            Some(CuaSessionState {
                backend: AgentIntegrationBackend::Embedded,
                enabled: true,
            })
        );
        assert!(
            cua_state_for_new_session(&stored, None, false)
                .unwrap()
                .is_none()
        );
        assert!(
            cua_state_for_new_session(&stored, Some(&[CUA_DRIVER_MCP_NAME.to_string()]), false)
                .unwrap_err()
                .contains("Maple desktop app")
        );
    }
}
