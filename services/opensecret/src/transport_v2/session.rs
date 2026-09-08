use std::{
    collections::{HashMap, HashSet},
    fmt,
    num::NonZeroUsize,
    sync::{
        atomic::{AtomicUsize, Ordering},
        Arc, Mutex,
    },
    time::{Duration, Instant},
};

use super::{
    crypto::{CryptoError, ResponseSealer, SessionId, SessionSecrets, HANDSHAKE_CHALLENGE_BYTES},
    envelope::RequestId,
};

#[derive(Debug, thiserror::Error, Eq, PartialEq)]
pub(crate) enum ReplayError {
    #[error("transport-v2 request ID was already used in this session")]
    Duplicate,
    #[error("transport-v2 session has reached its request-ID limit")]
    SessionCapacity,
    #[error("transport-v2 process has reached its aggregate request-ID limit")]
    GlobalCapacity,
    #[error("transport-v2 replay registry is unavailable")]
    Unavailable,
}

/// A process-wide count of replay IDs that have actually been retained.
/// Capacity is not preallocated and does not reserve memory.
pub(crate) struct ReplayBudget {
    claimed: AtomicUsize,
    capacity: NonZeroUsize,
}

impl fmt::Debug for ReplayBudget {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("ReplayBudget")
            .field("claimed", &self.claimed.load(Ordering::Relaxed))
            .field("capacity", &self.capacity)
            .finish()
    }
}

impl ReplayBudget {
    pub(crate) const fn new(capacity: NonZeroUsize) -> Self {
        Self {
            claimed: AtomicUsize::new(0),
            capacity,
        }
    }

    fn claim_one(&self) -> Result<(), ReplayError> {
        self.claimed
            .fetch_update(Ordering::AcqRel, Ordering::Acquire, |claimed| {
                (claimed < self.capacity.get()).then_some(claimed + 1)
            })
            .map(|_| ())
            .map_err(|_| ReplayError::GlobalCapacity)
    }

    fn release(&self, count: usize) {
        if count == 0 {
            return;
        }
        let previous = self.claimed.fetch_sub(count, Ordering::AcqRel);
        debug_assert!(previous >= count, "replay budget underflow");
    }

    pub(crate) fn claimed(&self) -> usize {
        self.claimed.load(Ordering::Acquire)
    }

    pub(crate) const fn capacity(&self) -> NonZeroUsize {
        self.capacity
    }
}

/// An exact, unordered, per-session replay set. It grows only for requests
/// actually admitted; a shared counter bounds aggregate retained entries
/// without reserving memory in advance.
pub(crate) struct ReplayRegistry {
    request_ids: Mutex<HashSet<RequestId>>,
    capacity: NonZeroUsize,
    budget: Arc<ReplayBudget>,
}

impl fmt::Debug for ReplayRegistry {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("ReplayRegistry")
            .field("capacity", &self.capacity)
            .field("budget", &self.budget)
            .finish_non_exhaustive()
    }
}

impl ReplayRegistry {
    pub(crate) fn new(capacity: NonZeroUsize, budget: Arc<ReplayBudget>) -> Self {
        Self {
            request_ids: Mutex::new(HashSet::new()),
            capacity,
            budget,
        }
    }

    /// Atomically claims an ID before application side effects. Arrival order
    /// is irrelevant: only exact reuse is rejected.
    pub(crate) fn claim(&self, request_id: RequestId) -> Result<(), ReplayError> {
        let mut request_ids = self
            .request_ids
            .lock()
            .map_err(|_| ReplayError::Unavailable)?;
        if request_ids.contains(&request_id) {
            return Err(ReplayError::Duplicate);
        }
        if request_ids.len() >= self.capacity.get() {
            return Err(ReplayError::SessionCapacity);
        }
        self.budget.claim_one()?;
        request_ids.insert(request_id);
        Ok(())
    }

    pub(crate) fn len(&self) -> Result<usize, ReplayError> {
        self.request_ids
            .lock()
            .map(|request_ids| request_ids.len())
            .map_err(|_| ReplayError::Unavailable)
    }

