//! Tauri-owned synchronized history-head attachment.
//!
//! The coordinator owns durable ordering; this module owns only the IPC lease
//! and channel lifecycle. A pending lease keeps the exact coordinator token
//! returned at C0. Activation finalizes that same token, queues every account-
//! wide delivery in `(C0, C1]` on the exact channel supplied to `begin`, and
//! only then replaces the prior active stream for the same account and target.
//!
//! Account ownership is always supplied by a verified service binding. This
//! adapter never derives an account or execution target from a global Tauri
//! event sink, a session ID, or an incoming channel payload.

#![allow(
    dead_code,
    reason = "the command wrappers are composed by the Agent host integration"
)]

use crate::{
    agent::{AgentHistoryPage, AgentHistoryPageRequest, AgentLiveEventCursor},
    agent_event_journal::{LiveEventCursor, LiveEventJournalError},
    agent_live_binding::{AgentLiveBindingLease, LocalAuthorizationContext},
    agent_live_coordinator::{
        AgentHeadAttach, AgentHeadAttachToken, AgentLiveCoordinatorError, AgentLiveDelivery,
        AgentLiveReceiveError, AgentLiveResume, AgentLiveSessionProjection, AgentLiveSubscription,
        HeadReloadReason, MapleLiveClearReason, MapleLiveEvent, MapleLiveRunTerminal,
        MapleLiveSessionSummary, MapleLiveTimelineItem,
    },
    agent_live_host::{AgentLiveHostError, AgentLivePeerRevocationHook},
    agent_live_projection::project_timeline_item,
    remote_protocol::{
        ConnectionStamp, MAX_HISTORY_RECORD_PRESENTATION_BYTES, SAFE_REMOTE_AGENT_ERROR,
        SAFE_REMOTE_TOOL_CANCELLED, SAFE_REMOTE_TOOL_FAILED, SAFE_REMOTE_TOOL_TITLE,
    },
    remote_transport::{PairingFence, VerifiedIncomingPeerAuthorization},
};
use async_trait::async_trait;
use getrandom::fill as fill_random;
use serde::{Deserialize, Serialize};
use std::{
    collections::HashMap,
    fmt,
    io::Write,
    sync::{Arc, Mutex, MutexGuard, Weak},
    time::{Duration, Instant},
};
use tauri::{ipc::Channel, State};
use tokio::{sync::oneshot, task::JoinHandle};

const DEFAULT_PENDING_ATTACH_TTL: Duration = Duration::from_secs(30);
const DEFAULT_SUBSCRIPTION_CAPACITY: usize = 128;
const MAX_PENDING_ATTACHES_PER_ACCOUNT_TARGET: usize = 16;
const MAX_PENDING_ATTACHES_TOTAL: usize = 128;
const MAX_LIVE_SESSIONS: usize = 64;
const MAX_LIVE_ITEMS: usize = 512;
const MAX_HISTORY_RECORDS_PER_PAGE: usize = 50;
const MAX_HISTORY_ITEMS_PER_RECORD: usize = 200;
const MAX_ACCOUNT_SCOPE_BYTES: usize = 256;
const MAX_EXECUTION_TARGET_BYTES: usize = 128;
const MAX_JAVASCRIPT_SAFE_INTEGER: u64 = 9_007_199_254_740_991;
const ATTACH_ID_RANDOM_BYTES: usize = 16;
const MAX_ATTACH_ID_ATTEMPTS: usize = 8;
const AGENT_LIVE_PRESENTATION_VERSION: u16 = 1;

/// Exact revocable ownership captured from the service's verified target
/// binding. Only the target ID and full connection stamp cross the IPC wire.
#[derive(Debug, Clone)]
pub(crate) struct AgentLiveLeaseOwner {
    pub(crate) opaque_account_scope: String,
    pub(crate) account_data_generation: u64,
    pub(crate) target_id: String,
    pub(crate) authorization: LocalAuthorizationContext,
    pub(crate) controller_endpoint: iroh::EndpointId,
    pub(crate) pairing_fence: PairingFence,
    pub(crate) connection_stamp: ConnectionStamp,
    pub(crate) binding_lineage_epoch: u64,
    pub(crate) peer_lineage_epoch: u64,
    native_authority: Option<VerifiedIncomingPeerAuthorization>,
}

impl PartialEq for AgentLiveLeaseOwner {
    fn eq(&self, other: &Self) -> bool {
        self.opaque_account_scope == other.opaque_account_scope
            && self.account_data_generation == other.account_data_generation
            && self.target_id == other.target_id
            && self.authorization == other.authorization
            && self.controller_endpoint == other.controller_endpoint
            && self.pairing_fence == other.pairing_fence
            && self.connection_stamp == other.connection_stamp
            && self.binding_lineage_epoch == other.binding_lineage_epoch
            && self.peer_lineage_epoch == other.peer_lineage_epoch
            && match (&self.native_authority, &other.native_authority) {
                (Some(left), Some(right)) => left.same_admission_instance(right),
                (None, None) => true,
                _ => false,
            }
    }
}

impl Eq for AgentLiveLeaseOwner {}

impl AgentLiveLeaseOwner {
    pub(crate) fn from_binding(lease: &AgentLiveBindingLease) -> Self {
        Self {
            opaque_account_scope: lease.account_scope().to_string(),
            account_data_generation: lease.account_generation(),
            target_id: lease.execution_target().as_str().to_string(),
            authorization: lease.authorization().clone(),
            controller_endpoint: lease.controller_endpoint(),
            pairing_fence: lease.pairing_fence(),
            connection_stamp: lease.connection_stamp(),
            binding_lineage_epoch: lease.lineage_epoch(),
            peer_lineage_epoch: lease.peer_lineage_epoch(),
            native_authority: lease.remote_authority().cloned(),
        }
    }

    fn validate(&self) -> Result<(), AgentLiveAttachError> {
        validate_bounded_id(
            &self.opaque_account_scope,
            MAX_ACCOUNT_SCOPE_BYTES,
            "Agent live account scope is invalid",
        )?;
        validate_bounded_id(
            &self.target_id,
            MAX_EXECUTION_TARGET_BYTES,
            "Agent execution target is invalid",
        )?;
        if self.authorization.account_epoch() == 0
            || self.authorization.snapshot_revision() == 0
            || self.binding_lineage_epoch == 0
            || self.peer_lineage_epoch == 0
            || self.connection_stamp.validate().is_err()
            || self.connection_stamp.generation() > MAX_JAVASCRIPT_SAFE_INTEGER
        {
            return Err(AgentLiveAttachError::InvalidRequest {
                message: "Agent live binding is invalid",
            });
        }
        match self.native_authority.as_ref() {
            Some(authority) => authority
                .revalidate_current()
                .map_err(|_| AgentLiveAttachError::StaleLease)?,
            #[cfg(test)]
            None => {}
            #[cfg(not(test))]
            None => return Err(AgentLiveAttachError::Unavailable),
        }
        Ok(())
    }

    fn stream_key(&self) -> AccountTargetKey {
        AccountTargetKey {
            opaque_account_scope: self.opaque_account_scope.clone(),
            account_data_generation: self.account_data_generation,
            target_id: self.target_id.clone(),
            controller_endpoint: self.controller_endpoint,
            pairing_fence: self.pairing_fence,
            binding_lineage_epoch: self.binding_lineage_epoch,
            peer_lineage_epoch: self.peer_lineage_epoch,
            connection_stamp: self.connection_stamp,
        }
    }

    fn stream_lineage_key(&self) -> AccountTargetLineageKey {
        AccountTargetLineageKey {
            opaque_account_scope: self.opaque_account_scope.clone(),
            account_data_generation: self.account_data_generation,
            target_id: self.target_id.clone(),
            controller_endpoint: self.controller_endpoint,
            pairing_fence: self.pairing_fence,
            binding_lineage_epoch: self.binding_lineage_epoch,
            peer_lineage_epoch: self.peer_lineage_epoch,
        }
    }

    fn with_current_authority<R>(
        &self,
        operation: impl FnOnce() -> R,
    ) -> Result<R, AgentLiveAttachError> {
        match self.native_authority.as_ref() {
            Some(authority) => authority
                .with_current(operation)
                .map_err(|_| AgentLiveAttachError::StaleLease),
            #[cfg(test)]
            None => Ok(operation()),
            #[cfg(not(test))]
            None => Err(AgentLiveAttachError::Unavailable),
        }
    }
}

/// Renderer-supplied rejection precondition. Native code resolves the current
/// owner independently, then compares all three fields before retaining or
/// writing the channel. It is never treated as authority.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct AgentExpectedLiveLease {
    pub(crate) target_id: String,
    pub(crate) host_epoch: String,
    pub(crate) connection_generation: u64,
}

impl AgentExpectedLiveLease {
    pub(crate) fn validate_against(
        &self,
        owner: &AgentLiveLeaseOwner,
    ) -> Result<(), AgentLiveAttachError> {
        validate_bounded_id(
            &self.target_id,
            MAX_EXECUTION_TARGET_BYTES,
            "Agent execution target is invalid",
        )?;
        let host_epoch = parse_canonical_host_epoch(&self.host_epoch)?;
        if self.connection_generation == 0
            || self.connection_generation > MAX_JAVASCRIPT_SAFE_INTEGER
        {
            return Err(AgentLiveAttachError::InvalidRequest {
                message: "Agent connection generation is invalid",
            });
        }
        if self.target_id != owner.target_id
            || host_epoch != owner.connection_stamp.host_epoch()
            || self.connection_generation != owner.connection_stamp.generation()
        {
            return Err(AgentLiveAttachError::StaleLease);
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
struct AccountTargetKey {
    opaque_account_scope: String,
    account_data_generation: u64,
    target_id: String,
    controller_endpoint: iroh::EndpointId,
    pairing_fence: PairingFence,
    binding_lineage_epoch: u64,
    peer_lineage_epoch: u64,
    connection_stamp: ConnectionStamp,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
struct AccountTargetLineageKey {
    opaque_account_scope: String,
    account_data_generation: u64,
    target_id: String,
    controller_endpoint: iroh::EndpointId,
    pairing_fence: PairingFence,
    binding_lineage_epoch: u64,
    peer_lineage_epoch: u64,
}

/// Literal complete account snapshot entry captured at C0.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct AgentLiveSessionSnapshot {
    pub(crate) session_id: String,
    pub(crate) live_items: Vec<MapleLiveTimelineItem>,
}

/// One Goose persisted row with a closed presentation-safe item projection.
/// Record count semantics remain native-row based even when `items` contains
/// several timeline cards.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct AgentSafeHistoryRecord {
    pub(crate) record_id: String,
    pub(crate) role: String,
    pub(crate) created_ms: u64,
    pub(crate) items: Vec<MapleLiveTimelineItem>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct AgentSafeHistoryPage {
    pub(crate) records: Vec<AgentSafeHistoryRecord>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) next_cursor: Option<String>,
    pub(crate) history_revision: String,
}

/// Begin wire contract. `live_session_count` is deliberately serialized and
/// independently checked against `live_sessions.len()` before this is built.
#[derive(Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct AgentBeginSessionHistoryAttachResponse {
    pub(crate) attach_id: String,
    pub(crate) page: AgentSafeHistoryPage,
    pub(crate) live_sessions_complete: bool,
    pub(crate) live_session_count: usize,
    pub(crate) live_sessions: Vec<AgentLiveSessionSnapshot>,
    pub(crate) through_event_cursor: AgentLiveEventCursor,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct AgentLiveBarrierResponse {
    pub(crate) through_event_cursor: AgentLiveEventCursor,
    pub(crate) live_stream_id: String,
}

/// Every ordinary live event carries the account-wide durable cursor before a
/// frontend decides whether its session route is currently visible.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct AgentOrderedLiveEvent {
    pub(crate) live_event_version: u16,
    pub(crate) target_id: String,
    pub(crate) host_epoch: String,
    pub(crate) connection_generation: u64,
    pub(crate) event_epoch: String,
    pub(crate) event_sequence: u64,
    pub(crate) session_id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) run_id: Option<String>,
    #[serde(flatten)]
    pub(crate) event: AgentPresentedLiveEvent,
}

/// Version-one closed presentation payload. Every field originates in the
/// already-validated durable Maple event; arbitrary provider JSON, tool
/// input/output, prompts, credentials, and actionable permissions have no
/// representation here.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "eventType", rename_all = "camelCase")]
pub(crate) enum AgentPresentedLiveEvent {
    RunStarted,
    TimelineUpsert {
        item: MapleLiveTimelineItem,
    },
    TimelineCleared {
        reason: MapleLiveClearReason,
    },
    HistoryReplaced,
    /// Advances the durable account cursor for an internal persisted-head
    /// acknowledgement without exposing its storage revision or event ID.
    CursorAdvanced,
    SessionUpdated {
        session: MapleLiveSessionSummary,
    },
    RunFinished {
        terminal: MapleLiveRunTerminal,
    },
    SessionDeleted,
    UserFacingError {
        item: MapleLiveTimelineItem,
    },
}

/// A channel can also terminate with a typed reload instruction. This control
/// frame is not assigned a synthetic event sequence and therefore cannot be
/// mistaken for a durable delivery.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct AgentLiveSnapshotRequiredFrame {
    pub(crate) live_event_version: u16,
    pub(crate) event_type: &'static str,
    pub(crate) target_id: String,
    pub(crate) host_epoch: String,
    pub(crate) connection_generation: u64,
    pub(crate) reason: AgentLiveSnapshotReason,
    pub(crate) last_event_cursor: AgentLiveEventCursor,
}

/// Untagged keeps ordinary event frames byte-compatible with the established
/// flat `eventType` envelope while retaining a closed control-frame shape.
#[derive(Debug, Clone, Serialize)]
#[serde(untagged)]
pub(crate) enum AgentLiveChannelFrame {
    Event(AgentOrderedLiveEvent),
    SnapshotRequired(AgentLiveSnapshotRequiredFrame),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub(crate) enum AgentLiveSnapshotReason {
    #[serde(rename = "paused_overflow")]
    PausedSubscriberOverflow,
    #[serde(rename = "slow_subscriber")]
    SlowSubscriber,
    #[serde(rename = "journal_replaced")]
    JournalReplaced,
    #[serde(rename = "retention_gap")]
    RetentionGap,
    #[serde(rename = "cursor_ahead")]
    CursorAhead,
    #[serde(rename = "owner_changed")]
    OwnerChanged,
    #[serde(rename = "ordering_lost")]
    OrderingLost,
    #[serde(rename = "journal_unavailable")]
    JournalUnavailable,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "code", rename_all = "snake_case")]
pub(crate) enum AgentLiveAttachError {
    InvalidRequest { message: &'static str },
    StaleLease,
    AttachNotFound,
    CapacityExceeded,
    ChannelClosed,
    ProjectionRejected,
    HistoryRecordTooLarge,
    SnapshotRequired { reason: AgentLiveSnapshotReason },
    Unavailable,
}

impl fmt::Display for AgentLiveAttachError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        let message = match self {
            Self::InvalidRequest { message } => message,
            Self::StaleLease => "Agent live attachment is stale",
            Self::AttachNotFound => "Agent live attachment was not found",
            Self::CapacityExceeded => "Too many Agent live attachments are pending",
            Self::ChannelClosed => "Agent live event channel is closed",
            Self::ProjectionRejected => "Agent live event projection was rejected",
            Self::HistoryRecordTooLarge => {
                "One Agent history record is too large to display safely"
            }
            Self::SnapshotRequired { .. } => "Agent history head must be reloaded",
            Self::Unavailable => "Agent live history is unavailable",
        };
        formatter.write_str(message)
    }
}

impl std::error::Error for AgentLiveAttachError {}

/// Service seam that resolves the exact account+target coordinator and the
/// exact account-data generation. Implementations must not silently substitute
/// a newly current owner when the supplied lease is stale.
#[async_trait]
pub(crate) trait AgentLiveAttachProvider: Send + Sync {
    async fn validate_lease(&self, owner: &AgentLiveLeaseOwner)
        -> Result<(), AgentLiveAttachError>;

    async fn begin_account_head_attach(
        &self,
        owner: &AgentLiveLeaseOwner,
        capacity: usize,
    ) -> Result<AgentLiveProviderHeadAttach, AgentLiveAttachError>;

    async fn list_history_page(
        &self,
        owner: &AgentLiveLeaseOwner,
        request: AgentHistoryPageRequest,
    ) -> Result<AgentHistoryPage, AgentLiveAttachError>;

    async fn begin_resume(
        &self,
        owner: &AgentLiveLeaseOwner,
        cursor: LiveEventCursor,
        capacity: usize,
    ) -> Result<AgentLiveProviderResume, AgentLiveAttachError>;
}

/// Closed delivery-to-legacy-envelope projection supplied by the reviewed
/// projection module. Ordering metadata is added only after this succeeds.
pub(crate) trait AgentLiveDeliveryProjector: Send + Sync {
    fn project_delivery(
        &self,
        delivery: &AgentLiveDelivery,
    ) -> Result<AgentPresentedLiveEvent, AgentLiveAttachError>;
}

/// Production projector for the durable closed event set. The trait remains so
/// lifecycle tests can inject a rejection, but even injected projectors can
/// return only this closed enum.
#[derive(Debug, Default)]
pub(crate) struct ClosedAgentLiveDeliveryProjector;

