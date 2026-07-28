use crate::agent::{
    cancel_agent_run_for_user, clear_agent_session_tool_environment, create_agent_session_for_user,
    delete_agent_session_for_user, ensure_agent_runtime_for_user, send_agent_message_for_user,
    set_agent_session_tool_environment, subscribe_agent_events, AgentCreateSessionRequest,
    AgentEventEnvelope, AgentRunTerminal, AgentSendMessageRequest, SharedAgentToolEnvironment,
};
use agent_client_protocol::schema::v1::{
    AgentCapabilities, CancelNotification, ContentBlock, ContentChunk, Implementation,
    InitializeRequest, InitializeResponse, McpServer, NewSessionRequest, NewSessionResponse,
    PromptCapabilities, PromptRequest, PromptResponse, SessionNotification, SessionUpdate,
    StopReason, TextContent,
};
use agent_client_protocol::util::MatchDispatchFrom;
use agent_client_protocol::{
    Agent as AcpAgent, ByteStreams, Client, ConnectionTo, Dispatch, HandleDispatchFrom, Handled,
    JsonRpcNotification, Responder,
};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::Arc;
use tauri::{AppHandle, Manager};
use tokio::sync::{Mutex, RwLock, Semaphore};
use tokio_util::compat::{TokioAsyncReadCompatExt as _, TokioAsyncWriteCompatExt as _};
use tokio_util::sync::CancellationToken;

#[cfg(target_os = "linux")]
use std::os::unix::fs::{DirBuilderExt as _, MetadataExt as _};
#[cfg(unix)]
use std::os::unix::fs::{FileTypeExt as _, PermissionsExt as _};
#[cfg(unix)]
use tokio::net::{UnixListener, UnixStream};

const ACP_PROTOCOL_VERSION: u16 = 1;
const MAX_ACP_CONNECTIONS: usize = 8;
const MAX_ACP_ERROR_CHARS: usize = 500;
const MAX_ACP_FRAME_BYTES: usize = 10 * 1024 * 1024;
const ACP_CONNECTION_CLEANUP_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(5);
const BRIDGE_HELLO_METHOD: &str = "_maple/bridge/hello";
const ALLOWED_BRIDGE_ENV: [&str; 6] = [
    "BUZZ_RELAY_URL",
    "BUZZ_PRIVATE_KEY",
    "BUZZ_AUTH_TAG",
    "BUZZ_API_TOKEN",
    "BUZZ_ACP_DISPLAY_NAME",
    "PATH",
];

#[cfg(unix)]
struct BoundedLineReader<R> {
    inner: R,
    bytes_since_newline: usize,
    eof: CancellationToken,
}

#[cfg(unix)]
impl<R> BoundedLineReader<R> {
    fn new(inner: R, eof: CancellationToken) -> Self {
        Self {
            inner,
            bytes_since_newline: 0,
            eof,
        }
    }
}

