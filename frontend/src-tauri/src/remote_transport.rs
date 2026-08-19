//! Iroh transport harness for the Maple-owned protocol.
//!
//! Address discovery is intentionally absent. A caller supplies the exact,
//! cached [`iroh::EndpointAddr`] obtained through Maple's authenticated pairing
//! and endpoint-refresh control plane. The normal reconnect path never waits on
//! that control plane.
//!
//! The POC pins Iroh with only `tls-ring`; its portmapper and fast Apple
//! datapath features are deliberately compiled out. Direct-path success,
//! battery/wake behavior, App Store/private-API policy, and whether either
//! feature should be enabled remain explicit device benchmarks before mobile
//! runtime enablement.
#![allow(
    dead_code,
    reason = "bounded foundation is wired in later vertical slices"
)]

use std::{
    collections::{HashMap, HashSet, VecDeque},
    future::Future,
    io::{self, Cursor, Write},
    sync::{
        atomic::{AtomicU64, Ordering},
        Arc, Mutex, RwLock,
    },
    time::Duration,
};

use futures_util::StreamExt;
use serde::{de::DeserializeOwned, Deserialize, Serialize};
use sha2::{Digest, Sha256};
use tokio::sync::{oneshot, Notify, OwnedSemaphorePermit, Semaphore};

use crate::{
    durable_host_epoch::reserve_next_host_epoch,
    remote_protocol::{
        validate_frame_len, ConnectionStamp, ErrorCode, PeerDirection, ProtocolError, RequestBody,
        RequestEnvelope, ResponseBody, ResponseEnvelope, StreamHeader, StreamKind, ALPN,
        MAX_FRAME_BYTES, PROTOCOL_VERSION,
    },
    secure_storage::{platform_store, DeviceIdentity, DeviceSecretSlot, DeviceSecretStore},
};

const MAX_CACHED_ADDRESSES: usize = 16;
const MAX_CACHED_IP_ADDRESSES: usize = 8;
const MAX_CACHED_RELAY_ADDRESSES: usize = 4;
const MAX_RELAY_URL_BYTES: usize = 512;
const MAX_CONFIGURED_RELAYS: usize = 8;
const MAX_AUTHORIZED_PEERS_PER_DIRECTION: usize = 64;
const MAX_BULK_STREAM_TASKS: usize = 6;
const MAX_EVENT_STREAM_TASKS: usize = 2;
const MAX_CONTROL_STREAM_TASKS: usize = 2;
const MAX_APPLICATION_STREAM_TASKS: usize =
    MAX_BULK_STREAM_TASKS + MAX_EVENT_STREAM_TASKS + MAX_CONTROL_STREAM_TASKS;
const MAX_INCOMING_BI_STREAMS: u32 = MAX_APPLICATION_STREAM_TASKS as u32;
const MAX_PENDING_HANDSHAKES: usize = 8;
const MAX_ACCEPTED_CONNECTION_QUEUE: usize = 16;
const MAX_ACTIVE_CONNECTIONS_PER_PEER_SIDE: usize = 4;
const MAX_ACTIVE_CONNECTIONS_GLOBAL: usize = 64;
const STREAM_RECEIVE_WINDOW_BYTES: u32 = 256 * 1024;
const CONNECTION_RECEIVE_WINDOW_BYTES: u32 = 4 * 1024 * 1024;
const CONNECTION_SEND_WINDOW_BYTES: u64 = 4 * 1024 * 1024;
const MAX_POLICY_DEADLINE: Duration = Duration::from_secs(60);
const BULK_STREAMING_OPERATION_DEADLINE: Duration = Duration::from_secs(60);
const EVENT_FRAME_WRITE_DEADLINE: Duration = Duration::from_secs(10);
const MAX_STREAM_HEADER_DEADLINE: Duration = Duration::from_secs(1);
const MAX_PREPARED_STREAM_ERRORS: usize = 2;
const MAX_CBOR_RECURSION: usize = 32;
const MAX_CBOR_CONTAINER_ITEMS: u64 = 256;

#[derive(Clone)]
pub struct RelayPolicy {
    mode: iroh::RelayMode,
    allowed_relays: HashSet<iroh::RelayUrl>,
}

impl std::fmt::Debug for RelayPolicy {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("RelayPolicy")
            .field("relay_count", &self.allowed_relays.len())
            .finish_non_exhaustive()
    }
}

impl RelayPolicy {
    pub fn disabled() -> Self {
        Self {
            mode: iroh::RelayMode::Disabled,
            allowed_relays: HashSet::new(),
        }
    }

    /// Production relay configuration is always an explicit Maple allowlist.
    /// Iroh's mutable Default/Staging maps are deliberately unavailable here.
    pub fn custom(relay_map: iroh::RelayMap) -> Result<Self, ProtocolError> {
        let urls = relay_map.urls::<Vec<_>>();
        let configs = relay_map.relays::<Vec<_>>();
        validate_relay_urls(&urls, false)?;
        let config_urls = configs
            .iter()
            .map(|config| config.url.clone())
            .collect::<HashSet<_>>();
        let key_urls = urls.iter().cloned().collect::<HashSet<_>>();
        if configs.len() != urls.len()
            || config_urls != key_urls
            || configs.iter().any(|config| config.auth_token.is_some())
        {
            return Err(ProtocolError::new(
                ErrorCode::InvalidFrame,
                "relay configuration does not match Maple's allowlist policy",
                false,
            ));
        }
        // RelayMap::Clone aliases its mutable backing map. Copy each config by
        // value so a caller retaining the source map cannot mutate the policy
        // after validation. Auth tokens are intentionally unsupported in this
        // POC, avoiding an unbounded header-bearing secret field.
        let owned_map: iroh::RelayMap = configs
            .iter()
            .map(|config| config.as_ref().clone())
            .collect();
        Ok(Self {
            mode: iroh::RelayMode::Custom(owned_map),
            allowed_relays: urls.into_iter().collect(),
        })
    }

    #[cfg(test)]
    fn ignored_public_smoke() -> Result<Self, ProtocolError> {
        let mode = iroh::RelayMode::Default;
        let urls = mode.relay_map().urls::<Vec<_>>();
        validate_relay_urls(&urls, false)?;
        Ok(Self {
            mode,
            allowed_relays: urls.into_iter().collect(),
        })
    }

    fn mode(&self) -> iroh::RelayMode {
        self.mode.clone()
    }

    fn validate_endpoint_addr(&self, addr: &iroh::EndpointAddr) -> Result<(), ProtocolError> {
        if addr.addrs.is_empty() || addr.addrs.len() > MAX_CACHED_ADDRESSES {
            return Err(ProtocolError::new(
                ErrorCode::TransportUnavailable,
                "cached endpoint address count is outside Maple bounds",
                false,
            ));
        }
        let mut ip_count = 0;
        let mut relay_count = 0;
        for transport in &addr.addrs {
            match transport {
                iroh::TransportAddr::Ip(_) => {
                    ip_count += 1;
                    if ip_count > MAX_CACHED_IP_ADDRESSES {
                        return Err(ProtocolError::new(
                            ErrorCode::TransportUnavailable,
                            "cached endpoint has too many IP addresses",
                            false,
                        ));
                    }
                }
                iroh::TransportAddr::Relay(url) => {
                    relay_count += 1;
                    if relay_count > MAX_CACHED_RELAY_ADDRESSES
                        || url.as_str().len() > MAX_RELAY_URL_BYTES
                        || !self.allowed_relays.contains(url)
                    {
                        return Err(ProtocolError::new(
                            ErrorCode::Unauthorized,
                            "cached endpoint contains a relay outside Maple's allowlist",
                            false,
                        ));
                    }
                }
                iroh::TransportAddr::Custom(_) => {
                    return Err(ProtocolError::new(
                        ErrorCode::TransportUnavailable,
                        "custom endpoint transports are not enabled",
                        false,
                    ));
                }
                _ => {
                    return Err(ProtocolError::new(
                        ErrorCode::TransportUnavailable,
                        "unsupported endpoint transport",
                        false,
                    ));
                }
            }
        }
        Ok(())
    }
}

fn validate_relay_urls(urls: &[iroh::RelayUrl], allow_empty: bool) -> Result<(), ProtocolError> {
    if (!allow_empty && urls.is_empty()) || urls.len() > MAX_CONFIGURED_RELAYS {
        return Err(ProtocolError::new(
            ErrorCode::InvalidFrame,
            "relay allowlist size is outside Maple bounds",
            false,
        ));
    }
    if urls.iter().any(|url| {
        url.as_str().len() > MAX_RELAY_URL_BYTES
            || url.scheme() != "https"
            || url.host_str().is_none()
            || !url.username().is_empty()
            || url.password().is_some()
            || url.query().is_some()
            || url.fragment().is_some()
    }) {
        return Err(ProtocolError::new(
            ErrorCode::InvalidFrame,
            "relay allowlist contains an unsupported URL",
            false,
        ));
    }
    Ok(())
}

#[derive(Clone)]
pub struct CachedEndpointAddr {
    addr: iroh::EndpointAddr,
}

impl std::fmt::Debug for CachedEndpointAddr {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("CachedEndpointAddr")
            .field("endpoint_id", &self.addr.id)
            .field("address_count", &self.addr.addrs.len())
            .finish()
    }
}

impl CachedEndpointAddr {
    pub fn new(
        addr: iroh::EndpointAddr,
        relay_policy: &RelayPolicy,
    ) -> Result<Self, ProtocolError> {
        relay_policy.validate_endpoint_addr(&addr)?;
        Ok(Self { addr })
    }

    pub fn endpoint_id(&self) -> iroh::EndpointId {
        self.addr.id
    }

    pub fn as_iroh(&self) -> &iroh::EndpointAddr {
        &self.addr
    }
}

#[derive(Clone)]
pub struct ConnectedPeer {
    connection: iroh::endpoint::Connection,
    connection_stamp: ConnectionStamp,
    pairing_fence: PairingFence,
    execution_target_id: Arc<str>,
    outbound_direction: PeerDirection,
    frame_deadline: Duration,
    outbound_requests: Arc<LaneSemaphores>,
    incoming_streams: Arc<IncomingStreamDispatcher>,
}

#[derive(Debug)]
struct LaneSemaphores {
    control: Arc<Semaphore>,
    events: Arc<Semaphore>,
    bulk: Arc<Semaphore>,
}

struct IncomingStreamDispatcher {
    queue: Arc<PreparedStreamQueue>,
    shutdown: Mutex<Option<oneshot::Sender<()>>>,
}

#[derive(Debug, Default)]
struct PreparedStreamQueue {
    state: Mutex<PreparedStreamQueueState>,
    ready: Notify,
}

#[derive(Debug, Default)]
struct PreparedStreamQueueState {
    control: VecDeque<AcceptedStream>,
    events: VecDeque<AcceptedStream>,
    bulk: VecDeque<AcceptedStream>,
    errors: VecDeque<ProtocolError>,
    closed: bool,
}

impl PreparedStreamQueueState {
    fn len(&self) -> usize {
        self.control.len() + self.events.len() + self.bulk.len() + self.errors.len()
    }
}

impl PreparedStreamQueue {
    fn publish(&self, result: Result<AcceptedStream, ProtocolError>) {
        let Ok(mut state) = self.state.lock() else {
            return;
        };
        if state.closed {
            return;
        }
        match result {
            Ok(stream) => {
                let kind = stream.header.stream_kind;
                if state.len() >= MAX_APPLICATION_STREAM_TASKS {
                    let evicted = state.errors.pop_back().is_some()
                        || match kind {
                            StreamKind::Control => state
                                .bulk
                                .pop_back()
                                .or_else(|| state.events.pop_back())
                                .is_some(),
                            StreamKind::Events => state.bulk.pop_back().is_some(),
                            StreamKind::Bulk => false,
                        };
                    if !evicted {
                        return;
                    }
                }
                match kind {
                    StreamKind::Control => state.control.push_back(stream),
                    StreamKind::Events => state.events.push_back(stream),
                    StreamKind::Bulk => state.bulk.push_back(stream),
                }
            }
            Err(error) => {
                if state.errors.len() >= MAX_PREPARED_STREAM_ERRORS
                    || state.len() >= MAX_APPLICATION_STREAM_TASKS
                {
                    return;
                }
                state.errors.push_back(error);
            }
        }
        drop(state);
        self.ready.notify_one();
    }

    async fn recv(&self, deadline: tokio::time::Instant) -> Result<AcceptedStream, ProtocolError> {
        loop {
            let notified = self.ready.notified();
            {
                let mut state = self.state.lock().map_err(|_| internal_state_error())?;
                if let Some(stream) = state
                    .control
                    .pop_front()
                    .or_else(|| state.events.pop_front())
                    .or_else(|| state.bulk.pop_front())
                {
                    return Ok(stream);
                }
                if let Some(error) = state.errors.pop_front() {
                    return Err(error);
                }
                if state.closed {
                    return Err(ProtocolError::new(
                        ErrorCode::TransportUnavailable,
                        "Maple stream dispatcher is closed",
                        true,
                    ));
                }
            }
            tokio::time::timeout_at(deadline, notified)
                .await
                .map_err(|_| operation_timeout("Maple stream accept deadline elapsed"))?;
        }
    }

    fn close(&self) {
        if let Ok(mut state) = self.state.lock() {
            state.closed = true;
            state.control.clear();
            state.events.clear();
            state.bulk.clear();
            state.errors.clear();
        }
        self.ready.notify_waiters();
    }
}

impl Drop for IncomingStreamDispatcher {
    fn drop(&mut self) {
        if let Ok(shutdown) = self.shutdown.get_mut() {
            if let Some(shutdown) = shutdown.take() {
                let _ = shutdown.send(());
            }
        }
    }
}

impl LaneSemaphores {
    fn new() -> Self {
        Self {
            control: Arc::new(Semaphore::new(MAX_CONTROL_STREAM_TASKS)),
            events: Arc::new(Semaphore::new(MAX_EVENT_STREAM_TASKS)),
            bulk: Arc::new(Semaphore::new(MAX_BULK_STREAM_TASKS)),
        }
    }

    fn for_kind(&self, kind: StreamKind) -> Arc<Semaphore> {
        match kind {
            StreamKind::Control => self.control.clone(),
            StreamKind::Events => self.events.clone(),
            StreamKind::Bulk => self.bulk.clone(),
        }
    }
}

impl std::fmt::Debug for ConnectedPeer {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("ConnectedPeer")
            .field("remote_id", &self.remote_id())
            .field("connection_stamp", &self.connection_stamp)
            .field("pairing_fence", &self.pairing_fence)
            .field("execution_target_id", &self.execution_target_id)
            .field("outbound_direction", &self.outbound_direction)
            .finish_non_exhaustive()
    }
}

#[derive(Debug, Clone, Copy)]
pub struct ConnectionPolicy {
    connect_deadline: Duration,
    handshake_deadline: Duration,
    frame_deadline: Duration,
}

impl ConnectionPolicy {
    pub fn new(connect_deadline: Duration) -> Result<Self, ProtocolError> {
        if connect_deadline.is_zero() || connect_deadline > MAX_POLICY_DEADLINE {
            return Err(ProtocolError::new(
                ErrorCode::InvalidFrame,
                "connection deadline is outside Maple bounds",
                false,
            ));
        }
        Ok(Self {
            connect_deadline,
            handshake_deadline: connect_deadline,
            frame_deadline: connect_deadline,
        })
    }

    pub fn with_handshake_deadline(
        mut self,
        handshake_deadline: Duration,
    ) -> Result<Self, ProtocolError> {
        if handshake_deadline.is_zero() || handshake_deadline > MAX_POLICY_DEADLINE {
            return Err(ProtocolError::new(
                ErrorCode::InvalidFrame,
                "handshake deadline is outside Maple bounds",
                false,
            ));
        }
        self.handshake_deadline = handshake_deadline;
        Ok(self)
    }

    pub fn with_frame_deadline(mut self, frame_deadline: Duration) -> Result<Self, ProtocolError> {
        if frame_deadline.is_zero() || frame_deadline > MAX_POLICY_DEADLINE {
            return Err(ProtocolError::new(
                ErrorCode::InvalidFrame,
                "frame deadline is outside Maple bounds",
                false,
            ));
        }
        self.frame_deadline = frame_deadline;
        Ok(self)
    }
}

/// Test-only raw host epoch. Production code cannot construct a host clock
/// from an integer; it must reserve one durably through
/// [`HostConnectionClock::reserve_for_runtime`].
#[cfg(test)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct HostEpoch(u64);

#[cfg(test)]
impl HostEpoch {
    pub fn new(value: u64) -> Result<Self, ProtocolError> {
        if value == 0 {
            Err(ProtocolError::new(
                ErrorCode::StaleGeneration,
                "host epoch must be positive",
                false,
            ))
        } else {
            Ok(Self(value))
        }
    }

    fn get(self) -> u64 {
        self.0
    }
}

/// Shared host-side allocator for connection stamps. Rebuilding an Iroh
/// endpoint during one host runtime must reuse the same clock so generations
/// never restart. A process restart constructs a new clock only after durably
/// reserving the next installation epoch through secure storage.
#[derive(Debug, Clone)]
pub struct HostConnectionClock {
    host_epoch: u64,
    next_generation: Arc<AtomicU64>,
}

impl HostConnectionClock {
    fn reserve_for_runtime(
        store: &dyn DeviceSecretStore,
        identity: &DeviceIdentity,
    ) -> Result<Self, ProtocolError> {
        let reservation = reserve_next_host_epoch(store, identity.host_epoch_storage_key())
            .map_err(|error| {
                ProtocolError::new(
                    ErrorCode::SecureStorageUnavailable,
                    format!("durable host epoch reservation failed: {error}"),
                    false,
                )
            })?;
        Ok(Self {
            host_epoch: reservation.get(),
            next_generation: Arc::new(AtomicU64::new(1)),
        })
    }

    #[cfg(test)]
    pub fn new(host_epoch: HostEpoch) -> Self {
        Self {
            host_epoch: host_epoch.get(),
            next_generation: Arc::new(AtomicU64::new(1)),
        }
    }

    #[cfg(test)]
    pub fn with_next_generation(
        host_epoch: HostEpoch,
        next_generation: u64,
    ) -> Result<Self, ProtocolError> {
        if next_generation == 0 {
            return Err(ProtocolError::new(
                ErrorCode::StaleGeneration,
                "next connection generation must be positive",
                false,
            ));
        }
        Ok(Self {
            host_epoch: host_epoch.get(),
            next_generation: Arc::new(AtomicU64::new(next_generation)),
        })
    }

    fn allocate(&self) -> Result<ConnectionStamp, ProtocolError> {
        let generation = self
            .next_generation
            .fetch_update(Ordering::SeqCst, Ordering::SeqCst, |value| {
                value.checked_add(1)
            })
            .map_err(|_| {
                ProtocolError::new(
                    ErrorCode::Internal,
                    "host connection generation exhausted",
                    false,
                )
            })?;
        ConnectionStamp::new(self.host_epoch, generation)
    }
}

/// Opaque native authority to bind one installation identity during one host
/// runtime. Identity loading and epoch reservation share the same secure-store
/// object and slot in [`Self::load_and_reserve`]; callers cannot separate a
/// clock from identity A and bind it with identity B. Renderer values are not
/// part of this construction path.
pub struct DurableHostRuntime {
    identity: DeviceIdentity,
    host_clock: HostConnectionClock,
}

impl std::fmt::Debug for DurableHostRuntime {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("DurableHostRuntime")
            .field("public_id", &self.identity.public_id())
            .finish_non_exhaustive()
    }
}

impl DurableHostRuntime {
    /// Load/create the native installation identity and durably advance its
    /// host epoch before returning any capability that can open a listener.
    /// Unsupported or corrupt secure storage fails closed before endpoint bind.
    pub(crate) fn load_from_platform(slot: &DeviceSecretSlot) -> Result<Self, ProtocolError> {
        let store = platform_store().map_err(host_secure_storage_error)?;
        Self::load_and_reserve(store.as_ref(), slot)
    }

    fn load_and_reserve(
        store: &dyn DeviceSecretStore,
        slot: &DeviceSecretSlot,
    ) -> Result<Self, ProtocolError> {
        let identity =
            DeviceIdentity::load_or_create(store, slot).map_err(host_secure_storage_error)?;
        let host_clock = HostConnectionClock::reserve_for_runtime(store, &identity)?;
        Ok(Self {
            identity,
            host_clock,
        })
    }

    #[cfg(test)]
    fn load_and_reserve_for_test(
        store: &dyn DeviceSecretStore,
        slot: &DeviceSecretSlot,
    ) -> Result<Self, ProtocolError> {
        Self::load_and_reserve(store, slot)
    }

    fn identity(&self) -> &DeviceIdentity {
        &self.identity
    }

    fn host_clock(&self) -> HostConnectionClock {
        self.host_clock.clone()
    }
}

fn host_secure_storage_error(error: crate::secure_storage::SecretStoreError) -> ProtocolError {
    ProtocolError::new(
        ErrorCode::SecureStorageUnavailable,
        format!("durable host runtime storage failed: {error}"),
        false,
    )
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(transparent)]
pub struct PairingIncarnation(u64);

impl PairingIncarnation {
    pub fn new(value: u64) -> Result<Self, ProtocolError> {
        if value == 0 {
            return Err(ProtocolError::new(
                ErrorCode::Unauthorized,
                "pairing incarnation must be positive",
                false,
            ));
        }
        Ok(Self(value))
    }

    pub fn get(self) -> u64 {
        self.0
    }
}

/// Wire-visible authorization lineage for one explicit, directed pairing.
///
/// The incarnation is allocated once and never reused for that directed pair.
/// Endpoint identity is authenticated by Iroh and bound by each endpoint's
/// local authorization map. Installation-local account epochs and snapshot
/// revisions never cross the wire: independent devices cannot coordinate
/// those anti-replay counters.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PairingFence {
    pairing_incarnation: PairingIncarnation,
}

impl PairingFence {
    pub fn new(pairing_incarnation: PairingIncarnation) -> Result<Self, ProtocolError> {
        let fence = Self {
            pairing_incarnation,
        };
        fence.validate()?;
        Ok(fence)
    }

    pub fn pairing_incarnation(self) -> PairingIncarnation {
        self.pairing_incarnation
    }

    fn validate(self) -> Result<(), ProtocolError> {
        if self.pairing_incarnation.get() == 0 {
            return Err(ProtocolError::new(
                ErrorCode::Unauthorized,
                "pairing fence is invalid",
                false,
            ));
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AuthorizationSnapshot {
    /// Durable, monotonically increasing account/authentication context. An
    /// account switch must advance this even when device IDs overlap.
    pub account_epoch: u64,
    /// Durable, monotonically increasing revision within `account_epoch`.
    /// Every pairing/revocation snapshot advances it, preventing replay.
    pub snapshot_revision: u64,
    /// Direction-specific pairing incarnation. Retaining the same entry over
    /// an unrelated snapshot revision preserves connection lineage; removing
    /// and later re-adding an endpoint must allocate a new incarnation.
    pub incoming_controllers: HashMap<iroh::EndpointId, PairingIncarnation>,
    pub outgoing_execution_targets: HashMap<iroh::EndpointId, PairingIncarnation>,
}

impl AuthorizationSnapshot {
    pub fn new(account_epoch: u64, snapshot_revision: u64) -> Result<Self, ProtocolError> {
        if account_epoch == 0 || snapshot_revision == 0 {
            return Err(ProtocolError::new(
                ErrorCode::Unauthorized,
                "authorization snapshot version must be positive",
                false,
            ));
        }
        Ok(Self {
            account_epoch,
            snapshot_revision,
            incoming_controllers: HashMap::new(),
            outgoing_execution_targets: HashMap::new(),
        })
    }
}

fn authorization_snapshot_digest(
    account_epoch: u64,
    snapshot_revision: u64,
    incoming_controllers: &HashMap<iroh::EndpointId, PairingIncarnation>,
    outgoing_execution_targets: &HashMap<iroh::EndpointId, PairingIncarnation>,
) -> [u8; 32] {
    fn hash_direction(
        hasher: &mut Sha256,
        direction: u8,
        peers: &HashMap<iroh::EndpointId, PairingIncarnation>,
    ) {
        let mut entries = peers.iter().collect::<Vec<_>>();
        entries.sort_unstable_by_key(|(endpoint, _)| **endpoint);
        hasher.update([direction]);
        hasher.update((entries.len() as u64).to_be_bytes());
        for (endpoint, incarnation) in entries {
            hasher.update(endpoint.as_bytes());
            hasher.update(incarnation.get().to_be_bytes());
        }
    }

    let mut hasher = Sha256::new();
    hasher.update(b"maple-authorization-snapshot-v1\0");
    hasher.update(account_epoch.to_be_bytes());
    hasher.update(snapshot_revision.to_be_bytes());
    hash_direction(&mut hasher, 0, incoming_controllers);
    hash_direction(&mut hasher, 1, outgoing_execution_targets);
    hasher.finalize().into()
}

fn digest_authorization_snapshot(snapshot: &AuthorizationSnapshot) -> [u8; 32] {
    authorization_snapshot_digest(
        snapshot.account_epoch,
        snapshot.snapshot_revision,
        &snapshot.incoming_controllers,
        &snapshot.outgoing_execution_targets,
    )
}

/// Opaque proof of the authorization snapshot currently installed in one
/// endpoint admission table.
///
/// Callers cannot construct this from wire or pairing-status fields. The only
/// production constructor is the endpoint's current-peer verifier below,
/// which captures the version and digest under the admission lock while also
/// proving the exact directed pairing grant remains installed.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub(crate) struct InstalledAuthorizationContext {
    account_epoch: u64,
    snapshot_revision: u64,
    snapshot_digest: [u8; 32],
}

impl InstalledAuthorizationContext {
    pub(crate) const fn account_epoch(&self) -> u64 {
        self.account_epoch
    }

    pub(crate) const fn snapshot_revision(&self) -> u64 {
        self.snapshot_revision
    }

    pub(crate) const fn snapshot_digest(&self) -> [u8; 32] {
        self.snapshot_digest
    }

    #[cfg(test)]
    pub(crate) const fn for_test(
        account_epoch: u64,
        snapshot_revision: u64,
        snapshot_digest: [u8; 32],
    ) -> Self {
        Self {
            account_epoch,
            snapshot_revision,
            snapshot_digest,
        }
    }
}

/// Native-only proof that one complete authorization snapshot atomically
/// replaced another in this endpoint admission table.
///
/// This is deliberately non-Clone and has no public constructor. It is minted
/// only after admission has installed the new snapshot and collected every
/// removed peer under the same write lock, allowing downstream live-state
/// revocation even when the last admitted controller no longer has a current
/// peer capability.
#[derive(Debug)]
pub struct AuthorizationTransitionReceipt {
    authorization_domain: InstalledAuthorizationDomain,
    previous: Option<InstalledAuthorizationContext>,
    current: InstalledAuthorizationContext,
    removed_incoming_controllers: Vec<iroh::EndpointId>,
    account_epoch_changed: bool,
}

impl AuthorizationTransitionReceipt {
    pub(crate) fn authorization_domain(&self) -> &InstalledAuthorizationDomain {
        &self.authorization_domain
    }

    pub(crate) fn previous(&self) -> Option<&InstalledAuthorizationContext> {
        self.previous.as_ref()
    }

    pub(crate) fn current(&self) -> &InstalledAuthorizationContext {
        &self.current
    }

    pub(crate) fn removed_incoming_controllers(&self) -> &[iroh::EndpointId] {
        &self.removed_incoming_controllers
    }

    pub(crate) const fn account_epoch_changed(&self) -> bool {
        self.account_epoch_changed
    }

    pub(crate) fn into_parts(
        self,
    ) -> (
        InstalledAuthorizationDomain,
        Option<InstalledAuthorizationContext>,
        InstalledAuthorizationContext,
        Vec<iroh::EndpointId>,
        bool,
    ) {
        (
            self.authorization_domain,
            self.previous,
            self.current,
            self.removed_incoming_controllers,
            self.account_epoch_changed,
        )
    }

    #[cfg(test)]
    pub(crate) fn for_test(
        previous: Option<InstalledAuthorizationContext>,
        current: InstalledAuthorizationContext,
        removed_incoming_controllers: Vec<iroh::EndpointId>,
        account_epoch_changed: bool,
    ) -> Self {
        Self::for_test_in_domain(
            InstalledAuthorizationDomain(PeerAdmission::default()),
            previous,
            current,
            removed_incoming_controllers,
            account_epoch_changed,
        )
    }

    #[cfg(test)]
    pub(crate) fn for_test_in_domain(
        authorization_domain: InstalledAuthorizationDomain,
        previous: Option<InstalledAuthorizationContext>,
        current: InstalledAuthorizationContext,
        removed_incoming_controllers: Vec<iroh::EndpointId>,
        account_epoch_changed: bool,
    ) -> Self {
        Self {
            authorization_domain,
            previous,
            current,
            removed_incoming_controllers,
            account_epoch_changed,
        }
    }
}

/// Opaque process-local identity of one endpoint admission/revocation domain.
/// Equal scalar grants from another endpoint runtime are never interchangeable.
#[derive(Debug, Clone)]
pub(crate) struct InstalledAuthorizationDomain(PeerAdmission);

impl InstalledAuthorizationDomain {
    pub(crate) fn same_instance(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.0.state, &other.0.state)
    }

    #[cfg(test)]
    pub(crate) fn for_test() -> Self {
        Self(PeerAdmission::default())
    }
}

/// Exact, currently installed authority for one authenticated controller.
///
/// The product execution target is the host registration ID configured on the
/// endpoint. Endpoint identity is intentionally retained only for later
/// revalidation and is never substituted for that product ID.
#[derive(Debug, Clone)]
pub(crate) struct VerifiedIncomingPeerAuthorization {
    admission: PeerAdmission,
    authorization: InstalledAuthorizationContext,
    controller_endpoint: iroh::EndpointId,
    execution_target_id: Arc<str>,
    pairing_fence: PairingFence,
    connection_stamp: ConnectionStamp,
}

impl VerifiedIncomingPeerAuthorization {
    pub(crate) fn revalidate_current(&self) -> Result<(), ProtocolError> {
        self.with_current(|| ())
    }

    /// Execute one non-blocking authority transition while retaining the
    /// admission read guard which proves this exact controller, pairing, and
    /// connection stamp are still current. Callers must not await inside the
    /// closure.
    pub(crate) fn with_current<R>(
        &self,
        operation: impl FnOnce() -> R,
    ) -> Result<R, ProtocolError> {
        let state = self
            .admission
            .state
            .read()
            .map_err(|_| internal_state_error())?;
        let current = PeerAdmission::current_incoming_authorization_in_state(
            &state,
            &self.controller_endpoint,
            self.connection_stamp,
            self.pairing_fence,
        )?;
        if current != self.authorization {
            return Err(ProtocolError::new(
                ErrorCode::Revoked,
                "remote controller authorization changed after verification",
                false,
            ));
        }
        Ok(operation())
    }

    pub(crate) fn authorization(&self) -> &InstalledAuthorizationContext {
        &self.authorization
    }

    pub(crate) fn controller_endpoint(&self) -> iroh::EndpointId {
        self.controller_endpoint
    }

    pub(crate) fn execution_target_id(&self) -> &str {
        &self.execution_target_id
    }

    pub(crate) const fn pairing_fence(&self) -> PairingFence {
        self.pairing_fence
    }

    pub(crate) const fn connection_stamp(&self) -> ConnectionStamp {
        self.connection_stamp
    }

    /// Native identity of the endpoint admission table which minted this
    /// capability. Scalar target/pair/stamp fields are not sufficient: two
    /// endpoint runtimes can legitimately contain identical values while
    /// representing different revocation domains.
    pub(crate) fn same_admission_instance(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.admission.state, &other.admission.state)
    }

    pub(crate) fn authorization_domain(&self) -> InstalledAuthorizationDomain {
        InstalledAuthorizationDomain(self.admission.clone())
    }

    #[cfg(test)]
    pub(crate) fn for_admission_identity_test(
        authorization: InstalledAuthorizationContext,
        controller_endpoint: iroh::EndpointId,
        execution_target_id: Arc<str>,
        pairing_fence: PairingFence,
        connection_stamp: ConnectionStamp,
    ) -> Self {
        Self {
            admission: PeerAdmission::default(),
            authorization,
            controller_endpoint,
            execution_target_id,
            pairing_fence,
            connection_stamp,
        }
    }
}

/// In-memory host lineage retained across an Iroh endpoint rebuild.
///
/// This snapshot deliberately has no serialization implementation. The
/// surrounding runtime owns persistence policy in a later slice; this seam
/// only makes the security binding and quiescent handoff explicit.
#[derive(Debug, PartialEq, Eq)]
pub struct EndpointLineageSnapshot {
    local_endpoint: iroh::EndpointId,
    execution_target_id: Arc<str>,
    account_epoch: u64,
    snapshot_revision: u64,
    authorization_digest: [u8; 32],
    incoming_controllers: HashMap<iroh::EndpointId, IncomingControllerLineage>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct IncomingControllerLineage {
    pairing_incarnation: PairingIncarnation,
    last_committed: Option<ConnectionStamp>,
    finalized_transition: Option<PendingCommit>,
}

impl EndpointLineageSnapshot {
    pub fn local_endpoint(&self) -> iroh::EndpointId {
        self.local_endpoint
    }

    pub fn execution_target_id(&self) -> &str {
        &self.execution_target_id
    }

    pub fn account_epoch(&self) -> u64 {
        self.account_epoch
    }

    pub fn authorization_revision_floor(&self) -> u64 {
        self.snapshot_revision
    }

    pub fn incoming_controller_count(&self) -> usize {
        self.incoming_controllers.len()
    }
}

#[derive(Debug, Clone, Default)]
struct PeerAdmission {
    state: Arc<RwLock<AdmissionState>>,
}

#[derive(Debug, Default)]
struct AdmissionState {
    account_epoch: Option<u64>,
    snapshot_revision: u64,
    /// Sign-out tombstone. Once set, the current account epoch can never be
    /// re-enabled; a freshly authenticated account context must advance it.
    authorization_disabled: bool,
    /// Process-monotonic race token. It is never keyed by peer, so repeated
    /// pair/revoke churn cannot accumulate tombstones.
    admission_revision: u64,
    incoming_controllers: DirectionalAdmission,
    outgoing_execution_targets: DirectionalAdmission,
}

#[derive(Debug, Default)]
struct DirectionalAdmission {
    allowed: HashMap<iroh::EndpointId, PairingIncarnation>,
    active: HashMap<iroh::EndpointId, VecDeque<iroh::endpoint::WeakConnectionHandle>>,
    /// Last generation which completed Maple's application-level handover.
    ///
    /// This is protocol lineage, not a transport-liveness cache.  A routine
    /// path loss may remove `current` before the other endpoint observes the
    /// close, but both sides must continue to name the same predecessor while
    /// racing its replacement.
    committed_lineage: HashMap<iroh::EndpointId, ConnectionStamp>,
    /// Exact most recently finalized A -> B transition. This bounded record
    /// lets a controller recover when the Finalized frame was lost, without
    /// replaying any application command or guessing from handle liveness.
    finalized_transitions: HashMap<iroh::EndpointId, PendingCommit>,
    current: HashMap<iroh::EndpointId, (ConnectionStamp, iroh::endpoint::WeakConnectionHandle)>,
    /// At most one incoming generation per controller may be in the
    /// commit/readiness window. The stable connection ID prevents a later
    /// provisional handshake from nesting over (and losing) the fallback.
    activating: HashMap<iroh::EndpointId, IncomingActivation>,
    /// During the final Ready write, both the candidate and the prior current
    /// generation remain router-valid. EOF confirms activation to the client;
    /// failure restores this fallback without breaking the live generation.
    activating_previous:
        HashMap<iroh::EndpointId, (ConnectionStamp, iroh::endpoint::WeakConnectionHandle)>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct IncomingActivation {
    stamp: ConnectionStamp,
    candidate_id: usize,
    pending: PendingCommit,
}

struct IncomingCommit {
    peer: iroh::EndpointId,
    stamp: ConnectionStamp,
    candidate_id: usize,
    candidate: iroh::endpoint::WeakConnectionHandle,
    previous_stamp: Option<ConnectionStamp>,
    /// Strong host-side overlap handle. Admission ordinarily stores weak
    /// handles so close-on-drop works, but a handover must guarantee A remains
    /// available until CommitObserved is validated.
    previous_connection: Option<iroh::endpoint::Connection>,
    pending: PendingCommit,
}

impl AdmissionState {
    fn directional(&self, side: iroh::endpoint::Side) -> &DirectionalAdmission {
        match side {
            iroh::endpoint::Side::Client => &self.outgoing_execution_targets,
            iroh::endpoint::Side::Server => &self.incoming_controllers,
        }
    }

    fn directional_mut(&mut self, side: iroh::endpoint::Side) -> &mut DirectionalAdmission {
        match side {
            iroh::endpoint::Side::Client => &mut self.outgoing_execution_targets,
            iroh::endpoint::Side::Server => &mut self.incoming_controllers,
        }
    }

    fn prune_and_count_active(&mut self) -> usize {
        let mut count = 0;
        for directional in [
            &mut self.incoming_controllers,
            &mut self.outgoing_execution_targets,
        ] {
            directional.active.retain(|_, handles| {
                handles.retain(weak_connection_is_open);
                count += handles.len();
                !handles.is_empty()
            });
            directional
                .current
                .retain(|_, (_, handle)| weak_connection_is_open(handle));
            directional
                .activating_previous
                .retain(|_, (_, handle)| weak_connection_is_open(handle));
        }
        count
    }
}

impl PeerAdmission {
    fn allow(
        &self,
        side: iroh::endpoint::Side,
        peer: iroh::EndpointId,
    ) -> Result<(), ProtocolError> {
        let mut state = self.state.write().map_err(|_| internal_state_error())?;
        if state.account_epoch.is_some() {
            return Err(ProtocolError::new(
                ErrorCode::Unauthorized,
                "versioned authorization snapshots own the active account context",
                false,
            ));
        }
        if state.authorization_disabled {
            return Err(ProtocolError::new(
                ErrorCode::Revoked,
                "authorization is disabled until a newer account epoch",
                false,
            ));
        }
        if state.directional(side).allowed.contains_key(&peer) {
            return Ok(());
        }
        if state.directional(side).allowed.len() >= MAX_AUTHORIZED_PEERS_PER_DIRECTION {
            return Err(ProtocolError::new(
                ErrorCode::Unauthorized,
                "authorized peer limit reached",
                false,
            ));
        }
        bump_admission_revision(&mut state)?;
        state
            .directional_mut(side)
            .allowed
            .insert(peer, PairingIncarnation(1));
        Ok(())
    }

    fn revoke(
        &self,
        side: iroh::endpoint::Side,
        peer: &iroh::EndpointId,
    ) -> Result<bool, ProtocolError> {
        let (was_allowed, active) = {
            let mut state = self.state.write().map_err(|_| internal_state_error())?;
            if state.account_epoch.is_some() {
                return Err(ProtocolError::new(
                    ErrorCode::Unauthorized,
                    "versioned authorization snapshots own the active account context",
                    false,
                ));
            }
            let was_allowed = state.directional(side).allowed.contains_key(peer);
            if was_allowed {
                bump_admission_revision(&mut state)?;
            }
            let directional = state.directional_mut(side);
            directional.allowed.remove(peer);
            directional.committed_lineage.remove(peer);
            directional.finalized_transitions.remove(peer);
            directional.current.remove(peer);
            directional.activating.remove(peer);
            directional.activating_previous.remove(peer);
            let active = directional.active.remove(peer).unwrap_or_default();
            (was_allowed, active)
        };
        for weak in active {
            if let Some(connection) = weak.upgrade() {
                connection.close(
                    iroh::endpoint::VarInt::from_u32(0x4d_52),
                    b"peer authorization revoked",
                );
            }
        }
        Ok(was_allowed)
    }

    fn is_allowed(&self, side: iroh::endpoint::Side, peer: &iroh::EndpointId) -> bool {
        self.state
            .read()
            .map(|state| state.directional(side).allowed.contains_key(peer))
            .unwrap_or(false)
    }

    fn pairing_fence(
        &self,
        side: iroh::endpoint::Side,
        peer: &iroh::EndpointId,
    ) -> Result<PairingFence, ProtocolError> {
        let state = self.state.read().map_err(|_| internal_state_error())?;
        let pairing_incarnation = state
            .directional(side)
            .allowed
            .get(peer)
            .copied()
            .ok_or_else(|| {
                ProtocolError::new(
                    ErrorCode::Unauthorized,
                    "paired endpoint is not admitted",
                    false,
                )
            })?;
        let fence = PairingFence::new(pairing_incarnation)?;
        fence.validate()?;
        Ok(fence)
    }

    /// Registration and revocation are linearized under the same state lock.
    /// If revoke wins, this closes instead of registering the racing handshake.
    fn register(
        &self,
        connection: &iroh::endpoint::Connection,
    ) -> Result<PairingFence, ProtocolError> {
        let peer = connection.remote_id();
        let side = connection.side();
        let pairing_fence = {
            let mut state = self.state.write().map_err(|_| internal_state_error())?;
            let pairing_incarnation = state
                .directional(side)
                .allowed
                .get(&peer)
                .copied()
                .ok_or_else(|| {
                    connection.close(
                        iroh::endpoint::VarInt::from_u32(0x4d_52),
                        b"peer authorization unavailable",
                    );
                    ProtocolError::new(
                        ErrorCode::Revoked,
                        "peer authorization is unavailable",
                        false,
                    )
                })?;
            if state.authorization_disabled {
                connection.close(
                    iroh::endpoint::VarInt::from_u32(0x4d_52),
                    b"peer authorization unavailable",
                );
                return Err(ProtocolError::new(
                    ErrorCode::Revoked,
                    "peer authorization is unavailable",
                    false,
                ));
            }
            if state.prune_and_count_active() >= MAX_ACTIVE_CONNECTIONS_GLOBAL {
                connection.close(
                    iroh::endpoint::VarInt::from_u32(0x4d_43),
                    b"connection admission capacity reached",
                );
                return Err(ProtocolError::new(
                    ErrorCode::TransportUnavailable,
                    "Maple connection admission is at capacity",
                    true,
                ));
            }
            let fence = PairingFence::new(pairing_incarnation)?;
            let directional = state.directional_mut(side);
            let active = directional.active.entry(peer).or_default();
            if active.len() >= MAX_ACTIVE_CONNECTIONS_PER_PEER_SIDE {
                connection.close(
                    iroh::endpoint::VarInt::from_u32(0x4d_43),
                    b"reconnect race limit reached",
                );
                return Err(ProtocolError::new(
                    ErrorCode::TransportUnavailable,
                    "Maple reconnect race is at capacity",
                    true,
                ));
            }
            active.push_back(connection.weak_handle());
            fence
        };
        Ok(pairing_fence)
    }

    /// Atomically stage a bootstrap generation and reserve its keyed router
    /// slot. The host's current generation and the queue's prior entry remain
    /// unchanged/hidden until the controller proves installation with the
    /// final CommitObserved frame.
    fn commit_and_publish_incoming(
        &self,
        connection: &iroh::endpoint::Connection,
        stamp: ConnectionStamp,
        expected_previous: Option<ConnectionStamp>,
        expected_pairing_fence: PairingFence,
        reservation: AcceptedPeerReservation,
        peer: PendingConnectedPeer,
        pending: PendingCommit,
    ) -> Result<IncomingCommit, ProtocolError> {
        debug_assert_eq!(connection.side(), iroh::endpoint::Side::Server);
        let peer_id = connection.remote_id();
        let previous_connection = {
            let mut state = self.state.write().map_err(|_| internal_state_error())?;
            let current_fence = state
                .incoming_controllers
                .allowed
                .get(&peer_id)
                .copied()
                .and_then(|incarnation| PairingFence::new(incarnation).ok());
            if state.authorization_disabled || current_fence != Some(expected_pairing_fence) {
                return Err(ProtocolError::new(
                    ErrorCode::Unauthorized,
                    "bootstrap authorization changed before activation",
                    false,
                ));
            }
            state.prune_and_count_active();
            let incoming = state.directional_mut(iroh::endpoint::Side::Server);
            if !incoming.allowed.contains_key(&peer_id) {
                return Err(ProtocolError::new(
                    ErrorCode::Unauthorized,
                    "controller authorization was revoked before activation",
                    false,
                ));
            }
            if incoming.activating.contains_key(&peer_id) {
                return Err(ProtocolError::new(
                    ErrorCode::TransportUnavailable,
                    "controller already has a generation awaiting readiness",
                    true,
                ));
            }
            let committed_previous = incoming.committed_lineage.get(&peer_id).copied();
            if committed_previous != expected_previous {
                return Err(ProtocolError::new(
                    ErrorCode::StaleGeneration,
                    "bootstrap previous generation does not match the host",
                    true,
                ));
            }
            if committed_previous.is_some_and(|current| current >= stamp) {
                return Err(ProtocolError::new(
                    ErrorCode::StaleGeneration,
                    "bootstrap lost the host generation race",
                    true,
                ));
            }
            reservation.publish(peer)?;
            incoming.activating.insert(
                peer_id,
                IncomingActivation {
                    stamp,
                    candidate_id: connection.stable_id(),
                    pending: pending.clone(),
                },
            );
            if let Some(previous) = incoming.current.get(&peer_id).cloned() {
                incoming.activating_previous.insert(peer_id, previous);
            }
            let previous_connection = incoming
                .current
                .get(&peer_id)
                .and_then(|(_, handle)| handle.upgrade());
            previous_connection
        };
        Ok(IncomingCommit {
            peer: peer_id,
            stamp,
            candidate_id: connection.stable_id(),
            candidate: connection.weak_handle(),
            previous_stamp: expected_previous,
            previous_connection,
            pending,
        })
    }

    /// Validate the staged activation before the client-visible FIN. No local
    /// fallible activation step remains after that FIN is sent.
    fn validate_incoming_activation(&self, commit: &IncomingCommit) -> Result<(), ProtocolError> {
        let state = self.state.read().map_err(|_| internal_state_error())?;
        let current_fence = state
            .incoming_controllers
            .allowed
            .get(&commit.peer)
            .copied()
            .and_then(|incarnation| PairingFence::new(incarnation).ok());
        if state.authorization_disabled || current_fence != Some(commit.pending.pairing_fence) {
            return Err(ProtocolError::new(
                ErrorCode::Unauthorized,
                "incoming pairing changed before final activation",
                false,
            ));
        }
        let incoming = state.directional(iroh::endpoint::Side::Server);
        if !activation_matches_commit(incoming, commit)
            || !candidate_matches_commit(commit)
            || committed_stamp(incoming, &commit.peer) != commit.previous_stamp
        {
            return Err(ProtocolError::new(
                ErrorCode::StaleGeneration,
                "incoming generation changed before final activation",
                true,
            ));
        }
        Ok(())
    }

    /// Atomically publish the controller-observed candidate and replace the
    /// host's prior current generation. The queue is ungated while the
    /// admission lock is held, so a router cannot observe B before admission
    /// names it current. No fallible state transition follows this commit.
    fn finalize_observed_incoming(
        &self,
        accepted_connections: &AcceptedPeerQueue,
        commit: &IncomingCommit,
    ) -> Result<
        (
            Option<iroh::endpoint::WeakConnectionHandle>,
            Option<ConnectedPeer>,
        ),
        ProtocolError,
    > {
        let mut state = self
            .state
            .write()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let current_fence = state
            .incoming_controllers
            .allowed
            .get(&commit.peer)
            .copied()
            .and_then(|incarnation| PairingFence::new(incarnation).ok());
        if state.authorization_disabled || current_fence != Some(commit.pending.pairing_fence) {
            return Err(ProtocolError::new(
                ErrorCode::Unauthorized,
                "incoming pairing changed before observed finalization",
                false,
            ));
        }
        let incoming = state.directional_mut(iroh::endpoint::Side::Server);
        if !activation_matches_commit(incoming, commit)
            || !candidate_matches_commit(commit)
            || committed_stamp(incoming, &commit.peer) != commit.previous_stamp
        {
            return Err(ProtocolError::new(
                ErrorCode::StaleGeneration,
                "incoming generation changed before observed finalization",
                true,
            ));
        }
        let queued_displaced =
            accepted_connections.finalize_candidate(commit.peer, commit.stamp)?;
        incoming.activating.remove(&commit.peer);
        let previous = incoming.activating_previous.remove(&commit.peer);
        incoming
            .current
            .insert(commit.peer, (commit.stamp, commit.candidate.clone()));
        incoming.committed_lineage.insert(commit.peer, commit.stamp);
        incoming
            .finalized_transitions
            .insert(commit.peer, commit.pending.clone());
        Ok((previous.map(|(_, handle)| handle), queued_displaced))
    }

    fn rollback_incoming(&self, commit: &IncomingCommit) -> Result<(), ProtocolError> {
        let mut state = self.state.write().map_err(|_| internal_state_error())?;
        let incoming = state.directional_mut(iroh::endpoint::Side::Server);
        if !activation_matches_commit(incoming, commit) {
            return Ok(());
        }
        incoming.activating.remove(&commit.peer);
        incoming.activating_previous.remove(&commit.peer);
        if let Some(active) = incoming.active.get_mut(&commit.peer) {
            active.retain(|handle| {
                handle
                    .upgrade()
                    .is_some_and(|connection| connection.stable_id() != commit.candidate_id)
            });
            if active.is_empty() {
                incoming.active.remove(&commit.peer);
            }
        }
        Ok(())
    }

    /// Resolve one exact ambiguous controller handover without allocating a
    /// generation or publishing an application connection. The decision is
    /// linearized with normal host finalization under the admission lock.
    fn reconcile_incoming(
        &self,
        accepted_connections: &AcceptedPeerQueue,
        controller: iroh::EndpointId,
        pending: &PendingCommit,
        expected_pairing_fence: PairingFence,
        local_host: iroh::EndpointId,
        execution_target_id: &str,
    ) -> Result<
        (
            Option<ConnectionStamp>,
            Vec<iroh::endpoint::WeakConnectionHandle>,
        ),
        ProtocolError,
    > {
        let mut state = self.state.write().map_err(|_| internal_state_error())?;
        let pairing_incarnation = state
            .incoming_controllers
            .allowed
            .get(&controller)
            .copied()
            .ok_or_else(|| {
                ProtocolError::new(
                    ErrorCode::Revoked,
                    "controller authorization is unavailable",
                    false,
                )
            })?;
        let fence = PairingFence::new(pairing_incarnation)?;
        if state.authorization_disabled || fence != expected_pairing_fence {
            return Err(ProtocolError::new(
                ErrorCode::Revoked,
                "pairing authorization changed during reconciliation",
                false,
            ));
        }
        pending.validate(execution_target_id, controller, local_host, fence)?;

        let incoming = &mut state.incoming_controllers;
        let committed = incoming.committed_lineage.get(&controller).copied();
        if committed == Some(pending.candidate_connection_stamp) {
            if incoming.finalized_transitions.get(&controller) != Some(pending) {
                return Err(ProtocolError::new(
                    ErrorCode::StaleGeneration,
                    "committed host transition does not match reconciliation",
                    true,
                ));
            }
            return Ok((committed, Vec::new()));
        }
        if committed != pending.previous_connection_stamp {
            return Err(ProtocolError::new(
                ErrorCode::StaleGeneration,
                "host lineage is unrelated to reconciliation",
                true,
            ));
        }

        let activation = incoming.activating.get(&controller).cloned();
        if let Some(activation) = activation {
            if activation.stamp != pending.candidate_connection_stamp {
                return Err(ProtocolError::new(
                    ErrorCode::TransportUnavailable,
                    "another controller handover is still resolving",
                    true,
                ));
            }
            if activation.pending != *pending {
                return Err(ProtocolError::new(
                    ErrorCode::StaleGeneration,
                    "active controller handover does not match reconciliation",
                    true,
                ));
            }
            accepted_connections.rollback_candidate(controller, activation.stamp)?;
            incoming.activating.remove(&controller);
            incoming.activating_previous.remove(&controller);
            let mut to_close = Vec::new();
            if let Some(active) = incoming.active.get_mut(&controller) {
                active.retain(|handle| {
                    let matches = handle.upgrade().is_some_and(|connection| {
                        connection.stable_id() == activation.candidate_id
                    });
                    if matches {
                        to_close.push(handle.clone());
                    }
                    !matches
                });
                if active.is_empty() {
                    incoming.active.remove(&controller);
                }
            }
            return Ok((committed, to_close));
        }
        Ok((committed, Vec::new()))
    }

    fn is_current_incoming(
        &self,
        peer: &iroh::EndpointId,
        stamp: ConnectionStamp,
        pairing_fence: PairingFence,
    ) -> bool {
        self.state
            .read()
            .map(|state| {
                !state.authorization_disabled
                    && state.incoming_controllers.allowed.get(peer).copied()
                        == Some(pairing_fence.pairing_incarnation())
                    && (state.incoming_controllers.current.get(peer).is_some_and(
                        |(current, handle)| *current == stamp && weak_connection_is_open(handle),
                    ) || state
                        .incoming_controllers
                        .activating_previous
                        .get(peer)
                        .is_some_and(|(previous, handle)| {
                            *previous == stamp && weak_connection_is_open(handle)
                        }))
            })
            .unwrap_or(false)
    }

    fn current_incoming_authorization(
        &self,
        peer: &iroh::EndpointId,
        stamp: ConnectionStamp,
        pairing_fence: PairingFence,
    ) -> Result<InstalledAuthorizationContext, ProtocolError> {
        let state = self.state.read().map_err(|_| internal_state_error())?;
        Self::current_incoming_authorization_in_state(&state, peer, stamp, pairing_fence)
    }

    fn current_incoming_authorization_in_state(
        state: &AdmissionState,
        peer: &iroh::EndpointId,
        stamp: ConnectionStamp,
        pairing_fence: PairingFence,
    ) -> Result<InstalledAuthorizationContext, ProtocolError> {
        let account_epoch = state
            .account_epoch
            .filter(|_| !state.authorization_disabled)
            .ok_or_else(|| {
                ProtocolError::new(
                    ErrorCode::Revoked,
                    "remote controller authorization is unavailable",
                    false,
                )
            })?;
        if state.snapshot_revision == 0
            || state.incoming_controllers.allowed.get(peer).copied()
                != Some(pairing_fence.pairing_incarnation())
            || !(state
                .incoming_controllers
                .current
                .get(peer)
                .is_some_and(|(current, handle)| {
                    *current == stamp && weak_connection_is_open(handle)
                })
                || state
                    .incoming_controllers
                    .activating_previous
                    .get(peer)
                    .is_some_and(|(previous, handle)| {
                        *previous == stamp && weak_connection_is_open(handle)
                    }))
        {
            return Err(ProtocolError::new(
                ErrorCode::Revoked,
                "remote controller generation is no longer authorized",
                false,
            ));
        }
        Ok(InstalledAuthorizationContext {
            account_epoch,
            snapshot_revision: state.snapshot_revision,
            snapshot_digest: authorization_snapshot_digest(
                account_epoch,
                state.snapshot_revision,
                &state.incoming_controllers.allowed,
                &state.outgoing_execution_targets.allowed,
            ),
        })
    }

    /// Capture host-side committed lineage while atomically fencing this
    /// endpoint's admission state. The caller consumes and closes the endpoint
    /// immediately afterwards, so no live transport handle is part of the
    /// returned value.
    fn capture_endpoint_lineage_and_fence(
        &self,
        local_endpoint: iroh::EndpointId,
        execution_target_id: Arc<str>,
    ) -> Result<EndpointLineageSnapshot, ProtocolError> {
        let (snapshot, to_close) = {
            let mut state = self.state.write().map_err(|_| internal_state_error())?;
            let account_epoch = state
                .account_epoch
                .filter(|_| !state.authorization_disabled)
                .ok_or_else(|| {
                    ProtocolError::new(
                        ErrorCode::Unauthorized,
                        "host lineage capture requires an active versioned authorization snapshot",
                        false,
                    )
                })?;
            let snapshot_revision = state.snapshot_revision;
            if snapshot_revision == 0 {
                return Err(ProtocolError::new(
                    ErrorCode::Unauthorized,
                    "host lineage capture requires a versioned authorization revision",
                    false,
                ));
            }
            let authorization_digest = authorization_snapshot_digest(
                account_epoch,
                snapshot_revision,
                &state.incoming_controllers.allowed,
                &state.outgoing_execution_targets.allowed,
            );
            let incoming_controllers = state
                .incoming_controllers
                .allowed
                .iter()
                .map(|(peer, pairing_incarnation)| {
                    (
                        *peer,
                        IncomingControllerLineage {
                            pairing_incarnation: *pairing_incarnation,
                            last_committed: state
                                .incoming_controllers
                                .committed_lineage
                                .get(peer)
                                .copied(),
                            finalized_transition: state
                                .incoming_controllers
                                .finalized_transitions
                                .get(peer)
                                .cloned(),
                        },
                    )
                })
                .collect::<HashMap<_, _>>();
            if incoming_controllers.len() > MAX_AUTHORIZED_PEERS_PER_DIRECTION {
                return Err(ProtocolError::new(
                    ErrorCode::Internal,
                    "host lineage exceeds Maple's peer bound",
                    false,
                ));
            }
            for (peer, lineage) in &incoming_controllers {
                validate_incoming_controller_lineage(
                    local_endpoint,
                    &execution_target_id,
                    *peer,
                    lineage,
                )?;
            }
            let snapshot = EndpointLineageSnapshot {
                local_endpoint,
                execution_target_id,
                account_epoch,
                snapshot_revision,
                authorization_digest,
                incoming_controllers,
            };

            // Linearize the handoff against register/activation under the same
            // lock. Advancing the process revision invalidates any handshake
            // which registered before this fence but has not finalized yet.
            bump_admission_revision(&mut state)?;
            state.authorization_disabled = true;
            let mut to_close = Vec::new();
            clear_directional_authorization(&mut state.incoming_controllers, &mut to_close);
            clear_directional_authorization(&mut state.outgoing_execution_targets, &mut to_close);
            (snapshot, to_close)
        };
        close_weak_connections(to_close, b"Maple endpoint lineage captured");
        Ok(snapshot)
    }

    /// Restore lineage only after the independently supplied current
    /// authorization snapshot has been installed and before the accept pump is
    /// started. A retained pairing survives unrelated authorization revisions;
    /// a changed pairing incarnation is an explicit lineage fence.
    fn restore_endpoint_lineage(
        &self,
        local_endpoint: iroh::EndpointId,
        execution_target_id: &str,
        snapshot: &EndpointLineageSnapshot,
    ) -> Result<(), ProtocolError> {
        if snapshot.local_endpoint != local_endpoint
            || snapshot.execution_target_id.as_ref() != execution_target_id
        {
            return Err(ProtocolError::new(
                ErrorCode::WrongEndpoint,
                "host lineage belongs to a different endpoint or execution target",
                false,
            ));
        }
        if snapshot.incoming_controllers.len() > MAX_AUTHORIZED_PEERS_PER_DIRECTION {
            return Err(ProtocolError::new(
                ErrorCode::Unauthorized,
                "host lineage exceeds Maple's peer bound",
                false,
            ));
        }
        if snapshot.snapshot_revision == 0 {
            return Err(ProtocolError::new(
                ErrorCode::Unauthorized,
                "host lineage authorization revision is invalid",
                false,
            ));
        }
        for (peer, lineage) in &snapshot.incoming_controllers {
            validate_incoming_controller_lineage(
                snapshot.local_endpoint,
                &snapshot.execution_target_id,
                *peer,
                lineage,
            )?;
        }

        let mut state = self.state.write().map_err(|_| internal_state_error())?;
        if state.account_epoch != Some(snapshot.account_epoch) {
            return Err(ProtocolError::new(
                ErrorCode::Unauthorized,
                "host lineage belongs to a different account authorization context",
                false,
            ));
        }
        if state.snapshot_revision < snapshot.snapshot_revision {
            return Err(stale_authorization_snapshot());
        }
        if state.snapshot_revision == snapshot.snapshot_revision
            && authorization_snapshot_digest(
                snapshot.account_epoch,
                state.snapshot_revision,
                &state.incoming_controllers.allowed,
                &state.outgoing_execution_targets.allowed,
            ) != snapshot.authorization_digest
        {
            return Err(stale_authorization_snapshot());
        }
        if state.authorization_disabled
            || state
                .incoming_controllers
                .active
                .values()
                .any(|handles| !handles.is_empty())
            || !state.incoming_controllers.current.is_empty()
            || !state.incoming_controllers.activating.is_empty()
            || !state.incoming_controllers.activating_previous.is_empty()
        {
            return Err(ProtocolError::new(
                ErrorCode::Unauthorized,
                "host lineage does not match a quiescent current authorization context",
                false,
            ));
        }
        for (peer, lineage) in &snapshot.incoming_controllers {
            if state.incoming_controllers.allowed.get(peer) == Some(&lineage.pairing_incarnation) {
                if let Some(last_committed) = lineage.last_committed {
                    state
                        .incoming_controllers
                        .committed_lineage
                        .insert(*peer, last_committed);
                }
                if let Some(transition) = &lineage.finalized_transition {
                    state
                        .incoming_controllers
                        .finalized_transitions
                        .insert(*peer, transition.clone());
                }
            }
        }
        Ok(())
    }

    fn replace_authorizations(
        &self,
        snapshot: AuthorizationSnapshot,
    ) -> Result<AuthorizationTransitionReceipt, ProtocolError> {
        if snapshot.account_epoch == 0 || snapshot.snapshot_revision == 0 {
            return Err(ProtocolError::new(
                ErrorCode::Unauthorized,
                "authorization snapshot version must be positive",
                false,
            ));
        }
        if snapshot.incoming_controllers.len() > MAX_AUTHORIZED_PEERS_PER_DIRECTION
            || snapshot.outgoing_execution_targets.len() > MAX_AUTHORIZED_PEERS_PER_DIRECTION
        {
            return Err(ProtocolError::new(
                ErrorCode::Unauthorized,
                "authorization snapshot exceeds Maple's peer limit",
                false,
            ));
        }
        let (receipt, to_close) = {
            let mut state = self.state.write().map_err(|_| internal_state_error())?;
            if state.authorization_disabled && state.account_epoch.is_none() {
                return Err(stale_authorization_snapshot());
            }
            let force_replace = match state.account_epoch {
                None => true,
                Some(current_account) if snapshot.account_epoch < current_account => {
                    return Err(stale_authorization_snapshot());
                }
                Some(current_account) if snapshot.account_epoch == current_account => {
                    if state.authorization_disabled {
                        return Err(stale_authorization_snapshot());
                    }
                    if snapshot.snapshot_revision < state.snapshot_revision {
                        return Err(stale_authorization_snapshot());
                    }
                    if snapshot.snapshot_revision == state.snapshot_revision {
                        if state.incoming_controllers.allowed == snapshot.incoming_controllers
                            && state.outgoing_execution_targets.allowed
                                == snapshot.outgoing_execution_targets
                        {
                            let current = installed_authorization_context(&state)?;
                            return Ok(AuthorizationTransitionReceipt {
                                authorization_domain: InstalledAuthorizationDomain(self.clone()),
                                previous: Some(current.clone()),
                                current,
                                removed_incoming_controllers: Vec::new(),
                                account_epoch_changed: false,
                            });
                        }
                        // Equal durable version with different canonical grants
                        // is control-plane equivocation. Poison admission before
                        // reporting it so the previously installed capability
                        // cannot remain live.
                        state.authorization_disabled = true;
                        let mut to_close = Vec::new();
                        clear_directional_authorization(
                            &mut state.incoming_controllers,
                            &mut to_close,
                        );
                        clear_directional_authorization(
                            &mut state.outgoing_execution_targets,
                            &mut to_close,
                        );
                        drop(state);
                        close_weak_connections(to_close, b"conflicting authorization snapshot");
                        return Err(authorization_snapshot_conflict());
                    }
                    false
                }
                Some(_) => true,
            };
            let previous = installed_authorization_context_if_enabled(&state);
            let previous_incoming = state
                .incoming_controllers
                .allowed
                .keys()
                .copied()
                .collect::<std::collections::HashSet<_>>();
            bump_admission_revision(&mut state)?;
            let mut to_close = Vec::new();
            replace_directional_authorization(
                &mut state.incoming_controllers,
                snapshot.incoming_controllers,
                &mut to_close,
                force_replace,
            );
            replace_directional_authorization(
                &mut state.outgoing_execution_targets,
                snapshot.outgoing_execution_targets,
                &mut to_close,
                force_replace,
            );
            state.account_epoch = Some(snapshot.account_epoch);
            state.snapshot_revision = snapshot.snapshot_revision;
            state.authorization_disabled = false;
            let current = installed_authorization_context(&state)?;
            let mut removed_incoming_controllers = previous_incoming
                .into_iter()
                .filter(|peer| !state.incoming_controllers.allowed.contains_key(peer))
                .collect::<Vec<_>>();
            removed_incoming_controllers.sort_unstable();
            let receipt = AuthorizationTransitionReceipt {
                authorization_domain: InstalledAuthorizationDomain(self.clone()),
                account_epoch_changed: previous
                    .as_ref()
                    .is_some_and(|previous| previous.account_epoch != current.account_epoch),
                previous,
                current,
                removed_incoming_controllers,
            };
            (receipt, to_close)
        };
        close_weak_connections(to_close, b"authorization snapshot replaced");
        Ok(receipt)
    }

    fn clear_all_and_close(&self) -> Result<(), ProtocolError> {
        let to_close = {
            let mut state = self.state.write().map_err(|_| internal_state_error())?;
            // Preserve the durable snapshot floor across sign-out. A delayed
            // old-account snapshot must not become "initial" and reauthorize
            // peers. The next account uses a larger account_epoch.
            // Clearing authority is terminal even if the process-local race
            // token has reached its numeric ceiling. Disable and erase the
            // capability first; token exhaustion must never preserve access.
            state.authorization_disabled = true;
            let mut to_close = Vec::new();
            clear_directional_authorization(&mut state.incoming_controllers, &mut to_close);
            clear_directional_authorization(&mut state.outgoing_execution_targets, &mut to_close);
            let revision_result = bump_admission_revision(&mut state);
            if revision_result.is_err() {
                state.admission_revision = u64::MAX;
            }
            to_close
        };
        close_weak_connections(to_close, b"authorization state cleared");
        Ok(())
    }
}

fn committed_stamp(
    incoming: &DirectionalAdmission,
    peer: &iroh::EndpointId,
) -> Option<ConnectionStamp> {
    incoming.committed_lineage.get(peer).copied()
}

fn candidate_matches_commit(commit: &IncomingCommit) -> bool {
    commit.candidate.upgrade().is_some_and(|connection| {
        connection.stable_id() == commit.candidate_id && connection.close_reason().is_none()
    })
}

fn activation_matches_commit(incoming: &DirectionalAdmission, commit: &IncomingCommit) -> bool {
    incoming
        .activating
        .get(&commit.peer)
        .is_some_and(|activation| {
            activation.stamp == commit.stamp
                && activation.candidate_id == commit.candidate_id
                && activation.pending == commit.pending
        })
}

fn replace_directional_authorization(
    directional: &mut DirectionalAdmission,
    allowed: HashMap<iroh::EndpointId, PairingIncarnation>,
    to_close: &mut Vec<iroh::endpoint::WeakConnectionHandle>,
    force_replace: bool,
) {
    let fenced = directional
        .allowed
        .iter()
        .filter_map(|(peer, incarnation)| {
            (force_replace || allowed.get(peer) != Some(incarnation)).then_some(*peer)
        })
        .collect::<HashSet<_>>();
    directional.allowed = allowed;
    if force_replace {
        directional.committed_lineage.clear();
        directional.finalized_transitions.clear();
        directional.current.clear();
        directional.activating.clear();
        directional.activating_previous.clear();
    } else {
        // Authorization removal is also a lineage fence even when the old
        // transport handle was already pruned. Re-adding the same endpoint is
        // an explicit new pairing lineage whose first bootstrap names `None`.
        let allowed = directional.allowed.clone();
        directional
            .committed_lineage
            .retain(|peer, _| allowed.contains_key(peer) && !fenced.contains(peer));
        directional
            .finalized_transitions
            .retain(|peer, _| allowed.contains_key(peer) && !fenced.contains(peer));
        directional
            .current
            .retain(|peer, _| allowed.contains_key(peer) && !fenced.contains(peer));
        directional
            .activating
            .retain(|peer, _| allowed.contains_key(peer) && !fenced.contains(peer));
        directional
            .activating_previous
            .retain(|peer, _| allowed.contains_key(peer) && !fenced.contains(peer));
    }
    let revoked = directional
        .active
        .keys()
        .copied()
        .filter(|peer| force_replace || fenced.contains(peer))
        .collect::<Vec<_>>();
    for peer in revoked {
        directional.committed_lineage.remove(&peer);
        directional.finalized_transitions.remove(&peer);
        directional.current.remove(&peer);
        directional.activating.remove(&peer);
        directional.activating_previous.remove(&peer);
        if let Some(handles) = directional.active.remove(&peer) {
            to_close.extend(handles);
        }
    }
}

fn clear_directional_authorization(
    directional: &mut DirectionalAdmission,
    to_close: &mut Vec<iroh::endpoint::WeakConnectionHandle>,
) {
    directional.allowed.clear();
    directional.committed_lineage.clear();
    directional.finalized_transitions.clear();
    directional.current.clear();
    directional.activating.clear();
    directional.activating_previous.clear();
    for (_, handles) in directional.active.drain() {
        to_close.extend(handles);
    }
}

fn bump_admission_revision(state: &mut AdmissionState) -> Result<u64, ProtocolError> {
    state.admission_revision = state.admission_revision.checked_add(1).ok_or_else(|| {
        ProtocolError::new(ErrorCode::Internal, "admission revision exhausted", false)
    })?;
    Ok(state.admission_revision)
}

fn installed_authorization_context_if_enabled(
    state: &AdmissionState,
) -> Option<InstalledAuthorizationContext> {
    let account_epoch = state
        .account_epoch
        .filter(|_| !state.authorization_disabled)?;
    (state.snapshot_revision != 0).then(|| InstalledAuthorizationContext {
        account_epoch,
        snapshot_revision: state.snapshot_revision,
        snapshot_digest: authorization_snapshot_digest(
            account_epoch,
            state.snapshot_revision,
            &state.incoming_controllers.allowed,
            &state.outgoing_execution_targets.allowed,
        ),
    })
}

fn installed_authorization_context(
    state: &AdmissionState,
) -> Result<InstalledAuthorizationContext, ProtocolError> {
    installed_authorization_context_if_enabled(state).ok_or_else(stale_authorization_snapshot)
}

fn authorization_snapshot_conflict() -> ProtocolError {
    ProtocolError::new(
        ErrorCode::Revoked,
        "authorization snapshot version has conflicting canonical grants",
        false,
    )
}

fn stale_authorization_snapshot() -> ProtocolError {
    ProtocolError::new(
        ErrorCode::Revoked,
        "authorization snapshot is stale or conflicting",
        false,
    )
}

fn weak_connection_is_open(handle: &iroh::endpoint::WeakConnectionHandle) -> bool {
    handle
        .upgrade()
        .is_some_and(|connection| connection.close_reason().is_none())
}

fn close_weak_connections(
    handles: impl IntoIterator<Item = iroh::endpoint::WeakConnectionHandle>,
    reason: &[u8],
) {
    for handle in handles {
        if let Some(connection) = handle.upgrade() {
            connection.close(iroh::endpoint::VarInt::from_u32(0x4d_52), reason);
        }
    }
}

impl iroh::endpoint::EndpointHooks for PeerAdmission {
    fn before_connect<'a>(
        &'a self,
        remote_addr: &'a iroh::EndpointAddr,
        alpn: &'a [u8],
    ) -> impl Future<Output = iroh::endpoint::BeforeConnectOutcome> + Send + 'a {
        let accepted =
            alpn == ALPN && self.is_allowed(iroh::endpoint::Side::Client, &remote_addr.id);
        async move {
            if accepted {
                iroh::endpoint::BeforeConnectOutcome::Accept
            } else {
                iroh::endpoint::BeforeConnectOutcome::Reject
            }
        }
    }

    fn after_handshake<'a>(
        &'a self,
        connection: &'a iroh::endpoint::Connection,
    ) -> impl Future<Output = iroh::endpoint::AfterHandshakeOutcome> + Send + 'a {
        let accepted = connection.alpn() == ALPN
            && self.is_allowed(connection.side(), &connection.remote_id());
        async move {
            if accepted {
                iroh::endpoint::AfterHandshakeOutcome::Accept
            } else {
                iroh::endpoint::AfterHandshakeOutcome::Reject {
                    error_code: iroh::endpoint::VarInt::from_u32(0x4d_50),
                    reason: b"peer not authorized".to_vec(),
                }
            }
        }
    }
}

fn spawn_incoming_stream_dispatcher(
    connection: iroh::endpoint::Connection,
    connection_stamp: ConnectionStamp,
    execution_target_id: Arc<str>,
    expected_direction: PeerDirection,
    frame_deadline: Duration,
    lane_capacity: Arc<LaneSemaphores>,
) -> Arc<IncomingStreamDispatcher> {
    let (shutdown_tx, mut shutdown_rx) = oneshot::channel();
    let queue = Arc::new(PreparedStreamQueue::default());
    let task_queue = queue.clone();
    tokio::spawn(async move {
        let mut headers = futures_util::stream::FuturesUnordered::new();
        loop {
            tokio::select! {
                biased;
                _ = &mut shutdown_rx => break,
                completed = headers.next(), if !headers.is_empty() => {
                    let result = match completed {
                        Some(Ok(result)) => result,
                        Some(Err(_)) => Err(internal_state_error()),
                        None => continue,
                    };
                    task_queue.publish(result);
                }
                incoming = connection.accept_bi(), if headers.len() < MAX_APPLICATION_STREAM_TASKS => {
                    let streams = match incoming {
                        Ok(streams) => streams,
                        Err(_) => break,
                    };
                    let execution_target_id = execution_target_id.clone();
                    let lane_capacity = lane_capacity.clone();
                    headers.push(tokio::spawn(async move {
                        prepare_accepted_stream(
                            streams,
                            connection_stamp,
                            execution_target_id,
                            expected_direction,
                            frame_deadline,
                            lane_capacity,
                        )
                        .await
                    }));
                }
            }
        }
        for task in headers.iter() {
            task.abort();
        }
        while headers.next().await.is_some() {}
        task_queue.close();
    });
    Arc::new(IncomingStreamDispatcher {
        queue,
        shutdown: Mutex::new(Some(shutdown_tx)),
    })
}

async fn prepare_accepted_stream(
    (send, mut recv): (iroh::endpoint::SendStream, iroh::endpoint::RecvStream),
    connection_stamp: ConnectionStamp,
    execution_target_id: Arc<str>,
    expected_direction: PeerDirection,
    frame_deadline: Duration,
    lane_capacity: Arc<LaneSemaphores>,
) -> Result<AcceptedStream, ProtocolError> {
    let now = tokio::time::Instant::now();
    let operation_deadline = now + frame_deadline;
    // The lane is unknown until this tiny header is decoded. Cap this
    // preclassification phase independently so a peer cannot occupy every QUIC
    // stream credit with partial headers for a long application-frame budget.
    let header_deadline = now + frame_deadline.min(MAX_STREAM_HEADER_DEADLINE);
    let header: StreamHeader = read_frame_until(&mut recv, header_deadline).await?;
    header.validate(expected_direction)?;
    if header.connection_stamp != connection_stamp {
        return Err(ProtocolError::new(
            ErrorCode::StaleGeneration,
            "stream belongs to a stale connection generation",
            true,
        ));
    }
    // Quotas are lane-specific so Bulk work cannot consume capacity reserved
    // for Control reconnect/session operations. A peer cannot gain Control
    // priority by relabelling a Bulk DTO: typed validation below binds the lane.
    let permit = lane_capacity
        .for_kind(header.stream_kind)
        .try_acquire_owned()
        .map_err(|_| {
            ProtocolError::new(
                ErrorCode::TransportUnavailable,
                "Maple stream lane is at capacity",
                true,
            )
        })?;
    // Until the typed operation is validated, the response half remains Bulk.
    send.set_priority(StreamKind::Bulk.priority())
        .map_err(|error| {
            transport_error(
                "failed to prioritize accepted Maple remote stream",
                error,
                true,
            )
        })?;
    Ok(AcceptedStream {
        header,
        send,
        recv,
        execution_target_id,
        connection_stamp,
        operation_deadline: Some(operation_deadline),
        _permit: permit,
    })
}

impl ConnectedPeer {
    fn new(
        connection: iroh::endpoint::Connection,
        connection_stamp: ConnectionStamp,
        pairing_fence: PairingFence,
        execution_target_id: Arc<str>,
        outbound_direction: PeerDirection,
        frame_deadline: Duration,
    ) -> Self {
        let inbound_stream_tasks = Arc::new(LaneSemaphores::new());
        let incoming_streams = spawn_incoming_stream_dispatcher(
            connection.clone(),
            connection_stamp,
            execution_target_id.clone(),
            outbound_direction.opposite(),
            frame_deadline,
            inbound_stream_tasks,
        );
        Self {
            connection,
            connection_stamp,
            pairing_fence,
            execution_target_id,
            outbound_direction,
            frame_deadline,
            outbound_requests: Arc::new(LaneSemaphores::new()),
            incoming_streams,
        }
    }

    pub fn remote_id(&self) -> iroh::EndpointId {
        self.connection.remote_id()
    }

    pub fn connection_stamp(&self) -> ConnectionStamp {
        self.connection_stamp
    }

    /// The directed pairing incarnation admitted during this connection's
    /// bootstrap. Application adapters revalidate this token immediately
    /// before dispatch so revoke-and-re-pair cannot reuse a queued stream.
    pub fn pairing_fence(&self) -> PairingFence {
        self.pairing_fence
    }

    pub fn execution_target_id(&self) -> &str {
        &self.execution_target_id
    }

    /// Resolve when this generation's transport is lost or explicitly closed.
    /// The underlying Iroh reason/path state is deliberately not returned: it
    /// may contain network addresses. Owners race this signal with requests and
    /// platform `network_change` callbacks to begin cached reconnect promptly.
    pub async fn wait_closed(&self) {
        let _ = self.connection.closed().await;
    }

    /// Explicitly stop this connection generation and wake every cloned
    /// owner's loss signal. Dropping the last Iroh handle also closes it, but
    /// lifecycle owners should call this when superseding/signing out so a
    /// forgotten clone cannot keep an obsolete generation alive.
    pub fn close(&self) {
        self.connection.close(
            iroh::endpoint::VarInt::from_u32(0),
            b"Maple connection generation stopped",
        );
    }

    #[cfg(test)]
    fn raw_connection(&self) -> &iroh::endpoint::Connection {
        &self.connection
    }

    /// Complete one typed request/response exchange on its operation-derived
    /// stream lane. Neither QUIC stream escapes this boundary: both frames are
    /// bounded and the response is correlated to the request, execution target,
    /// connection stamp, and lane before it is returned.
    pub async fn request<TRequest, TResponse>(
        &self,
        request: &RequestEnvelope<TRequest>,
    ) -> Result<ResponseEnvelope<TResponse>, ProtocolError>
    where
        TRequest: RequestBody + Serialize,
        TResponse: ResponseBody<TRequest> + DeserializeOwned,
    {
        let operation_deadline = tokio::time::Instant::now() + self.frame_deadline;
        let kind = request.body.stream_kind();
        let _permit = tokio::time::timeout_at(
            operation_deadline,
            self.outbound_requests.for_kind(kind).acquire_owned(),
        )
        .await
        .map_err(|_| {
            ProtocolError::new(
                ErrorCode::TransportUnavailable,
                "Maple outbound request capacity deadline elapsed",
                true,
            )
        })?
        .map_err(|_| internal_state_error())?;
        let (mut send, mut recv) = self
            .open_request_stream(request, operation_deadline)
            .await?;
        send.finish().map_err(|error| {
            transport_error("failed to finish Maple remote request", error, true)
        })?;
        let response: ResponseEnvelope<TResponse> =
            read_frame_until(&mut recv, operation_deadline).await?;
        response.validate(
            &request.request_id,
            &self.execution_target_id,
            self.connection_stamp,
            kind,
        )?;
        if let Ok(body) = &response.result {
            body.validate_response_to(&request.body)?;
        }
        Ok(response)
    }

    /// Start one typed request whose response is a bounded sequence of frames.
    ///
    /// Bulk history pages use this instead of placing a count-bounded page in
    /// one frame. Each native record therefore retains the universal 1 MiB
    /// frame cap without making aggregate byte size a pagination primitive.
    pub async fn start_streaming_request<TRequest>(
        &self,
        request: RequestEnvelope<TRequest>,
    ) -> Result<StreamingResponse<TRequest>, ProtocolError>
    where
        TRequest: RequestBody + Serialize,
    {
        let request_deadline = tokio::time::Instant::now() + self.frame_deadline;
        let kind = request.body.stream_kind();
        let permit = tokio::time::timeout_at(
            request_deadline,
            self.outbound_requests.for_kind(kind).acquire_owned(),
        )
        .await
        .map_err(|_| {
            ProtocolError::new(
                ErrorCode::TransportUnavailable,
                "Maple outbound request capacity deadline elapsed",
                true,
            )
        })?
        .map_err(|_| internal_state_error())?;
        let (mut send, recv) = self.open_request_stream(&request, request_deadline).await?;
        send.finish().map_err(|error| {
            transport_error("failed to finish Maple remote request", error, true)
        })?;
        Ok(StreamingResponse {
            recv,
            request,
            execution_target_id: Arc::clone(&self.execution_target_id),
            connection_stamp: self.connection_stamp,
            stream_kind: kind,
            operation_deadline: match kind {
                StreamKind::Events => None,
                StreamKind::Bulk => {
                    Some(tokio::time::Instant::now() + BULK_STREAMING_OPERATION_DEADLINE)
                }
                StreamKind::Control => Some(tokio::time::Instant::now() + self.frame_deadline),
            },
            _permit: permit,
        })
    }

    async fn open_request_stream<T>(
        &self,
        request: &RequestEnvelope<T>,
        operation_deadline: tokio::time::Instant,
    ) -> Result<(iroh::endpoint::SendStream, iroh::endpoint::RecvStream), ProtocolError>
    where
        T: RequestBody + Serialize,
    {
        let kind = request.body.stream_kind();
        if request.direction != request.body.allowed_direction() {
            return Err(ProtocolError::new(
                ErrorCode::WrongDirection,
                "request operation is not allowed in this direction",
                false,
            ));
        }
        request.validate(
            self.outbound_direction,
            &self.execution_target_id,
            self.connection_stamp,
            kind,
        )?;
        let (mut send, recv) =
            tokio::time::timeout_at(operation_deadline, self.connection.open_bi())
                .await
                .map_err(|_| operation_timeout("Maple stream-open deadline elapsed"))?
                .map_err(|error| {
                    transport_error("failed to open Maple remote stream", error, true)
                })?;
        send.set_priority(kind.priority()).map_err(|error| {
            transport_error("failed to prioritize Maple remote stream", error, true)
        })?;
        write_frame_until(
            &mut send,
            &StreamHeader {
                protocol_version: PROTOCOL_VERSION,
                stream_kind: kind,
                direction: self.outbound_direction,
                connection_stamp: self.connection_stamp,
            },
            operation_deadline,
        )
        .await?;
        write_frame_until(&mut send, request, operation_deadline).await?;
        Ok((send, recv))
    }

    pub async fn accept_stream(&self) -> Result<AcceptedStream, ProtocolError> {
        let deadline = tokio::time::Instant::now() + self.frame_deadline;
        self.incoming_streams.queue.recv(deadline).await
    }
}

/// Controller half of a typed multi-frame response on one request stream.
/// Every frame is independently correlated and validated before it escapes.
pub struct StreamingResponse<TRequest: RequestBody> {
    recv: iroh::endpoint::RecvStream,
    request: RequestEnvelope<TRequest>,
    execution_target_id: Arc<str>,
    connection_stamp: ConnectionStamp,
    stream_kind: StreamKind,
    operation_deadline: Option<tokio::time::Instant>,
    _permit: OwnedSemaphorePermit,
}

impl<TRequest: RequestBody> StreamingResponse<TRequest> {
    pub async fn read<TResponse>(&mut self) -> Result<ResponseEnvelope<TResponse>, ProtocolError>
    where
        TResponse: ResponseBody<TRequest> + DeserializeOwned,
    {
        let response: ResponseEnvelope<TResponse> = match self.operation_deadline {
            Some(deadline) => read_frame_until(&mut self.recv, deadline).await?,
            None => read_frame_unbounded(&mut self.recv).await?,
        };
        response.validate(
            &self.request.request_id,
            &self.execution_target_id,
            self.connection_stamp,
            self.stream_kind,
        )?;
        if let Ok(body) = &response.result {
            body.validate_response_to(&self.request.body)?;
        }
        Ok(response)
    }

    /// Require the host to terminate immediately after the protocol footer.
    pub async fn finish(mut self) -> Result<(), ProtocolError> {
        match self.operation_deadline {
            Some(deadline) => expect_stream_end(&mut self.recv, deadline).await,
            None => expect_stream_end_unbounded(&mut self.recv).await,
        }
    }
}

pub struct AcceptedStream {
    header: StreamHeader,
    send: iroh::endpoint::SendStream,
    recv: iroh::endpoint::RecvStream,
    execution_target_id: Arc<str>,
    connection_stamp: ConnectionStamp,
    operation_deadline: Option<tokio::time::Instant>,
    _permit: OwnedSemaphorePermit,
}

impl std::fmt::Debug for AcceptedStream {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("AcceptedStream")
            .field("stream_kind", &self.header.stream_kind)
            .field("connection_stamp", &self.connection_stamp)
            .finish_non_exhaustive()
    }
}

impl AcceptedStream {
    pub fn header(&self) -> &StreamHeader {
        &self.header
    }

    #[cfg(test)]
    fn send_stream(&mut self) -> &mut iroh::endpoint::SendStream {
        &mut self.send
    }

    pub async fn read_request<T>(mut self) -> Result<AcceptedRequest<T>, ProtocolError>
    where
        T: RequestBody + DeserializeOwned,
    {
        let request_deadline = self
            .operation_deadline
            .expect("accepted streams retain a deadline until the request is decoded");
        let request: RequestEnvelope<T> =
            read_frame_until(&mut self.recv, request_deadline).await?;
        request.validate(
            self.header.direction,
            &self.execution_target_id,
            self.connection_stamp,
            self.header.stream_kind,
        )?;
        if request.direction != request.body.allowed_direction() {
            return Err(ProtocolError::new(
                ErrorCode::WrongDirection,
                "request operation is not allowed in this direction",
                false,
            ));
        }
        self.send
            .set_priority(self.header.stream_kind.priority())
            .map_err(|error| {
                transport_error(
                    "failed to prioritize validated Maple remote stream",
                    error,
                    true,
                )
            })?;
        match request.body.stream_kind() {
            StreamKind::Bulk => {
                self.operation_deadline =
                    Some(tokio::time::Instant::now() + BULK_STREAMING_OPERATION_DEADLINE);
            }
            StreamKind::Events => {
                // An Events stream is bounded by its authenticated connection,
                // explicit RPC cancellation/revocation, and lane permit. It has
                // no wall-clock or idle-read lifetime deadline.
                self.operation_deadline = None;
            }
            StreamKind::Control => {}
        }
        Ok(AcceptedRequest {
            stream: self,
            request,
        })
    }
}

pub struct AcceptedRequest<T: RequestBody> {
    stream: AcceptedStream,
    request: RequestEnvelope<T>,
}

impl<T: RequestBody> std::fmt::Debug for AcceptedRequest<T> {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("AcceptedRequest")
            .field("request_id", &self.request.request_id)
            .field("stream_kind", &self.stream.header.stream_kind)
            .field("connection_stamp", &self.stream.connection_stamp)
            .finish_non_exhaustive()
    }
}

impl<T: RequestBody> AcceptedRequest<T> {
    pub fn request(&self) -> &RequestEnvelope<T> {
        &self.request
    }

    /// Deadline for one finite provider/adapter phase. Bulk and Control inherit
    /// their absolute operation deadline. Events deliberately has no lifetime
    /// deadline, so callers receive a fresh bounded phase deadline instead;
    /// they must never use this value as an Events read/stream lifetime.
    pub(crate) fn operation_deadline(&self) -> tokio::time::Instant {
        self.stream
            .operation_deadline
            .unwrap_or_else(|| tokio::time::Instant::now() + EVENT_FRAME_WRITE_DEADLINE)
    }

    /// Resolves when the controller abandons the response stream or the
    /// connection is lost. The returned future owns the QUIC stop waiter, so a
    /// host adapter can race it against injected work without borrowing this
    /// request and can still consume `self` to write a successful response.
    pub(crate) fn response_cancelled(
        &self,
    ) -> impl Future<Output = Result<(), ProtocolError>> + Send + 'static {
        let stopped = self.stream.send.stopped();
        async move {
            match stopped.await {
                Ok(Some(_)) => Ok(()),
                Ok(None) => Err(ProtocolError::new(
                    ErrorCode::TransportUnavailable,
                    "remote response stream ended before host dispatch",
                    true,
                )),
                Err(_) => Ok(()),
            }
        }
    }

    #[cfg(test)]
    fn send_stream(&mut self) -> &mut iroh::endpoint::SendStream {
        &mut self.stream.send
    }

    pub async fn write_response<TResponse>(
        mut self,
        response: &ResponseEnvelope<TResponse>,
    ) -> Result<(), ProtocolError>
    where
        TResponse: ResponseBody<T> + Serialize,
    {
        self.write_response_frame(response).await?;
        self.finish_response()
    }

    /// Write one independently bounded and correlated response frame while
    /// retaining the stream for a subsequent record or footer.
    pub async fn write_response_frame<TResponse>(
        &mut self,
        response: &ResponseEnvelope<TResponse>,
    ) -> Result<(), ProtocolError>
    where
        TResponse: ResponseBody<T> + Serialize,
    {
        self.validate_response_frame(response)?;
        let deadline = self
            .stream
            .operation_deadline
            .unwrap_or_else(|| tokio::time::Instant::now() + EVENT_FRAME_WRITE_DEADLINE);
        let result = write_frame_until(&mut self.stream.send, response, deadline).await;
        if result.is_err() && self.stream.header.stream_kind == StreamKind::Events {
            // A blocked controller cannot retain an Events lane or its native
            // subscription indefinitely. The RPC owner observes this error and
            // acknowledges subscription cancellation; resetting this exact
            // QUIC response stream fences any late frame bytes immediately.
            let _ = self
                .stream
                .send
                .reset(iroh::endpoint::VarInt::from_u32(0x4d_57));
        }
        result
    }

    /// Apply the exact semantic/correlation checks used by a response write
    /// without emitting bytes. Multi-frame adapters preflight every frame with
    /// this before Start so a malformed late record cannot create a partial
    /// page on the wire.
    pub(crate) fn validate_response_frame<TResponse>(
        &self,
        response: &ResponseEnvelope<TResponse>,
    ) -> Result<(), ProtocolError>
    where
        TResponse: ResponseBody<T>,
    {
        response.validate(
            &self.request.request_id,
            &self.stream.execution_target_id,
            self.stream.connection_stamp,
            self.stream.header.stream_kind,
        )?;
        if let Ok(body) = &response.result {
            body.validate_response_to(&self.request.body)?;
        }
        Ok(())
    }

    pub fn finish_response(mut self) -> Result<(), ProtocolError> {
        self.stream
            .send
            .finish()
            .map_err(|error| transport_error("failed to finish Maple remote response", error, true))
    }
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PendingCommit {
    pairing_fence: PairingFence,
    original_request_id: String,
    execution_target_id: String,
    controller_id: String,
    host_id: String,
    previous_connection_stamp: Option<ConnectionStamp>,
    candidate_connection_stamp: ConnectionStamp,
}

impl PendingCommit {
    fn new(
        pairing_fence: PairingFence,
        original_request_id: &str,
        execution_target_id: &str,
        controller_id: iroh::EndpointId,
        host_id: iroh::EndpointId,
        previous_connection_stamp: Option<ConnectionStamp>,
        candidate_connection_stamp: ConnectionStamp,
    ) -> Result<Self, ProtocolError> {
        let pending = Self {
            pairing_fence,
            original_request_id: original_request_id.into(),
            execution_target_id: execution_target_id.into(),
            controller_id: controller_id.to_string(),
            host_id: host_id.to_string(),
            previous_connection_stamp,
            candidate_connection_stamp,
        };
        pending.validate(execution_target_id, controller_id, host_id, pairing_fence)?;
        Ok(pending)
    }

    fn validate(
        &self,
        expected_target: &str,
        expected_controller: iroh::EndpointId,
        expected_host: iroh::EndpointId,
        expected_fence: PairingFence,
    ) -> Result<(), ProtocolError> {
        self.pairing_fence.validate()?;
        validate_bootstrap_id("original_request_id", &self.original_request_id)?;
        validate_bootstrap_id("execution_target_id", &self.execution_target_id)?;
        validate_bootstrap_id("controller_id", &self.controller_id)?;
        validate_bootstrap_id("host_id", &self.host_id)?;
        self.candidate_connection_stamp.validate()?;
        if let Some(previous) = self.previous_connection_stamp {
            previous.validate()?;
            if previous >= self.candidate_connection_stamp {
                return Err(ProtocolError::new(
                    ErrorCode::StaleGeneration,
                    "pending handover does not advance its predecessor",
                    true,
                ));
            }
        }
        if self.execution_target_id != expected_target
            || self.controller_id != expected_controller.to_string()
            || self.host_id != expected_host.to_string()
            || self.pairing_fence != expected_fence
        {
            return Err(ProtocolError::new(
                ErrorCode::Unauthorized,
                "pending handover does not match the current pairing",
                false,
            ));
        }
        Ok(())
    }
}

fn validate_incoming_controller_lineage(
    local_endpoint: iroh::EndpointId,
    execution_target_id: &str,
    controller: iroh::EndpointId,
    lineage: &IncomingControllerLineage,
) -> Result<(), ProtocolError> {
    match (lineage.last_committed, &lineage.finalized_transition) {
        (Some(last_committed), Some(transition)) => {
            last_committed.validate()?;
            if transition.candidate_connection_stamp != last_committed {
                return Err(ProtocolError::new(
                    ErrorCode::StaleGeneration,
                    "host lineage transition does not match its committed stamp",
                    false,
                ));
            }
            let pairing_fence = PairingFence::new(lineage.pairing_incarnation)?;
            transition.validate(
                execution_target_id,
                controller,
                local_endpoint,
                pairing_fence,
            )?;
        }
        (Some(_), None) => {
            return Err(ProtocolError::new(
                ErrorCode::StaleGeneration,
                "committed host lineage is missing its exact transition",
                false,
            ));
        }
        (None, Some(_)) => {
            return Err(ProtocolError::new(
                ErrorCode::StaleGeneration,
                "genesis host lineage cannot contain a finalized transition",
                false,
            ));
        }
        (None, None) => {}
    }
    Ok(())
}

#[derive(Debug, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct BootstrapRequest {
    protocol_version: u16,
    request_id: String,
    execution_target_id: String,
    bootstrap_generation: u8,
    pairing_fence: PairingFence,
    previous_connection_stamp: Option<ConnectionStamp>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    reconciliation: Option<PendingCommit>,
}

impl BootstrapRequest {
    fn validate(
        &self,
        expected_target: &str,
        expected_fence: PairingFence,
    ) -> Result<(), ProtocolError> {
        if self.protocol_version != PROTOCOL_VERSION {
            return Err(ProtocolError::new(
                ErrorCode::UnsupportedVersion,
                "bootstrap protocol version is unsupported",
                false,
            ));
        }
        validate_bootstrap_id("request_id", &self.request_id)?;
        validate_bootstrap_id("execution_target_id", &self.execution_target_id)?;
        if self.execution_target_id != expected_target {
            return Err(ProtocolError::new(
                ErrorCode::WrongEndpoint,
                "bootstrap execution target does not match this host",
                false,
            ));
        }
        if self.bootstrap_generation != 0 {
            return Err(ProtocolError::new(
                ErrorCode::StaleGeneration,
                "bootstrap lane must use generation zero",
                false,
            ));
        }
        validate_bootstrap_pairing_fence(self.pairing_fence, expected_fence)?;
        if let Some(previous) = self.previous_connection_stamp {
            previous.validate()?;
        }
        if let Some(pending) = &self.reconciliation {
            if pending.pairing_fence != self.pairing_fence {
                return Err(ProtocolError::new(
                    ErrorCode::Unauthorized,
                    "reconciliation pairing fence does not match bootstrap",
                    false,
                ));
            }
            if self.previous_connection_stamp != pending.previous_connection_stamp {
                return Err(ProtocolError::new(
                    ErrorCode::StaleGeneration,
                    "reconciliation predecessor does not match its pending handover",
                    true,
                ));
            }
        }
        Ok(())
    }
}

#[derive(Debug, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct BootstrapAccepted {
    connection_stamp: ConnectionStamp,
}

#[derive(Debug, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct BootstrapResponse {
    protocol_version: u16,
    request_id: String,
    execution_target_id: String,
    pairing_fence: PairingFence,
    result: Result<BootstrapAccepted, ProtocolError>,
}

#[derive(Debug, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct BootstrapReady {
    protocol_version: u16,
    request_id: String,
    execution_target_id: String,
    controller_id: String,
    pairing_fence: PairingFence,
    connection_stamp: ConnectionStamp,
    previous_connection_stamp: Option<ConnectionStamp>,
}

impl BootstrapReady {
    fn validate(
        &self,
        expected_request_id: &str,
        expected_target: &str,
        expected_controller: iroh::EndpointId,
        expected_fence: PairingFence,
        expected_stamp: ConnectionStamp,
        expected_previous: Option<ConnectionStamp>,
    ) -> Result<(), ProtocolError> {
        validate_bootstrap_pairing_fence(self.pairing_fence, expected_fence)?;
        if self.protocol_version != PROTOCOL_VERSION
            || self.request_id != expected_request_id
            || self.execution_target_id != expected_target
            || self.controller_id != expected_controller.to_string()
            || self.connection_stamp != expected_stamp
            || self.previous_connection_stamp != expected_previous
        {
            return Err(ProtocolError::new(
                ErrorCode::StaleGeneration,
                "bootstrap readiness did not match the committed connection",
                true,
            ));
        }
        validate_bootstrap_id("request_id", &self.request_id)?;
        validate_bootstrap_id("execution_target_id", &self.execution_target_id)?;
        self.connection_stamp.validate()
    }
}

#[derive(Debug, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct BootstrapInstalled {
    protocol_version: u16,
    request_id: String,
    execution_target_id: String,
    controller_id: String,
    pairing_fence: PairingFence,
    connection_stamp: ConnectionStamp,
    previous_connection_stamp: Option<ConnectionStamp>,
}

#[derive(Debug, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct BootstrapCommitted {
    protocol_version: u16,
    request_id: String,
    execution_target_id: String,
    controller_id: String,
    pairing_fence: PairingFence,
    connection_stamp: ConnectionStamp,
    previous_connection_stamp: Option<ConnectionStamp>,
}

#[derive(Debug, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct BootstrapCommitObserved {
    protocol_version: u16,
    request_id: String,
    execution_target_id: String,
    controller_id: String,
    pairing_fence: PairingFence,
    connection_stamp: ConnectionStamp,
    previous_connection_stamp: Option<ConnectionStamp>,
}

#[derive(Debug, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct BootstrapFinalized {
    protocol_version: u16,
    request_id: String,
    execution_target_id: String,
    controller_id: String,
    pairing_fence: PairingFence,
    connection_stamp: ConnectionStamp,
    previous_connection_stamp: Option<ConnectionStamp>,
}

#[derive(Debug, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct BootstrapReconciled {
    protocol_version: u16,
    request_id: String,
    execution_target_id: String,
    controller_id: String,
    host_id: String,
    pending: PendingCommit,
    committed_connection_stamp: Option<ConnectionStamp>,
}

impl BootstrapReconciled {
    fn validate(
        &self,
        expected_request_id: &str,
        expected_target: &str,
        expected_controller: iroh::EndpointId,
        expected_host: iroh::EndpointId,
        expected_pending: &PendingCommit,
        expected_fence: PairingFence,
    ) -> Result<Option<ConnectionStamp>, ProtocolError> {
        validate_bootstrap_id("request_id", &self.request_id)?;
        self.pending.validate(
            expected_target,
            expected_controller,
            expected_host,
            expected_fence,
        )?;
        if self.protocol_version != PROTOCOL_VERSION
            || self.request_id != expected_request_id
            || self.execution_target_id != expected_target
            || self.controller_id != expected_controller.to_string()
            || self.host_id != expected_host.to_string()
            || &self.pending != expected_pending
        {
            return Err(ProtocolError::new(
                ErrorCode::StaleGeneration,
                "bootstrap reconciliation correlation failed",
                true,
            ));
        }
        if let Some(committed) = self.committed_connection_stamp {
            committed.validate()?;
        }
        if self.committed_connection_stamp != expected_pending.previous_connection_stamp
            && self.committed_connection_stamp != Some(expected_pending.candidate_connection_stamp)
        {
            return Err(ProtocolError::new(
                ErrorCode::StaleGeneration,
                "host reconciliation reported an unrelated generation",
                true,
            ));
        }
        Ok(self.committed_connection_stamp)
    }
}

fn validate_bootstrap_handover(
    protocol_version: u16,
    request_id: &str,
    execution_target_id: &str,
    controller_id: &str,
    pairing_fence: PairingFence,
    connection_stamp: ConnectionStamp,
    previous_connection_stamp: Option<ConnectionStamp>,
    expected_request_id: &str,
    expected_target: &str,
    expected_controller: iroh::EndpointId,
    expected_fence: PairingFence,
    expected_stamp: ConnectionStamp,
    expected_previous: Option<ConnectionStamp>,
    phase: &str,
) -> Result<(), ProtocolError> {
    validate_bootstrap_id("request_id", request_id)?;
    validate_bootstrap_id("execution_target_id", execution_target_id)?;
    validate_bootstrap_pairing_fence(pairing_fence, expected_fence)?;
    connection_stamp.validate()?;
    if let Some(previous) = previous_connection_stamp {
        previous.validate()?;
    }
    if protocol_version != PROTOCOL_VERSION
        || request_id != expected_request_id
        || execution_target_id != expected_target
        || controller_id != expected_controller.to_string()
        || connection_stamp != expected_stamp
        || previous_connection_stamp != expected_previous
    {
        return Err(ProtocolError::new(
            ErrorCode::StaleGeneration,
            format!("bootstrap {phase} correlation failed"),
            true,
        ));
    }
    Ok(())
}

macro_rules! impl_bootstrap_handover_validation {
    ($type:ty, $phase:literal) => {
        impl $type {
            fn validate(
                &self,
                expected_request_id: &str,
                expected_target: &str,
                expected_controller: iroh::EndpointId,
                expected_fence: PairingFence,
                expected_stamp: ConnectionStamp,
                expected_previous: Option<ConnectionStamp>,
            ) -> Result<(), ProtocolError> {
                validate_bootstrap_handover(
                    self.protocol_version,
                    &self.request_id,
                    &self.execution_target_id,
                    &self.controller_id,
                    self.pairing_fence,
                    self.connection_stamp,
                    self.previous_connection_stamp,
                    expected_request_id,
                    expected_target,
                    expected_controller,
                    expected_fence,
                    expected_stamp,
                    expected_previous,
                    $phase,
                )
            }
        }
    };
}

impl_bootstrap_handover_validation!(BootstrapInstalled, "installed acknowledgment");
impl_bootstrap_handover_validation!(BootstrapCommitted, "commit marker");
impl_bootstrap_handover_validation!(BootstrapCommitObserved, "commit observation");
impl_bootstrap_handover_validation!(BootstrapFinalized, "finalization marker");

impl BootstrapResponse {
    fn validate(
        &self,
        expected_request_id: &str,
        expected_target: &str,
        expected_fence: PairingFence,
    ) -> Result<ConnectionStamp, ProtocolError> {
        if self.protocol_version != PROTOCOL_VERSION {
            return Err(ProtocolError::new(
                ErrorCode::UnsupportedVersion,
                "bootstrap response version is unsupported",
                false,
            ));
        }
        validate_bootstrap_id("request_id", &self.request_id)?;
        validate_bootstrap_id("execution_target_id", &self.execution_target_id)?;
        validate_bootstrap_pairing_fence(self.pairing_fence, expected_fence)?;
        if self.request_id != expected_request_id || self.execution_target_id != expected_target {
            return Err(ProtocolError::new(
                ErrorCode::WrongEndpoint,
                "bootstrap response correlation failed",
                false,
            ));
        }
        match &self.result {
            Ok(accepted) => {
                accepted.connection_stamp.validate()?;
                Ok(accepted.connection_stamp)
            }
            Err(error) => {
                error.validate()?;
                Err(error.clone())
            }
        }
    }
}

fn validate_bootstrap_pairing_fence(
    actual: PairingFence,
    expected: PairingFence,
) -> Result<(), ProtocolError> {
    actual.validate()?;
    expected.validate()?;
    if actual != expected {
        return Err(ProtocolError::new(
            ErrorCode::Unauthorized,
            "bootstrap pairing fence does not match current authorization",
            false,
        ));
    }
    Ok(())
}

fn validate_bootstrap_id(field: &str, value: &str) -> Result<(), ProtocolError> {
    if !value.is_empty()
        && value.len() <= crate::remote_protocol::MAX_ID_BYTES
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.' | b':'))
    {
        Ok(())
    } else {
        Err(ProtocolError::new(
            ErrorCode::InvalidFrame,
            format!("invalid bootstrap {field}"),
            false,
        ))
    }
}

pub struct MapleIrohEndpoint {
    endpoint: iroh::Endpoint,
    admission: PeerAdmission,
    connection_policy: ConnectionPolicy,
    relay_policy: RelayPolicy,
    execution_target_id: Arc<str>,
    accepted_connections: Arc<AcceptedPeerQueue>,
    shutdown: Mutex<Option<oneshot::Sender<()>>>,
}

/// Exclusive, retryable authority to move one host's committed connection
/// lineage to a rebuilt endpoint.
///
/// The capability owns both the fenced source endpoint and its non-Clone
/// lineage snapshot. Async close or bind cancellation cannot destroy the
/// snapshot because restore attempts borrow this value mutably; a successful
/// endpoint/pump installation consumes it synchronously.
#[must_use = "a lineage handoff must be restored or deliberately dropped"]
pub struct EndpointLineageHandoff {
    source: Option<MapleIrohEndpoint>,
    snapshot: Option<EndpointLineageSnapshot>,
    authorization_floor_revision: u64,
    authorization_floor_digest: [u8; 32],
    source_closed: bool,
    consumed: bool,
    #[cfg(test)]
    close_gate: Option<Arc<tokio::sync::Notify>>,
    #[cfg(test)]
    fail_next_bind: bool,
}

impl std::fmt::Debug for EndpointLineageHandoff {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("EndpointLineageHandoff")
            .field("source_closed", &self.source_closed)
            .field("consumed", &self.consumed)
            .finish_non_exhaustive()
    }
}

impl EndpointLineageHandoff {
    /// Monotonically record the newest independently supplied authorization
    /// truth before any cancellable close/bind work. A failed attempt may be
    /// retried with the exact same snapshot or a newer one, but can never roll
    /// this handoff back or fork one revision into different peer maps.
    fn authorize_restore_attempt(
        &mut self,
        authorization: &AuthorizationSnapshot,
    ) -> Result<(), ProtocolError> {
        let expected_account = self.snapshot()?.account_epoch;
        if authorization.account_epoch == 0
            || authorization.snapshot_revision == 0
            || authorization.incoming_controllers.len() > MAX_AUTHORIZED_PEERS_PER_DIRECTION
            || authorization.outgoing_execution_targets.len() > MAX_AUTHORIZED_PEERS_PER_DIRECTION
        {
            return Err(ProtocolError::new(
                ErrorCode::Unauthorized,
                "authorization snapshot is invalid for lineage restore",
                false,
            ));
        }
        if authorization.account_epoch < expected_account {
            return Err(stale_authorization_snapshot());
        }
        if authorization.account_epoch > expected_account {
            // A newer local account context is authoritative. Permanently
            // destroy this old-account reconstruction capability before any
            // cancellable close/bind work so a failed attempt cannot retry an
            // older account snapshot and resurrect its authorization.
            self.snapshot.take();
            self.source.take();
            self.source_closed = true;
            self.consumed = true;
            return Err(ProtocolError::new(
                ErrorCode::Unauthorized,
                "host lineage was invalidated by a newer account authorization context",
                false,
            ));
        }
        if authorization.snapshot_revision < self.authorization_floor_revision {
            return Err(stale_authorization_snapshot());
        }
        let digest = digest_authorization_snapshot(authorization);
        if authorization.snapshot_revision == self.authorization_floor_revision {
            if digest != self.authorization_floor_digest {
                return Err(stale_authorization_snapshot());
            }
        } else {
            self.authorization_floor_revision = authorization.snapshot_revision;
            self.authorization_floor_digest = digest;
        }
        Ok(())
    }

    async fn ensure_source_closed(&mut self) -> Result<(), ProtocolError> {
        if self.consumed {
            return Err(ProtocolError::new(
                ErrorCode::StaleGeneration,
                "host lineage handoff was already consumed",
                false,
            ));
        }
        if !self.source_closed {
            #[cfg(test)]
            if let Some(gate) = &self.close_gate {
                gate.notified().await;
            }
            let source = self.source.as_ref().ok_or_else(internal_state_error)?;
            source.endpoint.close().await;
            self.source_closed = true;
        }
        Ok(())
    }

    fn snapshot(&self) -> Result<&EndpointLineageSnapshot, ProtocolError> {
        if self.consumed {
            return Err(ProtocolError::new(
                ErrorCode::StaleGeneration,
                "host lineage handoff was already consumed",
                false,
            ));
        }
        self.snapshot.as_ref().ok_or_else(internal_state_error)
    }

    fn consume_after_commit(&mut self) {
        debug_assert!(self.source_closed);
        self.snapshot.take();
        self.source.take();
        self.consumed = true;
    }

    #[cfg(test)]
    fn is_consumed(&self) -> bool {
        self.consumed
    }

    #[cfg(test)]
    fn gate_source_close(&mut self, gate: Arc<tokio::sync::Notify>) {
        self.close_gate = Some(gate);
    }

    #[cfg(test)]
    fn ungate_source_close(&mut self) {
        self.close_gate = None;
    }

    #[cfg(test)]
    fn fail_next_bind(&mut self) {
        self.fail_next_bind = true;
    }

    #[cfg(test)]
    fn take_fail_next_bind(&mut self) -> bool {
        std::mem::take(&mut self.fail_next_bind)
    }

    #[cfg(test)]
    fn from_snapshot_fixture(snapshot: EndpointLineageSnapshot) -> Self {
        let authorization_floor_revision = snapshot.snapshot_revision;
        let authorization_floor_digest = snapshot.authorization_digest;
        Self {
            source: None,
            snapshot: Some(snapshot),
            authorization_floor_revision,
            authorization_floor_digest,
            source_closed: true,
            consumed: false,
            close_gate: None,
            fail_next_bind: false,
        }
    }
}

/// A host-side QUIC generation that has completed bootstrap transport auth but
/// is not yet application-routable. Delaying `ConnectedPeer::new` also delays
/// the application stream dispatcher, so B cannot prefill command queues while
/// A is still the committed generation.
struct PendingConnectedPeer {
    connection: iroh::endpoint::Connection,
    connection_stamp: ConnectionStamp,
    pairing_fence: PairingFence,
    execution_target_id: Arc<str>,
    outbound_direction: PeerDirection,
    frame_deadline: Duration,
}

impl PendingConnectedPeer {
    fn remote_id(&self) -> iroh::EndpointId {
        self.connection.remote_id()
    }

    fn connection_stamp(&self) -> ConnectionStamp {
        self.connection_stamp
    }

    fn finalize(self) -> ConnectedPeer {
        ConnectedPeer::new(
            self.connection,
            self.connection_stamp,
            self.pairing_fence,
            self.execution_target_id,
            self.outbound_direction,
            self.frame_deadline,
        )
    }

    fn close(&self) {
        self.connection.close(
            iroh::endpoint::VarInt::from_u32(0),
            b"Maple pending connection generation stopped",
        );
    }
}

impl std::fmt::Debug for PendingConnectedPeer {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("PendingConnectedPeer")
            .field("remote_id", &self.remote_id())
            .field("connection_stamp", &self.connection_stamp)
            .field("pairing_fence", &self.pairing_fence)
            .field("execution_target_id", &self.execution_target_id)
            .field("outbound_direction", &self.outbound_direction)
            .finish_non_exhaustive()
    }
}

#[derive(Debug, Default)]
struct AcceptedPeerQueue {
    state: Mutex<AcceptedPeerQueueState>,
    ready: Notify,
}

#[derive(Debug, Default)]
struct AcceptedPeerQueueState {
    /// At most one queued generation per controller. Superseded same-peer
    /// generations replace rather than consume global queue capacity.
    latest: HashMap<iroh::EndpointId, ConnectedPeer>,
    /// One non-routable handover candidate per controller. Staging B never
    /// replaces, removes, or gates queued A.
    pending: HashMap<iroh::EndpointId, PendingConnectedPeer>,
    ready_order: VecDeque<iroh::EndpointId>,
    reservations: HashMap<iroh::EndpointId, usize>,
    closed: bool,
}

struct AcceptedPeerReservation {
    queue: Arc<AcceptedPeerQueue>,
    peer: iroh::EndpointId,
    active: bool,
}

impl AcceptedPeerQueue {
    fn reserve(
        self: &Arc<Self>,
        peer: iroh::EndpointId,
    ) -> Result<AcceptedPeerReservation, ProtocolError> {
        let mut state = self.state.lock().map_err(|_| internal_state_error())?;
        if state.closed {
            return Err(ProtocolError::new(
                ErrorCode::TransportUnavailable,
                "Maple authenticated-controller queue is closed",
                true,
            ));
        }
        let already_counted = state.latest.contains_key(&peer)
            || state.pending.contains_key(&peer)
            || state.reservations.contains_key(&peer);
        let mut unique = state.latest.keys().copied().collect::<HashSet<_>>();
        unique.extend(state.pending.keys().copied());
        unique.extend(state.reservations.keys().copied());
        let unique_count = unique.len();
        if !already_counted && unique_count >= MAX_ACCEPTED_CONNECTION_QUEUE {
            return Err(ProtocolError::new(
                ErrorCode::TransportUnavailable,
                "Maple authenticated-controller queue is full",
                true,
            ));
        }
        *state.reservations.entry(peer).or_insert(0) += 1;
        Ok(AcceptedPeerReservation {
            queue: self.clone(),
            peer,
            active: true,
        })
    }

    async fn recv(&self) -> Result<ConnectedPeer, ProtocolError> {
        loop {
            let notified = self.ready.notified();
            {
                let mut state = self.state.lock().map_err(|_| internal_state_error())?;
                let ready_count = state.ready_order.len();
                for _ in 0..ready_count {
                    let Some(peer) = state.ready_order.pop_front() else {
                        break;
                    };
                    if let Some(candidate) = state.latest.remove(&peer) {
                        if candidate.connection.close_reason().is_none() {
                            return Ok(candidate);
                        }
                    }
                }
                if state.closed {
                    return Err(ProtocolError::new(
                        ErrorCode::TransportUnavailable,
                        "Maple authenticated-controller queue is closed",
                        true,
                    ));
                }
            }
            notified.await;
        }
    }

    fn close(&self) {
        let candidates = match self.state.lock() {
            Ok(mut state) => {
                state.closed = true;
                state.ready_order.clear();
                state.reservations.clear();
                let candidates = state
                    .latest
                    .drain()
                    .map(|(_, peer)| peer)
                    .collect::<Vec<_>>();
                for (_, pending) in state.pending.drain() {
                    pending.close();
                }
                candidates
            }
            Err(_) => Vec::new(),
        };
        for candidate in candidates {
            candidate.close();
        }
        self.ready.notify_waiters();
    }
}

impl AcceptedPeerReservation {
    fn publish(mut self, candidate: PendingConnectedPeer) -> Result<(), ProtocolError> {
        if candidate.remote_id() != self.peer {
            return Err(ProtocolError::new(
                ErrorCode::WrongEndpoint,
                "accepted queue reservation belongs to another controller",
                false,
            ));
        }
        {
            let mut state = self
                .queue
                .state
                .lock()
                .map_err(|_| internal_state_error())?;
            if state.closed || candidate.connection.close_reason().is_some() {
                return Err(ProtocolError::new(
                    ErrorCode::TransportUnavailable,
                    "Maple authenticated-controller candidate is no longer usable",
                    true,
                ));
            }
            release_queue_reservation(&mut state, self.peer);
            self.active = false;
            if state.pending.contains_key(&self.peer)
                || state
                    .latest
                    .get(&self.peer)
                    .is_some_and(|queued| queued.connection_stamp() >= candidate.connection_stamp())
            {
                return Err(ProtocolError::new(
                    ErrorCode::StaleGeneration,
                    "accepted controller queue already contains a newer generation",
                    true,
                ));
            }
            state.pending.insert(self.peer, candidate);
        }
        Ok(())
    }
}

impl AcceptedPeerQueue {
    fn finalize_candidate(
        &self,
        peer: iroh::EndpointId,
        stamp: ConnectionStamp,
    ) -> Result<Option<ConnectedPeer>, ProtocolError> {
        let mut state = self.state.lock().map_err(|_| internal_state_error())?;
        if state.closed
            || !state
                .pending
                .get(&peer)
                .is_some_and(|candidate| candidate.connection_stamp() == stamp)
        {
            return Err(ProtocolError::new(
                ErrorCode::StaleGeneration,
                "authenticated-controller candidate changed before finalization",
                true,
            ));
        }
        let candidate = state
            .pending
            .remove(&peer)
            .expect("validated pending controller candidate")
            .finalize();
        let displaced = state.latest.insert(peer, candidate);
        if !state.ready_order.contains(&peer) {
            state.ready_order.push_back(peer);
        }
        self.ready.notify_one();
        Ok(displaced)
    }

    fn rollback_candidate(
        &self,
        peer: iroh::EndpointId,
        stamp: ConnectionStamp,
    ) -> Result<(), ProtocolError> {
        let candidate = {
            let mut state = self.state.lock().map_err(|_| internal_state_error())?;
            if state
                .pending
                .get(&peer)
                .is_some_and(|candidate| candidate.connection_stamp() == stamp)
            {
                state.pending.remove(&peer)
            } else {
                None
            }
        };
        if let Some(candidate) = candidate {
            candidate.close();
        }
        Ok(())
    }
}

fn rollback_staged_incoming(
    admission: &PeerAdmission,
    accepted_connections: &AcceptedPeerQueue,
    commit: &mut IncomingCommit,
) -> Result<(), ProtocolError> {
    // Keep the admission activation token while restoring the keyed queue, so
    // a racing generation cannot publish between these two state machines.
    accepted_connections.rollback_candidate(commit.peer, commit.stamp)?;
    admission.rollback_incoming(commit)
}

impl Drop for AcceptedPeerReservation {
    fn drop(&mut self) {
        if !self.active {
            return;
        }
        if let Ok(mut state) = self.queue.state.lock() {
            release_queue_reservation(&mut state, self.peer);
        }
    }
}

fn release_queue_reservation(state: &mut AcceptedPeerQueueState, peer: iroh::EndpointId) {
    if let Some(count) = state.reservations.get_mut(&peer) {
        *count -= 1;
        if *count == 0 {
            state.reservations.remove(&peer);
        }
    }
}

impl std::fmt::Debug for MapleIrohEndpoint {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("MapleIrohEndpoint")
            .field("endpoint_id", &self.endpoint.id())
            .field("execution_target_id", &self.execution_target_id)
            .field("relay_policy", &self.relay_policy)
            .finish_non_exhaustive()
    }
}

/// Transport-handle-free controller lineage for reconstructing a generation
/// manager after a runtime or endpoint rebuild.
///
/// This is intentionally an in-memory typed handoff rather than a persistence
/// format. In particular, an ambiguous post-Observed handover carries the
/// exact [`PendingCommit`] that must be reconciled before a normal dial.
#[derive(Debug, PartialEq, Eq)]
pub struct GenerationLineageSnapshot {
    expected_controller: Option<iroh::EndpointId>,
    expected_remote: iroh::EndpointId,
    expected_execution_target_id: Arc<str>,
    expected_direction: PeerDirection,
    pairing_fence: PairingFence,
    minimum_accepted: Option<ConnectionStamp>,
    last_committed: Option<ConnectionStamp>,
    pending_reconciliation: Option<PendingCommit>,
}

impl GenerationLineageSnapshot {
    pub fn expected_remote(&self) -> iroh::EndpointId {
        self.expected_remote
    }

    pub fn execution_target_id(&self) -> &str {
        &self.expected_execution_target_id
    }

    pub fn pairing_fence(&self) -> PairingFence {
        self.pairing_fence
    }

    pub fn replay_floor(&self) -> Option<ConnectionStamp> {
        self.minimum_accepted
    }

    pub fn last_committed(&self) -> Option<ConnectionStamp> {
        self.last_committed
    }

    pub fn requires_reconciliation(&self) -> bool {
        self.pending_reconciliation.is_some()
    }
}

fn validate_generation_lineage(
    expected_controller: Option<iroh::EndpointId>,
    expected_remote: iroh::EndpointId,
    expected_execution_target_id: &str,
    expected_direction: PeerDirection,
    pairing_fence: PairingFence,
    minimum_accepted: Option<ConnectionStamp>,
    last_committed: Option<ConnectionStamp>,
    pending_reconciliation: Option<&PendingCommit>,
) -> Result<(), ProtocolError> {
    validate_bootstrap_id("execution_target_id", expected_execution_target_id)?;
    pairing_fence.validate()?;
    if expected_direction != PeerDirection::ControllerToHost {
        return Err(ProtocolError::new(
            ErrorCode::WrongDirection,
            "controller lineage has the wrong connection direction",
            false,
        ));
    }
    if let Some(minimum) = minimum_accepted {
        minimum.validate()?;
    }
    if let Some(committed) = last_committed {
        committed.validate()?;
        if minimum_accepted != Some(committed) {
            return Err(ProtocolError::new(
                ErrorCode::StaleGeneration,
                "controller replay floor does not match committed lineage",
                false,
            ));
        }
    }
    if let Some(pending) = pending_reconciliation {
        let controller = expected_controller.ok_or_else(|| {
            ProtocolError::new(
                ErrorCode::Unauthorized,
                "pending reconciliation requires an explicit controller binding",
                false,
            )
        })?;
        pending.validate(
            expected_execution_target_id,
            controller,
            expected_remote,
            pairing_fence,
        )?;
        if pending.previous_connection_stamp != last_committed
            || minimum_accepted.is_some_and(|minimum| pending.candidate_connection_stamp <= minimum)
        {
            return Err(ProtocolError::new(
                ErrorCode::StaleGeneration,
                "pending reconciliation does not advance controller lineage",
                false,
            ));
        }
    }
    Ok(())
}

/// Owns exactly one current connection for a peer. Reconnect attempts may run
/// concurrently; the first successful result for a newer generation becomes
/// current, and every stale/losing connection is closed immediately.
#[derive(Debug)]
pub struct GenerationConnectionManager {
    expected_controller: Option<iroh::EndpointId>,
    expected_remote: iroh::EndpointId,
    expected_execution_target_id: Arc<str>,
    expected_direction: PeerDirection,
    pairing_fence: PairingFence,
    state: Mutex<GenerationManagerState>,
}

#[derive(Debug)]
struct GenerationManagerState {
    minimum_accepted: Option<ConnectionStamp>,
    /// The last generation for which this controller received a correlated
    /// host Finalized marker. This is protocol lineage, not a liveness claim,
    /// and therefore survives pruning a closed `current` handle.
    last_committed: Option<ConnectionStamp>,
    current: Option<ConnectedPeer>,
    handover: Option<ManagedHandover>,
}

#[derive(Debug)]
enum ManagedHandover {
    Prepared {
        token: HandoverToken,
        candidate: ConnectedPeer,
    },
    Promoted {
        token: HandoverToken,
        fallback: Option<ConnectedPeer>,
    },
    AwaitingFinalized {
        token: HandoverToken,
        fallback: Option<ConnectedPeer>,
    },
    /// Reconstructed post-Observed ambiguity. No transport handle is trusted;
    /// the exact pending transition must be reconciled with the host first.
    ReconciliationRequired { pending: PendingCommit },
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct HandoverToken {
    previous_stamp: Option<ConnectionStamp>,
    candidate_stamp: ConnectionStamp,
    candidate_id: usize,
    pending: PendingCommit,
}

/// A reconnect candidate which has been validated but is not yet primary.
///
/// Dropping this guard closes the candidate and leaves the previous connection
/// untouched. This makes cancellation before the host commit marker safe.
#[derive(Debug)]
pub struct PreparedHandover<'a> {
    manager: &'a GenerationConnectionManager,
    token: Option<HandoverToken>,
}

/// A reconnect candidate which is primary while the previous live connection
/// is retained as a rollback fallback.
///
/// Dropping this guard restores the fallback when it remains live. Once the
/// commit-observed marker may have reached the host, `mark_observed_sent`
/// consumes this guard into [`AwaitingFinalizedHandover`] instead.
#[derive(Debug)]
pub struct PromotedHandover<'a> {
    manager: &'a GenerationConnectionManager,
    token: Option<HandoverToken>,
}

/// A handover whose commit-observed marker may have reached the host.
///
/// This state is intentionally not resolved from connection liveness. Only a
/// correlated host Finalized marker may advance logical lineage to B; dropping
/// the guard leaves the manager blocked in this ambiguous state until `clear`.
#[derive(Debug)]
pub struct AwaitingFinalizedHandover<'a> {
    manager: &'a GenerationConnectionManager,
    token: Option<HandoverToken>,
}

impl GenerationConnectionManager {
    pub fn new(
        expected_remote: iroh::EndpointId,
        expected_execution_target_id: impl Into<Arc<str>>,
        minimum_accepted: Option<ConnectionStamp>,
    ) -> Result<Self, ProtocolError> {
        Self::new_for_direction(
            None,
            expected_remote,
            expected_execution_target_id,
            PeerDirection::ControllerToHost,
            PairingFence::new(PairingIncarnation::new(1)?)?,
            minimum_accepted,
        )
    }

    pub fn new_for_pairing(
        expected_controller: iroh::EndpointId,
        expected_remote: iroh::EndpointId,
        expected_execution_target_id: impl Into<Arc<str>>,
        pairing_fence: PairingFence,
        minimum_accepted: Option<ConnectionStamp>,
    ) -> Result<Self, ProtocolError> {
        Self::new_for_direction(
            Some(expected_controller),
            expected_remote,
            expected_execution_target_id,
            PeerDirection::ControllerToHost,
            pairing_fence,
            minimum_accepted,
        )
    }

    fn new_for_direction(
        expected_controller: Option<iroh::EndpointId>,
        expected_remote: iroh::EndpointId,
        expected_execution_target_id: impl Into<Arc<str>>,
        expected_direction: PeerDirection,
        pairing_fence: PairingFence,
        minimum_accepted: Option<ConnectionStamp>,
    ) -> Result<Self, ProtocolError> {
        let expected_execution_target_id = expected_execution_target_id.into();
        validate_bootstrap_id("execution_target_id", &expected_execution_target_id)?;
        if let Some(stamp) = minimum_accepted {
            stamp.validate()?;
        }
        pairing_fence.validate()?;
        Ok(Self {
            expected_controller,
            expected_remote,
            expected_execution_target_id,
            expected_direction,
            pairing_fence,
            state: Mutex::new(GenerationManagerState {
                minimum_accepted,
                last_committed: None,
                current: None,
                handover: None,
            }),
        })
    }

    /// Capture controller lineage without exporting a live QUIC handle.
    /// Prepared or merely promoted candidates are cancellable local state and
    /// therefore cannot cross a reconstruction boundary. Post-Observed
    /// ambiguity is retained as an exact reconciliation obligation.
    pub fn capture_lineage(self) -> Result<GenerationLineageSnapshot, ProtocolError> {
        let mut state = self.state.lock().map_err(|_| internal_state_error())?;
        Self::prune_closed_current(&mut state);
        let pending_reconciliation = match state.handover.as_ref() {
            None => None,
            Some(ManagedHandover::AwaitingFinalized { token, .. }) => Some(token.pending.clone()),
            Some(ManagedHandover::ReconciliationRequired { pending }) => Some(pending.clone()),
            Some(ManagedHandover::Prepared { .. } | ManagedHandover::Promoted { .. }) => {
                return Err(ProtocolError::new(
                    ErrorCode::TransportUnavailable,
                    "controller lineage cannot be captured before commit observation",
                    true,
                ));
            }
        };
        if state.handover.is_none()
            && state.current.as_ref().is_some_and(|current| {
                Some(current.connection_stamp()) != state.last_committed
                    || self.validate_candidate(current).is_err()
            })
        {
            return Err(ProtocolError::new(
                ErrorCode::Internal,
                "controller manager current handle does not match committed lineage",
                false,
            ));
        }
        validate_generation_lineage(
            self.expected_controller,
            self.expected_remote,
            &self.expected_execution_target_id,
            self.expected_direction,
            self.pairing_fence,
            state.minimum_accepted,
            state.last_committed,
            pending_reconciliation.as_ref(),
        )?;
        let snapshot = GenerationLineageSnapshot {
            expected_controller: self.expected_controller,
            expected_remote: self.expected_remote,
            expected_execution_target_id: self.expected_execution_target_id.clone(),
            expected_direction: self.expected_direction,
            pairing_fence: self.pairing_fence,
            minimum_accepted: state.minimum_accepted,
            last_committed: state.last_committed,
            pending_reconciliation,
        };
        drop(state);
        // This is an ownership transfer, not a clone. Close every transport
        // handle and fence the consumed manager so two coordinators cannot
        // advance the same logical lineage concurrently.
        self.clear()?;
        Ok(snapshot)
    }

    /// Reconstruct a controller manager under independently supplied current
    /// pairing truth. Every identity, target, direction, account, incarnation,
    /// and stamp invariant must match the captured lineage exactly. The new
    /// manager intentionally starts without a live transport handle.
    pub fn restore_for_pairing(
        snapshot: GenerationLineageSnapshot,
        expected_controller: iroh::EndpointId,
        expected_remote: iroh::EndpointId,
        expected_execution_target_id: impl Into<Arc<str>>,
        pairing_fence: PairingFence,
    ) -> Result<Self, ProtocolError> {
        let expected_execution_target_id = expected_execution_target_id.into();
        validate_bootstrap_id("execution_target_id", &expected_execution_target_id)?;
        pairing_fence.validate()?;
        if snapshot.expected_controller != Some(expected_controller)
            || snapshot.expected_remote != expected_remote
            || snapshot.expected_execution_target_id != expected_execution_target_id
            || snapshot.expected_direction != PeerDirection::ControllerToHost
            || snapshot.pairing_fence != pairing_fence
        {
            return Err(ProtocolError::new(
                ErrorCode::Unauthorized,
                "controller lineage does not match the current pairing",
                false,
            ));
        }
        validate_generation_lineage(
            Some(expected_controller),
            expected_remote,
            &expected_execution_target_id,
            PeerDirection::ControllerToHost,
            pairing_fence,
            snapshot.minimum_accepted,
            snapshot.last_committed,
            snapshot.pending_reconciliation.as_ref(),
        )?;
        let handover = snapshot
            .pending_reconciliation
            .map(|pending| ManagedHandover::ReconciliationRequired { pending });
        Ok(Self {
            expected_controller: Some(expected_controller),
            expected_remote,
            expected_execution_target_id,
            expected_direction: PeerDirection::ControllerToHost,
            pairing_fence,
            state: Mutex::new(GenerationManagerState {
                minimum_accepted: snapshot.minimum_accepted,
                last_committed: snapshot.last_committed,
                current: None,
                handover,
            }),
        })
    }

    fn current_stamp(&self) -> Result<Option<ConnectionStamp>, ProtocolError> {
        let mut state = self.state.lock().map_err(|_| internal_state_error())?;
        Self::prune_closed_current(&mut state);
        Ok(state.last_committed)
    }

    fn pending_reconciliation(&self) -> Result<Option<PendingCommit>, ProtocolError> {
        let state = self.state.lock().map_err(|_| internal_state_error())?;
        Ok(match state.handover.as_ref() {
            Some(ManagedHandover::AwaitingFinalized { token, .. }) => Some(token.pending.clone()),
            Some(ManagedHandover::ReconciliationRequired { pending }) => Some(pending.clone()),
            _ => None,
        })
    }

    fn apply_reconciliation(
        &self,
        pending: &PendingCommit,
        committed: Option<ConnectionStamp>,
    ) -> Result<(), ProtocolError> {
        let mut state = self.state.lock().map_err(|_| internal_state_error())?;
        let matches = match state.handover.as_ref() {
            Some(ManagedHandover::AwaitingFinalized { token, .. }) => &token.pending == pending,
            Some(ManagedHandover::ReconciliationRequired { pending: stored }) => stored == pending,
            _ => false,
        };
        if !matches {
            if state.handover.is_none()
                && state.last_committed == Some(pending.candidate_connection_stamp)
                && committed == Some(pending.candidate_connection_stamp)
            {
                return Ok(());
            }
            return Err(ProtocolError::new(
                ErrorCode::StaleGeneration,
                "reconciliation no longer matches the pending handover",
                true,
            ));
        }
        if committed == Some(pending.candidate_connection_stamp) {
            let handover = state
                .handover
                .take()
                .expect("validated pending reconciliation");
            if let ManagedHandover::AwaitingFinalized { fallback, .. } = handover {
                if let Some(fallback) = fallback {
                    fallback.close();
                }
            }
            if let Some(candidate) = state.current.take() {
                if candidate.connection_stamp() == pending.candidate_connection_stamp
                    && candidate.connection.close_reason().is_none()
                {
                    state.current = Some(candidate);
                } else {
                    candidate.close();
                }
            }
            // The authenticated host decision is definitive even without a
            // live B handle. Apply it under this single manager lock so clear
            // or another reader cannot interleave between selection and commit.
            state.last_committed = Some(pending.candidate_connection_stamp);
            state.minimum_accepted = Some(pending.candidate_connection_stamp);
            return Ok(());
        }
        if committed != pending.previous_connection_stamp {
            return Err(ProtocolError::new(
                ErrorCode::StaleGeneration,
                "reconciliation returned an unrelated lineage",
                true,
            ));
        }
        match state.handover.take() {
            Some(ManagedHandover::AwaitingFinalized { fallback, .. }) => {
                if let Some(candidate) = state.current.take() {
                    candidate.close();
                }
                if let Some(fallback) = fallback {
                    if fallback.connection.close_reason().is_none() {
                        state.current = Some(fallback);
                    } else {
                        fallback.close();
                    }
                }
            }
            Some(ManagedHandover::ReconciliationRequired { .. }) => {}
            _ => unreachable!("validated pending reconciliation"),
        }
        // A remains the logical predecessor even if its transport handle is
        // gone. Never advance the replay floor to the uncommitted candidate.
        state.last_committed = pending.previous_connection_stamp;
        Ok(())
    }

    pub fn begin_handover(
        &self,
        candidate: ConnectedPeer,
        pending: PendingCommit,
    ) -> Result<PreparedHandover<'_>, ProtocolError> {
        if let Err(error) = self.validate_candidate(&candidate) {
            candidate.close();
            return Err(error);
        }
        let mut state = match self.state.lock() {
            Ok(state) => state,
            Err(_) => {
                candidate.close();
                return Err(internal_state_error());
            }
        };
        if candidate.connection.close_reason().is_some() {
            return Err(ProtocolError::new(
                ErrorCode::TransportUnavailable,
                "reconnect candidate closed before staging",
                true,
            ));
        }
        Self::prune_closed_current(&mut state);
        let actual_previous = state.last_committed;
        let expected_controller = self.expected_controller.ok_or_else(|| {
            ProtocolError::new(
                ErrorCode::Unauthorized,
                "managed handover requires an explicit local controller binding",
                false,
            )
        })?;
        if let Err(error) = pending.validate(
            &self.expected_execution_target_id,
            expected_controller,
            self.expected_remote,
            self.pairing_fence,
        ) {
            candidate.close();
            return Err(error);
        }
        if pending.candidate_connection_stamp != candidate.connection_stamp()
            || actual_previous != pending.previous_connection_stamp
            || state.handover.is_some()
        {
            candidate.close();
            return Err(ProtocolError::new(
                ErrorCode::StaleGeneration,
                "connection manager changed during handover",
                true,
            ));
        }
        if state
            .minimum_accepted
            .is_some_and(|minimum| candidate.connection_stamp() <= minimum)
            || actual_previous.is_some_and(|current| current >= candidate.connection_stamp())
        {
            candidate.close();
            return Err(ProtocolError::new(
                ErrorCode::StaleGeneration,
                "handover candidate does not advance the host generation",
                true,
            ));
        }
        let token = HandoverToken {
            previous_stamp: actual_previous,
            candidate_stamp: candidate.connection_stamp(),
            candidate_id: candidate.connection.stable_id(),
            pending,
        };
        state.handover = Some(ManagedHandover::Prepared {
            token: token.clone(),
            candidate,
        });
        Ok(PreparedHandover {
            manager: self,
            token: Some(token),
        })
    }

    fn promote_handover(&self, token: &HandoverToken) -> Result<(), ProtocolError> {
        let mut state = self.state.lock().map_err(|_| internal_state_error())?;
        if state.last_committed != token.previous_stamp {
            return Err(ProtocolError::new(
                ErrorCode::StaleGeneration,
                "committed connection lineage changed before handover promotion",
                true,
            ));
        }
        let candidate_matches = matches!(
            state.handover.as_ref(),
            Some(ManagedHandover::Prepared {
                token: active_token,
                candidate,
            }) if active_token == token && candidate.connection.close_reason().is_none()
        );
        if !candidate_matches {
            return Err(ProtocolError::new(
                ErrorCode::TransportUnavailable,
                "prepared handover candidate is no longer usable",
                true,
            ));
        }
        let Some(ManagedHandover::Prepared { candidate, .. }) = state.handover.take() else {
            unreachable!("validated prepared handover");
        };
        let fallback = state.current.replace(candidate);
        state.handover = Some(ManagedHandover::Promoted {
            token: token.clone(),
            fallback,
        });
        Ok(())
    }

    fn mark_observed_sent(&self, token: &HandoverToken) -> Result<(), ProtocolError> {
        let mut state = self.state.lock().map_err(|_| internal_state_error())?;
        let matches = matches!(
            state.handover.as_ref(),
            Some(ManagedHandover::Promoted {
                token: active_token,
                ..
            }) if active_token == token
        ) && state.current.as_ref().is_some_and(|candidate| {
            candidate.connection_stamp() == token.candidate_stamp
                && candidate.connection.stable_id() == token.candidate_id
                && candidate.connection.close_reason().is_none()
        });
        if !matches {
            return Err(ProtocolError::new(
                ErrorCode::TransportUnavailable,
                "promoted handover candidate cannot acknowledge commit",
                true,
            ));
        }
        let Some(ManagedHandover::Promoted { fallback, .. }) = state.handover.take() else {
            unreachable!("validated promoted handover");
        };
        state.handover = Some(ManagedHandover::AwaitingFinalized {
            token: token.clone(),
            fallback,
        });
        Ok(())
    }

    fn rollback_prepared(&self, token: &HandoverToken) -> Result<(), ProtocolError> {
        let mut state = self.state.lock().map_err(|_| internal_state_error())?;
        let matches = matches!(
            state.handover.as_ref(),
            Some(ManagedHandover::Prepared {
                token: active_token,
                ..
            }) if active_token == token
        );
        if matches {
            if let Some(ManagedHandover::Prepared { candidate, .. }) = state.handover.take() {
                candidate.close();
            }
        }
        Ok(())
    }

    fn rollback_promoted(&self, token: &HandoverToken) -> Result<(), ProtocolError> {
        let mut state = self.state.lock().map_err(|_| internal_state_error())?;
        let matches = matches!(
            state.handover.as_ref(),
            Some(ManagedHandover::Promoted {
                token: active_token,
                ..
            }) if active_token == token
        );
        if !matches {
            return Ok(());
        }
        let Some(ManagedHandover::Promoted { fallback, .. }) = state.handover.take() else {
            unreachable!("validated promoted handover");
        };
        match fallback {
            Some(previous) if previous.connection.close_reason().is_none() => {
                if let Some(candidate) = state.current.replace(previous) {
                    candidate.close();
                }
            }
            fallback => {
                if let Some(fallback) = fallback {
                    fallback.close();
                }
                // Before the observed marker may have reached the host, B is
                // not committed lineage. If A cannot be restored, retain A's
                // logical stamp and reconnect from it rather than adopting B
                // based on transport liveness.
                if let Some(candidate) = state.current.take() {
                    candidate.close();
                }
            }
        }
        Ok(())
    }

    fn finalize_awaiting(&self, token: &HandoverToken) -> Result<ConnectedPeer, ProtocolError> {
        let mut state = self.state.lock().map_err(|_| internal_state_error())?;
        if state.last_committed == Some(token.candidate_stamp) && state.handover.is_none() {
            return match state.current.as_ref() {
                Some(candidate)
                    if candidate.connection_stamp() == token.candidate_stamp
                        && candidate.connection.stable_id() == token.candidate_id
                        && candidate.connection.close_reason().is_none() =>
                {
                    Ok(candidate.clone())
                }
                _ => Err(ProtocolError::new(
                    ErrorCode::TransportUnavailable,
                    "finalized handover lineage is committed without a live connection",
                    true,
                )),
            };
        }
        let handover_matches = matches!(
            state.handover.as_ref(),
            Some(ManagedHandover::AwaitingFinalized { token: active_token, .. })
                if active_token == token
        );
        let current_matches = state.current.as_ref().is_none_or(|candidate| {
            candidate.connection_stamp() == token.candidate_stamp
                && candidate.connection.stable_id() == token.candidate_id
        });
        if !handover_matches || !current_matches {
            return Err(ProtocolError::new(
                ErrorCode::StaleGeneration,
                "finalized marker does not match the awaiting handover",
                true,
            ));
        }
        let Some(ManagedHandover::AwaitingFinalized { fallback, .. }) = state.handover.take()
        else {
            unreachable!("validated awaiting-finalized handover");
        };
        if let Some(previous) = fallback {
            previous.close();
        }
        state.last_committed = Some(token.candidate_stamp);
        state.minimum_accepted = Some(token.candidate_stamp);
        match state.current.take() {
            Some(candidate) if candidate.connection.close_reason().is_none() => {
                state.current = Some(candidate.clone());
                Ok(candidate)
            }
            Some(candidate) => {
                candidate.close();
                Err(ProtocolError::new(
                    ErrorCode::TransportUnavailable,
                    "finalized handover committed a connection that is no longer live",
                    true,
                ))
            }
            None => Err(ProtocolError::new(
                ErrorCode::TransportUnavailable,
                "finalized handover has no live connection handle",
                true,
            )),
        }
    }

    fn prune_closed_current(state: &mut GenerationManagerState) {
        if state.handover.is_none()
            && state
                .current
                .as_ref()
                .is_some_and(|peer| peer.connection.close_reason().is_some())
        {
            if let Some(peer) = state.current.take() {
                peer.close();
            }
        }
    }

    fn validate_candidate(&self, candidate: &ConnectedPeer) -> Result<(), ProtocolError> {
        if candidate.remote_id() != self.expected_remote
            || candidate.execution_target_id() != self.expected_execution_target_id.as_ref()
            || candidate.outbound_direction != self.expected_direction
        {
            return Err(ProtocolError::new(
                ErrorCode::WrongEndpoint,
                "connection manager cannot migrate between execution targets",
                false,
            ));
        }
        Ok(())
    }

    pub fn current(&self) -> Result<Option<ConnectedPeer>, ProtocolError> {
        let mut state = self.state.lock().map_err(|_| internal_state_error())?;
        if state.handover.is_some() {
            return Err(ProtocolError::new(
                ErrorCode::TransportUnavailable,
                "connection handover has not reached a correlated finalized marker",
                true,
            ));
        }
        Self::prune_closed_current(&mut state);
        Ok(state.current.clone())
    }

    pub fn install_first(&self, candidate: ConnectedPeer) -> Result<ConnectedPeer, ProtocolError> {
        if let Err(error) = self.validate_candidate(&candidate) {
            candidate.close();
            return Err(error);
        }
        if candidate.connection.close_reason().is_some() {
            return Err(ProtocolError::new(
                ErrorCode::TransportUnavailable,
                "completed reconnect candidate is already closed",
                true,
            ));
        }
        let mut state = match self.state.lock() {
            Ok(state) => state,
            Err(_) => {
                candidate.close();
                return Err(internal_state_error());
            }
        };
        Self::prune_closed_current(&mut state);
        if state.handover.is_some() || state.last_committed.is_some() {
            candidate.close();
            return Err(ProtocolError::new(
                ErrorCode::StaleGeneration,
                "install_first is only valid before a connection lineage is committed",
                true,
            ));
        }
        // The candidate may have closed while waiting for another racing
        // installer to release the manager lock. Never let a dead, newer stamp
        // displace a still-usable generation.
        if candidate.connection.close_reason().is_some() {
            return Err(ProtocolError::new(
                ErrorCode::TransportUnavailable,
                "completed reconnect candidate closed before installation",
                true,
            ));
        }
        if state
            .minimum_accepted
            .is_some_and(|minimum| candidate.connection_stamp() <= minimum)
        {
            candidate.close();
            return Err(ProtocolError::new(
                ErrorCode::StaleGeneration,
                "connection stamp does not advance the persisted host floor",
                true,
            ));
        }
        if state.current.is_some() {
            candidate.close();
            return Err(ProtocolError::new(
                ErrorCode::Internal,
                "uncommitted connection manager unexpectedly owns a current handle",
                false,
            ));
        }
        state.current = Some(candidate.clone());
        state.last_committed = Some(candidate.connection_stamp());
        state.minimum_accepted = Some(candidate.connection_stamp());
        Ok(candidate)
    }

    pub fn clear(&self) -> Result<(), ProtocolError> {
        let mut state = self.state.lock().map_err(|_| internal_state_error())?;
        if let Some(handover) = state.handover.take() {
            match handover {
                ManagedHandover::Prepared { candidate, .. } => candidate.close(),
                ManagedHandover::Promoted { fallback, .. } => {
                    if let Some(fallback) = fallback {
                        fallback.close();
                    }
                }
                ManagedHandover::AwaitingFinalized { fallback, .. } => {
                    if let Some(fallback) = fallback {
                        fallback.close();
                    }
                }
                ManagedHandover::ReconciliationRequired { .. } => {}
            }
        }
        if let Some(connection) = state.current.take() {
            connection.close();
        }
        // `clear` is an explicit local fence: callers must re-establish the
        // remote lineage rather than silently reconnecting from stale state.
        state.last_committed = None;
        state.minimum_accepted = None;
        Ok(())
    }
}

impl<'a> PreparedHandover<'a> {
    /// Makes the candidate primary without releasing the previous connection.
    pub fn promote(mut self) -> Result<PromotedHandover<'a>, ProtocolError> {
        let token = self.token.as_ref().expect("active prepared handover");
        self.manager.promote_handover(token)?;
        let token = self.token.take().expect("active prepared handover");
        Ok(PromotedHandover {
            manager: self.manager,
            token: Some(token),
        })
    }

    /// Explicitly rolls back this candidate. Dropping the guard is equivalent.
    pub fn rollback(mut self) -> Result<(), ProtocolError> {
        let token = self.token.take().expect("active prepared handover");
        self.manager.rollback_prepared(&token)
    }
}

impl Drop for PreparedHandover<'_> {
    fn drop(&mut self) {
        if let Some(token) = self.token.take() {
            let _ = self.manager.rollback_prepared(&token);
        }
    }
}

impl<'a> PromotedHandover<'a> {
    /// Records that the commit-observed marker has been sent to the host.
    /// From this point, cancellation must not restore A blindly: the host may
    /// already have committed B and started retiring A.
    pub fn mark_observed_sent(mut self) -> Result<AwaitingFinalizedHandover<'a>, ProtocolError> {
        let token = self.token.as_ref().expect("active promoted handover");
        self.manager.mark_observed_sent(token)?;
        let token = self.token.take().expect("active promoted handover");
        Ok(AwaitingFinalizedHandover {
            manager: self.manager,
            token: Some(token),
        })
    }

    /// Explicitly restores the live fallback. Dropping the guard is equivalent.
    pub fn rollback(mut self) -> Result<(), ProtocolError> {
        let token = self.token.take().expect("active promoted handover");
        self.manager.rollback_promoted(&token)
    }
}

impl Drop for PromotedHandover<'_> {
    fn drop(&mut self) {
        if let Some(token) = self.token.take() {
            let _ = self.manager.rollback_promoted(&token);
        }
    }
}

impl AwaitingFinalizedHandover<'_> {
    /// Applies a correlated Finalized decision. Logical lineage advances to B
    /// even when its transport handle died while the marker was in flight.
    pub fn finalize(mut self) -> Result<ConnectedPeer, ProtocolError> {
        let token = self
            .token
            .as_ref()
            .expect("active awaiting-finalized handover");
        let candidate = self.manager.finalize_awaiting(token);
        // A matching Finalized decision is terminal even when B is dead and
        // `finalize_awaiting` returns a retryable transport error.
        self.token.take();
        candidate
    }
}

impl Drop for AwaitingFinalizedHandover<'_> {
    fn drop(&mut self) {
        if self.token.take().is_some() {
            // Deliberately leave the manager in AwaitingFinalized. Transport
            // liveness cannot resolve whether the host committed B.
        }
    }
}

impl MapleIrohEndpoint {
    #[cfg(test)]
    fn test_policy() -> ConnectionPolicy {
        ConnectionPolicy {
            connect_deadline: Duration::from_secs(5),
            handshake_deadline: Duration::from_secs(2),
            frame_deadline: Duration::from_secs(2),
        }
    }

    /// Build a direct-IP endpoint without N0 discovery and without a relay.
    /// This binds only the loopback interface so local tests never depend on
    /// host routing, VPN interfaces, or external connectivity. Product
    /// construction should call [`Self::bind_with_relay_policy`] with
    /// Maple-owned relays.
    #[cfg(test)]
    pub async fn bind_direct(
        identity: &DeviceIdentity,
        execution_target_id: &str,
        host_clock: HostConnectionClock,
    ) -> Result<Self, ProtocolError> {
        let secret_key = identity.iroh_secret_key().map_err(|error| {
            ProtocolError::new(
                ErrorCode::SecureStorageUnavailable,
                error.to_string(),
                false,
            )
        })?;
        let admission = PeerAdmission::default();
        let relay_policy = RelayPolicy::disabled();
        let endpoint = iroh::Endpoint::builder(iroh::endpoint::presets::Minimal)
            .clear_address_lookup()
            .relay_mode(relay_policy.mode())
            .transport_config(bounded_transport_config()?)
            .secret_key(secret_key)
            .alpns(vec![ALPN.to_vec()])
            .hooks(admission.clone())
            .clear_ip_transports()
            .bind_addr_with_opts(
                "127.0.0.1:0",
                iroh::endpoint::BindOpts::default().set_prefix_len(8),
            )
            .map_err(|error| {
                transport_error(
                    "failed to configure local Maple Iroh endpoint",
                    error,
                    false,
                )
            })?
            .bind()
            .await
            .map_err(|error| transport_error("failed to bind Maple Iroh endpoint", error, true))?;
        Self::from_bound_endpoint(
            endpoint,
            admission,
            relay_policy,
            Self::test_policy(),
            execution_target_id,
            host_clock,
        )
    }

    /// Test-only direct-IP reconstruction path. The independent current
    /// authorization snapshot is installed first; only lineage entries with
    /// the same account and pairing incarnation are restored before the accept
    /// pump starts.
    #[cfg(test)]
    async fn bind_direct_restoring_lineage(
        identity: &DeviceIdentity,
        execution_target_id: &str,
        host_clock: HostConnectionClock,
        authorization_snapshot: AuthorizationSnapshot,
        handoff: &mut EndpointLineageHandoff,
    ) -> Result<Self, ProtocolError> {
        handoff.authorize_restore_attempt(&authorization_snapshot)?;
        validate_bootstrap_id("execution_target_id", execution_target_id)?;
        let local_endpoint = identity_endpoint_id(identity)?;
        let secret_key = identity.iroh_secret_key().map_err(|error| {
            ProtocolError::new(
                ErrorCode::SecureStorageUnavailable,
                error.to_string(),
                false,
            )
        })?;
        handoff.ensure_source_closed().await?;
        let admission = PeerAdmission::default();
        admission.replace_authorizations(authorization_snapshot)?;
        admission.restore_endpoint_lineage(
            local_endpoint,
            execution_target_id,
            handoff.snapshot()?,
        )?;
        if handoff.take_fail_next_bind() {
            return Err(ProtocolError::new(
                ErrorCode::TransportUnavailable,
                "test-injected rebuilt endpoint bind failure",
                true,
            ));
        }
        let relay_policy = RelayPolicy::disabled();
        let endpoint = iroh::Endpoint::builder(iroh::endpoint::presets::Minimal)
            .clear_address_lookup()
            .relay_mode(relay_policy.mode())
            .transport_config(bounded_transport_config()?)
            .secret_key(secret_key)
            .alpns(vec![ALPN.to_vec()])
            .hooks(admission.clone())
            .clear_ip_transports()
            .bind_addr_with_opts(
                "127.0.0.1:0",
                iroh::endpoint::BindOpts::default().set_prefix_len(8),
            )
            .map_err(|error| {
                transport_error(
                    "failed to configure rebuilt local Maple Iroh endpoint",
                    error,
                    false,
                )
            })?
            .bind()
            .await
            .map_err(|error| {
                transport_error("failed to rebuild Maple Iroh endpoint", error, true)
            })?;
        let rebuilt = Self::from_bound_endpoint(
            endpoint,
            admission,
            relay_policy,
            Self::test_policy(),
            execution_target_id,
            host_clock,
        )?;
        handoff.consume_after_commit();
        Ok(rebuilt)
    }

    /// Test-only endpoint with IP transports disabled and Iroh's official
    /// production public relay map enabled. This is reserved for the ignored,
    /// synthetic live smoke test; product code supplies Maple's relay policy.
    #[cfg(test)]
    async fn bind_public_relay_only(
        identity: &DeviceIdentity,
        execution_target_id: &str,
        host_clock: HostConnectionClock,
        connection_policy: ConnectionPolicy,
    ) -> Result<Self, ProtocolError> {
        let secret_key = identity.iroh_secret_key().map_err(|error| {
            ProtocolError::new(
                ErrorCode::SecureStorageUnavailable,
                error.to_string(),
                false,
            )
        })?;
        let admission = PeerAdmission::default();
        let relay_policy = RelayPolicy::ignored_public_smoke()?;
        let endpoint = iroh::Endpoint::builder(iroh::endpoint::presets::Minimal)
            .clear_address_lookup()
            .relay_mode(relay_policy.mode())
            .transport_config(bounded_transport_config()?)
            .secret_key(secret_key)
            .alpns(vec![ALPN.to_vec()])
            .hooks(admission.clone())
            .clear_ip_transports()
            .bind()
            .await
            .map_err(|error| {
                transport_error("failed to bind relay-only Maple Iroh endpoint", error, true)
            })?;
        Self::from_bound_endpoint(
            endpoint,
            admission,
            relay_policy,
            connection_policy,
            execution_target_id,
            host_clock,
        )
    }

    /// Bind using an explicit relay policy. The caller may supply a custom
    /// Maple relay map; this function never enables Iroh's N0 DNS discovery.
    /// The opaque runtime couples the identity and already-persisted host epoch
    /// before this function can reach Iroh's bind operation.
    pub async fn bind_with_relay_policy(
        runtime: &DurableHostRuntime,
        execution_target_id: &str,
        relay_policy: RelayPolicy,
        connection_policy: ConnectionPolicy,
    ) -> Result<Self, ProtocolError> {
        validate_bootstrap_id("execution_target_id", execution_target_id)?;
        let identity = runtime.identity();
        let secret_key = identity.iroh_secret_key().map_err(|error| {
            ProtocolError::new(
                ErrorCode::SecureStorageUnavailable,
                error.to_string(),
                false,
            )
        })?;
        let admission = PeerAdmission::default();
        let endpoint = iroh::Endpoint::builder(iroh::endpoint::presets::Minimal)
            .clear_address_lookup()
            .relay_mode(relay_policy.mode())
            .transport_config(bounded_transport_config()?)
            .secret_key(secret_key)
            .alpns(vec![ALPN.to_vec()])
            .hooks(admission.clone())
            .bind()
            .await
            .map_err(|error| transport_error("failed to bind Maple Iroh endpoint", error, true))?;
        Self::from_bound_endpoint(
            endpoint,
            admission,
            relay_policy,
            connection_policy,
            execution_target_id,
            runtime.host_clock(),
        )
    }

    /// Rebuild a host endpoint from a quiescent in-memory lineage handoff.
    /// Authorization is deliberately supplied independently: authorization is
    /// current control-plane truth, while the snapshot carries only retained
    /// generation lineage for exact matching pair incarnations.
    pub async fn bind_with_relay_policy_restoring_lineage(
        runtime: &DurableHostRuntime,
        execution_target_id: &str,
        authorization_snapshot: AuthorizationSnapshot,
        handoff: &mut EndpointLineageHandoff,
        relay_policy: RelayPolicy,
        connection_policy: ConnectionPolicy,
    ) -> Result<Self, ProtocolError> {
        handoff.authorize_restore_attempt(&authorization_snapshot)?;
        validate_bootstrap_id("execution_target_id", execution_target_id)?;
        let identity = runtime.identity();
        let local_endpoint = identity_endpoint_id(identity)?;
        let secret_key = identity.iroh_secret_key().map_err(|error| {
            ProtocolError::new(
                ErrorCode::SecureStorageUnavailable,
                error.to_string(),
                false,
            )
        })?;
        handoff.ensure_source_closed().await?;
        let admission = PeerAdmission::default();
        admission.replace_authorizations(authorization_snapshot)?;
        admission.restore_endpoint_lineage(
            local_endpoint,
            execution_target_id,
            handoff.snapshot()?,
        )?;
        let endpoint = iroh::Endpoint::builder(iroh::endpoint::presets::Minimal)
            .clear_address_lookup()
            .relay_mode(relay_policy.mode())
            .transport_config(bounded_transport_config()?)
            .secret_key(secret_key)
            .alpns(vec![ALPN.to_vec()])
            .hooks(admission.clone())
            .bind()
            .await
            .map_err(|error| {
                transport_error("failed to rebuild Maple Iroh endpoint", error, true)
            })?;
        let rebuilt = Self::from_bound_endpoint(
            endpoint,
            admission,
            relay_policy,
            connection_policy,
            execution_target_id,
            runtime.host_clock(),
        )?;
        handoff.consume_after_commit();
        Ok(rebuilt)
    }

    fn from_bound_endpoint(
        endpoint: iroh::Endpoint,
        admission: PeerAdmission,
        relay_policy: RelayPolicy,
        connection_policy: ConnectionPolicy,
        execution_target_id: &str,
        host_clock: HostConnectionClock,
    ) -> Result<Self, ProtocolError> {
        validate_bootstrap_id("execution_target_id", execution_target_id)?;
        let execution_target_id: Arc<str> = Arc::from(execution_target_id);
        let accepted_connections = Arc::new(AcceptedPeerQueue::default());
        let (shutdown_tx, shutdown_rx) = oneshot::channel();
        spawn_accept_pump(
            endpoint.clone(),
            admission.clone(),
            connection_policy,
            execution_target_id.clone(),
            host_clock,
            accepted_connections.clone(),
            shutdown_rx,
        );
        Ok(Self {
            endpoint,
            admission,
            connection_policy,
            relay_policy,
            execution_target_id,
            accepted_connections,
            shutdown: Mutex::new(Some(shutdown_tx)),
        })
    }

    pub fn public_id(&self) -> String {
        self.endpoint.id().to_string()
    }

    /// Capture current addressing for an authenticated control-plane update.
    /// The private identity is never included.
    pub fn endpoint_addr(&self) -> iroh::EndpointAddr {
        self.endpoint.addr()
    }

    pub fn cached_endpoint_addr(
        &self,
        addr: iroh::EndpointAddr,
    ) -> Result<CachedEndpointAddr, ProtocolError> {
        CachedEndpointAddr::new(addr, &self.relay_policy)
    }

    /// Bootstrap/test-only imperative admission before an account snapshot is
    /// installed. Production pairing and revocation use
    /// [`Self::replace_authorizations`], whose durable revisions cannot race
    /// unversioned mutations. One-way pairing remains explicit: permission to
    /// dial never grants the peer reverse initiation.
    pub fn authorize_outgoing_execution_target(
        &self,
        host: iroh::EndpointId,
    ) -> Result<(), ProtocolError> {
        self.admission.allow(iroh::endpoint::Side::Client, host)
    }

    pub fn authorize_incoming_controller(
        &self,
        controller: iroh::EndpointId,
    ) -> Result<(), ProtocolError> {
        self.admission
            .allow(iroh::endpoint::Side::Server, controller)
    }

    pub fn revoke_outgoing_execution_target(
        &self,
        host: &iroh::EndpointId,
    ) -> Result<bool, ProtocolError> {
        self.admission.revoke(iroh::endpoint::Side::Client, host)
    }

    pub fn revoke_incoming_controller(
        &self,
        controller: &iroh::EndpointId,
    ) -> Result<bool, ProtocolError> {
        self.admission
            .revoke(iroh::endpoint::Side::Server, controller)
    }

    /// Atomically install the complete authorization snapshot for an account
    /// transition. Connections removed by the new snapshot close immediately.
    pub fn replace_authorizations(
        &self,
        snapshot: AuthorizationSnapshot,
    ) -> Result<AuthorizationTransitionReceipt, ProtocolError> {
        self.admission.replace_authorizations(snapshot)
    }

    pub fn clear_authorizations_and_close(&self) -> Result<(), ProtocolError> {
        self.admission.clear_all_and_close()
    }

    /// Notify Iroh immediately when a Tauri platform reports a network change.
    /// Android/iOS lifecycle glue calls this; it never waits on a new grant.
    /// If a platform reports that the native socket is no longer viable, the
    /// owner rebuilds via `bind_with_relay_policy` using the same DeviceIdentity
    /// and the same HostConnectionClock, reapplies locally paired peer admission,
    /// then races a newer generation through `connect_and_install_cached`.
    pub async fn network_change(&self) -> Result<(), ProtocolError> {
        tokio::time::timeout(
            self.connection_policy.frame_deadline,
            self.endpoint.network_change(),
        )
        .await
        .map_err(|_| operation_timeout("Maple network-change notification deadline elapsed"))?;
        Ok(())
    }

    /// Fast path: immediately dial the cached address. The caller may refresh
    /// the address in parallel, but a routine resume is never gated on it.
    /// Production reconnects must use [`Self::connect_and_install_cached`] so
    /// the controller retains committed lineage independently of a live QUIC
    /// handle. This unmanaged entry point exists only for isolated transport
    /// tests which intentionally exercise the first-generation bootstrap.
    #[cfg(test)]
    pub async fn connect_cached(
        &self,
        cached: &CachedEndpointAddr,
        expected_endpoint: iroh::EndpointId,
        request_id: &str,
        execution_target_id: &str,
    ) -> Result<ConnectedPeer, ProtocolError> {
        self.connect_cached_handover(
            None,
            cached,
            expected_endpoint,
            request_id,
            execution_target_id,
        )
        .await
    }

    async fn connect_cached_handover<'a>(
        &'a self,
        manager: Option<&'a GenerationConnectionManager>,
        cached: &CachedEndpointAddr,
        expected_endpoint: iroh::EndpointId,
        request_id: &str,
        execution_target_id: &str,
    ) -> Result<ConnectedPeer, ProtocolError> {
        validate_bootstrap_id("request_id", request_id)?;
        validate_bootstrap_id("execution_target_id", execution_target_id)?;
        let previous_connection_stamp = match manager {
            Some(manager) => manager.current_stamp()?,
            None => None,
        };
        if cached.endpoint_id() != expected_endpoint {
            return Err(ProtocolError::new(
                ErrorCode::WrongEndpoint,
                "cached address does not belong to the paired endpoint",
                false,
            ));
        }
        let active_pairing_fence = self
            .admission
            .pairing_fence(iroh::endpoint::Side::Client, &expected_endpoint)?;
        if let Some(manager) = manager {
            if manager.expected_controller != Some(self.endpoint.id())
                || manager.expected_remote != expected_endpoint
                || manager.expected_execution_target_id.as_ref() != execution_target_id
                || manager.expected_direction != PeerDirection::ControllerToHost
                || manager.pairing_fence != active_pairing_fence
            {
                return Err(ProtocolError::new(
                    ErrorCode::Unauthorized,
                    "connection manager does not match the current pairing",
                    false,
                ));
            }
        }
        self.relay_policy.validate_endpoint_addr(cached.as_iroh())?;
        if !self
            .admission
            .is_allowed(iroh::endpoint::Side::Client, &expected_endpoint)
        {
            return Err(ProtocolError::new(
                ErrorCode::Unauthorized,
                "paired endpoint is not admitted",
                false,
            ));
        }
        let reconnect_deadline = tokio::time::Instant::now()
            + self
                .connection_policy
                .connect_deadline
                .max(self.connection_policy.frame_deadline);
        let connection = tokio::time::timeout_at(
            reconnect_deadline,
            self.endpoint.connect(cached.as_iroh().clone(), ALPN),
        )
        .await
        .map_err(|_| {
            ProtocolError::new(
                ErrorCode::TransportUnavailable,
                "Maple host connection deadline elapsed",
                true,
            )
        })?
        .map_err(|error| transport_error("failed to connect to Maple host", error, true))?;
        if connection.remote_id() != expected_endpoint
            || connection.alpn() != ALPN
            || connection.side() != iroh::endpoint::Side::Client
        {
            return Err(ProtocolError::new(
                ErrorCode::WrongEndpoint,
                "connected peer identity or protocol did not match pairing",
                false,
            ));
        }
        let registered_fence = self.admission.register(&connection)?;
        validate_bootstrap_pairing_fence(registered_fence, active_pairing_fence)?;
        let bootstrap = BootstrapRequest {
            protocol_version: PROTOCOL_VERSION,
            request_id: request_id.into(),
            execution_target_id: execution_target_id.into(),
            bootstrap_generation: 0,
            pairing_fence: active_pairing_fence,
            previous_connection_stamp,
            reconciliation: None,
        };
        let bootstrap_deadline = reconnect_deadline;
        let (mut send, mut recv) =
            tokio::time::timeout_at(bootstrap_deadline, connection.open_bi())
                .await
                .map_err(|_| {
                    ProtocolError::new(
                        ErrorCode::TransportUnavailable,
                        "Maple bootstrap stream-open deadline elapsed",
                        true,
                    )
                })?
                .map_err(|error| {
                    transport_error("failed to open Maple bootstrap stream", error, true)
                })?;
        send.set_priority(StreamKind::Control.priority())
            .map_err(|error| {
                transport_error("failed to prioritize Maple bootstrap stream", error, true)
            })?;
        write_frame_until(&mut send, &bootstrap, bootstrap_deadline).await?;
        let response: BootstrapResponse = read_frame_until(&mut recv, bootstrap_deadline).await?;
        let connection_stamp =
            response.validate(request_id, execution_target_id, active_pairing_fence)?;
        let ready: BootstrapReady = read_frame_until(&mut recv, bootstrap_deadline).await?;
        ready.validate(
            request_id,
            execution_target_id,
            self.endpoint.id(),
            active_pairing_fence,
            connection_stamp,
            previous_connection_stamp,
        )?;
        let candidate = ConnectedPeer::new(
            connection,
            connection_stamp,
            active_pairing_fence,
            Arc::from(execution_target_id),
            PeerDirection::ControllerToHost,
            self.connection_policy.frame_deadline,
        );
        let mut prepared = match manager {
            Some(manager) => {
                let pending = PendingCommit::new(
                    manager.pairing_fence,
                    request_id,
                    execution_target_id,
                    self.endpoint.id(),
                    expected_endpoint,
                    previous_connection_stamp,
                    connection_stamp,
                )?;
                Some(manager.begin_handover(candidate.clone(), pending)?)
            }
            None => None,
        };
        if candidate.connection.close_reason().is_some() {
            return Err(ProtocolError::new(
                ErrorCode::TransportUnavailable,
                "handover candidate closed before installation acknowledgment",
                true,
            ));
        }
        let installed = BootstrapInstalled {
            protocol_version: PROTOCOL_VERSION,
            request_id: request_id.into(),
            execution_target_id: execution_target_id.into(),
            controller_id: self.endpoint.id().to_string(),
            pairing_fence: active_pairing_fence,
            connection_stamp,
            previous_connection_stamp,
        };
        write_frame_until(&mut send, &installed, bootstrap_deadline).await?;
        let committed: BootstrapCommitted = read_frame_until(&mut recv, bootstrap_deadline).await?;
        committed.validate(
            request_id,
            execution_target_id,
            self.endpoint.id(),
            active_pairing_fence,
            connection_stamp,
            previous_connection_stamp,
        )?;
        let promoted = match prepared.take() {
            Some(prepared) => Some(prepared.promote()?),
            None => None,
        };
        let observed = BootstrapCommitObserved {
            protocol_version: PROTOCOL_VERSION,
            request_id: request_id.into(),
            execution_target_id: execution_target_id.into(),
            controller_id: self.endpoint.id().to_string(),
            pairing_fence: active_pairing_fence,
            connection_stamp,
            previous_connection_stamp,
        };
        // Mark the decision before attempting the write. A timeout can occur
        // after QUIC accepted part or all of the frame, so rollback to A would
        // be unsafe from this point onward.
        let awaiting_finalized = match promoted {
            Some(promoted) => Some(promoted.mark_observed_sent()?),
            None => None,
        };
        write_frame_until(&mut send, &observed, bootstrap_deadline).await?;
        send.finish().map_err(|error| {
            transport_error(
                "failed to finish Maple bootstrap commit observation",
                error,
                true,
            )
        })?;
        let finalized: BootstrapFinalized = read_frame_until(&mut recv, bootstrap_deadline).await?;
        finalized.validate(
            request_id,
            execution_target_id,
            self.endpoint.id(),
            active_pairing_fence,
            connection_stamp,
            previous_connection_stamp,
        )?;
        // A fully decoded and correlated Finalized frame is the irreversible
        // controller decision. EOF is only strict framing hygiene and must not
        // leave the manager ambiguous if the host FIN is lost.
        let installed = match awaiting_finalized {
            Some(awaiting) => awaiting.finalize(),
            None => Ok(candidate),
        };
        if let Err(error) = expect_stream_end(&mut recv, bootstrap_deadline).await {
            return Err(error);
        }
        installed
    }

    async fn reconcile_cached_pending(
        &self,
        manager: &GenerationConnectionManager,
        cached: &CachedEndpointAddr,
        expected_endpoint: iroh::EndpointId,
        request_id: &str,
        execution_target_id: &str,
        pending: PendingCommit,
    ) -> Result<(), ProtocolError> {
        validate_bootstrap_id("request_id", request_id)?;
        let active_fence = self
            .admission
            .pairing_fence(iroh::endpoint::Side::Client, &expected_endpoint)?;
        pending.validate(
            execution_target_id,
            self.endpoint.id(),
            expected_endpoint,
            active_fence,
        )?;
        if manager.pairing_fence != active_fence
            || manager.expected_controller != Some(self.endpoint.id())
            || manager.expected_remote != expected_endpoint
        {
            return Err(ProtocolError::new(
                ErrorCode::Unauthorized,
                "pending reconciliation does not match the current pairing",
                false,
            ));
        }
        self.relay_policy.validate_endpoint_addr(cached.as_iroh())?;
        let deadline = tokio::time::Instant::now()
            + self
                .connection_policy
                .connect_deadline
                .max(self.connection_policy.frame_deadline);
        let connection = tokio::time::timeout_at(
            deadline,
            self.endpoint.connect(cached.as_iroh().clone(), ALPN),
        )
        .await
        .map_err(|_| operation_timeout("Maple reconciliation connection deadline elapsed"))?
        .map_err(|error| {
            transport_error("failed to connect for Maple reconciliation", error, true)
        })?;
        if connection.remote_id() != expected_endpoint
            || connection.alpn() != ALPN
            || connection.side() != iroh::endpoint::Side::Client
        {
            close_bootstrap_connection(&connection);
            return Err(ProtocolError::new(
                ErrorCode::WrongEndpoint,
                "reconciliation peer identity or protocol did not match pairing",
                false,
            ));
        }
        let registered_fence = self.admission.register(&connection)?;
        validate_bootstrap_pairing_fence(registered_fence, active_fence)?;
        let (mut send, mut recv) = tokio::time::timeout_at(deadline, connection.open_bi())
            .await
            .map_err(|_| operation_timeout("Maple reconciliation stream deadline elapsed"))?
            .map_err(|error| {
                transport_error("failed to open Maple reconciliation stream", error, true)
            })?;
        send.set_priority(StreamKind::Control.priority())
            .map_err(|error| {
                transport_error("failed to prioritize Maple reconciliation", error, true)
            })?;
        let request = BootstrapRequest {
            protocol_version: PROTOCOL_VERSION,
            request_id: request_id.into(),
            execution_target_id: execution_target_id.into(),
            bootstrap_generation: 0,
            pairing_fence: active_fence,
            previous_connection_stamp: pending.previous_connection_stamp,
            reconciliation: Some(pending.clone()),
        };
        write_frame_until(&mut send, &request, deadline).await?;
        send.finish().map_err(|error| {
            transport_error("failed to finish Maple reconciliation request", error, true)
        })?;
        let response: BootstrapReconciled = read_frame_until(&mut recv, deadline).await?;
        let committed = response.validate(
            request_id,
            execution_target_id,
            self.endpoint.id(),
            expected_endpoint,
            &pending,
            active_fence,
        )?;
        // The authenticated decision frame, not its trailing EOF, resolves
        // ambiguity. A missing FIN therefore cannot wedge the manager.
        manager.apply_reconciliation(&pending, committed)?;
        let framing = expect_stream_end(&mut recv, deadline).await;
        connection.close(
            iroh::endpoint::VarInt::from_u32(0),
            b"Maple reconciliation complete",
        );
        framing
    }

    /// Connect using the cached fast path and atomically offer the completed
    /// connection to a generation manager. Multiple callers may race this
    /// method after a network transition; the first success for a generation
    /// wins without an enclave/grant round trip.
    pub async fn connect_and_install_cached(
        &self,
        manager: &GenerationConnectionManager,
        cached: &CachedEndpointAddr,
        expected_endpoint: iroh::EndpointId,
        request_id: &str,
        execution_target_id: &str,
    ) -> Result<ConnectedPeer, ProtocolError> {
        if let Some(pending) = manager.pending_reconciliation()? {
            self.reconcile_cached_pending(
                manager,
                cached,
                expected_endpoint,
                request_id,
                execution_target_id,
                pending,
            )
            .await?;
        }
        self.connect_cached_handover(
            Some(manager),
            cached,
            expected_endpoint,
            request_id,
            execution_target_id,
        )
        .await
    }

    /// Accept any authenticated, locally admitted controller. The host router
    /// decides which execution target/session owns it after identity is known;
    /// this transport method never consumes another admitted controller while
    /// waiting for a preselected one.
    pub async fn accept_authenticated(&self) -> Result<ConnectedPeer, ProtocolError> {
        loop {
            let peer = self.accepted_connections.recv().await?;
            if self.admission.is_current_incoming(
                &peer.remote_id(),
                peer.connection_stamp(),
                peer.pairing_fence(),
            ) {
                return Ok(peer);
            }
            peer.connection.close(
                iroh::endpoint::VarInt::from_u32(0x4d_47),
                b"queued connection generation was superseded",
            );
        }
    }

    /// Revalidate that a host-side application adapter still owns the exact
    /// authenticated controller generation returned by this endpoint.
    ///
    /// `accept_authenticated` performs the same check when dequeuing, but a
    /// revocation or replacement can race later stream handling. Read-only
    /// adapters call this again immediately before dispatch and disclosure;
    /// future mutating operations will additionally need their own durable
    /// authorization/idempotency admission boundary.
    pub fn validate_current_incoming_peer(
        &self,
        peer: &ConnectedPeer,
    ) -> Result<(), ProtocolError> {
        if peer.connection.side() != iroh::endpoint::Side::Server
            || peer.outbound_direction != PeerDirection::HostToController
            || peer.execution_target_id() != self.execution_target_id.as_ref()
        {
            return Err(ProtocolError::new(
                ErrorCode::WrongEndpoint,
                "remote request is not bound to this execution target",
                false,
            ));
        }
        if peer.connection.close_reason().is_some()
            || !self.admission.is_current_incoming(
                &peer.remote_id(),
                peer.connection_stamp(),
                peer.pairing_fence(),
            )
        {
            return Err(ProtocolError::new(
                ErrorCode::Revoked,
                "remote controller generation is no longer authorized",
                false,
            ));
        }
        Ok(())
    }

    /// Capture the exact installed authorization context for a current host-
    /// side controller connection. This is the only production capability
    /// accepted by synchronized Agent target binding; renderer values and
    /// pairing payload lifecycle revisions cannot construct it.
    pub(crate) fn verified_incoming_peer_authorization(
        &self,
        peer: &ConnectedPeer,
    ) -> Result<VerifiedIncomingPeerAuthorization, ProtocolError> {
        if peer.connection.side() != iroh::endpoint::Side::Server
            || peer.outbound_direction != PeerDirection::HostToController
            || peer.execution_target_id() != self.execution_target_id.as_ref()
            || peer.connection.close_reason().is_some()
        {
            return Err(ProtocolError::new(
                ErrorCode::WrongEndpoint,
                "remote request is not bound to this execution target",
                false,
            ));
        }
        let controller_endpoint = peer.remote_id();
        let pairing_fence = peer.pairing_fence();
        let connection_stamp = peer.connection_stamp();
        let authorization = self.admission.current_incoming_authorization(
            &controller_endpoint,
            connection_stamp,
            pairing_fence,
        )?;
        Ok(VerifiedIncomingPeerAuthorization {
            admission: self.admission.clone(),
            authorization,
            controller_endpoint,
            execution_target_id: Arc::clone(&self.execution_target_id),
            pairing_fence,
            connection_stamp,
        })
    }

    pub async fn close(&self) {
        if let Ok(mut shutdown) = self.shutdown.lock() {
            if let Some(shutdown) = shutdown.take() {
                let _ = shutdown.send(());
            }
        }
        self.accepted_connections.close();
        let _ = self.admission.clear_all_and_close();
        self.endpoint.close().await;
    }

    /// Synchronously fence this endpoint and begin an exclusive, retryable
    /// lineage handoff. No await occurs before the caller owns the capability.
    /// Use a restoring constructor with `&mut` access to the returned guard;
    /// cancellation or bind failure leaves it available for another attempt.
    pub fn begin_lineage_handoff(self) -> Result<EndpointLineageHandoff, ProtocolError> {
        let snapshot = self.admission.capture_endpoint_lineage_and_fence(
            self.endpoint.id(),
            self.execution_target_id.clone(),
        )?;
        let authorization_floor_revision = snapshot.snapshot_revision;
        let authorization_floor_digest = snapshot.authorization_digest;
        if let Ok(mut shutdown) = self.shutdown.lock() {
            if let Some(shutdown) = shutdown.take() {
                let _ = shutdown.send(());
            }
        }
        self.accepted_connections.close();
        Ok(EndpointLineageHandoff {
            source: Some(self),
            snapshot: Some(snapshot),
            authorization_floor_revision,
            authorization_floor_digest,
            source_closed: false,
            consumed: false,
            #[cfg(test)]
            close_gate: None,
            #[cfg(test)]
            fail_next_bind: false,
        })
    }
}

impl Drop for MapleIrohEndpoint {
    fn drop(&mut self) {
        if let Ok(shutdown) = self.shutdown.get_mut() {
            if let Some(shutdown) = shutdown.take() {
                let _ = shutdown.send(());
            }
        }
        self.accepted_connections.close();
        let _ = self.admission.clear_all_and_close();
    }
}

fn bounded_transport_config() -> Result<iroh::endpoint::QuicTransportConfig, ProtocolError> {
    // This bounds post-accept QUIC application resources. In Iroh 1.0.3 the
    // Endpoint builder does not expose replacement of noq's default
    // pre-accept Incoming buffering (65,536 entries, 10 MiB each, 100 MiB
    // aggregate); `Incoming::accept_with` is already too late. Maple's hook,
    // handshake deadline, and eight-task accept pump limit authenticated work,
    // but a fully bounded pre-auth queue needs an upstream Iroh API/change.
    let idle_timeout =
        iroh::endpoint::IdleTimeout::try_from(Duration::from_secs(30)).map_err(|_| {
            ProtocolError::new(
                ErrorCode::Internal,
                "invalid Maple QUIC idle timeout",
                false,
            )
        })?;
    Ok(iroh::endpoint::QuicTransportConfig::builder()
        .max_concurrent_bidi_streams(iroh::endpoint::VarInt::from_u32(MAX_INCOMING_BI_STREAMS))
        .max_concurrent_uni_streams(iroh::endpoint::VarInt::from_u32(0))
        .stream_receive_window(iroh::endpoint::VarInt::from_u32(
            STREAM_RECEIVE_WINDOW_BYTES,
        ))
        .receive_window(iroh::endpoint::VarInt::from_u32(
            CONNECTION_RECEIVE_WINDOW_BYTES,
        ))
        .send_window(CONNECTION_SEND_WINDOW_BYTES)
        .datagram_receive_buffer_size(None)
        .datagram_send_buffer_size(0)
        .max_idle_timeout(Some(idle_timeout))
        .build())
}

fn spawn_accept_pump(
    endpoint: iroh::Endpoint,
    admission: PeerAdmission,
    connection_policy: ConnectionPolicy,
    execution_target_id: Arc<str>,
    host_clock: HostConnectionClock,
    accepted_connections: Arc<AcceptedPeerQueue>,
    mut shutdown: oneshot::Receiver<()>,
) {
    let local_endpoint_id = endpoint.id();
    tokio::spawn(async move {
        let pending = Arc::new(Semaphore::new(MAX_PENDING_HANDSHAKES));
        let mut tasks = futures_util::stream::FuturesUnordered::new();
        loop {
            tokio::select! {
                biased;
                _ = &mut shutdown => break,
                completed = tasks.next(), if !tasks.is_empty() => {
                    let _ = completed;
                }
                incoming = endpoint.accept() => {
                    let Some(incoming) = incoming else { break; };
                    let Ok(permit) = pending.clone().try_acquire_owned() else {
                        incoming.refuse();
                        continue;
                    };
                    let admission = admission.clone();
                    let execution_target_id = execution_target_id.clone();
                    let host_clock = host_clock.clone();
                    let accepted_connections = accepted_connections.clone();
                    let local_endpoint_id = local_endpoint_id;
                    tasks.push(tokio::spawn(async move {
                        handle_incoming_bootstrap(
                            incoming,
                            local_endpoint_id,
                            admission,
                            connection_policy,
                            execution_target_id,
                            host_clock,
                            accepted_connections,
                            permit,
                        ).await
                    }));
                }
            }
        }
        // Ordinary MapleIrohEndpoint::drop reaches this path. Close the socket
        // before draining so noq cannot retain an unattended pre-accept queue,
        // then abort bounded handshake tasks instead of waiting up to the
        // policy deadline.
        endpoint.close().await;
        accepted_connections.close();
        for task in tasks.iter() {
            task.abort();
        }
        while let Some(completed) = tasks.next().await {
            let _ = completed;
        }
    });
}

async fn handle_incoming_bootstrap(
    incoming: iroh::endpoint::Incoming,
    local_endpoint_id: iroh::EndpointId,
    admission: PeerAdmission,
    connection_policy: ConnectionPolicy,
    execution_target_id: Arc<str>,
    host_clock: HostConnectionClock,
    accepted_connections: Arc<AcceptedPeerQueue>,
    _permit: OwnedSemaphorePermit,
) -> Result<(), ProtocolError> {
    let reconnect_deadline = tokio::time::Instant::now()
        + connection_policy
            .handshake_deadline
            .max(connection_policy.frame_deadline);
    let connection = tokio::time::timeout_at(reconnect_deadline, incoming)
        .await
        .map_err(|_| {
            ProtocolError::new(
                ErrorCode::TransportUnavailable,
                "Maple controller handshake deadline elapsed",
                true,
            )
        })?
        .map_err(|error| transport_error("failed to accept Maple controller", error, true))?;
    if connection.alpn() != ALPN || connection.side() != iroh::endpoint::Side::Server {
        return Err(ProtocolError::new(
            ErrorCode::UnsupportedVersion,
            "incoming peer protocol did not match Maple remote Agent Mode",
            false,
        ));
    }
    let pairing_fence = admission.register(&connection)?;

    let bootstrap_deadline = reconnect_deadline;
    let (mut send, mut recv) = tokio::time::timeout_at(bootstrap_deadline, connection.accept_bi())
        .await
        .map_err(|_| {
            ProtocolError::new(
                ErrorCode::TransportUnavailable,
                "Maple bootstrap stream deadline elapsed",
                true,
            )
        })?
        .map_err(|error| transport_error("failed to accept Maple bootstrap stream", error, true))?;
    let request: BootstrapRequest = read_frame_until(&mut recv, bootstrap_deadline).await?;
    if let Err(error) = request.validate(&execution_target_id, pairing_fence) {
        if error.code == ErrorCode::Unauthorized && !error.retryable {
            // Echo the controller-supplied fence, not the host's current
            // incarnation. This lets the caller authenticate/correlate the
            // denial without turning the response into pairing discovery.
            send.set_priority(StreamKind::Control.priority())
                .map_err(|priority_error| {
                    transport_error(
                        "failed to prioritize Maple bootstrap rejection",
                        priority_error,
                        true,
                    )
                })?;
            let rejection = BootstrapResponse {
                protocol_version: PROTOCOL_VERSION,
                request_id: request.request_id.clone(),
                execution_target_id: request.execution_target_id.clone(),
                pairing_fence: request.pairing_fence,
                result: Err(error.clone()),
            };
            write_frame_until(&mut send, &rejection, bootstrap_deadline).await?;
            send.finish().map_err(|finish_error| {
                transport_error(
                    "failed to finish Maple bootstrap rejection",
                    finish_error,
                    true,
                )
            })?;
            // `finish` only queues FIN locally. Retain the sole strong host
            // handle until the controller consumes the authenticated denial
            // and drops this bootstrap-only connection, bounded by the
            // existing bootstrap deadline.
            if tokio::time::timeout_at(bootstrap_deadline, connection.closed())
                .await
                .is_err()
            {
                close_bootstrap_connection(&connection);
            }
        }
        return Err(error);
    }
    let controller_id = connection.remote_id();
    if let Some(pending) = request.reconciliation.as_ref() {
        let local_host = local_endpoint_id;
        pending.validate(
            &execution_target_id,
            controller_id,
            local_host,
            pairing_fence,
        )?;
        if let Err(error) = expect_stream_end(&mut recv, bootstrap_deadline).await {
            close_bootstrap_connection(&connection);
            return Err(error);
        }
        let (committed_connection_stamp, to_close) = admission.reconcile_incoming(
            &accepted_connections,
            controller_id,
            pending,
            pairing_fence,
            local_host,
            &execution_target_id,
        )?;
        close_weak_connections(
            to_close,
            b"pending handover reconciled to previous generation",
        );
        let reconciled = BootstrapReconciled {
            protocol_version: PROTOCOL_VERSION,
            request_id: request.request_id,
            execution_target_id: request.execution_target_id,
            controller_id: controller_id.to_string(),
            host_id: local_host.to_string(),
            pending: pending.clone(),
            committed_connection_stamp,
        };
        write_frame_until(&mut send, &reconciled, bootstrap_deadline).await?;
        send.finish().map_err(|error| {
            transport_error(
                "failed to finish Maple reconciliation response",
                error,
                true,
            )
        })?;
        // `finish` only queues FIN locally. Keep the sole strong host handle
        // until the controller consumes the authenticated decision and closes
        // this reconciliation-only connection, bounded by the same deadline.
        // A lost response remains idempotently retryable from PendingCommit.
        if tokio::time::timeout_at(bootstrap_deadline, connection.closed())
            .await
            .is_err()
        {
            close_bootstrap_connection(&connection);
        }
        return Ok(());
    }
    // Reserve routable queue capacity before acknowledging or committing this
    // generation. Same-peer reconnects share one keyed slot; another peer can
    // never be displaced by stale generations.
    let queue_reservation = accepted_connections.reserve(controller_id)?;

    let connection_stamp = host_clock.allocate()?;
    send.set_priority(StreamKind::Control.priority())
        .map_err(|error| {
            transport_error("failed to prioritize Maple bootstrap response", error, true)
        })?;
    let pending_peer = PendingConnectedPeer {
        connection: connection.clone(),
        connection_stamp,
        pairing_fence,
        execution_target_id: execution_target_id.clone(),
        outbound_direction: PeerDirection::HostToController,
        frame_deadline: connection_policy.frame_deadline,
    };
    let ready = BootstrapReady {
        protocol_version: PROTOCOL_VERSION,
        request_id: request.request_id.clone(),
        execution_target_id: request.execution_target_id.clone(),
        controller_id: controller_id.to_string(),
        pairing_fence,
        connection_stamp,
        previous_connection_stamp: request.previous_connection_stamp,
    };
    let pending = PendingCommit::new(
        pairing_fence,
        &request.request_id,
        &request.execution_target_id,
        controller_id,
        local_endpoint_id,
        request.previous_connection_stamp,
        connection_stamp,
    )?;
    // Stage B before acknowledging it. Under the same admission lock, the
    // controller's claimed A lineage is compared to the host's live current A.
    // B remains queue-gated and A remains current through CommitObserved.
    let mut commit = match admission.commit_and_publish_incoming(
        &connection,
        connection_stamp,
        request.previous_connection_stamp,
        pairing_fence,
        queue_reservation,
        pending_peer,
        pending,
    ) {
        Ok(commit) => commit,
        Err(error) => {
            close_bootstrap_connection(&connection);
            return Err(error);
        }
    };
    let response = BootstrapResponse {
        protocol_version: PROTOCOL_VERSION,
        request_id: request.request_id.clone(),
        execution_target_id: request.execution_target_id.clone(),
        pairing_fence,
        result: Ok(BootstrapAccepted { connection_stamp }),
    };
    if let Err(error) = write_frame_until(&mut send, &response, bootstrap_deadline).await {
        let rollback = rollback_staged_incoming(&admission, &accepted_connections, &mut commit);
        close_bootstrap_connection(&connection);
        rollback?;
        return Err(error);
    }
    if let Err(error) = write_frame_until(&mut send, &ready, bootstrap_deadline).await {
        let rollback = rollback_staged_incoming(&admission, &accepted_connections, &mut commit);
        close_bootstrap_connection(&connection);
        rollback?;
        return Err(error);
    }
    if let Err(error) = admission.validate_incoming_activation(&commit) {
        let rollback = rollback_staged_incoming(&admission, &accepted_connections, &mut commit);
        close_bootstrap_connection(&connection);
        rollback?;
        return Err(error);
    }

    let installed: BootstrapInstalled = match read_frame_until(&mut recv, bootstrap_deadline).await
    {
        Ok(installed) => installed,
        Err(error) => {
            let rollback = rollback_staged_incoming(&admission, &accepted_connections, &mut commit);
            close_bootstrap_connection(&connection);
            rollback?;
            return Err(error);
        }
    };
    if let Err(error) = installed.validate(
        &request.request_id,
        &request.execution_target_id,
        controller_id,
        pairing_fence,
        connection_stamp,
        request.previous_connection_stamp,
    ) {
        let rollback = rollback_staged_incoming(&admission, &accepted_connections, &mut commit);
        close_bootstrap_connection(&connection);
        rollback?;
        return Err(error);
    }
    if let Err(error) = admission.validate_incoming_activation(&commit) {
        let rollback = rollback_staged_incoming(&admission, &accepted_connections, &mut commit);
        close_bootstrap_connection(&connection);
        rollback?;
        return Err(error);
    }

    let committed = BootstrapCommitted {
        protocol_version: PROTOCOL_VERSION,
        request_id: request.request_id.clone(),
        execution_target_id: request.execution_target_id.clone(),
        controller_id: controller_id.to_string(),
        pairing_fence,
        connection_stamp,
        previous_connection_stamp: request.previous_connection_stamp,
    };
    if let Err(error) = write_frame_until(&mut send, &committed, bootstrap_deadline).await {
        let rollback = rollback_staged_incoming(&admission, &accepted_connections, &mut commit);
        close_bootstrap_connection(&connection);
        rollback?;
        return Err(error);
    }

    let observed: BootstrapCommitObserved = match read_frame_until(&mut recv, bootstrap_deadline)
        .await
    {
        Ok(observed) => observed,
        Err(error) => {
            let rollback = rollback_staged_incoming(&admission, &accepted_connections, &mut commit);
            close_bootstrap_connection(&connection);
            rollback?;
            return Err(error);
        }
    };
    if let Err(error) = observed.validate(
        &request.request_id,
        &request.execution_target_id,
        controller_id,
        pairing_fence,
        connection_stamp,
        request.previous_connection_stamp,
    ) {
        let rollback = rollback_staged_incoming(&admission, &accepted_connections, &mut commit);
        close_bootstrap_connection(&connection);
        rollback?;
        return Err(error);
    }
    if let Err(error) = expect_stream_end(&mut recv, bootstrap_deadline).await {
        let rollback = rollback_staged_incoming(&admission, &accepted_connections, &mut commit);
        close_bootstrap_connection(&connection);
        rollback?;
        return Err(error);
    }

    // Commit point: a correlated Installed + Observed + controller FIN proves
    // that the controller selected B. Atomically make B current/routable, then
    // retire A. Failure to deliver Finalized after this point never restores A;
    // the next reconnect must advance from B's stamp.
    let (displaced_active, queued_displaced) = match admission
        .finalize_observed_incoming(&accepted_connections, &commit)
    {
        Ok(displaced) => displaced,
        Err(error) => {
            let rollback = rollback_staged_incoming(&admission, &accepted_connections, &mut commit);
            close_bootstrap_connection(&connection);
            rollback?;
            return Err(error);
        }
    };
    if let Some(displaced) = queued_displaced {
        displaced.close();
    }
    if let Some(displaced) = commit.previous_connection.take() {
        displaced.close(
            iroh::endpoint::VarInt::from_u32(0x4d_47),
            b"superseded connection generation",
        );
    }
    if let Some(displaced) = displaced_active.and_then(|handle| handle.upgrade()) {
        displaced.close(
            iroh::endpoint::VarInt::from_u32(0x4d_47),
            b"superseded connection generation",
        );
    }

    let finalized = BootstrapFinalized {
        protocol_version: PROTOCOL_VERSION,
        request_id: request.request_id,
        execution_target_id: request.execution_target_id,
        controller_id: controller_id.to_string(),
        pairing_fence,
        connection_stamp,
        previous_connection_stamp: request.previous_connection_stamp,
    };
    write_frame_until(&mut send, &finalized, bootstrap_deadline).await?;
    send.finish().map_err(|error| {
        transport_error("failed to finish Maple bootstrap finalization", error, true)
    })?;
    Ok(())
}

fn close_bootstrap_connection(connection: &iroh::endpoint::Connection) {
    connection.close(
        iroh::endpoint::VarInt::from_u32(0),
        b"Maple bootstrap handover failed",
    );
}

struct BoundedWriter {
    bytes: Vec<u8>,
}

impl BoundedWriter {
    fn new() -> Self {
        Self {
            bytes: Vec::with_capacity(4096),
        }
    }

    fn into_inner(self) -> Vec<u8> {
        self.bytes
    }
}

impl Write for BoundedWriter {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        let new_len =
            self.bytes.len().checked_add(bytes.len()).ok_or_else(|| {
                io::Error::new(io::ErrorKind::InvalidData, "frame length overflow")
            })?;
        if new_len > MAX_FRAME_BYTES as usize {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "frame exceeds Maple limit",
            ));
        }
        self.bytes.extend_from_slice(bytes);
        Ok(bytes.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

fn encode_frame_bounded<T: Serialize>(value: &T) -> Result<Vec<u8>, ProtocolError> {
    let mut writer = BoundedWriter::new();
    ciborium::ser::into_writer(value, &mut writer).map_err(|error| {
        let (code, message) = match error {
            ciborium::ser::Error::Io(_) => (
                ErrorCode::FrameTooLarge,
                "frame exceeds Maple's bounded encoder",
            ),
            ciborium::ser::Error::Value(_) => {
                (ErrorCode::InvalidFrame, "failed to encode Maple frame")
            }
        };
        ProtocolError::new(code, message, false)
    })?;
    Ok(writer.into_inner())
}

/// Preflight one concrete response frame without retaining its bytes. Host
/// adapters use this before starting a streamed page so a single oversized
/// record can be returned as a typed error rather than truncating mid-page.
pub(crate) fn validate_frame_encodable<T: Serialize>(value: &T) -> Result<(), ProtocolError> {
    encode_frame_bounded(value).map(|_| ())
}

async fn write_frame_bounded<T: Serialize>(
    send: &mut iroh::endpoint::SendStream,
    value: &T,
    deadline: Duration,
) -> Result<(), ProtocolError> {
    write_frame_until(send, value, tokio::time::Instant::now() + deadline).await
}

async fn write_frame_until<T: Serialize>(
    send: &mut iroh::endpoint::SendStream,
    value: &T,
    deadline: tokio::time::Instant,
) -> Result<(), ProtocolError> {
    let payload = encode_frame_bounded(value)?;
    let len = u32::try_from(payload.len()).map_err(|_| {
        ProtocolError::new(ErrorCode::FrameTooLarge, "frame length overflow", false)
    })?;
    tokio::time::timeout_at(deadline, async {
        send.write_all(&len.to_be_bytes())
            .await
            .map_err(|error| transport_error("failed to write frame length", error, true))?;
        send.write_all(&payload)
            .await
            .map_err(|error| transport_error("failed to write frame payload", error, true))?;
        Ok(())
    })
    .await
    .map_err(|_| {
        ProtocolError::new(
            ErrorCode::TransportUnavailable,
            "Maple frame write deadline elapsed",
            true,
        )
    })?
}

async fn read_frame_bounded<T: DeserializeOwned>(
    recv: &mut iroh::endpoint::RecvStream,
    deadline: Duration,
) -> Result<T, ProtocolError> {
    read_frame_until(recv, tokio::time::Instant::now() + deadline).await
}

async fn read_frame_until<T: DeserializeOwned>(
    recv: &mut iroh::endpoint::RecvStream,
    absolute_deadline: tokio::time::Instant,
) -> Result<T, ProtocolError> {
    let mut len = [0_u8; 4];
    read_exact_cancel_safe(recv, &mut len, absolute_deadline).await?;
    let len = u32::from_be_bytes(len);
    validate_frame_len(len as usize)?;
    let mut payload = vec![0_u8; len as usize];
    read_exact_cancel_safe(recv, &mut payload, absolute_deadline).await?;
    decode_frame_payload(&payload)
}

/// Read one Events response frame without imposing an application lifetime or
/// idle-read timeout. The authenticated QUIC connection, explicit RPC cancel,
/// and authority revocation own liveness; allocation and CBOR bounds remain
/// identical to finite request frames.
async fn read_frame_unbounded<T: DeserializeOwned>(
    recv: &mut iroh::endpoint::RecvStream,
) -> Result<T, ProtocolError> {
    let mut len = [0_u8; 4];
    read_exact_unbounded(recv, &mut len).await?;
    let len = u32::from_be_bytes(len);
    validate_frame_len(len as usize)?;
    let mut payload = vec![0_u8; len as usize];
    read_exact_unbounded(recv, &mut payload).await?;
    decode_frame_payload(&payload)
}

fn decode_frame_payload<T: DeserializeOwned>(payload: &[u8]) -> Result<T, ProtocolError> {
    validate_cbor_shape(payload)?;
    let mut cursor = Cursor::new(payload);
    let value = ciborium::de::from_reader_with_recursion_limit(&mut cursor, MAX_CBOR_RECURSION)
        .map_err(|_| {
            ProtocolError::new(
                ErrorCode::InvalidFrame,
                "failed to decode Maple frame",
                false,
            )
        })?;
    if cursor.position() != payload.len() as u64 {
        return Err(ProtocolError::new(
            ErrorCode::InvalidFrame,
            "Maple frame contains trailing data",
            false,
        ));
    }
    Ok(value)
}

async fn expect_stream_end(
    recv: &mut iroh::endpoint::RecvStream,
    deadline: tokio::time::Instant,
) -> Result<(), ProtocolError> {
    let mut unexpected = [0_u8; 1];
    match tokio::time::timeout_at(deadline, recv.read(&mut unexpected))
        .await
        .map_err(|_| operation_timeout("Maple bootstrap completion deadline elapsed"))?
        .map_err(|error| {
            transport_error("failed to finish Maple bootstrap response", error, true)
        })? {
        None => Ok(()),
        Some(_) => Err(ProtocolError::new(
            ErrorCode::InvalidFrame,
            "Maple bootstrap contains an unexpected trailing frame",
            false,
        )),
    }
}

async fn expect_stream_end_unbounded(
    recv: &mut iroh::endpoint::RecvStream,
) -> Result<(), ProtocolError> {
    let mut unexpected = [0_u8; 1];
    match recv
        .read(&mut unexpected)
        .await
        .map_err(|error| transport_error("failed to finish Maple Events response", error, true))?
    {
        None => Ok(()),
        Some(_) => Err(ProtocolError::new(
            ErrorCode::InvalidFrame,
            "Maple Events response contains unexpected trailing data",
            false,
        )),
    }
}

/// Validate the allocation-relevant CBOR shape before Serde sees any size
/// hints. Ciborium otherwise propagates attacker-declared container lengths to
/// `Vec::reserve`; nested tiny frames can amplify into large heap allocations.
/// Maple emits only definite-length CBOR, so indefinite forms are rejected.
fn validate_cbor_shape(payload: &[u8]) -> Result<(), ProtocolError> {
    let mut offset = 0;
    parse_cbor_item(payload, &mut offset, 0)?;
    if offset != payload.len() {
        return Err(invalid_cbor_shape("Maple frame contains trailing data"));
    }
    Ok(())
}

fn parse_cbor_item(payload: &[u8], offset: &mut usize, depth: usize) -> Result<(), ProtocolError> {
    if depth >= MAX_CBOR_RECURSION {
        return Err(invalid_cbor_shape("Maple frame nesting is too deep"));
    }
    let initial = *payload
        .get(*offset)
        .ok_or_else(|| invalid_cbor_shape("Maple frame is truncated"))?;
    *offset += 1;
    let major = initial >> 5;
    let additional = initial & 0x1f;
    if additional == 31 {
        return Err(invalid_cbor_shape("indefinite-length CBOR is not enabled"));
    }
    let argument = read_cbor_argument(payload, offset, additional)?;
    match major {
        0 | 1 => Ok(()),
        2 | 3 => {
            let length = usize::try_from(argument)
                .map_err(|_| invalid_cbor_shape("CBOR string length is invalid"))?;
            let end = offset
                .checked_add(length)
                .ok_or_else(|| invalid_cbor_shape("CBOR string length overflow"))?;
            if end > payload.len() {
                return Err(invalid_cbor_shape("CBOR string is truncated"));
            }
            *offset = end;
            Ok(())
        }
        4 => {
            if argument > MAX_CBOR_CONTAINER_ITEMS {
                return Err(invalid_cbor_shape("CBOR array exceeds Maple's item bound"));
            }
            for _ in 0..argument {
                parse_cbor_item(payload, offset, depth + 1)?;
            }
            Ok(())
        }
        5 => {
            if argument > MAX_CBOR_CONTAINER_ITEMS {
                return Err(invalid_cbor_shape("CBOR map exceeds Maple's item bound"));
            }
            for _ in 0..argument {
                parse_cbor_item(payload, offset, depth + 1)?;
                parse_cbor_item(payload, offset, depth + 1)?;
            }
            Ok(())
        }
        6 => parse_cbor_item(payload, offset, depth + 1),
        7 => match additional {
            0..=23 => Ok(()),
            24 => Ok(()), // one argument byte was consumed above
            25..=27 => Ok(()),
            _ => Err(invalid_cbor_shape("unsupported CBOR simple value")),
        },
        _ => Err(invalid_cbor_shape("unsupported CBOR major type")),
    }
}

fn read_cbor_argument(
    payload: &[u8],
    offset: &mut usize,
    additional: u8,
) -> Result<u64, ProtocolError> {
    let width = match additional {
        value @ 0..=23 => return Ok(u64::from(value)),
        24 => 1,
        25 => 2,
        26 => 4,
        27 => 8,
        _ => return Err(invalid_cbor_shape("reserved CBOR argument")),
    };
    let end = offset
        .checked_add(width)
        .ok_or_else(|| invalid_cbor_shape("CBOR argument overflow"))?;
    let bytes = payload
        .get(*offset..end)
        .ok_or_else(|| invalid_cbor_shape("CBOR argument is truncated"))?;
    *offset = end;
    Ok(bytes
        .iter()
        .fold(0_u64, |value, byte| (value << 8) | u64::from(*byte)))
}

fn invalid_cbor_shape(message: &str) -> ProtocolError {
    ProtocolError::new(ErrorCode::InvalidFrame, message, false)
}

async fn read_exact_cancel_safe(
    recv: &mut iroh::endpoint::RecvStream,
    mut remaining: &mut [u8],
    deadline: tokio::time::Instant,
) -> Result<(), ProtocolError> {
    while !remaining.is_empty() {
        let read = tokio::time::timeout_at(deadline, recv.read(remaining))
            .await
            .map_err(|_| {
                let _ = recv.stop(iroh::endpoint::VarInt::from_u32(0x4d_54));
                ProtocolError::new(
                    ErrorCode::TransportUnavailable,
                    "Maple frame read deadline elapsed",
                    true,
                )
            })?
            .map_err(|error| transport_error("failed to read Maple frame", error, true))?;
        let Some(read) = read else {
            return Err(ProtocolError::new(
                ErrorCode::InvalidFrame,
                "Maple frame ended before its declared length",
                false,
            ));
        };
        let (_, tail) = remaining.split_at_mut(read);
        remaining = tail;
    }
    Ok(())
}

async fn read_exact_unbounded(
    recv: &mut iroh::endpoint::RecvStream,
    mut remaining: &mut [u8],
) -> Result<(), ProtocolError> {
    while !remaining.is_empty() {
        let read = recv
            .read(remaining)
            .await
            .map_err(|error| transport_error("failed to read Maple Events frame", error, true))?;
        let Some(read) = read else {
            return Err(ProtocolError::new(
                ErrorCode::InvalidFrame,
                "Maple Events frame ended before its declared length",
                false,
            ));
        };
        if read == 0 {
            return Err(ProtocolError::new(
                ErrorCode::TransportUnavailable,
                "Maple Events frame made no read progress",
                true,
            ));
        }
        let (_, tail) = remaining.split_at_mut(read);
        remaining = tail;
    }
    Ok(())
}

/// Read a length prefix without allocating its payload. Used by tests and by
/// callers that want an explicit preflight check.
pub fn validate_wire_length_prefix(prefix: [u8; 4]) -> Result<u32, ProtocolError> {
    let len = u32::from_be_bytes(prefix);
    if len > MAX_FRAME_BYTES {
        return Err(ProtocolError::new(
            ErrorCode::FrameTooLarge,
            format!("frame exceeds {MAX_FRAME_BYTES} bytes"),
            false,
        ));
    }
    Ok(len)
}

fn transport_error(context: &str, error: impl std::fmt::Display, retryable: bool) -> ProtocolError {
    // Iroh error strings can contain IP/relay addresses. Do not persist them
    // in application logs; only emit a stable context and consume the detail.
    let _ = error;
    log::warn!("{context}");
    ProtocolError::new(ErrorCode::TransportUnavailable, context, retryable)
}

fn internal_state_error() -> ProtocolError {
    ProtocolError::new(
        ErrorCode::Internal,
        "remote peer admission state is unavailable",
        false,
    )
}

fn operation_timeout(message: &str) -> ProtocolError {
    ProtocolError::new(ErrorCode::TransportUnavailable, message, true)
}

fn identity_endpoint_id(identity: &DeviceIdentity) -> Result<iroh::EndpointId, ProtocolError> {
    identity.public_id().parse().map_err(|_| {
        ProtocolError::new(
            ErrorCode::SecureStorageUnavailable,
            "device identity contains an invalid Iroh endpoint ID",
            false,
        )
    })
}

#[cfg(test)]
mod transport_tests {
    use super::*;
    use crate::{
        remote_protocol::{
            Page, PageItem, PageRequest, RequestEnvelope, ResponseEnvelope, WireBody,
        },
        secure_storage::{testing::InMemorySecretStore, DeviceSecretSlot},
    };
    use serde::ser::SerializeSeq;
    use std::net::{IpAddr, Ipv4Addr, SocketAddr};

    const TEST_TIMEOUT: Duration = Duration::from_secs(5);

    #[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
    #[serde(deny_unknown_fields)]
    struct SyntheticPageItem {
        value: String,
    }

    impl WireBody for SyntheticPageItem {
        fn stream_kind(&self) -> StreamKind {
            StreamKind::Bulk
        }

        fn validate_body(&self) -> Result<(), ProtocolError> {
            validate_bootstrap_id("page item value", &self.value)
        }
    }

    impl PageItem for SyntheticPageItem {}

    #[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
    #[serde(deny_unknown_fields)]
    struct SyntheticEventsRequest {
        stream_id: String,
    }

    impl WireBody for SyntheticEventsRequest {
        fn stream_kind(&self) -> StreamKind {
            StreamKind::Events
        }

        fn validate_body(&self) -> Result<(), ProtocolError> {
            validate_bootstrap_id("synthetic Events stream id", &self.stream_id)
        }
    }

    impl RequestBody for SyntheticEventsRequest {
        fn allowed_direction(&self) -> PeerDirection {
            PeerDirection::ControllerToHost
        }
    }

    #[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
    #[serde(deny_unknown_fields)]
    struct SyntheticEventsFrame {
        stream_id: String,
        sequence: u64,
    }

    impl WireBody for SyntheticEventsFrame {
        fn stream_kind(&self) -> StreamKind {
            StreamKind::Events
        }

        fn validate_body(&self) -> Result<(), ProtocolError> {
            validate_bootstrap_id("synthetic Events stream id", &self.stream_id)?;
            if self.sequence == 0 {
                return Err(ProtocolError::new(
                    ErrorCode::InvalidFrame,
                    "synthetic Events sequence must be positive",
                    false,
                ));
            }
            Ok(())
        }
    }

    impl ResponseBody<SyntheticEventsRequest> for SyntheticEventsFrame {
        fn validate_response_to(
            &self,
            request: &SyntheticEventsRequest,
        ) -> Result<(), ProtocolError> {
            if self.stream_id != request.stream_id {
                return Err(ProtocolError::new(
                    ErrorCode::InvalidFrame,
                    "synthetic Events response belongs to another stream",
                    false,
                ));
            }
            Ok(())
        }
    }

    fn identity(install: &str) -> DeviceIdentity {
        let store = InMemorySecretStore::default();
        let slot = DeviceSecretSlot::new("cloud.opensecret.maple.test", install, 1).unwrap();
        DeviceIdentity::load_or_create(&store, &slot).unwrap()
    }

    fn endpoint_id(identity: &DeviceIdentity) -> iroh::EndpointId {
        identity.public_id().parse().unwrap()
    }

    fn epoch(value: u64) -> HostConnectionClock {
        HostConnectionClock::new(HostEpoch::new(value).unwrap())
    }

    #[test]
    fn durable_runtime_clock_uses_the_native_identity_lineage_and_advances_on_restart() {
        let store = InMemorySecretStore::default();
        let slot =
            DeviceSecretSlot::new("cloud.opensecret.maple.test", "durable-clock", 1).unwrap();

        let first = DurableHostRuntime::load_and_reserve_for_test(&store, &slot).unwrap();
        assert_eq!(
            first.host_clock().allocate().unwrap(),
            ConnectionStamp::new(1, 1).unwrap()
        );
        assert_eq!(
            first.host_clock().allocate().unwrap(),
            ConnectionStamp::new(1, 2).unwrap()
        );

        let restarted = DurableHostRuntime::load_and_reserve_for_test(&store, &slot).unwrap();
        assert_eq!(
            restarted.host_clock().allocate().unwrap(),
            ConnectionStamp::new(2, 1).unwrap()
        );
    }

    fn test_incarnation() -> PairingIncarnation {
        PairingIncarnation::new(1).unwrap()
    }

    fn test_pairing_fence() -> PairingFence {
        PairingFence::new(test_incarnation()).unwrap()
    }

    fn paired(
        peers: impl IntoIterator<Item = iroh::EndpointId>,
    ) -> HashMap<iroh::EndpointId, PairingIncarnation> {
        peers
            .into_iter()
            .map(|peer| (peer, test_incarnation()))
            .collect()
    }

    async fn within<T>(operation: impl Future<Output = T>, context: &'static str) -> T {
        tokio::time::timeout(TEST_TIMEOUT, operation)
            .await
            .unwrap_or_else(|_| panic!("{context} timed out"))
    }

    async fn bind_direct_endpoint(
        identity: &DeviceIdentity,
        target_id: &str,
        host_clock: HostConnectionClock,
    ) -> MapleIrohEndpoint {
        within(
            MapleIrohEndpoint::bind_direct(identity, target_id, host_clock),
            "direct endpoint bind",
        )
        .await
        .unwrap()
    }

    async fn wait_for_cached(endpoint: &MapleIrohEndpoint) -> CachedEndpointAddr {
        within(
            async {
                loop {
                    if let Ok(cached) = endpoint.cached_endpoint_addr(endpoint.endpoint_addr()) {
                        return cached;
                    }
                    tokio::time::sleep(Duration::from_millis(10)).await;
                }
            },
            "local endpoint address publication",
        )
        .await
    }

    async fn connect_pair(
        controller: &MapleIrohEndpoint,
        host: &MapleIrohEndpoint,
        cached_host: &CachedEndpointAddr,
        host_id: iroh::EndpointId,
        request_id: &str,
        target_id: &str,
    ) -> (ConnectedPeer, ConnectedPeer) {
        let (client, server) = within(
            async {
                tokio::join!(
                    controller.connect_cached(cached_host, host_id, request_id, target_id),
                    host.accept_authenticated(),
                )
            },
            "bootstrapped Iroh connection",
        )
        .await;
        (client.unwrap(), server.unwrap())
    }

    async fn connect_pair_managed(
        controller: &MapleIrohEndpoint,
        host: &MapleIrohEndpoint,
        manager: &GenerationConnectionManager,
        cached_host: &CachedEndpointAddr,
        host_id: iroh::EndpointId,
        request_id: &str,
        target_id: &str,
    ) -> (ConnectedPeer, ConnectedPeer) {
        let (client, server) = within(
            async {
                tokio::join!(
                    controller.connect_and_install_cached(
                        manager,
                        cached_host,
                        host_id,
                        request_id,
                        target_id,
                    ),
                    host.accept_authenticated(),
                )
            },
            "managed bootstrapped Iroh connection",
        )
        .await;
        (client.unwrap(), server.unwrap())
    }

    struct HandoverFixture {
        controller: MapleIrohEndpoint,
        host: MapleIrohEndpoint,
        manager: GenerationConnectionManager,
        cached_host: CachedEndpointAddr,
        controller_id: iroh::EndpointId,
        host_id: iroh::EndpointId,
        target_id: String,
        current_client: ConnectedPeer,
        current_server: ConnectedPeer,
    }

    async fn handover_fixture(label: &str, host_epoch: u64) -> HandoverFixture {
        let controller_identity = identity(&format!("{label}-controller"));
        let host_identity = identity(&format!("{label}-host"));
        let controller_id = endpoint_id(&controller_identity);
        let host_id = endpoint_id(&host_identity);
        let target_id = format!("{label}-target");
        let controller = bind_direct_endpoint(
            &controller_identity,
            &format!("{label}-controller-install"),
            epoch(host_epoch + 1),
        )
        .await;
        let host = bind_direct_endpoint(&host_identity, &target_id, epoch(host_epoch)).await;
        controller
            .authorize_outgoing_execution_target(host_id)
            .unwrap();
        host.authorize_incoming_controller(controller_id).unwrap();
        let cached_host = wait_for_cached(&host).await;
        let manager = GenerationConnectionManager::new_for_pairing(
            controller_id,
            host_id,
            target_id.clone(),
            test_pairing_fence(),
            None,
        )
        .unwrap();
        let (current_client, current_server) = connect_pair_managed(
            &controller,
            &host,
            &manager,
            &cached_host,
            host_id,
            &format!("{label}-initial"),
            &target_id,
        )
        .await;
        HandoverFixture {
            controller,
            host,
            manager,
            cached_host,
            controller_id,
            host_id,
            target_id,
            current_client,
            current_server,
        }
    }

    struct RawReadyHandover {
        connection: iroh::endpoint::Connection,
        send: iroh::endpoint::SendStream,
        recv: iroh::endpoint::RecvStream,
        request_id: String,
        target_id: String,
        controller_id: iroh::EndpointId,
        pairing_fence: PairingFence,
        connection_stamp: ConnectionStamp,
        previous_connection_stamp: Option<ConnectionStamp>,
    }

    impl RawReadyHandover {
        fn pending(&self, fixture: &HandoverFixture) -> PendingCommit {
            PendingCommit::new(
                fixture.manager.pairing_fence,
                &self.request_id,
                &self.target_id,
                self.controller_id,
                fixture.host_id,
                self.previous_connection_stamp,
                self.connection_stamp,
            )
            .unwrap()
        }

        fn installed(&self) -> BootstrapInstalled {
            BootstrapInstalled {
                protocol_version: PROTOCOL_VERSION,
                request_id: self.request_id.clone(),
                execution_target_id: self.target_id.clone(),
                controller_id: self.controller_id.to_string(),
                pairing_fence: self.pairing_fence,
                connection_stamp: self.connection_stamp,
                previous_connection_stamp: self.previous_connection_stamp,
            }
        }

        fn observed(&self) -> BootstrapCommitObserved {
            BootstrapCommitObserved {
                protocol_version: PROTOCOL_VERSION,
                request_id: self.request_id.clone(),
                execution_target_id: self.target_id.clone(),
                controller_id: self.controller_id.to_string(),
                pairing_fence: self.pairing_fence,
                connection_stamp: self.connection_stamp,
                previous_connection_stamp: self.previous_connection_stamp,
            }
        }

        async fn install_and_expect_committed(&mut self) {
            let deadline = tokio::time::Instant::now() + TEST_TIMEOUT;
            let installed = self.installed();
            write_frame_until(&mut self.send, &installed, deadline)
                .await
                .unwrap();
            let committed: BootstrapCommitted =
                read_frame_until(&mut self.recv, deadline).await.unwrap();
            committed
                .validate(
                    &self.request_id,
                    &self.target_id,
                    self.controller_id,
                    self.pairing_fence,
                    self.connection_stamp,
                    self.previous_connection_stamp,
                )
                .unwrap();
        }

        async fn expect_finalized(&mut self) {
            let deadline = tokio::time::Instant::now() + TEST_TIMEOUT;
            let finalized: BootstrapFinalized =
                read_frame_until(&mut self.recv, deadline).await.unwrap();
            finalized
                .validate(
                    &self.request_id,
                    &self.target_id,
                    self.controller_id,
                    self.pairing_fence,
                    self.connection_stamp,
                    self.previous_connection_stamp,
                )
                .unwrap();
            expect_stream_end(&mut self.recv, deadline).await.unwrap();
        }
    }

    async fn open_raw_handover_to_ready(
        fixture: &HandoverFixture,
        request_id: &str,
    ) -> RawReadyHandover {
        let deadline = tokio::time::Instant::now() + TEST_TIMEOUT;
        let connection = tokio::time::timeout_at(
            deadline,
            fixture
                .controller
                .endpoint
                .connect(fixture.cached_host.as_iroh().clone(), ALPN),
        )
        .await
        .unwrap()
        .unwrap();
        fixture.controller.admission.register(&connection).unwrap();
        let (mut send, mut recv) = tokio::time::timeout_at(deadline, connection.open_bi())
            .await
            .unwrap()
            .unwrap();
        send.set_priority(StreamKind::Control.priority()).unwrap();
        let previous_connection_stamp = Some(fixture.current_client.connection_stamp());
        let request = BootstrapRequest {
            protocol_version: PROTOCOL_VERSION,
            request_id: request_id.into(),
            execution_target_id: fixture.target_id.clone(),
            bootstrap_generation: 0,
            pairing_fence: fixture.manager.pairing_fence,
            previous_connection_stamp,
            reconciliation: None,
        };
        write_frame_until(&mut send, &request, deadline)
            .await
            .unwrap();
        let response: BootstrapResponse = read_frame_until(&mut recv, deadline).await.unwrap();
        let connection_stamp = response
            .validate(
                request_id,
                &fixture.target_id,
                fixture.manager.pairing_fence,
            )
            .unwrap();
        let ready: BootstrapReady = read_frame_until(&mut recv, deadline).await.unwrap();
        ready
            .validate(
                request_id,
                &fixture.target_id,
                fixture.controller_id,
                fixture.manager.pairing_fence,
                connection_stamp,
                previous_connection_stamp,
            )
            .unwrap();
        RawReadyHandover {
            connection,
            send,
            recv,
            request_id: request_id.into(),
            target_id: fixture.target_id.clone(),
            controller_id: fixture.controller_id,
            pairing_fence: fixture.manager.pairing_fence,
            connection_stamp,
            previous_connection_stamp,
        }
    }

    async fn wait_for_handover_rollback(fixture: &HandoverFixture) {
        within(
            async {
                loop {
                    let activating = fixture
                        .host
                        .admission
                        .state
                        .read()
                        .unwrap()
                        .incoming_controllers
                        .activating
                        .contains_key(&fixture.controller_id);
                    let pending = fixture
                        .host
                        .accepted_connections
                        .state
                        .lock()
                        .unwrap()
                        .pending
                        .contains_key(&fixture.controller_id);
                    if !activating && !pending {
                        return;
                    }
                    tokio::task::yield_now().await;
                }
            },
            "handover rollback",
        )
        .await;
    }

    async fn close_handover_fixture(fixture: HandoverFixture) {
        fixture.manager.clear().unwrap();
        within(
            async { tokio::join!(fixture.controller.close(), fixture.host.close()) },
            "handover fixture close",
        )
        .await;
    }

    async fn assert_page_roundtrip(
        client: &ConnectedPeer,
        server: &ConnectedPeer,
        request_id: &str,
    ) {
        let request = RequestEnvelope {
            protocol_version: PROTOCOL_VERSION,
            request_id: request_id.into(),
            execution_target_id: client.execution_target_id().into(),
            direction: PeerDirection::ControllerToHost,
            connection_stamp: client.connection_stamp(),
            body: PageRequest::default(),
        };
        let (client_result, server_result) = within(
            async {
                tokio::join!(
                    async {
                        let response: ResponseEnvelope<Page<SyntheticPageItem>> =
                            client.request(&request).await?;
                        let page = response.result?;
                        assert_eq!(
                            page.items,
                            vec![SyntheticPageItem {
                                value: "synthetic-item".into()
                            }]
                        );
                        Ok::<_, ProtocolError>(())
                    },
                    async {
                        let mut accepted = server.accept_stream().await?;
                        assert_eq!(accepted.header().stream_kind, StreamKind::Bulk);
                        assert_eq!(
                            accepted.send_stream().priority().unwrap(),
                            StreamKind::Bulk.priority()
                        );
                        let mut received: AcceptedRequest<PageRequest> =
                            accepted.read_request().await?;
                        assert_eq!(received.request().request_id, request_id);
                        assert_eq!(
                            received.request().connection_stamp,
                            server.connection_stamp()
                        );
                        assert_eq!(
                            received.send_stream().priority().unwrap(),
                            StreamKind::Bulk.priority()
                        );
                        let received_request = received.request();
                        let response = ResponseEnvelope {
                            protocol_version: PROTOCOL_VERSION,
                            request_id: received_request.request_id.clone(),
                            execution_target_id: received_request.execution_target_id.clone(),
                            connection_stamp: received_request.connection_stamp,
                            result: Ok(Page {
                                items: vec![SyntheticPageItem {
                                    value: "synthetic-item".into(),
                                }],
                                next_cursor: None,
                            }),
                        };
                        received.write_response(&response).await?;
                        Ok::<_, ProtocolError>(())
                    }
                )
            },
            "typed page roundtrip",
        )
        .await;
        client_result.unwrap();
        server_result.unwrap();
    }

    async fn bind_raw_endpoint(identity: &DeviceIdentity, alpns: Vec<Vec<u8>>) -> iroh::Endpoint {
        let builder = iroh::Endpoint::builder(iroh::endpoint::presets::Minimal)
            .clear_address_lookup()
            .relay_mode(iroh::RelayMode::Disabled)
            .secret_key(identity.iroh_secret_key().unwrap())
            .alpns(alpns)
            .clear_ip_transports()
            .bind_addr_with_opts(
                "127.0.0.1:0",
                iroh::endpoint::BindOpts::default().set_prefix_len(8),
            )
            .unwrap();
        within(builder.bind(), "raw endpoint bind").await.unwrap()
    }

    #[tokio::test]
    async fn bootstrapped_page_roundtrip_enforces_bulk_lane() {
        let controller_identity = identity("roundtrip-controller");
        let host_identity = identity("roundtrip-host");
        let controller_id = endpoint_id(&controller_identity);
        let host_id = endpoint_id(&host_identity);
        let controller =
            bind_direct_endpoint(&controller_identity, "controller-install", epoch(90)).await;
        let host = bind_direct_endpoint(&host_identity, "host-install", epoch(41)).await;
        controller
            .authorize_outgoing_execution_target(host_id)
            .unwrap();
        host.authorize_incoming_controller(controller_id).unwrap();
        let cached = wait_for_cached(&host).await;
        let (client, server) = connect_pair(
            &controller,
            &host,
            &cached,
            host_id,
            "bootstrap-roundtrip",
            "host-install",
        )
        .await;

        assert_eq!(client.connection_stamp(), server.connection_stamp());
        assert_eq!(client.connection_stamp().host_epoch(), 41);
        assert_eq!(client.connection_stamp().generation(), 1);
        assert_page_roundtrip(&client, &server, "page-roundtrip").await;

        let mislabeled = RequestEnvelope {
            protocol_version: PROTOCOL_VERSION,
            request_id: "mislabeled-page".into(),
            execution_target_id: "host-install".into(),
            direction: PeerDirection::ControllerToHost,
            connection_stamp: client.connection_stamp(),
            body: PageRequest::default(),
        };
        let (client_result, server_result) = within(
            async {
                tokio::join!(
                    async {
                        let (mut send, _recv) = client.raw_connection().open_bi().await.unwrap();
                        send.set_priority(StreamKind::Control.priority()).unwrap();
                        write_frame_bounded(
                            &mut send,
                            &StreamHeader {
                                protocol_version: PROTOCOL_VERSION,
                                stream_kind: StreamKind::Control,
                                direction: PeerDirection::ControllerToHost,
                                connection_stamp: client.connection_stamp(),
                            },
                            Duration::from_secs(1),
                        )
                        .await
                        .unwrap();
                        write_frame_bounded(&mut send, &mislabeled, Duration::from_secs(1))
                            .await
                            .unwrap();
                        send.finish().unwrap();
                    },
                    async {
                        let mut accepted = server.accept_stream().await.unwrap();
                        assert_eq!(accepted.header().stream_kind, StreamKind::Control);
                        assert_eq!(
                            accepted.send_stream().priority().unwrap(),
                            StreamKind::Bulk.priority()
                        );
                        let error = accepted.read_request::<PageRequest>().await.unwrap_err();
                        assert_eq!(error.code, ErrorCode::InvalidFrame);
                    }
                )
            },
            "mislabeled bulk request",
        )
        .await;
        let _ = (client_result, server_result);

        within(
            async { tokio::join!(controller.close(), host.close()) },
            "roundtrip endpoint close",
        )
        .await;
    }

    #[tokio::test]
    async fn events_stream_clears_lifetime_deadlines_and_bounds_each_write() {
        let controller_identity = identity("events-liveness-controller");
        let host_identity = identity("events-liveness-host");
        let controller_id = endpoint_id(&controller_identity);
        let host_id = endpoint_id(&host_identity);
        let controller = bind_direct_endpoint(
            &controller_identity,
            "events-liveness-controller-install",
            epoch(93),
        )
        .await;
        let host =
            bind_direct_endpoint(&host_identity, "events-liveness-host-install", epoch(53)).await;
        controller
            .authorize_outgoing_execution_target(host_id)
            .unwrap();
        host.authorize_incoming_controller(controller_id).unwrap();
        let cached = wait_for_cached(&host).await;
        let (client, server) = connect_pair(
            &controller,
            &host,
            &cached,
            host_id,
            "events-liveness-bootstrap",
            "events-liveness-host-install",
        )
        .await;
        let request = RequestEnvelope {
            protocol_version: PROTOCOL_VERSION,
            request_id: "events-liveness-request".into(),
            execution_target_id: client.execution_target_id().into(),
            direction: PeerDirection::ControllerToHost,
            connection_stamp: client.connection_stamp(),
            body: SyntheticEventsRequest {
                stream_id: "events-liveness-stream".into(),
            },
        };

        let (client_result, server_result) = within(
            async {
                tokio::join!(
                    async {
                        let mut response = client.start_streaming_request(request).await?;
                        assert_eq!(response.stream_kind, StreamKind::Events);
                        assert!(response.operation_deadline.is_none());
                        let frame: ResponseEnvelope<SyntheticEventsFrame> = response.read().await?;
                        assert_eq!(
                            frame.result?,
                            SyntheticEventsFrame {
                                stream_id: "events-liveness-stream".into(),
                                sequence: 1,
                            }
                        );
                        response.finish().await
                    },
                    async {
                        let accepted = server.accept_stream().await?;
                        assert_eq!(accepted.header().stream_kind, StreamKind::Events);
                        assert!(accepted.operation_deadline.is_some());
                        let mut request: AcceptedRequest<SyntheticEventsRequest> =
                            accepted.read_request().await?;
                        assert!(request.stream.operation_deadline.is_none());
                        assert_eq!(
                            request.stream.send.priority().unwrap(),
                            StreamKind::Events.priority()
                        );
                        let envelope = request.request();
                        let response = ResponseEnvelope {
                            protocol_version: PROTOCOL_VERSION,
                            request_id: envelope.request_id.clone(),
                            execution_target_id: envelope.execution_target_id.clone(),
                            connection_stamp: envelope.connection_stamp,
                            result: Ok(SyntheticEventsFrame {
                                stream_id: envelope.body.stream_id.clone(),
                                sequence: 1,
                            }),
                        };
                        request.write_response_frame(&response).await?;
                        request.finish_response()
                    }
                )
            },
            "Events lifetime/write deadline roundtrip",
        )
        .await;
        client_result.unwrap();
        server_result.unwrap();

        within(
            async { tokio::join!(controller.close(), host.close()) },
            "Events liveness endpoint close",
        )
        .await;
    }

    #[tokio::test]
    async fn application_stream_deadlines_recover_for_next_typed_request() {
        let controller_identity = identity("application-timeout-controller");
        let host_identity = identity("application-timeout-host");
        let controller_id = endpoint_id(&controller_identity);
        let host_id = endpoint_id(&host_identity);
        let controller = bind_direct_endpoint(
            &controller_identity,
            "application-timeout-controller-install",
            epoch(92),
        )
        .await;
        let host = bind_direct_endpoint(
            &host_identity,
            "application-timeout-host-install",
            epoch(52),
        )
        .await;
        controller
            .authorize_outgoing_execution_target(host_id)
            .unwrap();
        host.authorize_incoming_controller(controller_id).unwrap();
        let cached = wait_for_cached(&host).await;
        let (client, server) = connect_pair(
            &controller,
            &host,
            &cached,
            host_id,
            "application-timeout-bootstrap",
            "application-timeout-host-install",
        )
        .await;

        let error = within(
            server.accept_stream(),
            "silent application stream accept deadline",
        )
        .await
        .unwrap_err();
        assert_eq!(error.code, ErrorCode::TransportUnavailable);

        let (mut silent_header, _recv) =
            within(client.raw_connection().open_bi(), "silent header stream")
                .await
                .unwrap();
        // Sending only the length prefix makes the QUIC stream observable while
        // leaving the header payload silent until the bounded frame deadline.
        silent_header
            .write_all(&32_u32.to_be_bytes())
            .await
            .unwrap();
        let error = within(server.accept_stream(), "silent application header deadline")
            .await
            .unwrap_err();
        assert_eq!(error.code, ErrorCode::TransportUnavailable);
        drop(silent_header);

        let (mut partial_body, _recv) =
            within(client.raw_connection().open_bi(), "partial body stream")
                .await
                .unwrap();
        write_frame_bounded(
            &mut partial_body,
            &StreamHeader {
                protocol_version: PROTOCOL_VERSION,
                stream_kind: StreamKind::Bulk,
                direction: PeerDirection::ControllerToHost,
                connection_stamp: client.connection_stamp(),
            },
            Duration::from_secs(1),
        )
        .await
        .unwrap();
        partial_body.write_all(&32_u32.to_be_bytes()).await.unwrap();
        partial_body.write_all(&[0xa1]).await.unwrap();
        let accepted = within(server.accept_stream(), "valid application header")
            .await
            .unwrap();
        let error = within(
            accepted.read_request::<PageRequest>(),
            "partial application body deadline",
        )
        .await
        .unwrap_err();
        assert_eq!(error.code, ErrorCode::TransportUnavailable);
        drop(partial_body);

        let (mut trailing_body, _recv) =
            within(client.raw_connection().open_bi(), "trailing body stream")
                .await
                .unwrap();
        write_frame_bounded(
            &mut trailing_body,
            &StreamHeader {
                protocol_version: PROTOCOL_VERSION,
                stream_kind: StreamKind::Bulk,
                direction: PeerDirection::ControllerToHost,
                connection_stamp: client.connection_stamp(),
            },
            Duration::from_secs(1),
        )
        .await
        .unwrap();
        let request = RequestEnvelope {
            protocol_version: PROTOCOL_VERSION,
            request_id: "trailing-body".into(),
            execution_target_id: "application-timeout-host-install".into(),
            direction: PeerDirection::ControllerToHost,
            connection_stamp: client.connection_stamp(),
            body: PageRequest::default(),
        };
        let mut encoded = encode_frame_bounded(&request).unwrap();
        encoded.push(0xf6);
        trailing_body
            .write_all(&(encoded.len() as u32).to_be_bytes())
            .await
            .unwrap();
        trailing_body.write_all(&encoded).await.unwrap();
        let accepted = within(server.accept_stream(), "trailing body header")
            .await
            .unwrap();
        assert_eq!(
            accepted
                .read_request::<PageRequest>()
                .await
                .unwrap_err()
                .code,
            ErrorCode::InvalidFrame
        );

        assert_page_roundtrip(&client, &server, "page-after-application-timeouts").await;
        within(
            async { tokio::join!(controller.close(), host.close()) },
            "application timeout endpoint close",
        )
        .await;
    }

    #[tokio::test]
    async fn host_stamps_advance_and_manager_rejects_floor_then_supersedes() {
        let controller_identity = identity("stamp-controller");
        let host_identity = identity("stamp-host");
        let controller_id = endpoint_id(&controller_identity);
        let host_id = endpoint_id(&host_identity);
        let controller =
            bind_direct_endpoint(&controller_identity, "stamp-controller-install", epoch(91)).await;
        let host = bind_direct_endpoint(&host_identity, "stamp-host-install", epoch(77)).await;
        controller
            .authorize_outgoing_execution_target(host_id)
            .unwrap();
        host.authorize_incoming_controller(controller_id).unwrap();
        let cached = wait_for_cached(&host).await;
        let manager = GenerationConnectionManager::new_for_pairing(
            controller_id,
            host_id,
            "stamp-host-install",
            test_pairing_fence(),
            Some(ConnectionStamp::new(77, 1).unwrap()),
        )
        .unwrap();

        assert_eq!(
            within(
                controller.connect_and_install_cached(
                    &manager,
                    &cached,
                    host_id,
                    "stamp-bootstrap-1",
                    "stamp-host-install",
                ),
                "floor-rejected managed handover",
            )
            .await
            .unwrap_err()
            .code,
            ErrorCode::StaleGeneration
        );

        let (second_client, second_server) = connect_pair_managed(
            &controller,
            &host,
            &manager,
            &cached,
            host_id,
            "stamp-bootstrap-2",
            "stamp-host-install",
        )
        .await;
        assert_eq!(
            second_client.connection_stamp(),
            second_server.connection_stamp()
        );
        assert_eq!(second_client.connection_stamp().host_epoch(), 77);
        assert_eq!(second_client.connection_stamp().generation(), 2);
        let current = second_client;

        let (third_client, third_server) = connect_pair_managed(
            &controller,
            &host,
            &manager,
            &cached,
            host_id,
            "stamp-bootstrap-3",
            "stamp-host-install",
        )
        .await;
        assert_eq!(
            third_client.connection_stamp(),
            third_server.connection_stamp()
        );
        assert_eq!(third_client.connection_stamp().generation(), 3);
        let newest = third_client;
        assert_eq!(newest.connection_stamp().generation(), 3);
        within(
            current.raw_connection().closed(),
            "manager superseded close",
        )
        .await;
        within(
            second_server.raw_connection().closed(),
            "host sequential supersession close",
        )
        .await;
        manager.clear().unwrap();
        within(
            third_server.raw_connection().closed(),
            "manager clear close",
        )
        .await;

        within(
            async { tokio::join!(controller.close(), host.close()) },
            "stamp endpoint close",
        )
        .await;
    }

    #[tokio::test]
    async fn installed_ack_loss_keeps_previous_generation_routable() {
        let fixture = handover_fixture("ack-loss", 301).await;
        let mut handover = open_raw_handover_to_ready(&fixture, "ack-loss-b").await;

        // Ready alone cannot gate A. Exercise an actual typed request while B
        // is pending and has no application dispatcher.
        assert_page_roundtrip(
            &fixture.current_client,
            &fixture.current_server,
            "ack-loss-a-still-routable",
        )
        .await;
        handover
            .send
            .reset(iroh::endpoint::VarInt::from_u32(0x4d_41))
            .unwrap();
        let _ = handover
            .recv
            .stop(iroh::endpoint::VarInt::from_u32(0x4d_41));
        wait_for_handover_rollback(&fixture).await;

        let current = fixture.manager.current().unwrap().unwrap();
        assert_eq!(
            current.connection_stamp(),
            fixture.current_client.connection_stamp()
        );
        assert!(fixture
            .current_client
            .raw_connection()
            .close_reason()
            .is_none());
        close_handover_fixture(fixture).await;
    }

    #[tokio::test]
    async fn wrong_or_stale_installed_ack_rolls_back_to_previous_generation() {
        let fixture = handover_fixture("wrong-ack", 311).await;
        let mut handover = open_raw_handover_to_ready(&fixture, "wrong-ack-b").await;
        let mut wrong = handover.installed();
        wrong.previous_connection_stamp = None;
        let deadline = tokio::time::Instant::now() + TEST_TIMEOUT;
        write_frame_until(&mut handover.send, &wrong, deadline)
            .await
            .unwrap();
        within(handover.connection.closed(), "wrong acknowledgment close").await;
        wait_for_handover_rollback(&fixture).await;

        assert_eq!(
            fixture
                .manager
                .current()
                .unwrap()
                .unwrap()
                .connection_stamp(),
            fixture.current_client.connection_stamp()
        );
        assert_page_roundtrip(
            &fixture.current_client,
            &fixture.current_server,
            "wrong-ack-a-still-routable",
        )
        .await;
        close_handover_fixture(fixture).await;
    }

    #[tokio::test]
    async fn candidate_death_before_installed_ack_preserves_previous_generation() {
        let fixture = handover_fixture("candidate-death", 321).await;
        let handover = open_raw_handover_to_ready(&fixture, "candidate-death-b").await;
        handover.connection.close(
            iroh::endpoint::VarInt::from_u32(0x4d_42),
            b"test candidate died before Installed",
        );
        wait_for_handover_rollback(&fixture).await;

        assert!(fixture
            .current_client
            .raw_connection()
            .close_reason()
            .is_none());
        assert_page_roundtrip(
            &fixture.current_client,
            &fixture.current_server,
            "candidate-death-a-still-routable",
        )
        .await;
        close_handover_fixture(fixture).await;
    }

    #[tokio::test]
    async fn simultaneous_handover_candidate_cannot_nest_over_pending_candidate() {
        let fixture = handover_fixture("simultaneous", 331).await;
        let mut pending_b = open_raw_handover_to_ready(&fixture, "simultaneous-b").await;

        let competing = within(
            fixture.controller.connect_and_install_cached(
                &fixture.manager,
                &fixture.cached_host,
                fixture.host_id,
                "simultaneous-c",
                &fixture.target_id,
            ),
            "competing handover rejection",
        )
        .await;
        assert!(competing.is_err());
        assert_eq!(
            fixture
                .manager
                .current()
                .unwrap()
                .unwrap()
                .connection_stamp(),
            fixture.current_client.connection_stamp()
        );

        pending_b
            .send
            .reset(iroh::endpoint::VarInt::from_u32(0x4d_43))
            .unwrap();
        wait_for_handover_rollback(&fixture).await;
        assert_page_roundtrip(
            &fixture.current_client,
            &fixture.current_server,
            "simultaneous-a-still-routable",
        )
        .await;
        close_handover_fixture(fixture).await;
    }

    #[tokio::test]
    async fn successful_handover_retires_a_only_after_b_is_finalized() {
        let fixture = handover_fixture("successful-handover", 341).await;
        let a_client = fixture.current_client.clone();
        let a_server = fixture.current_server.clone();
        let (b_client, b_server) = connect_pair_managed(
            &fixture.controller,
            &fixture.host,
            &fixture.manager,
            &fixture.cached_host,
            fixture.host_id,
            "successful-handover-b",
            &fixture.target_id,
        )
        .await;

        assert!(b_client.connection_stamp() > a_client.connection_stamp());
        assert_eq!(b_client.connection_stamp(), b_server.connection_stamp());
        within(a_client.wait_closed(), "retired controller A").await;
        within(a_server.wait_closed(), "retired host A").await;
        assert_eq!(
            fixture
                .manager
                .current()
                .unwrap()
                .unwrap()
                .connection_stamp(),
            b_client.connection_stamp()
        );
        assert_page_roundtrip(&b_client, &b_server, "successful-handover-b-routable").await;
        close_handover_fixture(fixture).await;
    }

    #[tokio::test]
    async fn controller_restart_restores_committed_lineage_without_live_handle() {
        let fixture = handover_fixture("controller-lineage-restart", 346).await;
        let a_stamp = fixture.current_client.connection_stamp();
        let HandoverFixture {
            controller,
            host,
            manager,
            cached_host,
            controller_id,
            host_id,
            target_id,
            current_client,
            current_server,
        } = fixture;
        let snapshot = manager.capture_lineage().unwrap();
        assert_eq!(snapshot.last_committed(), Some(a_stamp));
        assert!(!snapshot.requires_reconciliation());
        within(
            current_client.wait_closed(),
            "captured controller lineage closes A",
        )
        .await;
        within(
            current_server.wait_closed(),
            "host observes captured A close",
        )
        .await;
        let restored = GenerationConnectionManager::restore_for_pairing(
            snapshot,
            controller_id,
            host_id,
            target_id.clone(),
            test_pairing_fence(),
        )
        .unwrap();
        let (b_client, b_server) = connect_pair_managed(
            &controller,
            &host,
            &restored,
            &cached_host,
            host_id,
            "controller-lineage-restart-b",
            &target_id,
        )
        .await;
        assert!(b_client.connection_stamp() > a_stamp);
        assert_eq!(b_client.connection_stamp(), b_server.connection_stamp());
        restored.clear().unwrap();
        within(
            async { tokio::join!(controller.close(), host.close()) },
            "controller lineage restart close",
        )
        .await;
    }

    #[tokio::test]
    async fn lost_finalized_survives_controller_restart_and_reconciles_before_next_dial() {
        let fixture = handover_fixture("lost-finalized-reconcile", 347).await;
        let mut handover = open_raw_handover_to_ready(&fixture, "lost-finalized-reconcile-b").await;
        let b_stamp = handover.connection_stamp;
        let candidate = ConnectedPeer::new(
            handover.connection.clone(),
            b_stamp,
            fixture.manager.pairing_fence,
            Arc::from(fixture.target_id.as_str()),
            PeerDirection::ControllerToHost,
            fixture.controller.connection_policy.frame_deadline,
        );
        let prepared = fixture
            .manager
            .begin_handover(candidate, handover.pending(&fixture))
            .unwrap();
        handover.install_and_expect_committed().await;
        let promoted = prepared.promote().unwrap();
        let awaiting = promoted.mark_observed_sent().unwrap();
        let deadline = tokio::time::Instant::now() + TEST_TIMEOUT;
        let observed = handover.observed();
        write_frame_until(&mut handover.send, &observed, deadline)
            .await
            .unwrap();
        handover.send.finish().unwrap();
        within(
            async {
                loop {
                    if fixture
                        .host
                        .admission
                        .state
                        .read()
                        .unwrap()
                        .incoming_controllers
                        .committed_lineage
                        .get(&fixture.controller_id)
                        == Some(&b_stamp)
                    {
                        break;
                    }
                    tokio::task::yield_now().await;
                }
            },
            "host commits B before Finalized loss",
        )
        .await;
        // Lose the correlated Finalized frame and every B/A handle. The exact
        // PendingCommit, not liveness, is the only safe reconstruction input.
        handover.connection.close(
            iroh::endpoint::VarInt::from_u32(0x4d_4b),
            b"test drops Finalized response",
        );
        drop(awaiting);
        drop(handover);

        let HandoverFixture {
            controller,
            host,
            manager,
            cached_host,
            controller_id,
            host_id,
            target_id,
            current_client,
            current_server,
        } = fixture;
        let snapshot = manager.capture_lineage().unwrap();
        assert!(snapshot.requires_reconciliation());
        assert_eq!(
            snapshot.last_committed(),
            Some(current_client.connection_stamp())
        );
        let restored = GenerationConnectionManager::restore_for_pairing(
            snapshot,
            controller_id,
            host_id,
            target_id.clone(),
            test_pairing_fence(),
        )
        .unwrap();
        let (c_client, first_dequeued) = within(
            async {
                tokio::join!(
                    controller.connect_and_install_cached(
                        &restored,
                        &cached_host,
                        host_id,
                        "lost-finalized-reconcile-c",
                        &target_id,
                    ),
                    host.accept_authenticated(),
                )
            },
            "reconcile then install C",
        )
        .await;
        let c_client = c_client.unwrap();
        let first_dequeued = first_dequeued.unwrap();
        let c_server = if first_dequeued.connection_stamp() == c_client.connection_stamp() {
            first_dequeued
        } else {
            // B was legitimately routable while C's handover was in progress.
            // Once C finalizes, B closes and the next dequeue is C.
            within(first_dequeued.wait_closed(), "reconciled B retires after C").await;
            within(host.accept_authenticated(), "dequeue C after reconciled B")
                .await
                .unwrap()
        };
        assert!(c_client.connection_stamp() > b_stamp);
        assert_eq!(c_client.connection_stamp(), c_server.connection_stamp());
        assert!(!restored.pending_reconciliation().unwrap().is_some());
        drop((current_client, current_server));
        restored.clear().unwrap();
        within(
            async { tokio::join!(controller.close(), host.close()) },
            "lost Finalized reconciliation close",
        )
        .await;
    }

    #[tokio::test]
    async fn validated_finalized_frame_commits_before_missing_eof() {
        let fixture = handover_fixture("finalized-before-eof", 348).await;
        let mut handover = open_raw_handover_to_ready(&fixture, "finalized-before-eof-b").await;
        let candidate = ConnectedPeer::new(
            handover.connection.clone(),
            handover.connection_stamp,
            fixture.manager.pairing_fence,
            Arc::from(fixture.target_id.as_str()),
            PeerDirection::ControllerToHost,
            fixture.controller.connection_policy.frame_deadline,
        );
        let prepared = fixture
            .manager
            .begin_handover(candidate, handover.pending(&fixture))
            .unwrap();
        handover.install_and_expect_committed().await;
        let awaiting = prepared.promote().unwrap().mark_observed_sent().unwrap();

        let deadline = tokio::time::Instant::now() + TEST_TIMEOUT;
        let observed = handover.observed();
        write_frame_until(&mut handover.send, &observed, deadline)
            .await
            .unwrap();
        handover.send.finish().unwrap();
        let received: BootstrapFinalized = read_frame_until(&mut handover.recv, deadline)
            .await
            .unwrap();
        received
            .validate(
                &handover.request_id,
                &handover.target_id,
                handover.controller_id,
                handover.pairing_fence,
                handover.connection_stamp,
                handover.previous_connection_stamp,
            )
            .unwrap();
        let _ = awaiting.finalize();
        assert_eq!(
            fixture.manager.current_stamp().unwrap(),
            Some(handover.connection_stamp)
        );
        // Locally lose the trailing FIN after the correlated frame. This is a
        // framing/transport failure only; B was already selected.
        handover
            .recv
            .stop(iroh::endpoint::VarInt::from_u32(0x4d_4c))
            .unwrap();
        let _ = expect_stream_end(&mut handover.recv, deadline).await;
        assert_eq!(
            fixture.manager.current_stamp().unwrap(),
            Some(handover.connection_stamp)
        );
        close_handover_fixture(fixture).await;
    }

    #[tokio::test]
    async fn controller_missing_a_handle_reconnects_from_committed_lineage_immediately() {
        let fixture = handover_fixture("controller-asymmetric-loss", 351).await;
        let a_stamp = fixture.current_client.connection_stamp();
        {
            let mut state = fixture.manager.state.lock().unwrap();
            let forgotten = state.current.take().expect("initial controller handle");
            drop(forgotten);
            assert_eq!(state.last_committed, Some(a_stamp));
        }
        assert_eq!(fixture.manager.current_stamp().unwrap(), Some(a_stamp));

        let (b_client, b_server) = connect_pair_managed(
            &fixture.controller,
            &fixture.host,
            &fixture.manager,
            &fixture.cached_host,
            fixture.host_id,
            "controller-asymmetric-loss-b",
            &fixture.target_id,
        )
        .await;
        assert!(b_client.connection_stamp() > a_stamp);
        assert_eq!(b_client.connection_stamp(), b_server.connection_stamp());
        assert_eq!(
            fixture.manager.current_stamp().unwrap(),
            Some(b_client.connection_stamp())
        );
        close_handover_fixture(fixture).await;
    }

    #[tokio::test]
    async fn host_missing_a_handle_reconnects_from_committed_lineage_immediately() {
        let fixture = handover_fixture("host-asymmetric-loss", 361).await;
        let a_stamp = fixture.current_server.connection_stamp();
        {
            let mut state = fixture.host.admission.state.write().unwrap();
            let incoming = &mut state.incoming_controllers;
            assert_eq!(
                incoming.committed_lineage.get(&fixture.controller_id),
                Some(&a_stamp)
            );
            incoming.current.remove(&fixture.controller_id);
        }

        let (b_client, b_server) = connect_pair_managed(
            &fixture.controller,
            &fixture.host,
            &fixture.manager,
            &fixture.cached_host,
            fixture.host_id,
            "host-asymmetric-loss-b",
            &fixture.target_id,
        )
        .await;
        assert!(b_server.connection_stamp() > a_stamp);
        assert_eq!(b_client.connection_stamp(), b_server.connection_stamp());
        assert_eq!(
            fixture
                .host
                .admission
                .state
                .read()
                .unwrap()
                .incoming_controllers
                .committed_lineage
                .get(&fixture.controller_id),
            Some(&b_server.connection_stamp())
        );
        close_handover_fixture(fixture).await;
    }

    #[tokio::test]
    async fn revoke_and_explicit_repairing_reset_host_lineage_to_genesis() {
        let fixture = handover_fixture("lineage-repairing", 366).await;
        let a_stamp = fixture.current_server.connection_stamp();
        assert_eq!(
            fixture
                .host
                .admission
                .state
                .read()
                .unwrap()
                .incoming_controllers
                .committed_lineage
                .get(&fixture.controller_id),
            Some(&a_stamp)
        );
        assert!(fixture
            .host
            .revoke_incoming_controller(&fixture.controller_id)
            .unwrap());
        fixture
            .host
            .authorize_incoming_controller(fixture.controller_id)
            .unwrap();
        assert!(!fixture
            .host
            .admission
            .state
            .read()
            .unwrap()
            .incoming_controllers
            .committed_lineage
            .contains_key(&fixture.controller_id));

        // The old manager still names A and is fenced by the host's explicit
        // revoke/re-pair. A fresh manager starts the new lineage with None.
        let stale = within(
            fixture.controller.connect_and_install_cached(
                &fixture.manager,
                &fixture.cached_host,
                fixture.host_id,
                "lineage-repairing-stale-a",
                &fixture.target_id,
            ),
            "old lineage rejected after repairing",
        )
        .await
        .unwrap_err();
        assert!(matches!(
            stale.code,
            ErrorCode::StaleGeneration | ErrorCode::TransportUnavailable
        ));
        let repaired_manager = GenerationConnectionManager::new_for_pairing(
            fixture.controller_id,
            fixture.host_id,
            fixture.target_id.clone(),
            test_pairing_fence(),
            None,
        )
        .unwrap();
        let (new_client, new_server) = connect_pair_managed(
            &fixture.controller,
            &fixture.host,
            &repaired_manager,
            &fixture.cached_host,
            fixture.host_id,
            "lineage-repairing-genesis",
            &fixture.target_id,
        )
        .await;
        assert_eq!(new_client.connection_stamp(), new_server.connection_stamp());
        repaired_manager.clear().unwrap();
        fixture.manager.clear().unwrap();
        within(
            async { tokio::join!(fixture.controller.close(), fixture.host.close()) },
            "lineage repairing fixture close",
        )
        .await;
    }

    #[tokio::test]
    async fn host_finalizes_b_when_a_dies_after_observed_before_eof() {
        let fixture = handover_fixture("a-dies-after-observed", 371).await;
        let mut handover = open_raw_handover_to_ready(&fixture, "a-dies-after-observed-b").await;
        handover.install_and_expect_committed().await;
        let deadline = tokio::time::Instant::now() + TEST_TIMEOUT;
        let observed = handover.observed();
        write_frame_until(&mut handover.send, &observed, deadline)
            .await
            .unwrap();

        // The host has the Observed frame but cannot finalize until the
        // controller closes its send half. Lose A in that exact window.
        fixture.current_server.raw_connection().close(
            iroh::endpoint::VarInt::from_u32(0x4d_48),
            b"test loses A after Observed",
        );
        handover.send.finish().unwrap();
        handover.expect_finalized().await;
        let b_server = within(
            fixture.host.accept_authenticated(),
            "host accepts B after A dies",
        )
        .await
        .unwrap();
        assert_eq!(b_server.connection_stamp(), handover.connection_stamp);
        assert_eq!(
            fixture
                .host
                .admission
                .state
                .read()
                .unwrap()
                .incoming_controllers
                .committed_lineage
                .get(&fixture.controller_id),
            Some(&handover.connection_stamp)
        );
        close_handover_fixture(fixture).await;
    }

    #[tokio::test]
    async fn observed_write_ambiguity_never_resolves_from_handle_liveness() {
        let fixture = handover_fixture("observed-ambiguity", 381).await;
        let a_stamp = fixture.current_client.connection_stamp();
        {
            let mut state = fixture.manager.state.lock().unwrap();
            drop(state.current.take());
            assert_eq!(state.last_committed, Some(a_stamp));
        }
        let mut handover = open_raw_handover_to_ready(&fixture, "observed-ambiguity-b").await;
        let candidate = ConnectedPeer::new(
            handover.connection.clone(),
            handover.connection_stamp,
            fixture.manager.pairing_fence,
            Arc::from(fixture.target_id.as_str()),
            PeerDirection::ControllerToHost,
            fixture.controller.connection_policy.frame_deadline,
        );
        let prepared = fixture
            .manager
            .begin_handover(candidate, handover.pending(&fixture))
            .unwrap();
        handover.install_and_expect_committed().await;
        let promoted = prepared.promote().unwrap();
        let token = promoted.token.as_ref().unwrap().clone();
        assert_eq!(
            fixture.manager.finalize_awaiting(&token).unwrap_err().code,
            ErrorCode::StaleGeneration
        );
        let awaiting = promoted.mark_observed_sent().unwrap();

        // Simulate a write which never reaches the host. B is live and the
        // controller has no A handle, but neither fact proves host commit.
        handover
            .send
            .reset(iroh::endpoint::VarInt::from_u32(0x4d_49))
            .unwrap();
        drop(awaiting);
        assert_eq!(fixture.manager.current_stamp().unwrap(), Some(a_stamp));
        assert_eq!(
            fixture.manager.current().unwrap_err().code,
            ErrorCode::TransportUnavailable
        );
        assert!(matches!(
            fixture.manager.state.lock().unwrap().handover,
            Some(ManagedHandover::AwaitingFinalized { .. })
        ));
        wait_for_handover_rollback(&fixture).await;
        assert_eq!(fixture.manager.current_stamp().unwrap(), Some(a_stamp));
        assert!(matches!(
            fixture.manager.state.lock().unwrap().handover,
            Some(ManagedHandover::AwaitingFinalized { .. })
        ));
        close_handover_fixture(fixture).await;
    }

    #[tokio::test]
    async fn finalized_b_lineage_survives_b_dying_before_local_finalize() {
        let fixture = handover_fixture("b-dies-after-finalized", 391).await;
        let a_stamp = fixture.current_client.connection_stamp();
        let mut handover = open_raw_handover_to_ready(&fixture, "b-dies-after-finalized-b").await;
        let candidate = ConnectedPeer::new(
            handover.connection.clone(),
            handover.connection_stamp,
            fixture.manager.pairing_fence,
            Arc::from(fixture.target_id.as_str()),
            PeerDirection::ControllerToHost,
            fixture.controller.connection_policy.frame_deadline,
        );
        let prepared = fixture
            .manager
            .begin_handover(candidate, handover.pending(&fixture))
            .unwrap();
        handover.install_and_expect_committed().await;
        let promoted = prepared.promote().unwrap();
        let awaiting = promoted.mark_observed_sent().unwrap();
        let deadline = tokio::time::Instant::now() + TEST_TIMEOUT;
        let observed = handover.observed();
        write_frame_until(&mut handover.send, &observed, deadline)
            .await
            .unwrap();
        handover.send.finish().unwrap();
        handover.expect_finalized().await;

        handover.connection.close(
            iroh::endpoint::VarInt::from_u32(0x4d_4a),
            b"test loses B after Finalized",
        );
        assert_eq!(
            awaiting.finalize().unwrap_err().code,
            ErrorCode::TransportUnavailable
        );
        assert_eq!(
            fixture.manager.current_stamp().unwrap(),
            Some(handover.connection_stamp)
        );
        assert!(fixture.manager.current().unwrap().is_none());
        assert_ne!(fixture.manager.current_stamp().unwrap(), Some(a_stamp));
        close_handover_fixture(fixture).await;
    }

    #[tokio::test]
    async fn managed_reconnect_survives_quiescent_host_endpoint_rebuild() {
        let controller_identity = identity("lineage-rebuild-controller");
        let host_identity = identity("lineage-rebuild-host");
        let controller_id = endpoint_id(&controller_identity);
        let host_id = endpoint_id(&host_identity);
        let target_id = "lineage-rebuild-target";
        let pairing_incarnation = PairingIncarnation::new(7).unwrap();
        let clock = epoch(401);

        let controller = bind_direct_endpoint(
            &controller_identity,
            "lineage-rebuild-controller-install",
            epoch(402),
        )
        .await;
        controller
            .replace_authorizations(AuthorizationSnapshot {
                account_epoch: 17,
                snapshot_revision: 1,
                incoming_controllers: HashMap::new(),
                outgoing_execution_targets: HashMap::from([(host_id, pairing_incarnation)]),
            })
            .unwrap();
        let first_host = bind_direct_endpoint(&host_identity, target_id, clock.clone()).await;
        first_host
            .replace_authorizations(AuthorizationSnapshot {
                account_epoch: 17,
                snapshot_revision: 1,
                incoming_controllers: HashMap::from([(controller_id, pairing_incarnation)]),
                outgoing_execution_targets: HashMap::new(),
            })
            .unwrap();
        let first_cached = wait_for_cached(&first_host).await;
        let manager = GenerationConnectionManager::new_for_pairing(
            controller_id,
            host_id,
            target_id,
            PairingFence::new(pairing_incarnation).unwrap(),
            None,
        )
        .unwrap();
        let (first_client, first_server) = connect_pair_managed(
            &controller,
            &first_host,
            &manager,
            &first_cached,
            host_id,
            "lineage-rebuild-first",
            target_id,
        )
        .await;
        let first_stamp = first_client.connection_stamp();

        let mut handoff = first_host.begin_lineage_handoff().unwrap();
        assert_eq!(handoff.snapshot().unwrap().local_endpoint(), host_id);
        assert_eq!(handoff.snapshot().unwrap().execution_target_id(), target_id);
        assert_eq!(handoff.snapshot().unwrap().account_epoch(), 17);
        assert_eq!(handoff.snapshot().unwrap().incoming_controller_count(), 1);
        within(handoff.ensure_source_closed(), "quiescent host close")
            .await
            .unwrap();
        within(first_client.wait_closed(), "captured controller A close").await;
        within(first_server.wait_closed(), "captured host A close").await;

        // An unrelated current authorization revision retains the same pair
        // incarnation and therefore the same committed generation lineage.
        let second_host = within(
            MapleIrohEndpoint::bind_direct_restoring_lineage(
                &host_identity,
                target_id,
                clock,
                AuthorizationSnapshot {
                    account_epoch: 17,
                    snapshot_revision: 9,
                    incoming_controllers: HashMap::from([(controller_id, pairing_incarnation)]),
                    outgoing_execution_targets: HashMap::new(),
                },
                &mut handoff,
            ),
            "host endpoint lineage restore",
        )
        .await
        .unwrap();
        assert!(handoff.is_consumed());
        assert_eq!(
            second_host
                .admission
                .state
                .read()
                .unwrap()
                .incoming_controllers
                .committed_lineage
                .get(&controller_id),
            Some(&first_stamp)
        );
        assert_eq!(
            second_host
                .admission
                .state
                .read()
                .unwrap()
                .snapshot_revision,
            9
        );

        let second_cached = wait_for_cached(&second_host).await;
        let (second_client, second_server) = connect_pair_managed(
            &controller,
            &second_host,
            &manager,
            &second_cached,
            host_id,
            "lineage-rebuild-second",
            target_id,
        )
        .await;
        assert!(second_client.connection_stamp() > first_stamp);
        assert_eq!(
            second_client.connection_stamp(),
            second_server.connection_stamp()
        );
        manager.clear().unwrap();
        within(
            async { tokio::join!(controller.close(), second_host.close()) },
            "lineage rebuilt endpoints close",
        )
        .await;
    }

    #[tokio::test]
    async fn host_lineage_restore_is_fenced_by_pairing_incarnation() {
        let controller_identity = identity("lineage-incarnation-controller");
        let host_identity = identity("lineage-incarnation-host");
        let controller_id = endpoint_id(&controller_identity);
        let host_id = endpoint_id(&host_identity);
        let target_id = "lineage-incarnation-target";
        let retained_incarnation = PairingIncarnation::new(3).unwrap();
        let repaired_incarnation = PairingIncarnation::new(4).unwrap();
        let retained_stamp = ConnectionStamp::new(411, 12).unwrap();
        let retained_transition = PendingCommit::new(
            PairingFence::new(retained_incarnation).unwrap(),
            "lineage-incarnation-commit",
            target_id,
            controller_id,
            host_id,
            Some(ConnectionStamp::new(411, 11).unwrap()),
            retained_stamp,
        )
        .unwrap();
        let lineage_fixture = || {
            let incoming_authorization = HashMap::from([(controller_id, retained_incarnation)]);
            EndpointLineageSnapshot {
                local_endpoint: host_id,
                execution_target_id: Arc::from(target_id),
                account_epoch: 21,
                snapshot_revision: 3,
                authorization_digest: authorization_snapshot_digest(
                    21,
                    3,
                    &incoming_authorization,
                    &HashMap::new(),
                ),
                incoming_controllers: HashMap::from([(
                    controller_id,
                    IncomingControllerLineage {
                        pairing_incarnation: retained_incarnation,
                        last_committed: Some(retained_stamp),
                        finalized_transition: Some(retained_transition.clone()),
                    },
                )]),
            }
        };

        let mut repaired_handoff = EndpointLineageHandoff::from_snapshot_fixture(lineage_fixture());

        let repaired_host = within(
            MapleIrohEndpoint::bind_direct_restoring_lineage(
                &host_identity,
                target_id,
                epoch(411),
                AuthorizationSnapshot {
                    account_epoch: 21,
                    snapshot_revision: 44,
                    incoming_controllers: HashMap::from([(controller_id, repaired_incarnation)]),
                    outgoing_execution_targets: HashMap::new(),
                },
                &mut repaired_handoff,
            ),
            "repaired-incarnation host bind",
        )
        .await
        .unwrap();
        assert!(!repaired_host
            .admission
            .state
            .read()
            .unwrap()
            .incoming_controllers
            .committed_lineage
            .contains_key(&controller_id));
        within(repaired_host.close(), "repaired-incarnation host close").await;

        let mut wrong_account_handoff =
            EndpointLineageHandoff::from_snapshot_fixture(lineage_fixture());
        let wrong_account = within(
            MapleIrohEndpoint::bind_direct_restoring_lineage(
                &host_identity,
                target_id,
                epoch(411),
                AuthorizationSnapshot {
                    account_epoch: 22,
                    snapshot_revision: 1,
                    incoming_controllers: HashMap::from([(controller_id, retained_incarnation)]),
                    outgoing_execution_targets: HashMap::new(),
                },
                &mut wrong_account_handoff,
            ),
            "wrong-account host lineage rejection",
        )
        .await
        .unwrap_err();
        assert_eq!(wrong_account.code, ErrorCode::Unauthorized);
    }

    #[test]
    fn bootstrap_pairing_fence_is_bound_to_every_normal_phase() {
        let controller = endpoint_id(&identity("phase-fence-controller"));
        let expected = PairingFence::new(PairingIncarnation::new(7).unwrap()).unwrap();
        let wrong = PairingFence::new(PairingIncarnation::new(8).unwrap()).unwrap();
        let stamp = ConnectionStamp::new(421, 2).unwrap();
        let request = BootstrapRequest {
            protocol_version: PROTOCOL_VERSION,
            request_id: "phase-fence".into(),
            execution_target_id: "phase-fence-target".into(),
            bootstrap_generation: 0,
            pairing_fence: wrong,
            previous_connection_stamp: None,
            reconciliation: None,
        };
        assert_eq!(
            request
                .validate("phase-fence-target", expected)
                .unwrap_err()
                .code,
            ErrorCode::Unauthorized
        );
        let response = BootstrapResponse {
            protocol_version: PROTOCOL_VERSION,
            request_id: "phase-fence".into(),
            execution_target_id: "phase-fence-target".into(),
            pairing_fence: wrong,
            result: Ok(BootstrapAccepted {
                connection_stamp: stamp,
            }),
        };
        assert_eq!(
            response
                .validate("phase-fence", "phase-fence-target", expected)
                .unwrap_err()
                .code,
            ErrorCode::Unauthorized
        );
        let ready = BootstrapReady {
            protocol_version: PROTOCOL_VERSION,
            request_id: "phase-fence".into(),
            execution_target_id: "phase-fence-target".into(),
            controller_id: controller.to_string(),
            pairing_fence: wrong,
            connection_stamp: stamp,
            previous_connection_stamp: None,
        };
        assert_eq!(
            ready
                .validate(
                    "phase-fence",
                    "phase-fence-target",
                    controller,
                    expected,
                    stamp,
                    None,
                )
                .unwrap_err()
                .code,
            ErrorCode::Unauthorized
        );

        macro_rules! assert_phase_fence_rejected {
            ($frame:expr) => {{
                assert_eq!(
                    $frame
                        .validate(
                            "phase-fence",
                            "phase-fence-target",
                            controller,
                            expected,
                            stamp,
                            None,
                        )
                        .unwrap_err()
                        .code,
                    ErrorCode::Unauthorized
                );
            }};
        }
        assert_phase_fence_rejected!(BootstrapInstalled {
            protocol_version: PROTOCOL_VERSION,
            request_id: "phase-fence".into(),
            execution_target_id: "phase-fence-target".into(),
            controller_id: controller.to_string(),
            pairing_fence: wrong,
            connection_stamp: stamp,
            previous_connection_stamp: None,
        });
        assert_phase_fence_rejected!(BootstrapCommitted {
            protocol_version: PROTOCOL_VERSION,
            request_id: "phase-fence".into(),
            execution_target_id: "phase-fence-target".into(),
            controller_id: controller.to_string(),
            pairing_fence: wrong,
            connection_stamp: stamp,
            previous_connection_stamp: None,
        });
        assert_phase_fence_rejected!(BootstrapCommitObserved {
            protocol_version: PROTOCOL_VERSION,
            request_id: "phase-fence".into(),
            execution_target_id: "phase-fence-target".into(),
            controller_id: controller.to_string(),
            pairing_fence: wrong,
            connection_stamp: stamp,
            previous_connection_stamp: None,
        });
        assert_phase_fence_rejected!(BootstrapFinalized {
            protocol_version: PROTOCOL_VERSION,
            request_id: "phase-fence".into(),
            execution_target_id: "phase-fence-target".into(),
            controller_id: controller.to_string(),
            pairing_fence: wrong,
            connection_stamp: stamp,
            previous_connection_stamp: None,
        });
    }

    #[tokio::test]
    async fn async_pairing_incarnation_mismatch_cannot_stage_host_lineage() {
        let controller_identity = identity("async-fence-controller");
        let host_identity = identity("async-fence-host");
        let controller_id = endpoint_id(&controller_identity);
        let host_id = endpoint_id(&host_identity);
        let target_id = "async-fence-target";
        let old_incarnation = PairingIncarnation::new(9).unwrap();
        let repaired_incarnation = PairingIncarnation::new(10).unwrap();
        let controller =
            bind_direct_endpoint(&controller_identity, "async-fence-client", epoch(431)).await;
        let host = bind_direct_endpoint(&host_identity, target_id, epoch(432)).await;
        controller
            .replace_authorizations(AuthorizationSnapshot {
                account_epoch: 41,
                snapshot_revision: 2,
                incoming_controllers: HashMap::new(),
                outgoing_execution_targets: HashMap::from([(host_id, repaired_incarnation)]),
            })
            .unwrap();
        host.replace_authorizations(AuthorizationSnapshot {
            account_epoch: 41,
            snapshot_revision: 1,
            incoming_controllers: HashMap::from([(controller_id, old_incarnation)]),
            outgoing_execution_targets: HashMap::new(),
        })
        .unwrap();
        let manager = GenerationConnectionManager::new_for_pairing(
            controller_id,
            host_id,
            target_id,
            PairingFence::new(repaired_incarnation).unwrap(),
            None,
        )
        .unwrap();
        let cached = wait_for_cached(&host).await;
        let mismatch = within(
            controller.connect_and_install_cached(
                &manager,
                &cached,
                host_id,
                "async-fence-bootstrap",
                target_id,
            ),
            "async pairing-fence rejection",
        )
        .await
        .unwrap_err();
        assert_eq!(mismatch.code, ErrorCode::Unauthorized);
        assert!(!mismatch.retryable);
        let state = host.admission.state.read().unwrap();
        assert!(state.incoming_controllers.committed_lineage.is_empty());
        assert!(state.incoming_controllers.activating.is_empty());
        drop(state);
        assert_eq!(manager.current_stamp().unwrap(), None);
        within(
            async { tokio::join!(controller.close(), host.close()) },
            "async pairing-fence endpoints close",
        )
        .await;
    }

    #[tokio::test]
    async fn independent_local_account_epochs_share_incarnation_and_repair_fences_dispatch() {
        let controller_identity = identity("local-epoch-controller");
        let host_identity = identity("local-epoch-host");
        let controller_id = endpoint_id(&controller_identity);
        let host_id = endpoint_id(&host_identity);
        let target_id = "local-epoch-target";
        let old_incarnation = PairingIncarnation::new(61).unwrap();
        let new_incarnation = PairingIncarnation::new(62).unwrap();
        let old_fence = PairingFence::new(old_incarnation).unwrap();
        let new_fence = PairingFence::new(new_incarnation).unwrap();
        let controller =
            bind_direct_endpoint(&controller_identity, "local-epoch-controller", epoch(433)).await;
        let host = bind_direct_endpoint(&host_identity, target_id, epoch(434)).await;

        // Account epochs are installation-local. The shared directed pairing
        // incarnation is the only authorization lineage placed on the wire.
        controller
            .replace_authorizations(AuthorizationSnapshot {
                account_epoch: 7,
                snapshot_revision: 1,
                incoming_controllers: HashMap::new(),
                outgoing_execution_targets: HashMap::from([(host_id, old_incarnation)]),
            })
            .unwrap();
        host.replace_authorizations(AuthorizationSnapshot {
            account_epoch: 41,
            snapshot_revision: 1,
            incoming_controllers: HashMap::from([(controller_id, old_incarnation)]),
            outgoing_execution_targets: HashMap::new(),
        })
        .unwrap();
        let cached = wait_for_cached(&host).await;
        let old_manager = GenerationConnectionManager::new_for_pairing(
            controller_id,
            host_id,
            target_id,
            old_fence,
            None,
        )
        .unwrap();
        let (old_client, old_server) = connect_pair_managed(
            &controller,
            &host,
            &old_manager,
            &cached,
            host_id,
            "local-epoch-old",
            target_id,
        )
        .await;
        assert_eq!(old_client.pairing_fence(), old_fence);
        assert_eq!(old_server.pairing_fence(), old_fence);
        host.validate_current_incoming_peer(&old_server).unwrap();

        // Hold a fully classified old-incarnation stream at the adapter
        // boundary, then revoke and re-pair the same EndpointId. Revalidation
        // must reject it before its request body can be dispatched.
        let old_request = RequestEnvelope {
            protocol_version: PROTOCOL_VERSION,
            request_id: "old-incarnation-prepared".into(),
            execution_target_id: target_id.into(),
            direction: PeerDirection::ControllerToHost,
            connection_stamp: old_client.connection_stamp(),
            body: PageRequest::default(),
        };
        let old_request_client = old_client.clone();
        let old_request_task = tokio::spawn(async move {
            old_request_client
                .request::<PageRequest, Page<SyntheticPageItem>>(&old_request)
                .await
        });
        let old_prepared = within(
            old_server.accept_stream(),
            "old-incarnation prepared stream",
        )
        .await
        .unwrap();

        controller
            .replace_authorizations(AuthorizationSnapshot {
                account_epoch: 7,
                snapshot_revision: 2,
                incoming_controllers: HashMap::new(),
                outgoing_execution_targets: HashMap::from([(host_id, new_incarnation)]),
            })
            .unwrap();
        host.replace_authorizations(AuthorizationSnapshot {
            account_epoch: 41,
            snapshot_revision: 2,
            incoming_controllers: HashMap::from([(controller_id, new_incarnation)]),
            outgoing_execution_targets: HashMap::new(),
        })
        .unwrap();
        let revoked = host
            .validate_current_incoming_peer(&old_server)
            .unwrap_err();
        assert_eq!(revoked.code, ErrorCode::Revoked);
        assert!(!revoked.retryable);
        drop(old_prepared);
        assert!(within(old_request_task, "old-incarnation request stops")
            .await
            .unwrap()
            .is_err());

        let stale_manager = within(
            controller.connect_and_install_cached(
                &old_manager,
                &cached,
                host_id,
                "local-epoch-stale-incarnation",
                target_id,
            ),
            "old-incarnation manager rejection",
        )
        .await
        .unwrap_err();
        assert_eq!(stale_manager.code, ErrorCode::Unauthorized);
        assert!(!stale_manager.retryable);
        old_manager.clear().unwrap();

        let new_manager = GenerationConnectionManager::new_for_pairing(
            controller_id,
            host_id,
            target_id,
            new_fence,
            None,
        )
        .unwrap();
        let (fresh_client, fresh_server) = connect_pair_managed(
            &controller,
            &host,
            &new_manager,
            &cached,
            host_id,
            "local-epoch-fresh-incarnation",
            target_id,
        )
        .await;
        assert_eq!(fresh_server.pairing_fence(), new_fence);
        host.validate_current_incoming_peer(&fresh_server).unwrap();
        assert_page_roundtrip(&fresh_client, &fresh_server, "local-epoch-fresh-dispatch").await;

        // A host-local account transition remains an independent hard fence:
        // it clears grants, live generations, activations, and lineage without
        // requiring the controller's local epoch to match.
        host.replace_authorizations(AuthorizationSnapshot {
            account_epoch: 42,
            snapshot_revision: 1,
            incoming_controllers: HashMap::new(),
            outgoing_execution_targets: HashMap::new(),
        })
        .unwrap();
        within(
            fresh_server.wait_closed(),
            "host-local account switch closes generation",
        )
        .await;
        assert_eq!(
            host.validate_current_incoming_peer(&fresh_server)
                .unwrap_err()
                .code,
            ErrorCode::Revoked
        );
        {
            let state = host.admission.state.read().unwrap();
            assert!(state.incoming_controllers.allowed.is_empty());
            assert!(state.incoming_controllers.active.is_empty());
            assert!(state.incoming_controllers.current.is_empty());
            assert!(state.incoming_controllers.activating.is_empty());
            assert!(state.incoming_controllers.committed_lineage.is_empty());
            assert!(state.incoming_controllers.finalized_transitions.is_empty());
        }

        new_manager.clear().unwrap();
        within(
            async { tokio::join!(controller.close(), host.close()) },
            "local-epoch endpoints close",
        )
        .await;
    }

    #[tokio::test]
    async fn newer_local_account_terminally_fences_lineage_handoff_before_preflight() {
        let host_identity = identity("account-advance-handoff-host");
        let controller_id = endpoint_id(&identity("account-advance-handoff-controller"));
        let target_id = "account-advance-handoff-target";
        let incarnation = PairingIncarnation::new(63).unwrap();
        let host = bind_direct_endpoint(&host_identity, target_id, epoch(435)).await;
        let current = AuthorizationSnapshot {
            account_epoch: 51,
            snapshot_revision: 5,
            incoming_controllers: HashMap::from([(controller_id, incarnation)]),
            outgoing_execution_targets: HashMap::new(),
        };
        host.replace_authorizations(current.clone()).unwrap();
        let mut handoff = host.begin_lineage_handoff().unwrap();

        let lower = MapleIrohEndpoint::bind_direct_restoring_lineage(
            &host_identity,
            target_id,
            epoch(435),
            AuthorizationSnapshot {
                account_epoch: 50,
                snapshot_revision: 99,
                incoming_controllers: HashMap::from([(controller_id, incarnation)]),
                outgoing_execution_targets: HashMap::new(),
            },
            &mut handoff,
        )
        .await
        .unwrap_err();
        assert_eq!(lower.code, ErrorCode::Revoked);
        assert!(!handoff.is_consumed());

        // The intentionally invalid target proves observing a newer local
        // account is the constructor's first operation, before ID/key/bind
        // preflight can fail and accidentally preserve old-account authority.
        let advanced = MapleIrohEndpoint::bind_direct_restoring_lineage(
            &host_identity,
            "invalid target with spaces",
            epoch(435),
            AuthorizationSnapshot {
                account_epoch: 52,
                snapshot_revision: 1,
                incoming_controllers: HashMap::new(),
                outgoing_execution_targets: HashMap::new(),
            },
            &mut handoff,
        )
        .await
        .unwrap_err();
        assert_eq!(advanced.code, ErrorCode::Unauthorized);
        assert!(!advanced.retryable);
        assert!(handoff.is_consumed());

        let old_account_retry = MapleIrohEndpoint::bind_direct_restoring_lineage(
            &host_identity,
            target_id,
            epoch(435),
            current,
            &mut handoff,
        )
        .await
        .unwrap_err();
        assert_eq!(old_account_retry.code, ErrorCode::StaleGeneration);
    }

    #[tokio::test]
    async fn host_lineage_handoff_survives_cancellation_failure_and_revision_rollback() {
        let host_identity = identity("retryable-lineage-host");
        let controller_id = endpoint_id(&identity("retryable-lineage-controller"));
        let outgoing_id = endpoint_id(&identity("retryable-lineage-outgoing"));
        let host_id = endpoint_id(&host_identity);
        let target_id = "retryable-lineage-target";
        let incarnation = PairingIncarnation::new(11).unwrap();
        let outgoing_incarnation = PairingIncarnation::new(12).unwrap();
        let authorization =
            |snapshot_revision: u64, include_incoming: bool, include_outgoing: bool| {
                AuthorizationSnapshot {
                    account_epoch: 51,
                    snapshot_revision,
                    incoming_controllers: include_incoming
                        .then(|| HashMap::from([(controller_id, incarnation)]))
                        .unwrap_or_default(),
                    outgoing_execution_targets: include_outgoing
                        .then(|| HashMap::from([(outgoing_id, outgoing_incarnation)]))
                        .unwrap_or_default(),
                }
            };
        let host = bind_direct_endpoint(&host_identity, target_id, epoch(441)).await;
        host.replace_authorizations(authorization(5, true, true))
            .unwrap();
        let mut handoff = host.begin_lineage_handoff().unwrap();
        assert_eq!(handoff.snapshot().unwrap().local_endpoint(), host_id);
        assert_eq!(
            handoff.snapshot().unwrap().authorization_revision_floor(),
            5
        );

        let close_gate = Arc::new(tokio::sync::Notify::new());
        handoff.gate_source_close(close_gate);
        assert!(
            tokio::time::timeout(Duration::from_millis(20), handoff.ensure_source_closed(),)
                .await
                .is_err()
        );
        assert!(!handoff.is_consumed());
        assert_eq!(
            handoff.snapshot().unwrap().authorization_revision_floor(),
            5
        );

        let equal_revision_incoming_fork = MapleIrohEndpoint::bind_direct_restoring_lineage(
            &host_identity,
            target_id,
            epoch(441),
            authorization(5, false, true),
            &mut handoff,
        )
        .await
        .unwrap_err();
        assert_eq!(equal_revision_incoming_fork.code, ErrorCode::Revoked);
        let equal_revision_outgoing_fork = MapleIrohEndpoint::bind_direct_restoring_lineage(
            &host_identity,
            target_id,
            epoch(441),
            authorization(5, true, false),
            &mut handoff,
        )
        .await
        .unwrap_err();
        assert_eq!(equal_revision_outgoing_fork.code, ErrorCode::Revoked);
        assert!(!handoff.is_consumed());

        let stale = MapleIrohEndpoint::bind_direct_restoring_lineage(
            &host_identity,
            target_id,
            epoch(441),
            authorization(4, true, true),
            &mut handoff,
        )
        .await
        .unwrap_err();
        assert_eq!(stale.code, ErrorCode::Revoked);
        assert!(!handoff.is_consumed());

        assert!(tokio::time::timeout(
            Duration::from_millis(20),
            MapleIrohEndpoint::bind_direct_restoring_lineage(
                &host_identity,
                target_id,
                epoch(441),
                authorization(6, true, true),
                &mut handoff,
            ),
        )
        .await
        .is_err());
        assert_eq!(handoff.authorization_floor_revision, 6);
        assert!(!handoff.is_consumed());
        handoff.ungate_source_close();

        handoff.fail_next_bind();
        let transient = MapleIrohEndpoint::bind_direct_restoring_lineage(
            &host_identity,
            target_id,
            epoch(441),
            authorization(6, true, true),
            &mut handoff,
        )
        .await
        .unwrap_err();
        assert_eq!(transient.code, ErrorCode::TransportUnavailable);
        assert!(!handoff.is_consumed());
        assert_eq!(handoff.authorization_floor_revision, 6);

        let rolled_back_after_failure = MapleIrohEndpoint::bind_direct_restoring_lineage(
            &host_identity,
            target_id,
            epoch(441),
            authorization(5, true, true),
            &mut handoff,
        )
        .await
        .unwrap_err();
        assert_eq!(rolled_back_after_failure.code, ErrorCode::Revoked);
        let forked_after_failure = MapleIrohEndpoint::bind_direct_restoring_lineage(
            &host_identity,
            target_id,
            epoch(441),
            authorization(6, false, true),
            &mut handoff,
        )
        .await
        .unwrap_err();
        assert_eq!(forked_after_failure.code, ErrorCode::Revoked);

        let rebuilt = MapleIrohEndpoint::bind_direct_restoring_lineage(
            &host_identity,
            target_id,
            epoch(441),
            authorization(6, true, true),
            &mut handoff,
        )
        .await
        .unwrap();
        assert!(handoff.is_consumed());
        let reused = MapleIrohEndpoint::bind_direct_restoring_lineage(
            &host_identity,
            target_id,
            epoch(441),
            authorization(7, true, true),
            &mut handoff,
        )
        .await
        .unwrap_err();
        assert_eq!(reused.code, ErrorCode::StaleGeneration);
        within(rebuilt.close(), "retryable lineage rebuilt host close").await;
    }

    #[tokio::test]
    async fn shared_host_clock_survives_endpoint_rebuild() {
        let controller_identity = identity("clock-rebuild-controller");
        let host_identity = identity("clock-rebuild-host");
        let controller_id = endpoint_id(&controller_identity);
        let host_id = endpoint_id(&host_identity);
        let clock = epoch(78);
        let controller = bind_direct_endpoint(
            &controller_identity,
            "clock-rebuild-controller-install",
            epoch(79),
        )
        .await;
        controller
            .authorize_outgoing_execution_target(host_id)
            .unwrap();

        let first_host =
            bind_direct_endpoint(&host_identity, "clock-rebuild-host-install", clock.clone()).await;
        first_host
            .authorize_incoming_controller(controller_id)
            .unwrap();
        let first_cached = wait_for_cached(&first_host).await;
        let (first_client, first_server) = connect_pair(
            &controller,
            &first_host,
            &first_cached,
            host_id,
            "clock-rebuild-first",
            "clock-rebuild-host-install",
        )
        .await;
        assert_eq!(first_client.connection_stamp().generation(), 1);
        within(first_host.close(), "first rebuilt endpoint close").await;
        within(
            first_client.wait_closed(),
            "first rebuilt client close signal",
        )
        .await;
        within(
            first_server.wait_closed(),
            "first rebuilt server close signal",
        )
        .await;

        let second_host =
            bind_direct_endpoint(&host_identity, "clock-rebuild-host-install", clock).await;
        second_host
            .authorize_incoming_controller(controller_id)
            .unwrap();
        let second_cached = wait_for_cached(&second_host).await;
        let (second_client, second_server) = connect_pair(
            &controller,
            &second_host,
            &second_cached,
            host_id,
            "clock-rebuild-second",
            "clock-rebuild-host-install",
        )
        .await;
        assert_eq!(second_client.connection_stamp().host_epoch(), 78);
        assert_eq!(second_client.connection_stamp().generation(), 2);
        assert_eq!(
            second_client.connection_stamp(),
            second_server.connection_stamp()
        );

        within(
            async { tokio::join!(controller.close(), second_host.close()) },
            "rebuilt endpoint final close",
        )
        .await;
    }

    #[tokio::test]
    async fn accept_skips_superseded_or_closed_queued_generations() {
        let controller_identity = identity("queued-generation-controller");
        let host_identity = identity("queued-generation-host");
        let controller_id = endpoint_id(&controller_identity);
        let host_id = endpoint_id(&host_identity);
        let controller = bind_direct_endpoint(
            &controller_identity,
            "queued-generation-controller-install",
            epoch(80),
        )
        .await;
        let host =
            bind_direct_endpoint(&host_identity, "queued-generation-host-install", epoch(81)).await;
        controller
            .authorize_outgoing_execution_target(host_id)
            .unwrap();
        host.authorize_incoming_controller(controller_id).unwrap();
        let cached = wait_for_cached(&host).await;
        let manager = GenerationConnectionManager::new_for_pairing(
            controller_id,
            host_id,
            "queued-generation-host-install",
            test_pairing_fence(),
            None,
        )
        .unwrap();

        // Let two complete handshakes queue before the router dequeues either.
        let first = within(
            controller.connect_and_install_cached(
                &manager,
                &cached,
                host_id,
                "queued-generation-first",
                "queued-generation-host-install",
            ),
            "first queued generation",
        )
        .await
        .unwrap();
        let second = within(
            controller.connect_and_install_cached(
                &manager,
                &cached,
                host_id,
                "queued-generation-second",
                "queued-generation-host-install",
            ),
            "second queued generation",
        )
        .await
        .unwrap();
        within(first.wait_closed(), "superseded generation loss signal").await;
        let accepted = within(host.accept_authenticated(), "skip superseded queued peer")
            .await
            .unwrap();
        assert_eq!(accepted.connection_stamp(), second.connection_stamp());

        let third = within(
            controller.connect_and_install_cached(
                &manager,
                &cached,
                host_id,
                "queued-generation-third",
                "queued-generation-host-install",
            ),
            "third queued generation",
        )
        .await
        .unwrap();
        within(accepted.wait_closed(), "accepted generation superseded").await;

        // A closed current generation must not remain current merely because
        // the accepted queue still owns a strong handle. Dequeue must skip it
        // and wait for the next live generation.
        third.raw_connection().close(
            iroh::endpoint::VarInt::from_u32(0),
            b"closed before host dequeue",
        );
        within(third.wait_closed(), "explicit queued close signal").await;
        within(
            async {
                while host.admission.is_current_incoming(
                    &controller_id,
                    third.connection_stamp(),
                    third.pairing_fence(),
                ) {
                    tokio::task::yield_now().await;
                }
            },
            "host observes queued connection close",
        )
        .await;
        let (fourth, dequeued) = within(
            async {
                tokio::join!(
                    controller.connect_and_install_cached(
                        &manager,
                        &cached,
                        host_id,
                        "queued-generation-fourth",
                        "queued-generation-host-install",
                    ),
                    host.accept_authenticated(),
                )
            },
            "replace closed queued generation",
        )
        .await;
        assert_eq!(
            fourth.unwrap().connection_stamp(),
            dequeued.unwrap().connection_stamp()
        );

        within(
            async { tokio::join!(controller.close(), host.close()) },
            "queued generation endpoint close",
        )
        .await;
    }

    #[tokio::test]
    async fn forward_pairing_is_one_way_and_directional_revocation_isolated() {
        let a_identity = identity("direction-device-a");
        let b_identity = identity("direction-device-b");
        let a_id = endpoint_id(&a_identity);
        let b_id = endpoint_id(&b_identity);
        let a = bind_direct_endpoint(&a_identity, "device-a", epoch(11)).await;
        let b = bind_direct_endpoint(&b_identity, "device-b", epoch(22)).await;
        a.authorize_outgoing_execution_target(b_id).unwrap();
        b.authorize_incoming_controller(a_id).unwrap();
        let cached_a = wait_for_cached(&a).await;
        let cached_b = wait_for_cached(&b).await;
        let (forward_client, forward_server) =
            connect_pair(&a, &b, &cached_b, b_id, "forward-bootstrap", "device-b").await;

        assert_eq!(
            b.connect_cached(&cached_a, a_id, "implicit-reverse", "device-a")
                .await
                .unwrap_err()
                .code,
            ErrorCode::Unauthorized
        );

        b.authorize_outgoing_execution_target(a_id).unwrap();
        a.authorize_incoming_controller(b_id).unwrap();
        let (reverse_client, reverse_server) =
            connect_pair(&b, &a, &cached_a, a_id, "explicit-reverse", "device-a").await;

        assert!(b.revoke_incoming_controller(&a_id).unwrap());
        within(
            forward_server.raw_connection().closed(),
            "incoming revoke local close",
        )
        .await;
        within(
            forward_client.raw_connection().closed(),
            "incoming revoke remote close",
        )
        .await;
        assert!(tokio::time::timeout(
            Duration::from_millis(100),
            reverse_client.raw_connection().closed(),
        )
        .await
        .is_err());
        assert_page_roundtrip(&reverse_client, &reverse_server, "reverse-still-usable").await;

        assert!(b.revoke_outgoing_execution_target(&a_id).unwrap());
        within(
            reverse_client.raw_connection().closed(),
            "outgoing revoke local close",
        )
        .await;
        within(
            reverse_server.raw_connection().closed(),
            "outgoing revoke remote close",
        )
        .await;

        within(
            async { tokio::join!(a.close(), b.close()) },
            "directional endpoint close",
        )
        .await;
    }

    #[tokio::test]
    async fn authorization_snapshot_replacement_and_clear_close_only_revoked_active() {
        let a_identity = identity("snapshot-device-a");
        let b_identity = identity("snapshot-device-b");
        let a_id = endpoint_id(&a_identity);
        let b_id = endpoint_id(&b_identity);
        let a = bind_direct_endpoint(&a_identity, "snapshot-a", epoch(31)).await;
        let b = bind_direct_endpoint(&b_identity, "snapshot-b", epoch(32)).await;
        a.replace_authorizations(AuthorizationSnapshot {
            account_epoch: 1,
            snapshot_revision: 1,
            incoming_controllers: paired([b_id]),
            outgoing_execution_targets: paired([b_id]),
        })
        .unwrap();
        b.replace_authorizations(AuthorizationSnapshot {
            account_epoch: 1,
            snapshot_revision: 1,
            incoming_controllers: paired([a_id]),
            outgoing_execution_targets: paired([a_id]),
        })
        .unwrap();
        let cached_a = wait_for_cached(&a).await;
        let cached_b = wait_for_cached(&b).await;
        let (forward_client, forward_server) =
            connect_pair(&a, &b, &cached_b, b_id, "snapshot-forward", "snapshot-b").await;
        let (reverse_client, reverse_server) =
            connect_pair(&b, &a, &cached_a, a_id, "snapshot-reverse", "snapshot-a").await;

        b.replace_authorizations(AuthorizationSnapshot {
            account_epoch: 1,
            snapshot_revision: 2,
            incoming_controllers: HashMap::new(),
            outgoing_execution_targets: paired([a_id]),
        })
        .unwrap();
        within(
            forward_server.raw_connection().closed(),
            "snapshot revoked local close",
        )
        .await;
        within(
            forward_client.raw_connection().closed(),
            "snapshot revoked remote close",
        )
        .await;
        assert!(tokio::time::timeout(
            Duration::from_millis(100),
            reverse_client.raw_connection().closed(),
        )
        .await
        .is_err());

        b.replace_authorizations(AuthorizationSnapshot {
            account_epoch: 2,
            snapshot_revision: 1,
            incoming_controllers: HashMap::new(),
            outgoing_execution_targets: paired([a_id]),
        })
        .unwrap();
        within(
            reverse_client.raw_connection().closed(),
            "authorization epoch transition local close",
        )
        .await;
        within(
            reverse_server.raw_connection().closed(),
            "authorization epoch transition remote close",
        )
        .await;

        b.clear_authorizations_and_close().unwrap();
        assert_eq!(
            b.replace_authorizations(AuthorizationSnapshot {
                account_epoch: 2,
                snapshot_revision: 2,
                incoming_controllers: paired([a_id]),
                outgoing_execution_targets: HashMap::new(),
            })
            .unwrap_err()
            .code,
            ErrorCode::Revoked
        );
        b.replace_authorizations(AuthorizationSnapshot {
            account_epoch: 3,
            snapshot_revision: 1,
            incoming_controllers: paired([a_id]),
            outgoing_execution_targets: HashMap::new(),
        })
        .unwrap();

        within(
            async { tokio::join!(a.close(), b.close()) },
            "snapshot endpoint close",
        )
        .await;
    }

    #[test]
    fn authorization_transition_receipt_names_removed_peer_and_account_epoch_change() {
        let admission = PeerAdmission::default();
        let first = identity("receipt-controller-a")
            .iroh_secret_key()
            .unwrap()
            .public();
        let second = identity("receipt-controller-b")
            .iroh_secret_key()
            .unwrap()
            .public();
        let initial = admission
            .replace_authorizations(AuthorizationSnapshot {
                account_epoch: 7,
                snapshot_revision: 1,
                incoming_controllers: paired([first]),
                outgoing_execution_targets: HashMap::new(),
            })
            .unwrap();
        assert!(initial.previous().is_none());
        assert_eq!(initial.current().account_epoch(), 7);
        assert!(initial.removed_incoming_controllers().is_empty());
        assert!(!initial.account_epoch_changed());

        let removed = admission
            .replace_authorizations(AuthorizationSnapshot {
                account_epoch: 7,
                snapshot_revision: 2,
                incoming_controllers: paired([second]),
                outgoing_execution_targets: HashMap::new(),
            })
            .unwrap();
        assert_eq!(removed.previous().unwrap().snapshot_revision(), 1);
        assert_eq!(removed.current().snapshot_revision(), 2);
        assert_eq!(removed.removed_incoming_controllers(), &[first]);
        assert!(!removed.account_epoch_changed());

        let switched = admission
            .replace_authorizations(AuthorizationSnapshot {
                account_epoch: 8,
                snapshot_revision: 1,
                incoming_controllers: HashMap::new(),
                outgoing_execution_targets: HashMap::new(),
            })
            .unwrap();
        assert_eq!(switched.previous().unwrap().account_epoch(), 7);
        assert_eq!(switched.current().account_epoch(), 8);
        assert_eq!(switched.removed_incoming_controllers(), &[second]);
        assert!(switched.account_epoch_changed());
    }

    #[test]
    fn admission_revision_exhaustion_cannot_preserve_authority_on_clear() {
        let admission = PeerAdmission::default();
        let controller = identity("exhausted-revision-controller")
            .iroh_secret_key()
            .unwrap()
            .public();
        admission
            .replace_authorizations(AuthorizationSnapshot {
                account_epoch: 11,
                snapshot_revision: 1,
                incoming_controllers: paired([controller]),
                outgoing_execution_targets: HashMap::new(),
            })
            .unwrap();
        admission.state.write().unwrap().admission_revision = u64::MAX;
        admission.clear_all_and_close().unwrap();
        let state = admission.state.read().unwrap();
        assert!(state.authorization_disabled);
        assert!(state.incoming_controllers.allowed.is_empty());
        assert!(state.outgoing_execution_targets.allowed.is_empty());
        assert_eq!(state.admission_revision, u64::MAX);
    }

    #[tokio::test]
    async fn silent_and_partial_bootstrap_streams_are_bounded() {
        let host_identity = identity("bootstrap-timeout-host");
        let raw_identity = identity("bootstrap-timeout-controller");
        let raw_id = endpoint_id(&raw_identity);
        let host = bind_direct_endpoint(&host_identity, "timeout-host", epoch(51)).await;
        host.authorize_incoming_controller(raw_id).unwrap();
        let cached = wait_for_cached(&host).await;
        let raw = bind_raw_endpoint(&raw_identity, vec![ALPN.to_vec()]).await;

        let silent = within(
            raw.connect(cached.as_iroh().clone(), ALPN),
            "silent bootstrap handshake",
        )
        .await
        .unwrap();
        within(silent.closed(), "silent bootstrap deadline close").await;

        let partial = within(
            raw.connect(cached.as_iroh().clone(), ALPN),
            "partial bootstrap handshake",
        )
        .await
        .unwrap();
        let (mut send, _recv) = within(partial.open_bi(), "partial bootstrap stream")
            .await
            .unwrap();
        send.write_all(&32_u32.to_be_bytes()).await.unwrap();
        send.write_all(&[0xa1]).await.unwrap();
        within(partial.closed(), "partial bootstrap deadline close").await;

        within(
            async { tokio::join!(raw.close(), host.close()) },
            "bootstrap timeout endpoint close",
        )
        .await;
    }

    #[test]
    fn cached_addresses_and_relay_policy_are_bounded_and_redacted() {
        let id = endpoint_id(&identity("cached-address-id"));
        let disabled = RelayPolicy::disabled();
        assert_eq!(
            CachedEndpointAddr::new(iroh::EndpointAddr::new(id), &disabled)
                .unwrap_err()
                .code,
            ErrorCode::TransportUnavailable
        );

        let too_many_total = (0..=MAX_CACHED_ADDRESSES)
            .map(|index| {
                iroh::TransportAddr::Ip(SocketAddr::new(
                    IpAddr::V4(Ipv4Addr::new(127, 0, 0, index as u8 + 1)),
                    10_000 + index as u16,
                ))
            })
            .collect::<Vec<_>>();
        assert_eq!(
            CachedEndpointAddr::new(
                iroh::EndpointAddr::from_parts(id, too_many_total),
                &disabled,
            )
            .unwrap_err()
            .code,
            ErrorCode::TransportUnavailable
        );

        let too_many_ips = (0..=MAX_CACHED_IP_ADDRESSES)
            .map(|index| {
                iroh::TransportAddr::Ip(SocketAddr::new(
                    IpAddr::V4(Ipv4Addr::new(127, 0, 1, index as u8 + 1)),
                    20_000 + index as u16,
                ))
            })
            .collect::<Vec<_>>();
        assert_eq!(
            CachedEndpointAddr::new(iroh::EndpointAddr::from_parts(id, too_many_ips), &disabled,)
                .unwrap_err()
                .code,
            ErrorCode::TransportUnavailable
        );

        let custom = iroh::TransportAddr::Custom("1_00".parse().unwrap());
        assert_eq!(
            CachedEndpointAddr::new(iroh::EndpointAddr::from_parts(id, [custom]), &disabled)
                .unwrap_err()
                .code,
            ErrorCode::TransportUnavailable
        );

        let allowed_urls = [
            "https://relay-a.example",
            "https://relay-b.example",
            "https://relay-c.example",
            "https://relay-d.example",
            "https://relay-e.example",
        ];
        let relay_policy =
            RelayPolicy::custom(iroh::RelayMap::try_from_iter(allowed_urls).unwrap()).unwrap();
        let mutable_source = iroh::RelayMap::try_from_iter(allowed_urls).unwrap();
        let isolated_policy = RelayPolicy::custom(mutable_source.clone()).unwrap();
        let injected_url: iroh::RelayUrl = "https://relay-injected.example".parse().unwrap();
        mutable_source.insert(
            injected_url.clone(),
            Arc::new(iroh::RelayConfig::from(injected_url.clone())),
        );
        assert!(!isolated_policy.mode().relay_map().contains(&injected_url));
        assert_eq!(
            CachedEndpointAddr::new(
                iroh::EndpointAddr::from_parts(id, [iroh::TransportAddr::Relay(injected_url)],),
                &isolated_policy,
            )
            .unwrap_err()
            .code,
            ErrorCode::Unauthorized
        );

        let mismatched = iroh::RelayMap::empty();
        let mismatch_key: iroh::RelayUrl = "https://relay-key.example".parse().unwrap();
        let mismatch_config_url: iroh::RelayUrl = "https://relay-config.example".parse().unwrap();
        mismatched.insert(
            mismatch_key,
            Arc::new(iroh::RelayConfig::from(mismatch_config_url)),
        );
        assert_eq!(
            RelayPolicy::custom(mismatched).unwrap_err().code,
            ErrorCode::InvalidFrame
        );
        let allowed_url: iroh::RelayUrl = allowed_urls[0].parse().unwrap();
        let cached = CachedEndpointAddr::new(
            iroh::EndpointAddr::from_parts(
                id,
                [
                    iroh::TransportAddr::Ip("203.0.113.7:443".parse().unwrap()),
                    iroh::TransportAddr::Relay(allowed_url),
                ],
            ),
            &relay_policy,
        )
        .unwrap();
        let cached_debug = format!("{cached:?}");
        assert!(!cached_debug.contains("203.0.113.7"));
        assert!(!cached_debug.contains("relay-a.example"));
        assert!(!format!("{relay_policy:?}").contains("relay-a.example"));

        let unlisted: iroh::RelayUrl = "https://relay-secret.example".parse().unwrap();
        assert_eq!(
            CachedEndpointAddr::new(
                iroh::EndpointAddr::from_parts(id, [iroh::TransportAddr::Relay(unlisted)]),
                &relay_policy,
            )
            .unwrap_err()
            .code,
            ErrorCode::Unauthorized
        );
        let five_relays = allowed_urls
            .iter()
            .map(|url| iroh::TransportAddr::Relay(url.parse().unwrap()))
            .collect::<Vec<_>>();
        assert_eq!(
            CachedEndpointAddr::new(
                iroh::EndpointAddr::from_parts(id, five_relays),
                &relay_policy,
            )
            .unwrap_err()
            .code,
            ErrorCode::Unauthorized
        );

        let nine_urls = [
            "https://r0.example",
            "https://r1.example",
            "https://r2.example",
            "https://r3.example",
            "https://r4.example",
            "https://r5.example",
            "https://r6.example",
            "https://r7.example",
            "https://r8.example",
        ];
        assert_eq!(
            RelayPolicy::custom(iroh::RelayMap::try_from_iter(nine_urls).unwrap())
                .unwrap_err()
                .code,
            ErrorCode::InvalidFrame
        );
        assert_eq!(
            RelayPolicy::custom(
                iroh::RelayMap::try_from_iter(["http://relay-insecure.example"]).unwrap(),
            )
            .unwrap_err()
            .code,
            ErrorCode::InvalidFrame
        );
    }

    struct OversizedSequence;

    impl Serialize for OversizedSequence {
        fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
        where
            S: serde::Serializer,
        {
            let len = MAX_FRAME_BYTES as usize + 1;
            let mut sequence = serializer.serialize_seq(Some(len))?;
            for _ in 0..len {
                sequence.serialize_element(&0_u8)?;
            }
            sequence.end()
        }
    }

    #[test]
    fn bounded_encoder_and_wire_prefix_reject_oversized_frames() {
        assert_eq!(
            encode_frame_bounded(&OversizedSequence).unwrap_err().code,
            ErrorCode::FrameTooLarge
        );
        assert_eq!(
            validate_wire_length_prefix((MAX_FRAME_BYTES + 1).to_be_bytes())
                .unwrap_err()
                .code,
            ErrorCode::FrameTooLarge
        );

        let encoded = encode_frame_bounded(&PageRequest::default()).unwrap();
        let mut trailing = encoded.clone();
        trailing.push(0xf6); // a second valid CBOR value must not be ignored
        let mut cursor = Cursor::new(trailing.as_slice());
        let _: PageRequest = ciborium::de::from_reader(&mut cursor).unwrap();
        assert_eq!(cursor.position(), encoded.len() as u64);
        assert_ne!(cursor.position(), trailing.len() as u64);

        let mut huge_declared_array = vec![0x9b];
        huge_declared_array.extend_from_slice(&u64::MAX.to_be_bytes());
        assert_eq!(
            validate_cbor_shape(&huge_declared_array).unwrap_err().code,
            ErrorCode::InvalidFrame
        );
        let mut huge_declared_map = vec![0xbb];
        huge_declared_map.extend_from_slice(&u64::MAX.to_be_bytes());
        assert_eq!(
            validate_cbor_shape(&huge_declared_map).unwrap_err().code,
            ErrorCode::InvalidFrame
        );
        let deeply_nested = std::iter::repeat_n(0x81, MAX_CBOR_RECURSION + 1)
            .chain(std::iter::once(0xf6))
            .collect::<Vec<_>>();
        assert_eq!(
            validate_cbor_shape(&deeply_nested).unwrap_err().code,
            ErrorCode::InvalidFrame
        );
        let mut trailing_value = encoded;
        trailing_value.push(0xf6);
        assert_eq!(
            validate_cbor_shape(&trailing_value).unwrap_err().code,
            ErrorCode::InvalidFrame
        );
    }

    #[test]
    fn policy_and_authorization_bounds_reject_replay() {
        assert!(ConnectionPolicy::new(Duration::ZERO).is_err());
        assert!(ConnectionPolicy::new(MAX_POLICY_DEADLINE + Duration::from_millis(1)).is_err());
        assert!(ConnectionPolicy::new(Duration::from_secs(1)).is_ok());

        let admission = PeerAdmission::default();
        let peers = (0..=MAX_AUTHORIZED_PEERS_PER_DIRECTION)
            .map(|index| endpoint_id(&identity(&format!("bounded-peer-{index}"))))
            .collect::<Vec<_>>();
        for peer in peers.iter().take(MAX_AUTHORIZED_PEERS_PER_DIRECTION) {
            admission
                .allow(iroh::endpoint::Side::Server, *peer)
                .unwrap();
        }
        assert_eq!(
            admission
                .allow(
                    iroh::endpoint::Side::Server,
                    peers[MAX_AUTHORIZED_PEERS_PER_DIRECTION],
                )
                .unwrap_err()
                .code,
            ErrorCode::Unauthorized
        );

        let versioned = PeerAdmission::default();
        let first_peer = peers[0];
        let second_peer = peers[1];
        versioned
            .replace_authorizations(AuthorizationSnapshot {
                account_epoch: 4,
                snapshot_revision: 2,
                incoming_controllers: paired([first_peer]),
                outgoing_execution_targets: HashMap::new(),
            })
            .unwrap();
        assert_eq!(
            versioned
                .replace_authorizations(AuthorizationSnapshot {
                    account_epoch: 4,
                    snapshot_revision: 1,
                    incoming_controllers: paired([second_peer]),
                    outgoing_execution_targets: HashMap::new(),
                })
                .unwrap_err()
                .code,
            ErrorCode::Revoked
        );
        assert_eq!(
            versioned
                .replace_authorizations(AuthorizationSnapshot {
                    account_epoch: 4,
                    snapshot_revision: 2,
                    incoming_controllers: paired([second_peer]),
                    outgoing_execution_targets: HashMap::new(),
                })
                .unwrap_err()
                .code,
            ErrorCode::Revoked
        );
        assert!(!versioned.is_allowed(iroh::endpoint::Side::Server, &first_peer));
        assert!(!versioned.is_allowed(iroh::endpoint::Side::Server, &second_peer));
        // Once a durable account snapshot is active, unversioned mutations are
        // rejected. A delayed higher snapshot can therefore never race an
        // imperative revoke that the durable revision stream did not record.
        assert_eq!(
            versioned
                .revoke(iroh::endpoint::Side::Server, &first_peer)
                .unwrap_err()
                .code,
            ErrorCode::Unauthorized
        );
        assert_eq!(
            versioned
                .allow(iroh::endpoint::Side::Server, second_peer)
                .unwrap_err()
                .code,
            ErrorCode::Unauthorized
        );
        assert!(!versioned.is_allowed(iroh::endpoint::Side::Server, &first_peer));

        let oversized_snapshot = paired(peers.iter().copied());
        assert_eq!(
            PeerAdmission::default()
                .replace_authorizations(AuthorizationSnapshot {
                    account_epoch: 1,
                    snapshot_revision: 1,
                    incoming_controllers: oversized_snapshot,
                    outgoing_execution_targets: HashMap::new(),
                })
                .unwrap_err()
                .code,
            ErrorCode::Unauthorized
        );

        let aba = PeerAdmission::default();
        aba.allow(iroh::endpoint::Side::Server, first_peer).unwrap();
        let before_clear = aba.state.read().unwrap().admission_revision;
        aba.clear_all_and_close().unwrap();
        assert_eq!(
            aba.allow(iroh::endpoint::Side::Server, first_peer)
                .unwrap_err()
                .code,
            ErrorCode::Revoked
        );
        assert!(aba.state.read().unwrap().admission_revision > before_clear);
        assert_eq!(
            aba.replace_authorizations(AuthorizationSnapshot {
                account_epoch: 1,
                snapshot_revision: 1,
                incoming_controllers: paired([first_peer]),
                outgoing_execution_targets: HashMap::new(),
            })
            .unwrap_err()
            .code,
            ErrorCode::Revoked
        );
    }

    #[tokio::test]
    async fn wrong_cached_endpoint_and_wrong_alpn_fail_closed() {
        let controller_identity = identity("wrong-controller");
        let host_identity = identity("wrong-host");
        let controller_id = endpoint_id(&controller_identity);
        let host_id = endpoint_id(&host_identity);
        let controller =
            bind_direct_endpoint(&controller_identity, "wrong-controller-install", epoch(61)).await;
        let host = bind_direct_endpoint(&host_identity, "wrong-host-install", epoch(62)).await;
        controller
            .authorize_outgoing_execution_target(host_id)
            .unwrap();
        host.authorize_incoming_controller(controller_id).unwrap();
        let cached = wait_for_cached(&host).await;
        let unrelated = endpoint_id(&identity("wrong-unrelated"));
        assert_eq!(
            controller
                .connect_cached(&cached, unrelated, "wrong-endpoint", "wrong-host-install",)
                .await
                .unwrap_err()
                .code,
            ErrorCode::WrongEndpoint
        );

        let wrong_alpn = b"cloud.opensecret.maple/agent/999";
        let raw_identity = identity("wrong-alpn-raw");
        host.authorize_incoming_controller(endpoint_id(&raw_identity))
            .unwrap();
        let raw = bind_raw_endpoint(&raw_identity, vec![wrong_alpn.to_vec()]).await;
        assert!(within(
            raw.connect(cached.as_iroh().clone(), wrong_alpn),
            "wrong ALPN connection",
        )
        .await
        .is_err());

        within(
            async { tokio::join!(raw.close(), controller.close(), host.close()) },
            "wrong endpoint test close",
        )
        .await;
    }

    /// External integration smoke. It uses only ephemeral in-memory identities,
    /// a synthetic page request, and Iroh's official public production relays.
    /// Direct IP transports and N0 discovery are disabled.
    #[tokio::test]
    #[ignore = "opt-in live test: contacts Iroh's official public production relays"]
    async fn synthetic_roundtrip_over_forced_public_relay() {
        const LIVE_TIMEOUT: Duration = Duration::from_secs(45);
        let controller_identity = identity("synthetic-live-relay-controller-v2");
        let host_identity = identity("synthetic-live-relay-host-v2");
        let controller_id = endpoint_id(&controller_identity);
        let host_id = endpoint_id(&host_identity);
        let policy = ConnectionPolicy::new(Duration::from_secs(20)).unwrap();
        let controller = tokio::time::timeout(
            LIVE_TIMEOUT,
            MapleIrohEndpoint::bind_public_relay_only(
                &controller_identity,
                "synthetic-controller",
                epoch(70),
                policy,
            ),
        )
        .await
        .expect("controller relay bind timed out")
        .unwrap();
        let host = tokio::time::timeout(
            LIVE_TIMEOUT,
            MapleIrohEndpoint::bind_public_relay_only(
                &host_identity,
                "synthetic-host",
                epoch(71),
                policy,
            ),
        )
        .await
        .expect("host relay bind timed out")
        .unwrap();
        controller
            .authorize_outgoing_execution_target(host_id)
            .unwrap();
        host.authorize_incoming_controller(controller_id).unwrap();

        tokio::time::timeout(LIVE_TIMEOUT, async {
            tokio::join!(controller.endpoint.online(), host.endpoint.online())
        })
        .await
        .expect("public relay endpoints did not become online");
        let cached = host.cached_endpoint_addr(host.endpoint_addr()).unwrap();
        assert_eq!(cached.as_iroh().ip_addrs().count(), 0);
        assert!(cached.as_iroh().relay_urls().next().is_some());

        let (client, server) = tokio::time::timeout(LIVE_TIMEOUT, async {
            tokio::join!(
                controller.connect_cached(
                    &cached,
                    host_id,
                    "synthetic-live-bootstrap",
                    "synthetic-host",
                ),
                host.accept_authenticated(),
            )
        })
        .await
        .expect("forced-relay connection phase timed out");
        let client = client.unwrap();
        let server = server.unwrap();
        assert!(client
            .raw_connection()
            .paths()
            .iter()
            .any(|path| path.is_selected() && path.is_relay()));
        assert!(server
            .raw_connection()
            .paths()
            .iter()
            .any(|path| path.is_selected() && path.is_relay()));

        tokio::time::timeout(
            LIVE_TIMEOUT,
            assert_page_roundtrip(&client, &server, "synthetic-live-page"),
        )
        .await
        .expect("forced-relay typed page roundtrip timed out");

        client.raw_connection().close(
            iroh::endpoint::VarInt::from_u32(0),
            b"synthetic test complete",
        );
        server.raw_connection().close(
            iroh::endpoint::VarInt::from_u32(0),
            b"synthetic test complete",
        );
        tokio::time::timeout(Duration::from_secs(10), async {
            tokio::join!(controller.close(), host.close())
        })
        .await
        .expect("forced-relay endpoint close timed out");
    }

    #[test]
    fn transport_errors_do_not_expose_backend_details() {
        let error = transport_error(
            "failed to connect to Maple host",
            "relay=https://private.example/ device-address=10.0.0.8",
            true,
        );
        assert_eq!(error.message, "failed to connect to Maple host");
        assert!(!error.message.contains("private.example"));
    }
}
