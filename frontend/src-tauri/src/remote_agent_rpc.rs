//! Typed Maple Agent operations carried by the authenticated Iroh transport.
//!
//! This first vertical slice intentionally exposes only runtime status. It is
//! not a Tauri command router and does not accept command names. The controller
//! selects the current authenticated generation through
//! [`GenerationConnectionManager`]; the desktop host injects the exact status
//! provider it wants this paired controller to observe.
#![allow(
    dead_code,
    reason = "library-level remote slice is wired to Tauri in a later milestone"
)]

use std::{future::Future, pin::Pin, time::Duration};

#[cfg(desktop)]
use std::{
    collections::HashMap,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc, Mutex as StdMutex,
    },
};

#[cfg(all(test, desktop))]
use crate::remote_protocol::RemoteAgentLiveControlRequest;
#[cfg(desktop)]
use crate::remote_protocol::{
    RemoteAgentBulkRequest, RemoteAgentControlRequest, RemoteAgentHistoryRecord,
    RemoteAgentLiveClearReason, RemoteAgentLiveEventsRequest, RemoteAgentLiveRunTerminal,
    RemoteAgentSessionSummary, RemoteAgentTimelineItem,
};
#[cfg(desktop)]
use crate::remote_transport::VerifiedIncomingPeerAuthorization;
#[cfg(desktop)]
use crate::{
    agent::{AgentHistoryPageRequest, AgentLiveEventCursor},
    agent_live_binding::AgentLiveBindingLease,
    agent_live_coordinator::{
        AgentLiveCoordinatorError, AgentLiveReceiveError, HeadReloadReason, MapleLiveClearReason,
        MapleLiveEvent, MapleLiveItemType, MapleLiveMerge, MapleLiveRole, MapleLiveRunTerminal,
        MapleLiveTimelineItem,
    },
    agent_live_host::{
        AgentLiveHostError, AgentLivePeerRevocationHook, AgentLiveRemoteAttachError,
        AgentLiveRemoteAttachProvider, AgentLiveRemoteAttachService, AgentLiveRemoteDelivery,
        AgentLiveRemoteHeadBegin, AgentLiveRemotePendingAttach, AgentLiveRemoteResume,
        AgentLiveRemoteStreamError,
    },
};
use crate::{
    remote_protocol::{
        remote_live_projection_item_wire_bytes, remote_live_projection_session_wire_bytes,
        ActivateAgentLiveAttachRequest, ActivateAgentLiveAttachResponse, AgentHistoryPageFrame,
        AgentLiveActivationDisposition, AgentLiveCancelKind, AgentLiveStreamFrame,
        BeginAgentLiveAttachRequest, CancelAgentLiveRequest, CancelAgentLiveResponse, ErrorCode,
        GetRuntimeStatusRequest, GetRuntimeStatusResponse, ListAgentHistoryRecordsRequest,
        ListAgentSessionsRequest, ListAgentSessionsResponse, PeerDirection, ProtocolError,
        RemoteAgentHistoryPage, RemoteAgentLiveDelivery, RemoteAgentLiveHeadSnapshot,
        RemoteAgentLiveSessionSnapshot, RemoteAgentLiveSnapshotReason, RemoteAgentLiveStreamStart,
        RemoteAgentRuntimeStatus, RemoteLiveEventCursor, RequestEnvelope, ResponseEnvelope,
        ResumeAgentLiveEventsRequest, LIVE_PROJECTION_OUTER_OVERHEAD_BYTES,
        MAX_LIVE_PROJECTION_BYTES_PER_ACCOUNT, PROTOCOL_VERSION,
    },
    remote_transport::{
        validate_frame_encodable, AcceptedRequest, ConnectedPeer, GenerationConnectionManager,
        MapleIrohEndpoint, StreamingResponse,
    },
};
#[cfg(desktop)]
use getrandom::fill as fill_random;
#[cfg(desktop)]
use tokio::sync::oneshot;

const RUNTIME_STATUS_PROVIDER_TIMEOUT: Duration = Duration::from_secs(1);
const RUNTIME_STATUS_RESPONSE_BUDGET: Duration = Duration::from_millis(50);
const AGENT_HISTORY_PROVIDER_TIMEOUT: Duration = Duration::from_secs(5);
const AGENT_HISTORY_RESPONSE_BUDGET: Duration = Duration::from_secs(1);
const REMOTE_LIVE_SUBSCRIPTION_CAPACITY: usize = 128;
const REMOTE_LIVE_PENDING_TTL: Duration = Duration::from_secs(30);
const MAX_REMOTE_LIVE_PENDING_PER_PEER: usize = 16;
const MAX_REMOTE_LIVE_LIFECYCLES: usize = 128;
const MAX_REMOTE_LIVE_REPLAY_EVENTS: usize = REMOTE_LIVE_SUBSCRIPTION_CAPACITY;
const LIVE_ID_RANDOM_BYTES: usize = 16;
const MAX_LIVE_ID_ATTEMPTS: usize = 8;

/// Transport-neutral host authority for the one implemented remote operation.
///
/// Implementations are injected; the shared protocol layer never imports
/// Goose, Tauri, or a generic native-command dispatcher.
pub trait RemoteRuntimeStatusProvider: Send + Sync {
    fn runtime_status(
        &self,
    ) -> Pin<Box<dyn Future<Output = Result<RemoteAgentRuntimeStatus, ProtocolError>> + Send + '_>>;
}

/// Exact transport-neutral host authority shared with embedded Tauri history.
pub trait RemoteAgentHistoryProvider: Send + Sync {
    fn list_agent_history(
        &self,
        request: &ListAgentHistoryRecordsRequest,
    ) -> Pin<Box<dyn Future<Output = Result<RemoteAgentHistoryPage, ProtocolError>> + Send + '_>>;
}

pub trait RemoteAgentSessionListProvider: Send + Sync {
    fn list_agent_sessions(
        &self,
        request: &ListAgentSessionsRequest,
    ) -> Pin<Box<dyn Future<Output = Result<ListAgentSessionsResponse, ProtocolError>> + Send + '_>>;
}

#[cfg(desktop)]
#[derive(Clone)]
pub struct RemoteAgentLiveRpcHost {
    inner: Arc<RemoteAgentLiveRpcHostInner>,
}

#[cfg(desktop)]
struct RemoteAgentLiveRpcHostInner {
    provider: Arc<dyn AgentLiveRemoteAttachProvider>,
    /// Every mutation is non-blocking and may therefore be linearized inside
    /// `VerifiedIncomingPeerAuthorization::with_current` while its admission
    /// read guard is held. No async work runs under this mutex.
    state: StdMutex<RemoteAgentLiveRpcState>,
}

#[cfg(desktop)]
#[derive(Default)]
struct RemoteAgentLiveRpcState {
    pending: HashMap<String, PendingRemoteAgentLiveAttach>,
    activating: HashMap<String, ActivatingRemoteAgentLiveStream>,
    active: HashMap<String, ActiveRemoteAgentLiveStream>,
}

#[cfg(desktop)]
struct PendingRemoteAgentLiveAttach {
    authority: VerifiedIncomingPeerAuthorization,
    service: Arc<dyn AgentLiveRemoteAttachService>,
    activate: Option<oneshot::Sender<ActivateRemoteAgentLiveCommand>>,
    cancellation: Arc<RemoteAgentLiveCancellation>,
    expires_at: Option<tokio::time::Instant>,
}

#[cfg(desktop)]
struct ActivateRemoteAgentLiveCommand {
    live_stream_id: String,
    response: oneshot::Sender<Result<AgentLiveActivationDisposition, ProtocolError>>,
}

#[cfg(desktop)]
struct ActivatingRemoteAgentLiveStream {
    authority: VerifiedIncomingPeerAuthorization,
    service: Arc<dyn AgentLiveRemoteAttachService>,
    cancellation: Arc<RemoteAgentLiveCancellation>,
    /// Becomes visible in StreamStart before the slot is promoted to Active.
    /// Keeping it on the continuously owned Activating slot lets an exact
    /// ActiveStream cancellation find the lifecycle in that narrow window.
    public_live_stream_id: Option<String>,
}

#[cfg(desktop)]
struct ActiveRemoteAgentLiveStream {
    authority: VerifiedIncomingPeerAuthorization,
    service: Arc<dyn AgentLiveRemoteAttachService>,
    cancellation: Arc<RemoteAgentLiveCancellation>,
    /// Retained until native unsubscribe acknowledgement so a controller that
    /// has not yet learned `live_stream_id` can still cancel by its attach ID.
    activation_id: String,
}

#[cfg(desktop)]
#[derive(Default)]
struct RemoteAgentLiveCancellation {
    requested: AtomicBool,
    requested_notify: tokio::sync::Notify,
    completion: StdMutex<Option<Result<(), ProtocolError>>>,
    completion_notify: tokio::sync::Notify,
}

#[cfg(desktop)]
impl RemoteAgentLiveCancellation {
    fn is_requested(&self) -> bool {
        self.requested.load(Ordering::Acquire)
    }

    async fn wait_requested(&self) {
        loop {
            let notified = self.requested_notify.notified();
            if self.is_requested() {
                return;
            }
            notified.await;
        }
    }

    fn request(&self) {
        self.requested.store(true, Ordering::Release);
        self.requested_notify.notify_waiters();
    }

    async fn wait_completion(&self) -> Result<(), ProtocolError> {
        loop {
            let notified = self.completion_notify.notified();
            if let Some(result) = self
                .completion
                .lock()
                .map_err(|_| live_lifecycle_state_error())?
                .clone()
            {
                return result;
            }
            notified.await;
        }
    }

    async fn request_and_wait(&self) -> Result<(), ProtocolError> {
        self.request();
        self.wait_completion().await
    }

    fn completed_successfully(&self) -> bool {
        self.completion
            .lock()
            .map(|completion| matches!(completion.as_ref(), Some(Ok(()))))
            .unwrap_or(false)
    }

    fn complete(&self, result: Result<(), ProtocolError>) {
        if let Ok(mut completion) = self.completion.lock() {
            if completion.is_none() {
                *completion = Some(result);
                self.completion_notify.notify_waiters();
            }
        }
    }
}

#[cfg(desktop)]
impl RemoteAgentLiveRpcHost {
    pub fn unavailable() -> Self {
        Self::new(Arc::new(
            crate::agent_live_host::UnavailableAgentLiveRemoteAttachProvider,
        ))
    }

    pub(crate) fn new(provider: Arc<dyn AgentLiveRemoteAttachProvider>) -> Self {
        Self {
            inner: Arc::new(RemoteAgentLiveRpcHostInner {
                provider,
                state: StdMutex::new(RemoteAgentLiveRpcState::default()),
            }),
        }
    }

    async fn bind_service(
        &self,
        authority: &VerifiedIncomingPeerAuthorization,
    ) -> Result<Arc<dyn AgentLiveRemoteAttachService>, ProtocolError> {
        authority.revalidate_current()?;
        let service = self
            .inner
            .provider
            .bind(authority.clone())
            .await
            .map_err(map_live_attach_error)?;
        authority.revalidate_current()?;
        Ok(service)
    }

    fn with_current_state<R>(
        &self,
        authority: &VerifiedIncomingPeerAuthorization,
        operation: impl FnOnce(&mut RemoteAgentLiveRpcState) -> Result<R, ProtocolError>,
    ) -> Result<R, ProtocolError> {
        authority.with_current(|| {
            let mut state = self
                .inner
                .state
                .lock()
                .map_err(|_| live_lifecycle_state_error())?;
            operation(&mut state)
        })?
    }

    async fn install_pending(
        &self,
        attach_id: String,
        authority: VerifiedIncomingPeerAuthorization,
        service: Arc<dyn AgentLiveRemoteAttachService>,
        activate: oneshot::Sender<ActivateRemoteAgentLiveCommand>,
        cancellation: Arc<RemoteAgentLiveCancellation>,
    ) -> Result<(), ProtocolError> {
        let admission = authority.clone();
        self.with_current_state(&admission, move |state| {
            prune_closed_lifecycles(state);
            let pending_for_peer = state
                .pending
                .values()
                .filter(|known| same_live_occupancy(&known.authority, &authority))
                .count();
            if live_lifecycle_count(state) >= MAX_REMOTE_LIVE_LIFECYCLES
                || pending_for_peer >= MAX_REMOTE_LIVE_PENDING_PER_PEER
                || stable_occupancy_in_use(state, &authority)
                || lifecycle_id_in_use(state, &attach_id)
            {
                return Err(ProtocolError::new(
                    ErrorCode::TransportUnavailable,
                    "remote Agent live attachment capacity is unavailable",
                    true,
                ));
            }
            state.pending.insert(
                attach_id,
                PendingRemoteAgentLiveAttach {
                    authority,
                    service,
                    activate: Some(activate),
                    cancellation,
                    expires_at: None,
                },
            );
            Ok(())
        })
    }

    async fn arm_pending(
        &self,
        authority: &VerifiedIncomingPeerAuthorization,
        attach_id: &str,
    ) -> Result<tokio::time::Instant, ProtocolError> {
        self.arm_pending_for(authority, attach_id, REMOTE_LIVE_PENDING_TTL)
            .await
    }

    async fn arm_pending_for(
        &self,
        authority: &VerifiedIncomingPeerAuthorization,
        attach_id: &str,
        ttl: Duration,
    ) -> Result<tokio::time::Instant, ProtocolError> {
        if ttl.is_zero() {
            return Err(live_lifecycle_state_error());
        }
        let deadline = tokio::time::Instant::now() + ttl;
        self.with_current_state(authority, |state| {
            let pending = state.pending.get_mut(attach_id).ok_or_else(|| {
                live_lifecycle_unavailable("attachment owner closed before arming")
            })?;
            if !same_remote_authority(&pending.authority, authority) {
                return Err(stale_live_lease());
            }
            if pending.expires_at.replace(deadline).is_some() {
                return Err(live_lifecycle_state_error());
            }
            Ok(deadline)
        })
    }

    async fn take_pending_for_activation(
        &self,
        authority: &VerifiedIncomingPeerAuthorization,
        attach_id: &str,
    ) -> Result<PendingRemoteAgentLiveAttach, ProtocolError> {
        self.with_current_state(authority, |state| {
            prune_closed_lifecycles(state);
            match state.pending.get(attach_id) {
                Some(pending) if !same_remote_authority(&pending.authority, authority) => {
                    return Err(stale_live_lease());
                }
                Some(pending)
                    if pending
                        .expires_at
                        .is_some_and(|deadline| deadline <= tokio::time::Instant::now()) =>
                {
                    return Err(ProtocolError::new(
                        ErrorCode::AgentLiveUnavailable,
                        "remote Agent live attachment expired",
                        true,
                    ));
                }
                Some(pending) if pending.expires_at.is_none() => {
                    return Err(live_lifecycle_unavailable(
                        "remote Agent live snapshot is not complete",
                    ));
                }
                Some(_) => {}
                None => {
                    return Err(ProtocolError::new(
                        ErrorCode::AgentLiveUnavailable,
                        "remote Agent live attachment was not found",
                        true,
                    ));
                }
            }
            let pending = state
                .pending
                .remove(attach_id)
                .expect("validated pending attachment exists");
            state.activating.insert(
                attach_id.to_string(),
                ActivatingRemoteAgentLiveStream {
                    authority: pending.authority.clone(),
                    service: Arc::clone(&pending.service),
                    cancellation: Arc::clone(&pending.cancellation),
                    public_live_stream_id: None,
                },
            );
            // Cancellation ownership remains continuously reachable through the
            // Activating entry. Only the command sender moves to the Control owner.
            Ok(pending)
        })
    }

    async fn reserve_activating(
        &self,
        live_stream_id: String,
        authority: VerifiedIncomingPeerAuthorization,
        service: Arc<dyn AgentLiveRemoteAttachService>,
        cancellation: Arc<RemoteAgentLiveCancellation>,
    ) -> Result<(), ProtocolError> {
        let admission = authority.clone();
        self.with_current_state(&admission, move |state| {
            prune_closed_lifecycles(state);
            if lifecycle_id_in_use(state, &live_stream_id)
                || stable_occupancy_in_use(state, &authority)
                || live_lifecycle_count(state) >= MAX_REMOTE_LIVE_LIFECYCLES
            {
                return Err(ProtocolError::new(
                    ErrorCode::TransportUnavailable,
                    "remote Agent live stream acquisition capacity is unavailable",
                    true,
                ));
            }
            state.activating.insert(
                live_stream_id.clone(),
                ActivatingRemoteAgentLiveStream {
                    authority,
                    service,
                    cancellation,
                    public_live_stream_id: Some(live_stream_id),
                },
            );
            Ok(())
        })
    }

    async fn name_activating_stream(
        &self,
        activation_id: &str,
        live_stream_id: &str,
        authority: &VerifiedIncomingPeerAuthorization,
    ) -> Result<(), ProtocolError> {
        self.with_current_state(authority, |state| {
            if lifecycle_id_in_use(state, live_stream_id) {
                return Err(live_lifecycle_unavailable(
                    "remote Agent live stream id is already in use",
                ));
            }
            let activating = state
                .activating
                .get_mut(activation_id)
                .ok_or_else(|| live_lifecycle_unavailable("live acquisition was cancelled"))?;
            if !same_remote_authority(&activating.authority, authority) {
                return Err(stale_live_lease());
            }
            if activating.cancellation.is_requested()
                || activating
                    .public_live_stream_id
                    .replace(live_stream_id.into())
                    .is_some()
            {
                return Err(live_lifecycle_unavailable("live acquisition was cancelled"));
            }
            Ok(())
        })
    }

    async fn promote_activating(
        &self,
        activation_id: &str,
        live_stream_id: &str,
        authority: &VerifiedIncomingPeerAuthorization,
    ) -> Result<(), ProtocolError> {
        self.with_current_state(authority, |state| {
            let activating = state
                .activating
                .get(activation_id)
                .ok_or_else(|| live_lifecycle_unavailable("live acquisition was cancelled"))?;
            if !same_remote_authority(&activating.authority, authority) {
                return Err(stale_live_lease());
            }
            if activating.cancellation.is_requested()
                || activating.public_live_stream_id.as_deref() != Some(live_stream_id)
            {
                return Err(live_lifecycle_unavailable("live acquisition was cancelled"));
            }
            let activating = state
                .activating
                .remove(activation_id)
                .expect("validated activating lifecycle exists");
            state.active.insert(
                live_stream_id.to_string(),
                ActiveRemoteAgentLiveStream {
                    authority: activating.authority,
                    service: activating.service,
                    cancellation: activating.cancellation,
                    activation_id: activation_id.to_string(),
                },
            );
            Ok(())
        })
    }

    async fn remove_pending(&self, live_id: &str, authority: &VerifiedIncomingPeerAuthorization) {
        let Ok(mut state) = self.inner.state.lock() else {
            return;
        };
        if state
            .pending
            .get(live_id)
            .is_some_and(|known| same_remote_authority(&known.authority, authority))
        {
            state.pending.remove(live_id);
        }
    }

    async fn remove_activating(
        &self,
        live_id: &str,
        authority: &VerifiedIncomingPeerAuthorization,
    ) {
        let Ok(mut state) = self.inner.state.lock() else {
            return;
        };
        if state
            .activating
            .get(live_id)
            .is_some_and(|known| same_remote_authority(&known.authority, authority))
        {
            state.activating.remove(live_id);
        }
    }

    async fn remove_active(&self, live_id: &str, authority: &VerifiedIncomingPeerAuthorization) {
        let Ok(mut state) = self.inner.state.lock() else {
            return;
        };
        if state
            .active
            .get(live_id)
            .is_some_and(|known| same_remote_authority(&known.authority, authority))
        {
            state.active.remove(live_id);
        }
    }

    async fn cancel_lifecycle(
        &self,
        authority: &VerifiedIncomingPeerAuthorization,
        kind: AgentLiveCancelKind,
        live_id: &str,
    ) -> Result<(), ProtocolError> {
        let cancellation = self.with_current_state(authority, |state| {
            prune_closed_lifecycles(state);
            Ok(match kind {
                AgentLiveCancelKind::PendingAttach => {
                    let lifecycle = state
                        .pending
                        .get(live_id)
                        .map(|known| (&known.authority, &known.cancellation))
                        .or_else(|| {
                            state
                                .activating
                                .get(live_id)
                                .map(|known| (&known.authority, &known.cancellation))
                        })
                        .or_else(|| {
                            state
                                .active
                                .values()
                                .find(|known| known.activation_id == live_id)
                                .map(|known| (&known.authority, &known.cancellation))
                        });
                    match lifecycle {
                        Some((known, _)) if !same_remote_authority(known, authority) => {
                            return Err(stale_live_lease());
                        }
                        Some((_, cancellation)) => Some(Arc::clone(cancellation)),
                        None => None,
                    }
                }
                AgentLiveCancelKind::ActiveStream => match state.active.get(live_id) {
                    Some(active) if !same_remote_authority(&active.authority, authority) => {
                        return Err(stale_live_lease());
                    }
                    Some(active) => Some(Arc::clone(&active.cancellation)),
                    None => {
                        let lifecycle = state
                            .activating
                            .values()
                            .find(|known| known.public_live_stream_id.as_deref() == Some(live_id));
                        match lifecycle {
                            Some(known) if !same_remote_authority(&known.authority, authority) => {
                                return Err(stale_live_lease());
                            }
                            Some(known) => Some(Arc::clone(&known.cancellation)),
                            None => None,
                        }
                    }
                },
            })
        })?;
        if let Some(cancellation) = cancellation {
            cancellation.request_and_wait().await?;
        }
        Ok(())
    }

    async fn revoke_binding_lease(
        &self,
        revoked: &AgentLiveBindingLease,
    ) -> Result<(), ProtocolError> {
        let Some(revoked_authority) = revoked.remote_authority() else {
            return Err(live_lifecycle_state_error());
        };
        let cancellations = {
            let state = self
                .inner
                .state
                .lock()
                .map_err(|_| live_lifecycle_state_error())?;
            let mut cancellations = Vec::new();
            let mut collect =
                |authority: &VerifiedIncomingPeerAuthorization,
                 cancellation: &Arc<RemoteAgentLiveCancellation>| {
                    if same_remote_authority(authority, revoked_authority) {
                        cancellations.push(Arc::clone(cancellation));
                    }
                };
            for known in state.pending.values() {
                collect(&known.authority, &known.cancellation);
            }
            for known in state.activating.values() {
                collect(&known.authority, &known.cancellation);
            }
            for known in state.active.values() {
                collect(&known.authority, &known.cancellation);
            }
            cancellations
        };
        for cancellation in &cancellations {
            cancellation.request();
        }
        for cancellation in cancellations {
            cancellation.wait_completion().await?;
        }
        Ok(())
    }
}

#[cfg(desktop)]
#[async_trait::async_trait]
impl AgentLivePeerRevocationHook for RemoteAgentLiveRpcHost {
    async fn revoke_exact_peer(
        &self,
        revoked: &AgentLiveBindingLease,
    ) -> Result<(), AgentLiveHostError> {
        self.revoke_binding_lease(revoked)
            .await
            .map_err(|_| AgentLiveHostError::BoundContextRevoked)
    }
}

#[cfg(desktop)]
fn prune_closed_lifecycles(state: &mut RemoteAgentLiveRpcState) {
    // A lifecycle may leave the registry only after its exact native
    // cancel/unsubscribe has acknowledged. Errors remain fail-closed and keep
    // stable occupancy rather than allowing a second overlapping subscriber.
    state
        .pending
        .retain(|_, pending| !pending.cancellation.completed_successfully());
    state
        .activating
        .retain(|_, known| !known.cancellation.completed_successfully());
    state
        .active
        .retain(|_, known| !known.cancellation.completed_successfully());
}

#[cfg(desktop)]
fn lifecycle_id_in_use(state: &RemoteAgentLiveRpcState, id: &str) -> bool {
    state.pending.contains_key(id)
        || state.activating.contains_key(id)
        || state
            .activating
            .values()
            .any(|known| known.public_live_stream_id.as_deref() == Some(id))
        || state.active.contains_key(id)
        || state.active.values().any(|known| known.activation_id == id)
}

#[cfg(desktop)]
fn live_lifecycle_count(state: &RemoteAgentLiveRpcState) -> usize {
    state.pending.len() + state.activating.len() + state.active.len()
}

#[cfg(desktop)]
fn stable_occupancy_in_use(
    state: &RemoteAgentLiveRpcState,
    authority: &VerifiedIncomingPeerAuthorization,
) -> bool {
    state
        .pending
        .values()
        .any(|known| same_live_occupancy(&known.authority, authority))
        || state
            .activating
            .values()
            .any(|known| same_live_occupancy(&known.authority, authority))
        || state
            .active
            .values()
            .any(|known| same_live_occupancy(&known.authority, authority))
}

#[cfg(desktop)]
fn same_live_occupancy(
    left: &VerifiedIncomingPeerAuthorization,
    right: &VerifiedIncomingPeerAuthorization,
) -> bool {
    left.same_admission_instance(right)
        && left.authorization().account_epoch() == right.authorization().account_epoch()
        && left.controller_endpoint() == right.controller_endpoint()
        && left.execution_target_id() == right.execution_target_id()
}

#[cfg(desktop)]
fn same_remote_authority(
    left: &VerifiedIncomingPeerAuthorization,
    right: &VerifiedIncomingPeerAuthorization,
) -> bool {
    left.same_admission_instance(right)
        && left.authorization() == right.authorization()
        && left.controller_endpoint() == right.controller_endpoint()
        && left.execution_target_id() == right.execution_target_id()
        && left.pairing_fence() == right.pairing_fence()
        && left.connection_stamp() == right.connection_stamp()
}

#[cfg(desktop)]
fn stale_live_lease() -> ProtocolError {
    ProtocolError::new(
        ErrorCode::Revoked,
        "remote Agent live lifecycle belongs to another authorization",
        false,
    )
}

#[cfg(desktop)]
fn live_lifecycle_state_error() -> ProtocolError {
    ProtocolError::new(
        ErrorCode::Internal,
        "remote Agent live lifecycle state is unavailable",
        false,
    )
}

#[cfg(desktop)]
fn live_lifecycle_unavailable(message: &'static str) -> ProtocolError {
    ProtocolError::new(ErrorCode::AgentLiveUnavailable, message, true)
}

