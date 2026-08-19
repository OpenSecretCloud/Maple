//! Common host composition for native Agent history paging and synchronized
//! live delivery.
//!
//! This module is an internal native composition seam. It does not by itself
//! claim or expose a remote streaming protocol. In particular, there is no
//! global event sink and no request-supplied execution target: a coordinator
//! is created only from a currently revalidated [`AgentLiveBindingLease`].

#![allow(
    dead_code,
    reason = "the common live host remains fail-closed until its native adapters are complete"
)]

#[cfg(test)]
use crate::remote_transport::InstalledAuthorizationContext;
use crate::{
    agent::{AgentHistoryPage, AgentHistoryPageRequest, AgentLiveEventCursor, AgentPagingError},
    agent_event_journal::{
        prepare_live_event_journal_parent, LiveEventAccountOwner, LiveEventCursor,
        LiveEventJournal, LiveEventJournalActivationError, LiveEventJournalError,
        LiveEventJournalReseedObligation, LiveEventJournalReseedRequired,
        LiveEventJournalRetirementToken, DEFAULT_LIVE_EVENT_JOURNAL_LIMITS,
    },
    agent_live_authority::{
        AgentDurableHeadCommitReceipt, AgentDurableStableOperationId, AgentLiveDataOwnerKey,
        VerifiedJournalReseedAuthority,
    },
    agent_live_binding::{
        AgentLiveBindOutcome, AgentLiveBindingError, AgentLiveBindingLease,
        AgentLiveBindingRegistry, AgentLiveRotationObligation, VerifiedAgentTargetBinding,
    },
    agent_live_coordinator::{
        target_bound_owner, AgentLiveCoordinator, AgentLiveCoordinatorError, AgentLiveDelivery,
        AgentLiveIngressLease, AgentLivePublishEvent, AgentLiveReceiveError, AgentLiveSeal,
        AgentLiveSealReason, AgentLiveSubscription, IngressEventId, MapleLiveEvent, MapleLiveMerge,
        MapleLiveTimelineItem,
    },
    remote_protocol::MAX_HISTORY_RECORD_PRESENTATION_BYTES,
    remote_transport::{AuthorizationTransitionReceipt, VerifiedIncomingPeerAuthorization},
};
use std::{
    collections::{HashMap, HashSet},
    fmt,
    future::Future,
    io::Write,
    path::Path,
    pin::Pin,
    sync::Arc,
};
use tokio::sync::Mutex;

const JOURNAL_DIRECTORY_NAME: &str = "journal";
const MAX_SYNCHRONIZED_HISTORY_RECORDS: usize = 50;
const MAX_SYNCHRONIZED_ITEMS_PER_RECORD: usize = 200;
const MAX_SYNCHRONIZED_HISTORY_TOKEN_BYTES: usize = 512;
const MAX_SYNCHRONIZED_ROLE_BYTES: usize = 128;
const MAX_SYNCHRONIZED_SESSION_ID_BYTES: usize = 128;
const MAX_SYNCHRONIZED_LIVE_SESSIONS: usize = 64;
const MAX_SYNCHRONIZED_LIVE_ITEMS_PER_SESSION: usize = 200;
const MAX_SYNCHRONIZED_LIVE_ITEMS_PER_ACCOUNT: usize = 512;
const MAX_JAVASCRIPT_SAFE_INTEGER: u64 = 9_007_199_254_740_991;

/// Narrow adapter over Maple's account-and-generation-bound runtime handle.
///
/// The production implementation belongs beside `AgentRuntimeHandle`, where
/// its private account scope and generation can be read without accepting
/// identity claims from a Tauri command or RPC envelope. Implementations must
/// call the runtime handle's common `list_session_records_page` method.
pub(crate) trait AgentHistoryPageProvider: Send + Sync + 'static {
    fn account_scope(&self) -> &str;

    fn account_generation(&self) -> u64;

    fn list_session_records_page(
        &self,
        request: AgentHistoryPageRequest,
    ) -> Pin<Box<dyn Future<Output = Result<AgentHistoryPage, AgentPagingError>> + Send + '_>>;
}

/// Exact authenticated peer context added only for synchronized operations.
/// Plain persisted-history paging remains independent of this binding.
///
/// Implementations are security-sensitive native adapters. Every identity
/// method must describe the same account-bound runtime and authenticated
/// connection; none may be populated from renderer or RPC scalar fields. The
/// generation fence must be the actual lifecycle barrier also taken by
/// `clear_data`/`clear_history`, and must prevent generation advancement for
/// its full lifetime.
pub(crate) trait AgentLiveAttachProvider: AgentHistoryPageProvider {
    type RuntimeGenerationFence: Send + 'static;

    /// Exact authenticated Iroh peer. It is captured by the native connection
    /// adapter, never accepted from a renderer/RPC field.
    fn controller_endpoint(&self) -> iroh::EndpointId;

    /// Re-run the endpoint's native current-peer verifier and return its opaque
    /// capability. The capability constructor itself rechecks the installed
    /// admission record; callers cannot reproduce it from copied scalar data.
    fn reverify_current_binding(
        &self,
    ) -> Pin<
        Box<
            dyn Future<Output = Result<VerifiedAgentTargetBinding, AgentLiveBindingError>>
                + Send
                + '_,
        >,
    >;

    /// Acquire the same runtime lifecycle fence used by generation-changing
    /// clear/reset operations. Host mutation methods acquire this before the
    /// host lifecycle lock and retain it through the durable coordinator call.
    fn acquire_runtime_generation_fence(
        &self,
    ) -> Pin<
        Box<
            dyn Future<Output = Result<Self::RuntimeGenerationFence, AgentPagingError>> + Send + '_,
        >,
    >;

    fn verify_current_generation_under_fence<'a>(
        &'a self,
        fence: &'a Self::RuntimeGenerationFence,
    ) -> Pin<Box<dyn Future<Output = Result<(), AgentPagingError>> + Send + 'a>>;
}

/// Trusted targeted-revocation hook implemented by the composed Tauri attach
/// manager. Registry revocation happens first; this hook then immediately
/// drops the exact peer's pending/active channel without inventing an ambient
/// owner or a second manager inside the host.
#[async_trait::async_trait]
pub(crate) trait AgentLivePeerRevocationHook: Send + Sync {
    async fn revoke_exact_peer(
        &self,
        revoked: &AgentLiveBindingLease,
    ) -> Result<(), AgentLiveHostError>;
}

/// Reviewed native projection boundary for synchronized attachment state.
///
/// The coordinator payload is already a closed, bounded Maple type. This last
/// projection converts its absolute head rows and persisted Goose page into
/// closed presentation types, and converts each delivery into the consumer's
/// internal event type. No raw [`AgentHistoryPage`] crosses the synchronized
/// attachment boundary.
pub(crate) trait AgentLiveDeliveryProjector: Send + Sync + 'static {
    type Delivery: Send + 'static;
    type Error: Send + 'static;

    /// Consume the rich local page and return only the reviewed, bounded
    /// presentation contract. Implementations must independently enforce the
    /// requested native-row limit and reject arbitrary tool input/output.
    fn project_history_page(
        &self,
        page: AgentHistoryPage,
        requested_limit: Option<usize>,
    ) -> Result<AgentLiveSafeHistoryPage, Self::Error>;

    fn project_head_items(
        &self,
        items: &[MapleLiveTimelineItem],
    ) -> Result<Vec<MapleLiveTimelineItem>, Self::Error>;

    fn project_delivery(&self, delivery: &AgentLiveDelivery)
        -> Result<Self::Delivery, Self::Error>;
}

/// Closed synchronized history row. Rich tool input/output fields are absent
/// by construction; the projector can only return the reviewed live item
/// contract that this host validates again before disclosure.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub(crate) struct AgentLiveSafeHistoryRecord {
    pub(crate) record_id: String,
    pub(crate) role: String,
    pub(crate) created_ms: u64,
    pub(crate) items: Vec<MapleLiveTimelineItem>,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub(crate) struct AgentLiveSafeHistoryPage {
    pub(crate) records: Vec<AgentLiveSafeHistoryRecord>,
    pub(crate) next_cursor: Option<String>,
    pub(crate) history_revision: String,
}

#[derive(Debug)]
pub(crate) enum AgentLiveHostError {
    Binding(AgentLiveBindingError),
    Paging(AgentPagingError),
    Coordinator(AgentLiveCoordinatorError),
    Journal(LiveEventJournalError),
    JournalReseedRequired(Box<LiveEventJournalReseedRequired>),
    RuntimeOwnerMismatch,
    OrdinaryPageContainedLiveState,
    SynchronizedPageProjectionRejected,
    SynchronizedHistoryRecordTooLarge,
    HeadAttachRequiresNewestPage,
    BoundContextSealed,
    BoundContextRevoked,
    RotationMustBeSealed,
    RotationMustBeDurable,
    RotationAlreadyDurable,
    RotationUnavailable,
    AuthorizationCleanupPending,
    NonAdjacentAccountGeneration,
    JournalWorkerUnavailable,
    ReseedContextMustBeClosed,
}

impl fmt::Display for AgentLiveHostError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Binding(error) => error.fmt(formatter),
            Self::Paging(error) => error.fmt(formatter),
            Self::Coordinator(error) => error.fmt(formatter),
            Self::Journal(error) => error.fmt(formatter),
            Self::JournalReseedRequired(_) => {
                formatter.write_str("the Agent live journal requires a verified durable reseed")
            }
            Self::RuntimeOwnerMismatch => {
                formatter.write_str("Agent runtime belongs to another live binding")
            }
            Self::OrdinaryPageContainedLiveState => formatter
                .write_str("ordinary Agent history pages must not contain synchronized live state"),
            Self::SynchronizedPageProjectionRejected => {
                formatter.write_str("synchronized Agent history projection is invalid")
            }
            Self::SynchronizedHistoryRecordTooLarge => formatter
                .write_str("one synchronized Agent history record is too large to display safely"),
            Self::HeadAttachRequiresNewestPage => {
                formatter.write_str("synchronized Agent attach requires the newest history page")
            }
            Self::BoundContextSealed => {
                formatter.write_str("the bound Agent live context is sealed")
            }
            Self::BoundContextRevoked => {
                formatter.write_str("the bound Agent live context is revoked")
            }
            Self::RotationMustBeSealed => formatter.write_str(
                "the previous Agent live context must be sealed before journal rotation",
            ),
            Self::RotationMustBeDurable => formatter
                .write_str("the Agent live journal must be durably rotated before binding commit"),
            Self::RotationAlreadyDurable => {
                formatter.write_str("the Agent live journal rotation is already durable")
            }
            Self::RotationUnavailable => {
                formatter.write_str("no recoverable Agent live binding rotation is pending")
            }
            Self::AuthorizationCleanupPending => formatter.write_str(
                "an earlier Agent authorization transition still requires exact cleanup",
            ),
            Self::NonAdjacentAccountGeneration => formatter
                .write_str("Agent live journal rotation requires an adjacent account generation"),
            Self::JournalWorkerUnavailable => {
                formatter.write_str("the Agent live journal worker is unavailable")
            }
            Self::ReseedContextMustBeClosed => {
                formatter.write_str("the Agent live context must be closed before journal reseed")
            }
        }
    }
}

impl std::error::Error for AgentLiveHostError {}

impl From<AgentLiveBindingError> for AgentLiveHostError {
    fn from(error: AgentLiveBindingError) -> Self {
        Self::Binding(error)
    }
}

impl From<AgentPagingError> for AgentLiveHostError {
    fn from(error: AgentPagingError) -> Self {
        Self::Paging(error)
    }
}

impl From<AgentLiveCoordinatorError> for AgentLiveHostError {
    fn from(error: AgentLiveCoordinatorError) -> Self {
        match error {
            AgentLiveCoordinatorError::ReseedRequired(required) => {
                Self::JournalReseedRequired(required)
            }
            error => Self::Coordinator(error),
        }
    }
}