    pub(crate) const fn capacity(&self) -> NonZeroUsize {
        self.capacity
    }
}

impl Drop for ReplayRegistry {
    fn drop(&mut self) {
        let retained = self
            .request_ids
            .get_mut()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .len();
        self.budget.release(retained);
    }
}

#[derive(Debug, thiserror::Error, Eq, PartialEq)]
pub(crate) enum SessionError {
    #[error("transport-v2 session lifetime is invalid")]
    InvalidLifetime,
    #[error("transport-v2 session expired")]
    Expired,
    #[error(transparent)]
    Crypto(#[from] CryptoError),
    #[error(transparent)]
    Replay(#[from] ReplayError),
}

/// One cryptographic session: directional keys, a fixed absolute expiry, and
/// its exact replay set. Identity and authorization intentionally do not live
/// here; credentials are authenticated independently on every request.
pub(crate) struct Session {
    secrets: SessionSecrets,
    routing_key: [u8; HANDSHAKE_CHALLENGE_BYTES],
    expires_at: Instant,
    replay: ReplayRegistry,
}

/// Proof that one request ID was atomically claimed under one exact session.
/// Only this type can begin an encrypted response for that request.
pub(crate) struct AdmittedRequest {
    session: Arc<Session>,
    request_id: RequestId,
}

impl fmt::Debug for AdmittedRequest {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("AdmittedRequest")
            .field("session_id", &self.session.id())
            .field("request_id", &self.request_id)
            .finish()
    }
}

/// The single response writer for an admitted request. It is derived from the
/// exact session that decrypted and admitted the request, and retains only the
/// response subkey and authenticated session/request identifiers needed to
/// seal that response.
pub(crate) struct ResponseWriter {
    sealer: ResponseSealer,
}

impl fmt::Debug for ResponseWriter {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("ResponseWriter")
            .field("sealer", &self.sealer)
            .finish_non_exhaustive()
    }
}

impl ResponseWriter {
    pub(crate) fn seal_next(&mut self, plaintext: &[u8]) -> Result<Vec<u8>, CryptoError> {
        self.sealer.seal_next(plaintext)
    }
}

impl AdmittedRequest {
    pub(crate) fn session_id(&self) -> SessionId {
        self.session.id()
    }

    pub(crate) const fn request_id(&self) -> RequestId {
        self.request_id
    }

    pub(crate) fn begin_response(self) -> Result<ResponseWriter, CryptoError> {
        let Self {
            session,
            request_id,
        } = self;
        let sealer = session.secrets.response_sealer(request_id)?;
        drop(session);
        Ok(ResponseWriter { sealer })
    }
}

impl fmt::Debug for Session {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("Session")
            .field("id", &self.id())
            .field("expires_at", &self.expires_at)
            .field("replay", &self.replay)
            .finish()
    }
}

impl Session {
    pub(crate) fn new(
        secrets: SessionSecrets,
        routing_key: [u8; HANDSHAKE_CHALLENGE_BYTES],
        lifetime: Duration,
        replay_capacity: NonZeroUsize,
        replay_budget: Arc<ReplayBudget>,
    ) -> Result<Self, SessionError> {
        Self::new_at(
            secrets,
            routing_key,
            Instant::now(),
            lifetime,
            replay_capacity,
            replay_budget,
        )
    }

    fn new_at(
        secrets: SessionSecrets,
        routing_key: [u8; HANDSHAKE_CHALLENGE_BYTES],
        now: Instant,
        lifetime: Duration,
        replay_capacity: NonZeroUsize,
        replay_budget: Arc<ReplayBudget>,
    ) -> Result<Self, SessionError> {
        if lifetime.is_zero() {
            return Err(SessionError::InvalidLifetime);
        }
        let expires_at = now
            .checked_add(lifetime)
            .ok_or(SessionError::InvalidLifetime)?;
        Ok(Self {
            secrets,
            routing_key,
            expires_at,
            replay: ReplayRegistry::new(replay_capacity, replay_budget),
        })
    }