pub async fn get_remote_agent_sessions_page(
    manager: &GenerationConnectionManager,
    request_id: &str,
    body: ListAgentSessionsRequest,
) -> Result<ListAgentSessionsResponse, ProtocolError> {
    let peer = manager.current()?.ok_or_else(|| {
        ProtocolError::new(
            ErrorCode::TransportUnavailable,
            "remote execution target has no current connection",
            true,
        )
    })?;
    let request = RequestEnvelope {
        protocol_version: PROTOCOL_VERSION,
        request_id: request_id.into(),
        execution_target_id: peer.execution_target_id().into(),
        direction: PeerDirection::ControllerToHost,
        connection_stamp: peer.connection_stamp(),
        body,
    };
    let response: ResponseEnvelope<ListAgentSessionsResponse> = peer.request(&request).await?;
    response.result
}

/// Fetch one native-record-count page over the Bulk lane. The response uses a
/// typed multi-frame sequence so aggregate bytes never redefine the requested
/// record count.
pub async fn get_remote_agent_history_page(
    manager: &GenerationConnectionManager,
    request_id: &str,
    body: ListAgentHistoryRecordsRequest,
) -> Result<RemoteAgentHistoryPage, ProtocolError> {
    let peer = manager.current()?.ok_or_else(|| {
        ProtocolError::new(
            ErrorCode::TransportUnavailable,
            "remote execution target has no current connection",
            true,
        )
    })?;
    get_remote_agent_history_page_on_peer(&peer, request_id, body).await
}

pub struct RemoteAgentLiveEventStream {
    peer: ConnectedPeer,
    response: StreamingResponse<BeginAgentLiveAttachRequest>,
    attach_id: String,
    last_cursor: RemoteLiveEventCursor,
    replay_through: Option<RemoteLiveEventCursor>,
    live_stream_id: Option<String>,
    replay_complete: bool,
}

pub struct ResumedRemoteAgentLiveEventStream {
    peer: ConnectedPeer,
    response: StreamingResponse<ResumeAgentLiveEventsRequest>,
    last_cursor: RemoteLiveEventCursor,
    replay_through: RemoteLiveEventCursor,
    live_stream_id: String,
    replay_complete: bool,
}

pub async fn begin_remote_agent_live_attach(
    manager: &GenerationConnectionManager,
    request_id: &str,
    body: BeginAgentLiveAttachRequest,
) -> Result<(RemoteAgentLiveHeadSnapshot, RemoteAgentLiveEventStream), ProtocolError> {
    let peer = manager.current()?.ok_or_else(|| {
        ProtocolError::new(
            ErrorCode::TransportUnavailable,
            "remote execution target has no current connection",
            true,
        )
    })?;
    let request = RequestEnvelope {
        protocol_version: PROTOCOL_VERSION,
        request_id: request_id.into(),
        execution_target_id: peer.execution_target_id().into(),
        direction: PeerDirection::ControllerToHost,
        connection_stamp: peer.connection_stamp(),
        body,
    };
    let mut response = peer.start_streaming_request(request).await?;
    let start: ResponseEnvelope<AgentLiveStreamFrame> = response.read().await?;
    let (attach_id, record_count, live_session_count, through_event_cursor) = match start.result? {
        AgentLiveStreamFrame::SnapshotStart {
            attach_id,
            record_count,
            live_session_count,
            live_sessions_complete: true,
            through_event_cursor,
        } => (
            attach_id,
            record_count,
            live_session_count,
            through_event_cursor,
        ),
        _ => {
            return Err(ProtocolError::new(
                ErrorCode::InvalidFrame,
                "remote Agent live attach did not start with a complete snapshot",
                false,
            ));
        }
    };
    let mut records = Vec::with_capacity(usize::from(record_count));
    for expected_index in 0..record_count {
        let frame: ResponseEnvelope<AgentLiveStreamFrame> = response.read().await?;
        match frame.result? {
            AgentLiveStreamFrame::HistoryRecord { index, record } if index == expected_index => {
                records.push(record);
            }
            _ => {
                return Err(ProtocolError::new(
                    ErrorCode::InvalidFrame,
                    "remote Agent live history records are discontinuous",
                    false,
                ));
            }
        }
    }
    let mut live_sessions = Vec::with_capacity(usize::from(live_session_count));
    let mut account_live_items = 0usize;
    let mut account_live_projection_bytes = LIVE_PROJECTION_OUTER_OVERHEAD_BYTES;
    let mut previous_session_id: Option<String> = None;
    for expected_index in 0..live_session_count {
        let frame: ResponseEnvelope<AgentLiveStreamFrame> = response.read().await?;
        let (session_id, item_count) = match frame.result? {
            AgentLiveStreamFrame::LiveSessionStart {
                index,
                session_id,
                item_count,
            } if index == expected_index
                && previous_session_id
                    .as_deref()
                    .is_none_or(|previous| previous < session_id.as_str()) =>
            {
                (session_id, item_count)
            }
            _ => {
                return Err(invalid_live_response(
                    "remote Agent live session snapshot is discontinuous",
                ));
            }
        };
        account_live_items = account_live_items
            .checked_add(usize::from(item_count))
            .ok_or_else(|| invalid_live_response("live snapshot item count overflow"))?;
        if account_live_items > crate::remote_protocol::MAX_LIVE_ITEMS_PER_ACCOUNT {
            return Err(invalid_live_response(
                "live account snapshot contains too many items",
            ));
        }
        accumulate_remote_live_projection_bytes(
            &mut account_live_projection_bytes,
            remote_live_projection_session_wire_bytes(&session_id)?,
        )?;
        let mut live_items = Vec::with_capacity(usize::from(item_count));
        for expected_item_index in 0..item_count {
            let frame: ResponseEnvelope<AgentLiveStreamFrame> = response.read().await?;
            match frame.result? {
                AgentLiveStreamFrame::LiveSessionItem {
                    session_index,
                    item_index,
                    item,
                } if session_index == expected_index && item_index == expected_item_index => {
                    accumulate_remote_live_projection_bytes(
                        &mut account_live_projection_bytes,
                        remote_live_projection_item_wire_bytes(&item)?,
                    )?;
                    live_items.push(item);
                }
                _ => {
                    return Err(invalid_live_response(
                        "remote Agent live session items are discontinuous",
                    ));
                }
            }
        }
        let snapshot = RemoteAgentLiveSessionSnapshot {
            session_id,
            live_items,
        };
        snapshot.validate()?;
        previous_session_id = Some(snapshot.session_id.clone());
        live_sessions.push(snapshot);
    }
    let footer: ResponseEnvelope<AgentLiveStreamFrame> = response.read().await?;
    let (next_cursor, history_revision) = match footer.result? {
        AgentLiveStreamFrame::SnapshotEnd {
            next_cursor,
            history_revision,
        } => (next_cursor, history_revision),
        _ => {
            return Err(invalid_live_response(
                "remote Agent live attach snapshot has no footer",
            ));
        }
    };
    if records.is_empty() && next_cursor.is_some() {
        return Err(invalid_live_response(
            "empty live history head cannot contain a continuation cursor",
        ));
    }
    let stream = RemoteAgentLiveEventStream {
        peer: peer.clone(),
        response,
        attach_id: attach_id.clone(),
        last_cursor: through_event_cursor.clone(),
        replay_through: None,
        live_stream_id: None,
        replay_complete: false,
    };
    Ok((
        RemoteAgentLiveHeadSnapshot {
            attach_id,
            records,
            next_cursor,
            history_revision,
            live_sessions,
            through_event_cursor,
            origin_host_epoch: peer.connection_stamp().host_epoch(),
        },
        stream,
    ))
}

fn accumulate_remote_live_projection_bytes(
    retained_bytes: &mut usize,
    additional_bytes: usize,
) -> Result<(), ProtocolError> {
    let next = retained_bytes
        .checked_add(additional_bytes)
        .ok_or_else(|| invalid_live_response("remote Agent live projection byte count overflow"))?;
    if next > MAX_LIVE_PROJECTION_BYTES_PER_ACCOUNT {
        return Err(invalid_live_response(
            "remote Agent live projection exceeds the account byte limit",
        ));
    }
    *retained_bytes = next;
    Ok(())
}

async fn request_remote_agent_live_activation(
    manager: &GenerationConnectionManager,
    request_id: &str,
    attach_id: &str,
) -> Result<ActivateAgentLiveAttachResponse, ProtocolError> {
    let peer = manager.current()?.ok_or_else(|| {
        ProtocolError::new(
            ErrorCode::TransportUnavailable,
            "remote execution target has no current connection",
            true,
        )
    })?;
    request_remote_agent_live_activation_on_peer(&peer, request_id, attach_id).await
}

async fn request_remote_agent_live_activation_on_peer(
    peer: &ConnectedPeer,
    request_id: &str,
    attach_id: &str,
) -> Result<ActivateAgentLiveAttachResponse, ProtocolError> {
    let request = RequestEnvelope {
        protocol_version: PROTOCOL_VERSION,
        request_id: request_id.into(),
        execution_target_id: peer.execution_target_id().into(),
        direction: PeerDirection::ControllerToHost,
        connection_stamp: peer.connection_stamp(),
        body: ActivateAgentLiveAttachRequest::new(attach_id)?,
    };
    let response: ResponseEnvelope<ActivateAgentLiveAttachResponse> =
        peer.request(&request).await?;
    response.result
}

/// Activate one paused C0 attachment while continuously draining its Events
/// stream. Every replay delivery is handed to `apply_replay` before the next
/// frame is read; the API therefore cannot aggregate a large replay or block
/// the host's bounded frame writer behind an unread Events stream.
///
/// This function takes ownership of the Events stream and returns it only
/// after the Control acknowledgement and Events replay barrier agree. If the
/// activation future is cancelled, dropping its owned receive half abandons
/// the response stream and wakes the host's acknowledged native cleanup path.
pub async fn activate_remote_agent_live_attach<F, Fut>(
    activation_request_id: &str,
    cancellation_request_id: &str,
    mut stream: RemoteAgentLiveEventStream,
    mut apply_replay: F,
) -> Result<(RemoteAgentLiveStreamStart, RemoteAgentLiveEventStream), ProtocolError>
where
    F: FnMut(RemoteAgentLiveDelivery) -> Fut,
    Fut: Future<Output = Result<(), ProtocolError>>,
{
    let attach_id = stream.attach_id.clone();
    let peer = stream.peer.clone();
    let result = tokio::try_join!(
        request_remote_agent_live_activation_on_peer(&peer, activation_request_id, &attach_id),
        stream.pump_activation(&mut apply_replay),
    );
    let (response, start) = match result {
        Ok(result) => result,
        Err(error) => {
            let (kind, live_id) = match stream.live_stream_id.as_deref() {
                Some(live_stream_id) => (AgentLiveCancelKind::ActiveStream, live_stream_id),
                None => (AgentLiveCancelKind::PendingAttach, attach_id.as_str()),
            };
            return match cancel_remote_agent_live_on_peer(
                &peer,
                cancellation_request_id,
                kind,
                live_id,
            )
            .await
            {
                Ok(()) => Err(error),
                Err(cleanup_error) => Err(cleanup_error),
            };
        }
    };
    match response.result {
        AgentLiveActivationDisposition::Activated {
            live_stream_id,
            through_event_cursor,
        } if live_stream_id == start.live_stream_id
            && through_event_cursor == start.through_event_cursor =>
        {
            Ok((start, stream))
        }
        AgentLiveActivationDisposition::SnapshotRequired {
            reason,
            last_event_cursor,
        } => {
            cancel_remote_agent_live_on_peer(
                &peer,
                cancellation_request_id,
                AgentLiveCancelKind::ActiveStream,
                &start.live_stream_id,
            )
            .await?;
            Err(snapshot_required_error(reason, &last_event_cursor))
        }
        AgentLiveActivationDisposition::Activated { .. } => {
            cancel_remote_agent_live_on_peer(
                &peer,
                cancellation_request_id,
                AgentLiveCancelKind::ActiveStream,
                &start.live_stream_id,
            )
            .await?;
            Err(invalid_live_response(
                "remote Agent live activation Control and Events barriers disagree",
            ))
        }
    }
}

impl RemoteAgentLiveEventStream {
    async fn read_start(&mut self) -> Result<RemoteAgentLiveStreamStart, ProtocolError> {
        if self.live_stream_id.is_some() {
            return Err(invalid_live_response(
                "remote Agent live stream already started",
            ));
        }
        let frame: ResponseEnvelope<AgentLiveStreamFrame> = self.response.read().await?;
        let (live_stream_id, from_event_cursor, through_event_cursor) = match frame.result? {
            AgentLiveStreamFrame::StreamStart {
                live_stream_id,
                from_event_cursor,
                through_event_cursor,
            } => (live_stream_id, from_event_cursor, through_event_cursor),
            AgentLiveStreamFrame::SnapshotRequired {
                reason,
                last_event_cursor,
            } => return Err(snapshot_required_error(reason, &last_event_cursor)),
            _ => {
                return Err(invalid_live_response(
                    "activated remote Agent live stream has no replay header",
                ));
            }
        };
        if from_event_cursor != self.last_cursor {
            return Err(invalid_live_response(
                "activated remote Agent live stream starts after the snapshot cursor",
            ));
        }
        self.replay_through = Some(through_event_cursor.clone());
        self.live_stream_id = Some(live_stream_id.clone());
        self.replay_complete = false;
        Ok(RemoteAgentLiveStreamStart {
            live_stream_id,
            from_event_cursor,
            through_event_cursor,
        })
    }

    async fn pump_activation<F, Fut>(
        &mut self,
        apply_replay: &mut F,
    ) -> Result<RemoteAgentLiveStreamStart, ProtocolError>
    where
        F: FnMut(RemoteAgentLiveDelivery) -> Fut,
        Fut: Future<Output = Result<(), ProtocolError>>,
    {
        let start = self.read_start().await?;
        loop {
            let frame: ResponseEnvelope<AgentLiveStreamFrame> = self.response.read().await?;
            match frame.result? {
                AgentLiveStreamFrame::Event { delivery } => {
                    validate_next_remote_delivery(
                        &self.last_cursor,
                        &delivery,
                        Some(&start.through_event_cursor),
                    )?;
                    self.last_cursor = delivery.cursor.clone();
                    apply_replay(delivery).await?;
                }
                AgentLiveStreamFrame::ReplayComplete {
                    through_event_cursor,
                } if through_event_cursor == start.through_event_cursor
                    && self.last_cursor == through_event_cursor =>
                {
                    self.replay_complete = true;
                    return Ok(start);
                }
                AgentLiveStreamFrame::SnapshotRequired {
                    reason,
                    last_event_cursor,
                } => return Err(snapshot_required_error(reason, &last_event_cursor)),
                _ => {
                    return Err(invalid_live_response(
                        "remote Agent live replay frame is out of order",
                    ))
                }
            }
        }
    }

    pub async fn recv(&mut self) -> Result<RemoteAgentLiveDelivery, ProtocolError> {
        let through = self
            .replay_through
            .clone()
            .ok_or_else(|| invalid_live_response("remote Agent live stream was not activated"))?;
        loop {
            let frame: ResponseEnvelope<AgentLiveStreamFrame> = self.response.read().await?;
            match frame.result? {
                AgentLiveStreamFrame::Event { delivery } => {
                    validate_next_remote_delivery(
                        &self.last_cursor,
                        &delivery,
                        (!self.replay_complete).then_some(&through),
                    )?;
                    self.last_cursor = delivery.cursor.clone();
                    return Ok(delivery);
                }
                AgentLiveStreamFrame::ReplayComplete {
                    through_event_cursor,
                } if !self.replay_complete
                    && through_event_cursor == through
                    && self.last_cursor == through_event_cursor =>
                {
                    self.replay_complete = true;
                }
                AgentLiveStreamFrame::SnapshotRequired {
                    reason,
                    last_event_cursor,
                } => return Err(snapshot_required_error(reason, &last_event_cursor)),
                _ => {
                    return Err(invalid_live_response(
                        "remote Agent live frame is out of order",
                    ))
                }
            }
        }
    }

    /// Cancel this stream on the exact connection generation which owns it.
    /// Consuming `self` also abandons the Events response if the Control
    /// cancellation cannot be delivered, waking host-side cleanup.
    pub async fn cancel(self, request_id: &str) -> Result<(), ProtocolError> {
        let live_stream_id = self
            .live_stream_id
            .as_deref()
            .ok_or_else(|| invalid_live_response("remote Agent live stream was not activated"))?;
        cancel_remote_agent_live_on_peer(
            &self.peer,
            request_id,
            AgentLiveCancelKind::ActiveStream,
            live_stream_id,
        )
        .await
    }

    pub async fn finish(self) -> Result<(), ProtocolError> {
        self.response.finish().await
    }
}

pub async fn resume_remote_agent_live_events(
    manager: &GenerationConnectionManager,
    request_id: &str,
    cursor: RemoteLiveEventCursor,
    origin_host_epoch: u64,
) -> Result<ResumedRemoteAgentLiveEventStream, ProtocolError> {
    let peer = manager.current()?.ok_or_else(|| {
        ProtocolError::new(
            ErrorCode::TransportUnavailable,
            "remote execution target has no current connection",
            true,
        )
    })?;
    let body = ResumeAgentLiveEventsRequest::new(cursor.clone(), origin_host_epoch)?;
    if let Err(error) = body.validate_for_connection_stamp(peer.connection_stamp()) {
        if error.code == ErrorCode::StaleGeneration {
            return Err(snapshot_required_error(
                RemoteAgentLiveSnapshotReason::OwnerChanged,
                &cursor,
            ));
        }
        return Err(error);
    }
    let request = RequestEnvelope {
        protocol_version: PROTOCOL_VERSION,
        request_id: request_id.into(),
        execution_target_id: peer.execution_target_id().into(),
        direction: PeerDirection::ControllerToHost,
        connection_stamp: peer.connection_stamp(),
        body,
    };
    let mut response = peer.start_streaming_request(request).await?;
    let start: ResponseEnvelope<AgentLiveStreamFrame> = response.read().await?;
    let (live_stream_id, from_event_cursor, through_event_cursor) = match start.result? {
        AgentLiveStreamFrame::StreamStart {
            live_stream_id,
            from_event_cursor,
            through_event_cursor,
        } => (live_stream_id, from_event_cursor, through_event_cursor),
        AgentLiveStreamFrame::SnapshotRequired {
            reason,
            last_event_cursor,
        } => return Err(snapshot_required_error(reason, &last_event_cursor)),
        _ => {
            return Err(invalid_live_response(
                "resumed remote Agent live stream has no replay header",
            ))
        }
    };
    if from_event_cursor != cursor {
        return Err(invalid_live_response(
            "resumed remote Agent live stream starts at another cursor",
        ));
    }
    Ok(ResumedRemoteAgentLiveEventStream {
        peer,
        response,
        last_cursor: cursor,
        replay_through: through_event_cursor,
        live_stream_id,
        replay_complete: false,
    })
}

impl ResumedRemoteAgentLiveEventStream {
    pub fn live_stream_id(&self) -> &str {
        &self.live_stream_id
    }

    pub async fn recv(&mut self) -> Result<RemoteAgentLiveDelivery, ProtocolError> {
        loop {
            let frame: ResponseEnvelope<AgentLiveStreamFrame> = self.response.read().await?;
            match frame.result? {
                AgentLiveStreamFrame::Event { delivery } => {
                    validate_next_remote_delivery(
                        &self.last_cursor,
                        &delivery,
                        (!self.replay_complete).then_some(&self.replay_through),
                    )?;
                    self.last_cursor = delivery.cursor.clone();
                    return Ok(delivery);
                }
                AgentLiveStreamFrame::ReplayComplete {
                    through_event_cursor,
                } if !self.replay_complete
                    && through_event_cursor == self.replay_through
                    && self.last_cursor == through_event_cursor =>
                {
                    self.replay_complete = true;
                }
                AgentLiveStreamFrame::SnapshotRequired {
                    reason,
                    last_event_cursor,
                } => return Err(snapshot_required_error(reason, &last_event_cursor)),
                _ => {
                    return Err(invalid_live_response(
                        "remote Agent live frame is out of order",
                    ))
                }
            }
        }
    }

    /// Cancel this resumed stream on its exact connection generation.
    pub async fn cancel(self, request_id: &str) -> Result<(), ProtocolError> {
        cancel_remote_agent_live_on_peer(
            &self.peer,
            request_id,
            AgentLiveCancelKind::ActiveStream,
            &self.live_stream_id,
        )
        .await
    }

    pub async fn finish(self) -> Result<(), ProtocolError> {
        self.response.finish().await
    }
}

pub async fn cancel_remote_agent_live(
    manager: &GenerationConnectionManager,
    request_id: &str,
    kind: AgentLiveCancelKind,
    live_id: &str,
) -> Result<(), ProtocolError> {
    let peer = manager.current()?.ok_or_else(|| {
        ProtocolError::new(
            ErrorCode::TransportUnavailable,
            "remote execution target has no current connection",
            true,
        )
    })?;
    cancel_remote_agent_live_on_peer(&peer, request_id, kind, live_id).await
}

async fn cancel_remote_agent_live_on_peer(
    peer: &ConnectedPeer,
    request_id: &str,
    kind: AgentLiveCancelKind,
    live_id: &str,
) -> Result<(), ProtocolError> {
    let request = RequestEnvelope {
        protocol_version: PROTOCOL_VERSION,
        request_id: request_id.into(),
        execution_target_id: peer.execution_target_id().into(),
        direction: PeerDirection::ControllerToHost,
        connection_stamp: peer.connection_stamp(),
        body: CancelAgentLiveRequest::new(kind, live_id)?,
    };
    let response: ResponseEnvelope<CancelAgentLiveResponse> = peer.request(&request).await?;
    response.result.map(|_| ())
}

async fn get_remote_agent_history_page_on_peer(
    peer: &ConnectedPeer,
    request_id: &str,
    body: ListAgentHistoryRecordsRequest,
) -> Result<RemoteAgentHistoryPage, ProtocolError> {
    let request = RequestEnvelope {
        protocol_version: PROTOCOL_VERSION,
        request_id: request_id.into(),
        execution_target_id: peer.execution_target_id().into(),
        direction: PeerDirection::ControllerToHost,
        connection_stamp: peer.connection_stamp(),
        body,
    };
    let mut response = peer.start_streaming_request(request).await?;
    let start: ResponseEnvelope<AgentHistoryPageFrame> = response.read().await?;
    let record_count = match start.result? {
        AgentHistoryPageFrame::Start { record_count } => record_count,
        _ => {
            return Err(ProtocolError::new(
                ErrorCode::InvalidPage,
                "history response did not start with a page header",
                false,
            ));
        }
    };
    let mut records = Vec::with_capacity(usize::from(record_count));
    for expected_index in 0..record_count {
        let frame: ResponseEnvelope<AgentHistoryPageFrame> = response.read().await?;
        match frame.result? {
            AgentHistoryPageFrame::Record { index, record } if index == expected_index => {
                records.push(record);
            }
            AgentHistoryPageFrame::Record { .. } => {
                return Err(ProtocolError::new(
                    ErrorCode::InvalidPage,
                    "history response record index is discontinuous",
                    false,
                ));
            }
            _ => {
                return Err(ProtocolError::new(
                    ErrorCode::InvalidPage,
                    "history response ended before every declared record",
                    false,
                ));
            }
        }
    }
    let footer: ResponseEnvelope<AgentHistoryPageFrame> = response.read().await?;
    let (next_cursor, history_revision) = match footer.result? {
        AgentHistoryPageFrame::End {
            next_cursor,
            history_revision,
        } => (next_cursor, history_revision),
        _ => {
            return Err(ProtocolError::new(
                ErrorCode::InvalidPage,
                "history response did not end with a page footer",
                false,
            ));
        }
    };
    response.finish().await?;
    if records.is_empty() && next_cursor.is_some() {
        return Err(ProtocolError::new(
            ErrorCode::InvalidPage,
            "empty history page cannot contain a continuation cursor",
            false,
        ));
    }
    Ok(RemoteAgentHistoryPage {
        records,
        next_cursor,
        history_revision,
    })
}

/// Query status through the manager's exact current pairing/generation.
///
/// A manager that is stale, has no live generation, or is in an ambiguous
/// handover state fails before an application stream is opened. Target ID and
/// connection stamp are always copied from the authenticated peer rather than
/// accepted as caller-controlled strings.
pub async fn get_remote_runtime_status(
    manager: &GenerationConnectionManager,
    request_id: &str,
) -> Result<RemoteAgentRuntimeStatus, ProtocolError> {
    let peer = manager.current()?.ok_or_else(|| {
        ProtocolError::new(
            ErrorCode::TransportUnavailable,
            "remote execution target has no current connection",
            true,
        )
    })?;
    get_remote_runtime_status_on_peer(&peer, request_id).await
}

async fn get_remote_runtime_status_on_peer(
    peer: &ConnectedPeer,
    request_id: &str,
) -> Result<RemoteAgentRuntimeStatus, ProtocolError> {
    let request = RequestEnvelope {
        protocol_version: PROTOCOL_VERSION,
        request_id: request_id.into(),
        execution_target_id: peer.execution_target_id().into(),
        direction: PeerDirection::ControllerToHost,
        connection_stamp: peer.connection_stamp(),
        body: GetRuntimeStatusRequest::new(),
    };
    let response: ResponseEnvelope<GetRuntimeStatusResponse> = peer.request(&request).await?;
    Ok(response.result?.status)
}

/// Accept and serve exactly one typed runtime-status request.
///
/// The provider work shares the stream's absolute operation deadline and is
/// dropped immediately if the controller abandons its response stream. No
/// operation name reaches Tauri or Maple's desktop command table.
#[cfg(test)]
pub async fn serve_next_remote_runtime_status<P>(
    host_endpoint: &MapleIrohEndpoint,
    peer: &ConnectedPeer,
    provider: &P,
) -> Result<(), ProtocolError>
where
    P: RemoteRuntimeStatusProvider + ?Sized,
{
    serve_next_remote_runtime_status_with_timeout(
        host_endpoint,
        peer,
        provider,
        RUNTIME_STATUS_PROVIDER_TIMEOUT,
    )
    .await
}

#[cfg(test)]
async fn serve_next_remote_runtime_status_with_timeout<P>(
    host_endpoint: &MapleIrohEndpoint,
    peer: &ConnectedPeer,
    provider: &P,
    provider_timeout: Duration,
) -> Result<(), ProtocolError>
where
    P: RemoteRuntimeStatusProvider + ?Sized,
{
    if provider_timeout.is_zero() {
        return Err(ProtocolError::new(
            ErrorCode::Internal,
            "runtime status provider timeout is invalid",
            false,
        ));
    }
    host_endpoint.validate_current_incoming_peer(peer)?;
    let accepted = peer.accept_stream().await?;
    let request: AcceptedRequest<GetRuntimeStatusRequest> = accepted.read_request().await?;
    serve_remote_runtime_status_request(host_endpoint, peer, provider, request, provider_timeout)
        .await
}