impl AgentLiveDeliveryProjector for ClosedAgentLiveDeliveryProjector {
    fn project_delivery(
        &self,
        delivery: &AgentLiveDelivery,
    ) -> Result<AgentPresentedLiveEvent, AgentLiveAttachError> {
        delivery
            .validate()
            .map_err(|_| AgentLiveAttachError::ProjectionRejected)?;
        validate_wire_safe_event(&delivery.event)?;
        Ok(match &delivery.event {
            MapleLiveEvent::RunStarted { .. } => AgentPresentedLiveEvent::RunStarted,
            MapleLiveEvent::TimelineUpsert { item, .. } => {
                AgentPresentedLiveEvent::TimelineUpsert { item: item.clone() }
            }
            MapleLiveEvent::TimelineCleared { reason, .. } => {
                AgentPresentedLiveEvent::TimelineCleared { reason: *reason }
            }
            MapleLiveEvent::HistoryReplaced { .. } => AgentPresentedLiveEvent::HistoryReplaced,
            MapleLiveEvent::HistoryHeadCommitted { .. } => AgentPresentedLiveEvent::CursorAdvanced,
            MapleLiveEvent::SessionUpdated { session, .. } => {
                AgentPresentedLiveEvent::SessionUpdated {
                    session: session.clone(),
                }
            }
            MapleLiveEvent::RunFinished { terminal, .. } => AgentPresentedLiveEvent::RunFinished {
                terminal: *terminal,
            },
            MapleLiveEvent::SessionDeleted { .. } => AgentPresentedLiveEvent::SessionDeleted,
            MapleLiveEvent::UserFacingError { error, .. } => {
                AgentPresentedLiveEvent::UserFacingError {
                    item: error.to_timeline_item(),
                }
            }
        })
    }
}

fn validate_wire_safe_event(event: &MapleLiveEvent) -> Result<(), AgentLiveAttachError> {
    let item = match event {
        MapleLiveEvent::TimelineUpsert { item, .. } => item,
        _ => return Ok(()),
    };
    match item.item_type {
        crate::agent_live_coordinator::MapleLiveItemType::Tool => {
            let expected_text = match item.status.as_deref() {
                None | Some("pending" | "running" | "completed") => None,
                Some("failed" | "error") => Some(SAFE_REMOTE_TOOL_FAILED),
                Some("cancelled") => Some(SAFE_REMOTE_TOOL_CANCELLED),
                Some(_) => return Err(AgentLiveAttachError::ProjectionRejected),
            };
            if item.title.as_deref() != Some(SAFE_REMOTE_TOOL_TITLE)
                || item.text.as_deref() != expected_text
            {
                return Err(AgentLiveAttachError::ProjectionRejected);
            }
        }
        crate::agent_live_coordinator::MapleLiveItemType::Error => {
            if item.title.as_deref() != Some("Agent error")
                || item.text.as_deref() != Some(SAFE_REMOTE_AGENT_ERROR)
            {
                return Err(AgentLiveAttachError::ProjectionRejected);
            }
        }
        _ => {}
    }
    Ok(())
}

pub(crate) trait AgentLiveEventSender: Send + Sync {
    fn send(&self, frame: AgentLiveChannelFrame) -> Result<(), AgentLiveAttachError>;
}

struct TauriAgentLiveEventSender {
    channel: Channel<AgentLiveChannelFrame>,
}

impl AgentLiveEventSender for TauriAgentLiveEventSender {
    fn send(&self, frame: AgentLiveChannelFrame) -> Result<(), AgentLiveAttachError> {
        self.channel
            .send(frame)
            .map_err(|_| AgentLiveAttachError::ChannelClosed)
    }
}

pub(crate) fn tauri_agent_live_sender(
    channel: Channel<AgentLiveChannelFrame>,
) -> Arc<dyn AgentLiveEventSender> {
    Arc::new(TauriAgentLiveEventSender { channel })
}

/// Native authority resolver installed only after the verified pairing and
/// account-generation host composition is available. Renderer lease fields
/// are rejection preconditions and never reach this trait as authority.
#[async_trait]
pub(crate) trait AgentLiveOwnerResolver: Send + Sync {
    async fn resolve_current_owner(
        &self,
        user_id: &str,
    ) -> Result<AgentLiveLeaseOwner, AgentLiveAttachError>;
}

#[derive(Clone)]
struct EnabledAgentLiveTauriRuntime {
    manager: AgentLiveAttachManager,
    owner_resolver: Arc<dyn AgentLiveOwnerResolver>,
}

/// Managed even while synchronized live mode is unavailable, so every command
/// fails with the stable typed contract instead of an unmanaged-State detail.
#[derive(Clone, Default)]
pub(crate) struct AgentLiveTauriState {
    runtime: Arc<std::sync::RwLock<Option<EnabledAgentLiveTauriRuntime>>>,
}

impl AgentLiveTauriState {
    pub(crate) fn disabled() -> Self {
        Self::default()
    }

    #[allow(dead_code, reason = "installed by the verified host composition")]
    pub(crate) fn install_verified_runtime(
        &self,
        manager: AgentLiveAttachManager,
        owner_resolver: Arc<dyn AgentLiveOwnerResolver>,
    ) -> Result<(), AgentLiveAttachError> {
        let mut runtime = self
            .runtime
            .write()
            .map_err(|_| AgentLiveAttachError::Unavailable)?;
        if runtime.is_some() {
            return Err(AgentLiveAttachError::Unavailable);
        }
        *runtime = Some(EnabledAgentLiveTauriRuntime {
            manager,
            owner_resolver,
        });
        Ok(())
    }

    fn enabled(&self) -> Result<EnabledAgentLiveTauriRuntime, AgentLiveAttachError> {
        self.runtime
            .read()
            .map_err(|_| AgentLiveAttachError::Unavailable)?
            .clone()
            .ok_or(AgentLiveAttachError::Unavailable)
    }

    pub(crate) async fn revoke_exact_owner(
        &self,
        owner: &AgentLiveLeaseOwner,
    ) -> Result<(), AgentLiveAttachError> {
        let runtime = {
            self.runtime
                .read()
                .map_err(|_| AgentLiveAttachError::Unavailable)?
                .clone()
        };
        if let Some(runtime) = runtime {
            runtime.manager.revoke_owner(owner).await;
        }
        Ok(())
    }
}

#[async_trait]
impl AgentLivePeerRevocationHook for AgentLiveTauriState {
    async fn revoke_exact_peer(
        &self,
        revoked: &AgentLiveBindingLease,
    ) -> Result<(), AgentLiveHostError> {
        self.revoke_exact_owner(&AgentLiveLeaseOwner::from_binding(revoked))
            .await
            .map_err(|_| AgentLiveHostError::BoundContextRevoked)
    }
}

async fn resolve_expected_owner(
    state: &AgentLiveTauriState,
    user_id: &str,
    expected_lease: &AgentExpectedLiveLease,
) -> Result<(EnabledAgentLiveTauriRuntime, AgentLiveLeaseOwner), AgentLiveAttachError> {
    validate_bounded_id(user_id, 512, "Agent user ID is invalid")?;
    let runtime = state.enabled()?;
    let owner = runtime
        .owner_resolver
        .resolve_current_owner(user_id)
        .await?;
    owner.validate()?;
    expected_lease.validate_against(&owner)?;
    Ok((runtime, owner))
}

#[tauri::command]
pub(crate) async fn agent_begin_session_history_attach(
    state: State<'_, AgentLiveTauriState>,
    user_id: String,
    request: AgentHistoryPageRequest,
    expected_lease: AgentExpectedLiveLease,
    events: Channel<AgentLiveChannelFrame>,
) -> Result<AgentBeginSessionHistoryAttachResponse, AgentLiveAttachError> {
    let (runtime, owner) = resolve_expected_owner(state.inner(), &user_id, &expected_lease).await?;
    runtime
        .manager
        .begin(owner, request, tauri_agent_live_sender(events))
        .await
}

#[tauri::command]
pub(crate) async fn agent_activate_session_history_attach(
    state: State<'_, AgentLiveTauriState>,
    user_id: String,
    attach_id: String,
    expected_lease: AgentExpectedLiveLease,
) -> Result<AgentLiveBarrierResponse, AgentLiveAttachError> {
    let (runtime, owner) = resolve_expected_owner(state.inner(), &user_id, &expected_lease).await?;
    runtime.manager.activate(owner, &attach_id).await
}

#[tauri::command]
pub(crate) async fn agent_cancel_session_history_attach(
    state: State<'_, AgentLiveTauriState>,
    user_id: String,
    attach_id: String,
    expected_lease: AgentExpectedLiveLease,
) -> Result<(), AgentLiveAttachError> {
    let (runtime, owner) = resolve_expected_owner(state.inner(), &user_id, &expected_lease).await?;
    runtime.manager.cancel(owner, &attach_id).await
}

#[tauri::command]
pub(crate) async fn agent_resume_live_events(
    state: State<'_, AgentLiveTauriState>,
    user_id: String,
    cursor: AgentLiveEventCursor,
    expected_lease: AgentExpectedLiveLease,
    events: Channel<AgentLiveChannelFrame>,
) -> Result<AgentLiveBarrierResponse, AgentLiveAttachError> {
    let (runtime, owner) = resolve_expected_owner(state.inner(), &user_id, &expected_lease).await?;
    runtime
        .manager
        .resume(owner, cursor, tauri_agent_live_sender(events))
        .await
}

#[tauri::command]
pub(crate) async fn agent_cancel_live_events(
    state: State<'_, AgentLiveTauriState>,
    user_id: String,
    live_stream_id: String,
    expected_lease: AgentExpectedLiveLease,
) -> Result<(), AgentLiveAttachError> {
    let (runtime, owner) = resolve_expected_owner(state.inner(), &user_id, &expected_lease).await?;
    runtime
        .manager
        .cancel_live_events(owner, &live_stream_id)
        .await
}

/// Object-safe pending token used so lifecycle tests do not need to fabricate
/// coordinator-private subscriber IDs.
#[async_trait]
pub(crate) trait AgentLivePendingAttach: Send {
    async fn finalize(self: Box<Self>) -> Result<AgentLiveProviderResume, AgentLiveAttachError>;

    async fn cancel(self: Box<Self>) -> Result<(), AgentLiveAttachError>;
}

#[async_trait]
pub(crate) trait AgentLiveProviderStream: Send {
    async fn recv(&mut self) -> Result<AgentLiveDelivery, AgentLiveReceiveError>;

    async fn unsubscribe(self: Box<Self>) -> Result<(), AgentLiveAttachError>;
}

pub(crate) struct AgentLiveProviderHeadAttach {
    pub(crate) through_cursor: LiveEventCursor,
    pub(crate) live_sessions_complete: bool,
    pub(crate) live_sessions: Vec<AgentLiveSessionProjection>,
    pub(crate) token: Box<dyn AgentLivePendingAttach>,
}

pub(crate) struct AgentLiveProviderResume {
    pub(crate) through_cursor: LiveEventCursor,
    pub(crate) stream: Box<dyn AgentLiveProviderStream>,
}

struct CoordinatorPendingAttach {
    token: Option<AgentHeadAttachToken>,
}

#[async_trait]
impl AgentLivePendingAttach for CoordinatorPendingAttach {
    async fn finalize(
        mut self: Box<Self>,
    ) -> Result<AgentLiveProviderResume, AgentLiveAttachError> {
        let token = self
            .token
            .take()
            .ok_or(AgentLiveAttachError::AttachNotFound)?;
        token
            .finalize()
            .await
            .map(coordinator_resume)
            .map_err(map_coordinator_error)
    }

    async fn cancel(mut self: Box<Self>) -> Result<(), AgentLiveAttachError> {
        let token = self
            .token
            .take()
            .ok_or(AgentLiveAttachError::AttachNotFound)?;
        token.cancel().await.map_err(map_coordinator_error)
    }
}

struct CoordinatorProviderStream {
    subscription: AgentLiveSubscription,
}

#[async_trait]
impl AgentLiveProviderStream for CoordinatorProviderStream {
    async fn recv(&mut self) -> Result<AgentLiveDelivery, AgentLiveReceiveError> {
        self.subscription.recv().await
    }

    async fn unsubscribe(self: Box<Self>) -> Result<(), AgentLiveAttachError> {
        self.subscription
            .unsubscribe()
            .await
            .map_err(map_coordinator_error)
    }
}

pub(crate) fn coordinator_head_attach(attach: AgentHeadAttach) -> AgentLiveProviderHeadAttach {
    AgentLiveProviderHeadAttach {
        through_cursor: attach.through_cursor,
        live_sessions_complete: attach.live_sessions_complete,
        live_sessions: attach.live_sessions,
        token: Box::new(CoordinatorPendingAttach {
            token: Some(attach.token),
        }),
    }
}

pub(crate) fn coordinator_resume(resume: AgentLiveResume) -> AgentLiveProviderResume {
    AgentLiveProviderResume {
        through_cursor: resume.through_cursor,
        stream: Box::new(CoordinatorProviderStream {
            subscription: resume.subscription,
        }),
    }
}

#[derive(Debug, Clone)]
pub(crate) struct AgentLiveAttachManagerConfig {
    pub(crate) pending_ttl: Duration,
    pub(crate) subscription_capacity: usize,
    pub(crate) max_pending_per_account_target: usize,
    pub(crate) max_pending_total: usize,
}

impl Default for AgentLiveAttachManagerConfig {
    fn default() -> Self {
        Self {
            pending_ttl: DEFAULT_PENDING_ATTACH_TTL,
            subscription_capacity: DEFAULT_SUBSCRIPTION_CAPACITY,
            max_pending_per_account_target: MAX_PENDING_ATTACHES_PER_ACCOUNT_TARGET,
            max_pending_total: MAX_PENDING_ATTACHES_TOTAL,
        }
    }
}

impl AgentLiveAttachManagerConfig {
    fn validate(&self) -> Result<(), AgentLiveAttachError> {
        if self.pending_ttl.is_zero()
            || self.subscription_capacity == 0
            || self.max_pending_per_account_target == 0
            || self.max_pending_total == 0
            || self.max_pending_per_account_target > self.max_pending_total
        {
            return Err(AgentLiveAttachError::InvalidRequest {
                message: "Agent live attachment limits are invalid",
            });
        }
        Ok(())
    }
}

#[derive(Clone)]
pub(crate) struct AgentLiveAttachManager {
    inner: Arc<AgentLiveAttachManagerInner>,
}

struct AgentLiveAttachManagerInner {
    provider: Arc<dyn AgentLiveAttachProvider>,
    projector: Arc<dyn AgentLiveDeliveryProjector>,
    config: AgentLiveAttachManagerConfig,
    state: Mutex<AgentLiveAttachState>,
}

#[derive(Default)]
struct AgentLiveAttachState {
    reservations: HashMap<String, AgentLiveLeaseOwner>,
    pending: HashMap<String, PendingAttach>,
    activating: HashMap<AccountTargetLineageKey, ActivatingStream>,
    active: HashMap<AccountTargetKey, ActiveStream>,
}

struct PendingAttach {
    owner: AgentLiveLeaseOwner,
    through_cursor: LiveEventCursor,
    token: Box<dyn AgentLivePendingAttach>,
    sender: Arc<dyn AgentLiveEventSender>,
    expires_at: Instant,
}

struct ActiveStream {
    stream_id: String,
    owner: AgentLiveLeaseOwner,
    send_fence: Arc<Mutex<bool>>,
    cancel: Option<oneshot::Sender<()>>,
    task: JoinHandle<()>,
}

struct ActivatingStream {
    operation_id: String,
    owner: AgentLiveLeaseOwner,
    owner_key: AccountTargetKey,
    send_fence: Arc<Mutex<bool>>,
    cancel: Option<oneshot::Sender<()>>,
    done: Option<oneshot::Receiver<()>>,
}

#[derive(Clone)]
struct ActiveStreamIdentity {
    key: AccountTargetKey,
    stream_id: String,
    owner: AgentLiveLeaseOwner,
}

struct ActivationReservation {
    lineage_key: AccountTargetLineageKey,
    owner_key: AccountTargetKey,
    operation_id: String,
    owner: AgentLiveLeaseOwner,
    superseded: Option<ActiveStreamIdentity>,
    send_fence: Arc<Mutex<bool>>,
    cancel: oneshot::Receiver<()>,
    done: Option<oneshot::Sender<()>>,
}

enum ActivationSource {
    Pending(PendingAttach),
    Resume {
        from: LiveEventCursor,
        sender: Arc<dyn AgentLiveEventSender>,
    },
}

#[derive(Default)]
struct CleanupBatch {
    pending: Vec<Box<dyn AgentLivePendingAttach>>,
    activating: Vec<ActivatingStream>,
    active: Vec<ActiveStream>,
}

impl CleanupBatch {
    async fn run(mut self) {
        for activating in &self.activating {
            close_send_fence(&activating.send_fence);
        }
        for active in &self.active {
            close_send_fence(&active.send_fence);
        }
        for activating in &mut self.activating {
            if let Some(cancel) = activating.cancel.take() {
                let _ = cancel.send(());
            }
        }
        for active in &mut self.active {
            if let Some(cancel) = active.cancel.take() {
                let _ = cancel.send(());
            }
        }
        for token in self.pending {
            let _ = token.cancel().await;
        }
        for mut activating in self.activating {
            if let Some(done) = activating.done.take() {
                let _ = done.await;
            }
        }
        for active in self.active {
            let _ = active.task.await;
        }
    }

    fn is_empty(&self) -> bool {
        self.pending.is_empty() && self.activating.is_empty() && self.active.is_empty()
    }
}

struct PendingTokenGuard {
    token: Option<Box<dyn AgentLivePendingAttach>>,
}

impl PendingTokenGuard {
    fn new(token: Box<dyn AgentLivePendingAttach>) -> Self {
        Self { token: Some(token) }
    }

    fn take(&mut self) -> Result<Box<dyn AgentLivePendingAttach>, AgentLiveAttachError> {
        self.token.take().ok_or(AgentLiveAttachError::Unavailable)
    }

