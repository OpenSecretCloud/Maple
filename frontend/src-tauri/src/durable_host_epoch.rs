//! Durable, installation-local allocation for remote Agent host epochs.
//!
//! A host epoch is reserved synchronously before an Iroh endpoint is allowed
//! to bind. The secure-state record is written first and a separate lineage
//! guard is written second. Consequently, interruption may consume an epoch,
//! but no interruption can make a previously returned epoch available again.
//!
//! The lineage guard detects a missing, corrupt, or lower secure-state record.
//! As with any entirely local scheme, a coordinated rollback of every secure
//! record is outside this boundary and ultimately needs an OS or remote
//! monotonic witness. Maple nevertheless fails closed for every partial
//! rollback it can observe instead of silently recreating state.
#![allow(
    dead_code,
    reason = "remote hosting remains disabled until a platform secure store is enabled"
)]

use sha2::{Digest, Sha256};
use zeroize::Zeroizing;

use crate::secure_storage::{DeviceSecretStore, SecretStoreError};

const PURPOSE: &str = "remote-agent-host-epoch-v1";
const STATE_ACCOUNT: &str = "host-epoch-state";
const GUARD_ACCOUNT: &str = "host-epoch-lineage-guard";
const RECORD_VERSION: u8 = 1;
const STATE_KIND: u8 = 1;
const GUARD_KIND: u8 = 2;
const KEY_DIGEST_LEN: usize = 32;
const EPOCH_LEN: usize = 8;
const COMMITMENT_LEN: usize = 32;
const CHECKSUM_LEN: usize = 32;
const STATE_RECORD_LEN: usize = 1 + 1 + KEY_DIGEST_LEN + EPOCH_LEN + CHECKSUM_LEN;
const GUARD_RECORD_LEN: usize = 1 + 1 + KEY_DIGEST_LEN + EPOCH_LEN + COMMITMENT_LEN + CHECKSUM_LEN;
const STATE_CHECKSUM_DOMAIN: &[u8] = b"maple-host-epoch-state-checksum-v1";
const GUARD_CHECKSUM_DOMAIN: &[u8] = b"maple-host-epoch-guard-checksum-v1";
const KEY_DIGEST_DOMAIN: &[u8] = b"maple-host-epoch-storage-key-v1";
const STATE_COMMITMENT_DOMAIN: &[u8] = b"maple-host-epoch-state-commitment-v1";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum HostEpochRecordKind {
    State,
    LineageGuard,
}

impl HostEpochRecordKind {
    pub(crate) const fn account(self) -> &'static str {
        match self {
            Self::State => STATE_ACCOUNT,
            Self::LineageGuard => GUARD_ACCOUNT,
        }
    }
}

/// Native installation coordinates for the epoch records. There is
/// deliberately no account identifier: signing out, changing accounts, or
/// clearing renderer state must not reset the host's installation lineage.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HostEpochStorageKey {
    app_identifier: String,
    install_marker: String,
    identity_generation: u64,
}

impl HostEpochStorageKey {
    pub(crate) fn new(
        app_identifier: impl Into<String>,
        install_marker: impl Into<String>,
        identity_generation: u64,
    ) -> Result<Self, SecretStoreError> {
        let key = Self {
            app_identifier: app_identifier.into(),
            install_marker: install_marker.into(),
            identity_generation,
        };
        if key.identity_generation == 0
            || !is_safe_component(&key.app_identifier)
            || !is_safe_component(&key.install_marker)
        {
            return Err(SecretStoreError::Corrupt(
                "host epoch storage key has an invalid shape".into(),
            ));
        }
        Ok(key)
    }

    pub(crate) fn service(&self) -> String {
        format!("{}.{}", self.app_identifier, PURPOSE)
    }