async fn serve_remote_runtime_status_request<T, P>(
    host_endpoint: &MapleIrohEndpoint,
    peer: &ConnectedPeer,
    provider: &P,
    request: AcceptedRequest<T>,
    provider_timeout: Duration,
) -> Result<(), ProtocolError>
where
    T: crate::remote_protocol::RequestBody,
    GetRuntimeStatusResponse: crate::remote_protocol::ResponseBody<T>,
    P: RemoteRuntimeStatusProvider + ?Sized,
{
    if provider_timeout.is_zero() {
        return Err(ProtocolError::new(
            ErrorCode::Internal,
            "runtime status provider timeout is invalid",
            false,
        ));
    }
    host_endpoint.validate_current_incoming_peer(peer)?;
    let operation_deadline = request.operation_deadline();
    let now = tokio::time::Instant::now();
    let latest_provider_deadline = operation_deadline
        .checked_sub(RUNTIME_STATUS_RESPONSE_BUDGET)
        .unwrap_or(now);
    let provider_deadline = latest_provider_deadline.min(now + provider_timeout);
    let mut response_cancelled = Box::pin(request.response_cancelled());
    let mut provider_result = provider.runtime_status();

    let result = tokio::select! {
        biased;
        cancelled = &mut response_cancelled => {
            return match cancelled {
                Ok(()) => Err(ProtocolError::new(
                    ErrorCode::TransportUnavailable,
                    "remote runtime status request was cancelled",
                    true,
                )),
                Err(error) => Err(error),
            };
        }
        provider_result = tokio::time::timeout_at(provider_deadline, provider_result.as_mut()) => {
            match provider_result {
                Ok(Ok(status)) => GetRuntimeStatusResponse::new(status),
                Ok(Err(error)) => Err(error),
                Err(_) => Err(ProtocolError::new(
                    ErrorCode::TransportUnavailable,
                    "remote runtime status provider deadline elapsed",
                    true,
                )),
            }
        }
    };
    // The provider may retain account-scoped resources. Drop it before any
    // potentially slow network response write, including the timeout path.
    drop(provider_result);
    drop(response_cancelled);
    host_endpoint.validate_current_incoming_peer(peer)?;

    let envelope = request.request();
    let response = ResponseEnvelope {
        protocol_version: PROTOCOL_VERSION,
        request_id: envelope.request_id.clone(),
        execution_target_id: envelope.execution_target_id.clone(),
        connection_stamp: envelope.connection_stamp,
        result,
    };
    request.write_response(&response).await
}

#[cfg(test)]
pub async fn serve_next_remote_agent_sessions_page<P>(
    host_endpoint: &MapleIrohEndpoint,
    peer: &ConnectedPeer,
    provider: &P,
) -> Result<(), ProtocolError>
where
    P: RemoteAgentSessionListProvider + ?Sized,
{
    host_endpoint.validate_current_incoming_peer(peer)?;
    let accepted = peer.accept_stream().await?;
    let request: AcceptedRequest<ListAgentSessionsRequest> = accepted.read_request().await?;
    let body = request.request().body.clone();
    serve_remote_agent_sessions_page_request(host_endpoint, peer, provider, request, body).await
}

async fn serve_remote_agent_sessions_page_request<T, P>(
    host_endpoint: &MapleIrohEndpoint,
    peer: &ConnectedPeer,
    provider: &P,
    request: AcceptedRequest<T>,
    body: ListAgentSessionsRequest,
) -> Result<(), ProtocolError>
where
    T: crate::remote_protocol::RequestBody,
    ListAgentSessionsResponse: crate::remote_protocol::ResponseBody<T>,
    P: RemoteAgentSessionListProvider + ?Sized,
{
    host_endpoint.validate_current_incoming_peer(peer)?;
    let operation_deadline = request.operation_deadline();
    let now = tokio::time::Instant::now();
    let provider_deadline = operation_deadline
        .checked_sub(AGENT_HISTORY_RESPONSE_BUDGET)
        .unwrap_or(now)
        .min(now + AGENT_HISTORY_PROVIDER_TIMEOUT);
    let mut response_cancelled = Box::pin(request.response_cancelled());
    let mut provider_result = provider.list_agent_sessions(&body);
    let result = tokio::select! {
        biased;
        cancelled = &mut response_cancelled => {
            return match cancelled {
                Ok(()) => Err(ProtocolError::new(
                    ErrorCode::TransportUnavailable,
                    "remote Agent task-page request was cancelled",
                    true,
                )),
                Err(error) => Err(error),
            };
        }
        provider_result = tokio::time::timeout_at(provider_deadline, provider_result.as_mut()) => {
            match provider_result {
                Ok(result) => result,
                Err(_) => Err(ProtocolError::new(
                    ErrorCode::TransportUnavailable,
                    "remote Agent task-page provider deadline elapsed",
                    true,
                )),
            }
        }
    };
    drop(provider_result);
    drop(response_cancelled);
    host_endpoint.validate_current_incoming_peer(peer)?;
    let envelope = request.request();
    let response = ResponseEnvelope {
        protocol_version: PROTOCOL_VERSION,
        request_id: envelope.request_id.clone(),
        execution_target_id: envelope.execution_target_id.clone(),
        connection_stamp: envelope.connection_stamp,
        result,
    };
    request.write_response(&response).await
}

/// Complete authority set consumed by the one peer-wide request dispatcher.
/// Each field is transport-neutral and may retain only the account/runtime
/// authority deliberately injected by the native host.
#[cfg(desktop)]
#[derive(Clone)]
pub(crate) struct RemoteAgentRpcServer {
    runtime: Arc<dyn RemoteRuntimeStatusProvider>,
    history: Arc<dyn RemoteAgentHistoryProvider>,
    sessions: Arc<dyn RemoteAgentSessionListProvider>,
    live: RemoteAgentLiveRpcHost,
}

#[cfg(desktop)]
impl RemoteAgentRpcServer {
    pub(crate) fn new(
        runtime: Arc<dyn RemoteRuntimeStatusProvider>,
        history: Arc<dyn RemoteAgentHistoryProvider>,
        sessions: Arc<dyn RemoteAgentSessionListProvider>,
        live: RemoteAgentLiveRpcHost,
    ) -> Self {
        Self {
            runtime,
            history,
            sessions,
            live,
        }
    }
}

/// Awaitable owner for one dispatched request worker.
///
/// The inner Tokio abort handle is deliberately not exposed. Dropping this
/// value (including cancellation of a task awaiting it) detaches the worker
/// instead of aborting it, so a live worker continues through its remote
/// STOP/peer-close/revocation path and awaits native cancel/unsubscribe before
/// releasing occupancy. Runtime shutdown is process-terminal and is not an
/// in-process lifecycle reuse boundary.
#[cfg(desktop)]
#[must_use = "remote Agent request workers must be awaited or deliberately detached"]
pub(crate) struct RemoteAgentRpcWorker {
    task: tokio::task::JoinHandle<Result<(), ProtocolError>>,
}

#[cfg(desktop)]
impl RemoteAgentRpcWorker {
    fn spawn(worker: impl Future<Output = Result<(), ProtocolError>> + Send + 'static) -> Self {
        Self {
            task: tokio::spawn(worker),
        }
    }
}

#[cfg(desktop)]
impl Future for RemoteAgentRpcWorker {
    type Output = Result<Result<(), ProtocolError>, tokio::task::JoinError>;

    fn poll(
        self: Pin<&mut Self>,
        context: &mut std::task::Context<'_>,
    ) -> std::task::Poll<Self::Output> {
        Pin::new(&mut self.get_mut().task).poll(context)
    }
}

/// Accept, authenticate, and decode exactly one request, then start its owned
/// worker. This is the only production entry point which dequeues a prepared
/// application stream. Callers may invoke it serially in their connection
/// loop; the returned task lets long-lived Events work continue while the loop
/// accepts the corresponding Control operation.
#[cfg(desktop)]
pub(crate) async fn serve_next_remote_agent_request(
    host_endpoint: Arc<MapleIrohEndpoint>,
    peer: ConnectedPeer,
    server: RemoteAgentRpcServer,
) -> Result<RemoteAgentRpcWorker, ProtocolError> {
    host_endpoint.validate_current_incoming_peer(&peer)?;
    let accepted = peer.accept_stream().await?;
    let lane = accepted.header().stream_kind;
    match lane {
        crate::remote_protocol::StreamKind::Control => {
            let request: AcceptedRequest<RemoteAgentControlRequest> =
                accepted.read_request().await?;
            host_endpoint.validate_current_incoming_peer(&peer)?;
            Ok(RemoteAgentRpcWorker::spawn(async move {
                match request.request().body.clone() {
                    RemoteAgentControlRequest::GetRuntimeStatus => {
                        serve_remote_runtime_status_request(
                            host_endpoint.as_ref(),
                            &peer,
                            server.runtime.as_ref(),
                            request,
                            RUNTIME_STATUS_PROVIDER_TIMEOUT,
                        )
                        .await
                    }
                    RemoteAgentControlRequest::ActivateAttach { attach_id } => {
                        let authority =
                            host_endpoint.verified_incoming_peer_authorization(&peer)?;
                        let body = ActivateAgentLiveAttachRequest::new(attach_id)?;
                        remote_live_server::serve_remote_agent_live_activation(
                            host_endpoint.as_ref(),
                            &peer,
                            &server.live,
                            authority,
                            request,
                            body,
                        )
                        .await
                    }
                    RemoteAgentControlRequest::Cancel { kind, live_id } => {
                        let authority =
                            host_endpoint.verified_incoming_peer_authorization(&peer)?;
                        let body = CancelAgentLiveRequest::new(kind, live_id)?;
                        remote_live_server::serve_remote_agent_live_cancel(
                            host_endpoint.as_ref(),
                            &peer,
                            &server.live,
                            authority,
                            request,
                            body,
                        )
                        .await
                    }
                }
            }))
        }
        crate::remote_protocol::StreamKind::Events => {
            let request: AcceptedRequest<RemoteAgentLiveEventsRequest> =
                accepted.read_request().await?;
            let authority = host_endpoint.verified_incoming_peer_authorization(&peer)?;
            Ok(RemoteAgentRpcWorker::spawn(async move {
                match request.request().body.clone() {
                    RemoteAgentLiveEventsRequest::BeginAttach { session_id, limit } => {
                        let body = BeginAgentLiveAttachRequest::new(session_id, limit)?;
                        remote_live_server::serve_remote_agent_live_begin(
                            host_endpoint.as_ref(),
                            &peer,
                            &server.live,
                            authority,
                            request,
                            body,
                        )
                        .await
                    }
                    RemoteAgentLiveEventsRequest::Resume {
                        cursor,
                        origin_host_epoch,
                    } => {
                        let body = ResumeAgentLiveEventsRequest::new(cursor, origin_host_epoch)?;
                        remote_live_server::serve_remote_agent_live_resume(
                            host_endpoint.as_ref(),
                            &peer,
                            &server.live,
                            authority,
                            request,
                            body,
                        )
                        .await
                    }
                }
            }))
        }
        crate::remote_protocol::StreamKind::Bulk => {
            let request: AcceptedRequest<RemoteAgentBulkRequest> = accepted.read_request().await?;
            host_endpoint.validate_current_incoming_peer(&peer)?;
            Ok(RemoteAgentRpcWorker::spawn(async move {
                match request.request().body.clone() {
                    RemoteAgentBulkRequest::ListSessionRecords {
                        session_id,
                        cursor,
                        limit,
                    } => {
                        let body = ListAgentHistoryRecordsRequest::new(session_id, cursor, limit)?;
                        serve_remote_agent_history_page_request(
                            host_endpoint.as_ref(),
                            &peer,
                            server.history.as_ref(),
                            request,
                            body,
                            AGENT_HISTORY_PROVIDER_TIMEOUT,
                        )
                        .await
                    }
                    RemoteAgentBulkRequest::ListSessions {
                        project_root,
                        cursor,
                        limit,
                    } => {
                        let body = ListAgentSessionsRequest {
                            operation:
                                crate::remote_protocol::AgentSessionListOperation::ListSessions,
                            project_root,
                            cursor,
                            limit,
                        };
                        serve_remote_agent_sessions_page_request(
                            host_endpoint.as_ref(),
                            &peer,
                            server.sessions.as_ref(),
                            request,
                            body,
                        )
                        .await
                    }
                }
            }))
        }
    }
}

#[cfg(desktop)]
mod remote_live_server {
    use super::*;

    pub(super) enum PendingActivationResult {
        Activated(crate::agent_live_host::AgentLiveRemoteActivated),
        NativeError(AgentLiveRemoteAttachError),
        Interrupted(ProtocolError),
    }

    /// Poll native activation until it finishes or the request owner is
    /// interrupted. The activation future is scoped entirely inside this
    /// helper, so it is dropped before the caller can invoke `pending.cancel()`.
    /// That ordering is the cancellation-safety boundary promised by
    /// `AgentLiveRemotePendingAttach`.
    pub(super) async fn activate_pending_until_interrupted<F>(
        pending: &mut dyn AgentLiveRemotePendingAttach,
        interruption: F,
    ) -> PendingActivationResult
    where
        F: Future<Output = ProtocolError>,
    {
        let mut activate = Box::pin(pending.activate());
        tokio::select! {
            biased;
            error = interruption => PendingActivationResult::Interrupted(error),
            result = &mut activate => match result {
                Ok(activated) => PendingActivationResult::Activated(activated),
                Err(error) => PendingActivationResult::NativeError(error),
            },
        }
    }

    /// Encode each absolute C0 session as one bounded header followed by one
    /// frame per already-bounded presentation item. Aggregate native overlay
    /// size may exceed the transport frame cap; no individual frame may.
    pub(super) fn append_live_session_snapshot_frames(
        frames: &mut Vec<AgentLiveStreamFrame>,
        live_sessions: Vec<RemoteAgentLiveSessionSnapshot>,
    ) -> Result<(), ProtocolError> {
        for (session_index, snapshot) in live_sessions.into_iter().enumerate() {
            snapshot.validate()?;
            let session_index = u16::try_from(session_index)
                .map_err(|_| invalid_live_response("live session index is too large"))?;
            let item_count = u16::try_from(snapshot.live_items.len())
                .map_err(|_| invalid_live_response("live session item count is too large"))?;
            frames.push(AgentLiveStreamFrame::LiveSessionStart {
                index: session_index,
                session_id: snapshot.session_id,
                item_count,
            });
            frames.extend(
                snapshot
                    .live_items
                    .into_iter()
                    .enumerate()
                    .map(|(item_index, item)| AgentLiveStreamFrame::LiveSessionItem {
                        session_index,
                        item_index: u16::try_from(item_index)
                            .expect("validated live session item index fits u16"),
                        item,
                    }),
            );
        }
        Ok(())
    }

    /// Accept exactly one remote Agent live request. This is the sole owner of
    /// the peer's prepared-stream queue for the live surface: it selects the
    /// lane from the already-authenticated stream header and only then decodes
    /// that lane's closed tagged operation union. Events and Control handlers
    /// can therefore never steal each other's streams.
    #[cfg(test)]
    async fn serve_next_remote_agent_live_request_for_test(
        host_endpoint: &MapleIrohEndpoint,
        peer: &ConnectedPeer,
        rpc: &RemoteAgentLiveRpcHost,
    ) -> Result<(), ProtocolError> {
        serve_next_remote_agent_live_request_for_test_with_ttl(
            host_endpoint,
            peer,
            rpc,
            REMOTE_LIVE_PENDING_TTL,
        )
        .await
    }

    #[cfg(test)]
    pub(super) async fn serve_next_remote_agent_live_request_for_test_with_ttl(
        host_endpoint: &MapleIrohEndpoint,
        peer: &ConnectedPeer,
        rpc: &RemoteAgentLiveRpcHost,
        pending_ttl: Duration,
    ) -> Result<(), ProtocolError> {
        host_endpoint.validate_current_incoming_peer(peer)?;
        let authority = host_endpoint.verified_incoming_peer_authorization(peer)?;
        let accepted = peer.accept_stream().await?;
        match accepted.header().stream_kind {
            crate::remote_protocol::StreamKind::Events => {
                let request: AcceptedRequest<RemoteAgentLiveEventsRequest> =
                    accepted.read_request().await?;
                revalidate_live_authority(host_endpoint, peer, &authority)?;
                match request.request().body.clone() {
                    RemoteAgentLiveEventsRequest::BeginAttach { session_id, limit } => {
                        let body = BeginAgentLiveAttachRequest::new(session_id, limit)?;
                        serve_remote_agent_live_begin_with_ttl(
                            host_endpoint,
                            peer,
                            rpc,
                            authority,
                            request,
                            body,
                            pending_ttl,
                        )
                        .await
                    }
                    RemoteAgentLiveEventsRequest::Resume {
                        cursor,
                        origin_host_epoch,
                    } => {
                        let body = ResumeAgentLiveEventsRequest::new(cursor, origin_host_epoch)?;
                        serve_remote_agent_live_resume(
                            host_endpoint,
                            peer,
                            rpc,
                            authority,
                            request,
                            body,
                        )
                        .await
                    }
                }
            }
            crate::remote_protocol::StreamKind::Control => {
                let request: AcceptedRequest<RemoteAgentLiveControlRequest> =
                    accepted.read_request().await?;
                revalidate_live_authority(host_endpoint, peer, &authority)?;
                match request.request().body.clone() {
                    RemoteAgentLiveControlRequest::ActivateAttach { attach_id } => {
                        let body = ActivateAgentLiveAttachRequest::new(attach_id)?;
                        serve_remote_agent_live_activation(
                            host_endpoint,
                            peer,
                            rpc,
                            authority,
                            request,
                            body,
                        )
                        .await
                    }
                    RemoteAgentLiveControlRequest::Cancel { kind, live_id } => {
                        let body = CancelAgentLiveRequest::new(kind, live_id)?;
                        serve_remote_agent_live_cancel(
                            host_endpoint,
                            peer,
                            rpc,
                            authority,
                            request,
                            body,
                        )
                        .await
                    }
                }
            }
            crate::remote_protocol::StreamKind::Bulk => Err(ProtocolError::new(
                ErrorCode::InvalidFrame,
                "bulk stream is not a remote Agent live operation",
                false,
            )),
        }
    }

    pub(super) async fn serve_remote_agent_live_begin(
        host_endpoint: &MapleIrohEndpoint,
        peer: &ConnectedPeer,
        rpc: &RemoteAgentLiveRpcHost,
        authority: VerifiedIncomingPeerAuthorization,
        request: AcceptedRequest<RemoteAgentLiveEventsRequest>,
        body: BeginAgentLiveAttachRequest,
    ) -> Result<(), ProtocolError> {
        serve_remote_agent_live_begin_with_ttl(
            host_endpoint,
            peer,
            rpc,
            authority,
            request,
            body,
            REMOTE_LIVE_PENDING_TTL,
        )
        .await
    }

    async fn serve_remote_agent_live_begin_with_ttl(
        host_endpoint: &MapleIrohEndpoint,
        peer: &ConnectedPeer,
        rpc: &RemoteAgentLiveRpcHost,
        authority: VerifiedIncomingPeerAuthorization,
        mut request: AcceptedRequest<RemoteAgentLiveEventsRequest>,
        body: BeginAgentLiveAttachRequest,
        pending_ttl: Duration,
    ) -> Result<(), ProtocolError> {
        if pending_ttl.is_zero() {
            return Err(live_lifecycle_state_error());
        }
        let mut response_cancelled = Box::pin(request.response_cancelled());
        let mut peer_closed = Box::pin(peer.wait_closed());
        let service = tokio::select! {
                biased;
                cancelled = &mut response_cancelled => return Err(cancelled_live_request(cancelled)),
                _ = &mut peer_closed => return Err(live_connection_closed()),
                result = rpc.bind_service(&authority) => match result {
                    Ok(service) => service,
                    Err(error) => {
                        revalidate_live_authority(host_endpoint, peer, &authority)?;
                        let response = live_response_envelope(&request, Err(error));
                        return request.write_response(&response).await;
                }
            },
        };
        let attach_id = allocate_live_id().await?;
        let cancellation = Arc::new(RemoteAgentLiveCancellation::default());
        let (activate, activate_receive) = oneshot::channel();
        if let Err(error) = rpc
            .install_pending(
                attach_id.clone(),
                authority.clone(),
                Arc::clone(&service),
                activate,
                Arc::clone(&cancellation),
            )
            .await
        {
            revalidate_live_authority(host_endpoint, peer, &authority)?;
            let response = live_response_envelope(&request, Err(error));
            return request.write_response(&response).await;
        }
        let head = tokio::select! {
        biased;
        _ = cancellation.wait_requested() => {
            rpc.remove_pending(&attach_id, &authority).await;
            cancellation.complete(Ok(()));
            return Err(live_lifecycle_unavailable(
                "remote Agent live attachment was cancelled before snapshot",
            ));
        }
        cancelled = &mut response_cancelled => {
            rpc.remove_pending(&attach_id, &authority).await;
            cancellation.complete(Ok(()));
            return Err(cancelled_live_request(cancelled));
        }
        _ = &mut peer_closed => {
            rpc.remove_pending(&attach_id, &authority).await;
            cancellation.complete(Ok(()));
            return Err(live_connection_closed());
        }
        result = service.begin_newest(
                AgentHistoryPageRequest {
                    session_id: body.session_id,
                    cursor: None,
                    limit: Some(usize::from(body.limit)),
                },
                Some(REMOTE_LIVE_SUBSCRIPTION_CAPACITY),
        ) => match result {
            Ok(head) => head,
            Err(error) => {
                rpc.remove_pending(&attach_id, &authority).await;
                cancellation.complete(Ok(()));
                revalidate_live_authority(host_endpoint, peer, &authority)?;
                    let response = live_response_envelope(
                        &request,
                        Err(map_live_attach_error(error)),
                    );
                    return request.write_response(&response).await;
                }
            },
        };
        let AgentLiveRemoteHeadBegin {
            page,
            through_event_cursor,
            live_sessions_complete,
            live_sessions,
            pending,
        } = head;

        let prepared =
            (|| -> Result<(Vec<AgentLiveStreamFrame>, RemoteLiveEventCursor), ProtocolError> {
                let through_event_cursor = remote_cursor(through_event_cursor)?;
                let records = page
                    .records
                    .into_iter()
                    .map(remote_safe_history_record)
                    .collect::<Result<Vec<_>, _>>()?;
                let live_sessions = live_sessions
                    .into_iter()
                    .map(|session| {
                        Ok(RemoteAgentLiveSessionSnapshot {
                            session_id: session.session_id,
                            live_items: session
                                .live_items
                                .into_iter()
                                .map(remote_live_timeline_item)
                                .collect::<Result<Vec<_>, ProtocolError>>()?,
                        })
                    })
                    .collect::<Result<Vec<_>, ProtocolError>>()?;
                validate_remote_live_snapshot(live_sessions_complete, &live_sessions)?;
                let record_count = u16::try_from(records.len())
                    .map_err(|_| invalid_live_response("live history head is too large"))?;
                let live_session_count = u16::try_from(live_sessions.len())
                    .map_err(|_| invalid_live_response("live account snapshot is too large"))?;
                let mut frames = Vec::with_capacity(records.len() + live_sessions.len() + 2);
                frames.push(AgentLiveStreamFrame::SnapshotStart {
                    attach_id: attach_id.clone(),
                    record_count,
                    live_session_count,
                    live_sessions_complete,
                    through_event_cursor: through_event_cursor.clone(),
                });
                frames.extend(records.into_iter().enumerate().map(|(index, record)| {
                    AgentLiveStreamFrame::HistoryRecord {
                        index: u16::try_from(index).expect("validated live record index fits u16"),
                        record,
                    }
                }));
                append_live_session_snapshot_frames(&mut frames, live_sessions)?;
                frames.push(AgentLiveStreamFrame::SnapshotEnd {
                    next_cursor: page.next_cursor,
                    history_revision: page.history_revision,
                });
                Ok((frames, through_event_cursor))
            })();
        let (frames, through_event_cursor) = match prepared {
            Ok(prepared) => prepared,
            Err(error) => {
                cancellation.request();
                let cleanup = pending.cancel().await.map_err(map_live_attach_error);
                cancellation.complete(cleanup.clone());
                if cleanup.is_ok() {
                    rpc.remove_pending(&attach_id, &authority).await;
                }
                cleanup?;
                revalidate_live_authority(host_endpoint, peer, &authority)?;
                let response = live_response_envelope(&request, Err(error));
                return request.write_response(&response).await;
            }
        };
        let preflight = frames.iter().try_for_each(|frame| {
            let response = live_response_envelope(&request, Ok(frame.clone()));
            request
                .validate_response_frame(&response)
                .and_then(|()| validate_frame_encodable(&response))
        });
        if let Err(error) = preflight {
            return finish_pending_lifecycle(
                rpc,
                &attach_id,
                &authority,
                &cancellation,
                pending,
                error,
            )
            .await;
        }
        for frame in frames {
            if cancellation.is_requested() {
                return finish_pending_lifecycle(
                    rpc,
                    &attach_id,
                    &authority,
                    &cancellation,
                    pending,
                    live_lifecycle_unavailable(
                        "remote Agent live attachment was cancelled during snapshot",
                    ),
                )
                .await;
            }
            if let Err(error) = write_live_frame_or_cancel(
                host_endpoint,
                peer,
                &authority,
                cancellation.as_ref(),
                &mut request,
                frame,
            )
            .await
            {
                return finish_pending_lifecycle(
                    rpc,
                    &attach_id,
                    &authority,
                    &cancellation,
                    pending,
                    error,
                )
                .await;
            }
        }
        let expiry = match rpc
            .arm_pending_for(&authority, &attach_id, pending_ttl)
            .await
        {
            Ok(expiry) => expiry,
            Err(error) => {
                return finish_pending_lifecycle(
                    rpc,
                    &attach_id,
                    &authority,
                    &cancellation,
                    pending,
                    error,
                )
                .await;
            }
        };
        let command = tokio::select! {
            biased;
            _ = cancellation.wait_requested() => Err(live_lifecycle_unavailable(
                "remote Agent live attachment was cancelled",
            )),
            cancelled = &mut response_cancelled => Err(cancelled_live_request(cancelled)),
            _ = &mut peer_closed => Err(live_connection_closed()),
            _ = tokio::time::sleep_until(expiry) => Err(live_lifecycle_unavailable(
                "remote Agent live attachment expired",
            )),
            command = activate_receive => command.map_err(|_| live_lifecycle_unavailable(
                "remote Agent live activation channel closed",
            )),
        };
        let mut command = match command {
            Ok(command) => command,
            Err(error) => {
                return finish_pending_lifecycle(
                    rpc,
                    &attach_id,
                    &authority,
                    &cancellation,
                    pending,
                    error,
                )
                .await;
            }
        };
        if let Err(error) = revalidate_live_authority(host_endpoint, peer, &authority) {
            let _ = command.response.send(Err(error.clone()));
            return finish_activating_pending_lifecycle(
                rpc,
                &attach_id,
                &authority,
                &cancellation,
                pending,
                error,
            )
            .await;
        }
        if cancellation.is_requested() || command.response.is_closed() {
            let error = live_lifecycle_unavailable("remote Agent live activation was cancelled");
            let _ = command.response.send(Err(error.clone()));
            return finish_activating_pending_lifecycle(
                rpc,
                &attach_id,
                &authority,
                &cancellation,
                pending,
                error,
            )
            .await;
        }
        let mut pending = pending;
        let interruption = async {
            tokio::select! {
                biased;
                _ = cancellation.wait_requested() => live_lifecycle_unavailable(
                    "remote Agent live activation was cancelled",
                ),
                cancelled = &mut response_cancelled => cancelled_live_request(cancelled),
                _ = &mut peer_closed => live_connection_closed(),
                _ = command.response.closed() => live_lifecycle_unavailable(
                    "remote Agent live activation acknowledgement was abandoned",
                ),
            }
        };
        let activation = activate_pending_until_interrupted(pending.as_mut(), interruption).await;
        let activated = match activation {
            PendingActivationResult::Activated(activated) => activated,
            PendingActivationResult::NativeError(error) => {
                let precise_snapshot_reason = snapshot_reason_from_attach_error(&error);
                let mapped = map_live_attach_error(error);
                // Native activation has already returned, so this owner can
                // synchronously reclaim the still-valid pending handle. Do
                // not mark the shared cancellation as requested here: the
                // Control waiter treats that signal as an external cancel and
                // would race the precise SnapshotRequired acknowledgement.
                let cleanup = pending.cancel().await.map_err(map_live_attach_error);
                cancellation.complete(cleanup.clone());
                if cleanup.is_ok() {
                    rpc.remove_activating(&attach_id, &authority).await;
                }
                cleanup?;
                if let Some(reason) =
                    precise_snapshot_reason.or_else(|| snapshot_reason_from_protocol_error(&mapped))
                {
                    let disposition = AgentLiveActivationDisposition::SnapshotRequired {
                        reason,
                        last_event_cursor: through_event_cursor.clone(),
                    };
                    let _ = command.response.send(Ok(disposition));
                    write_live_frame(
                        host_endpoint,
                        peer,
                        &authority,
                        &mut request,
                        AgentLiveStreamFrame::SnapshotRequired {
                            reason,
                            last_event_cursor: through_event_cursor,
                        },
                    )
                    .await?;
                    return request.finish_response();
                }
                let _ = command.response.send(Err(mapped.clone()));
                return Err(mapped);
            }
            PendingActivationResult::Interrupted(mapped) => {
                cancellation.request();
                let cleanup = pending.cancel().await.map_err(map_live_attach_error);
                cancellation.complete(cleanup.clone());
                if cleanup.is_ok() {
                    rpc.remove_activating(&attach_id, &authority).await;
                }
                cleanup?;
                if let Some(reason) = snapshot_reason_from_protocol_error(&mapped) {
                    let disposition = AgentLiveActivationDisposition::SnapshotRequired {
                        reason,
                        last_event_cursor: through_event_cursor.clone(),
                    };
                    let _ = command.response.send(Ok(disposition));
                    write_live_frame(
                        host_endpoint,
                        peer,
                        &authority,
                        &mut request,
                        AgentLiveStreamFrame::SnapshotRequired {
                            reason,
                            last_event_cursor: through_event_cursor,
                        },
                    )
                    .await?;
                    return request.finish_response();
                }
                let _ = command.response.send(Err(mapped.clone()));
                return Err(mapped);
            }
        };
        serve_owned_live_stream(
            host_endpoint,
            peer,
            rpc,
            authority,
            request,
            attach_id,
            command.live_stream_id,
            through_event_cursor,
            AgentLiveRemoteResume {
                through_event_cursor: activated.through_event_cursor,
                stream: activated.stream,
            },
            cancellation,
            Some(command.response),
        )
        .await
    }

