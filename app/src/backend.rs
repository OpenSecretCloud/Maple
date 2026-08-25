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
    AgentToolContextSpec, MapleAgentHostResources, MapleAgentService, RecentProjectRoot,
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

fn configured_client_id() -> Uuid {
    std::env::var("MAPLE_CLIENT_ID")
        .ok()
        .and_then(|value| value.parse().ok())
        .unwrap_or_else(|| DEFAULT_CLIENT_ID.parse().expect("valid uuid"))
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

/// Maple's public OpenSecret project id. The backend rejects unknown
/// client ids, so this must match the registered project.
const DEFAULT_CLIENT_ID: &str = "ba5a14b5-d915-47b1-b7b1-afda52bc5fc6";

/// OAuth providers supported by the OpenSecret backend.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OAuthProvider {
    Github,
    Google,
    Apple,
}

impl OAuthProvider {
    pub fn label(self) -> &'static str {
        match self {
            Self::Github => "GitHub",
            Self::Google => "Google",
            Self::Apple => "Apple",
        }
    }
}

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
            client_id: configured_client_id(),
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
        self.persist_auth(
            &snapshot.user_id,
            &snapshot.access_token,
            snapshot.refresh_token.as_deref(),
        );
        Ok(AuthSession { user_id, snapshot })
    }

    pub async fn logout(&self, user_id: &str) -> Result<(), String> {
        self.auth.clear_auth(user_id).await
    }

    fn auth_file() -> std::path::PathBuf {
        config_root().join("auth.json")
    }

    fn persist_auth(&self, user_id: &str, access_token: &str, refresh_token: Option<&str>) {
        let path = Self::auth_file();
        if let Some(parent) = path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        let record = serde_json::json!({
            "user_id": user_id,
            "api_url": self.api_url,
            "access_token": access_token,
            "refresh_token": refresh_token,
        });
        let _ = std::fs::write(&path, record.to_string());
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let _ = std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600));
        }
    }

    fn load_persisted_auth(&self) -> Option<(String, String, Option<String>)> {
        let path = Self::auth_file();
        let bytes = std::fs::read(&path).ok()?;
        let record: serde_json::Value = serde_json::from_slice(&bytes).ok()?;
        let user_id = record.get("user_id")?.as_str()?.to_string();
        let api_url = record.get("api_url")?.as_str()?.to_string();
        if api_url != self.api_url {
            // Credentials belong to a different backend; do not reuse them.
            return None;
        }
        let access_token = record.get("access_token")?.as_str()?.to_string();
        let refresh_token = record
            .get("refresh_token")
            .and_then(|value| value.as_str())
            .map(str::to_string);
        Some((user_id, access_token, refresh_token))
    }

    fn clear_persisted_auth() {
        let _ = std::fs::remove_file(Self::auth_file());
    }

    /// Restore a persisted session before the UI starts. Validates the
    /// credentials against the backend; returns the account id on success.
    pub fn restore_now(&self) -> Option<String> {
        let (user_id, access_token, refresh_token) = self.load_persisted_auth()?;
        let api_url = self.api_url.clone();
        let auth = &self.auth;
        let result = self.runtime.block_on(async move {
            let request = MapleApiAuthRequest {
                user_id,
                api_url,
                access_token,
                refresh_token,
            };
            match tokio::time::timeout(
                std::time::Duration::from_secs(30),
                auth.set_auth(Arc::new(NoopAuthEventSink), request),
            )
            .await
            {
                Ok(Ok(snapshot)) => Ok(snapshot.user_id),
                Ok(Err(error)) => Err(error),
                Err(_) => Err("timeout".to_string()),
            }
        });
        match result {
            Ok(user_id) => Some(user_id),
            Err(error) => {
                log::debug!("persisted auth rejected: {error:?}");
                Self::clear_persisted_auth();
                None
            }
        }
    }

    /// Sign out completely: forget persisted credentials, then clear the
    /// in-memory session.
    pub async fn logout_and_clear(&self, user_id: &str) -> Result<(), String> {
        Self::clear_persisted_auth();
        self.auth.clear_auth(user_id).await
    }

    fn oauth_client(&self) -> Result<OpenSecretClient, String> {
        let environment = configured_pcr0_environment()?;
        OpenSecretClient::new_with_pcr0_environment(self.api_url.clone(), environment)
            .map_err(|_| "Maple API authentication failed".to_string())
    }

    async fn publish_session(
        &self,
        user_id: String,
        access_token: String,
        refresh_token: Option<String>,
    ) -> Result<AuthSession, String> {
        self.auth
            .set_auth(
                Arc::new(NoopAuthEventSink),
                MapleApiAuthRequest {
                    user_id,
                    api_url: self.api_url.clone(),
                    access_token,
                    refresh_token,
                },
            )
            .await
            .map_err(|message| {
                // Keep validation detail out of the UI; it can echo the
                // configured URL back to the user.
                log::debug!("set_auth failed during sign in: {message}");
                "Sign in failed. Try again.".to_string()
            })
            .map(|snapshot| {
                self.persist_auth(
                    &snapshot.user_id,
                    &snapshot.access_token,
                    snapshot.refresh_token.as_deref(),
                );
                AuthSession {
                    user_id: snapshot.user_id.clone(),
                    snapshot,
                }
            })
    }
    /// Begin an OAuth flow: returns the authorization URL to open in a
    /// browser (also opens it via the system browser).
    pub async fn oauth_start(&self, provider: OAuthProvider) -> Result<String, String> {
        let client = self.oauth_client()?;
        let client_id = self.client_id;
        let (auth_url, state) = match provider {
            OAuthProvider::Github => {
                let response = client
                    .initiate_github_auth(client_id, None)
                    .await
                    .map_err(|_| "Could not start GitHub sign in".to_string())?;
                (response.auth_url, response.state)
            }
            OAuthProvider::Google => {
                let response = client
                    .initiate_google_auth(client_id, None)
                    .await
                    .map_err(|_| "Could not start Google sign in".to_string())?;
                (response.auth_url, response.state)
            }
            OAuthProvider::Apple => {
                let response = client
                    .initiate_apple_auth(client_id, None)
                    .await
                    .map_err(|_| "Could not start Apple sign in".to_string())?;
                (response.auth_url, response.state)
            }
        };
        // The state lives in the redirected URL the user pastes back; the
        // backend re-validates it during the callback exchange.
        let _ = state;
        if webbrowser::open(&auth_url).is_err() {
            // No system browser available: the UI still shows the URL.
            log::debug!("failed to open system browser for OAuth");
        }
        Ok(auth_url)
    }

    /// Complete an OAuth flow from the redirected URL (pasted by the user or
    /// captured from a loopback redirect).
    pub async fn oauth_complete(
        &self,
        provider: OAuthProvider,
        redirected_url: String,
    ) -> Result<AuthSession, String> {
        let Some((code, state)) = parse_oauth_callback(&redirected_url) else {
            return Err(
                "Paste the full URL you were redirected to (it contains code and state)"
                    .to_string(),
            );
        };
        let client = self.oauth_client()?;
        let response = match provider {
            OAuthProvider::Github => client
                .handle_github_callback(code, state, String::new())
                .await
                .map_err(|_| "GitHub sign in failed".to_string())?,
            OAuthProvider::Google => client
                .handle_google_callback(code, state, String::new())
                .await
                .map_err(|_| "Google sign in failed".to_string())?,
            OAuthProvider::Apple => client
                .handle_apple_callback(code, state, String::new())
                .await
                .map_err(|_| "Apple sign in failed".to_string())?,
        };
        self.publish_session(
            response.id.to_string(),
            response.access_token,
            Some(response.refresh_token),
        )
        .await
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
        // A wedged enclave connection must surface as an error, not an
        // eternal spinner.
        tokio::time::timeout(
            std::time::Duration::from_secs(60),
            handle.start(session, request),
        )
        .await
        .map_err(|_| "Runtime start timed out. Check your connection and retry.".to_string())?
    }

    pub async fn stop_runtime(&self, user_id: &str) -> Result<AgentRuntimeStatus, String> {
        self.service.handle_for_user(user_id).await?.stop().await
    }

    /// Roots recently used by this account, most recent first.
    pub async fn recent_project_roots(
        &self,
        user_id: &str,
    ) -> Result<Vec<RecentProjectRoot>, String> {
        self.service
            .handle_for_user(user_id)
            .await?
            .list_recent_project_roots()
            .await
    }

    /// Switch the runtime to a different project root: registers the root as
    /// recent, restarts the agent under it, and returns the new status.
    pub async fn set_project_root(
        &self,
        user_id: &str,
        path: String,
    ) -> Result<AgentRuntimeStatus, String> {
        let handle = self.service.handle_for_user(user_id).await?;
        handle.save_recent_project_root(path.clone()).await?;
        let session = self.auth.session_for(user_id).await?;
        handle
            .restart(
                session,
                Some(AgentStartRequest {
                    project_root: Some(path),
                    model: None,
                    mode: None,
                }),
            )
            .await
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
    /// directory with the configured model and the SmartApprove policy.
    pub fn default_start_request(&self) -> AgentStartRequest {
        AgentStartRequest {
            project_root: Some(default_project_root()),
            model: std::env::var("MAPLE_MODEL").ok(),
            mode: None,
        }
    }

    /// Model the UI should select initially: MAPLE_MODEL when set.
    pub fn configured_model(&self) -> Option<String> {
        std::env::var("MAPLE_MODEL").ok()
    }
}

/// Extract `code` and `state` query parameters from an OAuth redirect URL.
fn parse_oauth_callback(url: &str) -> Option<(String, String)> {
    let query = url.split_once('?')?.1;
    let mut code = None;
    let mut state = None;
    for pair in query.split(['&', '#']) {
        if let Some((key, value)) = pair.split_once('=') {
            match key {
                "code" => code = Some(value.to_string()),
                "state" => state = Some(value.to_string()),
                _ => {}
            }
        }
    }
    Some((code?, state?))
}
