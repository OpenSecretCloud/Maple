use crate::proxy;
use futures_util::StreamExt;
use goose::agents::{
    Agent, AgentConfig as GooseAgentConfig, AgentEvent, ExtensionConfig, GoosePlatform,
    SessionConfig,
};
use goose::config::{
    ConfigError, GooseMode, PermissionManager, DEFAULT_EXTENSION_DESCRIPTION,
    DEFAULT_EXTENSION_TIMEOUT,
};
use goose::conversation::message::{ActionRequiredData, Message, MessageContent};
use goose::execution::manager::AgentManager;
use goose::permission::permission_confirmation::PrincipalType;
use goose::permission::{Permission, PermissionConfirmation};
use goose::session::session_manager::{Session, SessionType};
use goose::session::SessionManager;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::HashMap;
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::str::FromStr;
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};
use tauri::{AppHandle, Emitter, Manager, State};
use tokio::sync::Mutex;
use tokio_util::sync::CancellationToken;

const DEFAULT_AGENT_MODEL: &str = "auto:powerful";
const DEFAULT_GOOSE_MODE: &str = "smart_approve";
const AGENT_EVENT_NAME: &str = "agent-event";

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentConfig {
    pub default_project_root: Option<String>,
    pub default_model: String,
    #[serde(default = "default_runtime_kind")]
    pub runtime_kind: String,
}

fn default_runtime_kind() -> String {
    "goose-direct".to_string()
}