    pub(super) async fn serve_remote_agent_live_activation<T>(
        host_endpoint: &MapleIrohEndpoint,
        peer: &ConnectedPeer,
        rpc: &RemoteAgentLiveRpcHost,
        authority: VerifiedIncomingPeerAuthorization,
        request: AcceptedRequest<T>,
        body: ActivateAgentLiveAttachRequest,
    ) -> Result<(), ProtocolError>
    where
        T: crate::remote_protocol::RequestBody,
        ActivateAgentLiveAttachResponse: crate::remote_protocol::ResponseBody<T>,
    {
        let mut pending = match rpc
            .take_pending_for_activation(&authority, &body.attach_id)
            .await
        {
            Ok(pending) => pending,
            Err(error) => {
                revalidate_live_authority(host_endpoint, peer, &authority)?;
                let response: ResponseEnvelope<ActivateAgentLiveAttachResponse> =
                    control_response_envelope(&request, Err(error));
                return request.write_response(&response).await;
            }
        };
        let cancellation = Arc::clone(&pending.cancellation);
        let activate = pending.activate.take().ok_or_else(|| {
            live_lifecycle_unavailable("remote Agent live attachment is no longer activatable")
        })?;
        let live_stream_id = match allocate_live_id().await {
            Ok(id) => id,
            Err(error) => {
                drop(activate);
                cancellation.request_and_wait().await?;
                return Err(error);
            }
        };
        if let Err(error) = rpc
            .name_activating_stream(&body.attach_id, &live_stream_id, &authority)
            .await
        {
            drop(activate);
            cancellation.request_and_wait().await?;
            return Err(error);
        }
        let (response_send, mut response_receive) = oneshot::channel();
        if activate
            .send(ActivateRemoteAgentLiveCommand {
                live_stream_id,
                response: response_send,
            })
            .is_err()
        {
            cancellation.request_and_wait().await?;
            return Err(live_lifecycle_unavailable(
                "remote Agent live attachment owner closed",
            ));
        }
        let mut response_cancelled = Box::pin(request.response_cancelled());
        let result: Result<AgentLiveActivationDisposition, ProtocolError> = tokio::select! {
            biased;
            _ = cancellation.wait_requested() => {
                cancellation.wait_completion().await?;
                Err(live_lifecycle_unavailable("remote Agent live activation was cancelled"))
            }
            cancelled = &mut response_cancelled => {
                cancellation.request_and_wait().await?;
                Err(cancelled_live_request(cancelled))
            }
            _ = peer.wait_closed() => {
                cancellation.request_and_wait().await?;
                Err(live_connection_closed())
            }
            result = &mut response_receive => result.unwrap_or_else(|_| {
                Err(live_lifecycle_unavailable(
                    "remote Agent live activation owner closed before acknowledgement",
                ))
            }),
        };
        if let Err(error) = revalidate_live_authority(host_endpoint, peer, &authority) {
            cancellation.request_and_wait().await?;
            return Err(error);
        }
        let response = control_response_envelope(
            &request,
            result.map(|result| ActivateAgentLiveAttachResponse {
                attach_id: body.attach_id,
                result,
            }),
        );
        match request.write_response(&response).await {
            Ok(()) => Ok(()),
            Err(error) => {
                cancellation.request_and_wait().await?;
                Err(error)
            }
        }
    }

    pub(super) async fn serve_remote_agent_live_cancel<T>(
        host_endpoint: &MapleIrohEndpoint,
        peer: &ConnectedPeer,
        rpc: &RemoteAgentLiveRpcHost,
        authority: VerifiedIncomingPeerAuthorization,
        request: AcceptedRequest<T>,
        body: CancelAgentLiveRequest,
    ) -> Result<(), ProtocolError>
    where
        T: crate::remote_protocol::RequestBody,
        CancelAgentLiveResponse: crate::remote_protocol::ResponseBody<T>,
    {
        let result = rpc
            .cancel_lifecycle(&authority, body.kind, &body.live_id)
            .await
            .map(|()| CancelAgentLiveResponse {
                kind: body.kind,
                live_id: body.live_id,
            });
        revalidate_live_authority(host_endpoint, peer, &authority)?;
        let response = control_response_envelope(&request, result);
        request.write_response(&response).await
    }

    pub(super) async fn serve_remote_agent_live_resume(
        host_endpoint: &MapleIrohEndpoint,
        peer: &ConnectedPeer,
        rpc: &RemoteAgentLiveRpcHost,
        authority: VerifiedIncomingPeerAuthorization,
        mut request: AcceptedRequest<RemoteAgentLiveEventsRequest>,
        body: ResumeAgentLiveEventsRequest,
    ) -> Result<(), ProtocolError> {
        let from = body.cursor.clone();
        let mut response_cancelled = Box::pin(request.response_cancelled());
        let mut peer_closed = Box::pin(peer.wait_closed());
        let (native_from, service) = tokio::select! {
            biased;
            cancelled = &mut response_cancelled => return Err(cancelled_live_request(cancelled)),
            _ = &mut peer_closed => return Err(live_connection_closed()),
            result = prepare_remote_agent_live_resume(
                rpc,
                &authority,
                peer.connection_stamp(),
                &body,
            ) => match result {
                Ok(prepared) => prepared,
                Err(error) => {
                    revalidate_live_authority(host_endpoint, peer, &authority)?;
                    let response = live_response_envelope(&request, Err(error));
                    return request.write_response(&response).await;
                }
            },
        };
        let live_stream_id = allocate_live_id().await?;
        let cancellation = Arc::new(RemoteAgentLiveCancellation::default());
        rpc.reserve_activating(
            live_stream_id.clone(),
            authority.clone(),
            Arc::clone(&service),
            Arc::clone(&cancellation),
        )
        .await?;
        let resume = tokio::select! {
            biased;
            _ = cancellation.wait_requested() => {
                rpc.remove_activating(&live_stream_id, &authority).await;
                cancellation.complete(Ok(()));
                return Err(live_lifecycle_unavailable("remote Agent live resume was cancelled"));
            }
            cancelled = &mut response_cancelled => {
                rpc.remove_activating(&live_stream_id, &authority).await;
                cancellation.complete(Ok(()));
                return Err(cancelled_live_request(cancelled));
            }
            _ = &mut peer_closed => {
                rpc.remove_activating(&live_stream_id, &authority).await;
                cancellation.complete(Ok(()));
                return Err(live_connection_closed());
            }
            result = service.resume(native_from, Some(REMOTE_LIVE_SUBSCRIPTION_CAPACITY)) => match result {
                Ok(resume) => resume,
                Err(error) => {
                    rpc.remove_activating(&live_stream_id, &authority).await;
                    cancellation.complete(Ok(()));
                    if let Some(reason) = snapshot_reason_from_attach_error(&error) {
                        write_live_frame(
                            host_endpoint,
                            peer,
                            &authority,
                            &mut request,
                            AgentLiveStreamFrame::SnapshotRequired {
                                reason,
                                last_event_cursor: from,
                            },
                        )
                        .await?;
                        return request.finish_response();
                    }
                    let response = live_response_envelope(
                        &request,
                        Err(map_live_attach_error(error)),
                    );
                    revalidate_live_authority(host_endpoint, peer, &authority)?;
                    return request.write_response(&response).await;
                }
            },
        };
        serve_owned_live_stream(
            host_endpoint,
            peer,
            rpc,
            authority,
            request,
            live_stream_id.clone(),
            live_stream_id,
            from,
            resume,
            cancellation,
            None,
        )
        .await
    }

    pub(super) async fn prepare_remote_agent_live_resume(
        rpc: &RemoteAgentLiveRpcHost,
        authority: &VerifiedIncomingPeerAuthorization,
        connection_stamp: crate::remote_protocol::ConnectionStamp,
        body: &ResumeAgentLiveEventsRequest,
    ) -> Result<(AgentLiveEventCursor, Arc<dyn AgentLiveRemoteAttachService>), ProtocolError> {
        body.validate_for_connection_stamp(connection_stamp)?;
        let native_from = native_cursor(&body.cursor)?;
        let service = rpc.bind_service(authority).await?;
        Ok((native_from, service))
    }

    #[allow(clippy::too_many_arguments)]
    async fn serve_owned_live_stream(
        host_endpoint: &MapleIrohEndpoint,
        peer: &ConnectedPeer,
        rpc: &RemoteAgentLiveRpcHost,
        authority: VerifiedIncomingPeerAuthorization,
        mut request: AcceptedRequest<RemoteAgentLiveEventsRequest>,
        activation_id: String,
        live_stream_id: String,
        from: RemoteLiveEventCursor,
        resume: AgentLiveRemoteResume,
        cancellation: Arc<RemoteAgentLiveCancellation>,
        mut activation_response: Option<
            oneshot::Sender<Result<AgentLiveActivationDisposition, ProtocolError>>,
        >,
    ) -> Result<(), ProtocolError> {
        let mut stream = resume.stream;
        let mut promoted = false;
        let through = match remote_cursor(resume.through_event_cursor) {
            Ok(cursor) => cursor,
            Err(error) => {
                let cleanup = stream.unsubscribe().await.map_err(map_live_attach_error);
                cancellation.complete(cleanup.clone());
                if cleanup.is_ok() {
                    rpc.remove_activating(&activation_id, &authority).await;
                }
                if let Some(response) = activation_response.take() {
                    let _ = response.send(Err(error.clone()));
                }
                cleanup?;
                return Err(error);
            }
        };
        if let Err(error) = validate_remote_cursor_range(&from, &through) {
            let cleanup = stream.unsubscribe().await.map_err(map_live_attach_error);
            cancellation.complete(cleanup.clone());
            if cleanup.is_ok() {
                rpc.remove_activating(&activation_id, &authority).await;
            }
            if let Some(response) = activation_response.take() {
                let _ = response.send(Err(error.clone()));
            }
            cleanup?;
            return Err(error);
        }
        let start = AgentLiveStreamFrame::StreamStart {
            live_stream_id: live_stream_id.clone(),
            from_event_cursor: from.clone(),
            through_event_cursor: through.clone(),
        };
        let setup = async {
            if cancellation.is_requested() {
                return Err(live_lifecycle_unavailable(
                    "remote Agent live stream was cancelled",
                ));
            }
            write_live_frame_or_cancel(
                host_endpoint,
                peer,
                &authority,
                cancellation.as_ref(),
                &mut request,
                start,
            )
            .await?;
            rpc.promote_activating(&activation_id, &live_stream_id, &authority)
                .await?;
            promoted = true;
            if let Some(response) = activation_response.take() {
                response
                    .send(Ok(AgentLiveActivationDisposition::Activated {
                        live_stream_id: live_stream_id.clone(),
                        through_event_cursor: through.clone(),
                    }))
                    .map_err(|_| {
                        live_lifecycle_unavailable(
                            "remote Agent live activation acknowledgement was abandoned",
                        )
                    })?;
            }
            Ok(())
        }
        .await;
        if let Err(error) = setup {
            let cleanup = stream.unsubscribe().await.map_err(map_live_attach_error);
            cancellation.complete(cleanup.clone());
            if cleanup.is_ok() {
                if promoted {
                    rpc.remove_active(&live_stream_id, &authority).await;
                } else {
                    rpc.remove_activating(&activation_id, &authority).await;
                }
            }
            if let Some(response) = activation_response.take() {
                let _ = response.send(Err(error.clone()));
            }
            cleanup?;
            return Err(error);
        }

        let mut response_cancelled = Box::pin(request.response_cancelled());
        let mut peer_closed = Box::pin(peer.wait_closed());
        let result = async {
        let mut last = from;
        let mut replay_count = 0usize;
        while last.sequence < through.sequence {
            if replay_count >= MAX_REMOTE_LIVE_REPLAY_EVENTS {
                write_live_frame_or_cancel(
                    host_endpoint,
                    peer,
                    &authority,
                    cancellation.as_ref(),
                    &mut request,
                    AgentLiveStreamFrame::SnapshotRequired {
                        reason: RemoteAgentLiveSnapshotReason::RetentionGap,
                        last_event_cursor: last,
                    },
                )
                .await?;
                return Ok(());
            }
            revalidate_live_authority(host_endpoint, peer, &authority)?;
            let delivery = tokio::select! {
                biased;
                _ = cancellation.wait_requested() => return Err(live_lifecycle_unavailable(
                    "remote Agent live stream was cancelled",
                )),
                cancelled = &mut response_cancelled => return Err(cancelled_live_request(cancelled)),
                _ = &mut peer_closed => return Err(live_connection_closed()),
                delivery = stream.recv() => delivery,
            };
            let delivery = match delivery {
                Ok(delivery) => remote_live_delivery(delivery)?,
                Err(error) => {
                    if let Some(reason) = snapshot_reason_from_stream_error(&error) {
                        write_live_frame_or_cancel(
                            host_endpoint,
                            peer,
                            &authority,
                            cancellation.as_ref(),
                            &mut request,
                            AgentLiveStreamFrame::SnapshotRequired {
                                reason,
                                last_event_cursor: last,
                            },
                        )
                        .await?;
                        return Ok(());
                    }
                    return Err(map_live_stream_error(error));
                }
            };
            validate_next_remote_delivery(&last, &delivery, Some(&through))?;
            last = delivery.cursor.clone();
            replay_count += 1;
            write_live_frame_or_cancel(
                host_endpoint,
                peer,
                &authority,
                cancellation.as_ref(),
                &mut request,
                AgentLiveStreamFrame::Event { delivery },
            )
            .await?;
        }
        if last != through {
            return Err(invalid_live_response(
                "remote Agent live replay ended at another cursor",
            ));
        }
        write_live_frame_or_cancel(
            host_endpoint,
            peer,
            &authority,
            cancellation.as_ref(),
            &mut request,
            AgentLiveStreamFrame::ReplayComplete {
                through_event_cursor: through,
            },
        )
        .await?;

        loop {
            revalidate_live_authority(host_endpoint, peer, &authority)?;
            let delivery = tokio::select! {
                biased;
                _ = cancellation.wait_requested() => return Err(live_lifecycle_unavailable(
                    "remote Agent live stream was cancelled",
                )),
                cancelled = &mut response_cancelled => return Err(cancelled_live_request(cancelled)),
                _ = &mut peer_closed => return Err(live_connection_closed()),
                delivery = stream.recv() => delivery,
            };
            let delivery = match delivery {
                Ok(delivery) => remote_live_delivery(delivery)?,
                Err(error) => {
                    if let Some(reason) = snapshot_reason_from_stream_error(&error) {
                        write_live_frame_or_cancel(
                            host_endpoint,
                            peer,
                            &authority,
                            cancellation.as_ref(),
                            &mut request,
                            AgentLiveStreamFrame::SnapshotRequired {
                                reason,
                                last_event_cursor: last,
                            },
                        )
                        .await?;
                        return Ok(());
                    }
                    return Err(map_live_stream_error(error));
                }
            };
            validate_next_remote_delivery(&last, &delivery, None)?;
            last = delivery.cursor.clone();
            write_live_frame_or_cancel(
                host_endpoint,
                peer,
                &authority,
                cancellation.as_ref(),
                &mut request,
                AgentLiveStreamFrame::Event { delivery },
            )
            .await?;
        }
    }
    .await;

        let cleanup = stream.unsubscribe().await.map_err(map_live_attach_error);
        cancellation.complete(cleanup.clone());
        if cleanup.is_ok() {
            rpc.remove_active(&live_stream_id, &authority).await;
        }
        cleanup?;
        result?;
        request.finish_response()
    }

    async fn finish_pending_lifecycle(
        rpc: &RemoteAgentLiveRpcHost,
        attach_id: &str,
        authority: &VerifiedIncomingPeerAuthorization,
        cancellation: &Arc<RemoteAgentLiveCancellation>,
        pending: Box<dyn AgentLiveRemotePendingAttach>,
        terminal_error: ProtocolError,
    ) -> Result<(), ProtocolError> {
        cancellation.request();
        let cleanup = pending.cancel().await.map_err(map_live_attach_error);
        cancellation.complete(cleanup.clone());
        if cleanup.is_ok() {
            rpc.remove_pending(attach_id, authority).await;
            // Activation may have atomically moved the registry marker while
            // this Events owner was waking. Native acknowledgement precedes
            // both removals, so stable occupancy remains fail-closed.
            rpc.remove_activating(attach_id, authority).await;
        }
        cleanup?;
        Err(terminal_error)
    }

    pub(super) async fn finish_activating_pending_lifecycle(
        rpc: &RemoteAgentLiveRpcHost,
        attach_id: &str,
        authority: &VerifiedIncomingPeerAuthorization,
        cancellation: &Arc<RemoteAgentLiveCancellation>,
        pending: Box<dyn AgentLiveRemotePendingAttach>,
        terminal_error: ProtocolError,
    ) -> Result<(), ProtocolError> {
        cancellation.request();
        let cleanup = pending.cancel().await.map_err(map_live_attach_error);
        cancellation.complete(cleanup.clone());
        if cleanup.is_ok() {
            rpc.remove_activating(attach_id, authority).await;
        }
        cleanup?;
        Err(terminal_error)
    }

    fn snapshot_reason_from_protocol_error(
        error: &ProtocolError,
    ) -> Option<RemoteAgentLiveSnapshotReason> {
        match error.code {
            ErrorCode::SnapshotRequired | ErrorCode::StaleGeneration => {
                Some(RemoteAgentLiveSnapshotReason::OwnerChanged)
            }
            _ => None,
        }
    }

    async fn write_live_frame<T>(
        host_endpoint: &MapleIrohEndpoint,
        peer: &ConnectedPeer,
        authority: &VerifiedIncomingPeerAuthorization,
        request: &mut AcceptedRequest<T>,
        frame: AgentLiveStreamFrame,
    ) -> Result<(), ProtocolError>
    where
        T: crate::remote_protocol::RequestBody + serde::Serialize,
        AgentLiveStreamFrame: crate::remote_protocol::ResponseBody<T>,
    {
        revalidate_live_authority(host_endpoint, peer, authority)?;
        let response = live_response_envelope(request, Ok(frame));
        request
            .validate_response_frame(&response)
            .and_then(|()| validate_frame_encodable(&response))?;
        revalidate_live_authority(host_endpoint, peer, authority)?;
        request.write_response_frame(&response).await
    }

    /// Write one Events frame while retaining prompt cancellation/revocation
    /// responsiveness. Dropping the in-flight write is terminal for this
    /// request owner; every caller immediately performs the exact native
    /// cancel/unsubscribe acknowledgement before releasing lifecycle
    /// occupancy.
    async fn write_live_frame_or_cancel<T>(
        host_endpoint: &MapleIrohEndpoint,
        peer: &ConnectedPeer,
        authority: &VerifiedIncomingPeerAuthorization,
        cancellation: &RemoteAgentLiveCancellation,
        request: &mut AcceptedRequest<T>,
        frame: AgentLiveStreamFrame,
    ) -> Result<(), ProtocolError>
    where
        T: crate::remote_protocol::RequestBody + serde::Serialize,
        AgentLiveStreamFrame: crate::remote_protocol::ResponseBody<T>,
    {
        let mut response_cancelled = Box::pin(request.response_cancelled());
        let mut peer_closed = Box::pin(peer.wait_closed());
        let mut write = Box::pin(write_live_frame(
            host_endpoint,
            peer,
            authority,
            request,
            frame,
        ));
        tokio::select! {
            biased;
            _ = cancellation.wait_requested() => Err(live_lifecycle_unavailable(
                "remote Agent live stream was cancelled during frame delivery",
            )),
            cancelled = &mut response_cancelled => Err(cancelled_live_request(cancelled)),
            _ = &mut peer_closed => Err(live_connection_closed()),
            result = &mut write => result,
        }
    }

    fn live_response_envelope<T>(
        request: &AcceptedRequest<T>,
        result: Result<AgentLiveStreamFrame, ProtocolError>,
    ) -> ResponseEnvelope<AgentLiveStreamFrame>
    where
        T: crate::remote_protocol::RequestBody,
    {
        let envelope = request.request();
        ResponseEnvelope {
            protocol_version: PROTOCOL_VERSION,
            request_id: envelope.request_id.clone(),
            execution_target_id: envelope.execution_target_id.clone(),
            connection_stamp: envelope.connection_stamp,
            result,
        }
    }

    fn control_response_envelope<T, R>(
        request: &AcceptedRequest<T>,
        result: Result<R, ProtocolError>,
    ) -> ResponseEnvelope<R>
    where
        T: crate::remote_protocol::RequestBody,
    {
        let envelope = request.request();
        ResponseEnvelope {
            protocol_version: PROTOCOL_VERSION,
            request_id: envelope.request_id.clone(),
            execution_target_id: envelope.execution_target_id.clone(),
            connection_stamp: envelope.connection_stamp,
            result,
        }
    }

    fn revalidate_live_authority(
        host_endpoint: &MapleIrohEndpoint,
        peer: &ConnectedPeer,
        authority: &VerifiedIncomingPeerAuthorization,
    ) -> Result<(), ProtocolError> {
        host_endpoint.validate_current_incoming_peer(peer)?;
        if authority.controller_endpoint() != peer.remote_id()
            || authority.execution_target_id() != peer.execution_target_id()
            || authority.pairing_fence() != peer.pairing_fence()
            || authority.connection_stamp() != peer.connection_stamp()
        {
            return Err(stale_live_lease());
        }
        authority.revalidate_current()
    }

    fn validate_remote_live_snapshot(
        complete: bool,
        sessions: &[RemoteAgentLiveSessionSnapshot],
    ) -> Result<(), ProtocolError> {
        if !complete || sessions.len() > crate::remote_protocol::MAX_LIVE_SESSIONS_PER_ACCOUNT {
            return Err(invalid_live_response(
                "remote Agent live snapshot is incomplete or too large",
            ));
        }
        let mut total_items = 0usize;
        let mut previous_session_id: Option<&str> = None;
        for session in sessions {
            session.validate()?;
            if previous_session_id.is_some_and(|previous| previous >= session.session_id.as_str()) {
                return Err(invalid_live_response(
                    "remote Agent live snapshot sessions are not unique and sorted",
                ));
            }
            previous_session_id = Some(&session.session_id);
            total_items = total_items
                .checked_add(session.live_items.len())
                .ok_or_else(|| invalid_live_response("remote live snapshot item count overflow"))?;
        }
        if total_items > crate::remote_protocol::MAX_LIVE_ITEMS_PER_ACCOUNT {
            return Err(invalid_live_response(
                "remote Agent live snapshot contains too many items",
            ));
        }
        Ok(())
    }
}