impl From<LiveEventJournalError> for AgentLiveHostError {
    fn from(error: LiveEventJournalError) -> Self {
        Self::Journal(error)
    }
}

#[derive(Debug)]
pub(crate) enum AgentLiveAttachError<E> {
    Host(AgentLiveHostError),
    Projection(E),
}

impl<E> From<AgentLiveHostError> for AgentLiveAttachError<E> {
    fn from(error: AgentLiveHostError) -> Self {
        Self::Host(error)
    }
}

#[derive(Debug)]
pub(crate) enum AgentLiveStreamError<E> {
    Host(AgentLiveHostError),
    Receive(AgentLiveReceiveError),
    Projection(E),
}

type BoundContextKey = AgentLiveDataOwnerKey;

enum BoundContextSlot {
    Active(ActiveBoundContext),
    /// Installed before awaiting the FIFO seal. Cancellation leaves this exact
    /// context retryable but non-recreatable and every lookup fails closed.
    Sealing {
        active: ActiveBoundContext,
        reason: AgentLiveSealReason,
        revoked: bool,
    },
    Sealed(AgentLiveSeal),
    /// The same stable journal key was atomically advanced to the next data
    /// generation. This old lease must never enter the retirement protocol.
    Superseded(AgentLiveSeal),
    Retiring {
        token: LiveEventJournalRetirementToken,
        sealed: AgentLiveSeal,
        revoked: bool,
    },
    Retired {
        sealed: AgentLiveSeal,
        revoked: bool,
    },
    Revoked(Option<AgentLiveSeal>),
}

#[derive(Clone)]
struct ActiveBoundContext {
    coordinator: AgentLiveCoordinator,
}

/// Exact producer admission for one bound owner and one session/run route.
///
/// This capability is deliberately obtained separately from publication.
/// Rollover invalidates its hidden ingress generation, and `publish` never
/// looks up or substitutes a newer capability on the caller's behalf.
#[derive(Clone)]
pub(crate) struct AgentLiveIngressPublisher {
    binding: AgentLiveBindingLease,
    ingress: AgentLiveIngressLease,
}

impl AgentLiveIngressPublisher {
    pub(crate) fn session_id(&self) -> &str {
        self.ingress.session_id()
    }

    pub(crate) fn run_id(&self) -> Option<&str> {
        self.ingress.run_id()
    }

    /// Derive a typed event ID only from the native durable-operation proof.
    /// Owner, route, projection schema, journal namespace, and payload
    /// commitment are rechecked by the ingress capability.
    pub(crate) fn event_id(
        &self,
        stable_operation: &AgentDurableStableOperationId,
    ) -> Result<IngressEventId, AgentLiveHostError> {
        self.ingress.event_id(stable_operation).map_err(Into::into)
    }
}

#[derive(Clone)]
struct PendingAccountRetirement {
    /// Exact committed lease retained so an impossible owner-derivation error
    /// remains fail-closed and diagnosable rather than losing recovery state.
    lease: Option<AgentLiveBindingLease>,
    /// Data-lineage key is used only to join an already materialized context.
    /// The journal owner below remains the sole disk authority.
    key: Option<BoundContextKey>,
    owner: Option<LiveEventAccountOwner>,
}

#[derive(Clone)]
pub(crate) struct AgentLiveHost {
    bindings: AgentLiveBindingRegistry,
    journal: LiveEventJournal<MapleLiveEvent>,
    contexts: Arc<Mutex<HashMap<BoundContextKey, BoundContextSlot>>>,
    /// Host-owned recovery copy. A cancelled edge call may drop its local
    /// handle, but cannot make a binding Transition permanently unreachable.
    pending_rotation: Arc<Mutex<Option<AgentLiveRotationObligation>>>,
    /// Exact committed data owner captured from a binding transition. This is
    /// independent of lazy coordinator materialization, so a never-attached
    /// account journal is still retired on an account-epoch change.
    pending_account_retirement: Arc<Mutex<Option<PendingAccountRetirement>>>,
    /// Exact leases already revoked by a consumed endpoint transition receipt
    /// but not yet acknowledged closed by the composed delivery manager.
    pending_peer_revocations: Arc<Mutex<Vec<AgentLiveBindingLease>>>,
    /// Serializes binding transitions with coordinator creation, publication,
    /// head attachment, sealing, and revocation. A subscription never holds
    /// this lock while waiting for its next delivery.
    lifecycle: Arc<Mutex<()>>,
}

impl AgentLiveHost {
    /// Open the single process-wide journal beneath a dedicated owner-only
    /// parent. Callers should pass an app-local-data child such as
    /// `app_local_data/agent-live`, not the broad app-local-data directory.
    pub(crate) fn open(journal_parent: &Path) -> Result<Self, AgentLiveHostError> {
        prepare_live_event_journal_parent(journal_parent)?;
        let journal = LiveEventJournal::open(
            journal_parent.join(JOURNAL_DIRECTORY_NAME),
            DEFAULT_LIVE_EVENT_JOURNAL_LIMITS,
        )?;
        Ok(Self::from_journal(journal))
    }

    fn from_journal(journal: LiveEventJournal<MapleLiveEvent>) -> Self {
        Self {
            bindings: AgentLiveBindingRegistry::new(),
            journal,
            contexts: Arc::new(Mutex::new(HashMap::new())),
            pending_rotation: Arc::new(Mutex::new(None)),
            pending_account_retirement: Arc::new(Mutex::new(None)),
            pending_peer_revocations: Arc::new(Mutex::new(Vec::new())),
            lifecycle: Arc::new(Mutex::new(())),
        }
    }

    /// Consume only the capability minted by the verified native endpoint
    /// adapter.
    /// A target change or account-generation change returns an obligation and
    /// leaves all synchronized operations fail-closed until it is completed.
    pub(crate) async fn bind_verified(
        &self,
        verified: VerifiedAgentTargetBinding,
    ) -> Result<AgentLiveHostBindOutcome, AgentLiveHostError> {
        let _lifecycle = self.lifecycle.lock().await;
        if self.pending_account_retirement.lock().await.is_some()
            || !self.pending_peer_revocations.lock().await.is_empty()
        {
            return Err(AgentLiveHostError::AuthorizationCleanupPending);
        }
        // Acquire before the registry mutation. Once bind_or_refresh returns a
        // Transition there is no further await before its recovery obligation
        // is stored, so cancellation cannot make the Transition unreachable.
        let mut pending_rotation = self.pending_rotation.lock().await;
        match self.bindings.bind_or_refresh(verified).await? {
            AgentLiveBindOutcome::Bound(lease) => Ok(AgentLiveHostBindOutcome::Bound(lease)),
            AgentLiveBindOutcome::RotationRequired(obligation) => {
                *pending_rotation = Some(obligation.clone());
                Ok(AgentLiveHostBindOutcome::RotationRequired(
                    AgentLiveHostRotation {
                        obligation,
                        sealed: None,
                        retirement: None,
                        journal_rotated: false,
                    },
                ))
            }
        }
    }

    /// Plain storage paging deliberately remains available without a live
    /// binding. The provider's common host method must return `None/None` for
    /// the live overlay and event cursor on every ordinary page.
    pub(crate) async fn ordinary_history_page<P>(
        &self,
        provider: &P,
        request: AgentHistoryPageRequest,
    ) -> Result<AgentHistoryPage, AgentLiveHostError>
    where
        P: AgentHistoryPageProvider,
    {
        let page = provider.list_session_records_page(request).await?;
        if page.live_items.is_some() || page.through_event_cursor.is_some() {
            return Err(AgentLiveHostError::OrdinaryPageContainedLiveState);
        }
        Ok(page)
    }

    /// Construct an account-bound manager. No target parameter is accepted;
    /// the exact target is recovered from the registry's current lease.
    pub(crate) async fn attach_manager<P, D>(
        &self,
        provider: P,
        projector: D,
    ) -> Result<AgentLiveAttachManager<P, D>, AgentLiveHostError>
    where
        P: AgentLiveAttachProvider,
        D: AgentLiveDeliveryProjector,
    {
        let runtime_fence = provider.acquire_runtime_generation_fence().await?;
        let _lifecycle = self.lifecycle.lock().await;
        let lease = self
            .require_provider_lease_under_fence(&provider, &runtime_fence)
            .await?;
        self.ensure_context_not_closed(&lease).await?;
        let _ = &runtime_fence;
        Ok(AgentLiveAttachManager {
            host: self.clone(),
            provider: Arc::new(provider),
            projector: Arc::new(projector),
            lease,
        })
    }

    /// Explicitly admit one producer for an exact session/run route.
    ///
    /// This is called when the native runtime starts or deliberately rebinds a
    /// producer. It is never called from `publish`, so journal rollover cannot
    /// silently move an existing producer onto a new ingress generation.
    pub(crate) async fn begin_ingress<P>(
        &self,
        provider: &P,
        session_id: impl Into<String>,
        run_id: Option<String>,
    ) -> Result<AgentLiveIngressPublisher, AgentLiveHostError>
    where
        P: AgentLiveAttachProvider,
    {
        let runtime_fence = provider.acquire_runtime_generation_fence().await?;
        provider
            .verify_current_generation_under_fence(&runtime_fence)
            .await?;
        let _lifecycle = self.lifecycle.lock().await;
        let lease = self
            .require_provider_lease_under_fence(provider, &runtime_fence)
            .await?;
        let coordinator = self.coordinator_for_lease(&lease).await?;
        let ingress = coordinator.begin_ingress(session_id, run_id).await?;
        let _ = &runtime_fence;
        Ok(AgentLiveIngressPublisher {
            binding: lease,
            ingress,
        })
    }

    /// Publish one already-projected, typed event through the exact producer.
    /// This is the only intended replacement for a global event sink.
    pub(crate) async fn publish<P>(
        &self,
        provider: &P,
        publisher: &AgentLiveIngressPublisher,
        event: AgentLivePublishEvent,
    ) -> Result<AgentLiveEventCursor, AgentLiveHostError>
    where
        P: AgentLiveAttachProvider,
    {
        // Runtime generation changes take this fence before entering host
        // lifecycle hooks. Preserve that lock order to avoid inversion.
        let runtime_fence = provider.acquire_runtime_generation_fence().await?;
        provider
            .verify_current_generation_under_fence(&runtime_fence)
            .await?;
        let _lifecycle = self.lifecycle.lock().await;
        let lease = self
            .require_provider_lease_under_fence(provider, &runtime_fence)
            .await?;
        if lease != publisher.binding {
            return Err(AgentLiveHostError::RuntimeOwnerMismatch);
        }
        let coordinator = self.coordinator_for_lease(&lease).await?;
        // Publication and the FIFO seal command share this lifecycle ordering.
        // If this append is admitted before a rotation, its durable success
        // remains success instead of becoming an ambiguous post-commit stale
        // binding error.
        let cursor = coordinator.publish(&publisher.ingress, event).await?;
        // The runtime fence makes this postcondition non-racy: generation
        // cannot advance between admission and returning its cursor. Do not
        // introduce a fallible post-commit check that could turn durable
        // success into an ambiguous error.
        let _ = (&runtime_fence, &lease);
        Ok(api_cursor(&cursor))
    }

