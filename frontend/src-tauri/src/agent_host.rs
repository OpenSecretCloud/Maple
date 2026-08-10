use crate::agent::{
    AgentPathLayout, AgentRuntimeHandle, AgentRuntimeStatus, AgentStartRequest,
    MapleAgentHostResources, MapleAgentService, SafeguardStartup,
};
use crate::maple_api::MapleApiSession;
use serde::Serialize;
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

/// Result of a runtime mutation that also had to clean up the ACP edge.
///
/// Runtime success is authoritative even when ACP cleanup reports an error.
/// Keeping both facts lets Desktop resynchronize its runtime state while
/// security-sensitive callers can still fail closed on the ACP warning.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentRuntimeLifecycleOutcome {
    pub status: AgentRuntimeStatus,
    pub acp_shutdown_error: Option<String>,
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
    ) -> Result<AgentRuntimeLifecycleOutcome, String> {
        let _guard = self.lock().await;
        handle.verify_generation().await?;
        let acp_result =
            crate::agent_acp::shutdown_agent_acp_locked(app_handle, Some(user_id)).await;
        let runtime_result = handle.stop().await;
        combine_runtime_lifecycle_results(acp_result, runtime_result, "stop")
    }

    pub(crate) async fn restart_runtime(
        &self,
        app_handle: &AppHandle,
        user_id: &str,
        handle: &AgentRuntimeHandle,
        api_session: Arc<MapleApiSession>,
        request: Option<AgentStartRequest>,
    ) -> Result<AgentRuntimeLifecycleOutcome, String> {
        let _guard = self.lock().await;
        handle.verify_generation().await?;
        let acp_result =
            crate::agent_acp::shutdown_agent_acp_locked(app_handle, Some(user_id)).await;
        let runtime_result = handle.restart(api_session, request).await;
        combine_runtime_lifecycle_results(acp_result, runtime_result, "restart")
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
        let result = combine_surface_results(acp_result, runtime_result, "stop");
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

fn combine_surface_results<T>(
    acp_result: Result<(), String>,
    runtime_result: Result<T, String>,
    runtime_action: &str,
) -> Result<T, String> {
    match (acp_result, runtime_result) {
        (Ok(()), Ok(runtime)) => Ok(runtime),
        (Err(acp), Ok(_)) => Err(acp),
        (Ok(()), Err(runtime)) => Err(runtime),
        (Err(acp), Err(runtime)) => Err(format!(
            "Failed to stop ACP ({acp}); failed to {runtime_action} Agent Mode ({runtime})"
        )),
    }
}

fn combine_runtime_lifecycle_results(
    acp_result: Result<(), String>,
    runtime_result: Result<AgentRuntimeStatus, String>,
    runtime_action: &str,
) -> Result<AgentRuntimeLifecycleOutcome, String> {
    match (acp_result, runtime_result) {
        (acp_result, Ok(status)) => Ok(AgentRuntimeLifecycleOutcome {
            status,
            acp_shutdown_error: acp_result.err(),
        }),
        (Ok(()), Err(runtime)) => Err(runtime),
        (Err(acp), Err(runtime)) => Err(format!(
            "Failed to stop ACP ({acp}); failed to {runtime_action} Agent Mode ({runtime})"
        )),
    }
}

/// Compose Maple's transport-neutral runtime with its two edge adapters.
///
/// Tauri owns Desktop event projection; ACP owns its transient environment
/// policy. Neither adapter reaches through the other to operate the runtime.
pub(crate) fn build_service(
    app_handle: &AppHandle,
    safeguard_startup: SafeguardStartup,
) -> Result<MapleAgentService, String> {
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
        safeguard_startup,
    )))
}

#[cfg(test)]
mod tests {
    use super::{
        combine_runtime_lifecycle_results, combine_surface_results, AgentRuntimeLifecycleOutcome,
    };
    use crate::agent::AgentRuntimeStatus;
    use std::collections::HashMap;

    fn runtime_status(running: bool) -> AgentRuntimeStatus {
        AgentRuntimeStatus {
            running,
            project_root: None,
            model: None,
            mode: None,
            active_runs: HashMap::new(),
        }
    }

    #[test]
    fn surface_results_return_the_runtime_value_when_both_succeed() {
        assert_eq!(
            combine_surface_results(Ok(()), Ok("runtime status"), "restart"),
            Ok("runtime status")
        );
    }

    #[test]
    fn surface_results_preserve_a_single_error() {
        assert_eq!(
            combine_surface_results::<()>(Err("acp error".into()), Ok(()), "stop"),
            Err("acp error".into())
        );
        assert_eq!(
            combine_surface_results::<()>(Ok(()), Err("runtime error".into()), "stop"),
            Err("runtime error".into())
        );
    }

    #[test]
    fn surface_results_report_both_errors_and_the_runtime_action() {
        assert_eq!(
            combine_surface_results::<()>(
                Err("acp error".into()),
                Err("runtime error".into()),
                "restart",
            ),
            Err(
                "Failed to stop ACP (acp error); failed to restart Agent Mode (runtime error)"
                    .into()
            )
        );
    }

    #[test]
    fn runtime_lifecycle_reports_acp_partial_failure_with_runtime_status() {
        let status = runtime_status(true);
        let outcome = combine_runtime_lifecycle_results(
            Err("acp error".into()),
            Ok(status.clone()),
            "restart",
        )
        .expect("runtime success should remain observable");

        assert_eq!(outcome.status.running, status.running);
        assert_eq!(outcome.acp_shutdown_error.as_deref(), Some("acp error"));
    }

    #[test]
    fn runtime_lifecycle_reports_runtime_failures_strictly() {
        assert_eq!(
            combine_runtime_lifecycle_results(Ok(()), Err("runtime error".into()), "stop"),
            Err("runtime error".into())
        );
        assert_eq!(
            combine_runtime_lifecycle_results(
                Err("acp error".into()),
                Err("runtime error".into()),
                "restart",
            ),
            Err(
                "Failed to stop ACP (acp error); failed to restart Agent Mode (runtime error)"
                    .into()
            )
        );
    }

    #[test]
    fn runtime_lifecycle_success_has_no_cleanup_warning() {
        let AgentRuntimeLifecycleOutcome {
            status,
            acp_shutdown_error,
        } = combine_runtime_lifecycle_results(Ok(()), Ok(runtime_status(false)), "stop")
            .expect("both surfaces should succeed");

        assert!(!status.running);
        assert!(acp_shutdown_error.is_none());
    }
}