fn validate_next_remote_delivery(
    last: &RemoteLiveEventCursor,
    delivery: &RemoteAgentLiveDelivery,
    replay_through: Option<&RemoteLiveEventCursor>,
) -> Result<(), ProtocolError> {
    delivery.validate()?;
    let expected = last.sequence.checked_add(1).ok_or_else(|| {
        ProtocolError::new(
            ErrorCode::SnapshotRequired,
            "remote Agent live event sequence exhausted",
            true,
        )
    })?;
    if delivery.cursor.journal_id != last.journal_id || delivery.cursor.sequence != expected {
        return Err(ProtocolError::new(
            ErrorCode::SnapshotRequired,
            "remote Agent live event ordering was lost",
            true,
        ));
    }
    if replay_through.is_some_and(|through| {
        delivery.cursor.journal_id != through.journal_id
            || delivery.cursor.sequence > through.sequence
    }) {
        return Err(invalid_live_response(
            "remote Agent live replay crossed its FIFO barrier",
        ));
    }
    Ok(())
}

fn validate_remote_cursor_range(
    from: &RemoteLiveEventCursor,
    through: &RemoteLiveEventCursor,
) -> Result<(), ProtocolError> {
    from.validate()?;
    through.validate()?;
    if from.journal_id == through.journal_id && from.sequence <= through.sequence {
        Ok(())
    } else {
        Err(ProtocolError::new(
            ErrorCode::SnapshotRequired,
            "remote Agent live cursor requires an authoritative snapshot",
            true,
        ))
    }
}

fn invalid_live_response(message: impl Into<String>) -> ProtocolError {
    ProtocolError::new(ErrorCode::InvalidFrame, message, false)
}

fn snapshot_required_error(
    reason: RemoteAgentLiveSnapshotReason,
    last: &RemoteLiveEventCursor,
) -> ProtocolError {
    ProtocolError::new(
        ErrorCode::SnapshotRequired,
        format!(
            "remote Agent live snapshot required ({reason:?}) after event {}",
            last.sequence
        ),
        true,
    )
}

fn cancelled_live_request(result: Result<(), ProtocolError>) -> ProtocolError {
    result.err().unwrap_or_else(|| {
        ProtocolError::new(
            ErrorCode::TransportUnavailable,
            "remote Agent live response was cancelled",
            true,
        )
    })
}

fn live_connection_closed() -> ProtocolError {
    ProtocolError::new(
        ErrorCode::TransportUnavailable,
        "remote Agent live connection closed",
        true,
    )
}

#[cfg(desktop)]
async fn allocate_live_id() -> Result<String, ProtocolError> {
    allocate_live_id_value()
}

#[cfg(desktop)]
fn allocate_live_id_value() -> Result<String, ProtocolError> {
    for _ in 0..MAX_LIVE_ID_ATTEMPTS {
        let mut bytes = [0_u8; LIVE_ID_RANDOM_BYTES];
        if fill_random(&mut bytes).is_err() {
            continue;
        }
        let mut encoded = String::with_capacity(LIVE_ID_RANDOM_BYTES * 2);
        const HEX: &[u8; 16] = b"0123456789abcdef";
        for byte in bytes {
            encoded.push(HEX[(byte >> 4) as usize] as char);
            encoded.push(HEX[(byte & 0x0f) as usize] as char);
        }
        return Ok(encoded);
    }
    Err(ProtocolError::new(
        ErrorCode::Internal,
        "remote Agent live ID allocation failed",
        false,
    ))
}

#[cfg(desktop)]
fn remote_cursor(cursor: AgentLiveEventCursor) -> Result<RemoteLiveEventCursor, ProtocolError> {
    let cursor = RemoteLiveEventCursor {
        journal_id: cursor.journal_id,
        sequence: cursor.sequence,
    };
    cursor.validate()?;
    Ok(cursor)
}

#[cfg(desktop)]
fn native_cursor(cursor: &RemoteLiveEventCursor) -> Result<AgentLiveEventCursor, ProtocolError> {
    cursor.validate()?;
    Ok(AgentLiveEventCursor {
        journal_id: cursor.journal_id.clone(),
        sequence: cursor.sequence,
    })
}

#[cfg(desktop)]
fn remote_safe_history_record(
    record: crate::agent_live_host::AgentLiveSafeHistoryRecord,
) -> Result<RemoteAgentHistoryRecord, ProtocolError> {
    let record = RemoteAgentHistoryRecord {
        record_id: record.record_id,
        role: record.role,
        created_ms: record.created_ms,
        items: record
            .items
            .into_iter()
            .map(remote_live_timeline_item)
            .collect::<Result<Vec<_>, _>>()?,
    };
    record.validate()?;
    Ok(record)
}

#[cfg(desktop)]
fn remote_live_timeline_item(
    item: MapleLiveTimelineItem,
) -> Result<RemoteAgentTimelineItem, ProtocolError> {
    let item = RemoteAgentTimelineItem {
        id: item.id,
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
        title: item.title,
        text: item.text,
        status: item.status,
        created_ms: item.created_ms,
        merge: match item.merge {
            MapleLiveMerge::Append => "append",
            MapleLiveMerge::Replace => "replace",
        }
        .to_string(),
    };
    item.validate()?;
    Ok(item)
}

#[cfg(desktop)]
fn remote_live_event_timeline_item(
    item: MapleLiveTimelineItem,
) -> Result<RemoteAgentTimelineItem, ProtocolError> {
    // The closed item validator admits only terminal, non-actionable
    // permission presentations. Pending/control-bearing permission state is
    // still rejected; resolved and cancelled audit rows remain displayable.
    remote_live_timeline_item(item)
}

#[cfg(desktop)]
fn remote_session_summary(
    session: crate::agent_live_coordinator::MapleLiveSessionSummary,
) -> Result<RemoteAgentSessionSummary, ProtocolError> {
    let session = RemoteAgentSessionSummary {
        id: session.id,
        title: session.title,
        project_root: session.project_root,
        created_ms: session.created_ms,
        updated_ms: session.updated_ms,
        page_sort_ms: session.page_sort_ms,
        message_count: u64::try_from(session.message_count)
            .map_err(|_| invalid_live_response("remote Agent live session count is invalid"))?,
        model: session.model,
        mode: session.mode,
    };
    session.validate()?;
    Ok(session)
}

#[cfg(desktop)]
fn remote_live_delivery(
    delivery: AgentLiveRemoteDelivery,
) -> Result<RemoteAgentLiveDelivery, ProtocolError> {
    let event = match delivery.event {
        MapleLiveEvent::RunStarted { .. } => {
            crate::remote_protocol::RemoteAgentPresentedLiveEvent::RunStarted
        }
        MapleLiveEvent::TimelineUpsert { item, .. } => {
            crate::remote_protocol::RemoteAgentPresentedLiveEvent::TimelineUpsert {
                item: remote_live_event_timeline_item(item)?,
            }
        }
        MapleLiveEvent::TimelineCleared { reason, .. } => {
            crate::remote_protocol::RemoteAgentPresentedLiveEvent::TimelineCleared {
                reason: match reason {
                    MapleLiveClearReason::RunStarted => RemoteAgentLiveClearReason::RunStarted,
                    MapleLiveClearReason::HistoryReplaced => {
                        RemoteAgentLiveClearReason::HistoryReplaced
                    }
                    MapleLiveClearReason::ExplicitReload => {
                        RemoteAgentLiveClearReason::ExplicitReload
                    }
                },
            }
        }
        MapleLiveEvent::HistoryReplaced { .. } => {
            crate::remote_protocol::RemoteAgentPresentedLiveEvent::HistoryReplaced
        }
        MapleLiveEvent::HistoryHeadCommitted { .. } => {
            crate::remote_protocol::RemoteAgentPresentedLiveEvent::CursorAdvanced
        }
        MapleLiveEvent::SessionUpdated { session, .. } => {
            crate::remote_protocol::RemoteAgentPresentedLiveEvent::SessionUpdated {
                session: remote_session_summary(session)?,
            }
        }
        MapleLiveEvent::RunFinished { terminal, .. } => {
            crate::remote_protocol::RemoteAgentPresentedLiveEvent::RunFinished {
                terminal: match terminal {
                    MapleLiveRunTerminal::Completed => RemoteAgentLiveRunTerminal::Completed,
                    MapleLiveRunTerminal::Cancelled => RemoteAgentLiveRunTerminal::Cancelled,
                    MapleLiveRunTerminal::Failed => RemoteAgentLiveRunTerminal::Failed,
                },
            }
        }
        MapleLiveEvent::SessionDeleted { .. } => {
            crate::remote_protocol::RemoteAgentPresentedLiveEvent::SessionDeleted
        }
        MapleLiveEvent::UserFacingError { error, .. } => {
            crate::remote_protocol::RemoteAgentPresentedLiveEvent::UserFacingError {
                item: remote_live_timeline_item(error.to_timeline_item())?,
            }
        }
    };
    let delivery = RemoteAgentLiveDelivery {
        cursor: remote_cursor(delivery.cursor)?,
        session_id: delivery.session_id,
        run_id: delivery.run_id,
        event,
    };
    delivery.validate()?;
    Ok(delivery)
}

#[cfg(desktop)]
fn map_head_reload_reason(reason: HeadReloadReason) -> RemoteAgentLiveSnapshotReason {
    match reason {
        HeadReloadReason::PausedSubscriberOverflow => {
            RemoteAgentLiveSnapshotReason::PausedSubscriberOverflow
        }
        HeadReloadReason::SlowSubscriber => RemoteAgentLiveSnapshotReason::SlowSubscriber,
        HeadReloadReason::JournalReplaced | HeadReloadReason::ReseedRequired => {
            RemoteAgentLiveSnapshotReason::JournalReplaced
        }
        HeadReloadReason::RetentionGap => RemoteAgentLiveSnapshotReason::RetentionGap,
        HeadReloadReason::CursorAhead => RemoteAgentLiveSnapshotReason::CursorAhead,
        HeadReloadReason::OwnerChanged => RemoteAgentLiveSnapshotReason::OwnerChanged,
        HeadReloadReason::OrderingLost => RemoteAgentLiveSnapshotReason::OrderingLost,
        HeadReloadReason::JournalUnavailable => RemoteAgentLiveSnapshotReason::JournalUnavailable,
    }
}

#[cfg(desktop)]
fn snapshot_reason_from_attach_error(
    error: &AgentLiveRemoteAttachError,
) -> Option<RemoteAgentLiveSnapshotReason> {
    match error {
        AgentLiveRemoteAttachError::Host(AgentLiveHostError::Coordinator(
            AgentLiveCoordinatorError::HeadReloadRequired(reason),
        )) => Some(map_head_reload_reason(*reason)),
        AgentLiveRemoteAttachError::Host(AgentLiveHostError::Coordinator(
            AgentLiveCoordinatorError::ReseedRequired(_),
        ))
        | AgentLiveRemoteAttachError::Host(AgentLiveHostError::JournalReseedRequired(_)) => {
            Some(RemoteAgentLiveSnapshotReason::JournalReplaced)
        }
        _ => None,
    }
}

#[cfg(desktop)]
fn snapshot_reason_from_stream_error(
    error: &AgentLiveRemoteStreamError,
) -> Option<RemoteAgentLiveSnapshotReason> {
    match error {
        AgentLiveRemoteStreamError::Receive(AgentLiveReceiveError::HeadReloadRequired(reason)) => {
            Some(map_head_reload_reason(*reason))
        }
        AgentLiveRemoteStreamError::Receive(AgentLiveReceiveError::Closed) => {
            Some(RemoteAgentLiveSnapshotReason::OrderingLost)
        }
        AgentLiveRemoteStreamError::Attach(error) => snapshot_reason_from_attach_error(error),
    }
}

#[cfg(desktop)]
fn map_live_attach_error(error: AgentLiveRemoteAttachError) -> ProtocolError {
    if let Some(reason) = snapshot_reason_from_attach_error(&error) {
        return ProtocolError::new(
            ErrorCode::SnapshotRequired,
            format!("remote Agent live snapshot required ({reason:?})"),
            true,
        );
    }
    match error {
        AgentLiveRemoteAttachError::Unavailable => ProtocolError::new(
            ErrorCode::AgentLiveUnavailable,
            "verified remote Agent live attachment is unavailable",
            true,
        ),
        AgentLiveRemoteAttachError::ProjectionRejected => ProtocolError::new(
            ErrorCode::InvalidFrame,
            "remote Agent live projection was rejected",
            false,
        ),
        AgentLiveRemoteAttachError::Host(AgentLiveHostError::BoundContextRevoked) => {
            ProtocolError::new(
                ErrorCode::Revoked,
                "remote Agent live authorization was revoked",
                false,
            )
        }
        AgentLiveRemoteAttachError::Host(AgentLiveHostError::RuntimeOwnerMismatch)
        | AgentLiveRemoteAttachError::Host(AgentLiveHostError::BoundContextSealed) => {
            stale_live_lease()
        }
        AgentLiveRemoteAttachError::Host(_) => ProtocolError::new(
            ErrorCode::AgentLiveUnavailable,
            "verified remote Agent live attachment is unavailable",
            true,
        ),
    }
}

#[cfg(desktop)]
fn map_live_stream_error(error: AgentLiveRemoteStreamError) -> ProtocolError {
    match error {
        AgentLiveRemoteStreamError::Attach(error) => map_live_attach_error(error),
        AgentLiveRemoteStreamError::Receive(AgentLiveReceiveError::HeadReloadRequired(reason)) => {
            ProtocolError::new(
                ErrorCode::SnapshotRequired,
                format!(
                    "remote Agent live snapshot required ({:?})",
                    map_head_reload_reason(reason)
                ),
                true,
            )
        }
        AgentLiveRemoteStreamError::Receive(AgentLiveReceiveError::Closed) => {
            live_connection_closed()
        }
    }
}

/// Accept and serve one authenticated, generation-fenced history page.
#[cfg(test)]
pub async fn serve_next_remote_agent_history_page<P>(
    host_endpoint: &MapleIrohEndpoint,
    peer: &ConnectedPeer,
    provider: &P,
) -> Result<(), ProtocolError>
where
    P: RemoteAgentHistoryProvider + ?Sized,
{
    serve_next_remote_agent_history_page_with_timeout(
        host_endpoint,
        peer,
        provider,
        AGENT_HISTORY_PROVIDER_TIMEOUT,
    )
    .await
}

#[cfg(test)]
async fn serve_next_remote_agent_history_page_with_timeout<P>(
    host_endpoint: &MapleIrohEndpoint,
    peer: &ConnectedPeer,
    provider: &P,
    provider_timeout: Duration,
) -> Result<(), ProtocolError>
where
    P: RemoteAgentHistoryProvider + ?Sized,
{
    if provider_timeout.is_zero() {
        return Err(ProtocolError::new(
            ErrorCode::Internal,
            "Agent history provider timeout is invalid",
            false,
        ));
    }
    host_endpoint.validate_current_incoming_peer(peer)?;
    let accepted = peer.accept_stream().await?;
    let request: AcceptedRequest<ListAgentHistoryRecordsRequest> = accepted.read_request().await?;
    let body = request.request().body.clone();
    serve_remote_agent_history_page_request(
        host_endpoint,
        peer,
        provider,
        request,
        body,
        provider_timeout,
    )
    .await
}

async fn serve_remote_agent_history_page_request<T, P>(
    host_endpoint: &MapleIrohEndpoint,
    peer: &ConnectedPeer,
    provider: &P,
    mut request: AcceptedRequest<T>,
    body: ListAgentHistoryRecordsRequest,
    provider_timeout: Duration,
) -> Result<(), ProtocolError>
where
    T: crate::remote_protocol::RequestBody,
    AgentHistoryPageFrame: crate::remote_protocol::ResponseBody<T>,
    P: RemoteAgentHistoryProvider + ?Sized,
{
    if provider_timeout.is_zero() {
        return Err(ProtocolError::new(
            ErrorCode::Internal,
            "Agent history provider timeout is invalid",
            false,
        ));
    }
    host_endpoint.validate_current_incoming_peer(peer)?;

    let operation_deadline = request.operation_deadline();
    let now = tokio::time::Instant::now();
    let latest_provider_deadline = operation_deadline
        .checked_sub(AGENT_HISTORY_RESPONSE_BUDGET)
        .unwrap_or(now);
    let provider_deadline = latest_provider_deadline.min(now + provider_timeout);
    let mut response_cancelled = Box::pin(request.response_cancelled());
    let mut provider_result = provider.list_agent_history(&body);
    let result = tokio::select! {
        biased;
        cancelled = &mut response_cancelled => {
            return match cancelled {
                Ok(()) => Err(ProtocolError::new(
                    ErrorCode::TransportUnavailable,
                    "remote Agent history request was cancelled",
                    true,
                )),
                Err(error) => Err(error),
            };
        }
        provider_result = tokio::time::timeout_at(provider_deadline, provider_result.as_mut()) => {
            match provider_result {
                Ok(result) => result,
                Err(_) => Err(ProtocolError::new(
                    ErrorCode::TransportUnavailable,
                    "remote Agent history provider deadline elapsed",
                    true,
                )),
            }
        }
    };
    drop(provider_result);
    drop(response_cancelled);
    host_endpoint.validate_current_incoming_peer(peer)?;

    let request_id = request.request().request_id.clone();
    let execution_target_id = request.request().execution_target_id.clone();
    let connection_stamp = request.request().connection_stamp;
    let requested_limit = body.limit;
    let response_envelope = |result| ResponseEnvelope {
        protocol_version: PROTOCOL_VERSION,
        request_id: request_id.clone(),
        execution_target_id: execution_target_id.clone(),
        connection_stamp,
        result,
    };

    let page = match result {
        Ok(page) => page,
        Err(error) => {
            let response: ResponseEnvelope<AgentHistoryPageFrame> = response_envelope(Err(error));
            return request.write_response(&response).await;
        }
    };
    if page.records.len() > usize::from(requested_limit)
        || page.records.len() > usize::from(crate::remote_protocol::MAX_PAGE_SIZE)
        || (page.records.is_empty() && page.next_cursor.is_some())
    {
        let response: ResponseEnvelope<AgentHistoryPageFrame> =
            response_envelope(Err(ProtocolError::new(
                ErrorCode::InvalidPage,
                "Agent history provider returned an invalid record count",
                false,
            )));
        return request.write_response(&response).await;
    }

    let mut frames = Vec::with_capacity(page.records.len() + 2);
    frames.push(AgentHistoryPageFrame::Start {
        record_count: u16::try_from(page.records.len()).map_err(|_| {
            ProtocolError::new(ErrorCode::InvalidPage, "history page is too large", false)
        })?,
    });
    frames.extend(page.records.into_iter().enumerate().map(|(index, record)| {
        AgentHistoryPageFrame::Record {
            index: u16::try_from(index).expect("validated history page index fits u16"),
            record,
        }
    }));
    frames.push(AgentHistoryPageFrame::End {
        next_cursor: page.next_cursor,
        history_revision: page.history_revision,
    });

    // Preflight every record before emitting Start. A single oversized record
    // therefore returns one typed error rather than a truncated partial page.
    for frame in &frames {
        let response = response_envelope(Ok(frame.clone()));
        if let Err(error) = request
            .validate_response_frame(&response)
            .and_then(|()| validate_frame_encodable(&response))
        {
            let error = if error.code == ErrorCode::FrameTooLarge {
                ProtocolError::new(
                    ErrorCode::HistoryRecordTooLarge,
                    "one Agent history record exceeds Maple's frame limit",
                    false,
                )
            } else {
                error
            };
            let response: ResponseEnvelope<AgentHistoryPageFrame> = response_envelope(Err(error));
            return request.write_response(&response).await;
        }
    }

    for frame in frames {
        host_endpoint.validate_current_incoming_peer(peer)?;
        let response = response_envelope(Ok(frame));
        request.write_response_frame(&response).await?;
    }
    request.finish_response()
}

#[cfg(desktop)]
impl RemoteRuntimeStatusProvider for crate::agent::AgentRuntimeHandle {
    fn runtime_status(
        &self,
    ) -> Pin<Box<dyn Future<Output = Result<RemoteAgentRuntimeStatus, ProtocolError>> + Send + '_>>
    {
        Box::pin(async move {
            let status = self.status().await.map_err(|_| {
                // Agent errors can contain local account/runtime detail. Keep
                // the remote result bounded and category-only.
                ProtocolError::new(
                    ErrorCode::Internal,
                    "Maple Agent runtime status is unavailable",
                    true,
                )
            })?;
            let status = RemoteAgentRuntimeStatus {
                running: status.running,
                project_root: status.project_root,
                model: status.model,
                mode: status.mode,
                active_runs: status.active_runs.into_iter().collect(),
            };
            status.validate()?;
            Ok(status)
        })
    }
}

#[cfg(desktop)]
impl RemoteAgentHistoryProvider for crate::agent::AgentRuntimeHandle {
    fn list_agent_history(
        &self,
        request: &ListAgentHistoryRecordsRequest,
    ) -> Pin<Box<dyn Future<Output = Result<RemoteAgentHistoryPage, ProtocolError>> + Send + '_>>
    {
        let request = request.clone();
        Box::pin(async move {
            let page = self
                .list_session_records_page(crate::agent::AgentHistoryPageRequest {
                    session_id: request.session_id,
                    cursor: request.cursor,
                    limit: Some(usize::from(request.limit)),
                })
                .await
                .map_err(remote_history_provider_error)?;
            if page.live_items.is_some() || page.through_event_cursor.is_some() {
                return Err(ProtocolError::new(
                    ErrorCode::InvalidPage,
                    "ordinary Agent history must be persisted-only",
                    false,
                ));
            }
            let records = page
                .records
                .into_iter()
                .map(|record| {
                    let items = record
                        .items
                        .iter()
                        .map(crate::agent::project_safe_remote_history_item)
                        .map(|item| item.map_err(remote_history_provider_error))
                        .collect::<Result<Vec<_>, ProtocolError>>()?;
                    let record = RemoteAgentHistoryRecord {
                        record_id: record.record_id,
                        role: record.role,
                        created_ms: record.created_ms,
                        items,
                    };
                    record.validate()?;
                    Ok(record)
                })
                .collect::<Result<Vec<_>, ProtocolError>>()?;
            Ok(RemoteAgentHistoryPage {
                records,
                next_cursor: page.next_cursor,
                history_revision: page.history_revision,
            })
        })
    }
}

#[cfg(desktop)]
fn remote_timeline_item(
    item: crate::agent::AgentTimelineItem,
    absolute_live_snapshot: bool,
) -> Result<RemoteAgentTimelineItem, ProtocolError> {
    let item = RemoteAgentTimelineItem {
        id: item.id,
        item_type: item.item_type,
        role: item.role,
        title: item.title,
        text: item.text,
        status: item.status,
        created_ms: u64::try_from(item.created_ms).map_err(|_| {
            ProtocolError::new(
                ErrorCode::InvalidFrame,
                "Agent timeline timestamp is invalid",
                false,
            )
        })?,
        merge: if absolute_live_snapshot {
            "replace".to_string()
        } else {
            item.merge
        },
    };
    item.validate()?;
    Ok(item)
}

#[cfg(desktop)]
impl RemoteAgentSessionListProvider for crate::agent::AgentRuntimeHandle {
    fn list_agent_sessions(
        &self,
        request: &ListAgentSessionsRequest,
    ) -> Pin<Box<dyn Future<Output = Result<ListAgentSessionsResponse, ProtocolError>> + Send + '_>>
    {
        let request = request.clone();
        Box::pin(async move {
            let page = self
                .list_sessions_page(crate::agent::AgentSessionPageRequest {
                    project_root: request.project_root,
                    cursor: request.cursor,
                    limit: Some(usize::from(request.limit)),
                })
                .await
                .map_err(remote_history_provider_error)?;
            let items = page
                .items
                .into_iter()
                .map(|session| {
                    let item = RemoteAgentSessionSummary {
                        id: session.id,
                        title: session.title,
                        project_root: session.project_root,
                        created_ms: session.created_ms,
                        updated_ms: session.updated_ms,
                        page_sort_ms: session.page_sort_ms,
                        message_count: u64::try_from(session.message_count).map_err(|_| {
                            ProtocolError::new(
                                ErrorCode::InvalidFrame,
                                "Agent task message count is invalid",
                                false,
                            )
                        })?,
                        model: session.model,
                        mode: session.mode,
                    };
                    item.validate()?;
                    Ok(item)
                })
                .collect::<Result<Vec<_>, ProtocolError>>()?;
            Ok(ListAgentSessionsResponse {
                items,
                next_cursor: page.next_cursor,
            })
        })
    }
}

#[cfg(desktop)]
fn remote_history_provider_error(error: crate::agent::AgentPagingError) -> ProtocolError {
    match error {
        crate::agent::AgentPagingError::InvalidRequest(_) => ProtocolError::new(
            ErrorCode::InvalidPage,
            "Agent history page request is invalid",
            false,
        ),
        crate::agent::AgentPagingError::StaleHistory => ProtocolError::new(
            ErrorCode::StaleHistory,
            "Agent task history changed; reload its newest page",
            true,
        ),
        crate::agent::AgentPagingError::HistoryRecordTooLarge => ProtocolError::new(
            ErrorCode::HistoryRecordTooLarge,
            "one Agent history record exceeds Maple's frame limit",
            false,
        ),
        crate::agent::AgentPagingError::Unavailable => ProtocolError::new(
            ErrorCode::Internal,
            "Agent task history is unavailable",
            true,
        ),
    }
}

#[cfg(all(test, desktop))]
mod tests {
    use super::*;
    use crate::{
        remote_protocol::{
            ConnectionStamp, ErrorCode, RemoteAgentPresentedLiveEvent, StreamKind, WireBody,
        },
        remote_transport::{
            AuthorizationSnapshot, CachedEndpointAddr, HostConnectionClock, HostEpoch,
            MapleIrohEndpoint, PairingFence, PairingIncarnation,
        },
        secure_storage::{testing::InMemorySecretStore, DeviceIdentity, DeviceSecretSlot},
    };
    use std::{
        collections::{BTreeMap, HashMap, VecDeque},
        sync::{
            atomic::{AtomicBool, AtomicUsize, Ordering},
            Arc,
        },
    };
    use tokio::sync::Notify;

    const TEST_TIMEOUT: Duration = Duration::from_secs(5);