    pub(crate) fn account(&self, kind: HostEpochRecordKind) -> &'static str {
        kind.account()
    }

    pub(crate) fn app_identifier(&self) -> &str {
        &self.app_identifier
    }

    fn digest(&self) -> [u8; KEY_DIGEST_LEN] {
        let mut hasher = Sha256::new();
        hasher.update(KEY_DIGEST_DOMAIN);
        hash_len_prefixed(&mut hasher, self.app_identifier.as_bytes());
        hash_len_prefixed(&mut hasher, self.install_marker.as_bytes());
        hasher.update(self.identity_generation.to_be_bytes());
        hasher.finalize().into()
    }
}

/// Opaque proof that the secure store and lineage guard durably accepted this
/// epoch. Only this module can create one; transport code consumes it to build
/// a host connection clock. It is intentionally neither serializable nor
/// constructible from a renderer/caller integer.
#[derive(Debug)]
pub(crate) struct ReservedHostEpoch(u64);

impl ReservedHostEpoch {
    pub(crate) const fn get(&self) -> u64 {
        self.0
    }
}

/// Reserve the next host epoch under the secure backend's cross-process lock.
/// A successful backend write must be atomic and durable before returning;
/// `DeviceSecretStore` documents that contract. No endpoint may bind until
/// this function returns its opaque reservation.
pub(crate) fn reserve_next_host_epoch(
    store: &dyn DeviceSecretStore,
    key: &HostEpochStorageKey,
) -> Result<ReservedHostEpoch, SecretStoreError> {
    let mut reserve = || reserve_next_host_epoch_locked(store, key);
    let epoch = store.with_host_epoch_lock(key, &mut reserve)?;
    if epoch == 0 {
        return Err(SecretStoreError::Corrupt(
            "secure storage returned an invalid host epoch reservation".into(),
        ));
    }
    Ok(ReservedHostEpoch(epoch))
}

fn reserve_next_host_epoch_locked(
    store: &dyn DeviceSecretStore,
    key: &HostEpochStorageKey,
) -> Result<u64, SecretStoreError> {
    let state = store.load_host_epoch_record(key, HostEpochRecordKind::State)?;
    let mut guard = store.load_host_epoch_record(key, HostEpochRecordKind::LineageGuard)?;

    // Persist an initialization sentinel before the first state write. This
    // makes `state present, guard missing` unambiguously unsafe rather than
    // indistinguishable from interruption during first use.
    if state.is_none() && guard.is_none() {
        let initialized = encode_guard_record(key, 0, [0; COMMITMENT_LEN]);
        store_and_verify(store, key, HostEpochRecordKind::LineageGuard, &initialized)?;
        guard = Some(initialized);
    }

    let guard = guard.ok_or_else(|| {
        SecretStoreError::Corrupt("host epoch secure state exists without its lineage guard".into())
    })?;
    let decoded_guard = decode_guard_record(key, &guard)?;
    let decoded_state = state
        .as_deref()
        .map(|record| decode_state_record(key, record))
        .transpose()?;

    match decoded_state.as_ref() {
        None if decoded_guard.epoch != 0 => {
            return Err(SecretStoreError::Corrupt(
                "host epoch secure state is missing below its lineage guard".into(),
            ));
        }
        None => {}
        Some(state) if state.epoch < decoded_guard.epoch => {
            return Err(SecretStoreError::Corrupt(
                "host epoch secure state is older than its lineage guard".into(),
            ));
        }
        Some(state) if state.epoch == decoded_guard.epoch => {
            if state.commitment != decoded_guard.state_commitment {
                return Err(SecretStoreError::Corrupt(
                    "host epoch secure state does not match its lineage guard".into(),
                ));
            }
        }
        // A higher secure epoch is the only expected partial state: the
        // process stopped after the state write and before the guard write.
        // Consume another value rather than ever returning the uncertain one.
        Some(_) => {}
    }

    let current = decoded_state.as_ref().map_or(decoded_guard.epoch, |state| {
        state.epoch.max(decoded_guard.epoch)
    });
    let next = current.checked_add(1).ok_or_else(|| {
        SecretStoreError::Corrupt("host epoch monotonic counter is exhausted".into())
    })?;

    let next_state = encode_state_record(key, next);
    store_and_verify(store, key, HostEpochRecordKind::State, &next_state)?;
    let state_commitment = state_commitment(&next_state);
    let next_guard = encode_guard_record(key, next, state_commitment);
    store_and_verify(store, key, HostEpochRecordKind::LineageGuard, &next_guard)?;
    Ok(next)
}

