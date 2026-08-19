//! Account-owned ordering for Maple-safe live Agent presentation events.
//!
//! Persisted Goose rows and live presentation events deliberately have
//! independent cursors. A head attach captures an absolute in-memory live
//! overlay at durable cursor C0, pauses a bounded subscriber while the caller
//! loads Goose's newest history page, then replays C0..C1 before making that
//! subscriber live. A cursor resume skips the history snapshot and replays
//! directly to its FIFO barrier.
//!
//! This module never accepts a raw Goose event, provider value, credential, or
//! arbitrary `serde_json::Value`. Callers must first project into the closed,
//! bounded [`MapleLiveEvent`] contract below. Runtime status is intentionally
//! absent: it is process/account state, not session replay state.

#![allow(
    dead_code,
    reason = "the coordinator is wired by the remote Agent vertical slice"
)]

use crate::agent_event_journal::{
    AppendOutcome, EventAdmission, LiveEventAccountOwner, LiveEventCursor, LiveEventJournal,
    LiveEventJournalActivation, LiveEventJournalActivationError, LiveEventJournalError,
    LiveEventJournalIngressLease, LiveEventJournalLease, LiveEventJournalReseedRequired,
    LiveEventJournalRolloverObligation, LiveProjectionCheckpoint, LiveReplayEntry,
    LiveReplayPayload, LiveReplayRead, SnapshotRequiredReason,
};
use crate::agent_live_authority::{
    AgentDurableStableOperationId, AgentLiveDataOwnerKey, AGENT_LIVE_PROJECTION_SCHEMA_VERSION,
};
use crate::remote_protocol::{
    SAFE_REMOTE_AGENT_ERROR, SAFE_REMOTE_PERMISSION_TITLE, SAFE_REMOTE_SETUP_WARNING,
    SAFE_REMOTE_TOOL_CANCELLED, SAFE_REMOTE_TOOL_FAILED, SAFE_REMOTE_TOOL_TITLE,
};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    collections::HashMap,
    fmt,
    sync::{mpsc as std_mpsc, Arc, Mutex as TerminalMutex},
    thread,
};
use tokio::sync::{broadcast, mpsc, oneshot};

const DEFAULT_COMMAND_CAPACITY: usize = 128;
const MAX_COMMAND_CAPACITY: usize = 1_024;
const DEFAULT_SUBSCRIPTION_CAPACITY: usize = 128;
const MAX_SUBSCRIPTION_CAPACITY: usize = 512;
const MAX_BUFFERED_DELIVERY_BYTES: usize = MAX_TEXT_BYTES + 16 * 1024;
const MAX_ACCOUNT_SUBSCRIPTION_BUFFER_BYTES: usize = 64 * 1024 * 1024;
const REPLAY_PAGE_SIZE: usize = 50;
const MAX_ACCOUNT_SCOPE_BYTES: usize = 256;
const MAX_EXECUTION_TARGET_BYTES: usize = 128;
const MAX_OWNER_ID_BYTES: usize = 128;
const MAX_EVENT_ID_BYTES: usize = 128;
const MAX_ITEM_ID_BYTES: usize = 128;
const MAX_TITLE_BYTES: usize = 1_024;
const MAX_TEXT_BYTES: usize = 192 * 1_024;
const MAX_STATUS_BYTES: usize = 256;
const MAX_USER_FACING_ERROR_TITLE_BYTES: usize = 256;
const MAX_USER_FACING_ERROR_MESSAGE_BYTES: usize = 8 * 1024;
const MAX_PROJECT_ROOT_BYTES: usize = 4_096;
const MAX_MODEL_BYTES: usize = 256;
const MAX_MODE_BYTES: usize = 64;
const MAX_HISTORY_REVISION_BYTES: usize = 512;
const MAX_LIVE_ITEMS_PER_SESSION: usize = 200;
const MAX_LIVE_SESSIONS_PER_ACCOUNT: usize = 64;
const MAX_LIVE_ITEMS_PER_ACCOUNT: usize = 512;
const MAX_LIVE_PROJECTION_BYTES_PER_ACCOUNT: usize =
    crate::remote_protocol::MAX_LIVE_PROJECTION_BYTES_PER_ACCOUNT;
const LIVE_CHECKPOINT_OUTER_OVERHEAD_BYTES: usize = 4 * 1024;
const LIVE_CHECKPOINT_SESSION_OVERHEAD_BYTES: usize = 256;
const LIVE_CHECKPOINT_ITEM_OVERHEAD_BYTES: usize = 256;
const MAX_SUBSCRIBERS_PER_ACCOUNT: usize = 64;
const MAX_INGRESS_ROUTES_PER_ACCOUNT: usize = 256;
const MAX_JAVASCRIPT_SAFE_INTEGER: u64 = 9_007_199_254_740_991;
const ACTOR_INGRESS_LINEAGE_BYTES: usize = 32;
const INGRESS_EVENT_ID_DOMAIN: &[u8] = b"maple-agent-live-ingress-event-v1\0";
const LIVE_PAYLOAD_COMMITMENT_DOMAIN: &[u8] = b"maple-agent-live-payload-v1\0";
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum MapleLiveItemType {
    Message,
    Thinking,
    Tool,
    Permission,
    System,
    Error,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum MapleLiveRole {
    User,
    Assistant,
    Thought,
    System,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum MapleLiveMerge {
    Append,
    Replace,
}

/// Presentation-safe live timeline row.
///
/// Tool arguments/results are intentionally not represented as arbitrary JSON.
/// Tool, error, and permission rows use fixed reviewed presentation strings;
/// only ordinary message/thinking/system rows may carry source presentation.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct MapleLiveTimelineItem {
    pub(crate) id: String,
    pub(crate) item_type: MapleLiveItemType,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) role: Option<MapleLiveRole>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) title: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) text: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) status: Option<String>,
    pub(crate) created_ms: u64,
    pub(crate) merge: MapleLiveMerge,
}

impl MapleLiveTimelineItem {
    pub(crate) fn validate(&self) -> Result<(), AgentLiveProjectionError> {
        validate_identifier(&self.id, MAX_ITEM_ID_BYTES)?;
        validate_optional_text(self.title.as_deref(), MAX_TITLE_BYTES)?;
        validate_optional_text(self.text.as_deref(), MAX_TEXT_BYTES)?;
        validate_optional_text(self.status.as_deref(), MAX_STATUS_BYTES)?;
        if self.item_type == MapleLiveItemType::Permission
            && !matches!(
                self.status.as_deref(),
                Some("allow_once" | "deny_once" | "completed" | "cancelled")
            )
        {
            return Err(AgentLiveProjectionError::ActionablePermission);
        }
        if self.created_ms > MAX_JAVASCRIPT_SAFE_INTEGER {
            return Err(AgentLiveProjectionError::InvalidTimestamp);
        }
        match self.item_type {
            MapleLiveItemType::Tool => {
                let expected_text = match self.status.as_deref() {
                    None | Some("pending" | "running" | "completed") => None,
                    Some("failed" | "error") => Some(SAFE_REMOTE_TOOL_FAILED),
                    Some("cancelled") => Some(SAFE_REMOTE_TOOL_CANCELLED),
                    Some(_) => return Err(AgentLiveProjectionError::UnsafePresentation),
                };
                if self.role != Some(MapleLiveRole::Assistant)
                    || self.title.as_deref() != Some(SAFE_REMOTE_TOOL_TITLE)
                    || self.text.as_deref() != expected_text
                {
                    return Err(AgentLiveProjectionError::UnsafePresentation);
                }
            }
            MapleLiveItemType::Permission => {
                if self.role != Some(MapleLiveRole::System)
                    || self.title.as_deref() != Some(SAFE_REMOTE_PERMISSION_TITLE)
                    || self.text.is_some()
                {
                    return Err(AgentLiveProjectionError::UnsafePresentation);
                }
            }
            MapleLiveItemType::Error => {
                if self.role != Some(MapleLiveRole::System)
                    || self.title.as_deref() != Some("Agent error")
                    || self.text.as_deref() != Some(SAFE_REMOTE_AGENT_ERROR)
                    || self.status.as_deref() != Some("failed")
                {
                    return Err(AgentLiveProjectionError::UnsafePresentation);
                }
            }
            MapleLiveItemType::Message
            | MapleLiveItemType::Thinking
            | MapleLiveItemType::System => {}
        }
        Ok(())
    }

    fn into_absolute(mut self) -> Self {
        self.merge = MapleLiveMerge::Replace;
        self
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct MapleLiveSessionSummary {
    pub(crate) id: String,
    pub(crate) title: String,
    pub(crate) project_root: String,
    pub(crate) created_ms: i64,
    pub(crate) updated_ms: i64,
    /// Storage-derived sidebar order key. `updated_ms` remains the product's
    /// semantic update time and must not be overloaded for keyset ordering.
    pub(crate) page_sort_ms: i64,
    pub(crate) message_count: usize,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) model: Option<String>,
    pub(crate) mode: String,
}

impl MapleLiveSessionSummary {
    fn validate(&self) -> Result<(), AgentLiveProjectionError> {
        validate_identifier(&self.id, MAX_OWNER_ID_BYTES)?;
        validate_text(&self.title, MAX_TITLE_BYTES)?;
        validate_text(&self.project_root, MAX_PROJECT_ROOT_BYTES)?;
        validate_optional_text(self.model.as_deref(), MAX_MODEL_BYTES)?;
        validate_text(&self.mode, MAX_MODE_BYTES)?;
        for timestamp in [self.created_ms, self.updated_ms, self.page_sort_ms] {
            if timestamp < 0 || (timestamp as u64) > MAX_JAVASCRIPT_SAFE_INTEGER {
                return Err(AgentLiveProjectionError::InvalidTimestamp);
            }
        }
        if u64::try_from(self.message_count)
            .ok()
            .is_none_or(|count| count > MAX_JAVASCRIPT_SAFE_INTEGER)
        {
            return Err(AgentLiveProjectionError::InvalidCount);
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum MapleLiveRunTerminal {
    Completed,
    Cancelled,
    Failed,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum MapleLiveClearReason {
    /// Explicitly discard the prior run's absolute overlay before a new run.
    /// Publishing [`MapleLiveEvent::RunStarted`] alone never clears it.
    RunStarted,
    /// Explicitly discard the overlay after reconciling it against persisted
    /// history. [`MapleLiveEvent::HistoryReplaced`] alone never clears it.
    HistoryReplaced,
    ExplicitReload,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum MapleLiveUserFacingErrorKind {
    Warning,
    Error,
}

/// Bounded, already-sanitized error presentation. Provider errors and raw
/// debug strings must be projected into this type before publication.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct MapleLiveUserFacingError {
    pub(crate) id: String,
    pub(crate) kind: MapleLiveUserFacingErrorKind,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) title: Option<String>,
    pub(crate) message: String,
    pub(crate) created_ms: u64,
}

impl MapleLiveUserFacingError {
    fn validate(&self) -> Result<(), AgentLiveProjectionError> {
        validate_identifier(&self.id, MAX_ITEM_ID_BYTES)?;
        validate_optional_text(self.title.as_deref(), MAX_USER_FACING_ERROR_TITLE_BYTES)?;
        if self.message.trim().is_empty() {
            return Err(AgentLiveProjectionError::InvalidUserFacingError);
        }
        validate_text(&self.message, MAX_USER_FACING_ERROR_MESSAGE_BYTES)?;
        if self.created_ms > MAX_JAVASCRIPT_SAFE_INTEGER {
            return Err(AgentLiveProjectionError::InvalidTimestamp);
        }
        let (expected_title, expected_message) = match self.kind {
            MapleLiveUserFacingErrorKind::Warning => ("Agent warning", SAFE_REMOTE_SETUP_WARNING),
            MapleLiveUserFacingErrorKind::Error => ("Agent error", SAFE_REMOTE_AGENT_ERROR),
        };
        if self.title.as_deref() != Some(expected_title) || self.message != expected_message {
            return Err(AgentLiveProjectionError::UnsafePresentation);
        }
        Ok(())
    }

    pub(crate) fn to_timeline_item(&self) -> MapleLiveTimelineItem {
        let (item_type, default_title, status) = match self.kind {
            MapleLiveUserFacingErrorKind::Warning => {
                (MapleLiveItemType::System, "Agent warning", "warning")
            }
            MapleLiveUserFacingErrorKind::Error => {
                (MapleLiveItemType::Error, "Agent error", "failed")
            }
        };
        MapleLiveTimelineItem {
            id: self.id.clone(),
            item_type,
            role: Some(MapleLiveRole::System),
            title: Some(
                self.title
                    .clone()
                    .unwrap_or_else(|| default_title.to_string()),
            ),
            text: Some(self.message.clone()),
            status: Some(status.to_string()),
            created_ms: self.created_ms,
            merge: MapleLiveMerge::Replace,
        }
    }
}

/// Closed durable event set admitted by the coordinator.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
pub(crate) enum MapleLiveEvent {
    RunStarted {
        event_id: String,
    },
    TimelineUpsert {
        event_id: String,
        item: MapleLiveTimelineItem,
    },
    TimelineCleared {
        event_id: String,
        reason: MapleLiveClearReason,
    },
    /// Signals that persisted history changed. This never mutates the absolute
    /// live overlay; publish `TimelineCleared` separately only after an
    /// authoritative reconciliation says the overlay should be discarded.
    HistoryReplaced {
        event_id: String,
    },
    /// Explicit acknowledgement that Goose history at `history_revision`
    /// includes every live update through `through_event_cursor`. The actor
    /// accepts this only when that cursor is its current FIFO head; newer live
    /// output therefore cannot be cleared by a delayed commit.
    HistoryHeadCommitted {
        event_id: String,
        history_revision: String,
        through_event_cursor: LiveEventCursor,
    },
    SessionUpdated {
        event_id: String,
        session: MapleLiveSessionSummary,
    },
    RunFinished {
        event_id: String,
        terminal: MapleLiveRunTerminal,
    },
    SessionDeleted {
        event_id: String,
    },
    UserFacingError {
        event_id: String,
        error: MapleLiveUserFacingError,
    },
}

impl MapleLiveEvent {
    pub(crate) fn event_id(&self) -> &str {
        match self {
            Self::RunStarted { event_id }
            | Self::TimelineUpsert { event_id, .. }
            | Self::TimelineCleared { event_id, .. }
            | Self::HistoryReplaced { event_id }
            | Self::HistoryHeadCommitted { event_id, .. }
            | Self::SessionUpdated { event_id, .. }
            | Self::RunFinished { event_id, .. }
            | Self::SessionDeleted { event_id }
            | Self::UserFacingError { event_id, .. } => event_id,
        }
    }

    fn validate(&self) -> Result<(), AgentLiveProjectionError> {
        validate_identifier(self.event_id(), MAX_EVENT_ID_BYTES)?;
        match self {
            Self::TimelineUpsert { item, .. } => item.validate(),
            Self::SessionUpdated { session, .. } => session.validate(),
            Self::UserFacingError { error, .. } => error.validate(),
            Self::HistoryHeadCommitted {
                history_revision,
                through_event_cursor,
                ..
            } => {
                validate_identifier(history_revision, MAX_HISTORY_REVISION_BYTES)?;
                through_event_cursor
                    .validate()
                    .map_err(|_| AgentLiveProjectionError::InvalidIdentifier)
            }
            Self::RunStarted { .. }
            | Self::TimelineCleared { .. }
            | Self::HistoryReplaced { .. }
            | Self::RunFinished { .. }
            | Self::SessionDeleted { .. } => Ok(()),
        }
    }
}

/// Opaque producer capability for one exact actor, journal generation, and
/// session/run route. Cloning shares that same capability; it never creates a
/// new producer epoch. The type is deliberately not serializable.
#[derive(Clone)]
pub(crate) struct AgentLiveIngressLease {
    journal_ingress: LiveEventJournalIngressLease,
    data_owner: AgentLiveDataOwnerKey,
    namespace: [u8; 32],
    actor_lineage: [u8; ACTOR_INGRESS_LINEAGE_BYTES],
    producer_epoch: u64,
    session_id: Arc<str>,
    run_id: Option<Arc<str>>,
}

impl fmt::Debug for AgentLiveIngressLease {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("AgentLiveIngressLease")
            .field("session_id", &self.session_id)
            .field("run_id", &self.run_id)
            .field("producer_epoch", &"<redacted>")
            .field("namespace", &"<redacted>")
            .field("actor_lineage", &"<redacted>")
            .finish_non_exhaustive()
    }
}

impl AgentLiveIngressLease {
    pub(crate) fn session_id(&self) -> &str {
        &self.session_id
    }

    pub(crate) fn run_id(&self) -> Option<&str> {
        self.run_id.as_deref()
    }

    /// Bind one native, durable stable operation ID to this exact producer.
    /// The input must come from the trusted Goose/runtime adapter, never a
    /// renderer or remote scalar. Reusing it in the same journal deterministically
    /// reconstructs an exact retry; creating it through a fresh post-rollover
    /// ingress is an explicit new-operation contract.
    pub(crate) fn event_id(
        &self,
        durable_stable_operation_id: &AgentDurableStableOperationId,
    ) -> Result<IngressEventId, AgentLiveCoordinatorError> {
        if durable_stable_operation_id.owner() != &self.data_owner
            || durable_stable_operation_id.session_id() != self.session_id.as_ref()
            || durable_stable_operation_id.run_id() != self.run_id.as_deref()
        {
            return Err(AgentLiveCoordinatorError::StableOperationMismatch);
        }
        if durable_stable_operation_id.projection_schema_version()
            != AGENT_LIVE_PROJECTION_SCHEMA_VERSION
        {
            return Err(AgentLiveCoordinatorError::ProjectionSchemaMismatch);
        }
        if durable_stable_operation_id.journal_namespace_commitment() != &self.namespace {
            return Err(AgentLiveCoordinatorError::IngressRebindRequired);
        }
        let wire = ingress_event_wire_id(
            &self.namespace,
            &self.session_id,
            self.run_id.as_deref(),
            durable_stable_operation_id.as_str(),
        );
        Ok(IngressEventId {
            wire,
            namespace: self.namespace,
            actor_lineage: self.actor_lineage,
            producer_epoch: self.producer_epoch,
            session_id: Arc::clone(&self.session_id),
            run_id: self.run_id.as_ref().map(Arc::clone),
            payload_commitment: *durable_stable_operation_id.payload_commitment(),
        })
    }
}

/// Typed event identity. Its wire string is durable, while the hidden producer
/// fields prevent pairing an old in-memory event with a newly admitted lease.
/// It is intentionally not deserializable; journal replay reconstructs only the
/// closed [`MapleLiveEvent`] DTO and can never re-enter producer publication.
#[derive(Clone)]
pub(crate) struct IngressEventId {
    wire: String,
    namespace: [u8; 32],
    actor_lineage: [u8; ACTOR_INGRESS_LINEAGE_BYTES],
    producer_epoch: u64,
    session_id: Arc<str>,
    run_id: Option<Arc<str>>,
    payload_commitment: [u8; 32],
}

impl fmt::Debug for IngressEventId {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("IngressEventId")
            .field("wire", &"<redacted>")
            .field("producer_epoch", &"<redacted>")
            .finish_non_exhaustive()
    }
}

impl IngressEventId {
    pub(crate) fn presentation_id(&self) -> &str {
        &self.wire
    }
}

/// Non-serializable producer envelope. Every constructor copies the typed ID
/// into the durable DTO, and publication rechecks both copies plus the exact
/// ingress capability before the journal sees the event.
#[derive(Debug, Clone)]
pub(crate) struct AgentLivePublishEvent {
    id: IngressEventId,
    event: MapleLiveEvent,
}

impl AgentLivePublishEvent {
    fn new(id: IngressEventId, event: MapleLiveEvent) -> Self {
        debug_assert_eq!(id.wire, event.event_id());
        Self { id, event }
    }

    pub(crate) fn run_started(id: IngressEventId) -> Self {
        Self::new(id.clone(), MapleLiveEvent::RunStarted { event_id: id.wire })
    }

    pub(crate) fn timeline_upsert(id: IngressEventId, item: MapleLiveTimelineItem) -> Self {
        Self::new(
            id.clone(),
            MapleLiveEvent::TimelineUpsert {
                event_id: id.wire,
                item,
            },
        )
    }

    pub(crate) fn timeline_cleared(id: IngressEventId, reason: MapleLiveClearReason) -> Self {
        Self::new(
            id.clone(),
            MapleLiveEvent::TimelineCleared {
                event_id: id.wire,
                reason,
            },
        )
    }

    pub(crate) fn history_replaced(id: IngressEventId) -> Self {
        Self::new(
            id.clone(),
            MapleLiveEvent::HistoryReplaced { event_id: id.wire },
        )
    }

    pub(crate) fn history_head_committed(
        id: IngressEventId,
        history_revision: String,
        through_event_cursor: LiveEventCursor,
    ) -> Self {
        Self::new(
            id.clone(),
            MapleLiveEvent::HistoryHeadCommitted {
                event_id: id.wire,
                history_revision,
                through_event_cursor,
            },
        )
    }

    pub(crate) fn session_updated(id: IngressEventId, session: MapleLiveSessionSummary) -> Self {
        Self::new(
            id.clone(),
            MapleLiveEvent::SessionUpdated {
                event_id: id.wire,
                session,
            },
        )
    }

    pub(crate) fn run_finished(id: IngressEventId, terminal: MapleLiveRunTerminal) -> Self {
        Self::new(
            id.clone(),
            MapleLiveEvent::RunFinished {
                event_id: id.wire,
                terminal,
            },
        )
    }

    pub(crate) fn session_deleted(id: IngressEventId) -> Self {
        Self::new(
            id.clone(),
            MapleLiveEvent::SessionDeleted { event_id: id.wire },
        )
    }

    pub(crate) fn user_facing_error(id: IngressEventId, error: MapleLiveUserFacingError) -> Self {
        Self::new(
            id.clone(),
            MapleLiveEvent::UserFacingError {
                event_id: id.wire,
                error,
            },
        )
    }

    #[cfg(test)]
    fn from_test_durable(id: IngressEventId, event: MapleLiveEvent) -> Self {
        match event {
            MapleLiveEvent::RunStarted { .. } => Self::run_started(id),
            MapleLiveEvent::TimelineUpsert { item, .. } => Self::timeline_upsert(id, item),
            MapleLiveEvent::TimelineCleared { reason, .. } => Self::timeline_cleared(id, reason),
            MapleLiveEvent::HistoryReplaced { .. } => Self::history_replaced(id),
            MapleLiveEvent::HistoryHeadCommitted {
                history_revision,
                through_event_cursor,
                ..
            } => Self::history_head_committed(id, history_revision, through_event_cursor),
            MapleLiveEvent::SessionUpdated { session, .. } => Self::session_updated(id, session),
            MapleLiveEvent::RunFinished { terminal, .. } => Self::run_finished(id, terminal),
            MapleLiveEvent::SessionDeleted { .. } => Self::session_deleted(id),
            MapleLiveEvent::UserFacingError { error, .. } => Self::user_facing_error(id, error),
        }
    }
}

impl LiveReplayPayload for MapleLiveEvent {
    fn live_replay_event_id(&self) -> &str {
        self.event_id()
    }

    fn validate_live_replay_payload(&self) -> Result<(), LiveEventJournalError> {
        self.validate().map_err(|error| match error {
            AgentLiveProjectionError::TextTooLarge
            | AgentLiveProjectionError::TooManyTimelineItems
            | AgentLiveProjectionError::MergedItemTooLarge
            | AgentLiveProjectionError::AccountProjectionCapacityExceeded => {
                LiveEventJournalError::PayloadTooLarge
            }
            AgentLiveProjectionError::InvalidIdentifier
            | AgentLiveProjectionError::ConflictingItemIdentity
            | AgentLiveProjectionError::ActionablePermission
            | AgentLiveProjectionError::InvalidTimestamp
            | AgentLiveProjectionError::InvalidCount
            | AgentLiveProjectionError::InvalidUserFacingError
            | AgentLiveProjectionError::UnsafePresentation => {
                LiveEventJournalError::InvalidEventOwner
            }
        })
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum AgentLiveProjectionError {
    InvalidIdentifier,
    TextTooLarge,
    TooManyTimelineItems,
    ConflictingItemIdentity,
    ActionablePermission,
    InvalidTimestamp,
    InvalidCount,
    InvalidUserFacingError,
    UnsafePresentation,
    MergedItemTooLarge,
    AccountProjectionCapacityExceeded,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum HeadReloadReason {
    PausedSubscriberOverflow,
    SlowSubscriber,
    JournalReplaced,
    RetentionGap,
    CursorAhead,
    OwnerChanged,
    OrderingLost,
    JournalUnavailable,
    ReseedRequired,
}

/// Terminal lifecycle reason for a coordinator instance. Sealing is distinct
/// from a recoverable head reload: callers must construct a newly owned
/// coordinator before any further publication.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum AgentLiveSealReason {
    OwnerChanged,
    AccountSignedOut,
    ExecutionTargetStopped,
    HostShutdown,
}

#[derive(Debug, PartialEq, Eq)]
pub(crate) enum AgentLiveCoordinatorError {
    InvalidAccountScope,
    InvalidExecutionTarget,
    InvalidSession,
    InvalidRun,
    DataOwnerMismatch,
    StableOperationMismatch,
    ProjectionSchemaMismatch,
    InvalidSubscriptionCapacity,
    InvalidCommandCapacity,
    SubscriberCapacityExceeded,
    IngressRouteCapacityExceeded,
    IngressEpochExhausted,
    IngressRebindRequired,
    StaleHistoryCommit,
    Projection(AgentLiveProjectionError),
    Journal(LiveEventJournalError),
    ReseedRequired(Box<LiveEventJournalReseedRequired>),
    HeadReloadRequired(HeadReloadReason),
    Sealed(AgentLiveSealReason),
    WorkerUnavailable,
    CoordinatorClosed,
}

impl fmt::Display for AgentLiveCoordinatorError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        let message = match self {
            Self::InvalidAccountScope => "live Agent account scope is invalid",
            Self::InvalidExecutionTarget => "live Agent execution target is invalid",
            Self::InvalidSession => "live Agent session owner is invalid",
            Self::InvalidRun => "live Agent run owner is invalid",
            Self::DataOwnerMismatch => {
                "live Agent data owner does not match the journal activation"
            }
            Self::StableOperationMismatch => {
                "durable operation authority does not match the live Agent owner or route"
            }
            Self::ProjectionSchemaMismatch => {
                "durable operation authority uses a different live projection schema"
            }
            Self::InvalidSubscriptionCapacity => "live Agent subscription capacity is invalid",
            Self::InvalidCommandCapacity => "live Agent command capacity is invalid",
            Self::SubscriberCapacityExceeded => "live Agent account has too many subscribers",
            Self::IngressRouteCapacityExceeded => {
                "live Agent account has too many admitted producer routes"
            }
            Self::IngressEpochExhausted => "live Agent producer epoch is exhausted",
            Self::IngressRebindRequired => {
                "live Agent producer must explicitly bind to the current journal generation"
            }
            Self::StaleHistoryCommit => {
                "Agent history advanced after the persisted-head acknowledgement"
            }
            Self::Projection(_) => "live Agent projection is invalid",
            Self::Journal(_) => "live Agent journal operation failed",
            Self::ReseedRequired(_) => {
                "the live Agent journal requires a verified authoritative reseed"
            }
            Self::HeadReloadRequired(_) => "the Agent history head must be reloaded",
            Self::Sealed(_) => "the live Agent coordinator is sealed",
            Self::WorkerUnavailable => "live Agent journal worker is unavailable",
            Self::CoordinatorClosed => "live Agent coordinator is closed",
        };
        formatter.write_str(message)
    }
}

impl std::error::Error for AgentLiveCoordinatorError {}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct AgentLiveDelivery {
    pub(crate) cursor: LiveEventCursor,
    pub(crate) session_id: String,
    pub(crate) run_id: Option<String>,
    pub(crate) event: MapleLiveEvent,
}

impl AgentLiveDelivery {
    pub(crate) fn validate(&self) -> Result<(), AgentLiveCoordinatorError> {
        validate_owner_id(&self.session_id).map_err(AgentLiveCoordinatorError::Projection)?;
        if let Some(run_id) = self.run_id.as_deref() {
            validate_owner_id(run_id).map_err(AgentLiveCoordinatorError::Projection)?;
        }
        self.event
            .validate()
            .map_err(AgentLiveCoordinatorError::Projection)?;
        validate_event_route(&self.session_id, self.run_id.as_deref(), &self.event)
    }
}

#[derive(Clone)]
pub(crate) struct AgentLiveCoordinator {
    data_owner: AgentLiveDataOwnerKey,
    execution_target: Arc<str>,
    commands: mpsc::Sender<CoordinatorCommand>,
}

impl AgentLiveCoordinator {
    /// Start from the exact opaque lease activated by the host while holding
    /// its account/target lifecycle lock. The host must derive the owner with
    /// [`target_bound_owner`] before activation; this coordinator never
    /// reconstructs authority from raw account-shaped values.
    pub(crate) async fn start_activated(
        journal: LiveEventJournal<MapleLiveEvent>,
        lease: LiveEventJournalLease,
        data_owner: AgentLiveDataOwnerKey,
        execution_target: impl Into<String>,
    ) -> Result<Self, AgentLiveCoordinatorError> {
        let execution_target = execution_target.into();
        validate_identifier(&execution_target, MAX_EXECUTION_TARGET_BYTES)
            .map_err(|_| AgentLiveCoordinatorError::InvalidExecutionTarget)?;
        if lease.account_generation() != data_owner.account_generation() {
            return Err(AgentLiveCoordinatorError::DataOwnerMismatch);
        }
        if execution_target != data_owner.execution_target() {
            return Err(AgentLiveCoordinatorError::InvalidExecutionTarget);
        }
        Self::start_with_backend(
            Arc::new(journal),
            lease,
            data_owner,
            execution_target,
            DEFAULT_COMMAND_CAPACITY,
        )
        .await
    }