    async fn cancel(mut self) {
        if let Some(token) = self.token.take() {
            let _ = token.cancel().await;
        }
    }
}

impl Drop for PendingTokenGuard {
    fn drop(&mut self) {
        if let Some(token) = self.token.take() {
            spawn_pending_cancel(token);
        }
    }
}

struct ProviderStreamGuard {
    stream: Option<Box<dyn AgentLiveProviderStream>>,
}

impl ProviderStreamGuard {
    fn new(stream: Box<dyn AgentLiveProviderStream>) -> Self {
        Self {
            stream: Some(stream),
        }
    }

    async fn recv(&mut self) -> Result<AgentLiveDelivery, AgentLiveReceiveError> {
        match self.stream.as_mut() {
            Some(stream) => stream.recv().await,
            None => Err(AgentLiveReceiveError::Closed),
        }
    }

    async fn unsubscribe(mut self) {
        if let Some(stream) = self.stream.take() {
            let _ = stream.unsubscribe().await;
        }
    }

    fn take(&mut self) -> Result<Box<dyn AgentLiveProviderStream>, AgentLiveAttachError> {
        self.stream.take().ok_or(AgentLiveAttachError::Unavailable)
    }
}

impl Drop for ProviderStreamGuard {
    fn drop(&mut self) {
        if let Some(stream) = self.stream.take() {
            spawn_stream_unsubscribe(stream);
        }
    }
}

/// Releases an in-flight capacity reservation if the async begin future is
/// cancelled at any await point before the paused token is committed.
struct PendingReservationGuard {
    inner: Weak<AgentLiveAttachManagerInner>,
    attach_id: String,
    owner: AgentLiveLeaseOwner,
    committed: bool,
}

impl PendingReservationGuard {
    fn new(
        inner: &Arc<AgentLiveAttachManagerInner>,
        attach_id: String,
        owner: AgentLiveLeaseOwner,
    ) -> Self {
        Self {
            inner: Arc::downgrade(inner),
            attach_id,
            owner,
            committed: false,
        }
    }

    fn commit(&mut self) {
        self.committed = true;
    }
}

impl Drop for PendingReservationGuard {
    fn drop(&mut self) {
        if self.committed {
            return;
        }
        let Some(inner) = self.inner.upgrade() else {
            return;
        };
        let Ok(mut state) = inner.state.lock() else {
            return;
        };
        if state
            .reservations
            .get(&self.attach_id)
            .is_some_and(|owner| owner == &self.owner)
        {
            state.reservations.remove(&self.attach_id);
        }
    }
}

struct ActivationDoneGuard {
    inner: Weak<AgentLiveAttachManagerInner>,
    lineage_key: AccountTargetLineageKey,
    operation_id: String,
    send_fence: Arc<Mutex<bool>>,
    committed_active: bool,
    done: Option<oneshot::Sender<()>>,
}

impl ActivationDoneGuard {
    fn new(
        inner: &Arc<AgentLiveAttachManagerInner>,
        reservation: &mut ActivationReservation,
    ) -> Self {
        Self {
            inner: Arc::downgrade(inner),
            lineage_key: reservation.lineage_key.clone(),
            operation_id: reservation.operation_id.clone(),
            send_fence: Arc::clone(&reservation.send_fence),
            committed_active: false,
            done: reservation.done.take(),
        }
    }

    fn commit_active(&mut self) {
        self.committed_active = true;
    }
}

impl Drop for ActivationDoneGuard {
    fn drop(&mut self) {
        if let Some(inner) = self.inner.upgrade() {
            if let Ok(mut state) = inner.state.lock() {
                if state
                    .activating
                    .get(&self.lineage_key)
                    .is_some_and(|activating| activating.operation_id == self.operation_id)
                {
                    state.activating.remove(&self.lineage_key);
                }
            }
        }
        if !self.committed_active {
            close_send_fence(&self.send_fence);
        }
        if let Some(done) = self.done.take() {
            let _ = done.send(());
        }
    }
}

impl Drop for AgentLiveAttachManagerInner {
    fn drop(&mut self) {
        if let Ok(mut state) = self.state.lock() {
            let cleanup = CleanupBatch {
                pending: state
                    .pending
                    .drain()
                    .map(|(_, pending)| pending.token)
                    .collect(),
                activating: state
                    .activating
                    .drain()
                    .map(|(_, activating)| activating)
                    .collect(),
                active: state.active.drain().map(|(_, active)| active).collect(),
            };
            state.reservations.clear();
            drop(state);
            spawn_cleanup(cleanup);
        }
    }
}

impl AgentLiveAttachManager {
    pub(crate) fn new(
        provider: Arc<dyn AgentLiveAttachProvider>,
        projector: Arc<dyn AgentLiveDeliveryProjector>,
    ) -> Self {
        Self::with_config(provider, projector, AgentLiveAttachManagerConfig::default())
            .expect("default Agent live attachment limits must be valid")
    }

    pub(crate) fn with_config(
        provider: Arc<dyn AgentLiveAttachProvider>,
        projector: Arc<dyn AgentLiveDeliveryProjector>,
        config: AgentLiveAttachManagerConfig,
    ) -> Result<Self, AgentLiveAttachError> {
        config.validate()?;
        Ok(Self {
            inner: Arc::new(AgentLiveAttachManagerInner {
                provider,
                projector,
                config,
                state: Mutex::new(AgentLiveAttachState::default()),
            }),
        })
    }

    pub(crate) async fn begin(
        &self,
        owner: AgentLiveLeaseOwner,
        request: AgentHistoryPageRequest,
        sender: Arc<dyn AgentLiveEventSender>,
    ) -> Result<AgentBeginSessionHistoryAttachResponse, AgentLiveAttachError> {
        owner.validate()?;
        if request.cursor.is_some() {
            return Err(AgentLiveAttachError::InvalidRequest {
                message: "A synchronized Agent history attach must start at the newest page",
            });
        }
        validate_bounded_id(&request.session_id, 128, "Agent task ID is invalid")?;
        let requested_limit = request.limit.unwrap_or(25);
        if !(1..=MAX_HISTORY_RECORDS_PER_PAGE).contains(&requested_limit) {
            return Err(AgentLiveAttachError::InvalidRequest {
                message: "Agent history page limit must be between 1 and 50",
            });
        }
        self.inner.provider.validate_lease(&owner).await?;

        let attach_id = owner.with_current_authority(|| self.reserve_pending_slot(&owner))??;
        let mut reservation =
            PendingReservationGuard::new(&self.inner, attach_id.clone(), owner.clone());
        let begun = match self
            .inner
            .provider
            .begin_account_head_attach(&owner, self.inner.config.subscription_capacity)
            .await
        {
            Ok(begun) => begun,
            Err(error) => {
                self.release_reservation(&attach_id, &owner);
                return Err(error);
            }
        };

        let AgentLiveProviderHeadAttach {
            through_cursor,
            live_sessions_complete,
            live_sessions,
            token,
        } = begun;
        let mut token = PendingTokenGuard::new(token);
        let snapshots = match validate_and_restore_snapshot(live_sessions_complete, live_sessions) {
            Ok(snapshots) => snapshots,
            Err(error) => {
                self.release_reservation(&attach_id, &owner);
                token.cancel().await;
                return Err(error);
            }
        };
        let through_event_cursor = cursor_to_wire(&through_cursor);

        let page = match self.inner.provider.list_history_page(&owner, request).await {
            Ok(page) => match project_safe_history_page(page, requested_limit) {
                Ok(page) => page,
                Err(error) => {
                    self.release_reservation(&attach_id, &owner);
                    token.cancel().await;
                    return Err(error);
                }
            },
            Err(error) => {
                self.release_reservation(&attach_id, &owner);
                token.cancel().await;
                return Err(error);
            }
        };
        if let Err(error) = self.inner.provider.validate_lease(&owner).await {
            self.release_reservation(&attach_id, &owner);
            token.cancel().await;
            return Err(error);
        }

        let live_session_count = snapshots.len();
        let pending = PendingAttach {
            owner: owner.clone(),
            through_cursor,
            token: token.take()?,
            sender,
            expires_at: Instant::now() + self.inner.config.pending_ttl,
        };
        if let Err((error, pending)) = self.finish_reservation(&attach_id, pending) {
            self.release_reservation(&attach_id, &owner);
            let _ = pending.token.cancel().await;
            return Err(error);
        }
        reservation.commit();
        self.spawn_expiry(attach_id.clone(), owner.clone());

        Ok(AgentBeginSessionHistoryAttachResponse {
            attach_id,
            page,
            live_sessions_complete: true,
            live_session_count,
            live_sessions: snapshots,
            through_event_cursor,
        })
    }

    pub(crate) async fn activate(
        &self,
        owner: AgentLiveLeaseOwner,
        attach_id: &str,
    ) -> Result<AgentLiveBarrierResponse, AgentLiveAttachError> {
        owner.validate()?;
        self.inner.provider.validate_lease(&owner).await?;
        let (pending, reservation) = self.reserve_pending_activation(&owner, attach_id)?;
        self.spawn_activation(reservation, ActivationSource::Pending(pending))
            .await
    }

    /// Idempotent for an already-cancelled or unknown ID, but never permits a
    /// caller with a stale owner to cancel a current owner's opaque lease.
    pub(crate) async fn cancel(
        &self,
        owner: AgentLiveLeaseOwner,
        attach_id: &str,
    ) -> Result<(), AgentLiveAttachError> {
        owner.validate()?;
        validate_attach_or_stream_id(attach_id, "Agent live attachment ID is invalid")?;
        self.inner.provider.validate_lease(&owner).await?;
        let cleanup =
            owner.with_current_authority(|| self.fence_attachment(&owner, attach_id))??;
        cleanup.run().await;
        Ok(())
    }

    pub(crate) async fn resume(
        &self,
        owner: AgentLiveLeaseOwner,
        cursor: AgentLiveEventCursor,
        sender: Arc<dyn AgentLiveEventSender>,
    ) -> Result<AgentLiveBarrierResponse, AgentLiveAttachError> {
        owner.validate()?;
        self.inner.provider.validate_lease(&owner).await?;
        let from = wire_to_cursor(&cursor)?;
        let live_stream_id = random_opaque_id()?;
        let reservation = owner
            .with_current_authority(|| self.reserve_resume_activation(&owner, &live_stream_id))??;
        self.spawn_activation(reservation, ActivationSource::Resume { from, sender })
            .await
    }

    /// Abort the exact active channel retained by an attached UI. Unknown or
    /// already-removed IDs are idempotent only when there is no different
    /// active stream for this current account+target.
    pub(crate) async fn cancel_live_events(
        &self,
        owner: AgentLiveLeaseOwner,
        live_stream_id: &str,
    ) -> Result<(), AgentLiveAttachError> {
        owner.validate()?;
        validate_attach_or_stream_id(live_stream_id, "Agent live stream ID is invalid")?;
        self.inner.provider.validate_lease(&owner).await?;
        let cleanup =
            owner.with_current_authority(|| self.fence_live_stream(&owner, live_stream_id))??;
        cleanup.run().await;
        Ok(())
    }

    /// Trusted binding-transition hook. It fences every matching lifecycle
    /// phase, then awaits paused-token cancellation and stream unsubscribe.
    pub(crate) async fn revoke_owner(&self, owner: &AgentLiveLeaseOwner) {
        let cleanup = {
            let Ok(mut state) = self.inner.state.lock() else {
                return;
            };
            state
                .reservations
                .retain(|_, reserved_owner| reserved_owner != owner);
            let mut cleanup = CleanupBatch::default();
            let pending_ids = state
                .pending
                .iter()
                .filter_map(|(attach_id, pending)| {
                    (&pending.owner == owner).then_some(attach_id.clone())
                })
                .collect::<Vec<_>>();
            for attach_id in pending_ids {
                if let Some(pending) = state.pending.remove(&attach_id) {
                    cleanup.pending.push(pending.token);
                }
            }
            let activating_keys = state
                .activating
                .iter()
                .filter_map(|(key, activating)| (&activating.owner == owner).then_some(key.clone()))
                .collect::<Vec<_>>();
            for key in activating_keys {
                if let Some(activating) = state.activating.remove(&key) {
                    cleanup.activating.push(activating);
                }
            }
            let active_keys = state
                .active
                .iter()
                .filter_map(|(key, active)| (&active.owner == owner).then_some(key.clone()))
                .collect::<Vec<_>>();
            for key in active_keys {
                if let Some(active) = state.active.remove(&key) {
                    cleanup.active.push(active);
                }
            }
            cleanup
        };
        cleanup.run().await;
    }

    fn reserve_pending_slot(
        &self,
        owner: &AgentLiveLeaseOwner,
    ) -> Result<String, AgentLiveAttachError> {
        let (result, expired) = {
            let mut state = self.lock_state()?;
            let expired = take_expired_pending(&mut state);
            let result = (|| {
                let total = state
                    .pending
                    .len()
                    .checked_add(state.reservations.len())
                    .and_then(|count| count.checked_add(state.activating.len()))
                    .ok_or(AgentLiveAttachError::CapacityExceeded)?;
                if total >= self.inner.config.max_pending_total {
                    return Err(AgentLiveAttachError::CapacityExceeded);
                }
                let key = owner.stream_lineage_key();
                let per_account = state
                    .pending
                    .values()
                    .filter(|pending| pending.owner.stream_lineage_key() == key)
                    .count()
                    + state
                        .reservations
                        .values()
                        .filter(|reserved_owner| reserved_owner.stream_lineage_key() == key)
                        .count()
                    + usize::from(state.activating.contains_key(&key));
                if per_account >= self.inner.config.max_pending_per_account_target {
                    return Err(AgentLiveAttachError::CapacityExceeded);
                }
                let attach_id = allocate_attach_id(&state)?;
                state.reservations.insert(attach_id.clone(), owner.clone());
                Ok(attach_id)
            })();
            (result, expired)
        };
        spawn_pending_cancels(expired);
        result
    }

    fn finish_reservation(
        &self,
        attach_id: &str,
        pending: PendingAttach,
    ) -> Result<(), (AgentLiveAttachError, PendingAttach)> {
        let mut state = match self.lock_state() {
            Ok(state) => state,
            Err(error) => return Err((error, pending)),
        };
        match state.reservations.get(attach_id) {
            Some(owner) if owner == &pending.owner => {
                if state.pending.contains_key(attach_id) {
                    return Err((AgentLiveAttachError::Unavailable, pending));
                }
                state.reservations.remove(attach_id);
                state.pending.insert(attach_id.to_string(), pending);
                Ok(())
            }
            _ => Err((AgentLiveAttachError::StaleLease, pending)),
        }
    }

    fn release_reservation(&self, attach_id: &str, owner: &AgentLiveLeaseOwner) {
        if let Ok(mut state) = self.inner.state.lock() {
            if state
                .reservations
                .get(attach_id)
                .is_some_and(|reserved| reserved == owner)
            {
                state.reservations.remove(attach_id);
            }
        }
    }

    fn spawn_expiry(&self, attach_id: String, owner: AgentLiveLeaseOwner) {
        let weak = Arc::downgrade(&self.inner);
        let ttl = self.inner.config.pending_ttl;
        tokio::spawn(async move {
            tokio::time::sleep(ttl).await;
            let Some(inner) = weak.upgrade() else {
                return;
            };
            let pending = {
                let Ok(mut state) = inner.state.lock() else {
                    return;
                };
                if state.pending.get(&attach_id).is_some_and(|pending| {
                    pending.owner == owner && Instant::now() >= pending.expires_at
                }) {
                    state.pending.remove(&attach_id)
                } else {
                    None
                }
            };
            if let Some(pending) = pending {
                let _ = pending.token.cancel().await;
            }
        });
    }

    fn reserve_pending_activation(
        &self,
        owner: &AgentLiveLeaseOwner,
        attach_id: &str,
    ) -> Result<(PendingAttach, ActivationReservation), AgentLiveAttachError> {
        validate_attach_or_stream_id(attach_id, "Agent live attachment ID is invalid")?;
        let (result, expired) = {
            let mut state = self.lock_state()?;
            let expired = take_expired_pending(&mut state);
            let result = (|| {
                let pending = state
                    .pending
                    .get(attach_id)
                    .ok_or(AgentLiveAttachError::AttachNotFound)?;
                if &pending.owner != owner {
                    return Err(AgentLiveAttachError::StaleLease);
                }
                let reservation = reserve_activation_locked(&mut state, owner, attach_id)?;
                let pending = state
                    .pending
                    .remove(attach_id)
                    .ok_or(AgentLiveAttachError::AttachNotFound)?;
                Ok((pending, reservation))
            })();
            (result, expired)
        };
        spawn_pending_cancels(expired);
        result
    }

    fn reserve_resume_activation(
        &self,
        owner: &AgentLiveLeaseOwner,
        live_stream_id: &str,
    ) -> Result<ActivationReservation, AgentLiveAttachError> {
        let (result, expired) = {
            let mut state = self.lock_state()?;
            let expired = take_expired_pending(&mut state);
            let result = if state_id_in_use(&state, live_stream_id) {
                Err(AgentLiveAttachError::Unavailable)
            } else {
                reserve_activation_locked(&mut state, owner, live_stream_id)
            };
            (result, expired)
        };
        spawn_pending_cancels(expired);
        result
    }

    async fn spawn_activation(
        &self,
        reservation: ActivationReservation,
        source: ActivationSource,
    ) -> Result<AgentLiveBarrierResponse, AgentLiveAttachError> {
        let (result, response) = oneshot::channel();
        let inner = Arc::clone(&self.inner);
        tokio::spawn(async move {
            run_activation(inner, reservation, source, result).await;
        });
        response
            .await
            .map_err(|_| AgentLiveAttachError::Unavailable)?
    }