    /// Retire a session's absolute live suffix only from an opaque receipt
    /// verified by the provider that performed the exact durable Goose write.
    ///
    /// This deliberately does not accept a loose session/revision/cursor
    /// tuple. Callers without a reviewed `AgentDurableHeadCommitReceipt`
    /// cannot reach the coordinator acknowledgement API.
    pub(crate) async fn acknowledge_persisted_head<P>(
        &self,
        provider: &P,
        receipt: &AgentDurableHeadCommitReceipt,
    ) -> Result<AgentLiveEventCursor, AgentLiveHostError>
    where
        P: AgentLiveAttachProvider,
    {
        let runtime_fence = provider.acquire_runtime_generation_fence().await?;
        provider
            .verify_current_generation_under_fence(&runtime_fence)
            .await?;
        let through_event_cursor = receipt.through_event_cursor();
        let through_event_cursor = LiveEventCursor::try_from_parts(
            through_event_cursor.journal_id.clone(),
            through_event_cursor.sequence,
        )?;

        let _lifecycle = self.lifecycle.lock().await;
        let lease = self
            .require_provider_lease_under_fence(provider, &runtime_fence)
            .await?;
        let coordinator = self.coordinator_for_lease(&lease).await?;
        let cursor = Self::acknowledge_persisted_head_on_coordinator(
            &coordinator,
            &BoundContextKey::from_binding_lease(&lease),
            receipt,
            through_event_cursor,
        )
        .await?;
        // As above, retaining the runtime fence is the atomic guarantee. The
        // coordinator independently CAS-checks the exact current head cursor.
        let _ = (&runtime_fence, &lease);
        Ok(api_cursor(&cursor))
    }

    async fn acknowledge_persisted_head_on_coordinator(
        coordinator: &AgentLiveCoordinator,
        expected_owner: &BoundContextKey,
        receipt: &AgentDurableHeadCommitReceipt,
        through_event_cursor: LiveEventCursor,
    ) -> Result<LiveEventCursor, AgentLiveHostError> {
        let stable_operation = receipt.stable_operation();
        if stable_operation.owner() != expected_owner || stable_operation.run_id().is_some() {
            return Err(AgentLiveHostError::RuntimeOwnerMismatch);
        }
        // Persisted-head acknowledgement is itself an explicit producer
        // lifecycle. The receipt is bound to this journal namespace; a delayed
        // pre-rollover receipt fails at event-ID derivation and remains
        // available to the caller for deterministic recovery.
        let ingress = coordinator
            .begin_ingress(stable_operation.session_id().to_string(), None)
            .await?;
        let event_id = ingress.event_id(stable_operation)?;
        coordinator
            .acknowledge_persisted_head(
                &ingress,
                event_id,
                receipt.history_revision().to_string(),
                through_event_cursor,
            )
            .await
            .map_err(Into::into)
    }

    /// Seal a runtime's coordinator and prevent accidental recreation for the
    /// same binding during this host lifetime.
    pub(crate) async fn seal<P>(
        &self,
        provider: &P,
        reason: AgentLiveSealReason,
    ) -> Result<(), AgentLiveHostError>
    where
        P: AgentLiveAttachProvider,
    {
        let runtime_fence = provider.acquire_runtime_generation_fence().await?;
        let _lifecycle = self.lifecycle.lock().await;
        let lease = self
            .require_provider_lease_under_fence(provider, &runtime_fence)
            .await?;
        let result = self.close_context(&lease, false, reason).await.map(|_| ());
        let _ = &runtime_fence;
        result
    }

    /// Consume the endpoint's one-use authorization swap receipt. Exact peer
    /// channels are closed continuously while the host lifecycle is held. An
    /// account-epoch change also FIFO-seals and durably retires every retained
    /// account context before returning.
    pub(crate) async fn apply_authorization_transition<H>(
        &self,
        receipt: AuthorizationTransitionReceipt,
        hook: Arc<H>,
    ) -> Result<(), AgentLiveHostError>
    where
        H: AgentLivePeerRevocationHook + 'static,
    {
        // Own the guard so the one-use receipt can be handed atomically to a
        // task which survives cancellation of this request future.
        let lifecycle = Arc::clone(&self.lifecycle).lock_owned().await;
        // Acquire every host recovery slot before consuming the receipt. Once
        // the registry mutates, recording exact cleanup state and spawning its
        // owned task introduce no further cancellation point in this poll.
        let mut pending_account = self.pending_account_retirement.lock().await;
        let mut pending_peers = self.pending_peer_revocations.lock().await;
        let applied = self
            .bindings
            .apply_authorization_transition(receipt)
            .await?;
        let committed_owner = applied
            .revoked_peers()
            .first()
            .map(|revoked| revoked.lease.clone());
        for revoked in applied.revoked_peers() {
            if !pending_peers.contains(&revoked.lease) {
                pending_peers.push(revoked.lease.clone());
            }
        }
        if applied.account_epoch_changed() {
            let key = committed_owner
                .as_ref()
                .map(BoundContextKey::from_binding_lease);
            let owner = committed_owner.as_ref().and_then(|lease| {
                target_bound_owner(
                    lease.account_scope(),
                    lease.account_generation(),
                    lease.execution_target().as_str(),
                )
                .ok()
            });
            // Installed without awaiting so cancellation after the consumed
            // receipt cannot lose the exact old data owner. `owner: None` is a
            // permanent fail-closed missing/derivation failure, never a reason
            // to bind. This also covers an epoch transition observed before a
            // live binding was ever materialized.
            *pending_account = Some(PendingAccountRetirement {
                lease: committed_owner,
                key,
                owner,
            });
        }
        drop(pending_peers);
        drop(pending_account);

        let host = self.clone();
        let cleanup = tokio::spawn(async move {
            let _lifecycle = lifecycle;
            host.finish_pending_authorization_cleanup(hook.as_ref())
                .await
        });
        cleanup
            .await
            .map_err(|_| AgentLiveHostError::JournalWorkerUnavailable)?
    }

    /// Resume only cleanup already authorized by a consumed transition receipt.
    /// Exact revoked leases and any account-retirement marker are host-owned;
    /// no account, endpoint, or target scalar is accepted. The owned task keeps
    /// making progress if its caller is cancelled.
    pub(crate) async fn resume_pending_authorization_cleanup<H>(
        &self,
        hook: Arc<H>,
    ) -> Result<(), AgentLiveHostError>
    where
        H: AgentLivePeerRevocationHook + 'static,
    {
        let lifecycle = Arc::clone(&self.lifecycle).lock_owned().await;
        let host = self.clone();
        let cleanup = tokio::spawn(async move {
            let _lifecycle = lifecycle;
            host.finish_pending_authorization_cleanup(hook.as_ref())
                .await
        });
        cleanup
            .await
            .map_err(|_| AgentLiveHostError::JournalWorkerUnavailable)?
    }

    #[cfg(test)]
    pub(crate) async fn revoke_peer<H>(
        &self,
        controller_endpoint: iroh::EndpointId,
        installed: &InstalledAuthorizationContext,
        hook: &H,
    ) -> Result<bool, AgentLiveHostError>
    where
        H: AgentLivePeerRevocationHook,
    {
        let _lifecycle = self.lifecycle.lock().await;
        let revoked = self
            .bindings
            .revoke_peer(controller_endpoint, installed)
            .await?;
        if let Some(revoked) = revoked {
            hook.revoke_exact_peer(&revoked.lease).await?;
            Ok(true)
        } else {
            Ok(false)
        }
    }

    #[cfg(test)]
    pub(crate) async fn revoke_account(
        &self,
        installed: &InstalledAuthorizationContext,
        reason: AgentLiveSealReason,
    ) -> Result<(), AgentLiveHostError> {
        let _lifecycle = self.lifecycle.lock().await;
        self.bindings.revoke_account(installed).await?;
        self.close_all_contexts(reason).await
    }

    /// FIFO-seal the previous owner named by an exact rotation obligation.
    pub(crate) async fn seal_rotation(
        &self,
        rotation: &mut AgentLiveHostRotation,
    ) -> Result<(), AgentLiveHostError> {
        let _lifecycle = self.lifecycle.lock().await;
        self.require_pending_rotation(&rotation.obligation).await?;
        self.bindings.abort_rotation(&rotation.obligation).await?;
        if rotation.sealed.is_some() {
            return Ok(());
        }
        let sealed = self
            .close_context(
                rotation.obligation.previous(),
                false,
                AgentLiveSealReason::OwnerChanged,
            )
            .await?
            .ok_or(AgentLiveHostError::BoundContextSealed)?;
        rotation.sealed = Some(sealed);
        Ok(())
    }

    /// Complete the durable half of an owner transition. Adjacent generation
    /// changes on the same stable target rotate directly from the FIFO-sealed
    /// lease. Target changes have a different stable key, so they retire the
    /// old journal before the new binding may activate its own owner.
    pub(crate) async fn rotate_journal(
        &self,
        rotation: &mut AgentLiveHostRotation,
    ) -> Result<AgentLiveEventCursor, AgentLiveHostError> {
        let _lifecycle = self.lifecycle.lock().await;
        self.require_pending_rotation(&rotation.obligation).await?;
        self.bindings.abort_rotation(&rotation.obligation).await?;
        let sealed = rotation
            .sealed
            .clone()
            .ok_or(AgentLiveHostError::RotationMustBeSealed)?;
        if rotation.journal_rotated {
            return Err(AgentLiveHostError::RotationAlreadyDurable);
        }
        let previous = rotation.obligation.previous();
        let proposed = rotation.obligation.proposed();
        let cursor = if previous.execution_target() == proposed.execution_target() {
            if previous.account_generation().checked_add(1) != Some(proposed.account_generation()) {
                return Err(AgentLiveHostError::NonAdjacentAccountGeneration);
            }
            let current_owner = target_bound_owner(
                proposed.account_scope(),
                proposed.account_generation(),
                proposed.execution_target().as_str(),
            )?;
            let journal = self.journal.clone();
            let previous_lease = sealed.journal_lease.clone();
            let cursor = tokio::task::spawn_blocking(move || {
                match journal.rotate_account_generation(&previous_lease, &current_owner) {
                    Ok(cursor) => Ok(cursor),
                    Err(rotation_error) => match journal.activate_account(&current_owner) {
                        Ok(current_lease) => journal.checkpoint(&current_lease),
                        Err(_) => Err(rotation_error),
                    },
                }
            })
            .await
            .map_err(|_| AgentLiveHostError::JournalWorkerUnavailable)??;
            let previous_key = BoundContextKey::from_binding_lease(previous);
            let mut contexts = self.contexts.lock().await;
            match contexts.remove(&previous_key) {
                Some(BoundContextSlot::Sealed(existing)) if existing == sealed => {
                    contexts.insert(previous_key, BoundContextSlot::Superseded(existing));
                }
                Some(BoundContextSlot::Superseded(existing)) if existing == sealed => {
                    contexts.insert(previous_key, BoundContextSlot::Superseded(existing));
                }
                existing => {
                    if let Some(existing) = existing {
                        contexts.insert(previous_key, existing);
                    }
                    return Err(AgentLiveHostError::BoundContextSealed);
                }
            }
            cursor
        } else {
            let previous_key = BoundContextKey::from_binding_lease(previous);
            // Acquire the slot before minting the one-use retirement token.
            // From `seal_for_retirement` through installing `Retiring` below
            // there is then no cancellation point which could lose the token.
            let mut contexts = self.contexts.lock().await;
            let token = match rotation.retirement.as_ref() {
                Some(token) => token.clone(),
                None => {
                    let token = self
                        .journal
                        .seal_for_retirement(&sealed.journal_lease, &sealed.through_cursor)?;
                    rotation.retirement = Some(token.clone());
                    token
                }
            };
            match contexts.remove(&previous_key) {
                Some(BoundContextSlot::Sealed(existing)) if existing == sealed => {
                    contexts.insert(
                        previous_key.clone(),
                        BoundContextSlot::Retiring {
                            token: token.clone(),
                            sealed: sealed.clone(),
                            revoked: false,
                        },
                    );
                }
                Some(BoundContextSlot::Retiring {
                    token: existing_token,
                    sealed: existing_seal,
                    revoked,
                }) if existing_token == token && existing_seal == sealed => {
                    contexts.insert(
                        previous_key.clone(),
                        BoundContextSlot::Retiring {
                            token: existing_token,
                            sealed: existing_seal,
                            revoked,
                        },
                    );
                }
                Some(BoundContextSlot::Retired {
                    sealed: existing_seal,
                    revoked,
                }) if existing_seal == sealed => {
                    contexts.insert(
                        previous_key,
                        BoundContextSlot::Retired {
                            sealed: existing_seal,
                            revoked,
                        },
                    );
                    rotation.journal_rotated = true;
                    return Ok(api_cursor(&sealed.through_cursor));
                }
                existing => {
                    if let Some(existing) = existing {
                        contexts.insert(previous_key, existing);
                    }
                    return Err(AgentLiveHostError::BoundContextSealed);
                }
            }
            drop(contexts);
            let journal = self.journal.clone();
            let retirement_result =
                tokio::task::spawn_blocking(move || journal.retire_account(&token))
                    .await
                    .map_err(|_| AgentLiveHostError::JournalWorkerUnavailable)?;
            match retirement_result {
                Ok(()) | Err(LiveEventJournalError::JournalRetired) => {}
                Err(error) => return Err(error.into()),
            }
            self.contexts.lock().await.insert(
                previous_key,
                BoundContextSlot::Retired {
                    sealed: sealed.clone(),
                    revoked: false,
                },
            );
            sealed.through_cursor.clone()
        };
        rotation.journal_rotated = true;
        Ok(api_cursor(&cursor))
    }