#[cfg(unix)]
impl<R: tokio::io::AsyncRead + Unpin> tokio::io::AsyncRead for BoundedLineReader<R> {
    fn poll_read(
        mut self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
        buf: &mut tokio::io::ReadBuf<'_>,
    ) -> std::task::Poll<std::io::Result<()>> {
        let previous_len = buf.filled().len();
        match std::pin::Pin::new(&mut self.inner).poll_read(cx, buf) {
            std::task::Poll::Ready(Ok(())) => {
                if buf.filled().len() == previous_len {
                    self.eof.cancel();
                }
                for byte in &buf.filled()[previous_len..] {
                    if *byte == b'\n' {
                        self.bytes_since_newline = 0;
                    } else {
                        self.bytes_since_newline = self.bytes_since_newline.saturating_add(1);
                        if self.bytes_since_newline > MAX_ACP_FRAME_BYTES {
                            return std::task::Poll::Ready(Err(std::io::Error::new(
                                std::io::ErrorKind::InvalidData,
                                "ACP frame exceeds the 10 MiB limit",
                            )));
                        }
                    }
                }
                std::task::Poll::Ready(Ok(()))
            }
            other => other,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum AgentAcpPermissionMode {
    ReadOnly,
    AllowAll,
}

impl AgentAcpPermissionMode {
    fn maple_mode(&self) -> &'static str {
        match self {
            Self::ReadOnly => "smart_approve",
            Self::AllowAll => "auto",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct AgentAcpConfig {
    #[serde(default)]
    pub enabled: bool,
    #[serde(default = "default_permission_mode")]
    pub permission_mode: AgentAcpPermissionMode,
    #[serde(default)]
    pub allowed_project_roots: Vec<String>,
    #[serde(default = "default_max_connections")]
    pub max_connections: usize,
}

fn default_permission_mode() -> AgentAcpPermissionMode {
    AgentAcpPermissionMode::ReadOnly
}

fn default_max_connections() -> usize {
    1
}

impl Default for AgentAcpConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            permission_mode: default_permission_mode(),
            allowed_project_roots: Vec::new(),
            max_connections: default_max_connections(),
        }
    }
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentAcpHarness {
    pub command: String,
    pub args: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentAcpStatus {
    pub running: bool,
    pub enabled: bool,
    pub connected_clients: usize,
    pub active_sessions: usize,
    pub active_runs: usize,
    pub endpoint: Option<String>,
    pub endpoint_kind: Option<String>,
    pub protocol_version: u16,
    pub error: Option<String>,
    pub buzz_credentials_available: bool,
    pub harness: AgentAcpHarness,
}

#[derive(Default)]
struct AgentAcpStats {
    running: AtomicBool,
    connected_clients: AtomicUsize,
    active_sessions: AtomicUsize,
    active_runs: AtomicUsize,
    credential_connections: AtomicUsize,
    last_error: Mutex<Option<String>>,
}

struct RunningAgentAcp {
    user_id: String,
    endpoint: PathBuf,
    config: Arc<RwLock<AgentAcpConfig>>,
    stats: Arc<AgentAcpStats>,
    cancellation: CancellationToken,
    task: tauri::async_runtime::JoinHandle<()>,
}

pub struct AgentAcpState {
    lifecycle: Mutex<()>,
    running: Mutex<Option<RunningAgentAcp>>,
}

impl AgentAcpState {
    pub fn new() -> Self {
        Self {
            lifecycle: Mutex::new(()),
            running: Mutex::new(None),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonRpcNotification)]
#[notification(method = "_maple/bridge/hello")]
struct BridgeHelloNotification {
    environment: HashMap<String, String>,
}

struct AcpConnectionContext {
    app_handle: AppHandle,
    user_id: String,
    config: Arc<RwLock<AgentAcpConfig>>,
    stats: Arc<AgentAcpStats>,
    bridge_environment: Mutex<HashMap<String, String>>,
    sessions: Mutex<HashMap<String, Option<SharedAgentToolEnvironment>>>,
    prompt_states: Mutex<HashMap<String, AcpPromptState>>,
    background_tasks: Mutex<tokio::task::JoinSet<()>>,
    finalization: Mutex<()>,
    closed: AtomicBool,
    has_credentials: AtomicBool,
}

enum AcpPromptState {
    Starting { cancel_requested: bool },
    Running { run_id: String },
}

impl AcpConnectionContext {
    fn new(
        app_handle: AppHandle,
        user_id: String,
        config: Arc<RwLock<AgentAcpConfig>>,
        stats: Arc<AgentAcpStats>,
    ) -> Arc<Self> {
        Arc::new(Self {
            app_handle,
            user_id,
            config,
            stats,
            bridge_environment: Mutex::new(HashMap::new()),
            sessions: Mutex::new(HashMap::new()),
            prompt_states: Mutex::new(HashMap::new()),
            background_tasks: Mutex::new(tokio::task::JoinSet::new()),
            finalization: Mutex::new(()),
            closed: AtomicBool::new(false),
            has_credentials: AtomicBool::new(false),
        })
    }

    async fn set_bridge_environment(&self, environment: HashMap<String, String>) {
        let _finalization = self.finalization.lock().await;
        if self.closed.load(Ordering::SeqCst) {
            return;
        }
        let environment = filter_bridge_environment(environment);
        let has_credentials = has_buzz_credentials(&environment);
        *self.bridge_environment.lock().await = environment;
        if has_credentials && !self.has_credentials.swap(true, Ordering::SeqCst) {
            self.stats
                .credential_connections
                .fetch_add(1, Ordering::SeqCst);
        }
    }

    async fn new_session(
        &self,
        request: NewSessionRequest,
    ) -> Result<NewSessionResponse, agent_client_protocol::Error> {
        if self.closed.load(Ordering::SeqCst) {
            return Err(agent_client_protocol::Error::internal_error()
                .data("The Maple ACP connection is closing"));
        }
        if !request.cwd.is_absolute() {
            return Err(agent_client_protocol::Error::invalid_params()
                .data("ACP session cwd must be an absolute path"));
        }
        let config = self.config.read().await.clone();
        ensure_allowed_project_root(&request.cwd, &config.allowed_project_roots)
            .map_err(|error| agent_client_protocol::Error::invalid_params().data(error))?;

        let mut environment = self.bridge_environment.lock().await.clone();
        merge_mcp_environment(&mut environment, &request.mcp_servers)?;
        let mode = config.permission_mode.maple_mode().to_string();
        let detail = create_agent_session_for_user(
            &self.app_handle,
            self.user_id.clone(),
            Some(AgentCreateSessionRequest {
                project_root: Some(request.cwd.to_string_lossy().into_owned()),
                title: Some("Buzz ACP".to_string()),
                model: None,
                context_limit: None,
                mode: Some(mode),
                mcp_server_names: None,
            }),
        )
        .await
        .map_err(internal_acp_error)?;
        let session_id = detail.session.id;
        if self
            .sessions
            .lock()
            .await
            .insert(session_id.clone(), None)
            .is_none()
        {
            self.stats.active_sessions.fetch_add(1, Ordering::SeqCst);
        }
        if self.closed.load(Ordering::SeqCst) {
            self.discard_uncommitted_session(&session_id).await;
            return Err(agent_client_protocol::Error::internal_error()
                .data("The Maple ACP connection closed while creating the session"));
        }
        let tool_environment = match set_agent_session_tool_environment(
            &self.app_handle,
            &self.user_id,
            &session_id,
            environment.clone(),
        )
        .await
        {
            Ok(tool_environment) => tool_environment,
            Err(error) => {
                self.discard_uncommitted_session(&session_id).await;
                return Err(internal_acp_error(error));
            }
        };
        let finalization = self.finalization.lock().await;
        if self.closed.load(Ordering::SeqCst) {
            tool_environment.write().await.clear();
            drop(finalization);
            self.discard_uncommitted_session(&session_id).await;
            return Err(agent_client_protocol::Error::internal_error()
                .data("The Maple ACP connection closed while configuring the session"));
        }
        let environment_slot = self.sessions.lock().await.get_mut(&session_id).map(|slot| {
            *slot = Some(tool_environment);
        });
        if environment_slot.is_none() {
            drop(finalization);
            self.discard_uncommitted_session(&session_id).await;
            return Err(agent_client_protocol::Error::internal_error()
                .data("The Maple ACP connection lost its new session"));
        }
        if has_buzz_credentials(&environment) && !self.has_credentials.swap(true, Ordering::SeqCst)
        {
            self.stats
                .credential_connections
                .fetch_add(1, Ordering::SeqCst);
        }
        drop(finalization);
        Ok(NewSessionResponse::new(session_id))
    }

    async fn discard_uncommitted_session(&self, session_id: &str) {
        let _ =
            clear_agent_session_tool_environment(&self.app_handle, &self.user_id, session_id).await;
        let _ = delete_agent_session_for_user(
            &self.app_handle,
            self.user_id.clone(),
            session_id.to_string(),
        )
        .await;
        if self.sessions.lock().await.remove(session_id).is_some() {
            self.stats.active_sessions.fetch_sub(1, Ordering::SeqCst);
        }
    }

    async fn begin_prompt(
        &self,
        request: &PromptRequest,
    ) -> Result<String, agent_client_protocol::Error> {
        if self.closed.load(Ordering::SeqCst) {
            return Err(agent_client_protocol::Error::internal_error()
                .data("The Maple ACP connection is closing"));
        }
        let session_id = request.session_id.0.to_string();
        if !self.sessions.lock().await.contains_key(&session_id) {
            return Err(
                agent_client_protocol::Error::resource_not_found(Some(session_id.clone()))
                    .data("ACP session is not owned by this connection"),
            );
        }
        let prompt = prompt_text(&request.prompt)?;
        let mut states = self.prompt_states.lock().await;
        if states.contains_key(&session_id) {
            return Err(agent_client_protocol::Error::invalid_request()
                .data("This ACP session already has an active prompt"));
        }
        states.insert(
            session_id,
            AcpPromptState::Starting {
                cancel_requested: false,
            },
        );
        Ok(prompt)
    }

    async fn prompt(
        &self,
        cx: &ConnectionTo<Client>,
        request: PromptRequest,
        prompt: String,
    ) -> Result<PromptResponse, agent_client_protocol::Error> {
        let session_id = request.session_id.0.to_string();
        let config = self.config.read().await.clone();
        let mut events = subscribe_agent_events(&self.app_handle);
        let run = match send_agent_message_for_user(
            &self.app_handle,
            self.user_id.clone(),
            AgentSendMessageRequest {
                session_id: session_id.clone(),
                text: prompt,
                model: None,
                context_limit: None,
                mode: Some(config.permission_mode.maple_mode().to_string()),
                vision_capable: false,
            },
        )
        .await
        {
            Ok(run) => run,
            Err(error) => {
                self.prompt_states.lock().await.remove(&session_id);
                return Err(internal_acp_error(error));
            }
        };
        let run_id = run.run_id;
        let mut terminal = run.terminal;
        let cancel_requested = {
            let mut states = self.prompt_states.lock().await;
            match states.get_mut(&session_id) {
                Some(state @ AcpPromptState::Starting { .. }) => {
                    let requested = match state {
                        AcpPromptState::Starting { cancel_requested } => *cancel_requested,
                        AcpPromptState::Running { .. } => unreachable!(),
                    };
                    *state = AcpPromptState::Running {
                        run_id: run_id.clone(),
                    };
                    Some(requested)
                }
                _ => None,
            }
        };
        let Some(cancel_requested) = cancel_requested else {
            let _ =
                cancel_agent_run_for_user(&self.app_handle, self.user_id.clone(), run_id.clone())
                    .await;
            return Err(agent_client_protocol::Error::internal_error()
                .data("The Maple ACP connection closed while starting the prompt"));
        };
        self.stats.active_runs.fetch_add(1, Ordering::SeqCst);
        if cancel_requested {
            // A cancellation failure does not make the active Maple run
            // disappear. Keep listening so its lifecycle remains tracked.
            let _ =
                cancel_agent_run_for_user(&self.app_handle, self.user_id.clone(), run_id.clone())
                    .await;
        }

        let mut observed_terminal = None;
        let mut event_stream_lagged = false;
        let result = loop {
            tokio::select! {
                event = events.recv() => match event {
                    Ok(event)
                        if event.session_id.as_deref() == Some(session_id.as_str())
                            && event.run_id.as_deref() == Some(run_id.as_str()) =>
                    {
                        if event.event_type == "timelineItem" {
                            if let Some(update) = timeline_update(&event) {
                                if let Err(error) = cx.send_notification(SessionNotification::new(
                                    request.session_id.clone(),
                                    update,
                                )) {
                                    break Err(error);
                                }
                            }
                        } else if event.event_type == "error" {
                            if let Some(message) = event_error_text(&event) {
                                if let Err(error) = cx.send_notification(SessionNotification::new(
                                    request.session_id.clone(),
                                    SessionUpdate::AgentMessageChunk(ContentChunk::new(
                                        ContentBlock::Text(TextContent::new(message)),
                                    )),
                                )) {
                                    break Err(error);
                                }
                            }
                        } else if event.event_type == "runFinished" {
                            let terminal = match event.message.as_deref() {
                                Some("cancelled") => AgentRunTerminal::Cancelled,
                                Some("failed") => AgentRunTerminal::Failed,
                                _ => AgentRunTerminal::Completed,
                            };
                            break prompt_result_from_terminal(terminal);
                        }
                    }
                    Ok(_) => {}
                    Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => {
                        // Once any frames were lost, the retained per-run signal
                        // becomes authoritative. It cannot be overwritten by
                        // unrelated Maple UI traffic.
                        event_stream_lagged = true;
                        if let Some(terminal) = observed_terminal {
                            break prompt_result_from_terminal(terminal);
                        }
                    }
                    Err(tokio::sync::broadcast::error::RecvError::Closed) => {
                        if let Some(terminal) = observed_terminal.or_else(|| *terminal.borrow()) {
                            break prompt_result_from_terminal(terminal);
                        }
                        break Err(agent_client_protocol::Error::internal_error()
                            .data("Maple Agent event stream closed"));
                    }
                },
                changed = terminal.changed(), if observed_terminal.is_none() => {
                    match changed {
                        Ok(()) => {
                            observed_terminal = *terminal.borrow_and_update();
                            if event_stream_lagged {
                                if let Some(terminal) = observed_terminal {
                                    break prompt_result_from_terminal(terminal);
                                }
                            }
                            // In the ordinary path runFinished was broadcast
                            // before this signal. Keep draining the ordered event
                            // stream so Buzz receives every available text chunk.
                        }
                        Err(_) => {
                            if let Some(terminal) = *terminal.borrow() {
                                observed_terminal = Some(terminal);
                                if event_stream_lagged {
                                    break prompt_result_from_terminal(terminal);
                                }
                            } else {
                                break Err(agent_client_protocol::Error::internal_error()
                                    .data("Maple Agent run ended without a terminal result"));
                            }
                        }
                    }
                }
            }
        };
        if result.is_err() {
            // If the ACP client disappears while Maple is still producing a
            // turn, stop the underlying run before removing it from this
            // connection's cleanup map. A completed/failed run simply makes
            // this best-effort cancellation a no-op.
            let _ = cancel_agent_run_for_user(&self.app_handle, self.user_id.clone(), run_id).await;
        }
        if matches!(
            self.prompt_states.lock().await.remove(&session_id),
            Some(AcpPromptState::Running { .. })
        ) {
            self.stats.active_runs.fetch_sub(1, Ordering::SeqCst);
        }
        result
    }

    async fn cancel(
        &self,
        notification: CancelNotification,
    ) -> Result<(), agent_client_protocol::Error> {
        let session_id = notification.session_id.0.to_string();
        let run_id = {
            let mut states = self.prompt_states.lock().await;
            match states.get_mut(&session_id) {
                Some(AcpPromptState::Starting { cancel_requested }) => {
                    *cancel_requested = true;
                    None
                }
                Some(AcpPromptState::Running { run_id }) => Some(run_id.clone()),
                None => None,
            }
        };
        if let Some(run_id) = run_id {
            cancel_agent_run_for_user(&self.app_handle, self.user_id.clone(), run_id)
                .await
                .map_err(internal_acp_error)?;
        }
        Ok(())
    }

    async fn cleanup(&self) {
        {
            // Linearize closure with the last new-session credential commit.
            // A task that reaches finalization after this point observes closed
            // and rolls its newly persisted session back instead of committing.
            let _finalization = self.finalization.lock().await;
            self.closed.store(true, Ordering::SeqCst);
            self.bridge_environment.lock().await.clear();
            if self.has_credentials.swap(false, Ordering::SeqCst) {
                self.stats
                    .credential_connections
                    .fetch_sub(1, Ordering::SeqCst);
            }
        }
        let prompt_states = std::mem::take(&mut *self.prompt_states.lock().await);
        for state in prompt_states.into_values() {
            if let AcpPromptState::Running { run_id } = state {
                let _ =
                    cancel_agent_run_for_user(&self.app_handle, self.user_id.clone(), run_id).await;
                self.stats.active_runs.fetch_sub(1, Ordering::SeqCst);
            }
        }
        let sessions = std::mem::take(&mut *self.sessions.lock().await);
        for environment in sessions.values().flatten() {
            // Clear the exact Arc installed in Maple's developer client. This
            // revokes Buzz secrets without waiting behind runtime setup locks.
            environment.write().await.clear();
        }
        self.stats
            .active_sessions
            .fetch_sub(sessions.len(), Ordering::SeqCst);
        let mut tasks = self.background_tasks.lock().await;
        let deadline = tokio::time::Instant::now() + ACP_CONNECTION_CLEANUP_TIMEOUT;
        loop {
            match tokio::time::timeout_at(deadline, tasks.join_next()).await {
                Ok(Some(_)) => {}
                Ok(None) => break,
                Err(_) => {
                    // Session creation and the pre-run prompt path both cross
                    // persistent core state before returning an ID. Aborting
                    // them here could orphan that state. Detaching preserves
                    // their existing closed checks and rollback/cancel paths
                    // while keeping connection shutdown bounded.
                    tasks.detach_all();
                    break;
                }
            }
        }
        drop(tasks);
    }
}

#[derive(Clone)]
struct MapleAcpHandler {
    context: Arc<AcpConnectionContext>,
}

impl HandleDispatchFrom<Client> for MapleAcpHandler {
    fn describe_chain(&self) -> impl std::fmt::Debug {
        "maple-acp"
    }

    fn handle_dispatch_from(
        &mut self,
        message: Dispatch,
        cx: ConnectionTo<Client>,
    ) -> impl std::future::Future<Output = Result<Handled<Dispatch>, agent_client_protocol::Error>> + Send
    {
        let context = Arc::clone(&self.context);
        Box::pin(async move {
            MatchDispatchFrom::new(message, &cx)
                .if_notification({
                    let context = Arc::clone(&context);
                    |notification: BridgeHelloNotification| async move {
                        context.set_bridge_environment(notification.environment).await;
                        Ok(())
                    }
                })
                .await
                .if_request(
                    |_request: InitializeRequest, responder: Responder<InitializeResponse>| async {
                        let capabilities = AgentCapabilities::new().prompt_capabilities(
                            PromptCapabilities::new()
                                .image(false)
                                .audio(false)
                                .embedded_context(false),
                        );
                        responder.respond(
                            InitializeResponse::new(
                                agent_client_protocol::schema::ProtocolVersion::V1,
                            )
                            .agent_info(Implementation::new("maple", env!("CARGO_PKG_VERSION")))
                            .agent_capabilities(capabilities),
                        )
                    },
                )
                .await
                .if_request({
                    let context = Arc::clone(&context);
                    |request: NewSessionRequest, responder: Responder<NewSessionResponse>| async move {
                        let task_context = Arc::clone(&context);
                        let mut tasks = context.background_tasks.lock().await;
                        while tasks.try_join_next().is_some() {}
                        if context.closed.load(Ordering::SeqCst) {
                            responder.respond_with_error(
                                agent_client_protocol::Error::internal_error()
                                    .data("The Maple ACP connection is closing"),
                            )?;
                            return Ok(());
                        }
                        tasks.spawn(async move {
                            let _ = responder
                                .respond_with_result(task_context.new_session(request).await);
                        });
                        Ok(())
                    }
                })
                .await
                .if_request({
                    let context = Arc::clone(&context);
                    let cx = cx.clone();
                    |request: PromptRequest, responder: Responder<PromptResponse>| async move {
                        let prompt = context.begin_prompt(&request).await?;
                        let prompt_cx = cx.clone();
                        let prompt_context = Arc::clone(&context);
                        let mut tasks = context.background_tasks.lock().await;
                        while tasks.try_join_next().is_some() {}
                        if context.closed.load(Ordering::SeqCst) {
                            context.prompt_states.lock().await.remove(
                                &request.session_id.0.to_string(),
                            );
                            responder.respond_with_error(
                                agent_client_protocol::Error::internal_error()
                                    .data("The Maple ACP connection is closing"),
                            )?;
                            return Ok(());
                        }
                        tasks.spawn(async move {
                            let _ = responder.respond_with_result(
                                prompt_context.prompt(&prompt_cx, request, prompt).await,
                            );
                        });
                        Ok(())
                    }
                })
                .await
                .if_notification({
                    let context = Arc::clone(&context);
                    |notification: CancelNotification| async move {
                        context.cancel(notification).await
                    }
                })
                .await
                .otherwise({
                    let cx = cx.clone();
                    |message: Dispatch| async move {
                        match message {
                            Dispatch::Request(_, responder) => {
                                responder.respond_with_error(
                                    agent_client_protocol::Error::method_not_found(),
                                )?;
                            }
                            Dispatch::Response(result, router) => {
                                router.respond_with_result(result)?;
                            }
                            Dispatch::Notification(_) => {}
                        }
                        let _ = cx;
                        Ok(())
                    }
                })
                .await
                .map(|()| Handled::Yes)
        })
    }
}

#[tauri::command]
pub async fn agent_acp_load_config(
    app_handle: AppHandle,
    user_id: String,
) -> Result<AgentAcpConfig, String> {
    load_config(&app_handle, &user_id)
}

#[tauri::command]
pub async fn agent_acp_save_config(
    app_handle: AppHandle,
    user_id: String,
    config: AgentAcpConfig,
) -> Result<AgentAcpConfig, String> {
    let config = normalize_config(config)?;
    let state = app_handle.state::<AgentAcpState>();
    let running = state.running.lock().await;
    if let Some(running) = running.as_ref() {
        if running.user_id == user_id {
            let current = running.config.read().await.clone();
            if current.permission_mode != config.permission_mode
                && running.stats.running.load(Ordering::SeqCst)
            {
                return Err(
                    "Stop the ACP service before changing the permission policy".to_string()
                );
            }
        }
    }
    save_config(&app_handle, &user_id, &config)?;
    if let Some(running) = running.as_ref() {
        if running.user_id == user_id {
            *running.config.write().await = config.clone();
        }
    }
    Ok(config)
}

#[tauri::command]
pub async fn agent_acp_start(
    app_handle: AppHandle,
    user_id: String,
) -> Result<AgentAcpStatus, String> {
    start_service(&app_handle, &user_id).await?;
    status(&app_handle, &user_id).await
}

#[tauri::command]
pub async fn agent_acp_stop(
    app_handle: AppHandle,
    user_id: String,
) -> Result<AgentAcpStatus, String> {
    stop_service(&app_handle, Some(&user_id), true).await?;
    status(&app_handle, &user_id).await
}

#[tauri::command]
pub async fn agent_acp_get_status(
    app_handle: AppHandle,
    user_id: String,
) -> Result<AgentAcpStatus, String> {
    status(&app_handle, &user_id).await
}

pub async fn shutdown_agent_acp(app_handle: &AppHandle) -> Result<(), String> {
    stop_service(app_handle, None, false).await
}

#[cfg(unix)]
async fn start_service(app_handle: &AppHandle, user_id: &str) -> Result<(), String> {
    if user_id.trim().is_empty() {
        return Err("Cannot start ACP without an authenticated Maple user".to_string());
    }
    let state = app_handle.state::<AgentAcpState>();
    let _guard = state.lifecycle.lock().await;
    let stale = {
        let mut slot = state.running.lock().await;
        match slot.as_ref() {
            Some(running) if running.stats.running.load(Ordering::SeqCst) => {
                if running.user_id == user_id {
                    return Ok(());
                }
                return Err("ACP is already running for another Maple account".to_string());
            }
            Some(_) => slot.take(),
            None => None,
        }
    };
    if let Some(stale) = stale {
        stale.cancellation.cancel();
        let _ = stale.task.await;
        remove_socket_if_present(&stale.endpoint)?;
    }
    ensure_agent_runtime_for_user(app_handle, user_id.to_string(), None).await?;
    let mut config = normalize_config(load_config(app_handle, user_id)?)?;
    config.enabled = true;

    let (listener, endpoint) = bind_listener()?;

    save_config(app_handle, user_id, &config)?;
    let config = Arc::new(RwLock::new(config));
    let stats = Arc::new(AgentAcpStats::default());
    stats.running.store(true, Ordering::SeqCst);
    let cancellation = CancellationToken::new();
    let task = tauri::async_runtime::spawn(run_listener(
        listener,
        endpoint.clone(),
        app_handle.clone(),
        user_id.to_string(),
        Arc::clone(&config),
        Arc::clone(&stats),
        cancellation.clone(),
    ));
    *state.running.lock().await = Some(RunningAgentAcp {
        user_id: user_id.to_string(),
        endpoint,
        config,
        stats,
        cancellation,
        task,
    });
    Ok(())
}

#[cfg(not(unix))]
async fn start_service(_app_handle: &AppHandle, _user_id: &str) -> Result<(), String> {
    Err("Maple ACP local IPC is not yet supported on this platform".to_string())
}

async fn stop_service(
    app_handle: &AppHandle,
    requested_user: Option<&str>,
    persist_disabled: bool,
) -> Result<(), String> {
    let state = app_handle.state::<AgentAcpState>();
    let _guard = state.lifecycle.lock().await;
    let running = {
        let mut slot = state.running.lock().await;
        if let (Some(requested), Some(running)) = (requested_user, slot.as_ref()) {
            if running.user_id != requested {
                return Err("ACP belongs to another Maple account".to_string());
            }
        }
        slot.take()
    };
    let Some(running) = running else {
        if persist_disabled {
            if let Some(user_id) = requested_user {
                let mut config = load_config(app_handle, user_id)?;
                config.enabled = false;
                save_config(app_handle, user_id, &config)?;
            }
        }
        return Ok(());
    };
    running.cancellation.cancel();
    let _ = running.task.await;
    remove_socket_if_present(&running.endpoint)?;
    if persist_disabled {
        let mut config = running.config.read().await.clone();
        config.enabled = false;
        save_config(app_handle, &running.user_id, &config)?;
    }
    Ok(())
}

async fn status(app_handle: &AppHandle, user_id: &str) -> Result<AgentAcpStatus, String> {
    let state = app_handle.state::<AgentAcpState>();
    let running = state.running.lock().await;
    let harness = harness()?;
    if let Some(running) = running.as_ref() {
        if running.user_id != user_id {
            return Err("ACP belongs to another Maple account".to_string());
        }
        let config = running.config.read().await.clone();
        return Ok(AgentAcpStatus {
            running: running.stats.running.load(Ordering::SeqCst),
            enabled: config.enabled,
            connected_clients: running.stats.connected_clients.load(Ordering::SeqCst),
            active_sessions: running.stats.active_sessions.load(Ordering::SeqCst),
            active_runs: running.stats.active_runs.load(Ordering::SeqCst),
            endpoint: Some(running.endpoint.to_string_lossy().into_owned()),
            endpoint_kind: Some("unix_socket".to_string()),
            protocol_version: ACP_PROTOCOL_VERSION,
            error: running.stats.last_error.lock().await.clone(),
            buzz_credentials_available: running.stats.credential_connections.load(Ordering::SeqCst)
                > 0,
            harness,
        });
    }
    drop(running);
    let config = load_config(app_handle, user_id)?;
    Ok(AgentAcpStatus {
        running: false,
        enabled: config.enabled,
        connected_clients: 0,
        active_sessions: 0,
        active_runs: 0,
        endpoint: endpoint_path()
            .ok()
            .map(|path| path.to_string_lossy().into_owned()),
        endpoint_kind: cfg!(unix).then(|| "unix_socket".to_string()),
        protocol_version: ACP_PROTOCOL_VERSION,
        error: None,
        buzz_credentials_available: false,
        harness,
    })
}

#[cfg(unix)]
async fn run_listener(
    listener: UnixListener,
    endpoint: PathBuf,
    app_handle: AppHandle,
    user_id: String,
    config: Arc<RwLock<AgentAcpConfig>>,
    stats: Arc<AgentAcpStats>,
    cancellation: CancellationToken,
) {
    let limit = Arc::new(Semaphore::new(config.read().await.max_connections));
    let mut connections = tokio::task::JoinSet::new();
    loop {
        tokio::select! {
            _ = cancellation.cancelled() => break,
            completed = connections.join_next(), if !connections.is_empty() => {
                if let Some(Err(error)) = completed {
                    *stats.last_error.lock().await = Some(bounded_error(&error.to_string()));
                }
            }
            accepted = listener.accept() => match accepted {
                Ok((stream, _)) => {
                    let Ok(permit) = Arc::clone(&limit).try_acquire_owned() else {
                        drop(stream);
                        continue;
                    };
                    let app_handle = app_handle.clone();
                    let user_id = user_id.clone();
                    let config = Arc::clone(&config);
                    let stats = Arc::clone(&stats);
                    let connection_cancel = cancellation.clone();
                    connections.spawn(async move {
                        let _permit = permit;
                        stats.connected_clients.fetch_add(1, Ordering::SeqCst);
                        let context = AcpConnectionContext::new(
                            app_handle,
                            user_id,
                            config,
                            Arc::clone(&stats),
                        );
                        let (read, write) = stream.into_split();
                        let peer_eof = CancellationToken::new();
                        let read = BoundedLineReader::new(read, peer_eof.clone());
                        let serving = AcpAgent
                            .builder()
                            .name("maple-acp")
                            .with_handler(MapleAcpHandler {
                                context: Arc::clone(&context),
                            })
                            .connect_to(ByteStreams::new(write.compat_write(), read.compat()));
                        tokio::select! {
                            result = serving => {
                                if let Err(error) = result {
                                    *stats.last_error.lock().await = Some(bounded_error(&error.to_string()));
                                }
                            }
                            _ = connection_cancel.cancelled() => {}
                            _ = peer_eof.cancelled() => {}
                        }
                        context.cleanup().await;
                        stats.connected_clients.fetch_sub(1, Ordering::SeqCst);
                    });
                }
                Err(error) => {
                    *stats.last_error.lock().await = Some(bounded_error(&error.to_string()));
                    break;
                }
            }
        }
    }
    cancellation.cancel();
    while connections.join_next().await.is_some() {}
    stats.running.store(false, Ordering::SeqCst);
    let _ = remove_socket_if_present(&endpoint);
}

#[cfg(unix)]
fn bind_listener() -> Result<(UnixListener, PathBuf), String> {
    let endpoint = endpoint_path()?;
    if let Ok(metadata) = std::fs::symlink_metadata(&endpoint) {
        if metadata.file_type().is_symlink() || !metadata.file_type().is_socket() {
            return Err(format!(
                "Refusing to replace unexpected ACP endpoint {}",
                endpoint.display()
            ));
        }
        if std::os::unix::net::UnixStream::connect(&endpoint).is_ok() {
            return Err("Another Maple ACP service is already listening".to_string());
        }
        std::fs::remove_file(&endpoint)
            .map_err(|error| format!("Failed to remove stale ACP endpoint: {error}"))?;
    }
    let listener = UnixListener::bind(&endpoint)
        .map_err(|error| format!("Failed to bind Maple ACP endpoint: {error}"))?;
    std::fs::set_permissions(&endpoint, std::fs::Permissions::from_mode(0o600))
        .map_err(|error| format!("Failed to secure Maple ACP endpoint: {error}"))?;
    Ok((listener, endpoint))
}

fn endpoint_path() -> Result<PathBuf, String> {
    let executable = stable_executable_path()?;
    let digest = Sha256::digest(executable.to_string_lossy().as_bytes());
    let suffix = digest[..8]
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect::<String>();
    Ok(endpoint_root()?.join(format!("maple-acp-{suffix}.sock")))
}

#[cfg(target_os = "linux")]
fn endpoint_root() -> Result<PathBuf, String> {
    let current_uid = std::fs::metadata("/proc/self")
        .map_err(|error| format!("Failed to resolve the current Linux user: {error}"))?
        .uid();
    let root = std::env::var_os("XDG_RUNTIME_DIR")
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
        .unwrap_or_else(|| std::env::temp_dir().join(format!("maple-acp-{current_uid}")));
    match std::fs::symlink_metadata(&root) {
        Ok(_) => {}
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            let mut builder = std::fs::DirBuilder::new();
            builder.mode(0o700);
            builder.create(&root).map_err(|error| {
                format!("Failed to create the Maple ACP runtime directory: {error}")
            })?;
        }
        Err(error) => {
            return Err(format!(
                "Failed to inspect the Maple ACP runtime directory: {error}"
            ));
        }
    }
    let metadata = std::fs::symlink_metadata(&root)
        .map_err(|error| format!("Failed to inspect the Maple ACP runtime directory: {error}"))?;
    if metadata.file_type().is_symlink()
        || !metadata.file_type().is_dir()
        || metadata.uid() != current_uid
    {
        return Err("Maple ACP runtime directory is not owned by the current user".to_string());
    }
    std::fs::set_permissions(&root, std::fs::Permissions::from_mode(0o700))
        .map_err(|error| format!("Failed to secure the Maple ACP runtime directory: {error}"))?;
    Ok(root)
}

#[cfg(not(target_os = "linux"))]
fn endpoint_root() -> Result<PathBuf, String> {
    Ok(std::env::temp_dir())
}

fn stable_executable_path() -> Result<PathBuf, String> {
    #[cfg(target_os = "linux")]
    if let Some(appimage) = std::env::var_os("APPIMAGE") {
        return Ok(PathBuf::from(appimage));
    }
    let executable = std::env::current_exe()
        .map_err(|error| format!("Failed to find the Maple executable: {error}"))?;
    Ok(executable.canonicalize().unwrap_or(executable))
}

fn harness() -> Result<AgentAcpHarness, String> {
    Ok(AgentAcpHarness {
        command: stable_executable_path()?.to_string_lossy().into_owned(),
        args: vec!["acp".to_string()],
    })
}

fn config_path(app_handle: &AppHandle, user_id: &str) -> Result<PathBuf, String> {
    if user_id.trim().is_empty() {
        return Err("Maple ACP configuration requires an authenticated user".to_string());
    }
    let digest = Sha256::digest(user_id.as_bytes());
    let scope = digest[..16]
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect::<String>();
    let root = app_handle
        .path()
        .app_local_data_dir()
        .map_err(|error| format!("Failed to resolve Maple local data: {error}"))?;
    Ok(root
        .join("acp")
        .join("accounts")
        .join(scope)
        .join("config.json"))
}

fn load_config(app_handle: &AppHandle, user_id: &str) -> Result<AgentAcpConfig, String> {
    let path = config_path(app_handle, user_id)?;
    match std::fs::read(&path) {
        Ok(bytes) => serde_json::from_slice(&bytes)
            .map_err(|error| format!("Failed to parse Maple ACP configuration: {error}"))
            .and_then(normalize_config),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(AgentAcpConfig::default()),
        Err(error) => Err(format!("Failed to read Maple ACP configuration: {error}")),
    }
}

fn save_config(
    app_handle: &AppHandle,
    user_id: &str,
    config: &AgentAcpConfig,
) -> Result<(), String> {
    let path = config_path(app_handle, user_id)?;
    let parent = path
        .parent()
        .ok_or_else(|| "Invalid Maple ACP configuration path".to_string())?;
    std::fs::create_dir_all(parent)
        .map_err(|error| format!("Failed to create Maple ACP configuration directory: {error}"))?;
    #[cfg(unix)]
    std::fs::set_permissions(parent, std::fs::Permissions::from_mode(0o700))
        .map_err(|error| format!("Failed to secure Maple ACP configuration directory: {error}"))?;
    let bytes = serde_json::to_vec_pretty(config)
        .map_err(|error| format!("Failed to encode Maple ACP configuration: {error}"))?;
    std::fs::write(&path, bytes)
        .map_err(|error| format!("Failed to save Maple ACP configuration: {error}"))?;
    #[cfg(unix)]
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600))
        .map_err(|error| format!("Failed to secure Maple ACP configuration: {error}"))?;
    Ok(())
}

pub(crate) fn clear_agent_acp_config(app_handle: &AppHandle, user_id: &str) -> Result<(), String> {
    let path = config_path(app_handle, user_id)?;
    let account_dir = path
        .parent()
        .ok_or_else(|| "Invalid Maple ACP configuration path".to_string())?;
    match std::fs::remove_dir_all(account_dir) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(format!("Failed to clear Maple ACP configuration: {error}")),
    }
}

fn normalize_config(mut config: AgentAcpConfig) -> Result<AgentAcpConfig, String> {
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

fn ensure_allowed_project_root(cwd: &Path, allowed_roots: &[String]) -> Result<(), String> {
    if allowed_roots.is_empty() {
        return Ok(());
    }
    let cwd = cwd
        .canonicalize()
        .map_err(|error| format!("Failed to resolve ACP session cwd: {error}"))?;
    for root in allowed_roots {
        if let Ok(root) = Path::new(root).canonicalize() {
            if cwd.starts_with(root) {
                return Ok(());
            }
        }
    }
    Err("ACP session cwd is outside the configured project roots".to_string())
}

fn merge_mcp_environment(
    environment: &mut HashMap<String, String>,
    servers: &[McpServer],
) -> Result<(), agent_client_protocol::Error> {
    for server in servers {
        let McpServer::Stdio(server) = server else {
            return Err(agent_client_protocol::Error::invalid_params()
                .data("Maple ACP currently supports only stdio MCP session definitions"));
        };
        let command = Path::new(&server.command);
        let is_buzz_dev_mcp = server.name == "buzz-dev-mcp"
            && command
                .file_name()
                .and_then(|name| name.to_str())
                .is_some_and(|name| name == "buzz-dev-mcp" || name == "buzz-dev-mcp.exe")
            && server.args.is_empty();
        if !is_buzz_dev_mcp || !command.is_absolute() {
            return Err(agent_client_protocol::Error::invalid_params().data(
                "Maple ACP currently adapts only Buzz's absolute buzz-dev-mcp stdio definition",
            ));
        }
        let metadata = std::fs::metadata(command).map_err(|_| {
            agent_client_protocol::Error::invalid_params()
                .data("Buzz buzz-dev-mcp command does not exist")
        })?;
        if !metadata.is_file() {
            return Err(agent_client_protocol::Error::invalid_params()
                .data("Buzz buzz-dev-mcp command is not a file"));
        }
        #[cfg(unix)]
        if metadata.permissions().mode() & 0o111 == 0 {
            return Err(agent_client_protocol::Error::invalid_params()
                .data("Buzz buzz-dev-mcp command is not executable"));
        }
        // Maple already exposes a tightly controlled shell tool. For Buzz's
        // dev MCP, adapt the exact child environment into that per-session
        // shell rather than launching a second general-purpose shell server.
        for variable in &server.env {
            if !ALLOWED_BRIDGE_ENV.contains(&variable.name.as_str()) {
                continue;
            }
            if let Some(existing) = environment.get(&variable.name) {
                if existing != &variable.value {
                    return Err(agent_client_protocol::Error::invalid_params().data(format!(
                        "Conflicting ACP environment value for {}",
                        variable.name
                    )));
                }
            } else {
                environment.insert(variable.name.clone(), variable.value.clone());
            }
        }
    }
    Ok(())
}

fn filter_bridge_environment(environment: HashMap<String, String>) -> HashMap<String, String> {
    environment
        .into_iter()
        .filter(|(key, value)| {
            ALLOWED_BRIDGE_ENV.contains(&key.as_str())
                && !value.contains('\0')
                && value.len() <= 16 * 1024
        })
        .collect()
}

fn has_buzz_credentials(environment: &HashMap<String, String>) -> bool {
    environment
        .get("BUZZ_RELAY_URL")
        .is_some_and(|value| !value.is_empty())
        && environment
            .get("BUZZ_PRIVATE_KEY")
            .is_some_and(|value| !value.is_empty())
}

fn prompt_text(blocks: &[ContentBlock]) -> Result<String, agent_client_protocol::Error> {
    let text = blocks
        .iter()
        .filter_map(|block| match block {
            ContentBlock::Text(text) => Some(text.text.as_str()),
            _ => None,
        })
        .collect::<Vec<_>>()
        .join("\n\n");
    if text.trim().is_empty() {
        return Err(agent_client_protocol::Error::invalid_params()
            .data("Maple ACP requires at least one text prompt block"));
    }
    Ok(text)
}

fn timeline_update(event: &AgentEventEnvelope) -> Option<SessionUpdate> {
    let item = event.item.as_ref()?;
    let text = item.text.as_deref()?.to_string();
    match item.item_type.as_str() {
        "message" if item.role.as_deref() == Some("assistant") => {
            Some(SessionUpdate::AgentMessageChunk(ContentChunk::new(
                ContentBlock::Text(TextContent::new(text)),
            )))
        }
        "thinking" => Some(SessionUpdate::AgentThoughtChunk(ContentChunk::new(
            ContentBlock::Text(TextContent::new(text)),
        ))),
        _ => None,
    }
}

fn event_error_text(event: &AgentEventEnvelope) -> Option<String> {
    event
        .message
        .clone()
        .or_else(|| event.item.as_ref().and_then(|item| item.text.clone()))
        .map(|message| bounded_error(&message))
}

fn prompt_result_from_terminal(
    terminal: AgentRunTerminal,
) -> Result<PromptResponse, agent_client_protocol::Error> {
    match terminal {
        AgentRunTerminal::Completed => Ok(PromptResponse::new(StopReason::EndTurn)),
        AgentRunTerminal::Cancelled => Ok(PromptResponse::new(StopReason::Cancelled)),
        AgentRunTerminal::Failed => {
            Err(agent_client_protocol::Error::internal_error().data("Maple Agent prompt failed"))
        }
    }
}

fn internal_acp_error(error: String) -> agent_client_protocol::Error {
    agent_client_protocol::Error::internal_error().data(bounded_error(&error))
}

fn bounded_error(error: &str) -> String {
    error.chars().take(MAX_ACP_ERROR_CHARS).collect()
}

fn remove_socket_if_present(path: &Path) -> Result<(), String> {
    match std::fs::remove_file(path) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(format!("Failed to remove Maple ACP endpoint: {error}")),
    }
}

#[cfg(target_os = "linux")]
fn verify_connector_endpoint(path: &Path) -> Result<(), String> {
    let current_uid = std::fs::metadata("/proc/self")
        .map_err(|error| format!("Failed to resolve the current Linux user: {error}"))?
        .uid();
    let metadata = std::fs::symlink_metadata(path)
        .map_err(|error| format!("Maple Desktop ACP service is unavailable: {error}"))?;
    if metadata.file_type().is_symlink()
        || !metadata.file_type().is_socket()
        || metadata.uid() != current_uid
        || metadata.permissions().mode() & 0o077 != 0
    {
        return Err("Refusing an insecure Maple ACP endpoint".to_string());
    }
    Ok(())
}

#[cfg(not(target_os = "linux"))]
fn verify_connector_endpoint(_path: &Path) -> Result<(), String> {
    Ok(())
}

pub fn run_acp_connector() -> Result<(), String> {
    #[cfg(unix)]
    {
        let runtime = tokio::runtime::Builder::new_multi_thread()
            .enable_all()
            .build()
            .map_err(|error| format!("Failed to start Maple ACP connector: {error}"))?;
        runtime.block_on(run_acp_connector_async())
    }
    #[cfg(not(unix))]
    Err("Maple ACP local IPC is not yet supported on this platform".to_string())
}

#[cfg(unix)]
async fn run_acp_connector_async() -> Result<(), String> {
    use tokio::io::{copy, AsyncWriteExt as _};

    let endpoint = endpoint_path()?;
    verify_connector_endpoint(&endpoint)?;
    let stream = UnixStream::connect(&endpoint).await.map_err(|error| {
        format!(
            "Maple Desktop ACP service is unavailable at {}: {error}",
            endpoint.display()
        )
    })?;
    let (mut socket_read, mut socket_write) = stream.into_split();
    let environment = ALLOWED_BRIDGE_ENV
        .iter()
        .filter_map(|key| {
            std::env::var(key)
                .ok()
                .map(|value| ((*key).to_string(), value))
        })
        .collect::<HashMap<_, _>>();
    let hello = serde_json::json!({
        "jsonrpc": "2.0",
        "method": BRIDGE_HELLO_METHOD,
        "params": { "environment": environment }
    });
    let mut hello = serde_json::to_vec(&hello)
        .map_err(|error| format!("Failed to encode Maple ACP bridge hello: {error}"))?;
    hello.push(b'\n');
    socket_write
        .write_all(&hello)
        .await
        .map_err(|error| format!("Failed to initialize Maple ACP bridge: {error}"))?;
    socket_write
        .flush()
        .await
        .map_err(|error| format!("Failed to flush Maple ACP bridge: {error}"))?;

    let mut stdin = tokio::io::stdin();
    let mut stdout = tokio::io::stdout();
    let outbound = async {
        copy(&mut stdin, &mut socket_write).await?;
        socket_write.shutdown().await
    };
    let inbound = async {
        copy(&mut socket_read, &mut stdout).await?;
        stdout.flush().await
    };
    tokio::pin!(outbound);
    tokio::pin!(inbound);
    tokio::select! {
        result = &mut inbound => {
            // The desktop service closing the socket must terminate the
            // Buzz-owned connector even while its stdin remains open.
            result.map_err(|error| format!("Maple ACP bridge failed: {error}"))?;
        }
        result = &mut outbound => {
            // When Buzz closes stdin, preserve the half-close behavior and
            // keep relaying any final ACP response until Maple closes output.
            result.map_err(|error| format!("Maple ACP bridge failed: {error}"))?;
            inbound
                .await
                .map_err(|error| format!("Maple ACP bridge failed: {error}"))?;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_config_is_disabled_and_read_only() {
        let config = AgentAcpConfig::default();
        assert!(!config.enabled);
        assert_eq!(config.permission_mode, AgentAcpPermissionMode::ReadOnly);
        assert_eq!(config.max_connections, 1);
    }

    #[test]
    fn bridge_environment_is_strictly_allowlisted() {
        let filtered = filter_bridge_environment(HashMap::from([
            (
                "BUZZ_RELAY_URL".to_string(),
                "ws://localhost:3000".to_string(),
            ),
            ("UNRELATED_SECRET".to_string(), "nope".to_string()),
        ]));
        assert_eq!(filtered.len(), 1);
        assert!(filtered.contains_key("BUZZ_RELAY_URL"));
    }

    #[test]
    fn prompt_blocks_preserve_buzz_order() {
        let blocks = vec![
            ContentBlock::Text(TextContent::new("[Base]\nbase")),
            ContentBlock::Text(TextContent::new("[System]\nsystem")),
        ];
        assert_eq!(
            prompt_text(&blocks).unwrap(),
            "[Base]\nbase\n\n[System]\nsystem"
        );
    }

    #[test]
    fn retained_terminal_results_preserve_all_stop_states() {
        let completed = prompt_result_from_terminal(AgentRunTerminal::Completed).unwrap();
        assert_eq!(
            serde_json::to_value(completed).unwrap()["stopReason"],
            "end_turn"
        );

        let cancelled = prompt_result_from_terminal(AgentRunTerminal::Cancelled).unwrap();
        assert_eq!(
            serde_json::to_value(cancelled).unwrap()["stopReason"],
            "cancelled"
        );

        assert!(prompt_result_from_terminal(AgentRunTerminal::Failed).is_err());
    }
}