    #[cfg(test)]
    async fn start(
        journal: LiveEventJournal<MapleLiveEvent>,
        opaque_account_scope: &str,
        account_generation: u64,
        execution_target: impl Into<String>,
    ) -> Result<Self, AgentLiveCoordinatorError> {
        let execution_target = execution_target.into();
        validate_identifier(&execution_target, MAX_EXECUTION_TARGET_BYTES)
            .map_err(|_| AgentLiveCoordinatorError::InvalidExecutionTarget)?;
        let owner =
            target_bound_owner(opaque_account_scope, account_generation, &execution_target)?;
        let lease = journal
            .activate_account(&owner)
            .map_err(map_journal_activation)?;
        let data_owner = AgentLiveDataOwnerKey::for_test(
            opaque_account_scope,
            account_generation,
            execution_target.clone(),
            0,
        );
        Self::start_activated(journal, lease, data_owner, execution_target).await
    }

    async fn start_with_backend(
        journal: Arc<dyn CoordinatorJournal>,
        lease: LiveEventJournalLease,
        data_owner: AgentLiveDataOwnerKey,
        execution_target: String,
        command_capacity: usize,
    ) -> Result<Self, AgentLiveCoordinatorError> {
        validate_identifier(&execution_target, MAX_EXECUTION_TARGET_BYTES)
            .map_err(|_| AgentLiveCoordinatorError::InvalidExecutionTarget)?;
        if command_capacity == 0 || command_capacity > MAX_COMMAND_CAPACITY {
            return Err(AgentLiveCoordinatorError::InvalidCommandCapacity);
        }

        // The exact opaque account/target lease is captured once here and
        // carried by every blocking operation. No publish/attach call can
        // substitute owner-shaped data or revive a retired journal.
        let disk = BlockingJournalWorker::spawn(journal, lease)?;
        let durable_cursor = disk.checkpoint().await.map_err(map_journal_for_attach)?;
        let projection_checkpoint = disk
            .load_projection_checkpoint()
            .await
            .map_err(map_journal_for_attach)?;
        let (commands, receiver) = mpsc::channel(command_capacity);
        let actor = CoordinatorActor::load(
            disk,
            data_owner.clone(),
            durable_cursor,
            projection_checkpoint,
        )
        .await?;
        tokio::spawn(actor.run(receiver));
        Ok(Self {
            data_owner,
            execution_target: Arc::from(execution_target),
            commands,
        })
    }

    pub(crate) fn execution_target(&self) -> &str {
        &self.execution_target
    }

    /// Explicitly admit one producer route at the actor's FIFO. This never runs
    /// inside `publish`: after a rollover or producer supersession, the native
    /// runtime must deliberately bind again before constructing any new event.
    pub(crate) async fn begin_ingress(
        &self,
        session_id: impl Into<String>,
        run_id: Option<String>,
    ) -> Result<AgentLiveIngressLease, AgentLiveCoordinatorError> {
        let session_id = session_id.into();
        validate_owner_id(&session_id).map_err(|_| AgentLiveCoordinatorError::InvalidSession)?;
        if let Some(run_id) = run_id.as_deref() {
            validate_owner_id(run_id).map_err(|_| AgentLiveCoordinatorError::InvalidRun)?;
        }
        let (reply, response) = oneshot::channel();
        self.commands
            .send(CoordinatorCommand::BeginIngress {
                session_id,
                run_id,
                reply,
            })
            .await
            .map_err(|_| AgentLiveCoordinatorError::CoordinatorClosed)?;
        response
            .await
            .map_err(|_| AgentLiveCoordinatorError::CoordinatorClosed)?
    }

    /// Durably append before updating the absolute overlay or notifying any
    /// subscriber. The route and event ID come exclusively from the exact
    /// opaque producer lease; publication never looks up or swaps to a current
    /// ingress capability on the caller's behalf.
    pub(crate) async fn publish(
        &self,
        ingress: &AgentLiveIngressLease,
        event: AgentLivePublishEvent,
    ) -> Result<LiveEventCursor, AgentLiveCoordinatorError> {
        event
            .event
            .validate()
            .map_err(AgentLiveCoordinatorError::Projection)?;
        validate_event_route(ingress.session_id(), ingress.run_id(), &event.event)?;
        let (reply, response) = oneshot::channel();
        self.commands
            .send(CoordinatorCommand::Publish {
                ingress: ingress.clone(),
                event,
                reply,
            })
            .await
            .map_err(|_| AgentLiveCoordinatorError::CoordinatorClosed)?;
        response
            .await
            .map_err(|_| AgentLiveCoordinatorError::CoordinatorClosed)?
    }

    /// FIFO barrier C0 plus an authoritative, order-stable absolute overlay.
    /// The returned subscriber remains paused until its token is finalized.
    pub(crate) async fn begin_account_head_attach(
        &self,
        capacity: Option<usize>,
    ) -> Result<AgentHeadAttach, AgentLiveCoordinatorError> {
        let capacity = validate_subscription_capacity(capacity)?;
        let (sender, receiver) = broadcast::channel(capacity);
        let terminal_reason = Arc::new(TerminalMutex::new(None));
        let (reply, response) = oneshot::channel();
        self.commands
            .send(CoordinatorCommand::BeginHeadAttach {
                capacity,
                sender,
                terminal_reason: Arc::clone(&terminal_reason),
                cancellation_commands: self.commands.clone(),
                reply,
            })
            .await
            .map_err(|_| AgentLiveCoordinatorError::CoordinatorClosed)?;
        let begun = response
            .await
            .map_err(|_| AgentLiveCoordinatorError::CoordinatorClosed)??;
        let subscriber_id = begun.subscriber.transfer();
        Ok(AgentHeadAttach {
            through_cursor: begun.through_cursor,
            live_sessions_complete: true,
            live_sessions: begun.live_sessions,
            token: AgentHeadAttachToken {
                subscriber_id: Some(subscriber_id),
                commands: self.commands.clone(),
                receiver: Some(receiver),
                terminal_reason,
            },
        })
    }

    /// Cursor-first replay for an already attached client. The FIFO barrier is
    /// held until the bounded replay is enqueued and the subscriber is live.
    pub(crate) async fn begin_resume(
        &self,
        cursor: LiveEventCursor,
        capacity: Option<usize>,
    ) -> Result<AgentLiveResume, AgentLiveCoordinatorError> {
        cursor
            .validate()
            .map_err(AgentLiveCoordinatorError::Journal)?;
        let capacity = validate_subscription_capacity(capacity)?;
        let (sender, receiver) = broadcast::channel(capacity);
        let terminal_reason = Arc::new(TerminalMutex::new(None));
        let (reply, response) = oneshot::channel();
        self.commands
            .send(CoordinatorCommand::BeginResume {
                cursor,
                capacity,
                sender,
                terminal_reason: Arc::clone(&terminal_reason),
                cancellation_commands: self.commands.clone(),
                reply,
            })
            .await
            .map_err(|_| AgentLiveCoordinatorError::CoordinatorClosed)?;
        let begun = response
            .await
            .map_err(|_| AgentLiveCoordinatorError::CoordinatorClosed)??;
        let subscriber_id = begun.subscriber.transfer();
        Ok(AgentLiveResume {
            through_cursor: begun.through_cursor,
            subscription: AgentLiveSubscription {
                subscriber_id: Some(subscriber_id),
                commands: self.commands.clone(),
                receiver,
                terminal_reason,
            },
        })
    }

    /// FIFO lifecycle barrier. Every command accepted before this command is
    /// handled first; once it returns, pending attaches and active subscribers
    /// are closed and every later mutation is rejected with the seal reason.
    pub(crate) async fn seal(
        &self,
        reason: AgentLiveSealReason,
    ) -> Result<AgentLiveSeal, AgentLiveCoordinatorError> {
        let (reply, response) = oneshot::channel();
        self.commands
            .send(CoordinatorCommand::Seal { reason, reply })
            .await
            .map_err(|_| AgentLiveCoordinatorError::CoordinatorClosed)?;
        response
            .await
            .map_err(|_| AgentLiveCoordinatorError::CoordinatorClosed)?
    }

    /// Retire one session's absolute live suffix only after the caller has
    /// durably replaced that session's Goose head through the supplied account
    /// cursor. `history_revision` must cover every projected run/item for the
    /// session; the coordinator can fence ordering but cannot inspect Goose.
    pub(crate) async fn acknowledge_persisted_head(
        &self,
        ingress: &AgentLiveIngressLease,
        event_id: IngressEventId,
        history_revision: impl Into<String>,
        through_event_cursor: LiveEventCursor,
    ) -> Result<LiveEventCursor, AgentLiveCoordinatorError> {
        self.publish(
            ingress,
            AgentLivePublishEvent::history_head_committed(
                event_id,
                history_revision.into(),
                through_event_cursor,
            ),
        )
        .await
    }

    #[cfg(test)]
    async fn publish_for_test(
        &self,
        session_id: impl Into<String>,
        run_id: Option<String>,
        event: MapleLiveEvent,
    ) -> Result<LiveEventCursor, AgentLiveCoordinatorError> {
        let session_id = session_id.into();
        let ingress = self
            .begin_ingress(session_id.clone(), run_id.clone())
            .await?;
        let event = self.publish_event_for_test(&ingress, event)?;
        self.publish(&ingress, event).await
    }

    #[cfg(test)]
    fn publish_event_for_test(
        &self,
        ingress: &AgentLiveIngressLease,
        event: MapleLiveEvent,
    ) -> Result<AgentLivePublishEvent, AgentLiveCoordinatorError> {
        let payload_commitment =
            live_event_payload_commitment(ingress.session_id(), ingress.run_id(), &event)?;
        let stable_operation = AgentDurableStableOperationId::for_test(
            self.data_owner.clone(),
            ingress.session_id(),
            ingress.run_id().map(str::to_string),
            event.event_id(),
            ingress.namespace,
            payload_commitment,
        );
        let event_id = ingress.event_id(&stable_operation)?;
        Ok(AgentLivePublishEvent::from_test_durable(event_id, event))
    }

    #[cfg(test)]
    async fn acknowledge_persisted_head_for_test(
        &self,
        session_id: impl Into<String>,
        stable_operation_id: impl Into<String>,
        history_revision: impl Into<String>,
        through_event_cursor: LiveEventCursor,
    ) -> Result<LiveEventCursor, AgentLiveCoordinatorError> {
        self.publish_for_test(
            session_id,
            None,
            MapleLiveEvent::HistoryHeadCommitted {
                event_id: stable_operation_id.into(),
                history_revision: history_revision.into(),
                through_event_cursor,
            },
        )
        .await
    }
}

/// FIFO proof that this coordinator stopped accepting mutations at one exact
/// durable head. The host may pass these fields directly to
/// `LiveEventJournal::seal_for_retirement` before retiring the account file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct AgentLiveSeal {
    pub(crate) journal_lease: LiveEventJournalLease,
    pub(crate) through_cursor: LiveEventCursor,
    pub(crate) reason: AgentLiveSealReason,
}

pub(crate) struct AgentHeadAttach {
    pub(crate) through_cursor: LiveEventCursor,
    /// Always true for this v1 coordinator. Consumers must clear cached live
    /// overlays for sessions absent from `live_sessions` at this same C0.
    pub(crate) live_sessions_complete: bool,
    pub(crate) live_sessions: Vec<AgentLiveSessionProjection>,
    pub(crate) token: AgentHeadAttachToken,
}

impl AgentHeadAttach {
    pub(crate) fn live_items_for_session(&self, session_id: &str) -> &[MapleLiveTimelineItem] {
        self.live_sessions
            .iter()
            .find(|session| session.session_id == session_id)
            .map(|session| session.live_items.as_slice())
            .unwrap_or_default()
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct AgentLiveSessionProjection {
    pub(crate) session_id: String,
    /// Empty is authoritative, not "snapshot omitted".
    pub(crate) live_items: Vec<MapleLiveTimelineItem>,
}

const LIVE_PROJECTION_CHECKPOINT_VERSION: u8 = 1;

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct CoordinatorProjectionCheckpoint {
    format_version: u8,
    live_sessions: Vec<AgentLiveSessionProjection>,
}

fn decode_projection_checkpoint(
    bytes: &[u8],
) -> Result<HashMap<String, SessionLiveProjection>, AgentLiveCoordinatorError> {
    if bytes.is_empty() || bytes.len() > MAX_LIVE_PROJECTION_BYTES_PER_ACCOUNT {
        return Err(invalid_projection_checkpoint());
    }
    let checkpoint: CoordinatorProjectionCheckpoint =
        serde_json::from_slice(bytes).map_err(|_| invalid_projection_checkpoint())?;
    if checkpoint.format_version != LIVE_PROJECTION_CHECKPOINT_VERSION
        || checkpoint.live_sessions.len() > MAX_LIVE_SESSIONS_PER_ACCOUNT
    {
        return Err(invalid_projection_checkpoint());
    }

    let mut sessions = HashMap::with_capacity(checkpoint.live_sessions.len());
    let mut previous_session_id: Option<String> = None;
    for live_session in checkpoint.live_sessions {
        if live_session.live_items.is_empty()
            || live_session.live_items.len() > MAX_LIVE_ITEMS_PER_SESSION
            || validate_owner_id(&live_session.session_id).is_err()
            || previous_session_id
                .as_deref()
                .is_some_and(|previous| previous >= live_session.session_id.as_str())
        {
            return Err(invalid_projection_checkpoint());
        }
        let session_id = live_session.session_id;
        previous_session_id = Some(session_id.clone());
        let mut projection = SessionLiveProjection::default();
        for item in live_session.live_items {
            if item.merge != MapleLiveMerge::Replace
                || projection.items.iter().any(|known| known.id == item.id)
            {
                return Err(invalid_projection_checkpoint());
            }
            projection
                .upsert(item)
                .map_err(|_| invalid_projection_checkpoint())?;
        }
        if sessions.insert(session_id, projection).is_some() {
            return Err(invalid_projection_checkpoint());
        }
    }

    let mut item_count = 0usize;
    let mut projected_bytes = LIVE_CHECKPOINT_OUTER_OVERHEAD_BYTES;
    for (session_id, projection) in &sessions {
        accumulate_projection_bounds(
            session_id,
            projection,
            &mut item_count,
            &mut projected_bytes,
        )
        .map_err(|_| invalid_projection_checkpoint())?;
    }
    if item_count > MAX_LIVE_ITEMS_PER_ACCOUNT
        || projected_bytes > MAX_LIVE_PROJECTION_BYTES_PER_ACCOUNT
    {
        return Err(invalid_projection_checkpoint());
    }
    Ok(sessions)
}

fn invalid_projection_checkpoint() -> AgentLiveCoordinatorError {
    AgentLiveCoordinatorError::Journal(LiveEventJournalError::InvalidCheckpoint)
}

fn projection_checkpoint_matches(
    checkpoint: Option<&LiveProjectionCheckpoint>,
    through: &LiveEventCursor,
    bytes: &[u8],
) -> bool {
    checkpoint.is_some_and(|checkpoint| {
        checkpoint.through_cursor == *through && checkpoint.bytes == bytes
    })
}

fn set_terminal_reason(
    target: &Arc<TerminalMutex<Option<HeadReloadReason>>>,
    reason: HeadReloadReason,
) {
    if let Ok(mut terminal) = target.lock() {
        *terminal = Some(reason);
    }
}

pub(crate) struct AgentHeadAttachToken {
    subscriber_id: Option<u64>,
    commands: mpsc::Sender<CoordinatorCommand>,
    receiver: Option<broadcast::Receiver<AgentLiveDelivery>>,
    terminal_reason: Arc<TerminalMutex<Option<HeadReloadReason>>>,
}

impl AgentHeadAttachToken {
    /// Replay C0..C1, then resume the same subscriber. Events published while
    /// Goose history was loading are delivered exactly once through `recv`.
    pub(crate) async fn finalize(mut self) -> Result<AgentLiveResume, AgentLiveCoordinatorError> {
        let subscriber_id = self
            .subscriber_id
            .take()
            .ok_or(AgentLiveCoordinatorError::CoordinatorClosed)?;
        let mut cancellation_guard =
            SubscriberCancellationGuard::new(subscriber_id, self.commands.clone());
        let (reply, response) = oneshot::channel();
        self.commands
            .send(CoordinatorCommand::FinalizeHeadAttach {
                subscriber_id,
                reply,
            })
            .await
            .map_err(|_| AgentLiveCoordinatorError::CoordinatorClosed)?;
        let through_cursor = match response.await {
            Ok(Ok(cursor)) => cursor,
            Ok(Err(AgentLiveCoordinatorError::CoordinatorClosed)) => {
                return Err(terminal_coordinator_error(&self.terminal_reason)
                    .unwrap_or(AgentLiveCoordinatorError::CoordinatorClosed));
            }
            Ok(Err(error)) => return Err(error),
            Err(_) => {
                return Err(terminal_coordinator_error(&self.terminal_reason)
                    .unwrap_or(AgentLiveCoordinatorError::CoordinatorClosed));
            }
        };
        let receiver = self
            .receiver
            .take()
            .ok_or(AgentLiveCoordinatorError::CoordinatorClosed)?;
        let resume = AgentLiveResume {
            through_cursor,
            subscription: AgentLiveSubscription {
                subscriber_id: Some(subscriber_id),
                commands: self.commands.clone(),
                receiver,
                terminal_reason: Arc::clone(&self.terminal_reason),
            },
        };
        cancellation_guard.disarm();
        Ok(resume)
    }

    /// Cancel a paused attach and wait until the actor has released its bounded
    /// subscriber reservation. Dropping the token is only best-effort; callers
    /// that need a lifecycle barrier should use this acknowledged API.
    pub(crate) async fn cancel(mut self) -> Result<(), AgentLiveCoordinatorError> {
        let subscriber_id = self
            .subscriber_id
            .take()
            .ok_or(AgentLiveCoordinatorError::CoordinatorClosed)?;
        let mut cancellation_guard =
            SubscriberCancellationGuard::new(subscriber_id, self.commands.clone());
        unsubscribe_with_ack(&self.commands, subscriber_id).await?;
        cancellation_guard.disarm();
        Ok(())
    }
}

impl Drop for AgentHeadAttachToken {
    fn drop(&mut self) {
        if let Some(subscriber_id) = self.subscriber_id.take() {
            try_unsubscribe(&self.commands, subscriber_id);
        }
    }
}

pub(crate) struct AgentLiveResume {
    pub(crate) through_cursor: LiveEventCursor,
    pub(crate) subscription: AgentLiveSubscription,
}

pub(crate) struct AgentLiveSubscription {
    subscriber_id: Option<u64>,
    commands: mpsc::Sender<CoordinatorCommand>,
    receiver: broadcast::Receiver<AgentLiveDelivery>,
    terminal_reason: Arc<TerminalMutex<Option<HeadReloadReason>>>,
}

impl AgentLiveSubscription {
    pub(crate) async fn recv(&mut self) -> Result<AgentLiveDelivery, AgentLiveReceiveError> {
        self.receiver.recv().await.map_err(|error| match error {
            broadcast::error::RecvError::Lagged(_) => {
                AgentLiveReceiveError::HeadReloadRequired(HeadReloadReason::SlowSubscriber)
            }
            broadcast::error::RecvError::Closed => self
                .terminal_reason
                .lock()
                .ok()
                .and_then(|reason| *reason)
                .map(AgentLiveReceiveError::HeadReloadRequired)
                .unwrap_or(AgentLiveReceiveError::Closed),
        })
    }

    /// Unregister this active subscription and wait for the actor to reclaim
    /// its aggregate buffer budget. This is safe after a seal or reload fence.
    pub(crate) async fn unsubscribe(mut self) -> Result<(), AgentLiveCoordinatorError> {
        let subscriber_id = self
            .subscriber_id
            .take()
            .ok_or(AgentLiveCoordinatorError::CoordinatorClosed)?;
        let mut cancellation_guard =
            SubscriberCancellationGuard::new(subscriber_id, self.commands.clone());
        unsubscribe_with_ack(&self.commands, subscriber_id).await?;
        cancellation_guard.disarm();
        Ok(())
    }
}

impl Drop for AgentLiveSubscription {
    fn drop(&mut self) {
        if let Some(subscriber_id) = self.subscriber_id.take() {
            try_unsubscribe(&self.commands, subscriber_id);
        }
    }
}

/// Cancellation safety for async subscriber lifecycle operations. Once an ID
/// leaves its owning token/subscription, this guard retains the cleanup duty
/// across every await until the actor acknowledges removal or ownership is
/// transferred into a successfully constructed live subscription.
struct SubscriberCancellationGuard {
    subscriber_id: Option<u64>,
    commands: mpsc::Sender<CoordinatorCommand>,
}

impl SubscriberCancellationGuard {
    fn new(subscriber_id: u64, commands: mpsc::Sender<CoordinatorCommand>) -> Self {
        Self {
            subscriber_id: Some(subscriber_id),
            commands,
        }
    }

    fn disarm(&mut self) {
        self.subscriber_id = None;
    }
}

impl Drop for SubscriberCancellationGuard {
    fn drop(&mut self) {
        if let Some(subscriber_id) = self.subscriber_id.take() {
            try_unsubscribe(&self.commands, subscriber_id);
        }
    }
}

async fn unsubscribe_with_ack(
    commands: &mpsc::Sender<CoordinatorCommand>,
    subscriber_id: u64,
) -> Result<(), AgentLiveCoordinatorError> {
    let (reply, response) = oneshot::channel();
    commands
        .send(CoordinatorCommand::Unsubscribe {
            subscriber_id,
            reply: Some(reply),
        })
        .await
        .map_err(|_| AgentLiveCoordinatorError::CoordinatorClosed)?;
    response
        .await
        .map_err(|_| AgentLiveCoordinatorError::CoordinatorClosed)?
}

fn try_unsubscribe(commands: &mpsc::Sender<CoordinatorCommand>, subscriber_id: u64) {
    let command = CoordinatorCommand::Unsubscribe {
        subscriber_id,
        reply: None,
    };
    if let Err(mpsc::error::TrySendError::Full(command)) = commands.try_send(command) {
        // A Drop can run while the bounded actor queue is full (including when
        // cancellation interrupts the original send). Preserve bounded
        // backpressure by waiting in one runtime task rather than losing the
        // unregister operation or introducing an unbounded side channel.
        let commands = commands.clone();
        if let Ok(runtime) = tokio::runtime::Handle::try_current() {
            runtime.spawn(async move {
                let _ = commands.send(command).await;
            });
        }
    }
}

fn terminal_coordinator_error(
    reason: &Arc<TerminalMutex<Option<HeadReloadReason>>>,
) -> Option<AgentLiveCoordinatorError> {
    reason
        .lock()
        .ok()
        .and_then(|reason| *reason)
        .map(AgentLiveCoordinatorError::HeadReloadRequired)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum AgentLiveReceiveError {
    HeadReloadRequired(HeadReloadReason),
    Closed,
}

struct PendingSubscriberOwnership {
    subscriber_id: Option<u64>,
    commands: mpsc::Sender<CoordinatorCommand>,
}

impl PendingSubscriberOwnership {
    fn new(subscriber_id: u64, commands: mpsc::Sender<CoordinatorCommand>) -> Self {
        Self {
            subscriber_id: Some(subscriber_id),
            commands,
        }
    }

    fn transfer(mut self) -> u64 {
        self.subscriber_id
            .take()
            .expect("pending subscriber ownership transfers once")
    }
}

impl Drop for PendingSubscriberOwnership {
    fn drop(&mut self) {
        if let Some(subscriber_id) = self.subscriber_id.take() {
            try_unsubscribe(&self.commands, subscriber_id);
        }
    }
}

struct BeginHeadAttachResult {
    subscriber: PendingSubscriberOwnership,
    through_cursor: LiveEventCursor,
    live_sessions: Vec<AgentLiveSessionProjection>,
}

struct BeginResumeResult {
    subscriber: PendingSubscriberOwnership,
    through_cursor: LiveEventCursor,
}

#[allow(
    clippy::large_enum_variant,
    reason = "the bounded FIFO command owns complete attach state until the actor accepts it"
)]
enum CoordinatorCommand {
    BeginIngress {
        session_id: String,
        run_id: Option<String>,
        reply: oneshot::Sender<Result<AgentLiveIngressLease, AgentLiveCoordinatorError>>,
    },
    Publish {
        ingress: AgentLiveIngressLease,
        event: AgentLivePublishEvent,
        reply: oneshot::Sender<Result<LiveEventCursor, AgentLiveCoordinatorError>>,
    },
    BeginHeadAttach {
        capacity: usize,
        sender: broadcast::Sender<AgentLiveDelivery>,
        terminal_reason: Arc<TerminalMutex<Option<HeadReloadReason>>>,
        cancellation_commands: mpsc::Sender<CoordinatorCommand>,
        reply: oneshot::Sender<Result<BeginHeadAttachResult, AgentLiveCoordinatorError>>,
    },
    FinalizeHeadAttach {
        subscriber_id: u64,
        reply: oneshot::Sender<Result<LiveEventCursor, AgentLiveCoordinatorError>>,
    },
    BeginResume {
        cursor: LiveEventCursor,
        capacity: usize,
        sender: broadcast::Sender<AgentLiveDelivery>,
        terminal_reason: Arc<TerminalMutex<Option<HeadReloadReason>>>,
        cancellation_commands: mpsc::Sender<CoordinatorCommand>,
        reply: oneshot::Sender<Result<BeginResumeResult, AgentLiveCoordinatorError>>,
    },
    Unsubscribe {
        subscriber_id: u64,
        reply: Option<oneshot::Sender<Result<(), AgentLiveCoordinatorError>>>,
    },
    Seal {
        reason: AgentLiveSealReason,
        reply: oneshot::Sender<Result<AgentLiveSeal, AgentLiveCoordinatorError>>,
    },
}

struct CoordinatorActor {
    disk: BlockingJournalWorker,
    data_owner: AgentLiveDataOwnerKey,
    durable_cursor: LiveEventCursor,
    pending_rollover: Option<PendingRollover>,
    actor_lineage: [u8; ACTOR_INGRESS_LINEAGE_BYTES],
    next_producer_epoch: u64,
    ingress_epochs: HashMap<IngressRoute, u64>,
    sessions: HashMap<String, SessionLiveProjection>,
    subscribers: HashMap<u64, SubscriberState>,
    next_subscriber_id: u64,
    poison: Option<HeadReloadReason>,
    sealed: Option<AgentLiveSealReason>,
    seal_result: Option<AgentLiveSeal>,
}

#[derive(Clone, PartialEq, Eq, Hash)]
struct IngressRoute {
    session_id: String,
    run_id: Option<String>,
}

/// The underlying capability is move-only. The actor retains the sole owning
/// allocation while its blocking worker borrows it through a short-lived
/// `Arc`; an ambiguous commit acknowledgement can therefore never discard or
/// substitute the exact prepared rollover obligation.
struct PendingRollover {
    obligation: Arc<LiveEventJournalRolloverObligation>,
    previous_cursor: LiveEventCursor,
    checkpoint_bytes: Vec<u8>,
}

impl CoordinatorActor {
    async fn load(
        disk: BlockingJournalWorker,
        data_owner: AgentLiveDataOwnerKey,
        durable_cursor: LiveEventCursor,
        projection_checkpoint: Option<LiveProjectionCheckpoint>,
    ) -> Result<Self, AgentLiveCoordinatorError> {
        let mut actor_lineage = [0u8; ACTOR_INGRESS_LINEAGE_BYTES];
        getrandom::fill(&mut actor_lineage)
            .map_err(|_| AgentLiveCoordinatorError::WorkerUnavailable)?;
        let (sessions, replay_cursor) = match projection_checkpoint {
            Some(checkpoint) => {
                if checkpoint.through_cursor.journal_id() != durable_cursor.journal_id() {
                    return Err(invalid_projection_checkpoint());
                }
                if checkpoint.through_cursor.sequence() > durable_cursor.sequence() {
                    return Err(invalid_projection_checkpoint());
                }
                (
                    decode_projection_checkpoint(&checkpoint.bytes)?,
                    checkpoint.through_cursor,
                )
            }
            None => (HashMap::new(), durable_cursor.beginning()),
        };
        let mut actor = Self {
            disk,
            data_owner,
            durable_cursor: durable_cursor.clone(),
            pending_rollover: None,
            actor_lineage,
            next_producer_epoch: 1,
            ingress_epochs: HashMap::new(),
            sessions,
            subscribers: HashMap::new(),
            next_subscriber_id: 1,
            poison: None,
            sealed: None,
            seal_result: None,
        };
        if replay_cursor != durable_cursor {
            let entries = actor
                .replay_until(replay_cursor.clone(), &durable_cursor)
                .await?;
            let mut applied_cursor = replay_cursor;
            for delivery in entries {
                if delivery.cursor.journal_id() != applied_cursor.journal_id()
                    || delivery.cursor.sequence()
                        != applied_cursor.sequence().checked_add(1).ok_or(
                            AgentLiveCoordinatorError::HeadReloadRequired(
                                HeadReloadReason::OrderingLost,
                            ),
                        )?
                {
                    return Err(AgentLiveCoordinatorError::HeadReloadRequired(
                        HeadReloadReason::OrderingLost,
                    ));
                }
                validate_event_route(
                    &delivery.session_id,
                    delivery.run_id.as_deref(),
                    &delivery.event,
                )?;
                if let MapleLiveEvent::HistoryHeadCommitted {
                    through_event_cursor,
                    ..
                } = &delivery.event
                {
                    if through_event_cursor != &applied_cursor {
                        return Err(AgentLiveCoordinatorError::HeadReloadRequired(
                            HeadReloadReason::OrderingLost,
                        ));
                    }
                }
                let mutation = actor.prepare_mutation(&delivery.session_id, &delivery.event)?;
                actor.apply_mutation(&delivery.session_id, mutation);
                applied_cursor = delivery.cursor;
            }
            if applied_cursor != durable_cursor {
                return Err(AgentLiveCoordinatorError::HeadReloadRequired(
                    HeadReloadReason::OrderingLost,
                ));
            }
        }
        let verified = actor
            .disk
            .checkpoint()
            .await
            .map_err(map_journal_for_attach)?;
        if verified != durable_cursor {
            return Err(AgentLiveCoordinatorError::HeadReloadRequired(
                HeadReloadReason::OrderingLost,
            ));
        }
        Ok(actor)
    }

