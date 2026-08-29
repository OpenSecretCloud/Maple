use crate::agent::{
    AGENT_TOOL_CONTEXT_INACTIVE_ERROR, AgentCreateSessionRequest, AgentHostEventPolicy,
    AgentMcpKeyValue, AgentPermissionDecision, AgentPermissionRequest, AgentRunCancellation,
    AgentRunEvent, AgentRunPermissionResponder, AgentRunTerminal, AgentRunUsage,
    AgentRuntimeHandle, AgentSendMessageRequest, AgentSessionSummary, AgentTimelineItem,
    AgentToolContextLease, AgentToolContextSpec, AgentTransientMcpServer,
    AgentTransientMcpTransport, SENSITIVE_BRIDGE_ENV, compaction_notice_text,
};
use crate::maple_api::account_scope;
use agent_client_protocol::schema::v1::{
    AgentCapabilities, BooleanPropertySchema, CancelNotification, CloseSessionRequest,
    CloseSessionResponse, ConfigOptionUpdate, ContentBlock, ContentChunk, CreateElicitationRequest,
    ElicitationAction, ElicitationContentValue, ElicitationFormMode, ElicitationSchema,
    ElicitationSessionScope, Implementation, InitializeRequest, InitializeResponse,
    ListSessionsRequest, ListSessionsResponse, LoadSessionRequest, LoadSessionResponse,
    McpCapabilities, McpServer, NewSessionRequest, NewSessionResponse, PermissionOption,
    PermissionOptionKind, PromptCapabilities, PromptRequest, PromptResponse,
    RequestPermissionOutcome, RequestPermissionRequest, SessionCapabilities,
    SessionCloseCapabilities, SessionConfigOption, SessionConfigOptionCategory,
    SessionConfigSelectOption, SessionId, SessionInfo, SessionListCapabilities, SessionMode,
    SessionModeState, SessionNotification, SessionUpdate, SetSessionConfigOptionRequest,
    SetSessionConfigOptionResponse, SetSessionModeRequest, SetSessionModeResponse, StopReason,
    TextContent, ToolCall, ToolCallContent, ToolCallLocation, ToolCallStatus, ToolCallUpdate,
    ToolCallUpdateFields, ToolKind, Usage,
};
use agent_client_protocol::util::MatchDispatchFrom;
use agent_client_protocol::{
    Agent as AcpAgent, Client, ConnectionTo, Dispatch, HandleDispatchFrom, Handled,
    JsonRpcNotification, Lines, Responder,
};
use futures_util::StreamExt as _;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet, VecDeque};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use tokio::sync::{Mutex, OwnedSemaphorePermit, RwLock, Semaphore};
use tokio_util::codec::{FramedRead, LinesCodec};
use tokio_util::sync::CancellationToken;

#[cfg(unix)]
use std::os::unix::fs::PermissionsExt as _;

const MAX_ACP_CONNECTIONS: usize = 8;
const MAX_ACP_ERROR_CHARS: usize = 500;
const MAX_ACP_FRAME_BYTES: usize = 10 * 1024 * 1024;
const MAX_ACP_OUTBOUND_EVENTS_IN_FLIGHT: usize = 256;
const MAX_ACP_OUTBOUND_BYTES_IN_FLIGHT: usize = 4 * 1024 * 1024;
const ACP_OUTBOUND_FRAME_OVERHEAD_BYTES: usize = 256;
const ACP_CONNECTION_CLEANUP_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(5);
const ACP_SYNTHETIC_STOP_DRAIN_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(5);
const ACP_SESSION_CLOSE_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(5);
const ACP_TRANSIENT_MCP_TIMEOUT_SECONDS: u64 = 30;
const ACP_LOADABLE_GOOSE_MODE: &str = "smart_approve";
const ALLOWED_BRIDGE_ENV: [&str; 6] = [
    "BUZZ_RELAY_URL",
    "BUZZ_PRIVATE_KEY",
    "BUZZ_AUTH_TAG",
    "BUZZ_API_TOKEN",
    "BUZZ_ACP_DISPLAY_NAME",
    "PATH",
];
static NEXT_ACP_MESSAGE_ID: AtomicUsize = AtomicUsize::new(1);

struct BoundedLineReader<R> {
    inner: R,
    bytes_since_newline: usize,
    eof: CancellationToken,
}

impl<R> BoundedLineReader<R> {
    fn new(inner: R, eof: CancellationToken) -> Self {
        Self {
            inner,
            bytes_since_newline: 0,
            eof,
        }
    }
}

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
    8
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

#[derive(Default)]
struct AgentAcpStats {
    active_sessions: AtomicUsize,
    active_runs: AtomicUsize,
    credential_connections: AtomicUsize,
}

/// Serve ACP on this process's stdin and stdout for one signed-in account.
///
/// `maple-gpui acp` calls this after it started the agent runtime. The
/// editor that spawned the process owns the connection: when its stdin
/// closes, every open session is cleaned up and the call returns. The
/// bridge environment (Buzz credentials and `PATH`) is read from this
/// process's own environment.
pub async fn serve_stdio(agent: AgentRuntimeHandle, config: AgentAcpConfig) -> Result<(), String> {
    let config = Arc::new(RwLock::new(normalize_config(config)?));
    let stats = Arc::new(AgentAcpStats::default());
    let context = AcpConnectionContext::new(agent, config, stats);
    let environment = ALLOWED_BRIDGE_ENV
        .iter()
        .filter_map(|key| {
            std::env::var(key)
                .ok()
                .map(|value| ((*key).to_string(), value))
        })
        .collect::<HashMap<_, _>>();
    context.set_bridge_environment(environment).await;

    let peer_eof = CancellationToken::new();
    let read = BoundedLineReader::new(tokio::io::stdin(), peer_eof.clone());
    let incoming =
        FramedRead::new(read, LinesCodec::new_with_max_length(MAX_ACP_FRAME_BYTES)).map(|result| {
            result.map_err(|error| std::io::Error::new(std::io::ErrorKind::InvalidData, error))
        });
    let outgoing = tracked_outgoing_lines(tokio::io::stdout(), Arc::clone(&context.outbound));
    let serving = AcpAgent
        .builder()
        .name("maple-acp")
        .with_handler(MapleAcpHandler {
            context: Arc::clone(&context),
        })
        .connect_to(Lines::new(outgoing, incoming));
    let result = tokio::select! {
        result = serving => result.map_err(|error| bounded_error(&error.to_string())),
        _ = peer_eof.cancelled() => Ok(()),
    };
    context.cleanup().await;
    result
}

/// Per-account ACP configuration saved under `local_data_root`.
pub fn load_acp_config(local_data_root: &Path, user_id: &str) -> Result<AgentAcpConfig, String> {
    load_config(local_data_root, user_id)
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
    sessions: Mutex<HashMap<String, AcpSession>>,
    session_operations: Mutex<HashMap<String, Arc<AcpSessionOperation>>>,
    closing_sessions: Mutex<HashSet<String>>,
    prompt_states: Mutex<HashMap<String, AcpPromptState>>,
    background_tasks: Mutex<tokio::task::JoinSet<()>>,
    finalization: Mutex<()>,
    lifetime: CancellationToken,
    closed: AtomicBool,
    has_credentials: AtomicBool,
    client_supports_form_elicitation: AtomicBool,
    outbound: Arc<AcpOutboundTracker>,
}

struct AcpSession {
    lease: Option<AgentToolContextLease>,
    model: String,
    available_models: Vec<String>,
    message_count: usize,
    created_here: bool,
    prompted: bool,
    project_root: PathBuf,
    project_trust_decision: Option<bool>,
}

struct UnpublishedAcpSession {
    lease: Option<AgentToolContextLease>,
    published: bool,
}

impl UnpublishedAcpSession {
    fn new(lease: AgentToolContextLease) -> Self {
        Self {
            lease: Some(lease),
            published: false,
        }
    }

    fn publish(mut self) -> AgentToolContextLease {
        self.published = true;
        self.lease
            .take()
            .expect("an unpublished ACP session must still own its lease")
    }
}

impl Drop for UnpublishedAcpSession {
    fn drop(&mut self) {
        if self.published {
            return;
        }
        let Some(lease) = self.lease.take() else {
            return;
        };
        lease.revoke();
        if let Ok(runtime) = tokio::runtime::Handle::try_current() {
            runtime.spawn(async move {
                lease.discard_created_if_untouched().await;
            });
        }
    }
}

struct AcpSessionOperation {
    gate: Arc<Mutex<()>>,
    cancellation: CancellationToken,
}

fn close_registration_may_be_released(
    cancellation_completed: bool,
    operation_drained: bool,
    cleanup_completed: bool,
) -> bool {
    // A timed-out load may already have passed its final core cancellation
    // check. Keep both its exact operation registration and the closing
    // tombstone until connection teardown so it can never publish a lease
    // after session/close has returned. The same fence stays in place while
    // run cancellation or exact-match lease cleanup is still settling.
    cancellation_completed && operation_drained && cleanup_completed
}

impl AcpSessionOperation {
    fn new(connection_lifetime: &CancellationToken) -> Arc<Self> {
        Arc::new(Self {
            gate: Arc::new(Mutex::new(())),
            cancellation: connection_lifetime.child_token(),
        })
    }
}

impl AcpSession {
    fn config_options(&self) -> Vec<SessionConfigOption> {
        acp_session_config_options(&self.model, &self.available_models, self.message_count)
    }
}

