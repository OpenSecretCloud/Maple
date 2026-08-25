//! Backend boundary for the gpui frontend.
//!
//! This is the only module that imports `maple_agent`. It owns a private
//! Tokio runtime and exposes an async facade plus an event stream. The UI
//! never touches the agent runtime directly, so this seam can later be moved
//! behind a process or socket boundary without touching UI code.

use std::collections::{BTreeMap, BTreeSet};
use std::future::Future;
use std::path::PathBuf;
use std::sync::Arc;

use maple_agent::agent::{
    AgentCreateSessionRequest, AgentEventSink, AgentRuntimeStatus, AgentSendMessageRequest,
    AgentServiceEvent, AgentSessionDetail, AgentSessionSummary, AgentStartRequest,
    AgentToolContextSpec, MapleAgentHostResources, MapleAgentService,
};
use maple_agent::maple_api::{
    MapleApiAuthRequest, MapleApiAuthSnapshot, MapleApiAuthState, NoopAuthEventSink,
};
use maple_agent::open_secret_config::configured_pcr0_environment;
use opensecret::OpenSecretClient;
use tokio::runtime::Runtime;
use tokio::sync::mpsc;
use uuid::Uuid;
#[derive(Debug, Clone)]
pub struct PendingPermission {
    pub session_id: String,
    pub run_id: String,
    pub request_id: String,
    pub tool_name: String,
    pub prompt: Option<String>,
    pub arguments: serde_json::Value,
}

/// The signed-in account identity plus the validated runtime auth snapshot.
#[derive(Debug, Clone)]
pub struct AuthSession {
    pub user_id: String,
    pub snapshot: MapleApiAuthSnapshot,
}

pub struct AgentBackend {
    runtime: Runtime,
    service: MapleAgentService,
    auth: MapleApiAuthState,
    api_url: String,
    client_id: Uuid,
    event_rx: tokio::sync::Mutex<Option<mpsc::UnboundedReceiver<AgentServiceEvent>>>,
}

struct ChannelEventSink(mpsc::UnboundedSender<AgentServiceEvent>);

impl AgentEventSink for ChannelEventSink {
    fn emit(&self, event: &AgentServiceEvent) {
        // The channel is unbounded, so sends only fail after the UI dropped
        // the receiver (window closed). The runtime tolerates missing
        // notifications for that case; surface it once for diagnosis.
        if self.0.send(event.clone()).is_err() {
            log::debug!("agent event receiver is gone; dropping further events");
        }
    }
}

const APP_DIR_NAME: &str = "maple-gpui";

