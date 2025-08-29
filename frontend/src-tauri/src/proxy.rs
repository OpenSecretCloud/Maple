use anyhow::{anyhow, Result};
use maple_proxy::{create_app, Config};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use std::sync::Arc;
use tauri::{AppHandle, Emitter, Manager, State};
use tokio::net::TcpListener;
use tokio::sync::Mutex;

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

impl ProxyState {
    pub fn new() -> Self {
        Self {
            handle: Arc::new(Mutex::new(None)),
            config: Arc::new(Mutex::new(ProxyConfig::default())),
            running: Arc::new(Mutex::new(false)),
        }
    }
}

#[tauri::command]
pub async fn start_proxy(
    state: State<'_, ProxyState>,
    config: ProxyConfig,
) -> Result<ProxyStatus, String> {
    log::info!("Starting proxy with config: {:?}", config);

    // Check if proxy is already running
    let mut running = state.running.lock().await;
    if *running {
        return Err("Proxy is already running".to_string());
    }

    // Update config
    let mut stored_config = state.config.lock().await;
    *stored_config = config.clone();

    // Use backend URL from config or fall back to production
    let backend_url = config.backend_url.clone()
        .unwrap_or_else(|| "https://enclave.trymaple.ai".to_string());
    
    // Create maple-proxy config
    let proxy_config = Config::new(
        config.host.clone(),
        config.port,
        backend_url,
    )
    .with_api_key(config.api_key.clone())
    .with_debug(false)
    .with_cors(config.enable_cors);

    // Try to bind to the address first to check if port is available
    let addr = proxy_config
        .socket_addr()
        .map_err(|e| format!("Invalid address: {}", e))?;

    let listener = match TcpListener::bind(&addr).await {
        Ok(listener) => listener,
        Err(e) => {
            return Err(format!(
                "Failed to bind to {}:{} - {}",
                config.host, config.port, e
            ));
        }
    };

    // Create the app
    let app = create_app(proxy_config);

    // Spawn the proxy server
    let handle = tokio::spawn(async move {
        log::info!("Maple proxy server running on http://{}", addr);
        if let Err(e) = axum::serve(listener, app).await {
            log::error!("Proxy server error: {}", e);
        }
    });

    // Store the handle
    let mut handle_guard = state.handle.lock().await;
    *handle_guard = Some(handle);
    *running = true;

    // Save config to disk
    if let Err(e) = save_proxy_config(&config).await {
        log::error!("Failed to save proxy config: {}", e);
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
    let running = *state.running.lock().await;
    let config = state.config.lock().await.clone();

    Ok(ProxyStatus {
        running,
        config,
        error: None,
    })
}

#[tauri::command]
pub async fn load_proxy_config() -> Result<ProxyConfig, String> {
    load_saved_proxy_config()
        .await
        .map_err(|e| format!("Failed to load proxy config: {}", e))
}

#[tauri::command]
pub async fn save_proxy_settings(config: ProxyConfig) -> Result<(), String> {
    save_proxy_config(&config)
        .await
        .map_err(|e| format!("Failed to save proxy config: {}", e))
}

#[tauri::command]
pub async fn test_proxy_port(host: String, port: u16) -> Result<bool, String> {
    // Try to bind to the address to check if it's available
    let addr = format!("{}:{}", host, port);
    match TcpListener::bind(&addr).await {
        Ok(_) => Ok(true), // Port is available
        Err(e) => {
            if e.kind() == std::io::ErrorKind::AddrInUse {
                Ok(false) // Port is in use
            } else {
                Err(format!("Failed to test port: {}", e))
            }
        }
    }
}

// Helper functions for config persistence
async fn get_config_path() -> Result<PathBuf> {
    // Use a hardcoded app name for the data directory
    let app_name = "maple";
    let home_dir = std::env::var("HOME").or_else(|_| std::env::var("USERPROFILE"))
        .map_err(|_| anyhow!("Failed to get home directory"))?;
    
    let app_dir = PathBuf::from(home_dir)
        .join(".config")
        .join(app_name);

    // Ensure directory exists
    tokio::fs::create_dir_all(&app_dir).await?;

    Ok(app_dir.join("proxy_config.json"))
}

async fn save_proxy_config(config: &ProxyConfig) -> Result<()> {
    let path = get_config_path().await?;
    let json = serde_json::to_string_pretty(config)?;
    tokio::fs::write(path, json).await?;
    Ok(())
}

async fn load_saved_proxy_config() -> Result<ProxyConfig> {
    let path = get_config_path().await?;

    if !path.exists() {
        return Ok(ProxyConfig::default());
    }

    let json = tokio::fs::read_to_string(path).await?;
    let config: ProxyConfig = serde_json::from_str(&json)?;
    Ok(config)
}

// Initialize proxy on app startup if auto_start is enabled
pub async fn init_proxy_on_startup_simple(app_handle: AppHandle) -> Result<()> {
    // Load saved config
    let config = load_saved_proxy_config().await?;
    
    // Check if auto-start is enabled and we have an API key
    if config.auto_start && !config.api_key.is_empty() {
        log::info!("Auto-starting proxy from saved config");
        
        // Get the proxy state from the app handle
        let proxy_state: tauri::State<ProxyState> = app_handle.state();
        
        // Try to start the proxy
        match start_proxy(proxy_state, config.clone()).await {
            Ok(_) => {
                log::info!("Proxy auto-started successfully on {}:{}", config.host, config.port);
                // Optionally emit an event to notify the frontend
                let _ = app_handle.emit("proxy-autostarted", &config);
            }
            Err(e) => {
                log::error!("Failed to auto-start proxy: {}", e);
                // Emit an event to notify the frontend of the failure
                let _ = app_handle.emit("proxy-autostart-failed", e);
            }
        }
    } else {
        log::info!("Proxy auto-start is disabled or no API key configured");
    }
    
    Ok(())
}