    /// Commit only a rotation whose old context was sealed and whose journal
    /// replacement was proven durable.
    pub(crate) async fn commit_rotation<P>(
        &self,
        rotation: &mut AgentLiveHostRotation,
        proposed_provider: &P,
    ) -> Result<AgentLiveBindingLease, AgentLiveHostError>
    where
        P: AgentLiveAttachProvider,
    {
        let runtime_fence = proposed_provider.acquire_runtime_generation_fence().await?;
        proposed_provider
            .verify_current_generation_under_fence(&runtime_fence)
            .await?;
        let _lifecycle = self.lifecycle.lock().await;
        // Keep the recovery copy locked across the registry commit and clear;
        // once commit succeeds there is no await before stale recovery state is
        // removed.
        let mut pending_rotation = self.pending_rotation.lock().await;
        if pending_rotation.as_ref() != Some(&rotation.obligation) {
            return Err(AgentLiveHostError::RotationUnavailable);
        }
        if rotation.sealed.is_none() {
            return Err(AgentLiveHostError::RotationMustBeSealed);
        }
        if !rotation.journal_rotated {
            return Err(AgentLiveHostError::RotationMustBeDurable);
        }
        let proposed = rotation.obligation.proposed();
        if proposed.account_scope() != proposed_provider.account_scope()
            || proposed.account_generation() != proposed_provider.account_generation()
            || proposed.controller_endpoint() != proposed_provider.controller_endpoint()
        {
            return Err(AgentLiveHostError::RuntimeOwnerMismatch);
        }
        // Capture this opaque capability immediately before activation. Its
        // constructor and the registry both revalidate the endpoint's current
        // admission record; no scalar field or renderer value can substitute.
        let reverified = proposed_provider.reverify_current_binding().await?;
        let lease = self
            .bindings
            .commit_rotation(rotation.obligation.clone(), reverified)
            .await?;
        *pending_rotation = None;
        let _ = runtime_fence;
        // The new data-lineage epoch is part of the context key. A -> B -> A
        // therefore cannot inherit either A's old coordinator or tombstone.
        Ok(lease)
    }

    /// Validate an obligation while deliberately leaving the registry in its
    /// fail-closed transition state. This never resurrects the previous lease.
    pub(crate) async fn abort_rotation(
        &self,
        rotation: &AgentLiveHostRotation,
    ) -> Result<(), AgentLiveHostError> {
        let _lifecycle = self.lifecycle.lock().await;
        self.require_pending_rotation(&rotation.obligation).await?;
        self.bindings.abort_rotation(&rotation.obligation).await?;
        Ok(())
    }

    /// Recover the exact process-local rotation after an edge future or handle
    /// was dropped. Durable progress is reconstructed only from fail-closed
    /// host slots; no caller-provided owner or target participates.
    pub(crate) async fn resume_rotation(
        &self,
    ) -> Result<AgentLiveHostRotation, AgentLiveHostError> {
        let _lifecycle = self.lifecycle.lock().await;
        let obligation = self
            .pending_rotation
            .lock()
            .await
            .clone()
            .ok_or(AgentLiveHostError::RotationUnavailable)?;
        self.bindings.abort_rotation(&obligation).await?;
        let key = BoundContextKey::from_binding_lease(obligation.previous());
        let (sealed, retirement, journal_rotated) = match self.contexts.lock().await.get(&key) {
            Some(BoundContextSlot::Active(_) | BoundContextSlot::Sealing { .. }) | None => {
                (None, None, false)
            }
            Some(BoundContextSlot::Sealed(sealed)) => (Some(sealed.clone()), None, false),
            Some(BoundContextSlot::Superseded(sealed)) => (Some(sealed.clone()), None, true),
            Some(BoundContextSlot::Retiring { token, sealed, .. }) => {
                (Some(sealed.clone()), Some(token.clone()), false)
            }
            Some(BoundContextSlot::Retired { sealed, .. }) => (Some(sealed.clone()), None, true),
            Some(BoundContextSlot::Revoked(_)) => {
                return Err(AgentLiveHostError::BoundContextRevoked);
            }
        };
        Ok(AgentLiveHostRotation {
            obligation,
            sealed,
            retirement,
            journal_rotated,
        })
    }

    /// Prepare a reseed only from the future Goose adapter's unforgeable
    /// durable-head authority. The returned obligation remains unusable until
    /// `seal_reseed` closes the exact in-process context and subscribers.
    pub(crate) async fn prepare_reseed(
        &self,
        required: LiveEventJournalReseedRequired,
        authority: VerifiedJournalReseedAuthority,
    ) -> Result<AgentLiveHostReseed, AgentLiveHostError> {
        let _lifecycle = self.lifecycle.lock().await;
        let owner_key = authority.binding_key().clone();
        if self.contexts.lock().await.contains_key(&owner_key) {
            return Err(AgentLiveHostError::ReseedContextMustBeClosed);
        }
        let obligation = self.journal.prepare_reseed(required, authority)?;
        Ok(AgentLiveHostReseed {
            owner_key,
            obligation,
            sealed: false,
        })
    }

    /// The corrupt generation has no activatable coordinator. This barrier
    /// nevertheless proves under the host lifecycle that no exact context or
    /// subscriber remains before the journal marks the obligation sealed.
    pub(crate) async fn seal_reseed(
        &self,
        reseed: &mut AgentLiveHostReseed,
    ) -> Result<(), AgentLiveHostError> {
        let _lifecycle = self.lifecycle.lock().await;
        if self.contexts.lock().await.contains_key(&reseed.owner_key) {
            return Err(AgentLiveHostError::ReseedContextMustBeClosed);
        }
        self.journal.mark_reseed_sealed(&mut reseed.obligation)?;
        reseed.sealed = true;
        Ok(())
    }

    /// Durably commit the exact sealed reseed. The obligation is retained by
    /// the caller on an ambiguous storage error and can be retried verbatim.
    pub(crate) async fn commit_reseed(
        &self,
        reseed: &AgentLiveHostReseed,
    ) -> Result<AgentLiveEventCursor, AgentLiveHostError> {
        let _lifecycle = self.lifecycle.lock().await;
        if !reseed.sealed || self.contexts.lock().await.contains_key(&reseed.owner_key) {
            return Err(AgentLiveHostError::ReseedContextMustBeClosed);
        }
        let activation = self.journal.commit_reseed(&reseed.obligation)?;
        let (_lease, cursor) = activation.into_parts();
        // Deliberately do not install the returned lease without a freshly
        // revalidated binding/provider. The next synchronized operation will
        // activate and bind a new coordinator through the normal lifecycle.
        Ok(api_cursor(&cursor))
    }

    /// Runs only while an owned host lifecycle guard is held. Every successful
    /// peer ACK is removed individually; failures remain exact and retryable.
    /// Account retirement still runs after a peer-hook error so one broken edge
    /// cannot prevent the durable confidentiality fence.
    async fn finish_pending_authorization_cleanup<H>(
        &self,
        hook: &H,
    ) -> Result<(), AgentLiveHostError>
    where
        H: AgentLivePeerRevocationHook,
    {
        // An authorization swap may have converted Transition to Fenced. Keep
        // a recovery obligation only if the registry still recognizes it.
        let pending_rotation = self.pending_rotation.lock().await.clone();
        if let Some(pending_rotation) = pending_rotation {
            if self
                .bindings
                .abort_rotation(&pending_rotation)
                .await
                .is_err()
            {
                *self.pending_rotation.lock().await = None;
            }
        }

        let revoked = self.pending_peer_revocations.lock().await.clone();
        let mut first_error = None;
        for lease in revoked {
            match hook.revoke_exact_peer(&lease).await {
                Ok(()) => {
                    self.pending_peer_revocations
                        .lock()
                        .await
                        .retain(|pending| pending != &lease);
                }
                Err(error) => {
                    first_error.get_or_insert(error);
                }
            }
        }

        if let Some(pending) = self.pending_account_retirement.lock().await.clone() {
            let _ = (&pending.lease, &pending.owner);
            let materialized_key = match pending.key.as_ref() {
                Some(key) if self.contexts.lock().await.contains_key(key) => Some(key),
                _ => None,
            };
            if let Some(key) = materialized_key {
                match self
                    .retire_context_key(key, AgentLiveSealReason::AccountSignedOut)
                    .await
                {
                    Ok(()) => {
                        *self.pending_account_retirement.lock().await = None;
                    }
                    Err(error) => {
                        first_error.get_or_insert(error);
                    }
                }
            } else {
                // The binding transition proves the exact old data owner, but
                // no coordinator ever activated a journal lease. The journal
                // intentionally has no inactive-owner deletion/claim API yet.
                // Retain this bounded entry and keep every new bind/sync call
                // unavailable until a verified lifecycle retirement primitive
                // can consume it.
                first_error.get_or_insert(AgentLiveHostError::AuthorizationCleanupPending);
            }
        }

        match first_error {
            Some(error) => Err(error),
            None => Ok(()),
        }
    }

    async fn require_pending_rotation(
        &self,
        obligation: &AgentLiveRotationObligation,
    ) -> Result<(), AgentLiveHostError> {
        if self.pending_rotation.lock().await.as_ref() == Some(obligation) {
            Ok(())
        } else {
            Err(AgentLiveHostError::RotationUnavailable)
        }
    }

    async fn require_provider_lease_under_fence<P>(
        &self,
        provider: &P,
        runtime_fence: &P::RuntimeGenerationFence,
    ) -> Result<AgentLiveBindingLease, AgentLiveHostError>
    where
        P: AgentLiveAttachProvider,
    {
        let account_scope = provider.account_scope().to_string();
        let account_generation = provider.account_generation();
        let controller_endpoint = provider.controller_endpoint();
        provider
            .verify_current_generation_under_fence(runtime_fence)
            .await?;
        let current = self
            .bindings
            .require_bound(&account_scope, account_generation, controller_endpoint)
            .await
            .map_err(AgentLiveHostError::from)?;
        // This revalidates the opaque endpoint admission capability retained
        // by the exact current lease. Ordinary mutation/disclosure paths must
        // never call `bind_or_refresh`: observing a newer owner there would
        // enter Transition and discard the resulting rotation obligation.
        self.bindings
            .revalidate(&account_scope, account_generation, &current)
            .await?;
        provider
            .verify_current_generation_under_fence(runtime_fence)
            .await?;
        if provider.account_scope() != account_scope.as_str()
            || provider.account_generation() != account_generation
            || provider.controller_endpoint() != controller_endpoint
            || current.account_scope() != account_scope.as_str()
            || current.account_generation() != account_generation
            || current.controller_endpoint() != controller_endpoint
        {
            return Err(AgentLiveHostError::RuntimeOwnerMismatch);
        }
        Ok(current)
    }