fn store_and_verify(
    store: &dyn DeviceSecretStore,
    key: &HostEpochStorageKey,
    kind: HostEpochRecordKind,
    expected: &[u8],
) -> Result<(), SecretStoreError> {
    store.store_host_epoch_record(key, kind, expected)?;
    let observed = store.load_host_epoch_record(key, kind)?.ok_or_else(|| {
        SecretStoreError::Backend("secure storage lost a completed host epoch write".into())
    })?;
    if observed.as_slice() != expected {
        return Err(SecretStoreError::Backend(
            "secure storage did not retain the completed host epoch write".into(),
        ));
    }
    Ok(())
}

#[derive(Debug)]
struct DecodedState {
    epoch: u64,
    commitment: [u8; COMMITMENT_LEN],
}

#[derive(Debug)]
struct DecodedGuard {
    epoch: u64,
    state_commitment: [u8; COMMITMENT_LEN],
}

fn encode_state_record(key: &HostEpochStorageKey, epoch: u64) -> Zeroizing<Vec<u8>> {
    debug_assert_ne!(epoch, 0);
    let mut record = Zeroizing::new(Vec::with_capacity(STATE_RECORD_LEN));
    record.push(RECORD_VERSION);
    record.push(STATE_KIND);
    record.extend_from_slice(&key.digest());
    record.extend_from_slice(&epoch.to_be_bytes());
    let checksum = checksum(STATE_CHECKSUM_DOMAIN, &record);
    record.extend_from_slice(&checksum);
    record
}

fn decode_state_record(
    key: &HostEpochStorageKey,
    record: &[u8],
) -> Result<DecodedState, SecretStoreError> {
    validate_record_prefix(
        key,
        record,
        STATE_RECORD_LEN,
        STATE_KIND,
        STATE_CHECKSUM_DOMAIN,
    )?;
    let epoch = decode_epoch(record)?;
    if epoch == 0 {
        return Err(SecretStoreError::Corrupt(
            "host epoch secure state contains zero".into(),
        ));
    }
    Ok(DecodedState {
        epoch,
        commitment: state_commitment(record),
    })
}

fn encode_guard_record(
    key: &HostEpochStorageKey,
    epoch: u64,
    state_commitment: [u8; COMMITMENT_LEN],
) -> Zeroizing<Vec<u8>> {
    let mut record = Zeroizing::new(Vec::with_capacity(GUARD_RECORD_LEN));
    record.push(RECORD_VERSION);
    record.push(GUARD_KIND);
    record.extend_from_slice(&key.digest());
    record.extend_from_slice(&epoch.to_be_bytes());
    record.extend_from_slice(&state_commitment);
    let checksum = checksum(GUARD_CHECKSUM_DOMAIN, &record);
    record.extend_from_slice(&checksum);
    record
}

fn decode_guard_record(
    key: &HostEpochStorageKey,
    record: &[u8],
) -> Result<DecodedGuard, SecretStoreError> {
    validate_record_prefix(
        key,
        record,
        GUARD_RECORD_LEN,
        GUARD_KIND,
        GUARD_CHECKSUM_DOMAIN,
    )?;
    let epoch = decode_epoch(record)?;
    let commitment_start = 1 + 1 + KEY_DIGEST_LEN + EPOCH_LEN;
    let state_commitment: [u8; COMMITMENT_LEN] = record
        [commitment_start..commitment_start + COMMITMENT_LEN]
        .try_into()
        .map_err(|_| SecretStoreError::Corrupt("invalid host epoch guard".into()))?;
    if (epoch == 0) != (state_commitment == [0; COMMITMENT_LEN]) {
        return Err(SecretStoreError::Corrupt(
            "host epoch guard initialization state is invalid".into(),
        ));
    }
    Ok(DecodedGuard {
        epoch,
        state_commitment,
    })
}

