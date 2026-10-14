#![cfg_attr(not(feature = "desktop"), allow(dead_code))]

//! Desktop and CLI facade for the profile's shared local host.
//! The host implementation retains the existing backend operations in `local`;
//! only this module knows about IPC. GPUI never owns a second agent runtime.

#[cfg_attr(not(feature = "desktop"), allow(unused_imports))]
pub use local::*;
pub(crate) mod local;

use crate::host::{RemoteClient, protocol::Request};
use maple_agent::agent::*;
use serde::{Serialize, de::DeserializeOwned};
use std::{collections::HashMap, future::Future, sync::Arc};
use tokio::{runtime::Runtime, sync::mpsc};

pub struct AgentBackend {
    runtime: tokio::runtime::Handle,
    owned_runtime: Option<Runtime>,
    client: BackendClient,
    api_url: String,
}

impl Drop for AgentBackend {
    fn drop(&mut self) {
        // A completing UI/IPC future may release the last Arc on a Tokio
        // worker. Runtime::drop would panic there by trying to block. The
        // frontend owns no agent runs; host shutdown remains explicit.
        if let Some(runtime) = self.owned_runtime.take() {
            runtime.shutdown_background();
        }
    }
}

enum BackendClient {
    Host(RemoteClient),
    #[cfg(test)]
    InProcess(Box<LocalBackend>),
}

// Borrowed facade parameters become owned messages at the process boundary.
macro_rules! wire_type {
    ((&str)) => { String };
    ((Option<&str>)) => { Option<String> };
    (($ty:ty)) => { $ty };
}
macro_rules! local_arg {
    ($value:ident, (&str)) => {
        &$value
    };
    ($value:ident, (Option<&str>)) => {
        $value.as_deref()
    };
    ($value:ident, ($ty:ty)) => {
        $value
    };
}
macro_rules! authorize_arg {
    ($backend:ident, user_id, $value:ident) => {
        $backend.authorize_account(&$value)?;
    };
    ($backend:ident, $name:ident, $value:ident) => {};
}
macro_rules! operations {
    ($( $name:ident ( $( $arg:ident : $ty:tt ),* ) -> $result:ty; )*) => {
        impl AgentBackend {
            $(pub async fn $name(&self, $($arg: unpack_type!($ty)),*) -> Result<$result, String> {
                self.call(stringify!($name), ($($arg,)* )).await
            })*
        }
        async fn dispatch_inner(backend: &LocalBackend, request: Request) -> Result<serde_json::Value, String> {
            match request.method.as_str() {
                $(stringify!($name) => {
                    let ($($arg,)*): ($(wire_type!($ty),)*) = serde_json::from_value(request.params)
                        .map_err(|_| "Invalid host request parameters".to_string())?;
                    $(authorize_arg!(backend, $arg, $arg);)*
                    let value = backend.$name($(local_arg!($arg, $ty)),*).await?;
                    serde_json::to_value(value).map_err(|_| "Could not encode host response".to_string())
                })*
                _ => dispatch_special(backend, request).await,
            }
        }
    }
}
macro_rules! unpack_type {
    (($ty:ty)) => {
        $ty
    };
}