    struct RpcFixture {
        controller_endpoint: Arc<MapleIrohEndpoint>,
        host_endpoint: Arc<MapleIrohEndpoint>,
        manager: GenerationConnectionManager,
        controller_peer: ConnectedPeer,
        host_peer: ConnectedPeer,
        target_id: String,
    }

    impl RpcFixture {
        async fn close(self) {
            self.manager.clear().expect("clear controller manager");
            tokio::time::timeout(TEST_TIMEOUT, async {
                tokio::join!(self.controller_endpoint.close(), self.host_endpoint.close())
            })
            .await
            .expect("endpoint close timed out");
        }
    }

    fn identity(label: &str) -> DeviceIdentity {
        let store = InMemorySecretStore::default();
        let slot = DeviceSecretSlot::new("cloud.opensecret.maple.rpc-test", label, 1)
            .expect("valid device slot");
        DeviceIdentity::load_or_create(&store, &slot).expect("test identity")
    }

    fn endpoint_id(identity: &DeviceIdentity) -> iroh::EndpointId {
        identity.public_id().parse().expect("endpoint id")
    }

    fn pairing_fence(incarnation: u64) -> PairingFence {
        PairingFence::new(PairingIncarnation::new(incarnation).expect("pairing incarnation"))
            .expect("pairing fence")
    }

    async fn cached_addr(endpoint: &MapleIrohEndpoint) -> CachedEndpointAddr {
        tokio::time::timeout(TEST_TIMEOUT, async {
            loop {
                if let Ok(cached) = endpoint.cached_endpoint_addr(endpoint.endpoint_addr()) {
                    return cached;
                }
                tokio::task::yield_now().await;
            }
        })
        .await
        .expect("endpoint address publication timed out")
    }

    async fn fixture(label: &str) -> RpcFixture {
        fixture_with_target(label, format!("{label}-host-install")).await
    }

    async fn fixture_with_target(label: &str, target_id: impl Into<String>) -> RpcFixture {
        let controller_identity = identity(&format!("{label}-controller"));
        let host_identity = identity(&format!("{label}-host"));
        let controller_id = endpoint_id(&controller_identity);
        let host_id = endpoint_id(&host_identity);
        let target_id = target_id.into();
        let controller_endpoint = Arc::new(
            MapleIrohEndpoint::bind_direct(
                &controller_identity,
                &format!("{label}-controller-install"),
                HostConnectionClock::new(HostEpoch::new(91).expect("controller epoch")),
            )
            .await
            .expect("bind controller"),
        );
        let host_endpoint = Arc::new(
            MapleIrohEndpoint::bind_direct(
                &host_identity,
                &target_id,
                HostConnectionClock::new(HostEpoch::new(41).expect("host epoch")),
            )
            .await
            .expect("bind host"),
        );
        let pairing_incarnation = PairingIncarnation::new(1).expect("pairing incarnation");
        controller_endpoint
            .replace_authorizations(AuthorizationSnapshot {
                account_epoch: 17,
                snapshot_revision: 1,
                incoming_controllers: HashMap::new(),
                outgoing_execution_targets: HashMap::from([(host_id, pairing_incarnation)]),
            })
            .expect("install controller authorization snapshot");
        host_endpoint
            .replace_authorizations(AuthorizationSnapshot {
                account_epoch: 17,
                snapshot_revision: 1,
                incoming_controllers: HashMap::from([(controller_id, pairing_incarnation)]),
                outgoing_execution_targets: HashMap::new(),
            })
            .expect("install host authorization snapshot");
        let cached_host = cached_addr(&host_endpoint).await;
        let manager = GenerationConnectionManager::new_for_pairing(
            controller_id,
            host_id,
            target_id.clone(),
            pairing_fence(1),
            None,
        )
        .expect("generation manager");
        let bootstrap_request_id = format!("{label}-bootstrap");
        let (controller_peer, host_peer) = tokio::time::timeout(TEST_TIMEOUT, async {
            tokio::join!(
                controller_endpoint.connect_and_install_cached(
                    &manager,
                    &cached_host,
                    host_id,
                    &bootstrap_request_id,
                    &target_id,
                ),
                host_endpoint.accept_authenticated(),
            )
        })
        .await
        .expect("pair bootstrap timed out");
        RpcFixture {
            controller_endpoint,
            host_endpoint,
            manager,
            controller_peer: controller_peer.expect("controller peer"),
            host_peer: host_peer.expect("host peer"),
            target_id,
        }
    }

    #[derive(Clone)]
    struct StaticProvider {
        status: RemoteAgentRuntimeStatus,
        calls: Arc<AtomicUsize>,
    }

    impl RemoteRuntimeStatusProvider for StaticProvider {
        fn runtime_status(
            &self,
        ) -> Pin<
            Box<dyn Future<Output = Result<RemoteAgentRuntimeStatus, ProtocolError>> + Send + '_>,
        > {
            self.calls.fetch_add(1, Ordering::SeqCst);
            Box::pin(std::future::ready(Ok(self.status.clone())))
        }
    }

    #[derive(Clone)]
    struct StaticHistoryProvider {
        page: RemoteAgentHistoryPage,
        calls: Arc<AtomicUsize>,
    }

    impl RemoteAgentHistoryProvider for StaticHistoryProvider {
        fn list_agent_history(
            &self,
            _request: &ListAgentHistoryRecordsRequest,
        ) -> Pin<Box<dyn Future<Output = Result<RemoteAgentHistoryPage, ProtocolError>> + Send + '_>>
        {
            self.calls.fetch_add(1, Ordering::SeqCst);
            Box::pin(std::future::ready(Ok(self.page.clone())))
        }
    }

    #[derive(Clone)]
    struct StaticSessionPageProvider {
        page: ListAgentSessionsResponse,
        calls: Arc<AtomicUsize>,
    }

    impl RemoteAgentSessionListProvider for StaticSessionPageProvider {
        fn list_agent_sessions(
            &self,
            _request: &ListAgentSessionsRequest,
        ) -> Pin<
            Box<dyn Future<Output = Result<ListAgentSessionsResponse, ProtocolError>> + Send + '_>,
        > {
            self.calls.fetch_add(1, Ordering::SeqCst);
            Box::pin(std::future::ready(Ok(self.page.clone())))
        }
    }

    #[derive(Clone)]
    struct SnapshotRequiredLiveProvider {
        cancel_calls: Arc<AtomicUsize>,
    }

    struct SnapshotRequiredLiveService {
        cancel_calls: Arc<AtomicUsize>,
    }

    struct SnapshotRequiredPendingAttach {
        cancel_calls: Arc<AtomicUsize>,
    }

    #[async_trait::async_trait]
    impl AgentLiveRemoteAttachProvider for SnapshotRequiredLiveProvider {
        async fn bind(
            &self,
            _authority: VerifiedIncomingPeerAuthorization,
        ) -> Result<Arc<dyn AgentLiveRemoteAttachService>, AgentLiveRemoteAttachError> {
            Ok(Arc::new(SnapshotRequiredLiveService {
                cancel_calls: Arc::clone(&self.cancel_calls),
            }))
        }
    }

    #[async_trait::async_trait]
    impl AgentLiveRemoteAttachService for SnapshotRequiredLiveService {
        async fn begin_newest(
            &self,
            _request: AgentHistoryPageRequest,
            _subscription_capacity: Option<usize>,
        ) -> Result<AgentLiveRemoteHeadBegin, AgentLiveRemoteAttachError> {
            Ok(AgentLiveRemoteHeadBegin {
                page: crate::agent_live_host::AgentLiveSafeHistoryPage {
                    records: Vec::new(),
                    next_cursor: None,
                    history_revision: "0123456789abcdef0123456789abcdef".to_string(),
                },
                through_event_cursor: AgentLiveEventCursor {
                    journal_id: "0123456789abcdef0123456789abcdef".to_string(),
                    sequence: 7,
                },
                live_sessions_complete: true,
                live_sessions: Vec::new(),
                pending: Box::new(SnapshotRequiredPendingAttach {
                    cancel_calls: Arc::clone(&self.cancel_calls),
                }),
            })
        }

        async fn resume(
            &self,
            _cursor: AgentLiveEventCursor,
            _subscription_capacity: Option<usize>,
        ) -> Result<AgentLiveRemoteResume, AgentLiveRemoteAttachError> {
            Err(AgentLiveRemoteAttachError::Host(
                AgentLiveHostError::Coordinator(AgentLiveCoordinatorError::HeadReloadRequired(
                    HeadReloadReason::RetentionGap,
                )),
            ))
        }
    }

    #[async_trait::async_trait]
    impl AgentLiveRemotePendingAttach for SnapshotRequiredPendingAttach {
        async fn activate(
            &mut self,
        ) -> Result<crate::agent_live_host::AgentLiveRemoteActivated, AgentLiveRemoteAttachError>
        {
            Err(AgentLiveRemoteAttachError::Host(
                AgentLiveHostError::Coordinator(AgentLiveCoordinatorError::HeadReloadRequired(
                    HeadReloadReason::PausedSubscriberOverflow,
                )),
            ))
        }

        async fn cancel(self: Box<Self>) -> Result<(), AgentLiveRemoteAttachError> {
            self.cancel_calls.fetch_add(1, Ordering::SeqCst);
            Ok(())
        }
    }

    #[derive(Clone)]
    struct ReplayLiveProvider {
        deliveries: Vec<AgentLiveRemoteDelivery>,
        pending_cancel_calls: Arc<AtomicUsize>,
        unsubscribe_calls: Arc<AtomicUsize>,
    }

    #[derive(Clone)]
    struct CountingBindLiveProvider {
        bind_calls: Arc<AtomicUsize>,
    }

    #[async_trait::async_trait]
    impl AgentLiveRemoteAttachProvider for CountingBindLiveProvider {
        async fn bind(
            &self,
            _authority: VerifiedIncomingPeerAuthorization,
        ) -> Result<Arc<dyn AgentLiveRemoteAttachService>, AgentLiveRemoteAttachError> {
            self.bind_calls.fetch_add(1, Ordering::SeqCst);
            Ok(Arc::new(
                crate::agent_live_host::UnavailableAgentLiveRemoteAttachService,
            ))
        }
    }

    struct ReplayLiveService {
        deliveries: Vec<AgentLiveRemoteDelivery>,
        pending_cancel_calls: Arc<AtomicUsize>,
        unsubscribe_calls: Arc<AtomicUsize>,
    }

    struct ReplayPendingAttach {
        deliveries: Option<Vec<AgentLiveRemoteDelivery>>,
        pending_cancel_calls: Arc<AtomicUsize>,
        unsubscribe_calls: Arc<AtomicUsize>,
    }

    struct ReplayRemoteStream {
        deliveries: VecDeque<AgentLiveRemoteDelivery>,
        unsubscribe_calls: Arc<AtomicUsize>,
    }

    #[async_trait::async_trait]
    impl AgentLiveRemoteAttachProvider for ReplayLiveProvider {
        async fn bind(
            &self,
            _authority: VerifiedIncomingPeerAuthorization,
        ) -> Result<Arc<dyn AgentLiveRemoteAttachService>, AgentLiveRemoteAttachError> {
            Ok(Arc::new(ReplayLiveService {
                deliveries: self.deliveries.clone(),
                pending_cancel_calls: Arc::clone(&self.pending_cancel_calls),
                unsubscribe_calls: Arc::clone(&self.unsubscribe_calls),
            }))
        }
    }

    #[async_trait::async_trait]
    impl AgentLiveRemoteAttachService for ReplayLiveService {
        async fn begin_newest(
            &self,
            _request: AgentHistoryPageRequest,
            _subscription_capacity: Option<usize>,
        ) -> Result<AgentLiveRemoteHeadBegin, AgentLiveRemoteAttachError> {
            Ok(AgentLiveRemoteHeadBegin {
                page: crate::agent_live_host::AgentLiveSafeHistoryPage {
                    records: Vec::new(),
                    next_cursor: None,
                    history_revision: "0123456789abcdef0123456789abcdef".to_string(),
                },
                through_event_cursor: replay_cursor(0),
                live_sessions_complete: true,
                live_sessions: Vec::new(),
                pending: Box::new(ReplayPendingAttach {
                    deliveries: Some(self.deliveries.clone()),
                    pending_cancel_calls: Arc::clone(&self.pending_cancel_calls),
                    unsubscribe_calls: Arc::clone(&self.unsubscribe_calls),
                }),
            })
        }

        async fn resume(
            &self,
            _cursor: AgentLiveEventCursor,
            _subscription_capacity: Option<usize>,
        ) -> Result<AgentLiveRemoteResume, AgentLiveRemoteAttachError> {
            Err(AgentLiveRemoteAttachError::Unavailable)
        }
    }

    #[async_trait::async_trait]
    impl AgentLiveRemotePendingAttach for ReplayPendingAttach {
        async fn activate(
            &mut self,
        ) -> Result<crate::agent_live_host::AgentLiveRemoteActivated, AgentLiveRemoteAttachError>
        {
            let deliveries = self
                .deliveries
                .take()
                .expect("test pending attachment activates once");
            let through_event_cursor = replay_cursor(
                u64::try_from(deliveries.len()).expect("test replay delivery count fits u64"),
            );
            Ok(crate::agent_live_host::AgentLiveRemoteActivated {
                through_event_cursor,
                stream: Box::new(ReplayRemoteStream {
                    deliveries: deliveries.into(),
                    unsubscribe_calls: Arc::clone(&self.unsubscribe_calls),
                }),
            })
        }

        async fn cancel(self: Box<Self>) -> Result<(), AgentLiveRemoteAttachError> {
            self.pending_cancel_calls.fetch_add(1, Ordering::SeqCst);
            Ok(())
        }
    }

    #[async_trait::async_trait]
    impl crate::agent_live_host::AgentLiveRemoteStream for ReplayRemoteStream {
        async fn recv(&mut self) -> Result<AgentLiveRemoteDelivery, AgentLiveRemoteStreamError> {
            match self.deliveries.pop_front() {
                Some(delivery) => Ok(delivery),
                None => std::future::pending().await,
            }
        }

        async fn unsubscribe(self: Box<Self>) -> Result<(), AgentLiveRemoteAttachError> {
            self.unsubscribe_calls.fetch_add(1, Ordering::SeqCst);
            Ok(())
        }
    }

    fn replay_cursor(sequence: u64) -> AgentLiveEventCursor {
        AgentLiveEventCursor {
            journal_id: "0123456789abcdef0123456789abcdef".to_string(),
            sequence,
        }
    }

    fn replay_delivery(sequence: u64, text: String) -> AgentLiveRemoteDelivery {
        AgentLiveRemoteDelivery {
            cursor: replay_cursor(sequence),
            session_id: "session-replay".to_string(),
            run_id: Some("run-replay".to_string()),
            event: MapleLiveEvent::TimelineUpsert {
                event_id: format!("event-{sequence:03}"),
                item: MapleLiveTimelineItem {
                    id: format!("message-{sequence:03}"),
                    item_type: MapleLiveItemType::Message,
                    role: Some(MapleLiveRole::Assistant),
                    title: None,
                    text: Some(text),
                    status: None,
                    created_ms: 1_700_000_000_000 + sequence,
                    merge: MapleLiveMerge::Replace,
                },
            },
        }
    }

    fn history_item(id: impl Into<String>, text: impl Into<String>) -> RemoteAgentTimelineItem {
        RemoteAgentTimelineItem {
            id: id.into(),
            item_type: "message".to_string(),
            role: Some("assistant".to_string()),
            title: None,
            text: Some(text.into()),
            status: None,
            created_ms: 1_700_000_000_000,
            merge: "replace".to_string(),
        }
    }

    fn history_record(
        record_id: impl Into<String>,
        items: Vec<RemoteAgentTimelineItem>,
    ) -> RemoteAgentHistoryRecord {
        RemoteAgentHistoryRecord {
            record_id: record_id.into(),
            role: "assistant".to_string(),
            created_ms: 1_700_000_000_000,
            items,
        }
    }

    fn history_page(records: Vec<RemoteAgentHistoryRecord>) -> RemoteAgentHistoryPage {
        RemoteAgentHistoryPage {
            records,
            next_cursor: None,
            history_revision: "0123456789abcdef0123456789abcdef".to_string(),
        }
    }

    fn running_status() -> RemoteAgentRuntimeStatus {
        RemoteAgentRuntimeStatus {
            running: true,
            project_root: Some("/tmp/maple-remote-rpc".into()),
            model: Some("glm-5-2".into()),
            mode: Some("smart_approve".into()),
            active_runs: BTreeMap::from([("session-01".into(), "run-01".into())]),
        }
    }

    fn test_server_with_live(live: RemoteAgentLiveRpcHost) -> RemoteAgentRpcServer {
        RemoteAgentRpcServer::new(
            Arc::new(StaticProvider {
                status: running_status(),
                calls: Arc::new(AtomicUsize::new(0)),
            }),
            Arc::new(StaticHistoryProvider {
                page: history_page(Vec::new()),
                calls: Arc::new(AtomicUsize::new(0)),
            }),
            Arc::new(StaticSessionPageProvider {
                page: ListAgentSessionsResponse {
                    items: Vec::new(),
                    next_cursor: None,
                },
                calls: Arc::new(AtomicUsize::new(0)),
            }),
            live,
        )
    }

    async fn wait_for_count(counter: &AtomicUsize, expected: usize, label: &str) {
        tokio::time::timeout(TEST_TIMEOUT, async {
            while counter.load(Ordering::SeqCst) != expected {
                tokio::task::yield_now().await;
            }
        })
        .await
        .unwrap_or_else(|_| panic!("timed out waiting for {label}"));
    }

    #[tokio::test]
    async fn runtime_status_roundtrip_decodes_the_remote_result_on_control_lane() {
        let fixture = fixture("status-roundtrip").await;
        let calls = Arc::new(AtomicUsize::new(0));
        let provider = StaticProvider {
            status: running_status(),
            calls: calls.clone(),
        };
        assert_eq!(
            GetRuntimeStatusRequest::new().stream_kind(),
            StreamKind::Control
        );

        let (controller_result, host_result) = tokio::time::timeout(TEST_TIMEOUT, async {
            tokio::join!(
                get_remote_runtime_status(&fixture.manager, "status-request-01"),
                serve_next_remote_runtime_status(
                    &fixture.host_endpoint,
                    &fixture.host_peer,
                    &provider,
                ),
            )
        })
        .await
        .expect("runtime status RPC timed out");
        assert_eq!(
            controller_result.expect("controller result"),
            running_status()
        );
        host_result.expect("host result");
        assert_eq!(calls.load(Ordering::SeqCst), 1);
        fixture.close().await;
    }

    #[tokio::test]
    async fn native_history_page_roundtrip_preserves_one_row_with_multiple_items() {
        let fixture = fixture("history-roundtrip").await;
        let calls = Arc::new(AtomicUsize::new(0));
        let page = history_page(vec![history_record(
            "epoch-record-01",
            vec![
                history_item("message-01", "answer"),
                RemoteAgentTimelineItem {
                    id: "tool-01".to_string(),
                    item_type: "tool".to_string(),
                    role: Some("assistant".to_string()),
                    title: Some(crate::remote_protocol::SAFE_REMOTE_TOOL_TITLE.to_string()),
                    text: None,
                    status: Some("completed".to_string()),
                    created_ms: 1_700_000_000_000,
                    merge: "replace".to_string(),
                },
            ],
        )]);
        let provider = StaticHistoryProvider {
            page: page.clone(),
            calls: calls.clone(),
        };
        let request = ListAgentHistoryRecordsRequest::new("session-01", None, 1)
            .expect("valid history request");

        let (controller_result, host_result) = tokio::time::timeout(TEST_TIMEOUT, async {
            tokio::join!(
                get_remote_agent_history_page(&fixture.manager, "history-request-01", request),
                serve_next_remote_agent_history_page(
                    &fixture.host_endpoint,
                    &fixture.host_peer,
                    &provider,
                ),
            )
        })
        .await
        .expect("history RPC timed out");
        assert_eq!(controller_result.expect("controller result"), page);
        host_result.expect("host result");
        assert_eq!(calls.load(Ordering::SeqCst), 1);
        fixture.close().await;
    }

    #[tokio::test]
    async fn count_page_can_exceed_one_mebibyte_in_aggregate() {
        let fixture = fixture("history-aggregate").await;
        let calls = Arc::new(AtomicUsize::new(0));
        let records = (0..40)
            .map(|index| {
                history_record(
                    format!("epoch-record-{index:02}"),
                    vec![history_item(
                        format!("message-{index:02}"),
                        "x".repeat(30_000),
                    )],
                )
            })
            .collect::<Vec<_>>();
        assert!(
            records
                .iter()
                .flat_map(|record| &record.items)
                .filter_map(|item| item.text.as_ref())
                .map(String::len)
                .sum::<usize>()
                > crate::remote_protocol::MAX_FRAME_BYTES as usize
        );
        let page = history_page(records);
        let provider = StaticHistoryProvider {
            page: page.clone(),
            calls: calls.clone(),
        };
        let request = ListAgentHistoryRecordsRequest::new("session-01", None, 50)
            .expect("valid history request");

        let (controller_result, host_result) = tokio::time::timeout(TEST_TIMEOUT, async {
            tokio::join!(
                get_remote_agent_history_page(
                    &fixture.manager,
                    "history-request-aggregate",
                    request
                ),
                serve_next_remote_agent_history_page(
                    &fixture.host_endpoint,
                    &fixture.host_peer,
                    &provider,
                ),
            )
        })
        .await
        .expect("aggregate history RPC timed out");
        assert_eq!(controller_result.expect("controller result"), page);
        host_result.expect("host result");
        assert_eq!(calls.load(Ordering::SeqCst), 1);
        fixture.close().await;
    }

    #[tokio::test]
    async fn one_oversized_history_record_returns_the_explicit_error() {
        let fixture = fixture("history-oversized").await;
        let half = crate::remote_protocol::MAX_HISTORY_RECORD_PRESENTATION_BYTES / 2;
        let provider = StaticHistoryProvider {
            page: history_page(vec![history_record(
                "epoch-record-oversized",
                vec![
                    history_item("message-a", "a".repeat(half)),
                    history_item("message-b", "b".repeat(half)),
                ],
            )]),
            calls: Arc::new(AtomicUsize::new(0)),
        };
        let request = ListAgentHistoryRecordsRequest::new("session-01", None, 1)
            .expect("valid history request");

        let (controller_result, host_result) = tokio::time::timeout(TEST_TIMEOUT, async {
            tokio::join!(
                get_remote_agent_history_page(
                    &fixture.manager,
                    "history-request-oversized",
                    request
                ),
                serve_next_remote_agent_history_page(
                    &fixture.host_endpoint,
                    &fixture.host_peer,
                    &provider,
                ),
            )
        })
        .await
        .expect("oversized history RPC timed out");
        assert_eq!(
            controller_result
                .expect_err("controller rejects oversized row")
                .code,
            ErrorCode::HistoryRecordTooLarge
        );
        host_result.expect("host writes typed oversized-row error");
        fixture.close().await;
    }

    #[tokio::test]
    async fn session_summary_page_uses_the_same_authenticated_bulk_adapter() {
        let fixture = fixture("session-page-roundtrip").await;
        let calls = Arc::new(AtomicUsize::new(0));
        let page = ListAgentSessionsResponse {
            items: vec![RemoteAgentSessionSummary {
                id: "session-01".to_string(),
                title: "Native task".to_string(),
                project_root: "/tmp/maple".to_string(),
                created_ms: 1_700_000_000_000,
                updated_ms: 1_700_000_000_001,
                page_sort_ms: 1_700_000_000_002,
                message_count: 3,
                model: Some("glm-5-2".to_string()),
                mode: "smart_approve".to_string(),
            }],
            next_cursor: Some("session-cursor-02".to_string()),
        };
        let provider = StaticSessionPageProvider {
            page: page.clone(),
            calls: calls.clone(),
        };
        let request = ListAgentSessionsRequest {
            operation: crate::remote_protocol::AgentSessionListOperation::ListSessions,
            project_root: Some("/tmp/maple".to_string()),
            cursor: None,
            limit: 1,
        };

        let (controller_result, host_result) = tokio::time::timeout(TEST_TIMEOUT, async {
            tokio::join!(
                get_remote_agent_sessions_page(&fixture.manager, "session-page-request", request),
                serve_next_remote_agent_sessions_page(
                    &fixture.host_endpoint,
                    &fixture.host_peer,
                    &provider,
                ),
            )
        })
        .await
        .expect("session page RPC timed out");
        assert_eq!(controller_result.expect("controller result"), page);
        host_result.expect("host result");
        assert_eq!(calls.load(Ordering::SeqCst), 1);
        fixture.close().await;
    }

    #[tokio::test]
    async fn peer_wide_dispatcher_routes_mixed_control_and_bulk_operations_without_theft() {
        let fixture = fixture("peer-wide-dispatch").await;
        let status_calls = Arc::new(AtomicUsize::new(0));
        let history_calls = Arc::new(AtomicUsize::new(0));
        let session_calls = Arc::new(AtomicUsize::new(0));
        let expected_history = history_page(vec![history_record(
            "dispatch-record-01",
            vec![history_item("dispatch-message-01", "answer")],
        )]);
        let expected_sessions = ListAgentSessionsResponse {
            items: vec![RemoteAgentSessionSummary {
                id: "session-dispatch".to_string(),
                title: "Dispatched task".to_string(),
                project_root: "/tmp/maple".to_string(),
                created_ms: 1_700_000_000_000,
                updated_ms: 1_700_000_000_001,
                page_sort_ms: 1_700_000_000_002,
                message_count: 1,
                model: None,
                mode: "smart_approve".to_string(),
            }],
            next_cursor: None,
        };
        let server = RemoteAgentRpcServer::new(
            Arc::new(StaticProvider {
                status: running_status(),
                calls: Arc::clone(&status_calls),
            }),
            Arc::new(StaticHistoryProvider {
                page: expected_history.clone(),
                calls: Arc::clone(&history_calls),
            }),
            Arc::new(StaticSessionPageProvider {
                page: expected_sessions.clone(),
                calls: Arc::clone(&session_calls),
            }),
            RemoteAgentLiveRpcHost::unavailable(),
        );
        let history_request = ListAgentHistoryRecordsRequest::new("session-dispatch", None, 1)
            .expect("history request");
        let sessions_request = ListAgentSessionsRequest {
            operation: crate::remote_protocol::AgentSessionListOperation::ListSessions,
            project_root: Some("/tmp/maple".to_string()),
            cursor: None,
            limit: 1,
        };

        let host = async {
            let mut workers = Vec::new();
            for _ in 0..4 {
                workers.push(
                    serve_next_remote_agent_request(
                        Arc::clone(&fixture.host_endpoint),
                        fixture.host_peer.clone(),
                        server.clone(),
                    )
                    .await
                    .expect("peer-wide dispatcher accepts mixed request"),
                );
            }
            for worker in workers {
                worker
                    .await
                    .expect("peer-wide worker join")
                    .expect("peer-wide worker result");
            }
        };
        let (status, activation, history, sessions, ()) =
            tokio::time::timeout(TEST_TIMEOUT, async {
                tokio::join!(
                    get_remote_runtime_status(&fixture.manager, "dispatch-status"),
                    request_remote_agent_live_activation(
                        &fixture.manager,
                        "dispatch-activate",
                        "missing-attach"
                    ),
                    get_remote_agent_history_page(
                        &fixture.manager,
                        "dispatch-history",
                        history_request
                    ),
                    get_remote_agent_sessions_page(
                        &fixture.manager,
                        "dispatch-sessions",
                        sessions_request
                    ),
                    host,
                )
            })
            .await
            .expect("mixed peer-wide dispatch timed out");
        assert_eq!(status.expect("status result"), running_status());
        assert_eq!(
            activation
                .expect_err("unknown attachment returns a typed live error")
                .code,
            ErrorCode::AgentLiveUnavailable
        );
        assert_eq!(history.expect("history result"), expected_history);
        assert_eq!(sessions.expect("task-list result"), expected_sessions);
        assert_eq!(status_calls.load(Ordering::SeqCst), 1);
        assert_eq!(history_calls.load(Ordering::SeqCst), 1);
        assert_eq!(session_calls.load(Ordering::SeqCst), 1);
        fixture.close().await;
    }