fn validate_record_prefix(
    key: &HostEpochStorageKey,
    record: &[u8],
    expected_len: usize,
    expected_kind: u8,
    checksum_domain: &[u8],
) -> Result<(), SecretStoreError> {
    if record.len() != expected_len
        || record.first() != Some(&RECORD_VERSION)
        || record.get(1) != Some(&expected_kind)
    {
        return Err(SecretStoreError::Corrupt(
            "host epoch record has an unsupported shape".into(),
        ));
    }
    if record[2..2 + KEY_DIGEST_LEN] != key.digest() {
        return Err(SecretStoreError::Corrupt(
            "host epoch record belongs to a different installation lineage".into(),
        ));
    }
    let checksum_start = record.len() - CHECKSUM_LEN;
    let expected_checksum = checksum(checksum_domain, &record[..checksum_start]);
    if record[checksum_start..] != expected_checksum {
        return Err(SecretStoreError::Corrupt(
            "host epoch record integrity check failed".into(),
        ));
    }
    Ok(())
}

fn decode_epoch(record: &[u8]) -> Result<u64, SecretStoreError> {
    let epoch_start = 1 + 1 + KEY_DIGEST_LEN;
    let epoch_bytes: [u8; EPOCH_LEN] = record[epoch_start..epoch_start + EPOCH_LEN]
        .try_into()
        .map_err(|_| SecretStoreError::Corrupt("invalid host epoch record".into()))?;
    Ok(u64::from_be_bytes(epoch_bytes))
}

fn checksum(domain: &[u8], record: &[u8]) -> [u8; CHECKSUM_LEN] {
    let mut hasher = Sha256::new();
    hasher.update(domain);
    hasher.update(record);
    hasher.finalize().into()
}

fn state_commitment(record: &[u8]) -> [u8; COMMITMENT_LEN] {
    checksum(STATE_COMMITMENT_DOMAIN, record)
}

fn hash_len_prefixed(hasher: &mut Sha256, value: &[u8]) {
    hasher.update(u64::try_from(value.len()).unwrap_or(u64::MAX).to_be_bytes());
    hasher.update(value);
}

