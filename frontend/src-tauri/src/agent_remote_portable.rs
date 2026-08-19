//! Fail-closed native composition for portable, persisted-only Agent access.
//!
//! Stored registry bytes are untrusted recovery material. They never construct
//! transport authority directly: only an injected verifier backed by a released
//! OpenSecret SDK may return the sealed verified registry consumed below. The
//! production composition remains disabled until that verifier, a secure store,
//! and a controller peer factory are installed in a later slice.
#![allow(
    dead_code,
    reason = "portable Agent composition is production-disabled until verified dependencies land"
)]

use std::{
    any::Any,
    collections::{HashMap, HashSet},
    future::Future,
    net::SocketAddr,
    pin::Pin,
    sync::{
        atomic::{AtomicBool, AtomicU64, Ordering},
        Arc, Mutex,
    },
};

use getrandom::fill as fill_random;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use tokio::sync::Notify;

#[cfg(any(mobile, test))]
#[path = "agent_remote_portable_tauri.rs"]
pub(crate) mod tauri;

const STORED_STATE_MAGIC: &[u8; 8] = b"MAPRSTA1";
const STORED_GUARD_MAGIC: &[u8; 8] = b"MAPRGRD1";
const STORED_SCHEMA_VERSION: u16 = 1;
const STORED_STATE_KIND: u8 = 1;
const STORED_GUARD_KIND: u8 = 2;
const DIGEST_BYTES: usize = 32;
const STATE_FIXED_BYTES: usize = 8 + 2 + 1 + DIGEST_BYTES + 8 + 4 + DIGEST_BYTES + DIGEST_BYTES;
const GUARD_FIXED_BYTES: usize = 8 + 2 + 1 + DIGEST_BYTES + 8 + DIGEST_BYTES + DIGEST_BYTES;
const MAX_STORED_REGISTRY_BYTES: usize = 1024 * 1024;
const MAX_STORED_BODY_BYTES: usize = MAX_STORED_REGISTRY_BYTES - STATE_FIXED_BYTES;
const MAX_CURRENT_TARGETS: usize = 64;
const MAX_RETAINED_LINEAGE_TOMBSTONES: usize = 128;
const MAX_OPAQUE_EVIDENCE_BYTES: usize = 64 * 1024;
const MAX_OPAQUE_LINEAGE_BYTES: usize = 16 * 1024;
const MAX_TARGET_LABEL_BYTES: usize = 256;
const MAX_TARGET_LABEL_CHARS: usize = 80;
const MAX_RELAY_HINTS: usize = 4;
const MAX_DIRECT_ADDRESS_HINTS: usize = 16;
const MAX_RELAY_URL_BYTES: usize = 512;
const MAX_DIRECT_ADDRESS_BYTES: usize = 64;
const MAX_CURSOR_BYTES: usize = 512;
const MAX_ID_BYTES: usize = 128;
const MAX_PAGE_SIZE: u16 = 50;
const MAX_RUNTIME_ACTIVE_RUNS: u16 = 64;
const MAX_JAVASCRIPT_SAFE_UNSIGNED_INTEGER: u64 = 9_007_199_254_740_991;
const MAX_JAVASCRIPT_SAFE_INTEGER: i64 = 9_007_199_254_740_991;
const MAX_BACKEND_COUNTER: u64 = i64::MAX as u64;
const MAX_HISTORY_ITEMS_PER_RECORD: usize = 200;
const MAX_TIMELINE_TEXT_BYTES: usize = 192 * 1024;
const MAX_PORTABLE_HISTORY_RECORD_BYTES: usize = 1_040_384;

const PAIR_AUTHORIZATION_EVIDENCE_FORMAT: &str = "os.maple-pair-authorization.v1";
const QUIESCENT_LINEAGE_FORMAT: &str = "cloud.opensecret.maple.transport-lineage.quiescent.v1";
const UNCERTAIN_LINEAGE_FORMAT: &str = "cloud.opensecret.maple.transport-lineage.uncertain.v1";

const STORED_BODY_DIGEST_DOMAIN: &[u8] = b"cloud.opensecret.maple/portable-registry-body/v1\0";
const STORED_STATE_CHECKSUM_DOMAIN: &[u8] = b"cloud.opensecret.maple/portable-registry-state/v1\0";
const STORED_GUARD_CHECKSUM_DOMAIN: &[u8] = b"cloud.opensecret.maple/portable-registry-guard/v1\0";

pub(crate) type PortableFuture<'a, T> = Pin<Box<dyn Future<Output = T> + Send + 'a>>;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum AgentPortableRemoteError {
    Unavailable,
    UnsupportedStoredVersion,
    CorruptStoredRegistry,
    InvalidStoredRegistry,
    StoredRegistryRollback,
    StoredRegistryInterrupted,
    StoredRegistryEquivocation,
    StoredRegistryConflict,
    DuplicateStoredTarget,
    Unauthenticated,
    AccountMismatch,
    Revoked,
    VerificationFailed,
    UnknownTarget,
    Busy,
    Cancelled,
    StaleLease,
    InvalidRequest,
    InvalidResponse,
    PeerUnavailable,
    CleanupFailed,
    Internal,
}

impl std::fmt::Display for AgentPortableRemoteError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let message = match self {
            Self::Unavailable => "portable Agent access is unavailable",
            Self::UnsupportedStoredVersion => "stored portable Agent state is unsupported",
            Self::CorruptStoredRegistry => "stored portable Agent state is corrupt",
            Self::InvalidStoredRegistry => "stored portable Agent state is invalid",
            Self::StoredRegistryRollback => "stored portable Agent state regressed",
            Self::StoredRegistryInterrupted => "stored portable Agent state needs reconciliation",
            Self::StoredRegistryEquivocation => "stored portable Agent state conflicts",
            Self::StoredRegistryConflict => "portable Agent state changed concurrently",
            Self::DuplicateStoredTarget => "stored portable Agent targets are duplicated",
            Self::Unauthenticated => "portable Agent authentication is unavailable",
            Self::AccountMismatch => "portable Agent account binding changed",
            Self::Revoked => "portable Agent pairing is revoked",
            Self::VerificationFailed => "portable Agent pairing could not be verified",
            Self::UnknownTarget => "portable Agent target is unavailable",
            Self::Busy => "portable Agent target is changing",
            Self::Cancelled => "portable Agent operation was cancelled",
            Self::StaleLease => "portable Agent target lease is stale",
            Self::InvalidRequest => "portable Agent request is invalid",
            Self::InvalidResponse => "portable Agent response is invalid",
            Self::PeerUnavailable => "portable Agent peer is unavailable",
            Self::CleanupFailed => "portable Agent cleanup did not complete",
            Self::Internal => "portable Agent state is unavailable",
        };
        formatter.write_str(message)
    }
}

impl std::error::Error for AgentPortableRemoteError {}

#[derive(Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) struct PairedTargetStorageKeyDigest([u8; DIGEST_BYTES]);

impl std::fmt::Debug for PairedTargetStorageKeyDigest {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("PairedTargetStorageKeyDigest(<redacted>)")
    }
}

impl PairedTargetStorageKeyDigest {
    fn validate(self) -> Result<(), AgentPortableRemoteError> {
        if self.0.iter().all(|byte| *byte == 0) {
            Err(AgentPortableRemoteError::InvalidStoredRegistry)
        } else {
            Ok(())
        }
    }

    #[cfg(test)]
    fn for_test(byte: u8) -> Self {
        Self([byte; DIGEST_BYTES])
    }
}

#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct StoredOpaqueEvidenceV1 {
    format: String,
    bytes: Vec<u8>,
    digest: [u8; DIGEST_BYTES],
}

impl std::fmt::Debug for StoredOpaqueEvidenceV1 {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("StoredOpaqueEvidenceV1")
            .field("format", &self.format)
            .field("byte_count", &self.bytes.len())
            .field("digest", &"<redacted>")
            .finish()
    }
}

impl StoredOpaqueEvidenceV1 {
    fn validate(&self, expected_format: Option<&str>) -> Result<(), AgentPortableRemoteError> {
        if expected_format.is_some_and(|expected| self.format != expected)
            || self.format.is_empty()
            || self.format.len() > MAX_ID_BYTES
            || !self.format.bytes().all(|byte| {
                byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'-' | b'_' | b'/')
            })
            || self.bytes.is_empty()
            || self.bytes.len() > MAX_OPAQUE_EVIDENCE_BYTES
            || self.digest.iter().all(|byte| *byte == 0)
        {
            return Err(AgentPortableRemoteError::InvalidStoredRegistry);
        }
        Ok(())
    }
}

#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct StoredConnectionLineageFloorV1 {
    host_epoch: u64,
    generation: u64,
}

impl std::fmt::Debug for StoredConnectionLineageFloorV1 {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("StoredConnectionLineageFloorV1(<redacted>)")
    }
}

impl StoredConnectionLineageFloorV1 {
    fn validate(&self) -> Result<(), AgentPortableRemoteError> {
        validate_backend_counter(self.host_epoch)?;
        validate_backend_counter(self.generation)
    }
}

#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "state", rename_all = "snake_case", deny_unknown_fields)]
pub(crate) enum StoredTransportLineageV1 {
    Quiescent {
        format: String,
        lineage_revision: u64,
        bytes: Vec<u8>,
        digest: [u8; DIGEST_BYTES],
        replay_floor: Option<StoredConnectionLineageFloorV1>,
    },
    Uncertain {
        format: String,
        lineage_revision: u64,
        bytes: Vec<u8>,
        digest: [u8; DIGEST_BYTES],
        replay_floor: Option<StoredConnectionLineageFloorV1>,
    },
}

impl std::fmt::Debug for StoredTransportLineageV1 {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Quiescent {
                bytes,
                replay_floor,
                ..
            } => formatter
                .debug_struct("Quiescent")
                .field("byte_count", &bytes.len())
                .field("replay_floor", replay_floor)
                .finish(),
            Self::Uncertain { bytes, .. } => formatter
                .debug_struct("Uncertain")
                .field("byte_count", &bytes.len())
                .finish(),
        }
    }
}

impl StoredTransportLineageV1 {
    fn validate(&self) -> Result<(), AgentPortableRemoteError> {
        let (format, lineage_revision, bytes, digest) = match self {
            Self::Quiescent {
                format,
                lineage_revision,
                bytes,
                digest,
                replay_floor,
            } => {
                if let Some(floor) = replay_floor {
                    floor.validate()?;
                }
                if format != QUIESCENT_LINEAGE_FORMAT {
                    return Err(AgentPortableRemoteError::InvalidStoredRegistry);
                }
                (format, lineage_revision, bytes, digest)
            }
            Self::Uncertain {
                format,
                lineage_revision,
                bytes,
                digest,
                replay_floor,
            } => {
                if let Some(floor) = replay_floor {
                    floor.validate()?;
                }
                if format != UNCERTAIN_LINEAGE_FORMAT {
                    return Err(AgentPortableRemoteError::InvalidStoredRegistry);
                }
                (format, lineage_revision, bytes, digest)
            }
        };
        if format.is_empty()
            || validate_backend_counter(*lineage_revision).is_err()
            || bytes.is_empty()
            || bytes.len() > MAX_OPAQUE_LINEAGE_BYTES
            || digest.iter().all(|byte| *byte == 0)
        {
            return Err(AgentPortableRemoteError::InvalidStoredRegistry);
        }
        Ok(())
    }

    fn is_quiescent(&self) -> bool {
        matches!(self, Self::Quiescent { .. })
    }
}

#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct StoredConnectionHintsV1 {
    /// Untrusted endpoint hints. A peer factory must intersect these with its
    /// independently configured relay and destination policy before dialing.
    relay_urls: Vec<String>,
    direct_addresses: Vec<String>,
}

impl std::fmt::Debug for StoredConnectionHintsV1 {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("StoredConnectionHintsV1")
            .field("relay_count", &self.relay_urls.len())
            .field("direct_address_count", &self.direct_addresses.len())
            .finish()
    }
}

impl StoredConnectionHintsV1 {
    fn validate(&self) -> Result<(), AgentPortableRemoteError> {
        if self.relay_urls.len() > MAX_RELAY_HINTS
            || self.direct_addresses.len() > MAX_DIRECT_ADDRESS_HINTS
            || self.relay_urls.len() + self.direct_addresses.len() == 0
            || !is_strictly_sorted_unique(&self.relay_urls)
            || !is_strictly_sorted_unique(&self.direct_addresses)
        {
            return Err(AgentPortableRemoteError::InvalidStoredRegistry);
        }
        for value in &self.relay_urls {
            if value.len() > MAX_RELAY_URL_BYTES {
                return Err(AgentPortableRemoteError::InvalidStoredRegistry);
            }
            let parsed = value
                .parse::<iroh::RelayUrl>()
                .map_err(|_| AgentPortableRemoteError::InvalidStoredRegistry)?;
            if parsed.as_str() != value
                || parsed.scheme() != "https"
                || parsed.host_str().is_none()
                || !parsed.username().is_empty()
                || parsed.password().is_some()
                || parsed.query().is_some()
                || parsed.fragment().is_some()
            {
                return Err(AgentPortableRemoteError::InvalidStoredRegistry);
            }
        }
        for value in &self.direct_addresses {
            if value.len() > MAX_DIRECT_ADDRESS_BYTES {
                return Err(AgentPortableRemoteError::InvalidStoredRegistry);
            }
            let parsed = value
                .parse::<SocketAddr>()
                .map_err(|_| AgentPortableRemoteError::InvalidStoredRegistry)?;
            if parsed.to_string() != *value {
                return Err(AgentPortableRemoteError::InvalidStoredRegistry);
            }
        }
        Ok(())
    }
}

#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct StoredRevocationNamespaceV1 {
    stream_id: String,
    generation: u64,
    applied_sequence: u64,
    checkpoint_digest: [u8; DIGEST_BYTES],
}

impl std::fmt::Debug for StoredRevocationNamespaceV1 {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("StoredRevocationNamespaceV1")
            .field("generation", &self.generation)
            .field("applied_sequence", &self.applied_sequence)
            .field("checkpoint_digest", &"<redacted>")
            .finish_non_exhaustive()
    }
}

impl StoredRevocationNamespaceV1 {
    fn validate(&self) -> Result<(), AgentPortableRemoteError> {
        validate_uuid("revocation stream", &self.stream_id)?;
        validate_backend_counter(self.generation)?;
        if self.applied_sequence > MAX_BACKEND_COUNTER
            || self.checkpoint_digest.iter().all(|byte| *byte == 0)
        {
            return Err(AgentPortableRemoteError::InvalidStoredRegistry);
        }
        Ok(())
    }
}

#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct StoredPairedTargetV1 {
    pair_id: String,
    pairing_revision: u64,
    directory_revision: u64,
    host_registration_id: String,
    host_device_id: String,
    host_installation_id: String,
    host_endpoint_id: String,
    host_endpoint_epoch: u64,
    host_display_name: String,
    pairing_incarnation: u64,
    authorization: StoredOpaqueEvidenceV1,
    revocation: StoredRevocationNamespaceV1,
    connection_hints: StoredConnectionHintsV1,
    transport_lineage: StoredTransportLineageV1,
}

impl std::fmt::Debug for StoredPairedTargetV1 {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("StoredPairedTargetV1")
            .field("pairing_revision", &self.pairing_revision)
            .field("directory_revision", &self.directory_revision)
            .field("host_endpoint_epoch", &self.host_endpoint_epoch)
            .field("pairing_incarnation", &self.pairing_incarnation)
            .field("authorization", &self.authorization)
            .field(
                "connection_hint_count",
                &(self.connection_hints.relay_urls.len()
                    + self.connection_hints.direct_addresses.len()),
            )
            .field("transport_lineage", &self.transport_lineage)
            .finish_non_exhaustive()
    }
}

impl StoredPairedTargetV1 {
    fn validate(&self) -> Result<(), AgentPortableRemoteError> {
        validate_uuid("pair", &self.pair_id)?;
        validate_backend_counter(self.pairing_revision)?;
        validate_backend_counter(self.directory_revision)?;
        validate_uuid("host registration", &self.host_registration_id)?;
        validate_uuid("host device", &self.host_device_id)?;
        validate_uuid("host installation", &self.host_installation_id)?;
        validate_endpoint_id(&self.host_endpoint_id)?;
        validate_backend_counter(self.host_endpoint_epoch)?;
        validate_display_label(&self.host_display_name)?;
        validate_backend_counter(self.pairing_incarnation)?;
        self.authorization
            .validate(Some(PAIR_AUTHORIZATION_EVIDENCE_FORMAT))?;
        self.revocation.validate()?;
        self.connection_hints.validate()?;
        self.transport_lineage.validate()?;
        Ok(())
    }
}

#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct StoredLineageTombstoneV1 {
    host_registration_id: String,
    host_endpoint_id: String,
    pair_id: String,
    retired_pairing_incarnation: u64,
    retired_authorization_revision: u64,
    replay_floor: Option<StoredConnectionLineageFloorV1>,
}

impl std::fmt::Debug for StoredLineageTombstoneV1 {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("StoredLineageTombstoneV1")
            .field(
                "retired_pairing_incarnation",
                &self.retired_pairing_incarnation,
            )
            .field(
                "retired_authorization_revision",
                &self.retired_authorization_revision,
            )
            .field("replay_floor", &self.replay_floor)
            .finish_non_exhaustive()
    }
}

impl StoredLineageTombstoneV1 {
    fn validate(&self) -> Result<(), AgentPortableRemoteError> {
        validate_uuid("retired host registration", &self.host_registration_id)?;
        validate_endpoint_id(&self.host_endpoint_id)?;
        validate_uuid("retired pair", &self.pair_id)?;
        validate_backend_counter(self.retired_pairing_incarnation)?;
        validate_backend_counter(self.retired_authorization_revision)?;
        if let Some(floor) = &self.replay_floor {
            floor.validate()?;
        }
        Ok(())
    }
}

#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct StoredPairedTargetRegistryV1 {
    account_id: String,
    project_id: String,
    local_registration_id: String,
    local_device_id: String,
    local_installation_id: String,
    controller_endpoint_id: String,
    controller_endpoint_epoch: u64,
    account_context_epoch: u64,
    security_epoch: u64,
    authorization_snapshot_revision: u64,
    registration_evidence: StoredOpaqueEvidenceV1,
    revocation_sync_evidence: StoredOpaqueEvidenceV1,
    targets: Vec<StoredPairedTargetV1>,
    lineage_tombstones: Vec<StoredLineageTombstoneV1>,
}

impl std::fmt::Debug for StoredPairedTargetRegistryV1 {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("StoredPairedTargetRegistryV1")
            .field("controller_endpoint_epoch", &self.controller_endpoint_epoch)
            .field("account_context_epoch", &self.account_context_epoch)
            .field("security_epoch", &self.security_epoch)
            .field(
                "authorization_snapshot_revision",
                &self.authorization_snapshot_revision,
            )
            .field("registration_evidence", &self.registration_evidence)
            .field("revocation_sync_evidence", &self.revocation_sync_evidence)
            .field("target_count", &self.targets.len())
            .field("lineage_tombstone_count", &self.lineage_tombstones.len())
            .finish_non_exhaustive()
    }
}

impl StoredPairedTargetRegistryV1 {
    fn validate(&self) -> Result<(), AgentPortableRemoteError> {
        validate_uuid("account", &self.account_id)?;
        validate_uuid("project", &self.project_id)?;
        validate_uuid("local registration", &self.local_registration_id)?;
        validate_uuid("local device", &self.local_device_id)?;
        validate_uuid("local installation", &self.local_installation_id)?;
        validate_endpoint_id(&self.controller_endpoint_id)?;
        validate_backend_counter(self.controller_endpoint_epoch)?;
        validate_backend_counter(self.account_context_epoch)?;
        validate_backend_counter(self.security_epoch)?;
        validate_backend_counter(self.authorization_snapshot_revision)?;
        self.registration_evidence.validate(None)?;
        self.revocation_sync_evidence.validate(None)?;
        if self.targets.len() > MAX_CURRENT_TARGETS
            || self.lineage_tombstones.len() > MAX_RETAINED_LINEAGE_TOMBSTONES
        {
            return Err(AgentPortableRemoteError::InvalidStoredRegistry);
        }

        let target_keys = self
            .targets
            .iter()
            .map(|target| target.host_registration_id.as_str())
            .collect::<Vec<_>>();
        if !is_strictly_sorted_unique(&target_keys) {
            return Err(AgentPortableRemoteError::DuplicateStoredTarget);
        }
        let tombstone_keys = self
            .lineage_tombstones
            .iter()
            .map(|tombstone| {
                (
                    tombstone.host_registration_id.as_str(),
                    tombstone.retired_pairing_incarnation,
                )
            })
            .collect::<Vec<_>>();
        if !is_strictly_sorted_unique(&tombstone_keys) {
            return Err(AgentPortableRemoteError::DuplicateStoredTarget);
        }

        let mut pair_ids = HashSet::with_capacity(self.targets.len());
        let mut host_endpoint_ids = HashSet::with_capacity(self.targets.len());
        for target in &self.targets {
            target.validate()?;
            if target.host_endpoint_id == self.controller_endpoint_id
                || !pair_ids.insert(target.pair_id.as_str())
                || !host_endpoint_ids.insert(target.host_endpoint_id.as_str())
            {
                return Err(AgentPortableRemoteError::StoredRegistryEquivocation);
            }
            let retired_floor = self
                .lineage_tombstones
                .iter()
                .filter(|tombstone| tombstone.host_registration_id == target.host_registration_id)
                .map(|tombstone| tombstone.retired_pairing_incarnation)
                .max();
            if retired_floor.is_some_and(|floor| target.pairing_incarnation <= floor) {
                return Err(AgentPortableRemoteError::StoredRegistryRollback);
            }
        }
        for tombstone in &self.lineage_tombstones {
            tombstone.validate()?;
            if !pair_ids.insert(tombstone.pair_id.as_str()) {
                return Err(AgentPortableRemoteError::StoredRegistryEquivocation);
            }
        }
        Ok(())
    }
}

#[derive(Clone, PartialEq, Eq)]
pub(crate) struct StoredRegistryEnvelopeV1 {
    storage_key_digest: PairedTargetStorageKeyDigest,
    storage_revision: u64,
    registry: StoredPairedTargetRegistryV1,
    body_digest: [u8; DIGEST_BYTES],
    record_checksum: [u8; DIGEST_BYTES],
}

impl std::fmt::Debug for StoredRegistryEnvelopeV1 {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("StoredRegistryEnvelopeV1")
            .field("storage_key_digest", &self.storage_key_digest)
            .field("storage_revision", &self.storage_revision)
            .field("registry", &self.registry)
            .field("body_digest", &"<redacted>")
            .field("record_checksum", &"<redacted>")
            .finish()
    }
}

impl StoredRegistryEnvelopeV1 {
    fn new(
        storage_key_digest: PairedTargetStorageKeyDigest,
        storage_revision: u64,
        registry: StoredPairedTargetRegistryV1,
    ) -> Result<Self, AgentPortableRemoteError> {
        storage_key_digest.validate()?;
        validate_backend_counter(storage_revision)?;
        registry.validate()?;
        let body = encode_registry_body(&registry)?;
        let body_digest = digest_parts(STORED_BODY_DIGEST_DOMAIN, &[&body]);
        let record_checksum = state_checksum(
            storage_key_digest,
            storage_revision,
            body.len(),
            &body,
            body_digest,
        );
        Ok(Self {
            storage_key_digest,
            storage_revision,
            registry,
            body_digest,
            record_checksum,
        })
    }