    async fn run(mut self, mut commands: mpsc::Receiver<CoordinatorCommand>) {
        while let Some(command) = commands.recv().await {
            match command {
                CoordinatorCommand::BeginIngress {
                    session_id,
                    run_id,
                    reply,
                } => {
                    let result = self.begin_ingress(session_id, run_id).await;
                    let _ = reply.send(result);
                }
                CoordinatorCommand::Publish {
                    ingress,
                    event,
                    reply,
                } => {
                    let result = self.publish(ingress, event).await;
                    let _ = reply.send(result);
                }
                CoordinatorCommand::BeginHeadAttach {
                    capacity,
                    sender,
                    terminal_reason,
                    cancellation_commands,
                    reply,
                } => {
                    let result = self
                        .begin_head_attach(capacity, sender, terminal_reason, cancellation_commands)
                        .await;
                    let _ = reply.send(result);
                }
                CoordinatorCommand::FinalizeHeadAttach {
                    subscriber_id,
                    reply,
                } => {
                    let result = self.finalize_head_attach(subscriber_id).await;
                    let _ = reply.send(result);
                }
                CoordinatorCommand::BeginResume {
                    cursor,
                    capacity,
                    sender,
                    terminal_reason,
                    cancellation_commands,
                    reply,
                } => {
                    let result = self
                        .begin_resume(
                            cursor,
                            capacity,
                            sender,
                            terminal_reason,
                            cancellation_commands,
                        )
                        .await;
                    let _ = reply.send(result);
                }
                CoordinatorCommand::Unsubscribe {
                    subscriber_id,
                    reply,
                } => {
                    self.subscribers.remove(&subscriber_id);
                    if let Some(reply) = reply {
                        let _ = reply.send(Ok(()));
                    }
                }
                CoordinatorCommand::Seal { reason, reply } => {
                    let result = self.seal(reason).await;
                    let _ = reply.send(result);
                }
            }
        }
    }

    async fn begin_ingress(
        &mut self,
        session_id: String,
        run_id: Option<String>,
    ) -> Result<AgentLiveIngressLease, AgentLiveCoordinatorError> {
        self.ensure_ready().await?;
        let route = IngressRoute {
            session_id: session_id.clone(),
            run_id: run_id.clone(),
        };
        if !self.ingress_epochs.contains_key(&route)
            && self.ingress_epochs.len() >= MAX_INGRESS_ROUTES_PER_ACCOUNT
        {
            return Err(AgentLiveCoordinatorError::IngressRouteCapacityExceeded);
        }
        let producer_epoch = self.next_producer_epoch;
        self.next_producer_epoch = self
            .next_producer_epoch
            .checked_add(1)
            .ok_or(AgentLiveCoordinatorError::IngressEpochExhausted)?;
        let journal_ingress = self
            .disk
            .bind_ingress()
            .await
            .map_err(|error| self.map_mutation_journal_error(error))?;
        let namespace = journal_ingress.event_namespace_commitment();
        self.ingress_epochs.insert(route, producer_epoch);
        Ok(AgentLiveIngressLease {
            journal_ingress,
            data_owner: self.data_owner.clone(),
            namespace,
            actor_lineage: self.actor_lineage,
            producer_epoch,
            session_id: Arc::from(session_id),
            run_id: run_id.map(Arc::from),
        })
    }

    async fn publish(
        &mut self,
        ingress: AgentLiveIngressLease,
        event: AgentLivePublishEvent,
    ) -> Result<LiveEventCursor, AgentLiveCoordinatorError> {
        // A command arriving after an ambiguous rollover acknowledgement may
        // drive only the exact prepared obligation to completion. No classify,
        // append, projection mutation, or subscriber registration can cross
        // the generation fence while that obligation remains pending.
        self.ensure_ready().await?;
        self.validate_ingress_event(&ingress, &event)?;
        let session_id = ingress.session_id.to_string();
        let run_id = ingress.run_id.as_deref().map(str::to_string);
        let event = event.event;
        match self
            .classify_durable(
                ingress.journal_ingress.clone(),
                &session_id,
                run_id.as_deref(),
                event.clone(),
            )
            .await?
        {
            EventAdmission::New => {}
            EventAdmission::Duplicate {
                event_cursor,
                head_cursor,
            } => {
                return self.recover_or_return_duplicate(
                    session_id,
                    run_id,
                    event,
                    event_cursor,
                    head_cursor,
                );
            }
        }
        if let MapleLiveEvent::HistoryHeadCommitted {
            through_event_cursor,
            ..
        } = &event
        {
            if through_event_cursor != &self.durable_cursor {
                return Err(AgentLiveCoordinatorError::StaleHistoryCommit);
            }
        }
        let mutation = self.prepare_mutation(&session_id, &event)?;
        let expected_head = self.durable_cursor.clone();
        let mut outcome = self
            .append_durable(
                ingress.journal_ingress.clone(),
                expected_head.clone(),
                &session_id,
                run_id.as_deref(),
                event.clone(),
            )
            .await;
        if matches!(
            &outcome,
            Err(AgentLiveCoordinatorError::Journal(
                LiveEventJournalError::CheckpointRequired
            ))
        ) {
            self.store_current_projection_checkpoint().await?;
            outcome = self
                .append_durable(
                    ingress.journal_ingress.clone(),
                    expected_head,
                    &session_id,
                    run_id.as_deref(),
                    event.clone(),
                )
                .await;
        }
        if matches!(
            &outcome,
            Err(AgentLiveCoordinatorError::Journal(
                LiveEventJournalError::IdempotencyCapacityExceeded
            ))
        ) {
            self.rollover_current_projection().await?;
            // The exact producer capability was revoked at the FIFO rollover
            // barrier. Never silently remint or replay this rejected event.
            return Err(AgentLiveCoordinatorError::IngressRebindRequired);
        }
        if matches!(
            &outcome,
            Err(AgentLiveCoordinatorError::Journal(
                LiveEventJournalError::StorageUnavailable
            ))
        ) {
            match self
                .disk
                .classify_event(
                    ingress.journal_ingress,
                    self.durable_cursor.clone(),
                    session_id.clone(),
                    run_id.clone(),
                    event.clone(),
                )
                .await
            {
                Ok(EventAdmission::Duplicate {
                    event_cursor,
                    head_cursor,
                }) => {
                    return self.recover_or_return_duplicate(
                        session_id,
                        run_id,
                        event,
                        event_cursor,
                        head_cursor,
                    );
                }
                Ok(EventAdmission::New) => {
                    return Err(AgentLiveCoordinatorError::Journal(
                        LiveEventJournalError::StorageUnavailable,
                    ));
                }
                Err(LiveEventJournalError::StorageUnavailable) => {
                    return Err(self.poison(HeadReloadReason::JournalUnavailable));
                }
                Err(error) => return Err(self.map_mutation_journal_error(error)),
            }
        }
        let cursor = match outcome? {
            AppendOutcome::Inserted(cursor) => cursor,
            AppendOutcome::Duplicate {
                event_cursor,
                head_cursor,
            } => {
                return self.recover_or_return_duplicate(
                    session_id,
                    run_id,
                    event,
                    event_cursor,
                    head_cursor,
                );
            }
        };

        if cursor.journal_id() != self.durable_cursor.journal_id() {
            return Err(self.poison(HeadReloadReason::JournalReplaced));
        }
        let expected = self
            .durable_cursor
            .sequence()
            .checked_add(1)
            .ok_or_else(|| self.poison(HeadReloadReason::OrderingLost))?;
        if cursor.sequence() < expected {
            return Err(self.poison(HeadReloadReason::OrderingLost));
        }
        if cursor.sequence() != expected {
            return Err(self.poison(HeadReloadReason::OrderingLost));
        }

        self.apply_mutation(&session_id, mutation);
        self.durable_cursor = cursor.clone();
        self.fan_out(AgentLiveDelivery {
            cursor: cursor.clone(),
            session_id,
            run_id,
            event,
        });
        Ok(cursor)
    }

    fn validate_ingress_event(
        &mut self,
        ingress: &AgentLiveIngressLease,
        event: &AgentLivePublishEvent,
    ) -> Result<(), AgentLiveCoordinatorError> {
        let route = IngressRoute {
            session_id: ingress.session_id.to_string(),
            run_id: ingress.run_id.as_deref().map(str::to_string),
        };
        if ingress.actor_lineage != self.actor_lineage
            || ingress.namespace != ingress.journal_ingress.event_namespace_commitment()
            || self.ingress_epochs.get(&route) != Some(&ingress.producer_epoch)
            || event.id.namespace != ingress.namespace
            || event.id.actor_lineage != ingress.actor_lineage
            || event.id.producer_epoch != ingress.producer_epoch
            || event.id.session_id.as_ref() != ingress.session_id.as_ref()
            || event.id.run_id.as_deref() != ingress.run_id.as_deref()
            || event.id.wire != event.event.event_id()
        {
            return Err(AgentLiveCoordinatorError::IngressRebindRequired);
        }
        let commitment =
            live_event_payload_commitment(ingress.session_id(), ingress.run_id(), &event.event)?;
        if commitment != event.id.payload_commitment {
            self.poison(HeadReloadReason::OrderingLost);
            return Err(AgentLiveCoordinatorError::Journal(
                LiveEventJournalError::EventIdConflict,
            ));
        }
        Ok(())
    }

    fn recover_or_return_duplicate(
        &mut self,
        session_id: String,
        run_id: Option<String>,
        event: MapleLiveEvent,
        event_cursor: LiveEventCursor,
        head_cursor: LiveEventCursor,
    ) -> Result<LiveEventCursor, AgentLiveCoordinatorError> {
        if event_cursor.journal_id() != self.durable_cursor.journal_id()
            || head_cursor.journal_id() != self.durable_cursor.journal_id()
        {
            return Err(self.poison(HeadReloadReason::OrderingLost));
        }
        if head_cursor == self.durable_cursor {
            if event_cursor.sequence() > head_cursor.sequence() {
                return Err(self.poison(HeadReloadReason::OrderingLost));
            }
            // A durable tombstone may outlive the retained payload and many
            // later events. That delayed retry is already reflected in this
            // actor's absolute projection, so it returns its original cursor
            // without mutation or fanout.
            return Ok(event_cursor);
        }
        let expected = self
            .durable_cursor
            .sequence()
            .checked_add(1)
            .ok_or_else(|| self.poison(HeadReloadReason::OrderingLost))?;
        if head_cursor.sequence() != expected || event_cursor != head_cursor {
            return Err(self.poison(HeadReloadReason::OrderingLost));
        }
        if let MapleLiveEvent::HistoryHeadCommitted {
            through_event_cursor,
            ..
        } = &event
        {
            if through_event_cursor != &self.durable_cursor {
                return Err(self.poison(HeadReloadReason::OrderingLost));
            }
        }
        let mutation = self.prepare_mutation(&session_id, &event)?;
        self.apply_mutation(&session_id, mutation);
        self.durable_cursor = head_cursor.clone();
        self.fan_out(AgentLiveDelivery {
            cursor: head_cursor.clone(),
            session_id,
            run_id,
            event,
        });
        Ok(head_cursor)
    }

    async fn classify_durable(
        &mut self,
        ingress: LiveEventJournalIngressLease,
        session_id: &str,
        run_id: Option<&str>,
        event: MapleLiveEvent,
    ) -> Result<EventAdmission, AgentLiveCoordinatorError> {
        match self
            .disk
            .classify_event(
                ingress,
                self.durable_cursor.clone(),
                session_id.to_string(),
                run_id.map(str::to_string),
                event,
            )
            .await
        {
            Ok(admission) => Ok(admission),
            Err(error) => Err(self.map_mutation_journal_error(error)),
        }
    }

    async fn append_durable(
        &mut self,
        ingress: LiveEventJournalIngressLease,
        expected_head: LiveEventCursor,
        session_id: &str,
        run_id: Option<&str>,
        event: MapleLiveEvent,
    ) -> Result<AppendOutcome, AgentLiveCoordinatorError> {
        match self
            .disk
            .append_outcome(
                ingress,
                expected_head,
                session_id.to_string(),
                run_id.map(str::to_string),
                event,
            )
            .await
        {
            Ok(outcome) => Ok(outcome),
            Err(error) => Err(self.map_mutation_journal_error(error)),
        }
    }

    async fn store_current_projection_checkpoint(
        &mut self,
    ) -> Result<(), AgentLiveCoordinatorError> {
        let bytes = self.encode_projection_checkpoint()?;
        let stored = match self
            .disk
            .store_projection_checkpoint(self.durable_cursor.clone(), bytes)
            .await
        {
            Ok(cursor) => cursor,
            Err(error) => return Err(self.map_mutation_journal_error(error)),
        };
        if stored != self.durable_cursor {
            return Err(self.poison(HeadReloadReason::OrderingLost));
        }
        Ok(())
    }

    async fn rollover_current_projection(&mut self) -> Result<(), AgentLiveCoordinatorError> {
        if self.pending_rollover.is_some() {
            return self.finish_pending_rollover().await;
        }
        let bytes = self.encode_projection_checkpoint()?;
        let previous = self.durable_cursor.clone();

        match self
            .disk
            .store_projection_checkpoint(previous.clone(), bytes.clone())
            .await
        {
            Ok(stored) if stored == previous => {}
            Ok(_) => return Err(self.poison(HeadReloadReason::OrderingLost)),
            Err(LiveEventJournalError::StorageUnavailable) => {
                let current = self
                    .disk
                    .checkpoint()
                    .await
                    .map_err(|error| self.map_mutation_journal_error(error))?;
                let checkpoint = self
                    .disk
                    .load_projection_checkpoint()
                    .await
                    .map_err(|error| self.map_mutation_journal_error(error))?;
                if current != previous
                    || !projection_checkpoint_matches(checkpoint.as_ref(), &previous, &bytes)
                {
                    return Err(self.poison(HeadReloadReason::OrderingLost));
                }
            }
            Err(error) => return Err(self.map_mutation_journal_error(error)),
        }

        // This FIFO point is the generation barrier. Both active and paused
        // subscribers are bound to the old journal ID, so close them before
        // preparing the move-only capability; none may observe a sequence
        // from both generations.
        self.ingress_epochs.clear();
        self.invalidate_subscribers(HeadReloadReason::JournalReplaced);
        let obligation = match self
            .disk
            .prepare_rollover(previous.clone(), bytes.clone())
            .await
        {
            Ok(obligation) => obligation,
            Err(error) => return Err(self.map_mutation_journal_error(error)),
        };
        self.pending_rollover = Some(PendingRollover {
            obligation: Arc::new(obligation),
            previous_cursor: previous,
            checkpoint_bytes: bytes,
        });
        self.finish_pending_rollover().await
    }

    async fn finish_pending_rollover(&mut self) -> Result<(), AgentLiveCoordinatorError> {
        let Some(pending) = self.pending_rollover.as_ref() else {
            return Ok(());
        };
        let previous = pending.previous_cursor.clone();
        let obligation = Arc::clone(&pending.obligation);
        let checkpoint_bytes = pending.checkpoint_bytes.clone();
        let activation = match self
            .disk
            .commit_rollover(obligation, checkpoint_bytes)
            .await
        {
            Ok(activation) => activation,
            Err(LiveEventJournalError::StorageUnavailable) => {
                // The atomic replace may already be durable. Retain the exact
                // non-clone obligation and checkpoint bytes; the next FIFO
                // command retries this commit before doing anything else.
                return Err(AgentLiveCoordinatorError::Journal(
                    LiveEventJournalError::StorageUnavailable,
                ));
            }
            Err(error) => return Err(self.map_mutation_journal_error(error)),
        };
        let (_fresh_lease, replacement) = activation.into_parts();
        if replacement.journal_id() == previous.journal_id() || replacement.sequence() != 0 {
            return Err(self.poison(HeadReloadReason::OrderingLost));
        }
        self.durable_cursor = replacement;
        self.pending_rollover = None;
        Ok(())
    }

    fn map_mutation_journal_error(
        &mut self,
        error: LiveEventJournalError,
    ) -> AgentLiveCoordinatorError {
        match error {
            LiveEventJournalError::OwnerGenerationMismatch
            | LiveEventJournalError::OwnerTransitionIncomplete => {
                self.poison(HeadReloadReason::OwnerChanged)
            }
            LiveEventJournalError::JournalReplaced | LiveEventJournalError::JournalRetired => {
                self.poison(HeadReloadReason::JournalReplaced)
            }
            LiveEventJournalError::ReseedRequired => self.poison(HeadReloadReason::ReseedRequired),
            LiveEventJournalError::HeadChanged => self.poison(HeadReloadReason::OrderingLost),
            LiveEventJournalError::StorageCorrupt => {
                self.poison(HeadReloadReason::JournalUnavailable)
            }
            other => AgentLiveCoordinatorError::Journal(other),
        }
    }

    async fn begin_head_attach(
        &mut self,
        capacity: usize,
        sender: broadcast::Sender<AgentLiveDelivery>,
        terminal_reason: Arc<TerminalMutex<Option<HeadReloadReason>>>,
        cancellation_commands: mpsc::Sender<CoordinatorCommand>,
    ) -> Result<BeginHeadAttachResult, AgentLiveCoordinatorError> {
        self.verify_checkpoint().await?;
        self.ensure_subscriber_capacity(capacity)?;
        let subscriber_id = self.allocate_subscriber_id()?;
        let sessions = self.absolute_live_sessions();
        self.subscribers.insert(
            subscriber_id,
            SubscriberState {
                sender,
                terminal_reason,
                reserved_buffer_bytes: reserved_subscriber_bytes(capacity)?,
                mode: SubscriberMode::Paused {
                    from: self.durable_cursor.clone(),
                    capacity,
                    observed_events: 0,
                    overflowed: false,
                },
            },
        );
        Ok(BeginHeadAttachResult {
            subscriber: PendingSubscriberOwnership::new(subscriber_id, cancellation_commands),
            through_cursor: self.durable_cursor.clone(),
            live_sessions: sessions,
        })
    }

    async fn finalize_head_attach(
        &mut self,
        subscriber_id: u64,
    ) -> Result<LiveEventCursor, AgentLiveCoordinatorError> {
        self.ensure_ready().await?;
        let (from, capacity, observed_events, overflowed) = {
            let subscriber = self
                .subscribers
                .get(&subscriber_id)
                .ok_or(AgentLiveCoordinatorError::CoordinatorClosed)?;
            let SubscriberMode::Paused {
                from,
                capacity,
                observed_events,
                overflowed,
            } = &subscriber.mode
            else {
                return Err(AgentLiveCoordinatorError::CoordinatorClosed);
            };
            (from.clone(), *capacity, *observed_events, *overflowed)
        };
        if overflowed {
            if let Some(subscriber) = self.subscribers.remove(&subscriber_id) {
                set_terminal_reason(
                    &subscriber.terminal_reason,
                    HeadReloadReason::PausedSubscriberOverflow,
                );
            }
            return Err(AgentLiveCoordinatorError::HeadReloadRequired(
                HeadReloadReason::PausedSubscriberOverflow,
            ));
        }

        let through = match self.verified_checkpoint().await {
            Ok(cursor) => cursor,
            Err(error) => {
                if let Some(subscriber) = self.subscribers.remove(&subscriber_id) {
                    if let AgentLiveCoordinatorError::HeadReloadRequired(reason) = &error {
                        set_terminal_reason(&subscriber.terminal_reason, *reason);
                    }
                }
                return Err(error);
            }
        };
        let deliveries = match self.replay_until(from, &through).await {
            Ok(deliveries) => deliveries,
            Err(error) => {
                if let Some(subscriber) = self.subscribers.remove(&subscriber_id) {
                    if let AgentLiveCoordinatorError::HeadReloadRequired(reason) = &error {
                        set_terminal_reason(&subscriber.terminal_reason, *reason);
                    }
                }
                return Err(error);
            }
        };
        if deliveries.len() != observed_events || deliveries.len() > capacity {
            if let Some(subscriber) = self.subscribers.remove(&subscriber_id) {
                set_terminal_reason(&subscriber.terminal_reason, HeadReloadReason::OrderingLost);
            }
            return Err(AgentLiveCoordinatorError::HeadReloadRequired(
                HeadReloadReason::OrderingLost,
            ));
        }

        let subscriber = self
            .subscribers
            .get_mut(&subscriber_id)
            .ok_or(AgentLiveCoordinatorError::CoordinatorClosed)?;
        for delivery in deliveries {
            if subscriber.sender.send(delivery).is_err() {
                if let Some(subscriber) = self.subscribers.remove(&subscriber_id) {
                    set_terminal_reason(
                        &subscriber.terminal_reason,
                        HeadReloadReason::OrderingLost,
                    );
                }
                return Err(AgentLiveCoordinatorError::CoordinatorClosed);
            }
        }
        subscriber.mode = SubscriberMode::Active;
        Ok(through)
    }