operations! {
    login(email: (String), password: (String)) -> AuthSession;
    logout_and_clear(user_id: (&str)) -> ();
    oauth_start(provider: (OAuthProvider)) -> String;
    oauth_wait_for_handoff(provider: (OAuthProvider)) -> AuthSession;
    oauth_complete(provider: (OAuthProvider), redirected_url: (String)) -> AuthSession;
    account(user_id: (&str)) -> MapleAccount;
    request_verification_email(user_id: (&str)) -> ();
    change_password(user_id: (&str), current_password: (String), new_password: (String)) -> ();
    request_account_deletion(user_id: (&str)) -> String;
    confirm_account_deletion(user_id: (&str), confirmation_code: (String), plaintext_secret: (String)) -> ();
    list_api_keys(user_id: (&str)) -> Vec<MapleApiKey>;
    create_api_key(user_id: (&str), name: (String)) -> MapleApiKeyCreated;
    delete_api_key(user_id: (&str), name: (&str)) -> ();
    plan_usage(user_id: (&str)) -> Option<crate::billing::PlanUsage>;
    billing_status(user_id: (&str)) -> crate::billing::BillingStatus;
    billing_products() -> Vec<crate::billing::Product>;
    open_billing_portal(user_id: (&str)) -> ();
    start_checkout(user_id: (&str), product_id: (String)) -> ();
    start_runtime(user_id: (&str), request: (Option<AgentStartRequest>)) -> AgentRuntimeStatus;
    stop_runtime(user_id: (&str)) -> AgentRuntimeStatus;
    recent_project_roots(user_id: (&str)) -> Vec<RecentProjectRoot>;
    select_project_root(user_id: (&str), path: (String)) -> AgentProjectRootRegistration;
    list_sessions(user_id: (&str), project_root: (Option<String>)) -> Vec<AgentSessionSummary>;
    local_bootstrap(user_id: (&str)) -> LocalBootstrap;
    recovery_snapshot(user_id: (&str), session_id: (Option<String>)) -> RecoverySnapshot;
    create_session(user_id: (&str), request: (Option<AgentCreateSessionRequest>)) -> AgentSessionDetail;
    load_session(user_id: (&str), session_id: (&str)) -> AgentSessionDetail;
    set_session_state(user_id: (&str), session_id: (&str), state: (AgentTaskState)) -> AgentSessionSummary;
    delete_session(user_id: (&str), session_id: (&str)) -> ();
    remove_project_root(user_id: (&str), path: (String), fallback: (Option<String>)) -> ();
    rename_session(user_id: (&str), session_id: (&str), title: (String)) -> AgentSessionSummary;
    project_trust(user_id: (&str), path: (String)) -> AgentProjectTrustStatus;
    set_project_trust(user_id: (&str), path: (String), trusted: (bool)) -> AgentProjectTrustStatus;
    cancel_queued_message(user_id: (&str), session_id: (&str), queue_id: (&str)) -> AgentDesktopQueueSnapshot;
    begin_queued_message_edit(user_id: (&str), session_id: (&str), queue_id: (&str)) -> ();
    end_queued_message_edit(user_id: (&str), session_id: (&str), queue_id: (&str)) -> ();
    send_message(user_id: (&str), request: (AgentSendMessageRequest)) -> String;
    read_image_attachment(user_id: (&str), session_id: (&str), attachment_id: (&str)) -> Vec<u8>;
    cancel_run(user_id: (&str), run_id: (&str)) -> ();
    available_model_ids(user_id: (&str)) -> Vec<String>;
    model_catalog_entry(user_id: (&str), model: (&str)) -> Option<maple_agent::agent::CatalogEntry>;
    audio_capabilities(user_id: (&str)) -> maple_agent::agent::AudioCapabilities;
    synthesize_speech(user_id: (&str), text: (String), voice: (String), speed: (f32)) -> Vec<u8>;
    transcribe_audio(user_id: (&str), wav: (Vec<u8>)) -> String;
    list_session_mcp_servers(user_id: (&str), session_id: (&str)) -> Vec<maple_agent::agent::AgentSessionMcpServer>;
    set_session_mcp_server_enabled(user_id: (&str), session_id: (&str), name: (&str), kind: (maple_agent::agent::AgentSessionIntegrationKind), enabled: (bool)) -> Vec<maple_agent::agent::AgentSessionMcpServer>;
    list_mcp_servers(user_id: (&str)) -> Vec<maple_agent::agent::AgentMcpServer>;
    list_project_mcp_servers(user_id: (&str), project_root: (&str)) -> Vec<maple_agent::agent::AgentMcpServer>;
    set_project_mcp_server_enabled(user_id: (&str), project_root: (&str), name: (&str), enabled: (bool)) -> ();
    list_integrations(user_id: (&str)) -> Vec<AgentIntegration>;
    set_integration_enabled(user_id: (&str), id: (&str), enabled: (bool)) -> Vec<AgentIntegration>;
    setup_integration(user_id: (&str), id: (&str), permissions: (AgentIntegrationPermissions)) -> Vec<AgentIntegration>;
    save_mcp_servers(user_id: (&str), servers: (Vec<maple_agent::agent::AgentMcpServer>)) -> Vec<maple_agent::agent::AgentMcpServer>;
    set_session_web_enabled(user_id: (&str), session_id: (&str), enabled: (bool)) -> AgentSessionSummary;
    compact_session(user_id: (&str), session_id: (&str)) -> ();
    session_subagents(user_id: (&str), session_id: (&str)) -> Vec<AgentSubagent>;
    cancel_external_agent(user_id: (&str), session_id: (&str), agent_id: (&str)) -> ();
    list_slash_commands(user_id: (&str), working_dir: (Option<String>)) -> Vec<AgentSlashCommand>;
    resolve_slash_command(user_id: (&str), working_dir: (Option<String>), command: (String), args: (String)) -> Option<String>;
    summarize_tool_call(user_id: (&str), session_id: (&str), tool_name: (String), input: (Option<serde_json::Value>), output_text: (String)) -> Option<String>;
    summarize_thinking(user_id: (&str), session_id: (&str), thinking_text: (String)) -> Option<String>;
    ask_side_question(user_id: (&str), session_id: (&str), request_id: (String), prior: (Vec<maple_agent::agent::SideQuestionTurn>), question: (String)) -> ();
    context_usage(user_id: (&str), session_id: (&str), model: (Option<&str>)) -> Option<(i64, i64)>;
    answer_question(user_id: (&str), request_id: (&str), answer: (String)) -> bool;
    thinking_state(user_id: (&str), session_id: (Option<&str>), model: (&str)) -> maple_agent::agent::AgentThinkingState;
    set_thinking_level(user_id: (&str), session_id: (Option<&str>), model: (&str), level: (maple_agent::agent::ThinkingLevel)) -> maple_agent::agent::AgentThinkingState;
    save_default_model(user_id: (&str), model: (String)) -> ();
}