    fn encode(&self) -> Result<Vec<u8>, AgentPortableRemoteError> {
        self.storage_key_digest.validate()?;
        validate_backend_counter(self.storage_revision)?;
        self.registry.validate()?;
        let body = encode_registry_body(&self.registry)?;
        let body_digest = digest_parts(STORED_BODY_DIGEST_DOMAIN, &[&body]);
        let checksum = state_checksum(
            self.storage_key_digest,
            self.storage_revision,
            body.len(),
            &body,
            body_digest,
        );
        if body_digest != self.body_digest || checksum != self.record_checksum {
            return Err(AgentPortableRemoteError::CorruptStoredRegistry);
        }
        let mut encoded = Vec::with_capacity(STATE_FIXED_BYTES + body.len());
        encoded.extend_from_slice(STORED_STATE_MAGIC);
        encoded.extend_from_slice(&STORED_SCHEMA_VERSION.to_be_bytes());
        encoded.push(STORED_STATE_KIND);
        encoded.extend_from_slice(&self.storage_key_digest.0);
        encoded.extend_from_slice(&self.storage_revision.to_be_bytes());
        encoded.extend_from_slice(
            &u32::try_from(body.len())
                .map_err(|_| AgentPortableRemoteError::InvalidStoredRegistry)?
                .to_be_bytes(),
        );
        encoded.extend_from_slice(&body);
        encoded.extend_from_slice(&body_digest);
        encoded.extend_from_slice(&checksum);
        Ok(encoded)
    }

    fn decode(encoded: &[u8]) -> Result<Self, AgentPortableRemoteError> {
        if encoded.len() < STATE_FIXED_BYTES || encoded.len() > MAX_STORED_REGISTRY_BYTES {
            return Err(AgentPortableRemoteError::CorruptStoredRegistry);
        }
        if &encoded[..8] != STORED_STATE_MAGIC {
            return Err(AgentPortableRemoteError::CorruptStoredRegistry);
        }
        let version = read_u16(encoded, 8)?;
        if version != STORED_SCHEMA_VERSION {
            return Err(AgentPortableRemoteError::UnsupportedStoredVersion);
        }
        if encoded[10] != STORED_STATE_KIND {
            return Err(AgentPortableRemoteError::CorruptStoredRegistry);
        }
        let storage_key_digest =
            PairedTargetStorageKeyDigest(read_array::<DIGEST_BYTES>(encoded, 11)?);
        storage_key_digest.validate()?;
        let storage_revision = read_u64(encoded, 43)?;
        validate_backend_counter(storage_revision)?;
        let body_len = usize::try_from(read_u32(encoded, 51)?)
            .map_err(|_| AgentPortableRemoteError::CorruptStoredRegistry)?;
        if body_len == 0 || body_len > MAX_STORED_BODY_BYTES {
            return Err(AgentPortableRemoteError::CorruptStoredRegistry);
        }
        let expected_len = STATE_FIXED_BYTES
            .checked_add(body_len)
            .ok_or(AgentPortableRemoteError::CorruptStoredRegistry)?;
        if encoded.len() != expected_len {
            return Err(AgentPortableRemoteError::CorruptStoredRegistry);
        }
        let body_start = 55;
        let body_end = body_start + body_len;
        let body = &encoded[body_start..body_end];
        let stored_body_digest = read_array::<DIGEST_BYTES>(encoded, body_end)?;
        let stored_checksum = read_array::<DIGEST_BYTES>(encoded, body_end + DIGEST_BYTES)?;
        let expected_body_digest = digest_parts(STORED_BODY_DIGEST_DOMAIN, &[body]);
        let expected_checksum = state_checksum(
            storage_key_digest,
            storage_revision,
            body_len,
            body,
            expected_body_digest,
        );
        if stored_body_digest != expected_body_digest || stored_checksum != expected_checksum {
            return Err(AgentPortableRemoteError::CorruptStoredRegistry);
        }
        let registry: StoredPairedTargetRegistryV1 = decode_json_exact(body)?;
        registry.validate()?;
        let canonical_body = encode_registry_body(&registry)?;
        if canonical_body != body {
            return Err(AgentPortableRemoteError::CorruptStoredRegistry);
        }
        Ok(Self {
            storage_key_digest,
            storage_revision,
            registry,
            body_digest: stored_body_digest,
            record_checksum: stored_checksum,
        })
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
struct StoredRegistryGuardV1 {
    storage_key_digest: PairedTargetStorageKeyDigest,
    committed_revision: u64,
    committed_state_digest: [u8; DIGEST_BYTES],
    checksum: [u8; DIGEST_BYTES],
}

impl std::fmt::Debug for StoredRegistryGuardV1 {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("StoredRegistryGuardV1")
            .field("storage_key_digest", &self.storage_key_digest)
            .field("committed_revision", &self.committed_revision)
            .field("committed_state_digest", &"<redacted>")
            .field("checksum", &"<redacted>")
            .finish()
    }
}

impl StoredRegistryGuardV1 {
    fn initial(
        storage_key_digest: PairedTargetStorageKeyDigest,
    ) -> Result<Self, AgentPortableRemoteError> {
        Self::new(storage_key_digest, 0, [0; DIGEST_BYTES])
    }

    fn committed(envelope: &StoredRegistryEnvelopeV1) -> Result<Self, AgentPortableRemoteError> {
        Self::new(
            envelope.storage_key_digest,
            envelope.storage_revision,
            envelope.record_checksum,
        )
    }

    fn new(
        storage_key_digest: PairedTargetStorageKeyDigest,
        committed_revision: u64,
        committed_state_digest: [u8; DIGEST_BYTES],
    ) -> Result<Self, AgentPortableRemoteError> {
        storage_key_digest.validate()?;
        if (committed_revision == 0) != committed_state_digest.iter().all(|byte| *byte == 0) {
            return Err(AgentPortableRemoteError::InvalidStoredRegistry);
        }
        if committed_revision > MAX_BACKEND_COUNTER {
            return Err(AgentPortableRemoteError::InvalidStoredRegistry);
        }
        let checksum = guard_checksum(
            storage_key_digest,
            committed_revision,
            committed_state_digest,
        );
        Ok(Self {
            storage_key_digest,
            committed_revision,
            committed_state_digest,
            checksum,
        })
    }

    fn encode(self) -> Result<Vec<u8>, AgentPortableRemoteError> {
        self.storage_key_digest.validate()?;
        if (self.committed_revision == 0)
            != self.committed_state_digest.iter().all(|byte| *byte == 0)
            || self.checksum
                != guard_checksum(
                    self.storage_key_digest,
                    self.committed_revision,
                    self.committed_state_digest,
                )
        {
            return Err(AgentPortableRemoteError::CorruptStoredRegistry);
        }
        let mut encoded = Vec::with_capacity(GUARD_FIXED_BYTES);
        encoded.extend_from_slice(STORED_GUARD_MAGIC);
        encoded.extend_from_slice(&STORED_SCHEMA_VERSION.to_be_bytes());
        encoded.push(STORED_GUARD_KIND);
        encoded.extend_from_slice(&self.storage_key_digest.0);
        encoded.extend_from_slice(&self.committed_revision.to_be_bytes());
        encoded.extend_from_slice(&self.committed_state_digest);
        encoded.extend_from_slice(&self.checksum);
        Ok(encoded)
    }

    fn decode(encoded: &[u8]) -> Result<Self, AgentPortableRemoteError> {
        if encoded.len() != GUARD_FIXED_BYTES || &encoded[..8] != STORED_GUARD_MAGIC {
            return Err(AgentPortableRemoteError::CorruptStoredRegistry);
        }
        let version = read_u16(encoded, 8)?;
        if version != STORED_SCHEMA_VERSION {
            return Err(AgentPortableRemoteError::UnsupportedStoredVersion);
        }
        if encoded[10] != STORED_GUARD_KIND {
            return Err(AgentPortableRemoteError::CorruptStoredRegistry);
        }
        let storage_key_digest = PairedTargetStorageKeyDigest(read_array(encoded, 11)?);
        storage_key_digest.validate()?;
        let committed_revision = read_u64(encoded, 43)?;
        let committed_state_digest = read_array(encoded, 51)?;
        let checksum = read_array(encoded, 83)?;
        let guard = Self::new(
            storage_key_digest,
            committed_revision,
            committed_state_digest,
        )?;
        if guard.checksum != checksum {
            return Err(AgentPortableRemoteError::CorruptStoredRegistry);
        }
        Ok(guard)
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) struct StoredRegistryCommitToken {
    storage_key_digest: PairedTargetStorageKeyDigest,
    committed_revision: u64,
    committed_state_digest: [u8; DIGEST_BYTES],
}

impl std::fmt::Debug for StoredRegistryCommitToken {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("StoredRegistryCommitToken")
            .field("storage_key_digest", &self.storage_key_digest)
            .field("committed_revision", &self.committed_revision)
            .field("committed_state_digest", &"<redacted>")
            .finish()
    }
}

impl From<StoredRegistryGuardV1> for StoredRegistryCommitToken {
    fn from(guard: StoredRegistryGuardV1) -> Self {
        Self {
            storage_key_digest: guard.storage_key_digest,
            committed_revision: guard.committed_revision,
            committed_state_digest: guard.committed_state_digest,
        }
    }
}

#[derive(Clone)]
pub(crate) enum StoredRegistryLoad {
    Empty {
        token: StoredRegistryCommitToken,
    },
    Committed {
        envelope: Box<StoredRegistryEnvelopeV1>,
        token: StoredRegistryCommitToken,
    },
}

impl std::fmt::Debug for StoredRegistryLoad {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Empty { token } => formatter
                .debug_struct("Empty")
                .field("token", token)
                .finish(),
            Self::Committed { envelope, token } => formatter
                .debug_struct("Committed")
                .field("storage_revision", &envelope.storage_revision)
                .field("token", token)
                .finish(),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum StoredRegistryCommitOutcome {
    Committed,
    AlreadyCommitted,
}

/// Sealed authorization to persist one complete registry replacement.
///
/// There is intentionally no production constructor in this slice. A future
/// released SDK verifier must own construction after authenticating a complete
/// control-plane snapshot and the installation-global account context. Raw
/// stored bytes can never mint this value.
pub(crate) struct VerifiedStoredRegistryReplacement {
    envelope: StoredRegistryEnvelopeV1,
}

impl std::fmt::Debug for VerifiedStoredRegistryReplacement {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("VerifiedStoredRegistryReplacement")
            .field("storage_revision", &self.envelope.storage_revision)
            .finish_non_exhaustive()
    }
}

#[cfg(test)]
impl VerifiedStoredRegistryReplacement {
    fn for_test(envelope: StoredRegistryEnvelopeV1) -> Self {
        Self { envelope }
    }
}

pub(crate) trait PairedTargetAuthorityStore: Send + Sync {
    /// Loads the single installation-scoped active registry slot. The key is
    /// installation-scoped; account changes replace this same slot by CAS.
    fn load(
        &self,
        expected_key: PairedTargetStorageKeyDigest,
    ) -> Result<StoredRegistryLoad, AgentPortableRemoteError>;

    /// Replaces the entire registry and its guard. A production implementation
    /// must put both records behind its secure-store lock and preserve the exact
    /// interrupted/equivocation classification used by the in-memory reference.
    fn compare_and_replace(
        &self,
        expected: StoredRegistryCommitToken,
        replacement: &VerifiedStoredRegistryReplacement,
    ) -> Result<StoredRegistryCommitOutcome, AgentPortableRemoteError>;
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[cfg_attr(not(test), allow(dead_code))]
enum InMemoryStoreFault {
    BeforeState,
    AfterState,
    AfterGuard,
}

#[derive(Clone, Default)]
pub(crate) struct InMemoryPairedTargetAuthorityStore {
    inner: Arc<Mutex<InMemoryPairedTargetAuthorityStoreState>>,
}

#[derive(Default)]
struct InMemoryPairedTargetAuthorityStoreState {
    state_record: Option<Vec<u8>>,
    guard_record: Option<Vec<u8>>,
    next_fault: Option<InMemoryStoreFault>,
}

impl std::fmt::Debug for InMemoryPairedTargetAuthorityStore {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("InMemoryPairedTargetAuthorityStore(<redacted>)")
    }
}

impl InMemoryPairedTargetAuthorityStore {
    fn take_fault_if(
        state: &mut InMemoryPairedTargetAuthorityStoreState,
        expected: InMemoryStoreFault,
    ) -> bool {
        if state.next_fault == Some(expected) {
            state.next_fault = None;
            true
        } else {
            false
        }
    }

    #[cfg(test)]
    fn inject_fault_once(&self, fault: InMemoryStoreFault) {
        self.inner.lock().unwrap().next_fault = Some(fault);
    }

    #[cfg(test)]
    fn raw_snapshot(&self) -> (Option<Vec<u8>>, Option<Vec<u8>>) {
        let state = self.inner.lock().unwrap();
        (state.state_record.clone(), state.guard_record.clone())
    }

    #[cfg(test)]
    fn restore_raw_snapshot(&self, snapshot: (Option<Vec<u8>>, Option<Vec<u8>>)) {
        let mut state = self.inner.lock().unwrap();
        state.state_record = snapshot.0;
        state.guard_record = snapshot.1;
        state.next_fault = None;
    }

    #[cfg(test)]
    fn mutate_state_record(&self, mutate: impl FnOnce(&mut Vec<u8>)) {
        if let Some(record) = self.inner.lock().unwrap().state_record.as_mut() {
            mutate(record);
        }
    }
}

impl PairedTargetAuthorityStore for InMemoryPairedTargetAuthorityStore {
    fn load(
        &self,
        expected_key: PairedTargetStorageKeyDigest,
    ) -> Result<StoredRegistryLoad, AgentPortableRemoteError> {
        expected_key.validate()?;
        let state = self.inner.lock().unwrap();
        classify_stored_registry_slot(
            state.state_record.as_deref(),
            state.guard_record.as_deref(),
            expected_key,
        )
    }

    fn compare_and_replace(
        &self,
        expected: StoredRegistryCommitToken,
        replacement: &VerifiedStoredRegistryReplacement,
    ) -> Result<StoredRegistryCommitOutcome, AgentPortableRemoteError> {
        let candidate = &replacement.envelope;
        expected.storage_key_digest.validate()?;
        if candidate.storage_key_digest != expected.storage_key_digest {
            return Err(AgentPortableRemoteError::StoredRegistryConflict);
        }
        let candidate_state = candidate.encode()?;
        let candidate_guard = StoredRegistryGuardV1::committed(candidate)?;
        let candidate_guard_record = candidate_guard.encode()?;

        let mut state = self.inner.lock().unwrap();
        let decoded_state = state
            .state_record
            .as_deref()
            .map(StoredRegistryEnvelopeV1::decode)
            .transpose()?;
        let decoded_guard = state
            .guard_record
            .as_deref()
            .map(StoredRegistryGuardV1::decode)
            .transpose()?;

        if decoded_state.as_ref() == Some(candidate) && decoded_guard == Some(candidate_guard) {
            return Ok(StoredRegistryCommitOutcome::AlreadyCommitted);
        }

        let next_revision = expected
            .committed_revision
            .checked_add(1)
            .ok_or(AgentPortableRemoteError::StoredRegistryConflict)?;
        if candidate.storage_revision != next_revision {
            return Err(AgentPortableRemoteError::StoredRegistryConflict);
        }

        let expected_guard = StoredRegistryGuardV1::new(
            expected.storage_key_digest,
            expected.committed_revision,
            expected.committed_state_digest,
        )?;
        let current_guard =
            decoded_guard.unwrap_or(StoredRegistryGuardV1::initial(expected.storage_key_digest)?);
        if current_guard != expected_guard {
            return Err(AgentPortableRemoteError::StoredRegistryConflict);
        }

        let current = classify_stored_registry_slot(
            state.state_record.as_deref(),
            state.guard_record.as_deref(),
            expected.storage_key_digest,
        )?;
        let current_token = match &current {
            StoredRegistryLoad::Empty { token } | StoredRegistryLoad::Committed { token, .. } => {
                *token
            }
        };
        if current_token != expected {
            return Err(AgentPortableRemoteError::StoredRegistryConflict);
        }
        if let StoredRegistryLoad::Committed { envelope, .. } = current {
            validate_stored_registry_transition(&envelope.registry, &candidate.registry)?;
        }

        if Self::take_fault_if(&mut state, InMemoryStoreFault::BeforeState) {
            return Err(AgentPortableRemoteError::StoredRegistryInterrupted);
        }
        state.state_record = Some(candidate_state);
        if Self::take_fault_if(&mut state, InMemoryStoreFault::AfterState) {
            return Err(AgentPortableRemoteError::StoredRegistryInterrupted);
        }
        state.guard_record = Some(candidate_guard_record);
        if Self::take_fault_if(&mut state, InMemoryStoreFault::AfterGuard) {
            return Err(AgentPortableRemoteError::StoredRegistryInterrupted);
        }
        Ok(StoredRegistryCommitOutcome::Committed)
    }
}

fn classify_stored_registry_slot(
    state_record: Option<&[u8]>,
    guard_record: Option<&[u8]>,
    expected_key: PairedTargetStorageKeyDigest,
) -> Result<StoredRegistryLoad, AgentPortableRemoteError> {
    let state = state_record
        .map(StoredRegistryEnvelopeV1::decode)
        .transpose()?;
    let guard = guard_record
        .map(StoredRegistryGuardV1::decode)
        .transpose()?
        .unwrap_or(StoredRegistryGuardV1::initial(expected_key)?);
    if guard.storage_key_digest != expected_key
        || state
            .as_ref()
            .is_some_and(|envelope| envelope.storage_key_digest != expected_key)
    {
        return Err(AgentPortableRemoteError::AccountMismatch);
    }
    let token = StoredRegistryCommitToken::from(guard);
    match state {
        None if guard.committed_revision == 0 => Ok(StoredRegistryLoad::Empty { token }),
        None => Err(AgentPortableRemoteError::StoredRegistryRollback),
        Some(envelope) if envelope.storage_revision < guard.committed_revision => {
            Err(AgentPortableRemoteError::StoredRegistryRollback)
        }
        Some(envelope) if envelope.storage_revision > guard.committed_revision => {
            if guard.committed_revision.checked_add(1) == Some(envelope.storage_revision) {
                Err(AgentPortableRemoteError::StoredRegistryInterrupted)
            } else {
                Err(AgentPortableRemoteError::CorruptStoredRegistry)
            }
        }
        Some(envelope) if envelope.record_checksum != guard.committed_state_digest => {
            Err(AgentPortableRemoteError::StoredRegistryEquivocation)
        }
        Some(envelope) => Ok(StoredRegistryLoad::Committed {
            envelope: Box::new(envelope),
            token,
        }),
    }
}

fn validate_stored_registry_transition(
    previous: &StoredPairedTargetRegistryV1,
    next: &StoredPairedTargetRegistryV1,
) -> Result<(), AgentPortableRemoteError> {
    if next.account_context_epoch < previous.account_context_epoch {
        return Err(AgentPortableRemoteError::StoredRegistryRollback);
    }
    if next.account_context_epoch > previous.account_context_epoch {
        // Only a sealed complete replacement reaches this function. A newer
        // installation-global context is the reset boundary even when a
        // sign-out/re-auth cycle happens to reuse every scalar identity.
        return Ok(());
    }
    // Within one installation-global context, the complete authenticated
    // authority snapshot is immutable. Storage metadata may be replayed at a
    // later storage revision, but every semantic change requires a freshly
    // sealed complete replacement with a strictly newer context epoch. This
    // conservatively fences revocation, re-pair, tombstone, hint, and transport
    // lineage transitions behind one native account-context boundary.
    if next == previous {
        Ok(())
    } else {
        Err(AgentPortableRemoteError::StoredRegistryEquivocation)
    }
}

#[derive(Clone)]
pub(crate) struct PortableCancellation {
    inner: Arc<PortableCancellationInner>,
}

struct PortableCancellationInner {
    cancelled: AtomicBool,
    notify: Notify,
}

impl PortableCancellation {
    fn new() -> Self {
        Self {
            inner: Arc::new(PortableCancellationInner {
                cancelled: AtomicBool::new(false),
                notify: Notify::new(),
            }),
        }
    }

    fn cancel(&self) {
        if !self.inner.cancelled.swap(true, Ordering::AcqRel) {
            self.inner.notify.notify_waiters();
        }
    }

    pub(crate) fn is_cancelled(&self) -> bool {
        self.inner.cancelled.load(Ordering::Acquire)
    }

    pub(crate) async fn cancelled(&self) {
        loop {
            let notified = self.inner.notify.notified();
            if self.is_cancelled() {
                return;
            }
            notified.await;
        }
    }
}

impl std::fmt::Debug for PortableCancellation {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("PortableCancellation")
            .field("cancelled", &self.is_cancelled())
            .finish()
    }
}

#[derive(Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub(crate) struct PortableTargetHandle(String);

impl std::fmt::Debug for PortableTargetHandle {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("PortableTargetHandle(<opaque>)")
    }
}

impl PortableTargetHandle {
    fn issue() -> Result<Self, AgentPortableRemoteError> {
        Ok(Self(issue_opaque_identifier("target")?))
    }

    fn validate(&self) -> Result<(), AgentPortableRemoteError> {
        validate_opaque_identifier(&self.0, "target")
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct PortableTargetDescriptor {
    handle: PortableTargetHandle,
    label: String,
}

impl PortableTargetDescriptor {
    fn validate(&self) -> Result<(), AgentPortableRemoteError> {
        self.handle.validate()?;
        validate_display_label(&self.label).map_err(|_| AgentPortableRemoteError::InvalidResponse)
    }
}

#[derive(Clone, PartialEq, Eq)]
pub(crate) struct PortableTargetLease {
    target_id: String,
    host_epoch: u64,
    connection_generation: u64,
}

impl std::fmt::Debug for PortableTargetLease {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("PortableTargetLease")
            .field("target_id", &"<opaque>")
            .field("host_epoch", &self.host_epoch)
            .field("connection_generation", &self.connection_generation)
            .finish()
    }
}

impl PortableTargetLease {
    fn issue(seed: PortablePeerLeaseSeed) -> Result<Self, AgentPortableRemoteError> {
        seed.validate()?;
        Ok(Self {
            target_id: issue_opaque_identifier("lease")?,
            host_epoch: seed.host_epoch,
            connection_generation: seed.connection_generation,
        })
    }

