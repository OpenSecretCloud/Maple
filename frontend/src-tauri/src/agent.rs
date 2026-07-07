use crate::proxy;
use rand::{rngs::OsRng, RngCore};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::Arc;
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use tauri::{AppHandle, Manager, State};
use tokio::sync::Mutex;

const DEFAULT_AGENT_MODEL: &str = "auto:powerful";
const DEFAULT_GOOSE_MODE: &str = "approve";
const READINESS_TIMEOUT: Duration = Duration::from_secs(30);
const READINESS_INTERVAL: Duration = Duration::from_millis(100);

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentConfig {
    pub default_project_root: Option<String>,
    pub default_model: String,
    #[serde(default = "default_runtime_kind")]
    pub runtime_kind: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub external_acp_url: Option<String>,
}

fn default_runtime_kind() -> String {
    "goose".to_string()
}

impl Default for AgentConfig {
    fn default() -> Self {
        Self {
            default_project_root: None,
            default_model: DEFAULT_AGENT_MODEL.to_string(),
            runtime_kind: default_runtime_kind(),
            external_acp_url: None,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentStartRequest {
    pub project_root: Option<String>,
    pub model: Option<String>,
    pub goose_binary: Option<String>,
    pub mode: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentRuntimeStatus {
    pub running: bool,
    pub acp_url: Option<String>,
    pub redacted_acp_url: Option<String>,
    pub http_base_url: Option<String>,
    pub status_url: Option<String>,
    pub project_root: Option<String>,
    pub goose_binary: Option<String>,
    pub pid: Option<u32>,
    pub model: Option<String>,
    pub mode: Option<String>,
    pub maple_proxy_base_url: Option<String>,
    pub config_dir: String,
    pub goose_path_root: Option<String>,
    pub log_path: Option<String>,
    pub error: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RecentProjectRoot {
    pub path: String,
    pub name: String,
    pub last_used_ms: u128,
}

struct AgentRuntime {
    child: Child,
    acp_url: String,
    redacted_acp_url: String,
    http_base_url: String,
    status_url: String,
    project_root: PathBuf,
    goose_binary: PathBuf,
    model: String,
    mode: String,
    maple_proxy_base_url: String,
    config_dir: PathBuf,
    goose_path_root: PathBuf,
    log_path: PathBuf,
}

impl AgentRuntime {
    fn status(&self, running: bool, error: Option<String>) -> AgentRuntimeStatus {
        AgentRuntimeStatus {
            running,
            acp_url: if running {
                Some(self.acp_url.clone())
            } else {
                None
            },
            redacted_acp_url: Some(self.redacted_acp_url.clone()),
            http_base_url: Some(self.http_base_url.clone()),
            status_url: Some(self.status_url.clone()),
            project_root: Some(path_string(&self.project_root)),
            goose_binary: Some(path_string(&self.goose_binary)),
            pid: Some(self.child.id()),
            model: Some(self.model.clone()),
            mode: Some(self.mode.clone()),
            maple_proxy_base_url: Some(self.maple_proxy_base_url.clone()),
            config_dir: path_string(&self.config_dir),
            goose_path_root: Some(path_string(&self.goose_path_root)),
            log_path: Some(path_string(&self.log_path)),
            error,
        }
    }
}

pub struct AgentRuntimeState {
    inner: Arc<Mutex<Option<AgentRuntime>>>,
    session_log: Arc<Mutex<()>>,
}

impl AgentRuntimeState {
    pub fn new() -> Self {
        Self {
            inner: Arc::new(Mutex::new(None)),
            session_log: Arc::new(Mutex::new(())),
        }
    }
}

#[tauri::command]
pub async fn agent_get_runtime_status(
    app_handle: AppHandle,
    state: State<'_, AgentRuntimeState>,
) -> Result<AgentRuntimeStatus, String> {
    let config_dir = agent_config_dir(&app_handle).map_err(|e| e.to_string())?;
    let mut runtime = state.inner.lock().await;

    if let Some(current) = runtime.as_mut() {
        match current.child.try_wait() {
            Ok(Some(exit_status)) => {
                let status = current.status(
                    false,
                    Some(format!("Goose exited with status {exit_status}")),
                );
                *runtime = None;
                return Ok(status);
            }
            Ok(None) => return Ok(current.status(true, None)),
            Err(e) => {
                let status = current.status(false, Some(format!("Failed to inspect Goose: {e}")));
                *runtime = None;
                return Ok(status);
            }
        }
    }

    Ok(stopped_status(config_dir, None))
}

#[tauri::command]
pub async fn agent_start_runtime(
    app_handle: AppHandle,
    state: State<'_, AgentRuntimeState>,
    proxy_state: State<'_, proxy::ProxyState>,
    request: Option<AgentStartRequest>,
) -> Result<AgentRuntimeStatus, String> {
    clear_exited_runtime(&state).await;

    {
        let runtime = state.inner.lock().await;
        if let Some(current) = runtime.as_ref() {
            return Ok(current.status(true, None));
        }
    }

    let agent_config = load_agent_config_inner(&app_handle).unwrap_or_default();
    let request = request.unwrap_or(AgentStartRequest {
        project_root: None,
        model: None,
        goose_binary: None,
        mode: None,
    });

    let proxy_status = proxy::ensure_proxy_running(app_handle.clone(), proxy_state).await?;
    let proxy_config = proxy_status.config;
    let proxy_host = if proxy_config.host == "0.0.0.0" {
        "127.0.0.1".to_string()
    } else {
        proxy_config.host.clone()
    };
    let maple_proxy_base_url = format!("http://{}:{}", proxy_host, proxy_config.port);

    let project_root =
        resolve_project_root(request.project_root.as_deref(), &agent_config).map_err(|e| {
            format!("Failed to resolve Agent Mode project root: {e}")
        })?;
    let model = request
        .model
        .or_else(|| std::env::var("MAPLE_GOOSE_MODEL").ok())
        .unwrap_or(agent_config.default_model);
    let mode = request
        .mode
        .or_else(|| std::env::var("MAPLE_GOOSE_MODE").ok())
        .unwrap_or_else(|| DEFAULT_GOOSE_MODE.to_string());
    let goose_binary =
        resolve_goose_binary(&app_handle, request.goose_binary.as_deref()).map_err(|e| {
            format!("Failed to resolve Goose binary: {e}")
        })?;

    let config_dir = agent_config_dir(&app_handle).map_err(|e| e.to_string())?;
    let goose_path_root = std::env::var("MAPLE_GOOSE_PATH_ROOT")
        .map(PathBuf::from)
        .unwrap_or_else(|_| config_dir.join("goose"));
    let log_dir = config_dir.join("logs");
    fs::create_dir_all(&log_dir).map_err(|e| format!("Failed to create log dir: {e}"))?;
    fs::create_dir_all(&goose_path_root)
        .map_err(|e| format!("Failed to create Goose data dir: {e}"))?;
    let log_path = log_dir.join("goose-serve.log");

    let port = find_available_port().map_err(|e| format!("Failed to allocate Goose port: {e}"))?;
    let token = secure_token();
    let http_base_url = format!("http://127.0.0.1:{port}");
    let status_url = format!("{http_base_url}/status");
    let acp_url = format!("ws://127.0.0.1:{port}/acp?token={token}");
    let redacted_acp_url = format!("ws://127.0.0.1:{port}/acp?token=REDACTED");

    let log_file = OpenOptions::new()
        .create(true)
        .append(true)
        .open(&log_path)
        .map_err(|e| format!("Failed to open Goose log: {e}"))?;
    set_owner_only_permissions(&log_path);
    let stdout_file = log_file
        .try_clone()
        .map_err(|e| format!("Failed to clone Goose log handle: {e}"))?;

    let mut command = Command::new(&goose_binary);
    let port_arg = port.to_string();
    command
        .args([
            "serve",
            "--host",
            "127.0.0.1",
            "--port",
            &port_arg,
        ])
        .current_dir(&project_root)
        .env("GOOSE_SERVER__SECRET_KEY", &token)
        .env("GOOSE_PROVIDER", "openai")
        .env("GOOSE_MODEL", &model)
        .env("GOOSE_FAST_MODEL", &model)
        .env("GOOSE_MODE", &mode)
        .env("GOOSE_DISABLE_KEYRING", "true")
        .env("GOOSE_PATH_ROOT", &goose_path_root)
        .env("OPENAI_BASE_URL", format!("{}/v1", maple_proxy_base_url))
        .env("OPENAI_API_KEY", proxy_config.api_key)
        .stdout(Stdio::from(stdout_file))
        .stderr(Stdio::from(log_file));

    if let Some(parent) = goose_binary.parent() {
        prepend_path_env(&mut command, parent);
    }

    log::info!(
        "Starting Goose ACP runtime from {} in {} on {}",
        goose_binary.display(),
        project_root.display(),
        redacted_acp_url
    );

    let mut child = command
        .spawn()
        .map_err(|e| format!("Failed to spawn Goose: {e}"))?;

    if let Err(e) = wait_for_goose_ready(&status_url, &mut child).await {
        let _ = child.kill();
        let _ = child.wait();
        return Err(format!("{e}. See {}", log_path.display()));
    }

    let runtime = AgentRuntime {
        child,
        acp_url,
        redacted_acp_url,
        http_base_url,
        status_url,
        project_root: project_root.clone(),
        goose_binary,
        model: model.clone(),
        mode,
        maple_proxy_base_url,
        config_dir: config_dir.clone(),
        goose_path_root,
        log_path,
    };

    let status = runtime.status(true, None);
    {
        let mut guard = state.inner.lock().await;
        *guard = Some(runtime);
    }

    let _ = save_recent_project_root_inner(&app_handle, &project_root);
    let mut next_config = load_agent_config_inner(&app_handle).unwrap_or_default();
    next_config.default_project_root = Some(path_string(&project_root));
    next_config.default_model = model;
    let _ = save_agent_config_inner(&app_handle, &next_config);

    Ok(status)
}

#[tauri::command]
pub async fn agent_stop_runtime(
    app_handle: AppHandle,
    state: State<'_, AgentRuntimeState>,
) -> Result<AgentRuntimeStatus, String> {
    let config_dir = agent_config_dir(&app_handle).map_err(|e| e.to_string())?;
    stop_runtime_inner(&state).await?;
    Ok(stopped_status(config_dir, None))
}

#[tauri::command]
pub async fn agent_restart_runtime(
    app_handle: AppHandle,
    state: State<'_, AgentRuntimeState>,
    proxy_state: State<'_, proxy::ProxyState>,
    request: Option<AgentStartRequest>,
) -> Result<AgentRuntimeStatus, String> {
    stop_runtime_inner(&state).await?;
    agent_start_runtime(app_handle, state, proxy_state, request).await
}

#[tauri::command]
pub async fn agent_load_config(app_handle: AppHandle) -> Result<AgentConfig, String> {
    load_agent_config_inner(&app_handle).map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn agent_save_config(app_handle: AppHandle, config: AgentConfig) -> Result<(), String> {
    save_agent_config_inner(&app_handle, &config).map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn agent_list_recent_project_roots(
    app_handle: AppHandle,
) -> Result<Vec<RecentProjectRoot>, String> {
    load_recent_project_roots_inner(&app_handle).map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn agent_save_recent_project_root(
    app_handle: AppHandle,
    path: String,
) -> Result<Vec<RecentProjectRoot>, String> {
    let project_root = normalize_project_root(Path::new(&path))?;
    save_recent_project_root_inner(&app_handle, &project_root).map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn agent_append_session_event(
    app_handle: AppHandle,
    state: State<'_, AgentRuntimeState>,
    session_id: String,
    event: Value,
) -> Result<(), String> {
    let _guard = state.session_log.lock().await;
    append_session_event_inner(&app_handle, &session_id, event).map_err(|e| e.to_string())
}

async fn clear_exited_runtime(state: &State<'_, AgentRuntimeState>) {
    let mut runtime = state.inner.lock().await;
    if let Some(current) = runtime.as_mut() {
        if matches!(current.child.try_wait(), Ok(Some(_)) | Err(_)) {
            *runtime = None;
        }
    }
}

async fn stop_runtime_inner(state: &State<'_, AgentRuntimeState>) -> Result<(), String> {
    let mut runtime = state.inner.lock().await;
    if let Some(mut current) = runtime.take() {
        log::info!("Stopping Goose ACP runtime");
        if let Err(e) = current.child.kill() {
            log::warn!("Failed to kill Goose runtime: {e}");
        }
        if let Err(e) = current.child.wait() {
            log::warn!("Failed to wait for Goose runtime exit: {e}");
        }
    }
    Ok(())
}

async fn wait_for_goose_ready(status_url: &str, child: &mut Child) -> Result<(), String> {
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(1))
        .build()
        .map_err(|e| format!("Failed to build readiness client: {e}"))?;
    let deadline = std::time::Instant::now() + READINESS_TIMEOUT;

    while std::time::Instant::now() < deadline {
        match child.try_wait() {
            Ok(Some(exit_status)) => {
                return Err(format!("Goose exited before becoming ready: {exit_status}"));
            }
            Ok(None) => {}
            Err(e) => return Err(format!("Failed to inspect Goose readiness: {e}")),
        }

        if let Ok(response) = client.get(status_url).send().await {
            if response.status().is_success() {
                return Ok(());
            }
        }

        tokio::time::sleep(READINESS_INTERVAL).await;
    }

    Err(format!("Goose did not become ready on {status_url}"))
}

fn stopped_status(config_dir: PathBuf, error: Option<String>) -> AgentRuntimeStatus {
    AgentRuntimeStatus {
        running: false,
        acp_url: None,
        redacted_acp_url: None,
        http_base_url: None,
        status_url: None,
        project_root: None,
        goose_binary: None,
        pid: None,
        model: None,
        mode: None,
        maple_proxy_base_url: None,
        config_dir: path_string(&config_dir),
        goose_path_root: None,
        log_path: None,
        error,
    }
}

fn resolve_project_root(
    requested: Option<&str>,
    config: &AgentConfig,
) -> Result<PathBuf, String> {
    if let Some(path) = requested.filter(|value| !value.trim().is_empty()) {
        return normalize_project_root(Path::new(path));
    }

    if let Some(path) = config
        .default_project_root
        .as_deref()
        .filter(|value| !value.trim().is_empty())
    {
        if let Ok(root) = normalize_project_root(Path::new(path)) {
            return Ok(root);
        }
    }

    std::env::current_dir()
        .map_err(|e| format!("Failed to read current directory: {e}"))
        .and_then(|path| normalize_project_root(&path))
}

fn normalize_project_root(path: &Path) -> Result<PathBuf, String> {
    let canonical = path
        .canonicalize()
        .map_err(|e| format!("{}: {e}", path.display()))?;
    if !canonical.is_dir() {
        return Err(format!("{} is not a folder", canonical.display()));
    }
    Ok(canonical)
}

fn resolve_goose_binary(app_handle: &AppHandle, requested: Option<&str>) -> Result<PathBuf, String> {
    let mut candidates = Vec::new();

    if let Some(path) = requested.filter(|value| !value.trim().is_empty()) {
        candidates.push(PathBuf::from(path));
    }

    if let Ok(path) = std::env::var("MAPLE_GOOSE_BINARY") {
        candidates.push(PathBuf::from(path));
    }

    if let Ok(path) = std::env::var("GOOSE_BINARY") {
        candidates.push(PathBuf::from(path));
    }

    if let Ok(resource_dir) = app_handle.path().resource_dir() {
        candidates.push(resource_dir.join("bin").join(goose_binary_name()));
        candidates.push(resource_dir.join(goose_binary_name()));
    }

    let manifest_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    candidates.push(
        manifest_dir
            .join("..")
            .join("node_modules")
            .join("@aaif")
            .join(goose_binary_package_name())
            .join("bin")
            .join(goose_binary_name()),
    );
    candidates.push(
        manifest_dir
            .join("bin")
            .join(goose_binary_name()),
    );

    if cfg!(debug_assertions) {
        if let Some(path) = find_on_path(goose_binary_name()) {
            candidates.push(path);
        }
    }

    for candidate in candidates {
        let candidate = expand_home(candidate);
        if candidate.is_file() {
            return Ok(candidate);
        }
    }

    Err("Goose binary not found. Set MAPLE_GOOSE_BINARY in development or bundle bin/goose with Maple.".to_string())
}

fn goose_binary_name() -> &'static str {
    if cfg!(target_os = "windows") {
        "goose.exe"
    } else {
        "goose"
    }
}

fn goose_binary_package_name() -> &'static str {
    if cfg!(all(target_os = "macos", target_arch = "aarch64")) {
        "goose-binary-darwin-arm64"
    } else if cfg!(all(target_os = "macos", target_arch = "x86_64")) {
        "goose-binary-darwin-x64"
    } else if cfg!(all(target_os = "linux", target_arch = "aarch64")) {
        "goose-binary-linux-arm64"
    } else if cfg!(all(target_os = "linux", target_arch = "x86_64")) {
        "goose-binary-linux-x64"
    } else if cfg!(all(target_os = "windows", target_arch = "x86_64")) {
        "goose-binary-win32-x64"
    } else {
        "goose-binary-unsupported"
    }
}

fn find_available_port() -> std::io::Result<u16> {
    let listener = std::net::TcpListener::bind(("127.0.0.1", 0))?;
    Ok(listener.local_addr()?.port())
}

fn secure_token() -> String {
    let mut bytes = [0u8; 32];
    OsRng.fill_bytes(&mut bytes);
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}

fn prepend_path_env(command: &mut Command, dir: &Path) {
    let key = if cfg!(target_os = "windows") {
        "Path"
    } else {
        "PATH"
    };
    let current = std::env::var_os(key).unwrap_or_default();
    let mut paths = vec![dir.to_path_buf()];
    paths.extend(std::env::split_paths(&current));
    if let Ok(joined) = std::env::join_paths(paths) {
        command.env(key, joined);
    }
}

fn find_on_path(binary: &str) -> Option<PathBuf> {
    let path_var = std::env::var_os(if cfg!(target_os = "windows") {
        "Path"
    } else {
        "PATH"
    })?;
    std::env::split_paths(&path_var)
        .map(|dir| dir.join(binary))
        .find(|candidate| candidate.is_file())
}

fn expand_home(path: PathBuf) -> PathBuf {
    let path_string = path.to_string_lossy();
    if path_string == "~" {
        return home_dir().unwrap_or(path);
    }
    if let Some(rest) = path_string.strip_prefix("~/") {
        if let Some(home) = home_dir() {
            return home.join(rest);
        }
    }
    path
}

fn home_dir() -> Option<PathBuf> {
    std::env::var_os("HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("USERPROFILE").map(PathBuf::from))
}

fn agent_config_dir(app_handle: &AppHandle) -> Result<PathBuf, anyhow::Error> {
    let base = if cfg!(target_os = "windows") {
        app_handle
            .path()
            .app_config_dir()
            .map_err(|e| anyhow::anyhow!("Failed to resolve app config dir: {e}"))?
    } else {
        let home = std::env::var("HOME")
            .map_err(|_| anyhow::anyhow!("Failed to get home directory"))?;
        PathBuf::from(home).join(".config").join("maple")
    };
    let path = base.join("agent");
    fs::create_dir_all(&path)?;
    Ok(path)
}

fn load_agent_config_inner(app_handle: &AppHandle) -> Result<AgentConfig, anyhow::Error> {
    let path = agent_config_dir(app_handle)?.join("config.json");
    if !path.exists() {
        return Ok(AgentConfig::default());
    }
    let contents = fs::read_to_string(path)?;
    Ok(serde_json::from_str(&contents)?)
}

fn save_agent_config_inner(
    app_handle: &AppHandle,
    config: &AgentConfig,
) -> Result<(), anyhow::Error> {
    let path = agent_config_dir(app_handle)?.join("config.json");
    write_json_file(&path, config)
}

fn load_recent_project_roots_inner(
    app_handle: &AppHandle,
) -> Result<Vec<RecentProjectRoot>, anyhow::Error> {
    let path = agent_config_dir(app_handle)?.join("recent_roots.json");
    if !path.exists() {
        return Ok(Vec::new());
    }
    let contents = fs::read_to_string(path)?;
    Ok(serde_json::from_str(&contents)?)
}

fn save_recent_project_root_inner(
    app_handle: &AppHandle,
    project_root: &Path,
) -> Result<Vec<RecentProjectRoot>, anyhow::Error> {
    let mut roots = load_recent_project_roots_inner(app_handle).unwrap_or_default();
    let path = path_string(project_root);
    roots.retain(|root| root.path != path);
    roots.insert(
        0,
        RecentProjectRoot {
            name: project_root
                .file_name()
                .and_then(|value| value.to_str())
                .unwrap_or(&path)
                .to_string(),
            path,
            last_used_ms: unix_ms(),
        },
    );
    roots.truncate(20);

    let file_path = agent_config_dir(app_handle)?.join("recent_roots.json");
    write_json_file(&file_path, &roots)?;
    Ok(roots)
}

fn append_session_event_inner(
    app_handle: &AppHandle,
    session_id: &str,
    event: Value,
) -> Result<(), anyhow::Error> {
    let sessions_dir = agent_config_dir(app_handle)?.join("sessions");
    fs::create_dir_all(&sessions_dir)?;
    let path = sessions_dir.join(format!("{}.jsonl", sanitize_file_id(session_id)));
    let mut file = OpenOptions::new().create(true).append(true).open(&path)?;
    set_owner_only_permissions(&path);
    let row = serde_json::json!({
        "ts": unix_ms(),
        "event": event,
    });
    serde_json::to_writer(&mut file, &row)?;
    file.write_all(b"\n")?;
    Ok(())
}

fn write_json_file<T: Serialize>(path: &Path, value: &T) -> Result<(), anyhow::Error> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::write(path, serde_json::to_string_pretty(value)?)?;
    set_owner_only_permissions(path);
    Ok(())
}

#[cfg(unix)]
fn set_owner_only_permissions(path: &Path) {
    use std::os::unix::fs::PermissionsExt;
    let _ = fs::set_permissions(path, fs::Permissions::from_mode(0o600));
}

#[cfg(not(unix))]
fn set_owner_only_permissions(_path: &Path) {}

fn sanitize_file_id(value: &str) -> String {
    let sanitized: String = value
        .chars()
        .map(|ch| {
            if ch.is_ascii_alphanumeric() || ch == '-' || ch == '_' {
                ch
            } else {
                '_'
            }
        })
        .collect();
    if sanitized.is_empty() {
        "session".to_string()
    } else {
        sanitized
    }
}

fn unix_ms() -> u128 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_millis())
        .unwrap_or_default()
}

fn path_string(path: &Path) -> String {
    path.to_string_lossy().to_string()
}