impl AgentBackend {
    /// UI state tests have no authenticated host and must not launch processes
    /// or attach to a developer's session. Runtime/IPC contracts are tested separately.
    #[cfg(test)]
    pub fn for_test(api_url: String, instructions: String) -> Result<Self, String> {
        let runtime = Runtime::new().map_err(|e| e.to_string())?;
        let guard = runtime.enter();
        let mut backend = LocalBackend::new(api_url.clone(), instructions)?;
        backend.ui_fixture = true;
        drop(guard);
        Ok(Self {
            runtime: runtime.handle().clone(),
            owned_runtime: Some(runtime),
            client: BackendClient::InProcess(Box::new(backend)),
            api_url,
        })
    }

    pub fn new(api_url: String, _harness_instructions: String) -> Result<Self, String> {
        let api_url = maple_agent::maple_api::validate_api_url(&api_url)?;
        let runtime = Runtime::new().map_err(|e| format!("Could not start host client: {e}"))?;
        let client = runtime.block_on(RemoteClient::connect(&api_url))?;
        Ok(Self {
            runtime: runtime.handle().clone(),
            owned_runtime: Some(runtime),
            client: BackendClient::Host(client),
            api_url,
        })
    }

    async fn call<R: DeserializeOwned>(
        &self,
        method: &str,
        params: impl Serialize,
    ) -> Result<R, String> {
        match &self.client {
            BackendClient::Host(client) => client.call(method, params).await,
            #[cfg(test)]
            BackendClient::InProcess(backend) => {
                let value = dispatch_inner(
                    backend,
                    Request {
                        id: 0,
                        method: method.into(),
                        send_id: None,
                        params: serde_json::to_value(params).map_err(|e| e.to_string())?,
                    },
                )
                .await?;
                serde_json::from_value(value).map_err(|e| e.to_string())
            }
        }
    }
    pub fn runtime_handle(&self) -> tokio::runtime::Handle {
        self.runtime.clone()
    }
    pub fn spawn<F>(&self, future: F) -> tokio::task::JoinHandle<F::Output>
    where
        F: Future + Send + 'static,
        F::Output: Send + 'static,
    {
        self.runtime.spawn(future)
    }
    pub async fn take_events(&self) -> Option<mpsc::Receiver<AgentServiceEvent>> {
        match &self.client {
            BackendClient::Host(client) => client.take_events().await,
            #[cfg(test)]
            BackendClient::InProcess(backend) => {
                let mut source = backend.take_events().await?;
                let (sender, receiver) = mpsc::channel(256);
                self.spawn(async move {
                    while let Some(event) = source.recv().await {
                        if sender.send(event).await.is_err() {
                            break;
                        }
                    }
                });
                Some(receiver)
            }
        }
    }
    pub fn api_url(&self) -> &str {
        &self.api_url
    }

    pub fn saved_user_id(&self) -> Option<String> {
        match &self.client {
            BackendClient::Host(client) => client.user_id(),
            #[cfg(test)]
            BackendClient::InProcess(_) => None,
        }
    }
    pub fn detach(self: &Arc<Self>) -> tokio::task::JoinHandle<()> {
        let backend = self.clone();
        self.spawn(async move {
            match &backend.client {
                BackendClient::Host(client) => client.detach().await,
                #[cfg(test)]
                BackendClient::InProcess(_) => {}
            }
        })
    }
    pub fn uses_hosted_oauth(&self) -> bool {
        crate::profile::AUTH_ORIGIN.is_some()
    }
    pub fn configured_model(&self) -> Option<String> {
        std::env::var("MAPLE_MODEL").ok()
    }
    pub fn default_start_request(&self) -> AgentStartRequest {
        AgentStartRequest {
            project_root: None,
            model: self.configured_model(),
        }
    }