    #[tokio::test]
    async fn activation_snapshot_required_is_correlated_on_control_and_first_events_frame() {
        let fixture = fixture("activation-snapshot-required").await;
        let cancel_calls = Arc::new(AtomicUsize::new(0));
        let server = RemoteAgentRpcServer::new(
            Arc::new(StaticProvider {
                status: running_status(),
                calls: Arc::new(AtomicUsize::new(0)),
            }),
            Arc::new(StaticHistoryProvider {
                page: history_page(Vec::new()),
                calls: Arc::new(AtomicUsize::new(0)),
            }),
            Arc::new(StaticSessionPageProvider {
                page: ListAgentSessionsResponse {
                    items: Vec::new(),
                    next_cursor: None,
                },
                calls: Arc::new(AtomicUsize::new(0)),
            }),
            RemoteAgentLiveRpcHost::new(Arc::new(SnapshotRequiredLiveProvider {
                cancel_calls: Arc::clone(&cancel_calls),
            })),
        );
        let begin_body =
            BeginAgentLiveAttachRequest::new("session-snapshot", 25).expect("valid Begin request");
        let (begin_result, begin_worker) = tokio::time::timeout(TEST_TIMEOUT, async {
            tokio::join!(
                begin_remote_agent_live_attach(
                    &fixture.manager,
                    "snapshot-required-begin",
                    begin_body,
                ),
                async {
                    serve_next_remote_agent_request(
                        Arc::clone(&fixture.host_endpoint),
                        fixture.host_peer.clone(),
                        server.clone(),
                    )
                    .await
                },
            )
        })
        .await
        .expect("Begin snapshot timed out");
        let (snapshot, stream) = begin_result.expect("receive complete C0 snapshot");
        assert_eq!(snapshot.through_event_cursor.sequence, 7);
        let begin_worker = begin_worker.expect("Events dispatcher accepts Begin");

        let host_control = async {
            let mut workers = Vec::new();
            for _ in 0..2 {
                workers.push(
                    serve_next_remote_agent_request(
                        Arc::clone(&fixture.host_endpoint),
                        fixture.host_peer.clone(),
                        server.clone(),
                    )
                    .await
                    .expect("dispatcher accepts Activate then cleanup Cancel"),
                );
            }
            for worker in workers {
                worker
                    .await
                    .expect("Control worker join")
                    .expect("Control worker result");
            }
        };
        let (activation, ()) = tokio::time::timeout(TEST_TIMEOUT, async {
            tokio::join!(
                activate_remote_agent_live_attach(
                    "snapshot-required-activate",
                    "snapshot-required-cleanup",
                    stream,
                    |_| async { Ok(()) },
                ),
                host_control,
            )
        })
        .await
        .expect("activation SnapshotRequired flow timed out");
        let error = match activation {
            Ok(_) => panic!("activation requires a fresh authoritative snapshot"),
            Err(error) => error,
        };
        assert_eq!(
            error.code,
            ErrorCode::SnapshotRequired,
            "unexpected activation error: {error:?}"
        );
        assert!(error.message.contains("PausedSubscriberOverflow"));
        begin_worker
            .await
            .expect("Events Begin worker join")
            .expect("Events Begin worker returns after terminal frame");
        assert_eq!(cancel_calls.load(Ordering::SeqCst), 1);
        fixture.close().await;
    }

    #[tokio::test]
    async fn activation_drains_replay_larger_than_256_kib_before_resolving() {
        let fixture = fixture("activation-large-replay").await;
        let deliveries = (1..=10)
            .map(|sequence| replay_delivery(sequence, "x".repeat(40 * 1_024)))
            .collect::<Vec<_>>();
        let replay_bytes = deliveries
            .iter()
            .filter_map(|delivery| match &delivery.event {
                MapleLiveEvent::TimelineUpsert { item, .. } => item.text.as_ref(),
                _ => None,
            })
            .map(String::len)
            .sum::<usize>();
        assert!(replay_bytes > 256 * 1_024);
        let pending_cancel_calls = Arc::new(AtomicUsize::new(0));
        let unsubscribe_calls = Arc::new(AtomicUsize::new(0));
        let live = RemoteAgentLiveRpcHost::new(Arc::new(ReplayLiveProvider {
            deliveries,
            pending_cancel_calls: Arc::clone(&pending_cancel_calls),
            unsubscribe_calls: Arc::clone(&unsubscribe_calls),
        }));
        let server = test_server_with_live(live);
        let begin_body = BeginAgentLiveAttachRequest::new("session-replay", 25)
            .expect("valid replay Begin request");
        let (begin_result, begin_dispatch) = tokio::time::timeout(TEST_TIMEOUT, async {
            tokio::join!(
                begin_remote_agent_live_attach(&fixture.manager, "large-replay-begin", begin_body,),
                serve_next_remote_agent_request(
                    Arc::clone(&fixture.host_endpoint),
                    fixture.host_peer.clone(),
                    server.clone(),
                ),
            )
        })
        .await
        .expect("large replay Begin timed out");
        let (snapshot, stream) = begin_result.expect("receive replay C0 snapshot");
        assert_eq!(snapshot.through_event_cursor.sequence, 0);
        let begin_worker = begin_dispatch.expect("Events dispatcher accepts replay Begin");

        let applied_events = Arc::new(AtomicUsize::new(0));
        let applied_bytes = Arc::new(AtomicUsize::new(0));
        let activation = {
            let applied_events = Arc::clone(&applied_events);
            let applied_bytes = Arc::clone(&applied_bytes);
            activate_remote_agent_live_attach(
                "large-replay-activate",
                "large-replay-activation-cleanup",
                stream,
                move |delivery| {
                    let applied_events = Arc::clone(&applied_events);
                    let applied_bytes = Arc::clone(&applied_bytes);
                    async move {
                        let text_bytes = match delivery.event {
                            RemoteAgentPresentedLiveEvent::TimelineUpsert { item } => {
                                item.text.map_or(0, |text| text.len())
                            }
                            _ => 0,
                        };
                        applied_events.fetch_add(1, Ordering::SeqCst);
                        applied_bytes.fetch_add(text_bytes, Ordering::SeqCst);
                        Ok(())
                    }
                },
            )
        };
        let host_activation = async {
            let worker = serve_next_remote_agent_request(
                Arc::clone(&fixture.host_endpoint),
                fixture.host_peer.clone(),
                server.clone(),
            )
            .await
            .expect("dispatcher accepts replay Activate");
            worker
                .await
                .expect("replay Activate worker join")
                .expect("replay Activate worker result");
        };
        let (activation, ()) = tokio::time::timeout(TEST_TIMEOUT, async {
            tokio::join!(activation, host_activation)
        })
        .await
        .expect("large replay activation timed out");
        let (start, stream) = activation.expect("large replay activation succeeds");
        assert_eq!(start.through_event_cursor.sequence, 10);
        assert_eq!(applied_events.load(Ordering::SeqCst), 10);
        assert_eq!(applied_bytes.load(Ordering::SeqCst), replay_bytes);

        let (cancel_result, cancel_dispatch) = tokio::time::timeout(TEST_TIMEOUT, async {
            tokio::join!(
                stream.cancel("large-replay-cancel"),
                serve_next_remote_agent_request(
                    Arc::clone(&fixture.host_endpoint),
                    fixture.host_peer.clone(),
                    server,
                ),
            )
        })
        .await
        .expect("large replay cancellation timed out");
        cancel_result.expect("active replay cancellation acknowledged");
        cancel_dispatch
            .expect("dispatcher accepts replay Cancel")
            .await
            .expect("replay Cancel worker join")
            .expect("replay Cancel worker result");
        let events_error = begin_worker
            .await
            .expect("large replay Events worker join")
            .expect_err("active cancellation terminates the Events owner");
        assert_eq!(events_error.code, ErrorCode::AgentLiveUnavailable);
        assert_eq!(pending_cancel_calls.load(Ordering::SeqCst), 0);
        assert_eq!(unsubscribe_calls.load(Ordering::SeqCst), 1);
        fixture.close().await;
    }

    #[tokio::test]
    async fn replay_callback_failure_cancels_the_learned_live_stream_id() {
        let fixture = fixture("activation-callback-failure").await;
        let deliveries = (1..=2)
            .map(|sequence| replay_delivery(sequence, format!("replay-{sequence}")))
            .collect::<Vec<_>>();
        let pending_cancel_calls = Arc::new(AtomicUsize::new(0));
        let unsubscribe_calls = Arc::new(AtomicUsize::new(0));
        let live = RemoteAgentLiveRpcHost::new(Arc::new(ReplayLiveProvider {
            deliveries,
            pending_cancel_calls: Arc::clone(&pending_cancel_calls),
            unsubscribe_calls: Arc::clone(&unsubscribe_calls),
        }));
        let server = test_server_with_live(live);
        let (begin_result, begin_dispatch) = tokio::time::timeout(TEST_TIMEOUT, async {
            tokio::join!(
                begin_remote_agent_live_attach(
                    &fixture.manager,
                    "callback-failure-begin",
                    BeginAgentLiveAttachRequest::new("session-replay", 25)
                        .expect("valid callback-failure Begin request"),
                ),
                serve_next_remote_agent_request(
                    Arc::clone(&fixture.host_endpoint),
                    fixture.host_peer.clone(),
                    server.clone(),
                ),
            )
        })
        .await
        .expect("callback-failure Begin timed out");
        let (_snapshot, stream) = begin_result.expect("receive callback-failure C0 snapshot");
        let begin_worker = begin_dispatch.expect("Events dispatcher accepts callback Begin");
        let callback_calls = Arc::new(AtomicUsize::new(0));

        let host_control = async {
            let mut workers = Vec::new();
            for _ in 0..2 {
                workers.push(
                    serve_next_remote_agent_request(
                        Arc::clone(&fixture.host_endpoint),
                        fixture.host_peer.clone(),
                        server.clone(),
                    )
                    .await
                    .expect("dispatcher accepts callback Activate then learned-id Cancel"),
                );
            }
            for worker in workers {
                worker
                    .await
                    .expect("callback Control worker join")
                    .expect("callback Control worker result");
            }
        };
        let activation = {
            let callback_calls = Arc::clone(&callback_calls);
            activate_remote_agent_live_attach(
                "callback-failure-activate",
                "callback-failure-cleanup",
                stream,
                move |_| {
                    let callback_calls = Arc::clone(&callback_calls);
                    async move {
                        callback_calls.fetch_add(1, Ordering::SeqCst);
                        Err(ProtocolError::new(
                            ErrorCode::Internal,
                            "test replay projection failed",
                            false,
                        ))
                    }
                },
            )
        };
        let (activation, ()) = tokio::time::timeout(TEST_TIMEOUT, async {
            tokio::join!(activation, host_control)
        })
        .await
        .expect("callback-failure activation timed out");
        let activation_error = match activation {
            Ok(_) => panic!("replay callback failure must remain visible"),
            Err(error) => error,
        };
        assert_eq!(activation_error.message, "test replay projection failed");
        let events_error = begin_worker
            .await
            .expect("callback Events worker join")
            .expect_err("learned-id cancellation terminates the Events owner");
        assert_eq!(events_error.code, ErrorCode::AgentLiveUnavailable);
        assert_eq!(callback_calls.load(Ordering::SeqCst), 1);
        assert_eq!(
            pending_cancel_calls.load(Ordering::SeqCst),
            0,
            "cleanup must use the live-stream alias learned from StreamStart"
        );
        assert_eq!(unsubscribe_calls.load(Ordering::SeqCst), 1);
        fixture.close().await;
    }

    #[tokio::test]
    async fn cancelling_controller_activation_after_stream_start_wakes_host_unsubscribe() {
        let fixture = fixture("activation-future-cancel").await;
        let pending_cancel_calls = Arc::new(AtomicUsize::new(0));
        let unsubscribe_calls = Arc::new(AtomicUsize::new(0));
        let live = RemoteAgentLiveRpcHost::new(Arc::new(ReplayLiveProvider {
            deliveries: vec![replay_delivery(1, "activation-cancel".to_string())],
            pending_cancel_calls: Arc::clone(&pending_cancel_calls),
            unsubscribe_calls: Arc::clone(&unsubscribe_calls),
        }));
        let server = test_server_with_live(live);
        let (begin_result, begin_dispatch) = tokio::time::timeout(TEST_TIMEOUT, async {
            tokio::join!(
                begin_remote_agent_live_attach(
                    &fixture.manager,
                    "activation-cancel-begin",
                    BeginAgentLiveAttachRequest::new("session-replay", 25)
                        .expect("valid activation-cancel Begin request"),
                ),
                serve_next_remote_agent_request(
                    Arc::clone(&fixture.host_endpoint),
                    fixture.host_peer.clone(),
                    server.clone(),
                ),
            )
        })
        .await
        .expect("activation-cancel Begin timed out");
        let (_snapshot, stream) = begin_result.expect("receive activation-cancel C0 snapshot");
        let begin_worker = begin_dispatch.expect("Events dispatcher accepts cancellation Begin");
        let callback_started = Arc::new(Notify::new());
        let activation_task = {
            let callback_started = Arc::clone(&callback_started);
            tokio::spawn(activate_remote_agent_live_attach(
                "activation-cancel-activate",
                "activation-cancel-explicit-cleanup",
                stream,
                move |_| {
                    let callback_started = Arc::clone(&callback_started);
                    async move {
                        callback_started.notify_one();
                        std::future::pending().await
                    }
                },
            ))
        };
        let activation_worker = serve_next_remote_agent_request(
            Arc::clone(&fixture.host_endpoint),
            fixture.host_peer.clone(),
            server,
        )
        .await
        .expect("dispatcher accepts cancellable Activate");
        tokio::time::timeout(TEST_TIMEOUT, callback_started.notified())
            .await
            .expect("controller observed StreamStart and first replay event");
        activation_task.abort();
        let _ = activation_task.await;
        let activation_result = activation_worker
            .await
            .expect("cancellable Activate worker join");
        if let Err(error) = activation_result {
            assert!(matches!(
                error.code,
                ErrorCode::TransportUnavailable | ErrorCode::AgentLiveUnavailable
            ));
        }
        wait_for_count(
            unsubscribe_calls.as_ref(),
            1,
            "host unsubscribe after controller activation cancellation",
        )
        .await;
        let events_error = begin_worker
            .await
            .expect("activation-cancel Events worker join")
            .expect_err("abandoned Events response terminates its native owner");
        assert!(matches!(
            events_error.code,
            ErrorCode::TransportUnavailable | ErrorCode::AgentLiveUnavailable
        ));
        assert_eq!(pending_cancel_calls.load(Ordering::SeqCst), 0);
        fixture.close().await;
    }

    #[tokio::test]
    async fn dropping_worker_waiter_detaches_owner_and_stop_still_cancels_pending_attach() {
        let fixture = fixture("worker-detach-stop").await;
        let pending_cancel_calls = Arc::new(AtomicUsize::new(0));
        let unsubscribe_calls = Arc::new(AtomicUsize::new(0));
        let live = RemoteAgentLiveRpcHost::new(Arc::new(ReplayLiveProvider {
            deliveries: Vec::new(),
            pending_cancel_calls: Arc::clone(&pending_cancel_calls),
            unsubscribe_calls,
        }));
        let server = test_server_with_live(live.clone());
        let (begin_result, begin_dispatch) = tokio::time::timeout(TEST_TIMEOUT, async {
            tokio::join!(
                begin_remote_agent_live_attach(
                    &fixture.manager,
                    "worker-detach-begin",
                    BeginAgentLiveAttachRequest::new("session-replay", 25)
                        .expect("valid worker-detach Begin request"),
                ),
                serve_next_remote_agent_request(
                    Arc::clone(&fixture.host_endpoint),
                    fixture.host_peer.clone(),
                    server,
                ),
            )
        })
        .await
        .expect("worker-detach Begin timed out");
        let (_snapshot, stream) = begin_result.expect("receive worker-detach C0 snapshot");
        let worker = begin_dispatch.expect("dispatcher returns opaque worker owner");
        drop(worker);
        drop(stream);
        wait_for_count(
            pending_cancel_calls.as_ref(),
            1,
            "detached owner pending cancellation",
        )
        .await;
        assert!(live
            .inner
            .state
            .lock()
            .expect("live registry")
            .pending
            .is_empty());
        fixture.close().await;
    }

    #[tokio::test]
    async fn pending_attach_ttl_cancels_native_token_before_releasing_occupancy() {
        let fixture = fixture("pending-ttl").await;
        let pending_cancel_calls = Arc::new(AtomicUsize::new(0));
        let live = RemoteAgentLiveRpcHost::new(Arc::new(ReplayLiveProvider {
            deliveries: Vec::new(),
            pending_cancel_calls: Arc::clone(&pending_cancel_calls),
            unsubscribe_calls: Arc::new(AtomicUsize::new(0)),
        }));
        let (begin_result, host_result) = tokio::time::timeout(TEST_TIMEOUT, async {
            tokio::join!(
                begin_remote_agent_live_attach(
                    &fixture.manager,
                    "pending-ttl-begin",
                    BeginAgentLiveAttachRequest::new("session-replay", 25)
                        .expect("valid pending-TTL Begin request"),
                ),
                remote_live_server::serve_next_remote_agent_live_request_for_test_with_ttl(
                    &fixture.host_endpoint,
                    &fixture.host_peer,
                    &live,
                    Duration::from_millis(20),
                ),
            )
        })
        .await
        .expect("pending TTL flow timed out");
        let (_snapshot, stream) = begin_result.expect("receive pending-TTL C0 snapshot");
        let host_error = host_result.expect_err("TTL terminates the pending Events owner");
        assert_eq!(host_error.code, ErrorCode::AgentLiveUnavailable);
        assert_eq!(pending_cancel_calls.load(Ordering::SeqCst), 1);
        assert!(live
            .inner
            .state
            .lock()
            .expect("live registry")
            .pending
            .is_empty());
        drop(stream);
        fixture.close().await;
    }

    #[tokio::test]
    async fn c0_to_activate_handover_uses_captured_peer_and_old_owner_cleans_on_close() {
        let fixture = fixture("activation-peer-handover").await;
        let pending_cancel_calls = Arc::new(AtomicUsize::new(0));
        let live = RemoteAgentLiveRpcHost::new(Arc::new(ReplayLiveProvider {
            deliveries: Vec::new(),
            pending_cancel_calls: Arc::clone(&pending_cancel_calls),
            unsubscribe_calls: Arc::new(AtomicUsize::new(0)),
        }));
        let server = test_server_with_live(live);
        let (begin_result, begin_dispatch) = tokio::time::timeout(TEST_TIMEOUT, async {
            tokio::join!(
                begin_remote_agent_live_attach(
                    &fixture.manager,
                    "handover-begin",
                    BeginAgentLiveAttachRequest::new("session-replay", 25)
                        .expect("valid handover Begin request"),
                ),
                serve_next_remote_agent_request(
                    Arc::clone(&fixture.host_endpoint),
                    fixture.host_peer.clone(),
                    server,
                ),
            )
        })
        .await
        .expect("handover Begin timed out");
        let (_snapshot, stream) = begin_result.expect("receive handover C0 snapshot");
        let begin_worker = begin_dispatch.expect("Events dispatcher accepts handover Begin");
        let old_stamp = fixture.controller_peer.connection_stamp();
        let cached_host = cached_addr(&fixture.host_endpoint).await;
        let host_id = fixture.controller_peer.remote_id();
        let (new_controller_peer, new_host_peer) = tokio::time::timeout(TEST_TIMEOUT, async {
            tokio::join!(
                fixture.controller_endpoint.connect_and_install_cached(
                    &fixture.manager,
                    &cached_host,
                    host_id,
                    "handover-reconnect",
                    &fixture.target_id,
                ),
                fixture.host_endpoint.accept_authenticated(),
            )
        })
        .await
        .expect("same-epoch handover timed out");
        let new_controller_peer = new_controller_peer.expect("install handover controller peer");
        let new_host_peer = new_host_peer.expect("accept handover host peer");
        assert_eq!(
            new_controller_peer.connection_stamp().host_epoch(),
            old_stamp.host_epoch()
        );
        assert!(new_controller_peer.connection_stamp().generation() > old_stamp.generation());

        let activation = tokio::time::timeout(
            TEST_TIMEOUT,
            activate_remote_agent_live_attach(
                "handover-activate",
                "handover-cleanup",
                stream,
                |_| async { Ok(()) },
            ),
        )
        .await
        .expect("captured old peer activation must fail promptly");
        let activation_error = match activation {
            Ok(_) => panic!("C0 attachment cannot migrate to the manager's new peer"),
            Err(error) => error,
        };
        assert!(matches!(
            activation_error.code,
            ErrorCode::TransportUnavailable | ErrorCode::AgentLiveUnavailable
        ));
        wait_for_count(
            pending_cancel_calls.as_ref(),
            1,
            "old peer pending cancellation after handover",
        )
        .await;
        begin_worker
            .await
            .expect("old peer Events worker join")
            .expect_err("handover closes the old Events owner");
        drop((new_controller_peer, new_host_peer));
        fixture.close().await;
    }

    #[tokio::test]
    async fn peer_wide_events_dispatcher_routes_resume_snapshot_required() {
        let fixture = fixture("resume-snapshot-required").await;
        let server = RemoteAgentRpcServer::new(
            Arc::new(StaticProvider {
                status: running_status(),
                calls: Arc::new(AtomicUsize::new(0)),
            }),
            Arc::new(StaticHistoryProvider {
                page: history_page(Vec::new()),
                calls: Arc::new(AtomicUsize::new(0)),
            }),
            Arc::new(StaticSessionPageProvider {
                page: ListAgentSessionsResponse {
                    items: Vec::new(),
                    next_cursor: None,
                },
                calls: Arc::new(AtomicUsize::new(0)),
            }),
            RemoteAgentLiveRpcHost::new(Arc::new(SnapshotRequiredLiveProvider {
                cancel_calls: Arc::new(AtomicUsize::new(0)),
            })),
        );
        let cursor = RemoteLiveEventCursor {
            journal_id: "0123456789abcdef0123456789abcdef".to_string(),
            sequence: 7,
        };
        let (controller, host_worker) = tokio::time::timeout(TEST_TIMEOUT, async {
            tokio::join!(
                resume_remote_agent_live_events(
                    &fixture.manager,
                    "resume-snapshot-required",
                    cursor,
                    fixture.controller_peer.connection_stamp().host_epoch(),
                ),
                async {
                    let worker = serve_next_remote_agent_request(
                        Arc::clone(&fixture.host_endpoint),
                        fixture.host_peer.clone(),
                        server,
                    )
                    .await
                    .expect("dispatcher accepts Resume");
                    worker.await.expect("Resume worker join")
                },
            )
        })
        .await
        .expect("Resume SnapshotRequired flow timed out");
        let error = match controller {
            Err(error) => error,
            Ok(_) => panic!("resume requires a fresh authoritative snapshot"),
        };
        assert_eq!(error.code, ErrorCode::SnapshotRequired);
        assert!(error.message.contains("RetentionGap"));
        host_worker.expect("Resume worker writes SnapshotRequired");
        fixture.close().await;
    }

    #[tokio::test]
    async fn stale_resume_origin_epoch_is_rejected_before_live_provider_bind() {
        let fixture = fixture("resume-origin-epoch-before-bind").await;
        let bind_calls = Arc::new(AtomicUsize::new(0));
        let live = RemoteAgentLiveRpcHost::new(Arc::new(CountingBindLiveProvider {
            bind_calls: Arc::clone(&bind_calls),
        }));
        let authority = fixture
            .host_endpoint
            .verified_incoming_peer_authorization(&fixture.host_peer)
            .expect("verified host authority");
        let current_stamp = fixture.host_peer.connection_stamp();
        let stale_epoch = current_stamp
            .host_epoch()
            .checked_add(1)
            .expect("test host epoch can advance");
        let request = ResumeAgentLiveEventsRequest::new(
            RemoteLiveEventCursor {
                journal_id: "0123456789abcdef0123456789abcdef".to_string(),
                sequence: 7,
            },
            stale_epoch,
        )
        .expect("valid stale-origin Resume request");

        let error = match remote_live_server::prepare_remote_agent_live_resume(
            &live,
            &authority,
            current_stamp,
            &request,
        )
        .await
        {
            Ok(_) => panic!("stale host epoch must fail before provider binding"),
            Err(error) => error,
        };
        assert_eq!(error.code, ErrorCode::StaleGeneration);
        assert_eq!(
            bind_calls.load(Ordering::SeqCst),
            0,
            "stale host-epoch validation must precede provider binding"
        );
        fixture.close().await;
    }

