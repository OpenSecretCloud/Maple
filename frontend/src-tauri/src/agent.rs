use crate::proxy;
use axum::http::HeaderValue;
use goose::acp::server_factory::{AcpServer, AcpServerFactoryConfig};
use goose::acp::transport::create_router as create_goose_acp_router;
use goose::agents::GoosePlatform;
use goose::config::ConfigError;
use rand::{rngs::OsRng, RngCore};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use tauri::{AppHandle, Manager, State};
use tokio::sync::{oneshot, Mutex};
use tokio::task::JoinHandle;

const DEFAULT_AGENT_MODEL: &str = "auto:powerful";
const DEFAULT_GOOSE_MODE: &str = "approve";
const READINESS_TIMEOUT: Duration = Duration::from_secs(30);
const READINESS_INTERVAL: Duration = Duration::from_millis(100);
const SHUTDOWN_TIMEOUT: Duration = Duration::from_secs(5);

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
    // Kept so older frontend state or dev tools can pass the old field without
    // breaking deserialization. Built-in Goose now runs in-process.
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
    pub llm_log_dir: Option<String>,
    pub latest_llm_log_path: Option<String>,
    pub proxy_llm_log_dir: Option<String>,
    pub latest_proxy_llm_log_path: Option<String>,
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
    shutdown: Option<oneshot::Sender<()>>,
    server_task: JoinHandle<Result<(), String>>,
    acp_url: String,
    redacted_acp_url: String,
    http_base_url: String,
    status_url: String,
    project_root: PathBuf,
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
            goose_binary: None,
            pid: None,
            model: Some(self.model.clone()),
            mode: Some(self.mode.clone()),
            maple_proxy_base_url: Some(self.maple_proxy_base_url.clone()),
            config_dir: path_string(&self.config_dir),
            goose_path_root: Some(path_string(&self.goose_path_root)),
            log_path: Some(path_string(&self.log_path)),
            llm_log_dir: Some(path_string(&goose_llm_log_dir(&self.goose_path_root))),
            latest_llm_log_path: Some(path_string(&latest_goose_llm_log_path(
                &self.goose_path_root,
            ))),
            proxy_llm_log_dir: Some(path_string(&proxy_llm_log_dir(&self.config_dir))),
            latest_proxy_llm_log_path: latest_proxy_llm_log_path(&self.config_dir),
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
        if !current.server_task.is_finished() {
            return Ok(current.status(true, None));
        }
    }

    if let Some(current) = runtime.take() {
        drop(runtime);
        let error = match current.server_task.await {
            Ok(Ok(())) => Some("Goose ACP server stopped".to_string()),
            Ok(Err(error)) => Some(error),
            Err(error) => Some(format!("Goose ACP server task failed: {error}")),
        };
        return Ok(stopped_status(config_dir, error));
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

    log::info!("Agent Mode requested embedded Goose runtime start");
    let proxy_status = proxy::ensure_proxy_running(app_handle.clone(), proxy_state).await?;
    let proxy_config = proxy_status.config;
    let proxy_host = if proxy_config.host == "0.0.0.0" {
        "127.0.0.1".to_string()
    } else {
        proxy_config.host.clone()
    };
    let maple_proxy_base_url = format!("http://{}:{}", proxy_host, proxy_config.port);

    let project_root = resolve_project_root(request.project_root.as_deref(), &agent_config)
        .map_err(|e| format!("Failed to resolve Agent Mode project root: {e}"))?;
    let model = request
        .model
        .or_else(|| std::env::var("MAPLE_GOOSE_MODEL").ok())
        .unwrap_or(agent_config.default_model);
    let mode = request
        .mode
        .or_else(|| std::env::var("MAPLE_GOOSE_MODE").ok())
        .unwrap_or_else(|| DEFAULT_GOOSE_MODE.to_string());

    let config_dir = agent_config_dir(&app_handle).map_err(|e| e.to_string())?;
    let goose_path_root = std::env::var("MAPLE_GOOSE_PATH_ROOT")
        .map(PathBuf::from)
        .unwrap_or_else(|_| config_dir.join("goose"));
    let log_dir = config_dir.join("logs");
    fs::create_dir_all(&log_dir).map_err(|e| format!("Failed to create log dir: {e}"))?;
    fs::create_dir_all(&goose_path_root)
        .map_err(|e| format!("Failed to create Goose data dir: {e}"))?;
    let log_path = log_dir.join("goose-embedded.log");
    let llm_log_dir = goose_llm_log_dir(&goose_path_root);
    let latest_llm_log_path = latest_goose_llm_log_path(&goose_path_root);
    let proxy_llm_log_dir = proxy_llm_log_dir(&config_dir);

    let port = find_available_port().map_err(|e| format!("Failed to allocate Goose port: {e}"))?;
    let token = secure_token();
    let http_base_url = format!("http://127.0.0.1:{port}");
    let status_url = format!("{http_base_url}/status");
    let acp_url = format!("ws://127.0.0.1:{port}/acp?token={token}");
    let redacted_acp_url = format!("ws://127.0.0.1:{port}/acp?token=REDACTED");
    let allowed_origins = tauri_acp_allowed_origins();
    let allowed_origin_log = allowed_origins
        .iter()
        .filter_map(|origin| origin.to_str().ok())
        .collect::<Vec<_>>()
        .join(", ");

    configure_embedded_goose(
        &goose_path_root,
        &model,
        &mode,
        &maple_proxy_base_url,
        &proxy_config.api_key,
    )?;

    let listener = tokio::net::TcpListener::bind(("127.0.0.1", port))
        .await
        .map_err(|e| format!("Failed to bind Goose ACP listener: {e}"))?;
    let acp_server = Arc::new(AcpServer::new(AcpServerFactoryConfig {
        builtins: vec!["developer".to_string()],
        data_dir: goose_path_root.join("data"),
        config_dir: goose_path_root.join("config"),
        goose_platform: GoosePlatform::GooseDesktop,
        additional_source_roots: Vec::new(),
        scheduler: None,
    }));
    let router = create_goose_acp_router(acp_server, token, true, allowed_origins);
    let (shutdown_tx, shutdown_rx) = oneshot::channel::<()>();

    log::info!(
        "Starting embedded Goose ACP runtime in {} on {} with allowed origins: {}",
        project_root.display(),
        redacted_acp_url,
        allowed_origin_log
    );
    append_runtime_log(
        &log_path,
        &format!(
            "starting embedded Goose ACP runtime: project_root={}, acp={}, proxy=http://{}:{}, llm_log_dir={}, latest_llm_log={}, proxy_llm_log_dir={}, allowed_origins={}",
            project_root.display(),
            redacted_acp_url,
            proxy_host,
            proxy_config.port,
            llm_log_dir.display(),
            latest_llm_log_path.display(),
            proxy_llm_log_dir.display(),
            allowed_origin_log
        ),
    );

    let server_task = tokio::spawn(async move {
        axum::serve(listener, router)
            .with_graceful_shutdown(async {
                let _ = shutdown_rx.await;
            })
            .await
            .map_err(|e| format!("Goose ACP server stopped with error: {e}"))
    });

    if let Err(e) = wait_for_goose_ready(&status_url).await {
        append_runtime_log(
            &log_path,
            &format!("embedded Goose ACP runtime readiness failed: {e}"),
        );
        let _ = shutdown_tx.send(());
        server_task.abort();
        return Err(format!("{e}. See {}", log_path.display()));
    }

    append_runtime_log(
        &log_path,
        &format!("embedded Goose ACP runtime ready: status={status_url}, acp={redacted_acp_url}"),
    );
    log::info!("Embedded Goose ACP runtime ready on {redacted_acp_url}");

    let runtime = AgentRuntime {
        shutdown: Some(shutdown_tx),
        server_task,
        acp_url,
        redacted_acp_url,
        http_base_url,
        status_url,
        project_root: project_root.clone(),
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

#[tauri::command]
pub async fn agent_append_runtime_log(
    app_handle: AppHandle,
    message: String,
) -> Result<(), String> {
    let log_path = agent_config_dir(&app_handle)
        .map_err(|e| e.to_string())?
        .join("logs")
        .join("goose-embedded.log");
    log::info!("[Agent Mode] {message}");
    append_runtime_log(&log_path, &format!("frontend {}", message));
    Ok(())
}

async fn clear_exited_runtime(state: &State<'_, AgentRuntimeState>) {
    let mut runtime = state.inner.lock().await;
    if runtime
        .as_ref()
        .map(|current| current.server_task.is_finished())
        .unwrap_or(false)
    {
        *runtime = None;
    }
}

async fn stop_runtime_inner(state: &State<'_, AgentRuntimeState>) -> Result<(), String> {
    let mut runtime = state.inner.lock().await;
    if let Some(mut current) = runtime.take() {
        log::info!("Stopping embedded Goose ACP runtime");
        append_runtime_log(&current.log_path, "stopping embedded Goose ACP runtime");
        if let Some(shutdown) = current.shutdown.take() {
            let _ = shutdown.send(());
        }
        let mut server_task = current.server_task;
        tokio::select! {
            result = &mut server_task => {
                match result {
                    Ok(Ok(())) => {}
                    Ok(Err(error)) => log::warn!("{error}"),
                    Err(error) => log::warn!("Goose ACP server task failed: {error}"),
                }
            }
            _ = tokio::time::sleep(SHUTDOWN_TIMEOUT) => {
                server_task.abort();
                let _ = server_task.await;
                log::warn!("Timed out stopping Goose ACP server; aborted task");
            }
        }
    }
    Ok(())
}

async fn wait_for_goose_ready(status_url: &str) -> Result<(), String> {
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(1))
        .build()
        .map_err(|e| format!("Failed to build readiness client: {e}"))?;
    let deadline = std::time::Instant::now() + READINESS_TIMEOUT;

    while std::time::Instant::now() < deadline {
        if let Ok(response) = client.get(status_url).send().await {
            if response.status().is_success() {
                return Ok(());
            }
        }

        tokio::time::sleep(READINESS_INTERVAL).await;
    }

    Err(format!("Goose did not become ready on {status_url}"))
}

fn configure_embedded_goose(
    goose_path_root: &Path,
    model: &str,
    mode: &str,
    maple_proxy_base_url: &str,
    proxy_api_key: &str,
) -> Result<(), String> {
    fs::create_dir_all(goose_path_root.join("config"))
        .map_err(|e| format!("Failed to create Goose config dir: {e}"))?;
    fs::create_dir_all(goose_path_root.join("data"))
        .map_err(|e| format!("Failed to create Goose data dir: {e}"))?;
    fs::create_dir_all(goose_path_root.join("state"))
        .map_err(|e| format!("Failed to create Goose state dir: {e}"))?;

    std::env::set_var("GOOSE_PATH_ROOT", goose_path_root);
    std::env::set_var("GOOSE_DISABLE_KEYRING", "true");
    std::env::remove_var("GOOSE_MAX_TOKENS");

    let config = goose::config::Config::global();
    delete_goose_config_key(config, "GOOSE_MAX_TOKENS")?;
    goose::config::set_active_provider(config, "openai", model)
        .map_err(|e| format!("Failed to configure Goose provider: {e}"))?;
    config
        .set_param("GOOSE_FAST_MODEL", model)
        .map_err(|e| format!("Failed to configure Goose fast model: {e}"))?;
    config
        .set_param("GOOSE_MODE", mode)
        .map_err(|e| format!("Failed to configure Goose mode: {e}"))?;
    config
        .set_param("OPENAI_BASE_URL", format!("{maple_proxy_base_url}/v1"))
        .map_err(|e| format!("Failed to configure Goose OpenAI base URL: {e}"))?;
    config
        .set_param("GOOSE_DISABLE_KEYRING", true)
        .map_err(|e| format!("Failed to configure Goose keyring mode: {e}"))?;
    config
        .set_secret("OPENAI_API_KEY", &proxy_api_key)
        .map_err(|e| format!("Failed to configure Goose OpenAI API key: {e}"))?;
    goose::providers::utils::init_goose_request_log()
        .map_err(|e| format!("Failed to initialize Goose LLM request logging: {e}"))?;

    set_owner_only_permissions(&goose_path_root.join("config").join("config.yaml"));
    set_owner_only_permissions(&goose_path_root.join("config").join("secrets.yaml"));
    Ok(())
}

fn delete_goose_config_key(config: &goose::config::Config, key: &str) -> Result<(), String> {
    match config.delete(key) {
        Ok(()) | Err(ConfigError::NotFound(_)) => Ok(()),
        Err(e) => Err(format!("Failed to clear Goose config key {key}: {e}")),
    }
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
        llm_log_dir: None,
        latest_llm_log_path: None,
        proxy_llm_log_dir: Some(path_string(&proxy_llm_log_dir(&config_dir))),
        latest_proxy_llm_log_path: latest_proxy_llm_log_path(&config_dir),
        error,
    }
}

