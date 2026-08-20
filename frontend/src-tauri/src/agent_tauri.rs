use crate::agent::{
    AgentConfig, AgentCreateSessionRequest, AgentDesktopQueueSnapshot, AgentEventSink,
    AgentMcpServer, AgentPermissionModeRequest, AgentPermissionResponse,
    AgentProjectRootRegistration, AgentProjectTrustStatus, AgentQueueControlRequest,
    AgentQueueUpdateRequest, AgentQueuedMessage, AgentRenameSessionRequest, AgentRunEvent,
    AgentRunResponse, AgentRunTerminal, AgentRuntimeHandle, AgentRuntimeStatus,
    AgentSendMessageRequest, AgentServiceEvent, AgentSessionDetail, AgentSessionMcpServer,
    AgentSessionSummary, AgentSetSessionMcpServerRequest, AgentStartRequest, AgentTimelineItem,
    MapleAgentService, RecentProjectRoot,
};
use crate::agent_host::{AgentHostLifecycle, AgentRuntimeLifecycleOutcome};
use crate::maple_api::MapleApiAuthState;
use serde::Serialize;
use std::sync::Arc;
use tauri::{AppHandle, Emitter, State};

const AGENT_EVENT_NAME: &str = "agent-event";

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct AgentEventEnvelope {
    pub(crate) event_type: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) session_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) run_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) item: Option<AgentTimelineItem>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) status: Option<AgentRuntimeStatus>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) session: Option<AgentSessionSummary>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) message: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) queue: Option<AgentDesktopQueueSnapshot>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) promoted_queue_id: Option<String>,
}

pub(crate) fn project_agent_event(event: &AgentServiceEvent) -> AgentEventEnvelope {
    let mut envelope = AgentEventEnvelope {
        event_type: String::new(),
        session_id: None,
        run_id: None,
        item: None,
        status: None,
        session: None,
        message: None,
        queue: None,
        promoted_queue_id: None,
    };
    match event {
        AgentServiceEvent::RuntimeStatus(status) => {
            envelope.event_type = "runtimeStatus".to_string();
            envelope.status = Some(status.clone());
        }
        AgentServiceEvent::SessionCreated(session) => {
            envelope.event_type = "sessionCreated".to_string();
            envelope.session_id = Some(session.id.clone());
            envelope.session = Some(session.clone());
        }
        AgentServiceEvent::SessionUpdated {
            session_id,
            run_id,
            session,
        } => {
            envelope.event_type = "sessionUpdated".to_string();
            envelope.session_id = Some(session_id.clone());
            envelope.run_id.clone_from(run_id);
            envelope.session = Some(session.clone());
        }
        AgentServiceEvent::TimelineItem {
            session_id,
            run_id,
            item,
        } => {
            envelope.event_type = "timelineItem".to_string();
            envelope.session_id = Some(session_id.clone());
            envelope.run_id.clone_from(run_id);
            envelope.item = Some(item.clone());
        }
        AgentServiceEvent::Run {
            session_id,
            run_id,
            event,
        } => {
            envelope.session_id = Some(session_id.clone());
            envelope.run_id = Some(run_id.clone());
            match event {
                AgentRunEvent::SessionUpdated(session) => {
                    envelope.event_type = "sessionUpdated".to_string();
                    envelope.session = Some(session.clone());
                }
                AgentRunEvent::Started => envelope.event_type = "runStarted".to_string(),
                AgentRunEvent::TimelineItem(item) => {
                    envelope.event_type = "timelineItem".to_string();
                    envelope.item = Some(item.clone());
                }
                AgentRunEvent::PermissionRequested { item, .. } => {
                    // Keep the Desktop wire contract unchanged while the shared
                    // service exposes a typed permission request to non-Tauri
                    // callers through the run-local event stream.
                    envelope.event_type = "timelineItem".to_string();
                    envelope.item = Some(item.clone());
                }
                AgentRunEvent::SetupWarning(message) => {
                    envelope.event_type = "error".to_string();
                    // Preserve the existing Desktop contract. Setup warnings
                    // historically carried a run ID but no task ID.
                    envelope.session_id = None;
                    envelope.message = Some(message.clone());
                }
                AgentRunEvent::HistoryReplaced => {
                    envelope.event_type = "historyReplaced".to_string();
                }
                AgentRunEvent::Error(item) => {
                    envelope.event_type = "error".to_string();
                    envelope.item = Some(item.clone());
                }
                AgentRunEvent::Finished(terminal) => {
                    envelope.event_type = "runFinished".to_string();
                    envelope.message = Some(
                        match terminal {
                            AgentRunTerminal::Completed => "completed",
                            AgentRunTerminal::Cancelled => "cancelled",
                            AgentRunTerminal::Failed => "failed",
                        }
                        .to_string(),
                    );
                }
                AgentRunEvent::QueueChanged(snapshot) => {
                    envelope.event_type = "queueChanged".to_string();
                    envelope.queue = Some(snapshot.clone());
                }
                AgentRunEvent::QueuePromoted {
                    snapshot,
                    queue_id,
                    item,
                } => {
                    envelope.event_type = "queuePromoted".to_string();
                    envelope.queue = Some(snapshot.clone());
                    envelope.promoted_queue_id = Some(queue_id.clone());
                    envelope.item = Some(item.clone());
                }
            }
        }
    }
    envelope
}