#[derive(Default)]
struct AcpToolProjection {
    seen: HashSet<String>,
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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum AcpProjectTrustResolution {
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
            session_operations: Mutex::new(HashMap::new()),
            closing_sessions: Mutex::new(HashSet::new()),
            prompt_states: Mutex::new(HashMap::new()),
            background_tasks: Mutex::new(tokio::task::JoinSet::new()),
            finalization: Mutex::new(()),
            lifetime: CancellationToken::new(),
            closed: AtomicBool::new(false),
            has_credentials: AtomicBool::new(false),
            client_supports_form_elicitation: AtomicBool::new(false),
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
        caller: AcpCallerSessionFields,
    ) -> Result<NewSessionResponse, agent_client_protocol::Error> {
        if self.closed.load(Ordering::SeqCst) {
            return Err(agent_client_protocol::Error::internal_error()
                .data("The Maple ACP connection is closing"));
        }
        if !request.cwd.is_absolute() {
            return Err(agent_client_protocol::Error::invalid_params()
                .data("ACP session cwd must be an absolute path"));
        }
        if !request.additional_directories.is_empty() {
            return Err(agent_client_protocol::Error::invalid_params()
                .data("Maple ACP does not support additional session directories"));
        }
        let config = self.config.read().await.clone();
        let project_root = ensure_allowed_project_root(&request.cwd, &config.allowed_project_roots)
            .map_err(|error| agent_client_protocol::Error::invalid_params().data(error))?;
        let project_trust = self
            .agent
            .get_project_trust(project_root.to_string_lossy().into_owned())
            .await
            .map_err(internal_acp_error)?;

        let available_models = self.available_models().await?;
        let model = available_models
            .first()
            .cloned()
            .ok_or_else(|| internal_acp_error("Maple returned no models".to_string()))?;
        let bridge_environment = self.bridge_environment.lock().await.clone();
        let (environment, transient_mcp_servers) =
            prepare_session_mcp(&bridge_environment, &request.mcp_servers)?;
        let tool_context = bridge_tool_context_spec(&environment).map_err(internal_acp_error)?;
        let mode = config.permission_mode.maple_mode().to_string();
        let created = self
            .agent
            .create_session_with_surface_context(
                Some(AgentCreateSessionRequest {
                    project_root: Some(project_root.to_string_lossy().into_owned()),
                    title: Some(
                        caller
                            .session_title
                            .unwrap_or_else(|| "Maple ACP".to_string()),
                    ),
                    model: Some(model.clone()),
                    context_limit: None,
                    mode: Some(mode),
                    mcp_server_names: None,
                    system_prompt: caller.system_prompt,
                }),
                Some(tool_context),
                transient_mcp_servers,
                self.lifetime.child_token(),
                // The ACP caller is the only interactive surface for this task.
                // Persisted history remains loadable in Maple Desktop, but live
                // permission cards must never create a second approval broker.
                AgentHostEventPolicy::Suppress,
            )
            .await
            .map_err(internal_acp_error)?;
        let session_id = canonical_session_id_text(&created.detail.session.id)?;
        let lease = created
            .tool_context_lease
            .expect("an explicit Agent tool context must return a lease");
        let unpublished = UnpublishedAcpSession::new(lease);
        let finalization = self.finalization.lock().await;
        if self.closed.load(Ordering::SeqCst) {
            drop(finalization);
            drop(unpublished);
            return Err(agent_client_protocol::Error::internal_error()
                .data("The Maple ACP connection closed while configuring the session"));
        }
        let mut sessions = self.sessions.lock().await;
        let mut operations = self.session_operations.lock().await;
        if sessions.contains_key(&session_id) || operations.contains_key(&session_id) {
            drop(operations);
            drop(sessions);
            drop(finalization);
            drop(unpublished);
            return Err(agent_client_protocol::Error::internal_error()
                .data("The Maple ACP connection duplicated a new session"));
        }
        let lease = unpublished.publish();
        sessions.insert(
            session_id.clone(),
            AcpSession {
                lease: Some(lease),
                model: model.clone(),
                available_models: available_models.clone(),
                message_count: created.detail.session.message_count,
                created_here: true,
                prompted: false,
                project_root: project_root.clone(),
                project_trust_decision: project_trust.decision,
            },
        );
        operations.insert(session_id.clone(), AcpSessionOperation::new(&self.lifetime));
        drop(operations);
        drop(sessions);
        self.stats.active_sessions.fetch_add(1, Ordering::SeqCst);
        if has_buzz_credentials(&environment) && !self.has_credentials.swap(true, Ordering::SeqCst)
        {
            self.stats
                .credential_connections
                .fetch_add(1, Ordering::SeqCst);
        }
        drop(finalization);
        Ok(NewSessionResponse::new(session_id)
            .modes(acp_session_modes())
            .config_options(acp_config_options(&model, &available_models)))
    }

    async fn retire_session(&self, session_id: &str) {
        let session = self.sessions.lock().await.remove(session_id);
        if let Some(operation) = self.session_operations.lock().await.remove(session_id) {
            operation.cancellation.cancel();
        }
        if let Some(mut session) = session {
            self.stats.active_sessions.fetch_sub(1, Ordering::SeqCst);
            if let Some(lease) = session.lease.take() {
                lease.release().await;
            }
        }
    }

    async fn available_models(&self) -> Result<Vec<String>, agent_client_protocol::Error> {
        tokio::select! {
            biased;
            _ = self.lifetime.cancelled() => Err(
                agent_client_protocol::Error::internal_error()
                    .data("The Maple ACP connection closed while loading models")
            ),
            result = self.agent.available_model_ids() => result.map_err(internal_acp_error),
            _ = tokio::time::sleep(std::time::Duration::from_secs(30)) => Err(
                agent_client_protocol::Error::internal_error()
                    .data("Maple model discovery timed out")
            ),
        }
    }

    async fn remove_session_operation_if_same(
        &self,
        session_id: &str,
        operation: &Arc<AcpSessionOperation>,
    ) {
        let mut operations = self.session_operations.lock().await;
        if operations
            .get(session_id)
            .is_some_and(|registered| Arc::ptr_eq(registered, operation))
        {
            operations.remove(session_id);
        }
    }

    async fn load_session(
        &self,
        cx: &ConnectionTo<Client>,
        request: LoadSessionRequest,
    ) -> Result<LoadSessionResponse, agent_client_protocol::Error> {
        if self.closed.load(Ordering::SeqCst) {
            return Err(agent_client_protocol::Error::internal_error()
                .data("The Maple ACP connection is closing"));
        }
        if !request.cwd.is_absolute() || !request.additional_directories.is_empty() {
            return Err(agent_client_protocol::Error::invalid_params()
                .data("Maple ACP load requires one absolute cwd and no additional directories"));
        }
        let config = self.config.read().await.clone();
        let project_root = ensure_allowed_project_root(&request.cwd, &config.allowed_project_roots)
            .map_err(|error| agent_client_protocol::Error::invalid_params().data(error))?;
        let project_root_text = project_root.to_string_lossy().into_owned();
        let project_trust = self
            .agent
            .get_project_trust(project_root_text.clone())
            .await
            .map_err(internal_acp_error)?;
        let session_id = canonical_session_id(&request.session_id)?;
        let persisted_sessions = self
            .agent
            .list_sessions(Some(project_root_text.clone()))
            .await
            .map_err(internal_acp_error)?;
        let persisted = ensure_acp_session_is_loadable(&persisted_sessions, &session_id)
            .map_err(|error| agent_client_protocol::Error::invalid_request().data(error))?;
        let persisted_model = persisted.model.clone();
        let available_models = self.available_models().await?;
        if let Some(model) = persisted_model.as_ref()
            && !available_models.iter().any(|available| available == model)
        {
            return Err(agent_client_protocol::Error::invalid_request().data(format!(
                    "This Maple Agent task uses model '{model}', which is no longer available; the task remains available in Maple Desktop"
                )));
        }
        let bridge_environment = self.bridge_environment.lock().await.clone();
        let (environment, transient_mcp_servers) =
            prepare_session_mcp(&bridge_environment, &request.mcp_servers)?;
        let tool_context = bridge_tool_context_spec(&environment).map_err(internal_acp_error)?;
        let protocol_session_id = SessionId::new(session_id.clone());
        let operation = AcpSessionOperation::new(&self.lifetime);
        let operation_guard = Arc::clone(&operation.gate).lock_owned().await;
        {
            // Register the operation before the fallible core attach. Close can
            // now mark it closing and wait, while disconnect linearizes through
            // the same finalization barrier used by session creation.
            let _finalization = self.finalization.lock().await;
            if self.closed.load(Ordering::SeqCst) {
                return Err(agent_client_protocol::Error::internal_error()
                    .data("The Maple ACP connection is closing"));
            }
            if self.closing_sessions.lock().await.contains(&session_id) {
                return Err(agent_client_protocol::Error::invalid_request()
                    .data("This ACP session is closing"));
            }
            if self.sessions.lock().await.contains_key(&session_id) {
                return Err(agent_client_protocol::Error::invalid_request()
                    .data("This ACP connection already owns the requested session"));
            }
            let mut operations = self.session_operations.lock().await;
            if operations.contains_key(&session_id) {
                return Err(agent_client_protocol::Error::invalid_request()
                    .data("This ACP connection is already loading the requested session"));
            }
            operations.insert(session_id.clone(), Arc::clone(&operation));
        }
        let attached = match self
            .agent
            .attach_session_with_surface_context(
                session_id.clone(),
                project_root_text,
                tool_context,
                transient_mcp_servers,
                operation.cancellation.child_token(),
            )
            .await
        {
            Ok(attached) => attached,
            Err(error) => {
                self.remove_session_operation_if_same(&session_id, &operation)
                    .await;
                return Err(internal_acp_error(error));
            }
        };
        let lease = attached
            .tool_context_lease
            .expect("an attached ACP task must return a tool-context lease");
        let Some(model) = attached
            .detail
            .session
            .model
            .clone()
            .or_else(|| available_models.first().cloned())
        else {
            lease.release().await;
            self.remove_session_operation_if_same(&session_id, &operation)
                .await;
            return Err(internal_acp_error("Maple returned no models".to_string()));
        };
        let timeline = attached.detail.timeline;
        let message_count = attached.detail.session.message_count;
        let finalization = self.finalization.lock().await;
        if self.closed.load(Ordering::SeqCst)
            || self.closing_sessions.lock().await.contains(&session_id)
        {
            drop(finalization);
            lease.release().await;
            self.remove_session_operation_if_same(&session_id, &operation)
                .await;
            return Err(agent_client_protocol::Error::internal_error()
                .data("The Maple ACP session closed while it was loading"));
        }
        let mut sessions = self.sessions.lock().await;
        if sessions.contains_key(&session_id) {
            drop(sessions);
            drop(finalization);
            lease.release().await;
            self.remove_session_operation_if_same(&session_id, &operation)
                .await;
            return Err(agent_client_protocol::Error::invalid_request()
                .data("This ACP connection duplicated the requested session"));
        }
        sessions.insert(
            session_id.clone(),
            AcpSession {
                lease: Some(lease),
                model: model.clone(),
                available_models: available_models.clone(),
                message_count,
                created_here: false,
                prompted: false,
                project_root,
                project_trust_decision: project_trust.decision,
            },
        );
        drop(sessions);
        self.stats.active_sessions.fetch_add(1, Ordering::SeqCst);
        drop(finalization);

        let mut projection = AcpToolProjection::default();
        for item in &timeline {
            if let Some(update) = timeline_update(item, &mut projection, true)
                && let Err(error) = self
                    .send_session_update(
                        cx,
                        SessionNotification::new(protocol_session_id.clone(), update),
                        &operation.cancellation,
                    )
                    .await
            {
                self.retire_session(&session_id).await;
                return Err(outbound_error(error));
            }
        }
        drop(operation_guard);
        Ok(LoadSessionResponse::new()
            .modes(acp_session_modes())
            .config_options(acp_session_config_options(
                &model,
                &available_models,
                message_count,
            )))
    }