impl Default for AgentConfig {
    fn default() -> Self {
        Self {
            default_project_root: None,
            default_model: DEFAULT_AGENT_MODEL.to_string(),
            runtime_kind: default_runtime_kind(),
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
    pub request_id: String,
    pub decision: String,
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

#[derive(Debug, Clone, Serialize)]
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
    #[serde(skip_serializing_if = "Option::is_none")]
    pub raw: Option<Value>,
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
    #[serde(skip_serializing_if = "Option::is_none")]
    pub details: Option<Value>,
}

struct ActiveAgentRun {
    token: CancellationToken,
}

struct AgentRuntime {
    agent_manager: Arc<AgentManager>,
    session_manager: Arc<SessionManager>,
    active_runs: HashMap<String, ActiveAgentRun>,
    project_root: PathBuf,
    model: String,
    mode: String,
    maple_proxy_base_url: String,
    config_dir: PathBuf,
    goose_path_root: PathBuf,
    log_path: PathBuf,
}

impl AgentRuntime {
    fn status(&self, error: Option<String>) -> AgentRuntimeStatus {
        AgentRuntimeStatus {
            running: true,
            project_root: Some(path_string(&self.project_root)),
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
    pending_permissions: Arc<Mutex<HashMap<String, String>>>,
}

impl AgentRuntimeState {
    pub fn new() -> Self {
        Self {
            inner: Arc::new(Mutex::new(None)),
            session_log: Arc::new(Mutex::new(())),
            pending_permissions: Arc::new(Mutex::new(HashMap::new())),
        }
    }
}

#[tauri::command]
pub async fn agent_get_runtime_status(
    app_handle: AppHandle,
    state: State<'_, AgentRuntimeState>,
) -> Result<AgentRuntimeStatus, String> {
    let config_dir = agent_config_dir(&app_handle).map_err(|e| e.to_string())?;
    let runtime = state.inner.lock().await;
    if let Some(current) = runtime.as_ref() {
        return Ok(current.status(None));
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
    {
        let runtime = state.inner.lock().await;
        if let Some(current) = runtime.as_ref() {
            return Ok(current.status(None));
        }
    }

    let agent_config = load_agent_config_inner(&app_handle).unwrap_or_default();
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

    let config_dir = agent_config_dir(&app_handle).map_err(|e| e.to_string())?;
    let goose_path_root = config_dir.join("goose");
    let log_dir = config_dir.join("logs");
    fs::create_dir_all(&log_dir).map_err(|e| format!("Failed to create log dir: {e}"))?;
    fs::create_dir_all(goose_path_root.join("data"))
        .map_err(|e| format!("Failed to create Goose data dir: {e}"))?;
    fs::create_dir_all(goose_path_root.join("config"))
        .map_err(|e| format!("Failed to create Goose config dir: {e}"))?;
    let log_path = log_dir.join("goose-direct.log");

    configure_embedded_goose(
        &goose_path_root,
        &model,
        &mode,
        &maple_proxy_base_url,
        &proxy_config.api_key,
    )?;

    let session_manager = Arc::new(SessionManager::new(goose_path_root.join("data")));
    let permission_manager = Arc::new(PermissionManager::new(goose_path_root.join("config")));
    let goose_mode = parse_goose_mode(&mode);
    let goose_config = GooseAgentConfig::new(
        Arc::clone(&session_manager),
        permission_manager,
        None,
        goose_mode,
        false,
        GoosePlatform::GooseDesktop,
    )
    .with_use_login_shell_path(true);
    let agent_manager = Arc::new(
        AgentManager::new(goose_config, None)
            .await
            .map_err(|e| format!("Failed to create Goose agent manager: {e}"))?,
    );

    append_runtime_log(
        &log_path,
        &format!(
            "started direct Goose runtime: project_root={}, proxy={}, model={}, mode={}",
            project_root.display(),
            maple_proxy_base_url,
            model,
            mode
        ),
    );

    let runtime = AgentRuntime {
        agent_manager,
        session_manager,
        active_runs: HashMap::new(),
        project_root: project_root.clone(),
        model: model.clone(),
        mode: mode.clone(),
        maple_proxy_base_url,
        config_dir: config_dir.clone(),
        goose_path_root,
        log_path,
    };
    let status = runtime.status(None);

    {
        let mut guard = state.inner.lock().await;
        *guard = Some(runtime);
    }

    let _ = save_recent_project_root_inner(&app_handle, &project_root);
    let mut next_config = load_agent_config_inner(&app_handle).unwrap_or_default();
    next_config.default_project_root = Some(path_string(&project_root));
    next_config.default_model = model;
    next_config.runtime_kind = default_runtime_kind();
    let _ = save_agent_config_inner(&app_handle, &next_config);

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
            details: None,
        },
    );

    Ok(status)
}

#[tauri::command]
pub async fn agent_stop_runtime(
    app_handle: AppHandle,
    state: State<'_, AgentRuntimeState>,
) -> Result<AgentRuntimeStatus, String> {
    let config_dir = agent_config_dir(&app_handle).map_err(|e| e.to_string())?;
    let mut runtime = state.inner.lock().await;
    if let Some(current) = runtime.as_mut() {
        for active_run in current.active_runs.values() {
            active_run.token.cancel();
        }
        append_runtime_log(&current.log_path, "stopped direct Goose runtime");
    }
    *runtime = None;
    state.pending_permissions.lock().await.clear();
    Ok(stopped_status(config_dir, None))
}

#[tauri::command]
pub async fn agent_restart_runtime(
    app_handle: AppHandle,
    state: State<'_, AgentRuntimeState>,
    proxy_state: State<'_, proxy::ProxyState>,
    request: Option<AgentStartRequest>,
) -> Result<AgentRuntimeStatus, String> {
    {
        let mut runtime = state.inner.lock().await;
        if let Some(current) = runtime.as_mut() {
            for active_run in current.active_runs.values() {
                active_run.token.cancel();
            }
        }
        *runtime = None;
    }
    state.pending_permissions.lock().await.clear();
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
pub async fn agent_create_session(
    app_handle: AppHandle,
    state: State<'_, AgentRuntimeState>,
    request: Option<AgentCreateSessionRequest>,
) -> Result<AgentSessionDetail, String> {
    let request = request.unwrap_or(AgentCreateSessionRequest {
        project_root: None,
        title: None,
        model: None,
        mode: None,
    });
    let (agent_manager, session_manager, runtime_project_root, runtime_model, runtime_mode) = {
        let runtime = state.inner.lock().await;
        let current = runtime
            .as_ref()
            .ok_or_else(|| "Agent runtime is not running".to_string())?;
        (
            Arc::clone(&current.agent_manager),
            Arc::clone(&current.session_manager),
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
        .unwrap_or_else(|| "New agent session".to_string());
    let mode = request.mode.unwrap_or(runtime_mode);
    let model = request.model.unwrap_or(runtime_model);
    let session = session_manager
        .create_session(
            root.clone(),
            title,
            SessionType::User,
            parse_goose_mode(&mode),
        )
        .await
        .map_err(|e| format!("Failed to create Goose session: {e}"))?;

    configure_session_agent(&agent_manager, &session, &model, &mode).await?;
    let summary = session_summary(&session);
    let _ = save_recent_project_root_inner(&app_handle, &root);
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
            details: None,
        },
    );
    Ok(detail)
}

#[tauri::command]
pub async fn agent_list_sessions(
    state: State<'_, AgentRuntimeState>,
    project_root: Option<String>,
) -> Result<Vec<AgentSessionSummary>, String> {
    let (session_manager, filter_root) = {
        let runtime = state.inner.lock().await;
        let current = runtime
            .as_ref()
            .ok_or_else(|| "Agent runtime is not running".to_string())?;
        let filter_root = project_root
            .as_deref()
            .filter(|value| !value.trim().is_empty())
            .map(|path| normalize_project_root(Path::new(path)))
            .transpose()?;
        (Arc::clone(&current.session_manager), filter_root)
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
    state: State<'_, AgentRuntimeState>,
    session_id: String,
) -> Result<AgentSessionDetail, String> {
    let session_manager = {
        let runtime = state.inner.lock().await;
        let current = runtime
            .as_ref()
            .ok_or_else(|| "Agent runtime is not running".to_string())?;
        Arc::clone(&current.session_manager)
    };
    let session = session_manager
        .get_session(&session_id, true)
        .await
        .map_err(|e| format!("Failed to load Goose session: {e}"))?;
    let timeline = session
        .conversation
        .as_ref()
        .map(|conversation| {
            let items = conversation
                .messages()
                .iter()
                .flat_map(|message| message_to_timeline_items(message, false))
                .collect::<Vec<_>>();
            coalesce_timeline_items(items)
        })
        .unwrap_or_default();

    Ok(AgentSessionDetail {
        session: session_summary(&session),
        timeline,
    })
}

#[tauri::command]
pub async fn agent_send_message(
    app_handle: AppHandle,
    state: State<'_, AgentRuntimeState>,
    request: AgentSendMessageRequest,
) -> Result<AgentRunResponse, String> {
    let text = request.text.trim().to_string();
    if text.is_empty() {
        return Err("Prompt cannot be empty".to_string());
    }

    let run_id = format!("run_{}", unix_ms());
    let cancel_token = CancellationToken::new();
    let (agent_manager, session_manager, log_path, model, mode) = {
        let runtime = state.inner.lock().await;
        let current = runtime
            .as_ref()
            .ok_or_else(|| "Agent runtime is not running".to_string())?;
        (
            Arc::clone(&current.agent_manager),
            Arc::clone(&current.session_manager),
            current.log_path.clone(),
            request
                .model
                .clone()
                .unwrap_or_else(|| current.model.clone()),
            request.mode.clone().unwrap_or_else(|| current.mode.clone()),
        )
    };

    let session = session_manager
        .get_session(&request.session_id, false)
        .await
        .map_err(|e| format!("Failed to load Goose session: {e}"))?;
    let agent = configure_session_agent(&agent_manager, &session, &model, &mode).await?;

    agent_manager
        .try_register_cancel_token(&request.session_id, cancel_token.clone())
        .await
        .map_err(|e| format!("Agent session is already running: {e}"))?;

    {
        let mut runtime = state.inner.lock().await;
        let Some(current) = runtime.as_mut() else {
            agent_manager.unregister_cancel_token(&request.session_id).await;
            return Err("Agent runtime is not running".to_string());
        };
        current.active_runs.insert(
            run_id.clone(),
            ActiveAgentRun {
                token: cancel_token.clone(),
            },
        );
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
            details: None,
        },
    );

    let user_item = AgentTimelineItem {
        id: format!("{run_id}-user"),
        item_type: "message".to_string(),
        role: Some("user".to_string()),
        title: None,
        text: Some(text.clone()),
        status: None,
        input: None,
        output: None,
        raw: None,
        created_ms: unix_ms(),
        merge: "replace".to_string(),
    };
    emit_timeline_item(&app_handle, &request.session_id, &run_id, user_item.clone());
    append_session_event(
        &app_handle,
        &state,
        &request.session_id,
        json!({
            "type": "userMessage",
            "runId": run_id,
            "item": user_item,
        }),
    )
    .await;

    let app_handle_for_task = app_handle.clone();
    let state_inner = Arc::clone(&state.inner);
    let session_log = Arc::clone(&state.session_log);
    let pending_permissions = Arc::clone(&state.pending_permissions);
    let session_id = request.session_id.clone();
    let task_run_id = run_id.clone();
    let task_agent_manager = Arc::clone(&agent_manager);

    tauri::async_runtime::spawn(async move {
        let result = run_agent_prompt(
            app_handle_for_task.clone(),
            agent,
            session_id.clone(),
            task_run_id.clone(),
            text,
            cancel_token.clone(),
            log_path.clone(),
            session_log,
            pending_permissions,
        )
        .await;

        {
            let mut runtime = state_inner.lock().await;
            if let Some(current) = runtime.as_mut() {
                current.active_runs.remove(&task_run_id);
            }
        }
        task_agent_manager.unregister_cancel_token(&session_id).await;

        let (status, message) = match result {
            Ok(()) if cancel_token.is_cancelled() => ("cancelled", None),
            Ok(()) => ("completed", None),
            Err(error) => ("failed", Some(error)),
        };
        if let Some(error) = message.as_ref() {
            append_runtime_log(&log_path, &format!("run failed: {error}"));
            emit_agent_event(
                &app_handle_for_task,
                AgentEventEnvelope {
                    event_type: "error".to_string(),
                    session_id: Some(session_id.clone()),
                    run_id: Some(task_run_id.clone()),
                    item: Some(error_item(error.clone(), None)),
                    status: None,
                    session: None,
                    message: Some(error.clone()),
                    details: None,
                },
            );
        }
        emit_agent_event(
            &app_handle_for_task,
            AgentEventEnvelope {
                event_type: "runFinished".to_string(),
                session_id: Some(session_id),
                run_id: Some(task_run_id),
                item: None,
                status: None,
                session: None,
                message: Some(status.to_string()),
                details: None,
            },
        );
    });

    Ok(AgentRunResponse { run_id })
}

#[tauri::command]
pub async fn agent_cancel_run(
    state: State<'_, AgentRuntimeState>,
    run_id: String,
) -> Result<(), String> {
    let runtime = state.inner.lock().await;
    let Some(current) = runtime.as_ref() else {
        return Ok(());
    };
    if let Some(active_run) = current.active_runs.get(&run_id) {
        active_run.token.cancel();
    }
    Ok(())
}

#[tauri::command]
pub async fn agent_permission_respond(
    state: State<'_, AgentRuntimeState>,
    response: AgentPermissionResponse,
) -> Result<(), String> {
    let (agent_manager, session_id) = {
        let runtime = state.inner.lock().await;
        let current = runtime
            .as_ref()
            .ok_or_else(|| "Agent runtime is not running".to_string())?;
        let session_id = state
            .pending_permissions
            .lock()
            .await
            .get(&response.request_id)
            .cloned()
            .ok_or_else(|| {
                format!(
                    "No pending Agent Mode permission request found for {}",
                    response.request_id
                )
            })?;
        (Arc::clone(&current.agent_manager), session_id)
    };
    let agent = agent_manager
        .get_or_create_agent(session_id)
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
    state
        .pending_permissions
        .lock()
        .await
        .remove(&response.request_id);
    Ok(())
}

#[tauri::command]
pub async fn agent_append_runtime_log(
    app_handle: AppHandle,
    message: String,
) -> Result<(), String> {
    let log_path = agent_config_dir(&app_handle)
        .map_err(|e| e.to_string())?
        .join("logs")
        .join("goose-direct.log");
    log::info!("[Agent Mode] {message}");
    append_runtime_log(&log_path, &format!("frontend {}", message));
    Ok(())
}

async fn run_agent_prompt(
    app_handle: AppHandle,
    agent: Arc<Agent>,
    session_id: String,
    run_id: String,
    text: String,
    cancel_token: CancellationToken,
    log_path: PathBuf,
    session_log: Arc<Mutex<()>>,
    pending_permissions: Arc<Mutex<HashMap<String, String>>>,
) -> Result<(), String> {
    append_runtime_log(
        &log_path,
        &format!("run started: session={}, run={}", session_id, run_id),
    );
    let session_config = SessionConfig {
        id: session_id.clone(),
        schedule_id: None,
        max_turns: None,
        retry_config: None,
    };
    let user_message = Message::user().with_text(text).with_generated_id();
    let mut stream = agent
        .reply(user_message, session_config, Some(cancel_token.clone()))
        .await
        .map_err(|e| format!("Goose reply failed: {e}"))?;

    while let Some(event) = stream.next().await {
        if cancel_token.is_cancelled() {
            break;
        }
        match event {
            Ok(AgentEvent::Message(message)) => {
                for item in message_to_timeline_items(&message, true) {
                    if let Some(request_id) = pending_permission_request_id(&item) {
                        pending_permissions
                            .lock()
                            .await
                            .insert(request_id, session_id.clone());
                    }
                    emit_timeline_item(&app_handle, &session_id, &run_id, item.clone());
                    append_session_event_with_lock(
                        &app_handle,
                        Arc::clone(&session_log),
                        &session_id,
                        json!({
                            "type": "timelineItem",
                            "runId": run_id,
                            "item": item,
                        }),
                    )
                    .await;
                }
            }
            Ok(AgentEvent::Usage(usage)) => {
                let item = AgentTimelineItem {
                    id: format!("{run_id}-usage"),
                    item_type: "usage".to_string(),
                    role: None,
                    title: Some("Usage".to_string()),
                    text: None,
                    status: None,
                    input: None,
                    output: None,
                    raw: Some(serde_json::to_value(usage).unwrap_or(Value::Null)),
                    created_ms: unix_ms(),
                    merge: "replace".to_string(),
                };
                emit_timeline_item(&app_handle, &session_id, &run_id, item.clone());
                append_session_event_with_lock(
                    &app_handle,
                    Arc::clone(&session_log),
                    &session_id,
                    json!({
                        "type": "timelineItem",
                        "runId": run_id,
                        "item": item,
                    }),
                )
                .await;
            }
            Ok(AgentEvent::McpNotification((request_id, notification))) => {
                let item = AgentTimelineItem {
                    id: format!("notification-{request_id}"),
                    item_type: "system".to_string(),
                    role: Some("system".to_string()),
                    title: Some("Tool notification".to_string()),
                    text: Some(format!("{notification:?}")),
                    status: None,
                    input: None,
                    output: None,
                    raw: None,
                    created_ms: unix_ms(),
                    merge: "replace".to_string(),
                };
                emit_timeline_item(&app_handle, &session_id, &run_id, item);
            }
            Ok(AgentEvent::HistoryReplaced(conversation)) => {
                emit_agent_event(
                    &app_handle,
                    AgentEventEnvelope {
                        event_type: "historyReplaced".to_string(),
                        session_id: Some(session_id.clone()),
                        run_id: Some(run_id.clone()),
                        item: None,
                        status: None,
                        session: None,
                        message: Some(format!("{} messages", conversation.len())),
                        details: None,
                    },
                );
            }
            Err(error) => {
                return Err(format!("Goose stream failed: {error}"));
            }
        }
    }

    append_runtime_log(
        &log_path,
        &format!("run finished: session={}, run={}", session_id, run_id),
    );
    Ok(())
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
        .update_goose_mode(parse_goose_mode(mode), &session.id)
        .await
        .map_err(|e| format!("Failed to update Goose mode: {e}"))?;
    let developer = ExtensionConfig::Builtin {
        name: "developer".to_string(),
        description: DEFAULT_EXTENSION_DESCRIPTION.to_string(),
        display_name: Some("Developer".to_string()),
        timeout: Some(DEFAULT_EXTENSION_TIMEOUT),
        bundled: Some(true),
        available_tools: Vec::new(),
    };
    agent
        .add_extension(developer, &session.id)
        .await
        .map_err(|e| format!("Failed to enable Goose developer tools: {e}"))?;
    Ok(agent)
}

fn message_to_timeline_items(message: &Message, live: bool) -> Vec<AgentTimelineItem> {
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
                raw: None,
                created_ms,
                merge: merge.clone(),
            }),
            MessageContent::Thinking(thinking) => Some(AgentTimelineItem {
                id: format!("{base_id}-thinking-{index}"),
                item_type: "thinking".to_string(),
                role: Some("thought".to_string()),
                title: Some("Thinking".to_string()),
                text: Some(thinking.thinking.clone()),
                status: None,
                input: None,
                output: None,
                raw: None,
                created_ms,
                merge: merge.clone(),
            }),
            MessageContent::RedactedThinking(_) => Some(AgentTimelineItem {
                id: format!("{base_id}-redacted-thinking-{index}"),
                item_type: "thinking".to_string(),
                role: Some("thought".to_string()),
                title: Some("Thinking".to_string()),
                text: Some("Thinking redacted by provider.".to_string()),
                status: None,
                input: None,
                output: None,
                raw: None,
                created_ms,
                merge: "replace".to_string(),
            }),
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
                raw: Some(serde_json::to_value(request).unwrap_or(Value::Null)),
                created_ms,
                merge: "replace".to_string(),
            }),
            MessageContent::ActionRequired(action) => {
                Some(action_required_item(action, created_ms))
            }
            MessageContent::FrontendToolRequest(request) => {
                let (title, input, status, raw) = match &request.tool_call {
                    Ok(call) => (
                        format_tool_title(call.name.as_ref()),
                        Some(serde_json::to_value(&call.arguments).unwrap_or(Value::Null)),
                        "pending".to_string(),
                        serde_json::to_value(call).unwrap_or(Value::Null),
                    ),
                    Err(error) => (
                        "Tool call parse failed".to_string(),
                        None,
                        "failed".to_string(),
                        json!({ "error": error.to_string() }),
                    ),
                };
                Some(AgentTimelineItem {
                    id: request.id.clone(),
                    item_type: "tool".to_string(),
                    role: Some("assistant".to_string()),
                    title: Some(title),
                    text: None,
                    status: Some(status),
                    input,
                    output: None,
                    raw: Some(raw),
                    created_ms,
                    merge: "replace".to_string(),
                })
            }
            MessageContent::SystemNotification(notification) => Some(AgentTimelineItem {
                id: format!("{base_id}-system-{index}"),
                item_type: "system".to_string(),
                role: Some("system".to_string()),
                title: Some(format!("{:?}", notification.notification_type)),
                text: Some(notification.msg.clone()),
                status: None,
                input: None,
                output: notification.data.clone(),
                raw: Some(serde_json::to_value(notification).unwrap_or(Value::Null)),
                created_ms,
                merge: "replace".to_string(),
            }),
            MessageContent::Image(_) => None,
        })
        .collect()
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
            raw: Some(serde_json::to_value(request).unwrap_or(Value::Null)),
            created_ms,
            merge: "replace".to_string(),
        },
        Err(error) => error_item(
            "Tool call parse failed".to_string(),
            Some(json!({
                "id": request.id,
                "error": error.to_string(),
                "raw": request,
            })),
        ),
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
            AgentTimelineItem {
                id: response.id.clone(),
                item_type: "tool".to_string(),
                role: Some("assistant".to_string()),
                title: tool_name_from_id(&response.id).map(|name| format_tool_title(&name)),
                text: None,
                status: Some("completed".to_string()),
                input: None,
                output: Some(json!({
                    "text": text,
                    "isError": result.is_error,
                    "raw": result,
                })),
                raw: Some(serde_json::to_value(response).unwrap_or(Value::Null)),
                created_ms,
                merge: "replace".to_string(),
            }
        }
        Err(error) => AgentTimelineItem {
            id: response.id.clone(),
            item_type: "tool".to_string(),
            role: Some("assistant".to_string()),
            title: tool_name_from_id(&response.id).map(|name| format_tool_title(&name)),
            text: Some(error.to_string()),
            status: Some("failed".to_string()),
            input: None,
            output: Some(json!({ "error": error.to_string() })),
            raw: Some(serde_json::to_value(response).unwrap_or(Value::Null)),
            created_ms,
            merge: "replace".to_string(),
        },
    }
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
        raw: incoming.raw.or(previous.raw),
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
            raw: Some(serde_json::to_value(action).unwrap_or(Value::Null)),
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
            raw: Some(serde_json::to_value(action).unwrap_or(Value::Null)),
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
            output: Some(serde_json::to_value(action).unwrap_or(Value::Null)),
            raw: Some(serde_json::to_value(action).unwrap_or(Value::Null)),
            created_ms,
            merge: "replace".to_string(),
        },
    }
}

