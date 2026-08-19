use crate::agent::{
    AgentCreateSessionRequest, AgentHostEventPolicy, AgentPermissionDecision,
    AgentPermissionRequest, AgentRunCancellation, AgentRunEvent, AgentRunPermissionResponder,
    AgentRunTerminal, AgentRuntimeHandle, AgentSendMessageRequest, AgentTimelineItem,
    AgentToolContextLease, AgentToolContextSpec, MapleAgentService,
    AGENT_TOOL_CONTEXT_INACTIVE_ERROR,
};
use crate::agent_host::AgentHostLifecycle;
use crate::maple_api::{account_scope, MapleApiAuthState};
use agent_client_protocol::schema::v1::{
    AgentCapabilities, CancelNotification, ContentBlock, ContentChunk, Implementation,
    InitializeRequest, InitializeResponse, McpServer, NewSessionRequest, NewSessionResponse,
    PermissionOption, PermissionOptionKind, PromptCapabilities, PromptRequest, PromptResponse,
    RequestPermissionOutcome, RequestPermissionRequest, SessionId, SessionNotification,
    SessionUpdate, StopReason, TextContent, ToolCall, ToolCallContent, ToolCallStatus, ToolKind,
};
use agent_client_protocol::util::MatchDispatchFrom;
use agent_client_protocol::{
    Agent as AcpAgent, Client, ConnectionTo, Dispatch, HandleDispatchFrom, Handled,
    JsonRpcNotification, Lines, Responder,
};
use futures_util::StreamExt as _;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet, HashMap, VecDeque};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::Arc;
use tauri::{AppHandle, Manager};
use tokio::sync::{Mutex, OwnedSemaphorePermit, RwLock, Semaphore};
use tokio_util::codec::{FramedRead, LinesCodec};
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
const MAX_ACP_OUTBOUND_EVENTS_IN_FLIGHT: usize = 256;
const MAX_ACP_OUTBOUND_BYTES_IN_FLIGHT: usize = 4 * 1024 * 1024;
const ACP_OUTBOUND_FRAME_OVERHEAD_BYTES: usize = 256;
const ACP_CONNECTION_CLEANUP_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(5);
const ACP_SYNTHETIC_STOP_DRAIN_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(5);
const BRIDGE_HELLO_METHOD: &str = "_maple/bridge/hello";
const ALLOWED_BRIDGE_ENV: [&str; 6] = [
    "BUZZ_RELAY_URL",
    "BUZZ_PRIVATE_KEY",
    "BUZZ_AUTH_TAG",
    "BUZZ_API_TOKEN",
    "BUZZ_ACP_DISPLAY_NAME",
    "PATH",
];
const SENSITIVE_BRIDGE_ENV: [&str; 5] = [
    "BUZZ_RELAY_URL",
    "BUZZ_PRIVATE_KEY",
    "BUZZ_AUTH_TAG",
    "BUZZ_API_TOKEN",
    "BUZZ_ACP_DISPLAY_NAME",
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

fn is_session_update_line(line: &str) -> bool {
    serde_json::from_str::<serde_json::Value>(line)
        .ok()
        .and_then(|message| {
            message
                .get("method")
                .and_then(serde_json::Value::as_str)
                .map(str::to_owned)
        })
        .as_deref()
        == Some("session/update")
}

#[cfg(unix)]
fn tracked_outgoing_lines<W>(
    writer: W,
    outbound: Arc<AcpOutboundTracker>,
) -> impl futures_util::Sink<String, Error = std::io::Error> + Send
where
    W: tokio::io::AsyncWrite + Unpin + Send + 'static,
{
    futures_util::sink::unfold(
        (writer, outbound),
        |(mut writer, outbound), line: String| async move {
            use tokio::io::AsyncWriteExt as _;

            let session_update = is_session_update_line(&line);
            writer.write_all(line.as_bytes()).await?;
            writer.write_all(b"\n").await?;
            writer.flush().await?;
            if session_update {
                // Credits return only after the real local socket accepted the
                // complete notification. A peer that stops reading therefore
                // backpressures Maple instead of growing ACP's internal queues.
                outbound.acknowledge_session_update();
            }
            Ok((writer, outbound))
        },
    )
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum AgentAcpPermissionMode {
    ReadOnly,
    AllowAll,
}

impl AgentAcpPermissionMode {
    fn maple_mode(&self) -> &'static str {
        // ACP callers own every unresolved interactive decision. Keep the old
        // allow_all variant readable for configuration compatibility, but do
        // not let it bypass the caller through Maple's Auto policy.
        "smart_approve"
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
    account_scope: String,
    endpoint: PathBuf,
    config: Arc<RwLock<AgentAcpConfig>>,
    stats: Arc<AgentAcpStats>,
    cancellation: CancellationToken,
    task: tauri::async_runtime::JoinHandle<()>,
}

pub struct AgentAcpState {
    running: Mutex<Option<RunningAgentAcp>>,
}

impl AgentAcpState {
    pub fn new() -> Self {
        Self {
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
    agent: AgentRuntimeHandle,
    config: Arc<RwLock<AgentAcpConfig>>,
    stats: Arc<AgentAcpStats>,
    bridge_environment: Mutex<HashMap<String, String>>,
    sessions: Mutex<HashMap<String, AgentToolContextLease>>,
    prompt_states: Mutex<HashMap<String, AcpPromptState>>,
    background_tasks: Mutex<tokio::task::JoinSet<()>>,
    finalization: Mutex<()>,
    lifetime: CancellationToken,
    closed: AtomicBool,
    has_credentials: AtomicBool,
    outbound: Arc<AcpOutboundTracker>,
}

enum AcpPromptState {
    Starting {
        cancellation: CancellationToken,
    },
    Running {
        cancellation: CancellationToken,
        run_cancellation: Box<AgentRunCancellation>,
    },
}

struct AcpOutboundTracker {
    event_slots: Arc<Semaphore>,
    byte_slots: Arc<Semaphore>,
    pending: std::sync::Mutex<VecDeque<AcpOutboundReservation>>,
}

struct AcpOutboundReservation {
    _event: OwnedSemaphorePermit,
    _bytes: OwnedSemaphorePermit,
}

#[derive(Debug)]
enum AcpOutboundSendError {
    UpdateTooLarge,
    Cancelled,
    Transport(agent_client_protocol::Error),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum AcpPermissionResolution {
    Continue,
    Cancelled,
}

impl AcpOutboundTracker {
    fn new() -> Arc<Self> {
        Self::with_limits(
            MAX_ACP_OUTBOUND_EVENTS_IN_FLIGHT,
            MAX_ACP_OUTBOUND_BYTES_IN_FLIGHT,
        )
    }

    fn with_limits(event_limit: usize, byte_limit: usize) -> Arc<Self> {
        Arc::new(Self {
            event_slots: Arc::new(Semaphore::new(event_limit)),
            byte_slots: Arc::new(Semaphore::new(byte_limit)),
            pending: std::sync::Mutex::new(VecDeque::new()),
        })
    }

    async fn reserve(
        &self,
        encoded_bytes: usize,
        cancellation: &CancellationToken,
    ) -> Result<AcpOutboundReservation, AcpOutboundSendError> {
        let charged_bytes = encoded_bytes.saturating_add(ACP_OUTBOUND_FRAME_OVERHEAD_BYTES);
        let Ok(charged_bytes) = u32::try_from(charged_bytes) else {
            return Err(AcpOutboundSendError::UpdateTooLarge);
        };
        if charged_bytes as usize > MAX_ACP_OUTBOUND_BYTES_IN_FLIGHT {
            return Err(AcpOutboundSendError::UpdateTooLarge);
        }

        let event = tokio::select! {
            biased;
            _ = cancellation.cancelled() => return Err(AcpOutboundSendError::Cancelled),
            permit = Arc::clone(&self.event_slots).acquire_owned() => {
                permit.map_err(|_| AcpOutboundSendError::Cancelled)?
            }
        };
        let bytes = tokio::select! {
            biased;
            _ = cancellation.cancelled() => return Err(AcpOutboundSendError::Cancelled),
            permit = Arc::clone(&self.byte_slots).acquire_many_owned(charged_bytes) => {
                permit.map_err(|_| AcpOutboundSendError::Cancelled)?
            }
        };
        Ok(AcpOutboundReservation {
            _event: event,
            _bytes: bytes,
        })
    }

    fn enqueue(
        &self,
        cx: &ConnectionTo<Client>,
        notification: SessionNotification,
        reservation: AcpOutboundReservation,
    ) -> Result<(), AcpOutboundSendError> {
        // Serialize reservation order with the protocol enqueue. The socket
        // writer can then release one exact FIFO credit for each written
        // session/update line, even when several ACP sessions stream together.
        let mut pending = self
            .pending
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        pending.push_back(reservation);
        if let Err(error) = cx.send_notification(notification) {
            pending.pop_back();
            return Err(AcpOutboundSendError::Transport(error));
        }
        Ok(())
    }

    fn acknowledge_session_update(&self) {
        let reservation = self
            .pending
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .pop_front();
        if reservation.is_none() {
            log::warn!("Maple ACP wrote an untracked session/update notification");
        }
        drop(reservation);
    }
}

impl AcpConnectionContext {
    fn new(
        agent: AgentRuntimeHandle,
        config: Arc<RwLock<AgentAcpConfig>>,
        stats: Arc<AgentAcpStats>,
    ) -> Arc<Self> {
        Arc::new(Self {
            agent,
            config,
            stats,
            bridge_environment: Mutex::new(HashMap::new()),
            sessions: Mutex::new(HashMap::new()),
            prompt_states: Mutex::new(HashMap::new()),
            background_tasks: Mutex::new(tokio::task::JoinSet::new()),
            finalization: Mutex::new(()),
            lifetime: CancellationToken::new(),
            closed: AtomicBool::new(false),
            has_credentials: AtomicBool::new(false),
            outbound: AcpOutboundTracker::new(),
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
        let project_root = ensure_allowed_project_root(&request.cwd, &config.allowed_project_roots)
            .map_err(|error| agent_client_protocol::Error::invalid_params().data(error))?;

        let mut environment = self.bridge_environment.lock().await.clone();
        merge_mcp_environment(&mut environment, &request.mcp_servers)?;
        let tool_context = bridge_tool_context_spec(&environment).map_err(internal_acp_error)?;
        let mode = config.permission_mode.maple_mode().to_string();
        let created = self
            .agent
            .create_session_with_tool_context(
                Some(AgentCreateSessionRequest {
                    project_root: Some(project_root.to_string_lossy().into_owned()),
                    title: Some("Buzz ACP".to_string()),
                    model: None,
                    context_limit: None,
                    mode: Some(mode),
                    mcp_server_names: None,
                }),
                Some(tool_context),
                // The ACP caller is the only interactive surface for this task.
                // Persisted history remains loadable in Maple Desktop, but live
                // permission cards must never create a second approval broker.
                AgentHostEventPolicy::Suppress,
            )
            .await
            .map_err(internal_acp_error)?;
        let session_id = created.detail.session.id;
        let lease = created
            .tool_context_lease
            .expect("an explicit Agent tool context must return a lease");
        let finalization = self.finalization.lock().await;
        if self.closed.load(Ordering::SeqCst) {
            drop(finalization);
            self.discard_uncommitted_session(&session_id, lease).await;
            return Err(agent_client_protocol::Error::internal_error()
                .data("The Maple ACP connection closed while configuring the session"));
        }
        let mut sessions = self.sessions.lock().await;
        if sessions.contains_key(&session_id) {
            drop(sessions);
            drop(finalization);
            self.discard_uncommitted_session(&session_id, lease).await;
            return Err(agent_client_protocol::Error::internal_error()
                .data("The Maple ACP connection duplicated a new session"));
        }
        sessions.insert(session_id.clone(), lease);
        drop(sessions);
        self.stats.active_sessions.fetch_add(1, Ordering::SeqCst);
        if has_buzz_credentials(&environment) && !self.has_credentials.swap(true, Ordering::SeqCst)
        {
            self.stats
                .credential_connections
                .fetch_add(1, Ordering::SeqCst);
        }
        drop(finalization);
        Ok(NewSessionResponse::new(session_id))
    }

    async fn discard_uncommitted_session(&self, session_id: &str, lease: AgentToolContextLease) {
        lease.release().await;
        let _ = self
            .agent
            .discard_session_during_cleanup(session_id.to_string())
            .await;
    }

    async fn retire_session(&self, session_id: &str) {
        let lease = self.sessions.lock().await.remove(session_id);
        if let Some(lease) = lease {
            self.stats.active_sessions.fetch_sub(1, Ordering::SeqCst);
            lease.release().await;
        }
    }

    async fn begin_prompt(
        &self,
        request: &PromptRequest,
    ) -> Result<(String, CancellationToken), agent_client_protocol::Error> {
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
        let cancellation = self.lifetime.child_token();
        states.insert(
            session_id,
            AcpPromptState::Starting {
                cancellation: cancellation.clone(),
            },
        );
        Ok((prompt, cancellation))
    }

    async fn send_session_update(
        &self,
        cx: &ConnectionTo<Client>,
        notification: SessionNotification,
        cancellation: &CancellationToken,
    ) -> Result<(), AcpOutboundSendError> {
        let encoded_bytes = serde_json::to_vec(&notification)
            .map_err(|error| {
                AcpOutboundSendError::Transport(
                    agent_client_protocol::Error::internal_error()
                        .data(format!("Failed to encode Maple ACP update: {error}")),
                )
            })?
            .len();
        let reservation = self.outbound.reserve(encoded_bytes, cancellation).await?;
        self.outbound.enqueue(cx, notification, reservation)
    }

    async fn send_final_agent_message(
        &self,
        cx: &ConnectionTo<Client>,
        session_id: SessionId,
        message: &str,
        cancellation: &CancellationToken,
    ) -> Result<(), AcpOutboundSendError> {
        self.send_session_update(
            cx,
            SessionNotification::new(
                session_id,
                SessionUpdate::AgentMessageChunk(ContentChunk::new(ContentBlock::Text(
                    TextContent::new(message.to_string()),
                ))),
            ),
            cancellation,
        )
        .await
    }

    async fn request_permission_from_caller(
        &self,
        cx: &ConnectionTo<Client>,
        session_id: SessionId,
        request: AgentPermissionRequest,
        item: &AgentTimelineItem,
        responder: &AgentRunPermissionResponder,
        cancellation: &CancellationToken,
    ) -> Result<AcpPermissionResolution, AcpOutboundSendError> {
        let tool_call = acp_permission_tool_call(&request, item);
        let permission_request =
            RequestPermissionRequest::new(session_id, tool_call.into(), acp_permission_options());
        let encoded_bytes = serde_json::to_vec(&permission_request)
            .map_err(|error| {
                AcpOutboundSendError::Transport(internal_acp_error(format!(
                    "Failed to encode Maple ACP permission request: {error}"
                )))
            })?
            .len();
        // Permission requests use the same global event/byte budget as streamed
        // notifications. Hold the reservation until the caller responds so a
        // slow client cannot accumulate unbounded JSON-RPC request frames.
        let reservation = self.outbound.reserve(encoded_bytes, cancellation).await?;
        if cancellation.is_cancelled() {
            cancel_maple_permission(responder, &request.request_id).await;
            return Ok(AcpPermissionResolution::Cancelled);
        }
        let sent_request = cx.send_request(permission_request);
        let mut response_future = Box::pin(sent_request.block_task());
        let response = tokio::select! {
            biased;
            _ = cancellation.cancelled() => None,
            response = &mut response_future => Some(response),
        };
        let Some(response) = response else {
            // ACP v1 has no stable request-cancellation primitive. Stop Maple
            // immediately, but keep consuming the already-sent JSON-RPC request
            // and retain its outbound credits until the client replies or the
            // connection closes. Otherwise a cancel-and-never-reply client can
            // accumulate unbounded SDK correlation entries outside our limit.
            cancel_maple_permission(responder, &request.request_id).await;
            retain_cancelled_permission_request(response_future, reservation);
            return Ok(AcpPermissionResolution::Cancelled);
        };
        drop(reservation);

        let (decision, resolution) = match response {
            Ok(response) => match acp_permission_decision(&response.outcome) {
                Ok(AgentPermissionDecision::Cancel) => (
                    AgentPermissionDecision::Cancel,
                    AcpPermissionResolution::Cancelled,
                ),
                Ok(decision) => (decision, AcpPermissionResolution::Continue),
                Err(error) => {
                    cancel_maple_permission(responder, &request.request_id).await;
                    return Err(AcpOutboundSendError::Transport(internal_acp_error(error)));
                }
            },
            Err(error) => {
                cancel_maple_permission(responder, &request.request_id).await;
                return Err(AcpOutboundSendError::Transport(error));
            }
        };

        if let Err(error) = responder.respond(request.request_id, decision).await {
            if cancellation.is_cancelled() {
                return Ok(AcpPermissionResolution::Cancelled);
            }
            return Err(AcpOutboundSendError::Transport(internal_acp_error(
                format!("Failed to resolve Maple permission request: {error}"),
            )));
        }
        Ok(resolution)
    }

    async fn prompt(
        self: &Arc<Self>,
        cx: &ConnectionTo<Client>,
        request: PromptRequest,
        prompt: String,
        prompt_lifetime: CancellationToken,
    ) -> Result<PromptResponse, agent_client_protocol::Error> {
        let session_id = request.session_id.0.to_string();
        let config = self.config.read().await.clone();
        let tool_context_access = match self.sessions.lock().await.get(&session_id) {
            Some(lease) => lease.access(),
            None => {
                self.prompt_states.lock().await.remove(&session_id);
                return Err(agent_client_protocol::Error::resource_not_found(Some(
                    session_id.clone(),
                ))
                .data("ACP session is no longer owned by this connection"));
            }
        };
        let run = match self
            .agent
            .send_message_with_tool_context(
                AgentSendMessageRequest {
                    session_id: session_id.clone(),
                    text: prompt,
                    model: None,
                    context_limit: None,
                    mode: Some(config.permission_mode.maple_mode().to_string()),
                    vision_capable: false,
                    steer: false,
                    queue_id: None,
                },
                tool_context_access,
                prompt_lifetime.clone(),
                AgentHostEventPolicy::Suppress,
            )
            .await
        {
            Ok(run) => run,
            Err(error) if error == AGENT_TOOL_CONTEXT_INACTIVE_ERROR => {
                self.prompt_states.lock().await.remove(&session_id);
                self.retire_session(&session_id).await;
                return Err(agent_client_protocol::Error::resource_not_found(Some(
                    session_id.clone(),
                ))
                .data("The Maple Agent task was removed outside this ACP connection"));
            }
            Err(error) => {
                self.prompt_states.lock().await.remove(&session_id);
                return Err(internal_acp_error(error));
            }
        };
        let mut events = run.events;
        let mut terminal = run.terminal;
        let event_overflowed = run.event_overflowed;
        let Some(run_cancellation) = run.cancellation else {
            prompt_lifetime.cancel();
            self.prompt_states.lock().await.remove(&session_id);
            return Err(agent_client_protocol::Error::internal_error()
                .data("Maple did not create an ACP cancellation capability for this run"));
        };
        let Some(permission_responder) = run.permission_responder else {
            prompt_lifetime.cancel();
            let _ = run_cancellation.cancel().await;
            self.prompt_states.lock().await.remove(&session_id);
            return Err(agent_client_protocol::Error::internal_error()
                .data("Maple did not create an ACP permission responder for this run"));
        };
        let prompt_registered = {
            let mut states = self.prompt_states.lock().await;
            match states.get_mut(&session_id) {
                Some(state @ AcpPromptState::Starting { .. }) => {
                    *state = AcpPromptState::Running {
                        cancellation: prompt_lifetime.clone(),
                        run_cancellation: Box::new(run_cancellation.clone()),
                    };
                    true
                }
                _ => false,
            }
        };
        if !prompt_registered {
            let _ = run_cancellation.cancel().await;
            return Err(agent_client_protocol::Error::internal_error()
                .data("The Maple ACP connection closed while starting the prompt"));
        }
        self.stats.active_runs.fetch_add(1, Ordering::SeqCst);
        if prompt_lifetime.is_cancelled() {
            // A cancellation failure does not make the active Maple run
            // disappear. Keep listening so its lifecycle remains tracked.
            let _ = run_cancellation.cancel().await;
        }

        let mut cancel_after_result = false;
        let result = loop {
            if event_overflowed.load(Ordering::Acquire) {
                cancel_after_result = true;
                let _ = run_cancellation.cancel().await;
                match self
                    .send_final_agent_message(
                        cx,
                        request.session_id.clone(),
                        "Maple stopped this turn because its bounded ACP event stream overflowed.",
                        &self.lifetime,
                    )
                    .await
                {
                    Ok(()) | Err(AcpOutboundSendError::UpdateTooLarge) => {
                        break Ok(PromptResponse::new(StopReason::EndTurn));
                    }
                    Err(AcpOutboundSendError::Cancelled) => {
                        break Ok(PromptResponse::new(StopReason::Cancelled));
                    }
                    Err(AcpOutboundSendError::Transport(error)) => break Err(error),
                }
            }
            let event = events.recv().await;
            if event_overflowed.load(Ordering::Acquire) {
                cancel_after_result = true;
                let _ = run_cancellation.cancel().await;
                match self
                    .send_final_agent_message(
                        cx,
                        request.session_id.clone(),
                        "Maple stopped this turn because its bounded ACP event stream overflowed.",
                        &self.lifetime,
                    )
                    .await
                {
                    Ok(()) | Err(AcpOutboundSendError::UpdateTooLarge) => {
                        break Ok(PromptResponse::new(StopReason::EndTurn));
                    }
                    Err(AcpOutboundSendError::Cancelled) => {
                        break Ok(PromptResponse::new(StopReason::Cancelled));
                    }
                    Err(AcpOutboundSendError::Transport(error)) => break Err(error),
                }
            }
            match event {
                Some(AgentRunEvent::TimelineItem(item)) => {
                    if let Some(update) = timeline_update(&item) {
                        match self
                            .send_session_update(
                                cx,
                                SessionNotification::new(request.session_id.clone(), update),
                                &prompt_lifetime,
                            )
                            .await
                        {
                            Ok(()) => {}
                            Err(AcpOutboundSendError::UpdateTooLarge) => {
                                cancel_after_result = true;
                                let _ = run_cancellation.cancel().await;
                                match self.send_final_agent_message(
                                    cx,
                                    request.session_id.clone(),
                                    "Maple stopped this turn because one ACP update exceeded the 4 MiB transport limit.",
                                    &self.lifetime,
                                )
                                .await
                                {
                                    Ok(()) | Err(AcpOutboundSendError::UpdateTooLarge) => {
                                        break Ok(PromptResponse::new(StopReason::EndTurn));
                                    }
                                    Err(AcpOutboundSendError::Cancelled) => {
                                        break Ok(PromptResponse::new(StopReason::Cancelled));
                                    }
                                    Err(AcpOutboundSendError::Transport(error)) => break Err(error),
                                }
                            }
                            Err(AcpOutboundSendError::Cancelled) => {
                                cancel_after_result = true;
                                break Ok(PromptResponse::new(StopReason::Cancelled));
                            }
                            Err(AcpOutboundSendError::Transport(error)) => break Err(error),
                        }
                    }
                }
                Some(AgentRunEvent::PermissionRequested {
                    request: permission,
                    item,
                }) => {
                    match self
                        .request_permission_from_caller(
                            cx,
                            request.session_id.clone(),
                            permission,
                            &item,
                            &permission_responder,
                            &prompt_lifetime,
                        )
                        .await
                    {
                        Ok(AcpPermissionResolution::Continue) => {}
                        Ok(AcpPermissionResolution::Cancelled) => {
                            cancel_after_result = true;
                            break Ok(PromptResponse::new(StopReason::Cancelled));
                        }
                        Err(AcpOutboundSendError::UpdateTooLarge) => {
                            cancel_after_result = true;
                            let _ = run_cancellation.cancel().await;
                            match self
                                .send_final_agent_message(
                                    cx,
                                    request.session_id.clone(),
                                    "Maple stopped this turn because one ACP permission request exceeded the 4 MiB transport limit.",
                                    &self.lifetime,
                                )
                                .await
                            {
                                Ok(()) | Err(AcpOutboundSendError::UpdateTooLarge) => {
                                    break Ok(PromptResponse::new(StopReason::EndTurn));
                                }
                                Err(AcpOutboundSendError::Cancelled) => {
                                    break Ok(PromptResponse::new(StopReason::Cancelled));
                                }
                                Err(AcpOutboundSendError::Transport(error)) => break Err(error),
                            }
                        }
                        Err(AcpOutboundSendError::Cancelled) => {
                            cancel_after_result = true;
                            break Ok(PromptResponse::new(StopReason::Cancelled));
                        }
                        Err(AcpOutboundSendError::Transport(error)) => break Err(error),
                    }
                }
                Some(AgentRunEvent::Error(item)) => {
                    if let Some(message) = event_error_text(&item) {
                        match self
                            .send_session_update(
                                cx,
                                SessionNotification::new(
                                    request.session_id.clone(),
                                    SessionUpdate::AgentMessageChunk(ContentChunk::new(
                                        ContentBlock::Text(TextContent::new(message)),
                                    )),
                                ),
                                &prompt_lifetime,
                            )
                            .await
                        {
                            Ok(()) => {}
                            Err(AcpOutboundSendError::UpdateTooLarge) => {
                                cancel_after_result = true;
                                let _ = run_cancellation.cancel().await;
                                match self.send_final_agent_message(
                                    cx,
                                    request.session_id.clone(),
                                    "Maple stopped this turn because one ACP update exceeded the 4 MiB transport limit.",
                                    &self.lifetime,
                                )
                                .await
                                {
                                    Ok(()) | Err(AcpOutboundSendError::UpdateTooLarge) => {
                                        break Ok(PromptResponse::new(StopReason::EndTurn));
                                    }
                                    Err(AcpOutboundSendError::Cancelled) => {
                                        break Ok(PromptResponse::new(StopReason::Cancelled));
                                    }
                                    Err(AcpOutboundSendError::Transport(error)) => break Err(error),
                                }
                            }
                            Err(AcpOutboundSendError::Cancelled) => {
                                cancel_after_result = true;
                                break Ok(PromptResponse::new(StopReason::Cancelled));
                            }
                            Err(AcpOutboundSendError::Transport(error)) => break Err(error),
                        }
                    }
                }
                Some(AgentRunEvent::Finished(terminal)) => {
                    break prompt_result_from_terminal(terminal);
                }
                Some(
                    AgentRunEvent::SessionUpdated(_)
                    | AgentRunEvent::Started
                    | AgentRunEvent::SetupWarning(_)
                    | AgentRunEvent::HistoryReplaced
                    | AgentRunEvent::QueueChanged(_)
                    | AgentRunEvent::QueuePromoted { .. },
                ) => {}
                None => {
                    let current_terminal = *terminal.borrow();
                    let fallback = match current_terminal {
                        Some(terminal) => Some(terminal),
                        None => match terminal.changed().await {
                            Ok(()) => *terminal.borrow_and_update(),
                            Err(_) => *terminal.borrow(),
                        },
                    };
                    if let Some(terminal) = fallback {
                        break prompt_result_from_terminal(terminal);
                    }
                    break Err(agent_client_protocol::Error::internal_error()
                        .data("Maple Agent run ended without a terminal result"));
                }
            }
        };
        let mut deferred_prompt_cleanup = false;
        if cancel_after_result {
            // Synthetic stream stops settle only after the underlying run has
            // drained, or retain a same-session fence while it finishes in the
            // background. A completed run makes this cancellation a no-op.
            let _ = run_cancellation.cancel().await;
            if tokio::time::timeout(
                ACP_SYNTHETIC_STOP_DRAIN_TIMEOUT,
                wait_for_retained_terminal(&mut terminal),
            )
            .await
            .is_err()
            {
                // Do not let Buzz start a replacement turn against the same
                // Goose session while cancellation is still draining. The ACP
                // response remains bounded, while this retained state and task
                // own the terminal barrier asynchronously.
                deferred_prompt_cleanup = true;
                let context = Arc::clone(self);
                let draining_session_id = session_id.clone();
                let mut tasks = self.background_tasks.lock().await;
                tasks.spawn(async move {
                    wait_for_retained_terminal(&mut terminal).await;
                    if matches!(
                        context
                            .prompt_states
                            .lock()
                            .await
                            .remove(&draining_session_id),
                        Some(AcpPromptState::Running { .. })
                    ) {
                        context.stats.active_runs.fetch_sub(1, Ordering::SeqCst);
                    }
                });
            }
        } else if result.is_err() {
            let _ = run_cancellation.cancel().await;
        }
        if !deferred_prompt_cleanup
            && matches!(
                self.prompt_states.lock().await.remove(&session_id),
                Some(AcpPromptState::Running { .. })
            )
        {
            self.stats.active_runs.fetch_sub(1, Ordering::SeqCst);
        }
        result
    }

    async fn cancel(
        &self,
        notification: CancelNotification,
    ) -> Result<(), agent_client_protocol::Error> {
        let session_id = notification.session_id.0.to_string();
        let (cancellation, run_cancellation) = {
            let states = self.prompt_states.lock().await;
            match states.get(&session_id) {
                Some(AcpPromptState::Starting { cancellation }) => {
                    (Some(cancellation.clone()), None)
                }
                Some(AcpPromptState::Running {
                    cancellation,
                    run_cancellation,
                }) => (Some(cancellation.clone()), Some(run_cancellation.clone())),
                None => (None, None),
            }
        };
        if let Some(cancellation) = cancellation {
            // This token reaches core setup before a run ID exists and fences
            // the worker start once core setup completes.
            cancellation.cancel();
        }
        if let Some(run_cancellation) = run_cancellation {
            run_cancellation
                .cancel()
                .await
                .map_err(internal_acp_error)?;
        }
        Ok(())
    }

    async fn cleanup(&self) {
        let deadline = tokio::time::Instant::now() + ACP_CONNECTION_CLEANUP_TIMEOUT;
        {
            // Linearize closure with the last new-session credential commit.
            // A task that reaches finalization after this point observes closed
            // and rolls its newly persisted session back instead of committing.
            let _finalization = self.finalization.lock().await;
            self.closed.store(true, Ordering::SeqCst);
            self.lifetime.cancel();
            self.bridge_environment.lock().await.clear();
            if self.has_credentials.swap(false, Ordering::SeqCst) {
                self.stats
                    .credential_connections
                    .fetch_sub(1, Ordering::SeqCst);
            }
        }
        let prompt_states = std::mem::take(&mut *self.prompt_states.lock().await);
        let mut running_cancellations = Vec::new();
        for state in prompt_states.into_values() {
            match state {
                AcpPromptState::Starting { cancellation } => cancellation.cancel(),
                AcpPromptState::Running {
                    cancellation,
                    run_cancellation,
                } => {
                    cancellation.cancel();
                    running_cancellations.push(run_cancellation);
                    self.stats.active_runs.fetch_sub(1, Ordering::SeqCst);
                }
            }
        }
        let sessions = std::mem::take(&mut *self.sessions.lock().await);
        let session_count = sessions.len();
        // Revoke every capability synchronously before awaiting registry cleanup.
        // No queued or detached task can launch another credential-bearing tool
        // after this barrier returns.
        for lease in sessions.values() {
            lease.revoke();
        }
        self.stats
            .active_sessions
            .fetch_sub(session_count, Ordering::SeqCst);
        let mut tasks = self.background_tasks.lock().await;
        for run_cancellation in running_cancellations {
            tasks.spawn(async move {
                let _ = run_cancellation.cancel().await;
            });
        }
        for lease in sessions.into_values() {
            tasks.spawn(async move {
                lease.release().await;
            });
        }
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
                        let (prompt, prompt_lifetime) = match context.begin_prompt(&request).await {
                            Ok(prepared) => prepared,
                            Err(error) => {
                                responder.respond_with_error(error)?;
                                return Ok(());
                            }
                        };
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
                                prompt_context
                                    .prompt(&prompt_cx, request, prompt, prompt_lifetime)
                                    .await,
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
    lifecycle: tauri::State<'_, AgentHostLifecycle>,
    user_id: String,
    config: AgentAcpConfig,
) -> Result<AgentAcpConfig, String> {
    let _guard = lifecycle.lock().await;
    app_handle
        .state::<MapleAgentService>()
        .ensure_accepting_new_work()?;
    let config = normalize_config(config)?;
    let requested_scope = account_scope(&user_id)?;
    let state = app_handle.state::<AgentAcpState>();
    let running = state.running.lock().await;
    if let Some(running) = running.as_ref() {
        if running.account_scope == requested_scope {
            let current = running.config.read().await.clone();
            if running.stats.running.load(Ordering::SeqCst)
                && (current.permission_mode != config.permission_mode
                    || current.allowed_project_roots != config.allowed_project_roots
                    || current.max_connections != config.max_connections)
            {
                return Err(
                    "Stop the ACP service before changing its permission, project-root, or connection policy"
                        .to_string(),
                );
            }
        }
    }
    save_config(&app_handle, &user_id, &config)?;
    if let Some(running) = running.as_ref() {
        if running.account_scope == requested_scope {
            *running.config.write().await = config.clone();
        }
    }
    Ok(config)
}

#[tauri::command]
pub async fn agent_acp_start(
    app_handle: AppHandle,
    lifecycle: tauri::State<'_, AgentHostLifecycle>,
    user_id: String,
) -> Result<AgentAcpStatus, String> {
    let _guard = lifecycle.lock().await;
    app_handle
        .state::<MapleAgentService>()
        .ensure_accepting_new_work()?;
    start_service_locked(&app_handle, &user_id).await?;
    status(&app_handle, &user_id).await
}

#[tauri::command]
pub async fn agent_acp_stop(
    app_handle: AppHandle,
    lifecycle: tauri::State<'_, AgentHostLifecycle>,
    user_id: String,
) -> Result<AgentAcpStatus, String> {
    let _guard = lifecycle.lock().await;
    stop_service_locked(&app_handle, Some(&user_id), true).await?;
    status(&app_handle, &user_id).await
}

#[tauri::command]
pub async fn agent_acp_get_status(
    app_handle: AppHandle,
    user_id: String,
) -> Result<AgentAcpStatus, String> {
    status(&app_handle, &user_id).await
}

pub(crate) async fn shutdown_agent_acp_locked(
    app_handle: &AppHandle,
    requested_user: Option<&str>,
) -> Result<(), String> {
    stop_service_locked(app_handle, requested_user, false).await
}

#[cfg(unix)]
async fn start_service_locked(app_handle: &AppHandle, user_id: &str) -> Result<(), String> {
    let requested_scope = account_scope(user_id)
        .map_err(|_| "Cannot start ACP without an authenticated Maple user".to_string())?;
    let state = app_handle.state::<AgentAcpState>();
    let stale = {
        let mut slot = state.running.lock().await;
        match slot.as_ref() {
            Some(running) if running.stats.running.load(Ordering::SeqCst) => {
                if running.account_scope == requested_scope {
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
    let agent = app_handle
        .state::<MapleAgentService>()
        .handle_for_user(user_id)
        .await?;
    let maple_api_session = app_handle
        .state::<MapleApiAuthState>()
        .session_for(user_id)
        .await?;
    agent.start(maple_api_session, None).await?;
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
        agent,
        Arc::clone(&config),
        Arc::clone(&stats),
        cancellation.clone(),
    ));
    *state.running.lock().await = Some(RunningAgentAcp {
        account_scope: requested_scope,
        endpoint,
        config,
        stats,
        cancellation,
        task,
    });
    Ok(())
}

#[cfg(not(unix))]
async fn start_service_locked(_app_handle: &AppHandle, _user_id: &str) -> Result<(), String> {
    Err("Maple ACP local IPC is not yet supported on this platform".to_string())
}

async fn stop_service_locked(
    app_handle: &AppHandle,
    requested_user: Option<&str>,
    persist_disabled: bool,
) -> Result<(), String> {
    let state = app_handle.state::<AgentAcpState>();
    let requested_scope = requested_user.map(account_scope).transpose()?;
    let running = {
        let mut slot = state.running.lock().await;
        if let (Some(requested), Some(running)) = (requested_scope.as_deref(), slot.as_ref()) {
            if running.account_scope != requested {
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
        save_config_for_scope(app_handle, &running.account_scope, &config)?;
    }
    Ok(())
}

async fn status(app_handle: &AppHandle, user_id: &str) -> Result<AgentAcpStatus, String> {
    let state = app_handle.state::<AgentAcpState>();
    let requested_scope = account_scope(user_id)?;
    let running = state.running.lock().await;
    let harness = harness()?;
    if let Some(running) = running.as_ref() {
        if running.account_scope != requested_scope {
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
    agent: AgentRuntimeHandle,
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
                    let agent = agent.clone();
                    let config = Arc::clone(&config);
                    let stats = Arc::clone(&stats);
                    let connection_cancel = cancellation.clone();
                    connections.spawn(async move {
                        let _permit = permit;
                        stats.connected_clients.fetch_add(1, Ordering::SeqCst);
                        let context = AcpConnectionContext::new(
                            agent,
                            config,
                            Arc::clone(&stats),
                        );
                        let (read, write) = stream.into_split();
                        let peer_eof = CancellationToken::new();
                        let read = BoundedLineReader::new(read, peer_eof.clone());
                        let incoming = FramedRead::new(
                            read,
                            LinesCodec::new_with_max_length(MAX_ACP_FRAME_BYTES),
                        )
                        .map(|result| {
                            result.map_err(|error| {
                                std::io::Error::new(std::io::ErrorKind::InvalidData, error)
                            })
                        });
                        let outgoing = tracked_outgoing_lines(
                            write,
                            Arc::clone(&context.outbound),
                        );
                        let serving = AcpAgent
                            .builder()
                            .name("maple-acp")
                            .with_handler(MapleAcpHandler {
                                context: Arc::clone(&context),
                            })
                            .connect_to(Lines::new(outgoing, incoming));
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
    let scope = account_scope(user_id)
        .map_err(|_| "Maple ACP configuration requires an authenticated user".to_string())?;
    config_path_for_scope(app_handle, &scope)
}

fn config_path_for_scope(app_handle: &AppHandle, scope: &str) -> Result<PathBuf, String> {
    Ok(acp_accounts_root(app_handle)?
        .join(scope)
        .join("config.json"))
}

fn acp_accounts_root(app_handle: &AppHandle) -> Result<PathBuf, String> {
    let root = app_handle
        .path()
        .app_local_data_dir()
        .map_err(|error| format!("Failed to resolve Maple local data: {error}"))?;
    Ok(root.join("acp").join("accounts"))
}

fn legacy_config_path(app_handle: &AppHandle, user_id: &str) -> Result<PathBuf, String> {
    if user_id.trim().is_empty() {
        return Err("Maple ACP configuration requires an authenticated user".to_string());
    }
    let digest = Sha256::digest(user_id.as_bytes());
    let legacy_scope = digest[..16]
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect::<String>();
    Ok(acp_accounts_root(app_handle)?
        .join(legacy_scope)
        .join("config.json"))
}

fn load_config(app_handle: &AppHandle, user_id: &str) -> Result<AgentAcpConfig, String> {
    let path = config_path(app_handle, user_id)?;
    match std::fs::read(&path) {
        Ok(bytes) => serde_json::from_slice(&bytes)
            .map_err(|error| format!("Failed to parse Maple ACP configuration: {error}"))
            .and_then(normalize_config),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            let legacy_path = legacy_config_path(app_handle, user_id)?;
            match std::fs::read(legacy_path) {
                Ok(bytes) => {
                    let config = serde_json::from_slice(&bytes)
                        .map_err(|error| {
                            format!("Failed to parse Maple ACP configuration: {error}")
                        })
                        .and_then(normalize_config)?;
                    // Keep the POC file intact so switching back to the original
                    // branch remains harmless, while future saves use Maple's
                    // canonical full account scope.
                    if let Err(error) = save_config(app_handle, user_id, &config) {
                        log::warn!("Failed to migrate Maple ACP configuration: {error}");
                    }
                    Ok(config)
                }
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                    Ok(AgentAcpConfig::default())
                }
                Err(error) => Err(format!("Failed to read Maple ACP configuration: {error}")),
            }
        }
        Err(error) => Err(format!("Failed to read Maple ACP configuration: {error}")),
    }
}

fn save_config(
    app_handle: &AppHandle,
    user_id: &str,
    config: &AgentAcpConfig,
) -> Result<(), String> {
    let scope = account_scope(user_id)
        .map_err(|_| "Maple ACP configuration requires an authenticated user".to_string())?;
    save_config_for_scope(app_handle, &scope, config)
}

fn save_config_for_scope(
    app_handle: &AppHandle,
    account_scope: &str,
    config: &AgentAcpConfig,
) -> Result<(), String> {
    let path = config_path_for_scope(app_handle, account_scope)?;
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
    let paths = [
        config_path(app_handle, user_id)?,
        legacy_config_path(app_handle, user_id)?,
    ];
    for path in paths {
        let account_dir = path
            .parent()
            .ok_or_else(|| "Invalid Maple ACP configuration path".to_string())?;
        match std::fs::remove_dir_all(account_dir) {
            Ok(()) => {}
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => {
                return Err(format!("Failed to clear Maple ACP configuration: {error}"));
            }
        }
    }
    Ok(())
}

fn normalize_config(mut config: AgentAcpConfig) -> Result<AgentAcpConfig, String> {
    // `allow_all` was the exploratory Desktop-owned bypass. Caller-owned ACP
    // supersedes it; old files migrate to the guarded policy on their next load.
    config.permission_mode = AgentAcpPermissionMode::ReadOnly;
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

fn ensure_allowed_project_root(cwd: &Path, allowed_roots: &[String]) -> Result<PathBuf, String> {
    let cwd = cwd
        .canonicalize()
        .map_err(|error| format!("Failed to resolve ACP session cwd: {error}"))?;
    if allowed_roots.is_empty() {
        return Ok(cwd);
    }
    for root in allowed_roots {
        if let Ok(root) = Path::new(root).canonicalize() {
            if cwd.starts_with(root) {
                return Ok(cwd);
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

pub(crate) fn default_tool_context_spec() -> Result<AgentToolContextSpec, String> {
    AgentToolContextSpec::try_new(
        BTreeMap::new(),
        SENSITIVE_BRIDGE_ENV
            .into_iter()
            .map(str::to_string)
            .collect(),
        false,
    )
}

fn bridge_tool_context_spec(
    environment: &HashMap<String, String>,
) -> Result<AgentToolContextSpec, String> {
    let values = environment
        .iter()
        .map(|(key, value)| (key.clone(), value.clone()))
        .collect::<BTreeMap<_, _>>();
    let scrub_from_parent = SENSITIVE_BRIDGE_ENV
        .into_iter()
        .map(str::to_string)
        .collect::<BTreeSet<_>>();
    let ephemeral = SENSITIVE_BRIDGE_ENV
        .iter()
        .any(|key| environment.contains_key(*key));
    AgentToolContextSpec::try_new(values, scrub_from_parent, ephemeral)
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

fn acp_permission_tool_call(
    request: &AgentPermissionRequest,
    item: &AgentTimelineItem,
) -> ToolCall {
    let title = item
        .title
        .clone()
        .unwrap_or_else(|| format!("Approve {}", request.tool_name));
    let mut tool_call = ToolCall::new(request.request_id.clone(), title)
        .kind(acp_tool_kind(&request.tool_name))
        .status(ToolCallStatus::Pending)
        .raw_input(serde_json::Value::Object(request.arguments.clone()));
    if let Some(prompt) = request.prompt.as_ref().filter(|prompt| !prompt.is_empty()) {
        tool_call = tool_call.content(vec![ToolCallContent::from(ContentBlock::Text(
            TextContent::new(prompt.clone()),
        ))]);
    }
    tool_call
}

fn acp_tool_kind(tool_name: &str) -> ToolKind {
    match tool_name.rsplit("__").next().unwrap_or(tool_name) {
        "shell" | "computer" => ToolKind::Execute,
        "text_editor" => ToolKind::Edit,
        "search" | "web_search" => ToolKind::Search,
        "open_url" => ToolKind::Fetch,
        _ => ToolKind::Other,
    }
}

fn acp_permission_options() -> Vec<PermissionOption> {
    vec![
        PermissionOption::new("allow_once", "Allow once", PermissionOptionKind::AllowOnce),
        PermissionOption::new(
            "reject_once",
            "Reject once",
            PermissionOptionKind::RejectOnce,
        ),
    ]
}

fn acp_permission_decision(
    outcome: &RequestPermissionOutcome,
) -> Result<AgentPermissionDecision, String> {
    match outcome {
        RequestPermissionOutcome::Cancelled => Ok(AgentPermissionDecision::Cancel),
        RequestPermissionOutcome::Selected(selected)
            if selected.option_id.0.as_ref() == "allow_once" =>
        {
            Ok(AgentPermissionDecision::AllowOnce)
        }
        RequestPermissionOutcome::Selected(selected)
            if selected.option_id.0.as_ref() == "reject_once" =>
        {
            Ok(AgentPermissionDecision::DenyOnce)
        }
        RequestPermissionOutcome::Selected(_) => {
            Err("ACP client selected an unknown Maple permission option".to_string())
        }
        _ => Err("ACP client returned an unsupported Maple permission outcome".to_string()),
    }
}

async fn cancel_maple_permission(responder: &AgentRunPermissionResponder, request_id: &str) {
    if let Err(error) = responder
        .respond(request_id.to_string(), AgentPermissionDecision::Cancel)
        .await
    {
        log::debug!(
            "Maple ACP permission request {request_id} was already resolved while failing closed: {error}"
        );
    }
}

fn retain_cancelled_permission_request<F, T>(response: F, reservation: AcpOutboundReservation)
where
    F: std::future::Future<Output = T> + Send + 'static,
    T: Send + 'static,
{
    tokio::spawn(async move {
        let _reservation = reservation;
        let _ = response.await;
    });
}

fn timeline_update(item: &AgentTimelineItem) -> Option<SessionUpdate> {
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

fn event_error_text(item: &AgentTimelineItem) -> Option<String> {
    item.text.clone().map(|message| bounded_error(&message))
}

async fn wait_for_retained_terminal(
    terminal: &mut tokio::sync::watch::Receiver<Option<AgentRunTerminal>>,
) {
    loop {
        if terminal.borrow().is_some() {
            return;
        }
        if terminal.changed().await.is_err() {
            return;
        }
    }
}

fn prompt_result_from_terminal(
    terminal: AgentRunTerminal,
) -> Result<PromptResponse, agent_client_protocol::Error> {
    match terminal {
        AgentRunTerminal::Completed => Ok(PromptResponse::new(StopReason::EndTurn)),
        AgentRunTerminal::Cancelled => Ok(PromptResponse::new(StopReason::Cancelled)),
        // A failed terminal is emitted only after a run was admitted. Goose may
        // already have persisted output or executed tools, and Buzz treats a
        // JSON-RPC AgentError as pre-mutation/retryable. The preceding error
        // update carries the failure text; settle the turn successfully here so
        // non-idempotent work is never replayed automatically.
        AgentRunTerminal::Failed => Ok(PromptResponse::new(StopReason::EndTurn)),
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
    use agent_client_protocol::schema::v1::SelectedPermissionOutcome;

    #[test]
    fn default_config_is_disabled_and_caller_mediated() {
        let config = AgentAcpConfig::default();
        assert!(!config.enabled);
        assert_eq!(config.permission_mode, AgentAcpPermissionMode::ReadOnly);
        assert_eq!(config.max_connections, 1);
    }

    #[test]
    fn legacy_allow_all_cannot_bypass_the_acp_caller() {
        assert_eq!(
            AgentAcpPermissionMode::ReadOnly.maple_mode(),
            "smart_approve"
        );
        assert_eq!(
            AgentAcpPermissionMode::AllowAll.maple_mode(),
            "smart_approve"
        );
        let migrated = normalize_config(AgentAcpConfig {
            permission_mode: AgentAcpPermissionMode::AllowAll,
            ..AgentAcpConfig::default()
        })
        .unwrap();
        assert_eq!(migrated.permission_mode, AgentAcpPermissionMode::ReadOnly);
    }

    #[test]
    fn permission_request_exposes_only_one_shot_caller_choices() {
        let request = AgentPermissionRequest {
            request_id: "request-1".to_string(),
            tool_name: "developer__shell".to_string(),
            arguments: serde_json::Map::from_iter([(
                "command".to_string(),
                serde_json::json!("git push"),
            )]),
            prompt: Some("Push this branch?".to_string()),
        };
        let item = AgentTimelineItem {
            id: "permission-request-1".to_string(),
            item_type: "permission".to_string(),
            role: Some("system".to_string()),
            title: Some("Push branch".to_string()),
            text: request.prompt.clone(),
            status: Some("pending".to_string()),
            input: Some(serde_json::Value::Object(request.arguments.clone())),
            output: None,
            created_ms: 1,
            merge: "replace".to_string(),
        };
        let permission = RequestPermissionRequest::new(
            "session-1",
            acp_permission_tool_call(&request, &item).into(),
            acp_permission_options(),
        );
        let encoded = serde_json::to_value(permission).unwrap();

        assert_eq!(encoded["toolCall"]["toolCallId"], "request-1");
        assert_eq!(encoded["toolCall"]["title"], "Push branch");
        assert_eq!(encoded["toolCall"]["kind"], "execute");
        assert_eq!(encoded["toolCall"]["status"], "pending");
        assert_eq!(encoded["toolCall"]["rawInput"]["command"], "git push");
        assert_eq!(
            encoded["options"],
            serde_json::json!([
                { "optionId": "allow_once", "name": "Allow once", "kind": "allow_once" },
                { "optionId": "reject_once", "name": "Reject once", "kind": "reject_once" }
            ])
        );
    }

    #[test]
    fn permission_outcomes_map_fail_closed() {
        assert_eq!(
            acp_permission_decision(&RequestPermissionOutcome::Selected(
                SelectedPermissionOutcome::new("allow_once")
            )),
            Ok(AgentPermissionDecision::AllowOnce)
        );
        assert_eq!(
            acp_permission_decision(&RequestPermissionOutcome::Selected(
                SelectedPermissionOutcome::new("reject_once")
            )),
            Ok(AgentPermissionDecision::DenyOnce)
        );
        assert_eq!(
            acp_permission_decision(&RequestPermissionOutcome::Cancelled),
            Ok(AgentPermissionDecision::Cancel)
        );
        assert!(acp_permission_decision(&RequestPermissionOutcome::Selected(
            SelectedPermissionOutcome::new("allow_always")
        ))
        .is_err());
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn outbound_tracker_releases_credit_only_after_a_socket_write_acknowledgement() {
        use futures_util::SinkExt as _;
        use tokio::io::AsyncReadExt as _;

        let tracker = AcpOutboundTracker::with_limits(1, 1024);
        let cancellation = CancellationToken::new();
        let first = tracker.reserve(1, &cancellation).await.unwrap();
        tracker
            .pending
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .push_back(first);

        let waiting_tracker = Arc::clone(&tracker);
        let waiting_cancellation = cancellation.clone();
        let waiting =
            tokio::spawn(async move { waiting_tracker.reserve(1, &waiting_cancellation).await });
        tokio::task::yield_now().await;
        assert!(!waiting.is_finished());

        let line = r#"{"jsonrpc":"2.0","method":"session/update","params":{}}"#;
        let (writer, mut reader) = tokio::io::duplex(1024);
        let mut sink = Box::pin(tracked_outgoing_lines(writer, Arc::clone(&tracker)));
        sink.send(line.to_string()).await.unwrap();
        let mut written = vec![0_u8; line.len() + 1];
        reader.read_exact(&mut written).await.unwrap();
        assert_eq!(written, format!("{line}\n").into_bytes());

        let second = waiting.await.unwrap().unwrap();
        drop(second);
    }

    #[tokio::test]
    async fn cancelled_permission_retains_credit_until_the_orphan_request_settles() {
        let tracker = AcpOutboundTracker::with_limits(1, 1024);
        let cancellation = CancellationToken::new();
        let first = tracker.reserve(1, &cancellation).await.unwrap();
        let (settled_tx, settled_rx) = tokio::sync::oneshot::channel::<()>();
        retain_cancelled_permission_request(
            async move {
                let _ = settled_rx.await;
            },
            first,
        );

        assert!(tokio::time::timeout(
            std::time::Duration::from_millis(10),
            tracker.reserve(1, &cancellation),
        )
        .await
        .is_err());

        settled_tx.send(()).unwrap();
        let second = tokio::time::timeout(
            std::time::Duration::from_secs(1),
            tracker.reserve(1, &cancellation),
        )
        .await
        .unwrap()
        .unwrap();
        drop(second);
    }

    #[test]
    fn outbound_credit_acknowledges_only_session_updates() {
        assert!(is_session_update_line(
            r#"{"jsonrpc":"2.0","method":"session/update","params":{}}"#
        ));
        assert!(!is_session_update_line(
            r#"{"jsonrpc":"2.0","id":1,"result":{}}"#
        ));
    }

    #[test]
    fn allowed_project_root_returns_the_canonical_admitted_path() {
        let root = tempfile::tempdir().unwrap();
        let project = root.path().join("project");
        std::fs::create_dir(&project).unwrap();

        let admitted =
            ensure_allowed_project_root(&project, &[root.path().to_string_lossy().into_owned()])
                .unwrap();

        assert_eq!(admitted, project.canonicalize().unwrap());
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

        let failed = prompt_result_from_terminal(AgentRunTerminal::Failed).unwrap();
        assert_eq!(
            serde_json::to_value(failed).unwrap()["stopReason"],
            "end_turn"
        );
    }
}