    fn fence_attachment(
        &self,
        owner: &AgentLiveLeaseOwner,
        attach_id: &str,
    ) -> Result<CleanupBatch, AgentLiveAttachError> {
        let mut state = self.lock_state()?;
        if state
            .reservations
            .get(attach_id)
            .is_some_and(|reserved| reserved != owner)
            || state
                .pending
                .get(attach_id)
                .is_some_and(|pending| &pending.owner != owner)
            || state.activating.values().any(|activating| {
                activating.operation_id == attach_id && &activating.owner != owner
            })
            || state
                .active
                .values()
                .any(|active| active.stream_id == attach_id && &active.owner != owner)
        {
            return Err(AgentLiveAttachError::StaleLease);
        }
        let mut cleanup = CleanupBatch::default();
        cleanup.pending.extend(
            take_expired_pending(&mut state)
                .into_iter()
                .map(|pending| pending.token),
        );
        state.reservations.remove(attach_id);
        if let Some(pending) = state.pending.remove(attach_id) {
            cleanup.pending.push(pending.token);
        }
        if let Some(key) = state.activating.iter().find_map(|(key, activating)| {
            (activating.operation_id == attach_id).then_some(key.clone())
        }) {
            if let Some(activating) = state.activating.remove(&key) {
                cleanup.activating.push(activating);
            }
        }
        if let Some(key) = state
            .active
            .iter()
            .find_map(|(key, active)| (active.stream_id == attach_id).then_some(key.clone()))
        {
            if let Some(active) = state.active.remove(&key) {
                cleanup.active.push(active);
            }
        }
        Ok(cleanup)
    }

    fn fence_live_stream(
        &self,
        owner: &AgentLiveLeaseOwner,
        live_stream_id: &str,
    ) -> Result<CleanupBatch, AgentLiveAttachError> {
        let mut state = self.lock_state()?;
        let lineage_key = owner.stream_lineage_key();
        if state
            .active
            .values()
            .any(|active| active.stream_id == live_stream_id && &active.owner != owner)
            || state.activating.values().any(|activating| {
                activating.operation_id == live_stream_id && &activating.owner != owner
            })
        {
            return Err(AgentLiveAttachError::StaleLease);
        }
        if state
            .activating
            .get(&lineage_key)
            .is_some_and(|activating| activating.operation_id != live_stream_id)
            || active_for_lineage(&state, &lineage_key)?
                .is_some_and(|active| active.stream_id != live_stream_id)
        {
            return Err(AgentLiveAttachError::StaleLease);
        }
        let mut cleanup = CleanupBatch::default();
        cleanup.pending.extend(
            take_expired_pending(&mut state)
                .into_iter()
                .map(|pending| pending.token),
        );
        if state
            .activating
            .get(&lineage_key)
            .is_some_and(|activating| activating.operation_id == live_stream_id)
        {
            if let Some(activating) = state.activating.remove(&lineage_key) {
                cleanup.activating.push(activating);
            }
        }
        if let Some(key) = state.active.iter().find_map(|(key, active)| {
            (active.stream_id == live_stream_id && active.owner.stream_lineage_key() == lineage_key)
                .then_some(key.clone())
        }) {
            if let Some(active) = state.active.remove(&key) {
                cleanup.active.push(active);
            }
        }
        Ok(cleanup)
    }

    fn lock_state(&self) -> Result<MutexGuard<'_, AgentLiveAttachState>, AgentLiveAttachError> {
        self.inner
            .state
            .lock()
            .map_err(|_| AgentLiveAttachError::Unavailable)
    }
}

fn reserve_activation_locked(
    state: &mut AgentLiveAttachState,
    owner: &AgentLiveLeaseOwner,
    operation_id: &str,
) -> Result<ActivationReservation, AgentLiveAttachError> {
    let lineage_key = owner.stream_lineage_key();
    let owner_key = owner.stream_key();
    if state.activating.contains_key(&lineage_key) {
        return Err(AgentLiveAttachError::StaleLease);
    }
    if state
        .active
        .get(&owner_key)
        .is_some_and(|active| active.owner != *owner)
    {
        return Err(AgentLiveAttachError::StaleLease);
    }
    let superseded = active_identity_for_lineage(state, &lineage_key)?;
    let send_fence = Arc::new(Mutex::new(false));
    let (cancel_send, cancel) = oneshot::channel();
    let (done, done_receive) = oneshot::channel();
    state.activating.insert(
        lineage_key.clone(),
        ActivatingStream {
            operation_id: operation_id.to_string(),
            owner: owner.clone(),
            owner_key: owner_key.clone(),
            send_fence: Arc::clone(&send_fence),
            cancel: Some(cancel_send),
            done: Some(done_receive),
        },
    );
    Ok(ActivationReservation {
        lineage_key,
        owner_key,
        operation_id: operation_id.to_string(),
        owner: owner.clone(),
        superseded,
        send_fence,
        cancel,
        done: Some(done),
    })
}

fn active_identity_for_lineage(
    state: &AgentLiveAttachState,
    lineage_key: &AccountTargetLineageKey,
) -> Result<Option<ActiveStreamIdentity>, AgentLiveAttachError> {
    let mut matches = state
        .active
        .iter()
        .filter(|(_, active)| active.owner.stream_lineage_key() == *lineage_key);
    let Some((key, active)) = matches.next() else {
        return Ok(None);
    };
    if matches.next().is_some() {
        return Err(AgentLiveAttachError::Unavailable);
    }
    Ok(Some(ActiveStreamIdentity {
        key: key.clone(),
        stream_id: active.stream_id.clone(),
        owner: active.owner.clone(),
    }))
}

fn active_for_lineage<'a>(
    state: &'a AgentLiveAttachState,
    lineage_key: &AccountTargetLineageKey,
) -> Result<Option<&'a ActiveStream>, AgentLiveAttachError> {
    let mut matches = state
        .active
        .values()
        .filter(|active| active.owner.stream_lineage_key() == *lineage_key);
    let active = matches.next();
    if matches.next().is_some() {
        return Err(AgentLiveAttachError::Unavailable);
    }
    Ok(active)
}

fn state_id_in_use(state: &AgentLiveAttachState, id: &str) -> bool {
    state.reservations.contains_key(id)
        || state.pending.contains_key(id)
        || state
            .activating
            .values()
            .any(|activating| activating.operation_id == id)
        || state.active.values().any(|active| active.stream_id == id)
}

fn take_expired_pending(state: &mut AgentLiveAttachState) -> Vec<PendingAttach> {
    let now = Instant::now();
    let expired = state
        .pending
        .iter()
        .filter_map(|(attach_id, pending)| (pending.expires_at <= now).then_some(attach_id.clone()))
        .collect::<Vec<_>>();
    expired
        .into_iter()
        .filter_map(|attach_id| state.pending.remove(&attach_id))
        .collect()
}

fn spawn_pending_cancels(pending: Vec<PendingAttach>) {
    if pending.is_empty() {
        return;
    }
    spawn_cleanup(CleanupBatch {
        pending: pending.into_iter().map(|pending| pending.token).collect(),
        ..CleanupBatch::default()
    });
}

fn spawn_pending_cancel(token: Box<dyn AgentLivePendingAttach>) {
    spawn_cleanup(CleanupBatch {
        pending: vec![token],
        ..CleanupBatch::default()
    });
}

fn spawn_stream_unsubscribe(stream: Box<dyn AgentLiveProviderStream>) {
    if let Ok(runtime) = tokio::runtime::Handle::try_current() {
        runtime.spawn(async move {
            let _ = stream.unsubscribe().await;
        });
    }
}

fn close_send_fence(send_fence: &Arc<Mutex<bool>>) {
    if let Ok(mut closed) = send_fence.lock() {
        *closed = true;
    }
}

fn spawn_cleanup(mut cleanup: CleanupBatch) {
    if cleanup.is_empty() {
        return;
    }
    match tokio::runtime::Handle::try_current() {
        Ok(runtime) => {
            runtime.spawn(cleanup.run());
        }
        Err(_) => {
            for activating in &cleanup.activating {
                close_send_fence(&activating.send_fence);
            }
            for active in &cleanup.active {
                close_send_fence(&active.send_fence);
            }
            for activating in &mut cleanup.activating {
                if let Some(cancel) = activating.cancel.take() {
                    let _ = cancel.send(());
                }
            }
            for active in &mut cleanup.active {
                if let Some(cancel) = active.cancel.take() {
                    let _ = cancel.send(());
                }
            }
        }
    }
}

fn activation_cancelled(cancel: &mut oneshot::Receiver<()>) -> bool {
    match cancel.try_recv() {
        Ok(()) | Err(oneshot::error::TryRecvError::Closed) => true,
        Err(oneshot::error::TryRecvError::Empty) => false,
    }
}

async fn run_activation(
    inner: Arc<AgentLiveAttachManagerInner>,
    mut reservation: ActivationReservation,
    source: ActivationSource,
    mut response: oneshot::Sender<Result<AgentLiveBarrierResponse, AgentLiveAttachError>>,
) {
    let mut done = ActivationDoneGuard::new(&inner, &mut reservation);
    let owner = reservation.owner.clone();
    let operation_id = reservation.operation_id.clone();
    let result = execute_activation(&inner, &mut reservation, source, &mut response).await;
    let installed = result.is_ok();
    if installed {
        done.commit_active();
    }
    if response.send(result).is_err() && installed {
        let cleanup = fence_installed_stream(&inner, &owner, &operation_id);
        cleanup.run().await;
    }
}

async fn execute_activation(
    inner: &Arc<AgentLiveAttachManagerInner>,
    reservation: &mut ActivationReservation,
    source: ActivationSource,
    response: &mut oneshot::Sender<Result<AgentLiveBarrierResponse, AgentLiveAttachError>>,
) -> Result<AgentLiveBarrierResponse, AgentLiveAttachError> {
    let (from, sender, resume) = match source {
        ActivationSource::Pending(pending) => {
            if activation_cancelled(&mut reservation.cancel) || response.is_closed() {
                let _ = pending.token.cancel().await;
                return Err(AgentLiveAttachError::AttachNotFound);
            }
            let from = pending.through_cursor;
            let sender = pending.sender;
            let resume = pending.token.finalize().await?;
            if response.is_closed() || activation_cancelled(&mut reservation.cancel) {
                let _ = resume.stream.unsubscribe().await;
                return Err(AgentLiveAttachError::AttachNotFound);
            }
            (from, sender, resume)
        }
        ActivationSource::Resume { from, sender } => {
            if activation_cancelled(&mut reservation.cancel) || response.is_closed() {
                return Err(AgentLiveAttachError::AttachNotFound);
            }
            // Do not cancel this future after it may have registered a
            // subscriber. A concurrent fence is observed immediately after
            // the actor returns the exact stream, which is then unsubscribed.
            let resume = inner
                .provider
                .begin_resume(
                    &reservation.owner,
                    from.clone(),
                    inner.config.subscription_capacity,
                )
                .await?;
            if response.is_closed() || activation_cancelled(&mut reservation.cancel) {
                let _ = resume.stream.unsubscribe().await;
                return Err(AgentLiveAttachError::AttachNotFound);
            }
            (from, sender, resume)
        }
    };

    let through = resume.through_cursor.clone();
    let mut stream = ProviderStreamGuard::new(resume.stream);
    if activation_cancelled(&mut reservation.cancel) || response.is_closed() {
        stream.unsubscribe().await;
        return Err(AgentLiveAttachError::AttachNotFound);
    }
    if let Err(error) = validate_cursor_range(&from, &through) {
        stream.unsubscribe().await;
        return Err(error);
    }
    if let Err(error) = queue_activation_replay(
        inner,
        reservation,
        response,
        &sender,
        &from,
        &through,
        &mut stream,
    )
    .await
    {
        stream.unsubscribe().await;
        return Err(error);
    }
    if let Err(error) = validate_activation_lease(inner, reservation, response).await {
        stream.unsubscribe().await;
        return Err(error);
    }

    let previous = match fence_superseded_active(inner, reservation) {
        Ok(previous) => previous,
        Err(error) => {
            stream.unsubscribe().await;
            return Err(error);
        }
    };
    previous.run().await;
    if activation_cancelled(&mut reservation.cancel) || response.is_closed() {
        stream.unsubscribe().await;
        return Err(AgentLiveAttachError::AttachNotFound);
    }
    if let Err(error) = validate_activation_lease(inner, reservation, response).await {
        stream.unsubscribe().await;
        return Err(error);
    }

    if response.is_closed() {
        stream.unsubscribe().await;
        return Err(AgentLiveAttachError::AttachNotFound);
    }

    let provider_stream = stream.take()?;
    if let Err(error) =
        install_reserved_active(inner, reservation, sender, through.clone(), provider_stream).await
    {
        return Err(error);
    }
    Ok(AgentLiveBarrierResponse {
        through_event_cursor: cursor_to_wire(&through),
        live_stream_id: reservation.operation_id.clone(),
    })
}

async fn validate_activation_lease(
    inner: &AgentLiveAttachManagerInner,
    reservation: &mut ActivationReservation,
    response: &mut oneshot::Sender<Result<AgentLiveBarrierResponse, AgentLiveAttachError>>,
) -> Result<(), AgentLiveAttachError> {
    tokio::select! {
        biased;
        _ = &mut reservation.cancel => Err(AgentLiveAttachError::AttachNotFound),
        _ = response.closed() => Err(AgentLiveAttachError::AttachNotFound),
        result = inner.provider.validate_lease(&reservation.owner) => result,
    }
}

async fn queue_activation_replay(
    inner: &AgentLiveAttachManagerInner,
    reservation: &mut ActivationReservation,
    response: &mut oneshot::Sender<Result<AgentLiveBarrierResponse, AgentLiveAttachError>>,
    sender: &Arc<dyn AgentLiveEventSender>,
    from: &LiveEventCursor,
    through: &LiveEventCursor,
    stream: &mut ProviderStreamGuard,
) -> Result<(), AgentLiveAttachError> {
    let mut last = from.clone();
    while last.sequence() < through.sequence() {
        validate_activation_lease(inner, reservation, response).await?;
        let delivery = tokio::select! {
            biased;
            _ = &mut reservation.cancel => return Err(AgentLiveAttachError::AttachNotFound),
            _ = response.closed() => return Err(AgentLiveAttachError::AttachNotFound),
            delivery = stream.recv() => delivery.map_err(map_receive_error)?,
        };
        validate_next_delivery(&last, &delivery.cursor, through)?;
        validate_activation_lease(inner, reservation, response).await?;
        let frame = project_ordered_event(&*inner.projector, &reservation.owner, &delivery)?;
        if response.is_closed() {
            return Err(AgentLiveAttachError::AttachNotFound);
        }
        send_activating_if_current(
            inner,
            reservation,
            sender,
            AgentLiveChannelFrame::Event(frame),
        )?;
        last = delivery.cursor;
    }
    if last != *through {
        return Err(snapshot_required(AgentLiveSnapshotReason::OrderingLost));
    }
    Ok(())
}

fn fence_superseded_active(
    inner: &AgentLiveAttachManagerInner,
    reservation: &ActivationReservation,
) -> Result<CleanupBatch, AgentLiveAttachError> {
    reservation.owner.with_current_authority(|| {
        let mut state = inner
            .state
            .lock()
            .map_err(|_| AgentLiveAttachError::Unavailable)?;
        ensure_activation_current(&state, reservation)?;
        let current = active_identity_for_lineage(&state, &reservation.lineage_key)?;
        match (&reservation.superseded, current) {
            (None, None) => Ok(CleanupBatch::default()),
            (Some(_), None) => Ok(CleanupBatch::default()),
            (None, Some(_)) => Err(AgentLiveAttachError::StaleLease),
            (Some(expected), Some(current))
                if expected.key == current.key
                    && expected.stream_id == current.stream_id
                    && expected.owner == current.owner =>
            {
                let active = state
                    .active
                    .remove(&current.key)
                    .ok_or(AgentLiveAttachError::StaleLease)?;
                Ok(CleanupBatch {
                    active: vec![active],
                    ..CleanupBatch::default()
                })
            }
            (Some(_), Some(_)) => Err(AgentLiveAttachError::StaleLease),
        }
    })?
}

async fn install_reserved_active(
    inner: &Arc<AgentLiveAttachManagerInner>,
    reservation: &ActivationReservation,
    sender: Arc<dyn AgentLiveEventSender>,
    through: LiveEventCursor,
    stream: Box<dyn AgentLiveProviderStream>,
) -> Result<(), AgentLiveAttachError> {
    let key = reservation.owner_key.clone();
    let lineage_key = reservation.lineage_key.clone();
    let stream_id = reservation.operation_id.clone();
    let owner = reservation.owner.clone();
    let weak = Arc::downgrade(inner);
    let task_key = key.clone();
    let task_stream_id = stream_id.clone();
    let task_owner = owner.clone();
    let task_send_fence = Arc::clone(&reservation.send_fence);
    let (start, started) = oneshot::channel();
    let (cancel, cancelled) = oneshot::channel();
    let task = tokio::spawn(async move {
        let mut stream = ProviderStreamGuard::new(stream);
        if started.await.is_err() {
            stream.unsubscribe().await;
            return;
        }
        run_active_stream(
            weak.clone(),
            &task_owner,
            &task_key,
            &task_stream_id,
            &task_send_fence,
            sender,
            through,
            &mut stream,
            cancelled,
        )
        .await;
        stream.unsubscribe().await;
        remove_active_if_current(&weak, &task_key, &task_stream_id);
    });
    let mut active = Some(ActiveStream {
        stream_id: stream_id.clone(),
        owner: owner.clone(),
        send_fence: Arc::clone(&reservation.send_fence),
        cancel: Some(cancel),
        task,
    });
    let commit = owner.with_current_authority(|| {
        let mut state = inner
            .state
            .lock()
            .map_err(|_| AgentLiveAttachError::Unavailable)?;
        ensure_activation_current(&state, reservation)?;
        if active_for_lineage(&state, &lineage_key)?.is_some() {
            return Err(AgentLiveAttachError::StaleLease);
        }
        state.activating.remove(&lineage_key);
        state
            .active
            .insert(key, active.take().ok_or(AgentLiveAttachError::Unavailable)?);
        Ok(())
    });
    let commit = match commit {
        Ok(result) => result,
        Err(error) => Err(error),
    };
    if let Err(error) = commit {
        drop(start);
        if let Some(mut active) = active {
            if let Some(cancel) = active.cancel.take() {
                let _ = cancel.send(());
            }
            let _ = active.task.await;
        }
        return Err(error);
    }
    if start.send(()).is_err() {
        let cleanup = fence_installed_stream(inner, &owner, &stream_id);
        cleanup.run().await;
        return Err(AgentLiveAttachError::Unavailable);
    }
    Ok(())
}

