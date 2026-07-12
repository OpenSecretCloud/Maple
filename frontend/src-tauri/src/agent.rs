mod developer_tools;
mod shell_permission;

use crate::proxy;
use developer_tools::MapleDeveloperClient;
use futures_util::StreamExt;
use goose::agents::{
    Agent, AgentConfig as GooseAgentConfig, AgentEvent, ExtensionConfig, GoosePlatform,
    SessionConfig,
};
use goose::config::{
    ConfigError, GooseMode, PermissionManager, DEFAULT_EXTENSION_DESCRIPTION,
    DEFAULT_EXTENSION_TIMEOUT,
};
use goose::conversation::message::{
    ActionRequiredData, Message, MessageContent, SystemNotificationContent, SystemNotificationType,
};
use goose::conversation::Conversation;
use goose::execution::manager::AgentManager;
use goose::permission::permission_confirmation::PrincipalType;
use goose::permission::{Permission, PermissionConfirmation};
use goose::session::session_manager::{Session, SessionType};
use goose::session::SessionManager;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use shell_permission::{
    local_read_image_request_id, local_read_request_id, ShellPermissionClassifier,
    ShellPermissionOutcome, ShellPermissionRequest,
};
use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::{Path, PathBuf};
use std::str::FromStr;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};
use tauri::{AppHandle, Emitter, Manager, State};
use tokio::sync::{oneshot, Mutex};
use tokio_util::sync::CancellationToken;

const DEFAULT_AGENT_MODEL: &str = "glm-5-2";
const LEGACY_AGENT_DEFAULT_MODEL: &str = "auto:powerful";
const DEFAULT_GOOSE_MODE: &str = "smart_approve";
// Keep Goose on its ActionRequired path so Maple can apply the currently selected
// policy at every tool boundary, including when the user changes it mid-run.
const GOOSE_PERMISSION_ROUTING_MODE: GooseMode = GooseMode::SmartApprove;
const AGENT_EVENT_NAME: &str = "agent-event";
const MAPLE_DEVELOPER_TOOLS: [&str; 5] = ["read", "shell", "edit", "write", "read_image"];
const MAPLE_GOOSE_PERMISSION_CONFIG: &str = r#"user:
  always_allow: []
  ask_before:
  - read
  - shell
  - edit
  - write
  - read_image
  never_allow: []
"#;
const RUN_SHUTDOWN_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(10);
const DEFAULT_AGENT_SESSION_TITLE: &str = "New agent session";
const MAX_AGENT_SESSION_TITLE_CHARS: usize = 80;
const MAX_AGENT_ERROR_CHARS: usize = 1_200;
static NEXT_RUN_ID: AtomicU64 = AtomicU64::new(1);

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentConfig {
    pub default_project_root: Option<String>,
    #[serde(default = "default_agent_model")]
    pub default_model: String,
}

fn default_agent_model() -> String {
    DEFAULT_AGENT_MODEL.to_string()
}