fn is_safe_component(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 200
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.' | b':'))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::secure_storage::DeviceSecretSlot;
    use std::{
        collections::HashMap,
        sync::{Arc, Mutex},
        thread,
    };

    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    enum FailureTiming {
        BeforeWrite,
        AfterWrite,
    }

    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    struct WriteFailure {
        kind: HostEpochRecordKind,
        timing: FailureTiming,
    }

    #[derive(Debug, Default)]
    struct FaultStore {
        records: Mutex<HashMap<HostEpochRecordKind, Vec<u8>>>,
        epoch_lock: Mutex<()>,
        next_write_failure: Mutex<Option<WriteFailure>>,
    }

    impl FaultStore {
        fn fail_next_write(&self, kind: HostEpochRecordKind, timing: FailureTiming) {
            *self.next_write_failure.lock().unwrap() = Some(WriteFailure { kind, timing });
        }

        fn snapshot(&self, kind: HostEpochRecordKind) -> Option<Vec<u8>> {
            self.records.lock().unwrap().get(&kind).cloned()
        }

        fn restore(&self, kind: HostEpochRecordKind, value: Option<Vec<u8>>) {
            let mut records = self.records.lock().unwrap();
            match value {
                Some(value) => {
                    records.insert(kind, value);
                }
                None => {
                    records.remove(&kind);
                }
            }
        }
    }

    impl DeviceSecretStore for FaultStore {
        fn load(
            &self,
            _slot: &DeviceSecretSlot,
        ) -> Result<Option<Zeroizing<Vec<u8>>>, SecretStoreError> {
            Err(SecretStoreError::Unavailable(
                "identity records are not used by this test store".into(),
            ))
        }

        fn store(&self, _slot: &DeviceSecretSlot, _secret: &[u8]) -> Result<(), SecretStoreError> {
            Err(SecretStoreError::Unavailable(
                "identity records are not used by this test store".into(),
            ))
        }

        fn delete(&self, _slot: &DeviceSecretSlot) -> Result<(), SecretStoreError> {
            Err(SecretStoreError::Unavailable(
                "identity records are not used by this test store".into(),
            ))
        }

        fn load_host_epoch_record(
            &self,
            _key: &HostEpochStorageKey,
            kind: HostEpochRecordKind,
        ) -> Result<Option<Zeroizing<Vec<u8>>>, SecretStoreError> {
            Ok(self
                .records
                .lock()
                .unwrap()
                .get(&kind)
                .cloned()
                .map(Zeroizing::new))
        }

        fn store_host_epoch_record(
            &self,
            _key: &HostEpochStorageKey,
            kind: HostEpochRecordKind,
            record: &[u8],
        ) -> Result<(), SecretStoreError> {
            let failure = {
                let mut pending = self.next_write_failure.lock().unwrap();
                if pending.as_ref().is_some_and(|failure| failure.kind == kind) {
                    pending.take()
                } else {
                    None
                }
            };
            if failure.is_some_and(|failure| failure.timing == FailureTiming::BeforeWrite) {
                return Err(SecretStoreError::Backend(
                    "test interruption before durable write".into(),
                ));
            }
            self.records.lock().unwrap().insert(kind, record.to_vec());
            if failure.is_some_and(|failure| failure.timing == FailureTiming::AfterWrite) {
                return Err(SecretStoreError::Backend(
                    "test interruption after durable write".into(),
                ));
            }
            Ok(())
        }

        fn with_host_epoch_lock(
            &self,
            _key: &HostEpochStorageKey,
            operation: &mut dyn FnMut() -> Result<u64, SecretStoreError>,
        ) -> Result<u64, SecretStoreError> {
            let _guard = self.epoch_lock.lock().unwrap();
            operation()
        }
    }

    fn key() -> HostEpochStorageKey {
        HostEpochStorageKey::new("cloud.opensecret.maple.test", "install-a", 1).unwrap()
    }

    fn reserve(store: &FaultStore) -> Result<u64, SecretStoreError> {
        reserve_next_host_epoch(store, &key()).map(|reservation| reservation.get())
    }

    #[test]
    fn process_restarts_advance_the_durable_epoch() {
        let store = FaultStore::default();
        assert_eq!(reserve(&store).unwrap(), 1);
        assert_eq!(reserve(&store).unwrap(), 2);
        assert_eq!(reserve(&store).unwrap(), 3);
    }

    #[test]
    fn concurrent_runtime_startups_receive_unique_epochs() {
        let store = Arc::new(FaultStore::default());
        let workers = (0..16)
            .map(|_| {
                let store = Arc::clone(&store);
                thread::spawn(move || reserve(&store).unwrap())
            })
            .collect::<Vec<_>>();
        let mut epochs = workers
            .into_iter()
            .map(|worker| worker.join().unwrap())
            .collect::<Vec<_>>();
        epochs.sort_unstable();
        assert_eq!(epochs, (1..=16).collect::<Vec<_>>());
    }

    #[test]
    fn interruption_before_state_persist_returns_no_capability_and_reuses_no_returned_epoch() {
        let store = FaultStore::default();
        assert_eq!(reserve(&store).unwrap(), 1);
        store.fail_next_write(HostEpochRecordKind::State, FailureTiming::BeforeWrite);
        assert!(
            reserve(&store).is_err(),
            "failed persistence must not return an epoch"
        );
        assert_eq!(reserve(&store).unwrap(), 2);
    }

    #[test]
    fn interruption_after_state_persist_skips_the_uncertain_epoch() {
        let store = FaultStore::default();
        assert_eq!(reserve(&store).unwrap(), 1);
        store.fail_next_write(HostEpochRecordKind::State, FailureTiming::AfterWrite);
        assert!(
            reserve(&store).is_err(),
            "interrupted reservation must not escape"
        );
        assert_eq!(reserve(&store).unwrap(), 3);
    }

    #[test]
    fn interruption_after_guard_persist_skips_the_uncertain_epoch() {
        let store = FaultStore::default();
        assert_eq!(reserve(&store).unwrap(), 1);
        store.fail_next_write(HostEpochRecordKind::LineageGuard, FailureTiming::AfterWrite);
        assert!(
            reserve(&store).is_err(),
            "interrupted reservation must not escape"
        );
        assert_eq!(reserve(&store).unwrap(), 3);
    }

    #[test]
    fn lower_secure_state_fails_closed_against_the_guard() {
        let store = FaultStore::default();
        assert_eq!(reserve(&store).unwrap(), 1);
        let old_state = store.snapshot(HostEpochRecordKind::State);
        assert_eq!(reserve(&store).unwrap(), 2);
        store.restore(HostEpochRecordKind::State, old_state);
        assert!(matches!(reserve(&store), Err(SecretStoreError::Corrupt(_))));
    }

    #[test]
    fn restored_aba_state_never_reissues_an_old_epoch() {
        let store = FaultStore::default();
        assert_eq!(reserve(&store).unwrap(), 1);
        let state_a = store.snapshot(HostEpochRecordKind::State);
        assert_eq!(reserve(&store).unwrap(), 2);
        assert_eq!(reserve(&store).unwrap(), 3);
        store.restore(HostEpochRecordKind::State, state_a);
        assert!(matches!(reserve(&store), Err(SecretStoreError::Corrupt(_))));
    }

    #[test]
    fn missing_or_corrupt_secure_state_fails_closed() {
        let store = FaultStore::default();
        assert_eq!(reserve(&store).unwrap(), 1);
        store.restore(HostEpochRecordKind::State, None);
        assert!(matches!(reserve(&store), Err(SecretStoreError::Corrupt(_))));

        let store = FaultStore::default();
        assert_eq!(reserve(&store).unwrap(), 1);
        let mut corrupt = store.snapshot(HostEpochRecordKind::State).unwrap();
        corrupt[4] ^= 0x80;
        store.restore(HostEpochRecordKind::State, Some(corrupt));
        assert!(matches!(reserve(&store), Err(SecretStoreError::Corrupt(_))));
    }

    #[test]
    fn state_without_initialization_guard_fails_closed() {
        let store = FaultStore::default();
        assert_eq!(reserve(&store).unwrap(), 1);
        store.restore(HostEpochRecordKind::LineageGuard, None);
        assert!(matches!(reserve(&store), Err(SecretStoreError::Corrupt(_))));
    }

    #[test]
    fn installation_lineage_change_requires_explicit_teardown_instead_of_resetting() {
        let store = FaultStore::default();
        assert_eq!(reserve(&store).unwrap(), 1);
        let changed =
            HostEpochStorageKey::new("cloud.opensecret.maple.test", "install-b", 2).unwrap();
        assert!(matches!(
            reserve_next_host_epoch(&store, &changed),
            Err(SecretStoreError::Corrupt(_))
        ));
    }

    #[test]
    fn backend_without_epoch_lock_fails_closed() {
        struct NoEpochBackend;

        impl DeviceSecretStore for NoEpochBackend {
            fn load(
                &self,
                _slot: &DeviceSecretSlot,
            ) -> Result<Option<Zeroizing<Vec<u8>>>, SecretStoreError> {
                Ok(None)
            }

            fn store(
                &self,
                _slot: &DeviceSecretSlot,
                _secret: &[u8],
            ) -> Result<(), SecretStoreError> {
                Ok(())
            }

            fn delete(&self, _slot: &DeviceSecretSlot) -> Result<(), SecretStoreError> {
                Ok(())
            }
        }

        assert!(matches!(
            reserve_next_host_epoch(&NoEpochBackend, &key()),
            Err(SecretStoreError::Unavailable(_))
        ));
    }
}