fn goose_llm_log_dir(goose_path_root: &Path) -> PathBuf {
    goose_path_root.join("state").join("logs")
}

fn latest_goose_llm_log_path(goose_path_root: &Path) -> PathBuf {
    goose_llm_log_dir(goose_path_root).join("llm_request.0.jsonl")
}

fn proxy_llm_log_dir(agent_config_dir: &Path) -> PathBuf {
    agent_config_dir.join("proxy-llm-logs")
}

fn latest_proxy_llm_log_path(agent_config_dir: &Path) -> Option<String> {
    let dir = proxy_llm_log_dir(agent_config_dir);
    let entries = fs::read_dir(dir).ok()?;
    entries
        .filter_map(|entry| {
            let entry = entry.ok()?;
            let path = entry.path();
            if path.extension().and_then(|value| value.to_str()) != Some("jsonl") {
                return None;
            }
            let modified = entry.metadata().ok()?.modified().ok()?;
            Some((modified, path))
        })
        .max_by_key(|(modified, _)| *modified)
        .map(|(_, path)| path_string(&path))
}

fn resolve_project_root(requested: Option<&str>, config: &AgentConfig) -> Result<PathBuf, String> {
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

fn find_available_port() -> std::io::Result<u16> {
    let listener = std::net::TcpListener::bind(("127.0.0.1", 0))?;
    Ok(listener.local_addr()?.port())
}

fn secure_token() -> String {
    let mut bytes = [0u8; 32];
    OsRng.fill_bytes(&mut bytes);
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}

fn tauri_acp_allowed_origins() -> Vec<HeaderValue> {
    let mut origins = vec![
        HeaderValue::from_static("http://localhost:5173"),
        HeaderValue::from_static("http://127.0.0.1:5173"),
        HeaderValue::from_static("http://tauri.localhost"),
        HeaderValue::from_static("https://tauri.localhost"),
        HeaderValue::from_static("tauri://localhost"),
        HeaderValue::from_static("null"),
        HeaderValue::from_static("file://"),
    ];

    if let Ok(extra_origins) = std::env::var("MAPLE_GOOSE_ACP_ALLOWED_ORIGINS") {
        for origin in extra_origins
            .split(',')
            .map(str::trim)
            .filter(|s| !s.is_empty())
        {
            match HeaderValue::from_str(origin) {
                Ok(header) if !origins.contains(&header) => origins.push(header),
                Ok(_) => {}
                Err(error) => {
                    log::warn!("Ignoring invalid MAPLE_GOOSE_ACP_ALLOWED_ORIGINS entry {origin:?}: {error}");
                }
            }
        }
    }

    origins
}

fn agent_config_dir(app_handle: &AppHandle) -> Result<PathBuf, anyhow::Error> {
    let base = if cfg!(target_os = "windows") {
        app_handle
            .path()
            .app_config_dir()
            .map_err(|e| anyhow::anyhow!("Failed to resolve app config dir: {e}"))?
    } else {
        let home =
            std::env::var("HOME").map_err(|_| anyhow::anyhow!("Failed to get home directory"))?;
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

fn append_runtime_log(path: &Path, message: &str) {
    let result = (|| -> Result<(), anyhow::Error> {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        let mut file = OpenOptions::new().create(true).append(true).open(path)?;
        set_owner_only_permissions(path);
        writeln!(file, "{} {}", unix_ms(), message)?;
        Ok(())
    })();
    if let Err(error) = result {
        log::warn!("Failed to write Goose runtime log: {error}");
    }
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
