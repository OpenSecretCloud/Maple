use anyhow::{anyhow, Result};
use axum::{
    body::{to_bytes, Body, Bytes},
    extract::{Request, State as AxumState},
    http::{Method, StatusCode},
    middleware::{self, Next},
    response::{IntoResponse, Response},
    Router,
};
use futures::{Stream, StreamExt};
use maple_proxy::{create_app, Config};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::fs::{self, File, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::pin::Pin;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex as StdMutex};
use std::time::{SystemTime, UNIX_EPOCH};
use tauri::{AppHandle, Emitter, Manager, State};
use tokio::net::TcpListener;
use tokio::sync::Mutex;

static PROXY_LLM_LOG_COUNTER: AtomicU64 = AtomicU64::new(0);

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProxyConfig {
    pub host: String,
    pub port: u16,
    pub api_key: String,
    pub enabled: bool,
    #[serde(default = "default_cors")]
    pub enable_cors: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub backend_url: Option<String>,
    #[serde(default)]
    pub auto_start: bool,
}

fn default_cors() -> bool {
    true
}

impl Default for ProxyConfig {
    fn default() -> Self {
        Self {
            host: "127.0.0.1".to_string(),
            port: 8080,
            api_key: String::new(),
            enabled: false,
            enable_cors: true,
            backend_url: None,
            auto_start: false,
        }
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct ProxyStatus {
    pub running: bool,
    pub config: ProxyConfig,
    pub error: Option<String>,
}

pub struct ProxyState {
    handle: Arc<Mutex<Option<tokio::task::JoinHandle<()>>>>,
    config: Arc<Mutex<ProxyConfig>>,
    running: Arc<Mutex<bool>>,
}

#[derive(Clone)]
struct LoggedProxyState {
    llm_log_dir: PathBuf,
}

#[derive(Clone)]
struct ProxyLlmLog {
    request_id: String,
    path: PathBuf,
    file: Arc<StdMutex<File>>,
}

impl ProxyState {
    pub fn new() -> Self {
        Self {
            handle: Arc::new(Mutex::new(None)),
            config: Arc::new(Mutex::new(ProxyConfig::default())),
            running: Arc::new(Mutex::new(false)),
        }
    }

    pub async fn status(&self) -> ProxyStatus {
        ProxyStatus {
            running: *self.running.lock().await,
            config: self.config.lock().await.clone(),
            error: None,
        }
    }
}

// On Windows the proxy config lives in the roaming %APPDATA% profile, so a
// plaintext api_key could sync across machines in a domain/AAD environment.
// Store it in Windows Credential Manager instead and keep it out of the JSON.
// macOS/Linux keep their local plaintext-with-0o600 behavior unchanged.
#[cfg(target_os = "windows")]
const KEYRING_SERVICE: &str = "cloud.opensecret.maple";
#[cfg(target_os = "windows")]
const KEYRING_USER: &str = "proxy_api_key";

/// Persist the API key in Windows Credential Manager. An empty key clears the
/// entry. Returns `Ok(true)` when the key was stored (or cleared), `Ok(false)`
/// when no secure storage is available (caller keeps the plaintext fallback),
/// and `Err` when a clear was requested but the stale credential could not be
/// removed (caller must not scrub the JSON, or the old key would be resurrected).
#[cfg(target_os = "windows")]
fn store_api_key(key: &str) -> Result<bool> {
    let entry = match keyring::Entry::new(KEYRING_SERVICE, KEYRING_USER) {
        Ok(entry) => entry,
        Err(e) => {
            log::warn!("Credential Manager unavailable, keeping plaintext config: {e}");
            return Ok(false);
        }
    };

    if key.is_empty() {
        return match entry.delete_credential() {
            Ok(()) | Err(keyring::Error::NoEntry) => Ok(true),
            // A hard delete failure leaves the old credential in place. Surface
            // it instead of reporting success: the caller must not scrub the
            // JSON, or the stale key would be resurrected on the next load.
            Err(e) => Err(anyhow!(
                "Failed to clear API key from Credential Manager: {e}"
            )),
        };
    }

    match entry.set_password(key) {
        Ok(()) => Ok(true),
        Err(keyring::Error::PlatformFailure(_)) | Err(keyring::Error::NoStorageAccess(_)) => {
            log::warn!("No secure credential storage available; keeping plaintext config");
            Ok(false)
        }
        Err(e) => Err(anyhow!(
            "Failed to store API key in Credential Manager: {e}"
        )),
    }
}

/// Load the API key from Windows Credential Manager.
/// - `Ok(Some(key))` — Credential Manager is available (`key` may be empty).
/// - `Ok(None)` — unavailable; caller should fall back to the JSON value.
#[cfg(target_os = "windows")]
fn load_api_key() -> Result<Option<String>> {
    let entry = match keyring::Entry::new(KEYRING_SERVICE, KEYRING_USER) {
        Ok(entry) => entry,
        Err(_) => return Ok(None),
    };

    match entry.get_password() {
        Ok(key) => Ok(Some(key)),
        Err(keyring::Error::NoEntry) => Ok(Some(String::new())),
        Err(keyring::Error::PlatformFailure(_)) | Err(keyring::Error::NoStorageAccess(_)) => {
            Ok(None)
        }
        Err(e) => Err(anyhow!(
            "Failed to read API key from Credential Manager: {e}"
        )),
    }
}

#[tauri::command]
pub async fn start_proxy(
    app_handle: AppHandle,
    state: State<'_, ProxyState>,
    config: ProxyConfig,
) -> Result<ProxyStatus, String> {
    log::info!(
        "Starting proxy on {}:{} (cors={}, auto_start={})",
        config.host,
        config.port,
        config.enable_cors,
        config.auto_start
    );

    // Check if proxy is already running
    let mut running = state.running.lock().await;
    if *running {
        return Err("Proxy is already running".to_string());
    }

    // Update config
    let mut stored_config = state.config.lock().await;
    *stored_config = config.clone();
    drop(stored_config); // Release config lock early

    // Use backend URL from config or fall back to production
    let backend_url = config
        .backend_url
        .clone()
        .unwrap_or_else(|| "https://enclave.trymaple.ai".to_string());

    // Create maple-proxy config
    let proxy_config = Config::new(config.host.clone(), config.port, backend_url)
        .with_api_key(config.api_key.clone())
        .with_debug(false)
        .with_cors(config.enable_cors);

    // Try to bind to the address first to check if port is available
    let addr = proxy_config
        .socket_addr()
        .map_err(|e| format!("Invalid address: {e}"))?;

    let listener = match TcpListener::bind(&addr).await {
        Ok(listener) => listener,
        Err(e) => {
            return Err(format!(
                "Failed to bind to {}:{} - {}",
                config.host, config.port, e
            ));
        }
    };

    let llm_log_dir = proxy_llm_log_dir(&app_handle)
        .map_err(|e| format!("Failed to create proxy LLM log dir: {e}"))?;
    log::info!("Maple proxy LLM logs enabled at {}", llm_log_dir.display());

    // Create the app. This mirrors maple-proxy's OpenAI-compatible routes, but
    // tees chat completion traffic into local JSONL logs before Goose parses it.
    let app = create_logged_proxy_app(proxy_config, llm_log_dir);

    // Spawn the proxy server
    let handle = tokio::spawn(async move {
        log::info!("Maple proxy server running on http://{addr}");
        if let Err(e) = axum::serve(listener, app).await {
            log::error!("Proxy server error: {e}");
        }
    });

    // Store the handle
    let mut handle_guard = state.handle.lock().await;
    *handle_guard = Some(handle);
    drop(handle_guard); // Release handle lock early

    *running = true;

    // Save config to disk
    if let Err(e) = save_proxy_config(&app_handle, &config).await {
        log::error!("Failed to save proxy config: {e}");
    }

    Ok(ProxyStatus {
        running: true,
        config,
        error: None,
    })
}

#[tauri::command]
pub async fn stop_proxy(state: State<'_, ProxyState>) -> Result<ProxyStatus, String> {
    log::info!("Stopping proxy");

    let mut running = state.running.lock().await;
    if !*running {
        return Err("Proxy is not running".to_string());
    }

    // Abort the proxy task
    let mut handle_guard = state.handle.lock().await;
    if let Some(handle) = handle_guard.take() {
        handle.abort();
    }
    drop(handle_guard); // Release handle lock before taking config lock to avoid deadlock

    *running = false;

    let config = state.config.lock().await.clone();

    // Config persists even when stopped (we don't auto-start anyway)

    Ok(ProxyStatus {
        running: false,
        config,
        error: None,
    })
}

#[tauri::command]
pub async fn get_proxy_status(state: State<'_, ProxyState>) -> Result<ProxyStatus, String> {
    Ok(state.status().await)
}

#[tauri::command]
pub async fn load_proxy_config(app_handle: AppHandle) -> Result<ProxyConfig, String> {
    load_saved_proxy_config(&app_handle)
        .await
        .map_err(|e| format!("Failed to load proxy config: {e}"))
}

#[tauri::command]
pub async fn save_proxy_settings(app_handle: AppHandle, config: ProxyConfig) -> Result<(), String> {
    save_proxy_config(&app_handle, &config)
        .await
        .map_err(|e| format!("Failed to save proxy config: {e}"))
}

#[tauri::command]
pub async fn test_proxy_port(host: String, port: u16) -> Result<bool, String> {
    // Try to bind to the address to check if it's available
    let addr = format!("{host}:{port}");
    match TcpListener::bind(&addr).await {
        Ok(_) => Ok(true), // Port is available
        Err(e) => {
            if e.kind() == std::io::ErrorKind::AddrInUse {
                Ok(false) // Port is in use
            } else {
                Err(format!("Failed to test port: {e}"))
            }
        }
    }
}

fn create_logged_proxy_app(config: Config, llm_log_dir: PathBuf) -> Router {
    create_app(config).layer(middleware::from_fn_with_state(
        Arc::new(LoggedProxyState { llm_log_dir }),
        log_chat_completion_traffic,
    ))
}

async fn log_chat_completion_traffic(
    AxumState(state): AxumState<Arc<LoggedProxyState>>,
    request: Request,
    next: Next,
) -> Response {
    if request.method() != Method::POST || request.uri().path() != "/v1/chat/completions" {
        return next.run(request).await;
    }

    let request_id = next_proxy_llm_request_id();
    let llm_log = create_proxy_llm_log(&state.llm_log_dir, &request_id);
    let (parts, body) = request.into_parts();
    let path = parts.uri.path().to_string();

    let body_bytes = match to_bytes(body, usize::MAX).await {
        Ok(bytes) => bytes,
        Err(error) => {
            append_proxy_llm_log_row(
                llm_log.as_ref(),
                serde_json::json!({
                    "type": "request_read_error",
                    "request_id": request_id,
                    "ts": unix_ms(),
                    "path": path,
                    "error": error.to_string(),
                }),
            );
            return (StatusCode::BAD_REQUEST, "Failed to read proxy request body").into_response();
        }
    };

    let raw_body = parse_body_json(&body_bytes);
    let model = raw_body
        .get("model")
        .and_then(Value::as_str)
        .unwrap_or("<missing>");
    let is_streaming = raw_body
        .get("stream")
        .and_then(Value::as_bool)
        .unwrap_or(false);

    append_proxy_llm_log_row(
        llm_log.as_ref(),
        serde_json::json!({
            "type": "request",
            "request_id": request_id,
            "ts": unix_ms(),
            "path": path,
            "model": model,
            "stream": is_streaming,
            "tools_len": raw_body.get("tools").and_then(Value::as_array).map(|tools| tools.len()).unwrap_or(0),
            "raw_has_max_tokens": raw_body.get("max_tokens").is_some(),
            "raw_has_max_completion_tokens": raw_body.get("max_completion_tokens").is_some(),
            "raw_has_max_output_tokens": raw_body.get("max_output_tokens").is_some(),
            "raw_body": raw_body,
            "note": "Logged in Maple proxy middleware before maple-proxy parses the request; response_chunk rows are raw SSE/HTTP body bytes returned by maple-proxy to Goose.",
        }),
    );

    log::info!(
        "Proxy LLM request {request_id} model={} stream={} log={}",
        model,
        is_streaming,
        llm_log
            .as_ref()
            .map(|log| log.path.display().to_string())
            .unwrap_or_else(|| "unavailable".to_string())
    );

    let request = Request::from_parts(parts, Body::from(body_bytes));
    let response = next.run(request).await;
    let (response_parts, response_body) = response.into_parts();
    let status = response_parts.status;

    append_proxy_llm_log_row(
        llm_log.as_ref(),
        serde_json::json!({
            "type": "response_start",
            "request_id": request_id,
            "ts": unix_ms(),
            "status": status.as_u16(),
        }),
    );

    let response_stream = response_body.into_data_stream();
    let logged_stream = log_response_body_stream(response_stream, request_id, llm_log);
    Response::from_parts(response_parts, Body::from_stream(logged_stream))
}

fn parse_body_json(body: &Bytes) -> Value {
    serde_json::from_slice::<Value>(body).unwrap_or_else(|error| {
        serde_json::json!({
            "_parse_error": error.to_string(),
            "_raw_utf8_lossy": String::from_utf8_lossy(body),
        })
    })
}

fn log_response_body_stream<S>(
    stream: S,
    request_id: String,
    llm_log: Option<ProxyLlmLog>,
) -> impl Stream<Item = Result<Bytes, axum::Error>> + Send
where
    S: Stream<Item = Result<Bytes, axum::Error>> + Send + 'static,
{
    type BoxedBodyStream = Pin<Box<dyn Stream<Item = Result<Bytes, axum::Error>> + Send>>;
    let stream: BoxedBodyStream = Box::pin(stream);

    futures::stream::unfold((stream, llm_log), move |(mut stream, llm_log)| {
        let request_id = request_id.clone();
        async move {
            match stream.next().await {
                Some(Ok(bytes)) => {
                    append_proxy_llm_log_row(
                        llm_log.as_ref(),
                        serde_json::json!({
                            "type": "response_chunk",
                            "request_id": request_id,
                            "ts": unix_ms(),
                            "bytes_len": bytes.len(),
                            "body_utf8_lossy": String::from_utf8_lossy(&bytes),
                        }),
                    );
                    Some((Ok(bytes), (stream, llm_log)))
                }
                Some(Err(error)) => {
                    append_proxy_llm_log_row(
                        llm_log.as_ref(),
                        serde_json::json!({
                            "type": "response_body_error",
                            "request_id": request_id,
                            "ts": unix_ms(),
                            "error": error.to_string(),
                        }),
                    );
                    Some((Err(error), (stream, llm_log)))
                }
                None => {
                    append_proxy_llm_log_row(
                        llm_log.as_ref(),
                        serde_json::json!({
                            "type": "response_done",
                            "request_id": request_id,
                            "ts": unix_ms(),
                        }),
                    );
                    None
                }
            }
        }
    })
}

fn create_proxy_llm_log(log_dir: &Path, request_id: &str) -> Option<ProxyLlmLog> {
    let path = log_dir.join(format!("{request_id}.jsonl"));
    match OpenOptions::new().create(true).append(true).open(&path) {
        Ok(file) => {
            set_owner_only_permissions(&path);
            Some(ProxyLlmLog {
                request_id: request_id.to_string(),
                path,
                file: Arc::new(StdMutex::new(file)),
            })
        }
        Err(error) => {
            log::warn!("Failed to create proxy LLM log {}: {error}", path.display());
            None
        }
    }
}

fn append_proxy_llm_log_row(log: Option<&ProxyLlmLog>, mut row: Value) {
    let Some(log) = log else {
        return;
    };
    if let Value::Object(ref mut object) = row {
        object
            .entry("request_id")
            .or_insert_with(|| Value::String(log.request_id.clone()));
    }

    let result = (|| -> Result<()> {
        let mut file = log
            .file
            .lock()
            .map_err(|_| anyhow!("proxy LLM log lock poisoned"))?;
        serde_json::to_writer(&mut *file, &row)?;
        file.write_all(b"\n")?;
        file.flush()?;
        Ok(())
    })();

    if let Err(error) = result {
        log::warn!(
            "Failed to write proxy LLM log {}: {error}",
            log.path.display()
        );
    }
}

fn proxy_llm_log_dir(app_handle: &AppHandle) -> Result<PathBuf> {
    let base = if cfg!(target_os = "windows") {
        app_handle
            .path()
            .app_config_dir()
            .map_err(|e| anyhow!("Failed to resolve app config dir: {e}"))?
    } else {
        let home = std::env::var("HOME").map_err(|_| anyhow!("Failed to get home directory"))?;
        PathBuf::from(home).join(".config").join("maple")
    };
    let path = base.join("agent").join("proxy-llm-logs");
    fs::create_dir_all(&path)?;
    set_owner_only_dir_permissions(&path);
    Ok(path)
}

// Helper functions for config persistence.
// Windows uses the Tauri-managed app_config_dir() (%APPDATA%); macOS/Linux keep
// the historical ~/.config/maple/ location. The Windows API key is additionally
// stored in Credential Manager rather than plaintext (see store/load_api_key).
async fn get_config_path(app_handle: &AppHandle) -> Result<PathBuf> {
    let app_dir = if cfg!(target_os = "windows") {
        // Resolves to %APPDATA%\cloud.opensecret.maple\ (Roaming).
        app_handle
            .path()
            .app_config_dir()
            .map_err(|e| anyhow!("Failed to resolve app config dir: {e}"))?
    } else {
        // macOS/Linux: ~/.config/maple/ — unchanged for byte-identical behavior.
        let app_name = "maple";
        let home_dir =
            std::env::var("HOME").map_err(|_| anyhow!("Failed to get home directory"))?;
        PathBuf::from(home_dir).join(".config").join(app_name)
    };

    // Ensure directory exists
    tokio::fs::create_dir_all(&app_dir).await?;

    Ok(app_dir.join("proxy_config.json"))
}

async fn save_proxy_config(app_handle: &AppHandle, config: &ProxyConfig) -> Result<()> {
    let path = get_config_path(app_handle).await?;

    // On Windows, move the API key into Credential Manager and scrub it from
    // the JSON (the config dir is the roaming profile). Other platforms keep
    // the existing plaintext-in-JSON behavior unchanged.
    #[cfg(target_os = "windows")]
    let json = {
        let scrubbed = match store_api_key(&config.api_key) {
            Ok(true) => ProxyConfig {
                api_key: String::new(),
                ..config.clone()
            },
            Ok(false) => config.clone(),
            // Clearing the key failed: don't scrub the JSON and report success,
            // since the stale credential survives and would be resurrected on
            // the next load. Propagate so the failure is visible.
            Err(e) if config.api_key.is_empty() => return Err(e),
            // Storing failed for another reason: fall back to persisting the key
            // in plaintext JSON so it isn't lost.
            Err(e) => {
                log::warn!("{e}");
                config.clone()
            }
        };
        serde_json::to_string_pretty(&scrubbed)?
    };
    #[cfg(not(target_os = "windows"))]
    let json = serde_json::to_string_pretty(config)?;

    // Write the config file
    tokio::fs::write(&path, json).await?;

    // Set restrictive permissions on Unix systems (owner read/write only)
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let perms = std::fs::Permissions::from_mode(0o600);
        tokio::fs::set_permissions(&path, perms).await?;
    }

    Ok(())
}

pub async fn load_saved_proxy_config(app_handle: &AppHandle) -> Result<ProxyConfig> {
    let path = get_config_path(app_handle).await?;

    if !path.exists() {
        return Ok(ProxyConfig::default());
    }

    let json = tokio::fs::read_to_string(path).await?;
    #[cfg_attr(not(target_os = "windows"), allow(unused_mut))]
    let mut config: ProxyConfig = serde_json::from_str(&json)?;

    // On Windows, prefer the API key from Credential Manager; fall back to any
    // plaintext value still in the JSON if it's unavailable.
    #[cfg(target_os = "windows")]
    if let Some(key) = load_api_key()? {
        if !key.is_empty() {
            config.api_key = key;
        }
    }

    Ok(config)
}

pub async fn ensure_proxy_running(
    app_handle: AppHandle,
    state: State<'_, ProxyState>,
) -> Result<ProxyStatus, String> {
    let current = state.status().await;
    if current.running {
        log::info!(
            "Maple proxy already running on {}:{}",
            current.config.host,
            current.config.port
        );
        return Ok(current);
    }

    let config = load_saved_proxy_config(&app_handle)
        .await
        .map_err(|e| format!("Failed to load proxy config: {e}"))?;

    if config.api_key.trim().is_empty() {
        log::warn!("Maple proxy cannot auto-start because saved config has no API key");
        return Err("Maple proxy is not configured with an API key yet".to_string());
    }

    log::info!(
        "Starting Maple proxy from saved config on {}:{}",
        config.host,
        config.port
    );
    start_proxy(app_handle, state, config).await
}

// Initialize proxy on app startup if auto_start is enabled
pub async fn init_proxy_on_startup_simple(app_handle: AppHandle) -> Result<()> {
    // Load saved config
    let config = load_saved_proxy_config(&app_handle).await?;

    // Check if auto-start is enabled and we have an API key
    if config.auto_start && !config.api_key.is_empty() {
        log::info!("Auto-starting proxy from saved config");

        // Get the proxy state from the app handle
        let proxy_state: tauri::State<ProxyState> = app_handle.state();

        // Try to start the proxy
        match start_proxy(app_handle.clone(), proxy_state, config.clone()).await {
            Ok(_) => {
                log::info!(
                    "Proxy auto-started successfully on {}:{}",
                    config.host,
                    config.port
                );
                // Optionally emit an event to notify the frontend
                let _ = app_handle.emit("proxy-autostarted", &config);
            }
            Err(e) => {
                log::error!("Failed to auto-start proxy: {e}");
                // Emit an event to notify the frontend of the failure
                let _ = app_handle.emit("proxy-autostart-failed", e);
            }
        }
    } else {
        log::info!("Proxy auto-start is disabled or no API key configured");
    }

    Ok(())
}

fn next_proxy_llm_request_id() -> String {
    let counter = PROXY_LLM_LOG_COUNTER.fetch_add(1, Ordering::Relaxed);
    format!("proxy_llm_{}_{}", unix_ms(), counter)
}

fn unix_ms() -> u128 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_millis())
        .unwrap_or_default()
}

#[cfg(unix)]
fn set_owner_only_permissions(path: &Path) {
    use std::os::unix::fs::PermissionsExt;
    let _ = fs::set_permissions(path, fs::Permissions::from_mode(0o600));
}

#[cfg(not(unix))]
fn set_owner_only_permissions(_path: &Path) {}

#[cfg(unix)]
fn set_owner_only_dir_permissions(path: &Path) {
    use std::os::unix::fs::PermissionsExt;
    let _ = fs::set_permissions(path, fs::Permissions::from_mode(0o700));
}

#[cfg(not(unix))]
fn set_owner_only_dir_permissions(_path: &Path) {}