struct TauriAgentEventSink {
    app_handle: AppHandle,
}

impl AgentEventSink for TauriAgentEventSink {
    fn emit(&self, event: &AgentServiceEvent) {
        if let Err(error) = self
            .app_handle
            .emit(AGENT_EVENT_NAME, project_agent_event(event))
        {
            log::warn!("Failed to emit Agent Mode event: {error}");
        }
    }
}

pub(crate) fn event_sink(app_handle: &AppHandle) -> Arc<dyn AgentEventSink> {
    Arc::new(TauriAgentEventSink {
        app_handle: app_handle.clone(),
    })
}

async fn handle_for_user(
    state: &State<'_, MapleAgentService>,
    user_id: &str,
) -> Result<AgentRuntimeHandle, String> {
    state.handle_for_user(user_id).await
}

#[tauri::command]
pub async fn agent_get_runtime_status(
    state: State<'_, MapleAgentService>,
    user_id: String,
) -> Result<AgentRuntimeStatus, String> {
    handle_for_user(&state, &user_id).await?.status().await
}

#[tauri::command]
pub async fn agent_start_runtime(
    app_handle: AppHandle,
    state: State<'_, MapleAgentService>,
    api_auth_state: State<'_, MapleApiAuthState>,
    host_lifecycle: State<'_, AgentHostLifecycle>,
    user_id: String,
    request: Option<AgentStartRequest>,
) -> Result<AgentRuntimeStatus, String> {
    let _ = app_handle;
    let handle = handle_for_user(&state, &user_id).await?;
    let api_session = api_auth_state.session_for(&user_id).await?;
    host_lifecycle
        .start_runtime(&handle, api_session, request)
        .await
}

#[tauri::command]
pub async fn agent_stop_runtime(
    app_handle: AppHandle,
    state: State<'_, MapleAgentService>,
    host_lifecycle: State<'_, AgentHostLifecycle>,
    user_id: String,
) -> Result<AgentRuntimeLifecycleOutcome, String> {
    let handle = handle_for_user(&state, &user_id).await?;
    host_lifecycle
        .stop_runtime(&app_handle, &user_id, &handle)
        .await
}

#[tauri::command]
pub async fn agent_restart_runtime(
    app_handle: AppHandle,
    state: State<'_, MapleAgentService>,
    api_auth_state: State<'_, MapleApiAuthState>,
    host_lifecycle: State<'_, AgentHostLifecycle>,
    user_id: String,
    request: Option<AgentStartRequest>,
) -> Result<AgentRuntimeLifecycleOutcome, String> {
    let handle = handle_for_user(&state, &user_id).await?;
    let api_session = api_auth_state.session_for(&user_id).await?;
    host_lifecycle
        .restart_runtime(&app_handle, &user_id, &handle, api_session, request)
        .await
}

#[tauri::command]
pub async fn agent_clear_user_data(
    app_handle: AppHandle,
    state: State<'_, MapleAgentService>,
    host_lifecycle: State<'_, AgentHostLifecycle>,
    user_id: String,
) -> Result<(), String> {
    let handle = handle_for_user(&state, &user_id).await?;
    host_lifecycle
        .clear_user_data(&app_handle, &user_id, &handle)
        .await
}