    async fn revalidate_provider_lease_under_fence<P>(
        &self,
        provider: &P,
        runtime_fence: &P::RuntimeGenerationFence,
        lease: &AgentLiveBindingLease,
    ) -> Result<(), AgentLiveHostError>
    where
        P: AgentLiveAttachProvider,
    {
        let current = self
            .require_provider_lease_under_fence(provider, runtime_fence)
            .await?;
        if current != *lease {
            return Err(AgentLiveHostError::RuntimeOwnerMismatch);
        }
        Ok(())
    }

    async fn ensure_context_not_closed(
        &self,
        lease: &AgentLiveBindingLease,
    ) -> Result<(), AgentLiveHostError> {
        match self
            .contexts
            .lock()
            .await
            .get(&BoundContextKey::from_binding_lease(lease))
        {
            Some(
                BoundContextSlot::Sealing { .. }
                | BoundContextSlot::Sealed(_)
                | BoundContextSlot::Superseded(_)
                | BoundContextSlot::Retiring { .. }
                | BoundContextSlot::Retired { .. },
            ) => Err(AgentLiveHostError::BoundContextSealed),
            Some(BoundContextSlot::Revoked(_)) => Err(AgentLiveHostError::BoundContextRevoked),
            Some(BoundContextSlot::Active(_)) | None => Ok(()),
        }
    }

    async fn coordinator_for_lease(
        &self,
        lease: &AgentLiveBindingLease,
    ) -> Result<AgentLiveCoordinator, AgentLiveHostError> {
        self.bindings
            .revalidate(lease.account_scope(), lease.account_generation(), lease)
            .await?;
        let key = BoundContextKey::from_binding_lease(lease);
        {
            let contexts = self.contexts.lock().await;
            match contexts.get(&key) {
                Some(BoundContextSlot::Active(active)) => {
                    if active.coordinator.execution_target() != lease.execution_target().as_str() {
                        return Err(AgentLiveHostError::RuntimeOwnerMismatch);
                    }
                    return Ok(active.coordinator.clone());
                }
                Some(
                    BoundContextSlot::Sealing { .. }
                    | BoundContextSlot::Sealed(_)
                    | BoundContextSlot::Superseded(_)
                    | BoundContextSlot::Retiring { .. }
                    | BoundContextSlot::Retired { .. },
                ) => {
                    return Err(AgentLiveHostError::BoundContextSealed);
                }
                Some(BoundContextSlot::Revoked(_)) => {
                    return Err(AgentLiveHostError::BoundContextRevoked);
                }
                None => {}
            }
        }

        let owner = target_bound_owner(
            lease.account_scope(),
            lease.account_generation(),
            lease.execution_target().as_str(),
        )?;
        let journal_lease = match self.journal.activate_account(&owner) {
            Ok(lease) => lease,
            Err(LiveEventJournalActivationError::Journal(error)) => return Err(error.into()),
            Err(LiveEventJournalActivationError::ReseedRequired(required)) => {
                return Err(AgentLiveHostError::JournalReseedRequired(required));
            }
        };
        let data_owner = BoundContextKey::from_binding_lease(lease);
        let coordinator = AgentLiveCoordinator::start_activated(
            self.journal.clone(),
            journal_lease,
            data_owner,
            lease.execution_target().as_str().to_string(),
        )
        .await?;
        self.bindings
            .revalidate(lease.account_scope(), lease.account_generation(), lease)
            .await?;
        let mut contexts = self.contexts.lock().await;
        match contexts.get(&key) {
            Some(
                BoundContextSlot::Sealing { .. }
                | BoundContextSlot::Sealed(_)
                | BoundContextSlot::Superseded(_)
                | BoundContextSlot::Retiring { .. }
                | BoundContextSlot::Retired { .. },
            ) => Err(AgentLiveHostError::BoundContextSealed),
            Some(BoundContextSlot::Revoked(_)) => Err(AgentLiveHostError::BoundContextRevoked),
            Some(BoundContextSlot::Active(existing)) => Ok(existing.coordinator.clone()),
            None => {
                contexts.insert(
                    key,
                    BoundContextSlot::Active(ActiveBoundContext {
                        coordinator: coordinator.clone(),
                    }),
                );
                Ok(coordinator)
            }
        }
    }

    async fn revalidate_active_lease(
        &self,
        lease: &AgentLiveBindingLease,
    ) -> Result<(), AgentLiveHostError> {
        self.bindings
            .revalidate(lease.account_scope(), lease.account_generation(), lease)
            .await?;
        match self
            .contexts
            .lock()
            .await
            .get(&BoundContextKey::from_binding_lease(lease))
        {
            Some(BoundContextSlot::Active(active))
                if active.coordinator.execution_target() == lease.execution_target().as_str() =>
            {
                Ok(())
            }
            Some(BoundContextSlot::Active(_)) => Err(AgentLiveHostError::RuntimeOwnerMismatch),
            Some(
                BoundContextSlot::Sealing { .. }
                | BoundContextSlot::Sealed(_)
                | BoundContextSlot::Superseded(_)
                | BoundContextSlot::Retiring { .. }
                | BoundContextSlot::Retired { .. },
            ) => Err(AgentLiveHostError::BoundContextSealed),
            Some(BoundContextSlot::Revoked(_)) => Err(AgentLiveHostError::BoundContextRevoked),
            None => Err(AgentLiveHostError::BoundContextSealed),
        }
    }

    async fn close_context(
        &self,
        lease: &AgentLiveBindingLease,
        revoked: bool,
        reason: AgentLiveSealReason,
    ) -> Result<Option<AgentLiveSeal>, AgentLiveHostError> {
        let key = BoundContextKey::from_binding_lease(lease);
        self.seal_context_key(&key, revoked, reason).await
    }

    async fn seal_context_key(
        &self,
        key: &BoundContextKey,
        revoked: bool,
        reason: AgentLiveSealReason,
    ) -> Result<Option<AgentLiveSeal>, AgentLiveHostError> {
        let active = {
            let mut contexts = self.contexts.lock().await;
            let existing = contexts.remove(key);
            match existing {
                Some(BoundContextSlot::Active(active)) => {
                    contexts.insert(
                        key.clone(),
                        BoundContextSlot::Sealing {
                            active: active.clone(),
                            reason,
                            revoked,
                        },
                    );
                    active
                }
                Some(BoundContextSlot::Sealing {
                    active,
                    reason: existing_reason,
                    revoked: existing_revoked,
                }) => {
                    let effective_revoked = revoked || existing_revoked;
                    contexts.insert(
                        key.clone(),
                        BoundContextSlot::Sealing {
                            active: active.clone(),
                            reason: existing_reason,
                            revoked: effective_revoked,
                        },
                    );
                    if existing_reason != reason {
                        return Err(AgentLiveHostError::BoundContextSealed);
                    }
                    active
                }
                Some(BoundContextSlot::Sealed(sealed)) => {
                    let result = sealed.clone();
                    contexts.insert(
                        key.clone(),
                        if revoked {
                            BoundContextSlot::Revoked(Some(sealed))
                        } else {
                            BoundContextSlot::Sealed(sealed)
                        },
                    );
                    return Ok(Some(result));
                }
                Some(BoundContextSlot::Superseded(sealed)) => {
                    let result = sealed.clone();
                    contexts.insert(key.clone(), BoundContextSlot::Superseded(sealed));
                    return Ok(Some(result));
                }
                Some(BoundContextSlot::Retiring {
                    token,
                    sealed,
                    revoked: existing_revoked,
                }) => {
                    let result = sealed.clone();
                    contexts.insert(
                        key.clone(),
                        BoundContextSlot::Retiring {
                            token,
                            sealed,
                            revoked: revoked || existing_revoked,
                        },
                    );
                    return Ok(Some(result));
                }
                Some(BoundContextSlot::Retired {
                    sealed,
                    revoked: existing_revoked,
                }) => {
                    let result = sealed.clone();
                    contexts.insert(
                        key.clone(),
                        BoundContextSlot::Retired {
                            sealed,
                            revoked: revoked || existing_revoked,
                        },
                    );
                    return Ok(Some(result));
                }
                Some(BoundContextSlot::Revoked(sealed)) => {
                    let result = sealed.clone();
                    contexts.insert(key.clone(), BoundContextSlot::Revoked(sealed));
                    return Ok(result);
                }
                None => return Ok(None),
            }
        };

        // `Sealing` was installed before this await. Cancellation or an error
        // leaves the exact active context retryable and all lookups closed.
        // The returned proof carries the coordinator's current journal lease,
        // including any activation adopted through rollover. The host must not
        // compare it with or substitute the original activation lease.
        let sealed = active.coordinator.seal(reason).await?;
        let mut contexts = self.contexts.lock().await;
        let effective_revoked = match contexts.get(key) {
            Some(BoundContextSlot::Sealing { revoked, .. }) => *revoked,
            _ => return Err(AgentLiveHostError::BoundContextSealed),
        };
        contexts.insert(
            key.clone(),
            if effective_revoked {
                BoundContextSlot::Revoked(Some(sealed.clone()))
            } else {
                BoundContextSlot::Sealed(sealed.clone())
            },
        );
        Ok(Some(sealed))
    }

    async fn close_all_contexts(
        &self,
        reason: AgentLiveSealReason,
    ) -> Result<(), AgentLiveHostError> {
        let keys = self
            .contexts
            .lock()
            .await
            .keys()
            .cloned()
            .collect::<Vec<_>>();
        let mut first_error = None;
        for key in keys {
            if let Err(error) = self.seal_context_key(&key, true, reason).await {
                first_error.get_or_insert(error);
            }
        }
        match first_error {
            Some(error) => Err(error),
            None => Ok(()),
        }
    }

    async fn retire_all_contexts(
        &self,
        reason: AgentLiveSealReason,
    ) -> Result<(), AgentLiveHostError> {
        let keys = self
            .contexts
            .lock()
            .await
            .keys()
            .cloned()
            .collect::<Vec<_>>();
        for key in keys {
            self.retire_context_key(&key, reason).await?;
        }
        Ok(())
    }

    async fn retire_context_key(
        &self,
        key: &BoundContextKey,
        reason: AgentLiveSealReason,
    ) -> Result<(), AgentLiveHostError> {
        // An adjacent same-target rotation already replaced this stable journal
        // file. Its old lease must never enter the retirement protocol.
        if matches!(
            self.contexts.lock().await.get(key),
            Some(BoundContextSlot::Superseded(_)) | Some(BoundContextSlot::Retired { .. })
        ) {
            return Ok(());
        }
        let sealed = self
            .seal_context_key(key, true, reason)
            .await?
            .ok_or(AgentLiveHostError::BoundContextSealed)?;
        let token = {
            let mut contexts = self.contexts.lock().await;
            match contexts.remove(key) {
                Some(BoundContextSlot::Retiring {
                    token,
                    sealed,
                    revoked,
                }) => {
                    let result = token.clone();
                    contexts.insert(
                        key.clone(),
                        BoundContextSlot::Retiring {
                            token,
                            sealed,
                            revoked,
                        },
                    );
                    result
                }
                Some(BoundContextSlot::Retired { sealed, revoked }) => {
                    contexts.insert(key.clone(), BoundContextSlot::Retired { sealed, revoked });
                    return Ok(());
                }
                existing => {
                    if let Some(existing) = existing {
                        contexts.insert(key.clone(), existing);
                    }
                    // No await from minting the token until `Retiring` owns it.
                    let token = self
                        .journal
                        .seal_for_retirement(&sealed.journal_lease, &sealed.through_cursor)?;
                    contexts.insert(
                        key.clone(),
                        BoundContextSlot::Retiring {
                            token: token.clone(),
                            sealed: sealed.clone(),
                            revoked: true,
                        },
                    );
                    token
                }
            }
        };
        let journal = self.journal.clone();
        let retirement_result = tokio::task::spawn_blocking(move || journal.retire_account(&token))
            .await
            .map_err(|_| AgentLiveHostError::JournalWorkerUnavailable)?;
        match retirement_result {
            Ok(()) | Err(LiveEventJournalError::JournalRetired) => {}
            Err(error) => return Err(error.into()),
        }
        self.contexts.lock().await.insert(
            key.clone(),
            BoundContextSlot::Retired {
                sealed,
                revoked: true,
            },
        );
        Ok(())
    }
}