    async fn list_sessions(
        &self,
        request: ListSessionsRequest,
    ) -> Result<ListSessionsResponse, agent_client_protocol::Error> {
        let config = self.config.read().await.clone();
        let project_root = match request.cwd.as_deref() {
            Some(cwd) => {
                if !cwd.is_absolute() {
                    return Err(agent_client_protocol::Error::invalid_params()
                        .data("ACP session-list cwd must be an absolute path"));
                }
                Some(
                    ensure_allowed_project_root(cwd, &config.allowed_project_roots).map_err(
                        |error| agent_client_protocol::Error::invalid_params().data(error),
                    )?,
                )
            }
            None => None,
        };
        let sessions = self
            .agent
            .list_sessions(
                project_root
                    .as_ref()
                    .map(|root| root.to_string_lossy().into_owned()),
            )
            .await
            .map_err(internal_acp_error)?;
        let visible = sessions
            .into_iter()
            .filter(|session| {
                is_acp_loadable_session_mode(&session.mode)
                    && ensure_allowed_project_root(
                        Path::new(&session.project_root),
                        &config.allowed_project_roots,
                    )
                    .is_ok()
            })
            .collect::<Vec<_>>();
        let start = request
            .cursor
            .as_deref()
            .map(str::parse::<usize>)
            .transpose()
            .map_err(|_| {
                agent_client_protocol::Error::invalid_params()
                    .data("Invalid Maple ACP session-list cursor")
            })?
            .unwrap_or(0);
        if start > visible.len() {
            return Err(agent_client_protocol::Error::invalid_params()
                .data("Maple ACP session-list cursor is out of range"));
        }
        let end = start.saturating_add(100).min(visible.len());
        let listed = visible[start..end]
            .iter()
            .map(|session| {
                let mut info =
                    SessionInfo::new(session.id.clone(), PathBuf::from(&session.project_root))
                        .title(session.title.clone());
                if let Some(updated_at) =
                    chrono::DateTime::from_timestamp_millis(session.updated_ms)
                {
                    info = info.updated_at(updated_at.to_rfc3339());
                }
                info
            })
            .collect();
        let mut response = ListSessionsResponse::new(listed);
        if end < visible.len() {
            response = response.next_cursor(end.to_string());
        }
        Ok(response)
    }

    async fn close_session(
        &self,
        request: CloseSessionRequest,
    ) -> Result<CloseSessionResponse, agent_client_protocol::Error> {
        let session_id = canonical_session_id(&request.session_id)?;
        let operation = self
            .session_operations
            .lock()
            .await
            .get(&session_id)
            .cloned()
            .ok_or_else(|| {
                agent_client_protocol::Error::resource_not_found(Some(session_id.clone()))
                    .data("ACP session is not owned by this connection")
            })?;
        self.closing_sessions
            .lock()
            .await
            .insert(session_id.clone());
        operation.cancellation.cancel();
        // Use one absolute deadline for every potentially blocking close
        // phase. Paseo awaits this response before terminating the ACP child,
        // so a fresh timeout per phase could still hang it for multiples of
        // the advertised close bound.
        let close_deadline = tokio::time::Instant::now() + ACP_SESSION_CLOSE_TIMEOUT;
        let cancel_result =
            tokio::time::timeout_at(close_deadline, self.cancel_session(&session_id)).await;
        let cancellation_completed = matches!(&cancel_result, Ok(Ok(())));
        // Starting and running prompts normally retain this guard through their
        // terminal barrier. A broken provider must not make ACP close hang
        // forever, though: after the bound, revoke the lease synchronously and
        // let its Drop retry exact-match cleanup while Paseo can terminate the
        // child process.
        let operation_guard =
            tokio::time::timeout_at(close_deadline, Arc::clone(&operation.gate).lock_owned())
                .await
                .ok();
        let operation_drained = operation_guard.is_some();
        let Some(mut session) = self.sessions.lock().await.remove(&session_id) else {
            if close_registration_may_be_released(cancellation_completed, operation_drained, true) {
                self.remove_session_operation_if_same(&session_id, &operation)
                    .await;
                self.closing_sessions.lock().await.remove(&session_id);
            }
            if let Ok(cancel_result) = cancel_result {
                cancel_result?;
            }
            return Ok(CloseSessionResponse::new());
        };
        self.stats.active_sessions.fetch_sub(1, Ordering::SeqCst);
        let discard = operation_drained && session.created_here && !session.prompted;
        let cleanup_completed = if let Some(lease) = session.lease.take() {
            if operation_drained {
                tokio::time::timeout_at(close_deadline, async move {
                    if discard {
                        lease.discard_created_if_untouched().await;
                    } else {
                        lease.release().await;
                    }
                })
                .await
                .is_ok()
            } else {
                lease.revoke();
                drop(lease);
                false
            }
        } else {
            operation_drained
        };
        if close_registration_may_be_released(
            cancellation_completed,
            operation_drained,
            cleanup_completed,
        ) {
            self.remove_session_operation_if_same(&session_id, &operation)
                .await;
            self.closing_sessions.lock().await.remove(&session_id);
        }
        if let Ok(cancel_result) = cancel_result {
            cancel_result?;
        }
        Ok(CloseSessionResponse::new())
    }

    async fn set_config_option(
        &self,
        request: SetSessionConfigOptionRequest,
    ) -> Result<SetSessionConfigOptionResponse, agent_client_protocol::Error> {
        let session_id = canonical_session_id(&request.session_id)?;
        let operation = self
            .session_operations
            .lock()
            .await
            .get(&session_id)
            .cloned()
            .ok_or_else(|| {
                agent_client_protocol::Error::resource_not_found(Some(session_id.clone()))
                    .data("ACP session is not owned by this connection")
            })?;
        let _operation_guard = Arc::clone(&operation.gate).lock_owned().await;
        if self.closed.load(Ordering::SeqCst)
            || self.closing_sessions.lock().await.contains(&session_id)
        {
            return Err(
                agent_client_protocol::Error::resource_not_found(Some(session_id.clone()))
                    .data("ACP session is closing"),
            );
        }
        let mut sessions = self.sessions.lock().await;
        let session = sessions.get_mut(&session_id).ok_or_else(|| {
            agent_client_protocol::Error::resource_not_found(Some(session_id.clone()))
                .data("ACP session is not owned by this connection")
        })?;
        let selected_value = request.value.as_value_id().ok_or_else(|| {
            agent_client_protocol::Error::invalid_params()
                .data("Maple ACP configuration options require a select value")
        })?;
        match request.config_id.0.as_ref() {
            "model" => {
                let model = selected_value.0.as_ref();
                if !session
                    .available_models
                    .iter()
                    .any(|candidate| candidate == model)
                {
                    return Err(
                        agent_client_protocol::Error::invalid_params().data("Unknown Maple model")
                    );
                }
                if session.message_count > 0 && session.model != model {
                    return Err(agent_client_protocol::Error::invalid_params()
                        .data("Maple tasks are model-locked after their first message"));
                }
                session.model = model.to_string();
            }
            "mode" if selected_value.0.as_ref() == "interactive" => {}
            "mode" => {
                return Err(agent_client_protocol::Error::invalid_params()
                    .data("Maple ACP supports only caller-mediated interactive mode"));
            }
            _ => {
                return Err(agent_client_protocol::Error::invalid_params()
                    .data("Unknown Maple ACP configuration option"));
            }
        }
        Ok(SetSessionConfigOptionResponse::new(
            session.config_options(),
        ))
    }

    async fn set_mode(
        &self,
        request: SetSessionModeRequest,
    ) -> Result<SetSessionModeResponse, agent_client_protocol::Error> {
        let session_id = canonical_session_id(&request.session_id)?;
        let operation = self
            .session_operations
            .lock()
            .await
            .get(&session_id)
            .cloned()
            .ok_or_else(|| {
                agent_client_protocol::Error::resource_not_found(Some(session_id.clone()))
                    .data("ACP session is not owned by this connection")
            })?;
        let _operation_guard = Arc::clone(&operation.gate).lock_owned().await;
        if self.closed.load(Ordering::SeqCst)
            || self.closing_sessions.lock().await.contains(&session_id)
            || !self.sessions.lock().await.contains_key(&session_id)
        {
            return Err(
                agent_client_protocol::Error::resource_not_found(Some(session_id))
                    .data("ACP session is not available on this connection"),
            );
        }
        if request.mode_id.0.as_ref() != "interactive" {
            return Err(agent_client_protocol::Error::invalid_params()
                .data("Maple ACP supports only caller-mediated interactive mode"));
        }
        Ok(SetSessionModeResponse::new())
    }

    async fn begin_prompt(
        &self,
        request: &PromptRequest,
    ) -> Result<
        (
            String,
            String,
            CancellationToken,
            tokio::sync::OwnedMutexGuard<()>,
        ),
        agent_client_protocol::Error,
    > {
        if self.closed.load(Ordering::SeqCst) {
            return Err(agent_client_protocol::Error::internal_error()
                .data("The Maple ACP connection is closing"));
        }
        let session_id = canonical_session_id(&request.session_id)?;
        let prompt = prompt_text(&request.prompt)?;
        let operation = self
            .session_operations
            .lock()
            .await
            .get(&session_id)
            .cloned()
            .ok_or_else(|| {
                agent_client_protocol::Error::resource_not_found(Some(session_id.clone()))
                    .data("ACP session is not owned by this connection")
            })?;
        let operation_guard = Arc::clone(&operation.gate).lock_owned().await;
        if self.closed.load(Ordering::SeqCst)
            || self.closing_sessions.lock().await.contains(&session_id)
            || !self.sessions.lock().await.contains_key(&session_id)
        {
            return Err(
                agent_client_protocol::Error::resource_not_found(Some(session_id.clone()))
                    .data("ACP session is not available on this connection"),
            );
        }
        let mut states = self.prompt_states.lock().await;
        if states.contains_key(&session_id) {
            return Err(agent_client_protocol::Error::invalid_request()
                .data("This ACP session already has an active prompt"));
        }
        let cancellation = operation.cancellation.child_token();
        states.insert(
            session_id.clone(),
            AcpPromptState::Starting {
                cancellation: cancellation.clone(),
            },
        );
        Ok((prompt, session_id, cancellation, operation_guard))
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
        let message_id = format!(
            "maple-acp-notice-{}",
            NEXT_ACP_MESSAGE_ID.fetch_add(1, Ordering::Relaxed)
        );
        self.send_session_update(
            cx,
            SessionNotification::new(
                session_id,
                SessionUpdate::AgentMessageChunk(
                    ContentChunk::new(ContentBlock::Text(TextContent::new(message.to_string())))
                        .message_id(message_id.as_str()),
                ),
            ),
            cancellation,
        )
        .await
    }

    async fn request_project_trust_with_elicitation(
        &self,
        cx: &ConnectionTo<Client>,
        session_id: SessionId,
        project_root: &Path,
        cancellation: &CancellationToken,
    ) -> Result<Option<bool>, AcpOutboundSendError> {
        let request = project_trust_elicitation_request(session_id, project_root);
        let encoded_bytes = serde_json::to_vec(&request)
            .map_err(|error| {
                AcpOutboundSendError::Transport(internal_acp_error(format!(
                    "Failed to encode Maple ACP project trust elicitation: {error}"
                )))
            })?
            .len();
        let reservation = self.outbound.reserve(encoded_bytes, cancellation).await?;
        if cancellation.is_cancelled() {
            return Ok(None);
        }
        let sent_request = cx.send_request(request);
        let mut response_future = Box::pin(sent_request.block_task());
        let response = tokio::select! {
            biased;
            _ = cancellation.cancelled() => None,
            response = &mut response_future => Some(response),
        };
        let Some(response) = response else {
            retain_cancelled_permission_request(response_future, reservation);
            return Ok(None);
        };
        drop(reservation);
        let response = response.map_err(AcpOutboundSendError::Transport)?;
        match response.action {
            ElicitationAction::Accept(action) => {
                let trusted = action
                    .content
                    .as_ref()
                    .and_then(|content| content.get("trustProject"))
                    .and_then(|value| match value {
                        ElicitationContentValue::Boolean(value) => Some(*value),
                        _ => None,
                    })
                    .ok_or_else(|| {
                        AcpOutboundSendError::Transport(internal_acp_error(
                            "ACP client returned an invalid project trust elicitation response"
                                .to_string(),
                        ))
                    })?;
                Ok(Some(trusted))
            }
            ElicitationAction::Decline => Ok(Some(false)),
            ElicitationAction::Cancel => Ok(None),
            _ => Ok(None),
        }
    }