    pub(crate) const fn id(&self) -> SessionId {
        self.secrets.session_id()
    }

    pub(crate) const fn expires_at(&self) -> Instant {
        self.expires_at
    }

    /// Confirms that the public load-balancer routing key belongs to this
    /// attested session. The key is the client challenge already bound into
    /// the handshake transcript; it conveys no identity or authorization.
    pub(crate) fn matches_routing_key(
        &self,
        routing_key: &[u8; HANDSHAKE_CHALLENGE_BYTES],
    ) -> bool {
        &self.routing_key == routing_key
    }

    pub(crate) fn is_expired(&self, now: Instant) -> bool {
        self.expires_at <= now
    }

    /// Authenticates the request record before claiming replay state. A caller
    /// receives plaintext and a response capability only after both succeed.
    pub(crate) fn open_and_admit(
        self: &Arc<Self>,
        record: &[u8],
    ) -> Result<(AdmittedRequest, Vec<u8>), SessionError> {
        let (request_id, plaintext) = self.secrets.decrypt_request(record)?;
        self.claim_request_at(request_id, Instant::now())?;
        Ok((
            AdmittedRequest {
                session: Arc::clone(self),
                request_id,
            },
            plaintext,
        ))
    }

    fn claim_request_at(&self, request_id: RequestId, now: Instant) -> Result<(), SessionError> {
        if self.is_expired(now) {
            return Err(SessionError::Expired);
        }
        self.replay.claim(request_id)?;
        Ok(())
    }
}

#[derive(Debug, thiserror::Error, Eq, PartialEq)]
pub(crate) enum SessionStoreError {
    #[error("transport-v2 session store is unavailable")]
    Unavailable,
    #[error("transport-v2 session ID collision")]
    Collision,
    #[error("transport-v2 session store is full")]
    Full,
    #[error("transport-v2 session is missing")]
    Missing,
    #[error("transport-v2 session expired")]
    Expired,
}

/// A fixed-count session map. Expired entries are reclaimed by lookup or the
/// gateway's periodic purge, but a live session is never evicted to admit an
/// unauthenticated new handshake.
pub(crate) struct SessionStore {
    sessions: Mutex<HashMap<SessionId, Arc<Session>>>,
    capacity: NonZeroUsize,
}

impl fmt::Debug for SessionStore {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("SessionStore")
            .field("capacity", &self.capacity)
            .finish_non_exhaustive()
    }
}

impl SessionStore {
    pub(crate) fn new(capacity: NonZeroUsize) -> Self {
        Self {
            sessions: Mutex::new(HashMap::new()),
            capacity,
        }
    }

    pub(crate) fn insert(&self, session: Arc<Session>) -> Result<(), SessionStoreError> {
        self.insert_at(session, Instant::now())
    }

    fn insert_at(&self, session: Arc<Session>, now: Instant) -> Result<(), SessionStoreError> {
        if session.is_expired(now) {
            return Err(SessionStoreError::Expired);
        }
        let mut sessions = self
            .sessions
            .lock()
            .map_err(|_| SessionStoreError::Unavailable)?;
        let mut reclaimed = Vec::new();
        if let Some(existing) = sessions.get(&session.id()) {
            if existing.is_expired(now) {
                if let Some(expired) = sessions.remove(&session.id()) {
                    reclaimed.push(expired);
                }
            } else {
                return Err(SessionStoreError::Collision);
            }
        }
        if sessions.len() >= self.capacity.get() {
            drop(sessions);
            drop(reclaimed);
            return Err(SessionStoreError::Full);
        }
        sessions.insert(session.id(), session);
        drop(sessions);
        drop(reclaimed);
        Ok(())
    }

    pub(crate) fn get(&self, session_id: SessionId) -> Result<Arc<Session>, SessionStoreError> {
        self.get_at(session_id, Instant::now())
    }