#[allow(
    clippy::large_enum_variant,
    reason = "rotation owns the complete fail-closed binding and journal transition obligation"
)]
pub(crate) enum AgentLiveHostBindOutcome {
    Bound(AgentLiveBindingLease),
    RotationRequired(AgentLiveHostRotation),
}

/// Process-local obligation. Dropping or explicitly aborting it leaves the
/// binding registry in Transition, so synchronized operations remain closed.
#[must_use = "a live binding rotation must be sealed, durably rotated, and committed"]
pub(crate) struct AgentLiveHostRotation {
    obligation: AgentLiveRotationObligation,
    sealed: Option<AgentLiveSeal>,
    retirement: Option<LiveEventJournalRetirementToken>,
    journal_rotated: bool,
}

impl AgentLiveHostRotation {
    pub(crate) fn previous(&self) -> &AgentLiveBindingLease {
        self.obligation.previous()
    }

    pub(crate) fn proposed(&self) -> &AgentLiveBindingLease {
        self.obligation.proposed()
    }

    pub(crate) const fn is_sealed(&self) -> bool {
        self.sealed.is_some()
    }

    pub(crate) const fn is_journal_rotated(&self) -> bool {
        self.journal_rotated
    }
}

#[must_use = "a verified Agent journal reseed must be sealed and durably committed"]
pub(crate) struct AgentLiveHostReseed {
    owner_key: BoundContextKey,
    obligation: LiveEventJournalReseedObligation,
    sealed: bool,
}

pub(crate) struct AgentLiveAttachManager<P, D>
where
    P: AgentLiveAttachProvider,
    D: AgentLiveDeliveryProjector,
{
    host: AgentLiveHost,
    provider: Arc<P>,
    projector: Arc<D>,
    lease: AgentLiveBindingLease,
}

impl<P, D> AgentLiveAttachManager<P, D>
where
    P: AgentLiveAttachProvider,
    D: AgentLiveDeliveryProjector,
{
    /// Ordinary older-page loads retain the exact pager contract and do not
    /// acquire or depend on a synchronized live binding.
    pub(crate) async fn ordinary_history_page(
        &self,
        request: AgentHistoryPageRequest,
    ) -> Result<AgentHistoryPage, AgentLiveHostError> {
        self.host
            .ordinary_history_page(self.provider.as_ref(), request)
            .await
    }

    /// Capture C0, load and safely project the native newest page, retain the
    /// complete account overlay, then replay C0..C1 before going live.
    pub(crate) async fn attach_newest_page(
        &self,
        request: AgentHistoryPageRequest,
        subscription_capacity: Option<usize>,
    ) -> Result<AgentLiveHeadAttachment<P, D>, AgentLiveAttachError<D::Error>> {
        if request.cursor.is_some() {
            return Err(AgentLiveHostError::HeadAttachRequiresNewestPage.into());
        }
        let requested_limit = request.limit;
        let initial_runtime_fence = self
            .provider
            .acquire_runtime_generation_fence()
            .await
            .map_err(AgentLiveHostError::from)?;
        let head = {
            let _lifecycle = self.host.lifecycle.lock().await;
            self.host
                .revalidate_provider_lease_under_fence(
                    self.provider.as_ref(),
                    &initial_runtime_fence,
                    &self.lease,
                )
                .await?;
            let coordinator = self.host.coordinator_for_lease(&self.lease).await?;
            coordinator
                .begin_account_head_attach(subscription_capacity)
                .await
                .map_err(AgentLiveHostError::from)?
        };
        drop(initial_runtime_fence);
        let page = self
            .host
            .ordinary_history_page(self.provider.as_ref(), request)
            .await?;
        let page = self
            .projector
            .project_history_page(page, requested_limit)
            .map_err(AgentLiveAttachError::Projection)?;
        validate_safe_history_page(&page, requested_limit)?;
        let mut live_sessions = Vec::with_capacity(head.live_sessions.len());
        for session in &head.live_sessions {
            live_sessions.push(AgentLiveProjectedSessionHead {
                session_id: session.session_id.clone(),
                live_items: self
                    .projector
                    .project_head_items(&session.live_items)
                    .map_err(AgentLiveAttachError::Projection)?,
            });
        }
        let live_sessions_complete = head.live_sessions_complete;
        validate_safe_session_heads(live_sessions_complete, &live_sessions)?;
        let through_event_cursor = api_cursor(&head.through_cursor);
        let runtime_fence = self
            .provider
            .acquire_runtime_generation_fence()
            .await
            .map_err(AgentLiveHostError::from)?;
        self.provider
            .verify_current_generation_under_fence(&runtime_fence)
            .await
            .map_err(AgentLiveHostError::from)?;
        let resume = {
            let _lifecycle = self.host.lifecycle.lock().await;
            self.host
                .revalidate_provider_lease_under_fence(
                    self.provider.as_ref(),
                    &runtime_fence,
                    &self.lease,
                )
                .await?;
            let resume = head
                .token
                .finalize()
                .await
                .map_err(AgentLiveHostError::from)?;
            self.host.revalidate_active_lease(&self.lease).await?;
            resume
        };
        let _ = &runtime_fence;
        Ok(AgentLiveHeadAttachment {
            page,
            through_event_cursor,
            live_sessions_complete,
            live_sessions,
            resume_through_cursor: api_cursor(&resume.through_cursor),
            subscription: AgentLiveProjectedSubscription {
                host: self.host.clone(),
                provider: Arc::clone(&self.provider),
                lease: self.lease.clone(),
                projector: Arc::clone(&self.projector),
                subscription: resume.subscription,
                terminal: false,
            },
        })
    }

    pub(crate) async fn resume(
        &self,
        cursor: AgentLiveEventCursor,
        subscription_capacity: Option<usize>,
    ) -> Result<AgentLiveResumeAttachment<P, D>, AgentLiveHostError> {
        let cursor = LiveEventCursor::try_from_parts(cursor.journal_id, cursor.sequence)?;
        let runtime_fence = self.provider.acquire_runtime_generation_fence().await?;
        self.provider
            .verify_current_generation_under_fence(&runtime_fence)
            .await?;
        let coordinator = {
            let _lifecycle = self.host.lifecycle.lock().await;
            self.host
                .revalidate_provider_lease_under_fence(
                    self.provider.as_ref(),
                    &runtime_fence,
                    &self.lease,
                )
                .await?;
            self.host.coordinator_for_lease(&self.lease).await?
        };
        let resume = coordinator
            .begin_resume(cursor, subscription_capacity)
            .await?;
        {
            let _lifecycle = self.host.lifecycle.lock().await;
            self.host
                .revalidate_provider_lease_under_fence(
                    self.provider.as_ref(),
                    &runtime_fence,
                    &self.lease,
                )
                .await?;
            self.host.revalidate_active_lease(&self.lease).await?;
        }
        let _ = &runtime_fence;
        Ok(AgentLiveResumeAttachment {
            through_cursor: api_cursor(&resume.through_cursor),
            subscription: AgentLiveProjectedSubscription {
                host: self.host.clone(),
                provider: Arc::clone(&self.provider),
                lease: self.lease.clone(),
                projector: Arc::clone(&self.projector),
                subscription: resume.subscription,
                terminal: false,
            },
        })
    }
}

pub(crate) struct AgentLiveHeadAttachment<P, D>
where
    P: AgentLiveAttachProvider,
    D: AgentLiveDeliveryProjector,
{
    pub(crate) page: AgentLiveSafeHistoryPage,
    /// C0 paired with the complete account-wide live snapshot below.
    pub(crate) through_event_cursor: AgentLiveEventCursor,
    pub(crate) live_sessions_complete: bool,
    /// Complete account-wide C0 overlay. Consumers must clear cached session
    /// overlays absent from this list when `live_sessions_complete` is true.
    pub(crate) live_sessions: Vec<AgentLiveProjectedSessionHead>,
    /// C1 is internal attachment state. The response snapshot acknowledges C0;
    /// deliveries queued during finalization cover C0..C1 exactly once.
    pub(crate) resume_through_cursor: AgentLiveEventCursor,
    pub(crate) subscription: AgentLiveProjectedSubscription<P, D>,
}

pub(crate) struct AgentLiveProjectedSessionHead {
    pub(crate) session_id: String,
    pub(crate) live_items: Vec<MapleLiveTimelineItem>,
}

/// Per-peer factory for the object-safe remote attachment service below.
///
/// The endpoint-global RPC host must bind exclusively from the opaque
/// authority minted by the current incoming admission. It must never select
/// an account, target, controller, pairing lineage, or connection generation
/// from request fields. The returned service is therefore irreversibly scoped
/// to this exact peer authority and must retain/revalidate it at every mutation
/// and disclosure boundary.
///
/// `bind` is an authority-scoping constructor, not a native lifecycle
/// acquisition. The RPC edge may call it again for another request from the
/// same admitted peer, including concurrent requests which later lose the
/// stable-occupancy race. Implementations must therefore acquire no exclusive
/// subscriber, paused token, stream, or other asynchronously-released native
/// capacity here. Those resources may be acquired only by `begin_newest` or
/// `resume`, after the RPC lifecycle slot has been reserved.
#[async_trait::async_trait]
pub(crate) trait AgentLiveRemoteAttachProvider: Send + Sync {
    /// Bind one exact peer-scoped, non-exclusive service. Dropping this future
    /// or the returned service must require no asynchronous native cleanup.
    async fn bind(
        &self,
        authority: VerifiedIncomingPeerAuthorization,
    ) -> Result<Arc<dyn AgentLiveRemoteAttachService>, AgentLiveRemoteAttachError>;
}

/// Object-safe per-peer core seam consumed by the authenticated remote RPC
/// edge.
///
/// The service is constructed around a native [`AgentLiveAttachProvider`]; no
/// account, execution-target, endpoint, authorization, or generation scalar is
/// accepted on these methods. A production implementation must retain the
/// provider's runtime-generation fence and revalidate its exact binding and
/// current peer admission before every mutation or disclosure.
#[async_trait::async_trait]
pub(crate) trait AgentLiveRemoteAttachService: Send + Sync {
    /// Capture C0 and return one safely projected newest persisted page plus
    /// the complete account-wide absolute live snapshot at C0. The subscriber
    /// remains paused: this method must not finalize the coordinator token.
    ///
    /// This acquisition is cancellation-safe: if the returned future is
    /// dropped before yielding `Ok`, an owned task/guard inside the production
    /// implementation must await or otherwise reliably complete unsubscribe.
    /// It must keep the affected native capacity unavailable for reuse until
    /// that cleanup completes. A caller cannot acknowledge a lifecycle handle
    /// which it never received, so deferring this duty back to the RPC edge is
    /// forbidden.
    async fn begin_newest(
        &self,
        request: AgentHistoryPageRequest,
        subscription_capacity: Option<usize>,
    ) -> Result<AgentLiveRemoteHeadBegin, AgentLiveRemoteAttachError>;