    async fn request_project_trust_with_chooser(
        &self,
        cx: &ConnectionTo<Client>,
        session_id: SessionId,
        project_root: &Path,
        cancellation: &CancellationToken,
    ) -> Result<Option<bool>, AcpOutboundSendError> {
        let request_id = format!(
            "maple-project-trust-{}",
            NEXT_ACP_MESSAGE_ID.fetch_add(1, Ordering::Relaxed)
        );
        let tool_call = ToolCall::new(request_id, "Trust this project?")
            .kind(ToolKind::Other)
            .status(ToolCallStatus::Pending)
            .content(vec![ToolCallContent::from(ContentBlock::Text(
                TextContent::new(format!(
                    "Trusting '{}' allows Maple to use project-provided guidance, including agent skills. These instructions can influence how agents work and use tools. Normal tool permissions still apply.",
                    project_root.display()
                )),
            ))]);
        let request = RequestPermissionRequest::new(
            session_id,
            tool_call.into(),
            project_trust_permission_options(),
        );
        let encoded_bytes = serde_json::to_vec(&request)
            .map_err(|error| {
                AcpOutboundSendError::Transport(internal_acp_error(format!(
                    "Failed to encode Maple ACP project trust request: {error}"
                )))
            })?
            .len();
        let reservation = self.outbound.reserve(encoded_bytes, cancellation).await?;
        if cancellation.is_cancelled() {
            return Ok(None);
        }
        let sent_request = cx.send_request(request);
        let mut response_future = Box::pin(sent_request.block_task());
        let response = tokio::select! {
            biased;
            _ = cancellation.cancelled() => None,
            response = &mut response_future => Some(response),
        };
        let Some(response) = response else {
            retain_cancelled_permission_request(response_future, reservation);
            return Ok(None);
        };
        drop(reservation);
        let response = response.map_err(AcpOutboundSendError::Transport)?;
        project_trust_permission_decision(&response.outcome)
            .map_err(|error| AcpOutboundSendError::Transport(internal_acp_error(error)))
    }