#[tauri::command]
pub async fn agent_clear_user_history(
    app_handle: AppHandle,
    state: State<'_, MapleAgentService>,
    host_lifecycle: State<'_, AgentHostLifecycle>,
    user_id: String,
) -> Result<(), String> {
    let handle = handle_for_user(&state, &user_id).await?;
    host_lifecycle
        .clear_user_history(&app_handle, &user_id, &handle)
        .await
}

#[tauri::command]
pub async fn agent_load_config(
    app_handle: AppHandle,
    state: State<'_, MapleAgentService>,
    user_id: String,
) -> Result<AgentConfig, String> {
    let _ = app_handle;
    handle_for_user(&state, &user_id).await?.load_config().await
}

#[tauri::command]
pub async fn agent_save_config(
    app_handle: AppHandle,
    state: State<'_, MapleAgentService>,
    user_id: String,
    config: AgentConfig,
) -> Result<(), String> {
    let _ = app_handle;
    handle_for_user(&state, &user_id)
        .await?
        .save_config(config)
        .await
}

#[tauri::command]
pub async fn agent_list_mcp_servers(
    app_handle: AppHandle,
    state: State<'_, MapleAgentService>,
    user_id: String,
) -> Result<Vec<AgentMcpServer>, String> {
    let _ = app_handle;
    handle_for_user(&state, &user_id)
        .await?
        .list_mcp_servers()
        .await
}

#[tauri::command]
pub async fn agent_save_mcp_servers(
    app_handle: AppHandle,
    state: State<'_, MapleAgentService>,
    user_id: String,
    servers: Vec<AgentMcpServer>,
) -> Result<Vec<AgentMcpServer>, String> {
    let _ = app_handle;
    handle_for_user(&state, &user_id)
        .await?
        .save_mcp_servers(servers)
        .await
}

#[tauri::command]
pub async fn agent_list_recent_project_roots(
    app_handle: AppHandle,
    state: State<'_, MapleAgentService>,
    user_id: String,
) -> Result<Vec<RecentProjectRoot>, String> {
    let _ = app_handle;
    handle_for_user(&state, &user_id)
        .await?
        .list_recent_project_roots()
        .await
}

#[tauri::command]
pub async fn agent_save_recent_project_root(
    app_handle: AppHandle,
    state: State<'_, MapleAgentService>,
    user_id: String,
    path: String,
) -> Result<AgentProjectRootRegistration, String> {
    let _ = app_handle;
    handle_for_user(&state, &user_id)
        .await?
        .save_recent_project_root(path)
        .await
}

#[tauri::command]
pub async fn agent_remove_project_root(
    app_handle: AppHandle,
    state: State<'_, MapleAgentService>,
    user_id: String,
    path: String,
    fallback_path: Option<String>,
) -> Result<AgentConfig, String> {
    let _ = app_handle;
    handle_for_user(&state, &user_id)
        .await?
        .remove_project_root(path, fallback_path)
        .await
}

#[tauri::command]
pub async fn agent_get_project_trust(
    app_handle: AppHandle,
    state: State<'_, MapleAgentService>,
    user_id: String,
    path: String,
) -> Result<AgentProjectTrustStatus, String> {
    let _ = app_handle;
    handle_for_user(&state, &user_id)
        .await?
        .get_project_trust(path)
        .await
}

#[tauri::command]
pub async fn agent_set_project_trust(
    app_handle: AppHandle,
    state: State<'_, MapleAgentService>,
    user_id: String,
    path: String,
    trusted: bool,
) -> Result<AgentProjectTrustStatus, String> {
    let _ = app_handle;
    handle_for_user(&state, &user_id)
        .await?
        .set_project_trust(path, trusted)
        .await
}

#[tauri::command]
pub async fn agent_save_project_root_order(
    app_handle: AppHandle,
    state: State<'_, MapleAgentService>,
    user_id: String,
    paths: Vec<String>,
) -> Result<Vec<RecentProjectRoot>, String> {
    let _ = app_handle;
    handle_for_user(&state, &user_id)
        .await?
        .save_project_root_order(paths)
        .await
}

