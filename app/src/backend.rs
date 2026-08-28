//! Backend boundary for the gpui frontend.
//!
//! This is the only module that imports `maple_agent`. It owns a private
//! Tokio runtime and exposes an async facade plus an event stream. The UI
//! never touches the agent runtime directly, so this seam can later be moved
//! behind a process or socket boundary without touching UI code.

use std::collections::HashMap;
use std::future::Future;
use std::path::PathBuf;
use std::sync::Arc;

use maple_agent::agent::{
    AgentCreateSessionRequest, AgentDesktopQueueSnapshot, AgentEventSink, AgentProjectTrustStatus,
    AgentQueueControlRequest, AgentRenameSessionRequest, AgentRuntimeStatus,
    AgentSendMessageRequest, AgentServiceEvent, AgentSessionDetail, AgentSessionSummary,
    AgentSlashCommand, AgentStartRequest, MapleAgentHostResources, MapleAgentService,
    RecentProjectRoot,
};
use maple_agent::maple_api::{MapleApiAuthRequest, MapleApiAuthState, NoopAuthEventSink};
use maple_agent::open_secret_config::configured_pcr0_environment;
use opensecret::OpenSecretClient;
use tokio::runtime::Runtime;
use tokio::sync::mpsc;
use uuid::Uuid;

#[derive(Debug, Clone)]
pub struct PendingQuestion {
    pub session_id: String,
    pub request_id: String,
    /// One or more related questions answered together in one card.
    pub questions: Vec<maple_agent::agent::AgentQuestion>,
}

#[derive(Debug, Clone)]
pub struct PendingPermission {
    pub session_id: String,
    pub run_id: String,
    pub request_id: String,
    pub tool_name: String,
    pub prompt: Option<String>,
}

/// The signed-in account identity.
#[derive(Debug, Clone)]
pub struct AuthSession {
    pub user_id: String,
}