    async fn begin_resume(
        &mut self,
        cursor: LiveEventCursor,
        capacity: usize,
        sender: broadcast::Sender<AgentLiveDelivery>,
        terminal_reason: Arc<TerminalMutex<Option<HeadReloadReason>>>,
        cancellation_commands: mpsc::Sender<CoordinatorCommand>,
    ) -> Result<BeginResumeResult, AgentLiveCoordinatorError> {
        let through = self.verified_checkpoint().await?;
        self.ensure_subscriber_capacity(capacity)?;
        let deliveries = self.replay_until(cursor, &through).await?;
        if deliveries.len() > capacity {
            return Err(AgentLiveCoordinatorError::HeadReloadRequired(
                HeadReloadReason::PausedSubscriberOverflow,
            ));
        }
        for delivery in deliveries {
            sender.send(delivery).map_err(|_| {
                AgentLiveCoordinatorError::HeadReloadRequired(HeadReloadReason::OrderingLost)
            })?;
        }
        let subscriber_id = self.allocate_subscriber_id()?;
        self.subscribers.insert(
            subscriber_id,
            SubscriberState {
                sender,
                terminal_reason,
                reserved_buffer_bytes: reserved_subscriber_bytes(capacity)?,
                mode: SubscriberMode::Active,
            },
        );
        Ok(BeginResumeResult {
            subscriber: PendingSubscriberOwnership::new(subscriber_id, cancellation_commands),
            through_cursor: through,
        })
    }

    async fn replay_until(
        &mut self,
        mut cursor: LiveEventCursor,
        through: &LiveEventCursor,
    ) -> Result<Vec<AgentLiveDelivery>, AgentLiveCoordinatorError> {
        if cursor.journal_id() != through.journal_id() {
            return Err(AgentLiveCoordinatorError::HeadReloadRequired(
                HeadReloadReason::JournalReplaced,
            ));
        }
        if cursor.sequence() > through.sequence() {
            return Err(AgentLiveCoordinatorError::HeadReloadRequired(
                HeadReloadReason::CursorAhead,
            ));
        }

        let mut deliveries = Vec::new();
        while cursor.sequence() < through.sequence() {
            let replay = match self.disk.replay_after(cursor.clone()).await {
                Ok(replay) => replay,
                Err(error) => return Err(self.map_mutation_journal_error(error)),
            };
            match replay {
                LiveReplayRead::SnapshotRequired(required) => {
                    let reason = map_snapshot_reason(required.reason);
                    if matches!(
                        reason,
                        HeadReloadReason::JournalReplaced | HeadReloadReason::ReseedRequired
                    ) {
                        return Err(self.poison(reason));
                    }
                    return Err(AgentLiveCoordinatorError::HeadReloadRequired(reason));
                }
                LiveReplayRead::Events {
                    entries,
                    next_cursor,
                    has_more,
                } => {
                    let previous_sequence = cursor.sequence();
                    for entry in entries {
                        if entry.cursor().sequence() > through.sequence() {
                            break;
                        }
                        deliveries.push(delivery_from_entry(entry));
                    }
                    if next_cursor.journal_id() != through.journal_id()
                        || next_cursor.sequence() <= previous_sequence
                        || next_cursor.sequence() > through.sequence()
                    {
                        return Err(AgentLiveCoordinatorError::HeadReloadRequired(
                            HeadReloadReason::OrderingLost,
                        ));
                    }
                    cursor = next_cursor;
                    if !has_more && cursor.sequence() < through.sequence() {
                        return Err(AgentLiveCoordinatorError::HeadReloadRequired(
                            HeadReloadReason::OrderingLost,
                        ));
                    }
                }
            }
        }
        Ok(deliveries)
    }

    async fn verify_checkpoint(&mut self) -> Result<(), AgentLiveCoordinatorError> {
        self.verified_checkpoint().await.map(|_| ())
    }

    async fn verified_checkpoint(&mut self) -> Result<LiveEventCursor, AgentLiveCoordinatorError> {
        self.ensure_ready().await?;
        let current = match self.disk.checkpoint().await {
            Ok(cursor) => cursor,
            Err(
                LiveEventJournalError::OwnerGenerationMismatch
                | LiveEventJournalError::OwnerTransitionIncomplete,
            ) => return Err(self.poison(HeadReloadReason::OwnerChanged)),
            Err(LiveEventJournalError::JournalReplaced | LiveEventJournalError::JournalRetired) => {
                return Err(self.poison(HeadReloadReason::JournalReplaced));
            }
            Err(LiveEventJournalError::ReseedRequired) => {
                return Err(self.poison(HeadReloadReason::ReseedRequired));
            }
            Err(LiveEventJournalError::StorageCorrupt) => {
                return Err(self.poison(HeadReloadReason::JournalUnavailable));
            }
            Err(error) => return Err(map_journal_for_attach(error)),
        };
        if current.journal_id() != self.durable_cursor.journal_id() {
            return Err(self.poison(HeadReloadReason::JournalReplaced));
        }
        if current.sequence() != self.durable_cursor.sequence() {
            return Err(self.poison(HeadReloadReason::OrderingLost));
        }
        Ok(current)
    }

    fn prepare_mutation(
        &self,
        session_id: &str,
        event: &MapleLiveEvent,
    ) -> Result<ProjectionMutation, AgentLiveCoordinatorError> {
        let current = self.sessions.get(session_id).cloned().unwrap_or_default();
        let mutation = current
            .prepare(event)
            .map_err(AgentLiveCoordinatorError::Projection)?;
        self.validate_projection_bounds(session_id, &mutation)?;
        Ok(mutation)
    }

    fn apply_mutation(&mut self, session_id: &str, mutation: ProjectionMutation) {
        match mutation {
            ProjectionMutation::Noop => {}
            ProjectionMutation::Remove => {
                self.sessions.remove(session_id);
            }
            ProjectionMutation::Set(projection) => {
                self.sessions.insert(session_id.to_string(), projection);
            }
        }
    }

    fn absolute_live_sessions(&self) -> Vec<AgentLiveSessionProjection> {
        let mut live_sessions = self
            .sessions
            .iter()
            .map(|(session_id, projection)| AgentLiveSessionProjection {
                session_id: session_id.clone(),
                live_items: projection.absolute_items(),
            })
            .collect::<Vec<_>>();
        live_sessions.sort_by(|left, right| left.session_id.cmp(&right.session_id));
        live_sessions
    }

    fn encode_projection_checkpoint(&self) -> Result<Vec<u8>, AgentLiveCoordinatorError> {
        let bytes = serde_json::to_vec(&CoordinatorProjectionCheckpoint {
            format_version: LIVE_PROJECTION_CHECKPOINT_VERSION,
            live_sessions: self.absolute_live_sessions(),
        })
        .map_err(|_| {
            AgentLiveCoordinatorError::Journal(LiveEventJournalError::InvalidCheckpoint)
        })?;
        if bytes.len() > MAX_LIVE_PROJECTION_BYTES_PER_ACCOUNT {
            return Err(AgentLiveCoordinatorError::Projection(
                AgentLiveProjectionError::AccountProjectionCapacityExceeded,
            ));
        }
        Ok(bytes)
    }

    fn fan_out(&mut self, delivery: AgentLiveDelivery) {
        self.subscribers.retain(|_, subscriber| {
            if subscriber.sender.receiver_count() == 0 {
                return false;
            }
            match &mut subscriber.mode {
                SubscriberMode::Paused {
                    capacity,
                    observed_events,
                    overflowed,
                    ..
                } => {
                    *observed_events = observed_events.saturating_add(1);
                    if *observed_events > *capacity {
                        *overflowed = true;
                    }
                    true
                }
                SubscriberMode::Active => subscriber.sender.send(delivery.clone()).is_ok(),
            }
        });
    }

    fn validate_projection_bounds(
        &self,
        session_id: &str,
        mutation: &ProjectionMutation,
    ) -> Result<(), AgentLiveCoordinatorError> {
        let replacement = match mutation {
            ProjectionMutation::Set(projection) => Some(projection),
            ProjectionMutation::Noop => return Ok(()),
            ProjectionMutation::Remove => None,
        };
        let replacing_existing = self.sessions.contains_key(session_id);
        let projected_session_count = self
            .sessions
            .len()
            .saturating_sub(usize::from(replacing_existing))
            .saturating_add(usize::from(replacement.is_some()));
        if projected_session_count > MAX_LIVE_SESSIONS_PER_ACCOUNT {
            return Err(AgentLiveCoordinatorError::Projection(
                AgentLiveProjectionError::AccountProjectionCapacityExceeded,
            ));
        }

        let mut item_count = 0usize;
        let mut projected_bytes = LIVE_CHECKPOINT_OUTER_OVERHEAD_BYTES;
        for (existing_session_id, projection) in &self.sessions {
            if existing_session_id == session_id {
                continue;
            }
            accumulate_projection_bounds(
                existing_session_id,
                projection,
                &mut item_count,
                &mut projected_bytes,
            )?;
        }
        if let Some(projection) = replacement {
            accumulate_projection_bounds(
                session_id,
                projection,
                &mut item_count,
                &mut projected_bytes,
            )?;
        }
        if item_count > MAX_LIVE_ITEMS_PER_ACCOUNT
            || projected_bytes > MAX_LIVE_PROJECTION_BYTES_PER_ACCOUNT
        {
            return Err(AgentLiveCoordinatorError::Projection(
                AgentLiveProjectionError::AccountProjectionCapacityExceeded,
            ));
        }
        Ok(())
    }

    fn ensure_subscriber_capacity(
        &mut self,
        requested_capacity: usize,
    ) -> Result<(), AgentLiveCoordinatorError> {
        self.subscribers
            .retain(|_, subscriber| subscriber.sender.receiver_count() != 0);
        if self.subscribers.len() >= MAX_SUBSCRIBERS_PER_ACCOUNT {
            return Err(AgentLiveCoordinatorError::SubscriberCapacityExceeded);
        }
        let new_buffer_bytes = reserved_subscriber_bytes(requested_capacity)?;
        let existing_buffer_bytes = self
            .subscribers
            .values()
            .try_fold(0usize, |total, subscriber| {
                total.checked_add(subscriber.reserved_buffer_bytes)
            })
            .ok_or(AgentLiveCoordinatorError::SubscriberCapacityExceeded)?;
        if existing_buffer_bytes
            .checked_add(new_buffer_bytes)
            .is_none_or(|total| total > MAX_ACCOUNT_SUBSCRIPTION_BUFFER_BYTES)
        {
            return Err(AgentLiveCoordinatorError::SubscriberCapacityExceeded);
        }
        Ok(())
    }

    fn allocate_subscriber_id(&mut self) -> Result<u64, AgentLiveCoordinatorError> {
        let id = self.next_subscriber_id;
        self.next_subscriber_id = self
            .next_subscriber_id
            .checked_add(1)
            .ok_or_else(|| self.poison(HeadReloadReason::OrderingLost))?;
        Ok(id)
    }

    fn ensure_healthy(&self) -> Result<(), AgentLiveCoordinatorError> {
        if let Some(reason) = self.sealed {
            return Err(AgentLiveCoordinatorError::Sealed(reason));
        }
        if let Some(reason) = self.poison {
            return Err(AgentLiveCoordinatorError::HeadReloadRequired(reason));
        }
        Ok(())
    }

    async fn ensure_ready(&mut self) -> Result<(), AgentLiveCoordinatorError> {
        self.ensure_healthy()?;
        self.finish_pending_rollover().await?;
        self.ensure_healthy()
    }

    async fn seal(
        &mut self,
        reason: AgentLiveSealReason,
    ) -> Result<AgentLiveSeal, AgentLiveCoordinatorError> {
        if let Some(existing) = self.sealed {
            if existing != reason {
                return Err(AgentLiveCoordinatorError::Sealed(existing));
            }
            if let Some(sealed) = self.seal_result.clone() {
                return Ok(sealed);
            }
            // A prior transient verification failure left this actor terminal
            // but without retirement authority. Rechecking the same immutable
            // actor head is safe and lets the host resume its fail-closed
            // Sealing state; publication is never reopened.
        } else {
            // The FIFO lifecycle fence is terminal even if disk verification
            // fails. Never allow a failed attempt to reopen publication while
            // the host decides whether it can retire or must reseed this
            // journal.
            self.sealed = Some(reason);
            self.invalidate_subscribers(HeadReloadReason::OwnerChanged);
        }
        if let Some(poison) = self.poison {
            return Err(AgentLiveCoordinatorError::HeadReloadRequired(poison));
        }
        // Sealing is allowed to resume, but never bypass, an ambiguous
        // rollover. The same-reason seal retry will continue holding the
        // terminal actor fence while the exact obligation is retried.
        self.finish_pending_rollover().await?;
        let through_cursor = match self.disk.checkpoint().await {
            Ok(cursor) => cursor,
            Err(error) => return Err(self.map_mutation_journal_error(error)),
        };
        if through_cursor.journal_id() != self.durable_cursor.journal_id() {
            return Err(self.poison(HeadReloadReason::JournalReplaced));
        }
        if through_cursor.sequence() != self.durable_cursor.sequence() {
            return Err(self.poison(HeadReloadReason::OrderingLost));
        }
        let sealed = AgentLiveSeal {
            journal_lease: self
                .disk
                .lease()
                .map_err(|error| self.map_mutation_journal_error(error))?,
            through_cursor,
            reason,
        };
        self.seal_result = Some(sealed.clone());
        Ok(sealed)
    }

    fn poison(&mut self, reason: HeadReloadReason) -> AgentLiveCoordinatorError {
        self.poison = Some(reason);
        self.invalidate_subscribers(reason);
        AgentLiveCoordinatorError::HeadReloadRequired(reason)
    }

    fn invalidate_subscribers(&mut self, reason: HeadReloadReason) {
        for subscriber in self.subscribers.values() {
            if let Ok(mut terminal) = subscriber.terminal_reason.lock() {
                *terminal = Some(reason);
            }
        }
        self.subscribers.clear();
    }
}

#[derive(Debug, Clone, Default)]
struct SessionLiveProjection {
    items: Vec<MapleLiveTimelineItem>,
    checkpoint_wire_bytes: usize,
}

impl SessionLiveProjection {
    fn prepare(
        mut self,
        event: &MapleLiveEvent,
    ) -> Result<ProjectionMutation, AgentLiveProjectionError> {
        match event {
            MapleLiveEvent::TimelineUpsert { item, .. } => {
                self.upsert(item.clone())?;
                Ok(ProjectionMutation::Set(self))
            }
            MapleLiveEvent::UserFacingError { error, .. } => {
                self.upsert(error.to_timeline_item())?;
                Ok(ProjectionMutation::Set(self))
            }
            MapleLiveEvent::TimelineCleared { .. } => Ok(ProjectionMutation::Remove),
            MapleLiveEvent::HistoryHeadCommitted { .. } => Ok(ProjectionMutation::Remove),
            MapleLiveEvent::SessionDeleted { .. } => Ok(ProjectionMutation::Remove),
            MapleLiveEvent::RunStarted { .. }
            | MapleLiveEvent::HistoryReplaced { .. }
            | MapleLiveEvent::SessionUpdated { .. }
            | MapleLiveEvent::RunFinished { .. } => Ok(ProjectionMutation::Noop),
        }
    }

    fn upsert(&mut self, incoming: MapleLiveTimelineItem) -> Result<(), AgentLiveProjectionError> {
        incoming.validate()?;
        if let Some(existing) = self.items.iter_mut().find(|item| item.id == incoming.id) {
            let previous_bytes = live_item_checkpoint_wire_bytes(existing)?;
            let merged = merge_live_item(existing, incoming)?;
            let merged_bytes = live_item_checkpoint_wire_bytes(&merged)?;
            self.checkpoint_wire_bytes = self
                .checkpoint_wire_bytes
                .checked_sub(previous_bytes)
                .and_then(|bytes| bytes.checked_add(merged_bytes))
                .ok_or(AgentLiveProjectionError::AccountProjectionCapacityExceeded)?;
            *existing = merged;
            return Ok(());
        }
        if self.items.len() >= MAX_LIVE_ITEMS_PER_SESSION {
            return Err(AgentLiveProjectionError::TooManyTimelineItems);
        }
        let incoming = incoming.into_absolute();
        self.checkpoint_wire_bytes = self
            .checkpoint_wire_bytes
            .checked_add(live_item_checkpoint_wire_bytes(&incoming)?)
            .ok_or(AgentLiveProjectionError::AccountProjectionCapacityExceeded)?;
        self.items.push(incoming);
        Ok(())
    }

    fn absolute_items(&self) -> Vec<MapleLiveTimelineItem> {
        self.items
            .iter()
            .cloned()
            .map(MapleLiveTimelineItem::into_absolute)
            .collect()
    }
}

enum ProjectionMutation {
    Noop,
    Remove,
    Set(SessionLiveProjection),
}

fn accumulate_projection_bounds(
    session_id: &str,
    projection: &SessionLiveProjection,
    item_count: &mut usize,
    projected_bytes: &mut usize,
) -> Result<(), AgentLiveCoordinatorError> {
    *item_count = item_count.checked_add(projection.items.len()).ok_or(
        AgentLiveCoordinatorError::Projection(
            AgentLiveProjectionError::AccountProjectionCapacityExceeded,
        ),
    )?;
    let session_bytes = LIVE_CHECKPOINT_SESSION_OVERHEAD_BYTES
        .checked_add(
            json_string_checkpoint_wire_bytes(session_id)
                .map_err(AgentLiveCoordinatorError::Projection)?,
        )
        .and_then(|bytes| bytes.checked_add(projection.checkpoint_wire_bytes))
        .ok_or(AgentLiveCoordinatorError::Projection(
            AgentLiveProjectionError::AccountProjectionCapacityExceeded,
        ))?;
    *projected_bytes =
        projected_bytes
            .checked_add(session_bytes)
            .ok_or(AgentLiveCoordinatorError::Projection(
                AgentLiveProjectionError::AccountProjectionCapacityExceeded,
            ))?;
    Ok(())
}

fn live_item_checkpoint_wire_bytes(
    item: &MapleLiveTimelineItem,
) -> Result<usize, AgentLiveProjectionError> {
    [
        Some(item.id.as_str()),
        item.title.as_deref(),
        item.text.as_deref(),
        item.status.as_deref(),
    ]
    .into_iter()
    .flatten()
    .try_fold(LIVE_CHECKPOINT_ITEM_OVERHEAD_BYTES, |bytes, value| {
        bytes
            .checked_add(json_string_checkpoint_wire_bytes(value)?)
            .ok_or(AgentLiveProjectionError::AccountProjectionCapacityExceeded)
    })
}

/// Upper-bound a JSON string without allocating. ASCII controls may use the
/// six-byte `\u00XX` form; quotes and backslashes use two bytes; every other
/// scalar is emitted as its UTF-8 bytes. The two quote bytes are included.
fn json_string_checkpoint_wire_bytes(value: &str) -> Result<usize, AgentLiveProjectionError> {
    value
        .chars()
        .try_fold(2usize, |bytes, character| {
            let encoded = if character.is_ascii_control() {
                6
            } else if matches!(character, '"' | '\\') {
                2
            } else {
                character.len_utf8()
            };
            bytes.checked_add(encoded)
        })
        .ok_or(AgentLiveProjectionError::AccountProjectionCapacityExceeded)
}

fn merge_live_item(
    existing: &MapleLiveTimelineItem,
    incoming: MapleLiveTimelineItem,
) -> Result<MapleLiveTimelineItem, AgentLiveProjectionError> {
    if incoming.merge == MapleLiveMerge::Replace {
        return Ok(incoming.into_absolute());
    }
    if existing.item_type != incoming.item_type || existing.role != incoming.role {
        return Err(AgentLiveProjectionError::ConflictingItemIdentity);
    }
    let mut merged = existing.clone();
    if let Some(text) = incoming.text {
        let target = merged.text.get_or_insert_with(String::new);
        if target
            .len()
            .checked_add(text.len())
            .is_none_or(|length| length > MAX_TEXT_BYTES)
        {
            return Err(AgentLiveProjectionError::MergedItemTooLarge);
        }
        target.push_str(&text);
    }
    if incoming.title.is_some() {
        merged.title = incoming.title;
    }
    if incoming.status.is_some() {
        merged.status = incoming.status;
    }
    merged.created_ms = incoming.created_ms;
    merged.merge = MapleLiveMerge::Replace;
    merged.validate()?;
    Ok(merged)
}

struct SubscriberState {
    sender: broadcast::Sender<AgentLiveDelivery>,
    terminal_reason: Arc<TerminalMutex<Option<HeadReloadReason>>>,
    reserved_buffer_bytes: usize,
    mode: SubscriberMode,
}

enum SubscriberMode {
    Paused {
        from: LiveEventCursor,
        capacity: usize,
        observed_events: usize,
        overflowed: bool,
    },
    Active,
}

trait CoordinatorJournal: Send + Sync + 'static {
    fn max_replay_entries(&self) -> usize;

    fn checkpoint(
        &self,
        lease: &LiveEventJournalLease,
    ) -> Result<LiveEventCursor, LiveEventJournalError>;
    fn load_projection_checkpoint(
        &self,
        lease: &LiveEventJournalLease,
    ) -> Result<Option<LiveProjectionCheckpoint>, LiveEventJournalError>;
    fn store_projection_checkpoint(
        &self,
        lease: &LiveEventJournalLease,
        expected_head: &LiveEventCursor,
        bytes: &[u8],
    ) -> Result<LiveEventCursor, LiveEventJournalError>;
    fn bind_ingress(
        &self,
        lease: &LiveEventJournalLease,
    ) -> Result<LiveEventJournalIngressLease, LiveEventJournalError>;
    fn prepare_rollover(
        &self,
        lease: &LiveEventJournalLease,
        expected_head: &LiveEventCursor,
        bytes: &[u8],
    ) -> Result<LiveEventJournalRolloverObligation, LiveEventJournalError>;
    fn commit_rollover(
        &self,
        obligation: &LiveEventJournalRolloverObligation,
        bytes: &[u8],
    ) -> Result<LiveEventJournalActivation, LiveEventJournalError>;
    fn classify_event(
        &self,
        ingress: &LiveEventJournalIngressLease,
        expected_head: &LiveEventCursor,
        session_id: &str,
        run_id: Option<&str>,
        event: &MapleLiveEvent,
    ) -> Result<EventAdmission, LiveEventJournalError>;
    fn append_outcome(
        &self,
        ingress: &LiveEventJournalIngressLease,
        expected_head: &LiveEventCursor,
        session_id: &str,
        run_id: Option<&str>,
        event: MapleLiveEvent,
    ) -> Result<AppendOutcome, LiveEventJournalError>;
    fn replay_after(
        &self,
        lease: &LiveEventJournalLease,
        cursor: &LiveEventCursor,
        limit: usize,
    ) -> Result<LiveReplayRead<MapleLiveEvent>, LiveEventJournalError>;
}

impl CoordinatorJournal for LiveEventJournal<MapleLiveEvent> {
    fn max_replay_entries(&self) -> usize {
        self.max_replay_entries()
    }

    fn checkpoint(
        &self,
        lease: &LiveEventJournalLease,
    ) -> Result<LiveEventCursor, LiveEventJournalError> {
        self.checkpoint(lease)
    }

    fn load_projection_checkpoint(
        &self,
        lease: &LiveEventJournalLease,
    ) -> Result<Option<LiveProjectionCheckpoint>, LiveEventJournalError> {
        self.load_checkpoint(lease)
    }

    fn store_projection_checkpoint(
        &self,
        lease: &LiveEventJournalLease,
        expected_head: &LiveEventCursor,
        bytes: &[u8],
    ) -> Result<LiveEventCursor, LiveEventJournalError> {
        self.store_checkpoint(lease, expected_head, bytes)
    }

    fn bind_ingress(
        &self,
        lease: &LiveEventJournalLease,
    ) -> Result<LiveEventJournalIngressLease, LiveEventJournalError> {
        self.bind_ingress(lease)
    }

    fn prepare_rollover(
        &self,
        lease: &LiveEventJournalLease,
        expected_head: &LiveEventCursor,
        bytes: &[u8],
    ) -> Result<LiveEventJournalRolloverObligation, LiveEventJournalError> {
        self.prepare_rollover(lease, expected_head, bytes)
    }

    fn commit_rollover(
        &self,
        obligation: &LiveEventJournalRolloverObligation,
        bytes: &[u8],
    ) -> Result<LiveEventJournalActivation, LiveEventJournalError> {
        self.commit_rollover(obligation, bytes)
    }

    fn classify_event(
        &self,
        ingress: &LiveEventJournalIngressLease,
        expected_head: &LiveEventCursor,
        session_id: &str,
        run_id: Option<&str>,
        event: &MapleLiveEvent,
    ) -> Result<EventAdmission, LiveEventJournalError> {
        self.classify_event(ingress, expected_head, session_id, run_id, event)
    }

    fn append_outcome(
        &self,
        ingress: &LiveEventJournalIngressLease,
        expected_head: &LiveEventCursor,
        session_id: &str,
        run_id: Option<&str>,
        event: MapleLiveEvent,
    ) -> Result<AppendOutcome, LiveEventJournalError> {
        self.append_outcome(ingress, expected_head, session_id, run_id, event)
    }