#[tauri::command]
pub async fn agent_create_session(
    app_handle: AppHandle,
    state: State<'_, MapleAgentService>,
    user_id: String,
    request: Option<AgentCreateSessionRequest>,
) -> Result<AgentSessionDetail, String> {
    let _ = app_handle;
    handle_for_user(&state, &user_id)
        .await?
        .create_session(request)
        .await
}

#[tauri::command]
pub async fn agent_list_sessions(
    app_handle: AppHandle,
    state: State<'_, MapleAgentService>,
    user_id: String,
    project_root: Option<String>,
) -> Result<Vec<AgentSessionSummary>, String> {
    let _ = app_handle;
    handle_for_user(&state, &user_id)
        .await?
        .list_sessions(project_root)
        .await
}

#[tauri::command]
pub async fn agent_load_session(
    app_handle: AppHandle,
    state: State<'_, MapleAgentService>,
    user_id: String,
    session_id: String,
) -> Result<AgentSessionDetail, String> {
    let _ = app_handle;
    handle_for_user(&state, &user_id)
        .await?
        .load_session(session_id)
        .await
}

#[tauri::command]
pub async fn agent_rename_session(
    state: State<'_, MapleAgentService>,
    api_auth_state: State<'_, MapleApiAuthState>,
    user_id: String,
    request: AgentRenameSessionRequest,
) -> Result<AgentSessionSummary, String> {
    let handle = handle_for_user(&state, &user_id).await?;
    let api_session = api_auth_state.session_for(&user_id).await?;
    handle.rename_session(api_session, request).await
}

#[tauri::command]
pub async fn agent_list_session_mcp_servers(
    app_handle: AppHandle,
    state: State<'_, MapleAgentService>,
    user_id: String,
    session_id: String,
) -> Result<Vec<AgentSessionMcpServer>, String> {
    let _ = app_handle;
    handle_for_user(&state, &user_id)
        .await?
        .list_session_mcp_servers(session_id)
        .await
}

#[tauri::command]
pub async fn agent_set_session_mcp_server_enabled(
    app_handle: AppHandle,
    state: State<'_, MapleAgentService>,
    user_id: String,
    request: AgentSetSessionMcpServerRequest,
) -> Result<Vec<AgentSessionMcpServer>, String> {
    let _ = app_handle;
    handle_for_user(&state, &user_id)
        .await?
        .set_session_mcp_server_enabled(request)
        .await
}

#[tauri::command]
pub async fn agent_delete_session(
    app_handle: AppHandle,
    state: State<'_, MapleAgentService>,
    user_id: String,
    session_id: String,
) -> Result<(), String> {
    let _ = app_handle;
    handle_for_user(&state, &user_id)
        .await?
        .delete_session(session_id)
        .await
}

#[tauri::command]
pub async fn agent_send_message(
    app_handle: AppHandle,
    state: State<'_, MapleAgentService>,
    user_id: String,
    request: AgentSendMessageRequest,
) -> Result<AgentRunResponse, String> {
    let _ = app_handle;
    let run = handle_for_user(&state, &user_id)
        .await?
        .send_message(request)
        .await?;
    Ok(AgentRunResponse {
        run_id: run.run_id,
        queued: run.queued,
        queue: run.queue,
    })
}

#[tauri::command]
pub async fn agent_cancel_queued_message(
    app_handle: AppHandle,
    state: State<'_, MapleAgentService>,
    user_id: String,
    request: AgentQueueControlRequest,
) -> Result<AgentDesktopQueueSnapshot, String> {
    let _ = app_handle;
    handle_for_user(&state, &user_id)
        .await?
        .cancel_queued_message(request)
        .await
}

#[tauri::command]
pub async fn agent_unqueue_message_for_edit(
    app_handle: AppHandle,
    state: State<'_, MapleAgentService>,
    user_id: String,
    request: AgentQueueControlRequest,
) -> Result<AgentQueuedMessage, String> {
    let _ = app_handle;
    handle_for_user(&state, &user_id)
        .await?
        .unqueue_message_for_edit(request)
        .await
}