    fn get_at(
        &self,
        session_id: SessionId,
        now: Instant,
    ) -> Result<Arc<Session>, SessionStoreError> {
        let mut sessions = self
            .sessions
            .lock()
            .map_err(|_| SessionStoreError::Unavailable)?;
        let Some(session) = sessions.get(&session_id) else {
            return Err(SessionStoreError::Missing);
        };
        if session.is_expired(now) {
            let expired = sessions.remove(&session_id);
            drop(sessions);
            drop(expired);
            return Err(SessionStoreError::Expired);
        }
        Ok(Arc::clone(session))
    }

    pub(crate) fn remove(
        &self,
        session_id: SessionId,
    ) -> Result<Option<Arc<Session>>, SessionStoreError> {
        self.sessions
            .lock()
            .map(|mut sessions| sessions.remove(&session_id))
            .map_err(|_| SessionStoreError::Unavailable)
    }

    pub(crate) fn purge_expired(&self, now: Instant) -> Result<usize, SessionStoreError> {
        let mut sessions = self
            .sessions
            .lock()
            .map_err(|_| SessionStoreError::Unavailable)?;
        let expired_ids: Vec<_> = sessions
            .iter()
            .filter_map(|(id, session)| session.is_expired(now).then_some(*id))
            .collect();
        let expired: Vec<_> = expired_ids
            .iter()
            .filter_map(|id| sessions.remove(id))
            .collect();
        drop(sessions);
        let removed = expired.len();
        drop(expired);
        Ok(removed)
    }

    pub(crate) fn len(&self) -> Result<usize, SessionStoreError> {
        self.sessions
            .lock()
            .map(|sessions| sessions.len())
            .map_err(|_| SessionStoreError::Unavailable)
    }