fn ensure_activation_current(
    state: &AgentLiveAttachState,
    reservation: &ActivationReservation,
) -> Result<(), AgentLiveAttachError> {
    match state.activating.get(&reservation.lineage_key) {
        Some(activating)
            if activating.operation_id == reservation.operation_id
                && activating.owner == reservation.owner
                && activating.owner_key == reservation.owner_key
                && Arc::ptr_eq(&activating.send_fence, &reservation.send_fence) =>
        {
            Ok(())
        }
        _ => Err(AgentLiveAttachError::StaleLease),
    }
}

fn send_activating_if_current(
    inner: &AgentLiveAttachManagerInner,
    reservation: &ActivationReservation,
    sender: &Arc<dyn AgentLiveEventSender>,
    frame: AgentLiveChannelFrame,
) -> Result<(), AgentLiveAttachError> {
    reservation.owner.with_current_authority(|| {
        let fence = reservation
            .send_fence
            .lock()
            .map_err(|_| AgentLiveAttachError::Unavailable)?;
        if *fence {
            return Err(AgentLiveAttachError::StaleLease);
        }
        {
            let state = inner
                .state
                .lock()
                .map_err(|_| AgentLiveAttachError::Unavailable)?;
            ensure_activation_current(&state, reservation)?;
            if !state
                .activating
                .get(&reservation.lineage_key)
                .is_some_and(|activating| {
                    Arc::ptr_eq(&activating.send_fence, &reservation.send_fence)
                })
            {
                return Err(AgentLiveAttachError::StaleLease);
            }
        }
        sender.send(frame)
    })?
}

fn project_ordered_event(
    projector: &dyn AgentLiveDeliveryProjector,
    owner: &AgentLiveLeaseOwner,
    delivery: &AgentLiveDelivery,
) -> Result<AgentOrderedLiveEvent, AgentLiveAttachError> {
    let event = projector.project_delivery(delivery)?;
    Ok(AgentOrderedLiveEvent {
        live_event_version: AGENT_LIVE_PRESENTATION_VERSION,
        target_id: owner.target_id.clone(),
        host_epoch: owner.connection_stamp.host_epoch().to_string(),
        connection_generation: owner.connection_stamp.generation(),
        event_epoch: delivery.cursor.journal_id().to_string(),
        event_sequence: delivery.cursor.sequence(),
        session_id: delivery.session_id.clone(),
        run_id: delivery.run_id.clone(),
        event,
    })
}

async fn run_active_stream(
    weak: Weak<AgentLiveAttachManagerInner>,
    owner: &AgentLiveLeaseOwner,
    key: &AccountTargetKey,
    stream_id: &str,
    send_fence: &Arc<Mutex<bool>>,
    sender: Arc<dyn AgentLiveEventSender>,
    mut last: LiveEventCursor,
    stream: &mut ProviderStreamGuard,
    mut cancelled: oneshot::Receiver<()>,
) {
    loop {
        let Some(inner) = weak.upgrade() else {
            return;
        };
        let validation = tokio::select! {
            biased;
            _ = &mut cancelled => return,
            result = inner.provider.validate_lease(owner) => result,
        };
        if validation.is_err() {
            send_snapshot_notice_if_current(
                &inner,
                key,
                stream_id,
                send_fence,
                &sender,
                owner,
                AgentLiveSnapshotReason::OwnerChanged,
                &last,
            );
            return;
        }
        let delivery = tokio::select! {
            biased;
            _ = &mut cancelled => return,
            delivery = stream.recv() => delivery,
        };
        let delivery = match delivery {
            Ok(delivery) => delivery,
            Err(AgentLiveReceiveError::HeadReloadRequired(reason)) => {
                send_snapshot_notice_if_current(
                    &inner,
                    key,
                    stream_id,
                    send_fence,
                    &sender,
                    owner,
                    map_head_reload_reason(reason),
                    &last,
                );
                return;
            }
            Err(AgentLiveReceiveError::Closed) => {
                send_snapshot_notice_if_current(
                    &inner,
                    key,
                    stream_id,
                    send_fence,
                    &sender,
                    owner,
                    AgentLiveSnapshotReason::OrderingLost,
                    &last,
                );
                return;
            }
        };
        let validation = tokio::select! {
            biased;
            _ = &mut cancelled => return,
            result = inner.provider.validate_lease(owner) => result,
        };
        if validation.is_err() {
            send_snapshot_notice_if_current(
                &inner,
                key,
                stream_id,
                send_fence,
                &sender,
                owner,
                AgentLiveSnapshotReason::OwnerChanged,
                &last,
            );
            return;
        }
        if validate_next_cursor(&last, &delivery.cursor).is_err() {
            send_snapshot_notice_if_current(
                &inner,
                key,
                stream_id,
                send_fence,
                &sender,
                owner,
                AgentLiveSnapshotReason::OrderingLost,
                &last,
            );
            return;
        }
        let ordered = match project_ordered_event(&*inner.projector, owner, &delivery) {
            Ok(ordered) => ordered,
            Err(_) => {
                send_snapshot_notice_if_current(
                    &inner,
                    key,
                    stream_id,
                    send_fence,
                    &sender,
                    owner,
                    AgentLiveSnapshotReason::OrderingLost,
                    &last,
                );
                return;
            }
        };
        if send_active_if_current(
            &inner,
            key,
            stream_id,
            send_fence,
            sender.as_ref(),
            owner,
            AgentLiveChannelFrame::Event(ordered),
        )
        .is_err()
        {
            return;
        }
        last = delivery.cursor;
    }
}

fn send_snapshot_notice_if_current(
    inner: &AgentLiveAttachManagerInner,
    key: &AccountTargetKey,
    stream_id: &str,
    send_fence: &Arc<Mutex<bool>>,
    sender: &Arc<dyn AgentLiveEventSender>,
    owner: &AgentLiveLeaseOwner,
    reason: AgentLiveSnapshotReason,
    last: &LiveEventCursor,
) {
    let _ = send_active_if_current(
        inner,
        key,
        stream_id,
        send_fence,
        sender.as_ref(),
        owner,
        AgentLiveChannelFrame::SnapshotRequired(AgentLiveSnapshotRequiredFrame {
            live_event_version: AGENT_LIVE_PRESENTATION_VERSION,
            event_type: "snapshotRequired",
            target_id: owner.target_id.clone(),
            host_epoch: owner.connection_stamp.host_epoch().to_string(),
            connection_generation: owner.connection_stamp.generation(),
            reason,
            last_event_cursor: cursor_to_wire(last),
        }),
    );
}

fn send_active_if_current(
    inner: &AgentLiveAttachManagerInner,
    key: &AccountTargetKey,
    stream_id: &str,
    send_fence: &Arc<Mutex<bool>>,
    sender: &dyn AgentLiveEventSender,
    owner: &AgentLiveLeaseOwner,
    frame: AgentLiveChannelFrame,
) -> Result<(), AgentLiveAttachError> {
    owner.with_current_authority(|| {
        let fence = send_fence
            .lock()
            .map_err(|_| AgentLiveAttachError::Unavailable)?;
        if *fence {
            return Err(AgentLiveAttachError::StaleLease);
        }
        {
            let state = inner
                .state
                .lock()
                .map_err(|_| AgentLiveAttachError::Unavailable)?;
            if !state.active.get(key).is_some_and(|active| {
                active.stream_id == stream_id
                    && active.owner == *owner
                    && Arc::ptr_eq(&active.send_fence, send_fence)
            }) {
                return Err(AgentLiveAttachError::StaleLease);
            }
        }
        sender.send(frame)
    })?
}

fn remove_active_if_current(
    weak: &Weak<AgentLiveAttachManagerInner>,
    key: &AccountTargetKey,
    stream_id: &str,
) {
    let Some(inner) = weak.upgrade() else {
        return;
    };
    let Ok(mut state) = inner.state.lock() else {
        return;
    };
    if state
        .active
        .get(key)
        .is_some_and(|active| active.stream_id == stream_id)
    {
        if let Some(active) = state.active.remove(key) {
            drop(state);
            close_send_fence(&active.send_fence);
        }
    }
}

fn fence_installed_stream(
    inner: &AgentLiveAttachManagerInner,
    owner: &AgentLiveLeaseOwner,
    stream_id: &str,
) -> CleanupBatch {
    let Ok(mut state) = inner.state.lock() else {
        return CleanupBatch::default();
    };
    let key = state.active.iter().find_map(|(key, active)| {
        (active.stream_id == stream_id && &active.owner == owner).then_some(key.clone())
    });
    let mut cleanup = CleanupBatch::default();
    if let Some(key) = key {
        if let Some(active) = state.active.remove(&key) {
            cleanup.active.push(active);
        }
    }
    cleanup
}

fn project_safe_history_page(
    page: AgentHistoryPage,
    requested_limit: usize,
) -> Result<AgentSafeHistoryPage, AgentLiveAttachError> {
    if page.live_items.is_some() || page.through_event_cursor.is_some() {
        return Err(AgentLiveAttachError::InvalidRequest {
            message: "The persisted Agent history pager returned live attachment fields",
        });
    }
    if page.records.len() > requested_limit || page.records.len() > MAX_HISTORY_RECORDS_PER_PAGE {
        return Err(AgentLiveAttachError::ProjectionRejected);
    }
    if !is_safe_history_token(&page.history_revision, 512) {
        return Err(AgentLiveAttachError::ProjectionRejected);
    }
    if let Some(next_cursor) = page.next_cursor.as_deref() {
        if !is_safe_history_token(next_cursor, 512) {
            return Err(AgentLiveAttachError::ProjectionRejected);
        }
    }
    let mut record_ids = std::collections::HashSet::with_capacity(page.records.len());
    let records = page
        .records
        .into_iter()
        .map(|record| {
            if !is_safe_history_token(&record.record_id, 512) {
                return Err(AgentLiveAttachError::ProjectionRejected);
            }
            if record.role.is_empty()
                || record.role.len() > 128
                || !record
                    .role
                    .bytes()
                    .all(|byte| byte.is_ascii_graphic() || byte == b' ')
            {
                return Err(AgentLiveAttachError::ProjectionRejected);
            }
            if record.created_ms > MAX_JAVASCRIPT_SAFE_INTEGER
                || !record_ids.insert(record.record_id.clone())
                || record.items.len() > MAX_HISTORY_ITEMS_PER_RECORD
            {
                return Err(AgentLiveAttachError::ProjectionRejected);
            }
            let items = record
                .items
                .iter()
                .map(project_timeline_item)
                .collect::<Result<Vec<_>, _>>()
                .map_err(|_| AgentLiveAttachError::ProjectionRejected)?;
            for item in &items {
                item.validate()
                    .map_err(|_| AgentLiveAttachError::ProjectionRejected)?;
            }
            let safe_record = AgentSafeHistoryRecord {
                record_id: record.record_id,
                role: record.role,
                created_ms: record.created_ms,
                items,
            };
            let mut encoded = SerializedHistoryByteCounter::default();
            ciborium::ser::into_writer(&safe_record, &mut encoded)
                .map_err(|_| AgentLiveAttachError::ProjectionRejected)?;
            if encoded.bytes > MAX_HISTORY_RECORD_PRESENTATION_BYTES {
                return Err(AgentLiveAttachError::HistoryRecordTooLarge);
            }
            Ok(safe_record)
        })
        .collect::<Result<Vec<_>, AgentLiveAttachError>>()?;
    Ok(AgentSafeHistoryPage {
        records,
        next_cursor: page.next_cursor,
        history_revision: page.history_revision,
    })
}

fn is_safe_history_token(value: &str, max_bytes: usize) -> bool {
    !value.is_empty()
        && value.len() <= max_bytes
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.' | b':'))
}

#[derive(Default)]
struct SerializedHistoryByteCounter {
    bytes: usize,
}