    /// Resume directly from an opaque live-event cursor. The returned C1 is
    /// the FIFO replay barrier and the stream owns all later deliveries.
    /// Dropping this acquisition future before `Ok` is subject to the same
    /// owned-cleanup requirement as `begin_newest`.
    async fn resume(
        &self,
        cursor: AgentLiveEventCursor,
        subscription_capacity: Option<usize>,
    ) -> Result<AgentLiveRemoteResume, AgentLiveRemoteAttachError>;
}

/// One unactivated synchronized attach. The page and snapshot are safe to
/// serialize, but no event can be consumed until the edge has installed both
/// and explicitly calls `activate`.
pub(crate) struct AgentLiveRemoteHeadBegin {
    pub(crate) page: AgentLiveSafeHistoryPage,
    pub(crate) through_event_cursor: AgentLiveEventCursor,
    pub(crate) live_sessions_complete: bool,
    pub(crate) live_sessions: Vec<AgentLiveProjectedSessionHead>,
    pub(crate) pending: Box<dyn AgentLiveRemotePendingAttach>,
}

#[async_trait::async_trait]
pub(crate) trait AgentLiveRemotePendingAttach: Send {
    /// Revalidate the exact runtime generation, binding, and installed peer,
    /// then finalize C0..C1.
    ///
    /// Activation is cancellation-safe at this object boundary. Until this
    /// method returns `Ok`, dropping its future leaves the object valid and
    /// `cancel(self: Box<Self>)` must reclaim and acknowledge the exact paused
    /// or partially-finalized native lifecycle. Returning `Err` has the same
    /// rule: the caller must still consume the object through `cancel`. After
    /// `Ok`, the implementation has consumed its internal pending token and
    /// dropping this wrapper is inert.
    async fn activate(&mut self) -> Result<AgentLiveRemoteActivated, AgentLiveRemoteAttachError>;

    /// Cancel the paused coordinator token and await its actor acknowledgement
    /// so aggregate subscriber capacity is reclaimed before returning.
    async fn cancel(self: Box<Self>) -> Result<(), AgentLiveRemoteAttachError>;
}

pub(crate) struct AgentLiveRemoteActivated {
    /// C1 reached by replaying every event after the response's C0 snapshot.
    pub(crate) through_event_cursor: AgentLiveEventCursor,
    pub(crate) stream: Box<dyn AgentLiveRemoteStream>,
}

pub(crate) struct AgentLiveRemoteResume {
    pub(crate) through_event_cursor: AgentLiveEventCursor,
    pub(crate) stream: Box<dyn AgentLiveRemoteStream>,
}

#[async_trait::async_trait]
pub(crate) trait AgentLiveRemoteStream: Send {
    /// Wait for one durable closed delivery. Implementations must revalidate
    /// before waiting and again after consuming the event but before returning
    /// it. Any failure after consumption is terminal, preventing a caller from
    /// skipping a sequence by invoking `recv` again.
    async fn recv(&mut self) -> Result<AgentLiveRemoteDelivery, AgentLiveRemoteStreamError>;

    /// Stop the stream and await the coordinator's unsubscribe acknowledgement.
    async fn unsubscribe(self: Box<Self>) -> Result<(), AgentLiveRemoteAttachError>;
}

/// Closed remote delivery. It contains only the reviewed Maple live event
/// contract and opaque ordering metadata; rich Goose/tool values are absent.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct AgentLiveRemoteDelivery {
    pub(crate) cursor: AgentLiveEventCursor,
    pub(crate) session_id: String,
    pub(crate) run_id: Option<String>,
    pub(crate) event: MapleLiveEvent,
}

impl AgentLiveRemoteDelivery {
    fn from_closed(delivery: AgentLiveDelivery) -> Result<Self, AgentLiveRemoteAttachError> {
        delivery
            .validate()
            .map_err(|_| AgentLiveRemoteAttachError::ProjectionRejected)?;
        Ok(Self {
            cursor: api_cursor(&delivery.cursor),
            session_id: delivery.session_id,
            run_id: delivery.run_id,
            event: delivery.event,
        })
    }
}

#[derive(Debug)]
pub(crate) enum AgentLiveRemoteAttachError {
    Host(AgentLiveHostError),
    ProjectionRejected,
    /// The core contract exists, but its verified runtime/provider adapter is
    /// intentionally unavailable until the pinned Goose pager is integrated.
    Unavailable,
}

impl From<AgentLiveHostError> for AgentLiveRemoteAttachError {
    fn from(error: AgentLiveHostError) -> Self {
        Self::Host(error)
    }
}

impl fmt::Display for AgentLiveRemoteAttachError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Host(error) => error.fmt(formatter),
            Self::ProjectionRejected => {
                formatter.write_str("the closed Agent live delivery projection was rejected")
            }
            Self::Unavailable => {
                formatter.write_str("verified remote Agent live attachment is unavailable")
            }
        }
    }
}

impl std::error::Error for AgentLiveRemoteAttachError {}

#[derive(Debug)]
pub(crate) enum AgentLiveRemoteStreamError {
    Attach(AgentLiveRemoteAttachError),
    Receive(AgentLiveReceiveError),
}

impl fmt::Display for AgentLiveRemoteStreamError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Attach(error) => error.fmt(formatter),
            Self::Receive(AgentLiveReceiveError::HeadReloadRequired(_)) => {
                formatter.write_str("the Agent live stream requires an authoritative head reload")
            }
            Self::Receive(AgentLiveReceiveError::Closed) => {
                formatter.write_str("the Agent live stream is closed")
            }
        }
    }
}

impl std::error::Error for AgentLiveRemoteStreamError {}

/// Fail-closed default used until a native `AgentRuntimeHandle` provider and
/// safe Goose-row projector are composed. It cannot disclose a page, capture
/// a coordinator token, or create a stream.
#[derive(Debug, Default)]
pub(crate) struct UnavailableAgentLiveRemoteAttachService;

#[async_trait::async_trait]
impl AgentLiveRemoteAttachService for UnavailableAgentLiveRemoteAttachService {
    async fn begin_newest(
        &self,
        _request: AgentHistoryPageRequest,
        _subscription_capacity: Option<usize>,
    ) -> Result<AgentLiveRemoteHeadBegin, AgentLiveRemoteAttachError> {
        Err(AgentLiveRemoteAttachError::Unavailable)
    }

    async fn resume(
        &self,
        _cursor: AgentLiveEventCursor,
        _subscription_capacity: Option<usize>,
    ) -> Result<AgentLiveRemoteResume, AgentLiveRemoteAttachError> {
        Err(AgentLiveRemoteAttachError::Unavailable)
    }
}

pub(crate) struct UnavailableAgentLiveRemoteAttachProvider;

#[async_trait::async_trait]
impl AgentLiveRemoteAttachProvider for UnavailableAgentLiveRemoteAttachProvider {
    async fn bind(
        &self,
        _authority: VerifiedIncomingPeerAuthorization,
    ) -> Result<Arc<dyn AgentLiveRemoteAttachService>, AgentLiveRemoteAttachError> {
        Err(AgentLiveRemoteAttachError::Unavailable)
    }
}

pub(crate) struct AgentLiveResumeAttachment<P, D>
where
    P: AgentLiveAttachProvider,
    D: AgentLiveDeliveryProjector,
{
    pub(crate) through_cursor: AgentLiveEventCursor,
    pub(crate) subscription: AgentLiveProjectedSubscription<P, D>,
}

pub(crate) struct AgentLiveProjectedSubscription<P, D>
where
    P: AgentLiveAttachProvider,
    D: AgentLiveDeliveryProjector,
{
    host: AgentLiveHost,
    provider: Arc<P>,
    lease: AgentLiveBindingLease,
    projector: Arc<D>,
    subscription: AgentLiveSubscription,
    terminal: bool,
}

impl<P, D> AgentLiveProjectedSubscription<P, D>
where
    P: AgentLiveAttachProvider,
    D: AgentLiveDeliveryProjector,
{
    pub(crate) async fn recv(&mut self) -> Result<D::Delivery, AgentLiveStreamError<D::Error>> {
        if self.terminal {
            return Err(AgentLiveStreamError::Receive(AgentLiveReceiveError::Closed));
        }
        let pre_receive_fence = match self.provider.acquire_runtime_generation_fence().await {
            Ok(fence) => fence,
            Err(error) => {
                self.terminal = true;
                return Err(AgentLiveStreamError::Host(error.into()));
            }
        };
        {
            let _lifecycle = self.host.lifecycle.lock().await;
            if let Err(error) = self
                .host
                .revalidate_provider_lease_under_fence(
                    self.provider.as_ref(),
                    &pre_receive_fence,
                    &self.lease,
                )
                .await
            {
                self.terminal = true;
                return Err(AgentLiveStreamError::Host(error));
            }
            if let Err(error) = self.host.revalidate_active_lease(&self.lease).await {
                self.terminal = true;
                return Err(AgentLiveStreamError::Host(error));
            }
        }
        drop(pre_receive_fence);
        let delivery = match self.subscription.recv().await {
            Ok(delivery) => delivery,
            Err(error) => {
                self.terminal = true;
                return Err(AgentLiveStreamError::Receive(error));
            }
        };

        // From this point on the durable delivery has been consumed. Every
        // failure is terminal: allowing another `recv` would silently skip
        // the consumed sequence and violate the edge's replay contract. Set
        // the bit before the next await so cancelling this future is terminal
        // too; clear it only on the synchronous success return below.
        self.terminal = true;
        let runtime_fence = match self.provider.acquire_runtime_generation_fence().await {
            Ok(fence) => fence,
            Err(error) => {
                self.terminal = true;
                return Err(AgentLiveStreamError::Host(error.into()));
            }
        };
        if let Err(error) = self
            .provider
            .verify_current_generation_under_fence(&runtime_fence)
            .await
        {
            self.terminal = true;
            return Err(AgentLiveStreamError::Host(error.into()));
        }
        let _lifecycle = self.host.lifecycle.lock().await;
        if let Err(error) = self
            .host
            .revalidate_provider_lease_under_fence(
                self.provider.as_ref(),
                &runtime_fence,
                &self.lease,
            )
            .await
        {
            self.terminal = true;
            return Err(AgentLiveStreamError::Host(error));
        }
        if let Err(error) = self.host.revalidate_active_lease(&self.lease).await {
            self.terminal = true;
            return Err(AgentLiveStreamError::Host(error));
        }
        let projected = match self.projector.project_delivery(&delivery) {
            Ok(projected) => projected,
            Err(error) => {
                self.terminal = true;
                return Err(AgentLiveStreamError::Projection(error));
            }
        };
        if let Err(error) = self
            .host
            .revalidate_provider_lease_under_fence(
                self.provider.as_ref(),
                &runtime_fence,
                &self.lease,
            )
            .await
        {
            self.terminal = true;
            return Err(AgentLiveStreamError::Host(error));
        }
        if let Err(error) = self.host.revalidate_active_lease(&self.lease).await {
            self.terminal = true;
            return Err(AgentLiveStreamError::Host(error));
        }
        let _ = &runtime_fence;
        self.terminal = false;
        Ok(projected)
    }
}

fn api_cursor(cursor: &LiveEventCursor) -> AgentLiveEventCursor {
    AgentLiveEventCursor {
        journal_id: cursor.journal_id().to_string(),
        sequence: cursor.sequence(),
    }
}

