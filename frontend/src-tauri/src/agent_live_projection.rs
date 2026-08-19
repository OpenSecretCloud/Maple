//! Closed projection between Maple's rich in-process Agent events and the
//! durable, remotely safe live-event contract.
//!
//! Rich desktop events may contain arbitrary tool input/output values and an
//! actionable permission capability. Neither is admitted here. Projection is
//! deliberately two-step: first build the closed payload used for the durable
//! stable-operation commitment, then consume that projection with a typed
//! ingress event ID. A raw string can therefore never become publish authority.

#![allow(
    dead_code,
    reason = "the projector is consumed by the synchronized Agent attach slice"
)]

use crate::{
    agent::{
        AgentRunEvent, AgentRunTerminal, AgentServiceEvent, AgentSessionSummary, AgentTimelineItem,
    },
    agent_live_coordinator::{
        AgentLivePublishEvent, IngressEventId, MapleLiveEvent, MapleLiveItemType, MapleLiveMerge,
        MapleLiveRole, MapleLiveRunTerminal, MapleLiveSessionSummary, MapleLiveTimelineItem,
        MapleLiveUserFacingError, MapleLiveUserFacingErrorKind,
    },
    remote_protocol::{
        SAFE_REMOTE_AGENT_ERROR as REMOTE_AGENT_ERROR,
        SAFE_REMOTE_PERMISSION_TITLE as REMOTE_PERMISSION_TITLE,
        SAFE_REMOTE_SETUP_WARNING as REMOTE_SETUP_WARNING,
        SAFE_REMOTE_TOOL_CANCELLED as REMOTE_TOOL_CANCELLED,
        SAFE_REMOTE_TOOL_FAILED as REMOTE_TOOL_FAILED, SAFE_REMOTE_TOOL_TITLE as REMOTE_TOOL_TITLE,
    },
};

const MAX_JAVASCRIPT_SAFE_INTEGER: u128 = 9_007_199_254_740_991;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum AgentLiveProjectionBoundaryError {
    ControlPlaneOnly,
    InvalidItemType,
    InvalidRole,
    InvalidMerge,
    InvalidTimestamp,
    InvalidToolStatus,
    ActionablePermission,
}