    pub(crate) const fn capacity(&self) -> NonZeroUsize {
        self.capacity
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::transport_v2::crypto::{
        derive_client_session, derive_server_session, HandshakeTranscript, X25519_PUBLIC_KEY_BYTES,
    };
    use rand_core::OsRng;
    use std::sync::Barrier;
    use x25519_dalek::{EphemeralSecret, PublicKey};

    const REPLAY_CAPACITY: NonZeroUsize = NonZeroUsize::new(8).unwrap();

    fn replay_budget(capacity: usize) -> Arc<ReplayBudget> {
        Arc::new(ReplayBudget::new(NonZeroUsize::new(capacity).unwrap()))
    }

    fn session_at(now: Instant, marker: u8, lifetime: Duration) -> Arc<Session> {
        let client_secret = EphemeralSecret::random_from_rng(OsRng);
        let client_public = PublicKey::from(&client_secret).to_bytes();
        let server_secret = EphemeralSecret::random_from_rng(OsRng);
        let server_public = PublicKey::from(&server_secret).to_bytes();
        let transcript = HandshakeTranscript::new([marker; 32], client_public, server_public);
        let secrets = derive_server_session(server_secret, &transcript).unwrap();
        Arc::new(
            Session::new_at(
                secrets,
                [marker; HANDSHAKE_CHALLENGE_BYTES],
                now,
                lifetime,
                REPLAY_CAPACITY,
                replay_budget(128),
            )
            .unwrap(),
        )
    }

    #[test]
    fn replay_claims_are_exact_and_arrival_order_independent() {
        let registry = ReplayRegistry::new(NonZeroUsize::new(3).unwrap(), replay_budget(16));
        let first = RequestId::from_bytes([1; 16]);
        let second = RequestId::from_bytes([2; 16]);
        let third = RequestId::from_bytes([3; 16]);

        registry.claim(third).unwrap();
        registry.claim(first).unwrap();
        registry.claim(second).unwrap();
        assert_eq!(registry.claim(first), Err(ReplayError::Duplicate));
        assert_eq!(registry.len().unwrap(), 3);
        assert_eq!(registry.capacity().get(), 3);
        assert_eq!(
            registry.claim(RequestId::from_bytes([4; 16])),
            Err(ReplayError::SessionCapacity)
        );
    }

    #[test]
    fn concurrent_replay_has_exactly_one_winner() {
        let registry = Arc::new(ReplayRegistry::new(
            NonZeroUsize::new(4).unwrap(),
            replay_budget(16),
        ));
        let barrier = Arc::new(Barrier::new(9));
        let request_id = RequestId::from_bytes([9; 16]);
        let mut threads = Vec::new();
        for _ in 0..8 {
            let registry = Arc::clone(&registry);
            let barrier = Arc::clone(&barrier);
            threads.push(std::thread::spawn(move || {
                barrier.wait();
                registry.claim(request_id)
            }));
        }
        barrier.wait();

        let results: Vec<_> = threads
            .into_iter()
            .map(|thread| thread.join().unwrap())
            .collect();
        assert_eq!(results.iter().filter(|result| result.is_ok()).count(), 1);
        assert_eq!(
            results
                .iter()
                .filter(|result| **result == Err(ReplayError::Duplicate))
                .count(),
            7
        );
    }

    #[test]
    fn aggregate_replay_pressure_is_shared_retryable_and_released_on_drop() {
        let budget = replay_budget(1);
        assert_eq!(budget.capacity().get(), 1);
        let first = ReplayRegistry::new(NonZeroUsize::new(8).unwrap(), Arc::clone(&budget));
        let second = ReplayRegistry::new(NonZeroUsize::new(8).unwrap(), Arc::clone(&budget));

        first.claim(RequestId::from_bytes([1; 16])).unwrap();
        assert_eq!(budget.claimed(), 1);
        assert_eq!(
            second.claim(RequestId::from_bytes([2; 16])),
            Err(ReplayError::GlobalCapacity)
        );
        assert_eq!(second.len().unwrap(), 0);

        drop(first);
        assert_eq!(budget.claimed(), 0);
        second.claim(RequestId::from_bytes([2; 16])).unwrap();
        assert_eq!(budget.claimed(), 1);
        drop(second);
        assert_eq!(budget.claimed(), 0);
    }

    #[test]
    fn session_expiry_is_absolute_and_checked_before_replay_claim() {
        let start = Instant::now();
        let session = session_at(start, 1, Duration::from_secs(10));
        let request_id = RequestId::from_bytes([1; 16]);
        session
            .claim_request_at(request_id, start + Duration::from_secs(9))
            .unwrap();
        assert_eq!(
            session.claim_request_at(
                RequestId::from_bytes([2; 16]),
                start + Duration::from_secs(10)
            ),
            Err(SessionError::Expired)
        );
        assert_eq!(session.replay.len().unwrap(), 1);
    }

    #[test]
    fn response_writer_releases_session_memory_but_keeps_exact_binding() {
        let start = Instant::now();
        let client_secret = EphemeralSecret::random_from_rng(OsRng);
        let client_public = PublicKey::from(&client_secret).to_bytes();
        let server_secret = EphemeralSecret::random_from_rng(OsRng);
        let server_public = PublicKey::from(&server_secret).to_bytes();
        let transcript = HandshakeTranscript::new([7; 32], client_public, server_public);
        let client = derive_client_session(client_secret, &transcript).unwrap();
        let budget = replay_budget(128);
        let session = Arc::new(
            Session::new_at(
                derive_server_session(server_secret, &transcript).unwrap(),
                [7; HANDSHAKE_CHALLENGE_BYTES],
                start,
                Duration::from_secs(60),
                REPLAY_CAPACITY,
                Arc::clone(&budget),
            )
            .unwrap(),
        );
        let request_id = RequestId::from_bytes([7; 16]);
        let request = client.encrypt_request(request_id, b"request").unwrap();
        let (admitted, plaintext) = session.open_and_admit(&request).unwrap();
        assert_eq!(plaintext, b"request");
        assert_eq!(budget.claimed(), 1);
        assert!(matches!(
            session.open_and_admit(&request),
            Err(SessionError::Replay(ReplayError::Duplicate))
        ));

        let weak_session = Arc::downgrade(&session);
        let mut writer = admitted.begin_response().unwrap();
        drop(session);
        assert!(weak_session.upgrade().is_none());
        assert_eq!(budget.claimed(), 0);

        let first = writer.seal_next(b"start").unwrap();
        let second = writer.seal_next(b"end").unwrap();
        assert_eq!(
            client.decrypt_response(request_id, 0, &first).unwrap(),
            b"start"
        );
        assert_eq!(
            client.decrypt_response(request_id, 1, &second).unwrap(),
            b"end"
        );
    }

    #[test]
    fn store_rejects_collisions_and_never_evicts_live_sessions() {
        let start = Instant::now();
        let store = SessionStore::new(NonZeroUsize::new(1).unwrap());
        let first = session_at(start, 1, Duration::from_secs(60));
        store.insert_at(Arc::clone(&first), start).unwrap();
        assert_eq!(
            store.insert_at(Arc::clone(&first), start),
            Err(SessionStoreError::Collision)
        );

        let second = session_at(start, 2, Duration::from_secs(60));
        assert_eq!(
            store.insert_at(Arc::clone(&second), start),
            Err(SessionStoreError::Full)
        );
        assert!(Arc::ptr_eq(
            &store.get_at(first.id(), start).unwrap(),
            &first
        ));
        assert!(matches!(
            store.get_at(second.id(), start),
            Err(SessionStoreError::Missing)
        ));
        assert_eq!(store.len().unwrap(), 1);
        assert_eq!(store.capacity().get(), 1);
    }

    #[test]
    fn expired_sessions_are_reclaimed_without_invalidating_held_leases() {
        let start = Instant::now();
        let store = SessionStore::new(NonZeroUsize::new(1).unwrap());
        let first = session_at(start, 1, Duration::from_secs(10));
        store.insert_at(Arc::clone(&first), start).unwrap();
        let held = store.get_at(first.id(), start).unwrap();

        assert!(matches!(
            store.get_at(first.id(), start + Duration::from_secs(10)),
            Err(SessionStoreError::Expired)
        ));
        assert!(Arc::ptr_eq(&held, &first));

        let second = session_at(start, 2, Duration::from_secs(60));
        store
            .insert_at(Arc::clone(&second), start + Duration::from_secs(10))
            .unwrap();
        assert!(Arc::ptr_eq(
            &store
                .get_at(second.id(), start + Duration::from_secs(10))
                .unwrap(),
            &second
        ));
    }

    #[test]
    fn purge_and_remove_release_only_the_targeted_store_entries() {
        let start = Instant::now();
        let store = SessionStore::new(NonZeroUsize::new(3).unwrap());
        let expired = session_at(start, 1, Duration::from_secs(5));
        let live = session_at(start, 2, Duration::from_secs(60));
        store.insert_at(Arc::clone(&expired), start).unwrap();
        store.insert_at(Arc::clone(&live), start).unwrap();

        assert_eq!(
            store.purge_expired(start + Duration::from_secs(5)).unwrap(),
            1
        );
        assert_eq!(store.len().unwrap(), 1);
        assert!(Arc::ptr_eq(
            &store.remove(live.id()).unwrap().unwrap(),
            &live
        ));
        assert_eq!(store.len().unwrap(), 0);
    }

    #[test]
    fn invalid_session_lifetime_is_rejected() {
        let now = Instant::now();
        let client_secret = EphemeralSecret::random_from_rng(OsRng);
        let client_public = PublicKey::from(&client_secret).to_bytes();
        let server_secret = EphemeralSecret::random_from_rng(OsRng);
        let server_public = PublicKey::from(&server_secret).to_bytes();
        let transcript = HandshakeTranscript::new([3; 32], client_public, server_public);
        let secrets = derive_server_session(server_secret, &transcript).unwrap();
        assert_eq!(
            Session::new_at(
                secrets,
                [3; HANDSHAKE_CHALLENGE_BYTES],
                now,
                Duration::ZERO,
                REPLAY_CAPACITY,
                replay_budget(16),
            )
            .unwrap_err(),
            SessionError::InvalidLifetime
        );
    }

    #[test]
    fn x25519_public_keys_remain_full_width() {
        assert_eq!(X25519_PUBLIC_KEY_BYTES, 32);
    }
}