pub struct AgentBackend {
    runtime: Runtime,
    service: MapleAgentService,
    auth: MapleApiAuthState,
    api_url: String,
    client_id: Uuid,
    event_rx: tokio::sync::Mutex<Option<mpsc::UnboundedReceiver<AgentServiceEvent>>>,
    billing: crate::billing::BillingClient,
    /// Cached billing JWT per user id. Replaced after a 401.
    billing_tokens: tokio::sync::Mutex<HashMap<String, String>>,
    /// Open handle to the usage ledger DB; the context ring polls it every
    /// second during a run, so it is not reopened per query.
    usage_db: std::sync::Mutex<Option<(PathBuf, rusqlite::Connection)>>,
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

impl AgentBackend {
    /// The account scope (sha of the user id) used for on-disk layout.
    pub fn account_scope(&self, user_id: &str) -> Option<String> {
        maple_agent::maple_api::account_scope(user_id).ok()
    }
}

/// App configuration root (XDG-style), also used by the settings store.
pub fn app_config_root() -> PathBuf {
    config_root()
}

/// Path to the goose sessions database for one account scope.
pub fn account_session_db(account_scope: &str) -> PathBuf {
    config_root()
        .join("agent")
        .join("accounts")
        .join(account_scope)
        .join("goose")
        .join("data")
        .join("sessions")
        .join("sessions.db")
}

fn config_root() -> PathBuf {
    let base = std::env::var_os("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .filter(|path| path.is_absolute())
        .or_else(|| home_dir().map(|home| home.join(".config")))
        .unwrap_or_else(|| PathBuf::from("."));
    base.join(APP_DIR_NAME)
}

pub fn local_data_root() -> PathBuf {
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

/// Root the desktop app opens when the account has no saved root. The GUI
/// must not depend on the directory it was launched from: that is the job of
/// the `maple acp` command, not a windowed app started from a launcher.
fn fallback_project_root() -> Option<String> {
    home_dir().map(|path| path.to_string_lossy().to_string())
}

/// Root for a GUI start: the saved default when it still is a folder, else
/// the home directory. Never the process working directory.
fn gui_project_root(config: &maple_agent::agent::AgentConfig) -> Option<String> {
    config
        .default_project_root
        .as_deref()
        .filter(|path| !path.trim().is_empty() && std::path::Path::new(path).is_dir())
        .map(str::to_owned)
        .or_else(fallback_project_root)
}

impl AgentBackend {
    pub fn new(api_url: String, harness_instructions: String) -> Result<Self, String> {
        // Enforce the credential-bearing URL policy before any client is
        // built, including the login-time SDK client.
        let api_url = maple_agent::maple_api::validate_api_url(&api_url)?;
        let (event_tx, event_rx) = mpsc::unbounded_channel();
        let paths =
            maple_agent::agent::AgentPathLayout::from_app_roots(config_root(), local_data_root());
        // Keeps ACP bridge credentials out of desktop tool environments.
        let default_tool_context = maple_agent::acp::default_tool_context_spec()?;
        let service = MapleAgentService::new(MapleAgentHostResources::new(
            paths,
            Arc::new(ChannelEventSink(event_tx)),
            default_tool_context,
            harness_instructions,
        ));
        let runtime =
            Runtime::new().map_err(|error| format!("failed to start runtime: {error}"))?;
        // reqwest clients must be built inside a Tokio runtime context; one
        // built outside never completes a request.
        let billing = {
            let _guard = runtime.enter();
            crate::billing::BillingClient::new(crate::billing::configured_billing_api_url())
        };
        Ok(Self {
            runtime,
            service,
            auth: MapleApiAuthState::new(),
            api_url,
            client_id: configured_client_id(),
            event_rx: tokio::sync::Mutex::new(Some(event_rx)),
            billing,
            billing_tokens: tokio::sync::Mutex::new(HashMap::new()),
            usage_db: std::sync::Mutex::new(None),
        })
    }

    /// Replace the opening system prompt text for tasks this app hosts.
    /// Applies to agents built after the call, so to a task's next fresh
    /// agent, not to one already loaded.
    pub fn set_harness_instructions(&self, harness_instructions: String) {
        self.service.set_harness_instructions(harness_instructions);
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
        Ok(AuthSession { user_id })
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

    /// Plan usage for the sidebar card from the Maple billing API. Returns
    /// `None` when the subscription has no token meter.
    pub async fn plan_usage(
        &self,
        user_id: &str,
    ) -> Result<Option<crate::billing::PlanUsage>, String> {
        use crate::billing::BillingError;
        let session = self.auth.session_for(user_id).await?;
        let cached = self.billing_tokens.lock().await.get(user_id).cloned();
        let mut token = match cached {
            Some(token) => token,
            None => self.mint_billing_token(&session, user_id).await?,
        };
        let mut result = self.subscription_status(&token).await;
        if matches!(result, Err(BillingError::Unauthorized)) {
            // The cached token expired or was revoked: mint one and retry once.
            token = self.mint_billing_token(&session, user_id).await?;
            result = self.subscription_status(&token).await;
        }
        let status = match result {
            Ok(status) => status,
            Err(BillingError::Unauthorized) => {
                self.billing_tokens.lock().await.remove(user_id);
                return Err(BillingError::Unauthorized.to_string());
            }
            Err(BillingError::Other(error)) => return Err(error),
        };
        let plan = crate::billing::PlanUsage::from_status(&status, chrono::Local::now());
        log::debug!("plan usage: {plan:?}");
        Ok(plan)
    }

    async fn subscription_status(
        &self,
        token: &str,
    ) -> Result<crate::billing::BillingStatus, crate::billing::BillingError> {
        tokio::time::timeout(
            std::time::Duration::from_secs(20),
            self.billing.subscription_status(token),
        )
        .await
        .unwrap_or_else(|_| {
            Err(crate::billing::BillingError::Other(
                "billing request timed out".to_string(),
            ))
        })
    }

    async fn mint_billing_token(
        &self,
        session: &Arc<maple_agent::maple_api::MapleApiSession>,
        user_id: &str,
    ) -> Result<String, String> {
        let token = session
            .third_party_token(self.billing.base_url().to_string())
            .await?;
        self.billing_tokens
            .lock()
            .await
            .insert(user_id.to_string(), token.clone());
        Ok(token)
    }

    pub async fn start_runtime(
        &self,
        user_id: &str,
        request: Option<AgentStartRequest>,
    ) -> Result<AgentRuntimeStatus, String> {
        let handle = self.service.handle_for_user(user_id).await?;
        let session = self.auth.session_for(user_id).await?;
        // The agent falls back to the process working directory when no root
        // is given. That is right for `maple acp`, not for the GUI: pick the
        // saved root or the home directory instead.
        let request = match request {
            Some(AgentStartRequest {
                project_root: None,
                model,
                mode,
            }) => {
                let config = handle.load_config().await?;
                Some(AgentStartRequest {
                    project_root: gui_project_root(&config),
                    model,
                    mode,
                })
            }
            other => other,
        };
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

    /// Serve ACP on stdin/stdout for `user_id` until the peer closes stdin.
    /// Starts the runtime first, rooted at the process working directory,
    /// and stops it when the connection ends.
    pub fn run_acp_stdio(&self, user_id: &str) -> Result<(), String> {
        self.runtime.block_on(async {
            let handle = self.service.handle_for_user(user_id).await?;
            let session = self.auth.session_for(user_id).await?;
            handle.start(session, None).await?;
            let config = maple_agent::acp::load_acp_config(&local_data_root(), user_id)?;
            let result = maple_agent::acp::serve_stdio(handle.clone(), config).await;
            if let Err(error) = handle.stop().await {
                log::warn!("failed to stop the agent runtime after ACP: {error}");
            }
            result
        })
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
        let mut sessions = self
            .service
            .handle_for_user(user_id)
            .await?
            .list_sessions(project_root)
            .await?;
        // Tasks that an ACP client created belong to that client's UI, not
        // to the desktop task list.
        sessions.retain(|session| !session.acp);
        Ok(sessions)
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

    pub async fn set_session_archived(
        &self,
        user_id: &str,
        session_id: &str,
        archived: bool,
    ) -> Result<AgentSessionSummary, String> {
        self.service
            .handle_for_user(user_id)
            .await?
            .set_session_archived(session_id.to_string(), archived)
            .await
    }

    /// Drop a root from the recent list. `fallback` becomes the runtime
    /// root when the removed one was current.
    pub async fn remove_project_root(
        &self,
        user_id: &str,
        path: String,
        fallback: Option<String>,
    ) -> Result<(), String> {
        self.service
            .handle_for_user(user_id)
            .await?
            .remove_project_root(path, fallback)
            .await
            .map(|_| ())
    }

    pub async fn rename_session(
        &self,
        user_id: &str,
        session_id: &str,
        title: String,
    ) -> Result<AgentSessionSummary, String> {
        let handle = self.service.handle_for_user(user_id).await?;
        let session = self.auth.session_for(user_id).await?;
        handle
            .rename_session(
                session,
                AgentRenameSessionRequest {
                    session_id: session_id.to_string(),
                    title,
                },
            )
            .await
    }

    /// Whether the project at `path` has skills or other guidance that
    /// need a trust decision, and what the saved decision is.
    pub async fn project_trust(
        &self,
        user_id: &str,
        path: String,
    ) -> Result<AgentProjectTrustStatus, String> {
        self.service
            .handle_for_user(user_id)
            .await?
            .get_project_trust(path)
            .await
    }

    pub async fn set_project_trust(
        &self,
        user_id: &str,
        path: String,
        trusted: bool,
    ) -> Result<AgentProjectTrustStatus, String> {
        self.service
            .handle_for_user(user_id)
            .await?
            .set_project_trust(path, trusted)
            .await
    }

    /// Drop a message that waits behind the active run.
    pub async fn cancel_queued_message(
        &self,
        user_id: &str,
        session_id: &str,
        queue_id: &str,
    ) -> Result<AgentDesktopQueueSnapshot, String> {
        self.service
            .handle_for_user(user_id)
            .await?
            .cancel_queued_message(AgentQueueControlRequest {
                session_id: session_id.to_string(),
                queue_id: queue_id.to_string(),
            })
            .await
    }

    /// Hold a queued message while the user edits it: it is not promoted
    /// into the run until the edit ends.
    pub async fn begin_queued_message_edit(
        &self,
        user_id: &str,
        session_id: &str,
        queue_id: &str,
    ) -> Result<(), String> {
        self.service
            .handle_for_user(user_id)
            .await?
            .begin_queued_message_edit(AgentQueueControlRequest {
                session_id: session_id.to_string(),
                queue_id: queue_id.to_string(),
            })
            .await
    }

    /// Release a queued message held by [`Self::begin_queued_message_edit`]
    /// without changing it.
    pub async fn end_queued_message_edit(
        &self,
        user_id: &str,
        session_id: &str,
        queue_id: &str,
    ) -> Result<(), String> {
        self.service
            .handle_for_user(user_id)
            .await?
            .end_queued_message_edit(AgentQueueControlRequest {
                session_id: session_id.to_string(),
                queue_id: queue_id.to_string(),
            })
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

    /// Bytes of an image the user attached to a message in `session_id`.
    pub async fn read_image_attachment(
        &self,
        user_id: &str,
        session_id: &str,
        attachment_id: &str,
    ) -> Result<Vec<u8>, String> {
        self.service
            .handle_for_user(user_id)
            .await?
            .read_image_attachment(session_id.to_string(), attachment_id.to_string())
            .await
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

    /// Catalog vision flag for a model; None when unknown.
    pub async fn model_supports_vision(
        &self,
        user_id: &str,
        model: &str,
    ) -> Result<Option<bool>, String> {
        self.service
            .handle_for_user(user_id)
            .await?
            .model_supports_vision(model)
            .await
    }

    /// MCP servers configured for the account, with the session's enabled
    /// state for each.
    pub async fn list_session_mcp_servers(
        &self,
        user_id: &str,
        session_id: &str,
    ) -> Result<Vec<maple_agent::agent::AgentSessionMcpServer>, String> {
        self.service
            .handle_for_user(user_id)
            .await?
            .list_session_mcp_servers(session_id.to_string())
            .await
    }

    pub async fn set_session_mcp_server_enabled(
        &self,
        user_id: &str,
        session_id: &str,
        name: &str,
        enabled: bool,
    ) -> Result<Vec<maple_agent::agent::AgentSessionMcpServer>, String> {
        self.service
            .handle_for_user(user_id)
            .await?
            .set_session_mcp_server_enabled(maple_agent::agent::AgentSetSessionMcpServerRequest {
                session_id: session_id.to_string(),
                name: name.to_string(),
                enabled,
            })
            .await
    }

    pub async fn list_mcp_servers(
        &self,
        user_id: &str,
    ) -> Result<Vec<maple_agent::agent::AgentMcpServer>, String> {
        self.service
            .handle_for_user(user_id)
            .await?
            .list_mcp_servers()
            .await
    }

    pub async fn save_mcp_servers(
        &self,
        user_id: &str,
        servers: Vec<maple_agent::agent::AgentMcpServer>,
    ) -> Result<Vec<maple_agent::agent::AgentMcpServer>, String> {
        self.service
            .handle_for_user(user_id)
            .await?
            .save_mcp_servers(servers)
            .await
    }

    /// Turn the web tools on or off for a session (next turn onward).
    pub async fn set_session_web_enabled(
        &self,
        user_id: &str,
        session_id: &str,
        enabled: bool,
    ) -> Result<AgentSessionSummary, String> {
        self.service
            .handle_for_user(user_id)
            .await?
            .set_session_web_enabled(maple_agent::agent::AgentSetSessionWebRequest {
                session_id: session_id.to_string(),
                enabled,
            })
            .await
    }

    /// Set the permission policy for a session: "smart_approve" asks for
    /// each gated tool, "auto" approves everything (bypass).
    pub async fn set_permission_mode(
        &self,
        user_id: &str,
        session_id: &str,
        mode: &str,
    ) -> Result<(), String> {
        self.service
            .handle_for_user(user_id)
            .await?
            .set_permission_mode(maple_agent::agent::AgentPermissionModeRequest {
                session_id: session_id.to_string(),
                mode: mode.to_string(),
            })
            .await
    }

    /// Compact a session's history now; reload the session afterwards.
    pub async fn compact_session(&self, user_id: &str, session_id: &str) -> Result<(), String> {
        self.service
            .handle_for_user(user_id)
            .await?
            .compact_session(session_id.to_string())
            .await
    }

    /// Slash commands (installed skills) for a working directory. Filesystem
    /// scan, so it runs on a blocking thread.
    pub async fn list_slash_commands(
        &self,
        working_dir: Option<String>,
    ) -> Result<Vec<AgentSlashCommand>, String> {
        let service = self.service.clone();
        tokio::task::spawn_blocking(move || service.list_slash_commands(working_dir.as_deref()))
            .await
            .map_err(|error| format!("Slash command scan failed: {error}"))
    }

    /// Expand `/command args` into the skill prompt; `None` when the command
    /// matches no skill.
    pub async fn resolve_slash_command(
        &self,
        working_dir: Option<String>,
        command: String,
        args: String,
    ) -> Result<Option<String>, String> {
        let service = self.service.clone();
        tokio::task::spawn_blocking(move || {
            service.resolve_slash_command(working_dir.as_deref(), &command, &args)
        })
        .await
        .map_err(|error| format!("Slash command resolve failed: {error}"))?
    }

    /// One-line summary of a completed tool call from the cheap title model.
    pub async fn summarize_tool_call(
        &self,
        user_id: &str,
        session_id: &str,
        tool_name: String,
        input: Option<serde_json::Value>,
        output_text: String,
    ) -> Result<Option<String>, String> {
        self.service
            .handle_for_user(user_id)
            .await?
            .summarize_tool_call(session_id, &tool_name, input.as_ref(), &output_text)
            .await
    }
    /// Latest context usage for a session from the goose usage ledger:
    /// (context tokens, context limit). The limit comes from the model
    /// catalog for the selected model; MAPLE_CONTEXT_LIMIT is a manual
    /// override; 200k is the fallback when the catalog lacks the model.
    pub async fn context_usage(
        &self,
        user_id: &str,
        session_id: &str,
        model: Option<&str>,
    ) -> Result<Option<(i64, i64)>, String> {
        let Some(scope) = self.account_scope(user_id) else {
            return Ok(None);
        };
        let limit: i64 = match std::env::var("MAPLE_CONTEXT_LIMIT")
            .ok()
            .and_then(|value| value.parse().ok())
        {
            Some(limit) if limit > 0 => limit,
            _ => match model {
                Some(model) => self
                    .service
                    .handle_for_user(user_id)
                    .await?
                    .context_limit_for_model(model)
                    .await?
                    .unwrap_or(200_000),
                None => 200_000,
            },
        };
        let db = crate::backend::account_session_db(&scope);
        let mut guard = self.usage_db.lock().unwrap_or_else(|e| e.into_inner());
        if guard.as_ref().map(|(path, _)| path != &db).unwrap_or(true) {
            let Ok(conn) = rusqlite::Connection::open(&db) else {
                return Ok(None);
            };
            *guard = Some((db, conn));
        }
        let conn = &guard.as_ref().expect("usage db opened above").1;
        let row = conn
            .query_row(
                "SELECT COALESCE(input_tokens,0) + COALESCE(cache_read_tokens,0) \
                 + COALESCE(cache_write_tokens,0) FROM usage_ledger \
                 WHERE session_id = ?1 AND is_compaction = 0 \
                 ORDER BY id DESC LIMIT 1",
                [session_id],
                |row| row.get::<_, i64>(0),
            )
            .ok();
        Ok(row.map(|tokens| (tokens, limit)))
    }

    /// Deliver the user's answer to an ask_user question. Returns false
    /// when no question was pending.
    pub async fn answer_question(
        &self,
        user_id: &str,
        request_id: &str,
        answer: String,
    ) -> Result<bool, String> {
        let _service = self.service.clone();
        let request_id = request_id.to_string();
        self.service
            .handle_for_user(user_id)
            .await?
            .answer_question_via_handle(&request_id, answer)
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

    /// Standard start request for this app: the saved project root (see
    /// `start_runtime`) with the configured model and the SmartApprove policy.
    pub fn default_start_request(&self) -> AgentStartRequest {
        AgentStartRequest {
            project_root: None,
            model: std::env::var("MAPLE_MODEL").ok(),
            mode: None,
        }
    }

    /// Persist the UI's model choice as the account's default model.
    pub async fn save_default_model(&self, user_id: &str, model: String) -> Result<(), String> {
        let handle = self.service.handle_for_user(user_id).await?;
        let mut config = handle.load_config().await?;
        config.default_model = model;
        handle.save_config(config).await
    }

    /// Model the UI should select initially: MAPLE_MODEL when set.
    pub fn configured_model(&self) -> Option<String> {
        std::env::var("MAPLE_MODEL").ok()
    }

    /// The account's saved default model, if any.
    pub async fn saved_model(&self, user_id: &str) -> Option<String> {
        let handle = self.service.handle_for_user(user_id).await.ok()?;
        let config = handle.load_config().await.ok()?;
        Some(config.default_model).filter(|model| !model.is_empty())
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