fn error_item(message: String, details: Option<Value>) -> AgentTimelineItem {
    AgentTimelineItem {
        id: format!("error-{}", unix_ms()),
        item_type: "error".to_string(),
        role: Some("system".to_string()),
        title: Some("Agent error".to_string()),
        text: Some(message),
        status: Some("failed".to_string()),
        input: None,
        output: None,
        raw: details,
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
    let name = id
        .strip_prefix("functions.")
        .unwrap_or(id)
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
        "always_allow" => Ok(Permission::AlwaysAllow),
        "deny_once" | "deny" => Ok(Permission::DenyOnce),
        "always_deny" => Ok(Permission::AlwaysDeny),
        "cancel" => Ok(Permission::Cancel),
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
            details: None,
        },
    );
}

fn emit_agent_event(app_handle: &AppHandle, event: AgentEventEnvelope) {
    if let Err(error) = app_handle.emit(AGENT_EVENT_NAME, event) {
        log::warn!("Failed to emit Agent Mode event: {error}");
    }
}

async fn append_session_event(
    app_handle: &AppHandle,
    state: &State<'_, AgentRuntimeState>,
    session_id: &str,
    event: Value,
) {
    append_session_event_with_lock(
        app_handle,
        Arc::clone(&state.session_log),
        session_id,
        event,
    )
    .await;
}

async fn append_session_event_with_lock(
    app_handle: &AppHandle,
    session_log: Arc<Mutex<()>>,
    session_id: &str,
    event: Value,
) {
    let _guard = session_log.lock().await;
    if let Err(error) = append_session_event_inner(app_handle, session_id, event) {
        log::warn!("Failed to append Agent Mode session log: {error}");
    }
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
        .set_secret("OPENAI_API_KEY", &proxy_api_key.to_string())
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

fn parse_goose_mode(mode: &str) -> GooseMode {
    GooseMode::from_str(mode).unwrap_or(GooseMode::SmartApprove)
}

fn stopped_status(config_dir: PathBuf, error: Option<String>) -> AgentRuntimeStatus {
    AgentRuntimeStatus {
        running: false,
        project_root: None,
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
    let row = json!({
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

#[cfg(test)]
mod tests {
    use super::*;

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
            raw: Some(json!({ "request": true })),
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
            raw: Some(json!({ "response": true })),
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
        assert_eq!(items[0].raw, Some(json!({ "response": true })));
    }
}