fn validate_safe_history_page(
    page: &AgentLiveSafeHistoryPage,
    requested_limit: Option<usize>,
) -> Result<(), AgentLiveHostError> {
    if requested_limit.is_some_and(|limit| !(1..=MAX_SYNCHRONIZED_HISTORY_RECORDS).contains(&limit))
    {
        return Err(AgentLiveHostError::SynchronizedPageProjectionRejected);
    }
    let maximum_records = requested_limit.unwrap_or(MAX_SYNCHRONIZED_HISTORY_RECORDS);
    if page.records.len() > maximum_records {
        return Err(AgentLiveHostError::SynchronizedPageProjectionRejected);
    }
    if !is_safe_history_token(&page.history_revision, MAX_SYNCHRONIZED_HISTORY_TOKEN_BYTES)
        || page.next_cursor.as_deref().is_some_and(|cursor| {
            !is_safe_history_token(cursor, MAX_SYNCHRONIZED_HISTORY_TOKEN_BYTES)
        })
    {
        return Err(AgentLiveHostError::SynchronizedPageProjectionRejected);
    }

    let mut record_ids = HashSet::with_capacity(page.records.len());
    for record in &page.records {
        if !is_safe_history_token(&record.record_id, MAX_SYNCHRONIZED_HISTORY_TOKEN_BYTES)
            || !record_ids.insert(record.record_id.as_str())
            || record.role.is_empty()
            || record.role.len() > MAX_SYNCHRONIZED_ROLE_BYTES
            || !record
                .role
                .bytes()
                .all(|byte| byte.is_ascii_graphic() || byte == b' ')
            || record.created_ms > MAX_JAVASCRIPT_SAFE_INTEGER
            || record.items.len() > MAX_SYNCHRONIZED_ITEMS_PER_RECORD
            || record.items.iter().any(|item| item.validate().is_err())
        {
            return Err(AgentLiveHostError::SynchronizedPageProjectionRejected);
        }
        let mut encoded = SerializedHistoryByteCounter::new(MAX_HISTORY_RECORD_PRESENTATION_BYTES);
        let encoding = ciborium::ser::into_writer(record, &mut encoded);
        if encoded.limit_exceeded {
            return Err(AgentLiveHostError::SynchronizedHistoryRecordTooLarge);
        }
        encoding.map_err(|_| AgentLiveHostError::SynchronizedPageProjectionRejected)?;
    }
    Ok(())
}

struct SerializedHistoryByteCounter {
    bytes: usize,
    limit: usize,
    limit_exceeded: bool,
}

impl SerializedHistoryByteCounter {
    const fn new(limit: usize) -> Self {
        Self {
            bytes: 0,
            limit,
            limit_exceeded: false,
        }
    }
}

impl Write for SerializedHistoryByteCounter {
    fn write(&mut self, buffer: &[u8]) -> std::io::Result<usize> {
        let next = self
            .bytes
            .checked_add(buffer.len())
            .ok_or_else(|| std::io::Error::other("safe history CBOR length overflow"))?;
        if next > self.limit {
            self.limit_exceeded = true;
            return Err(std::io::Error::other(
                "safe history CBOR presentation limit exceeded",
            ));
        }
        self.bytes = next;
        Ok(buffer.len())
    }

    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

fn validate_safe_session_heads(
    live_sessions_complete: bool,
    sessions: &[AgentLiveProjectedSessionHead],
) -> Result<(), AgentLiveHostError> {
    if !live_sessions_complete || sessions.len() > MAX_SYNCHRONIZED_LIVE_SESSIONS {
        return Err(AgentLiveHostError::SynchronizedPageProjectionRejected);
    }
    let mut previous_session_id: Option<&str> = None;
    let mut account_item_count = 0usize;
    for session in sessions {
        if !is_safe_history_token(&session.session_id, MAX_SYNCHRONIZED_SESSION_ID_BYTES)
            || previous_session_id.is_some_and(|previous| previous >= session.session_id.as_str())
            || session.live_items.len() > MAX_SYNCHRONIZED_LIVE_ITEMS_PER_SESSION
        {
            return Err(AgentLiveHostError::SynchronizedPageProjectionRejected);
        }
        previous_session_id = Some(&session.session_id);
        account_item_count = account_item_count
            .checked_add(session.live_items.len())
            .ok_or(AgentLiveHostError::SynchronizedPageProjectionRejected)?;
        if account_item_count > MAX_SYNCHRONIZED_LIVE_ITEMS_PER_ACCOUNT {
            return Err(AgentLiveHostError::SynchronizedPageProjectionRejected);
        }
        let mut item_ids = HashSet::with_capacity(session.live_items.len());
        if session.live_items.iter().any(|item| {
            item.validate().is_err()
                || item.merge != MapleLiveMerge::Replace
                || !item_ids.insert(item.id.as_str())
        }) {
            return Err(AgentLiveHostError::SynchronizedPageProjectionRejected);
        }
    }
    Ok(())
}

fn is_safe_history_token(value: &str, max_bytes: usize) -> bool {
    !value.is_empty()
        && value.len() <= max_bytes
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.' | b':'))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agent_event_journal::LiveEventJournalLease;
    use crate::agent_live_coordinator::live_event_payload_commitment;
    use tempfile::TempDir;

    const TEST_TARGET: &str = "11111111-1111-4111-8111-111111111111";
    const TEST_SESSION: &str = "session-a";
    const TEST_HISTORY_REVISION: &str = "history-revision-1";

    fn open_test_journal() -> (
        TempDir,
        LiveEventJournal<MapleLiveEvent>,
        LiveEventAccountOwner,
    ) {
        let root = tempfile::tempdir().expect("temporary journal root");
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(root.path(), std::fs::Permissions::from_mode(0o700))
                .expect("owner-only temporary journal root");
        }
        let private_parent = root.path().join("private");
        prepare_live_event_journal_parent(&private_parent).expect("prepare private journal parent");
        let journal = LiveEventJournal::open(
            private_parent.join("live"),
            DEFAULT_LIVE_EVENT_JOURNAL_LIMITS,
        )
        .expect("open live journal");
        let owner = target_bound_owner("account-a", 7, TEST_TARGET).expect("valid test owner");
        (root, journal, owner)
    }

    async fn start_ack_coordinator(
        journal: &LiveEventJournal<MapleLiveEvent>,
        owner: &LiveEventAccountOwner,
        data_owner: &AgentLiveDataOwnerKey,
    ) -> (
        AgentLiveCoordinator,
        LiveEventJournalLease,
        LiveEventCursor,
        [u8; 32],
    ) {
        let lease = journal
            .activate_account(owner)
            .expect("activate test journal");
        let probe_lease = lease.clone();
        let through_cursor = journal
            .checkpoint(&lease)
            .expect("capture initial journal head");
        let namespace = journal
            .bind_ingress(&lease)
            .expect("bind journal ingress")
            .event_namespace_commitment();
        let coordinator = AgentLiveCoordinator::start_activated(
            journal.clone(),
            lease,
            data_owner.clone(),
            TEST_TARGET,
        )
        .await
        .expect("start test coordinator");
        (coordinator, probe_lease, through_cursor, namespace)
    }

    fn persisted_head_receipt(
        data_owner: AgentLiveDataOwnerKey,
        namespace: [u8; 32],
        through_cursor: &LiveEventCursor,
    ) -> AgentDurableHeadCommitReceipt {
        let event = MapleLiveEvent::HistoryHeadCommitted {
            // The canonical payload commitment deliberately excludes this
            // derived wire identifier.
            event_id: "not-the-wire-id".to_string(),
            history_revision: TEST_HISTORY_REVISION.to_string(),
            through_event_cursor: through_cursor.clone(),
        };
        let payload_commitment = live_event_payload_commitment(TEST_SESSION, None, &event)
            .expect("commit canonical persisted-head payload");
        let stable_operation = AgentDurableStableOperationId::for_test(
            data_owner,
            TEST_SESSION,
            None,
            "durable-head-operation-1",
            namespace,
            payload_commitment,
        );
        AgentDurableHeadCommitReceipt::for_test(
            stable_operation,
            TEST_HISTORY_REVISION,
            api_cursor(through_cursor),
        )
    }

    #[tokio::test]
    async fn remote_attach_seam_is_object_safe_and_unavailable_without_native_adapter() {
        let _provider: &dyn AgentLiveRemoteAttachProvider =
            &UnavailableAgentLiveRemoteAttachProvider;
        let service: Box<dyn AgentLiveRemoteAttachService> =
            Box::new(UnavailableAgentLiveRemoteAttachService);
        assert!(matches!(
            service
                .begin_newest(
                    AgentHistoryPageRequest {
                        session_id: TEST_SESSION.to_string(),
                        cursor: None,
                        limit: Some(1),
                    },
                    Some(4),
                )
                .await,
            Err(AgentLiveRemoteAttachError::Unavailable)
        ));
        assert!(matches!(
            service
                .resume(
                    AgentLiveEventCursor {
                        journal_id: "opaque-cursor".to_string(),
                        sequence: 0,
                    },
                    Some(4),
                )
                .await,
            Err(AgentLiveRemoteAttachError::Unavailable)
        ));
    }

    #[tokio::test]
    async fn persisted_head_receipt_retries_in_generation_but_not_after_namespace_change() {
        let data_owner = AgentLiveDataOwnerKey::for_test("account-a", 7, TEST_TARGET, 1);
        let (_root, journal, owner) = open_test_journal();
        let (old_coordinator, old_probe, old_head, old_namespace) =
            start_ack_coordinator(&journal, &owner, &data_owner).await;
        let receipt = persisted_head_receipt(data_owner.clone(), old_namespace, &old_head);

        let first = AgentLiveHost::acknowledge_persisted_head_on_coordinator(
            &old_coordinator,
            &data_owner,
            &receipt,
            old_head.clone(),
        )
        .await
        .expect("same-generation acknowledgement succeeds");
        let retry = AgentLiveHost::acknowledge_persisted_head_on_coordinator(
            &old_coordinator,
            &data_owner,
            &receipt,
            old_head.clone(),
        )
        .await
        .expect("same-generation receipt is retryable");
        assert_eq!(retry, first);
        assert_eq!(
            journal
                .checkpoint(&old_probe)
                .expect("read old journal head")
                .sequence(),
            1
        );

        // Rollover the exact owner after FIFO-sealing the old coordinator. The
        // returned seal lease, rather than the initial activation clone, is
        // the current journal authority.
        let sealed = old_coordinator
            .seal(AgentLiveSealReason::OwnerChanged)
            .await
            .expect("FIFO-seal old coordinator");
        let empty_projection = br#"{"formatVersion":1,"liveSessions":[]}"#;
        journal
            .store_checkpoint(
                &sealed.journal_lease,
                &sealed.through_cursor,
                empty_projection,
            )
            .expect("store exact empty absolute projection");
        let rollover = journal
            .prepare_rollover(
                &sealed.journal_lease,
                &sealed.through_cursor,
                empty_projection,
            )
            .expect("prepare exact journal rollover");
        let activation = journal
            .commit_rollover(&rollover, empty_projection)
            .expect("commit journal rollover");
        let (fresh_lease, fresh_head) = activation.into_parts();
        let fresh_probe = fresh_lease.clone();
        let fresh_namespace = journal
            .bind_ingress(&fresh_lease)
            .expect("bind fresh journal ingress")
            .event_namespace_commitment();
        assert_ne!(fresh_namespace, old_namespace);
        let fresh_coordinator = AgentLiveCoordinator::start_activated(
            journal.clone(),
            fresh_lease,
            data_owner.clone(),
            TEST_TARGET,
        )
        .await
        .expect("start fresh-generation coordinator");

        // The borrowed pre-rollover receipt is rejected before append and
        // remains available for deterministic recovery. A post-rollover
        // persistence commit must mint a fresh receipt in the new namespace.
        assert!(matches!(
            AgentLiveHost::acknowledge_persisted_head_on_coordinator(
                &fresh_coordinator,
                &data_owner,
                &receipt,
                fresh_head,
            )
            .await,
            Err(AgentLiveHostError::Coordinator(
                AgentLiveCoordinatorError::IngressRebindRequired
            ))
        ));
        assert_eq!(
            journal
                .checkpoint(&fresh_probe)
                .expect("fresh journal remains unchanged")
                .sequence(),
            0
        );
    }
}