    fn replay_after(
        &self,
        lease: &LiveEventJournalLease,
        cursor: &LiveEventCursor,
        limit: usize,
    ) -> Result<LiveReplayRead<MapleLiveEvent>, LiveEventJournalError> {
        self.replay_after(lease, cursor, limit)
    }
}

struct BlockingJournalWorker {
    commands: std_mpsc::SyncSender<DiskCommand>,
    replay_page_size: usize,
    lease: Arc<TerminalMutex<LiveEventJournalLease>>,
}

impl BlockingJournalWorker {
    fn spawn(
        journal: Arc<dyn CoordinatorJournal>,
        lease: LiveEventJournalLease,
    ) -> Result<Self, AgentLiveCoordinatorError> {
        let replay_page_size = journal.max_replay_entries().min(REPLAY_PAGE_SIZE);
        if replay_page_size == 0 {
            return Err(AgentLiveCoordinatorError::Journal(
                LiveEventJournalError::InvalidReplayLimit,
            ));
        }
        let shared_lease = Arc::new(TerminalMutex::new(lease));
        let worker_lease = Arc::clone(&shared_lease);
        let (commands, receiver) = std_mpsc::sync_channel(1);
        thread::Builder::new()
            .name("maple-agent-live-journal".to_string())
            .spawn(move || {
                while let Ok(command) = receiver.recv() {
                    match command {
                        DiskCommand::Checkpoint { reply } => {
                            let result = worker_lease
                                .lock()
                                .map_err(|_| LiveEventJournalError::StorageUnavailable)
                                .and_then(|lease| journal.checkpoint(&lease));
                            let _ = reply.send(result);
                        }
                        DiskCommand::LoadProjectionCheckpoint { reply } => {
                            let result = worker_lease
                                .lock()
                                .map_err(|_| LiveEventJournalError::StorageUnavailable)
                                .and_then(|lease| journal.load_projection_checkpoint(&lease));
                            let _ = reply.send(result);
                        }
                        DiskCommand::StoreProjectionCheckpoint {
                            expected_head,
                            bytes,
                            reply,
                        } => {
                            let result = worker_lease
                                .lock()
                                .map_err(|_| LiveEventJournalError::StorageUnavailable)
                                .and_then(|lease| {
                                    journal.store_projection_checkpoint(
                                        &lease,
                                        &expected_head,
                                        &bytes,
                                    )
                                });
                            let _ = reply.send(result);
                        }
                        DiskCommand::BindIngress { reply } => {
                            let result = worker_lease
                                .lock()
                                .map_err(|_| LiveEventJournalError::StorageUnavailable)
                                .and_then(|lease| journal.bind_ingress(&lease));
                            let _ = reply.send(result);
                        }
                        DiskCommand::PrepareRollover {
                            expected_head,
                            bytes,
                            reply,
                        } => {
                            let result = worker_lease
                                .lock()
                                .map_err(|_| LiveEventJournalError::StorageUnavailable)
                                .and_then(|lease| {
                                    journal.prepare_rollover(&lease, &expected_head, &bytes)
                                });
                            let _ = reply.send(result);
                        }
                        DiskCommand::CommitRollover {
                            obligation,
                            bytes,
                            reply,
                        } => {
                            let result = journal.commit_rollover(obligation.as_ref(), &bytes);
                            if let Ok(activation) = &result {
                                if let Ok(mut lease) = worker_lease.lock() {
                                    *lease = activation.lease.clone();
                                } else {
                                    let _ =
                                        reply.send(Err(LiveEventJournalError::StorageUnavailable));
                                    continue;
                                }
                            }
                            let _ = reply.send(result);
                        }
                        DiskCommand::Classify {
                            ingress,
                            expected_head,
                            session_id,
                            run_id,
                            event,
                            reply,
                        } => {
                            let _ = reply.send(journal.classify_event(
                                &ingress,
                                &expected_head,
                                &session_id,
                                run_id.as_deref(),
                                &event,
                            ));
                        }
                        DiskCommand::Append {
                            ingress,
                            expected_head,
                            session_id,
                            run_id,
                            event,
                            reply,
                        } => {
                            let _ = reply.send(journal.append_outcome(
                                &ingress,
                                &expected_head,
                                &session_id,
                                run_id.as_deref(),
                                event,
                            ));
                        }
                        DiskCommand::Replay {
                            cursor,
                            limit,
                            reply,
                        } => {
                            let result = worker_lease
                                .lock()
                                .map_err(|_| LiveEventJournalError::StorageUnavailable)
                                .and_then(|lease| journal.replay_after(&lease, &cursor, limit));
                            let _ = reply.send(result);
                        }
                    }
                }
            })
            .map_err(|_| AgentLiveCoordinatorError::WorkerUnavailable)?;
        Ok(Self {
            commands,
            replay_page_size,
            lease: shared_lease,
        })
    }

    fn lease(&self) -> Result<LiveEventJournalLease, LiveEventJournalError> {
        self.lease
            .lock()
            .map(|lease| lease.clone())
            .map_err(|_| LiveEventJournalError::StorageUnavailable)
    }

    async fn checkpoint(&self) -> Result<LiveEventCursor, LiveEventJournalError> {
        let (reply, response) = oneshot::channel();
        self.commands
            .try_send(DiskCommand::Checkpoint { reply })
            .map_err(|_| LiveEventJournalError::StorageUnavailable)?;
        response
            .await
            .map_err(|_| LiveEventJournalError::StorageUnavailable)?
    }

    async fn load_projection_checkpoint(
        &self,
    ) -> Result<Option<LiveProjectionCheckpoint>, LiveEventJournalError> {
        let (reply, response) = oneshot::channel();
        self.commands
            .try_send(DiskCommand::LoadProjectionCheckpoint { reply })
            .map_err(|_| LiveEventJournalError::StorageUnavailable)?;
        response
            .await
            .map_err(|_| LiveEventJournalError::StorageUnavailable)?
    }

    async fn store_projection_checkpoint(
        &self,
        expected_head: LiveEventCursor,
        bytes: Vec<u8>,
    ) -> Result<LiveEventCursor, LiveEventJournalError> {
        let (reply, response) = oneshot::channel();
        self.commands
            .try_send(DiskCommand::StoreProjectionCheckpoint {
                expected_head,
                bytes,
                reply,
            })
            .map_err(|_| LiveEventJournalError::StorageUnavailable)?;
        response
            .await
            .map_err(|_| LiveEventJournalError::StorageUnavailable)?
    }

    async fn bind_ingress(&self) -> Result<LiveEventJournalIngressLease, LiveEventJournalError> {
        let (reply, response) = oneshot::channel();
        self.commands
            .try_send(DiskCommand::BindIngress { reply })
            .map_err(|_| LiveEventJournalError::StorageUnavailable)?;
        response
            .await
            .map_err(|_| LiveEventJournalError::StorageUnavailable)?
    }

    async fn prepare_rollover(
        &self,
        expected_head: LiveEventCursor,
        bytes: Vec<u8>,
    ) -> Result<LiveEventJournalRolloverObligation, LiveEventJournalError> {
        let (reply, response) = oneshot::channel();
        self.commands
            .try_send(DiskCommand::PrepareRollover {
                expected_head,
                bytes,
                reply,
            })
            .map_err(|_| LiveEventJournalError::StorageUnavailable)?;
        response
            .await
            .map_err(|_| LiveEventJournalError::StorageUnavailable)?
    }

    async fn commit_rollover(
        &self,
        obligation: Arc<LiveEventJournalRolloverObligation>,
        bytes: Vec<u8>,
    ) -> Result<LiveEventJournalActivation, LiveEventJournalError> {
        let (reply, response) = oneshot::channel();
        self.commands
            .try_send(DiskCommand::CommitRollover {
                obligation,
                bytes,
                reply,
            })
            .map_err(|_| LiveEventJournalError::StorageUnavailable)?;
        response
            .await
            .map_err(|_| LiveEventJournalError::StorageUnavailable)?
    }

    async fn classify_event(
        &self,
        ingress: LiveEventJournalIngressLease,
        expected_head: LiveEventCursor,
        session_id: String,
        run_id: Option<String>,
        event: MapleLiveEvent,
    ) -> Result<EventAdmission, LiveEventJournalError> {
        let (reply, response) = oneshot::channel();
        self.commands
            .try_send(DiskCommand::Classify {
                ingress,
                expected_head,
                session_id,
                run_id,
                event,
                reply,
            })
            .map_err(|_| LiveEventJournalError::StorageUnavailable)?;
        response
            .await
            .map_err(|_| LiveEventJournalError::StorageUnavailable)?
    }

    async fn append_outcome(
        &self,
        ingress: LiveEventJournalIngressLease,
        expected_head: LiveEventCursor,
        session_id: String,
        run_id: Option<String>,
        event: MapleLiveEvent,
    ) -> Result<AppendOutcome, LiveEventJournalError> {
        let (reply, response) = oneshot::channel();
        self.commands
            .try_send(DiskCommand::Append {
                ingress,
                expected_head,
                session_id,
                run_id,
                event,
                reply,
            })
            .map_err(|_| LiveEventJournalError::StorageUnavailable)?;
        response
            .await
            .map_err(|_| LiveEventJournalError::StorageUnavailable)?
    }

    async fn replay_after(
        &self,
        cursor: LiveEventCursor,
    ) -> Result<LiveReplayRead<MapleLiveEvent>, LiveEventJournalError> {
        let (reply, response) = oneshot::channel();
        self.commands
            .try_send(DiskCommand::Replay {
                cursor,
                limit: self.replay_page_size,
                reply,
            })
            .map_err(|_| LiveEventJournalError::StorageUnavailable)?;
        response
            .await
            .map_err(|_| LiveEventJournalError::StorageUnavailable)?
    }
}

enum DiskCommand {
    Checkpoint {
        reply: oneshot::Sender<Result<LiveEventCursor, LiveEventJournalError>>,
    },
    LoadProjectionCheckpoint {
        reply: oneshot::Sender<Result<Option<LiveProjectionCheckpoint>, LiveEventJournalError>>,
    },
    StoreProjectionCheckpoint {
        expected_head: LiveEventCursor,
        bytes: Vec<u8>,
        reply: oneshot::Sender<Result<LiveEventCursor, LiveEventJournalError>>,
    },
    BindIngress {
        reply: oneshot::Sender<Result<LiveEventJournalIngressLease, LiveEventJournalError>>,
    },
    PrepareRollover {
        expected_head: LiveEventCursor,
        bytes: Vec<u8>,
        reply: oneshot::Sender<Result<LiveEventJournalRolloverObligation, LiveEventJournalError>>,
    },
    CommitRollover {
        obligation: Arc<LiveEventJournalRolloverObligation>,
        bytes: Vec<u8>,
        reply: oneshot::Sender<Result<LiveEventJournalActivation, LiveEventJournalError>>,
    },
    Classify {
        ingress: LiveEventJournalIngressLease,
        expected_head: LiveEventCursor,
        session_id: String,
        run_id: Option<String>,
        event: MapleLiveEvent,
        reply: oneshot::Sender<Result<EventAdmission, LiveEventJournalError>>,
    },
    Append {
        ingress: LiveEventJournalIngressLease,
        expected_head: LiveEventCursor,
        session_id: String,
        run_id: Option<String>,
        event: MapleLiveEvent,
        reply: oneshot::Sender<Result<AppendOutcome, LiveEventJournalError>>,
    },
    Replay {
        cursor: LiveEventCursor,
        limit: usize,
        reply: oneshot::Sender<Result<LiveReplayRead<MapleLiveEvent>, LiveEventJournalError>>,
    },
}

fn delivery_from_entry(entry: LiveReplayEntry<MapleLiveEvent>) -> AgentLiveDelivery {
    let (cursor, session_id, run_id, event) = entry.into_parts();
    AgentLiveDelivery {
        cursor,
        session_id,
        run_id,
        event,
    }
}

fn validate_subscription_capacity(
    capacity: Option<usize>,
) -> Result<usize, AgentLiveCoordinatorError> {
    let capacity = capacity.unwrap_or(DEFAULT_SUBSCRIPTION_CAPACITY);
    if capacity == 0 || capacity > MAX_SUBSCRIPTION_CAPACITY {
        return Err(AgentLiveCoordinatorError::InvalidSubscriptionCapacity);
    }
    Ok(capacity)
}

fn reserved_subscriber_bytes(capacity: usize) -> Result<usize, AgentLiveCoordinatorError> {
    // Tokio's broadcast ring rounds the requested capacity up to a power of
    // two. Account against the real slot count, not the public request, so a
    // caller cannot bypass the aggregate byte cap with capacities such as 257.
    let ring_slots = capacity
        .checked_next_power_of_two()
        .ok_or(AgentLiveCoordinatorError::SubscriberCapacityExceeded)?;
    MAX_BUFFERED_DELIVERY_BYTES
        .checked_mul(ring_slots)
        .ok_or(AgentLiveCoordinatorError::SubscriberCapacityExceeded)
}

/// Derive the journal owner used for account-generation rotation. The domain
/// separator and length prefixes make an execution target a cryptographic
/// owner boundary rather than advisory metadata.
pub(crate) fn target_bound_owner(
    opaque_account_scope: &str,
    account_generation: u64,
    execution_target: &str,
) -> Result<LiveEventAccountOwner, AgentLiveCoordinatorError> {
    validate_identifier(opaque_account_scope, MAX_ACCOUNT_SCOPE_BYTES)
        .map_err(|_| AgentLiveCoordinatorError::InvalidAccountScope)?;
    validate_identifier(execution_target, MAX_EXECUTION_TARGET_BYTES)
        .map_err(|_| AgentLiveCoordinatorError::InvalidExecutionTarget)?;
    let mut digest = Sha256::new();
    digest.update(b"maple-agent-live-owner-v1\0");
    let account_scope_bytes = u64::try_from(opaque_account_scope.len())
        .map_err(|_| AgentLiveCoordinatorError::InvalidAccountScope)?;
    let execution_target_bytes = u64::try_from(execution_target.len())
        .map_err(|_| AgentLiveCoordinatorError::InvalidExecutionTarget)?;
    digest.update(account_scope_bytes.to_be_bytes());
    digest.update(opaque_account_scope.as_bytes());
    digest.update(execution_target_bytes.to_be_bytes());
    digest.update(execution_target.as_bytes());
    let target_scope = format!("maple-agent-live-v1:{:x}", digest.finalize());
    LiveEventAccountOwner::new(&target_scope, account_generation)
        .map_err(AgentLiveCoordinatorError::Journal)
}

fn validate_event_route(
    session_id: &str,
    run_id: Option<&str>,
    event: &MapleLiveEvent,
) -> Result<(), AgentLiveCoordinatorError> {
    match event {
        MapleLiveEvent::SessionUpdated { session, .. } if session.id != session_id => {
            Err(AgentLiveCoordinatorError::Projection(
                AgentLiveProjectionError::ConflictingItemIdentity,
            ))
        }
        MapleLiveEvent::RunStarted { .. }
        | MapleLiveEvent::RunFinished { .. }
        | MapleLiveEvent::HistoryReplaced { .. }
        | MapleLiveEvent::UserFacingError { .. }
            if run_id.is_none() =>
        {
            Err(AgentLiveCoordinatorError::InvalidRun)
        }
        MapleLiveEvent::HistoryHeadCommitted { .. } | MapleLiveEvent::SessionDeleted { .. }
            if run_id.is_some() =>
        {
            Err(AgentLiveCoordinatorError::InvalidRun)
        }
        MapleLiveEvent::TimelineCleared {
            reason: MapleLiveClearReason::RunStarted | MapleLiveClearReason::HistoryReplaced,
            ..
        } if run_id.is_none() => Err(AgentLiveCoordinatorError::InvalidRun),
        MapleLiveEvent::TimelineCleared {
            reason: MapleLiveClearReason::ExplicitReload,
            ..
        } if run_id.is_some() => Err(AgentLiveCoordinatorError::InvalidRun),
        _ => Ok(()),
    }
}

#[cfg(test)]
mod route_contract_tests {
    use super::*;

    fn cursor() -> LiveEventCursor {
        LiveEventCursor::try_from_parts("11".repeat(16), 1).expect("test cursor")
    }