#[tauri::command]
pub async fn agent_begin_queued_message_edit(
    app_handle: AppHandle,
    state: State<'_, MapleAgentService>,
    user_id: String,
    request: AgentQueueControlRequest,
) -> Result<(), String> {
    let _ = app_handle;
    handle_for_user(&state, &user_id)
        .await?
        .begin_queued_message_edit(request)
        .await
}

#[tauri::command]
pub async fn agent_end_queued_message_edit(
    app_handle: AppHandle,
    state: State<'_, MapleAgentService>,
    user_id: String,
    request: AgentQueueControlRequest,
) -> Result<(), String> {
    let _ = app_handle;
    handle_for_user(&state, &user_id)
        .await?
        .end_queued_message_edit(request)
        .await
}

#[tauri::command]
pub async fn agent_update_queued_message(
    app_handle: AppHandle,
    state: State<'_, MapleAgentService>,
    user_id: String,
    request: AgentQueueUpdateRequest,
) -> Result<AgentDesktopQueueSnapshot, String> {
    let _ = app_handle;
    handle_for_user(&state, &user_id)
        .await?
        .update_queued_message(request)
        .await
}

#[tauri::command]
pub async fn agent_cancel_run(
    app_handle: AppHandle,
    state: State<'_, MapleAgentService>,
    user_id: String,
    run_id: String,
) -> Result<(), String> {
    let _ = app_handle;
    handle_for_user(&state, &user_id)
        .await?
        .cancel_desktop_run(run_id)
        .await
}

#[tauri::command]
pub async fn agent_set_permission_mode(
    app_handle: AppHandle,
    state: State<'_, MapleAgentService>,
    user_id: String,
    request: AgentPermissionModeRequest,
) -> Result<(), String> {
    let _ = app_handle;
    handle_for_user(&state, &user_id)
        .await?
        .set_permission_mode(request)
        .await
}