    pub fn restore_in_background(self: &Arc<Self>) -> tokio::task::JoinHandle<RestoreOutcome> {
        let backend = self.clone();
        self.spawn(async move {
            backend
                .call("restore_auth", ())
                .await
                .unwrap_or(RestoreOutcome::Unavailable)
        })
    }
    pub fn set_harness_instructions(self: &Arc<Self>, instructions: String) {
        let backend = self.clone();
        self.spawn(async move {
            if let Err(error) = backend
                .call::<()>("set_harness_instructions", (instructions,))
                .await
            {
                log::warn!("Could not update host instructions: {error}");
            }
        });
    }
    pub fn cancel_oauth(self: &Arc<Self>) {
        let backend = self.clone();
        self.spawn(async move {
            let _ = backend.call::<()>("cancel_oauth", ()).await;
        });
    }
    pub fn begin_integration_setup(&self, id: &str) -> Result<AgentIntegrationPermissions, String> {
        // Native permission prompts remain on the signed app's UI thread.
        maple_agent::agent::begin_integration_setup(&AgentSetupIntegrationRequest { id: id.into() })
    }
    pub fn load_tool_summaries_blocking(
        &self,
        user_id: &str,
        session_id: &str,
    ) -> Result<HashMap<String, String>, String> {
        self.runtime
            .block_on(self.call("load_tool_summaries", (user_id, session_id)))
    }
    pub fn store_tool_summary_blocking(
        &self,
        user_id: &str,
        session_id: &str,
        item_id: &str,
        summary: &str,
    ) -> Result<(), String> {
        self.runtime.block_on(self.call(
            "store_tool_summary",
            (user_id, session_id, item_id, summary),
        ))
    }
    pub async fn saved_model(&self, user_id: &str) -> Option<String> {
        self.call("saved_model", (user_id,)).await.ok().flatten()
    }
}

async fn dispatch_special(
    backend: &LocalBackend,
    request: Request,
) -> Result<serde_json::Value, String> {
    let invalid = |_| "Invalid host request parameters".to_string();
    let encode = |value| {
        serde_json::to_value(value).map_err(|_| "Could not encode host response".to_string())
    };
    match request.method.as_str() {
        "restore_auth" => {
            serde_json::to_value(backend.validate_persisted_auth().await).map_err(|e| e.to_string())
        }
        "cancel_oauth" => {
            backend.cancel_oauth();
            Ok(serde_json::Value::Null)
        }
        "set_harness_instructions" => {
            let (instructions,): (String,) =
                serde_json::from_value(request.params).map_err(invalid)?;
            backend.set_harness_instructions(instructions);
            Ok(serde_json::Value::Null)
        }
        "saved_model" => {
            let (user_id,): (String,) = serde_json::from_value(request.params).map_err(invalid)?;
            backend.authorize_account(&user_id)?;
            serde_json::to_value(backend.saved_model(&user_id).await).map_err(|e| e.to_string())
        }
        "load_tool_summaries" => {
            let (user_id, session_id): (String, String) =
                serde_json::from_value(request.params).map_err(invalid)?;
            backend.authorize_account(&user_id)?;
            encode(backend.load_tool_summaries_blocking(&user_id, &session_id)?)
        }
        "store_tool_summary" => {
            let (user_id, session_id, item_id, summary): (String, String, String, String) =
                serde_json::from_value(request.params).map_err(invalid)?;
            backend.authorize_account(&user_id)?;
            backend.store_tool_summary_blocking(&user_id, &session_id, &item_id, &summary)?;
            Ok(serde_json::Value::Null)
        }
        _ => Err("Unknown Maple host operation".into()),
    }
}

pub(crate) async fn dispatch(
    backend: &LocalBackend,
    request: Request,
) -> Result<serde_json::Value, String> {
    // Account switches cannot interleave with an operation admitted for the
    // previous account. Restoring that same account is a reader: its network
    // validation must not prevent local history, configuration, or stop calls.
    // The restore mutex still serializes credential validation/publication.
    // Cancelling an OAuth wait must remain independent.
    if request.method == "cancel_oauth" {
        return dispatch_inner(backend, request).await;
    }
    if matches!(
        request.method.as_str(),
        "login"
            | "logout_and_clear"
            | "oauth_wait_for_handoff"
            | "oauth_complete"
            | "confirm_account_deletion"
    ) {
        let _account = backend.account_gate.write().await;
        dispatch_inner(backend, request).await
    } else {
        let _account = backend.account_gate.read().await;
        dispatch_inner(backend, request).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn final_frontend_reference_can_be_dropped_by_an_async_callback() {
        let backend = tokio::task::spawn_blocking(|| {
            AgentBackend::for_test("http://127.0.0.1:1".into(), String::new()).unwrap()
        })
        .await
        .unwrap();
        tokio::spawn(async move {
            drop(backend);
        })
        .await
        .expect("frontend shutdown must not panic on a runtime worker");
    }
}
