use crate::agent::{
    AgentPathLayout, AgentRuntimeHandle, AgentRuntimeStatus, AgentStartRequest,
    MapleAgentHostResources, MapleAgentService,
};
use crate::maple_api::MapleApiSession;
use std::sync::Arc;
use tauri::{AppHandle, Manager};
use tokio::sync::{Mutex, MutexGuard};

/// Serializes operations that span both Agent surfaces.
///
/// Maple's core runtime has its own internal lifecycle lock, while ACP also
/// owns listener and connection state. Composite host operations such as
/// stop, restart, clear, and app exit must cover both or an ACP start can slip
/// between the two phases and retain a stale runtime handle.
pub(crate) struct AgentHostLifecycle {
    gate: Mutex<()>,
}

impl AgentHostLifecycle {
    pub(crate) fn new() -> Self {
        Self {
            gate: Mutex::new(()),
        }
    }

    pub(crate) async fn lock(&self) -> MutexGuard<'_, ()> {
        self.gate.lock().await
    }

    pub(crate) async fn start_runtime(
        &self,
        handle: &AgentRuntimeHandle,
        api_session: Arc<MapleApiSession>,
        request: Option<AgentStartRequest>,
    ) -> Result<AgentRuntimeStatus, String> {
        let _guard = self.lock().await;
        handle.verify_generation().await?;
        handle.start(api_session, request).await
    }

    pub(crate) async fn stop_runtime(
        &self,
        app_handle: &AppHandle,
        user_id: &str,
        handle: &AgentRuntimeHandle,
    ) -> Result<AgentRuntimeStatus, String> {
        let _guard = self.lock().await;
        handle.verify_generation().await?;
        crate::agent_acp::shutdown_agent_acp_locked(app_handle, Some(user_id)).await?;
        handle.stop().await
    }

    pub(crate) async fn restart_runtime(
        &self,
        app_handle: &AppHandle,
        user_id: &str,
        handle: &AgentRuntimeHandle,
        api_session: Arc<MapleApiSession>,
        request: Option<AgentStartRequest>,
    ) -> Result<AgentRuntimeStatus, String> {
        let _guard = self.lock().await;
        handle.verify_generation().await?;
        crate::agent_acp::shutdown_agent_acp_locked(app_handle, Some(user_id)).await?;
        handle.restart(api_session, request).await
    }

    pub(crate) async fn clear_user_data(
        &self,
        app_handle: &AppHandle,
        user_id: &str,
        handle: &AgentRuntimeHandle,
    ) -> Result<(), String> {
        let _guard = self.lock().await;
        handle.verify_generation().await?;
        crate::agent_acp::shutdown_agent_acp_locked(app_handle, Some(user_id)).await?;
        handle.clear_data().await?;
        crate::agent_acp::clear_agent_acp_config(app_handle, user_id)
    }

    pub(crate) async fn clear_user_history(
        &self,
        app_handle: &AppHandle,
        user_id: &str,
        handle: &AgentRuntimeHandle,
    ) -> Result<(), String> {
        let _guard = self.lock().await;
        handle.verify_generation().await?;
        crate::agent_acp::shutdown_agent_acp_locked(app_handle, Some(user_id)).await?;
        handle.clear_history().await
    }

    async fn shutdown_services(
        &self,
        app_handle: &AppHandle,
        reopen_on_error: bool,
    ) -> Result<(), String> {
        let _guard = self.lock().await;
        let service = app_handle.state::<MapleAgentService>();
        service.begin_draining();
        let acp_result = crate::agent_acp::shutdown_agent_acp_locked(app_handle, None).await;
        let runtime_result = service.shutdown_all().await;
        let result = match (acp_result, runtime_result) {
            (Ok(()), Ok(())) => Ok(()),
            (Err(acp), Ok(())) => Err(acp),
            (Ok(()), Err(runtime)) => Err(runtime),
            (Err(acp), Err(runtime)) => Err(format!(
                "Failed to stop ACP ({acp}); failed to stop Agent Mode ({runtime})"
            )),
        };
        if reopen_on_error && result.is_err() {
            service.reopen_after_failed_shutdown();
        }
        result
    }

    pub(crate) async fn shutdown_for_exit(&self, app_handle: &AppHandle) -> Result<(), String> {
        self.shutdown_services(app_handle, false).await
    }

    pub(crate) async fn shutdown_for_update(&self, app_handle: &AppHandle) -> Result<(), String> {
        self.shutdown_services(app_handle, true).await
    }
}

/// Compose Maple's transport-neutral runtime with its two edge adapters.
///
/// Tauri owns Desktop event projection; ACP owns its transient environment
/// policy. Neither adapter reaches through the other to operate the runtime.
pub(crate) fn build_service(app_handle: &AppHandle) -> Result<MapleAgentService, String> {
    let app_config_root = app_handle
        .path()
        .app_config_dir()
        .map_err(|error| format!("Failed to resolve Maple config directory: {error}"))?;
    let app_local_data_root = app_handle
        .path()
        .app_local_data_dir()
        .map_err(|error| format!("Failed to resolve Maple local data directory: {error}"))?;
    let paths = AgentPathLayout::from_app_roots(app_config_root, app_local_data_root);
    let event_sink = crate::agent_tauri::event_sink(app_handle);
    let default_tool_context = crate::agent_acp::default_tool_context_spec()?;
    Ok(MapleAgentService::new(MapleAgentHostResources::new(
        paths,
        event_sink,
        default_tool_context,
    )))
}