    fn validate(&self) -> Result<(), AgentPortableRemoteError> {
        validate_opaque_identifier(&self.target_id, "lease")?;
        if self.host_epoch == 0
            || self.connection_generation == 0
            || self.connection_generation > MAX_JAVASCRIPT_SAFE_UNSIGNED_INTEGER
        {
            return Err(AgentPortableRemoteError::StaleLease);
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct PortablePageRequest {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    cursor: Option<String>,
    limit: u16,
}

impl PortablePageRequest {
    fn validate(&self) -> Result<(), AgentPortableRemoteError> {
        validate_page_limit_and_cursor(self.limit, self.cursor.as_deref())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct PortableRecordsPageRequest {
    session_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    cursor: Option<String>,
    limit: u16,
}

impl PortableRecordsPageRequest {
    fn validate(&self) -> Result<(), AgentPortableRemoteError> {
        validate_safe_id(&self.session_id).map_err(|_| AgentPortableRemoteError::InvalidRequest)?;
        validate_page_limit_and_cursor(self.limit, self.cursor.as_deref())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct PortableRuntimeStatus {
    running: bool,
    active_run_count: u16,
}

impl PortableRuntimeStatus {
    fn validate(&self) -> Result<(), AgentPortableRemoteError> {
        if self.active_run_count > MAX_RUNTIME_ACTIVE_RUNS
            || (!self.running && self.active_run_count != 0)
        {
            return Err(AgentPortableRemoteError::InvalidResponse);
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct PortableSessionSummary {
    id: String,
    title: String,
    created_ms: i64,
    updated_ms: i64,
    page_sort_ms: i64,
    message_count: u64,
}

impl PortableSessionSummary {
    fn validate(&self) -> Result<(), AgentPortableRemoteError> {
        validate_safe_id(&self.id)?;
        validate_safe_display_text(&self.title, 1_024, false)?;
        for timestamp in [self.created_ms, self.updated_ms, self.page_sort_ms] {
            if !(0..=MAX_JAVASCRIPT_SAFE_INTEGER).contains(&timestamp) {
                return Err(AgentPortableRemoteError::InvalidResponse);
            }
        }
        if self.message_count > MAX_JAVASCRIPT_SAFE_UNSIGNED_INTEGER {
            return Err(AgentPortableRemoteError::InvalidResponse);
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct PortableSessionPage {
    items: Vec<PortableSessionSummary>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    next_cursor: Option<String>,
}

impl PortableSessionPage {
    fn validate_for(&self, request: &PortablePageRequest) -> Result<(), AgentPortableRemoteError> {
        request.validate()?;
        validate_page_shape(
            self.items.len(),
            request.limit,
            request.cursor.as_deref(),
            self.next_cursor.as_deref(),
        )?;
        let mut seen_ids = HashSet::with_capacity(self.items.len());
        for item in &self.items {
            item.validate()?;
            if !seen_ids.insert(item.id.as_str()) {
                return Err(AgentPortableRemoteError::InvalidResponse);
            }
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct PortableTimelineItem {
    id: String,
    item_type: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    role: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    title: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    text: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    status: Option<String>,
    created_ms: u64,
    merge: String,
}

impl PortableTimelineItem {
    fn validate(&self) -> Result<(), AgentPortableRemoteError> {
        const SAFE_TOOL_TITLE: &str = "Tool activity";
        const SAFE_TOOL_FAILED: &str =
            "The tool failed. Open the host for additional diagnostic details.";
        const SAFE_TOOL_CANCELLED: &str = "The tool was cancelled.";
        const SAFE_PERMISSION_TITLE: &str = "Tool permission";
        const SAFE_AGENT_ERROR: &str =
            "The Agent task failed. Open the host for additional diagnostic details.";

        validate_safe_id(&self.id)?;
        if self.created_ms > MAX_JAVASCRIPT_SAFE_UNSIGNED_INTEGER
            || !matches!(
                self.item_type.as_str(),
                "message" | "thinking" | "tool" | "permission" | "system" | "error"
            )
            || self
                .role
                .as_deref()
                .is_some_and(|role| !matches!(role, "user" | "assistant" | "thought" | "system"))
            || !matches!(self.merge.as_str(), "append" | "replace")
        {
            return Err(AgentPortableRemoteError::InvalidResponse);
        }
        validate_optional_safe_display_text(self.title.as_deref(), 1_024)?;
        validate_optional_safe_display_text(self.status.as_deref(), 64)?;
        if self
            .text
            .as_deref()
            .is_some_and(|text| text.len() > MAX_TIMELINE_TEXT_BYTES || text.contains('\0'))
        {
            return Err(AgentPortableRemoteError::InvalidResponse);
        }
        match self.item_type.as_str() {
            "tool" => {
                let expected_text = match self.status.as_deref() {
                    None | Some("pending" | "running" | "completed") => None,
                    Some("failed" | "error") => Some(SAFE_TOOL_FAILED),
                    Some("cancelled") => Some(SAFE_TOOL_CANCELLED),
                    Some(_) => return Err(AgentPortableRemoteError::InvalidResponse),
                };
                if self.role.as_deref() != Some("assistant")
                    || self.title.as_deref() != Some(SAFE_TOOL_TITLE)
                    || self.text.as_deref() != expected_text
                {
                    return Err(AgentPortableRemoteError::InvalidResponse);
                }
            }
            "permission" => {
                if self.role.as_deref() != Some("system")
                    || self.title.as_deref() != Some(SAFE_PERMISSION_TITLE)
                    || self.text.is_some()
                    || !matches!(
                        self.status.as_deref(),
                        Some("allow_once" | "deny_once" | "completed" | "cancelled")
                    )
                {
                    return Err(AgentPortableRemoteError::InvalidResponse);
                }
            }
            "error" => {
                if self.role.as_deref() != Some("system")
                    || self.title.as_deref() != Some("Agent error")
                    || self.text.as_deref() != Some(SAFE_AGENT_ERROR)
                    || self.status.as_deref() != Some("failed")
                {
                    return Err(AgentPortableRemoteError::InvalidResponse);
                }
            }
            _ => {}
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct PortableHistoryRecord {
    record_id: String,
    role: String,
    created_ms: u64,
    items: Vec<PortableTimelineItem>,
}

impl PortableHistoryRecord {
    fn validate(&self) -> Result<(), AgentPortableRemoteError> {
        validate_safe_cursor(&self.record_id)?;
        if self.role.is_empty()
            || self.role.len() > MAX_ID_BYTES
            || !self
                .role
                .bytes()
                .all(|byte| byte.is_ascii_graphic() || byte == b' ')
            || self.created_ms > MAX_JAVASCRIPT_SAFE_UNSIGNED_INTEGER
            || self.items.len() > MAX_HISTORY_ITEMS_PER_RECORD
        {
            return Err(AgentPortableRemoteError::InvalidResponse);
        }
        for item in &self.items {
            item.validate()?;
        }
        let mut presentation = Vec::new();
        ciborium::ser::into_writer(self, &mut presentation)
            .map_err(|_| AgentPortableRemoteError::InvalidResponse)?;
        if presentation.len() > MAX_PORTABLE_HISTORY_RECORD_BYTES {
            return Err(AgentPortableRemoteError::InvalidResponse);
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct PortableHistoryPage {
    items: Vec<PortableHistoryRecord>,
    history_revision: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    next_cursor: Option<String>,
}

impl PortableHistoryPage {
    fn validate_for(
        &self,
        request: &PortableRecordsPageRequest,
    ) -> Result<(), AgentPortableRemoteError> {
        request.validate()?;
        validate_safe_cursor(&self.history_revision)?;
        validate_page_shape(
            self.items.len(),
            request.limit,
            request.cursor.as_deref(),
            self.next_cursor.as_deref(),
        )?;
        let mut seen_record_ids = HashSet::with_capacity(self.items.len());
        for item in &self.items {
            item.validate()?;
            if !seen_record_ids.insert(item.record_id.as_str()) {
                return Err(AgentPortableRemoteError::InvalidResponse);
            }
        }
        Ok(())
    }
}

fn validate_page_limit_and_cursor(
    limit: u16,
    cursor: Option<&str>,
) -> Result<(), AgentPortableRemoteError> {
    if !(1..=MAX_PAGE_SIZE).contains(&limit) {
        return Err(AgentPortableRemoteError::InvalidRequest);
    }
    if let Some(cursor) = cursor {
        validate_safe_cursor(cursor).map_err(|_| AgentPortableRemoteError::InvalidRequest)?;
    }
    Ok(())
}

fn validate_page_shape(
    item_count: usize,
    requested_limit: u16,
    request_cursor: Option<&str>,
    next_cursor: Option<&str>,
) -> Result<(), AgentPortableRemoteError> {
    if item_count > usize::from(requested_limit)
        || (item_count == 0 && next_cursor.is_some())
        || (next_cursor.is_some() && next_cursor == request_cursor)
    {
        return Err(AgentPortableRemoteError::InvalidResponse);
    }
    if let Some(cursor) = next_cursor {
        validate_safe_cursor(cursor).map_err(|_| AgentPortableRemoteError::InvalidResponse)?;
    }
    Ok(())
}

fn validate_safe_id(value: &str) -> Result<(), AgentPortableRemoteError> {
    if value.len() > MAX_ID_BYTES {
        return Err(AgentPortableRemoteError::InvalidResponse);
    }
    validate_safe_cursor(value)
}

fn validate_safe_cursor(value: &str) -> Result<(), AgentPortableRemoteError> {
    if value.is_empty()
        || value.len() > MAX_CURSOR_BYTES
        || !value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.' | b':'))
    {
        return Err(AgentPortableRemoteError::InvalidResponse);
    }
    Ok(())
}

fn validate_safe_display_text(
    value: &str,
    max_bytes: usize,
    allow_empty: bool,
) -> Result<(), AgentPortableRemoteError> {
    if (!allow_empty && value.is_empty())
        || value.len() > max_bytes
        || value.chars().any(is_unsafe_display_character)
    {
        return Err(AgentPortableRemoteError::InvalidResponse);
    }
    Ok(())
}

fn validate_optional_safe_display_text(
    value: Option<&str>,
    max_bytes: usize,
) -> Result<(), AgentPortableRemoteError> {
    value.map_or(Ok(()), |value| {
        validate_safe_display_text(value, max_bytes, true)
    })
}

fn issue_opaque_identifier(prefix: &str) -> Result<String, AgentPortableRemoteError> {
    let mut random = [0u8; 24];
    fill_random(&mut random).map_err(|_| AgentPortableRemoteError::Internal)?;
    let mut value = String::with_capacity(prefix.len() + 1 + random.len() * 2);
    value.push_str(prefix);
    value.push('_');
    for byte in random {
        use std::fmt::Write as _;
        write!(&mut value, "{byte:02x}").map_err(|_| AgentPortableRemoteError::Internal)?;
    }
    Ok(value)
}

fn validate_opaque_identifier(value: &str, prefix: &str) -> Result<(), AgentPortableRemoteError> {
    let expected_len = prefix.len() + 1 + 48;
    if value.len() != expected_len
        || !value.starts_with(prefix)
        || value.as_bytes().get(prefix.len()) != Some(&b'_')
        || !value[prefix.len() + 1..]
            .bytes()
            .all(|byte| matches!(byte, b'0'..=b'9' | b'a'..=b'f'))
    {
        return Err(AgentPortableRemoteError::StaleLease);
    }
    Ok(())
}

#[derive(Clone, PartialEq, Eq)]
pub(crate) struct NativePortableCredentialClaims {
    account_id: String,
    project_id: String,
    local_registration_id: String,
    local_device_id: String,
    local_installation_id: String,
    controller_endpoint_id: String,
    controller_endpoint_epoch: u64,
    account_context_epoch: u64,
    storage_key_digest: PairedTargetStorageKeyDigest,
}

impl std::fmt::Debug for NativePortableCredentialClaims {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("NativePortableCredentialClaims")
            .field("controller_endpoint_epoch", &self.controller_endpoint_epoch)
            .field("account_context_epoch", &self.account_context_epoch)
            .field("storage_key_digest", &self.storage_key_digest)
            .finish_non_exhaustive()
    }
}

impl NativePortableCredentialClaims {
    fn validate(&self) -> Result<(), AgentPortableRemoteError> {
        validate_uuid("credential account", &self.account_id)
            .map_err(|_| AgentPortableRemoteError::Unauthenticated)?;
        validate_uuid("credential project", &self.project_id)
            .map_err(|_| AgentPortableRemoteError::Unauthenticated)?;
        validate_uuid("credential registration", &self.local_registration_id)
            .map_err(|_| AgentPortableRemoteError::Unauthenticated)?;
        validate_uuid("credential device", &self.local_device_id)
            .map_err(|_| AgentPortableRemoteError::Unauthenticated)?;
        validate_uuid("credential installation", &self.local_installation_id)
            .map_err(|_| AgentPortableRemoteError::Unauthenticated)?;
        validate_endpoint_id(&self.controller_endpoint_id)
            .map_err(|_| AgentPortableRemoteError::Unauthenticated)?;
        validate_backend_counter(self.controller_endpoint_epoch)
            .map_err(|_| AgentPortableRemoteError::Unauthenticated)?;
        validate_backend_counter(self.account_context_epoch)
            .map_err(|_| AgentPortableRemoteError::Unauthenticated)?;
        self.storage_key_digest
            .validate()
            .map_err(|_| AgentPortableRemoteError::Unauthenticated)
    }
}

pub(crate) trait NativePortableCredentialLease: Send + Sync {
    fn claims(&self) -> &NativePortableCredentialClaims;

    /// Re-checks the native session and installation-global account context.
    /// Implementations must fail after sign-out or any A to B transition. The
    /// native auth owner must also await `native_credentials_invalidated`
    /// before publishing that transition so the cancellation passed into a
    /// concurrent factory dial is fenced at its linearization point.
    fn revalidate_current(
        self: Arc<Self>,
    ) -> PortableFuture<'static, Result<(), AgentPortableRemoteError>>;
}

pub(crate) trait NativePortableCredentialProvider: Send + Sync {
    fn current(
        self: Arc<Self>,
    ) -> PortableFuture<
        'static,
        Result<Arc<dyn NativePortableCredentialLease>, AgentPortableRemoteError>,
    >;
}

pub(crate) trait NativePortablePairingVerifier: Send + Sync {
    /// Verifies opaque SDK/backend evidence. Stored values remain untrusted and
    /// cannot construct this sealed registry themselves.
    fn verify_registry(
        self: Arc<Self>,
        credential: Arc<dyn NativePortableCredentialLease>,
        stored: StoredPairedTargetRegistryV1,
    ) -> PortableFuture<
        'static,
        Result<VerifiedAgentPortableTargetRegistry, AgentPortableRemoteError>,
    >;
}

#[derive(Clone, PartialEq, Eq)]
struct VerifiedLineageTombstone {
    host_registration_id: String,
    host_endpoint_id: String,
    pair_id: String,
    retired_pairing_incarnation: u64,
    retired_authorization_revision: u64,
    replay_floor: Option<StoredConnectionLineageFloorV1>,
}

impl std::fmt::Debug for VerifiedLineageTombstone {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("VerifiedLineageTombstone(<sealed>)")
    }
}

/// Type-erased, native verifier-produced transport authority.
///
/// Stored lineage bytes and digests never construct this value and are never
/// passed to a dialer. A future released verifier/peer-factory adapter may use
/// a private descendant module to wrap and downcast its transport-owned,
/// authenticated lineage codec handle. Keeping the constructor private makes
/// it impossible for sibling production code to promote stored scalars.
#[derive(Clone)]
pub(crate) struct VerifiedPortableTransportLineageAuthority {
    inner: Arc<dyn Any + Send + Sync>,
}

impl std::fmt::Debug for VerifiedPortableTransportLineageAuthority {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("VerifiedPortableTransportLineageAuthority(<sealed>)")
    }
}

impl VerifiedPortableTransportLineageAuthority {
    fn new<T: Any + Send + Sync>(authority: T) -> Self {
        Self {
            inner: Arc::new(authority),
        }
    }

    pub(crate) fn downcast_ref<T: Any + Send + Sync>(&self) -> Option<&T> {
        self.inner.downcast_ref::<T>()
    }
}

#[cfg(test)]
#[derive(Debug)]
struct TestVerifiedPortableTransportLineageAuthority;

#[derive(Clone)]
pub(crate) struct VerifiedAgentPortableTargetRegistry {
    account_id: String,
    project_id: String,
    local_registration_id: String,
    local_device_id: String,
    local_installation_id: String,
    controller_endpoint_id: String,
    controller_endpoint_epoch: u64,
    account_context_epoch: u64,
    security_epoch: u64,
    authorization_snapshot_revision: u64,
    registration_evidence_digest: [u8; DIGEST_BYTES],
    revocation_sync_evidence_digest: [u8; DIGEST_BYTES],
    complete_snapshot: bool,
    targets: Vec<Arc<VerifiedAgentPortableTarget>>,
    lineage_tombstones: Vec<VerifiedLineageTombstone>,
}

impl std::fmt::Debug for VerifiedAgentPortableTargetRegistry {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("VerifiedAgentPortableTargetRegistry")
            .field("controller_endpoint_epoch", &self.controller_endpoint_epoch)
            .field("account_context_epoch", &self.account_context_epoch)
            .field("security_epoch", &self.security_epoch)
            .field(
                "authorization_snapshot_revision",
                &self.authorization_snapshot_revision,
            )
            .field("complete_snapshot", &self.complete_snapshot)
            .field("target_count", &self.targets.len())
            .field("lineage_tombstone_count", &self.lineage_tombstones.len())
            .finish_non_exhaustive()
    }
}

#[derive(Clone)]
pub(crate) struct VerifiedAgentPortableTarget {
    pair_id: String,
    pairing_revision: u64,
    directory_revision: u64,
    host_registration_id: String,
    host_device_id: String,
    host_installation_id: String,
    host_endpoint_id: String,
    host_endpoint_epoch: u64,
    host_display_name: String,
    pairing_incarnation: u64,
    authorization_evidence_digest: [u8; DIGEST_BYTES],
    revocation_stream_id: String,
    revocation_generation: u64,
    revocation_applied_sequence: u64,
    revocation_checkpoint_digest: [u8; DIGEST_BYTES],
    connection_hints: StoredConnectionHintsV1,
    lineage_format: String,
    lineage_revision: u64,
    lineage_digest: [u8; DIGEST_BYTES],
    replay_floor: Option<StoredConnectionLineageFloorV1>,
    transport_lineage_authority: VerifiedPortableTransportLineageAuthority,
    revoked: bool,
}

impl std::fmt::Debug for VerifiedAgentPortableTarget {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("VerifiedAgentPortableTarget")
            .field("pairing_revision", &self.pairing_revision)
            .field("directory_revision", &self.directory_revision)
            .field("host_endpoint_epoch", &self.host_endpoint_epoch)
            .field("pairing_incarnation", &self.pairing_incarnation)
            .field("revocation_generation", &self.revocation_generation)
            .field(
                "revocation_applied_sequence",
                &self.revocation_applied_sequence,
            )
            .field("revoked", &self.revoked)
            .finish_non_exhaustive()
    }
}

impl VerifiedAgentPortableTarget {
    pub(crate) fn host_endpoint_id(&self) -> &str {
        &self.host_endpoint_id
    }

    pub(crate) fn host_endpoint_epoch(&self) -> u64 {
        self.host_endpoint_epoch
    }

    pub(crate) fn connection_hints(&self) -> &StoredConnectionHintsV1 {
        &self.connection_hints
    }

    pub(crate) fn replay_floor(&self) -> Option<&StoredConnectionLineageFloorV1> {
        self.replay_floor.as_ref()
    }

    /// Returns only the verifier-produced opaque native authority. Stored
    /// lineage bytes are intentionally absent from the peer-factory boundary.
    pub(crate) fn transport_lineage_authority(&self) -> &VerifiedPortableTransportLineageAuthority {
        &self.transport_lineage_authority
    }
}

#[cfg(test)]
impl VerifiedAgentPortableTargetRegistry {
    fn for_test(stored: &StoredPairedTargetRegistryV1) -> Self {
        let targets = stored
            .targets
            .iter()
            .map(|target| {
                let (lineage_format, lineage_revision, lineage_digest, replay_floor) =
                    match &target.transport_lineage {
                        StoredTransportLineageV1::Quiescent {
                            format,
                            lineage_revision,
                            bytes: _,
                            digest,
                            replay_floor,
                        } => (
                            format.clone(),
                            *lineage_revision,
                            *digest,
                            replay_floor.clone(),
                        ),
                        StoredTransportLineageV1::Uncertain {
                            format,
                            lineage_revision,
                            bytes: _,
                            digest,
                            replay_floor,
                        } => (
                            format.clone(),
                            *lineage_revision,
                            *digest,
                            replay_floor.clone(),
                        ),
                    };
                Arc::new(VerifiedAgentPortableTarget {
                    pair_id: target.pair_id.clone(),
                    pairing_revision: target.pairing_revision,
                    directory_revision: target.directory_revision,
                    host_registration_id: target.host_registration_id.clone(),
                    host_device_id: target.host_device_id.clone(),
                    host_installation_id: target.host_installation_id.clone(),
                    host_endpoint_id: target.host_endpoint_id.clone(),
                    host_endpoint_epoch: target.host_endpoint_epoch,
                    host_display_name: target.host_display_name.clone(),
                    pairing_incarnation: target.pairing_incarnation,
                    authorization_evidence_digest: target.authorization.digest,
                    revocation_stream_id: target.revocation.stream_id.clone(),
                    revocation_generation: target.revocation.generation,
                    revocation_applied_sequence: target.revocation.applied_sequence,
                    revocation_checkpoint_digest: target.revocation.checkpoint_digest,
                    connection_hints: target.connection_hints.clone(),
                    lineage_format,
                    lineage_revision,
                    lineage_digest,
                    replay_floor,
                    transport_lineage_authority: VerifiedPortableTransportLineageAuthority::new(
                        TestVerifiedPortableTransportLineageAuthority,
                    ),
                    revoked: false,
                })
            })
            .collect();
        let lineage_tombstones = stored
            .lineage_tombstones
            .iter()
            .map(|tombstone| VerifiedLineageTombstone {
                host_registration_id: tombstone.host_registration_id.clone(),
                host_endpoint_id: tombstone.host_endpoint_id.clone(),
                pair_id: tombstone.pair_id.clone(),
                retired_pairing_incarnation: tombstone.retired_pairing_incarnation,
                retired_authorization_revision: tombstone.retired_authorization_revision,
                replay_floor: tombstone.replay_floor.clone(),
            })
            .collect();
        Self {
            account_id: stored.account_id.clone(),
            project_id: stored.project_id.clone(),
            local_registration_id: stored.local_registration_id.clone(),
            local_device_id: stored.local_device_id.clone(),
            local_installation_id: stored.local_installation_id.clone(),
            controller_endpoint_id: stored.controller_endpoint_id.clone(),
            controller_endpoint_epoch: stored.controller_endpoint_epoch,
            account_context_epoch: stored.account_context_epoch,
            security_epoch: stored.security_epoch,
            authorization_snapshot_revision: stored.authorization_snapshot_revision,
            registration_evidence_digest: stored.registration_evidence.digest,
            revocation_sync_evidence_digest: stored.revocation_sync_evidence.digest,
            complete_snapshot: true,
            targets,
            lineage_tombstones,
        }
    }
}

fn validate_credential_against_registry(
    credential: &NativePortableCredentialClaims,
    envelope: &StoredRegistryEnvelopeV1,
) -> Result<(), AgentPortableRemoteError> {
    credential.validate()?;
    let registry = &envelope.registry;
    if credential.storage_key_digest != envelope.storage_key_digest
        || credential.account_id != registry.account_id
        || credential.project_id != registry.project_id
        || credential.local_registration_id != registry.local_registration_id
        || credential.local_device_id != registry.local_device_id
        || credential.local_installation_id != registry.local_installation_id
        || credential.controller_endpoint_id != registry.controller_endpoint_id
        || credential.controller_endpoint_epoch != registry.controller_endpoint_epoch
        || credential.account_context_epoch != registry.account_context_epoch
    {
        return Err(AgentPortableRemoteError::AccountMismatch);
    }
    Ok(())
}

fn validate_verified_registry(
    credential: &NativePortableCredentialClaims,
    stored: &StoredPairedTargetRegistryV1,
    verified: &VerifiedAgentPortableTargetRegistry,
) -> Result<(), AgentPortableRemoteError> {
    if !verified.complete_snapshot
        || verified.account_id != credential.account_id
        || verified.project_id != credential.project_id
        || verified.local_registration_id != credential.local_registration_id
        || verified.local_device_id != credential.local_device_id
        || verified.local_installation_id != credential.local_installation_id
        || verified.controller_endpoint_id != credential.controller_endpoint_id
        || verified.controller_endpoint_epoch != credential.controller_endpoint_epoch
        || verified.account_context_epoch != credential.account_context_epoch
        || verified.security_epoch != stored.security_epoch
        || verified.authorization_snapshot_revision != stored.authorization_snapshot_revision
        || verified.registration_evidence_digest != stored.registration_evidence.digest
        || verified.revocation_sync_evidence_digest != stored.revocation_sync_evidence.digest
        || verified.targets.len() != stored.targets.len()
        || verified.lineage_tombstones.len() != stored.lineage_tombstones.len()
    {
        return Err(AgentPortableRemoteError::VerificationFailed);
    }
    for (stored_target, verified_target) in stored.targets.iter().zip(&verified.targets) {
        validate_verified_target(stored_target, verified_target)?;
    }
    for (stored_tombstone, verified_tombstone) in stored
        .lineage_tombstones
        .iter()
        .zip(&verified.lineage_tombstones)
    {
        if verified_tombstone.host_registration_id != stored_tombstone.host_registration_id
            || verified_tombstone.host_endpoint_id != stored_tombstone.host_endpoint_id
            || verified_tombstone.pair_id != stored_tombstone.pair_id
            || verified_tombstone.retired_pairing_incarnation
                != stored_tombstone.retired_pairing_incarnation
            || verified_tombstone.retired_authorization_revision
                != stored_tombstone.retired_authorization_revision
            || verified_tombstone.replay_floor != stored_tombstone.replay_floor
        {
            return Err(AgentPortableRemoteError::VerificationFailed);
        }
    }
    Ok(())
}

fn validate_verified_target(
    stored: &StoredPairedTargetV1,
    verified: &VerifiedAgentPortableTarget,
) -> Result<(), AgentPortableRemoteError> {
    let (
        stored_lineage_format,
        stored_lineage_revision,
        stored_lineage_digest,
        stored_replay_floor,
    ) = match &stored.transport_lineage {
        StoredTransportLineageV1::Quiescent {
            format,
            lineage_revision,
            bytes: _,
            digest,
            replay_floor,
        } => (
            format.as_str(),
            *lineage_revision,
            *digest,
            replay_floor.as_ref(),
        ),
        StoredTransportLineageV1::Uncertain {
            format,
            lineage_revision,
            bytes: _,
            digest,
            replay_floor,
        } => (
            format.as_str(),
            *lineage_revision,
            *digest,
            replay_floor.as_ref(),
        ),
    };
    if verified.revoked {
        return Err(AgentPortableRemoteError::Revoked);
    }
    if verified.pair_id != stored.pair_id
        || verified.pairing_revision != stored.pairing_revision
        || verified.directory_revision != stored.directory_revision
        || verified.host_registration_id != stored.host_registration_id
        || verified.host_device_id != stored.host_device_id
        || verified.host_installation_id != stored.host_installation_id
        || verified.host_endpoint_id != stored.host_endpoint_id
        || verified.host_endpoint_epoch != stored.host_endpoint_epoch
        || verified.host_display_name != stored.host_display_name
        || verified.pairing_incarnation != stored.pairing_incarnation
        || verified.authorization_evidence_digest != stored.authorization.digest
        || verified.revocation_stream_id != stored.revocation.stream_id
        || verified.revocation_generation != stored.revocation.generation
        || verified.revocation_applied_sequence != stored.revocation.applied_sequence
        || verified.revocation_checkpoint_digest != stored.revocation.checkpoint_digest
        || verified.connection_hints != stored.connection_hints
        || verified.lineage_format != stored_lineage_format
        || verified.lineage_revision != stored_lineage_revision
        || verified.lineage_digest != stored_lineage_digest
        || verified.replay_floor.as_ref() != stored_replay_floor
    {
        return Err(AgentPortableRemoteError::VerificationFailed);
    }
    Ok(())
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct PortablePeerLeaseSeed {
    host_epoch: u64,
    connection_generation: u64,
}

impl PortablePeerLeaseSeed {
    pub(crate) fn new(
        host_epoch: u64,
        connection_generation: u64,
    ) -> Result<Self, AgentPortableRemoteError> {
        let seed = Self {
            host_epoch,
            connection_generation,
        };
        seed.validate()?;
        Ok(seed)
    }

    fn validate(self) -> Result<(), AgentPortableRemoteError> {
        if self.host_epoch == 0
            || self.connection_generation == 0
            || self.connection_generation > MAX_JAVASCRIPT_SAFE_UNSIGNED_INTEGER
        {
            return Err(AgentPortableRemoteError::PeerUnavailable);
        }
        Ok(())
    }
}

pub(crate) trait AgentPortablePeer: Send + Sync {
    fn lease_seed(&self) -> PortablePeerLeaseSeed;

    /// Synchronously prevents creation of new native peer operations.
    fn fence(&self);

    /// Every operation below must promptly observe `cancellation`, release its
    /// per-operation native resources, and complete with `Cancelled`. Cleanup
    /// deliberately waits for those operation acknowledgements before calling
    /// `dispose`, so an adapter must not defer cancellation until disposal.
    fn runtime_status(
        self: Arc<Self>,
        cancellation: PortableCancellation,
    ) -> PortableFuture<'static, Result<PortableRuntimeStatus, AgentPortableRemoteError>>;

    fn sessions_page(
        self: Arc<Self>,
        request: PortablePageRequest,
        cancellation: PortableCancellation,
    ) -> PortableFuture<'static, Result<PortableSessionPage, AgentPortableRemoteError>>;

    fn records_page(
        self: Arc<Self>,
        request: PortableRecordsPageRequest,
        cancellation: PortableCancellation,
    ) -> PortableFuture<'static, Result<PortableHistoryPage, AgentPortableRemoteError>>;

    fn network_changed(
        self: Arc<Self>,
        cancellation: PortableCancellation,
    ) -> PortableFuture<'static, Result<(), AgentPortableRemoteError>>;

    /// Completes only after the peer acknowledges cancellation and native
    /// transport resources are disposed.
    fn dispose(self: Arc<Self>) -> PortableFuture<'static, Result<(), AgentPortableRemoteError>>;
}

pub(crate) trait PortablePeerFactory: Send + Sync {
    /// `target.connection_hints()` is untrusted routing input. Implementations
    /// must intersect it with their independently configured relay and network
    /// destination policy before any dial. The concrete native adapter must
    /// also recognize the sealed transport-lineage authority before dialing;
    /// a downcast/type mismatch fails closed. Before its first network action,
    /// the factory must atomically bind and check `cancellation`; cancellation
    /// then synchronously prohibits every new dial or peer operation at the
    /// native linearization point. `Err` is terminal only after the factory has
    /// acknowledged cleanup of every partially acquired native endpoint or
    /// connection. Once `Ok(peer)` is returned, the controller owns all
    /// fencing and asynchronous disposal.
    fn connect(
        self: Arc<Self>,
        target: Arc<VerifiedAgentPortableTarget>,
        cancellation: PortableCancellation,
    ) -> PortableFuture<'static, Result<Arc<dyn AgentPortablePeer>, AgentPortableRemoteError>>;
}

#[derive(Debug)]
struct DisabledNativePortableComposition;

impl NativePortableCredentialProvider for DisabledNativePortableComposition {
    fn current(
        self: Arc<Self>,
    ) -> PortableFuture<
        'static,
        Result<Arc<dyn NativePortableCredentialLease>, AgentPortableRemoteError>,
    > {
        Box::pin(async { Err(AgentPortableRemoteError::Unavailable) })
    }
}

impl NativePortablePairingVerifier for DisabledNativePortableComposition {
    fn verify_registry(
        self: Arc<Self>,
        _credential: Arc<dyn NativePortableCredentialLease>,
        _stored: StoredPairedTargetRegistryV1,
    ) -> PortableFuture<
        'static,
        Result<VerifiedAgentPortableTargetRegistry, AgentPortableRemoteError>,
    > {
        Box::pin(async { Err(AgentPortableRemoteError::Unavailable) })
    }
}

impl PortablePeerFactory for DisabledNativePortableComposition {
    fn connect(
        self: Arc<Self>,
        _target: Arc<VerifiedAgentPortableTarget>,
        _cancellation: PortableCancellation,
    ) -> PortableFuture<'static, Result<Arc<dyn AgentPortablePeer>, AgentPortableRemoteError>> {
        Box::pin(async { Err(AgentPortableRemoteError::Unavailable) })
    }
}

struct PortableCompletion<T: Clone> {
    value: Mutex<Option<T>>,
    notify: Notify,
}

impl<T: Clone> PortableCompletion<T> {
    fn pending() -> Arc<Self> {
        Arc::new(Self {
            value: Mutex::new(None),
            notify: Notify::new(),
        })
    }

    fn completed(value: T) -> Arc<Self> {
        Arc::new(Self {
            value: Mutex::new(Some(value)),
            notify: Notify::new(),
        })
    }

    fn complete(&self, value: T) {
        let mut slot = self.value.lock().unwrap();
        if slot.is_none() {
            *slot = Some(value);
            drop(slot);
            self.notify.notify_waiters();
        }
    }

    async fn wait(&self) -> T {
        loop {
            let notified = self.notify.notified();
            if let Some(value) = self.value.lock().unwrap().clone() {
                return value;
            }
            notified.await;
        }
    }
}

impl<T: Clone> std::fmt::Debug for PortableCompletion<T> {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("PortableCompletion")
            .field("completed", &self.value.lock().unwrap().is_some())
            .finish()
    }
}

type PortableTransition = (
    u64,
    Arc<PortableCompletion<Result<(), AgentPortableRemoteError>>>,
);

#[derive(Clone)]
pub(crate) struct AgentPortableRemoteController {
    inner: Arc<AgentPortableRemoteControllerInner>,
}

impl std::fmt::Debug for AgentPortableRemoteController {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("AgentPortableRemoteController(<native>)")
    }
}

struct AgentPortableRemoteControllerInner {
    store: Arc<dyn PairedTargetAuthorityStore>,
    credentials: Arc<dyn NativePortableCredentialProvider>,
    verifier: Arc<dyn NativePortablePairingVerifier>,
    peer_factory: Arc<dyn PortablePeerFactory>,
    next_request_id: AtomicU64,
    runtime: Mutex<Option<tokio::runtime::Handle>>,
    state: Mutex<AgentPortableRemoteControllerState>,
}

struct AgentPortableRemoteControllerState {
    fence_epoch: u64,
    prepared: Option<PreparedPortableAccount>,
    pending: Option<PendingPortableConnection>,
    active: Option<ActivePortableConnection>,
    requests: HashMap<u64, PortableRequestOwner>,
    cleanup_barrier: Arc<PortableCompletion<Result<(), AgentPortableRemoteError>>>,
}

struct PreparedPortableAccount {
    credential: Arc<dyn NativePortableCredentialLease>,
    storage_token: StoredRegistryCommitToken,
    verified_registry: Arc<VerifiedAgentPortableTargetRegistry>,
    targets: HashMap<PortableTargetHandle, Arc<VerifiedAgentPortableTarget>>,
    descriptors: Vec<PortableTargetDescriptor>,
}

impl std::fmt::Debug for PreparedPortableAccount {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("PreparedPortableAccount")
            .field("verified_registry", &self.verified_registry)
            .field("target_count", &self.targets.len())
            .finish_non_exhaustive()
    }
}

struct PendingPortableConnection {
    id: u64,
    fence_epoch: u64,
    cancellation: PortableCancellation,
    ready: Arc<PortableCompletion<Result<PortableTargetLease, AgentPortableRemoteError>>>,
    accepted: Arc<PortableCompletion<bool>>,
    worker_done: Arc<PortableCompletion<Result<(), AgentPortableRemoteError>>>,
    acquired: Option<AcquiredPortableConnection>,
}

struct AcquiredPortableConnection {
    handle: PortableTargetHandle,
    lease: PortableTargetLease,
    peer: Arc<dyn AgentPortablePeer>,
}

struct ActivePortableConnection {
    fence_epoch: u64,
    handle: PortableTargetHandle,
    lease: PortableTargetLease,
    peer: Arc<dyn AgentPortablePeer>,
}

struct PortableRequestOwner {
    cancellation: PortableCancellation,
    completion: Arc<PortableCompletion<()>>,
}

struct PrepareWaiterGuard {
    cancellation: PortableCancellation,
    armed: bool,
}

impl PrepareWaiterGuard {
    fn disarm(&mut self) {
        self.armed = false;
    }
}

impl Drop for PrepareWaiterGuard {
    fn drop(&mut self) {
        if self.armed {
            self.cancellation.cancel();
        }
    }
}

struct PortableRetirement {
    previous_barrier: Arc<PortableCompletion<Result<(), AgentPortableRemoteError>>>,
    pending: Option<PendingPortableConnection>,
    active: Option<ActivePortableConnection>,
    requests: Vec<PortableRequestOwner>,
    completion: Arc<PortableCompletion<Result<(), AgentPortableRemoteError>>>,
}

struct PortableRequestContext {
    request_id: u64,
    fence_epoch: u64,
    lease: PortableTargetLease,
    peer: Arc<dyn AgentPortablePeer>,
    credential: Arc<dyn NativePortableCredentialLease>,
    storage_key_digest: PairedTargetStorageKeyDigest,
    storage_token: StoredRegistryCommitToken,
    cancellation: PortableCancellation,
    owner_completion: Arc<PortableCompletion<()>>,
}

impl AgentPortableRemoteControllerState {
    fn new() -> Self {
        Self {
            fence_epoch: 0,
            prepared: None,
            pending: None,
            active: None,
            requests: HashMap::new(),
            cleanup_barrier: PortableCompletion::completed(Ok(())),
        }
    }
}

impl AgentPortableRemoteController {
    /// The only production constructor in this slice. No platform storage,
    /// credential, verifier, peer, or renderer command is silently installed.
    pub(crate) fn disabled() -> Self {
        let disabled = Arc::new(DisabledNativePortableComposition);
        Self {
            inner: Arc::new(AgentPortableRemoteControllerInner {
                store: Arc::new(InMemoryPairedTargetAuthorityStore::default()),
                credentials: disabled.clone(),
                verifier: disabled.clone(),
                peer_factory: disabled,
                next_request_id: AtomicU64::new(1),
                runtime: Mutex::new(None),
                state: Mutex::new(AgentPortableRemoteControllerState::new()),
            }),
        }
    }

    #[cfg(test)]
    fn with_dependencies(
        store: Arc<dyn PairedTargetAuthorityStore>,
        credentials: Arc<dyn NativePortableCredentialProvider>,
        verifier: Arc<dyn NativePortablePairingVerifier>,
        peer_factory: Arc<dyn PortablePeerFactory>,
    ) -> Self {
        Self {
            inner: Arc::new(AgentPortableRemoteControllerInner {
                store,
                credentials,
                verifier,
                peer_factory,
                next_request_id: AtomicU64::new(1),
                runtime: Mutex::new(None),
                state: Mutex::new(AgentPortableRemoteControllerState::new()),
            }),
        }
    }

    /// Replaces the native account view. A newer refresh, sign-out, or dispose
    /// fences this acquisition; stale results never publish renderer handles.
    pub(crate) async fn refresh_targets_for_account(
        &self,
        expected_account_id: &str,
    ) -> Result<Vec<PortableTargetDescriptor>, AgentPortableRemoteError> {
        validate_uuid("expected account", expected_account_id)
            .map_err(|_| AgentPortableRemoteError::InvalidRequest)?;
        self.refresh_targets_inner(Some(expected_account_id)).await
    }

    #[cfg(test)]
    async fn refresh_targets(
        &self,
    ) -> Result<Vec<PortableTargetDescriptor>, AgentPortableRemoteError> {
        self.refresh_targets_inner(None).await
    }

    async fn refresh_targets_inner(
        &self,
        expected_account_id: Option<&str>,
    ) -> Result<Vec<PortableTargetDescriptor>, AgentPortableRemoteError> {
        let (fence_epoch, barrier) = self.begin_transition(true)?;
        barrier.wait().await?;
        self.require_current_epoch(fence_epoch)?;

        let credential = self.inner.credentials.clone().current().await?;
        let credential_claims = credential.claims().clone();
        credential_claims.validate()?;
        if expected_account_id.is_some_and(|expected| credential_claims.account_id != expected) {
            return Err(AgentPortableRemoteError::AccountMismatch);
        }
        self.require_current_epoch(fence_epoch)?;
        let (stored, stored_token) = match self
            .inner
            .store
            .load(credential_claims.storage_key_digest)?
        {
            StoredRegistryLoad::Empty { .. } => {
                return Err(AgentPortableRemoteError::Unavailable);
            }
            StoredRegistryLoad::Committed { envelope, token } => (*envelope, token),
        };
        validate_credential_against_registry(&credential_claims, &stored)?;
        credential.clone().revalidate_current().await?;
        self.require_current_epoch(fence_epoch)?;
        let verified = self
            .inner
            .verifier
            .clone()
            .verify_registry(credential.clone(), stored.registry.clone())
            .await?;
        credential.clone().revalidate_current().await?;
        self.require_current_epoch(fence_epoch)?;
        validate_credential_against_registry(&credential_claims, &stored)?;
        validate_verified_registry(&credential_claims, &stored.registry, &verified)?;
        require_store_current(
            self.inner.store.as_ref(),
            credential_claims.storage_key_digest,
            stored_token,
        )?;

        let mut targets = HashMap::with_capacity(verified.targets.len());
        let mut descriptors = Vec::with_capacity(verified.targets.len());
        for (stored_target, verified_target) in
            stored.registry.targets.iter().zip(&verified.targets)
        {
            if !stored_target.transport_lineage.is_quiescent() {
                continue;
            }
            let handle = PortableTargetHandle::issue()?;
            let descriptor = PortableTargetDescriptor {
                handle: handle.clone(),
                label: verified_target.host_display_name.clone(),
            };
            descriptor.validate()?;
            targets.insert(handle, verified_target.clone());
            descriptors.push(descriptor);
        }
        let verified_registry = Arc::new(verified);
        let mut state = self.inner.state.lock().unwrap();
        if state.fence_epoch != fence_epoch || state.pending.is_some() || state.active.is_some() {
            return Err(AgentPortableRemoteError::Cancelled);
        }
        state.prepared = Some(PreparedPortableAccount {
            credential,
            storage_token: stored_token,
            verified_registry,
            targets,
            descriptors: descriptors.clone(),
        });
        Ok(descriptors)
    }

    /// Prepares exactly one allowlisted target. The detached worker owns any
    /// acquired peer until it publishes it or completes acknowledged disposal.
    pub(crate) async fn prepare_target(
        &self,
        handle: &PortableTargetHandle,
    ) -> Result<PortableTargetLease, AgentPortableRemoteError> {
        handle.validate()?;
        {
            let state = self.inner.state.lock().unwrap();
            let prepared = state
                .prepared
                .as_ref()
                .ok_or(AgentPortableRemoteError::Unauthenticated)?;
            if !prepared.targets.contains_key(handle) {
                return Err(AgentPortableRemoteError::UnknownTarget);
            }
        }
        let (fence_epoch, barrier) = self.begin_transition(false)?;
        barrier.wait().await?;

        let (target, credential, storage_token) = {
            let state = self.inner.state.lock().unwrap();
            if state.fence_epoch != fence_epoch {
                return Err(AgentPortableRemoteError::Cancelled);
            }
            let prepared = state
                .prepared
                .as_ref()
                .ok_or(AgentPortableRemoteError::Unauthenticated)?;
            let target = prepared
                .targets
                .get(handle)
                .cloned()
                .ok_or(AgentPortableRemoteError::UnknownTarget)?;
            (target, prepared.credential.clone(), prepared.storage_token)
        };
        credential.clone().revalidate_current().await?;
        self.require_current_epoch(fence_epoch)?;
        require_store_current(
            self.inner.store.as_ref(),
            credential.claims().storage_key_digest,
            storage_token,
        )?;

        let id = self.issue_request_id()?;
        let runtime = self.record_runtime()?;
        let cancellation = PortableCancellation::new();
        let ready = PortableCompletion::pending();
        let accepted = PortableCompletion::pending();
        let worker_done = PortableCompletion::pending();
        {
            let mut state = self.inner.state.lock().unwrap();
            if state.fence_epoch != fence_epoch || state.prepared.is_none() {
                return Err(AgentPortableRemoteError::Cancelled);
            }
            state.pending = Some(PendingPortableConnection {
                id,
                fence_epoch,
                cancellation: cancellation.clone(),
                ready: ready.clone(),
                accepted: accepted.clone(),
                worker_done: worker_done.clone(),
                acquired: None,
            });
        }
        let mut waiter_guard = PrepareWaiterGuard {
            cancellation: cancellation.clone(),
            armed: true,
        };
        let inner = Arc::downgrade(&self.inner);
        let peer_factory = self.inner.peer_factory.clone();
        let store = self.inner.store.clone();
        let selected_handle = handle.clone();
        let worker_ready = ready.clone();
        let worker_accepted = accepted.clone();
        let worker_done_owner = worker_done.clone();
        let worker_cancellation = cancellation.clone();
        let _worker = runtime.spawn(async move {
            run_portable_connect(PortableConnectTask {
                inner,
                peer_factory,
                store,
                id,
                fence_epoch,
                handle: selected_handle,
                target,
                credential,
                storage_token,
                cancellation: worker_cancellation,
                ready: worker_ready,
                accepted: worker_accepted,
                worker_done: worker_done_owner,
            })
            .await;
        });
        let lease = match ready.wait().await {
            Ok(lease) => lease,
            Err(error) => {
                let cleanup = worker_done.wait().await;
                waiter_guard.disarm();
                return cleanup.and(Err(error));
            }
        };
        let accepted_peer = {
            let mut state = self.inner.state.lock().unwrap();
            let pending_matches = state
                .pending
                .as_ref()
                .is_some_and(|pending| pending.id == id && pending.fence_epoch == fence_epoch);
            if state.fence_epoch != fence_epoch || !pending_matches || state.active.is_some() {
                None
            } else {
                let mut pending = state.pending.take().expect("matched pending connection");
                pending.acquired.take().map(|acquired| {
                    debug_assert_eq!(acquired.lease, lease);
                    state.active = Some(ActivePortableConnection {
                        fence_epoch,
                        handle: acquired.handle,
                        lease: acquired.lease.clone(),
                        peer: acquired.peer,
                    });
                    acquired.lease
                })
            }
        };
        if let Some(lease) = accepted_peer {
            accepted.complete(true);
            waiter_guard.disarm();
            Ok(lease)
        } else {
            cancellation.cancel();
            let cleanup = worker_done.wait().await;
            waiter_guard.disarm();
            cleanup.and(Err(AgentPortableRemoteError::Cancelled))
        }
    }

    pub(crate) async fn runtime_status(
        &self,
        lease: &PortableTargetLease,
    ) -> Result<PortableRuntimeStatus, AgentPortableRemoteError> {
        let runtime = self.record_runtime()?;
        let context = self.begin_request(lease)?;
        let mut waiter_guard = PrepareWaiterGuard {
            cancellation: context.cancellation.clone(),
            armed: true,
        };
        let result = PortableCompletion::pending();
        let inner = Arc::downgrade(&self.inner);
        let result_owner = result.clone();
        let _worker = runtime.spawn(async move {
            let operation = async {
                revalidate_request_authority(&inner, &context).await?;
                if context.cancellation.is_cancelled() {
                    return Err(AgentPortableRemoteError::Cancelled);
                }
                let response = context
                    .peer
                    .clone()
                    .runtime_status(context.cancellation.clone())
                    .await?;
                response.validate()?;
                revalidate_request_authority(&inner, &context).await?;
                Ok(response)
            }
            .await;
            retire_failed_request(&inner, &context, operation.as_ref().err());
            finish_portable_request(&inner, &context);
            result_owner.complete(operation);
        });
        let outcome = result.wait().await;
        waiter_guard.disarm();
        outcome
    }

    pub(crate) async fn sessions_page(
        &self,
        lease: &PortableTargetLease,
        request: PortablePageRequest,
    ) -> Result<PortableSessionPage, AgentPortableRemoteError> {
        request.validate()?;
        let runtime = self.record_runtime()?;
        let context = self.begin_request(lease)?;
        let mut waiter_guard = PrepareWaiterGuard {
            cancellation: context.cancellation.clone(),
            armed: true,
        };
        let result = PortableCompletion::pending();
        let inner = Arc::downgrade(&self.inner);
        let result_owner = result.clone();
        let _worker = runtime.spawn(async move {
            let operation = async {
                revalidate_request_authority(&inner, &context).await?;
                if context.cancellation.is_cancelled() {
                    return Err(AgentPortableRemoteError::Cancelled);
                }
                let response = context
                    .peer
                    .clone()
                    .sessions_page(request.clone(), context.cancellation.clone())
                    .await?;
                response.validate_for(&request)?;
                revalidate_request_authority(&inner, &context).await?;
                Ok(response)
            }
            .await;
            retire_failed_request(&inner, &context, operation.as_ref().err());
            finish_portable_request(&inner, &context);
            result_owner.complete(operation);
        });
        let outcome = result.wait().await;
        waiter_guard.disarm();
        outcome
    }

    pub(crate) async fn records_page(
        &self,
        lease: &PortableTargetLease,
        request: PortableRecordsPageRequest,
    ) -> Result<PortableHistoryPage, AgentPortableRemoteError> {
        request.validate()?;
        let runtime = self.record_runtime()?;
        let context = self.begin_request(lease)?;
        let mut waiter_guard = PrepareWaiterGuard {
            cancellation: context.cancellation.clone(),
            armed: true,
        };
        let result = PortableCompletion::pending();
        let inner = Arc::downgrade(&self.inner);
        let result_owner = result.clone();
        let _worker = runtime.spawn(async move {
            let operation = async {
                revalidate_request_authority(&inner, &context).await?;
                if context.cancellation.is_cancelled() {
                    return Err(AgentPortableRemoteError::Cancelled);
                }
                let response = context
                    .peer
                    .clone()
                    .records_page(request.clone(), context.cancellation.clone())
                    .await?;
                response.validate_for(&request)?;
                revalidate_request_authority(&inner, &context).await?;
                Ok(response)
            }
            .await;
            retire_failed_request(&inner, &context, operation.as_ref().err());
            finish_portable_request(&inner, &context);
            result_owner.complete(operation);
        });
        let outcome = result.wait().await;
        waiter_guard.disarm();
        outcome
    }

    pub(crate) async fn network_changed(
        &self,
        lease: &PortableTargetLease,
    ) -> Result<(), AgentPortableRemoteError> {
        let runtime = self.record_runtime()?;
        let context = self.begin_request(lease)?;
        let mut waiter_guard = PrepareWaiterGuard {
            cancellation: context.cancellation.clone(),
            armed: true,
        };
        let result = PortableCompletion::pending();
        let inner = Arc::downgrade(&self.inner);
        let result_owner = result.clone();
        let _worker = runtime.spawn(async move {
            let operation = async {
                revalidate_request_authority(&inner, &context).await?;
                if context.cancellation.is_cancelled() {
                    return Err(AgentPortableRemoteError::Cancelled);
                }
                context
                    .peer
                    .clone()
                    .network_changed(context.cancellation.clone())
                    .await?;
                revalidate_request_authority(&inner, &context).await
            }
            .await;
            retire_failed_request(&inner, &context, operation.as_ref().err());
            finish_portable_request(&inner, &context);
            result_owner.complete(operation);
        });
        let outcome = result.wait().await;
        waiter_guard.disarm();
        outcome
    }

    /// Native-only account-context fence. The native authentication owner must
    /// call this and await its cleanup acknowledgement before publishing
    /// sign-out, credential revocation, or an A-to-B account transition. It is
    /// intentionally not registered as a renderer command.
    pub(crate) async fn native_credentials_invalidated(
        &self,
    ) -> Result<(), AgentPortableRemoteError> {
        let (_, barrier) = self.begin_transition(true)?;
        barrier.wait().await
    }

    /// Fences all handles and requests immediately, then waits for the detached
    /// native cleanup owner to receive peer disposal acknowledgements.
    pub(crate) async fn dispose(&self) -> Result<(), AgentPortableRemoteError> {
        let (_, barrier) = self.begin_transition(true)?;
        barrier.wait().await
    }

    fn begin_transition(
        &self,
        clear_account: bool,
    ) -> Result<PortableTransition, AgentPortableRemoteError> {
        let runtime = self.record_runtime()?;
        let (fence_epoch, retirement) = {
            let mut state = self.inner.state.lock().unwrap();
            state.fence_epoch = state
                .fence_epoch
                .checked_add(1)
                .ok_or(AgentPortableRemoteError::Internal)?;
            let fence_epoch = state.fence_epoch;
            let completion = PortableCompletion::pending();
            let previous_barrier =
                std::mem::replace(&mut state.cleanup_barrier, completion.clone());
            let pending = state.pending.take();
            let active = state.active.take();
            let requests = state.requests.drain().map(|(_, owner)| owner).collect();
            if clear_account {
                state.prepared = None;
            }
            (
                fence_epoch,
                PortableRetirement {
                    previous_barrier,
                    pending,
                    active,
                    requests,
                    completion,
                },
            )
        };
        let barrier = retirement.completion.clone();
        start_portable_retirement(retirement, &runtime);
        Ok((fence_epoch, barrier))
    }

    fn require_current_epoch(&self, fence_epoch: u64) -> Result<(), AgentPortableRemoteError> {
        if self.inner.state.lock().unwrap().fence_epoch == fence_epoch {
            Ok(())
        } else {
            Err(AgentPortableRemoteError::Cancelled)
        }
    }

    fn issue_request_id(&self) -> Result<u64, AgentPortableRemoteError> {
        let id = self.inner.next_request_id.fetch_add(1, Ordering::Relaxed);
        if id == 0 || id == u64::MAX {
            Err(AgentPortableRemoteError::Internal)
        } else {
            Ok(id)
        }
    }

    fn record_runtime(&self) -> Result<tokio::runtime::Handle, AgentPortableRemoteError> {
        let runtime = tokio::runtime::Handle::try_current()
            .map_err(|_| AgentPortableRemoteError::Internal)?;
        *self.inner.runtime.lock().unwrap() = Some(runtime.clone());
        Ok(runtime)
    }

    fn begin_request(
        &self,
        lease: &PortableTargetLease,
    ) -> Result<PortableRequestContext, AgentPortableRemoteError> {
        lease.validate()?;
        let request_id = self.issue_request_id()?;
        let cancellation = PortableCancellation::new();
        let owner_completion = PortableCompletion::pending();
        let mut state = self.inner.state.lock().unwrap();
        let active = state
            .active
            .as_ref()
            .ok_or(AgentPortableRemoteError::PeerUnavailable)?;
        if &active.lease != lease || active.fence_epoch != state.fence_epoch {
            return Err(AgentPortableRemoteError::StaleLease);
        }
        let prepared = state
            .prepared
            .as_ref()
            .ok_or(AgentPortableRemoteError::Unauthenticated)?;
        let context = PortableRequestContext {
            request_id,
            fence_epoch: state.fence_epoch,
            lease: lease.clone(),
            peer: active.peer.clone(),
            credential: prepared.credential.clone(),
            storage_key_digest: prepared.credential.claims().storage_key_digest,
            storage_token: prepared.storage_token,
            cancellation: cancellation.clone(),
            owner_completion: owner_completion.clone(),
        };
        state.requests.insert(
            request_id,
            PortableRequestOwner {
                cancellation,
                completion: owner_completion,
            },
        );
        Ok(context)
    }
}

fn start_portable_retirement(retirement: PortableRetirement, runtime: &tokio::runtime::Handle) {
    if let Some(active) = &retirement.active {
        active.peer.fence();
    }
    if let Some(pending) = &retirement.pending {
        pending.cancellation.cancel();
        if let Some(acquired) = &pending.acquired {
            acquired.peer.fence();
        }
    }
    for request in &retirement.requests {
        request.cancellation.cancel();
    }
    let _worker = runtime.spawn(async move {
        let mut cleanup_failed = retirement.previous_barrier.wait().await.is_err();
        if let Some(pending) = retirement.pending {
            if matches!(
                pending.worker_done.wait().await,
                Err(AgentPortableRemoteError::CleanupFailed)
            ) {
                cleanup_failed = true;
            }
            if let Some(acquired) = pending.acquired {
                if acquired.peer.dispose().await.is_err() {
                    cleanup_failed = true;
                }
            }
        }
        for request in retirement.requests {
            request.completion.wait().await;
        }
        if let Some(active) = retirement.active {
            if active.peer.dispose().await.is_err() {
                cleanup_failed = true;
            }
        }
        retirement.completion.complete(if cleanup_failed {
            Err(AgentPortableRemoteError::CleanupFailed)
        } else {
            Ok(())
        });
    });
}

impl Drop for AgentPortableRemoteControllerInner {
    fn drop(&mut self) {
        let state = match self.state.get_mut() {
            Ok(state) => state,
            Err(poisoned) => poisoned.into_inner(),
        };
        let completion = PortableCompletion::pending();
        let retirement = PortableRetirement {
            previous_barrier: state.cleanup_barrier.clone(),
            pending: state.pending.take(),
            active: state.active.take(),
            requests: state.requests.drain().map(|(_, owner)| owner).collect(),
            completion,
        };
        state.prepared = None;
        let runtime = match self.runtime.get_mut() {
            Ok(runtime) => runtime.take(),
            Err(poisoned) => poisoned.into_inner().take(),
        };
        if let Some(runtime) = runtime {
            start_portable_retirement(retirement, &runtime);
        } else {
            if let Some(active) = retirement.active {
                active.peer.fence();
            }
            if let Some(pending) = retirement.pending {
                pending.cancellation.cancel();
            }
            for request in retirement.requests {
                request.cancellation.cancel();
            }
        }
    }
}

struct PortableConnectTask {
    inner: std::sync::Weak<AgentPortableRemoteControllerInner>,
    peer_factory: Arc<dyn PortablePeerFactory>,
    store: Arc<dyn PairedTargetAuthorityStore>,
    id: u64,
    fence_epoch: u64,
    handle: PortableTargetHandle,
    target: Arc<VerifiedAgentPortableTarget>,
    credential: Arc<dyn NativePortableCredentialLease>,
    storage_token: StoredRegistryCommitToken,
    cancellation: PortableCancellation,
    ready: Arc<PortableCompletion<Result<PortableTargetLease, AgentPortableRemoteError>>>,
    accepted: Arc<PortableCompletion<bool>>,
    worker_done: Arc<PortableCompletion<Result<(), AgentPortableRemoteError>>>,
}

async fn run_portable_connect(task: PortableConnectTask) {
    let PortableConnectTask {
        inner,
        peer_factory,
        store,
        id,
        fence_epoch,
        handle,
        target,
        credential,
        storage_token,
        cancellation,
        ready,
        accepted,
        worker_done,
    } = task;
    if let Err(error) = credential.clone().revalidate_current().await {
        remove_matching_pending(&inner, id, fence_epoch);
        ready.complete(Err(error));
        worker_done.complete(Ok(()));
        return;
    }
    if let Err(error) = require_store_current(
        store.as_ref(),
        credential.claims().storage_key_digest,
        storage_token,
    ) {
        remove_matching_pending(&inner, id, fence_epoch);
        ready.complete(Err(error));
        worker_done.complete(Ok(()));
        return;
    }
    {
        let Some(owner) = inner.upgrade() else {
            ready.complete(Err(AgentPortableRemoteError::Cancelled));
            worker_done.complete(Ok(()));
            return;
        };
        let state = owner.state.lock().unwrap();
        let current = state.fence_epoch == fence_epoch
            && state
                .pending
                .as_ref()
                .is_some_and(|pending| pending.id == id && pending.fence_epoch == fence_epoch)
            && !cancellation.is_cancelled();
        if !current {
            drop(state);
            remove_matching_pending(&inner, id, fence_epoch);
            ready.complete(Err(AgentPortableRemoteError::Cancelled));
            worker_done.complete(Ok(()));
            return;
        }
    }
    let result = peer_factory.connect(target, cancellation.clone()).await;
    match result {
        Err(error) => {
            remove_matching_pending(&inner, id, fence_epoch);
            ready.complete(Err(if cancellation.is_cancelled() {
                AgentPortableRemoteError::Cancelled
            } else {
                error
            }));
            worker_done.complete(Ok(()));
        }
        Ok(peer) => {
            let lease = match PortableTargetLease::issue(peer.lease_seed()) {
                Ok(lease) => lease,
                Err(error) => {
                    finish_unaccepted_peer(peer, error, ready, worker_done).await;
                    remove_matching_pending(&inner, id, fence_epoch);
                    return;
                }
            };
            let authority_current = credential
                .clone()
                .revalidate_current()
                .await
                .and_then(|()| {
                    require_store_current(
                        store.as_ref(),
                        credential.claims().storage_key_digest,
                        storage_token,
                    )
                });
            if let Err(error) = authority_current {
                finish_unaccepted_peer(peer, error, ready, worker_done).await;
                remove_matching_pending(&inner, id, fence_epoch);
                return;
            }
            let staged = {
                let Some(owner) = inner.upgrade() else {
                    finish_unaccepted_peer(
                        peer,
                        AgentPortableRemoteError::Cancelled,
                        ready,
                        worker_done,
                    )
                    .await;
                    return;
                };
                let mut state = owner.state.lock().unwrap();
                let pending_matches = state
                    .pending
                    .as_ref()
                    .is_some_and(|pending| pending.id == id && pending.fence_epoch == fence_epoch);
                if state.fence_epoch == fence_epoch
                    && pending_matches
                    && !cancellation.is_cancelled()
                    && state.active.is_none()
                    && state.prepared.is_some()
                {
                    state.pending.as_mut().expect("matched pending").acquired =
                        Some(AcquiredPortableConnection {
                            handle,
                            lease: lease.clone(),
                            peer: peer.clone(),
                        });
                    true
                } else {
                    false
                }
            };
            if !staged {
                finish_unaccepted_peer(
                    peer,
                    AgentPortableRemoteError::Cancelled,
                    ready,
                    worker_done,
                )
                .await;
                remove_matching_pending(&inner, id, fence_epoch);
                return;
            }
            ready.complete(Ok(lease));
            tokio::select! {
                accepted = accepted.wait() => {
                    if accepted {
                        worker_done.complete(Ok(()));
                        return;
                    }
                    cancellation.cancel();
                }
                () = cancellation.cancelled() => {}
            }
            let acquired = {
                inner.upgrade().and_then(|owner| {
                    let mut state = owner.state.lock().unwrap();
                    state
                        .pending
                        .as_mut()
                        .filter(|pending| pending.id == id && pending.fence_epoch == fence_epoch)
                        .and_then(|pending| pending.acquired.take())
                })
            };
            if let Some(acquired) = acquired {
                acquired.peer.fence();
                worker_done.complete(if acquired.peer.dispose().await.is_err() {
                    Err(AgentPortableRemoteError::CleanupFailed)
                } else {
                    Ok(())
                });
            } else {
                worker_done.complete(Ok(()));
            }
            remove_matching_pending(&inner, id, fence_epoch);
        }
    }
}

async fn finish_unaccepted_peer(
    peer: Arc<dyn AgentPortablePeer>,
    error: AgentPortableRemoteError,
    ready: Arc<PortableCompletion<Result<PortableTargetLease, AgentPortableRemoteError>>>,
    worker_done: Arc<PortableCompletion<Result<(), AgentPortableRemoteError>>>,
) {
    peer.fence();
    let cleanup = peer.dispose().await;
    ready.complete(Err(if cleanup.is_err() {
        AgentPortableRemoteError::CleanupFailed
    } else {
        error
    }));
    worker_done.complete(if cleanup.is_err() {
        Err(AgentPortableRemoteError::CleanupFailed)
    } else {
        Ok(())
    });
}

fn remove_matching_pending(
    inner: &std::sync::Weak<AgentPortableRemoteControllerInner>,
    id: u64,
    fence_epoch: u64,
) {
    let Some(inner) = inner.upgrade() else {
        return;
    };
    let mut state = inner.state.lock().unwrap();
    if state
        .pending
        .as_ref()
        .is_some_and(|pending| pending.id == id && pending.fence_epoch == fence_epoch)
    {
        state.pending = None;
    }
}

fn require_request_current(
    inner: &std::sync::Weak<AgentPortableRemoteControllerInner>,
    context: &PortableRequestContext,
) -> Result<(), AgentPortableRemoteError> {
    if context.cancellation.is_cancelled() {
        return Err(AgentPortableRemoteError::Cancelled);
    }
    let inner = inner.upgrade().ok_or(AgentPortableRemoteError::Cancelled)?;
    require_store_current(
        inner.store.as_ref(),
        context.storage_key_digest,
        context.storage_token,
    )?;
    let state = inner.state.lock().unwrap();
    let active = state
        .active
        .as_ref()
        .ok_or(AgentPortableRemoteError::StaleLease)?;
    if state.fence_epoch != context.fence_epoch
        || active.fence_epoch != context.fence_epoch
        || active.lease != context.lease
        || !state.requests.contains_key(&context.request_id)
    {
        return Err(AgentPortableRemoteError::StaleLease);
    }
    Ok(())
}

async fn revalidate_request_authority(
    inner: &std::sync::Weak<AgentPortableRemoteControllerInner>,
    context: &PortableRequestContext,
) -> Result<(), AgentPortableRemoteError> {
    context.credential.clone().revalidate_current().await?;
    require_request_current(inner, context)
}

fn retire_failed_request(
    inner: &std::sync::Weak<AgentPortableRemoteControllerInner>,
    context: &PortableRequestContext,
    error: Option<&AgentPortableRemoteError>,
) {
    let Some(error) = error else {
        return;
    };
    if !matches!(
        error,
        AgentPortableRemoteError::Unavailable
            | AgentPortableRemoteError::CorruptStoredRegistry
            | AgentPortableRemoteError::UnsupportedStoredVersion
            | AgentPortableRemoteError::InvalidStoredRegistry
            | AgentPortableRemoteError::StoredRegistryRollback
            | AgentPortableRemoteError::StoredRegistryInterrupted
            | AgentPortableRemoteError::StoredRegistryEquivocation
            | AgentPortableRemoteError::StoredRegistryConflict
            | AgentPortableRemoteError::DuplicateStoredTarget
            | AgentPortableRemoteError::Unauthenticated
            | AgentPortableRemoteError::AccountMismatch
            | AgentPortableRemoteError::Revoked
            | AgentPortableRemoteError::VerificationFailed
            | AgentPortableRemoteError::StaleLease
            | AgentPortableRemoteError::InvalidResponse
            | AgentPortableRemoteError::PeerUnavailable
    ) {
        return;
    }
    let Some(inner) = inner.upgrade() else {
        return;
    };
    let runtime = inner.runtime.lock().unwrap().clone();
    let Some(runtime) = runtime else {
        context.peer.fence();
        context.cancellation.cancel();
        return;
    };
    let retirement = {
        let mut state = inner.state.lock().unwrap();
        let active_matches = state.active.as_ref().is_some_and(|active| {
            active.fence_epoch == context.fence_epoch && active.lease == context.lease
        });
        if !active_matches {
            return;
        }
        let Some(next_epoch) = state.fence_epoch.checked_add(1) else {
            context.peer.fence();
            context.cancellation.cancel();
            return;
        };
        state.fence_epoch = next_epoch;
        let completion = PortableCompletion::pending();
        let previous_barrier = std::mem::replace(&mut state.cleanup_barrier, completion.clone());
        let retirement = PortableRetirement {
            previous_barrier,
            pending: state.pending.take(),
            active: state.active.take(),
            requests: state.requests.drain().map(|(_, owner)| owner).collect(),
            completion,
        };
        state.prepared = None;
        retirement
    };
    start_portable_retirement(retirement, &runtime);
}

fn require_store_current(
    store: &dyn PairedTargetAuthorityStore,
    storage_key_digest: PairedTargetStorageKeyDigest,
    expected: StoredRegistryCommitToken,
) -> Result<(), AgentPortableRemoteError> {
    match store.load(storage_key_digest)? {
        StoredRegistryLoad::Committed { token, .. } if token == expected => Ok(()),
        StoredRegistryLoad::Empty { .. } | StoredRegistryLoad::Committed { .. } => {
            Err(AgentPortableRemoteError::StaleLease)
        }
    }
}

fn finish_portable_request(
    inner: &std::sync::Weak<AgentPortableRemoteControllerInner>,
    context: &PortableRequestContext,
) {
    if let Some(inner) = inner.upgrade() {
        inner
            .state
            .lock()
            .unwrap()
            .requests
            .remove(&context.request_id);
    }
    context.owner_completion.complete(());
}

fn encode_registry_body(
    registry: &StoredPairedTargetRegistryV1,
) -> Result<Vec<u8>, AgentPortableRemoteError> {
    let body = serde_json::to_vec(registry)
        .map_err(|_| AgentPortableRemoteError::InvalidStoredRegistry)?;
    if body.is_empty() || body.len() > MAX_STORED_BODY_BYTES {
        return Err(AgentPortableRemoteError::InvalidStoredRegistry);
    }
    Ok(body)
}

fn decode_json_exact<T: for<'de> Deserialize<'de>>(
    bytes: &[u8],
) -> Result<T, AgentPortableRemoteError> {
    serde_json::from_slice(bytes).map_err(|_| AgentPortableRemoteError::CorruptStoredRegistry)
}

fn state_checksum(
    storage_key_digest: PairedTargetStorageKeyDigest,
    storage_revision: u64,
    body_len: usize,
    body: &[u8],
    body_digest: [u8; DIGEST_BYTES],
) -> [u8; DIGEST_BYTES] {
    let version = STORED_SCHEMA_VERSION.to_be_bytes();
    let revision = storage_revision.to_be_bytes();
    let body_len = u32::try_from(body_len).unwrap_or(u32::MAX).to_be_bytes();
    digest_parts(
        STORED_STATE_CHECKSUM_DOMAIN,
        &[
            STORED_STATE_MAGIC,
            &version,
            &[STORED_STATE_KIND],
            &storage_key_digest.0,
            &revision,
            &body_len,
            body,
            &body_digest,
        ],
    )
}

fn guard_checksum(
    storage_key_digest: PairedTargetStorageKeyDigest,
    committed_revision: u64,
    committed_state_digest: [u8; DIGEST_BYTES],
) -> [u8; DIGEST_BYTES] {
    let version = STORED_SCHEMA_VERSION.to_be_bytes();
    let revision = committed_revision.to_be_bytes();
    digest_parts(
        STORED_GUARD_CHECKSUM_DOMAIN,
        &[
            STORED_GUARD_MAGIC,
            &version,
            &[STORED_GUARD_KIND],
            &storage_key_digest.0,
            &revision,
            &committed_state_digest,
        ],
    )
}

fn digest_parts(domain: &[u8], parts: &[&[u8]]) -> [u8; DIGEST_BYTES] {
    let mut hasher = Sha256::new();
    hasher.update(domain);
    for part in parts {
        hasher.update(part);
    }
    hasher.finalize().into()
}

fn validate_uuid(_field: &str, value: &str) -> Result<(), AgentPortableRemoteError> {
    if value.len() != 36
        || value == "00000000-0000-0000-0000-000000000000"
        || value.bytes().enumerate().any(|(index, byte)| {
            if matches!(index, 8 | 13 | 18 | 23) {
                byte != b'-'
            } else {
                !matches!(byte, b'0'..=b'9' | b'a'..=b'f')
            }
        })
    {
        return Err(AgentPortableRemoteError::InvalidStoredRegistry);
    }
    Ok(())
}

fn validate_endpoint_id(value: &str) -> Result<(), AgentPortableRemoteError> {
    if value.len() != 64
        || !value
            .bytes()
            .all(|byte| matches!(byte, b'0'..=b'9' | b'a'..=b'f'))
        || value.bytes().all(|byte| byte == b'0')
    {
        return Err(AgentPortableRemoteError::InvalidStoredRegistry);
    }
    Ok(())
}

fn validate_backend_counter(value: u64) -> Result<(), AgentPortableRemoteError> {
    if value == 0 || value > MAX_BACKEND_COUNTER {
        Err(AgentPortableRemoteError::InvalidStoredRegistry)
    } else {
        Ok(())
    }
}

fn validate_display_label(value: &str) -> Result<(), AgentPortableRemoteError> {
    let trimmed = value.trim();
    if trimmed != value
        || value.is_empty()
        || value.len() > MAX_TARGET_LABEL_BYTES
        || value.chars().count() > MAX_TARGET_LABEL_CHARS
        || value.chars().any(is_unsafe_display_character)
    {
        return Err(AgentPortableRemoteError::InvalidStoredRegistry);
    }
    Ok(())
}

fn is_unsafe_display_character(character: char) -> bool {
    character.is_control()
        || matches!(
            character,
            '\u{061c}'
                | '\u{200e}'
                | '\u{200f}'
                | '\u{202a}'..='\u{202e}'
                | '\u{2066}'..='\u{2069}'
        )
}

fn is_strictly_sorted_unique<T: Ord>(values: &[T]) -> bool {
    values.windows(2).all(|pair| pair[0] < pair[1])
}

fn read_u16(bytes: &[u8], offset: usize) -> Result<u16, AgentPortableRemoteError> {
    Ok(u16::from_be_bytes(read_array(bytes, offset)?))
}

fn read_u32(bytes: &[u8], offset: usize) -> Result<u32, AgentPortableRemoteError> {
    Ok(u32::from_be_bytes(read_array(bytes, offset)?))
}

fn read_u64(bytes: &[u8], offset: usize) -> Result<u64, AgentPortableRemoteError> {
    Ok(u64::from_be_bytes(read_array(bytes, offset)?))
}

fn read_array<const N: usize>(
    bytes: &[u8],
    offset: usize,
) -> Result<[u8; N], AgentPortableRemoteError> {
    let end = offset
        .checked_add(N)
        .ok_or(AgentPortableRemoteError::CorruptStoredRegistry)?;
    bytes
        .get(offset..end)
        .ok_or(AgentPortableRemoteError::CorruptStoredRegistry)?
        .try_into()
        .map_err(|_| AgentPortableRemoteError::CorruptStoredRegistry)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn uuid(seed: u64) -> String {
        format!("10000000-0000-4000-8000-{seed:012x}")
    }

    fn endpoint(byte: char) -> String {
        std::iter::repeat_n(byte, 64).collect()
    }

    fn evidence(format: &str, seed: u8) -> StoredOpaqueEvidenceV1 {
        StoredOpaqueEvidenceV1 {
            format: format.to_string(),
            bytes: vec![seed, seed.wrapping_add(1)],
            digest: [seed; DIGEST_BYTES],
        }
    }

    fn stored_target(seed: u64) -> StoredPairedTargetV1 {
        let digest_seed = u8::try_from(seed).unwrap_or(1).max(1);
        StoredPairedTargetV1 {
            pair_id: uuid(100 + seed),
            pairing_revision: 5,
            directory_revision: 7,
            host_registration_id: uuid(200 + seed),
            host_device_id: uuid(300 + seed),
            host_installation_id: uuid(400 + seed),
            host_endpoint_id: endpoint(if seed.is_multiple_of(2) { 'c' } else { 'b' }),
            host_endpoint_epoch: 3,
            host_display_name: format!("Maple host {seed}"),
            pairing_incarnation: 2,
            authorization: evidence(PAIR_AUTHORIZATION_EVIDENCE_FORMAT, digest_seed),
            revocation: StoredRevocationNamespaceV1 {
                stream_id: uuid(500 + seed),
                generation: 1,
                applied_sequence: 0,
                checkpoint_digest: [digest_seed.wrapping_add(1); DIGEST_BYTES],
            },
            connection_hints: StoredConnectionHintsV1 {
                relay_urls: vec!["https://relay.example.com/".to_string()],
                direct_addresses: vec!["127.0.0.1:443".to_string()],
            },
            transport_lineage: StoredTransportLineageV1::Quiescent {
                format: QUIESCENT_LINEAGE_FORMAT.to_string(),
                lineage_revision: 1,
                bytes: vec![digest_seed, 9],
                digest: [digest_seed.wrapping_add(2); DIGEST_BYTES],
                replay_floor: Some(StoredConnectionLineageFloorV1 {
                    host_epoch: 2,
                    generation: 4,
                }),
            },
        }
    }

    fn stored_registry(
        account_seed: u64,
        account_context_epoch: u64,
    ) -> StoredPairedTargetRegistryV1 {
        StoredPairedTargetRegistryV1 {
            account_id: uuid(account_seed),
            project_id: uuid(account_seed + 1),
            local_registration_id: uuid(10),
            local_device_id: uuid(11),
            local_installation_id: uuid(12),
            controller_endpoint_id: endpoint('a'),
            controller_endpoint_epoch: 2,
            account_context_epoch,
            security_epoch: 4,
            authorization_snapshot_revision: 9,
            registration_evidence: evidence("sdk.device-registration.v1", 21),
            revocation_sync_evidence: evidence("sdk.revocation-sync.v1", 22),
            targets: vec![stored_target(1)],
            lineage_tombstones: Vec::new(),
        }
    }

    fn claims_for(
        registry: &StoredPairedTargetRegistryV1,
        storage_key_digest: PairedTargetStorageKeyDigest,
    ) -> NativePortableCredentialClaims {
        NativePortableCredentialClaims {
            account_id: registry.account_id.clone(),
            project_id: registry.project_id.clone(),
            local_registration_id: registry.local_registration_id.clone(),
            local_device_id: registry.local_device_id.clone(),
            local_installation_id: registry.local_installation_id.clone(),
            controller_endpoint_id: registry.controller_endpoint_id.clone(),
            controller_endpoint_epoch: registry.controller_endpoint_epoch,
            account_context_epoch: registry.account_context_epoch,
            storage_key_digest,
        }
    }

    fn commit_candidate(
        store: &InMemoryPairedTargetAuthorityStore,
        key: PairedTargetStorageKeyDigest,
        revision: u64,
        registry: StoredPairedTargetRegistryV1,
    ) -> StoredRegistryEnvelopeV1 {
        let token = match store.load(key).expect("load current slot") {
            StoredRegistryLoad::Empty { token } | StoredRegistryLoad::Committed { token, .. } => {
                token
            }
        };
        let candidate =
            StoredRegistryEnvelopeV1::new(key, revision, registry).expect("valid candidate");
        let replacement = VerifiedStoredRegistryReplacement::for_test(candidate.clone());
        assert!(matches!(
            store.compare_and_replace(token, &replacement),
            Ok(StoredRegistryCommitOutcome::Committed)
        ));
        candidate
    }

    fn raw_state_with_body(
        key: PairedTargetStorageKeyDigest,
        revision: u64,
        body: &[u8],
    ) -> Vec<u8> {
        let body_digest = digest_parts(STORED_BODY_DIGEST_DOMAIN, &[body]);
        let checksum = state_checksum(key, revision, body.len(), body, body_digest);
        let mut encoded = Vec::with_capacity(STATE_FIXED_BYTES + body.len());
        encoded.extend_from_slice(STORED_STATE_MAGIC);
        encoded.extend_from_slice(&STORED_SCHEMA_VERSION.to_be_bytes());
        encoded.push(STORED_STATE_KIND);
        encoded.extend_from_slice(&key.0);
        encoded.extend_from_slice(&revision.to_be_bytes());
        encoded.extend_from_slice(&u32::try_from(body.len()).unwrap().to_be_bytes());
        encoded.extend_from_slice(body);
        encoded.extend_from_slice(&body_digest);
        encoded.extend_from_slice(&checksum);
        encoded
    }

    fn serialized_keys(value: &impl Serialize) -> Vec<String> {
        let mut keys = serde_json::to_value(value)
            .unwrap()
            .as_object()
            .unwrap()
            .keys()
            .cloned()
            .collect::<Vec<_>>();
        keys.sort();
        keys
    }

    #[derive(Default)]
    struct TestGate {
        open: AtomicBool,
        notify: Notify,
    }

    impl TestGate {
        fn open(&self) {
            self.open.store(true, Ordering::Release);
            self.notify.notify_waiters();
        }

        async fn wait(&self) {
            loop {
                let notified = self.notify.notified();
                if self.open.load(Ordering::Acquire) {
                    return;
                }
                notified.await;
            }
        }
    }

    async fn wait_for_counter(counter: &AtomicU64, expected: u64) {
        for _ in 0..10_000 {
            if counter.load(Ordering::Acquire) >= expected {
                return;
            }
            tokio::task::yield_now().await;
        }
        panic!("counter did not reach {expected}");
    }

    struct TestCredential {
        claims: NativePortableCredentialClaims,
        current: AtomicBool,
        revalidation_calls: AtomicU64,
    }

    impl TestCredential {
        fn new(claims: NativePortableCredentialClaims) -> Self {
            Self {
                claims,
                current: AtomicBool::new(true),
                revalidation_calls: AtomicU64::new(0),
            }
        }
    }

    impl NativePortableCredentialLease for TestCredential {
        fn claims(&self) -> &NativePortableCredentialClaims {
            &self.claims
        }

        fn revalidate_current(
            self: Arc<Self>,
        ) -> PortableFuture<'static, Result<(), AgentPortableRemoteError>> {
            Box::pin(async move {
                self.revalidation_calls.fetch_add(1, Ordering::AcqRel);
                if self.current.load(Ordering::Acquire) {
                    Ok(())
                } else {
                    Err(AgentPortableRemoteError::Unauthenticated)
                }
            })
        }
    }

    struct TestCredentialProvider {
        credential: Mutex<Option<Arc<TestCredential>>>,
        calls: AtomicU64,
    }

    impl NativePortableCredentialProvider for TestCredentialProvider {
        fn current(
            self: Arc<Self>,
        ) -> PortableFuture<
            'static,
            Result<Arc<dyn NativePortableCredentialLease>, AgentPortableRemoteError>,
        > {
            Box::pin(async move {
                self.calls.fetch_add(1, Ordering::AcqRel);
                self.credential
                    .lock()
                    .unwrap()
                    .clone()
                    .map(|credential| credential as Arc<dyn NativePortableCredentialLease>)
                    .ok_or(AgentPortableRemoteError::Unauthenticated)
            })
        }
    }

    #[derive(Clone, Copy)]
    enum TestVerifierMode {
        Valid,
        Mismatch,
        Revoked,
        Fail,
    }

    struct TestVerifier {
        mode: TestVerifierMode,
        calls: AtomicU64,
    }

    impl NativePortablePairingVerifier for TestVerifier {
        fn verify_registry(
            self: Arc<Self>,
            _credential: Arc<dyn NativePortableCredentialLease>,
            stored: StoredPairedTargetRegistryV1,
        ) -> PortableFuture<
            'static,
            Result<VerifiedAgentPortableTargetRegistry, AgentPortableRemoteError>,
        > {
            Box::pin(async move {
                self.calls.fetch_add(1, Ordering::AcqRel);
                if matches!(self.mode, TestVerifierMode::Fail) {
                    return Err(AgentPortableRemoteError::VerificationFailed);
                }
                let mut verified = VerifiedAgentPortableTargetRegistry::for_test(&stored);
                match self.mode {
                    TestVerifierMode::Mismatch => {
                        Arc::make_mut(&mut verified.targets[0]).host_endpoint_epoch += 1;
                    }
                    TestVerifierMode::Revoked => {
                        Arc::make_mut(&mut verified.targets[0]).revoked = true;
                    }
                    TestVerifierMode::Valid | TestVerifierMode::Fail => {}
                }
                Ok(verified)
            })
        }
    }

    #[derive(Clone, Copy)]
    enum TestConnectMode {
        Immediate,
        WaitForGate,
        IgnoreCancellationUntilGate,
        FailAfterAcknowledgedCleanup,
    }

    struct TestPeer {
        lease_seed: PortablePeerLeaseSeed,
        status_gate: Option<Arc<TestGate>>,
        dispose_gate: Option<Arc<TestGate>>,
        fenced: AtomicBool,
        disposed: AtomicBool,
        status_calls: AtomicU64,
        status_cancellations: AtomicU64,
        sessions_calls: AtomicU64,
        records_calls: AtomicU64,
        network_calls: AtomicU64,
        dispose_calls: AtomicU64,
    }

    impl TestPeer {
        fn new(
            generation: u64,
            status_gate: Option<Arc<TestGate>>,
            dispose_gate: Option<Arc<TestGate>>,
        ) -> Self {
            Self {
                lease_seed: PortablePeerLeaseSeed::new(7, generation).unwrap(),
                status_gate,
                dispose_gate,
                fenced: AtomicBool::new(false),
                disposed: AtomicBool::new(false),
                status_calls: AtomicU64::new(0),
                status_cancellations: AtomicU64::new(0),
                sessions_calls: AtomicU64::new(0),
                records_calls: AtomicU64::new(0),
                network_calls: AtomicU64::new(0),
                dispose_calls: AtomicU64::new(0),
            }
        }
    }

    impl AgentPortablePeer for TestPeer {
        fn lease_seed(&self) -> PortablePeerLeaseSeed {
            self.lease_seed
        }

        fn fence(&self) {
            self.fenced.store(true, Ordering::Release);
        }

        fn runtime_status(
            self: Arc<Self>,
            cancellation: PortableCancellation,
        ) -> PortableFuture<'static, Result<PortableRuntimeStatus, AgentPortableRemoteError>>
        {
            Box::pin(async move {
                self.status_calls.fetch_add(1, Ordering::AcqRel);
                if let Some(gate) = &self.status_gate {
                    tokio::select! {
                        () = gate.wait() => {}
                        () = cancellation.cancelled() => {
                            self.status_cancellations.fetch_add(1, Ordering::AcqRel);
                            return Err(AgentPortableRemoteError::Cancelled);
                        }
                    }
                }
                if cancellation.is_cancelled() {
                    return Err(AgentPortableRemoteError::Cancelled);
                }
                Ok(PortableRuntimeStatus {
                    running: true,
                    active_run_count: 1,
                })
            })
        }

        fn sessions_page(
            self: Arc<Self>,
            request: PortablePageRequest,
            cancellation: PortableCancellation,
        ) -> PortableFuture<'static, Result<PortableSessionPage, AgentPortableRemoteError>>
        {
            Box::pin(async move {
                self.sessions_calls.fetch_add(1, Ordering::AcqRel);
                if cancellation.is_cancelled() {
                    return Err(AgentPortableRemoteError::Cancelled);
                }
                Ok(PortableSessionPage {
                    items: vec![PortableSessionSummary {
                        id: "session-1".to_string(),
                        title: "Portable session".to_string(),
                        created_ms: 1,
                        updated_ms: 2,
                        page_sort_ms: 2,
                        message_count: 3,
                    }]
                    .into_iter()
                    .take(usize::from(request.limit))
                    .collect(),
                    next_cursor: Some("sessions:next".to_string()),
                })
            })
        }

        fn records_page(
            self: Arc<Self>,
            _request: PortableRecordsPageRequest,
            cancellation: PortableCancellation,
        ) -> PortableFuture<'static, Result<PortableHistoryPage, AgentPortableRemoteError>>
        {
            Box::pin(async move {
                self.records_calls.fetch_add(1, Ordering::AcqRel);
                if cancellation.is_cancelled() {
                    return Err(AgentPortableRemoteError::Cancelled);
                }
                Ok(PortableHistoryPage {
                    items: vec![PortableHistoryRecord {
                        record_id: "record:1".to_string(),
                        role: "user".to_string(),
                        created_ms: 3,
                        items: vec![PortableTimelineItem {
                            id: "item-1".to_string(),
                            item_type: "message".to_string(),
                            role: Some("user".to_string()),
                            title: Some("Message".to_string()),
                            text: Some("hello".to_string()),
                            status: Some("completed".to_string()),
                            created_ms: 3,
                            merge: "append".to_string(),
                        }],
                    }],
                    history_revision: "history-revision:1".to_string(),
                    next_cursor: Some("records:next".to_string()),
                })
            })
        }

        fn network_changed(
            self: Arc<Self>,
            cancellation: PortableCancellation,
        ) -> PortableFuture<'static, Result<(), AgentPortableRemoteError>> {
            Box::pin(async move {
                self.network_calls.fetch_add(1, Ordering::AcqRel);
                if cancellation.is_cancelled() {
                    Err(AgentPortableRemoteError::Cancelled)
                } else {
                    Ok(())
                }
            })
        }

        fn dispose(
            self: Arc<Self>,
        ) -> PortableFuture<'static, Result<(), AgentPortableRemoteError>> {
            Box::pin(async move {
                self.dispose_calls.fetch_add(1, Ordering::AcqRel);
                if let Some(gate) = &self.dispose_gate {
                    gate.wait().await;
                }
                self.disposed.store(true, Ordering::Release);
                Ok(())
            })
        }
    }

    struct TestPeerFactory {
        mode: TestConnectMode,
        connect_gate: Arc<TestGate>,
        status_gate: Option<Arc<TestGate>>,
        dispose_gate: Option<Arc<TestGate>>,
        connect_calls: AtomicU64,
        peers: Mutex<Vec<Arc<TestPeer>>>,
    }

    impl TestPeerFactory {
        fn immediate() -> Self {
            let connect_gate = Arc::new(TestGate::default());
            connect_gate.open();
            Self {
                mode: TestConnectMode::Immediate,
                connect_gate,
                status_gate: None,
                dispose_gate: None,
                connect_calls: AtomicU64::new(0),
                peers: Mutex::new(Vec::new()),
            }
        }

        fn peers(&self) -> Vec<Arc<TestPeer>> {
            self.peers.lock().unwrap().clone()
        }
    }

    impl PortablePeerFactory for TestPeerFactory {
        fn connect(
            self: Arc<Self>,
            target: Arc<VerifiedAgentPortableTarget>,
            cancellation: PortableCancellation,
        ) -> PortableFuture<'static, Result<Arc<dyn AgentPortablePeer>, AgentPortableRemoteError>>
        {
            Box::pin(async move {
                // Exercise the sealed-only route surface. A production factory
                // must additionally intersect these hints with native policy.
                assert!(!target.host_endpoint_id().is_empty());
                assert!(target.host_endpoint_epoch() > 0);
                assert!(!target.connection_hints().relay_urls.is_empty());
                assert!(target.replay_floor().is_some());
                if target
                    .transport_lineage_authority()
                    .downcast_ref::<TestVerifiedPortableTransportLineageAuthority>()
                    .is_none()
                {
                    return Err(AgentPortableRemoteError::VerificationFailed);
                }
                let generation = self.connect_calls.fetch_add(1, Ordering::AcqRel) + 1;
                match self.mode {
                    TestConnectMode::Immediate => {}
                    TestConnectMode::WaitForGate => {
                        tokio::select! {
                            () = self.connect_gate.wait() => {}
                            () = cancellation.cancelled() => {
                                return Err(AgentPortableRemoteError::Cancelled);
                            }
                        }
                    }
                    TestConnectMode::IgnoreCancellationUntilGate => {
                        self.connect_gate.wait().await;
                    }
                    TestConnectMode::FailAfterAcknowledgedCleanup => {}
                }
                let peer = Arc::new(TestPeer::new(
                    generation,
                    self.status_gate.clone(),
                    self.dispose_gate.clone(),
                ));
                self.peers.lock().unwrap().push(peer.clone());
                if matches!(self.mode, TestConnectMode::FailAfterAcknowledgedCleanup) {
                    peer.fence();
                    peer.clone().dispose().await?;
                    return Err(AgentPortableRemoteError::PeerUnavailable);
                }
                Ok(peer as Arc<dyn AgentPortablePeer>)
            })
        }
    }

    async fn wait_for_peer(factory: &TestPeerFactory, index: usize) -> Arc<TestPeer> {
        for _ in 0..10_000 {
            if let Some(peer) = factory.peers().get(index).cloned() {
                return peer;
            }
            tokio::task::yield_now().await;
        }
        panic!("peer {index} was not acquired");
    }

    struct ControllerFixture {
        controller: AgentPortableRemoteController,
        store: Arc<InMemoryPairedTargetAuthorityStore>,
        credential: Arc<TestCredential>,
        provider: Arc<TestCredentialProvider>,
        verifier: Arc<TestVerifier>,
        factory: Arc<TestPeerFactory>,
        registry: StoredPairedTargetRegistryV1,
        key: PairedTargetStorageKeyDigest,
    }

    fn controller_fixture(
        verifier_mode: TestVerifierMode,
        factory: TestPeerFactory,
    ) -> ControllerFixture {
        let store = Arc::new(InMemoryPairedTargetAuthorityStore::default());
        let key = PairedTargetStorageKeyDigest::for_test(31);
        let registry = stored_registry(1, 1);
        commit_candidate(&store, key, 1, registry.clone());
        let credential = Arc::new(TestCredential::new(claims_for(&registry, key)));
        let provider = Arc::new(TestCredentialProvider {
            credential: Mutex::new(Some(credential.clone())),
            calls: AtomicU64::new(0),
        });
        let verifier = Arc::new(TestVerifier {
            mode: verifier_mode,
            calls: AtomicU64::new(0),
        });
        let factory = Arc::new(factory);
        let controller = AgentPortableRemoteController::with_dependencies(
            store.clone(),
            provider.clone(),
            verifier.clone(),
            factory.clone(),
        );
        ControllerFixture {
            controller,
            store,
            credential,
            provider,
            verifier,
            factory,
            registry,
            key,
        }
    }

    #[test]
    fn stored_state_rejects_corruption_version_and_noncanonical_json() {
        let key = PairedTargetStorageKeyDigest::for_test(1);
        let registry = stored_registry(1, 1);
        let envelope = StoredRegistryEnvelopeV1::new(key, 1, registry.clone()).unwrap();
        let encoded = envelope.encode().unwrap();
        assert_eq!(
            StoredRegistryEnvelopeV1::decode(&encoded).unwrap(),
            envelope
        );

        let mut corrupt = encoded.clone();
        *corrupt.last_mut().unwrap() ^= 1;
        assert_eq!(
            StoredRegistryEnvelopeV1::decode(&corrupt).unwrap_err(),
            AgentPortableRemoteError::CorruptStoredRegistry
        );

        let mut unsupported = encoded;
        unsupported[8..10].copy_from_slice(&2u16.to_be_bytes());
        assert_eq!(
            StoredRegistryEnvelopeV1::decode(&unsupported).unwrap_err(),
            AgentPortableRemoteError::UnsupportedStoredVersion
        );

        let canonical = encode_registry_body(&registry).unwrap();
        let noncanonical = serde_json::to_vec_pretty(&registry).unwrap();
        assert_ne!(noncanonical, canonical);
        let encoded = raw_state_with_body(key, 1, &noncanonical);
        assert_eq!(
            StoredRegistryEnvelopeV1::decode(&encoded).unwrap_err(),
            AgentPortableRemoteError::CorruptStoredRegistry
        );

        let canonical_text = std::str::from_utf8(&canonical).unwrap();
        let duplicate_field = format!(
            "{{\"accountId\":\"{}\",{}",
            registry.account_id,
            &canonical_text[1..]
        );
        let encoded = raw_state_with_body(key, 1, duplicate_field.as_bytes());
        assert_eq!(
            StoredRegistryEnvelopeV1::decode(&encoded).unwrap_err(),
            AgentPortableRemoteError::CorruptStoredRegistry
        );

        let unknown_field = format!("{{\"unknownAuthority\":true,{}", &canonical_text[1..]);
        let encoded = raw_state_with_body(key, 1, unknown_field.as_bytes());
        assert_eq!(
            StoredRegistryEnvelopeV1::decode(&encoded).unwrap_err(),
            AgentPortableRemoteError::CorruptStoredRegistry
        );

        let deeply_nested = format!("{}0{}", "[".repeat(256), "]".repeat(256));
        assert_eq!(
            decode_json_exact::<serde_json::Value>(deeply_nested.as_bytes()).unwrap_err(),
            AgentPortableRemoteError::CorruptStoredRegistry
        );

        let oversized = vec![b' '; MAX_STORED_BODY_BYTES + 1];
        assert_eq!(
            StoredRegistryEnvelopeV1::decode(&raw_state_with_body(key, 1, &oversized)).unwrap_err(),
            AgentPortableRemoteError::CorruptStoredRegistry
        );
    }

    #[test]
    fn stored_registry_rejects_duplicate_and_equivocating_targets() {
        let key = PairedTargetStorageKeyDigest::for_test(2);
        let mut duplicate = stored_registry(1, 1);
        duplicate.targets.push(duplicate.targets[0].clone());
        assert_eq!(
            StoredRegistryEnvelopeV1::new(key, 1, duplicate).unwrap_err(),
            AgentPortableRemoteError::DuplicateStoredTarget
        );

        let mut equivocation = stored_registry(1, 1);
        let mut second = equivocation.targets[0].clone();
        second.host_registration_id = uuid(202);
        second.host_device_id = uuid(302);
        second.host_installation_id = uuid(402);
        second.host_endpoint_id = endpoint('c');
        equivocation.targets.push(second);
        assert_eq!(
            StoredRegistryEnvelopeV1::new(key, 1, equivocation).unwrap_err(),
            AgentPortableRemoteError::StoredRegistryEquivocation
        );
    }

    #[test]
    fn stored_registry_enforces_labels_routes_evidence_and_count_bounds() {
        let key = PairedTargetStorageKeyDigest::for_test(34);

        let mut boundary_label = stored_registry(1, 1);
        boundary_label.targets[0].host_display_name = "🦀".repeat(64);
        StoredRegistryEnvelopeV1::new(key, 1, boundary_label).unwrap();

        let mut oversized_label = stored_registry(1, 1);
        oversized_label.targets[0].host_display_name = "🦀".repeat(65);
        assert_eq!(
            StoredRegistryEnvelopeV1::new(key, 1, oversized_label).unwrap_err(),
            AgentPortableRemoteError::InvalidStoredRegistry
        );

        let mut too_many_relays = stored_registry(1, 1);
        too_many_relays.targets[0].connection_hints.relay_urls = (0..=MAX_RELAY_HINTS)
            .map(|index| format!("https://relay-{index}.example.com/"))
            .collect();
        assert_eq!(
            StoredRegistryEnvelopeV1::new(key, 1, too_many_relays).unwrap_err(),
            AgentPortableRemoteError::InvalidStoredRegistry
        );

        let mut oversized_evidence = stored_registry(1, 1);
        oversized_evidence.registration_evidence.bytes = vec![1; MAX_OPAQUE_EVIDENCE_BYTES + 1];
        assert_eq!(
            StoredRegistryEnvelopeV1::new(key, 1, oversized_evidence).unwrap_err(),
            AgentPortableRemoteError::InvalidStoredRegistry
        );

        let mut too_many_targets = stored_registry(1, 1);
        too_many_targets.targets = (1..=(MAX_CURRENT_TARGETS + 1))
            .map(|seed| stored_target(u64::try_from(seed).unwrap()))
            .collect();
        assert_eq!(
            StoredRegistryEnvelopeV1::new(key, 1, too_many_targets).unwrap_err(),
            AgentPortableRemoteError::InvalidStoredRegistry
        );
    }

    #[test]
    fn stored_and_verified_debug_output_redacts_authority_and_routes() {
        let key = PairedTargetStorageKeyDigest::for_test(35);
        let registry = stored_registry(1, 1);
        let target = &registry.targets[0];
        let tombstone = StoredLineageTombstoneV1 {
            host_registration_id: uuid(900),
            host_endpoint_id: endpoint('d'),
            pair_id: uuid(901),
            retired_pairing_incarnation: 1,
            retired_authorization_revision: 1,
            replay_floor: Some(StoredConnectionLineageFloorV1 {
                host_epoch: 1,
                generation: 1,
            }),
        };
        let envelope = StoredRegistryEnvelopeV1::new(key, 1, registry.clone()).unwrap();
        let guard = StoredRegistryGuardV1::committed(&envelope).unwrap();
        let token = StoredRegistryCommitToken::from(guard);
        let verified = VerifiedAgentPortableTargetRegistry::for_test(&registry);
        let replacement = VerifiedStoredRegistryReplacement::for_test(envelope.clone());
        let empty_token = StoredRegistryCommitToken::from(
            StoredRegistryGuardV1::initial(key).expect("valid initial guard"),
        );
        let empty_load = StoredRegistryLoad::Empty { token: empty_token };
        let committed_load = StoredRegistryLoad::Committed {
            envelope: Box::new(envelope.clone()),
            token,
        };
        let uncertain_floor = StoredConnectionLineageFloorV1 {
            host_epoch: 8_700_000_000_000_001,
            generation: 8_700_000_000_000_002,
        };
        let uncertain_lineage = StoredTransportLineageV1::Uncertain {
            format: UNCERTAIN_LINEAGE_FORMAT.to_string(),
            lineage_revision: 8_700_000_000_000_003,
            bytes: vec![241, 202, 187, 172, 157],
            digest: [231; DIGEST_BYTES],
            replay_floor: Some(uncertain_floor.clone()),
        };
        let output = format!(
            "{:?} {:?} {:?} {:?} {:?} {:?} {:?} {:?} {:?} {:?} {:?} {:?} {:?} {:?} {:?} {:?} {:?}",
            target.authorization,
            target.transport_lineage,
            uncertain_lineage,
            target.connection_hints,
            target.revocation,
            target,
            tombstone,
            registry,
            envelope,
            guard,
            token,
            verified,
            verified.targets[0],
            verified.targets[0].transport_lineage_authority(),
            replacement,
            empty_load,
            committed_load,
        );
        let (lineage_bytes, lineage_digest) = match &target.transport_lineage {
            StoredTransportLineageV1::Quiescent { bytes, digest, .. }
            | StoredTransportLineageV1::Uncertain { bytes, digest, .. } => (bytes, digest),
        };
        for secret in [
            registry.account_id.clone(),
            registry.project_id.clone(),
            registry.local_registration_id.clone(),
            registry.local_device_id.clone(),
            registry.local_installation_id.clone(),
            registry.controller_endpoint_id.clone(),
            target.pair_id.clone(),
            target.host_registration_id.clone(),
            target.host_device_id.clone(),
            target.host_installation_id.clone(),
            target.host_endpoint_id.clone(),
            target.host_display_name.clone(),
            target.revocation.stream_id.clone(),
            target.connection_hints.relay_urls[0].clone(),
            target.connection_hints.direct_addresses[0].clone(),
            tombstone.host_registration_id.clone(),
            tombstone.host_endpoint_id.clone(),
            tombstone.pair_id.clone(),
            format!("{:?}", registry.registration_evidence.bytes),
            format!("{:?}", registry.registration_evidence.digest),
            format!("{:?}", registry.revocation_sync_evidence.bytes),
            format!("{:?}", registry.revocation_sync_evidence.digest),
            format!("{:?}", target.authorization.bytes),
            format!("{:?}", target.authorization.digest),
            format!("{:?}", target.revocation.checkpoint_digest),
            format!("{lineage_bytes:?}"),
            format!("{lineage_digest:?}"),
            format!("{:?}", [241, 202, 187, 172, 157]),
            format!("{:?}", [231; DIGEST_BYTES]),
            uncertain_floor.host_epoch.to_string(),
            uncertain_floor.generation.to_string(),
            format!("{:?}", envelope.body_digest),
            format!("{:?}", envelope.record_checksum),
            format!("{:?}", guard.committed_state_digest),
            format!("{:?}", guard.checksum),
            format!("{:?}", key.0),
        ] {
            assert!(!output.contains(secret.as_str()));
        }
        for covered in [
            "Uncertain",
            "VerifiedPortableTransportLineageAuthority(<sealed>)",
            "VerifiedStoredRegistryReplacement",
            "Empty",
            "Committed",
        ] {
            assert!(output.contains(covered));
        }
    }

    #[test]
    fn guard_checksum_wire_preimage_matches_golden_vector() {
        assert_eq!(
            guard_checksum(
                PairedTargetStorageKeyDigest([0x11; DIGEST_BYTES]),
                0x0102_0304_0506_0708,
                [0x22; DIGEST_BYTES],
            ),
            [
                36, 200, 160, 198, 164, 160, 158, 72, 197, 224, 72, 142, 228, 127, 246, 30, 97,
                235, 231, 26, 224, 214, 159, 2, 243, 240, 129, 76, 239, 69, 193, 122,
            ]
        );
    }

    #[test]
    fn guard_detects_equal_revision_equivocation_and_state_rollback() {
        let key = PairedTargetStorageKeyDigest::for_test(3);
        let store = InMemoryPairedTargetAuthorityStore::default();
        let first = StoredRegistryEnvelopeV1::new(key, 1, stored_registry(1, 1)).unwrap();
        let mut conflicting_registry = stored_registry(1, 1);
        conflicting_registry.targets[0].host_display_name = "Other host".to_string();
        conflicting_registry.targets[0].directory_revision += 1;
        let conflicting = StoredRegistryEnvelopeV1::new(key, 1, conflicting_registry).unwrap();
        store.restore_raw_snapshot((
            Some(first.encode().unwrap()),
            Some(
                StoredRegistryGuardV1::committed(&conflicting)
                    .unwrap()
                    .encode()
                    .unwrap(),
            ),
        ));
        assert_eq!(
            store.load(key).unwrap_err(),
            AgentPortableRemoteError::StoredRegistryEquivocation
        );

        let newer = StoredRegistryEnvelopeV1::new(key, 2, stored_registry(1, 1)).unwrap();
        store.restore_raw_snapshot((
            Some(first.encode().unwrap()),
            Some(
                StoredRegistryGuardV1::committed(&newer)
                    .unwrap()
                    .encode()
                    .unwrap(),
            ),
        ));
        assert_eq!(
            store.load(key).unwrap_err(),
            AgentPortableRemoteError::StoredRegistryRollback
        );

        let gap = StoredRegistryEnvelopeV1::new(key, 3, stored_registry(1, 1)).unwrap();
        store.restore_raw_snapshot((
            Some(gap.encode().unwrap()),
            Some(
                StoredRegistryGuardV1::committed(&first)
                    .unwrap()
                    .encode()
                    .unwrap(),
            ),
        ));
        assert_eq!(
            store.load(key).unwrap_err(),
            AgentPortableRemoteError::CorruptStoredRegistry
        );
    }

    #[test]
    fn same_context_allows_only_exact_registry_replay() {
        let key = PairedTargetStorageKeyDigest::for_test(33);
        let store = InMemoryPairedTargetAuthorityStore::default();
        let original = stored_registry(1, 1);
        commit_candidate(&store, key, 1, original.clone());
        commit_candidate(&store, key, 2, original.clone());

        let token = match store.load(key).unwrap() {
            StoredRegistryLoad::Committed { token, .. } => token,
            StoredRegistryLoad::Empty { .. } => panic!("slot should be committed"),
        };
        let mut changed = original.clone();
        changed.targets[0].revocation.applied_sequence += 1;
        changed.targets[0].revocation.checkpoint_digest = [91; DIGEST_BYTES];
        let changed = StoredRegistryEnvelopeV1::new(key, 3, changed).unwrap();
        assert_eq!(
            store
                .compare_and_replace(token, &VerifiedStoredRegistryReplacement::for_test(changed),)
                .unwrap_err(),
            AgentPortableRemoteError::StoredRegistryEquivocation
        );

        let mut replacement = original;
        replacement.account_context_epoch = 2;
        replacement.targets[0].revocation.applied_sequence += 1;
        replacement.targets[0].revocation.checkpoint_digest = [92; DIGEST_BYTES];
        let replacement = StoredRegistryEnvelopeV1::new(key, 3, replacement).unwrap();
        assert_eq!(
            store
                .compare_and_replace(
                    token,
                    &VerifiedStoredRegistryReplacement::for_test(replacement),
                )
                .unwrap(),
            StoredRegistryCommitOutcome::Committed
        );
    }

    #[test]
    fn in_memory_store_recovers_interrupted_commit_and_exact_post_guard_replay() {
        let key = PairedTargetStorageKeyDigest::for_test(4);
        let store = InMemoryPairedTargetAuthorityStore::default();
        let initial_token = match store.load(key).unwrap() {
            StoredRegistryLoad::Empty { token } => token,
            StoredRegistryLoad::Committed { .. } => panic!("slot should be empty"),
        };
        let candidate = StoredRegistryEnvelopeV1::new(key, 1, stored_registry(1, 1)).unwrap();
        let replacement = VerifiedStoredRegistryReplacement::for_test(candidate.clone());
        let empty_snapshot = store.raw_snapshot();

        store.inject_fault_once(InMemoryStoreFault::BeforeState);
        assert_eq!(
            store
                .compare_and_replace(initial_token, &replacement)
                .unwrap_err(),
            AgentPortableRemoteError::StoredRegistryInterrupted
        );
        assert!(matches!(
            store.load(key).unwrap(),
            StoredRegistryLoad::Empty { .. }
        ));

        store.inject_fault_once(InMemoryStoreFault::AfterState);
        assert_eq!(
            store
                .compare_and_replace(initial_token, &replacement)
                .unwrap_err(),
            AgentPortableRemoteError::StoredRegistryInterrupted
        );
        assert_eq!(
            store.load(key).unwrap_err(),
            AgentPortableRemoteError::StoredRegistryInterrupted
        );
        assert_eq!(
            store
                .compare_and_replace(initial_token, &replacement)
                .unwrap_err(),
            AgentPortableRemoteError::StoredRegistryInterrupted
        );
        store.restore_raw_snapshot(empty_snapshot);
        assert_eq!(
            store
                .compare_and_replace(initial_token, &replacement)
                .unwrap(),
            StoredRegistryCommitOutcome::Committed
        );

        let committed_token = match store.load(key).unwrap() {
            StoredRegistryLoad::Committed { token, .. } => token,
            StoredRegistryLoad::Empty { .. } => panic!("slot should be committed"),
        };
        let next_registry = stored_registry(1, 1);
        let next = StoredRegistryEnvelopeV1::new(key, 2, next_registry).unwrap();
        let next_replacement = VerifiedStoredRegistryReplacement::for_test(next.clone());
        store.inject_fault_once(InMemoryStoreFault::AfterGuard);
        assert_eq!(
            store
                .compare_and_replace(committed_token, &next_replacement)
                .unwrap_err(),
            AgentPortableRemoteError::StoredRegistryInterrupted
        );
        let reopened_token = match store.load(key).unwrap() {
            StoredRegistryLoad::Committed { envelope, token } => {
                assert_eq!(envelope.as_ref(), &next);
                token
            }
            StoredRegistryLoad::Empty { .. } => panic!("slot should be committed"),
        };
        assert_eq!(
            store
                .compare_and_replace(reopened_token, &next_replacement)
                .unwrap(),
            StoredRegistryCommitOutcome::AlreadyCommitted
        );
    }

    #[test]
    fn installation_slot_orders_account_a_to_b_to_a_without_epoch_reuse() {
        let key = PairedTargetStorageKeyDigest::for_test(5);
        let store = InMemoryPairedTargetAuthorityStore::default();
        commit_candidate(&store, key, 1, stored_registry(1, 1));
        commit_candidate(&store, key, 2, stored_registry(20, 2));

        let token = match store.load(key).unwrap() {
            StoredRegistryLoad::Committed { token, .. } => token,
            StoredRegistryLoad::Empty { .. } => panic!("slot should be committed"),
        };
        let stale_return = StoredRegistryEnvelopeV1::new(key, 3, stored_registry(1, 1)).unwrap();
        let stale_replacement = VerifiedStoredRegistryReplacement::for_test(stale_return);
        assert_eq!(
            store
                .compare_and_replace(token, &stale_replacement)
                .unwrap_err(),
            AgentPortableRemoteError::StoredRegistryRollback
        );
        let mut fresh_registry = stored_registry(1, 3);
        fresh_registry.security_epoch += 1;
        fresh_registry.authorization_snapshot_revision += 1;
        fresh_registry.registration_evidence = evidence("sdk.device-registration.v2", 41);
        fresh_registry.revocation_sync_evidence = evidence("sdk.revocation-sync.v2", 42);
        fresh_registry.targets[0].pairing_revision += 1;
        fresh_registry.targets[0].authorization = evidence(PAIR_AUTHORIZATION_EVIDENCE_FORMAT, 43);
        let fresh_return = StoredRegistryEnvelopeV1::new(key, 3, fresh_registry).unwrap();
        let fresh_replacement = VerifiedStoredRegistryReplacement::for_test(fresh_return);
        assert_eq!(
            store
                .compare_and_replace(token, &fresh_replacement)
                .unwrap(),
            StoredRegistryCommitOutcome::Committed
        );
    }

    #[tokio::test]
    async fn transport_uncertainty_is_retained_but_never_issued_as_a_target() {
        let key = PairedTargetStorageKeyDigest::for_test(6);
        let mut registry = stored_registry(1, 1);
        registry.targets[0].transport_lineage = StoredTransportLineageV1::Uncertain {
            format: UNCERTAIN_LINEAGE_FORMAT.to_string(),
            lineage_revision: 2,
            bytes: vec![1, 2, 3],
            digest: [9; DIGEST_BYTES],
            replay_floor: Some(StoredConnectionLineageFloorV1 {
                host_epoch: 2,
                generation: 4,
            }),
        };
        let envelope = StoredRegistryEnvelopeV1::new(key, 1, registry.clone()).unwrap();
        assert_eq!(
            StoredRegistryEnvelopeV1::decode(&envelope.encode().unwrap())
                .unwrap()
                .registry,
            registry
        );
        assert!(!registry.targets[0].transport_lineage.is_quiescent());

        let store = Arc::new(InMemoryPairedTargetAuthorityStore::default());
        commit_candidate(&store, key, 1, registry.clone());
        let credential = Arc::new(TestCredential::new(claims_for(&registry, key)));
        let provider = Arc::new(TestCredentialProvider {
            credential: Mutex::new(Some(credential)),
            calls: AtomicU64::new(0),
        });
        let verifier = Arc::new(TestVerifier {
            mode: TestVerifierMode::Valid,
            calls: AtomicU64::new(0),
        });
        let factory = Arc::new(TestPeerFactory::immediate());
        let controller = AgentPortableRemoteController::with_dependencies(
            store,
            provider,
            verifier,
            factory.clone(),
        );
        assert!(controller.refresh_targets().await.unwrap().is_empty());
        assert_eq!(factory.connect_calls.load(Ordering::Acquire), 0);
        controller.dispose().await.unwrap();
    }

    #[tokio::test]
    async fn disabled_composition_is_unavailable_and_registers_no_capability() {
        let controller = AgentPortableRemoteController::disabled();
        assert_eq!(
            controller.refresh_targets().await.unwrap_err(),
            AgentPortableRemoteError::Unavailable
        );
        controller.dispose().await.unwrap();
    }

    #[tokio::test]
    async fn expected_account_is_rejected_before_verification_or_factory_activity() {
        let fixture = controller_fixture(TestVerifierMode::Valid, TestPeerFactory::immediate());
        assert_eq!(
            fixture
                .controller
                .refresh_targets_for_account("not-a-canonical-account-id")
                .await
                .unwrap_err(),
            AgentPortableRemoteError::InvalidRequest
        );
        assert_eq!(fixture.provider.calls.load(Ordering::Acquire), 0);
        assert_eq!(fixture.verifier.calls.load(Ordering::Acquire), 0);
        assert_eq!(fixture.factory.connect_calls.load(Ordering::Acquire), 0);

        assert_eq!(
            fixture
                .controller
                .refresh_targets_for_account(&uuid(999))
                .await
                .unwrap_err(),
            AgentPortableRemoteError::AccountMismatch
        );
        assert_eq!(fixture.provider.calls.load(Ordering::Acquire), 1);
        assert_eq!(fixture.verifier.calls.load(Ordering::Acquire), 0);
        assert_eq!(fixture.factory.connect_calls.load(Ordering::Acquire), 0);
        fixture.controller.dispose().await.unwrap();
    }

    #[tokio::test]
    async fn credentials_and_verification_fail_before_factory_connect() {
        let store = Arc::new(InMemoryPairedTargetAuthorityStore::default());
        let key = PairedTargetStorageKeyDigest::for_test(41);
        let registry = stored_registry(1, 1);
        commit_candidate(&store, key, 1, registry.clone());
        let no_credentials = Arc::new(TestCredentialProvider {
            credential: Mutex::new(None),
            calls: AtomicU64::new(0),
        });
        let verifier = Arc::new(TestVerifier {
            mode: TestVerifierMode::Valid,
            calls: AtomicU64::new(0),
        });
        let factory = Arc::new(TestPeerFactory::immediate());
        let controller = AgentPortableRemoteController::with_dependencies(
            store.clone(),
            no_credentials,
            verifier.clone(),
            factory.clone(),
        );
        assert_eq!(
            controller.refresh_targets().await.unwrap_err(),
            AgentPortableRemoteError::Unauthenticated
        );
        assert_eq!(verifier.calls.load(Ordering::Acquire), 0);
        assert_eq!(factory.connect_calls.load(Ordering::Acquire), 0);

        let mut wrong_claims = claims_for(&registry, key);
        wrong_claims.account_id = uuid(99);
        let wrong_credential = Arc::new(TestCredential::new(wrong_claims));
        let wrong_provider = Arc::new(TestCredentialProvider {
            credential: Mutex::new(Some(wrong_credential)),
            calls: AtomicU64::new(0),
        });
        let controller = AgentPortableRemoteController::with_dependencies(
            store,
            wrong_provider,
            verifier.clone(),
            factory.clone(),
        );
        assert_eq!(
            controller.refresh_targets().await.unwrap_err(),
            AgentPortableRemoteError::AccountMismatch
        );
        assert_eq!(verifier.calls.load(Ordering::Acquire), 0);
        assert_eq!(factory.connect_calls.load(Ordering::Acquire), 0);

        for (mode, expected) in [
            (
                TestVerifierMode::Mismatch,
                AgentPortableRemoteError::VerificationFailed,
            ),
            (TestVerifierMode::Revoked, AgentPortableRemoteError::Revoked),
            (
                TestVerifierMode::Fail,
                AgentPortableRemoteError::VerificationFailed,
            ),
        ] {
            let fixture = controller_fixture(mode, TestPeerFactory::immediate());
            assert_eq!(
                fixture.controller.refresh_targets().await.unwrap_err(),
                expected
            );
            assert_eq!(fixture.factory.connect_calls.load(Ordering::Acquire), 0);
        }
    }

    #[tokio::test]
    async fn exact_read_allowlist_returns_only_sanitized_portable_shapes() {
        let fixture = controller_fixture(TestVerifierMode::Valid, TestPeerFactory::immediate());
        let descriptors = fixture.controller.refresh_targets().await.unwrap();
        assert_eq!(descriptors.len(), 1);
        let lease = fixture
            .controller
            .prepare_target(&descriptors[0].handle)
            .await
            .unwrap();

        let status = fixture.controller.runtime_status(&lease).await.unwrap();
        assert_eq!(
            status,
            PortableRuntimeStatus {
                running: true,
                active_run_count: 1,
            }
        );
        let sessions = fixture
            .controller
            .sessions_page(
                &lease,
                PortablePageRequest {
                    cursor: None,
                    limit: 10,
                },
            )
            .await
            .unwrap();
        assert_eq!(sessions.items.len(), 1);
        assert_eq!(sessions.items[0].message_count, 3);
        let records = fixture
            .controller
            .records_page(
                &lease,
                PortableRecordsPageRequest {
                    session_id: sessions.items[0].id.clone(),
                    cursor: None,
                    limit: 10,
                },
            )
            .await
            .unwrap();
        assert_eq!(records.items.len(), 1);
        fixture.controller.network_changed(&lease).await.unwrap();

        assert_eq!(
            serialized_keys(&descriptors[0]),
            ["handle", "label"].map(str::to_string)
        );
        assert_eq!(
            serialized_keys(&status),
            ["activeRunCount", "running"].map(str::to_string)
        );
        assert_eq!(
            serialized_keys(&sessions),
            ["items", "nextCursor"].map(str::to_string)
        );
        assert_eq!(
            serialized_keys(&sessions.items[0]),
            [
                "createdMs",
                "id",
                "messageCount",
                "pageSortMs",
                "title",
                "updatedMs",
            ]
            .map(str::to_string)
        );
        assert_eq!(
            serialized_keys(&records),
            ["historyRevision", "items", "nextCursor"].map(str::to_string)
        );
        assert_eq!(
            serialized_keys(&records.items[0]),
            ["createdMs", "items", "recordId", "role"].map(str::to_string)
        );
        assert_eq!(
            serialized_keys(&records.items[0].items[0]),
            [
                "createdMs",
                "id",
                "itemType",
                "merge",
                "role",
                "status",
                "text",
                "title",
            ]
            .map(str::to_string)
        );
        // The native lease deliberately has no Serde implementation. Only the
        // mobile Tauri child projects it into a string-epoch wire lease.
        let serialized =
            serde_json::to_string(&(&descriptors[0], &status, &sessions, &records)).unwrap();
        for forbidden in [
            "accountId",
            "projectId",
            "projectRoot",
            "endpointId",
            "model",
            "mode",
            "runId",
            "toolArguments",
            "toolOutput",
            "permissionDetails",
        ] {
            assert!(!serialized.contains(forbidden));
        }

        let peer = fixture.factory.peers()[0].clone();
        assert_eq!(peer.status_calls.load(Ordering::Acquire), 1);
        assert_eq!(peer.sessions_calls.load(Ordering::Acquire), 1);
        assert_eq!(peer.records_calls.load(Ordering::Acquire), 1);
        assert_eq!(peer.network_calls.load(Ordering::Acquire), 1);
        fixture.controller.dispose().await.unwrap();
        assert!(peer.fenced.load(Ordering::Acquire));
        assert!(peer.disposed.load(Ordering::Acquire));
    }

    #[test]
    fn lease_epochs_accept_full_positive_u64_while_generation_stays_js_safe() {
        let seed = PortablePeerLeaseSeed::new(u64::MAX, 1).unwrap();
        let lease = PortableTargetLease {
            target_id: format!("lease_{}", "1".repeat(48)),
            host_epoch: seed.host_epoch,
            connection_generation: seed.connection_generation,
        };
        assert_eq!(lease.host_epoch, u64::MAX);
        lease.validate().unwrap();

        assert_eq!(
            PortablePeerLeaseSeed::new(0, 1).unwrap_err(),
            AgentPortableRemoteError::PeerUnavailable
        );
        assert_eq!(
            PortablePeerLeaseSeed::new(1, MAX_JAVASCRIPT_SAFE_UNSIGNED_INTEGER + 1).unwrap_err(),
            AgentPortableRemoteError::PeerUnavailable
        );
    }

    #[tokio::test]
    async fn invalid_pages_and_js_unsafe_responses_fail_closed() {
        let fixture = controller_fixture(TestVerifierMode::Valid, TestPeerFactory::immediate());
        let descriptors = fixture.controller.refresh_targets().await.unwrap();
        let lease = fixture
            .controller
            .prepare_target(&descriptors[0].handle)
            .await
            .unwrap();
        let peer = fixture.factory.peers()[0].clone();
        assert_eq!(
            fixture
                .controller
                .sessions_page(
                    &lease,
                    PortablePageRequest {
                        cursor: None,
                        limit: MAX_PAGE_SIZE + 1,
                    },
                )
                .await
                .unwrap_err(),
            AgentPortableRemoteError::InvalidRequest
        );
        assert_eq!(peer.sessions_calls.load(Ordering::Acquire), 0);

        let unsafe_session = PortableSessionSummary {
            id: "session-1".to_string(),
            title: "title".to_string(),
            created_ms: 1,
            updated_ms: 1,
            page_sort_ms: 1,
            message_count: MAX_JAVASCRIPT_SAFE_UNSIGNED_INTEGER + 1,
        };
        assert_eq!(
            unsafe_session.validate().unwrap_err(),
            AgentPortableRemoteError::InvalidResponse
        );
        let unsafe_permission = PortableTimelineItem {
            id: "item-1".to_string(),
            item_type: "permission".to_string(),
            role: Some("system".to_string()),
            title: Some("Tool permission".to_string()),
            text: Some("host detail".to_string()),
            status: Some("allow_once".to_string()),
            created_ms: 1,
            merge: "append".to_string(),
        };
        assert_eq!(
            unsafe_permission.validate().unwrap_err(),
            AgentPortableRemoteError::InvalidResponse
        );
        fixture.controller.dispose().await.unwrap();
    }

    #[test]
    fn session_page_validation_rejects_duplicate_session_ids() {
        let request = PortablePageRequest {
            cursor: None,
            limit: 2,
        };
        let session = PortableSessionSummary {
            id: "session-1".to_string(),
            title: "Portable session".to_string(),
            created_ms: 1,
            updated_ms: 2,
            page_sort_ms: 2,
            message_count: 3,
        };
        assert!(PortableSessionPage {
            items: vec![session.clone()],
            next_cursor: None,
        }
        .validate_for(&request)
        .is_ok());
        assert_eq!(
            PortableSessionPage {
                items: vec![session.clone(), session],
                next_cursor: None,
            }
            .validate_for(&request)
            .unwrap_err(),
            AgentPortableRemoteError::InvalidResponse
        );
    }

    #[test]
    fn history_page_validation_rejects_duplicate_record_ids() {
        let request = PortableRecordsPageRequest {
            session_id: "session-1".to_string(),
            cursor: None,
            limit: 2,
        };
        let record = PortableHistoryRecord {
            record_id: "record:1".to_string(),
            role: "user".to_string(),
            created_ms: 3,
            items: vec![PortableTimelineItem {
                id: "item-1".to_string(),
                item_type: "message".to_string(),
                role: Some("user".to_string()),
                title: None,
                text: Some("hello".to_string()),
                status: None,
                created_ms: 3,
                merge: "append".to_string(),
            }],
        };
        assert!(PortableHistoryPage {
            items: vec![record.clone()],
            history_revision: "history:1".to_string(),
            next_cursor: None,
        }
        .validate_for(&request)
        .is_ok());
        assert_eq!(
            PortableHistoryPage {
                items: vec![record.clone(), record],
                history_revision: "history:1".to_string(),
                next_cursor: None,
            }
            .validate_for(&request)
            .unwrap_err(),
            AgentPortableRemoteError::InvalidResponse
        );
    }

    #[tokio::test]
    async fn concurrent_dispose_cancels_connect_and_disposes_stale_acquisition() {
        let connect_gate = Arc::new(TestGate::default());
        let factory = TestPeerFactory {
            mode: TestConnectMode::IgnoreCancellationUntilGate,
            connect_gate: connect_gate.clone(),
            status_gate: None,
            dispose_gate: None,
            connect_calls: AtomicU64::new(0),
            peers: Mutex::new(Vec::new()),
        };
        let fixture = controller_fixture(TestVerifierMode::Valid, factory);
        let descriptors = fixture.controller.refresh_targets().await.unwrap();
        let controller = fixture.controller.clone();
        let handle = descriptors[0].handle.clone();
        let prepare = tokio::spawn(async move { controller.prepare_target(&handle).await });
        wait_for_counter(&fixture.factory.connect_calls, 1).await;

        let controller = fixture.controller.clone();
        let dispose = tokio::spawn(async move { controller.dispose().await });
        connect_gate.open();
        assert_eq!(
            prepare.await.unwrap().unwrap_err(),
            AgentPortableRemoteError::Cancelled
        );
        dispose.await.unwrap().unwrap();
        let peer = fixture.factory.peers()[0].clone();
        assert!(peer.fenced.load(Ordering::Acquire));
        assert!(peer.disposed.load(Ordering::Acquire));
    }

    #[tokio::test]
    async fn credential_flip_during_dial_rejects_and_disposes_acquired_peer() {
        let connect_gate = Arc::new(TestGate::default());
        let factory = TestPeerFactory {
            mode: TestConnectMode::IgnoreCancellationUntilGate,
            connect_gate: connect_gate.clone(),
            status_gate: None,
            dispose_gate: None,
            connect_calls: AtomicU64::new(0),
            peers: Mutex::new(Vec::new()),
        };
        let fixture = controller_fixture(TestVerifierMode::Valid, factory);
        let descriptors = fixture.controller.refresh_targets().await.unwrap();
        let controller = fixture.controller.clone();
        let handle = descriptors[0].handle.clone();
        let prepare = tokio::spawn(async move { controller.prepare_target(&handle).await });
        wait_for_counter(&fixture.factory.connect_calls, 1).await;
        fixture.credential.current.store(false, Ordering::Release);
        connect_gate.open();
        assert_eq!(
            prepare.await.unwrap().unwrap_err(),
            AgentPortableRemoteError::Unauthenticated
        );
        let peer = wait_for_peer(&fixture.factory, 0).await;
        wait_for_counter(&peer.dispose_calls, 1).await;
        assert!(peer.fenced.load(Ordering::Acquire));
        assert!(peer.disposed.load(Ordering::Acquire));
        fixture.controller.dispose().await.unwrap();
    }

    #[tokio::test]
    async fn dropped_prepare_waiter_does_not_abort_native_cleanup_owner() {
        let connect_gate = Arc::new(TestGate::default());
        let factory = TestPeerFactory {
            mode: TestConnectMode::IgnoreCancellationUntilGate,
            connect_gate: connect_gate.clone(),
            status_gate: None,
            dispose_gate: None,
            connect_calls: AtomicU64::new(0),
            peers: Mutex::new(Vec::new()),
        };
        let fixture = controller_fixture(TestVerifierMode::Valid, factory);
        let descriptors = fixture.controller.refresh_targets().await.unwrap();
        let controller = fixture.controller.clone();
        let handle = descriptors[0].handle.clone();
        let prepare = tokio::spawn(async move { controller.prepare_target(&handle).await });
        wait_for_counter(&fixture.factory.connect_calls, 1).await;
        prepare.abort();
        connect_gate.open();
        let peer = wait_for_peer(&fixture.factory, 0).await;
        wait_for_counter(&peer.dispose_calls, 1).await;
        assert!(peer.disposed.load(Ordering::Acquire));
        fixture.controller.dispose().await.unwrap();
    }

    #[tokio::test]
    async fn dropped_prepare_cleanup_blocks_the_next_connect_until_dispose_ack() {
        let connect_gate = Arc::new(TestGate::default());
        let dispose_gate = Arc::new(TestGate::default());
        let factory = TestPeerFactory {
            mode: TestConnectMode::IgnoreCancellationUntilGate,
            connect_gate: connect_gate.clone(),
            status_gate: None,
            dispose_gate: Some(dispose_gate.clone()),
            connect_calls: AtomicU64::new(0),
            peers: Mutex::new(Vec::new()),
        };
        let fixture = controller_fixture(TestVerifierMode::Valid, factory);
        let descriptors = fixture.controller.refresh_targets().await.unwrap();
        let controller = fixture.controller.clone();
        let handle = descriptors[0].handle.clone();
        let first = tokio::spawn(async move { controller.prepare_target(&handle).await });
        wait_for_counter(&fixture.factory.connect_calls, 1).await;
        first.abort();
        connect_gate.open();
        let first_peer = wait_for_peer(&fixture.factory, 0).await;
        wait_for_counter(&first_peer.dispose_calls, 1).await;

        let controller = fixture.controller.clone();
        let handle = descriptors[0].handle.clone();
        let second = tokio::spawn(async move { controller.prepare_target(&handle).await });
        for _ in 0..100 {
            tokio::task::yield_now().await;
        }
        assert_eq!(fixture.factory.connect_calls.load(Ordering::Acquire), 1);
        assert!(!second.is_finished());

        dispose_gate.open();
        let second_lease = second.await.unwrap().unwrap();
        assert_eq!(fixture.factory.connect_calls.load(Ordering::Acquire), 2);
        fixture
            .controller
            .runtime_status(&second_lease)
            .await
            .unwrap();
        fixture.controller.dispose().await.unwrap();
    }

    #[tokio::test]
    async fn factory_error_is_returned_only_after_partial_acquisition_cleanup_ack() {
        let factory = TestPeerFactory {
            mode: TestConnectMode::FailAfterAcknowledgedCleanup,
            connect_gate: Arc::new(TestGate::default()),
            status_gate: None,
            dispose_gate: None,
            connect_calls: AtomicU64::new(0),
            peers: Mutex::new(Vec::new()),
        };
        let fixture = controller_fixture(TestVerifierMode::Valid, factory);
        let descriptors = fixture.controller.refresh_targets().await.unwrap();
        assert_eq!(
            fixture
                .controller
                .prepare_target(&descriptors[0].handle)
                .await
                .unwrap_err(),
            AgentPortableRemoteError::PeerUnavailable
        );
        let peer = fixture.factory.peers()[0].clone();
        assert!(peer.fenced.load(Ordering::Acquire));
        assert!(peer.disposed.load(Ordering::Acquire));
        assert_eq!(peer.dispose_calls.load(Ordering::Acquire), 1);
        fixture.controller.dispose().await.unwrap();
    }

    #[tokio::test]
    async fn dispose_cancels_inflight_request_before_peer_teardown() {
        let status_gate = Arc::new(TestGate::default());
        let factory = TestPeerFactory {
            mode: TestConnectMode::Immediate,
            connect_gate: Arc::new(TestGate::default()),
            status_gate: Some(status_gate),
            dispose_gate: None,
            connect_calls: AtomicU64::new(0),
            peers: Mutex::new(Vec::new()),
        };
        let fixture = controller_fixture(TestVerifierMode::Valid, factory);
        let descriptors = fixture.controller.refresh_targets().await.unwrap();
        let lease = fixture
            .controller
            .prepare_target(&descriptors[0].handle)
            .await
            .unwrap();
        let peer = fixture.factory.peers()[0].clone();
        let controller = fixture.controller.clone();
        let status_lease = lease.clone();
        let status = tokio::spawn(async move { controller.runtime_status(&status_lease).await });
        wait_for_counter(&peer.status_calls, 1).await;
        fixture.controller.dispose().await.unwrap();
        assert_eq!(
            status.await.unwrap().unwrap_err(),
            AgentPortableRemoteError::Cancelled
        );
        assert!(peer.disposed.load(Ordering::Acquire));
    }

    #[tokio::test]
    async fn credential_flip_during_rpc_rejects_response_and_retires_peer() {
        let status_gate = Arc::new(TestGate::default());
        let factory = TestPeerFactory {
            mode: TestConnectMode::Immediate,
            connect_gate: Arc::new(TestGate::default()),
            status_gate: Some(status_gate.clone()),
            dispose_gate: None,
            connect_calls: AtomicU64::new(0),
            peers: Mutex::new(Vec::new()),
        };
        let fixture = controller_fixture(TestVerifierMode::Valid, factory);
        let descriptors = fixture.controller.refresh_targets().await.unwrap();
        let lease = fixture
            .controller
            .prepare_target(&descriptors[0].handle)
            .await
            .unwrap();
        let peer = fixture.factory.peers()[0].clone();
        let controller = fixture.controller.clone();
        let status_lease = lease.clone();
        let status = tokio::spawn(async move { controller.runtime_status(&status_lease).await });
        wait_for_counter(&peer.status_calls, 1).await;
        fixture.credential.current.store(false, Ordering::Release);
        status_gate.open();
        assert_eq!(
            status.await.unwrap().unwrap_err(),
            AgentPortableRemoteError::Unauthenticated
        );
        wait_for_counter(&peer.dispose_calls, 1).await;
        assert!(peer.fenced.load(Ordering::Acquire));
        assert!(peer.disposed.load(Ordering::Acquire));
        fixture.controller.dispose().await.unwrap();
    }

    #[tokio::test]
    async fn registry_replacement_during_rpc_rejects_response_and_retires_peer() {
        let status_gate = Arc::new(TestGate::default());
        let factory = TestPeerFactory {
            mode: TestConnectMode::Immediate,
            connect_gate: Arc::new(TestGate::default()),
            status_gate: Some(status_gate.clone()),
            dispose_gate: None,
            connect_calls: AtomicU64::new(0),
            peers: Mutex::new(Vec::new()),
        };
        let fixture = controller_fixture(TestVerifierMode::Valid, factory);
        let descriptors = fixture.controller.refresh_targets().await.unwrap();
        let lease = fixture
            .controller
            .prepare_target(&descriptors[0].handle)
            .await
            .unwrap();
        let peer = fixture.factory.peers()[0].clone();
        let controller = fixture.controller.clone();
        let status_lease = lease.clone();
        let status = tokio::spawn(async move { controller.runtime_status(&status_lease).await });
        wait_for_counter(&peer.status_calls, 1).await;
        let mut replacement = fixture.registry.clone();
        replacement.account_context_epoch += 1;
        commit_candidate(&fixture.store, fixture.key, 2, replacement);
        status_gate.open();
        assert_eq!(
            status.await.unwrap().unwrap_err(),
            AgentPortableRemoteError::StaleLease
        );
        wait_for_counter(&peer.dispose_calls, 1).await;
        assert!(peer.fenced.load(Ordering::Acquire));
        assert!(peer.disposed.load(Ordering::Acquire));
        fixture.controller.dispose().await.unwrap();
    }

    #[tokio::test]
    async fn dropped_read_waiter_cancels_peer_operation_without_dropping_controller() {
        let status_gate = Arc::new(TestGate::default());
        let factory = TestPeerFactory {
            mode: TestConnectMode::Immediate,
            connect_gate: Arc::new(TestGate::default()),
            status_gate: Some(status_gate.clone()),
            dispose_gate: None,
            connect_calls: AtomicU64::new(0),
            peers: Mutex::new(Vec::new()),
        };
        let fixture = controller_fixture(TestVerifierMode::Valid, factory);
        let descriptors = fixture.controller.refresh_targets().await.unwrap();
        let lease = fixture
            .controller
            .prepare_target(&descriptors[0].handle)
            .await
            .unwrap();
        let peer = fixture.factory.peers()[0].clone();
        let controller = fixture.controller.clone();
        let status_lease = lease.clone();
        let status = tokio::spawn(async move { controller.runtime_status(&status_lease).await });
        wait_for_counter(&peer.status_calls, 1).await;
        status.abort();
        wait_for_counter(&peer.status_cancellations, 1).await;

        status_gate.open();
        fixture.controller.runtime_status(&lease).await.unwrap();
        fixture.controller.dispose().await.unwrap();
    }

    #[tokio::test]
    async fn last_controller_drop_during_request_still_drains_native_peer() {
        let status_gate = Arc::new(TestGate::default());
        let factory = TestPeerFactory {
            mode: TestConnectMode::Immediate,
            connect_gate: Arc::new(TestGate::default()),
            status_gate: Some(status_gate),
            dispose_gate: None,
            connect_calls: AtomicU64::new(0),
            peers: Mutex::new(Vec::new()),
        };
        let fixture = controller_fixture(TestVerifierMode::Valid, factory);
        let descriptors = fixture.controller.refresh_targets().await.unwrap();
        let lease = fixture
            .controller
            .prepare_target(&descriptors[0].handle)
            .await
            .unwrap();
        let peer = fixture.factory.peers()[0].clone();
        let controller = fixture.controller.clone();
        let request = tokio::spawn(async move { controller.runtime_status(&lease).await });
        wait_for_counter(&peer.status_calls, 1).await;

        drop(fixture);
        request.abort();
        let _ = request.await;
        wait_for_counter(&peer.status_cancellations, 1).await;
        wait_for_counter(&peer.dispose_calls, 1).await;
        assert!(peer.fenced.load(Ordering::Acquire));
        assert!(peer.disposed.load(Ordering::Acquire));
    }

    #[tokio::test]
    async fn dispose_waits_for_native_acknowledgement() {
        let dispose_gate = Arc::new(TestGate::default());
        let factory = TestPeerFactory {
            mode: TestConnectMode::Immediate,
            connect_gate: Arc::new(TestGate::default()),
            status_gate: None,
            dispose_gate: Some(dispose_gate.clone()),
            connect_calls: AtomicU64::new(0),
            peers: Mutex::new(Vec::new()),
        };
        let fixture = controller_fixture(TestVerifierMode::Valid, factory);
        let descriptors = fixture.controller.refresh_targets().await.unwrap();
        fixture
            .controller
            .prepare_target(&descriptors[0].handle)
            .await
            .unwrap();
        let peer = fixture.factory.peers()[0].clone();
        let controller = fixture.controller.clone();
        let dispose = tokio::spawn(async move { controller.dispose().await });
        wait_for_counter(&peer.dispose_calls, 1).await;
        assert!(!dispose.is_finished());
        dispose_gate.open();
        dispose.await.unwrap().unwrap();
        assert!(peer.disposed.load(Ordering::Acquire));
    }

    #[tokio::test]
    async fn target_switch_fences_old_lease_before_new_connect() {
        let fixture = controller_fixture(TestVerifierMode::Valid, TestPeerFactory::immediate());
        let descriptors = fixture.controller.refresh_targets().await.unwrap();
        let first_lease = fixture
            .controller
            .prepare_target(&descriptors[0].handle)
            .await
            .unwrap();
        let second_lease = fixture
            .controller
            .prepare_target(&descriptors[0].handle)
            .await
            .unwrap();
        assert_ne!(first_lease, second_lease);
        let peers = fixture.factory.peers();
        assert_eq!(peers.len(), 2);
        assert!(peers[0].fenced.load(Ordering::Acquire));
        assert!(peers[0].disposed.load(Ordering::Acquire));
        assert_eq!(
            fixture
                .controller
                .runtime_status(&first_lease)
                .await
                .unwrap_err(),
            AgentPortableRemoteError::StaleLease
        );
        fixture
            .controller
            .runtime_status(&second_lease)
            .await
            .unwrap();
        fixture.controller.dispose().await.unwrap();
    }

    #[tokio::test]
    async fn account_switch_replaces_handles_and_fences_old_peer() {
        let fixture = controller_fixture(TestVerifierMode::Valid, TestPeerFactory::immediate());
        let first_descriptors = fixture.controller.refresh_targets().await.unwrap();
        let first_lease = fixture
            .controller
            .prepare_target(&first_descriptors[0].handle)
            .await
            .unwrap();
        let first_peer = fixture.factory.peers()[0].clone();

        let next_registry = stored_registry(20, 2);
        commit_candidate(&fixture.store, fixture.key, 2, next_registry.clone());
        let next_credential =
            Arc::new(TestCredential::new(claims_for(&next_registry, fixture.key)));
        *fixture.provider.credential.lock().unwrap() = Some(next_credential);
        let next_descriptors = fixture.controller.refresh_targets().await.unwrap();
        assert_ne!(first_descriptors[0].handle, next_descriptors[0].handle);
        assert!(first_peer.fenced.load(Ordering::Acquire));
        assert!(first_peer.disposed.load(Ordering::Acquire));
        assert!(matches!(
            fixture.controller.runtime_status(&first_lease).await,
            Err(AgentPortableRemoteError::PeerUnavailable)
                | Err(AgentPortableRemoteError::StaleLease)
        ));
        fixture.controller.dispose().await.unwrap();
    }

    #[tokio::test]
    async fn native_signout_hook_fences_idle_peer_before_acknowledgement() {
        let fixture = controller_fixture(TestVerifierMode::Valid, TestPeerFactory::immediate());
        let descriptors = fixture.controller.refresh_targets().await.unwrap();
        let lease = fixture
            .controller
            .prepare_target(&descriptors[0].handle)
            .await
            .unwrap();
        let peer = fixture.factory.peers()[0].clone();
        fixture.credential.current.store(false, Ordering::Release);
        fixture
            .controller
            .native_credentials_invalidated()
            .await
            .unwrap();
        assert!(peer.fenced.load(Ordering::Acquire));
        assert!(peer.disposed.load(Ordering::Acquire));
        assert_eq!(
            fixture.controller.runtime_status(&lease).await.unwrap_err(),
            AgentPortableRemoteError::PeerUnavailable
        );
        fixture.controller.dispose().await.unwrap();
    }

    #[tokio::test]
    async fn revoked_credential_blocks_request_before_peer_call() {
        let fixture = controller_fixture(TestVerifierMode::Valid, TestPeerFactory::immediate());
        let descriptors = fixture.controller.refresh_targets().await.unwrap();
        let lease = fixture
            .controller
            .prepare_target(&descriptors[0].handle)
            .await
            .unwrap();
        let peer = fixture.factory.peers()[0].clone();
        fixture.credential.current.store(false, Ordering::Release);
        assert_eq!(
            fixture.controller.runtime_status(&lease).await.unwrap_err(),
            AgentPortableRemoteError::Unauthenticated
        );
        assert_eq!(peer.status_calls.load(Ordering::Acquire), 0);
        fixture.controller.dispose().await.unwrap();
    }

    #[tokio::test]
    async fn newer_authority_snapshot_blocks_request_before_peer_call() {
        let fixture = controller_fixture(TestVerifierMode::Valid, TestPeerFactory::immediate());
        let descriptors = fixture.controller.refresh_targets().await.unwrap();
        let lease = fixture
            .controller
            .prepare_target(&descriptors[0].handle)
            .await
            .unwrap();
        let peer = fixture.factory.peers()[0].clone();
        let mut next_registry = fixture.registry.clone();
        next_registry.account_context_epoch += 1;
        next_registry.authorization_snapshot_revision += 1;
        commit_candidate(&fixture.store, fixture.key, 2, next_registry);
        assert_eq!(
            fixture.controller.runtime_status(&lease).await.unwrap_err(),
            AgentPortableRemoteError::StaleLease
        );
        assert_eq!(peer.status_calls.load(Ordering::Acquire), 0);
        fixture.controller.dispose().await.unwrap();
    }
}