impl Write for SerializedHistoryByteCounter {
    fn write(&mut self, buffer: &[u8]) -> std::io::Result<usize> {
        self.bytes = self
            .bytes
            .checked_add(buffer.len())
            .ok_or_else(|| std::io::Error::other("serialized safe history length overflow"))?;
        Ok(buffer.len())
    }

    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

fn validate_and_restore_snapshot(
    live_sessions_complete: bool,
    sessions: Vec<AgentLiveSessionProjection>,
) -> Result<Vec<AgentLiveSessionSnapshot>, AgentLiveAttachError> {
    if !live_sessions_complete {
        return Err(snapshot_required(AgentLiveSnapshotReason::OrderingLost));
    }
    if sessions.len() > MAX_LIVE_SESSIONS {
        return Err(snapshot_required(AgentLiveSnapshotReason::OrderingLost));
    }
    let mut previous: Option<&str> = None;
    let mut item_count = 0usize;
    let mut restored = Vec::with_capacity(sessions.len());
    for session in &sessions {
        validate_bounded_id(&session.session_id, 128, "Agent task ID is invalid")?;
        if previous.is_some_and(|value| value >= session.session_id.as_str()) {
            return Err(snapshot_required(AgentLiveSnapshotReason::OrderingLost));
        }
        previous = Some(&session.session_id);
        item_count = item_count
            .checked_add(session.live_items.len())
            .ok_or_else(|| snapshot_required(AgentLiveSnapshotReason::OrderingLost))?;
        if item_count > MAX_LIVE_ITEMS {
            return Err(snapshot_required(AgentLiveSnapshotReason::OrderingLost));
        }
        if session.live_items.len() > 200 {
            return Err(snapshot_required(AgentLiveSnapshotReason::OrderingLost));
        }
        let mut previous_item_id = std::collections::HashSet::new();
        let mut live_items = Vec::with_capacity(session.live_items.len());
        for item in &session.live_items {
            item.validate()
                .map_err(|_| snapshot_required(AgentLiveSnapshotReason::OrderingLost))?;
            if item.merge != crate::agent_live_coordinator::MapleLiveMerge::Replace
                || !previous_item_id.insert(item.id.as_str())
            {
                return Err(snapshot_required(AgentLiveSnapshotReason::OrderingLost));
            }
            live_items.push(item.clone());
        }
        restored.push(AgentLiveSessionSnapshot {
            session_id: session.session_id.clone(),
            live_items,
        });
    }
    if restored.len() != sessions.len() {
        return Err(snapshot_required(AgentLiveSnapshotReason::OrderingLost));
    }
    Ok(restored)
}

fn validate_cursor_range(
    from: &LiveEventCursor,
    through: &LiveEventCursor,
) -> Result<(), AgentLiveAttachError> {
    if from.journal_id() != through.journal_id() || from.sequence() > through.sequence() {
        return Err(snapshot_required(
            if from.journal_id() != through.journal_id() {
                AgentLiveSnapshotReason::JournalReplaced
            } else {
                AgentLiveSnapshotReason::CursorAhead
            },
        ));
    }
    Ok(())
}

fn validate_next_delivery(
    previous: &LiveEventCursor,
    next: &LiveEventCursor,
    through: &LiveEventCursor,
) -> Result<(), AgentLiveAttachError> {
    validate_next_cursor(previous, next)?;
    if next.journal_id() != through.journal_id() || next.sequence() > through.sequence() {
        return Err(snapshot_required(AgentLiveSnapshotReason::OrderingLost));
    }
    Ok(())
}

fn validate_next_cursor(
    previous: &LiveEventCursor,
    next: &LiveEventCursor,
) -> Result<(), AgentLiveAttachError> {
    let expected = previous
        .sequence()
        .checked_add(1)
        .ok_or_else(|| snapshot_required(AgentLiveSnapshotReason::OrderingLost))?;
    if previous.journal_id() != next.journal_id() || next.sequence() != expected {
        return Err(snapshot_required(AgentLiveSnapshotReason::OrderingLost));
    }
    Ok(())
}

fn cursor_to_wire(cursor: &LiveEventCursor) -> AgentLiveEventCursor {
    AgentLiveEventCursor {
        journal_id: cursor.journal_id().to_string(),
        sequence: cursor.sequence(),
    }
}

fn wire_to_cursor(cursor: &AgentLiveEventCursor) -> Result<LiveEventCursor, AgentLiveAttachError> {
    LiveEventCursor::try_from_parts(cursor.journal_id.clone(), cursor.sequence).map_err(|_| {
        AgentLiveAttachError::InvalidRequest {
            message: "Agent live-event cursor is invalid",
        }
    })
}

fn map_receive_error(error: AgentLiveReceiveError) -> AgentLiveAttachError {
    match error {
        AgentLiveReceiveError::HeadReloadRequired(reason) => {
            snapshot_required(map_head_reload_reason(reason))
        }
        AgentLiveReceiveError::Closed => snapshot_required(AgentLiveSnapshotReason::OrderingLost),
    }
}

pub(crate) fn map_coordinator_error(error: AgentLiveCoordinatorError) -> AgentLiveAttachError {
    match error {
        AgentLiveCoordinatorError::HeadReloadRequired(reason) => {
            snapshot_required(map_head_reload_reason(reason))
        }
        // Reseed is a native authoritative-head workflow. The renderer must
        // discard the old journal epoch and perform a synchronized head reload;
        // it never receives or attempts to satisfy the reseed capability.
        AgentLiveCoordinatorError::ReseedRequired(_) => {
            snapshot_required(AgentLiveSnapshotReason::JournalReplaced)
        }
        AgentLiveCoordinatorError::Sealed(_)
        | AgentLiveCoordinatorError::DataOwnerMismatch
        | AgentLiveCoordinatorError::StableOperationMismatch
        | AgentLiveCoordinatorError::ProjectionSchemaMismatch
        | AgentLiveCoordinatorError::IngressRebindRequired
        | AgentLiveCoordinatorError::Journal(LiveEventJournalError::OwnerGenerationMismatch)
        | AgentLiveCoordinatorError::Journal(LiveEventJournalError::OwnerTransitionIncomplete) => {
            AgentLiveAttachError::StaleLease
        }
        AgentLiveCoordinatorError::InvalidAccountScope
        | AgentLiveCoordinatorError::InvalidExecutionTarget
        | AgentLiveCoordinatorError::InvalidSession
        | AgentLiveCoordinatorError::InvalidRun
        | AgentLiveCoordinatorError::InvalidSubscriptionCapacity
        | AgentLiveCoordinatorError::InvalidCommandCapacity => {
            AgentLiveAttachError::InvalidRequest {
                message: "Agent live attachment request is invalid",
            }
        }
        AgentLiveCoordinatorError::SubscriberCapacityExceeded
        | AgentLiveCoordinatorError::IngressRouteCapacityExceeded => {
            AgentLiveAttachError::CapacityExceeded
        }
        AgentLiveCoordinatorError::Projection(_) => AgentLiveAttachError::ProjectionRejected,
        AgentLiveCoordinatorError::StaleHistoryCommit
        | AgentLiveCoordinatorError::IngressEpochExhausted
        | AgentLiveCoordinatorError::Journal(_)
        | AgentLiveCoordinatorError::WorkerUnavailable
        | AgentLiveCoordinatorError::CoordinatorClosed => AgentLiveAttachError::Unavailable,
    }
}

fn map_head_reload_reason(reason: HeadReloadReason) -> AgentLiveSnapshotReason {
    match reason {
        HeadReloadReason::PausedSubscriberOverflow => {
            AgentLiveSnapshotReason::PausedSubscriberOverflow
        }
        HeadReloadReason::SlowSubscriber => AgentLiveSnapshotReason::SlowSubscriber,
        HeadReloadReason::JournalReplaced => AgentLiveSnapshotReason::JournalReplaced,
        HeadReloadReason::ReseedRequired => AgentLiveSnapshotReason::JournalReplaced,
        HeadReloadReason::RetentionGap => AgentLiveSnapshotReason::RetentionGap,
        HeadReloadReason::CursorAhead => AgentLiveSnapshotReason::CursorAhead,
        HeadReloadReason::OwnerChanged => AgentLiveSnapshotReason::OwnerChanged,
        HeadReloadReason::OrderingLost => AgentLiveSnapshotReason::OrderingLost,
        HeadReloadReason::JournalUnavailable => AgentLiveSnapshotReason::JournalUnavailable,
    }
}

fn snapshot_required(reason: AgentLiveSnapshotReason) -> AgentLiveAttachError {
    AgentLiveAttachError::SnapshotRequired { reason }
}

fn allocate_attach_id(state: &AgentLiveAttachState) -> Result<String, AgentLiveAttachError> {
    for _ in 0..MAX_ATTACH_ID_ATTEMPTS {
        let candidate = random_opaque_id()?;
        if !state_id_in_use(state, &candidate) {
            return Ok(candidate);
        }
    }
    Err(AgentLiveAttachError::Unavailable)
}

fn random_opaque_id() -> Result<String, AgentLiveAttachError> {
    let mut bytes = [0_u8; ATTACH_ID_RANDOM_BYTES];
    fill_random(&mut bytes).map_err(|_| AgentLiveAttachError::Unavailable)?;
    let mut encoded = String::with_capacity(ATTACH_ID_RANDOM_BYTES * 2);
    const HEX: &[u8; 16] = b"0123456789abcdef";
    for byte in bytes {
        encoded.push(HEX[(byte >> 4) as usize] as char);
        encoded.push(HEX[(byte & 0x0f) as usize] as char);
    }
    Ok(encoded)
}

fn validate_bounded_id(
    value: &str,
    max_bytes: usize,
    message: &'static str,
) -> Result<(), AgentLiveAttachError> {
    if value.trim().is_empty() || value.len() > max_bytes || value.chars().any(char::is_control) {
        return Err(AgentLiveAttachError::InvalidRequest { message });
    }
    Ok(())
}

fn parse_canonical_host_epoch(value: &str) -> Result<u64, AgentLiveAttachError> {
    if value.is_empty()
        || value.len() > 20
        || value.starts_with('0')
        || !value.bytes().all(|byte| byte.is_ascii_digit())
    {
        return Err(AgentLiveAttachError::InvalidRequest {
            message: "Agent host epoch is invalid",
        });
    }
    let parsed = value
        .parse::<u64>()
        .map_err(|_| AgentLiveAttachError::InvalidRequest {
            message: "Agent host epoch is invalid",
        })?;
    if parsed == 0 || parsed.to_string() != value {
        return Err(AgentLiveAttachError::InvalidRequest {
            message: "Agent host epoch is invalid",
        });
    }
    Ok(parsed)
}

fn validate_attach_or_stream_id(
    value: &str,
    message: &'static str,
) -> Result<(), AgentLiveAttachError> {
    if value.len() != ATTACH_ID_RANDOM_BYTES * 2
        || !value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
    {
        return Err(AgentLiveAttachError::InvalidRequest { message });
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agent::{AgentHistoryRecord, AgentTimelineItem};
    use crate::agent_live_coordinator::{
        MapleLiveEvent, MapleLiveItemType, MapleLiveMerge, MapleLiveTimelineItem,
        MapleLiveUserFacingError,
    };
    use crate::remote_transport::PairingIncarnation;
    use serde_json::Value;
    use std::{
        collections::VecDeque,
        sync::atomic::{AtomicBool, AtomicUsize, Ordering},
    };
    use tokio::sync::{mpsc, Notify};

    fn endpoint(seed: u8) -> iroh::EndpointId {
        iroh::SecretKey::from_bytes(&[seed; 32]).public()
    }

    fn cursor(sequence: u64) -> LiveEventCursor {
        LiveEventCursor::try_from_parts("11".repeat(16), sequence).unwrap()
    }

    fn owner() -> AgentLiveLeaseOwner {
        AgentLiveLeaseOwner {
            opaque_account_scope: "account-hash".into(),
            account_data_generation: 7,
            target_id: "11111111-1111-4111-8111-111111111111".into(),
            authorization: LocalAuthorizationContext::for_test(17, 9, [7; 32]),
            controller_endpoint: endpoint(1),
            pairing_fence: PairingFence::new(PairingIncarnation::new(3).unwrap()).unwrap(),
            connection_stamp: ConnectionStamp::new(11, 12).unwrap(),
            binding_lineage_epoch: 5,
            peer_lineage_epoch: 6,
            native_authority: None,
        }
    }

    #[test]
    fn coordinator_error_mapping_fails_closed_for_ingress_authority_and_capacity() {
        for error in [
            AgentLiveCoordinatorError::DataOwnerMismatch,
            AgentLiveCoordinatorError::StableOperationMismatch,
            AgentLiveCoordinatorError::ProjectionSchemaMismatch,
            AgentLiveCoordinatorError::IngressRebindRequired,
        ] {
            assert_eq!(
                map_coordinator_error(error),
                AgentLiveAttachError::StaleLease
            );
        }
        assert_eq!(
            map_coordinator_error(AgentLiveCoordinatorError::IngressRouteCapacityExceeded),
            AgentLiveAttachError::CapacityExceeded
        );
        assert_eq!(
            map_coordinator_error(AgentLiveCoordinatorError::IngressEpochExhausted),
            AgentLiveAttachError::Unavailable
        );
    }

    fn request() -> AgentHistoryPageRequest {
        AgentHistoryPageRequest {
            session_id: "session-b".into(),
            cursor: None,
            limit: Some(25),
        }
    }

    fn history_page() -> AgentHistoryPage {
        AgentHistoryPage {
            records: vec![AgentHistoryRecord {
                record_id: "record-1".into(),
                role: "assistant".into(),
                created_ms: 1,
                items: vec![],
            }],
            next_cursor: Some("next".into()),
            history_revision: "history-revision".into(),
            live_items: None,
            through_event_cursor: None,
        }
    }

    fn live_item(id: &str) -> MapleLiveTimelineItem {
        MapleLiveTimelineItem {
            id: id.into(),
            item_type: MapleLiveItemType::Message,
            role: None,
            title: None,
            text: Some(id.into()),
            status: None,
            created_ms: 1,
            merge: MapleLiveMerge::Replace,
        }
    }

    fn projection(session_id: &str, item_ids: &[&str]) -> AgentLiveSessionProjection {
        AgentLiveSessionProjection {
            session_id: session_id.into(),
            live_items: item_ids.iter().map(|id| live_item(id)).collect(),
        }
    }

    fn delivery(sequence: u64, session_id: &str) -> AgentLiveDelivery {
        AgentLiveDelivery {
            cursor: cursor(sequence),
            session_id: session_id.into(),
            run_id: Some("run".into()),
            event: MapleLiveEvent::RunStarted {
                event_id: format!("event-{sequence}"),
            },
        }
    }

    #[derive(Default)]
    struct RecordingSender {
        frames: Mutex<Vec<AgentLiveChannelFrame>>,
        fail: AtomicBool,
        sent: Notify,
    }

    impl RecordingSender {
        fn event_sequences(&self) -> Vec<u64> {
            self.frames
                .lock()
                .unwrap()
                .iter()
                .filter_map(|frame| match frame {
                    AgentLiveChannelFrame::Event(event) => Some(event.event_sequence),
                    AgentLiveChannelFrame::SnapshotRequired(_) => None,
                })
                .collect()
        }

        async fn wait_for_event_count(&self, expected: usize) {
            while self.event_sequences().len() < expected {
                self.sent.notified().await;
            }
        }
    }

    impl AgentLiveEventSender for RecordingSender {
        fn send(&self, frame: AgentLiveChannelFrame) -> Result<(), AgentLiveAttachError> {
            if self.fail.load(Ordering::SeqCst) {
                return Err(AgentLiveAttachError::ChannelClosed);
            }
            self.frames.lock().unwrap().push(frame);
            self.sent.notify_one();
            Ok(())
        }
    }

    struct TestProjector;

    impl AgentLiveDeliveryProjector for TestProjector {
        fn project_delivery(
            &self,
            _delivery: &AgentLiveDelivery,
        ) -> Result<AgentPresentedLiveEvent, AgentLiveAttachError> {
            Ok(AgentPresentedLiveEvent::RunStarted)
        }
    }

    struct QueueStream {
        receiver: mpsc::UnboundedReceiver<Result<AgentLiveDelivery, AgentLiveReceiveError>>,
        unsubscribed: Arc<AtomicBool>,
    }

    #[async_trait]
    impl AgentLiveProviderStream for QueueStream {
        async fn recv(&mut self) -> Result<AgentLiveDelivery, AgentLiveReceiveError> {
            self.receiver
                .recv()
                .await
                .unwrap_or(Err(AgentLiveReceiveError::Closed))
        }

        async fn unsubscribe(self: Box<Self>) -> Result<(), AgentLiveAttachError> {
            self.unsubscribed.store(true, Ordering::SeqCst);
            Ok(())
        }
    }

    fn stream(
        deliveries: impl IntoIterator<Item = Result<AgentLiveDelivery, AgentLiveReceiveError>>,
    ) -> (
        Box<dyn AgentLiveProviderStream>,
        mpsc::UnboundedSender<Result<AgentLiveDelivery, AgentLiveReceiveError>>,
    ) {
        let (stream, sender, _) = tracked_stream(deliveries);
        (stream, sender)
    }

    fn tracked_stream(
        deliveries: impl IntoIterator<Item = Result<AgentLiveDelivery, AgentLiveReceiveError>>,
    ) -> (
        Box<dyn AgentLiveProviderStream>,
        mpsc::UnboundedSender<Result<AgentLiveDelivery, AgentLiveReceiveError>>,
        Arc<AtomicBool>,
    ) {
        let (sender, receiver) = mpsc::unbounded_channel();
        for delivery in deliveries {
            sender.send(delivery).unwrap();
        }
        let unsubscribed = Arc::new(AtomicBool::new(false));
        (
            Box::new(QueueStream {
                receiver,
                unsubscribed: Arc::clone(&unsubscribed),
            }),
            sender,
            unsubscribed,
        )
    }

    struct FakePendingToken {
        resume: Mutex<Option<AgentLiveProviderResume>>,
        finalized: Arc<AtomicBool>,
        cancelled: Arc<AtomicBool>,
    }

    struct BlockingPendingToken {
        resume: Mutex<Option<AgentLiveProviderResume>>,
        started: Arc<AtomicBool>,
        started_notify: Arc<Notify>,
        release: Arc<Notify>,
        cancelled: Arc<AtomicBool>,
    }

    #[async_trait]
    impl AgentLivePendingAttach for BlockingPendingToken {
        async fn finalize(
            self: Box<Self>,
        ) -> Result<AgentLiveProviderResume, AgentLiveAttachError> {
            self.started.store(true, Ordering::SeqCst);
            self.started_notify.notify_waiters();
            self.release.notified().await;
            self.resume
                .lock()
                .unwrap()
                .take()
                .ok_or(AgentLiveAttachError::Unavailable)
        }

        async fn cancel(self: Box<Self>) -> Result<(), AgentLiveAttachError> {
            self.cancelled.store(true, Ordering::SeqCst);
            Ok(())
        }
    }

    #[async_trait]
    impl AgentLivePendingAttach for FakePendingToken {
        async fn finalize(
            self: Box<Self>,
        ) -> Result<AgentLiveProviderResume, AgentLiveAttachError> {
            self.finalized.store(true, Ordering::SeqCst);
            self.resume
                .lock()
                .unwrap()
                .take()
                .ok_or(AgentLiveAttachError::Unavailable)
        }

        async fn cancel(self: Box<Self>) -> Result<(), AgentLiveAttachError> {
            self.cancelled.store(true, Ordering::SeqCst);
            Ok(())
        }
    }

    struct FakeProvider {
        valid: AtomicBool,
        validate_calls: AtomicUsize,
        resume_calls: AtomicUsize,
        page: Mutex<AgentHistoryPage>,
        heads: Mutex<VecDeque<AgentLiveProviderHeadAttach>>,
        resumes: Mutex<VecDeque<AgentLiveProviderResume>>,
        invalidate_after_finalize: Option<Arc<AtomicBool>>,
    }

    impl FakeProvider {
        fn new(heads: Vec<AgentLiveProviderHeadAttach>) -> Self {
            Self {
                valid: AtomicBool::new(true),
                validate_calls: AtomicUsize::new(0),
                resume_calls: AtomicUsize::new(0),
                page: Mutex::new(history_page()),
                heads: Mutex::new(heads.into()),
                resumes: Mutex::new(VecDeque::new()),
                invalidate_after_finalize: None,
            }
        }
    }

    #[async_trait]
    impl AgentLiveAttachProvider for FakeProvider {
        async fn validate_lease(
            &self,
            _owner: &AgentLiveLeaseOwner,
        ) -> Result<(), AgentLiveAttachError> {
            self.validate_calls.fetch_add(1, Ordering::SeqCst);
            if self
                .invalidate_after_finalize
                .as_ref()
                .is_some_and(|finalized| finalized.load(Ordering::SeqCst))
            {
                self.valid.store(false, Ordering::SeqCst);
            }
            self.valid
                .load(Ordering::SeqCst)
                .then_some(())
                .ok_or(AgentLiveAttachError::StaleLease)
        }

        async fn begin_account_head_attach(
            &self,
            _owner: &AgentLiveLeaseOwner,
            _capacity: usize,
        ) -> Result<AgentLiveProviderHeadAttach, AgentLiveAttachError> {
            self.heads
                .lock()
                .unwrap()
                .pop_front()
                .ok_or(AgentLiveAttachError::Unavailable)
        }

        async fn list_history_page(
            &self,
            _owner: &AgentLiveLeaseOwner,
            _request: AgentHistoryPageRequest,
        ) -> Result<AgentHistoryPage, AgentLiveAttachError> {
            Ok(self.page.lock().unwrap().clone())
        }

        async fn begin_resume(
            &self,
            _owner: &AgentLiveLeaseOwner,
            _cursor: LiveEventCursor,
            _capacity: usize,
        ) -> Result<AgentLiveProviderResume, AgentLiveAttachError> {
            self.resume_calls.fetch_add(1, Ordering::SeqCst);
            self.resumes
                .lock()
                .unwrap()
                .pop_front()
                .ok_or(AgentLiveAttachError::Unavailable)
        }
    }

    fn head(
        through: u64,
        live_sessions: Vec<AgentLiveSessionProjection>,
        replay: Vec<AgentLiveDelivery>,
        live_sender_out: Option<
            &mut Option<mpsc::UnboundedSender<Result<AgentLiveDelivery, AgentLiveReceiveError>>>,
        >,
        finalized: Arc<AtomicBool>,
    ) -> AgentLiveProviderHeadAttach {
        let (stream, sender) = stream(replay.into_iter().map(Ok));
        if let Some(output) = live_sender_out {
            *output = Some(sender);
        }
        AgentLiveProviderHeadAttach {
            through_cursor: cursor(through.saturating_sub(1)),
            live_sessions_complete: true,
            live_sessions,
            token: Box::new(FakePendingToken {
                resume: Mutex::new(Some(AgentLiveProviderResume {
                    through_cursor: cursor(through),
                    stream,
                })),
                finalized,
                cancelled: Arc::new(AtomicBool::new(false)),
            }),
        }
    }

    fn tracked_head(
        through: u64,
        replay: Vec<AgentLiveDelivery>,
    ) -> (
        AgentLiveProviderHeadAttach,
        mpsc::UnboundedSender<Result<AgentLiveDelivery, AgentLiveReceiveError>>,
        Arc<AtomicBool>,
        Arc<AtomicBool>,
    ) {
        let (stream, sender, unsubscribed) = tracked_stream(replay.into_iter().map(Ok));
        let cancelled = Arc::new(AtomicBool::new(false));
        (
            AgentLiveProviderHeadAttach {
                through_cursor: cursor(through.saturating_sub(1)),
                live_sessions_complete: true,
                live_sessions: vec![],
                token: Box::new(FakePendingToken {
                    resume: Mutex::new(Some(AgentLiveProviderResume {
                        through_cursor: cursor(through),
                        stream,
                    })),
                    finalized: Arc::new(AtomicBool::new(false)),
                    cancelled: Arc::clone(&cancelled),
                }),
            },
            sender,
            cancelled,
            unsubscribed,
        )
    }

    fn manager(provider: Arc<FakeProvider>) -> AgentLiveAttachManager {
        AgentLiveAttachManager::new(provider, Arc::new(TestProjector))
    }

    struct TestOwnerResolver;

    #[async_trait]
    impl AgentLiveOwnerResolver for TestOwnerResolver {
        async fn resolve_current_owner(
            &self,
            _user_id: &str,
        ) -> Result<AgentLiveLeaseOwner, AgentLiveAttachError> {
            Ok(owner())
        }
    }

    #[tokio::test]
    async fn begin_returns_literal_complete_account_snapshot_and_keeps_token_paused() {
        let finalized = Arc::new(AtomicBool::new(false));
        let provider = Arc::new(FakeProvider::new(vec![head(
            1,
            vec![
                projection("session-a", &["a"]),
                projection("session-b", &["b"]),
            ],
            vec![delivery(1, "session-a")],
            None,
            finalized.clone(),
        )]));
        let manager = manager(provider);
        let response = manager
            .begin(owner(), request(), Arc::new(RecordingSender::default()))
            .await
            .unwrap();
        assert!(!finalized.load(Ordering::SeqCst));
        assert!(manager
            .inner
            .state
            .lock()
            .unwrap()
            .pending
            .contains_key(&response.attach_id));
        assert!(response.live_sessions_complete);
        assert_eq!(response.live_session_count, 2);
        assert_eq!(
            response
                .live_sessions
                .iter()
                .map(|session| session.session_id.as_str())
                .collect::<Vec<_>>(),
            ["session-a", "session-b"]
        );
        assert_eq!(response.through_event_cursor.sequence, 0);
        let encoded = serde_json::to_value(&response).unwrap();
        assert_eq!(encoded["liveSessionsComplete"], Value::Bool(true));
        assert_eq!(encoded["liveSessionCount"], Value::from(2));
    }

    #[tokio::test]
    async fn begin_rejects_plain_page_that_smuggles_a_live_pair() {
        let finalized = Arc::new(AtomicBool::new(false));
        let provider = Arc::new(FakeProvider::new(vec![head(
            0,
            vec![],
            vec![],
            None,
            finalized,
        )]));
        provider.page.lock().unwrap().live_items = Some(vec![]);
        assert!(matches!(
            manager(provider)
                .begin(owner(), request(), Arc::new(RecordingSender::default()))
                .await,
            Err(AgentLiveAttachError::InvalidRequest { .. })
        ));
    }

    #[tokio::test]
    async fn synchronized_head_projects_persisted_tool_rows_through_closed_safe_boundary() {
        let finalized = Arc::new(AtomicBool::new(false));
        let provider = Arc::new(FakeProvider::new(vec![head(
            0,
            vec![],
            vec![],
            None,
            finalized,
        )]));
        provider.page.lock().unwrap().records[0].items = vec![AgentTimelineItem {
            id: "tool-record".into(),
            item_type: "tool".into(),
            role: Some("assistant".into()),
            title: Some("curl https://secret.invalid?token=hunter2".into()),
            text: Some("failed at /Users/alice/.env: API_KEY=hunter2".into()),
            status: Some("failed".into()),
            input: Some(serde_json::json!({"token": "hunter2"})),
            output: Some(serde_json::json!({"path": "/Users/alice/.env"})),
            created_ms: 1,
            merge: "replace".into(),
        }];

        let response = manager(provider)
            .begin(owner(), request(), Arc::new(RecordingSender::default()))
            .await
            .unwrap();
        assert_eq!(response.page.records.len(), 1);
        let item = &response.page.records[0].items[0];
        assert_eq!(item.title.as_deref(), Some(SAFE_REMOTE_TOOL_TITLE));
        assert_eq!(item.text.as_deref(), Some(SAFE_REMOTE_TOOL_FAILED));
        let encoded = serde_json::to_string(&response).unwrap();
        for forbidden in [
            "hunter2",
            "/Users/alice/.env",
            "secret.invalid",
            "\"input\"",
            "\"output\"",
        ] {
            assert!(!encoded.contains(forbidden), "leaked {forbidden}");
        }
    }

    #[test]
    fn synchronized_head_rejects_unsafe_timestamp_and_oversized_single_record() {
        let mut unsafe_timestamp = history_page();
        unsafe_timestamp.records[0].created_ms = MAX_JAVASCRIPT_SAFE_INTEGER + 1;
        assert_eq!(
            project_safe_history_page(unsafe_timestamp, 25),
            Err(AgentLiveAttachError::ProjectionRejected)
        );

        let mut oversized = history_page();
        oversized.records[0].items = (0..6)
            .map(|index| AgentTimelineItem {
                id: format!("message-{index}"),
                item_type: "message".into(),
                role: Some("assistant".into()),
                title: None,
                text: Some("x".repeat(192 * 1024)),
                status: None,
                input: None,
                output: None,
                created_ms: 1,
                merge: "replace".into(),
            })
            .collect();
        assert_eq!(
            project_safe_history_page(oversized, 25),
            Err(AgentLiveAttachError::HistoryRecordTooLarge)
        );
    }

    #[tokio::test]
    async fn activate_queues_strict_replay_before_return_and_continues_same_channel_live() {
        let finalized = Arc::new(AtomicBool::new(false));
        let mut live_sender = None;
        let provider = Arc::new(FakeProvider::new(vec![head(
            2,
            vec![projection("session-a", &["a"])],
            vec![delivery(2, "session-b")],
            Some(&mut live_sender),
            finalized.clone(),
        )]));
        let manager = manager(provider);
        let sender = Arc::new(RecordingSender::default());
        let begun = manager
            .begin(owner(), request(), sender.clone())
            .await
            .unwrap();
        let activated = manager.activate(owner(), &begun.attach_id).await.unwrap();
        assert!(finalized.load(Ordering::SeqCst));
        assert_eq!(activated.through_event_cursor.sequence, 2);
        assert_eq!(activated.live_stream_id, begun.attach_id);
        assert_eq!(sender.event_sequences(), [2]);
        live_sender
            .unwrap()
            .send(Ok(delivery(3, "session-a")))
            .unwrap();
        tokio::time::timeout(Duration::from_secs(1), sender.wait_for_event_count(2))
            .await
            .unwrap();
        assert_eq!(sender.event_sequences(), [2, 3]);
        let frames = sender.frames.lock().unwrap();
        let AgentLiveChannelFrame::Event(first) = &frames[0] else {
            panic!("expected ordered event")
        };
        assert_eq!(first.target_id, owner().target_id);
        assert_eq!(first.host_epoch, "11");
        assert_eq!(first.connection_generation, 12);
        assert_eq!(first.event_epoch, "11".repeat(16));
    }

    #[tokio::test]
    async fn active_stream_requires_exact_teardown_handle_and_cancel_is_idempotent() {
        let finalized = Arc::new(AtomicBool::new(false));
        let mut live_sender = None;
        let provider = Arc::new(FakeProvider::new(vec![head(
            0,
            vec![],
            vec![],
            Some(&mut live_sender),
            finalized,
        )]));
        let manager = manager(provider);
        let channel = Arc::new(RecordingSender::default());
        let begun = manager
            .begin(owner(), request(), channel.clone())
            .await
            .unwrap();
        let active = manager.activate(owner(), &begun.attach_id).await.unwrap();
        let different_stream = "22".repeat(ATTACH_ID_RANDOM_BYTES);
        assert_eq!(
            manager.cancel_live_events(owner(), &different_stream).await,
            Err(AgentLiveAttachError::StaleLease)
        );
        assert_eq!(manager.inner.state.lock().unwrap().active.len(), 1);
        manager
            .cancel_live_events(owner(), &active.live_stream_id)
            .await
            .unwrap();
        tokio::task::yield_now().await;
        manager
            .cancel_live_events(owner(), &active.live_stream_id)
            .await
            .unwrap();
        assert!(manager.inner.state.lock().unwrap().active.is_empty());
        let _ = live_sender.unwrap().send(Ok(delivery(1, "session")));
        tokio::task::yield_now().await;
        assert!(channel.event_sequences().is_empty());
    }

    #[tokio::test]
    async fn tauri_lifecycle_pending_cancel_and_ttl_await_token_cancellation() {
        let (cancel_head, _cancel_live, cancelled, _cancel_unsubscribed) = tracked_head(0, vec![]);
        let (expiry_head, _expiry_live, expired, _expiry_unsubscribed) = tracked_head(0, vec![]);
        let provider = Arc::new(FakeProvider::new(vec![cancel_head, expiry_head]));
        let manager = AgentLiveAttachManager::with_config(
            provider,
            Arc::new(TestProjector),
            AgentLiveAttachManagerConfig {
                pending_ttl: Duration::from_millis(20),
                ..Default::default()
            },
        )
        .unwrap();

        let pending = manager
            .begin(owner(), request(), Arc::new(RecordingSender::default()))
            .await
            .unwrap();
        manager.cancel(owner(), &pending.attach_id).await.unwrap();
        assert!(cancelled.load(Ordering::SeqCst));

        manager
            .begin(owner(), request(), Arc::new(RecordingSender::default()))
            .await
            .unwrap();
        tokio::time::timeout(Duration::from_secs(1), async {
            while !expired.load(Ordering::SeqCst) {
                tokio::task::yield_now().await;
            }
        })
        .await
        .unwrap();
    }

    #[tokio::test]
    async fn tauri_lifecycle_active_cancel_awaits_stream_unsubscribe() {
        let (head, _live, _cancelled, unsubscribed) = tracked_head(0, vec![]);
        let manager = manager(Arc::new(FakeProvider::new(vec![head])));
        let pending = manager
            .begin(owner(), request(), Arc::new(RecordingSender::default()))
            .await
            .unwrap();
        let active = manager.activate(owner(), &pending.attach_id).await.unwrap();
        manager
            .cancel_live_events(owner(), &active.live_stream_id)
            .await
            .unwrap();
        assert!(unsubscribed.load(Ordering::SeqCst));
    }

    #[tokio::test]
    async fn tauri_lifecycle_activating_slot_blocks_resume_and_cancel_awaits_cleanup() {
        let (stream, _live, unsubscribed) = tracked_stream([]);
        let started = Arc::new(AtomicBool::new(false));
        let started_notify = Arc::new(Notify::new());
        let release = Arc::new(Notify::new());
        let cancelled = Arc::new(AtomicBool::new(false));
        let head = AgentLiveProviderHeadAttach {
            through_cursor: cursor(0),
            live_sessions_complete: true,
            live_sessions: vec![],
            token: Box::new(BlockingPendingToken {
                resume: Mutex::new(Some(AgentLiveProviderResume {
                    through_cursor: cursor(0),
                    stream,
                })),
                started: Arc::clone(&started),
                started_notify: Arc::clone(&started_notify),
                release: Arc::clone(&release),
                cancelled,
            }),
        };
        let provider = Arc::new(FakeProvider::new(vec![head]));
        let manager = manager(Arc::clone(&provider));
        let pending = manager
            .begin(owner(), request(), Arc::new(RecordingSender::default()))
            .await
            .unwrap();
        let activation = tokio::spawn({
            let manager = manager.clone();
            let attach_id = pending.attach_id.clone();
            async move { manager.activate(owner(), &attach_id).await }
        });
        loop {
            let notified = started_notify.notified();
            if started.load(Ordering::SeqCst) {
                break;
            }
            notified.await;
        }

        assert_eq!(
            manager
                .resume(
                    owner(),
                    cursor_to_wire(&cursor(0)),
                    Arc::new(RecordingSender::default()),
                )
                .await,
            Err(AgentLiveAttachError::StaleLease)
        );
        assert_eq!(provider.resume_calls.load(Ordering::SeqCst), 0);

        let cancellation = tokio::spawn({
            let manager = manager.clone();
            let attach_id = pending.attach_id.clone();
            async move { manager.cancel(owner(), &attach_id).await }
        });
        tokio::time::timeout(Duration::from_secs(1), async {
            loop {
                let activating = manager.inner.state.lock().unwrap().activating.len();
                if activating == 0 {
                    break;
                }
                tokio::task::yield_now().await;
            }
        })
        .await
        .unwrap();
        assert!(!cancellation.is_finished());
        release.notify_one();
        cancellation.await.unwrap().unwrap();
        assert_eq!(
            activation.await.unwrap(),
            Err(AgentLiveAttachError::AttachNotFound)
        );
        assert!(unsubscribed.load(Ordering::SeqCst));
    }

    #[tokio::test]
    async fn tauri_lifecycle_reconnect_uses_full_stamp_and_retires_old_after_replay() {
        let (old_head, _old_live, _old_cancelled, old_unsubscribed) = tracked_head(0, vec![]);
        let (new_head, _new_live, _new_cancelled, new_unsubscribed) =
            tracked_head(1, vec![delivery(1, "session")]);
        let manager = manager(Arc::new(FakeProvider::new(vec![old_head, new_head])));
        let first = manager
            .begin(owner(), request(), Arc::new(RecordingSender::default()))
            .await
            .unwrap();
        manager.activate(owner(), &first.attach_id).await.unwrap();

        let mut reconnected = owner();
        reconnected.connection_stamp = ConnectionStamp::new(11, 13).unwrap();
        assert_ne!(owner().stream_key(), reconnected.stream_key());
        assert_eq!(
            owner().stream_lineage_key(),
            reconnected.stream_lineage_key()
        );
        let second = manager
            .begin(
                reconnected.clone(),
                request(),
                Arc::new(RecordingSender::default()),
            )
            .await
            .unwrap();
        let active = manager
            .activate(reconnected.clone(), &second.attach_id)
            .await
            .unwrap();
        assert!(old_unsubscribed.load(Ordering::SeqCst));
        let state = manager.inner.state.lock().unwrap();
        assert_eq!(state.active.len(), 1);
        assert!(state.active.contains_key(&reconnected.stream_key()));
        drop(state);
        manager
            .cancel_live_events(reconnected, &active.live_stream_id)
            .await
            .unwrap();
        assert!(new_unsubscribed.load(Ordering::SeqCst));
    }

    #[tokio::test]
    async fn tauri_lifecycle_runtime_install_is_one_shot_and_old_manager_stays_revocable() {
        let (old_head, _old_live, _old_cancelled, old_unsubscribed) = tracked_head(0, vec![]);
        let old_manager = manager(Arc::new(FakeProvider::new(vec![old_head])));
        let new_manager = manager(Arc::new(FakeProvider::new(vec![])));
        let state = AgentLiveTauriState::disabled();
        state
            .install_verified_runtime(old_manager.clone(), Arc::new(TestOwnerResolver))
            .unwrap();
        assert_eq!(
            state.install_verified_runtime(new_manager, Arc::new(TestOwnerResolver)),
            Err(AgentLiveAttachError::Unavailable)
        );

        let pending = old_manager
            .begin(owner(), request(), Arc::new(RecordingSender::default()))
            .await
            .unwrap();
        old_manager
            .activate(owner(), &pending.attach_id)
            .await
            .unwrap();
        state.revoke_exact_owner(&owner()).await.unwrap();
        assert!(old_unsubscribed.load(Ordering::SeqCst));
        assert!(old_manager.inner.state.lock().unwrap().active.is_empty());
    }

    #[tokio::test]
    async fn equal_fence_and_stamp_controllers_cannot_supersede_or_cancel_each_other() {
        let mut second_owner = owner();
        second_owner.controller_endpoint = endpoint(2);
        second_owner.peer_lineage_epoch = 7;
        // Numeric fence/stamp values intentionally collide; native endpoint
        // identity and peer lineage still define different controller leases.
        assert_eq!(second_owner.pairing_fence, owner().pairing_fence);
        assert_eq!(second_owner.connection_stamp, owner().connection_stamp);

        let mut first_live = None;
        let mut second_live = None;
        let provider = Arc::new(FakeProvider::new(vec![
            head(
                0,
                vec![],
                vec![],
                Some(&mut first_live),
                Arc::new(AtomicBool::new(false)),
            ),
            head(
                0,
                vec![],
                vec![],
                Some(&mut second_live),
                Arc::new(AtomicBool::new(false)),
            ),
        ]));
        let manager = manager(provider);
        let first = manager
            .begin(owner(), request(), Arc::new(RecordingSender::default()))
            .await
            .unwrap();
        let first_active = manager.activate(owner(), &first.attach_id).await.unwrap();
        let second = manager
            .begin(
                second_owner.clone(),
                request(),
                Arc::new(RecordingSender::default()),
            )
            .await
            .unwrap();
        let second_active = manager
            .activate(second_owner.clone(), &second.attach_id)
            .await
            .unwrap();
        assert_eq!(manager.inner.state.lock().unwrap().active.len(), 2);

        assert_eq!(
            manager
                .cancel_live_events(second_owner.clone(), &first_active.live_stream_id)
                .await,
            Err(AgentLiveAttachError::StaleLease)
        );
        manager.revoke_owner(&owner()).await;
        assert_eq!(manager.inner.state.lock().unwrap().active.len(), 1);
        manager
            .cancel_live_events(second_owner, &second_active.live_stream_id)
            .await
            .unwrap();
        assert!(manager.inner.state.lock().unwrap().active.is_empty());
        drop((first_live, second_live));
    }

    #[tokio::test]
    async fn activate_revalidates_after_finalize_and_fails_closed() {
        let finalized = Arc::new(AtomicBool::new(false));
        let mut provider = FakeProvider::new(vec![head(
            1,
            vec![],
            vec![delivery(1, "session")],
            None,
            finalized.clone(),
        )]);
        provider.invalidate_after_finalize = Some(finalized);
        let provider = Arc::new(provider);
        let manager = manager(provider.clone());
        let begun = manager
            .begin(owner(), request(), Arc::new(RecordingSender::default()))
            .await
            .unwrap();
        assert!(matches!(
            manager.activate(owner(), &begun.attach_id).await,
            Err(AgentLiveAttachError::StaleLease)
        ));
        assert!(provider.validate_calls.load(Ordering::SeqCst) >= 4);
    }

    #[tokio::test]
    async fn activate_revalidates_before_finalize_and_leaves_token_unconsumed_on_stale_owner() {
        let finalized = Arc::new(AtomicBool::new(false));
        let provider = Arc::new(FakeProvider::new(vec![head(
            0,
            vec![],
            vec![],
            None,
            finalized.clone(),
        )]));
        let manager = manager(provider.clone());
        let begun = manager
            .begin(owner(), request(), Arc::new(RecordingSender::default()))
            .await
            .unwrap();
        provider.valid.store(false, Ordering::SeqCst);
        assert!(matches!(
            manager.activate(owner(), &begun.attach_id).await,
            Err(AgentLiveAttachError::StaleLease)
        ));
        assert!(!finalized.load(Ordering::SeqCst));
    }

    #[tokio::test]
    async fn activating_new_stream_supersedes_old_only_after_new_replay_is_queued() {
        let finalized_a = Arc::new(AtomicBool::new(false));
        let finalized_b = Arc::new(AtomicBool::new(false));
        let mut live_a = None;
        let mut live_b = None;
        let provider = Arc::new(FakeProvider::new(vec![
            head(0, vec![], vec![], Some(&mut live_a), finalized_a),
            head(
                1,
                vec![],
                vec![delivery(1, "session-b")],
                Some(&mut live_b),
                finalized_b,
            ),
        ]));
        let manager = manager(provider);
        let old_sender = Arc::new(RecordingSender::default());
        let first = manager
            .begin(owner(), request(), old_sender.clone())
            .await
            .unwrap();
        manager.activate(owner(), &first.attach_id).await.unwrap();

        let new_sender = Arc::new(RecordingSender::default());
        let second = manager
            .begin(owner(), request(), new_sender.clone())
            .await
            .unwrap();
        let second_active = manager.activate(owner(), &second.attach_id).await.unwrap();
        tokio::task::yield_now().await;
        assert_eq!(new_sender.event_sequences(), [1]);
        assert_eq!(second_active.live_stream_id, second.attach_id);
        let _ = live_a.unwrap().send(Ok(delivery(1, "session-a")));
        tokio::task::yield_now().await;
        live_b.unwrap().send(Ok(delivery(2, "session-b"))).unwrap();
        tokio::time::timeout(Duration::from_secs(1), new_sender.wait_for_event_count(2))
            .await
            .unwrap();
        assert_eq!(new_sender.event_sequences(), [1, 2]);
        assert!(old_sender.event_sequences().is_empty());
    }

    #[tokio::test]
    async fn cancel_is_idempotent_wrong_owner_is_rejected_and_ttl_drops_token() {
        let finalized = Arc::new(AtomicBool::new(false));
        let provider = Arc::new(FakeProvider::new(vec![
            head(0, vec![], vec![], None, finalized.clone()),
            head(0, vec![], vec![], None, finalized),
        ]));
        let config = AgentLiveAttachManagerConfig {
            pending_ttl: Duration::from_millis(20),
            ..Default::default()
        };
        let manager =
            AgentLiveAttachManager::with_config(provider, Arc::new(TestProjector), config).unwrap();
        let begun = manager
            .begin(owner(), request(), Arc::new(RecordingSender::default()))
            .await
            .unwrap();
        let mut wrong = owner();
        wrong.account_data_generation += 1;
        let wrong_begin = manager
            .begin(owner(), request(), Arc::new(RecordingSender::default()))
            .await
            .unwrap();
        assert_eq!(
            manager.cancel(wrong, &wrong_begin.attach_id).await,
            Err(AgentLiveAttachError::StaleLease)
        );
        let already_gone = "00".repeat(ATTACH_ID_RANDOM_BYTES);
        manager.cancel(owner(), &already_gone).await.unwrap();
        manager.cancel(owner(), &already_gone).await.unwrap();
        tokio::time::sleep(Duration::from_millis(40)).await;
        assert!(matches!(
            manager.activate(owner(), &begun.attach_id).await,
            Err(AgentLiveAttachError::AttachNotFound)
        ));
    }

    #[test]
    fn complete_snapshot_is_deterministic_and_enforces_account_bounds() {
        assert!(validate_and_restore_snapshot(
            true,
            vec![projection("session-a", &[]), projection("session-b", &[])]
        )
        .is_ok());
        assert!(matches!(
            validate_and_restore_snapshot(
                true,
                vec![projection("session-b", &[]), projection("session-a", &[])]
            ),
            Err(AgentLiveAttachError::SnapshotRequired { .. })
        ));
        let too_many_sessions = (0..=MAX_LIVE_SESSIONS)
            .map(|index| projection(&format!("session-{index:03}"), &[]))
            .collect();
        assert!(validate_and_restore_snapshot(true, too_many_sessions).is_err());
        let max_sessions = (0..MAX_LIVE_SESSIONS)
            .map(|index| projection(&format!("session-{index:03}"), &[]))
            .collect();
        assert_eq!(
            validate_and_restore_snapshot(true, max_sessions)
                .unwrap()
                .len(),
            MAX_LIVE_SESSIONS
        );
        let item_ids = (0..=MAX_LIVE_ITEMS)
            .map(|index| format!("item-{index}"))
            .collect::<Vec<_>>();
        let item_refs = item_ids.iter().map(String::as_str).collect::<Vec<_>>();
        assert!(
            validate_and_restore_snapshot(true, vec![projection("session", &item_refs)]).is_err()
        );
        let max_item_refs = item_refs[..200].to_vec();
        assert_eq!(
            validate_and_restore_snapshot(true, vec![projection("session", &max_item_refs)])
                .unwrap()[0]
                .live_items
                .len(),
            200
        );
    }

    #[tokio::test]
    async fn resume_maps_typed_snapshot_reason_and_channel_failure_installs_no_active() {
        let provider = Arc::new(FakeProvider::new(vec![]));
        let (failed_stream, _) = stream([Err(AgentLiveReceiveError::HeadReloadRequired(
            HeadReloadReason::RetentionGap,
        ))]);
        provider
            .resumes
            .lock()
            .unwrap()
            .push_back(AgentLiveProviderResume {
                through_cursor: cursor(1),
                stream: failed_stream,
            });
        let manager = manager(provider.clone());
        assert!(matches!(
            manager
                .resume(
                    owner(),
                    cursor_to_wire(&cursor(0)),
                    Arc::new(RecordingSender::default())
                )
                .await,
            Err(AgentLiveAttachError::SnapshotRequired {
                reason: AgentLiveSnapshotReason::RetentionGap
            })
        ));

        let (next_stream, _) = stream([Ok(delivery(1, "session"))]);
        provider
            .resumes
            .lock()
            .unwrap()
            .push_back(AgentLiveProviderResume {
                through_cursor: cursor(1),
                stream: next_stream,
            });
        let sender = Arc::new(RecordingSender::default());
        sender.fail.store(true, Ordering::SeqCst);
        assert!(matches!(
            manager
                .resume(owner(), cursor_to_wire(&cursor(0)), sender)
                .await,
            Err(AgentLiveAttachError::ChannelClosed)
        ));
        assert!(manager.inner.state.lock().unwrap().active.is_empty());
    }

    #[test]
    fn snapshot_wire_includes_explicit_absence_semantics_without_selected_duplication() {
        let restored = validate_and_restore_snapshot(
            true,
            vec![
                projection("session-a", &["a"]),
                projection("session-b", &["b"]),
            ],
        )
        .unwrap();
        assert_eq!(restored.len(), 2);
        assert!(!restored
            .iter()
            .any(|session| session.session_id == "session-c"));

        let frame = AgentLiveChannelFrame::SnapshotRequired(AgentLiveSnapshotRequiredFrame {
            live_event_version: AGENT_LIVE_PRESENTATION_VERSION,
            event_type: "snapshotRequired",
            target_id: owner().target_id,
            host_epoch: "11".into(),
            connection_generation: 12,
            reason: AgentLiveSnapshotReason::PausedSubscriberOverflow,
            last_event_cursor: cursor_to_wire(&cursor(4)),
        });
        let encoded = serde_json::to_value(frame).unwrap();
        assert_eq!(encoded["liveEventVersion"], 1);
        assert_eq!(encoded["eventType"], "snapshotRequired");
        assert_eq!(encoded["hostEpoch"], "11");
        assert_eq!(encoded["reason"], "paused_overflow");
    }

    #[test]
    fn expected_lease_requires_canonical_decimal_epoch_and_exact_full_stamp() {
        let current = owner();
        let exact = AgentExpectedLiveLease {
            target_id: current.target_id.clone(),
            host_epoch: "11".into(),
            connection_generation: 12,
        };
        exact.validate_against(&current).unwrap();
        for invalid in ["", "0", "01", "+11", " 11", "11 ", "18446744073709551616"] {
            let mut candidate = exact.clone();
            candidate.host_epoch = invalid.into();
            assert!(matches!(
                candidate.validate_against(&current),
                Err(AgentLiveAttachError::InvalidRequest { .. })
            ));
        }
        let mut restarted = exact.clone();
        restarted.host_epoch = "12".into();
        restarted.connection_generation = 1;
        assert_eq!(
            restarted.validate_against(&current),
            Err(AgentLiveAttachError::StaleLease)
        );
    }

    #[test]
    fn managed_state_is_typed_unavailable_until_native_authority_is_installed() {
        assert_eq!(
            AgentLiveTauriState::disabled().enabled().err(),
            Some(AgentLiveAttachError::Unavailable)
        );
    }

    #[test]
    fn host_restart_epoch_prevents_generation_aba_in_owner_and_wire() {
        let old = owner();
        let mut restarted = old.clone();
        restarted.connection_stamp = ConnectionStamp::new(12, 1).unwrap();
        assert_ne!(old, restarted);
        let old_expected = AgentExpectedLiveLease {
            target_id: old.target_id.clone(),
            host_epoch: old.connection_stamp.host_epoch().to_string(),
            connection_generation: old.connection_stamp.generation(),
        };
        assert_eq!(
            old_expected.validate_against(&restarted),
            Err(AgentLiveAttachError::StaleLease)
        );
        let serialized = serde_json::to_value(AgentExpectedLiveLease {
            target_id: restarted.target_id,
            host_epoch: restarted.connection_stamp.host_epoch().to_string(),
            connection_generation: restarted.connection_stamp.generation(),
        })
        .unwrap();
        assert_eq!(serialized["hostEpoch"], "12");
        assert_eq!(serialized["connectionGeneration"], 1);
    }

    #[test]
    fn closed_live_wire_is_versioned_exhaustive_and_hides_commit_storage_fields() {
        let projector = ClosedAgentLiveDeliveryProjector;
        let events = vec![
            MapleLiveEvent::RunStarted {
                event_id: "event-run-started".into(),
            },
            MapleLiveEvent::TimelineUpsert {
                event_id: "event-upsert".into(),
                item: live_item("safe-item"),
            },
            MapleLiveEvent::TimelineCleared {
                event_id: "event-cleared".into(),
                reason: MapleLiveClearReason::ExplicitReload,
            },
            MapleLiveEvent::HistoryReplaced {
                event_id: "event-replaced".into(),
            },
            MapleLiveEvent::HistoryHeadCommitted {
                event_id: "private-commit-id".into(),
                history_revision: "private-storage-revision".into(),
                through_event_cursor: cursor(0),
            },
            MapleLiveEvent::SessionUpdated {
                event_id: "event-session".into(),
                session: MapleLiveSessionSummary {
                    id: "session".into(),
                    title: "Title".into(),
                    project_root: "/project".into(),
                    created_ms: 1,
                    updated_ms: 2,
                    page_sort_ms: 3,
                    message_count: 4,
                    model: None,
                    mode: "auto".into(),
                },
            },
            MapleLiveEvent::RunFinished {
                event_id: "event-finished".into(),
                terminal: MapleLiveRunTerminal::Completed,
            },
            MapleLiveEvent::SessionDeleted {
                event_id: "event-deleted".into(),
            },
            MapleLiveEvent::UserFacingError {
                event_id: "event-error".into(),
                error: MapleLiveUserFacingError {
                    id: "safe-error".into(),
                    kind: crate::agent_live_coordinator::MapleLiveUserFacingErrorKind::Error,
                    title: Some("Agent error".into()),
                    message: SAFE_REMOTE_AGENT_ERROR.into(),
                    created_ms: 5,
                },
            },
        ];
        let expected_types = [
            "runStarted",
            "timelineUpsert",
            "timelineCleared",
            "historyReplaced",
            "cursorAdvanced",
            "sessionUpdated",
            "runFinished",
            "sessionDeleted",
            "userFacingError",
        ];
        for (index, (event, expected_type)) in events.into_iter().zip(expected_types).enumerate() {
            let run_id = match &event {
                MapleLiveEvent::HistoryHeadCommitted { .. }
                | MapleLiveEvent::SessionDeleted { .. }
                | MapleLiveEvent::TimelineCleared {
                    reason: MapleLiveClearReason::ExplicitReload,
                    ..
                } => None,
                _ => Some("run".into()),
            };
            let delivery = AgentLiveDelivery {
                cursor: cursor(index as u64 + 1),
                session_id: "session".into(),
                run_id,
                event,
            };
            let projected = projector.project_delivery(&delivery).unwrap();
            let encoded = serde_json::to_value(AgentOrderedLiveEvent {
                live_event_version: AGENT_LIVE_PRESENTATION_VERSION,
                target_id: owner().target_id,
                host_epoch: "11".into(),
                connection_generation: 12,
                event_epoch: delivery.cursor.journal_id().into(),
                event_sequence: delivery.cursor.sequence(),
                session_id: delivery.session_id,
                run_id: delivery.run_id,
                event: projected,
            })
            .unwrap();
            assert_eq!(encoded["liveEventVersion"], 1);
            assert_eq!(encoded["eventType"], expected_type);
            let bytes = serde_json::to_string(&encoded).unwrap();
            assert!(!bytes.contains("private-storage-revision"));
            assert!(!bytes.contains("private-commit-id"));
            assert!(!bytes.contains("eventId"));
            assert!(!bytes.contains("input"));
            assert!(!bytes.contains("output"));
        }
    }

    #[test]
    fn closed_wire_rejects_crate_internal_unredacted_tool_and_error_payloads() {
        let projector = ClosedAgentLiveDeliveryProjector;
        for (item_type, title, text, status) in [
            (
                MapleLiveItemType::Tool,
                "run curl https://secret.invalid?token=hunter2",
                "/Users/alice/.env API_KEY=hunter2",
                "failed",
            ),
            (
                MapleLiveItemType::Error,
                "provider parser failed",
                "token=hunter2 at /Users/alice/private",
                "failed",
            ),
        ] {
            let delivery = AgentLiveDelivery {
                cursor: cursor(1),
                session_id: "session".into(),
                run_id: Some("run".into()),
                event: MapleLiveEvent::TimelineUpsert {
                    event_id: "event-redaction-guard".into(),
                    item: MapleLiveTimelineItem {
                        id: "unsafe-item".into(),
                        item_type,
                        role: None,
                        title: Some(title.into()),
                        text: Some(text.into()),
                        status: Some(status.into()),
                        created_ms: 1,
                        merge: MapleLiveMerge::Replace,
                    },
                },
            };
            assert_eq!(
                projector.project_delivery(&delivery),
                Err(AgentLiveAttachError::ProjectionRejected)
            );
        }
    }
}