    #[tokio::test]
    async fn wrong_target_and_generation_are_rejected_before_host_dispatch() {
        let fixture = fixture("status-fence").await;
        let request = RequestEnvelope {
            protocol_version: PROTOCOL_VERSION,
            request_id: "wrong-target".into(),
            execution_target_id: "different-host".into(),
            direction: PeerDirection::ControllerToHost,
            connection_stamp: fixture.controller_peer.connection_stamp(),
            body: GetRuntimeStatusRequest::new(),
        };
        let target_error = fixture
            .controller_peer
            .request::<_, GetRuntimeStatusResponse>(&request)
            .await
            .expect_err("wrong target must fail");
        assert_eq!(target_error.code, ErrorCode::WrongEndpoint);

        let request = RequestEnvelope {
            protocol_version: PROTOCOL_VERSION,
            request_id: "wrong-generation".into(),
            execution_target_id: fixture.target_id.clone(),
            direction: PeerDirection::ControllerToHost,
            connection_stamp: ConnectionStamp::new(
                fixture.controller_peer.connection_stamp().host_epoch(),
                fixture.controller_peer.connection_stamp().generation() + 1,
            )
            .expect("different valid stamp"),
            body: GetRuntimeStatusRequest::new(),
        };
        let generation_error = fixture
            .controller_peer
            .request::<_, GetRuntimeStatusResponse>(&request)
            .await
            .expect_err("stale generation must fail");
        assert_eq!(generation_error.code, ErrorCode::StaleGeneration);

        let history_body = ListAgentHistoryRecordsRequest::new("session-01", None, 1)
            .expect("valid history request");
        let wrong_target_history = RequestEnvelope {
            protocol_version: PROTOCOL_VERSION,
            request_id: "wrong-history-target".into(),
            execution_target_id: "different-host".into(),
            direction: PeerDirection::ControllerToHost,
            connection_stamp: fixture.controller_peer.connection_stamp(),
            body: history_body.clone(),
        };
        let history_target_error = match fixture
            .controller_peer
            .start_streaming_request(wrong_target_history)
            .await
        {
            Err(error) => error,
            Ok(_) => panic!("wrong history target must fail before opening the stream"),
        };
        assert_eq!(history_target_error.code, ErrorCode::WrongEndpoint);

        let wrong_generation_history = RequestEnvelope {
            protocol_version: PROTOCOL_VERSION,
            request_id: "wrong-history-generation".into(),
            execution_target_id: fixture.target_id.clone(),
            direction: PeerDirection::ControllerToHost,
            connection_stamp: ConnectionStamp::new(
                fixture.controller_peer.connection_stamp().host_epoch(),
                fixture.controller_peer.connection_stamp().generation() + 1,
            )
            .expect("different valid stamp"),
            body: history_body,
        };
        let history_generation_error = match fixture
            .controller_peer
            .start_streaming_request(wrong_generation_history)
            .await
        {
            Err(error) => error,
            Ok(_) => panic!("stale history generation must fail before opening the stream"),
        };
        assert_eq!(history_generation_error.code, ErrorCode::StaleGeneration);
        fixture.close().await;
    }

    #[tokio::test]
    async fn generation_manager_rejects_a_different_pairing_fence_before_dial() {
        let controller_identity = identity("wrong-pair-controller");
        let host_identity = identity("wrong-pair-host");
        let controller_id = endpoint_id(&controller_identity);
        let host_id = endpoint_id(&host_identity);
        let target_id = "wrong-pair-host-install";
        let controller = MapleIrohEndpoint::bind_direct(
            &controller_identity,
            "wrong-pair-controller-install",
            HostConnectionClock::new(HostEpoch::new(92).expect("controller epoch")),
        )
        .await
        .expect("bind controller");
        let host = MapleIrohEndpoint::bind_direct(
            &host_identity,
            target_id,
            HostConnectionClock::new(HostEpoch::new(42).expect("host epoch")),
        )
        .await
        .expect("bind host");
        controller
            .authorize_outgoing_execution_target(host_id)
            .expect("authorize outgoing host");
        host.authorize_incoming_controller(controller_id)
            .expect("authorize incoming controller");
        let cached_host = cached_addr(&host).await;
        let wrong_manager = GenerationConnectionManager::new_for_pairing(
            controller_id,
            host_id,
            target_id,
            pairing_fence(2),
            None,
        )
        .expect("wrong-fence manager is structurally valid");
        let error = controller
            .connect_and_install_cached(
                &wrong_manager,
                &cached_host,
                host_id,
                "wrong-pair-bootstrap",
                target_id,
            )
            .await
            .expect_err("pairing fence mismatch must fail before dial");
        assert_eq!(error.code, ErrorCode::Unauthorized);
        tokio::time::timeout(TEST_TIMEOUT, async {
            tokio::join!(controller.close(), host.close())
        })
        .await
        .expect("endpoint close timed out");
    }

    #[tokio::test]
    async fn revoked_host_generation_is_rejected_before_provider_dispatch() {
        let fixture = fixture("status-revoked").await;
        let calls = Arc::new(AtomicUsize::new(0));
        let provider = StaticProvider {
            status: running_status(),
            calls: calls.clone(),
        };
        fixture
            .host_endpoint
            .replace_authorizations(AuthorizationSnapshot {
                account_epoch: 17,
                snapshot_revision: 2,
                incoming_controllers: HashMap::new(),
                outgoing_execution_targets: HashMap::new(),
            })
            .expect("revoke controller through a newer authorization snapshot");

        let error =
            serve_next_remote_runtime_status(&fixture.host_endpoint, &fixture.host_peer, &provider)
                .await
                .expect_err("revoked generation must fail closed");
        assert_eq!(error.code, ErrorCode::Revoked);
        assert_eq!(calls.load(Ordering::SeqCst), 0);
        fixture.close().await;
    }

    struct PendingProvider {
        started: Arc<Notify>,
        dropped: Arc<Notify>,
    }

    struct DropSignal(Arc<Notify>);

    impl Drop for DropSignal {
        fn drop(&mut self) {
            self.0.notify_one();
        }
    }

    impl RemoteRuntimeStatusProvider for PendingProvider {
        fn runtime_status(
            &self,
        ) -> Pin<
            Box<dyn Future<Output = Result<RemoteAgentRuntimeStatus, ProtocolError>> + Send + '_>,
        > {
            let started = self.started.clone();
            let dropped = self.dropped.clone();
            Box::pin(async move {
                let _drop_signal = DropSignal(dropped);
                started.notify_one();
                std::future::pending().await
            })
        }
    }

    #[tokio::test]
    async fn controller_cancellation_drops_provider_work_and_releases_the_lane() {
        let fixture = fixture("status-cancel").await;
        let started = Arc::new(Notify::new());
        let dropped = Arc::new(Notify::new());
        let provider = PendingProvider {
            started: started.clone(),
            dropped: dropped.clone(),
        };
        let controller_peer = fixture.controller_peer.clone();
        let controller_task = tokio::spawn(async move {
            get_remote_runtime_status_on_peer(&controller_peer, "status-cancel-01").await
        });
        let host_result = tokio::time::timeout(TEST_TIMEOUT, async {
            let (host_result, ()) = tokio::join!(
                serve_next_remote_runtime_status(
                    &fixture.host_endpoint,
                    &fixture.host_peer,
                    &provider,
                ),
                async {
                    started.notified().await;
                    controller_task.abort();
                    let _ = controller_task.await;
                    dropped.notified().await;
                },
            );
            host_result
        })
        .await
        .expect("host cancellation did not finish");
        let host_error = host_result.expect_err("cancelled request must not report success");
        assert_eq!(host_error.code, ErrorCode::TransportUnavailable);

        let calls = Arc::new(AtomicUsize::new(0));
        let succeeding = StaticProvider {
            status: running_status(),
            calls: calls.clone(),
        };
        let (controller_result, host_result) = tokio::time::timeout(TEST_TIMEOUT, async {
            tokio::join!(
                get_remote_runtime_status(&fixture.manager, "status-after-cancel"),
                serve_next_remote_runtime_status(
                    &fixture.host_endpoint,
                    &fixture.host_peer,
                    &succeeding,
                ),
            )
        })
        .await
        .expect("follow-up status RPC timed out");
        assert_eq!(
            controller_result.expect("follow-up result"),
            running_status()
        );
        host_result.expect("follow-up host result");
        assert_eq!(calls.load(Ordering::SeqCst), 1);
        fixture.close().await;
    }

    #[tokio::test]
    async fn provider_timeout_returns_a_typed_retryable_error() {
        let fixture = fixture("status-timeout").await;
        let provider = PendingProvider {
            started: Arc::new(Notify::new()),
            dropped: Arc::new(Notify::new()),
        };
        let (controller_result, host_result) = tokio::time::timeout(TEST_TIMEOUT, async {
            tokio::join!(
                get_remote_runtime_status(&fixture.manager, "status-timeout-01"),
                serve_next_remote_runtime_status_with_timeout(
                    &fixture.host_endpoint,
                    &fixture.host_peer,
                    &provider,
                    Duration::from_millis(25),
                ),
            )
        })
        .await
        .expect("timeout RPC did not terminate");
        let controller_error = controller_result.expect_err("controller must receive timeout");
        assert_eq!(controller_error.code, ErrorCode::TransportUnavailable);
        assert!(controller_error.retryable);
        host_result.expect("host writes the typed timeout response");
        fixture.close().await;
    }

    struct ActivationInFlightGuard(Arc<AtomicBool>);

    impl Drop for ActivationInFlightGuard {
        fn drop(&mut self) {
            self.0.store(false, Ordering::Release);
        }
    }

    struct StalledPendingAttach {
        activation_in_flight: Arc<AtomicBool>,
        activation_started: Arc<Notify>,
        cancel_calls: Arc<AtomicUsize>,
        cancel_started: Arc<Notify>,
        allow_cancel_ack: Arc<Notify>,
    }

    #[async_trait::async_trait]
    impl AgentLiveRemotePendingAttach for StalledPendingAttach {
        async fn activate(
            &mut self,
        ) -> Result<crate::agent_live_host::AgentLiveRemoteActivated, AgentLiveRemoteAttachError>
        {
            assert!(!self.activation_in_flight.swap(true, Ordering::AcqRel));
            let _in_flight = ActivationInFlightGuard(Arc::clone(&self.activation_in_flight));
            self.activation_started.notify_one();
            std::future::pending().await
        }

        async fn cancel(self: Box<Self>) -> Result<(), AgentLiveRemoteAttachError> {
            assert!(
                !self.activation_in_flight.load(Ordering::Acquire),
                "activation future must be dropped before native cancel"
            );
            self.cancel_calls.fetch_add(1, Ordering::SeqCst);
            self.cancel_started.notify_one();
            self.allow_cancel_ack.notified().await;
            Ok(())
        }
    }

    #[tokio::test]
    async fn stalled_activation_is_dropped_then_cancelled_once_before_occupancy_release() {
        let fixture = fixture("live-stalled-activation").await;
        let authority = fixture
            .host_endpoint
            .verified_incoming_peer_authorization(&fixture.host_peer)
            .expect("verified host authority");
        let rpc = RemoteAgentLiveRpcHost::unavailable();
        let service: Arc<dyn AgentLiveRemoteAttachService> =
            Arc::new(crate::agent_live_host::UnavailableAgentLiveRemoteAttachService);
        let cancellation = Arc::new(RemoteAgentLiveCancellation::default());
        let (activate_send, _activate_receive) = oneshot::channel();
        rpc.install_pending(
            "attach-stalled".to_string(),
            authority.clone(),
            Arc::clone(&service),
            activate_send,
            Arc::clone(&cancellation),
        )
        .await
        .expect("install pending lifecycle");
        rpc.arm_pending(&authority, "attach-stalled")
            .await
            .expect("arm pending lifecycle");
        drop(
            rpc.take_pending_for_activation(&authority, "attach-stalled")
                .await
                .expect("move pending lifecycle to Activating"),
        );
        rpc.name_activating_stream("attach-stalled", "live-stalled", &authority)
            .await
            .expect("name activating stream");

        let activation_in_flight = Arc::new(AtomicBool::new(false));
        let activation_started = Arc::new(Notify::new());
        let cancel_calls = Arc::new(AtomicUsize::new(0));
        let cancel_started = Arc::new(Notify::new());
        let allow_cancel_ack = Arc::new(Notify::new());
        let mut pending: Box<dyn AgentLiveRemotePendingAttach> = Box::new(StalledPendingAttach {
            activation_in_flight: Arc::clone(&activation_in_flight),
            activation_started: Arc::clone(&activation_started),
            cancel_calls: Arc::clone(&cancel_calls),
            cancel_started: Arc::clone(&cancel_started),
            allow_cancel_ack: Arc::clone(&allow_cancel_ack),
        });
        let interrupt = Arc::new(Notify::new());
        let outcome = {
            let interrupted = Arc::clone(&interrupt);
            let activation = remote_live_server::activate_pending_until_interrupted(
                pending.as_mut(),
                async move {
                    interrupted.notified().await;
                    live_lifecycle_unavailable("test activation interruption")
                },
            );
            tokio::pin!(activation);
            tokio::select! {
                _ = activation_started.notified() => {}
                _ = &mut activation => panic!("stalled activation completed unexpectedly"),
            }
            assert!(activation_in_flight.load(Ordering::Acquire));
            interrupt.notify_one();
            activation.await
        };
        assert!(matches!(
            outcome,
            remote_live_server::PendingActivationResult::Interrupted(_)
        ));
        assert!(!activation_in_flight.load(Ordering::Acquire));

        let finish_rpc = rpc.clone();
        let finish_authority = authority.clone();
        let finish_cancellation = Arc::clone(&cancellation);
        let finish = tokio::spawn(async move {
            remote_live_server::finish_activating_pending_lifecycle(
                &finish_rpc,
                "attach-stalled",
                &finish_authority,
                &finish_cancellation,
                pending,
                live_lifecycle_unavailable("test activation interruption"),
            )
            .await
        });
        cancel_started.notified().await;
        assert_eq!(cancel_calls.load(Ordering::SeqCst), 1);
        assert!(
            rpc.inner
                .state
                .lock()
                .expect("live registry")
                .activating
                .contains_key("attach-stalled"),
            "occupancy must remain until native cancel acknowledges"
        );
        assert_eq!(
            rpc.reserve_activating(
                "live-overlap".to_string(),
                authority.clone(),
                Arc::clone(&service),
                Arc::new(RemoteAgentLiveCancellation::default()),
            )
            .await
            .expect_err("stable occupancy must remain fail-closed")
            .code,
            ErrorCode::TransportUnavailable
        );
        allow_cancel_ack.notify_one();
        assert_eq!(
            finish
                .await
                .expect("cleanup task join")
                .expect_err("terminal owner error remains visible")
                .code,
            ErrorCode::AgentLiveUnavailable
        );
        assert_eq!(cancel_calls.load(Ordering::SeqCst), 1);
        assert!(
            !rpc.inner
                .state
                .lock()
                .expect("live registry")
                .activating
                .contains_key("attach-stalled"),
            "occupancy is released only after native cancel acknowledgement"
        );
        rpc.reserve_activating(
            "live-after-ack".to_string(),
            authority.clone(),
            service,
            Arc::new(RemoteAgentLiveCancellation::default()),
        )
        .await
        .expect("capacity is reusable after acknowledged cleanup");
        rpc.remove_activating("live-after-ack", &authority).await;
        fixture.close().await;
    }

    #[tokio::test]
    async fn concurrent_pending_cancels_resolve_the_active_alias_and_share_cleanup_error() {
        let fixture = fixture("live-active-alias").await;
        let authority = fixture
            .host_endpoint
            .verified_incoming_peer_authorization(&fixture.host_peer)
            .expect("verified host authority");
        let rpc = RemoteAgentLiveRpcHost::unavailable();
        let service: Arc<dyn AgentLiveRemoteAttachService> =
            Arc::new(crate::agent_live_host::UnavailableAgentLiveRemoteAttachService);
        let cancellation = Arc::new(RemoteAgentLiveCancellation::default());
        let (activate_send, _activate_receive) = oneshot::channel();
        rpc.install_pending(
            "attach-alias".to_string(),
            authority.clone(),
            Arc::clone(&service),
            activate_send,
            Arc::clone(&cancellation),
        )
        .await
        .expect("install pending lifecycle");
        rpc.arm_pending(&authority, "attach-alias")
            .await
            .expect("arm pending lifecycle");
        drop(
            rpc.take_pending_for_activation(&authority, "attach-alias")
                .await
                .expect("move pending lifecycle to Activating"),
        );
        rpc.name_activating_stream("attach-alias", "live-active", &authority)
            .await
            .expect("name activating stream");
        rpc.promote_activating("attach-alias", "live-active", &authority)
            .await
            .expect("promote lifecycle");

        let first = rpc.cancel_lifecycle(
            &authority,
            AgentLiveCancelKind::PendingAttach,
            "attach-alias",
        );
        let second = rpc.cancel_lifecycle(
            &authority,
            AgentLiveCancelKind::PendingAttach,
            "attach-alias",
        );
        let cleanup = async {
            cancellation.wait_requested().await;
            tokio::task::yield_now().await;
            cancellation.complete(Err(ProtocolError::new(
                ErrorCode::Internal,
                "test native cleanup failed",
                false,
            )));
        };
        let (first, second, ()) = tokio::join!(first, second, cleanup);
        assert_eq!(
            first.expect_err("first cancel shares cleanup error").code,
            ErrorCode::Internal
        );
        assert_eq!(
            second
                .expect_err("concurrent cancel shares cleanup error")
                .code,
            ErrorCode::Internal
        );
        {
            let mut state = rpc.inner.state.lock().expect("live registry");
            prune_closed_lifecycles(&mut state);
            let active = state
                .active
                .get("live-active")
                .expect("failed cleanup retains active occupancy");
            assert_eq!(active.activation_id, "attach-alias");
            assert!(stable_occupancy_in_use(&state, &authority));
        }
        assert_eq!(
            rpc.reserve_activating(
                "live-overlap".to_string(),
                authority,
                service,
                Arc::new(RemoteAgentLiveCancellation::default()),
            )
            .await
            .expect_err("cleanup failure must retain stable occupancy")
            .code,
            ErrorCode::TransportUnavailable
        );
        fixture.close().await;
    }

    #[tokio::test]
    async fn retained_peer_authorization_refresh_awaits_live_cleanup_before_reoccupancy() {
        let fixture = fixture_with_target(
            "live-retained-auth-refresh",
            "11111111-1111-4111-8111-111111111111",
        )
        .await;
        let old_authority = fixture
            .host_endpoint
            .verified_incoming_peer_authorization(&fixture.host_peer)
            .expect("verified old host authority");
        let journal_root = tempfile::tempdir().expect("temporary live host root");
        let live_host = crate::agent_live_host::AgentLiveHost::open(journal_root.path())
            .expect("open test live host");
        let initial_binding =
            crate::agent_live_binding::VerifiedAgentTargetBinding::from_verified_remote_adapter(
                "account-scope-a".to_string(),
                7,
                old_authority.clone(),
            )
            .expect("construct initial verified binding");
        assert!(matches!(
            live_host
                .bind_verified(initial_binding)
                .await
                .expect("bind initial remote authority"),
            crate::agent_live_host::AgentLiveHostBindOutcome::Bound(_)
        ));

        let rpc = RemoteAgentLiveRpcHost::unavailable();
        let service: Arc<dyn AgentLiveRemoteAttachService> =
            Arc::new(crate::agent_live_host::UnavailableAgentLiveRemoteAttachService);
        let cancellation = Arc::new(RemoteAgentLiveCancellation::default());
        rpc.reserve_activating(
            "live-old-authority".to_string(),
            old_authority.clone(),
            Arc::clone(&service),
            Arc::clone(&cancellation),
        )
        .await
        .expect("reserve old-authority lifecycle");
        rpc.promote_activating("live-old-authority", "live-old-authority", &old_authority)
            .await
            .expect("promote idle old-authority lifecycle");

        let cleanup_started = Arc::new(Notify::new());
        let allow_cleanup_ack = Arc::new(Notify::new());
        let unsubscribe_calls = Arc::new(AtomicUsize::new(0));
        let cleanup_owner = {
            let rpc = rpc.clone();
            let authority = old_authority.clone();
            let cancellation = Arc::clone(&cancellation);
            let cleanup_started = Arc::clone(&cleanup_started);
            let allow_cleanup_ack = Arc::clone(&allow_cleanup_ack);
            let unsubscribe_calls = Arc::clone(&unsubscribe_calls);
            tokio::spawn(async move {
                cancellation.wait_requested().await;
                cleanup_started.notify_one();
                allow_cleanup_ack.notified().await;
                unsubscribe_calls.fetch_add(1, Ordering::SeqCst);
                cancellation.complete(Ok(()));
                rpc.remove_active("live-old-authority", &authority).await;
            })
        };

        let controller_endpoint = fixture.host_peer.remote_id();
        let transition_receipt = fixture
            .host_endpoint
            .replace_authorizations(AuthorizationSnapshot {
                account_epoch: 17,
                snapshot_revision: 2,
                incoming_controllers: HashMap::from([(
                    controller_endpoint,
                    PairingIncarnation::new(1).expect("pairing incarnation"),
                )]),
                outgoing_execution_targets: HashMap::new(),
            })
            .expect("install retained-peer authorization revision");
        fixture
            .host_endpoint
            .validate_current_incoming_peer(&fixture.host_peer)
            .expect("retained peer connection stays current");
        assert!(old_authority.revalidate_current().is_err());

        let transition = {
            let live_host = live_host.clone();
            let rpc = Arc::new(rpc.clone());
            tokio::spawn(async move {
                live_host
                    .apply_authorization_transition(transition_receipt, rpc)
                    .await
            })
        };
        tokio::time::timeout(TEST_TIMEOUT, cleanup_started.notified())
            .await
            .expect("authorization refresh wakes idle live owner");
        assert!(
            !transition.is_finished(),
            "authorization transition must await native unsubscribe acknowledgement"
        );
        assert!(
            rpc.inner
                .state
                .lock()
                .expect("live registry")
                .active
                .contains_key("live-old-authority"),
            "old occupancy remains fail-closed before cleanup acknowledgement"
        );
        allow_cleanup_ack.notify_one();
        transition
            .await
            .expect("authorization transition task join")
            .expect("authorization transition cleanup succeeds");
        cleanup_owner.await.expect("idle cleanup owner task join");
        assert_eq!(unsubscribe_calls.load(Ordering::SeqCst), 1);

        let refreshed_authority = fixture
            .host_endpoint
            .verified_incoming_peer_authorization(&fixture.host_peer)
            .expect("verified refreshed host authority");
        assert!(old_authority.same_admission_instance(&refreshed_authority));
        assert_ne!(
            old_authority.authorization(),
            refreshed_authority.authorization()
        );
        let refreshed_binding =
            crate::agent_live_binding::VerifiedAgentTargetBinding::from_verified_remote_adapter(
                "account-scope-a".to_string(),
                7,
                refreshed_authority.clone(),
            )
            .expect("construct refreshed verified binding");
        assert!(matches!(
            live_host
                .bind_verified(refreshed_binding)
                .await
                .expect("bind refreshed remote authority"),
            crate::agent_live_host::AgentLiveHostBindOutcome::Bound(_)
        ));
        rpc.reserve_activating(
            "live-refreshed-authority".to_string(),
            refreshed_authority.clone(),
            service,
            Arc::new(RemoteAgentLiveCancellation::default()),
        )
        .await
        .expect("refreshed authority can occupy only after old cleanup ACK");
        rpc.remove_activating("live-refreshed-authority", &refreshed_authority)
            .await;
        fixture.close().await;
    }

    #[test]
    fn aggregate_c0_overlay_larger_than_one_frame_is_split_into_encodable_item_frames() {
        let live_items = (0..6)
            .map(|index| history_item(format!("live-item-{index}"), "x".repeat(190 * 1_024)))
            .collect::<Vec<_>>();
        assert!(
            live_items
                .iter()
                .filter_map(|item| item.text.as_ref())
                .map(String::len)
                .sum::<usize>()
                > crate::remote_protocol::MAX_FRAME_BYTES as usize
        );
        let mut frames = Vec::new();
        remote_live_server::append_live_session_snapshot_frames(
            &mut frames,
            vec![RemoteAgentLiveSessionSnapshot {
                session_id: "session-large-c0".to_string(),
                live_items,
            }],
        )
        .expect("split a valid aggregate C0 overlay");
        assert_eq!(frames.len(), 7);
        assert!(matches!(
            frames.first(),
            Some(AgentLiveStreamFrame::LiveSessionStart {
                index: 0,
                item_count: 6,
                ..
            })
        ));
        for frame in frames {
            frame.validate().expect("split frame is valid");
            let response = ResponseEnvelope {
                protocol_version: PROTOCOL_VERSION,
                request_id: "large-c0-response".to_string(),
                execution_target_id: "host-01".to_string(),
                connection_stamp: ConnectionStamp::new(41, 1).expect("valid stamp"),
                result: Ok(frame),
            };
            validate_frame_encodable(&response).expect("each C0 item frame fits transport cap");
        }
    }

    #[test]
    fn c0_projection_budget_rejects_oversized_account_before_retaining_the_next_item() {
        let mut retained_bytes = LIVE_PROJECTION_OUTER_OVERHEAD_BYTES;
        accumulate_remote_live_projection_bytes(
            &mut retained_bytes,
            remote_live_projection_session_wire_bytes("session-budget").expect("session charge"),
        )
        .expect("session header fits account budget");
        let item = history_item(
            "budget-item",
            "x".repeat(crate::remote_protocol::MAX_LIVE_ITEM_PRESENTATION_BYTES),
        );
        let item_bytes =
            remote_live_projection_item_wire_bytes(&item).expect("bounded item charge");
        let fitting_items = (MAX_LIVE_PROJECTION_BYTES_PER_ACCOUNT - retained_bytes) / item_bytes;
        assert!(fitting_items < crate::remote_protocol::MAX_LIVE_ITEMS_PER_ACCOUNT);
        for _ in 0..fitting_items {
            accumulate_remote_live_projection_bytes(&mut retained_bytes, item_bytes)
                .expect("item within native-parity account budget");
        }
        let before_rejection = retained_bytes;
        let error = accumulate_remote_live_projection_bytes(&mut retained_bytes, item_bytes)
            .expect_err("next individually valid item exceeds aggregate account budget");
        assert_eq!(error.code, ErrorCode::InvalidFrame);
        assert_eq!(retained_bytes, before_rejection);
    }

    #[test]
    fn status_request_schema_cannot_name_an_arbitrary_tauri_command() {
        let request = serde_json::json!({
            "operation": "agent_clear_user_data",
            "userId": "user-a"
        });
        assert!(serde_json::from_value::<GetRuntimeStatusRequest>(request).is_err());
    }
}
