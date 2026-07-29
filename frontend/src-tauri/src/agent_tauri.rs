use crate::agent::{
    AgentEventEnvelope, AgentEventSink, AgentPathLayout, MapleAgentHostResources,
    MapleAgentService, AGENT_EVENT_NAME,
};
use std::sync::Arc;
use tauri::{AppHandle, Emitter, Manager};

struct TauriAgentEventSink {
    app_handle: AppHandle,
}

impl AgentEventSink for TauriAgentEventSink {
    fn emit(&self, event: &AgentEventEnvelope) {
        if let Err(error) = self.app_handle.emit(AGENT_EVENT_NAME, event) {
            log::warn!("Failed to emit Agent Mode event: {error}");
        }
    }
}

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
    let event_sink = Arc::new(TauriAgentEventSink {
        app_handle: app_handle.clone(),
    });
    Ok(MapleAgentService::new(MapleAgentHostResources::new(
        paths, event_sink,
    )))
}