#[tauri::command]
pub async fn agent_permission_respond(
    app_handle: AppHandle,
    state: State<'_, MapleAgentService>,
    user_id: String,
    response: AgentPermissionResponse,
) -> Result<(), String> {
    let _ = app_handle;
    handle_for_user(&state, &user_id)
        .await?
        .permission_respond(response)
        .await
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use std::collections::HashMap;

    #[test]
    fn service_events_project_to_the_stable_desktop_wire_contract() {
        let status = AgentRuntimeStatus {
            running: true,
            project_root: Some("/tmp/project".to_string()),
            model: Some("maple-model".to_string()),
            mode: Some("smart_approve".to_string()),
            active_runs: HashMap::from([("session-1".to_string(), "run-1".to_string())]),
        };
        let session = AgentSessionSummary {
            id: "session-1".to_string(),
            title: "Task".to_string(),
            project_root: "/tmp/project".to_string(),
            created_ms: 1,
            updated_ms: 2,
            message_count: 3,
            model: Some("maple-model".to_string()),
            mode: "smart_approve".to_string(),
        };
        let item = AgentTimelineItem {
            id: "message-1".to_string(),
            item_type: "message".to_string(),
            role: Some("assistant".to_string()),
            title: None,
            text: Some("hello".to_string()),
            status: None,
            input: None,
            output: None,
            created_ms: 4,
            merge: "append".to_string(),
        };
        let events = [
            AgentServiceEvent::RuntimeStatus(status),
            AgentServiceEvent::SessionCreated(session.clone()),
            AgentServiceEvent::SessionUpdated {
                session_id: "session-1".to_string(),
                run_id: Some("run-1".to_string()),
                session,
            },
            AgentServiceEvent::Run {
                session_id: "session-1".to_string(),
                run_id: "run-1".to_string(),
                event: AgentRunEvent::Started,
            },
            AgentServiceEvent::Run {
                session_id: "session-1".to_string(),
                run_id: "run-1".to_string(),
                event: AgentRunEvent::TimelineItem(item.clone()),
            },
            AgentServiceEvent::Run {
                session_id: "session-1".to_string(),
                run_id: "run-1".to_string(),
                event: AgentRunEvent::Error(item),
            },
            AgentServiceEvent::Run {
                session_id: "session-1".to_string(),
                run_id: "run-1".to_string(),
                event: AgentRunEvent::HistoryReplaced,
            },
            AgentServiceEvent::Run {
                session_id: "session-1".to_string(),
                run_id: "run-1".to_string(),
                event: AgentRunEvent::Finished(AgentRunTerminal::Completed),
            },
        ];
        let projected = events.iter().map(project_agent_event).collect::<Vec<_>>();

        assert_eq!(
            serde_json::to_value(projected).unwrap(),
            json!([
                {
                    "eventType": "runtimeStatus",
                    "status": {
                        "running": true,
                        "projectRoot": "/tmp/project",
                        "model": "maple-model",
                        "mode": "smart_approve",
                        "activeRuns": { "session-1": "run-1" }
                    }
                },
                {
                    "eventType": "sessionCreated",
                    "sessionId": "session-1",
                    "session": {
                        "id": "session-1",
                        "title": "Task",
                        "projectRoot": "/tmp/project",
                        "createdMs": 1,
                        "updatedMs": 2,
                        "messageCount": 3,
                        "model": "maple-model",
                        "mode": "smart_approve"
                    }
                },
                {
                    "eventType": "sessionUpdated",
                    "sessionId": "session-1",
                    "runId": "run-1",
                    "session": {
                        "id": "session-1",
                        "title": "Task",
                        "projectRoot": "/tmp/project",
                        "createdMs": 1,
                        "updatedMs": 2,
                        "messageCount": 3,
                        "model": "maple-model",
                        "mode": "smart_approve"
                    }
                },
                { "eventType": "runStarted", "sessionId": "session-1", "runId": "run-1" },
                {
                    "eventType": "timelineItem",
                    "sessionId": "session-1",
                    "runId": "run-1",
                    "item": {
                        "id": "message-1",
                        "itemType": "message",
                        "role": "assistant",
                        "text": "hello",
                        "createdMs": 4,
                        "merge": "append"
                    }
                },
                {
                    "eventType": "error",
                    "sessionId": "session-1",
                    "runId": "run-1",
                    "item": {
                        "id": "message-1",
                        "itemType": "message",
                        "role": "assistant",
                        "text": "hello",
                        "createdMs": 4,
                        "merge": "append"
                    }
                },
                { "eventType": "historyReplaced", "sessionId": "session-1", "runId": "run-1" },
                {
                    "eventType": "runFinished",
                    "sessionId": "session-1",
                    "runId": "run-1",
                    "message": "completed"
                }
            ])
        );

        let warning = project_agent_event(&AgentServiceEvent::Run {
            session_id: "session-1".to_string(),
            run_id: "run-1".to_string(),
            event: AgentRunEvent::SetupWarning("setup warning".to_string()),
        });
        assert_eq!(warning.event_type, "error");
        assert_eq!(warning.session_id, None);
        assert_eq!(warning.run_id.as_deref(), Some("run-1"));
        assert_eq!(warning.message.as_deref(), Some("setup warning"));
    }

    #[test]
    fn typed_permission_requests_keep_the_existing_desktop_timeline_shape() {
        let item = AgentTimelineItem {
            id: "permission-request-1".to_string(),
            item_type: "permission".to_string(),
            role: Some("system".to_string()),
            title: Some("Run shell command".to_string()),
            text: Some("Run this command?".to_string()),
            status: Some("pending".to_string()),
            input: Some(json!({ "command": "git status --short" })),
            output: None,
            created_ms: 4,
            merge: "replace".to_string(),
        };
        let projected = project_agent_event(&AgentServiceEvent::Run {
            session_id: "session-1".to_string(),
            run_id: "run-1".to_string(),
            event: AgentRunEvent::PermissionRequested {
                request: crate::agent::AgentPermissionRequest {
                    request_id: "request-1".to_string(),
                    tool_name: "shell".to_string(),
                    arguments: serde_json::Map::from_iter([(
                        "command".to_string(),
                        json!("git status --short"),
                    )]),
                    prompt: Some("Run this command?".to_string()),
                },
                item: item.clone(),
            },
        });

        assert_eq!(projected.event_type, "timelineItem");
        assert_eq!(projected.session_id.as_deref(), Some("session-1"));
        assert_eq!(projected.run_id.as_deref(), Some("run-1"));
        assert_eq!(projected.item, Some(item));
    }
}