/// Input outside [`AgentServiceEvent`] for account-visible lifecycle mutations
/// that did not historically have a rich Desktop event variant.
pub(crate) enum AgentLiveProjectionSource<'a> {
    Service(&'a AgentServiceEvent),
    SessionDeleted { session_id: &'a str },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ProjectedAgentLiveEvent {
    pub(crate) session_id: String,
    pub(crate) run_id: Option<String>,
    pub(crate) event: MapleLiveEvent,
}

impl ProjectedAgentLiveEvent {
    /// Consume the reviewed closed projection and replace its untrusted
    /// presentation-only event ID with the exact typed ingress identity.
    /// This is the only route from rich local data into a publishable event.
    pub(crate) fn into_publish_event(self, event_id: IngressEventId) -> AgentLivePublishEvent {
        match self.event {
            MapleLiveEvent::RunStarted { .. } => AgentLivePublishEvent::run_started(event_id),
            MapleLiveEvent::TimelineUpsert { item, .. } => {
                AgentLivePublishEvent::timeline_upsert(event_id, item)
            }
            MapleLiveEvent::TimelineCleared { reason, .. } => {
                AgentLivePublishEvent::timeline_cleared(event_id, reason)
            }
            MapleLiveEvent::HistoryReplaced { .. } => {
                AgentLivePublishEvent::history_replaced(event_id)
            }
            MapleLiveEvent::HistoryHeadCommitted {
                history_revision,
                through_event_cursor,
                ..
            } => AgentLivePublishEvent::history_head_committed(
                event_id,
                history_revision,
                through_event_cursor,
            ),
            MapleLiveEvent::SessionUpdated { session, .. } => {
                AgentLivePublishEvent::session_updated(event_id, session)
            }
            MapleLiveEvent::RunFinished { terminal, .. } => {
                AgentLivePublishEvent::run_finished(event_id, terminal)
            }
            MapleLiveEvent::SessionDeleted { .. } => {
                AgentLivePublishEvent::session_deleted(event_id)
            }
            MapleLiveEvent::UserFacingError { error, .. } => {
                AgentLivePublishEvent::user_facing_error(event_id, error)
            }
        }
    }
}

/// Project one account-visible event into the closed durable contract.
///
/// `presentation_id` is a bounded, non-secret ID durably assigned to the same
/// logical mutation. It is presentation data only; the returned event cannot
/// be published until [`ProjectedAgentLiveEvent::into_publish_event`] receives
/// a typed ingress ID whose stable-operation commitment covers this payload.
///
/// `created_ms` is used only for a setup warning, whose legacy event carries no
/// timestamp. All timeline items retain their source timestamp.
pub(crate) fn project_agent_service_event(
    source: AgentLiveProjectionSource<'_>,
    presentation_id: &str,
    created_ms: u64,
) -> Result<ProjectedAgentLiveEvent, AgentLiveProjectionBoundaryError> {
    match source {
        AgentLiveProjectionSource::SessionDeleted { session_id } => Ok(ProjectedAgentLiveEvent {
            session_id: session_id.to_string(),
            run_id: None,
            event: MapleLiveEvent::SessionDeleted {
                event_id: presentation_id.to_string(),
            },
        }),
        AgentLiveProjectionSource::Service(event) => {
            project_rich_service_event(event, presentation_id, created_ms)
        }
    }
}

fn project_rich_service_event(
    source: &AgentServiceEvent,
    presentation_id: &str,
    created_ms: u64,
) -> Result<ProjectedAgentLiveEvent, AgentLiveProjectionBoundaryError> {
    let presentation_id = presentation_id.to_string();
    let projected = match source {
        AgentServiceEvent::RuntimeStatus(_) => {
            return Err(AgentLiveProjectionBoundaryError::ControlPlaneOnly);
        }
        AgentServiceEvent::SessionCreated(session) => ProjectedAgentLiveEvent {
            session_id: session.id.clone(),
            run_id: None,
            event: MapleLiveEvent::SessionUpdated {
                event_id: presentation_id,
                session: project_session_summary(session),
            },
        },
        AgentServiceEvent::SessionUpdated {
            session_id,
            run_id,
            session,
        } => ProjectedAgentLiveEvent {
            session_id: session_id.clone(),
            run_id: run_id.clone(),
            event: MapleLiveEvent::SessionUpdated {
                event_id: presentation_id,
                session: project_session_summary(session),
            },
        },
        AgentServiceEvent::TimelineItem {
            session_id,
            run_id,
            item,
        } => ProjectedAgentLiveEvent {
            session_id: session_id.clone(),
            run_id: run_id.clone(),
            event: MapleLiveEvent::TimelineUpsert {
                event_id: presentation_id,
                item: project_timeline_item(item)?,
            },
        },
        AgentServiceEvent::Run {
            session_id,
            run_id,
            event,
        } => {
            let event = match event {
                AgentRunEvent::SessionUpdated(session) => MapleLiveEvent::SessionUpdated {
                    event_id: presentation_id,
                    session: project_session_summary(session),
                },
                AgentRunEvent::Started => MapleLiveEvent::RunStarted {
                    event_id: presentation_id,
                },
                AgentRunEvent::TimelineItem(item) | AgentRunEvent::Error(item) => {
                    MapleLiveEvent::TimelineUpsert {
                        event_id: presentation_id,
                        item: project_timeline_item(item)?,
                    }
                }
                AgentRunEvent::PermissionRequested { .. } => {
                    return Err(AgentLiveProjectionBoundaryError::ControlPlaneOnly);
                }
                AgentRunEvent::SetupWarning(message) => {
                    MapleLiveEvent::UserFacingError {
                        event_id: presentation_id.clone(),
                        error: MapleLiveUserFacingError {
                            id: presentation_id,
                            kind: MapleLiveUserFacingErrorKind::Warning,
                            title: Some("Agent warning".to_string()),
                            // MCP loader diagnostics can contain provider strings,
                            // local paths, or environment details. The rich local
                            // event retains them; the durable remote event does not.
                            message: sanitize_setup_warning(message),
                            created_ms,
                        },
                    }
                }
                AgentRunEvent::HistoryReplaced => MapleLiveEvent::HistoryReplaced {
                    event_id: presentation_id,
                },
                AgentRunEvent::Finished(terminal) => MapleLiveEvent::RunFinished {
                    event_id: presentation_id,
                    terminal: project_terminal(*terminal),
                },
            };
            ProjectedAgentLiveEvent {
                session_id: session_id.clone(),
                run_id: Some(run_id.clone()),
                event,
            }
        }
    };
    Ok(projected)
}

pub(crate) fn project_timeline_item(
    item: &AgentTimelineItem,
) -> Result<MapleLiveTimelineItem, AgentLiveProjectionBoundaryError> {
    let item_type = match item.item_type.as_str() {
        "message" => MapleLiveItemType::Message,
        "thinking" => MapleLiveItemType::Thinking,
        "tool" => MapleLiveItemType::Tool,
        "permission" => MapleLiveItemType::Permission,
        "system" => MapleLiveItemType::System,
        "error" => MapleLiveItemType::Error,
        _ => return Err(AgentLiveProjectionBoundaryError::InvalidItemType),
    };
    let role = match item_type {
        // These presentation rows cross the remote boundary only in their
        // fixed reviewed roles. Never retain a provider- or argument-derived
        // source role for them.
        MapleLiveItemType::Tool => Some(MapleLiveRole::Assistant),
        MapleLiveItemType::Permission | MapleLiveItemType::Error => Some(MapleLiveRole::System),
        MapleLiveItemType::Message | MapleLiveItemType::Thinking | MapleLiveItemType::System => {
            match item.role.as_deref() {
                None => None,
                Some("user") => Some(MapleLiveRole::User),
                Some("assistant") => Some(MapleLiveRole::Assistant),
                Some("thought") => Some(MapleLiveRole::Thought),
                Some("system") => Some(MapleLiveRole::System),
                Some(_) => return Err(AgentLiveProjectionBoundaryError::InvalidRole),
            }
        }
    };
    let merge = match item.merge.as_str() {
        "append" => MapleLiveMerge::Append,
        "replace" => MapleLiveMerge::Replace,
        _ => return Err(AgentLiveProjectionBoundaryError::InvalidMerge),
    };
    if item.created_ms > MAX_JAVASCRIPT_SAFE_INTEGER {
        return Err(AgentLiveProjectionBoundaryError::InvalidTimestamp);
    }
    let status = match item_type {
        MapleLiveItemType::Permission => Some(normalize_terminal_permission_status(
            item.status.as_deref(),
        )?),
        MapleLiveItemType::Tool => normalize_tool_status(item.status.as_deref())?,
        MapleLiveItemType::Error => Some("failed".to_string()),
        _ => item.status.clone(),
    };
    // Rich tool titles are constructed from commands, paths, queries, URLs,
    // and skill arguments. Failure text can be a raw parser/runtime error.
    // Neither is a reviewed disclosure surface, so the durable/remote row uses
    // a fixed presentation. The original local event remains unchanged.
    let (title, text) = match item_type {
        MapleLiveItemType::Tool => {
            let text = match status.as_deref() {
                Some("failed" | "error") => Some(REMOTE_TOOL_FAILED.to_string()),
                Some("cancelled") => Some(REMOTE_TOOL_CANCELLED.to_string()),
                _ => None,
            };
            (Some(REMOTE_TOOL_TITLE.to_string()), text)
        }
        MapleLiveItemType::Permission => (Some(REMOTE_PERMISSION_TITLE.to_string()), None),
        MapleLiveItemType::Error => (
            Some("Agent error".to_string()),
            Some(REMOTE_AGENT_ERROR.to_string()),
        ),
        _ => (item.title.clone(), item.text.clone()),
    };

    Ok(MapleLiveTimelineItem {
        id: item.id.clone(),
        item_type,
        role,
        title,
        text,
        status,
        created_ms: item.created_ms as u64,
        merge,
    })
}

fn normalize_tool_status(
    status: Option<&str>,
) -> Result<Option<String>, AgentLiveProjectionBoundaryError> {
    match status {
        None => Ok(None),
        Some("pending" | "running" | "completed" | "failed" | "error") => {
            Ok(status.map(str::to_string))
        }
        Some("cancel" | "canceled" | "cancelled") => Ok(Some("cancelled".to_string())),
        Some(_) => Err(AgentLiveProjectionBoundaryError::InvalidToolStatus),
    }
}

fn normalize_terminal_permission_status(
    status: Option<&str>,
) -> Result<String, AgentLiveProjectionBoundaryError> {
    match status {
        // Current Desktop sends the `_once` spellings. The shorter spellings
        // remain accepted by the Rust command boundary for compatibility.
        Some("allow_once" | "allow" | "allow_always") => Ok("allow_once".to_string()),
        Some("deny_once" | "deny" | "deny_always") => Ok("deny_once".to_string()),
        Some("cancel" | "cancelled") => Ok("cancelled".to_string()),
        Some("completed") => Ok("completed".to_string()),
        // Missing, pending, or any future unreviewed state could still carry
        // an actionable capability and therefore remains local control-plane.
        _ => Err(AgentLiveProjectionBoundaryError::ActionablePermission),
    }
}

fn sanitize_setup_warning(_message: &str) -> String {
    REMOTE_SETUP_WARNING.to_string()
}

pub(crate) fn project_session_summary(session: &AgentSessionSummary) -> MapleLiveSessionSummary {
    MapleLiveSessionSummary {
        id: session.id.clone(),
        title: session.title.clone(),
        project_root: session.project_root.clone(),
        created_ms: session.created_ms,
        updated_ms: session.updated_ms,
        page_sort_ms: session.page_sort_ms,
        message_count: session.message_count,
        model: session.model.clone(),
        mode: session.mode.clone(),
    }
}

fn project_terminal(terminal: AgentRunTerminal) -> MapleLiveRunTerminal {
    match terminal {
        AgentRunTerminal::Completed => MapleLiveRunTerminal::Completed,
        AgentRunTerminal::Cancelled => MapleLiveRunTerminal::Cancelled,
        AgentRunTerminal::Failed => MapleLiveRunTerminal::Failed,
    }
}

/// Convert a closed live row back into Maple's established safe presentation
/// item. Rich `input` and `output` are always absent by construction.
pub(crate) fn restore_safe_timeline_item(item: &MapleLiveTimelineItem) -> AgentTimelineItem {
    AgentTimelineItem {
        id: item.id.clone(),
        item_type: match item.item_type {
            MapleLiveItemType::Message => "message",
            MapleLiveItemType::Thinking => "thinking",
            MapleLiveItemType::Tool => "tool",
            MapleLiveItemType::Permission => "permission",
            MapleLiveItemType::System => "system",
            MapleLiveItemType::Error => "error",
        }
        .to_string(),
        role: item.role.map(|role| {
            match role {
                MapleLiveRole::User => "user",
                MapleLiveRole::Assistant => "assistant",
                MapleLiveRole::Thought => "thought",
                MapleLiveRole::System => "system",
            }
            .to_string()
        }),
        title: item.title.clone(),
        text: item.text.clone(),
        status: item.status.clone(),
        input: None,
        output: None,
        created_ms: u128::from(item.created_ms),
        merge: match item.merge {
            MapleLiveMerge::Append => "append",
            MapleLiveMerge::Replace => "replace",
        }
        .to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn item(item_type: &str, status: Option<&str>) -> AgentTimelineItem {
        AgentTimelineItem {
            id: "item-1".to_string(),
            item_type: item_type.to_string(),
            role: Some("assistant".to_string()),
            title: Some("Visible title".to_string()),
            text: Some("Visible text".to_string()),
            status: status.map(str::to_string),
            input: Some(json!({"secretProviderInput": "must not cross"})),
            output: Some(json!({"secretProviderOutput": "must not cross"})),
            created_ms: 123,
            merge: "replace".to_string(),
        }
    }

    #[test]
    fn projection_drops_arbitrary_tool_input_and_output() {
        let projected = project_timeline_item(&item("tool", Some("completed"))).unwrap();
        let encoded = serde_json::to_string(&projected).unwrap();
        assert!(!encoded.contains("secretProviderInput"));
        assert!(!encoded.contains("secretProviderOutput"));
        let restored = restore_safe_timeline_item(&projected);
        assert!(restored.input.is_none());
        assert!(restored.output.is_none());
    }

    #[test]
    fn durable_tool_projection_redacts_argument_titles_and_failure_diagnostics() {
        let secrets = [
            "sk-live-secret",
            "/Users/private/.env",
            "DATABASE_URL=postgres://admin:password@host/db",
            "https://example.invalid/?token=secret",
            "curl -H 'Authorization: Bearer secret'",
        ];
        for (index, secret) in secrets.iter().enumerate() {
            let mut rich = item(
                "tool",
                Some(if index % 2 == 0 {
                    "failed"
                } else {
                    "cancelled"
                }),
            );
            rich.title = Some(format!("Terminal: {secret}"));
            rich.text = Some(format!("runtime parser diagnostic: {secret}"));
            let projected = project_timeline_item(&rich).unwrap();
            assert_eq!(projected.title.as_deref(), Some(REMOTE_TOOL_TITLE));
            assert!(matches!(
                projected.text.as_deref(),
                Some(REMOTE_TOOL_FAILED | REMOTE_TOOL_CANCELLED)
            ));

            // Exercise the exact durable representation a coordinator writes,
            // then reconstruct it as a process restart would.
            let durable = MapleLiveEvent::TimelineUpsert {
                event_id: format!("event-{index}"),
                item: projected,
            };
            let encoded = serde_json::to_vec(&durable).unwrap();
            let restored: MapleLiveEvent = serde_json::from_slice(&encoded).unwrap();
            let reencoded = serde_json::to_string(&restored).unwrap();
            assert!(!reencoded.contains(secret), "leaked {secret}");
            assert!(!reencoded.contains("runtime parser diagnostic"));
        }
    }

    #[test]
    fn durable_tool_status_is_closed_and_success_does_not_copy_rich_text() {
        let mut completed = item("tool", Some("completed"));
        completed.title = Some("Web Search: secret query".to_string());
        completed.text = Some("provider-private successful output".to_string());
        let projected = project_timeline_item(&completed).unwrap();
        assert_eq!(projected.title.as_deref(), Some(REMOTE_TOOL_TITLE));
        assert!(projected.text.is_none());

        let unknown = item("tool", Some("provider_private"));
        assert_eq!(
            project_timeline_item(&unknown),
            Err(AgentLiveProjectionBoundaryError::InvalidToolStatus)
        );
    }

    #[test]
    fn actionable_permission_is_control_plane_only() {
        assert_eq!(
            project_timeline_item(&item("permission", Some("pending"))),
            Err(AgentLiveProjectionBoundaryError::ActionablePermission)
        );
        let source = AgentServiceEvent::Run {
            session_id: "session".to_string(),
            run_id: "run".to_string(),
            event: AgentRunEvent::PermissionRequested {
                request: crate::agent::AgentPermissionRequest {
                    request_id: "permission".to_string(),
                    tool_name: "shell".to_string(),
                    arguments: Default::default(),
                    prompt: None,
                },
                item: item("permission", Some("pending")),
            },
        };
        assert_eq!(
            project_agent_service_event(
                AgentLiveProjectionSource::Service(&source),
                "event-1",
                123,
            ),
            Err(AgentLiveProjectionBoundaryError::ControlPlaneOnly)
        );
    }

    #[test]
    fn every_accepted_permission_spelling_is_terminal_and_normalized() {
        for (source, expected) in [
            ("allow_once", "allow_once"),
            ("allow", "allow_once"),
            ("allow_always", "allow_once"),
            ("deny_once", "deny_once"),
            ("deny", "deny_once"),
            ("deny_always", "deny_once"),
            ("cancel", "cancelled"),
            ("cancelled", "cancelled"),
            ("completed", "completed"),
        ] {
            let projected = project_timeline_item(&item("permission", Some(source))).unwrap();
            assert_eq!(projected.status.as_deref(), Some(expected), "{source}");
        }
        for unsafe_status in [None, Some("pending"), Some("running"), Some("unknown")] {
            assert_eq!(
                project_timeline_item(&item("permission", unsafe_status)),
                Err(AgentLiveProjectionBoundaryError::ActionablePermission),
                "{unsafe_status:?}"
            );
        }
    }

    #[test]
    fn durable_warnings_and_errors_redact_host_diagnostics() {
        let warning = AgentServiceEvent::Run {
            session_id: "session".to_string(),
            run_id: "run".to_string(),
            event: AgentRunEvent::SetupWarning(
                "provider token at /Users/private/.config failed: sk-secret".to_string(),
            ),
        };
        let projected = project_agent_service_event(
            AgentLiveProjectionSource::Service(&warning),
            "event-warning",
            123,
        )
        .unwrap();
        let encoded = serde_json::to_string(&projected.event).unwrap();
        assert!(encoded.contains(REMOTE_SETUP_WARNING));
        assert!(!encoded.contains("/Users/private"));
        assert!(!encoded.contains("sk-secret"));

        let mut rich_error = item("error", Some("failed"));
        rich_error.text = Some("provider request included sk-secret".to_string());
        let projected = project_timeline_item(&rich_error).unwrap();
        assert_eq!(projected.text.as_deref(), Some(REMOTE_AGENT_ERROR));
    }

    #[test]
    fn fixed_remote_rows_ignore_hostile_source_roles_and_error_status() {
        let mut tool = item("tool", Some("completed"));
        tool.role = Some("provider-private-role".to_string());
        let tool = project_timeline_item(&tool).unwrap();
        assert_eq!(tool.role, Some(MapleLiveRole::Assistant));
        tool.validate().unwrap();

        let mut permission = item("permission", Some("completed"));
        permission.role = Some("user".to_string());
        let permission = project_timeline_item(&permission).unwrap();
        assert_eq!(permission.role, Some(MapleLiveRole::System));
        permission.validate().unwrap();

        let mut error = item("error", Some("DATABASE_URL=postgres://secret"));
        error.role = Some("thought".to_string());
        let error = project_timeline_item(&error).unwrap();
        assert_eq!(error.role, Some(MapleLiveRole::System));
        assert_eq!(error.status.as_deref(), Some("failed"));
        error.validate().unwrap();
        let encoded = serde_json::to_string(&error).unwrap();
        assert!(!encoded.contains("DATABASE_URL"));
        assert!(!encoded.contains("secret"));
    }

    #[test]
    fn session_projection_preserves_storage_sort_key() {
        let summary = AgentSessionSummary {
            id: "session".to_string(),
            title: "Task".to_string(),
            project_root: "/project".to_string(),
            created_ms: 1,
            updated_ms: 2,
            page_sort_ms: 9,
            message_count: 3,
            model: Some("model".to_string()),
            mode: "chat".to_string(),
        };
        assert_eq!(project_session_summary(&summary).page_sort_ms, 9);
    }

    #[test]
    fn unknown_enum_strings_and_unsafe_timestamp_fail_closed() {
        let mut candidate = item("provider_private", None);
        assert_eq!(
            project_timeline_item(&candidate),
            Err(AgentLiveProjectionBoundaryError::InvalidItemType)
        );
        candidate.item_type = "message".to_string();
        candidate.role = Some("provider".to_string());
        assert_eq!(
            project_timeline_item(&candidate),
            Err(AgentLiveProjectionBoundaryError::InvalidRole)
        );
        candidate.role = Some("assistant".to_string());
        candidate.merge = "splice".to_string();
        assert_eq!(
            project_timeline_item(&candidate),
            Err(AgentLiveProjectionBoundaryError::InvalidMerge)
        );
        candidate.merge = "replace".to_string();
        candidate.created_ms = MAX_JAVASCRIPT_SAFE_INTEGER + 1;
        assert_eq!(
            project_timeline_item(&candidate),
            Err(AgentLiveProjectionBoundaryError::InvalidTimestamp)
        );
    }

    #[test]
    fn runtime_status_never_enters_the_durable_journal() {
        let source = AgentServiceEvent::RuntimeStatus(crate::agent::AgentRuntimeStatus {
            running: false,
            project_root: None,
            model: None,
            mode: None,
            active_runs: Default::default(),
        });
        assert_eq!(
            project_agent_service_event(
                AgentLiveProjectionSource::Service(&source),
                "event-1",
                123,
            ),
            Err(AgentLiveProjectionBoundaryError::ControlPlaneOnly)
        );
    }
}