    async fn resolve_project_trust_before_prompt(
        &self,
        cx: &ConnectionTo<Client>,
        session_id: &str,
        cancellation: &CancellationToken,
    ) -> Result<AcpProjectTrustResolution, agent_client_protocol::Error> {
        let (project_root, configured_decision, tool_context_access) = {
            let sessions = self.sessions.lock().await;
            let session = sessions.get(session_id).ok_or_else(|| {
                agent_client_protocol::Error::resource_not_found(Some(session_id.to_string()))
                    .data("ACP session is no longer owned by this connection")
            })?;
            (
                session.project_root.clone(),
                session.project_trust_decision,
                session
                    .lease
                    .as_ref()
                    .expect("a runnable ACP session must own a lease")
                    .access(),
            )
        };
        let mut status = self
            .agent
            .get_project_trust(project_root.to_string_lossy().into_owned())
            .await
            .map_err(internal_acp_error)?;
        if status.decision.is_none() && !status.protected_features.is_empty() {
            let protocol_session_id = SessionId::new(session_id.to_string());
            let decision = if self.client_supports_form_elicitation.load(Ordering::SeqCst) {
                match self
                    .request_project_trust_with_elicitation(
                        cx,
                        protocol_session_id.clone(),
                        &project_root,
                        cancellation,
                    )
                    .await
                {
                    Ok(decision) => decision,
                    Err(error) if !cancellation.is_cancelled() => {
                        log::warn!(
                            "ACP client advertised form elicitation but project trust elicitation failed; using the permission chooser fallback: {error:?}"
                        );
                        self.request_project_trust_with_chooser(
                            cx,
                            protocol_session_id,
                            &project_root,
                            cancellation,
                        )
                        .await
                        .map_err(outbound_error)?
                    }
                    Err(error) => return Err(outbound_error(error)),
                }
            } else {
                self.request_project_trust_with_chooser(
                    cx,
                    protocol_session_id,
                    &project_root,
                    cancellation,
                )
                .await
                .map_err(outbound_error)?
            };
            let Some(trusted) = decision else {
                return Ok(AcpProjectTrustResolution::Cancelled);
            };
            status = self
                .agent
                .set_project_trust_for_surface_session(
                    project_root.to_string_lossy().into_owned(),
                    trusted,
                    tool_context_access.clone(),
                )
                .await
                .map_err(internal_acp_error)?;
        } else if status.decision != configured_decision {
            let trusted = status.decision.unwrap_or(false);
            status = self
                .agent
                .set_project_trust_for_surface_session(
                    project_root.to_string_lossy().into_owned(),
                    trusted,
                    tool_context_access,
                )
                .await
                .map_err(internal_acp_error)?;
        }
        if let Some(session) = self.sessions.lock().await.get_mut(session_id) {
            session.project_trust_decision = status.decision;
        }
        Ok(AcpProjectTrustResolution::Continue)
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
        session_id: String,
        prompt: String,
        prompt_lifetime: CancellationToken,
        operation_guard: tokio::sync::OwnedMutexGuard<()>,
    ) -> Result<PromptResponse, agent_client_protocol::Error> {
        let mut operation_guard = Some(operation_guard);
        let protocol_session_id = SessionId::new(session_id.clone());
        let config = self.config.read().await.clone();
        match self
            .resolve_project_trust_before_prompt(cx, &session_id, &prompt_lifetime)
            .await
        {
            Ok(AcpProjectTrustResolution::Continue) => {}
            Ok(AcpProjectTrustResolution::Cancelled) => {
                self.prompt_states.lock().await.remove(&session_id);
                return Ok(PromptResponse::new(StopReason::Cancelled));
            }
            Err(error) => {
                self.prompt_states.lock().await.remove(&session_id);
                return Err(error);
            }
        }
        let (tool_context_access, model) = {
            let sessions = self.sessions.lock().await;
            match sessions.get(&session_id) {
                Some(session) => (
                    session
                        .lease
                        .as_ref()
                        .expect("a runnable ACP session must own a lease")
                        .access(),
                    session.model.clone(),
                ),
                None => {
                    drop(sessions);
                    self.prompt_states.lock().await.remove(&session_id);
                    return Err(agent_client_protocol::Error::resource_not_found(Some(
                        session_id.clone(),
                    ))
                    .data("ACP session is no longer owned by this connection"));
                }
            }
        };
        let run = match self
            .agent
            .send_message_with_tool_context(
                AgentSendMessageRequest {
                    session_id: session_id.clone(),
                    text: prompt,
                    model: Some(model),
                    context_limit: None,
                    mode: Some(config.permission_mode.maple_mode().to_string()),
                    vision_capable: false,
                    steer: false,
                    queue_id: None,
                    attachments: Vec::new(),
                },
                tool_context_access,
                prompt_lifetime.clone(),
                AgentHostEventPolicy::Suppress,
            )
            .await
        {
            Ok(run) => run,
            Err(_) if prompt_lifetime.is_cancelled() => {
                self.prompt_states.lock().await.remove(&session_id);
                return Ok(PromptResponse::new(StopReason::Cancelled));
            }
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
        let locked_config_options = {
            let mut sessions = self.sessions.lock().await;
            sessions.get_mut(&session_id).and_then(|session| {
                session.prompted = true;
                let first_message = session.message_count == 0;
                session.message_count = session.message_count.saturating_add(1);
                first_message.then(|| session.config_options())
            })
        };
        let mut events = run.events;
        let mut terminal = run.terminal;
        let usage = run.usage;
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

        if let Some(config_options) = locked_config_options {
            match self
                .send_session_update(
                    cx,
                    SessionNotification::new(
                        protocol_session_id.clone(),
                        SessionUpdate::ConfigOptionUpdate(ConfigOptionUpdate::new(config_options)),
                    ),
                    &prompt_lifetime,
                )
                .await
            {
                Ok(()) => {}
                Err(AcpOutboundSendError::Cancelled) => {
                    let _ = run_cancellation.cancel().await;
                    if matches!(
                        self.prompt_states.lock().await.remove(&session_id),
                        Some(AcpPromptState::Running { .. })
                    ) {
                        self.stats.active_runs.fetch_sub(1, Ordering::SeqCst);
                    }
                    return Ok(PromptResponse::new(StopReason::Cancelled));
                }
                Err(AcpOutboundSendError::UpdateTooLarge) => {
                    let _ = run_cancellation.cancel().await;
                    if matches!(
                        self.prompt_states.lock().await.remove(&session_id),
                        Some(AcpPromptState::Running { .. })
                    ) {
                        self.stats.active_runs.fetch_sub(1, Ordering::SeqCst);
                    }
                    return Err(agent_client_protocol::Error::internal_error()
                        .data("Maple's locked model selector exceeded the ACP update limit"));
                }
                Err(AcpOutboundSendError::Transport(error)) => {
                    let _ = run_cancellation.cancel().await;
                    if matches!(
                        self.prompt_states.lock().await.remove(&session_id),
                        Some(AcpPromptState::Running { .. })
                    ) {
                        self.stats.active_runs.fetch_sub(1, Ordering::SeqCst);
                    }
                    return Err(error);
                }
            }
        }

        let mut cancel_after_result = false;
        let mut tool_projection = AcpToolProjection::default();
        let result = loop {
            if event_overflowed.load(Ordering::Acquire) {
                cancel_after_result = true;
                let _ = run_cancellation.cancel().await;
                match self
                    .send_final_agent_message(
                        cx,
                        protocol_session_id.clone(),
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
                        protocol_session_id.clone(),
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
                    if let Some(update) = timeline_update(&item, &mut tool_projection, false) {
                        match self
                            .send_session_update(
                                cx,
                                SessionNotification::new(protocol_session_id.clone(), update),
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
                                    protocol_session_id.clone(),
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
                            protocol_session_id.clone(),
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
                                    protocol_session_id.clone(),
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
                                    protocol_session_id.clone(),
                                    SessionUpdate::AgentMessageChunk(
                                        ContentChunk::new(ContentBlock::Text(TextContent::new(
                                            message,
                                        )))
                                        .message_id(item.id.as_str()),
                                    ),
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
                                    protocol_session_id.clone(),
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
                Some(AgentRunEvent::HistoryReplaced) => {
                    // The client keeps its own transcript and cannot reload
                    // ours, so only say that the compaction finished.
                    match self
                        .send_final_agent_message(
                            cx,
                            protocol_session_id.clone(),
                            COMPACTION_COMPLETED_NOTICE,
                            &prompt_lifetime,
                        )
                        .await
                    {
                        Ok(()) | Err(AcpOutboundSendError::UpdateTooLarge) => {}
                        Err(AcpOutboundSendError::Cancelled) => {
                            cancel_after_result = true;
                            break Ok(PromptResponse::new(StopReason::Cancelled));
                        }
                        Err(AcpOutboundSendError::Transport(error)) => break Err(error),
                    }
                }
                Some(
                    AgentRunEvent::SessionUpdated(_)
                    | AgentRunEvent::Started
                    | AgentRunEvent::SetupWarning(_)
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
        let turn_usage = usage.borrow().as_ref().copied().unwrap_or_default();
        // ACP defines PromptResponse.usage as usage for this prompt turn. Paseo
        // stores it as currentTurnUsage, so cumulative session totals would be
        // double-counted on every later turn.
        let result = result.map(|response| response.usage(acp_usage(turn_usage)));
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
                let draining_operation_guard = operation_guard
                    .take()
                    .expect("a deferred ACP prompt must retain its session operation fence");
                let mut tasks = self.background_tasks.lock().await;
                tasks.spawn(async move {
                    let _operation_guard = draining_operation_guard;
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
        drop(operation_guard);
        result
    }

    async fn cancel(
        &self,
        notification: CancelNotification,
    ) -> Result<(), agent_client_protocol::Error> {
        let session_id = canonical_session_id(&notification.session_id)?;
        self.cancel_session(&session_id).await
    }

    async fn cancel_session(&self, session_id: &str) -> Result<(), agent_client_protocol::Error> {
        let (cancellation, run_cancellation) = {
            let states = self.prompt_states.lock().await;
            match states.get(session_id) {
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
        // Revoke every capability synchronously before awaiting registry cleanup.
        // No queued or detached task can launch another credential-bearing tool
        // after this barrier returns.
        let session_ids = {
            let sessions = self.sessions.lock().await;
            for session in sessions.values() {
                if let Some(lease) = session.lease.as_ref() {
                    lease.revoke();
                }
            }
            sessions.keys().cloned().collect::<Vec<_>>()
        };
        let mut retired_sessions = Vec::with_capacity(session_ids.len());
        for session_id in session_ids {
            let operation = self
                .session_operations
                .lock()
                .await
                .get(&session_id)
                .cloned();
            // A prompt marks the session as prompted while holding this gate.
            // Waiting here closes the admission gap before deciding whether a
            // newly created empty task may be discarded. At the cleanup
            // deadline we preserve the durable row rather than risk deleting
            // work whose admission is still settling.
            let operation_drained = match operation {
                Some(operation) => {
                    tokio::time::timeout_at(deadline, Arc::clone(&operation.gate).lock_owned())
                        .await
                        .is_ok()
                }
                None => true,
            };
            let session = self.sessions.lock().await.remove(&session_id);
            if let Some(session) = session {
                let discard = operation_drained && session.created_here && !session.prompted;
                retired_sessions.push((session_id, session, discard));
                self.stats.active_sessions.fetch_sub(1, Ordering::SeqCst);
            }
        }
        // Take ownership of the current task set before awaiting it. A prompt
        // that is itself in this set may need to publish a retained drain task;
        // leaving an empty shared set lets that path proceed without a mutex
        // self-deadlock. Newly published tasks are collected on the next pass.
        let mut tasks = {
            let mut shared = self.background_tasks.lock().await;
            std::mem::take(&mut *shared)
        };
        for run_cancellation in running_cancellations {
            tasks.spawn(async move {
                let _ = run_cancellation.cancel().await;
            });
        }
        for (_session_id, mut session, discard) in retired_sessions {
            tasks.spawn(async move {
                if let Some(lease) = session.lease.take() {
                    if discard {
                        lease.discard_created_if_untouched().await;
                    } else {
                        lease.release().await;
                    }
                }
            });
        }
        'drain: loop {
            match tokio::time::timeout_at(deadline, tasks.join_next()).await {
                Ok(Some(_)) => {}
                Ok(None) => {
                    let mut shared = self.background_tasks.lock().await;
                    if shared.is_empty() {
                        break 'drain;
                    }
                    tasks = std::mem::take(&mut *shared);
                }
                Err(_) => {
                    // Session creation and the pre-run prompt path both cross
                    // persistent core state before returning an ID. Aborting
                    // them here could orphan that state. Detaching preserves
                    // their existing closed checks and rollback/cancel paths
                    // while keeping connection shutdown bounded.
                    tasks.detach_all();
                    self.background_tasks.lock().await.detach_all();
                    break 'drain;
                }
            }
        }
        self.session_operations.lock().await.clear();
        self.closing_sessions.lock().await.clear();
    }
}

#[derive(Clone)]
struct MapleAcpHandler {
    context: Arc<AcpConnectionContext>,
}

/// `session/new` fields that Buzz adds beyond the ACP 1 schema: a top-level
/// `systemPrompt` (its protocol 2 system role) and `_meta.sessionTitle`.
/// They are read from the raw request before typed dispatch drops them.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
struct AcpCallerSessionFields {
    system_prompt: Option<String>,
    session_title: Option<String>,
}

impl AcpCallerSessionFields {
    fn from_dispatch(message: &Dispatch) -> Self {
        let Dispatch::Request(request, _) = message else {
            return Self::default();
        };
        if request.method() != "session/new" {
            return Self::default();
        }
        Self::from_params(request.params())
    }

    fn from_params(params: &serde_json::Value) -> Self {
        let text = |value: Option<&serde_json::Value>| {
            value
                .and_then(serde_json::Value::as_str)
                .map(str::trim)
                .filter(|text| !text.is_empty())
                .map(str::to_owned)
        };
        Self {
            system_prompt: text(params.get("systemPrompt")),
            session_title: text(params.pointer("/_meta/sessionTitle")),
        }
    }
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
        let caller_fields = AcpCallerSessionFields::from_dispatch(&message);
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
                .if_request({
                    let context = Arc::clone(&context);
                    move |request: InitializeRequest, responder: Responder<InitializeResponse>| async move {
                        context.client_supports_form_elicitation.store(
                            client_supports_form_elicitation(&request),
                            Ordering::SeqCst,
                        );
                        let capabilities = AgentCapabilities::new()
                            .load_session(true)
                            .prompt_capabilities(
                                PromptCapabilities::new()
                                    .image(false)
                                    .audio(false)
                                    .embedded_context(false),
                            )
                            .mcp_capabilities(McpCapabilities::new().http(true))
                            .session_capabilities(
                                SessionCapabilities::new()
                                    .list(SessionListCapabilities::new())
                                    .close(SessionCloseCapabilities::new()),
                            );
                        responder.respond(
                            // Protocol 2 tells Buzz that `session/new`
                            // accepts its `systemPrompt` field. The schema
                            // crate keeps `V2` behind an unstable feature,
                            // so build the value from the number.
                            InitializeResponse::new(
                                agent_client_protocol::schema::ProtocolVersion::from(2u16),
                            )
                            .agent_info(Implementation::new("maple", env!("CARGO_PKG_VERSION")))
                            .agent_capabilities(capabilities),
                        )
                    }
                })
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
                            let _ = responder.respond_with_result(
                                task_context.new_session(request, caller_fields).await,
                            );
                        });
                        Ok(())
                    }
                })
                .await
                .if_request({
                    let context = Arc::clone(&context);
                    let cx = cx.clone();
                    |request: LoadSessionRequest, responder: Responder<LoadSessionResponse>| async move {
                        let task_context = Arc::clone(&context);
                        let task_cx = cx.clone();
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
                            let _ = responder.respond_with_result(
                                task_context.load_session(&task_cx, request).await,
                            );
                        });
                        Ok(())
                    }
                })
                .await
                .if_request({
                    let context = Arc::clone(&context);
                    |request: ListSessionsRequest, responder: Responder<ListSessionsResponse>| async move {
                        responder.respond_with_result(context.list_sessions(request).await)
                    }
                })
                .await
                .if_request({
                    let context = Arc::clone(&context);
                    |request: CloseSessionRequest, responder: Responder<CloseSessionResponse>| async move {
                        responder.respond_with_result(context.close_session(request).await)
                    }
                })
                .await
                .if_request({
                    let context = Arc::clone(&context);
                    |request: SetSessionConfigOptionRequest, responder: Responder<SetSessionConfigOptionResponse>| async move {
                        responder.respond_with_result(context.set_config_option(request).await)
                    }
                })
                .await
                .if_request({
                    let context = Arc::clone(&context);
                    |request: SetSessionModeRequest, responder: Responder<SetSessionModeResponse>| async move {
                        responder.respond_with_result(context.set_mode(request).await)
                    }
                })
                .await
                .if_request({
                    let context = Arc::clone(&context);
                    let cx = cx.clone();
                    |request: PromptRequest, responder: Responder<PromptResponse>| async move {
                        let (prompt, session_id, prompt_lifetime, operation_guard) =
                            match context.begin_prompt(&request).await {
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
                            context.prompt_states.lock().await.remove(&session_id);
                            responder.respond_with_error(
                                agent_client_protocol::Error::internal_error()
                                    .data("The Maple ACP connection is closing"),
                            )?;
                            return Ok(());
                        }
                        tasks.spawn(async move {
                            let _ = responder.respond_with_result(
                                prompt_context
                                    .prompt(
                                        &prompt_cx,
                                        session_id,
                                        prompt,
                                        prompt_lifetime,
                                        operation_guard,
                                    )
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
                                router.route_with_result(result)?;
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

fn config_path(local_data_root: &Path, user_id: &str) -> Result<PathBuf, String> {
    let scope = account_scope(user_id)
        .map_err(|_| "Maple ACP configuration requires an authenticated user".to_string())?;
    config_path_for_scope(local_data_root, &scope)
}

fn config_path_for_scope(local_data_root: &Path, scope: &str) -> Result<PathBuf, String> {
    Ok(acp_accounts_root(local_data_root)?
        .join(scope)
        .join("config.json"))
}

fn acp_accounts_root(local_data_root: &Path) -> Result<PathBuf, String> {
    Ok(local_data_root.join("acp").join("accounts"))
}

fn legacy_config_path(local_data_root: &Path, user_id: &str) -> Result<PathBuf, String> {
    if user_id.trim().is_empty() {
        return Err("Maple ACP configuration requires an authenticated user".to_string());
    }
    let digest = Sha256::digest(user_id.as_bytes());
    let legacy_scope = digest[..16]
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect::<String>();
    Ok(acp_accounts_root(local_data_root)?
        .join(legacy_scope)
        .join("config.json"))
}

fn load_config(local_data_root: &Path, user_id: &str) -> Result<AgentAcpConfig, String> {
    let path = config_path(local_data_root, user_id)?;
    match std::fs::read(&path) {
        Ok(bytes) => serde_json::from_slice(&bytes)
            .map_err(|error| format!("Failed to parse Maple ACP configuration: {error}"))
            .and_then(normalize_config),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            let legacy_path = legacy_config_path(local_data_root, user_id)?;
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
                    if let Err(error) = save_config(local_data_root, user_id, &config) {
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
    local_data_root: &Path,
    user_id: &str,
    config: &AgentAcpConfig,
) -> Result<(), String> {
    let scope = account_scope(user_id)
        .map_err(|_| "Maple ACP configuration requires an authenticated user".to_string())?;
    save_config_for_scope(local_data_root, &scope, config)
}

fn save_config_for_scope(
    local_data_root: &Path,
    account_scope: &str,
    config: &AgentAcpConfig,
) -> Result<(), String> {
    let path = config_path_for_scope(local_data_root, account_scope)?;
    let parent = path
        .parent()
        .ok_or_else(|| "Invalid Maple ACP configuration path".to_string())?;
    std::fs::create_dir_all(parent)
        .map_err(|error| format!("Failed to create Maple ACP configuration directory: {error}"))?;
    #[cfg(unix)]
    std::fs::set_permissions(parent, std::fs::Permissions::from_mode(0o700))
        .map_err(|error| format!("Failed to secure Maple ACP configuration directory: {error}"))?;
    // Atomic replace: a crash mid-write must not leave a truncated file
    // that `load_config` rejects, which would block `maple-gpui acp`.
    crate::private_file::write_private_json(&path, config)
        .map_err(|error| format!("Failed to save Maple ACP configuration: {error}"))?;
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
    if !cwd.is_absolute() {
        return Err("ACP session cwd must be an absolute path".to_string());
    }
    let cwd = cwd
        .canonicalize()
        .map_err(|error| format!("Failed to resolve ACP session cwd: {error}"))?;
    if allowed_roots.is_empty() {
        return Ok(cwd);
    }
    for root in allowed_roots {
        if let Ok(root) = Path::new(root).canonicalize()
            && cwd.starts_with(root)
        {
            return Ok(cwd);
        }
    }
    Err("ACP session cwd is outside the configured project roots".to_string())
}

fn prepare_session_mcp(
    bridge_environment: &HashMap<String, String>,
    servers: &[McpServer],
) -> Result<(HashMap<String, String>, Vec<AgentTransientMcpServer>), agent_client_protocol::Error> {
    let mut environment = bridge_environment.clone();
    let mut transient = Vec::new();
    for server in servers {
        match server {
            McpServer::Stdio(server) => {
                let command = server.command.as_path();
                let is_buzz_dev_mcp = server.name == "buzz-dev-mcp"
                    && command
                        .file_name()
                        .and_then(|name| name.to_str())
                        .is_some_and(|name| name == "buzz-dev-mcp" || name == "buzz-dev-mcp.exe")
                    && server.args.is_empty();
                if is_buzz_dev_mcp {
                    if !command.is_absolute() {
                        return Err(agent_client_protocol::Error::invalid_params()
                            .data("Buzz buzz-dev-mcp command must be absolute"));
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
                    // Preserve the historical Buzz adapter: its exact MCP
                    // definition contributes only the allowlisted shell env.
                    for variable in &server.env {
                        if !ALLOWED_BRIDGE_ENV.contains(&variable.name.as_str()) {
                            continue;
                        }
                        if let Some(existing) = environment.get(&variable.name) {
                            if existing != &variable.value {
                                return Err(agent_client_protocol::Error::invalid_params().data(
                                    format!(
                                        "Conflicting ACP environment value for {}",
                                        variable.name
                                    ),
                                ));
                            }
                        } else {
                            environment.insert(variable.name.clone(), variable.value.clone());
                        }
                    }
                    continue;
                }
                return Err(agent_client_protocol::Error::invalid_params().data(format!(
                    "Transient stdio MCP server '{}' is disabled because it would execute caller-supplied native code without a Maple approval boundary",
                    server.name
                )));
            }
            McpServer::Http(server) => {
                let mut headers = Vec::new();
                let mut header_names = HashSet::new();
                for header in &server.headers {
                    if !header_names.insert(header.name.to_ascii_lowercase()) {
                        return Err(agent_client_protocol::Error::invalid_params().data(format!(
                            "Duplicate HTTP header in MCP server '{}'",
                            server.name
                        )));
                    }
                    headers.push(AgentMcpKeyValue {
                        key: header.name.clone(),
                        value: header.value.clone(),
                    });
                }
                transient.push(AgentTransientMcpServer {
                    name: server.name.clone(),
                    description: "ACP session MCP server".to_string(),
                    timeout_seconds: ACP_TRANSIENT_MCP_TIMEOUT_SECONDS,
                    transport: AgentTransientMcpTransport::StreamableHttp {
                        url: server.url.clone(),
                        headers,
                    },
                });
            }
            McpServer::Sse(_) => {
                return Err(agent_client_protocol::Error::invalid_params()
                    .data("Maple ACP does not support legacy SSE MCP servers"));
            }
            _ => {
                return Err(agent_client_protocol::Error::invalid_params()
                    .data("Maple ACP does not support this MCP transport"));
            }
        }
    }
    Ok((environment, transient))
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

fn canonical_session_id(session_id: &SessionId) -> Result<String, agent_client_protocol::Error> {
    canonical_session_id_text(&session_id.0)
}

fn canonical_session_id_text(session_id: &str) -> Result<String, agent_client_protocol::Error> {
    let session_id = session_id.trim();
    if session_id.is_empty() {
        return Err(agent_client_protocol::Error::invalid_params()
            .data("Maple ACP requires a non-empty session ID"));
    }
    Ok(session_id.to_string())
}

fn is_acp_loadable_session_mode(mode: &str) -> bool {
    mode == ACP_LOADABLE_GOOSE_MODE
}

fn ensure_acp_session_is_loadable<'a>(
    sessions: &'a [AgentSessionSummary],
    session_id: &str,
) -> Result<&'a AgentSessionSummary, String> {
    let session = sessions
        .iter()
        .find(|session| session.id == session_id)
        .ok_or_else(|| {
            "The requested Maple Agent task does not exist in the supplied project directory"
                .to_string()
        })?;
    if !is_acp_loadable_session_mode(&session.mode) {
        return Err(
            "Maple ACP can load only Read only Agent tasks; this task remains available in Maple Desktop"
                .to_string(),
        );
    }
    Ok(session)
}

fn acp_session_modes() -> SessionModeState {
    SessionModeState::new(
        "interactive",
        vec![
            SessionMode::new("interactive", "Interactive")
                .description("Maple asks the ACP caller to approve sensitive tools"),
        ],
    )
}

fn acp_config_options(model: &str, available_models: &[String]) -> Vec<SessionConfigOption> {
    let model_options = available_models
        .iter()
        .map(|model| SessionConfigSelectOption::new(model.clone(), model.clone()))
        .collect::<Vec<_>>();
    vec![
        SessionConfigOption::select("model", "Model", model.to_string(), model_options)
            .category(SessionConfigOptionCategory::Model),
        SessionConfigOption::select(
            "mode",
            "Mode",
            "interactive",
            vec![SessionConfigSelectOption::new("interactive", "Interactive")],
        )
        .category(SessionConfigOptionCategory::Mode),
    ]
}

fn acp_session_config_options(
    model: &str,
    available_models: &[String],
    message_count: usize,
) -> Vec<SessionConfigOption> {
    if message_count == 0 {
        acp_config_options(model, available_models)
    } else {
        let locked_models = [model.to_string()];
        acp_config_options(model, &locked_models)
    }
}

fn acp_usage(usage: AgentRunUsage) -> Usage {
    Usage::new(usage.total_tokens, usage.input_tokens, usage.output_tokens)
        .cached_read_tokens(usage.cached_read_tokens)
        .cached_write_tokens(usage.cached_write_tokens)
}

fn outbound_error(error: AcpOutboundSendError) -> agent_client_protocol::Error {
    match error {
        AcpOutboundSendError::Transport(error) => error,
        AcpOutboundSendError::UpdateTooLarge => agent_client_protocol::Error::internal_error()
            .data("A Maple ACP history update exceeded the transport limit"),
        AcpOutboundSendError::Cancelled => agent_client_protocol::Error::internal_error()
            .data("The Maple ACP connection closed while replaying history"),
    }
}

fn prompt_text(blocks: &[ContentBlock]) -> Result<String, agent_client_protocol::Error> {
    let mut parts = Vec::new();
    for block in blocks {
        match block {
            ContentBlock::Text(text) => parts.push(text.text.clone()),
            ContentBlock::ResourceLink(link) => {
                parts.push(format!("[Resource: {}]\n{}", link.name, link.uri));
            }
            _ => {
                return Err(agent_client_protocol::Error::invalid_params()
                    .data("Maple ACP currently accepts text and resource-link prompt blocks"));
            }
        }
    }
    let text = parts.join("\n\n");
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
        "read" | "read_image" => ToolKind::Read,
        "edit" | "write" | "text_editor" => ToolKind::Edit,
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

fn client_supports_form_elicitation(request: &InitializeRequest) -> bool {
    request
        .client_capabilities
        .elicitation
        .as_ref()
        .is_some_and(|capabilities| capabilities.form.is_some())
}

fn project_trust_elicitation_request(
    session_id: SessionId,
    project_root: &Path,
) -> CreateElicitationRequest {
    let schema = ElicitationSchema::new()
        .title("Trust this project?")
        .description(
            "Choose whether Maple may use guidance supplied by this project. The decision is remembered and reversible.",
        )
        .property(
            "trustProject",
            BooleanPropertySchema::new()
                .title("Trust this project")
                .description(
                    "Allow Maple to use project-provided guidance, including agent skills. These instructions can influence how agents work and use tools.",
                )
                .default_value(false),
            true,
        );
    CreateElicitationRequest::new(
        ElicitationFormMode::new(ElicitationSessionScope::new(session_id), schema),
        format!(
            "Trust project '{}'? Normal tool permissions still apply.",
            project_root.display()
        ),
    )
}

fn project_trust_permission_options() -> Vec<PermissionOption> {
    vec![
        // Keep the fail-closed choice first for clients that present a default.
        // Two AllowOnce options deliberately make this a chooser in Paseo, so
        // its generic auto-accept feature cannot silently resolve project trust.
        PermissionOption::new(
            "keep_untrusted",
            "Keep project trust disabled",
            PermissionOptionKind::AllowOnce,
        ),
        PermissionOption::new(
            "trust_project",
            "Trust this project",
            PermissionOptionKind::AllowOnce,
        ),
        PermissionOption::new("cancel", "Cancel turn", PermissionOptionKind::RejectOnce),
    ]
}

fn project_trust_permission_decision(
    outcome: &RequestPermissionOutcome,
) -> Result<Option<bool>, String> {
    match outcome {
        RequestPermissionOutcome::Selected(selected)
            if selected.option_id.0.as_ref() == "keep_untrusted" =>
        {
            Ok(Some(false))
        }
        RequestPermissionOutcome::Selected(selected)
            if selected.option_id.0.as_ref() == "trust_project" =>
        {
            Ok(Some(true))
        }
        RequestPermissionOutcome::Cancelled => Ok(None),
        RequestPermissionOutcome::Selected(selected)
            if selected.option_id.0.as_ref() == "cancel" =>
        {
            Ok(None)
        }
        _ => Err("ACP client selected an unknown Maple project trust option".to_string()),
    }
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

/// Agent-text notice sent when Goose replaced the history after compaction.
const COMPACTION_COMPLETED_NOTICE: &str = "Compaction completed.\n";

fn timeline_update(
    item: &AgentTimelineItem,
    tools: &mut AcpToolProjection,
    include_user_messages: bool,
) -> Option<SessionUpdate> {
    match item.item_type.as_str() {
        "message" if include_user_messages && item.role.as_deref() == Some("user") => {
            item.text.as_ref().map(|text| {
                SessionUpdate::UserMessageChunk(
                    ContentChunk::new(ContentBlock::Text(TextContent::new(text.clone())))
                        .message_id(item.id.as_str()),
                )
            })
        }
        "message" if item.role.as_deref() == Some("assistant") => item.text.as_ref().map(|text| {
            SessionUpdate::AgentMessageChunk(
                ContentChunk::new(ContentBlock::Text(TextContent::new(text.clone())))
                    .message_id(item.id.as_str()),
            )
        }),
        "thinking" => item.text.as_ref().map(|text| {
            SessionUpdate::AgentThoughtChunk(
                ContentChunk::new(ContentBlock::Text(TextContent::new(text.clone())))
                    .message_id(item.id.as_str()),
            )
        }),
        "tool" => Some(acp_tool_update(item, tools)),
        // Compaction is a full model round-trip with no other output. The
        // reference adapters (claude-agent-acp, codex-acp) tell the client
        // with a short agent-text notice, so clients render it the same way.
        "system" => item
            .text
            .as_deref()
            .and_then(compaction_notice_text)
            .map(|notice| {
                SessionUpdate::AgentMessageChunk(
                    ContentChunk::new(ContentBlock::Text(TextContent::new(format!("{notice}\n"))))
                        .message_id(item.id.as_str()),
                )
            }),
        _ => None,
    }
}

fn acp_tool_update(item: &AgentTimelineItem, tools: &mut AcpToolProjection) -> SessionUpdate {
    let status = match item.status.as_deref() {
        Some("completed") => ToolCallStatus::Completed,
        Some("failed" | "cancelled") => ToolCallStatus::Failed,
        Some("pending") => ToolCallStatus::Pending,
        _ => ToolCallStatus::InProgress,
    };
    let kind = timeline_tool_kind(item);
    let content = timeline_tool_text(item).map(|text| {
        vec![ToolCallContent::from(ContentBlock::Text(TextContent::new(
            text,
        )))]
    });
    let locations = timeline_tool_locations(item);
    let raw_input = item.input.as_ref().map(bounded_raw_json);
    let raw_output = timeline_tool_raw_output(item);
    let title = item
        .title
        .clone()
        .unwrap_or_else(|| "Maple tool".to_string());
    if tools.seen.insert(item.id.clone()) {
        let mut call = ToolCall::new(item.id.clone(), title)
            .kind(kind)
            .status(status);
        if let Some(content) = content {
            call = call.content(content);
        }
        if !locations.is_empty() {
            call = call.locations(locations);
        }
        if let Some(raw_input) = raw_input {
            call = call.raw_input(raw_input);
        }
        if let Some(raw_output) = raw_output {
            call = call.raw_output(raw_output);
        }
        SessionUpdate::ToolCall(call)
    } else {
        let mut fields = ToolCallUpdateFields::new()
            .title(title)
            .kind(kind)
            .status(status);
        if let Some(content) = content {
            fields = fields.content(content);
        }
        if !locations.is_empty() {
            fields = fields.locations(locations);
        }
        if let Some(raw_input) = raw_input {
            fields = fields.raw_input(raw_input);
        }
        if let Some(raw_output) = raw_output {
            fields = fields.raw_output(raw_output);
        }
        SessionUpdate::ToolCallUpdate(ToolCallUpdate::new(item.id.clone(), fields))
    }
}

fn timeline_tool_kind(item: &AgentTimelineItem) -> ToolKind {
    let title = item
        .title
        .as_deref()
        .unwrap_or_default()
        .to_ascii_lowercase();
    let input = item.input.as_ref();
    if input.and_then(|value| value.get("command")).is_some() || title.contains("terminal") {
        ToolKind::Execute
    } else if input.and_then(|value| value.get("url")).is_some()
        || title.contains("web")
        || title.contains("url")
    {
        ToolKind::Fetch
    } else if input.and_then(|value| value.get("query")).is_some()
        || input.and_then(|value| value.get("pattern")).is_some()
        || title.contains("search")
        || title.contains("find")
    {
        ToolKind::Search
    } else if title.contains("read") {
        ToolKind::Read
    } else if input.and_then(tool_path).is_some()
        || title.contains("edit")
        || title.contains("write")
    {
        ToolKind::Edit
    } else {
        ToolKind::Other
    }
}

fn timeline_tool_text(item: &AgentTimelineItem) -> Option<String> {
    let output_text = || {
        item.output
            .as_ref()
            .and_then(|output| output.get("text"))
            .and_then(serde_json::Value::as_str)
            .map(str::to_string)
    };
    let text = if matches!(
        item.status.as_deref(),
        Some("completed" | "failed" | "cancelled")
    ) {
        // Coalesced replay items retain the original request summary in
        // item.text while the terminal tool result lives in output.text.
        // Paseo renders ACP text content before rawOutput, so preferring the
        // summary here would hide the actual imported result.
        output_text().or_else(|| item.text.clone())
    } else {
        item.text.clone().or_else(output_text)
    };
    text.map(|text| text.chars().take(16_000).collect())
}

fn tool_path(value: &serde_json::Value) -> Option<&str> {
    ["path", "file_path", "file"]
        .into_iter()
        .find_map(|key| value.get(key).and_then(serde_json::Value::as_str))
}

fn timeline_tool_locations(item: &AgentTimelineItem) -> Vec<ToolCallLocation> {
    item.input
        .as_ref()
        .and_then(tool_path)
        .filter(|path| Path::new(path).is_absolute())
        .map(|path| vec![ToolCallLocation::new(PathBuf::from(path))])
        .unwrap_or_default()
}

fn timeline_tool_raw_output(item: &AgentTimelineItem) -> Option<serde_json::Value> {
    let output = item.output.as_ref()?;
    let failure_message = matches!(item.status.as_deref(), Some("failed" | "cancelled"))
        .then(|| {
            output
                .get("text")
                .and_then(serde_json::Value::as_str)
                .map(|text| text.chars().take(MAX_ACP_ERROR_CHARS).collect::<String>())
        })
        .flatten()
        .filter(|message| !message.trim().is_empty());
    let mut bounded = bounded_raw_json(output);
    if let (Some(message), serde_json::Value::Object(fields)) = (failure_message, &mut bounded) {
        // Paseo derives the failure badge from rawOutput.message/error. Maple's
        // persisted tool shape uses output.text, so provide a bounded alias
        // without changing the canonical result or exposing any extra data.
        if !fields.contains_key("message") && !fields.contains_key("error") {
            fields.insert("message".to_string(), serde_json::Value::String(message));
        }
    }
    Some(bounded)
}

fn bounded_raw_json(value: &serde_json::Value) -> serde_json::Value {
    match serde_json::to_vec(value) {
        Ok(encoded) if encoded.len() <= 64 * 1024 => value.clone(),
        Ok(encoded) => serde_json::json!({
            "truncated": true,
            "encodedBytes": encoded.len(),
        }),
        Err(_) => serde_json::json!({ "unavailable": true }),
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

#[cfg(test)]
mod tests {
    use super::*;
    use agent_client_protocol::schema::v1::{
        ClientCapabilities, ElicitationCapabilities, ElicitationFormCapabilities, McpServerStdio,
        SelectedPermissionOutcome,
    };
    use serde_json::json;

    #[test]
    fn caller_session_fields_come_from_the_raw_session_new_params() {
        let fields = AcpCallerSessionFields::from_params(&json!({
            "cwd": "/tmp/project",
            "mcpServers": [],
            "systemPrompt": "  You are Buzz's Maple persona.  ",
            "_meta": { "sessionTitle": "general" },
        }));
        assert_eq!(
            fields,
            AcpCallerSessionFields {
                system_prompt: Some("You are Buzz's Maple persona.".to_string()),
                session_title: Some("general".to_string()),
            }
        );
        let empty = AcpCallerSessionFields::from_params(&json!({
            "cwd": "/tmp/project",
            "systemPrompt": "   ",
        }));
        assert_eq!(empty, AcpCallerSessionFields::default());
    }

    #[test]
    fn default_config_is_disabled_and_caller_mediated() {
        let config = AgentAcpConfig::default();
        assert!(!config.enabled);
        assert_eq!(config.permission_mode, AgentAcpPermissionMode::ReadOnly);
        assert_eq!(config.max_connections, 8);
    }

    #[test]
    fn explicit_connection_limits_remain_configurable_below_the_default() {
        let one = normalize_config(AgentAcpConfig {
            max_connections: 1,
            ..AgentAcpConfig::default()
        })
        .unwrap();
        assert_eq!(one.max_connections, 1);

        let capped = normalize_config(AgentAcpConfig {
            max_connections: usize::MAX,
            ..AgentAcpConfig::default()
        })
        .unwrap();
        assert_eq!(capped.max_connections, MAX_ACP_CONNECTIONS);
    }

    #[test]
    fn session_ids_are_canonicalized_and_empty_ids_are_rejected() {
        assert_eq!(
            canonical_session_id(&SessionId::new("  task-123  ")).unwrap(),
            "task-123"
        );
        assert!(canonical_session_id(&SessionId::new(" \n\t ")).is_err());
    }

    fn session_summary_with_mode(id: &str, mode: &str) -> AgentSessionSummary {
        AgentSessionSummary {
            id: id.to_string(),
            title: id.to_string(),
            project_root: "/tmp/project".to_string(),
            created_ms: 1,
            updated_ms: 1,
            message_count: 0,
            model: Some("model".to_string()),
            mode: mode.to_string(),
            web_enabled: false,
            archived: false,
            acp: false,
        }
    }

    #[test]
    fn session_list_loadability_is_fail_closed_to_read_only_tasks() {
        let sessions = [
            session_summary_with_mode("read-only", "smart_approve"),
            session_summary_with_mode("allow-all", "auto"),
            session_summary_with_mode("approval", "approve"),
            session_summary_with_mode("chat", "chat"),
            session_summary_with_mode("unknown", "future_mode"),
        ];

        let visible = sessions
            .iter()
            .filter(|session| is_acp_loadable_session_mode(&session.mode))
            .map(|session| session.id.as_str())
            .collect::<Vec<_>>();

        assert_eq!(visible, vec!["read-only"]);
    }

    #[test]
    fn session_load_preflight_rejects_non_read_only_and_missing_tasks() {
        let sessions = [
            session_summary_with_mode("read-only", "smart_approve"),
            session_summary_with_mode("allow-all", "auto"),
        ];

        assert_eq!(
            ensure_acp_session_is_loadable(&sessions, "read-only")
                .unwrap()
                .id,
            "read-only"
        );
        assert!(
            ensure_acp_session_is_loadable(&sessions, "allow-all")
                .unwrap_err()
                .contains("only Read only")
        );
        assert!(
            ensure_acp_session_is_loadable(&sessions, "missing")
                .unwrap_err()
                .contains("does not exist")
        );
    }

    #[tokio::test]
    async fn session_operation_cancellation_keeps_close_behind_the_active_fence() {
        let connection_lifetime = CancellationToken::new();
        let operation = AcpSessionOperation::new(&connection_lifetime);
        let active = Arc::clone(&operation.gate).lock_owned().await;
        let waiting_gate = Arc::clone(&operation.gate);
        let waiting = tokio::spawn(async move { waiting_gate.lock_owned().await });

        operation.cancellation.cancel();
        assert!(operation.cancellation.is_cancelled());
        tokio::task::yield_now().await;
        assert!(!waiting.is_finished());

        drop(active);
        let closing = tokio::time::timeout(std::time::Duration::from_secs(1), waiting)
            .await
            .unwrap()
            .unwrap();
        drop(closing);
    }

    #[test]
    fn timed_out_close_keeps_its_resurrection_fence() {
        assert!(close_registration_may_be_released(true, true, true));
        assert!(!close_registration_may_be_released(false, true, true));
        assert!(!close_registration_may_be_released(true, false, true));
        assert!(!close_registration_may_be_released(true, true, false));
    }

    #[test]
    fn completed_replayed_tool_prefers_result_over_request_summary() {
        let item = AgentTimelineItem {
            id: "tool-1".to_string(),
            item_type: "tool".to_string(),
            role: Some("assistant".to_string()),
            title: Some("Terminal".to_string()),
            text: Some("listing project root".to_string()),
            status: Some("completed".to_string()),
            input: Some(serde_json::json!({ "command": "pwd" })),
            output: Some(serde_json::json!({ "text": "/tmp/project" })),
            created_ms: 1,
            merge: "replace".to_string(),
        };
        let mut projection = AcpToolProjection::default();
        let encoded = serde_json::to_value(acp_tool_update(&item, &mut projection)).unwrap();

        assert_eq!(timeline_tool_text(&item).as_deref(), Some("/tmp/project"));
        assert_eq!(encoded["content"][0]["content"]["text"], "/tmp/project");
        assert_eq!(encoded["rawInput"]["command"], "pwd");
    }

    #[test]
    fn failed_replayed_tool_preserves_result_and_failure_badge_message() {
        let mut projection = AcpToolProjection::default();
        let pending = AgentTimelineItem {
            id: "tool-1".to_string(),
            item_type: "tool".to_string(),
            role: Some("assistant".to_string()),
            title: Some("Terminal".to_string()),
            text: Some("running command".to_string()),
            status: Some("pending".to_string()),
            input: Some(serde_json::json!({ "command": "false" })),
            output: None,
            created_ms: 1,
            merge: "replace".to_string(),
        };
        let _ = acp_tool_update(&pending, &mut projection);
        let failed = AgentTimelineItem {
            text: Some("running command".to_string()),
            status: Some("failed".to_string()),
            output: Some(serde_json::json!({
                "text": "command exited with status 1",
                "isError": true,
            })),
            ..pending
        };
        let encoded = serde_json::to_value(acp_tool_update(&failed, &mut projection)).unwrap();

        assert_eq!(encoded["sessionUpdate"], "tool_call_update");
        assert_eq!(
            encoded["content"][0]["content"]["text"],
            "command exited with status 1"
        );
        assert_eq!(
            encoded["rawOutput"]["message"],
            "command exited with status 1"
        );
    }

    #[test]
    fn prompt_usage_serializes_one_turn_without_session_accumulation() {
        let turn = AgentRunUsage {
            input_tokens: 10,
            output_tokens: 4,
            total_tokens: 14,
            cached_read_tokens: 3,
            cached_write_tokens: 1,
        };
        let encoded = serde_json::to_value(acp_usage(turn)).unwrap();

        assert_eq!(encoded["inputTokens"], 10);
        assert_eq!(encoded["outputTokens"], 4);
        assert_eq!(encoded["totalTokens"], 14);
        assert_eq!(encoded["cachedReadTokens"], 3);
        assert_eq!(encoded["cachedWriteTokens"], 1);
    }

    #[test]
    fn model_selector_locks_to_the_persisted_model_after_first_message() {
        let models = vec!["model-a".to_string(), "model-b".to_string()];
        let fresh = serde_json::to_value(acp_session_config_options("model-b", &models, 0))
            .expect("fresh model options should serialize");
        let locked = serde_json::to_value(acp_session_config_options("model-b", &models, 1))
            .expect("locked model options should serialize");

        assert_eq!(fresh[0]["currentValue"], "model-b");
        assert_eq!(fresh[0]["options"].as_array().unwrap().len(), 2);
        assert_eq!(locked[0]["currentValue"], "model-b");
        assert_eq!(locked[0]["options"].as_array().unwrap().len(), 1);
        assert_eq!(locked[0]["options"][0]["value"], "model-b");
    }

    #[test]
    fn streamed_message_chunks_keep_the_timeline_item_id() {
        let mut projection = AcpToolProjection::default();
        for (role, item_type, expected_variant) in [
            (Some("user"), "message", "user_message_chunk"),
            (Some("assistant"), "message", "agent_message_chunk"),
            (None, "thinking", "agent_thought_chunk"),
        ] {
            let item = AgentTimelineItem {
                id: format!("stable-{expected_variant}"),
                item_type: item_type.to_string(),
                role: role.map(str::to_string),
                title: None,
                text: Some("delta".to_string()),
                status: None,
                input: None,
                output: None,
                created_ms: 1,
                merge: "append".to_string(),
            };
            let update = timeline_update(&item, &mut projection, true).unwrap();
            let encoded = serde_json::to_value(update).unwrap();
            assert_eq!(encoded["sessionUpdate"], expected_variant);
            assert_eq!(encoded["messageId"], item.id);
        }
    }

    #[test]
    fn compaction_notices_reach_the_client_as_agent_text() {
        let mut projection = AcpToolProjection::default();
        let system_item = |text: &str| AgentTimelineItem {
            id: "system-1".to_string(),
            item_type: "system".to_string(),
            role: Some("system".to_string()),
            title: Some("Progress".to_string()),
            text: Some(text.to_string()),
            status: None,
            input: None,
            output: None,
            created_ms: 1,
            merge: "replace".to_string(),
        };
        let update = timeline_update(
            &system_item("goose is compacting the conversation..."),
            &mut projection,
            false,
        )
        .unwrap();
        let encoded = serde_json::to_value(update).unwrap();
        assert_eq!(encoded["sessionUpdate"], "agent_message_chunk");
        assert_eq!(encoded["content"]["text"], "Compacting the conversation…\n");
        assert_eq!(encoded["messageId"], "system-1");
        // Other runtime notices stay out of the ACP stream.
        assert!(timeline_update(&system_item("Thinking hard"), &mut projection, false).is_none());
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
    fn project_trust_chooser_is_fail_closed_and_cannot_be_auto_accepted() {
        let encoded = serde_json::to_value(project_trust_permission_options()).unwrap();

        assert_eq!(
            encoded,
            serde_json::json!([
                {
                    "optionId": "keep_untrusted",
                    "name": "Keep project trust disabled",
                    "kind": "allow_once"
                },
                {
                    "optionId": "trust_project",
                    "name": "Trust this project",
                    "kind": "allow_once"
                },
                { "optionId": "cancel", "name": "Cancel turn", "kind": "reject_once" }
            ])
        );
        assert_eq!(
            project_trust_permission_decision(&RequestPermissionOutcome::Selected(
                SelectedPermissionOutcome::new("keep_untrusted")
            )),
            Ok(Some(false))
        );
        assert_eq!(
            project_trust_permission_decision(&RequestPermissionOutcome::Selected(
                SelectedPermissionOutcome::new("trust_project")
            )),
            Ok(Some(true))
        );
        assert_eq!(
            project_trust_permission_decision(&RequestPermissionOutcome::Selected(
                SelectedPermissionOutcome::new("cancel")
            )),
            Ok(None)
        );
        assert_eq!(
            project_trust_permission_decision(&RequestPermissionOutcome::Cancelled),
            Ok(None)
        );
        assert!(
            project_trust_permission_decision(&RequestPermissionOutcome::Selected(
                SelectedPermissionOutcome::new("allow_always")
            ))
            .is_err()
        );
    }

    #[test]
    fn project_trust_elicitation_is_session_scoped_reversible_and_defaults_off() {
        let request = project_trust_elicitation_request(
            SessionId::new("session-1"),
            Path::new("/tmp/maple-project"),
        );
        let encoded = serde_json::to_value(request).unwrap();

        assert_eq!(encoded["mode"], "form");
        assert_eq!(encoded["sessionId"], "session-1");
        assert_eq!(
            encoded["requestedSchema"]["required"],
            json!(["trustProject"])
        );
        assert_eq!(
            encoded["requestedSchema"]["properties"]["trustProject"]["type"],
            "boolean"
        );
        assert_eq!(
            encoded["requestedSchema"]["properties"]["trustProject"]["default"],
            false
        );
        assert!(
            encoded["message"]
                .as_str()
                .unwrap()
                .contains("Normal tool permissions still apply")
        );
    }

    #[test]
    fn form_elicitation_is_used_only_when_the_client_advertises_it() {
        let plain = InitializeRequest::new(agent_client_protocol::schema::ProtocolVersion::V1);
        assert!(!client_supports_form_elicitation(&plain));

        let mut form = InitializeRequest::new(agent_client_protocol::schema::ProtocolVersion::V1);
        form.client_capabilities = ClientCapabilities::new()
            .elicitation(ElicitationCapabilities::new().form(ElicitationFormCapabilities::new()));
        assert!(client_supports_form_elicitation(&form));
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
        assert!(
            acp_permission_decision(&RequestPermissionOutcome::Selected(
                SelectedPermissionOutcome::new("allow_always")
            ))
            .is_err()
        );
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

        assert!(
            tokio::time::timeout(
                std::time::Duration::from_millis(10),
                tracker.reserve(1, &cancellation),
            )
            .await
            .is_err()
        );

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
    fn allowed_project_root_rejects_relative_paths() {
        assert!(ensure_allowed_project_root(Path::new("relative/project"), &[]).is_err());
    }

    #[test]
    fn save_config_writes_atomically_and_round_trips() {
        let root = tempfile::tempdir().unwrap();
        let user_id = "acp-config-user";
        let config = AgentAcpConfig {
            enabled: true,
            allowed_project_roots: vec!["/tmp/project".to_string()],
            ..AgentAcpConfig::default()
        };

        save_config(root.path(), user_id, &config).unwrap();
        // Overwrite once more: the replacement must not leave a temp file.
        save_config(root.path(), user_id, &config).unwrap();

        let path = config_path(root.path(), user_id).unwrap();
        let leftovers = path
            .parent()
            .unwrap()
            .read_dir()
            .unwrap()
            .filter(|entry| {
                entry
                    .as_ref()
                    .unwrap()
                    .file_name()
                    .to_string_lossy()
                    .ends_with(".tmp")
            })
            .count();
        assert_eq!(leftovers, 0);
        #[cfg(unix)]
        {
            let mode = std::fs::metadata(&path).unwrap().permissions().mode() & 0o777;
            assert_eq!(mode, 0o600);
        }
        assert_eq!(load_config(root.path(), user_id).unwrap(), config);
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
    fn arbitrary_stdio_mcp_is_rejected_before_any_process_can_start() {
        let server = McpServer::Stdio(
            McpServerStdio::new("untrusted", "/bin/sh")
                .args(vec!["-c".to_string(), "exit 0".to_string()]),
        );

        assert!(prepare_session_mcp(&HashMap::new(), &[server]).is_err());
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