fn config_root() -> PathBuf {
    let base = std::env::var_os("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .filter(|path| path.is_absolute())
        .or_else(|| home_dir().map(|home| home.join(".config")))
        .unwrap_or_else(|| PathBuf::from("."));
    base.join(APP_DIR_NAME)
}

fn local_data_root() -> PathBuf {
    let base = std::env::var_os("XDG_DATA_HOME")
        .map(PathBuf::from)
        .filter(|path| path.is_absolute())
        .or_else(|| home_dir().map(|home| home.join(".local").join("share")))
        .unwrap_or_else(|| PathBuf::from("."));
    base.join(APP_DIR_NAME)
}

fn home_dir() -> Option<PathBuf> {
    std::env::var_os("HOME")
        .map(PathBuf::from)
        .filter(|path| path.is_absolute())
}

fn default_project_root() -> String {
    std::env::current_dir()
        .map(|path| path.to_string_lossy().to_string())
        .unwrap_or_else(|_| {
            home_dir()
                .map(|p| p.to_string_lossy().to_string())
                .unwrap_or_default()
        })
}

impl AgentBackend {
    pub fn new(api_url: String) -> Result<Self, String> {
        // Enforce the credential-bearing URL policy before any client is
        // built, including the login-time SDK client.
        let api_url = maple_agent::maple_api::validate_api_url(&api_url)?;
        let (event_tx, event_rx) = mpsc::unbounded_channel();
        let paths =
            maple_agent::agent::AgentPathLayout::from_app_roots(config_root(), local_data_root());
        let default_tool_context =
            AgentToolContextSpec::try_new(BTreeMap::new(), BTreeSet::new(), false)?;
        let service = MapleAgentService::new(MapleAgentHostResources::new(
            paths,
            Arc::new(ChannelEventSink(event_tx)),
            default_tool_context,
        ));
        Ok(Self {
            runtime: Runtime::new().map_err(|error| format!("failed to start runtime: {error}"))?,
            service,
            auth: MapleApiAuthState::new(),
            api_url,
            client_id: Uuid::new_v4(),
            event_rx: tokio::sync::Mutex::new(Some(event_rx)),
        })
    }

    pub fn api_url(&self) -> &str {
        &self.api_url
    }

    /// Run a backend future on the backend runtime. The returned handle is a
    /// plain future, so the UI executor can await it without owning Tokio.
    pub fn spawn<F>(&self, future: F) -> tokio::task::JoinHandle<F::Output>
    where
        F: Future + Send + 'static,
        F::Output: Send + 'static,
    {
        self.runtime.spawn(future)
    }

    /// Take the backend event stream. Only the first caller receives it.
    pub async fn take_events(&self) -> Option<mpsc::UnboundedReceiver<AgentServiceEvent>> {
        self.event_rx.lock().await.take()
    }

    fn normalize_email(email: &str) -> Result<String, String> {
        let email = email.trim().to_ascii_lowercase();
        if email.is_empty() {
            return Err("Enter an email address".to_string());
        }
        Ok(email)
    }

    /// Sign in with email and password through the OpenSecret SDK, then hand
    /// the validated credentials to the agent runtime.
    pub async fn login(&self, email: String, password: String) -> Result<AuthSession, String> {
        let email = Self::normalize_email(&email)?;
        if password.is_empty() {
            return Err("Enter a password".to_string());
        }
        let environment = configured_pcr0_environment()?;
        let client = OpenSecretClient::new_with_pcr0_environment(self.api_url.clone(), environment)
            .map_err(|_| "Maple API authentication failed".to_string())?;
        let response = client
            .login(email.clone(), password, self.client_id)
            .await
            .map_err(|_| "Sign in failed. Check your email and password.".to_string())?;
        let user_id = response.id.to_string();
        let snapshot = self
            .auth
            .set_auth(
                Arc::new(NoopAuthEventSink),
                MapleApiAuthRequest {
                    user_id: user_id.clone(),
                    api_url: self.api_url.clone(),
                    access_token: response.access_token,
                    refresh_token: Some(response.refresh_token),
                },
            )
            .await
            .map_err(|message| {
                // Keep validation detail out of the UI; it can echo the
                // configured URL back to the user.
                log::debug!("set_auth failed during sign in: {message}");
                "Sign in failed. Try again.".to_string()
            })?;
        Ok(AuthSession { user_id, snapshot })
    }

    pub async fn logout(&self, user_id: &str) -> Result<(), String> {
        self.auth.clear_auth(user_id).await
    }

    pub async fn runtime_status(&self, user_id: &str) -> Result<AgentRuntimeStatus, String> {
        self.service.handle_for_user(user_id).await?.status().await
    }

    pub async fn start_runtime(
        &self,
        user_id: &str,
        request: Option<AgentStartRequest>,
    ) -> Result<AgentRuntimeStatus, String> {
        let handle = self.service.handle_for_user(user_id).await?;
        let session = self.auth.session_for(user_id).await?;
        handle.start(session, request).await
    }

    pub async fn stop_runtime(&self, user_id: &str) -> Result<AgentRuntimeStatus, String> {
        self.service.handle_for_user(user_id).await?.stop().await
    }

    pub async fn list_sessions(
        &self,
        user_id: &str,
        project_root: Option<String>,
    ) -> Result<Vec<AgentSessionSummary>, String> {
        self.service
            .handle_for_user(user_id)
            .await?
            .list_sessions(project_root)
            .await
    }

    pub async fn create_session(
        &self,
        user_id: &str,
        request: Option<AgentCreateSessionRequest>,
    ) -> Result<AgentSessionDetail, String> {
        self.service
            .handle_for_user(user_id)
            .await?
            .create_session(request)
            .await
    }

    pub async fn load_session(
        &self,
        user_id: &str,
        session_id: &str,
    ) -> Result<AgentSessionDetail, String> {
        self.service
            .handle_for_user(user_id)
            .await?
            .load_session(session_id.to_string())
            .await
    }

    pub async fn delete_session(&self, user_id: &str, session_id: &str) -> Result<(), String> {
        self.service
            .handle_for_user(user_id)
            .await?
            .delete_session(session_id.to_string())
            .await
    }

    pub async fn send_message(
        &self,
        user_id: &str,
        request: AgentSendMessageRequest,
    ) -> Result<String, String> {
        let run_id = self
            .service
            .handle_for_user(user_id)
            .await?
            .send_message(request)
            .await?
            .run_id;
        Ok(run_id)
    }

    pub async fn cancel_run(&self, user_id: &str, run_id: &str) -> Result<(), String> {
        self.service
            .handle_for_user(user_id)
            .await?
            .cancel_desktop_run(run_id.to_string())
            .await
    }

    pub async fn available_model_ids(&self, user_id: &str) -> Result<Vec<String>, String> {
        self.service
            .handle_for_user(user_id)
            .await?
            .available_model_ids()
            .await
    }

    pub async fn permission_respond(
        &self,
        user_id: &str,
        session_id: &str,
        request_id: &str,
        allow: bool,
    ) -> Result<(), String> {
        self.service
            .handle_for_user(user_id)
            .await?
            .permission_respond(maple_agent::agent::AgentPermissionResponse {
                session_id: session_id.to_string(),
                request_id: request_id.to_string(),
                decision: if allow {
                    "allow_once".to_string()
                } else {
                    "deny_once".to_string()
                },
            })
            .await
    }

    /// Standard start request for this app: agent rooted at the launch
    /// directory with the default model and the SmartApprove policy.
    pub fn default_start_request(&self) -> AgentStartRequest {
        AgentStartRequest {
            project_root: Some(default_project_root()),
            model: None,
            mode: None,
        }
    }
}