impl Default for AgentConfig {
    fn default() -> Self {
        Self {
            default_project_root: None,
            default_model: default_agent_model(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentStartRequest {
    pub project_root: Option<String>,
    pub model: Option<String>,
    pub mode: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentRuntimeStatus {
    pub running: bool,
    pub project_root: Option<String>,
    pub model: Option<String>,
    pub mode: Option<String>,
    pub active_runs: HashMap<String, String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RecentProjectRoot {
    pub path: String,
    pub name: String,
    pub last_used_ms: u128,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentCreateSessionRequest {
    pub project_root: Option<String>,
    pub title: Option<String>,
    pub model: Option<String>,
    pub mode: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentSendMessageRequest {
    pub session_id: String,
    pub text: String,
    pub model: Option<String>,
    pub mode: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentPermissionResponse {
    pub session_id: String,
    pub request_id: String,
    pub decision: String,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentPermissionModeRequest {
    pub session_id: String,
    pub mode: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentRunResponse {
    pub run_id: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentSessionSummary {
    pub id: String,
    pub title: String,
    pub project_root: String,
    pub created_ms: i64,
    pub updated_ms: i64,
    pub message_count: usize,
    pub model: Option<String>,
    pub mode: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentSessionDetail {
    pub session: AgentSessionSummary,
    pub timeline: Vec<AgentTimelineItem>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentTimelineItem {
    pub id: String,
    pub item_type: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub role: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub text: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub status: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub input: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub output: Option<Value>,
    pub created_ms: u128,
    pub merge: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentEventEnvelope {
    pub event_type: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub session_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub run_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub item: Option<AgentTimelineItem>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub status: Option<AgentRuntimeStatus>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub session: Option<AgentSessionSummary>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub message: Option<String>,
}

struct ActiveAgentRun {
    token: CancellationToken,
    session_id: String,
    task_handle: tauri::async_runtime::JoinHandle<()>,
}

type PendingPermissionKey = (String, String);
type PendingPermissions = Arc<Mutex<HashMap<PendingPermissionKey, ()>>>;
type SessionPermissionModes = Arc<Mutex<HashMap<String, GooseMode>>>;

struct AgentRuntime {
    agent_manager: Arc<AgentManager>,
    session_manager: Arc<SessionManager>,
    active_runs: HashMap<String, ActiveAgentRun>,
    permission_modes: SessionPermissionModes,
    project_root: PathBuf,
    model: String,
    mode: String,
    account_scope: String,
}

impl AgentRuntime {
    fn status(&self) -> AgentRuntimeStatus {
        AgentRuntimeStatus {
            running: true,
            project_root: Some(path_string(&self.project_root)),
            model: Some(self.model.clone()),
            mode: Some(self.mode.clone()),
            active_runs: self
                .active_runs
                .iter()
                .map(|(run_id, run)| (run.session_id.clone(), run_id.clone()))
                .collect(),
        }
    }
}

pub struct AgentRuntimeState {
    inner: Arc<Mutex<Option<AgentRuntime>>>,
    runtime_lifecycle: Arc<Mutex<()>>,
    account_generations: Arc<Mutex<HashMap<String, u64>>>,
    session_lifecycle: Arc<Mutex<()>>,
    pending_permissions: PendingPermissions,
    live_timelines: LiveTimelines,
}

type LiveTimelines = Arc<Mutex<HashMap<String, LiveTimeline>>>;

#[derive(Clone, Debug, PartialEq)]
enum LiveTimeline {
    /// The current turn is still emitting events, so this is the authoritative
    /// presentation suffix from its real-user boundary onward.
    Streaming(Vec<AgentTimelineItem>),
    /// Goose finished the turn. Most terminal messages are persisted, but its
    /// synthetic provider errors and notices can be live-only. Resolve that
    /// distinction against the conversation already loaded for the next view.
    Completed(LiveMessageCandidate),
    /// A Maple/Goose task failure is never part of provider history. Keep only
    /// its bounded user-facing error row between views and retries.
    Failed(Vec<AgentTimelineItem>),
}

impl LiveTimeline {
    fn items(&self) -> &[AgentTimelineItem] {
        match self {
            Self::Streaming(items) => items,
            Self::Completed(candidate) => &candidate.items,
            Self::Failed(items) => items,
        }
    }

    fn items_mut(&mut self) -> &mut Vec<AgentTimelineItem> {
        match self {
            Self::Streaming(items) => items,
            Self::Completed(candidate) => &mut candidate.items,
            Self::Failed(items) => items,
        }
    }
}

impl AgentRuntimeState {
    pub fn new() -> Self {
        Self {
            inner: Arc::new(Mutex::new(None)),
            runtime_lifecycle: Arc::new(Mutex::new(())),
            account_generations: Arc::new(Mutex::new(HashMap::new())),
            session_lifecycle: Arc::new(Mutex::new(())),
            pending_permissions: Arc::new(Mutex::new(HashMap::new())),
            live_timelines: Arc::new(Mutex::new(HashMap::new())),
        }
    }
}

fn account_scope(user_id: &str) -> Result<String, String> {
    let user_id = user_id.trim();
    if user_id.is_empty() {
        return Err("Agent Mode requires a signed-in account".to_string());
    }
    let digest = Sha256::digest(user_id.as_bytes());
    Ok(format!("{digest:x}"))
}

fn ensure_runtime_account(runtime: &AgentRuntime, account_scope: &str) -> Result<(), String> {
    ensure_account_scope(&runtime.account_scope, account_scope)
}

fn ensure_account_scope(current_scope: &str, requested_scope: &str) -> Result<(), String> {
    if current_scope == requested_scope {
        Ok(())
    } else {
        Err("Agent runtime belongs to a different signed-in account".to_string())
    }
}

async fn account_generation(state: &AgentRuntimeState, account_scope: &str) -> u64 {
    *state
        .account_generations
        .lock()
        .await
        .get(account_scope)
        .unwrap_or(&0)
}

async fn ensure_account_generation(
    state: &AgentRuntimeState,
    account_scope: &str,
    expected: u64,
) -> Result<(), String> {
    if account_generation(state, account_scope).await == expected {
        Ok(())
    } else {
        Err("Agent Mode data changed while this operation was waiting".to_string())
    }
}

async fn advance_account_generation(state: &AgentRuntimeState, account_scope: &str) -> u64 {
    let mut generations = state.account_generations.lock().await;
    let generation = generations.entry(account_scope.to_string()).or_default();
    *generation = generation
        .checked_add(1)
        .expect("Agent Mode exhausted its account operation generation");
    *generation
}

fn next_run_id() -> String {
    let sequence = NEXT_RUN_ID
        .fetch_update(Ordering::Relaxed, Ordering::Relaxed, |value| {
            value.checked_add(1)
        })
        .expect("Agent Mode exhausted its run ID sequence");
    format!("run_{}_{sequence}", unix_ms())
}

fn session_title_from_prompt(prompt: &str) -> String {
    let collapsed = prompt.split_whitespace().collect::<Vec<_>>().join(" ");
    if collapsed.chars().count() <= MAX_AGENT_SESSION_TITLE_CHARS {
        return collapsed;
    }

    let mut title = collapsed
        .chars()
        .take(MAX_AGENT_SESSION_TITLE_CHARS - 1)
        .collect::<String>();
    title.truncate(title.trim_end().len());
    title.push('…');
    title
}

fn should_name_session_from_prompt(session: &Session) -> bool {
    session.message_count == 0
        && !session.user_set_name
        && session.name == DEFAULT_AGENT_SESSION_TITLE
}

async fn pending_permissions_for_sessions(
    pending_permissions: &PendingPermissions,
    session_ids: &[String],
) -> Vec<(String, String)> {
    let pending = pending_permissions.lock().await;
    pending
        .keys()
        .filter(|(session_id, _)| session_ids.contains(session_id))
        .map(|(session_id, request_id)| (request_id.clone(), session_id.clone()))
        .collect()
}

async fn cancel_pending_permissions_for_sessions(
    agent_manager: &Arc<AgentManager>,
    pending_permissions: &PendingPermissions,
    session_ids: &[String],
) -> Vec<(String, String)> {
    let mut cancelled = Vec::new();
    for (request_id, session_id) in
        pending_permissions_for_sessions(pending_permissions, session_ids).await
    {
        match agent_manager.get_or_create_agent(session_id.clone()).await {
            Ok(agent) => {
                agent
                    .handle_confirmation(
                        request_id.clone(),
                        PermissionConfirmation {
                            principal_type: PrincipalType::Tool,
                            permission: Permission::Cancel,
                        },
                    )
                    .await;
                let mut pending = pending_permissions.lock().await;
                pending.remove(&(session_id.clone(), request_id.clone()));
                cancelled.push((request_id, session_id));
            }
            Err(error) => {
                log::warn!(
                    "Failed to cancel pending Agent Mode permission for session {session_id}: {error}"
                );
            }
        }
    }
    cancelled
}

async fn register_pending_permission(
    pending_permissions: &PendingPermissions,
    request_id: &str,
    session_id: &str,
    cancel_token: &CancellationToken,
) -> bool {
    if cancel_token.is_cancelled() {
        return false;
    }
    let mut pending = pending_permissions.lock().await;
    let key = (session_id.to_string(), request_id.to_string());
    pending.insert(key.clone(), ());
    if cancel_token.is_cancelled() {
        pending.remove(&key);
        false
    } else {
        true
    }
}

async fn stop_runtime_for_user(state: &AgentRuntimeState, user_id: &str) -> Result<(), String> {
    let account_scope = account_scope(user_id)?;
    stop_runtime_inner(state, Some(&account_scope)).await
}

async fn stop_runtime_inner(
    state: &AgentRuntimeState,
    requested_scope: Option<&str>,
) -> Result<(), String> {
    let (agent_manager, active_runs) = {
        let mut runtime = state.inner.lock().await;
        let Some(current) = runtime.as_mut() else {
            return Ok(());
        };
        if let Some(account_scope) = requested_scope {
            ensure_runtime_account(current, account_scope)?;
        }
        (
            Arc::clone(&current.agent_manager),
            std::mem::take(&mut current.active_runs),
        )
    };

    let session_ids = active_runs
        .values()
        .map(|run| run.session_id.clone())
        .collect::<Vec<_>>();
    let mut task_handles = Vec::with_capacity(active_runs.len());
    for (_, active_run) in active_runs {
        // Cancel first so an ActionRequired event racing this snapshot will
        // take the immediate-cancel path in register_pending_permission.
        active_run.token.cancel();
        task_handles.push(active_run.task_handle);
    }
    let _ = cancel_pending_permissions_for_sessions(
        &agent_manager,
        &state.pending_permissions,
        &session_ids,
    )
    .await;

    join_agent_tasks(task_handles, RUN_SHUTDOWN_TIMEOUT).await;

    state.pending_permissions.lock().await.clear();
    state.live_timelines.lock().await.clear();
    *state.inner.lock().await = None;
    Ok(())
}

async fn join_agent_tasks(
    mut task_handles: Vec<tauri::async_runtime::JoinHandle<()>>,
    graceful_timeout: std::time::Duration,
) {
    let graceful = futures_util::future::join_all(task_handles.iter_mut());
    if tokio::time::timeout(graceful_timeout, graceful)
        .await
        .is_err()
    {
        for task_handle in &task_handles {
            task_handle.abort();
        }
        // Once abort is requested, join every task without another timeout.
        // Dropping a still-running JoinHandle detaches it and could leave an OS
        // child or old-account event source alive after a new runtime starts.
        let _ = futures_util::future::join_all(task_handles).await;
    }
}

pub async fn shutdown_agent_runtime(app_handle: &AppHandle) -> Result<(), String> {
    let state = app_handle.state::<AgentRuntimeState>();
    let _runtime_lifecycle_guard = state.runtime_lifecycle.lock().await;
    stop_runtime_inner(&state, None).await
}

#[tauri::command]
pub async fn agent_get_runtime_status(
    state: State<'_, AgentRuntimeState>,
    user_id: String,
) -> Result<AgentRuntimeStatus, String> {
    let _runtime_lifecycle_guard = state.runtime_lifecycle.lock().await;
    let account_scope = account_scope(&user_id)?;
    let runtime = state.inner.lock().await;
    if let Some(current) = runtime.as_ref() {
        ensure_runtime_account(current, &account_scope)?;
        return Ok(current.status());
    }
    Ok(stopped_status())
}

#[tauri::command]
pub async fn agent_start_runtime(
    app_handle: AppHandle,
    state: State<'_, AgentRuntimeState>,
    proxy_state: State<'_, proxy::ProxyState>,
    user_id: String,
    request: Option<AgentStartRequest>,
) -> Result<AgentRuntimeStatus, String> {
    let account_scope = account_scope(&user_id)?;
    let generation = account_generation(&state, &account_scope).await;
    let _runtime_lifecycle_guard = state.runtime_lifecycle.lock().await;
    ensure_account_generation(&state, &account_scope, generation).await?;
    start_runtime_for_user(app_handle, &state, proxy_state, user_id, request).await
}

async fn start_runtime_for_user(
    app_handle: AppHandle,
    state: &AgentRuntimeState,
    proxy_state: State<'_, proxy::ProxyState>,
    user_id: String,
    request: Option<AgentStartRequest>,
) -> Result<AgentRuntimeStatus, String> {
    let account_scope = account_scope(&user_id)?;
    {
        let runtime = state.inner.lock().await;
        if let Some(current) = runtime.as_ref() {
            ensure_runtime_account(current, &account_scope)?;
            return Ok(current.status());
        }
    }

    let agent_config = load_agent_config_inner(&app_handle, &user_id).unwrap_or_default();
    let request = request.unwrap_or(AgentStartRequest {
        project_root: None,
        model: None,
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

    let project_root = resolve_project_root(request.project_root.as_deref(), &agent_config)
        .map_err(|e| format!("Failed to resolve Agent Mode project root: {e}"))?;
    let model = request.model.unwrap_or(agent_config.default_model);
    let mode = request
        .mode
        .unwrap_or_else(|| DEFAULT_GOOSE_MODE.to_string());
    parse_user_permission_mode(&mode)?;

    let config_dir = agent_config_dir(&app_handle, &user_id).map_err(|e| e.to_string())?;
    let goose_path_root = config_dir.join("goose");
    fs::create_dir_all(goose_path_root.join("data"))
        .map_err(|e| format!("Failed to create Goose data dir: {e}"))?;
    fs::create_dir_all(goose_path_root.join("config"))
        .map_err(|e| format!("Failed to create Goose config dir: {e}"))?;
    // This account-scoped PermissionManager is the one AgentManager actually
    // inspects. Force every Maple-routed tool through ActionRequired before it
    // is constructed so stale Goose AlwaysAllow entries cannot bypass Maple.
    reset_maple_owned_permission_file(&goose_path_root.join("config").join("permission.yaml"))?;

    configure_embedded_goose(
        &agent_root_dir(&app_handle)
            .map_err(|e| e.to_string())?
            .join("goose-runtime"),
        &model,
        DEFAULT_GOOSE_MODE,
        &maple_proxy_base_url,
    )?;

    let session_manager = Arc::new(SessionManager::new(goose_path_root.join("data")));
    let permission_manager = Arc::new(PermissionManager::new(goose_path_root.join("config")));
    let goose_config = GooseAgentConfig::new(
        Arc::clone(&session_manager),
        permission_manager,
        None,
        GOOSE_PERMISSION_ROUTING_MODE,
        true,
        GoosePlatform::GooseDesktop,
    )
    .with_use_login_shell_path(true);
    let agent_manager = Arc::new(
        AgentManager::new(goose_config, None)
            .await
            .map_err(|e| format!("Failed to create Goose agent manager: {e}"))?,
    );

    let runtime = AgentRuntime {
        agent_manager,
        session_manager,
        active_runs: HashMap::new(),
        permission_modes: Arc::new(Mutex::new(HashMap::new())),
        project_root: project_root.clone(),
        model: model.clone(),
        mode: mode.clone(),
        account_scope,
    };
    let status = runtime.status();

    {
        let mut guard = state.inner.lock().await;
        *guard = Some(runtime);
    }

    let _ = save_recent_project_root_inner(&app_handle, &user_id, &project_root);
    let mut next_config = load_agent_config_inner(&app_handle, &user_id).unwrap_or_default();
    next_config.default_project_root = Some(path_string(&project_root));
    next_config.default_model = model;
    let _ = save_agent_config_inner(&app_handle, &user_id, &next_config);

    emit_agent_event(
        &app_handle,
        AgentEventEnvelope {
            event_type: "runtimeStatus".to_string(),
            session_id: None,
            run_id: None,
            item: None,
            status: Some(status.clone()),
            session: None,
            message: None,
        },
    );

    Ok(status)
}

#[tauri::command]
pub async fn agent_stop_runtime(
    state: State<'_, AgentRuntimeState>,
    user_id: String,
) -> Result<AgentRuntimeStatus, String> {
    let account_scope = account_scope(&user_id)?;
    let generation = account_generation(&state, &account_scope).await;
    let _runtime_lifecycle_guard = state.runtime_lifecycle.lock().await;
    ensure_account_generation(&state, &account_scope, generation).await?;
    stop_runtime_for_user(&state, &user_id).await?;
    Ok(stopped_status())
}

#[tauri::command]
pub async fn agent_restart_runtime(
    app_handle: AppHandle,
    state: State<'_, AgentRuntimeState>,
    proxy_state: State<'_, proxy::ProxyState>,
    user_id: String,
    request: Option<AgentStartRequest>,
) -> Result<AgentRuntimeStatus, String> {
    let account_scope = account_scope(&user_id)?;
    let generation = account_generation(&state, &account_scope).await;
    let _runtime_lifecycle_guard = state.runtime_lifecycle.lock().await;
    ensure_account_generation(&state, &account_scope, generation).await?;
    stop_runtime_for_user(&state, &user_id).await?;
    start_runtime_for_user(app_handle, &state, proxy_state, user_id, request).await
}

#[tauri::command]
pub async fn agent_clear_user_data(
    app_handle: AppHandle,
    state: State<'_, AgentRuntimeState>,
    user_id: String,
) -> Result<(), String> {
    let requested_scope = account_scope(&user_id)?;
    let _runtime_lifecycle_guard = state.runtime_lifecycle.lock().await;
    advance_account_generation(&state, &requested_scope).await;
    let is_running_account = {
        let runtime = state.inner.lock().await;
        runtime
            .as_ref()
            .is_some_and(|current| current.account_scope == requested_scope)
    };
    if is_running_account {
        stop_runtime_for_user(&state, &user_id).await?;
    }

    let account_dir =
        account_config_dir_path(&app_handle, &user_id).map_err(|error| error.to_string())?;
    match fs::remove_dir_all(account_dir) {
        Ok(()) => {}
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(error) => return Err(format!("Failed to clear Agent Mode data: {error}")),
    }
    Ok(())
}

#[tauri::command]
pub async fn agent_clear_user_history(
    app_handle: AppHandle,
    state: State<'_, AgentRuntimeState>,
    user_id: String,
) -> Result<(), String> {
    let requested_scope = account_scope(&user_id)?;
    let _runtime_lifecycle_guard = state.runtime_lifecycle.lock().await;
    advance_account_generation(&state, &requested_scope).await;
    let is_running_account = {
        let runtime = state.inner.lock().await;
        runtime
            .as_ref()
            .is_some_and(|current| current.account_scope == requested_scope)
    };
    if is_running_account {
        stop_runtime_for_user(&state, &user_id).await?;
    }

    let account_dir =
        account_config_dir_path(&app_handle, &user_id).map_err(|error| error.to_string())?;
    clear_agent_history(&account_dir)
        .map_err(|error| format!("Failed to clear Agent Mode history: {error}"))
}

#[tauri::command]
pub async fn agent_load_config(
    app_handle: AppHandle,
    state: State<'_, AgentRuntimeState>,
    user_id: String,
) -> Result<AgentConfig, String> {
    let account_scope = account_scope(&user_id)?;
    let generation = account_generation(&state, &account_scope).await;
    let _runtime_lifecycle_guard = state.runtime_lifecycle.lock().await;
    ensure_account_generation(&state, &account_scope, generation).await?;
    load_agent_config_inner(&app_handle, &user_id).map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn agent_save_config(
    app_handle: AppHandle,
    state: State<'_, AgentRuntimeState>,
    user_id: String,
    config: AgentConfig,
) -> Result<(), String> {
    let account_scope = account_scope(&user_id)?;
    let generation = account_generation(&state, &account_scope).await;
    let _runtime_lifecycle_guard = state.runtime_lifecycle.lock().await;
    ensure_account_generation(&state, &account_scope, generation).await?;
    save_agent_config_inner(&app_handle, &user_id, &config).map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn agent_list_recent_project_roots(
    app_handle: AppHandle,
    state: State<'_, AgentRuntimeState>,
    user_id: String,
) -> Result<Vec<RecentProjectRoot>, String> {
    let account_scope = account_scope(&user_id)?;
    let generation = account_generation(&state, &account_scope).await;
    let _runtime_lifecycle_guard = state.runtime_lifecycle.lock().await;
    ensure_account_generation(&state, &account_scope, generation).await?;
    load_recent_project_roots_inner(&app_handle, &user_id).map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn agent_save_recent_project_root(
    app_handle: AppHandle,
    state: State<'_, AgentRuntimeState>,
    user_id: String,
    path: String,
) -> Result<Vec<RecentProjectRoot>, String> {
    let account_scope = account_scope(&user_id)?;
    let generation = account_generation(&state, &account_scope).await;
    let _runtime_lifecycle_guard = state.runtime_lifecycle.lock().await;
    ensure_account_generation(&state, &account_scope, generation).await?;
    let project_root = normalize_project_root(Path::new(&path))?;
    save_recent_project_root_inner(&app_handle, &user_id, &project_root).map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn agent_create_session(
    app_handle: AppHandle,
    state: State<'_, AgentRuntimeState>,
    user_id: String,
    request: Option<AgentCreateSessionRequest>,
) -> Result<AgentSessionDetail, String> {
    let account_scope = account_scope(&user_id)?;
    let generation = account_generation(&state, &account_scope).await;
    let _runtime_lifecycle_guard = state.runtime_lifecycle.lock().await;
    ensure_account_generation(&state, &account_scope, generation).await?;
    let request = request.unwrap_or(AgentCreateSessionRequest {
        project_root: None,
        title: None,
        model: None,
        mode: None,
    });
    let (
        agent_manager,
        session_manager,
        permission_modes,
        runtime_project_root,
        runtime_model,
        runtime_mode,
    ) = {
        let runtime = state.inner.lock().await;
        let current = runtime
            .as_ref()
            .ok_or_else(|| "Agent runtime is not running".to_string())?;
        ensure_runtime_account(current, &account_scope)?;
        (
            Arc::clone(&current.agent_manager),
            Arc::clone(&current.session_manager),
            Arc::clone(&current.permission_modes),
            current.project_root.clone(),
            current.model.clone(),
            current.mode.clone(),
        )
    };

    let root = match request.project_root.as_deref() {
        Some(path) if !path.trim().is_empty() => normalize_project_root(Path::new(path))?,
        _ => runtime_project_root,
    };
    let title = request
        .title
        .filter(|value| !value.trim().is_empty())
        .unwrap_or_else(|| DEFAULT_AGENT_SESSION_TITLE.to_string());
    let mode = request.mode.unwrap_or(runtime_mode);
    let permission_mode = parse_user_permission_mode(&mode)?;
    let model = request.model.unwrap_or(runtime_model);
    let session = session_manager
        .create_session(root.clone(), title, SessionType::User, permission_mode)
        .await
        .map_err(|e| format!("Failed to create Goose session: {e}"))?;

    permission_modes
        .lock()
        .await
        .insert(session.id.clone(), permission_mode);
    configure_session_agent(&agent_manager, &session_manager, &session, &model, &mode).await?;
    let summary = session_summary(&session);
    let _ = save_recent_project_root_inner(&app_handle, &user_id, &root);
    let detail = AgentSessionDetail {
        session: summary.clone(),
        timeline: Vec::new(),
    };
    emit_agent_event(
        &app_handle,
        AgentEventEnvelope {
            event_type: "sessionCreated".to_string(),
            session_id: Some(summary.id.clone()),
            run_id: None,
            item: None,
            status: None,
            session: Some(summary),
            message: None,
        },
    );
    Ok(detail)
}

#[tauri::command]
pub async fn agent_list_sessions(
    app_handle: AppHandle,
    state: State<'_, AgentRuntimeState>,
    user_id: String,
    project_root: Option<String>,
) -> Result<Vec<AgentSessionSummary>, String> {
    let account_scope = account_scope(&user_id)?;
    let generation = account_generation(&state, &account_scope).await;
    let _runtime_lifecycle_guard = state.runtime_lifecycle.lock().await;
    ensure_account_generation(&state, &account_scope, generation).await?;
    let (session_manager, filter_root) = {
        let runtime = state.inner.lock().await;
        let session_manager = match runtime.as_ref() {
            Some(current) => {
                ensure_runtime_account(current, &account_scope)?;
                Arc::clone(&current.session_manager)
            }
            None => account_session_manager(&app_handle, &user_id)?,
        };
        let filter_root = project_root
            .as_deref()
            .filter(|value| !value.trim().is_empty())
            .map(|path| normalize_project_root(Path::new(path)))
            .transpose()?;
        (session_manager, filter_root)
    };

    let mut sessions = session_manager
        .list_all_sessions()
        .await
        .map_err(|e| format!("Failed to list Goose sessions: {e}"))?
        .into_iter()
        .filter(|session| {
            if let Some(root) = filter_root.as_ref() {
                session.working_dir == *root
            } else {
                true
            }
        })
        .map(|session| session_summary(&session))
        .collect::<Vec<_>>();
    sessions.sort_by(|a, b| b.updated_ms.cmp(&a.updated_ms));
    Ok(sessions)
}

#[tauri::command]
pub async fn agent_load_session(
    app_handle: AppHandle,
    state: State<'_, AgentRuntimeState>,
    user_id: String,
    session_id: String,
) -> Result<AgentSessionDetail, String> {
    let account_scope = account_scope(&user_id)?;
    let generation = account_generation(&state, &account_scope).await;
    let _runtime_lifecycle_guard = state.runtime_lifecycle.lock().await;
    ensure_account_generation(&state, &account_scope, generation).await?;
    let session_manager = {
        let runtime = state.inner.lock().await;
        match runtime.as_ref() {
            Some(current) => {
                ensure_runtime_account(current, &account_scope)?;
                Arc::clone(&current.session_manager)
            }
            None => account_session_manager(&app_handle, &user_id)?,
        }
    };
    let session = session_manager
        .get_session(&session_id, true)
        .await
        .map_err(|e| format!("Failed to load Goose session: {e}"))?;
    let conversation = session
        .conversation
        .as_ref()
        .ok_or_else(|| "Goose session history was not loaded".to_string())?;
    let timeline = conversation_to_timeline_items(conversation);
    let timeline =
        overlay_live_timeline(&state.live_timelines, &session_id, conversation, timeline).await;

    Ok(AgentSessionDetail {
        session: session_summary(&session),
        timeline,
    })
}

#[tauri::command]
pub async fn agent_delete_session(
    app_handle: AppHandle,
    state: State<'_, AgentRuntimeState>,
    user_id: String,
    session_id: String,
) -> Result<(), String> {
    let account_scope = account_scope(&user_id)?;
    let generation = account_generation(&state, &account_scope).await;
    let _runtime_lifecycle_guard = state.runtime_lifecycle.lock().await;
    ensure_account_generation(&state, &account_scope, generation).await?;
    let session_id = session_id.trim().to_string();
    if session_id.is_empty() {
        return Err("Agent session ID cannot be empty".to_string());
    }

    let _session_lifecycle_guard = state.session_lifecycle.lock().await;
    let (agent_manager, session_manager, permission_modes) = {
        let runtime = state.inner.lock().await;
        match runtime.as_ref() {
            Some(current) => {
                ensure_runtime_account(current, &account_scope)?;
                if has_active_session_run(&current.active_runs, &session_id) {
                    return Err("Stop the running agent before deleting this chat".to_string());
                }
                (
                    Some(Arc::clone(&current.agent_manager)),
                    Arc::clone(&current.session_manager),
                    Some(Arc::clone(&current.permission_modes)),
                )
            }
            None => (None, account_session_manager(&app_handle, &user_id)?, None),
        }
    };

    delete_persisted_agent_session(
        session_manager.as_ref(),
        &state.pending_permissions,
        &state.live_timelines,
        &session_id,
    )
    .await?;
    if let Some(agent_manager) = agent_manager {
        if let Err(error) = agent_manager.remove_session_if_loaded(&session_id).await {
            log::warn!(
                "Deleted Goose session {session_id}, but failed to unload its agent: {error}"
            );
        }
    }
    if let Some(permission_modes) = permission_modes {
        permission_modes.lock().await.remove(&session_id);
    }

    Ok(())
}

async fn delete_persisted_agent_session(
    session_manager: &SessionManager,
    pending_permissions: &PendingPermissions,
    live_timelines: &LiveTimelines,
    session_id: &str,
) -> Result<(), String> {
    session_manager
        .get_session(session_id, false)
        .await
        .map_err(|e| format!("Failed to find Goose session {session_id}: {e}"))?;
    session_manager
        .delete_session(session_id)
        .await
        .map_err(|e| format!("Failed to delete Goose session {session_id}: {e}"))?;

    live_timelines.lock().await.remove(session_id);
    pending_permissions
        .lock()
        .await
        .retain(|(pending_session_id, _), _| pending_session_id != session_id);

    Ok(())
}

struct AgentTurnSnapshot {
    conversation: Conversation,
    autogenerated_title: Option<String>,
    live_timeline: Option<LiveTimeline>,
}

async fn rollback_cancelled_agent_turn(
    session_manager: &SessionManager,
    live_timelines: &LiveTimelines,
    session_id: &str,
    snapshot: &AgentTurnSnapshot,
) -> Result<(), String> {
    session_manager
        .replace_conversation(session_id, &snapshot.conversation)
        .await
        .map_err(|error| format!("Failed to restore conversation after cancellation: {error}"))?;

    if let Some(title) = snapshot.autogenerated_title.as_ref() {
        let session = session_manager
            .get_session(session_id, false)
            .await
            .map_err(|error| format!("Failed to inspect cancelled Agent session: {error}"))?;
        if !session.user_set_name {
            session_manager
                .update(session_id)
                .system_generated_name(title.clone())
                .apply()
                .await
                .map_err(|error| {
                    format!("Failed to restore cancelled Agent session title: {error}")
                })?;
        }
    }

    // HistoryReplaced can clear the optimistic user boundary before later
    // current-turn events arrive, so restore the exact pre-turn map entry
    // instead of trying to identify and truncate a suffix.
    let mut timelines = live_timelines.lock().await;
    match snapshot.live_timeline.as_ref() {
        Some(items) => {
            timelines.insert(session_id.to_string(), items.clone());
        }
        None => {
            timelines.remove(session_id);
        }
    }

    Ok(())
}

#[tauri::command]
pub async fn agent_send_message(
    app_handle: AppHandle,
    state: State<'_, AgentRuntimeState>,
    user_id: String,
    request: AgentSendMessageRequest,
) -> Result<AgentRunResponse, String> {
    let account_scope = account_scope(&user_id)?;
    let generation = account_generation(&state, &account_scope).await;
    let _runtime_lifecycle_guard = state.runtime_lifecycle.lock().await;
    ensure_account_generation(&state, &account_scope, generation).await?;
    let text = request.text.trim().to_string();
    if text.is_empty() {
        return Err("Prompt cannot be empty".to_string());
    }

    let session_lifecycle_guard = state.session_lifecycle.lock().await;
    let run_id = next_run_id();
    let cancel_token = CancellationToken::new();
    let prompt_title = session_title_from_prompt(&text);
    let user_message = Message::user().with_text(text).with_generated_id();
    let (agent_manager, session_manager, permission_modes, model, mode) = {
        let runtime = state.inner.lock().await;
        let current = runtime
            .as_ref()
            .ok_or_else(|| "Agent runtime is not running".to_string())?;
        ensure_runtime_account(current, &account_scope)?;
        (
            Arc::clone(&current.agent_manager),
            Arc::clone(&current.session_manager),
            Arc::clone(&current.permission_modes),
            request
                .model
                .clone()
                .unwrap_or_else(|| current.model.clone()),
            request.mode.clone().unwrap_or_else(|| current.mode.clone()),
        )
    };
    let requested_permission_mode = parse_user_permission_mode(&mode)?;

    let user_item = message_to_timeline_items(&user_message, false)
        .into_iter()
        .next()
        .ok_or_else(|| "Failed to create user timeline item".to_string())?;
    let live_timelines = Arc::clone(&state.live_timelines);

    // Claim the session before changing its title, provider, mode, or
    // extensions. A duplicate send must not mutate an Agent that is already
    // serving another run.
    agent_manager
        .try_register_cancel_token(&request.session_id, cancel_token.clone())
        .await
        .map_err(|e| format!("Agent session is already running: {e}"))?;

    // A rejected or delayed send must not be able to change a live policy that
    // the mode command already made authoritative. Seed only sessions that do
    // not yet have runtime policy state, after Goose grants this run its claim.
    let (permission_mode, seeded_permission_mode) = {
        let mut modes = permission_modes.lock().await;
        select_session_permission_mode(&mut modes, &request.session_id, requested_permission_mode)
    };
    let effective_mode = permission_mode.to_string();

    let setup_result: Result<(Arc<Agent>, AgentTurnSnapshot), String> = async {
        let mut session = session_manager
            .get_session(&request.session_id, true)
            .await
            .map_err(|e| format!("Failed to load Goose session: {e}"))?;
        let should_restore_autogenerated_title = should_name_session_from_prompt(&session);
        // Cancellation must be able to reverse Goose compaction or recovery
        // that rewrites history during this turn. Move the loaded conversation
        // into the snapshot to avoid cloning large persisted image payloads.
        let turn_snapshot = AgentTurnSnapshot {
            conversation: session.conversation.take().unwrap_or_default(),
            autogenerated_title: should_restore_autogenerated_title.then(|| session.name.clone()),
            live_timeline: live_timelines.lock().await.get(&session.id).cloned(),
        };
        if should_restore_autogenerated_title {
            session_manager
                .update(&session.id)
                .system_generated_name(prompt_title)
                .apply()
                .await
                .map_err(|e| format!("Failed to name Agent session: {e}"))?;
            session = session_manager
                .get_session(&session.id, false)
                .await
                .map_err(|e| format!("Failed to load named Goose session: {e}"))?;
            emit_agent_event(
                &app_handle,
                AgentEventEnvelope {
                    event_type: "sessionUpdated".to_string(),
                    session_id: Some(session.id.clone()),
                    run_id: Some(run_id.clone()),
                    item: None,
                    status: None,
                    session: Some(session_summary(&session)),
                    message: None,
                },
            );
        }
        let agent = configure_session_agent(
            &agent_manager,
            &session_manager,
            &session,
            &model,
            &effective_mode,
        )
        .await?;
        Ok((agent, turn_snapshot))
    }
    .await;
    let (agent, task_turn_snapshot) = match setup_result {
        Ok(setup) => setup,
        Err(error) => {
            if seeded_permission_mode {
                permission_modes.lock().await.remove(&request.session_id);
            }
            agent_manager
                .unregister_cancel_token(&request.session_id)
                .await;
            return Err(error);
        }
    };

    let app_handle_for_task = app_handle.clone();
    let state_inner = Arc::clone(&state.inner);
    let session_lifecycle = Arc::clone(&state.session_lifecycle);
    let pending_permissions = Arc::clone(&state.pending_permissions);
    let session_id = request.session_id.clone();
    let task_run_id = run_id.clone();
    let task_agent_manager = Arc::clone(&agent_manager);
    let task_session_manager = Arc::clone(&session_manager);
    let task_permission_modes = Arc::clone(&permission_modes);
    let task_user_message = user_message.clone();
    let task_cancel_token = cancel_token.clone();
    let (start_tx, start_rx) = oneshot::channel();
    let task = tauri::async_runtime::spawn(async move {
        let should_run = tokio::select! {
            biased;
            _ = task_cancel_token.cancelled() => false,
            start = start_rx => start.is_ok(),
        };
        let result = if should_run {
            run_agent_prompt(AgentPromptRun {
                app_handle: app_handle_for_task.clone(),
                agent,
                session_manager: Arc::clone(&task_session_manager),
                live_timelines: live_timelines.clone(),
                session_id: session_id.clone(),
                run_id: task_run_id.clone(),
                user_message: task_user_message,
                permission_modes: task_permission_modes,
                cancel_token: task_cancel_token.clone(),
                pending_permissions,
            })
            .await
        } else {
            Ok(AgentPromptOutcome::default())
        };

        // Keep deletion serialized until every terminal write and event for
        // this run has completed. The active-run entry stays visible while
        // the cleanup is in progress, so deletion continues to reject it.
        let _session_lifecycle_guard = session_lifecycle.lock().await;
        let result = if task_cancel_token.is_cancelled() {
            rollback_cancelled_agent_turn(
                task_session_manager.as_ref(),
                &live_timelines,
                &session_id,
                &task_turn_snapshot,
            )
            .await
            .map(|_| AgentPromptOutcome::default())
        } else {
            result
        };
        task_agent_manager
            .unregister_cancel_token(&session_id)
            .await;
        if !task_cancel_token.is_cancelled() {
            if let Ok(outcome) = &result {
                let mut timelines = live_timelines.lock().await;
                apply_successful_prompt_outcome(&mut timelines, &session_id, outcome);
            }
        }

        let (status, message) = match result {
            Ok(_) if task_cancel_token.is_cancelled() => ("cancelled", None),
            Ok(_) => ("completed", None),
            Err(error) => ("failed", Some(error)),
        };
        if let Some(error) = message.as_ref() {
            let item = error_item(error.clone());
            {
                let mut timelines = live_timelines.lock().await;
                apply_failed_prompt_outcome(&mut timelines, &session_id, item.clone());
            }
            emit_agent_event(
                &app_handle_for_task,
                AgentEventEnvelope {
                    event_type: "error".to_string(),
                    session_id: Some(session_id.clone()),
                    run_id: Some(task_run_id.clone()),
                    item: Some(item),
                    status: None,
                    session: None,
                    message: None,
                },
            );
        }
        emit_agent_event(
            &app_handle_for_task,
            AgentEventEnvelope {
                event_type: "runFinished".to_string(),
                session_id: Some(session_id),
                run_id: Some(task_run_id.clone()),
                item: None,
                status: None,
                session: None,
                message: Some(status.to_string()),
            },
        );
        // Remove the stored JoinHandle only after the final externally visible
        // side effect. Stop may otherwise miss this task and return while its
        // runFinished event is still pending.
        let mut runtime = state_inner.lock().await;
        if let Some(current) = runtime.as_mut() {
            current.active_runs.remove(&task_run_id);
        }
    });

    let mut task = Some(task);
    let insertion_error = {
        let mut runtime = state.inner.lock().await;
        match runtime.as_mut() {
            None => Some("Agent runtime is not running".to_string()),
            Some(current) => match ensure_runtime_account(current, &account_scope) {
                Err(error) => Some(error),
                Ok(()) => {
                    current.active_runs.insert(
                        run_id.clone(),
                        ActiveAgentRun {
                            token: cancel_token.clone(),
                            session_id: request.session_id.clone(),
                            task_handle: task.take().expect("task handle must be available"),
                        },
                    );
                    None
                }
            },
        }
    };
    if let Some(error) = insertion_error {
        let task = task.expect("failed insertion must retain task handle");
        task.abort();
        let _ = task.await;
        agent_manager
            .unregister_cancel_token(&request.session_id)
            .await;
        return Err(error);
    }
    emit_agent_event(
        &app_handle,
        AgentEventEnvelope {
            event_type: "runStarted".to_string(),
            session_id: Some(request.session_id.clone()),
            run_id: Some(run_id.clone()),
            item: None,
            status: None,
            session: None,
            message: None,
        },
    );

    record_and_emit_timeline_item(
        &app_handle,
        &state.live_timelines,
        &request.session_id,
        &run_id,
        user_item.clone(),
    )
    .await;
    let _ = start_tx.send(());
    // Keep the session claimed until the optimistic timeline item and start
    // signal are ordered. A cancellation cleanup must not finish and then be
    // followed by this send path re-appending the cancelled prompt.
    drop(session_lifecycle_guard);

    Ok(AgentRunResponse { run_id })
}

#[tauri::command]
pub async fn agent_cancel_run(
    app_handle: AppHandle,
    state: State<'_, AgentRuntimeState>,
    user_id: String,
    run_id: String,
) -> Result<(), String> {
    let account_scope = account_scope(&user_id)?;
    let generation = account_generation(&state, &account_scope).await;
    let _runtime_lifecycle_guard = state.runtime_lifecycle.lock().await;
    ensure_account_generation(&state, &account_scope, generation).await?;
    let (agent_manager, session_id, cancel_token) = {
        let runtime = state.inner.lock().await;
        let Some(current) = runtime.as_ref() else {
            return Ok(());
        };
        ensure_runtime_account(current, &account_scope)?;
        let Some(active_run) = current.active_runs.get(&run_id) else {
            return Ok(());
        };
        (
            Arc::clone(&current.agent_manager),
            active_run.session_id.clone(),
            active_run.token.clone(),
        )
    };
    cancel_token.cancel();
    let cancelled_permissions = cancel_pending_permissions_for_sessions(
        &agent_manager,
        &state.pending_permissions,
        std::slice::from_ref(&session_id),
    )
    .await;
    for (request_id, session_id) in cancelled_permissions {
        if let Some(item) = update_live_permission_status(
            &state.live_timelines,
            &session_id,
            &request_id,
            "cancelled",
        )
        .await
        {
            emit_timeline_item(&app_handle, &session_id, &run_id, item);
        }
    }
    Ok(())
}

#[tauri::command]
pub async fn agent_set_permission_mode(
    app_handle: AppHandle,
    state: State<'_, AgentRuntimeState>,
    user_id: String,
    request: AgentPermissionModeRequest,
) -> Result<(), String> {
    let account_scope = account_scope(&user_id)?;
    let generation = account_generation(&state, &account_scope).await;
    let _runtime_lifecycle_guard = state.runtime_lifecycle.lock().await;
    ensure_account_generation(&state, &account_scope, generation).await?;

    let session_id = request.session_id.trim().to_string();
    if session_id.is_empty() {
        return Err("Agent permission mode update requires a session ID".to_string());
    }
    let goose_mode = parse_user_permission_mode(&request.mode)?;
    let (agent_manager, session_manager, permission_modes) = {
        let runtime = state.inner.lock().await;
        let current = runtime
            .as_ref()
            .ok_or_else(|| "Agent runtime is not running".to_string())?;
        ensure_runtime_account(current, &account_scope)?;
        (
            Arc::clone(&current.agent_manager),
            Arc::clone(&current.session_manager),
            Arc::clone(&current.permission_modes),
        )
    };

    // Restrictive transitions take effect before any fallible Goose or disk
    // work. Otherwise the selector could say Read only while a still-live Auto
    // policy approves the next write. If setup fails, restore the previous
    // policy so the command and optimistic UI can roll back consistently.
    let previous_restrictive_mode = if goose_mode == GooseMode::SmartApprove {
        permission_modes
            .lock()
            .await
            .insert(session_id.clone(), goose_mode)
    } else {
        None
    };
    let update_result: Result<Arc<Agent>, String> = async {
        let agent = agent_manager
            .get_or_create_agent(session_id.clone())
            .await
            .map_err(|error| format!("Failed to resolve Goose agent for mode update: {error}"))?;
        agent
            .update_goose_mode(GOOSE_PERMISSION_ROUTING_MODE, &session_id)
            .await
            .map_err(|error| format!("Failed to update Goose mode: {error}"))?;
        // update_goose_mode already persists SmartApprove, which is both our
        // internal Goose routing mode and the user-facing Read-only mode. Auto
        // is Maple-owned, so only that case needs a second persistence step.
        // Keeping Read-only to one write avoids a failed duplicate write
        // leaving the persisted session stricter than the live Maple policy.
        if goose_mode == GooseMode::Auto {
            session_manager
                .update(&session_id)
                .goose_mode(goose_mode)
                .apply()
                .await
                .map_err(|error| format!("Failed to persist Agent permission mode: {error}"))?;
        }
        Ok(agent)
    }
    .await;
    let agent = match update_result {
        Ok(agent) => agent,
        Err(error) => {
            if goose_mode == GooseMode::SmartApprove {
                let mut modes = permission_modes.lock().await;
                match previous_restrictive_mode {
                    Some(previous) => {
                        modes.insert(session_id.clone(), previous);
                    }
                    None => {
                        modes.remove(&session_id);
                    }
                }
            }
            return Err(error);
        }
    };
    if goose_mode == GooseMode::Auto {
        permission_modes
            .lock()
            .await
            .insert(session_id.clone(), goose_mode);
    }
    {
        let mut runtime = state.inner.lock().await;
        let current = runtime
            .as_mut()
            .ok_or_else(|| "Agent runtime is not running".to_string())?;
        ensure_runtime_account(current, &account_scope)?;
        current.mode = request.mode.clone();
    }

    if goose_mode == GooseMode::Auto {
        let request_ids = {
            let mut pending = state.pending_permissions.lock().await;
            let request_ids = pending
                .keys()
                .filter(|(pending_session_id, _)| pending_session_id == &session_id)
                .map(|(_, request_id)| request_id.clone())
                .collect::<Vec<_>>();
            for request_id in &request_ids {
                pending.remove(&(session_id.clone(), request_id.clone()));
            }
            request_ids
        };
        for request_id in request_ids {
            deliver_tool_permission(&agent, request_id.clone(), Permission::AllowOnce).await;
            if let Some(item) = update_live_permission_status(
                &state.live_timelines,
                &session_id,
                &request_id,
                "allow_once",
            )
            .await
            {
                emit_agent_event(
                    &app_handle,
                    AgentEventEnvelope {
                        event_type: "timelineItem".to_string(),
                        session_id: Some(session_id.clone()),
                        run_id: None,
                        item: Some(item),
                        status: None,
                        session: None,
                        message: None,
                    },
                );
            }
        }
    }

    // The policy is already committed at this point. A best-effort refresh
    // must not report failure to the selector and make it roll back to a mode
    // that is no longer authoritative.
    match session_manager.get_session(&session_id, false).await {
        Ok(session) => emit_agent_event(
            &app_handle,
            AgentEventEnvelope {
                event_type: "sessionUpdated".to_string(),
                session_id: Some(session_id),
                run_id: None,
                item: None,
                status: None,
                session: Some(session_summary(&session)),
                message: None,
            },
        ),
        Err(error) => log::warn!(
            "Agent permission mode was updated, but the refreshed session could not be loaded: {error}"
        ),
    }
    Ok(())
}

#[tauri::command]
pub async fn agent_permission_respond(
    app_handle: AppHandle,
    state: State<'_, AgentRuntimeState>,
    user_id: String,
    response: AgentPermissionResponse,
) -> Result<(), String> {
    let account_scope = account_scope(&user_id)?;
    let generation = account_generation(&state, &account_scope).await;
    let _runtime_lifecycle_guard = state.runtime_lifecycle.lock().await;
    ensure_account_generation(&state, &account_scope, generation).await?;
    let (agent_manager, session_id) = {
        let runtime = state.inner.lock().await;
        let current = runtime
            .as_ref()
            .ok_or_else(|| "Agent runtime is not running".to_string())?;
        ensure_runtime_account(current, &account_scope)?;
        let session_id = response.session_id.trim().to_string();
        if session_id.is_empty() {
            return Err("Agent permission response requires a session ID".to_string());
        }
        let key = (session_id.clone(), response.request_id.clone());
        if !state.pending_permissions.lock().await.contains_key(&key) {
            return Err(format!(
                "No pending Agent Mode permission request found for {} in session {}",
                response.request_id, session_id
            ));
        }
        (Arc::clone(&current.agent_manager), session_id)
    };
    let agent = agent_manager
        .get_or_create_agent(session_id.clone())
        .await
        .map_err(|e| format!("Failed to resolve Goose agent for permission response: {e}"))?;
    agent
        .handle_confirmation(
            response.request_id.clone(),
            PermissionConfirmation {
                principal_type: PrincipalType::Tool,
                permission: permission_from_decision(&response.decision)?,
            },
        )
        .await;
    if let Some(item) = update_live_permission_status(
        &state.live_timelines,
        &session_id,
        &response.request_id,
        &response.decision,
    )
    .await
    {
        emit_agent_event(
            &app_handle,
            AgentEventEnvelope {
                event_type: "timelineItem".to_string(),
                session_id: Some(session_id.clone()),
                run_id: None,
                item: Some(item),
                status: None,
                session: None,
                message: None,
            },
        );
    }
    state
        .pending_permissions
        .lock()
        .await
        .remove(&(session_id, response.request_id));
    Ok(())
}

struct AgentPromptRun {
    app_handle: AppHandle,
    agent: Arc<Agent>,
    session_manager: Arc<SessionManager>,
    live_timelines: LiveTimelines,
    session_id: String,
    run_id: String,
    user_message: Message,
    permission_modes: SessionPermissionModes,
    cancel_token: CancellationToken,
    pending_permissions: PendingPermissions,
}

#[derive(Default)]
struct AgentPromptOutcome {
    terminal_message: Option<LiveMessageCandidate>,
}

#[derive(Clone, Debug, PartialEq)]
struct LiveMessageCandidate {
    id: Option<String>,
    role: String,
    created: i64,
    items: Vec<AgentTimelineItem>,
}

fn apply_successful_prompt_outcome(
    timelines: &mut HashMap<String, LiveTimeline>,
    session_id: &str,
    outcome: &AgentPromptOutcome,
) {
    match outcome.terminal_message.as_ref() {
        Some(candidate) => {
            timelines.insert(
                session_id.to_string(),
                LiveTimeline::Completed(candidate.clone()),
            );
        }
        None => {
            timelines.remove(session_id);
        }
    }
}

fn apply_failed_prompt_outcome(
    timelines: &mut HashMap<String, LiveTimeline>,
    session_id: &str,
    item: AgentTimelineItem,
) {
    timelines.insert(session_id.to_string(), LiveTimeline::Failed(vec![item]));
}

async fn selected_permission_mode(
    permission_modes: &SessionPermissionModes,
    session_id: &str,
) -> GooseMode {
    permission_modes
        .lock()
        .await
        .get(session_id)
        .copied()
        .unwrap_or(GOOSE_PERMISSION_ROUTING_MODE)
}

fn select_session_permission_mode(
    permission_modes: &mut HashMap<String, GooseMode>,
    session_id: &str,
    requested_mode: GooseMode,
) -> (GooseMode, bool) {
    if let Some(mode) = permission_modes.get(session_id).copied() {
        (mode, false)
    } else {
        permission_modes.insert(session_id.to_string(), requested_mode);
        (requested_mode, true)
    }
}

async fn deliver_tool_permission(agent: &Agent, request_id: String, permission: Permission) {
    agent
        .handle_confirmation(
            request_id,
            PermissionConfirmation {
                principal_type: PrincipalType::Tool,
                permission,
            },
        )
        .await;
}

async fn deliver_tool_permission_if_auto(
    agent: &Agent,
    session_id: &str,
    permission_modes: &SessionPermissionModes,
    request_id: &str,
    cancel_token: &CancellationToken,
) -> bool {
    // Keep the policy lock through confirmation delivery. This is the
    // linearization point for Auto -> Read only: once the restrictive mode
    // command returns, no permission decision based on an older Auto snapshot
    // can still be delivered.
    let modes = permission_modes.lock().await;
    if modes
        .get(session_id)
        .copied()
        .unwrap_or(GOOSE_PERMISSION_ROUTING_MODE)
        != GooseMode::Auto
    {
        return false;
    }
    let permission = if cancel_token.is_cancelled() {
        Permission::Cancel
    } else {
        Permission::AllowOnce
    };
    deliver_tool_permission(agent, request_id.to_string(), permission).await;
    drop(modes);
    true
}

async fn claim_pending_permission_if_auto(
    agent: &Agent,
    session_id: &str,
    permission_modes: &SessionPermissionModes,
    pending_permissions: &PendingPermissions,
    request_id: &str,
    cancel_token: &CancellationToken,
) -> bool {
    // This is the same Auto -> Read only linearization boundary as the direct
    // path above, with the pending request claimed while the policy is locked.
    let modes = permission_modes.lock().await;
    if modes
        .get(session_id)
        .copied()
        .unwrap_or(GOOSE_PERMISSION_ROUTING_MODE)
        != GooseMode::Auto
    {
        return false;
    }
    let claimed = pending_permissions
        .lock()
        .await
        .remove(&(session_id.to_string(), request_id.to_string()))
        .is_some();
    if claimed {
        let permission = if cancel_token.is_cancelled() {
            Permission::Cancel
        } else {
            Permission::AllowOnce
        };
        deliver_tool_permission(agent, request_id.to_string(), permission).await;
    }
    drop(modes);
    true
}

async fn automatically_handle_permissions(
    agent: &Agent,
    session_id: &str,
    permission_modes: &SessionPermissionModes,
    working_dir: &Path,
    message: &Message,
    cancel_token: &CancellationToken,
) -> HashSet<String> {
    let classifier = ShellPermissionClassifier;
    let mut handled = HashSet::new();

    for content in &message.content {
        let MessageContent::ActionRequired(action) = content else {
            continue;
        };
        let tool_request_id = match &action.data {
            ActionRequiredData::ToolConfirmation { id, .. } => Some(id.clone()),
            _ => None,
        };
        if let Some(request_id) = tool_request_id.as_ref() {
            if deliver_tool_permission_if_auto(
                agent,
                session_id,
                permission_modes,
                request_id,
                cancel_token,
            )
            .await
            {
                let request_id = request_id.clone();
                handled.insert(request_id);
                continue;
            }
        }
        let current_mode = selected_permission_mode(permission_modes, session_id)
            .await
            .to_string();
        if let Some(request_id) = local_read_request_id(&current_mode, action)
            .or_else(|| local_read_image_request_id(&current_mode, action))
            .map(str::to_string)
        {
            let permission = if cancel_token.is_cancelled() {
                Permission::Cancel
            } else {
                log::info!("Auto-approved local Agent Mode file read request {request_id}");
                Permission::AllowOnce
            };
            deliver_tool_permission(agent, request_id.clone(), permission).await;
            handled.insert(request_id);
            continue;
        }
        let Some(request) = ShellPermissionRequest::from_action(&current_mode, working_dir, action)
        else {
            if let Some(request_id) = tool_request_id {
                if deliver_tool_permission_if_auto(
                    agent,
                    session_id,
                    permission_modes,
                    &request_id,
                    cancel_token,
                )
                .await
                {
                    handled.insert(request_id);
                }
            }
            continue;
        };
        let request_id = request.request_id().to_string();
        let outcome = classifier
            .classify(agent, session_id, &request, cancel_token)
            .await;
        if deliver_tool_permission_if_auto(
            agent,
            session_id,
            permission_modes,
            &request_id,
            cancel_token,
        )
        .await
        {
            handled.insert(request_id);
            continue;
        }
        let permission = if cancel_token.is_cancelled() {
            Permission::Cancel
        } else {
            match outcome {
                ShellPermissionOutcome::ReadOnly => {
                    log::info!("Auto-approved read-only Agent Mode shell request {request_id}");
                    Permission::AllowOnce
                }
                ShellPermissionOutcome::Cancelled => Permission::Cancel,
                ShellPermissionOutcome::RequiresApproval => continue,
            }
        };

        deliver_tool_permission(agent, request_id.clone(), permission).await;
        handled.insert(request_id);
    }

    handled
}

async fn run_agent_prompt(run: AgentPromptRun) -> Result<AgentPromptOutcome, String> {
    let AgentPromptRun {
        app_handle,
        agent,
        session_manager,
        live_timelines,
        session_id,
        run_id,
        user_message,
        permission_modes,
        cancel_token,
        pending_permissions,
    } = run;
    let mut terminal_message = None;
    let session_config = SessionConfig {
        id: session_id.clone(),
        schedule_id: None,
        max_turns: None,
        retry_config: None,
    };
    let mut stream = agent
        .reply(user_message, session_config, Some(cancel_token.clone()))
        .await
        .map_err(|e| format!("Goose reply failed: {e}"))?;
    let updated_session = session_manager
        .get_session(&session_id, false)
        .await
        .map_err(|e| format!("Failed to load updated Goose session: {e}"))?;
    let working_dir = updated_session.working_dir.clone();
    emit_agent_event(
        &app_handle,
        AgentEventEnvelope {
            event_type: "sessionUpdated".to_string(),
            session_id: Some(session_id.clone()),
            run_id: Some(run_id.clone()),
            item: None,
            status: None,
            session: Some(session_summary(&updated_session)),
            message: None,
        },
    );

    while let Some(event) = stream.next().await {
        match event {
            Ok(AgentEvent::Message(message)) => {
                let automatically_handled = automatically_handle_permissions(
                    &agent,
                    &session_id,
                    &permission_modes,
                    &working_dir,
                    &message,
                    &cancel_token,
                )
                .await;
                let mut items = message_to_timeline_items(&message, true);
                items.retain(|item| {
                    pending_permission_request_id(item)
                        .is_none_or(|request_id| !automatically_handled.contains(&request_id))
                });
                let mut newly_auto_handled = HashSet::new();
                for item in &mut items {
                    if let Some(request_id) = pending_permission_request_id(item) {
                        if !register_pending_permission(
                            &pending_permissions,
                            &request_id,
                            &session_id,
                            &cancel_token,
                        )
                        .await
                        {
                            agent
                                .handle_confirmation(
                                    request_id,
                                    PermissionConfirmation {
                                        principal_type: PrincipalType::Tool,
                                        permission: Permission::Cancel,
                                    },
                                )
                                .await;
                            item.status = Some("cancelled".to_string());
                        } else if claim_pending_permission_if_auto(
                            &agent,
                            &session_id,
                            &permission_modes,
                            &pending_permissions,
                            &request_id,
                            &cancel_token,
                        )
                        .await
                        {
                            newly_auto_handled.insert(request_id);
                        }
                    }
                }
                items.retain(|item| {
                    pending_permission_request_id(item)
                        .is_none_or(|request_id| !newly_auto_handled.contains(&request_id))
                });
                // Publish a permission card while holding the same claim lock
                // used by an Allow-all transition. If that transition already
                // drained the request, suppress the now-non-actionable card; if
                // this path wins, the transition will immediately replace the
                // published card with its allowed status.
                let pending_publication_guard = if items
                    .iter()
                    .any(|item| pending_permission_request_id(item).is_some())
                {
                    Some(pending_permissions.lock().await)
                } else {
                    None
                };
                if let Some(pending) = pending_publication_guard.as_ref() {
                    items.retain(|item| {
                        pending_permission_request_id(item).is_none_or(|request_id| {
                            pending.contains_key(&(session_id.clone(), request_id))
                        })
                    });
                }
                if !items.is_empty() {
                    terminal_message = Some(update_live_message_candidate(
                        terminal_message,
                        &message,
                        &items,
                    ));
                }
                for item in items {
                    record_and_emit_timeline_item(
                        &app_handle,
                        &live_timelines,
                        &session_id,
                        &run_id,
                        item,
                    )
                    .await;
                }
                drop(pending_publication_guard);
            }
            // Usage ledgers remain in Goose's persisted messages for context
            // accounting, but Agent Mode does not render ephemeral token rows.
            Ok(AgentEvent::Usage(_) | AgentEvent::MessageUsage { .. }) => {}
            // Developer/MCP notifications are transport diagnostics. Tool
            // requests, results, permissions, and failures arrive as messages
            // and form the stable user-facing timeline.
            Ok(AgentEvent::McpNotification(_)) => {}
            Ok(AgentEvent::HistoryReplaced(conversation)) => {
                terminal_message = None;
                reseed_live_timeline_after_history_replaced(
                    &live_timelines,
                    &session_id,
                    &conversation,
                )
                .await;
                emit_agent_event(
                    &app_handle,
                    AgentEventEnvelope {
                        event_type: "historyReplaced".to_string(),
                        session_id: Some(session_id.clone()),
                        run_id: Some(run_id.clone()),
                        item: None,
                        status: None,
                        session: None,
                        message: None,
                    },
                );
            }
            Err(error) => {
                return Err(format!("Goose stream failed: {error}"));
            }
        }
        if cancel_token.is_cancelled() {
            break;
        }
    }

    Ok(AgentPromptOutcome { terminal_message })
}

fn live_message_candidate(message: &Message, items: &[AgentTimelineItem]) -> LiveMessageCandidate {
    LiveMessageCandidate {
        id: message.id.clone(),
        role: message_role(message),
        created: message.created,
        items: coalesce_timeline_items(items.to_vec()),
    }
}

fn update_live_message_candidate(
    current: Option<LiveMessageCandidate>,
    message: &Message,
    items: &[AgentTimelineItem],
) -> LiveMessageCandidate {
    let role = message_role(message);
    // Provider stream chunks have a stable ID. Id-less Goose messages are
    // complete logical events and may share the same second-resolution
    // timestamp, so combining them would conflate a reply with a later notice.
    let Some(mut current) = current.filter(|current| {
        current.id.is_some()
            && current.id == message.id
            && current.role == role
            && current.items.iter().all(|item| item.item_type != "system")
            && items.iter().all(|item| item.item_type != "system")
    }) else {
        return live_message_candidate(message, items);
    };

    for item in items {
        current.items = merge_timeline_item(current.items, item.clone());
    }
    current
}

fn timeline_item_matches(
    live: &AgentTimelineItem,
    persisted: &AgentTimelineItem,
    match_id: bool,
) -> bool {
    (!match_id || live.id == persisted.id)
        && live.item_type == persisted.item_type
        && live.role == persisted.role
        && live.title == persisted.title
        && live.text == persisted.text
        && live.status == persisted.status
        && live.input == persisted.input
        && live.output == persisted.output
}

fn terminal_message_is_persisted(
    conversation: &Conversation,
    candidate: &LiveMessageCandidate,
) -> bool {
    let messages = conversation.messages();
    let current_turn_start = messages
        .iter()
        .rposition(|message| {
            let role = message_role(message);
            is_real_user_message(message, &role)
        })
        .unwrap_or(0);
    let turn_messages = &messages[current_turn_start..];
    if let Some(id) = candidate.id.as_deref() {
        let mut persisted_items = Vec::new();
        for message in turn_messages.iter().filter(|message| {
            message_role(message) == candidate.role && message.id.as_deref() == Some(id)
        }) {
            for item in message_to_timeline_items(message, true) {
                persisted_items = merge_timeline_item(persisted_items, item);
            }
        }
        return timeline_projection_matches(&candidate.items, &persisted_items, true);
    }

    turn_messages
        .iter()
        .filter(|message| {
            message_role(message) == candidate.role && message.created == candidate.created
        })
        .any(|message| {
            let persisted_items = coalesce_timeline_items(message_to_timeline_items(message, true));
            timeline_projection_matches(&candidate.items, &persisted_items, false)
        })
}

fn timeline_projection_matches(
    live: &[AgentTimelineItem],
    persisted: &[AgentTimelineItem],
    match_id: bool,
) -> bool {
    live.len() == persisted.len()
        && live
            .iter()
            .zip(persisted)
            .all(|(live, persisted)| timeline_item_matches(live, persisted, match_id))
}

fn bounded_timeline_text(value: &str, max_chars: usize) -> String {
    let mut chars = value.chars();
    let bounded = chars.by_ref().take(max_chars).collect::<String>();
    if chars.next().is_some() {
        format!("{bounded}…")
    } else {
        bounded
    }
}

fn pending_permission_request_id(item: &AgentTimelineItem) -> Option<String> {
    if item.item_type == "permission" {
        return item
            .id
            .strip_prefix("permission-")
            .filter(|request_id| !request_id.is_empty())
            .map(ToString::to_string);
    }
    None
}

async fn configure_session_agent(
    agent_manager: &Arc<AgentManager>,
    session_manager: &Arc<SessionManager>,
    session: &Session,
    model: &str,
    mode: &str,
) -> Result<Arc<Agent>, String> {
    let agent = agent_manager
        .get_or_create_agent(session.id.clone())
        .await
        .map_err(|e| format!("Failed to get Goose agent for session {}: {e}", session.id))?;
    let provider = goose::providers::create_with_working_dir(
        "openai",
        Vec::new(),
        session.working_dir.clone(),
    )
    .await
    .map_err(|e| format!("Failed to create Goose OpenAI provider: {e}"))?;
    let model_config = goose::model_config::model_config_from_user_config("openai", model)
        .map_err(|e| format!("Failed to configure Goose model {model}: {e}"))?;
    agent
        .update_provider(provider, model_config, &session.id)
        .await
        .map_err(|e| format!("Failed to update Goose provider: {e}"))?;
    agent
        .update_goose_mode(GOOSE_PERMISSION_ROUTING_MODE, &session.id)
        .await
        .map_err(|e| format!("Failed to configure Goose permission routing: {e}"))?;
    let developer = ExtensionConfig::Builtin {
        name: "developer".to_string(),
        description: DEFAULT_EXTENSION_DESCRIPTION.to_string(),
        display_name: Some("Developer".to_string()),
        timeout: Some(DEFAULT_EXTENSION_TIMEOUT),
        bundled: Some(true),
        available_tools: MAPLE_DEVELOPER_TOOLS
            .iter()
            .map(|tool| tool.to_string())
            .collect(),
    };
    let developer_client = MapleDeveloperClient::new(agent.extension_manager.get_context().clone())
        .map_err(|e| format!("Failed to create Maple developer tools: {e}"))?;
    agent
        .extension_manager
        .add_client(
            "developer".to_string(),
            developer,
            Arc::new(developer_client),
            None,
            None,
        )
        .await;
    agent
        .persist_extension_state(&session.id)
        .await
        .map_err(|e| format!("Failed to persist Maple developer tools: {e}"))?;
    // Goose's live mode remains SmartApprove so every sensitive call reaches Maple.
    // Persist the user-facing policy separately for session restoration and display.
    session_manager
        .update(&session.id)
        .goose_mode(parse_goose_mode(mode))
        .apply()
        .await
        .map_err(|e| format!("Failed to persist Agent permission mode: {e}"))?;
    Ok(agent)
}

#[derive(Default)]
struct ConversationTimelineProjectionState {
    surfaced_thinking_in_inference: bool,
}

/// Project a stored Goose conversation into Maple's presentation timeline.
///
/// Goose deliberately repeats reasoning blocks on each split tool-request
/// message. That replay belongs in the provider history, but it is not a second
/// user-visible thought. Keep this normalization local to a single conversation
/// so concurrent Agent sessions cannot affect one another and the
/// persisted/provider-facing history remains byte-for-byte unchanged.
fn conversation_to_timeline_items(conversation: &Conversation) -> Vec<AgentTimelineItem> {
    let mut state = ConversationTimelineProjectionState::default();
    let mut items = Vec::new();
    let messages = conversation.messages();

    for (index, message) in messages.iter().enumerate() {
        let role = message_role(message);
        let assistant = role == "assistant";
        let inference_ends = assistant && message.metadata.usage.is_some();

        // A real user message starts a new user turn. Tool responses are
        // intentionally chain-neutral because Goose interleaves them between
        // split requests from the same turn.
        if is_real_user_message(message, &role) {
            state.surfaced_thinking_in_inference = false;
        }

        // Match Goose's own session presentation contract: agent-only grind,
        // retry, goal, and other internal messages stay in provider history but
        // never become user-facing Maple timeline rows.
        if !message.is_user_visible() {
            if inference_ends {
                state.surfaced_thinking_in_inference = false;
            }
            continue;
        }

        let mut thinking = message_thinking_projection(message);
        let has_tool_request = message.content.iter().any(|content| {
            matches!(
                content,
                MessageContent::ToolRequest(_) | MessageContent::FrontendToolRequest(_)
            )
        });

        // Goose intentionally copies reasoning onto every persisted split
        // tool-request message for provider history. Its live AgentEvent stream
        // emits that reasoning only once per provider inference. Reconstruct the
        // same presentation boundary from the usage ledger Goose attaches to the
        // inference's final assistant message. If no ledger boundary is reachable
        // before the next real user turn, preserve every block rather than guess.
        // Replace this reconstruction if Goose adds an explicit persisted
        // inference ID or replay marker to its public message contract.
        let has_usage_boundary =
            assistant && provider_inference_has_usage_boundary(&messages[index..]);
        if assistant
            && has_tool_request
            && state.surfaced_thinking_in_inference
            && has_usage_boundary
        {
            thinking = None;
        } else if assistant && thinking.is_some() {
            state.surfaced_thinking_in_inference = true;
        }

        items.extend(message_to_timeline_items_with_thinking(
            message,
            false,
            thinking.as_deref(),
        ));

        if inference_ends {
            state.surfaced_thinking_in_inference = false;
        }
    }

    coalesce_timeline_items(items)
}

fn is_real_user_message(message: &Message, role: &str) -> bool {
    role == "user"
        && message
            .content
            .iter()
            .any(|content| !matches!(content, MessageContent::ToolResponse(_)))
}

fn provider_inference_has_usage_boundary(messages: &[Message]) -> bool {
    for message in messages {
        let role = message_role(message);
        if is_real_user_message(message, &role) {
            return false;
        }
        if role == "assistant" && message.metadata.usage.is_some() {
            return true;
        }
    }
    false
}

fn message_thinking_projection(message: &Message) -> Option<String> {
    // Match Goose Desktop's ACP adapter: concatenate adjacent thought chunks
    // by message without rewriting their text. The frontend decides whether
    // the fully merged thought is renderable, so a streamed punctuation or
    // whitespace suffix is never lost.
    let mut text = String::new();
    let mut found = false;

    for content in &message.content {
        match content {
            MessageContent::Thinking(thinking) => {
                found = true;
                text.push_str(&thinking.thinking);
            }
            MessageContent::RedactedThinking(_) => {
                found = true;
                text.push_str("Thinking redacted by provider.");
            }
            _ => {}
        }
    }
    found.then_some(text)
}

fn message_to_timeline_items(message: &Message, live: bool) -> Vec<AgentTimelineItem> {
    if !message.is_user_visible() {
        return Vec::new();
    }
    let thinking = message_thinking_projection(message);
    message_to_timeline_items_with_thinking(message, live, thinking.as_deref())
}

fn message_to_timeline_items_with_thinking(
    message: &Message,
    live: bool,
    thinking: Option<&str>,
) -> Vec<AgentTimelineItem> {
    let role = message_role(message);
    let base_id = message
        .id
        .clone()
        .unwrap_or_else(|| format!("message-{}-{}", role, message.created));
    let created_ms = if message.created > 0 {
        (message.created as u128) * 1000
    } else {
        unix_ms()
    };
    let merge = if live { "append" } else { "replace" }.to_string();

    let mut emitted_thinking = false;
    message
        .content
        .iter()
        .enumerate()
        .filter_map(|(index, content)| match content {
            MessageContent::Text(text) => Some(AgentTimelineItem {
                id: format!("{base_id}-text"),
                item_type: "message".to_string(),
                role: Some(role.clone()),
                title: None,
                text: Some(text.text.clone()),
                status: None,
                input: None,
                output: None,
                created_ms,
                merge: merge.clone(),
            }),
            MessageContent::Thinking(_) | MessageContent::RedactedThinking(_) => {
                if emitted_thinking {
                    return None;
                }
                emitted_thinking = true;
                thinking.map(|thinking| AgentTimelineItem {
                    id: format!("{base_id}-thinking"),
                    item_type: "thinking".to_string(),
                    role: Some("thought".to_string()),
                    title: Some("Thinking".to_string()),
                    text: Some(thinking.to_string()),
                    status: None,
                    input: None,
                    output: None,
                    created_ms,
                    merge: merge.clone(),
                })
            }
            MessageContent::ToolRequest(request) => Some(tool_request_item(request, created_ms)),
            MessageContent::ToolResponse(response) => {
                Some(tool_response_item(response, created_ms))
            }
            MessageContent::ToolConfirmationRequest(request) => Some(AgentTimelineItem {
                id: format!("permission-{}", request.id),
                item_type: "permission".to_string(),
                role: Some("system".to_string()),
                title: Some(format_tool_title(&request.tool_name)),
                text: request.prompt.clone(),
                status: Some("pending".to_string()),
                input: Some(Value::Object(request.arguments.clone())),
                output: None,
                created_ms,
                merge: "replace".to_string(),
            }),
            MessageContent::ActionRequired(action) => {
                Some(action_required_item(action, created_ms))
            }
            MessageContent::FrontendToolRequest(request) => {
                let (title, text, input, status) = match &request.tool_call {
                    Ok(call) => (
                        format_tool_title(call.name.as_ref()),
                        None,
                        Some(serde_json::to_value(&call.arguments).unwrap_or(Value::Null)),
                        "pending".to_string(),
                    ),
                    Err(error) => (
                        "Tool call parse failed".to_string(),
                        Some(bounded_timeline_text(
                            &error.to_string(),
                            MAX_AGENT_ERROR_CHARS,
                        )),
                        None,
                        "failed".to_string(),
                    ),
                };
                Some(AgentTimelineItem {
                    id: request.id.clone(),
                    item_type: "tool".to_string(),
                    role: Some("assistant".to_string()),
                    title: Some(title),
                    text,
                    status: Some(status),
                    input,
                    output: None,
                    created_ms,
                    merge: "replace".to_string(),
                })
            }
            MessageContent::SystemNotification(notification) => Some(system_notification_item(
                &base_id,
                index,
                notification,
                created_ms,
            )),
            // Images are provider-history payloads, not timeline events. The
            // read_image tool request/result already gives users the useful,
            // bounded presentation without exposing base64 metadata.
            MessageContent::Image(_) => None,
        })
        .collect()
}

fn system_notification_item(
    base_id: &str,
    index: usize,
    notification: &SystemNotificationContent,
    created_ms: u128,
) -> AgentTimelineItem {
    let title = match notification.notification_type {
        SystemNotificationType::ThinkingMessage => "Thinking",
        SystemNotificationType::ProgressMessage => "Progress",
        SystemNotificationType::InlineMessage => "Agent notice",
        SystemNotificationType::CreditsExhausted => "Credits exhausted",
    };
    AgentTimelineItem {
        id: format!("{base_id}-system-{index}"),
        item_type: "system".to_string(),
        role: Some("system".to_string()),
        title: Some(title.to_string()),
        text: Some(bounded_timeline_text(&notification.msg, 500)),
        status: None,
        input: None,
        // Provider-specific structured data can contain raw request or model
        // payloads. The stable title/message above is the user-facing contract.
        output: None,
        created_ms,
        merge: "replace".to_string(),
    }
}

fn tool_request_item(
    request: &goose::conversation::message::ToolRequest,
    created_ms: u128,
) -> AgentTimelineItem {
    match &request.tool_call {
        Ok(call) => AgentTimelineItem {
            id: request.id.clone(),
            item_type: "tool".to_string(),
            role: Some("assistant".to_string()),
            title: Some(
                request
                    .persisted_title()
                    .unwrap_or_else(|| call.name.as_ref())
                    .to_string(),
            ),
            text: request
                .persisted_chain_summary()
                .map(|summary| summary.summary),
            status: Some("running".to_string()),
            input: Some(serde_json::to_value(&call.arguments).unwrap_or(Value::Null)),
            output: None,
            created_ms,
            merge: "replace".to_string(),
        },
        Err(error) => error_item(format!("Tool call parse failed: {error}")),
    }
}

fn tool_response_item(
    response: &goose::conversation::message::ToolResponse,
    created_ms: u128,
) -> AgentTimelineItem {
    match &response.tool_result {
        Ok(result) => {
            let text = result
                .content
                .iter()
                .filter_map(|content| content.as_text().map(|text| text.text.to_string()))
                .collect::<Vec<_>>()
                .join("\n");
            let content = result
                .content
                .iter()
                .map(summarize_tool_content)
                .collect::<Vec<_>>();
            AgentTimelineItem {
                id: response.id.clone(),
                item_type: "tool".to_string(),
                role: Some("assistant".to_string()),
                title: tool_name_from_id(&response.id).map(|name| format_tool_title(&name)),
                text: None,
                status: Some(
                    if result.is_error.unwrap_or(false) {
                        "failed"
                    } else {
                        "completed"
                    }
                    .to_string(),
                ),
                input: None,
                output: Some(json!({
                    "text": text,
                    "isError": result.is_error,
                    "structuredContent": result.structured_content,
                    "content": content,
                })),
                created_ms,
                merge: "replace".to_string(),
            }
        }
        Err(error) => AgentTimelineItem {
            id: response.id.clone(),
            item_type: "tool".to_string(),
            role: Some("assistant".to_string()),
            title: tool_name_from_id(&response.id).map(|name| format_tool_title(&name)),
            text: Some(bounded_timeline_text(
                &error.to_string(),
                MAX_AGENT_ERROR_CHARS,
            )),
            status: Some("failed".to_string()),
            input: None,
            output: None,
            created_ms,
            merge: "replace".to_string(),
        },
    }
}

fn summarize_tool_content(content: &rmcp::model::Content) -> Value {
    if let Some(text) = content.as_text() {
        return json!({
            "type": "text",
            "text": text.text,
        });
    }

    if let Some(image) = content.as_image() {
        return image_metadata_value(&image.mime_type, image.data.len());
    }

    json!({
        "type": "other",
        "dataOmitted": true,
    })
}

fn image_metadata_value(mime_type: &str, base64_chars: usize) -> Value {
    json!({
        "type": "image",
        "mimeType": mime_type,
        "base64Chars": base64_chars,
        "dataOmitted": true,
    })
}

fn coalesce_timeline_items(items: Vec<AgentTimelineItem>) -> Vec<AgentTimelineItem> {
    items.into_iter().fold(Vec::new(), merge_timeline_item)
}

fn merge_timeline_item(
    mut current: Vec<AgentTimelineItem>,
    incoming: AgentTimelineItem,
) -> Vec<AgentTimelineItem> {
    let Some(index) = current.iter().position(|item| item.id == incoming.id) else {
        current.push(incoming);
        return current;
    };

    let previous = current[index].clone();
    let append_text = incoming.merge == "append"
        && matches!(incoming.item_type.as_str(), "message" | "thinking")
        && incoming.text.is_some();

    current[index] = AgentTimelineItem {
        id: incoming.id,
        item_type: incoming.item_type,
        role: incoming.role.or(previous.role),
        title: incoming.title.or(previous.title),
        text: if append_text {
            Some(format!(
                "{}{}",
                previous.text.unwrap_or_default(),
                incoming.text.unwrap_or_default()
            ))
        } else {
            incoming.text.or(previous.text)
        },
        status: incoming.status.or(previous.status),
        input: incoming.input.or(previous.input),
        output: incoming.output.or(previous.output),
        created_ms: incoming.created_ms,
        merge: incoming.merge,
    };

    current
}

fn action_required_item(
    action: &goose::conversation::message::ActionRequired,
    created_ms: u128,
) -> AgentTimelineItem {
    match &action.data {
        ActionRequiredData::ToolConfirmation {
            id,
            tool_name,
            arguments,
            prompt,
        } => AgentTimelineItem {
            id: format!("permission-{id}"),
            item_type: "permission".to_string(),
            role: Some("system".to_string()),
            title: Some(format_tool_title(tool_name)),
            text: prompt.clone(),
            status: Some("pending".to_string()),
            input: Some(Value::Object(arguments.clone())),
            output: None,
            created_ms,
            merge: "replace".to_string(),
        },
        ActionRequiredData::Elicitation {
            id,
            message,
            requested_schema,
        } => AgentTimelineItem {
            id: format!("elicitation-{id}"),
            item_type: "permission".to_string(),
            role: Some("system".to_string()),
            title: Some("Input requested".to_string()),
            text: Some(message.clone()),
            status: Some("pending".to_string()),
            input: Some(requested_schema.clone()),
            output: None,
            created_ms,
            merge: "replace".to_string(),
        },
        ActionRequiredData::ElicitationResponse { id, .. } => AgentTimelineItem {
            id: format!("elicitation-response-{id}"),
            item_type: "system".to_string(),
            role: Some("system".to_string()),
            title: Some("Input response".to_string()),
            text: None,
            status: Some("completed".to_string()),
            input: None,
            output: None,
            created_ms,
            merge: "replace".to_string(),
        },
    }
}

fn error_item(message: String) -> AgentTimelineItem {
    AgentTimelineItem {
        id: format!("error-{}", unix_ms()),
        item_type: "error".to_string(),
        role: Some("system".to_string()),
        title: Some("Agent error".to_string()),
        text: Some(bounded_timeline_text(&message, MAX_AGENT_ERROR_CHARS)),
        status: Some("failed".to_string()),
        input: None,
        output: None,
        created_ms: unix_ms(),
        merge: "replace".to_string(),
    }
}

fn message_role(message: &Message) -> String {
    serde_json::to_value(&message.role)
        .ok()
        .and_then(|value| value.as_str().map(ToOwned::to_owned))
        .unwrap_or_else(|| format!("{:?}", message.role).to_lowercase())
}

fn format_tool_title(name: &str) -> String {
    let normalized = name.replace("__", ": ").replace('_', " ");
    normalized
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .trim()
        .to_string()
}

fn tool_name_from_id(id: &str) -> Option<String> {
    // Goose's `functions.<tool>:<sequence>` IDs encode a tool name. Provider
    // IDs such as `chatcmpl-tool-*` do not; returning a title for those would
    // overwrite the request's already-correct title during timeline merging.
    let name = id
        .strip_prefix("functions.")?
        .split(':')
        .next()
        .unwrap_or("")
        .trim();
    if name.is_empty() {
        None
    } else {
        Some(name.to_string())
    }
}

fn permission_from_decision(decision: &str) -> Result<Permission, String> {
    match decision {
        "allow_once" | "allow" => Ok(Permission::AllowOnce),
        "deny_once" | "deny" => Ok(Permission::DenyOnce),
        "cancel" => Ok(Permission::Cancel),
        "always_allow" | "always_deny" => {
            Err("Persistent tool permissions are not supported by Maple Agent Mode".to_string())
        }
        other => Err(format!("Unknown permission decision: {other}")),
    }
}

fn session_summary(session: &Session) -> AgentSessionSummary {
    AgentSessionSummary {
        id: session.id.clone(),
        title: session.name.clone(),
        project_root: path_string(&session.working_dir),
        created_ms: session.created_at.timestamp_millis(),
        updated_ms: session.updated_at.timestamp_millis(),
        message_count: session.message_count,
        model: session
            .model_config
            .as_ref()
            .map(|model| model.model_name.clone()),
        mode: session.goose_mode.to_string(),
    }
}

fn emit_timeline_item(
    app_handle: &AppHandle,
    session_id: &str,
    run_id: &str,
    item: AgentTimelineItem,
) {
    emit_agent_event(
        app_handle,
        AgentEventEnvelope {
            event_type: "timelineItem".to_string(),
            session_id: Some(session_id.to_string()),
            run_id: Some(run_id.to_string()),
            item: Some(item),
            status: None,
            session: None,
            message: None,
        },
    );
}

async fn record_and_emit_timeline_item(
    app_handle: &AppHandle,
    live_timelines: &LiveTimelines,
    session_id: &str,
    run_id: &str,
    item: AgentTimelineItem,
) {
    record_timeline_item(live_timelines, session_id, item.clone()).await;
    emit_timeline_item(app_handle, session_id, run_id, item);
}

async fn record_timeline_item(
    live_timelines: &LiveTimelines,
    session_id: &str,
    item: AgentTimelineItem,
) {
    let mut timelines = live_timelines.lock().await;
    let current = match timelines.remove(session_id) {
        Some(LiveTimeline::Streaming(items)) => items,
        // A real user message starts a new live suffix. The preceding terminal
        // row is either already persisted or was a one-turn-only error/notice;
        // carrying it forward could duplicate it on a mid-run session reload.
        Some(LiveTimeline::Completed(_) | LiveTimeline::Failed(_))
            if is_user_message_item(&item) =>
        {
            Vec::new()
        }
        Some(LiveTimeline::Completed(candidate)) => candidate.items,
        Some(LiveTimeline::Failed(items)) => items,
        None => Vec::new(),
    };
    timelines.insert(
        session_id.to_string(),
        LiveTimeline::Streaming(merge_timeline_item(current, item)),
    );
}

/// Goose replaces persisted history during compaction, so any live rows from
/// before that replacement are stale. Keep only the newest visible real-user
/// row as an ID boundary for later events in the still-running turn. A session
/// reload can then use Goose's live presentation suffix wholesale instead of
/// merging it with differently-IDed provider-history reasoning.
async fn reseed_live_timeline_after_history_replaced(
    live_timelines: &LiveTimelines,
    session_id: &str,
    conversation: &Conversation,
) {
    let replacement_boundary = conversation
        .messages()
        .iter()
        .rev()
        .find(|message| {
            let role = message_role(message);
            message.is_user_visible() && is_real_user_message(message, &role)
        })
        .and_then(|message| {
            coalesce_timeline_items(message_to_timeline_items(message, false))
                .into_iter()
                .find(is_user_message_item)
        });

    let mut timelines = live_timelines.lock().await;
    match replacement_boundary {
        Some(replacement_boundary) => {
            // Prefer the existing live representation, but only for the user
            // ID confirmed by Goose's replacement history. That preserves the
            // authoritative presentation item without retaining a boundary
            // that compaction or an explicit history command removed.
            let boundary = timelines
                .get(session_id)
                .and_then(|items| {
                    items.items().iter().rev().find(|item| {
                        is_user_message_item(item) && item.id == replacement_boundary.id
                    })
                })
                .cloned()
                .unwrap_or(replacement_boundary);
            timelines.insert(
                session_id.to_string(),
                LiveTimeline::Streaming(vec![boundary]),
            );
        }
        None => {
            timelines.remove(session_id);
        }
    }
}

async fn overlay_live_timeline(
    live_timelines: &LiveTimelines,
    session_id: &str,
    conversation: &Conversation,
    persisted: Vec<AgentTimelineItem>,
) -> Vec<AgentTimelineItem> {
    let live_items = {
        let mut timelines = live_timelines.lock().await;
        match timelines.get(session_id).cloned() {
            Some(LiveTimeline::Streaming(items)) => items,
            Some(LiveTimeline::Completed(candidate)) => {
                // agent_load_session already paid to load Goose history. Use
                // that snapshot here instead of deserializing it a second time
                // at the end of every prompt.
                if terminal_message_is_persisted(conversation, &candidate) {
                    timelines.remove(session_id);
                    Vec::new()
                } else {
                    candidate.items
                }
            }
            Some(LiveTimeline::Failed(items)) => items,
            None => Vec::new(),
        }
    };
    if live_items.is_empty() {
        return persisted;
    }

    overlay_live_timeline_items(persisted, live_items)
}

fn overlay_live_timeline_items(
    persisted: Vec<AgentTimelineItem>,
    live_items: Vec<AgentTimelineItem>,
) -> Vec<AgentTimelineItem> {
    // AgentEvent is Goose's authoritative presentation stream. Once its first
    // user boundary also exists in persisted history, keep only the persisted
    // prefix before that turn and use the live suffix wholesale. This avoids
    // matching or rewriting reasoning text when Goose's provider-history copy
    // has a different message ID from the live thought.
    let persisted_boundary = live_items
        .iter()
        .filter(|item| is_user_message_item(item))
        .find_map(|live_user| persisted.iter().position(|item| item.id == live_user.id));
    let mut timeline = match persisted_boundary {
        Some(index) => persisted[..index].to_vec(),
        None => persisted,
    };
    timeline.extend(live_items.into_iter().map(live_overlay_item));
    coalesce_timeline_items(timeline)
}

fn is_user_message_item(item: &AgentTimelineItem) -> bool {
    item.item_type == "message" && item.role.as_deref() == Some("user")
}

fn live_overlay_item(mut item: AgentTimelineItem) -> AgentTimelineItem {
    item.merge = "replace".to_string();
    item
}

async fn update_live_permission_status(
    live_timelines: &LiveTimelines,
    session_id: &str,
    request_id: &str,
    decision: &str,
) -> Option<AgentTimelineItem> {
    let permission_id = format!("permission-{request_id}");
    let mut timelines = live_timelines.lock().await;
    let items = timelines.get_mut(session_id)?.items_mut();
    let item = items.iter_mut().find(|item| item.id == permission_id)?;
    item.status = Some(decision.to_string());
    item.merge = "replace".to_string();
    Some(item.clone())
}

fn emit_agent_event(app_handle: &AppHandle, event: AgentEventEnvelope) {
    if let Err(error) = app_handle.emit(AGENT_EVENT_NAME, event) {
        log::warn!("Failed to emit Agent Mode event: {error}");
    }
}

fn configure_embedded_goose(
    goose_path_root: &Path,
    model: &str,
    mode: &str,
    maple_proxy_base_url: &str,
) -> Result<(), String> {
    fs::create_dir_all(goose_path_root.join("config"))
        .map_err(|e| format!("Failed to create Goose config dir: {e}"))?;
    fs::create_dir_all(goose_path_root.join("data"))
        .map_err(|e| format!("Failed to create Goose data dir: {e}"))?;
    fs::create_dir_all(goose_path_root.join("state"))
        .map_err(|e| format!("Failed to create Goose state dir: {e}"))?;

    std::env::set_var("GOOSE_PATH_ROOT", goose_path_root);
    // The embedded Goose provider talks only to Maple's loopback proxy. The
    // proxy owns upstream authentication, so Goose must not receive or persist
    // an API key of its own.
    std::env::remove_var("OPENAI_API_KEY");
    std::env::remove_var("GOOSE_DISABLE_KEYRING");
    std::env::remove_var("GOOSE_MAX_TOKENS");

    remove_maple_owned_goose_file(
        &goose_path_root.join("config").join("secrets.yaml"),
        "secrets",
    )?;
    let config = goose::config::Config::global();
    config.invalidate_secrets_cache();
    delete_goose_config_key(config, "GOOSE_DISABLE_KEYRING")?;
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

    set_owner_only_permissions(&goose_path_root.join("config").join("config.yaml"));
    Ok(())
}

fn delete_goose_config_key(config: &goose::config::Config, key: &str) -> Result<(), String> {
    match config.delete(key) {
        Ok(()) | Err(ConfigError::NotFound(_)) => Ok(()),
        Err(e) => Err(format!("Failed to clear Goose config key {key}: {e}")),
    }
}

fn remove_maple_owned_goose_file(path: &Path, description: &str) -> Result<(), String> {
    match fs::remove_file(path) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(format!(
            "Failed to remove Maple-owned Goose {description} file {}: {error}",
            path.display()
        )),
    }
}

fn reset_maple_owned_permission_file(path: &Path) -> Result<(), String> {
    fs::write(path, MAPLE_GOOSE_PERMISSION_CONFIG).map_err(|error| {
        format!(
            "Failed to reset Maple-owned Goose permission file {}: {error}",
            path.display()
        )
    })?;
    set_owner_only_permissions(path);
    Ok(())
}

fn parse_goose_mode(mode: &str) -> GooseMode {
    GooseMode::from_str(mode).unwrap_or(GooseMode::SmartApprove)
}

fn parse_user_permission_mode(mode: &str) -> Result<GooseMode, String> {
    match mode {
        "auto" => Ok(GooseMode::Auto),
        "smart_approve" => Ok(GooseMode::SmartApprove),
        _ => Err(format!("Unsupported Agent permission mode: {mode}")),
    }
}

fn stopped_status() -> AgentRuntimeStatus {
    AgentRuntimeStatus {
        running: false,
        project_root: None,
        model: None,
        mode: None,
        active_runs: HashMap::new(),
    }
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

fn agent_root_dir(app_handle: &AppHandle) -> Result<PathBuf, anyhow::Error> {
    let base = app_handle
        .path()
        .app_config_dir()
        .map_err(|error| anyhow::anyhow!("Failed to resolve app config dir: {error}"))?;
    let path = base.join("agent");
    fs::create_dir_all(&path)?;
    set_owner_only_dir_permissions(&path);
    Ok(path)
}

fn account_config_dir_path(
    app_handle: &AppHandle,
    user_id: &str,
) -> Result<PathBuf, anyhow::Error> {
    let scope = account_scope(user_id).map_err(anyhow::Error::msg)?;
    Ok(agent_root_dir(app_handle)?.join("accounts").join(scope))
}

fn agent_config_dir(app_handle: &AppHandle, user_id: &str) -> Result<PathBuf, anyhow::Error> {
    let path = account_config_dir_path(app_handle, user_id)?;
    fs::create_dir_all(&path)?;
    set_owner_only_dir_permissions(&path);
    Ok(path)
}

fn account_session_manager(
    app_handle: &AppHandle,
    user_id: &str,
) -> Result<Arc<SessionManager>, String> {
    let account_dir = agent_config_dir(app_handle, user_id).map_err(|error| error.to_string())?;
    session_manager_for_account_dir(&account_dir)
}

fn session_manager_for_account_dir(account_dir: &Path) -> Result<Arc<SessionManager>, String> {
    let data_dir = account_dir.join("goose/data");
    fs::create_dir_all(&data_dir)
        .map_err(|error| format!("Failed to create Goose data dir: {error}"))?;
    Ok(Arc::new(SessionManager::new(data_dir)))
}

fn clear_agent_history(account_dir: &Path) -> Result<(), anyhow::Error> {
    remove_agent_history_path(&account_dir.join("goose/data"))
}

fn remove_agent_history_path(path: &Path) -> Result<(), anyhow::Error> {
    let metadata = match fs::symlink_metadata(path) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(error) => return Err(error.into()),
    };
    let result = if metadata.file_type().is_dir() {
        fs::remove_dir_all(path)
    } else {
        fs::remove_file(path)
    };
    result.map_err(Into::into)
}
fn load_agent_config_inner(
    app_handle: &AppHandle,
    user_id: &str,
) -> Result<AgentConfig, anyhow::Error> {
    let path = agent_config_dir(app_handle, user_id)?.join("config.json");
    if !path.exists() {
        return Ok(AgentConfig::default());
    }
    let contents = fs::read_to_string(path)?;
    let mut config: AgentConfig = serde_json::from_str(&contents)?;
    if migrate_agent_config(&mut config) {
        save_agent_config_inner(app_handle, user_id, &config)?;
    }
    Ok(config)
}

fn migrate_agent_config(config: &mut AgentConfig) -> bool {
    if config.default_model != LEGACY_AGENT_DEFAULT_MODEL {
        return false;
    }
    config.default_model = default_agent_model();
    true
}

fn save_agent_config_inner(
    app_handle: &AppHandle,
    user_id: &str,
    config: &AgentConfig,
) -> Result<(), anyhow::Error> {
    let path = agent_config_dir(app_handle, user_id)?.join("config.json");
    write_json_file(&path, config)
}

fn load_recent_project_roots_inner(
    app_handle: &AppHandle,
    user_id: &str,
) -> Result<Vec<RecentProjectRoot>, anyhow::Error> {
    let path = agent_config_dir(app_handle, user_id)?.join("recent_roots.json");
    if !path.exists() {
        return Ok(Vec::new());
    }
    let contents = fs::read_to_string(path)?;
    Ok(serde_json::from_str(&contents)?)
}

fn save_recent_project_root_inner(
    app_handle: &AppHandle,
    user_id: &str,
    project_root: &Path,
) -> Result<Vec<RecentProjectRoot>, anyhow::Error> {
    let mut roots = load_recent_project_roots_inner(app_handle, user_id).unwrap_or_default();
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

    let file_path = agent_config_dir(app_handle, user_id)?.join("recent_roots.json");
    write_json_file(&file_path, &roots)?;
    Ok(roots)
}

fn has_active_session_run(active_runs: &HashMap<String, ActiveAgentRun>, session_id: &str) -> bool {
    active_runs.values().any(|run| run.session_id == session_id)
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

#[cfg(unix)]
fn set_owner_only_dir_permissions(path: &Path) {
    use std::os::unix::fs::PermissionsExt;
    let _ = fs::set_permissions(path, fs::Permissions::from_mode(0o700));
}

#[cfg(not(unix))]
fn set_owner_only_dir_permissions(_path: &Path) {}

fn unix_ms() -> u128 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_millis())
        .unwrap_or_default()
}

fn path_string(path: &Path) -> String {
    path.to_string_lossy().to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fresh_agent_config_defaults_to_glm() {
        assert_eq!(AgentConfig::default().default_model, DEFAULT_AGENT_MODEL);

        let config: AgentConfig = serde_json::from_value(json!({
            "defaultProjectRoot": null,
            "runtimeKind": "goose-direct"
        }))
        .expect("legacy config without a model should deserialize");
        assert_eq!(config.default_model, DEFAULT_AGENT_MODEL);
    }

    #[tokio::test]
    async fn permission_policy_is_session_scoped_and_mutable_mid_run() {
        assert_eq!(
            parse_user_permission_mode("smart_approve"),
            Ok(GooseMode::SmartApprove)
        );
        assert_eq!(parse_user_permission_mode("auto"), Ok(GooseMode::Auto));
        assert!(parse_user_permission_mode("approve").is_err());

        let modes = SessionPermissionModes::default();
        assert_eq!(
            selected_permission_mode(&modes, "session-1").await,
            GooseMode::SmartApprove
        );
        modes
            .lock()
            .await
            .insert("session-1".to_string(), GooseMode::Auto);
        assert_eq!(
            selected_permission_mode(&modes, "session-1").await,
            GooseMode::Auto
        );
        assert_eq!(
            selected_permission_mode(&modes, "session-2").await,
            GooseMode::SmartApprove
        );

        let mut claimed = HashMap::from([("session-1".to_string(), GooseMode::SmartApprove)]);
        assert_eq!(
            select_session_permission_mode(&mut claimed, "session-1", GooseMode::Auto),
            (GooseMode::SmartApprove, false),
            "a delayed send must not overwrite a newer authoritative policy"
        );
        assert_eq!(
            select_session_permission_mode(&mut claimed, "session-2", GooseMode::Auto),
            (GooseMode::Auto, true)
        );
    }

    #[test]
    fn agent_mode_accepts_only_one_shot_permission_decisions() {
        assert_eq!(
            permission_from_decision("allow_once").unwrap(),
            Permission::AllowOnce
        );
        assert_eq!(
            permission_from_decision("deny_once").unwrap(),
            Permission::DenyOnce
        );
        assert!(permission_from_decision("always_allow").is_err());
        assert!(permission_from_decision("always_deny").is_err());
    }

    #[test]
    fn maple_permission_file_forces_every_routed_tool_through_ask_before() {
        let root = std::env::temp_dir().join(format!(
            "maple-permissions-{}-{}",
            std::process::id(),
            NEXT_RUN_ID.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir_all(&root).unwrap();
        let path = root.join("permission.yaml");
        fs::write(
            &path,
            "user:\n  always_allow:\n  - shell\n  ask_before: []\n  never_allow: []\n",
        )
        .unwrap();

        reset_maple_owned_permission_file(&path).unwrap();
        let manager = PermissionManager::new(root.clone());
        for tool in MAPLE_DEVELOPER_TOOLS {
            assert_eq!(
                manager.get_user_permission(tool),
                Some(goose::config::permission::PermissionLevel::AskBefore)
            );
        }
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn legacy_powerful_agent_default_migrates_to_glm() {
        let mut config = AgentConfig {
            default_project_root: Some("/tmp/project".to_string()),
            default_model: LEGACY_AGENT_DEFAULT_MODEL.to_string(),
        };

        assert!(migrate_agent_config(&mut config));
        assert_eq!(config.default_model, DEFAULT_AGENT_MODEL);
        assert!(!migrate_agent_config(&mut config));
    }

    #[test]
    fn explicit_agent_model_choices_are_not_migrated() {
        for model in ["kimi-k2-6", "auto:quick", "glm-5-2", "gemma-3-27b"] {
            let mut config = AgentConfig {
                default_project_root: None,
                default_model: model.to_string(),
            };

            assert!(!migrate_agent_config(&mut config));
            assert_eq!(config.default_model, model);
        }
    }

    #[test]
    fn image_history_payloads_do_not_create_timeline_rows() {
        let message = Message::user()
            .with_id("image-message")
            .with_text("Inspect this image")
            .with_image("aW1hZ2U=", "image/png");

        let items = message_to_timeline_items(&message, false);

        assert_eq!(items.len(), 1);
        assert_eq!(items[0].id, "image-message-text");
        assert_eq!(items[0].text.as_deref(), Some("Inspect this image"));
    }

    #[test]
    fn agent_errors_are_bounded_for_the_timeline() {
        let item = error_item("x".repeat(MAX_AGENT_ERROR_CHARS + 100));
        let text = item.text.expect("error should contain a summary");

        assert_eq!(text.chars().count(), MAX_AGENT_ERROR_CHARS + 1);
        assert!(text.ends_with('…'));
    }

    #[test]
    fn terminal_projection_handles_stream_ids_and_idless_collisions() {
        let mut first_chunk = Message::assistant()
            .with_id("stream-message")
            .with_text("Hello");
        first_chunk.created = 100;
        let mut second_chunk = Message::assistant()
            .with_id("stream-message")
            .with_text(" world");
        second_chunk.created = 101;
        let first_items = message_to_timeline_items(&first_chunk, true);
        let second_items = message_to_timeline_items(&second_chunk, true);
        let candidate = update_live_message_candidate(
            Some(live_message_candidate(&first_chunk, &first_items)),
            &second_chunk,
            &second_items,
        );

        let mut persisted = Message::assistant()
            .with_id("stream-message")
            .with_text("Hello world");
        persisted.created = 100;
        let conversation = Conversation::new_unvalidated(vec![persisted]);
        assert!(terminal_message_is_persisted(&conversation, &candidate));

        let mut persisted_reply = Message::assistant().with_text("Persisted reply");
        persisted_reply.created = 200;
        let stored_reply = persisted_reply.clone().with_id("database-id");
        let reply_items = message_to_timeline_items(&persisted_reply, true);
        let reply_candidate = live_message_candidate(&persisted_reply, &reply_items);
        assert!(terminal_message_is_persisted(
            &Conversation::new_unvalidated(vec![stored_reply.clone()]),
            &reply_candidate
        ));

        let mut live_only_notice = Message::assistant().with_text("Transient provider error");
        live_only_notice.created = persisted_reply.created;
        let notice_items = message_to_timeline_items(&live_only_notice, true);
        let notice_candidate =
            update_live_message_candidate(Some(reply_candidate), &live_only_notice, &notice_items);
        assert_eq!(notice_candidate.items.len(), 1);
        assert_eq!(
            notice_candidate.items[0].text.as_deref(),
            Some("Transient provider error")
        );
        assert!(!terminal_message_is_persisted(
            &Conversation::new_unvalidated(vec![stored_reply]),
            &notice_candidate
        ));

        let mut same_id_notice = Message::assistant()
            .with_id("stream-message")
            .with_system_notification(SystemNotificationType::InlineMessage, "Live-only notice");
        same_id_notice.created = 100;
        let same_id_items = message_to_timeline_items(&same_id_notice, true);
        let same_id_candidate =
            update_live_message_candidate(Some(candidate), &same_id_notice, &same_id_items);
        assert_eq!(same_id_candidate.items.len(), 1);
        assert!(!terminal_message_is_persisted(
            &conversation,
            &same_id_candidate
        ));
    }

    #[tokio::test]
    async fn completed_timeline_reuses_session_load_and_retains_only_live_only_message() {
        let session_id = "session";
        let mut live_reply = Message::assistant().with_text("Persisted reply");
        live_reply.created = 300;
        let stored_reply = live_reply.clone().with_id("database-id");
        let persisted_conversation = Conversation::new_unvalidated(vec![stored_reply]);
        let persisted_timeline = conversation_to_timeline_items(&persisted_conversation);
        let reply_items = message_to_timeline_items(&live_reply, true);
        let reply_candidate = live_message_candidate(&live_reply, &reply_items);
        let live_timelines = Arc::new(Mutex::new(HashMap::from([(
            session_id.to_string(),
            LiveTimeline::Completed(reply_candidate),
        )])));

        let loaded = overlay_live_timeline(
            &live_timelines,
            session_id,
            &persisted_conversation,
            persisted_timeline.clone(),
        )
        .await;
        assert_eq!(loaded.len(), persisted_timeline.len());
        assert!(!live_timelines.lock().await.contains_key(session_id));

        let mut notice = Message::assistant().with_text("Transient provider error");
        notice.created = live_reply.created;
        let notice_items = message_to_timeline_items(&notice, true);
        let notice_candidate = live_message_candidate(&notice, &notice_items);
        let mut timelines = HashMap::new();

        apply_successful_prompt_outcome(
            &mut timelines,
            session_id,
            &AgentPromptOutcome {
                terminal_message: Some(notice_candidate),
            },
        );
        let live_timelines = Arc::new(Mutex::new(timelines));
        let loaded = overlay_live_timeline(
            &live_timelines,
            session_id,
            &persisted_conversation,
            persisted_timeline,
        )
        .await;
        assert_eq!(
            loaded.last().and_then(|item| item.text.as_deref()),
            Some("Transient provider error")
        );
        assert!(matches!(
            live_timelines.lock().await.get(session_id),
            Some(LiveTimeline::Completed(_))
        ));

        let mut timelines = live_timelines.lock().await;
        apply_successful_prompt_outcome(&mut timelines, session_id, &AgentPromptOutcome::default());
        assert!(!timelines.contains_key(session_id));
    }

    #[tokio::test]
    async fn failed_prompt_outcome_keeps_only_the_latest_error() {
        let session_id = "failed-session";
        let prior_turn = message_to_timeline_items(
            &Message::user()
                .with_id("prior-user")
                .with_text("Prior turn"),
            false,
        );
        let mut timelines =
            HashMap::from([(session_id.to_string(), LiveTimeline::Streaming(prior_turn))]);

        apply_failed_prompt_outcome(
            &mut timelines,
            session_id,
            error_item("First failure".to_string()),
        );
        apply_failed_prompt_outcome(
            &mut timelines,
            session_id,
            error_item("Second failure".to_string()),
        );

        let LiveTimeline::Failed(items) = timelines.get(session_id).unwrap() else {
            panic!("failed run should leave a bounded failed timeline");
        };
        assert_eq!(items.len(), 1);
        assert_eq!(items[0].text.as_deref(), Some("Second failure"));

        let live_timelines = Arc::new(Mutex::new(timelines));
        let next_user = message_to_timeline_items(
            &Message::user().with_id("next-user").with_text("Retry"),
            false,
        )
        .into_iter()
        .next()
        .unwrap();
        record_timeline_item(&live_timelines, session_id, next_user).await;
        let timelines = live_timelines.lock().await;
        let LiveTimeline::Streaming(items) = timelines.get(session_id).unwrap() else {
            panic!("a retry should start a fresh streaming timeline");
        };
        assert_eq!(items.len(), 1);
        assert_eq!(items[0].id, "next-user-text");
    }

    fn write_test_file(path: &Path) {
        fs::create_dir_all(path.parent().expect("test file should have a parent"))
            .expect("test file parent should be created");
        fs::write(path, b"sentinel").expect("test file should be written");
    }

    fn assistant_tool_message(
        message_id: &str,
        tool_id: &str,
        thinking: &str,
        signature: &str,
    ) -> Message {
        Message::assistant()
            .with_id(message_id)
            .with_thinking(thinking, signature)
            .with_tool_request(
                tool_id,
                Ok(rmcp::model::CallToolRequestParams::new("shell")),
            )
    }

    fn with_usage(mut message: Message) -> Message {
        message.metadata.usage = Some(Box::default());
        message
    }

    fn assistant_redacted_tool_message(
        message_id: &str,
        tool_id: &str,
        redacted_data: &str,
    ) -> Message {
        Message::assistant()
            .with_id(message_id)
            .with_redacted_thinking(redacted_data)
            .with_tool_request(
                tool_id,
                Ok(rmcp::model::CallToolRequestParams::new("shell")),
            )
    }

    fn tool_response_message(message_id: &str, tool_id: &str) -> Message {
        Message::user().with_id(message_id).with_tool_response(
            tool_id,
            Ok(rmcp::model::CallToolResult::success(vec![
                rmcp::model::Content::text("ok"),
            ])),
        )
    }

    fn timeline_thinking_texts(items: &[AgentTimelineItem]) -> Vec<&str> {
        items
            .iter()
            .filter(|item| item.item_type == "thinking")
            .filter_map(|item| item.text.as_deref())
            .collect()
    }

    fn merge_test_timeline_items(
        mut current: Vec<AgentTimelineItem>,
        incoming: Vec<AgentTimelineItem>,
    ) -> Vec<AgentTimelineItem> {
        for item in incoming {
            current = merge_timeline_item(current, item);
        }
        current
    }

    #[test]
    fn joins_thinking_fragments_within_each_goose_message() {
        let message = Message::assistant()
            .with_id("assistant-1")
            .with_thinking("I can", "")
            .with_thinking(" help.", "")
            .with_text("Done");
        let conversation = Conversation::new_unvalidated(vec![message.clone()]);

        let live = message_to_timeline_items(&message, true);
        let loaded = conversation_to_timeline_items(&conversation);

        assert_eq!(timeline_thinking_texts(&live), vec!["I can help."]);
        assert_eq!(timeline_thinking_texts(&loaded), vec!["I can help."]);
        assert_eq!(
            loaded
                .iter()
                .find(|item| item.item_type == "thinking")
                .map(|item| item.id.as_str()),
            Some("assistant-1-thinking")
        );
    }

    #[test]
    fn hides_tool_reasoning_after_prior_visible_thinking() {
        let surfaced = "Inspect the project before running both commands.";
        let tool_attached = "Reasoning accumulated before the tool request.";
        let conversation = Conversation::new_unvalidated(vec![
            Message::assistant()
                .with_id("surfaced")
                .with_thinking(surfaced, "")
                .with_text("Starting now."),
            assistant_tool_message("request-1", "tool-1", tool_attached, ""),
            tool_response_message("response-1", "tool-1"),
            with_usage(assistant_tool_message(
                "request-2",
                "tool-2",
                tool_attached,
                "",
            )),
            tool_response_message("response-2", "tool-2"),
        ]);

        let items = conversation_to_timeline_items(&conversation);

        assert_eq!(timeline_thinking_texts(&items), vec![surfaced]);
        assert_eq!(
            items.iter().filter(|item| item.item_type == "tool").count(),
            2
        );
    }

    #[test]
    fn suppresses_replayed_thinking_on_split_tool_requests() {
        let reasoning = "Run both requested commands.";
        let conversation = Conversation::new_unvalidated(vec![
            assistant_tool_message("request-1", "tool-1", reasoning, ""),
            tool_response_message("response-1", "tool-1"),
            with_usage(assistant_tool_message(
                "request-2",
                "tool-2",
                "A later accumulated copy from the same inference.",
                "",
            )),
            tool_response_message("response-2", "tool-2"),
        ]);

        let items = conversation_to_timeline_items(&conversation);

        assert_eq!(timeline_thinking_texts(&items), vec![reasoning]);
        assert_eq!(
            items.iter().filter(|item| item.item_type == "tool").count(),
            2
        );
    }

    #[test]
    fn usage_boundary_preserves_identical_thinking_in_the_next_inference() {
        let reasoning = "Run the requested command.";
        let conversation = Conversation::new_unvalidated(vec![
            with_usage(assistant_tool_message("request-1", "tool-1", reasoning, "")),
            tool_response_message("response-1", "tool-1"),
            with_usage(assistant_tool_message("request-2", "tool-2", reasoning, "")),
            tool_response_message("response-2", "tool-2"),
        ]);

        let items = conversation_to_timeline_items(&conversation);

        assert_eq!(timeline_thinking_texts(&items), vec![reasoning, reasoning]);
    }

    #[test]
    fn histories_without_usage_preserve_every_tool_thought() {
        let conversation = Conversation::new_unvalidated(vec![
            assistant_tool_message("request-1", "tool-1", "First thought.", ""),
            tool_response_message("response-1", "tool-1"),
            assistant_tool_message("request-2", "tool-2", "Second thought.", ""),
            tool_response_message("response-2", "tool-2"),
        ]);

        let items = conversation_to_timeline_items(&conversation);

        assert_eq!(
            timeline_thinking_texts(&items),
            vec!["First thought.", "Second thought."]
        );
    }

    #[test]
    fn preserves_legacy_thinking_text_for_the_rendering_boundary() {
        let reasoning = "Inspect the repository and summarize it.";
        let conversation = Conversation::new_unvalidated(vec![
            Message::assistant()
                .with_id("standalone-reasoning")
                .with_thinking(reasoning, ""),
            Message::assistant()
                .with_id("standalone-period")
                .with_thinking(".", ""),
            assistant_tool_message("request-1", "tool-1", ".", ""),
            tool_response_message("response-1", "tool-1"),
            with_usage(assistant_tool_message("request-2", "tool-2", ".", "")),
            tool_response_message("response-2", "tool-2"),
        ]);

        let items = conversation_to_timeline_items(&conversation);

        assert_eq!(timeline_thinking_texts(&items), vec![reasoning, "."]);
        assert_eq!(
            items.iter().filter(|item| item.item_type == "tool").count(),
            2
        );
    }

    #[test]
    fn live_thinking_chunks_match_persisted_message_projection() {
        let user = Message::user()
            .with_id("current-user")
            .with_text("Inspect the project.");
        let persisted_conversation = Conversation::new_unvalidated(vec![
            user.clone(),
            Message::assistant()
                .with_id("assistant")
                .with_thinking(". ", "")
                .with_thinking("First", "")
                .with_thinking(" ", "")
                .with_thinking("second", "")
                .with_thinking(".", ""),
        ]);
        let persisted = conversation_to_timeline_items(&persisted_conversation);
        let live_messages = vec![
            user,
            Message::assistant()
                .with_id("live-assistant")
                .with_thinking(". ", ""),
            Message::assistant()
                .with_id("live-assistant")
                .with_thinking("First", ""),
            Message::assistant()
                .with_id("live-assistant")
                .with_thinking(" ", ""),
            Message::assistant()
                .with_id("live-assistant")
                .with_thinking("second", ""),
            Message::assistant()
                .with_id("live-assistant")
                .with_thinking(".", ""),
        ];
        let live = live_messages
            .into_iter()
            .fold(Vec::new(), |items, message| {
                merge_test_timeline_items(items, message_to_timeline_items(&message, true))
            });

        assert_eq!(timeline_thinking_texts(&persisted), vec![". First second."]);
        assert_eq!(timeline_thinking_texts(&live), vec![". First second."]);
    }

    #[test]
    fn suppresses_signed_thinking_replayed_within_one_inference() {
        let conversation = Conversation::new_unvalidated(vec![
            assistant_tool_message("request-1", "tool-1", "Signed reasoning", "signature-a"),
            tool_response_message("response-1", "tool-1"),
            with_usage(assistant_tool_message(
                "request-2",
                "tool-2",
                "Signed reasoning",
                "signature-b",
            )),
            tool_response_message("response-2", "tool-2"),
        ]);

        let items = conversation_to_timeline_items(&conversation);

        assert_eq!(timeline_thinking_texts(&items), vec!["Signed reasoning"]);
    }

    #[test]
    fn suppresses_redacted_thinking_replayed_within_one_inference() {
        let conversation = Conversation::new_unvalidated(vec![
            assistant_redacted_tool_message("request-1", "tool-1", "opaque-payload-a"),
            tool_response_message("response-1", "tool-1"),
            with_usage(assistant_redacted_tool_message(
                "request-2",
                "tool-2",
                "opaque-payload-b",
            )),
            tool_response_message("response-2", "tool-2"),
        ]);

        let items = conversation_to_timeline_items(&conversation);

        assert_eq!(
            timeline_thinking_texts(&items),
            vec!["Thinking redacted by provider."]
        );
    }

    #[test]
    fn preserves_reasoning_text_for_the_rendering_boundary() {
        let conversation = Conversation::new_unvalidated(vec![
            Message::assistant()
                .with_id("emoji")
                .with_thinking("🤔", ""),
            Message::assistant()
                .with_id("operator")
                .with_thinking("=>", ""),
            Message::assistant()
                .with_id("ellipsis")
                .with_thinking("…...", ""),
        ]);

        let items = conversation_to_timeline_items(&conversation);

        assert_eq!(timeline_thinking_texts(&items), vec!["🤔", "=>", "…..."]);
    }

    #[test]
    fn unsigned_thinking_dedupe_resets_on_the_next_user_turn() {
        let reasoning = "Run the requested command.";
        let conversation = Conversation::new_unvalidated(vec![
            with_usage(assistant_tool_message("request-1", "tool-1", reasoning, "")),
            tool_response_message("response-1", "tool-1"),
            Message::user()
                .with_id("next-turn")
                .with_text("Run it again."),
            with_usage(assistant_tool_message("request-2", "tool-2", reasoning, "")),
            tool_response_message("response-2", "tool-2"),
        ]);

        let items = conversation_to_timeline_items(&conversation);

        assert_eq!(timeline_thinking_texts(&items), vec![reasoning, reasoning]);
    }

    #[test]
    fn hidden_goose_messages_neither_render_nor_consume_visible_replay() {
        let reasoning = "Inspect the project.";
        let hidden = Message::assistant()
            .with_id("hidden-assistant")
            .with_thinking(reasoning, "")
            .with_text("internal grind details")
            .with_visibility(false, true);
        let conversation = Conversation::new_unvalidated(vec![
            Message::user().with_id("user").with_text("Inspect it."),
            hidden.clone(),
            with_usage(assistant_tool_message(
                "visible-request",
                "tool-1",
                reasoning,
                "",
            )),
            tool_response_message("response", "tool-1"),
        ]);

        let items = conversation_to_timeline_items(&conversation);

        assert_eq!(timeline_thinking_texts(&items), vec![reasoning]);
        assert!(!items.iter().any(|item| {
            item.id.starts_with("hidden-assistant")
                || item.text.as_deref() == Some("internal grind details")
        }));
        assert!(message_to_timeline_items(&hidden, true).is_empty());
    }

    #[test]
    fn hidden_usage_boundary_resets_visible_inference_state() {
        let first = "First visible thought.";
        let second = "Second visible thought.";
        let hidden_boundary = with_usage(
            Message::assistant()
                .with_id("hidden-boundary")
                .with_text("internal")
                .with_visibility(false, true),
        );
        let conversation = Conversation::new_unvalidated(vec![
            Message::assistant()
                .with_id("first")
                .with_thinking(first, ""),
            hidden_boundary,
            with_usage(assistant_tool_message("request", "tool", second, "")),
            tool_response_message("response", "tool"),
        ]);

        let items = conversation_to_timeline_items(&conversation);

        assert_eq!(timeline_thinking_texts(&items), vec![first, second]);
    }

    #[test]
    fn hidden_user_message_still_resets_provider_turn_replay() {
        let reasoning = "Run the requested command.";
        let conversation = Conversation::new_unvalidated(vec![
            with_usage(assistant_tool_message("request-1", "tool-1", reasoning, "")),
            tool_response_message("response-1", "tool-1"),
            Message::user()
                .with_id("hidden-user")
                .with_text("internal retry turn")
                .with_visibility(false, true),
            with_usage(assistant_tool_message("request-2", "tool-2", reasoning, "")),
            tool_response_message("response-2", "tool-2"),
        ]);

        let items = conversation_to_timeline_items(&conversation);

        assert_eq!(timeline_thinking_texts(&items), vec![reasoning, reasoning]);
        assert!(!items.iter().any(|item| item.id.starts_with("hidden-user")));
    }

    #[test]
    fn thinking_projection_is_session_local_and_does_not_mutate_history() {
        let build_conversation = || {
            Conversation::new_unvalidated(vec![
                assistant_tool_message("request-1", "tool-1", "Shared replay", ""),
                tool_response_message("response-1", "tool-1"),
                with_usage(assistant_tool_message(
                    "request-2",
                    "tool-2",
                    "Shared replay",
                    "",
                )),
                tool_response_message("response-2", "tool-2"),
            ])
        };
        let first = build_conversation();
        let second = build_conversation();
        let first_before = first.clone();
        let second_before = second.clone();

        let first_items = conversation_to_timeline_items(&first);
        let second_items = conversation_to_timeline_items(&second);

        assert_eq!(timeline_thinking_texts(&first_items), vec!["Shared replay"]);
        assert_eq!(
            timeline_thinking_texts(&second_items),
            vec!["Shared replay"]
        );
        assert_eq!(first, first_before);
        assert_eq!(second, second_before);
    }

    #[test]
    fn live_overlay_splices_at_the_first_shared_user_boundary() {
        let prior_user = Message::user()
            .with_id("prior-user")
            .with_text("Earlier turn");
        let prior_assistant = Message::assistant()
            .with_id("prior-assistant")
            .with_text("Earlier answer");
        let current_user = Message::user()
            .with_id("current-user")
            .with_text("Current turn");
        let persisted_thought = Message::assistant()
            .with_id("persisted-copy")
            .with_thinking("Persisted provider-history copy", "");
        let live_thought = Message::assistant()
            .with_id("live-thought")
            .with_thinking("Authoritative live thought", "");

        let persisted = [
            message_to_timeline_items(&prior_user, false),
            message_to_timeline_items(&prior_assistant, false),
            message_to_timeline_items(&current_user, false),
            message_to_timeline_items(&persisted_thought, false),
        ]
        .concat();
        let live = [
            message_to_timeline_items(&current_user, true),
            message_to_timeline_items(&live_thought, true),
        ]
        .concat();

        let overlaid = overlay_live_timeline_items(persisted, live);

        assert!(overlaid.iter().any(|item| item.id == "prior-user-text"));
        assert!(overlaid
            .iter()
            .any(|item| item.id == "prior-assistant-text"));
        assert!(overlaid.iter().any(|item| item.id == "current-user-text"));
        assert!(overlaid
            .iter()
            .any(|item| item.id == "live-thought-thinking"));
        assert!(!overlaid
            .iter()
            .any(|item| item.id == "persisted-copy-thinking"));
    }

    #[tokio::test]
    async fn history_replaced_preserves_the_matching_live_user_boundary() {
        let session_id = "history-replaced-boundary";
        let prior_user = Message::user()
            .with_id("prior-user")
            .with_text("Earlier turn");
        let current_user = Message::user()
            .with_id("current-user")
            .with_text("Current turn");
        let hidden_user = Message::user()
            .with_id("hidden-user")
            .with_text("Internal retry turn")
            .with_visibility(false, true);
        let conversation = Conversation::new_unvalidated(vec![
            prior_user,
            current_user,
            tool_response_message("tool-response", "tool-1"),
            hidden_user,
        ]);
        let stale = message_to_timeline_items(
            &Message::assistant()
                .with_id("stale-live")
                .with_thinking("Stale thought", ""),
            true,
        );
        let mut live = stale;
        live.extend(message_to_timeline_items(
            &Message::user()
                .with_id("current-user")
                .with_text("Live current turn"),
            false,
        ));
        let live_timelines = Arc::new(Mutex::new(HashMap::from([(
            session_id.to_string(),
            LiveTimeline::Streaming(live),
        )])));

        reseed_live_timeline_after_history_replaced(&live_timelines, session_id, &conversation)
            .await;

        let timelines = live_timelines.lock().await;
        let items = timelines
            .get(session_id)
            .expect("replacement should retain a user boundary")
            .items();
        assert_eq!(items.len(), 1);
        assert_eq!(items[0].id, "current-user-text");
        assert_eq!(items[0].text.as_deref(), Some("Live current turn"));
        assert!(!items.iter().any(|item| item.id == "stale-live-thinking"));
        assert!(!items.iter().any(|item| item.id == "hidden-user-text"));
    }

    #[tokio::test]
    async fn history_replaced_boundary_prevents_post_compaction_replay_on_reload() {
        let session_id = "post-compaction-reload";
        let prior_user = Message::user()
            .with_id("prior-user")
            .with_text("Earlier turn");
        let prior_assistant = Message::assistant()
            .with_id("prior-assistant")
            .with_text("Earlier answer");
        let current_user = Message::user()
            .with_id("current-user")
            .with_text("Current turn");
        let replacement = Conversation::new_unvalidated(vec![
            prior_user.clone(),
            prior_assistant.clone(),
            current_user.clone(),
        ]);
        let live_timelines = Arc::new(Mutex::new(HashMap::new()));
        reseed_live_timeline_after_history_replaced(&live_timelines, session_id, &replacement)
            .await;

        let live_response = assistant_tool_message(
            "live-provider-response",
            "tool-1",
            "Authoritative live thought",
            "",
        );
        for item in message_to_timeline_items(&live_response, true) {
            record_timeline_item(&live_timelines, session_id, item).await;
        }

        let persisted_conversation = Conversation::new_unvalidated(vec![
            prior_user,
            prior_assistant,
            current_user,
            with_usage(assistant_tool_message(
                "persisted-split-request",
                "tool-1",
                "Persisted provider-history copy",
                "",
            )),
            tool_response_message("persisted-tool-response", "tool-1"),
        ]);
        let persisted = conversation_to_timeline_items(&persisted_conversation);
        assert_eq!(
            timeline_thinking_texts(&persisted),
            vec!["Persisted provider-history copy"]
        );

        let overlaid = overlay_live_timeline(
            &live_timelines,
            session_id,
            &persisted_conversation,
            persisted,
        )
        .await;

        assert!(overlaid.iter().any(|item| item.id == "prior-user-text"));
        assert!(overlaid
            .iter()
            .any(|item| item.id == "prior-assistant-text"));
        assert_eq!(
            overlaid
                .iter()
                .filter(|item| item.id == "current-user-text")
                .count(),
            1
        );
        assert_eq!(
            timeline_thinking_texts(&overlaid),
            vec!["Authoritative live thought"]
        );
        assert!(overlaid
            .iter()
            .any(|item| item.id == "live-provider-response-thinking"));
        assert!(!overlaid
            .iter()
            .any(|item| item.id == "persisted-split-request-thinking"));
        assert_eq!(
            overlaid
                .iter()
                .filter(|item| item.item_type == "tool" && item.id == "tool-1")
                .count(),
            1
        );
    }

    #[test]
    fn clear_history_removes_only_the_target_account_session_store() {
        let test_root = std::env::temp_dir().join(format!(
            "maple-agent-history-clear-{}-{}",
            std::process::id(),
            unix_ms()
        ));
        let app_config_dir = test_root.join("app-config");
        let agent_root = app_config_dir.join("agent");
        let account_dir = agent_root.join("accounts/target");
        let other_account_dir = agent_root.join("accounts/other");
        let removed = [account_dir.join("goose/data/session.db")];
        for path in &removed {
            write_test_file(path);
        }

        let preserved = [
            account_dir.join("config.json"),
            account_dir.join("recent_roots.json"),
            account_dir.join("goose/config/permissions.json"),
            other_account_dir.join("goose/data/session.db"),
            agent_root.join("goose-runtime/config/config.yaml"),
            app_config_dir.join("proxy_config.json"),
        ];
        for path in &preserved {
            write_test_file(path);
        }

        clear_agent_history(&account_dir).expect("Agent history should be cleared");

        for path in removed {
            assert!(!path.exists(), "history remained at {}", path.display());
        }
        for path in preserved {
            assert!(path.exists(), "configuration removed at {}", path.display());
        }

        clear_agent_history(&account_dir).expect("clearing missing history should be idempotent");
        let _ = fs::remove_dir_all(test_root);
    }

    #[tokio::test]
    async fn offline_session_managers_reopen_only_their_account_data() {
        let test_root = std::env::temp_dir().join(format!(
            "maple-agent-offline-sessions-{}-{}",
            std::process::id(),
            unix_ms()
        ));
        let project_dir = test_root.join("project");
        let account_a = test_root.join("accounts/a");
        let account_b = test_root.join("accounts/b");
        fs::create_dir_all(&project_dir).expect("project directory should be created");

        let manager_a = session_manager_for_account_dir(&account_a)
            .expect("account A session manager should open");
        let manager_b = session_manager_for_account_dir(&account_b)
            .expect("account B session manager should open");
        let session_a = manager_a
            .create_session(
                project_dir.clone(),
                "Account A chat".to_string(),
                SessionType::User,
                GooseMode::SmartApprove,
            )
            .await
            .expect("account A session should be created");
        let session_b = manager_b
            .create_session(
                project_dir.clone(),
                "Account B chat".to_string(),
                SessionType::User,
                GooseMode::SmartApprove,
            )
            .await
            .expect("account B session should be created");
        let account_b_only_session = manager_b
            .create_session(
                project_dir,
                "Account B second chat".to_string(),
                SessionType::User,
                GooseMode::SmartApprove,
            )
            .await
            .expect("account B second session should be created");
        drop(manager_a);
        drop(manager_b);

        let reopened_a = session_manager_for_account_dir(&account_a)
            .expect("account A session manager should reopen");
        let reopened_b = session_manager_for_account_dir(&account_b)
            .expect("account B session manager should reopen");
        let loaded_a = reopened_a
            .get_session(&session_a.id, true)
            .await
            .expect("account A session should reload");
        let loaded_b = reopened_b
            .get_session(&session_b.id, true)
            .await
            .expect("account B session should reload");
        assert_eq!(loaded_a.name, "Account A chat");
        assert_eq!(loaded_b.name, "Account B chat");
        assert!(reopened_a
            .get_session(&account_b_only_session.id, true)
            .await
            .is_err());

        reopened_a
            .delete_session(&session_a.id)
            .await
            .expect("account A session should be deleted");
        assert!(reopened_a.list_all_sessions().await.unwrap().is_empty());
        assert_eq!(reopened_b.list_all_sessions().await.unwrap().len(), 2);

        drop(reopened_a);
        drop(reopened_b);
        let _ = fs::remove_dir_all(test_root);
    }

    #[tokio::test]
    async fn deletes_only_target_session_runtime_state() {
        let test_root = std::env::temp_dir().join(format!(
            "maple-agent-session-delete-flow-{}-{}",
            std::process::id(),
            unix_ms()
        ));
        let data_dir = test_root.join("goose-data");
        let project_dir = test_root.join("project");
        fs::create_dir_all(&data_dir).expect("Goose data directory should be created");
        fs::create_dir_all(&project_dir).expect("project directory should be created");

        let session_manager = SessionManager::new(data_dir);
        let target = session_manager
            .create_session(
                project_dir.clone(),
                "Target chat".to_string(),
                SessionType::User,
                GooseMode::SmartApprove,
            )
            .await
            .expect("target session should be created");
        let survivor = session_manager
            .create_session(
                project_dir,
                "Surviving chat".to_string(),
                SessionType::User,
                GooseMode::SmartApprove,
            )
            .await
            .expect("surviving session should be created");

        let live_timelines = Arc::new(Mutex::new(HashMap::from([
            (target.id.clone(), LiveTimeline::Streaming(Vec::new())),
            (survivor.id.clone(), LiveTimeline::Streaming(Vec::new())),
        ])));
        let pending_permissions = Arc::new(Mutex::new(HashMap::from([
            ((target.id.clone(), "target-request".to_string()), ()),
            ((survivor.id.clone(), "survivor-request".to_string()), ()),
        ])));
        delete_persisted_agent_session(
            &session_manager,
            &pending_permissions,
            &live_timelines,
            &target.id,
        )
        .await
        .expect("target session deletion should succeed");

        assert!(session_manager
            .get_session(&target.id, false)
            .await
            .is_err());
        assert!(session_manager
            .get_session(&survivor.id, false)
            .await
            .is_ok());
        assert!(!live_timelines.lock().await.contains_key(&target.id));
        assert!(live_timelines.lock().await.contains_key(&survivor.id));
        let permissions = pending_permissions.lock().await;
        assert!(!permissions
            .keys()
            .any(|(session_id, _)| session_id == &target.id));
        assert!(permissions
            .keys()
            .any(|(session_id, _)| session_id == &survivor.id));

        let _ = fs::remove_dir_all(test_root);
    }

    #[tokio::test]
    async fn cancelled_turn_rollback_restores_exact_pre_turn_state() {
        let test_root = std::env::temp_dir().join(format!(
            "maple-agent-cancelled-turn-{}-{}",
            std::process::id(),
            unix_ms()
        ));
        let data_dir = test_root.join("goose-data");
        let project_dir = test_root.join("project");
        fs::create_dir_all(&data_dir).expect("Goose data directory should be created");
        fs::create_dir_all(&project_dir).expect("project directory should be created");

        let session_manager = SessionManager::new(data_dir);
        let session = session_manager
            .create_session(
                project_dir.clone(),
                "Cancellation test".to_string(),
                SessionType::User,
                GooseMode::SmartApprove,
            )
            .await
            .expect("test session should be created");
        let prior_user = Message::user().with_text("keep me").with_generated_id();
        let prior_assistant = Message::assistant()
            .with_text("kept response")
            .with_generated_id();
        for message in [&prior_user, &prior_assistant] {
            session_manager
                .add_message(&session.id, message)
                .await
                .expect("test message should be persisted");
        }
        let prior_live_item = error_item("keep prior live error".to_string());
        let live_timelines = Arc::new(Mutex::new(HashMap::from([(
            session.id.clone(),
            LiveTimeline::Streaming(vec![prior_live_item.clone()]),
        )])));

        let mut before_turn = session_manager
            .get_session(&session.id, true)
            .await
            .expect("pre-turn session should load");
        let snapshot = AgentTurnSnapshot {
            conversation: before_turn
                .conversation
                .take()
                .expect("pre-turn conversation should be loaded"),
            autogenerated_title: None,
            live_timeline: live_timelines.lock().await.get(&session.id).cloned(),
        };

        let configured_model_name = "configured-after-snapshot";
        session_manager
            .update(&session.id)
            .model_config(
                serde_json::from_value(json!({
                    "model_name": configured_model_name,
                    "context_limit": 200_000,
                    "temperature": null,
                    "max_tokens": 8_192,
                    "toolshim": false,
                    "toolshim_model": null
                }))
                .expect("test model configuration should deserialize"),
            )
            .goose_mode(GooseMode::Auto)
            .apply()
            .await
            .expect("turn configuration should be persisted");

        let compacted_history = Message::assistant()
            .with_text("replacement compacted history")
            .with_generated_id();
        let cancelled_user = Message::user().with_text("discard me").with_generated_id();
        let partial_assistant = Message::assistant()
            .with_text("partial response")
            .with_generated_id();
        session_manager
            .replace_conversation(
                &session.id,
                &Conversation::new_unvalidated(vec![
                    compacted_history,
                    cancelled_user.clone(),
                    partial_assistant.clone(),
                ]),
            )
            .await
            .expect("Goose history replacement should be simulated");

        // Simulate HistoryReplaced clearing the session entry, followed by a
        // later current-turn event that has no optimistic user boundary.
        let post_history_replaced_item = error_item("post-replacement partial event".to_string());
        live_timelines.lock().await.insert(
            session.id.clone(),
            LiveTimeline::Streaming(vec![post_history_replaced_item.clone()]),
        );
        rollback_cancelled_agent_turn(&session_manager, &live_timelines, &session.id, &snapshot)
            .await
            .expect("cancelled turn should be discarded");
        // Restoring the exact snapshot is idempotent, including when
        // cancellation raced before Goose persisted any part of the turn.
        rollback_cancelled_agent_turn(&session_manager, &live_timelines, &session.id, &snapshot)
            .await
            .expect("repeated snapshot restoration should be a no-op");

        let reloaded = session_manager
            .get_session(&session.id, true)
            .await
            .expect("test session should reload");
        let conversation = reloaded
            .conversation
            .as_ref()
            .expect("test session should have a conversation");
        assert_eq!(conversation, &snapshot.conversation);
        assert_eq!(reloaded.name, "Cancellation test");
        assert_eq!(reloaded.goose_mode, GooseMode::Auto);
        assert_eq!(
            reloaded
                .model_config
                .as_ref()
                .map(|config| config.model_name.as_str()),
            Some(configured_model_name)
        );
        let restored_live_timeline = live_timelines.lock().await.get(&session.id).cloned();
        assert_eq!(restored_live_timeline, snapshot.live_timeline);
        assert!(!live_timelines
            .lock()
            .await
            .get(&session.id)
            .is_some_and(|timeline| timeline
                .items()
                .iter()
                .any(|item| item.id == post_history_replaced_item.id)));
        let persisted_timeline = conversation_to_timeline_items(conversation);
        let overlaid_timeline = overlay_live_timeline(
            &live_timelines,
            &session.id,
            conversation,
            persisted_timeline,
        )
        .await;
        assert!(overlaid_timeline
            .iter()
            .any(|item| item.id == prior_live_item.id));
        assert!(!overlaid_timeline.iter().any(|item| {
            item.text.as_deref().is_some_and(|text| {
                text.contains("discard me") || text.contains("partial response")
            })
        }));

        let first_turn_session = session_manager
            .create_session(
                project_dir,
                DEFAULT_AGENT_SESSION_TITLE.to_string(),
                SessionType::User,
                GooseMode::SmartApprove,
            )
            .await
            .expect("first-turn session should be created");
        let mut first_turn_before = session_manager
            .get_session(&first_turn_session.id, true)
            .await
            .expect("first-turn snapshot should load");
        let first_turn_snapshot = AgentTurnSnapshot {
            conversation: first_turn_before
                .conversation
                .take()
                .expect("empty first-turn conversation should be loaded"),
            autogenerated_title: Some(first_turn_before.name.clone()),
            live_timeline: live_timelines
                .lock()
                .await
                .get(&first_turn_session.id)
                .cloned(),
        };
        session_manager
            .update(&first_turn_session.id)
            .system_generated_name("discarded first prompt".to_string())
            .apply()
            .await
            .expect("first prompt should generate a title");
        let first_turn_message = Message::user()
            .with_text("discarded first prompt")
            .with_generated_id();
        session_manager
            .add_message(&first_turn_session.id, &first_turn_message)
            .await
            .expect("first-turn message should be persisted");
        live_timelines.lock().await.insert(
            first_turn_session.id.clone(),
            LiveTimeline::Streaming(vec![error_item("first-turn partial event".to_string())]),
        );
        rollback_cancelled_agent_turn(
            &session_manager,
            &live_timelines,
            &first_turn_session.id,
            &first_turn_snapshot,
        )
        .await
        .expect("cancelled first turn should be discarded");
        let first_turn_reloaded = session_manager
            .get_session(&first_turn_session.id, true)
            .await
            .expect("first-turn session should reload");
        assert_eq!(first_turn_reloaded.message_count, 0);
        assert_eq!(first_turn_reloaded.name, DEFAULT_AGENT_SESSION_TITLE);
        assert_eq!(
            first_turn_reloaded.conversation.as_ref(),
            Some(&first_turn_snapshot.conversation)
        );
        assert!(!live_timelines
            .lock()
            .await
            .contains_key(&first_turn_session.id));

        let _ = fs::remove_dir_all(test_root);
    }

    #[tokio::test]
    async fn detects_active_run_for_session() {
        let mut active_runs = HashMap::new();
        let task_handle = tauri::async_runtime::spawn(async {});
        active_runs.insert(
            "run-1".to_string(),
            ActiveAgentRun {
                token: CancellationToken::new(),
                session_id: "session-1".to_string(),
                task_handle,
            },
        );

        assert!(has_active_session_run(&active_runs, "session-1"));
        assert!(!has_active_session_run(&active_runs, "session-2"));
    }

    #[test]
    fn account_scopes_are_deterministic_isolated_and_opaque() {
        let first = account_scope("user-123").expect("account ID should be valid");
        assert_eq!(first, account_scope(" user-123 ").unwrap());
        assert_ne!(first, account_scope("user-456").unwrap());
        assert_eq!(first.len(), 64);
        assert!(!first.contains("user-123"));
    }

    #[test]
    fn rejects_wrong_runtime_account_scope() {
        let first = account_scope("first-user").unwrap();
        let second = account_scope("second-user").unwrap();
        assert!(ensure_account_scope(&first, &first).is_ok());
        assert!(ensure_account_scope(&first, &second).is_err());
    }

    #[tokio::test]
    async fn rejects_operations_captured_before_account_clear() {
        let state = AgentRuntimeState::new();
        let scope = account_scope("user-to-clear").unwrap();
        let stale_generation = account_generation(&state, &scope).await;

        let current_generation = advance_account_generation(&state, &scope).await;

        assert!(ensure_account_generation(&state, &scope, stale_generation)
            .await
            .is_err());
        assert!(
            ensure_account_generation(&state, &scope, current_generation)
                .await
                .is_ok()
        );
    }

    #[test]
    fn run_ids_are_unique() {
        let ids = (0..10_000)
            .map(|_| next_run_id())
            .collect::<std::collections::HashSet<_>>();
        assert_eq!(ids.len(), 10_000);
    }

    #[tokio::test]
    async fn forced_task_shutdown_joins_aborted_task() {
        struct DropFlag(Arc<std::sync::atomic::AtomicBool>);
        impl Drop for DropFlag {
            fn drop(&mut self) {
                self.0.store(true, Ordering::SeqCst);
            }
        }

        let dropped = Arc::new(std::sync::atomic::AtomicBool::new(false));
        let (started_tx, started_rx) = oneshot::channel();
        let task_dropped = Arc::clone(&dropped);
        let task = tauri::async_runtime::spawn(async move {
            let _drop_flag = DropFlag(task_dropped);
            let _ = started_tx.send(());
            futures_util::future::pending::<()>().await;
        });
        started_rx.await.unwrap();

        join_agent_tasks(vec![task], std::time::Duration::from_millis(1)).await;

        assert!(dropped.load(Ordering::SeqCst));
    }

    #[test]
    fn session_title_collapses_whitespace_and_bounds_unicode() {
        assert_eq!(
            session_title_from_prompt("  inspect\n\tthis   repo  "),
            "inspect this repo"
        );

        let title = session_title_from_prompt(&"🙂 ".repeat(100));
        assert!(title.chars().count() <= MAX_AGENT_SESSION_TITLE_CHARS);
        assert!(title.ends_with('…'));
        assert!(!title.contains("  "));
    }

    #[tokio::test]
    async fn cancelled_permission_is_not_registered() {
        let pending = Arc::new(Mutex::new(HashMap::new()));
        let cancel_token = CancellationToken::new();
        cancel_token.cancel();

        assert!(
            !register_pending_permission(&pending, "request-1", "session-1", &cancel_token).await
        );
        assert!(pending.lock().await.is_empty());
    }

    #[tokio::test]
    async fn pending_permission_ids_are_scoped_by_session() {
        let pending = Arc::new(Mutex::new(HashMap::from([
            (("session-1".to_string(), "shared-request".to_string()), ()),
            (("session-2".to_string(), "shared-request".to_string()), ()),
        ])));

        let selected = pending_permissions_for_sessions(&pending, &["session-1".to_string()]).await;

        assert_eq!(
            selected,
            vec![("shared-request".to_string(), "session-1".to_string())]
        );
        assert_eq!(pending.lock().await.len(), 2);
    }

    #[test]
    fn coalesces_tool_request_and_response_for_loaded_sessions() {
        let request = AgentTimelineItem {
            id: "functions.shell:7".to_string(),
            item_type: "tool".to_string(),
            role: Some("assistant".to_string()),
            title: Some("shell".to_string()),
            text: Some("listing project root".to_string()),
            status: Some("running".to_string()),
            input: Some(json!({ "command": "ls -la" })),
            output: None,
            created_ms: 1000,
            merge: "replace".to_string(),
        };
        let response = AgentTimelineItem {
            id: "functions.shell:7".to_string(),
            item_type: "tool".to_string(),
            role: Some("assistant".to_string()),
            title: None,
            text: None,
            status: Some("completed".to_string()),
            input: None,
            output: Some(json!({ "text": "ok" })),
            created_ms: 2000,
            merge: "replace".to_string(),
        };

        let items = coalesce_timeline_items(vec![request, response]);

        assert_eq!(items.len(), 1);
        assert_eq!(items[0].id, "functions.shell:7");
        assert_eq!(items[0].title.as_deref(), Some("shell"));
        assert_eq!(items[0].text.as_deref(), Some("listing project root"));
        assert_eq!(items[0].status.as_deref(), Some("completed"));
        assert_eq!(items[0].input, Some(json!({ "command": "ls -la" })));
        assert_eq!(items[0].output, Some(json!({ "text": "ok" })));
    }

    #[test]
    fn tool_error_preserves_request_title_for_provider_generated_id() {
        let id = "chatcmpl-tool-123";
        let request = AgentTimelineItem {
            id: id.to_string(),
            item_type: "tool".to_string(),
            role: Some("assistant".to_string()),
            title: Some("shell".to_string()),
            text: None,
            status: Some("running".to_string()),
            input: Some(json!({ "command": "false" })),
            output: None,
            created_ms: 1000,
            merge: "replace".to_string(),
        };
        let response = goose::conversation::message::ToolResponse {
            id: id.to_string(),
            tool_result: Ok(rmcp::model::CallToolResult::error(vec![
                rmcp::model::Content::text("command failed"),
            ])),
            metadata: None,
        };
        let response = tool_response_item(&response, 2000);
        assert_eq!(response.status.as_deref(), Some("failed"));
        assert!(response.title.is_none());

        let merged = coalesce_timeline_items(vec![request, response]);
        assert_eq!(merged[0].title.as_deref(), Some("shell"));
        assert_eq!(merged[0].status.as_deref(), Some("failed"));
    }

    #[test]
    fn system_notification_omits_structured_data_and_bounds_message() {
        let notification = SystemNotificationContent {
            notification_type: SystemNotificationType::InlineMessage,
            msg: "x".repeat(600),
            data: Some(json!({ "raw": "must-not-render" })),
        };

        let item = system_notification_item("message", 0, &notification, 1000);

        assert_eq!(item.title.as_deref(), Some("Agent notice"));
        assert_eq!(item.text.as_ref().unwrap().chars().count(), 501);
        assert!(item.text.as_ref().unwrap().ends_with('…'));
        assert!(item.output.is_none());
    }

    #[test]
    fn progress_notification_has_stable_title() {
        let notification = SystemNotificationContent {
            notification_type: SystemNotificationType::ProgressMessage,
            msg: "Loading...".to_string(),
            data: None,
        };

        let item = system_notification_item("message", 0, &notification, 1000);

        assert_eq!(item.title.as_deref(), Some("Progress"));
        assert_eq!(item.text.as_deref(), Some("Loading..."));
    }

    #[test]
    fn timeline_text_is_bounded_by_characters() {
        assert_eq!(bounded_timeline_text("éclair", 2), "éc…");
        assert_eq!(bounded_timeline_text("short", 10), "short");
    }
}