    #[test]
    fn lifecycle_routes_do_not_smuggle_or_drop_run_ownership() {
        let committed = MapleLiveEvent::HistoryHeadCommitted {
            event_id: "commit".into(),
            history_revision: "revision".into(),
            through_event_cursor: cursor(),
        };
        let deleted = MapleLiveEvent::SessionDeleted {
            event_id: "delete".into(),
        };
        let replaced = MapleLiveEvent::HistoryReplaced {
            event_id: "replace".into(),
        };
        let run_clear = MapleLiveEvent::TimelineCleared {
            event_id: "run-clear".into(),
            reason: MapleLiveClearReason::RunStarted,
        };
        let explicit_clear = MapleLiveEvent::TimelineCleared {
            event_id: "explicit-clear".into(),
            reason: MapleLiveClearReason::ExplicitReload,
        };
        let user_error = MapleLiveEvent::UserFacingError {
            event_id: "error".into(),
            error: MapleLiveUserFacingError {
                id: "error-item".into(),
                kind: MapleLiveUserFacingErrorKind::Error,
                title: Some("Agent error".into()),
                message: SAFE_REMOTE_AGENT_ERROR.into(),
                created_ms: 1,
            },
        };
        assert!(validate_event_route("session", None, &committed).is_ok());
        assert!(validate_event_route("session", Some("run"), &committed).is_err());
        assert!(validate_event_route("session", None, &deleted).is_ok());
        assert!(validate_event_route("session", Some("run"), &deleted).is_err());
        assert!(validate_event_route("session", None, &replaced).is_err());
        assert!(validate_event_route("session", Some("run"), &replaced).is_ok());
        assert!(validate_event_route("session", None, &run_clear).is_err());
        assert!(validate_event_route("session", Some("run"), &run_clear).is_ok());
        assert!(validate_event_route("session", None, &explicit_clear).is_ok());
        assert!(validate_event_route("session", Some("run"), &explicit_clear).is_err());
        assert!(validate_event_route("session", None, &user_error).is_err());
        assert!(validate_event_route("session", Some("run"), &user_error).is_ok());
    }
}

fn validate_owner_id(value: &str) -> Result<(), AgentLiveProjectionError> {
    validate_identifier(value, MAX_OWNER_ID_BYTES)
}

/// Canonical commitment used by the native persistence authority before it
/// mints a stable-operation capability. The schema is deliberately closed and
/// ordered: it contains the exact owner route, event variant, and every
/// presentation-semantic field, but never the journal `event_id`.
pub(crate) fn live_event_payload_commitment(
    session_id: &str,
    run_id: Option<&str>,
    event: &MapleLiveEvent,
) -> Result<[u8; 32], AgentLiveCoordinatorError> {
    validate_owner_id(session_id).map_err(|_| AgentLiveCoordinatorError::InvalidSession)?;
    if let Some(run_id) = run_id {
        validate_owner_id(run_id).map_err(|_| AgentLiveCoordinatorError::InvalidRun)?;
    }
    event
        .validate()
        .map_err(AgentLiveCoordinatorError::Projection)?;
    validate_event_route(session_id, run_id, event)?;

    let mut digest = Sha256::new();
    digest.update(LIVE_PAYLOAD_COMMITMENT_DOMAIN);
    digest.update(AGENT_LIVE_PROJECTION_SCHEMA_VERSION.to_be_bytes());
    update_ingress_hash_part(&mut digest, session_id.as_bytes());
    hash_optional_str(&mut digest, run_id);
    match event {
        MapleLiveEvent::RunStarted { .. } => digest.update([0]),
        MapleLiveEvent::TimelineUpsert { item, .. } => {
            digest.update([1]);
            hash_timeline_item(&mut digest, item);
        }
        MapleLiveEvent::TimelineCleared { reason, .. } => {
            digest.update([2]);
            digest.update([match reason {
                MapleLiveClearReason::RunStarted => 0,
                MapleLiveClearReason::HistoryReplaced => 1,
                MapleLiveClearReason::ExplicitReload => 2,
            }]);
        }
        MapleLiveEvent::HistoryReplaced { .. } => digest.update([3]),
        MapleLiveEvent::HistoryHeadCommitted {
            history_revision,
            through_event_cursor,
            ..
        } => {
            digest.update([4]);
            update_ingress_hash_part(&mut digest, history_revision.as_bytes());
            update_ingress_hash_part(&mut digest, through_event_cursor.journal_id().as_bytes());
            digest.update(through_event_cursor.sequence().to_be_bytes());
        }
        MapleLiveEvent::SessionUpdated { session, .. } => {
            digest.update([5]);
            update_ingress_hash_part(&mut digest, session.id.as_bytes());
            update_ingress_hash_part(&mut digest, session.title.as_bytes());
            update_ingress_hash_part(&mut digest, session.project_root.as_bytes());
            digest.update(session.created_ms.to_be_bytes());
            digest.update(session.updated_ms.to_be_bytes());
            digest.update(session.page_sort_ms.to_be_bytes());
            digest.update(
                u64::try_from(session.message_count)
                    .unwrap_or(u64::MAX)
                    .to_be_bytes(),
            );
            hash_optional_str(&mut digest, session.model.as_deref());
            update_ingress_hash_part(&mut digest, session.mode.as_bytes());
        }
        MapleLiveEvent::RunFinished { terminal, .. } => {
            digest.update([6]);
            digest.update([match terminal {
                MapleLiveRunTerminal::Completed => 0,
                MapleLiveRunTerminal::Cancelled => 1,
                MapleLiveRunTerminal::Failed => 2,
            }]);
        }
        MapleLiveEvent::SessionDeleted { .. } => digest.update([7]),
        MapleLiveEvent::UserFacingError { error, .. } => {
            digest.update([8]);
            update_ingress_hash_part(&mut digest, error.id.as_bytes());
            digest.update([match error.kind {
                MapleLiveUserFacingErrorKind::Warning => 0,
                MapleLiveUserFacingErrorKind::Error => 1,
            }]);
            hash_optional_str(&mut digest, error.title.as_deref());
            update_ingress_hash_part(&mut digest, error.message.as_bytes());
            digest.update(error.created_ms.to_be_bytes());
        }
    }
    Ok(digest.finalize().into())
}

fn hash_timeline_item(digest: &mut Sha256, item: &MapleLiveTimelineItem) {
    update_ingress_hash_part(digest, item.id.as_bytes());
    digest.update([match item.item_type {
        MapleLiveItemType::Message => 0,
        MapleLiveItemType::Thinking => 1,
        MapleLiveItemType::Tool => 2,
        MapleLiveItemType::Permission => 3,
        MapleLiveItemType::System => 4,
        MapleLiveItemType::Error => 5,
    }]);
    match item.role {
        None => digest.update([0]),
        Some(role) => {
            digest.update([1]);
            digest.update([match role {
                MapleLiveRole::User => 0,
                MapleLiveRole::Assistant => 1,
                MapleLiveRole::Thought => 2,
                MapleLiveRole::System => 3,
            }]);
        }
    }
    hash_optional_str(digest, item.title.as_deref());
    hash_optional_str(digest, item.text.as_deref());
    hash_optional_str(digest, item.status.as_deref());
    digest.update(item.created_ms.to_be_bytes());
    digest.update([match item.merge {
        MapleLiveMerge::Append => 0,
        MapleLiveMerge::Replace => 1,
    }]);
}

fn hash_optional_str(digest: &mut Sha256, value: Option<&str>) {
    match value {
        Some(value) => {
            digest.update([1]);
            update_ingress_hash_part(digest, value.as_bytes());
        }
        None => digest.update([0]),
    }
}

fn ingress_event_wire_id(
    namespace: &[u8; 32],
    session_id: &str,
    run_id: Option<&str>,
    durable_stable_operation_id: &str,
) -> String {
    let mut digest = Sha256::new();
    digest.update(INGRESS_EVENT_ID_DOMAIN);
    digest.update(namespace);
    update_ingress_hash_part(&mut digest, session_id.as_bytes());
    match run_id {
        Some(run_id) => {
            digest.update([1]);
            update_ingress_hash_part(&mut digest, run_id.as_bytes());
        }
        None => digest.update([0]),
    }
    update_ingress_hash_part(&mut digest, durable_stable_operation_id.as_bytes());
    format!("v1.{:x}", digest.finalize())
}

fn update_ingress_hash_part(digest: &mut Sha256, value: &[u8]) {
    let length = u64::try_from(value.len()).unwrap_or(u64::MAX);
    digest.update(length.to_be_bytes());
    digest.update(value);
}

fn validate_identifier(value: &str, max_bytes: usize) -> Result<(), AgentLiveProjectionError> {
    if value.is_empty()
        || value.len() > max_bytes
        || value.chars().any(|character| {
            character.is_control()
                || matches!(character, '\u{202a}'..='\u{202e}' | '\u{2066}'..='\u{2069}')
        })
    {
        return Err(AgentLiveProjectionError::InvalidIdentifier);
    }
    Ok(())
}

fn validate_text(value: &str, max_bytes: usize) -> Result<(), AgentLiveProjectionError> {
    if value.len() > max_bytes || value.contains('\0') {
        return Err(AgentLiveProjectionError::TextTooLarge);
    }
    Ok(())
}

fn validate_optional_text(
    value: Option<&str>,
    max_bytes: usize,
) -> Result<(), AgentLiveProjectionError> {
    value.map_or(Ok(()), |value| validate_text(value, max_bytes))
}

fn map_snapshot_reason(reason: SnapshotRequiredReason) -> HeadReloadReason {
    match reason {
        SnapshotRequiredReason::JournalReplaced => HeadReloadReason::JournalReplaced,
        SnapshotRequiredReason::RetentionGap => HeadReloadReason::RetentionGap,
        SnapshotRequiredReason::CursorAhead => HeadReloadReason::CursorAhead,
    }
}

fn map_journal_activation(error: LiveEventJournalActivationError) -> AgentLiveCoordinatorError {
    match error {
        LiveEventJournalActivationError::Journal(error) => map_journal_for_attach(error),
        LiveEventJournalActivationError::ReseedRequired(required) => {
            AgentLiveCoordinatorError::ReseedRequired(required)
        }
    }
}

fn map_journal_for_attach(error: LiveEventJournalError) -> AgentLiveCoordinatorError {
    match error {
        LiveEventJournalError::JournalReplaced | LiveEventJournalError::JournalRetired => {
            AgentLiveCoordinatorError::HeadReloadRequired(HeadReloadReason::JournalReplaced)
        }
        LiveEventJournalError::ReseedRequired => {
            AgentLiveCoordinatorError::HeadReloadRequired(HeadReloadReason::ReseedRequired)
        }
        LiveEventJournalError::OwnerGenerationMismatch
        | LiveEventJournalError::OwnerTransitionIncomplete => {
            AgentLiveCoordinatorError::HeadReloadRequired(HeadReloadReason::OwnerChanged)
        }
        LiveEventJournalError::StorageCorrupt
        | LiveEventJournalError::StorageUnavailable
        | LiveEventJournalError::LockUnavailable => {
            AgentLiveCoordinatorError::HeadReloadRequired(HeadReloadReason::JournalUnavailable)
        }
        other => AgentLiveCoordinatorError::Journal(other),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agent_event_journal::{prepare_live_event_journal_parent, LiveEventJournalLimits};
    use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
    use std::sync::{Condvar, Mutex as StdMutex};
    use std::time::Duration;
    use tempfile::TempDir;
    use tokio::time::timeout;

    fn journal_limits(max_entries: usize) -> LiveEventJournalLimits {
        LiveEventJournalLimits {
            max_entries,
            max_payload_bytes: 256 * 1_024,
            max_total_payload_bytes: 512 * 1_024,
            max_replay_entries: max_entries.min(REPLAY_PAGE_SIZE).max(1),
            max_replay_payload_bytes: 256 * 1_024,
        }
    }

    fn open_journal(
        max_entries: usize,
    ) -> (
        TempDir,
        LiveEventJournal<MapleLiveEvent>,
        LiveEventAccountOwner,
    ) {
        // Match the journal's own proven fixture recipe. The system temporary
        // hierarchy is root-owned and sticky, while this leaf is owner-only.
        let root = tempfile::tempdir().expect("temporary journal root");
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(root.path(), std::fs::Permissions::from_mode(0o700))
                .expect("owner-only temporary journal root");
        }
        let private_parent = root.path().join("private");
        prepare_live_event_journal_parent(&private_parent).expect("prepare private journal parent");
        let journal =
            LiveEventJournal::open(private_parent.join("live"), journal_limits(max_entries))
                .expect("open live journal");
        let owner = LiveEventAccountOwner::new("account-a", 7).expect("valid owner");
        (root, journal, owner)
    }

    fn timeline_event(event_id: &str, item_id: &str, text: &str) -> MapleLiveEvent {
        MapleLiveEvent::TimelineUpsert {
            event_id: event_id.to_string(),
            item: MapleLiveTimelineItem {
                id: item_id.to_string(),
                item_type: MapleLiveItemType::Message,
                role: Some(MapleLiveRole::Assistant),
                title: None,
                text: Some(text.to_string()),
                status: None,
                created_ms: 1,
                merge: MapleLiveMerge::Replace,
            },
        }
    }

    fn permission_event(event_id: &str, status: Option<&str>) -> MapleLiveEvent {
        MapleLiveEvent::TimelineUpsert {
            event_id: event_id.to_string(),
            item: MapleLiveTimelineItem {
                id: "permission-request-1".to_string(),
                item_type: MapleLiveItemType::Permission,
                role: Some(MapleLiveRole::System),
                title: Some(SAFE_REMOTE_PERMISSION_TITLE.to_string()),
                text: None,
                status: status.map(str::to_string),
                created_ms: 1,
                merge: MapleLiveMerge::Replace,
            },
        }
    }

    fn session_updated_event(event_id: &str) -> MapleLiveEvent {
        MapleLiveEvent::SessionUpdated {
            event_id: event_id.to_string(),
            session: MapleLiveSessionSummary {
                id: "session-a".to_string(),
                title: "Task".to_string(),
                project_root: "/workspace".to_string(),
                created_ms: 1,
                updated_ms: 2,
                page_sort_ms: 3,
                message_count: 4,
                model: Some("model".to_string()),
                mode: "auto".to_string(),
            },
        }
    }

    fn user_facing_error_event(event_id: &str, message: impl Into<String>) -> MapleLiveEvent {
        MapleLiveEvent::UserFacingError {
            event_id: event_id.to_string(),
            error: MapleLiveUserFacingError {
                id: format!("error-{event_id}"),
                kind: MapleLiveUserFacingErrorKind::Error,
                title: Some("Agent error".to_string()),
                message: message.into(),
                created_ms: 4,
            },
        }
    }

    fn test_data_owner(target: &str) -> AgentLiveDataOwnerKey {
        AgentLiveDataOwnerKey::for_test("account-a", 7, target, 0)
    }

    async fn active_subscription(
        coordinator: &AgentLiveCoordinator,
        capacity: usize,
    ) -> AgentLiveSubscription {
        let attach = coordinator
            .begin_account_head_attach(Some(capacity))
            .await
            .expect("begin head attach");
        attach
            .token
            .finalize()
            .await
            .expect("finalize head attach")
            .subscription
    }

    fn head_items<'a>(
        attach: &'a AgentHeadAttach,
        session_id: &str,
    ) -> &'a [MapleLiveTimelineItem] {
        attach.live_items_for_session(session_id)
    }

    async fn start_test_coordinator(
        journal: LiveEventJournal<MapleLiveEvent>,
        owner: LiveEventAccountOwner,
        target: &str,
    ) -> AgentLiveCoordinator {
        let lease = journal
            .activate_account(&owner)
            .expect("activate test journal");
        AgentLiveCoordinator::start_with_backend(
            Arc::new(journal),
            lease,
            test_data_owner(target),
            target.to_string(),
            DEFAULT_COMMAND_CAPACITY,
        )
        .await
        .expect("start coordinator")
    }

    #[tokio::test]
    async fn ingress_pairings_and_payload_commitments_fail_closed_before_append() {
        let (_root, journal, owner) = open_journal(16);
        let probe = journal.clone();
        let probe_lease = journal
            .activate_account(&owner)
            .expect("activate probe lease");
        let coordinator = start_test_coordinator(journal, owner, "local").await;
        let old_ingress = coordinator
            .begin_ingress("session-a", Some("run-a".to_string()))
            .await
            .expect("bind old producer");
        let raw = timeline_event("stable-op", "item-1", "original");
        let old_event = coordinator
            .publish_event_for_test(&old_ingress, raw.clone())
            .expect("construct old event");
        let fresh_ingress = coordinator
            .begin_ingress("session-a", Some("run-a".to_string()))
            .await
            .expect("supersede producer");
        let fresh_event = coordinator
            .publish_event_for_test(&fresh_ingress, raw.clone())
            .expect("construct fresh event");

        assert!(matches!(
            coordinator.publish(&fresh_ingress, old_event).await,
            Err(AgentLiveCoordinatorError::IngressRebindRequired)
        ));
        assert!(matches!(
            coordinator.publish(&old_ingress, fresh_event.clone()).await,
            Err(AgentLiveCoordinatorError::IngressRebindRequired)
        ));
        assert_eq!(
            probe
                .checkpoint(&probe_lease)
                .expect("unchanged journal")
                .sequence(),
            0
        );

        let mismatched = AgentLivePublishEvent::timeline_upsert(
            fresh_event.id.clone(),
            match timeline_event("ignored", "item-1", "changed payload") {
                MapleLiveEvent::TimelineUpsert { item, .. } => item,
                _ => unreachable!(),
            },
        );
        assert!(matches!(
            coordinator.publish(&fresh_ingress, mismatched).await,
            Err(AgentLiveCoordinatorError::Journal(
                LiveEventJournalError::EventIdConflict
            ))
        ));
        assert_eq!(
            probe
                .checkpoint(&probe_lease)
                .expect("still unchanged")
                .sequence(),
            0
        );
        assert!(matches!(
            coordinator.begin_account_head_attach(Some(4)).await,
            Err(AgentLiveCoordinatorError::HeadReloadRequired(
                HeadReloadReason::OrderingLost
            ))
        ));
    }

    #[test]
    fn canonical_payload_commitment_excludes_event_id_and_covers_closed_semantics() {
        let first = timeline_event("id-one", "item-1", "same");
        let second = timeline_event("id-two", "item-1", "same");
        assert_eq!(
            live_event_payload_commitment("session-a", Some("run-a"), &first).unwrap(),
            live_event_payload_commitment("session-a", Some("run-a"), &second).unwrap()
        );

        let changed = timeline_event("id-one", "item-1", "changed");
        assert_ne!(
            live_event_payload_commitment("session-a", Some("run-a"), &first).unwrap(),
            live_event_payload_commitment("session-a", Some("run-a"), &changed).unwrap()
        );
        assert_ne!(
            live_event_payload_commitment("session-a", Some("run-a"), &first).unwrap(),
            live_event_payload_commitment("session-b", Some("run-a"), &first).unwrap()
        );

        let unsafe_tool = MapleLiveEvent::TimelineUpsert {
            event_id: "unsafe".to_string(),
            item: MapleLiveTimelineItem {
                id: "tool-1".to_string(),
                item_type: MapleLiveItemType::Tool,
                role: Some(MapleLiveRole::Assistant),
                title: Some("provider secret argument".to_string()),
                text: None,
                status: Some("running".to_string()),
                created_ms: 1,
                merge: MapleLiveMerge::Replace,
            },
        };
        assert!(matches!(
            live_event_payload_commitment("session-a", Some("run-a"), &unsafe_tool),
            Err(AgentLiveCoordinatorError::Projection(
                AgentLiveProjectionError::UnsafePresentation
            ))
        ));
    }

    #[tokio::test]
    async fn activated_start_rejects_mismatched_data_owner_and_target() {
        let (_root, journal, owner) = open_journal(16);
        let lease = journal
            .activate_account(&owner)
            .expect("activate test journal");
        assert!(matches!(
            AgentLiveCoordinator::start_activated(
                journal.clone(),
                lease.clone(),
                AgentLiveDataOwnerKey::for_test("account-a", 8, "local", 0),
                "local",
            )
            .await,
            Err(AgentLiveCoordinatorError::DataOwnerMismatch)
        ));
        assert!(matches!(
            AgentLiveCoordinator::start_activated(
                journal,
                lease,
                AgentLiveDataOwnerKey::for_test("account-a", 7, "target-b", 0),
                "local",
            )
            .await,
            Err(AgentLiveCoordinatorError::InvalidExecutionTarget)
        ));
    }

    #[tokio::test]
    async fn ingress_route_and_epoch_state_are_strictly_bounded() {
        let (_root, journal, owner) = open_journal(16);
        let coordinator = start_test_coordinator(journal, owner, "local").await;
        for index in 0..MAX_INGRESS_ROUTES_PER_ACCOUNT {
            coordinator
                .begin_ingress(format!("session-{index}"), None)
                .await
                .expect("admit bounded route");
        }
        coordinator
            .begin_ingress("session-0", None)
            .await
            .expect("existing route may explicitly rebind at capacity");
        assert!(matches!(
            coordinator.begin_ingress("one-route-too-many", None).await,
            Err(AgentLiveCoordinatorError::IngressRouteCapacityExceeded)
        ));

        let (_root, journal, owner) = open_journal(16);
        let lease = journal
            .activate_account(&owner)
            .expect("activate overflow journal");
        let disk =
            BlockingJournalWorker::spawn(Arc::new(journal), lease).expect("start overflow worker");
        let cursor = disk.checkpoint().await.expect("overflow checkpoint");
        let checkpoint = disk
            .load_projection_checkpoint()
            .await
            .expect("overflow projection checkpoint");
        let mut actor = CoordinatorActor::load(disk, test_data_owner("local"), cursor, checkpoint)
            .await
            .expect("load overflow actor");
        actor.next_producer_epoch = u64::MAX;
        assert!(matches!(
            actor.begin_ingress("session-a".to_string(), None).await,
            Err(AgentLiveCoordinatorError::IngressEpochExhausted)
        ));
        assert!(actor.ingress_epochs.is_empty());
        assert_eq!(actor.next_producer_epoch, u64::MAX);
    }

    #[tokio::test]
    async fn restart_changes_actor_lineage_but_same_journal_retry_stays_idempotent() {
        let (_root, journal, owner) = open_journal(16);
        let coordinator = start_test_coordinator(journal.clone(), owner.clone(), "local").await;
        let old_ingress = coordinator
            .begin_ingress("session-a", Some("run-a".to_string()))
            .await
            .expect("bind original producer");
        let raw = timeline_event("restart-stable-op", "item-1", "exact payload");
        let old_event = coordinator
            .publish_event_for_test(&old_ingress, raw.clone())
            .expect("construct original event");
        let first = coordinator
            .publish(&old_ingress, old_event.clone())
            .await
            .expect("publish original event");

        let restarted = start_test_coordinator(journal, owner, "local").await;
        assert!(matches!(
            restarted.publish(&old_ingress, old_event).await,
            Err(AgentLiveCoordinatorError::IngressRebindRequired)
        ));
        let fresh_ingress = restarted
            .begin_ingress("session-a", Some("run-a".to_string()))
            .await
            .expect("bind restarted producer");
        let retry = restarted
            .publish(
                &fresh_ingress,
                restarted
                    .publish_event_for_test(&fresh_ingress, raw)
                    .expect("reconstruct exact retry"),
            )
            .await
            .expect("same-journal retry is duplicate");
        assert_eq!(retry, first);
    }

    #[tokio::test]
    async fn publish_during_head_load_is_delivered_exactly_once() {
        let (_root, journal, owner) = open_journal(16);
        let coordinator = start_test_coordinator(journal, owner, "local").await;
        let attach = coordinator
            .begin_account_head_attach(Some(4))
            .await
            .expect("begin head attach");
        assert!(head_items(&attach, "session-a").is_empty());
        let published = coordinator
            .publish_for_test(
                "session-a",
                Some("run-a".to_string()),
                timeline_event("event-1", "item-1", "hello"),
            )
            .await
            .expect("publish while paused");

        let mut resumed = attach.token.finalize().await.expect("finalize attach");
        assert_eq!(resumed.through_cursor, published);
        let delivered = timeout(Duration::from_secs(1), resumed.subscription.recv())
            .await
            .expect("delivery timeout")
            .expect("delivery");
        assert_eq!(delivered.cursor, published);
        assert!(matches!(
            delivered.event,
            MapleLiveEvent::TimelineUpsert { ref item, .. }
                if item.id == "item-1" && item.text.as_deref() == Some("hello")
        ));
        assert!(
            timeout(Duration::from_millis(30), resumed.subscription.recv())
                .await
                .is_err()
        );
    }

    #[tokio::test]
    async fn paused_subscriber_overflow_requires_a_head_reload() {
        let (_root, journal, owner) = open_journal(16);
        let coordinator = start_test_coordinator(journal, owner, "local").await;
        let attach = coordinator
            .begin_account_head_attach(Some(1))
            .await
            .expect("begin head attach");
        coordinator
            .publish_for_test(
                "session-a",
                None,
                timeline_event("event-1", "item-1", "one"),
            )
            .await
            .expect("first publish");
        coordinator
            .publish_for_test(
                "session-a",
                None,
                timeline_event("event-2", "item-2", "two"),
            )
            .await
            .expect("second publish");

        assert!(matches!(
            attach.token.finalize().await,
            Err(AgentLiveCoordinatorError::HeadReloadRequired(
                HeadReloadReason::PausedSubscriberOverflow
            ))
        ));
    }

    #[tokio::test]
    async fn aggregate_subscriber_buffer_reservations_are_strictly_bounded() {
        let (_root, journal, owner) = open_journal(16);
        let coordinator = start_test_coordinator(journal, owner, "local").await;
        let first = coordinator
            .begin_account_head_attach(Some(128))
            .await
            .expect("first bounded subscriber");
        let second = coordinator
            .begin_account_head_attach(Some(128))
            .await
            .expect("second bounded subscriber");
        assert!(matches!(
            coordinator.begin_account_head_attach(Some(128)).await,
            Err(AgentLiveCoordinatorError::SubscriberCapacityExceeded)
        ));
        first
            .token
            .cancel()
            .await
            .expect("acknowledged paused attach cancellation");
        let replacement = coordinator
            .begin_account_head_attach(Some(128))
            .await
            .expect("acknowledged cancellation reclaims reservation");
        replacement
            .token
            .cancel()
            .await
            .expect("cancel replacement attach");
        second.token.cancel().await.expect("cancel second attach");
    }

    async fn await_actor_barrier(coordinator: &AgentLiveCoordinator, label: &str) {
        coordinator
            .begin_ingress(format!("barrier-{label}"), None)
            .await
            .expect("actor barrier");
    }

    async fn assert_full_head_capacity(coordinator: &AgentLiveCoordinator) {
        let first = coordinator
            .begin_account_head_attach(Some(128))
            .await
            .expect("first full-size subscriber after cancelled begin");
        let second = coordinator
            .begin_account_head_attach(Some(128))
            .await
            .expect("second full-size subscriber proves no leaked reservation");
        first
            .token
            .cancel()
            .await
            .expect("cancel first proof attach");
        second
            .token
            .cancel()
            .await
            .expect("cancel second proof attach");
    }

    #[tokio::test]
    async fn cancelled_begin_results_release_subscribers_before_capacity_reuse() {
        for drop_before_send in [true, false] {
            let (_root, journal, owner) = open_journal(16);
            let coordinator = start_test_coordinator(journal, owner, "local").await;

            let (sender, _receiver) = broadcast::channel(128);
            let terminal_reason = Arc::new(TerminalMutex::new(None));
            let (reply, response) = oneshot::channel();
            let response = if drop_before_send {
                drop(response);
                None
            } else {
                Some(response)
            };
            coordinator
                .commands
                .send(CoordinatorCommand::BeginHeadAttach {
                    capacity: 128,
                    sender,
                    terminal_reason,
                    cancellation_commands: coordinator.commands.clone(),
                    reply,
                })
                .await
                .expect("queue raw head begin");
            await_actor_barrier(&coordinator, "head-send").await;
            drop(response);
            await_actor_barrier(&coordinator, "head-drop").await;
            assert_full_head_capacity(&coordinator).await;

            let seed = coordinator
                .begin_account_head_attach(Some(1))
                .await
                .expect("capture resume cursor");
            let resume_cursor = seed.through_cursor.clone();
            seed.token.cancel().await.expect("cancel cursor seed");
            let (sender, _receiver) = broadcast::channel(128);
            let terminal_reason = Arc::new(TerminalMutex::new(None));
            let (reply, response) = oneshot::channel();
            let response = if drop_before_send {
                drop(response);
                None
            } else {
                Some(response)
            };
            coordinator
                .commands
                .send(CoordinatorCommand::BeginResume {
                    cursor: resume_cursor,
                    capacity: 128,
                    sender,
                    terminal_reason,
                    cancellation_commands: coordinator.commands.clone(),
                    reply,
                })
                .await
                .expect("queue raw resume begin");
            await_actor_barrier(&coordinator, "resume-send").await;
            drop(response);
            await_actor_barrier(&coordinator, "resume-drop").await;
            assert_full_head_capacity(&coordinator).await;
        }
    }

    #[tokio::test]
    async fn active_unsubscribe_acknowledges_actor_buffer_reclamation() {
        let (_root, journal, owner) = open_journal(16);
        let coordinator = start_test_coordinator(journal, owner, "local").await;
        let first = active_subscription(&coordinator, 128).await;
        let second = active_subscription(&coordinator, 128).await;
        assert!(matches!(
            coordinator.begin_account_head_attach(Some(128)).await,
            Err(AgentLiveCoordinatorError::SubscriberCapacityExceeded)
        ));

        first
            .unsubscribe()
            .await
            .expect("active unsubscribe actor acknowledgement");
        let replacement = coordinator
            .begin_account_head_attach(Some(128))
            .await
            .expect("active subscriber reservation reclaimed");
        replacement
            .token
            .cancel()
            .await
            .expect("cancel replacement");
        second.unsubscribe().await.expect("unsubscribe second");
    }

    fn detached_attach_token(
        subscriber_id: u64,
        commands: mpsc::Sender<CoordinatorCommand>,
    ) -> AgentHeadAttachToken {
        let (_sender, receiver) = broadcast::channel(1);
        AgentHeadAttachToken {
            subscriber_id: Some(subscriber_id),
            commands,
            receiver: Some(receiver),
            terminal_reason: Arc::new(TerminalMutex::new(None)),
        }
    }

    #[tokio::test]
    async fn cancelled_finalize_while_command_send_is_blocked_unregisters_after_backpressure() {
        let (commands, mut actor_commands) = mpsc::channel(1);
        commands
            .send(CoordinatorCommand::Unsubscribe {
                subscriber_id: 999,
                reply: None,
            })
            .await
            .expect("fill bounded actor queue");
        let finalize = tokio::spawn(detached_attach_token(7, commands).finalize());
        tokio::task::yield_now().await;
        assert!(!finalize.is_finished());
        finalize.abort();
        let _ = finalize.await;

        assert!(matches!(
            actor_commands.recv().await,
            Some(CoordinatorCommand::Unsubscribe {
                subscriber_id: 999,
                reply: None,
            })
        ));
        assert!(matches!(
            timeout(Duration::from_secs(1), actor_commands.recv()).await,
            Ok(Some(CoordinatorCommand::Unsubscribe {
                subscriber_id: 7,
                reply: None,
            }))
        ));
    }

    #[tokio::test]
    async fn cancelled_finalize_while_waiting_for_actor_reply_unregisters() {
        let (commands, mut actor_commands) = mpsc::channel(2);
        let finalize = tokio::spawn(detached_attach_token(8, commands).finalize());
        let held_reply = match actor_commands.recv().await {
            Some(CoordinatorCommand::FinalizeHeadAttach {
                subscriber_id: 8,
                reply,
            }) => reply,
            _ => panic!("expected finalize command"),
        };
        assert!(!finalize.is_finished());
        finalize.abort();
        let _ = finalize.await;

        assert!(matches!(
            timeout(Duration::from_secs(1), actor_commands.recv()).await,
            Ok(Some(CoordinatorCommand::Unsubscribe {
                subscriber_id: 8,
                reply: None,
            }))
        ));
        drop(held_reply);
    }

    #[tokio::test]
    async fn account_gap_recovers_only_with_one_complete_interleaved_account_head() {
        let (_root, journal, owner) = open_journal(2);
        let coordinator = start_test_coordinator(journal, owner, "local").await;
        let attach = coordinator
            .begin_account_head_attach(Some(4))
            .await
            .expect("begin head attach");
        for (session_id, event_id, item_id) in [
            ("session-a", "A1", "item-a1"),
            ("session-b", "B1", "item-b1"),
            ("session-a", "A2", "item-a2"),
        ] {
            coordinator
                .publish_for_test(
                    session_id,
                    None,
                    timeline_event(event_id, item_id, event_id),
                )
                .await
                .expect("publish retained event");
        }

        assert!(matches!(
            attach.token.finalize().await,
            Err(AgentLiveCoordinatorError::HeadReloadRequired(
                HeadReloadReason::RetentionGap
            ))
        ));

        let recovered = coordinator
            .begin_account_head_attach(Some(4))
            .await
            .expect("authoritative account recovery head");
        assert!(recovered.live_sessions_complete);
        assert_eq!(recovered.through_cursor.sequence(), 3);
        assert_eq!(
            recovered
                .live_sessions
                .iter()
                .map(|session| session.session_id.as_str())
                .collect::<Vec<_>>(),
            ["session-a", "session-b"]
        );
        assert_eq!(head_items(&recovered, "session-a").len(), 2);
        assert_eq!(head_items(&recovered, "session-b").len(), 1);
    }

    #[tokio::test]
    async fn owner_generation_rotation_invalidates_a_paused_attach() {
        let (_root, journal, owner) = open_journal(16);
        let coordinator = start_test_coordinator(journal.clone(), owner.clone(), "local").await;
        let attach = coordinator
            .begin_account_head_attach(Some(4))
            .await
            .expect("begin head attach");
        let current_owner = LiveEventAccountOwner::new(
            "account-a",
            owner
                .account_generation()
                .checked_add(1)
                .expect("generation"),
        )
        .expect("next owner");
        let lease = journal
            .activate_account(&owner)
            .expect("recover exact active lease");
        journal
            .rotate_account_generation(&lease, &current_owner)
            .expect("rotate account owner");

        assert!(matches!(
            attach.token.finalize().await,
            Err(AgentLiveCoordinatorError::HeadReloadRequired(
                HeadReloadReason::OwnerChanged
            ))
        ));
    }

    #[tokio::test]
    async fn retired_lease_requires_head_reload_and_closes_every_subscriber() {
        let (_root, journal, owner) = open_journal(16);
        let lease = journal
            .activate_account(&owner)
            .expect("activate test journal");
        let coordinator = AgentLiveCoordinator::start_with_backend(
            Arc::new(journal.clone()),
            lease.clone(),
            test_data_owner("local"),
            "local".to_string(),
            DEFAULT_COMMAND_CAPACITY,
        )
        .await
        .expect("start coordinator");
        let mut active = active_subscription(&coordinator, 4).await;
        let pending = coordinator
            .begin_account_head_attach(Some(4))
            .await
            .expect("begin paused attach");
        journal
            .seal_for_retirement(&lease, &pending.through_cursor)
            .expect("externally fence exact lease");

        assert!(matches!(
            coordinator
                .publish_for_test(
                    "session-a",
                    None,
                    timeline_event("after-retirement", "item-1", "must reload"),
                )
                .await,
            Err(AgentLiveCoordinatorError::HeadReloadRequired(
                HeadReloadReason::JournalReplaced
            ))
        ));
        assert_eq!(
            active.recv().await,
            Err(AgentLiveReceiveError::HeadReloadRequired(
                HeadReloadReason::JournalReplaced
            ))
        );
        assert!(matches!(
            pending.token.finalize().await,
            Err(AgentLiveCoordinatorError::HeadReloadRequired(
                HeadReloadReason::JournalReplaced
            ))
        ));
    }

    #[tokio::test]
    async fn replaced_journal_requires_head_reload_and_closes_every_subscriber() {
        let (_root, journal, owner) = open_journal(16);
        let lease = journal
            .activate_account(&owner)
            .expect("activate test journal");
        let coordinator = AgentLiveCoordinator::start_with_backend(
            Arc::new(journal.clone()),
            lease.clone(),
            test_data_owner("local"),
            "local".to_string(),
            DEFAULT_COMMAND_CAPACITY,
        )
        .await
        .expect("start coordinator");
        let mut active = active_subscription(&coordinator, 4).await;
        let pending = coordinator
            .begin_account_head_attach(Some(4))
            .await
            .expect("begin paused attach");
        let bytes = b"external absolute projection";
        journal
            .store_checkpoint(&lease, &pending.through_cursor, bytes)
            .expect("store exact external checkpoint");
        let obligation = journal
            .prepare_rollover(&lease, &pending.through_cursor, bytes)
            .expect("prepare journal generation replacement");
        journal
            .commit_rollover(&obligation, bytes)
            .expect("replace journal generation");

        assert!(matches!(
            coordinator
                .publish_for_test(
                    "session-a",
                    None,
                    timeline_event("after-replacement", "item-1", "must reload"),
                )
                .await,
            Err(AgentLiveCoordinatorError::HeadReloadRequired(
                HeadReloadReason::JournalReplaced
            ))
        ));
        assert_eq!(
            active.recv().await,
            Err(AgentLiveReceiveError::HeadReloadRequired(
                HeadReloadReason::JournalReplaced
            ))
        );
        assert!(matches!(
            pending.token.finalize().await,
            Err(AgentLiveCoordinatorError::HeadReloadRequired(
                HeadReloadReason::JournalReplaced
            ))
        ));
    }

    #[derive(Clone)]
    struct FailingAppendJournal {
        inner: LiveEventJournal<MapleLiveEvent>,
        fail_next_append: Arc<AtomicBool>,
        fail_next_checkpoint: Arc<AtomicBool>,
        commit_before_failure: Arc<AtomicBool>,
        force_next_id_capacity: Arc<AtomicBool>,
        rollover_commit_failures: Arc<AtomicUsize>,
        rollover_commit_calls: Arc<AtomicUsize>,
        rollover_obligation_address: Arc<AtomicUsize>,
        rollover_obligation_reused: Arc<AtomicBool>,
    }

    #[derive(Clone, Default)]
    struct ReplayGate {
        shared: Arc<(StdMutex<ReplayGateState>, Condvar)>,
    }

    #[derive(Default)]
    struct ReplayGateState {
        armed: bool,
        started: bool,
        released: bool,
    }

    impl ReplayGate {
        fn arm(&self) {
            let (state, _) = &*self.shared;
            let mut state = state.lock().expect("lock replay gate");
            state.armed = true;
            state.started = false;
            state.released = false;
        }

        fn block_if_armed(&self) {
            let (state, changed) = &*self.shared;
            let mut state = state.lock().expect("lock replay gate");
            if !state.armed {
                return;
            }
            state.armed = false;
            state.started = true;
            changed.notify_all();
            while !state.released {
                state = changed.wait(state).expect("wait for replay release");
            }
        }

        fn wait_until_started(&self) {
            let (state, changed) = &*self.shared;
            let mut state = state.lock().expect("lock replay gate");
            while !state.started {
                state = changed.wait(state).expect("wait for replay start");
            }
        }

        fn release(&self) {
            let (state, changed) = &*self.shared;
            let mut state = state.lock().expect("lock replay gate");
            state.released = true;
            changed.notify_all();
        }
    }

    #[derive(Clone)]
    struct GatedReplayJournal {
        inner: LiveEventJournal<MapleLiveEvent>,
        gate: ReplayGate,
    }

    impl CoordinatorJournal for GatedReplayJournal {
        fn max_replay_entries(&self) -> usize {
            self.inner.max_replay_entries()
        }

        fn checkpoint(
            &self,
            lease: &LiveEventJournalLease,
        ) -> Result<LiveEventCursor, LiveEventJournalError> {
            self.inner.checkpoint(lease)
        }

        fn load_projection_checkpoint(
            &self,
            lease: &LiveEventJournalLease,
        ) -> Result<Option<LiveProjectionCheckpoint>, LiveEventJournalError> {
            self.inner.load_checkpoint(lease)
        }

        fn store_projection_checkpoint(
            &self,
            lease: &LiveEventJournalLease,
            expected_head: &LiveEventCursor,
            bytes: &[u8],
        ) -> Result<LiveEventCursor, LiveEventJournalError> {
            self.inner.store_checkpoint(lease, expected_head, bytes)
        }

        fn bind_ingress(
            &self,
            lease: &LiveEventJournalLease,
        ) -> Result<LiveEventJournalIngressLease, LiveEventJournalError> {
            self.inner.bind_ingress(lease)
        }

        fn prepare_rollover(
            &self,
            lease: &LiveEventJournalLease,
            expected_head: &LiveEventCursor,
            bytes: &[u8],
        ) -> Result<LiveEventJournalRolloverObligation, LiveEventJournalError> {
            self.inner.prepare_rollover(lease, expected_head, bytes)
        }

        fn commit_rollover(
            &self,
            obligation: &LiveEventJournalRolloverObligation,
            bytes: &[u8],
        ) -> Result<LiveEventJournalActivation, LiveEventJournalError> {
            self.inner.commit_rollover(obligation, bytes)
        }

        fn classify_event(
            &self,
            ingress: &LiveEventJournalIngressLease,
            expected_head: &LiveEventCursor,
            session_id: &str,
            run_id: Option<&str>,
            event: &MapleLiveEvent,
        ) -> Result<EventAdmission, LiveEventJournalError> {
            self.inner
                .classify_event(ingress, expected_head, session_id, run_id, event)
        }

        fn append_outcome(
            &self,
            ingress: &LiveEventJournalIngressLease,
            expected_head: &LiveEventCursor,
            session_id: &str,
            run_id: Option<&str>,
            event: MapleLiveEvent,
        ) -> Result<AppendOutcome, LiveEventJournalError> {
            self.inner
                .append_outcome(ingress, expected_head, session_id, run_id, event)
        }

        fn replay_after(
            &self,
            lease: &LiveEventJournalLease,
            cursor: &LiveEventCursor,
            limit: usize,
        ) -> Result<LiveReplayRead<MapleLiveEvent>, LiveEventJournalError> {
            self.gate.block_if_armed();
            self.inner.replay_after(lease, cursor, limit)
        }
    }

    impl CoordinatorJournal for FailingAppendJournal {
        fn max_replay_entries(&self) -> usize {
            self.inner.max_replay_entries()
        }

        fn checkpoint(
            &self,
            lease: &LiveEventJournalLease,
        ) -> Result<LiveEventCursor, LiveEventJournalError> {
            if self.fail_next_checkpoint.swap(false, Ordering::SeqCst) {
                return Err(LiveEventJournalError::StorageUnavailable);
            }
            self.inner.checkpoint(lease)
        }

        fn load_projection_checkpoint(
            &self,
            lease: &LiveEventJournalLease,
        ) -> Result<Option<LiveProjectionCheckpoint>, LiveEventJournalError> {
            self.inner.load_checkpoint(lease)
        }

        fn store_projection_checkpoint(
            &self,
            lease: &LiveEventJournalLease,
            expected_head: &LiveEventCursor,
            bytes: &[u8],
        ) -> Result<LiveEventCursor, LiveEventJournalError> {
            self.inner.store_checkpoint(lease, expected_head, bytes)
        }

        fn bind_ingress(
            &self,
            lease: &LiveEventJournalLease,
        ) -> Result<LiveEventJournalIngressLease, LiveEventJournalError> {
            self.inner.bind_ingress(lease)
        }

        fn prepare_rollover(
            &self,
            lease: &LiveEventJournalLease,
            expected_head: &LiveEventCursor,
            bytes: &[u8],
        ) -> Result<LiveEventJournalRolloverObligation, LiveEventJournalError> {
            self.inner.prepare_rollover(lease, expected_head, bytes)
        }

        fn commit_rollover(
            &self,
            obligation: &LiveEventJournalRolloverObligation,
            bytes: &[u8],
        ) -> Result<LiveEventJournalActivation, LiveEventJournalError> {
            self.rollover_commit_calls.fetch_add(1, Ordering::SeqCst);
            let address = obligation as *const LiveEventJournalRolloverObligation as usize;
            match self.rollover_obligation_address.compare_exchange(
                0,
                address,
                Ordering::SeqCst,
                Ordering::SeqCst,
            ) {
                Ok(_) => {}
                Err(first) if first != address => {
                    self.rollover_obligation_reused
                        .store(false, Ordering::SeqCst);
                }
                Err(_) => {}
            }
            if self
                .rollover_commit_failures
                .fetch_update(Ordering::SeqCst, Ordering::SeqCst, |remaining| {
                    remaining.checked_sub(1)
                })
                .is_ok()
            {
                return Err(LiveEventJournalError::StorageUnavailable);
            }
            self.inner.commit_rollover(obligation, bytes)
        }

        fn classify_event(
            &self,
            ingress: &LiveEventJournalIngressLease,
            expected_head: &LiveEventCursor,
            session_id: &str,
            run_id: Option<&str>,
            event: &MapleLiveEvent,
        ) -> Result<EventAdmission, LiveEventJournalError> {
            self.inner
                .classify_event(ingress, expected_head, session_id, run_id, event)
        }

        fn append_outcome(
            &self,
            ingress: &LiveEventJournalIngressLease,
            expected_head: &LiveEventCursor,
            session_id: &str,
            run_id: Option<&str>,
            event: MapleLiveEvent,
        ) -> Result<AppendOutcome, LiveEventJournalError> {
            if self.force_next_id_capacity.swap(false, Ordering::SeqCst) {
                return Err(LiveEventJournalError::IdempotencyCapacityExceeded);
            }
            if self.fail_next_append.swap(false, Ordering::SeqCst) {
                if self.commit_before_failure.load(Ordering::SeqCst) {
                    self.inner
                        .append_outcome(ingress, expected_head, session_id, run_id, event)?;
                }
                return Err(LiveEventJournalError::StorageUnavailable);
            }
            self.inner
                .append_outcome(ingress, expected_head, session_id, run_id, event)
        }

        fn replay_after(
            &self,
            lease: &LiveEventJournalLease,
            cursor: &LiveEventCursor,
            limit: usize,
        ) -> Result<LiveReplayRead<MapleLiveEvent>, LiveEventJournalError> {
            self.inner.replay_after(lease, cursor, limit)
        }
    }

    #[tokio::test]
    async fn append_failure_neither_updates_snapshot_nor_fans_out() {
        let (_root, journal, owner) = open_journal(16);
        let lease = journal
            .activate_account(&owner)
            .expect("activate test journal");
        let fail_next_append = Arc::new(AtomicBool::new(false));
        let backend = FailingAppendJournal {
            inner: journal,
            fail_next_append: fail_next_append.clone(),
            fail_next_checkpoint: Arc::new(AtomicBool::new(false)),
            commit_before_failure: Arc::new(AtomicBool::new(false)),
            force_next_id_capacity: Arc::new(AtomicBool::new(false)),
            rollover_commit_failures: Arc::new(AtomicUsize::new(0)),
            rollover_commit_calls: Arc::new(AtomicUsize::new(0)),
            rollover_obligation_address: Arc::new(AtomicUsize::new(0)),
            rollover_obligation_reused: Arc::new(AtomicBool::new(true)),
        };
        let coordinator = AgentLiveCoordinator::start_with_backend(
            Arc::new(backend),
            lease,
            test_data_owner("local"),
            "local".to_string(),
            DEFAULT_COMMAND_CAPACITY,
        )
        .await
        .expect("start coordinator");
        let mut subscription = active_subscription(&coordinator, 4).await;
        fail_next_append.store(true, Ordering::SeqCst);

        assert!(matches!(
            coordinator
                .publish_for_test(
                    "session-a",
                    None,
                    timeline_event("event-1", "item-1", "not durable"),
                )
                .await,
            Err(AgentLiveCoordinatorError::Journal(
                LiveEventJournalError::StorageUnavailable
            ))
        ));
        assert!(timeout(Duration::from_millis(30), subscription.recv())
            .await
            .is_err());
        let attach = coordinator
            .begin_account_head_attach(Some(4))
            .await
            .expect("begin replacement head");
        assert!(head_items(&attach, "session-a").is_empty());
    }

    #[tokio::test]
    async fn ambiguous_post_sync_append_reconciles_before_next_distinct_event() {
        let (_root, journal, owner) = open_journal(16);
        let lease = journal
            .activate_account(&owner)
            .expect("activate test journal");
        let fail_next_append = Arc::new(AtomicBool::new(false));
        let commit_before_failure = Arc::new(AtomicBool::new(true));
        let backend = FailingAppendJournal {
            inner: journal,
            fail_next_append: fail_next_append.clone(),
            fail_next_checkpoint: Arc::new(AtomicBool::new(false)),
            commit_before_failure,
            force_next_id_capacity: Arc::new(AtomicBool::new(false)),
            rollover_commit_failures: Arc::new(AtomicUsize::new(0)),
            rollover_commit_calls: Arc::new(AtomicUsize::new(0)),
            rollover_obligation_address: Arc::new(AtomicUsize::new(0)),
            rollover_obligation_reused: Arc::new(AtomicBool::new(true)),
        };
        let coordinator = AgentLiveCoordinator::start_with_backend(
            Arc::new(backend),
            lease,
            test_data_owner("local"),
            "local".to_string(),
            DEFAULT_COMMAND_CAPACITY,
        )
        .await
        .expect("start coordinator");
        let mut subscription = active_subscription(&coordinator, 4).await;
        fail_next_append.store(true, Ordering::SeqCst);

        let first = coordinator
            .publish_for_test(
                "session-a",
                None,
                timeline_event("event-1", "item-1", "first"),
            )
            .await
            .expect("ambiguous committed append reconciles");
        assert_eq!(first.sequence(), 1);
        let second = coordinator
            .publish_for_test(
                "session-a",
                None,
                timeline_event("event-2", "item-2", "second"),
            )
            .await
            .expect("next distinct event remains ordered");
        assert_eq!(second.sequence(), 2);
        assert_eq!(subscription.recv().await.unwrap().cursor.sequence(), 1);
        assert_eq!(subscription.recv().await.unwrap().cursor.sequence(), 2);
    }

    #[tokio::test]
    async fn ambiguous_rollover_retries_exact_obligation_and_fences_old_subscribers() {
        let (_root, journal, owner) = open_journal(16);
        let lease = journal
            .activate_account(&owner)
            .expect("activate test journal");
        let force_next_id_capacity = Arc::new(AtomicBool::new(false));
        let rollover_commit_failures = Arc::new(AtomicUsize::new(0));
        let rollover_commit_calls = Arc::new(AtomicUsize::new(0));
        let rollover_obligation_reused = Arc::new(AtomicBool::new(true));
        let backend = FailingAppendJournal {
            inner: journal,
            fail_next_append: Arc::new(AtomicBool::new(false)),
            fail_next_checkpoint: Arc::new(AtomicBool::new(false)),
            commit_before_failure: Arc::new(AtomicBool::new(false)),
            force_next_id_capacity: force_next_id_capacity.clone(),
            rollover_commit_failures: rollover_commit_failures.clone(),
            rollover_commit_calls: rollover_commit_calls.clone(),
            rollover_obligation_address: Arc::new(AtomicUsize::new(0)),
            rollover_obligation_reused: rollover_obligation_reused.clone(),
        };
        let coordinator = AgentLiveCoordinator::start_with_backend(
            Arc::new(backend),
            lease,
            test_data_owner("local"),
            "local".to_string(),
            DEFAULT_COMMAND_CAPACITY,
        )
        .await
        .expect("start coordinator");
        let mut active = active_subscription(&coordinator, 4).await;
        let paused = coordinator
            .begin_account_head_attach(Some(4))
            .await
            .expect("begin old-generation paused attach");
        let old_cursor = paused.through_cursor.clone();
        let event = timeline_event("rollover-event", "item-1", "after rollover");
        let old_ingress = coordinator
            .begin_ingress("session-a", None)
            .await
            .expect("bind old-generation publisher");
        let before_barrier = coordinator
            .publish_event_for_test(
                &old_ingress,
                timeline_event("before-rollover", "item-before", "accepted before barrier"),
            )
            .expect("construct pre-barrier event");
        let before_cursor = coordinator
            .publish(&old_ingress, before_barrier)
            .await
            .expect("FIFO event before rollover is accepted");
        assert_eq!(before_cursor.sequence(), 1);
        assert_eq!(
            active
                .recv()
                .await
                .expect("pre-rollover delivery remains ordered")
                .cursor,
            before_cursor
        );
        let old_event = coordinator
            .publish_event_for_test(&old_ingress, event.clone())
            .expect("construct old-generation event");
        force_next_id_capacity.store(true, Ordering::SeqCst);
        rollover_commit_failures.store(2, Ordering::SeqCst);

        assert!(matches!(
            coordinator.publish(&old_ingress, old_event.clone()).await,
            Err(AgentLiveCoordinatorError::Journal(
                LiveEventJournalError::StorageUnavailable
            ))
        ));
        assert_eq!(
            active.recv().await,
            Err(AgentLiveReceiveError::HeadReloadRequired(
                HeadReloadReason::JournalReplaced
            ))
        );

        // An ordinary attach may retry the exact pending commit, but it may
        // not register or inspect the journal while that retry is unresolved.
        assert!(matches!(
            coordinator.begin_account_head_attach(Some(4)).await,
            Err(AgentLiveCoordinatorError::Journal(
                LiveEventJournalError::StorageUnavailable
            ))
        ));
        assert!(matches!(
            paused.token.finalize().await,
            Err(AgentLiveCoordinatorError::HeadReloadRequired(
                HeadReloadReason::JournalReplaced
            ))
        ));
        assert_eq!(rollover_commit_calls.load(Ordering::SeqCst), 3);
        assert!(rollover_obligation_reused.load(Ordering::SeqCst));

        assert!(matches!(
            coordinator.publish(&old_ingress, old_event).await,
            Err(AgentLiveCoordinatorError::IngressRebindRequired)
        ));
        let fresh_ingress = coordinator
            .begin_ingress("session-a", None)
            .await
            .expect("explicitly bind fresh generation");
        let stale_payload = live_event_payload_commitment("session-a", None, &event)
            .expect("canonical old payload");
        let stale_operation = AgentDurableStableOperationId::for_test(
            coordinator.data_owner.clone(),
            "session-a",
            None,
            "rollover-event",
            old_ingress.namespace,
            stale_payload,
        );
        assert!(matches!(
            fresh_ingress.event_id(&stale_operation),
            Err(AgentLiveCoordinatorError::IngressRebindRequired)
        ));
        let fresh_event = timeline_event("post-rollover-operation", "item-2", "after rollover");
        let fresh = coordinator
            .publish(
                &fresh_ingress,
                coordinator
                    .publish_event_for_test(&fresh_ingress, fresh_event)
                    .expect("construct explicit fresh operation"),
            )
            .await
            .expect("publish only after exact rollover commit completed");
        assert_eq!(fresh.sequence(), 1);
        assert_ne!(fresh.journal_id(), old_cursor.journal_id());
        let head = coordinator
            .begin_account_head_attach(Some(4))
            .await
            .expect("attach to fresh generation");
        assert_eq!(head_items(&head, "session-a").len(), 2);
        assert_eq!(head_items(&head, "session-a")[0].id, "item-before");
        assert_eq!(head_items(&head, "session-a")[1].id, "item-2");
    }

    #[tokio::test]
    async fn failed_seal_verification_is_still_a_terminal_fifo_fence() {
        let (_root, journal, owner) = open_journal(16);
        let lease = journal
            .activate_account(&owner)
            .expect("activate test journal");
        let fail_next_checkpoint = Arc::new(AtomicBool::new(false));
        let backend = FailingAppendJournal {
            inner: journal,
            fail_next_append: Arc::new(AtomicBool::new(false)),
            fail_next_checkpoint: fail_next_checkpoint.clone(),
            commit_before_failure: Arc::new(AtomicBool::new(false)),
            force_next_id_capacity: Arc::new(AtomicBool::new(false)),
            rollover_commit_failures: Arc::new(AtomicUsize::new(0)),
            rollover_commit_calls: Arc::new(AtomicUsize::new(0)),
            rollover_obligation_address: Arc::new(AtomicUsize::new(0)),
            rollover_obligation_reused: Arc::new(AtomicBool::new(true)),
        };
        let coordinator = AgentLiveCoordinator::start_with_backend(
            Arc::new(backend),
            lease,
            test_data_owner("local"),
            "local".to_string(),
            DEFAULT_COMMAND_CAPACITY,
        )
        .await
        .expect("start coordinator");
        let mut active = active_subscription(&coordinator, 4).await;
        fail_next_checkpoint.store(true, Ordering::SeqCst);

        assert!(matches!(
            coordinator.seal(AgentLiveSealReason::HostShutdown).await,
            Err(AgentLiveCoordinatorError::Journal(
                LiveEventJournalError::StorageUnavailable
            ))
        ));
        assert_eq!(
            active.recv().await,
            Err(AgentLiveReceiveError::HeadReloadRequired(
                HeadReloadReason::OwnerChanged
            ))
        );
        assert!(matches!(
            coordinator
                .publish_for_test(
                    "session-a",
                    None,
                    timeline_event("after-failed-seal", "item-1", "must fail"),
                )
                .await,
            Err(AgentLiveCoordinatorError::Sealed(
                AgentLiveSealReason::HostShutdown
            ))
        ));
        let recovered = coordinator
            .seal(AgentLiveSealReason::HostShutdown)
            .await
            .expect("same terminal seal retries exact head verification");
        assert_eq!(recovered.reason, AgentLiveSealReason::HostShutdown);
        assert_eq!(recovered.through_cursor.sequence(), 0);
    }

    #[tokio::test]
    async fn actionable_permissions_are_rejected_before_append() {
        let (_root, journal, owner) = open_journal(16);
        let coordinator = start_test_coordinator(journal, owner, "local").await;
        for (event_id, status) in [
            ("missing-status", None),
            ("pending-status", Some("pending")),
        ] {
            assert!(matches!(
                coordinator
                    .publish_for_test(
                        "session-a",
                        Some("run-a".to_string()),
                        permission_event(event_id, status)
                    )
                    .await,
                Err(AgentLiveCoordinatorError::Projection(
                    AgentLiveProjectionError::ActionablePermission
                ))
            ));
        }
        let accepted = coordinator
            .publish_for_test(
                "session-a",
                Some("run-a".to_string()),
                permission_event("cancelled-status", Some("cancelled")),
            )
            .await
            .expect("resolved permission is safe to persist");
        assert_eq!(accepted.sequence(), 1);
        let attach = coordinator
            .begin_account_head_attach(Some(4))
            .await
            .expect("resolved head");
        assert_eq!(head_items(&attach, "session-a").len(), 1);
        assert_eq!(
            head_items(&attach, "session-a")[0].status.as_deref(),
            Some("cancelled")
        );
    }

    #[tokio::test]
    async fn javascript_unsafe_timestamps_and_counts_are_rejected_before_append() {
        let (_root, journal, owner) = open_journal(16);
        let coordinator = start_test_coordinator(journal, owner, "local").await;

        let mut unsafe_item = timeline_event("unsafe-item-time", "item-1", "unsafe");
        let MapleLiveEvent::TimelineUpsert { item, .. } = &mut unsafe_item else {
            unreachable!();
        };
        item.created_ms = MAX_JAVASCRIPT_SAFE_INTEGER + 1;
        assert!(matches!(
            coordinator
                .publish_for_test("session-a", None, unsafe_item)
                .await,
            Err(AgentLiveCoordinatorError::Projection(
                AgentLiveProjectionError::InvalidTimestamp
            ))
        ));

        let timestamp_mutators: [(&str, fn(&mut MapleLiveSessionSummary)); 3] = [
            (
                "negative-created",
                |summary: &mut MapleLiveSessionSummary| summary.created_ms = -1,
            ),
            ("unsafe-updated", |summary: &mut MapleLiveSessionSummary| {
                summary.updated_ms = (MAX_JAVASCRIPT_SAFE_INTEGER + 1) as i64
            }),
            ("unsafe-sort", |summary: &mut MapleLiveSessionSummary| {
                summary.page_sort_ms = (MAX_JAVASCRIPT_SAFE_INTEGER + 1) as i64
            }),
        ];
        for (event_id, mutate) in timestamp_mutators {
            let mut event = session_updated_event(event_id);
            let MapleLiveEvent::SessionUpdated { session, .. } = &mut event else {
                unreachable!();
            };
            mutate(session);
            assert!(matches!(
                coordinator.publish_for_test("session-a", None, event).await,
                Err(AgentLiveCoordinatorError::Projection(
                    AgentLiveProjectionError::InvalidTimestamp
                ))
            ));
        }

        if usize::BITS > 53 {
            let mut unsafe_count = session_updated_event("unsafe-count");
            let MapleLiveEvent::SessionUpdated { session, .. } = &mut unsafe_count else {
                unreachable!();
            };
            session.message_count = usize::try_from(MAX_JAVASCRIPT_SAFE_INTEGER + 1)
                .expect("64-bit usize test platform");
            assert!(matches!(
                coordinator
                    .publish_for_test("session-a", None, unsafe_count)
                    .await,
                Err(AgentLiveCoordinatorError::Projection(
                    AgentLiveProjectionError::InvalidCount
                ))
            ));
        }

        let attach = coordinator
            .begin_account_head_attach(Some(4))
            .await
            .expect("rejections do not advance journal");
        assert_eq!(attach.through_cursor.sequence(), 0);
    }

    #[tokio::test]
    async fn account_cursor_is_contiguous_across_sessions_but_accounts_are_isolated() {
        let (_root, journal, owner_a) = open_journal(32);
        let owner_b = LiveEventAccountOwner::new("account-b", 7).expect("second account");
        let coordinator_a = start_test_coordinator(journal.clone(), owner_a, "same-target").await;
        let coordinator_b = start_test_coordinator(journal, owner_b, "same-target").await;
        let mut subscription_a = active_subscription(&coordinator_a, 8).await;
        let mut subscription_b = active_subscription(&coordinator_b, 8).await;

        for (session_id, event_id, item_id) in [
            ("session-a", "A7", "item-a7"),
            ("session-b", "B8", "item-b8"),
            ("session-a", "A9", "item-a9"),
        ] {
            coordinator_a
                .publish_for_test(
                    session_id,
                    None,
                    timeline_event(event_id, item_id, event_id),
                )
                .await
                .expect("publish interleaved account event");
        }
        let mut previous_sequence = 0;
        for expected_text in ["A7", "B8", "A9"] {
            let delivery = timeout(Duration::from_secs(1), subscription_a.recv())
                .await
                .expect("account A timeout")
                .expect("account A delivery");
            assert!(matches!(
                delivery.event,
                MapleLiveEvent::TimelineUpsert { ref item, .. }
                    if item.text.as_deref() == Some(expected_text)
            ));
            assert_eq!(delivery.cursor.sequence(), previous_sequence + 1);
            previous_sequence = delivery.cursor.sequence();
        }
        assert!(timeout(Duration::from_millis(30), subscription_b.recv())
            .await
            .is_err());

        coordinator_b
            .publish_for_test(
                "session-a",
                None,
                timeline_event("account-b-event", "item-b", "B"),
            )
            .await
            .expect("publish account B");
        let from_b = timeout(Duration::from_secs(1), subscription_b.recv())
            .await
            .expect("account B timeout")
            .expect("account B delivery");
        assert!(matches!(
            from_b.event,
            MapleLiveEvent::TimelineUpsert { ref item, .. }
                if item.id == "item-b" && item.text.as_deref() == Some("B")
        ));
        assert!(timeout(Duration::from_millis(30), subscription_a.recv())
            .await
            .is_err());
    }

    #[tokio::test]
    async fn account_head_is_complete_sorted_unique_and_order_stable_at_one_c0() {
        let (_root, journal, owner) = open_journal(16);
        let coordinator = start_test_coordinator(journal, owner, "local").await;
        coordinator
            .publish_for_test(
                "session-b",
                None,
                timeline_event("event-b1", "item-b1", "B at C1"),
            )
            .await
            .expect("publish session B");
        let c0 = coordinator
            .publish_for_test(
                "session-a",
                None,
                timeline_event("event-a1", "item-a1", "A at C2"),
            )
            .await
            .expect("publish session A");

        let attach = coordinator
            .begin_account_head_attach(Some(4))
            .await
            .expect("begin complete account head");
        assert!(attach.live_sessions_complete);
        assert_eq!(attach.through_cursor, c0);
        assert_eq!(
            attach
                .live_sessions
                .iter()
                .map(|session| session.session_id.as_str())
                .collect::<Vec<_>>(),
            ["session-a", "session-b"]
        );
        assert_eq!(head_items(&attach, "session-a").len(), 1);
        assert_eq!(head_items(&attach, "session-b").len(), 1);
        assert!(head_items(&attach, "session-missing").is_empty());

        let c1 = coordinator
            .publish_for_test(
                "session-a",
                None,
                timeline_event("event-a2", "item-a2", "after C0"),
            )
            .await
            .expect("publish after snapshot barrier");
        assert_eq!(head_items(&attach, "session-a").len(), 1);
        let mut resumed = attach
            .token
            .finalize()
            .await
            .expect("finalize account head");
        assert_eq!(resumed.through_cursor, c1);
        let delivery = timeout(Duration::from_secs(1), resumed.subscription.recv())
            .await
            .expect("post-C0 delivery timeout")
            .expect("post-C0 delivery");
        assert_eq!(delivery.cursor, c1);
        assert!(matches!(
            delivery.event,
            MapleLiveEvent::TimelineUpsert { ref item, .. } if item.id == "item-a2"
        ));
    }

    #[tokio::test]
    async fn empty_snapshot_is_authoritative_and_cursor_resume_is_live() {
        let (_root, journal, owner) = open_journal(16);
        let coordinator = start_test_coordinator(journal, owner, "local").await;
        let attach = coordinator
            .begin_account_head_attach(Some(4))
            .await
            .expect("begin head attach");
        assert_eq!(head_items(&attach, "session-a"), []);
        assert_eq!(attach.through_cursor.sequence(), 0);
        let cursor = attach.through_cursor.clone();
        drop(attach);

        let mut resumed = coordinator
            .begin_resume(cursor, Some(4))
            .await
            .expect("cursor resume");
        let published = coordinator
            .publish_for_test(
                "session-a",
                None,
                timeline_event("event-1", "item-1", "live"),
            )
            .await
            .expect("publish live event");
        let delivered = timeout(Duration::from_secs(1), resumed.subscription.recv())
            .await
            .expect("delivery timeout")
            .expect("delivery");
        assert_eq!(delivered.cursor, published);
    }

    #[tokio::test]
    async fn cursor_resume_replays_the_durable_gap_before_going_live() {
        let (_root, journal, owner) = open_journal(16);
        let coordinator = start_test_coordinator(journal, owner, "local").await;
        let attach = coordinator
            .begin_account_head_attach(Some(4))
            .await
            .expect("capture old cursor");
        let old_cursor = attach.through_cursor.clone();
        drop(attach);
        let published = coordinator
            .publish_for_test(
                "session-a",
                Some("run-a".to_string()),
                timeline_event("event-1", "item-1", "during disconnect"),
            )
            .await
            .expect("publish replay event");

        let mut resumed = coordinator
            .begin_resume(old_cursor, Some(4))
            .await
            .expect("resume cursor");
        assert_eq!(resumed.through_cursor, published);
        let replayed = timeout(Duration::from_secs(1), resumed.subscription.recv())
            .await
            .expect("replay timeout")
            .expect("replayed delivery");
        assert_eq!(replayed.cursor, published);
        assert!(matches!(
            replayed.event,
            MapleLiveEvent::TimelineUpsert { ref item, .. } if item.id == "item-1"
        ));
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn resume_registration_barrier_cannot_lose_a_publish_queued_during_replay() {
        let (_root, journal, owner) = open_journal(16);
        let lease = journal
            .activate_account(&owner)
            .expect("activate test journal");
        let gate = ReplayGate::default();
        let backend = GatedReplayJournal {
            inner: journal,
            gate: gate.clone(),
        };
        let coordinator = AgentLiveCoordinator::start_with_backend(
            Arc::new(backend),
            lease,
            test_data_owner("local"),
            "local".to_string(),
            DEFAULT_COMMAND_CAPACITY,
        )
        .await
        .expect("start coordinator");
        let initial = coordinator
            .begin_account_head_attach(Some(4))
            .await
            .expect("capture resume cursor");
        let resume_cursor = initial.through_cursor;
        drop(initial.token);
        coordinator
            .publish_for_test(
                "session-a",
                None,
                timeline_event("event-1", "item-1", "replayed"),
            )
            .await
            .expect("publish replay gap");

        let queued_ingress = coordinator
            .begin_ingress("session-b", None)
            .await
            .expect("bind queued publisher");
        let queued_event = coordinator
            .publish_event_for_test(
                &queued_ingress,
                timeline_event("event-2", "item-2", "queued live"),
            )
            .expect("construct queued event");

        gate.arm();
        let resume_coordinator = coordinator.clone();
        let resume_task = tokio::spawn(async move {
            resume_coordinator
                .begin_resume(resume_cursor, Some(4))
                .await
        });
        let wait_gate = gate.clone();
        tokio::task::spawn_blocking(move || wait_gate.wait_until_started())
            .await
            .expect("wait for replay worker");

        // Queue directly while the actor is blocked at its replay barrier. A
        // normal `publish` call performs this same send before awaiting reply.
        let (publish_reply, publish_response) = oneshot::channel();
        coordinator
            .commands
            .send(CoordinatorCommand::Publish {
                ingress: queued_ingress,
                event: queued_event,
                reply: publish_reply,
            })
            .await
            .expect("queue publish behind resume barrier");
        gate.release();

        let mut resumed = resume_task
            .await
            .expect("join resume")
            .expect("resume succeeds");
        let published = publish_response
            .await
            .expect("publish response")
            .expect("queued publish succeeds");
        let replayed = timeout(Duration::from_secs(1), resumed.subscription.recv())
            .await
            .expect("replay timeout")
            .expect("replay delivery");
        let live = timeout(Duration::from_secs(1), resumed.subscription.recv())
            .await
            .expect("live timeout")
            .expect("live delivery");
        assert!(matches!(
            replayed.event,
            MapleLiveEvent::TimelineUpsert { ref item, .. } if item.id == "item-1"
        ));
        assert!(matches!(
            live.event,
            MapleLiveEvent::TimelineUpsert { ref item, .. } if item.id == "item-2"
        ));
        assert_eq!(live.session_id, "session-b");
        assert_eq!(published.sequence(), replayed.cursor.sequence() + 1);
        assert_eq!(live.cursor, published);
    }

    #[tokio::test]
    async fn a_stable_event_retry_is_not_applied_or_fanned_out_twice() {
        let (_root, journal, owner) = open_journal(16);
        let coordinator = start_test_coordinator(journal, owner, "local").await;
        let mut subscription = active_subscription(&coordinator, 4).await;
        let event = timeline_event("stable-event", "item-1", "once");
        let first = coordinator
            .publish_for_test("session-a", None, event.clone())
            .await
            .expect("initial append");
        let retry = coordinator
            .publish_for_test("session-a", None, event)
            .await
            .expect("idempotent retry");
        assert_eq!(first, retry);
        let delivered = timeout(Duration::from_secs(1), subscription.recv())
            .await
            .expect("delivery timeout")
            .expect("delivery");
        assert_eq!(delivered.cursor, first);
        assert!(timeout(Duration::from_millis(30), subscription.recv())
            .await
            .is_err());

        let attach = coordinator
            .begin_account_head_attach(Some(4))
            .await
            .expect("absolute snapshot");
        assert_eq!(head_items(&attach, "session-a").len(), 1);
        assert_eq!(
            head_items(&attach, "session-a")[0].text.as_deref(),
            Some("once")
        );
    }

    #[tokio::test]
    async fn a_slow_active_subscriber_gets_an_explicit_reload_error() {
        let (_root, journal, owner) = open_journal(16);
        let coordinator = start_test_coordinator(journal, owner, "local").await;
        let mut subscription = active_subscription(&coordinator, 1).await;
        coordinator
            .publish_for_test(
                "session-a",
                None,
                timeline_event("event-1", "item-1", "one"),
            )
            .await
            .expect("first publish");
        coordinator
            .publish_for_test(
                "session-a",
                None,
                timeline_event("event-2", "item-2", "two"),
            )
            .await
            .expect("second publish");

        assert!(matches!(
            subscription.recv().await,
            Err(AgentLiveReceiveError::HeadReloadRequired(
                HeadReloadReason::SlowSubscriber
            ))
        ));
    }

    #[tokio::test]
    async fn absolute_snapshot_folds_append_events_in_stable_item_order() {
        let (_root, journal, owner) = open_journal(16);
        let coordinator = start_test_coordinator(journal, owner, "local").await;
        coordinator
            .publish_for_test(
                "session-a",
                None,
                timeline_event("event-1", "item-1", "hel"),
            )
            .await
            .expect("initial item");
        let mut appended = timeline_event("event-2", "item-1", "lo");
        let MapleLiveEvent::TimelineUpsert { item, .. } = &mut appended else {
            unreachable!();
        };
        item.merge = MapleLiveMerge::Append;
        coordinator
            .publish_for_test("session-a", None, appended)
            .await
            .expect("append item");
        coordinator
            .publish_for_test(
                "session-a",
                None,
                timeline_event("event-3", "item-2", "second"),
            )
            .await
            .expect("second item");

        let attach = coordinator
            .begin_account_head_attach(Some(4))
            .await
            .expect("head snapshot");
        assert_eq!(head_items(&attach, "session-a").len(), 2);
        assert_eq!(head_items(&attach, "session-a")[0].id, "item-1");
        assert_eq!(
            head_items(&attach, "session-a")[0].text.as_deref(),
            Some("hello")
        );
        assert_eq!(
            head_items(&attach, "session-a")[0].merge,
            MapleLiveMerge::Replace
        );
        assert_eq!(head_items(&attach, "session-a")[1].id, "item-2");
    }

    #[tokio::test]
    async fn startup_rebuilds_absolute_projection_and_preserves_terminal_suffix() {
        let (_root, journal, owner) = open_journal(16);
        journal
            .append(
                &owner,
                "session-a",
                Some("run-a"),
                timeline_event("event-1", "item-1", "hel"),
            )
            .expect("persist initial live item");
        let mut appended = timeline_event("event-2", "item-1", "lo");
        let MapleLiveEvent::TimelineUpsert { item, .. } = &mut appended else {
            unreachable!();
        };
        item.merge = MapleLiveMerge::Append;
        journal
            .append(&owner, "session-a", Some("run-a"), appended)
            .expect("persist append");
        journal
            .append(
                &owner,
                "session-a",
                Some("run-a"),
                MapleLiveEvent::RunFinished {
                    event_id: "event-3".to_string(),
                    terminal: MapleLiveRunTerminal::Completed,
                },
            )
            .expect("persist terminal");

        let coordinator = start_test_coordinator(journal, owner, "local").await;
        let attach = coordinator
            .begin_account_head_attach(Some(4))
            .await
            .expect("rebuilt head");
        assert_eq!(attach.through_cursor.sequence(), 3);
        assert_eq!(head_items(&attach, "session-a").len(), 1);
        assert_eq!(
            head_items(&attach, "session-a")[0].text.as_deref(),
            Some("hello")
        );
        assert_eq!(
            head_items(&attach, "session-a")[0].merge,
            MapleLiveMerge::Replace
        );
    }

    #[tokio::test]
    async fn startup_rebuild_honors_an_explicit_timeline_clear() {
        let (_root, journal, owner) = open_journal(16);
        journal
            .append(
                &owner,
                "session-a",
                Some("run-a"),
                timeline_event("event-1", "item-1", "stale"),
            )
            .expect("persist item");
        journal
            .append(
                &owner,
                "session-a",
                Some("run-a"),
                MapleLiveEvent::TimelineCleared {
                    event_id: "event-2".to_string(),
                    reason: MapleLiveClearReason::HistoryReplaced,
                },
            )
            .expect("persist clear");

        let coordinator = start_test_coordinator(journal, owner, "local").await;
        let attach = coordinator
            .begin_account_head_attach(Some(4))
            .await
            .expect("rebuilt clear head");
        assert_eq!(attach.through_cursor.sequence(), 2);
        assert!(head_items(&attach, "session-a").is_empty());
    }

    #[tokio::test]
    async fn startup_rejects_a_history_commit_that_does_not_name_its_predecessor() {
        let (_root, journal, owner) = open_journal(16);
        let first = journal
            .append(
                &owner,
                "session-a",
                None,
                timeline_event("event-1", "item-1", "must not be erased"),
            )
            .expect("persist item");
        journal
            .append(
                &owner,
                "session-a",
                None,
                MapleLiveEvent::HistoryHeadCommitted {
                    event_id: "malformed-commit".to_string(),
                    history_revision: "revision-1".to_string(),
                    through_event_cursor: first.beginning(),
                },
            )
            .expect("journal accepts structurally valid projected payload");
        let lease = journal
            .activate_account(&owner)
            .expect("activate test journal");

        assert!(matches!(
            AgentLiveCoordinator::start_with_backend(
                Arc::new(journal),
                lease,
                test_data_owner("local"),
                "local".to_string(),
                DEFAULT_COMMAND_CAPACITY,
            )
            .await,
            Err(AgentLiveCoordinatorError::HeadReloadRequired(
                HeadReloadReason::OrderingLost
            ))
        ));
    }

    #[tokio::test]
    async fn restart_restores_checkpoint_then_replays_compacted_terminal_suffix() {
        let (_root, journal, owner) = open_journal(2);
        let lease = journal
            .activate_account(&owner)
            .expect("activate checkpoint journal");
        let namespace = journal
            .bind_ingress(&lease)
            .expect("bind checkpoint journal ingress")
            .event_namespace_commitment();
        let first = timeline_event(
            &ingress_event_wire_id(&namespace, "session-a", None, "event-1"),
            "item-1",
            "checkpointed",
        );
        let first_cursor = journal
            .append(&owner, "session-a", None, first.clone())
            .expect("persist first item");
        let second_cursor = journal
            .append(
                &owner,
                "session-b",
                None,
                timeline_event("event-2", "item-2", "also checkpointed"),
            )
            .expect("persist second item");
        let checkpoint_bytes = serde_json::to_vec(&CoordinatorProjectionCheckpoint {
            format_version: LIVE_PROJECTION_CHECKPOINT_VERSION,
            live_sessions: vec![
                AgentLiveSessionProjection {
                    session_id: "session-a".to_string(),
                    live_items: vec![match first {
                        MapleLiveEvent::TimelineUpsert { item, .. } => item.into_absolute(),
                        _ => unreachable!(),
                    }],
                },
                AgentLiveSessionProjection {
                    session_id: "session-b".to_string(),
                    live_items: vec![MapleLiveTimelineItem {
                        id: "item-2".to_string(),
                        item_type: MapleLiveItemType::Message,
                        role: Some(MapleLiveRole::Assistant),
                        title: None,
                        text: Some("also checkpointed".to_string()),
                        status: None,
                        created_ms: 1,
                        merge: MapleLiveMerge::Replace,
                    }],
                },
            ],
        })
        .expect("encode safe projection checkpoint");
        journal
            .store_checkpoint(&owner, &second_cursor, &checkpoint_bytes)
            .expect("store exact projection checkpoint");
        journal
            .append(
                &owner,
                "session-a",
                Some("run-a"),
                MapleLiveEvent::RunFinished {
                    event_id: "event-3".to_string(),
                    terminal: MapleLiveRunTerminal::Completed,
                },
            )
            .expect("compact covered entries and retain terminal suffix");

        let coordinator = start_test_coordinator(journal, owner, "local").await;
        let attach = coordinator
            .begin_account_head_attach(Some(4))
            .await
            .expect("restart from checkpoint plus suffix");
        assert_eq!(attach.through_cursor.sequence(), 3);
        assert_eq!(head_items(&attach, "session-a").len(), 1);
        assert_eq!(head_items(&attach, "session-b").len(), 1);

        let retry = coordinator
            .publish_for_test(
                "session-a",
                None,
                timeline_event("event-1", "item-1", "checkpointed"),
            )
            .await
            .expect("durable retry remains idempotent after payload eviction");
        assert_eq!(retry, first_cursor);
    }

    #[tokio::test]
    async fn account_projection_exhaustion_is_typed_and_does_not_append() {
        let (_root, journal, owner) = open_journal(128);
        let coordinator = start_test_coordinator(journal, owner, "local").await;
        for index in 0..MAX_LIVE_SESSIONS_PER_ACCOUNT {
            coordinator
                .publish_for_test(
                    format!("session-{index}"),
                    None,
                    timeline_event(
                        &format!("event-{index}"),
                        &format!("item-{index}"),
                        "bounded",
                    ),
                )
                .await
                .expect("publish bounded session");
        }
        assert!(matches!(
            coordinator
                .publish_for_test(
                    "session-overflow",
                    None,
                    timeline_event("overflow-event", "overflow-item", "rejected"),
                )
                .await,
            Err(AgentLiveCoordinatorError::Projection(
                AgentLiveProjectionError::AccountProjectionCapacityExceeded
            ))
        ));
        let attach = coordinator
            .begin_account_head_attach(Some(4))
            .await
            .expect("head remains usable");
        assert_eq!(
            attach.through_cursor.sequence(),
            MAX_LIVE_SESSIONS_PER_ACCOUNT as u64
        );
        assert!(head_items(&attach, "session-overflow").is_empty());
    }

    #[tokio::test]
    async fn terminal_sessions_are_retired_only_after_a_persisted_head_commit() {
        let (_root, journal, owner) = open_journal(256);
        let coordinator = start_test_coordinator(journal, owner, "local").await;
        for index in 0..MAX_LIVE_SESSIONS_PER_ACCOUNT {
            let session_id = format!("session-{index}");
            let run_id = format!("run-{index}");
            coordinator
                .publish_for_test(
                    session_id.clone(),
                    Some(run_id.clone()),
                    timeline_event(
                        &format!("item-event-{index}"),
                        &format!("item-{index}"),
                        "terminal suffix",
                    ),
                )
                .await
                .expect("publish terminal suffix");
            coordinator
                .publish_for_test(
                    session_id,
                    Some(run_id),
                    MapleLiveEvent::RunFinished {
                        event_id: format!("finished-{index}"),
                        terminal: MapleLiveRunTerminal::Completed,
                    },
                )
                .await
                .expect("publish terminal state");
        }
        assert!(matches!(
            coordinator
                .publish_for_test(
                    "session-overflow",
                    Some("run-overflow".to_string()),
                    timeline_event("overflow", "overflow-item", "blocked"),
                )
                .await,
            Err(AgentLiveCoordinatorError::Projection(
                AgentLiveProjectionError::AccountProjectionCapacityExceeded
            ))
        ));

        let current = coordinator
            .begin_account_head_attach(Some(4))
            .await
            .expect("capture persisted head cursor");
        let through = current.through_cursor.clone();
        drop(current);
        coordinator
            .acknowledge_persisted_head_for_test(
                "session-0",
                "commit-session-0",
                "history-revision-session-0",
                through,
            )
            .await
            .expect("retire committed terminal session");
        coordinator
            .publish_for_test(
                "session-after-retirement",
                Some("run-after-retirement".to_string()),
                timeline_event("after-retirement", "new-item", "admitted"),
            )
            .await
            .expect("capacity released only after commit");
    }

    #[tokio::test]
    async fn an_execution_target_cannot_resume_another_targets_cursor() {
        let (_root, journal, _owner) = open_journal(16);
        let coordinator_a =
            AgentLiveCoordinator::start(journal.clone(), "account-a", 7, "target-a")
                .await
                .expect("start target A");
        let coordinator_b = AgentLiveCoordinator::start(journal, "account-a", 7, "target-b")
            .await
            .expect("start target B");
        let target_a = coordinator_a
            .begin_account_head_attach(Some(4))
            .await
            .expect("target A cursor");

        assert!(matches!(
            coordinator_b
                .begin_resume(target_a.through_cursor, Some(4))
                .await,
            Err(AgentLiveCoordinatorError::HeadReloadRequired(
                HeadReloadReason::JournalReplaced
            ))
        ));
    }

    #[tokio::test]
    async fn history_replaced_is_a_non_clearing_signal_until_explicitly_cleared() {
        let (_root, journal, owner) = open_journal(16);
        let coordinator = start_test_coordinator(journal, owner, "local").await;
        coordinator
            .publish_for_test(
                "session-a",
                Some("run-a".to_string()),
                timeline_event("event-1", "item-1", "uncommitted suffix"),
            )
            .await
            .expect("publish live suffix");
        coordinator
            .publish_for_test(
                "session-a",
                Some("run-a".to_string()),
                MapleLiveEvent::HistoryReplaced {
                    event_id: "event-2".to_string(),
                },
            )
            .await
            .expect("publish non-clearing history signal");

        let retained = coordinator
            .begin_account_head_attach(Some(4))
            .await
            .expect("head after history replacement");
        assert_eq!(head_items(&retained, "session-a").len(), 1);
        assert_eq!(
            head_items(&retained, "session-a")[0].text.as_deref(),
            Some("uncommitted suffix")
        );
        drop(retained);

        coordinator
            .publish_for_test(
                "session-a",
                Some("run-a".to_string()),
                MapleLiveEvent::TimelineCleared {
                    event_id: "event-3".to_string(),
                    reason: MapleLiveClearReason::HistoryReplaced,
                },
            )
            .await
            .expect("publish explicit clear");
        let cleared = coordinator
            .begin_account_head_attach(Some(4))
            .await
            .expect("head after explicit clear");
        assert!(head_items(&cleared, "session-a").is_empty());
    }

    #[tokio::test]
    async fn persisted_head_commit_retires_only_the_exact_acknowledged_live_head() {
        let (_root, journal, owner) = open_journal(16);
        let coordinator = start_test_coordinator(journal, owner, "local").await;
        let first = coordinator
            .publish_for_test(
                "session-a",
                Some("run-a".to_string()),
                timeline_event("event-1", "item-1", "persist me"),
            )
            .await
            .expect("publish first suffix");
        let delayed_cursor = first.clone();
        coordinator
            .publish_for_test(
                "session-b",
                Some("run-b".to_string()),
                timeline_event("event-2", "item-2", "newer account event"),
            )
            .await
            .expect("advance account cursor");

        assert!(matches!(
            coordinator
                .acknowledge_persisted_head_for_test(
                    "session-a",
                    "commit-stale",
                    "history-revision-a",
                    delayed_cursor,
                )
                .await,
            Err(AgentLiveCoordinatorError::StaleHistoryCommit)
        ));
        let current = coordinator
            .begin_account_head_attach(Some(4))
            .await
            .expect("capture current account head");
        let through = current.through_cursor.clone();
        drop(current);
        coordinator
            .acknowledge_persisted_head_for_test(
                "session-a",
                "commit-current",
                "history-revision-b",
                through,
            )
            .await
            .expect("commit exact current head");

        let retired = coordinator
            .begin_account_head_attach(Some(4))
            .await
            .expect("complete account head after retirement");
        assert!(retired.live_sessions_complete);
        assert!(head_items(&retired, "session-a").is_empty());
        assert_eq!(head_items(&retired, "session-b").len(), 1);
    }

    #[tokio::test]
    async fn added_lifecycle_variants_are_closed_bounded_and_project_safely() {
        let (_root, journal, owner) = open_journal(16);
        let coordinator = start_test_coordinator(journal, owner, "local").await;

        assert!(matches!(
            coordinator
                .publish_for_test(
                    "session-a",
                    None,
                    MapleLiveEvent::RunStarted {
                        event_id: "run-without-owner".to_string(),
                    },
                )
                .await,
            Err(AgentLiveCoordinatorError::InvalidRun)
        ));
        coordinator
            .publish_for_test(
                "session-a",
                Some("run-a".to_string()),
                MapleLiveEvent::RunStarted {
                    event_id: "run-started".to_string(),
                },
            )
            .await
            .expect("publish owned run start");

        assert!(matches!(
            coordinator
                .publish_for_test(
                    "session-a",
                    Some("run-a".to_string()),
                    user_facing_error_event(
                        "oversized-error",
                        "x".repeat(MAX_USER_FACING_ERROR_MESSAGE_BYTES + 1),
                    ),
                )
                .await,
            Err(AgentLiveCoordinatorError::Projection(
                AgentLiveProjectionError::TextTooLarge
            ))
        ));
        assert!(matches!(
            coordinator
                .publish_for_test(
                    "session-a",
                    Some("run-a".to_string()),
                    user_facing_error_event("empty-error", "   "),
                )
                .await,
            Err(AgentLiveCoordinatorError::Projection(
                AgentLiveProjectionError::InvalidUserFacingError
            ))
        ));
        coordinator
            .publish_for_test(
                "session-a",
                Some("run-a".to_string()),
                user_facing_error_event("safe-error", SAFE_REMOTE_AGENT_ERROR),
            )
            .await
            .expect("publish bounded user-facing error");
        let error_head = coordinator
            .begin_account_head_attach(Some(4))
            .await
            .expect("error head");
        assert_eq!(head_items(&error_head, "session-a").len(), 1);
        assert_eq!(
            head_items(&error_head, "session-a")[0].item_type,
            MapleLiveItemType::Error
        );
        assert_eq!(
            head_items(&error_head, "session-a")[0].text.as_deref(),
            Some(SAFE_REMOTE_AGENT_ERROR)
        );
        drop(error_head);

        coordinator
            .publish_for_test(
                "session-a",
                None,
                MapleLiveEvent::SessionDeleted {
                    event_id: "session-deleted".to_string(),
                },
            )
            .await
            .expect("publish session deletion");
        let deleted_head = coordinator
            .begin_account_head_attach(Some(4))
            .await
            .expect("deleted session head");
        assert!(head_items(&deleted_head, "session-a").is_empty());
        assert_eq!(deleted_head.through_cursor.sequence(), 3);
    }

    #[tokio::test]
    async fn seal_is_a_fifo_terminal_barrier_for_publishes_and_attaches() {
        let (_root, journal, owner) = open_journal(16);
        let retirement_journal = journal.clone();
        let coordinator = start_test_coordinator(journal, owner, "local").await;
        let mut active = active_subscription(&coordinator, 4).await;
        let pending = coordinator
            .begin_account_head_attach(Some(4))
            .await
            .expect("begin pending attach");

        let queued_ingress = coordinator
            .begin_ingress("session-a", None)
            .await
            .expect("bind queued publisher");
        let queued_event = coordinator
            .publish_event_for_test(
                &queued_ingress,
                timeline_event("before-seal", "item-1", "durable first"),
            )
            .expect("construct queued event");

        let (publish_reply, publish_response) = oneshot::channel();
        coordinator
            .commands
            .send(CoordinatorCommand::Publish {
                ingress: queued_ingress,
                event: queued_event,
                reply: publish_reply,
            })
            .await
            .expect("queue publish before seal");
        let sealed = coordinator
            .seal(AgentLiveSealReason::OwnerChanged)
            .await
            .expect("seal after queued publish");

        let persisted = publish_response
            .await
            .expect("publish reply")
            .expect("prior publish completed");
        assert_eq!(persisted.sequence(), 1);
        assert_eq!(sealed.reason, AgentLiveSealReason::OwnerChanged);
        assert_eq!(sealed.through_cursor, persisted);
        retirement_journal
            .seal_for_retirement(&sealed.journal_lease, &sealed.through_cursor)
            .expect("seal result is exact retirement authority");
        let delivered = active.recv().await.expect("buffered prior delivery");
        assert!(matches!(
            delivered.event,
            MapleLiveEvent::TimelineUpsert { ref item, .. }
                if item.id == "item-1" && item.text.as_deref() == Some("durable first")
        ));
        assert_eq!(
            active.recv().await,
            Err(AgentLiveReceiveError::HeadReloadRequired(
                HeadReloadReason::OwnerChanged
            ))
        );
        assert!(matches!(
            pending.token.finalize().await,
            Err(AgentLiveCoordinatorError::Sealed(
                AgentLiveSealReason::OwnerChanged
            ))
        ));
        assert!(matches!(
            coordinator
                .publish_for_test(
                    "session-a",
                    None,
                    timeline_event("after-seal", "item-2", "must fail"),
                )
                .await,
            Err(AgentLiveCoordinatorError::Sealed(
                AgentLiveSealReason::OwnerChanged
            ))
        ));
        assert!(matches!(
            coordinator.begin_account_head_attach(Some(4)).await,
            Err(AgentLiveCoordinatorError::Sealed(
                AgentLiveSealReason::OwnerChanged
            ))
        ));
        let repeated = coordinator
            .seal(AgentLiveSealReason::OwnerChanged)
            .await
            .expect("same seal is idempotent");
        assert_eq!(repeated, sealed);
        assert!(matches!(
            coordinator.seal(AgentLiveSealReason::HostShutdown).await,
            Err(AgentLiveCoordinatorError::Sealed(
                AgentLiveSealReason::OwnerChanged
            ))
        ));
    }
}
