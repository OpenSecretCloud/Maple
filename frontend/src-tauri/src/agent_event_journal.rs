//! Bounded durable replay for already-sanitized Agent presentation events.
//!
//! This journal is deliberately independent from persisted-history pagination:
//! history cursors address Goose storage, while [`LiveEventCursor`] addresses a
//! short retained suffix of live presentation events. A missing retained suffix
//! is never treated as an empty replay; callers receive [`SnapshotRequired`]
//! and must rebuild from paged history before resuming live delivery.
//!
//! The journal does not project `AgentServiceEvent` itself. The global Agent
//! event sink does not carry account ownership, so persisting at that boundary
//! could publish an old account's event after an account transition. Callers
//! must first project an event into a reviewed, bounded payload type and append
//! it with the exact opaque account scope and current account generation.
//!
//! Exactly one journal instance must be composed into Maple's single host
//! process; clones share its mutex. An exclusive root lock also rejects a
//! second process or accidentally independent instance for the same root.
//!
//! Format v3 keeps a large immutable snapshot behind a fixed prefix and writes
//! bounded hash-chained event frames after it. Two alternating checksummed
//! anchor slots in that prefix commit the exact terminal sequence, byte offset,
//! and chain hash without rewriting the snapshot on every event. Only an
//! all-zero slot is absent: every nonzero slot must be fully checksummed and
//! valid, so a torn anchor write fails closed instead of rolling back. File EOF
//! must equal the selected anchor's exact committed end; even a well-formed
//! hash-chained frame beyond it is ambiguous and requires authoritative reseed
//! instead of automatic adoption or truncation. The checksum is a crash-tear
//! detector, not a keyed authenticity proof; malicious same-UID storage
//! mutation remains outside this journal's threat model.
//!
//! Durable construction is currently supported only on macOS and Linux. The
//! journal fails closed elsewhere until that platform has an owner-only ACL,
//! no-follow opens, and a durable atomic replacement implementation.

#![allow(
    dead_code,
    reason = "the replay journal is wired by the remote Agent vertical slice"
)]

use crate::agent_live_authority::VerifiedJournalReseedAuthority;
use fs2::FileExt;
use getrandom::fill as fill_random;
use serde::{de::DeserializeOwned, Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    collections::{HashMap, HashSet, VecDeque},
    fmt,
    fs::{self, File, OpenOptions},
    io::{ErrorKind, Read, Write},
    path::{Path, PathBuf},
    sync::{Arc, Mutex, MutexGuard},
};

#[cfg(test)]
use std::sync::atomic::{AtomicBool, AtomicU8, Ordering};

#[cfg(unix)]
use std::os::unix::fs::MetadataExt;

const JOURNAL_FORMAT_VERSION: u8 = 3;
const JOURNAL_ID_BYTES: usize = 16;
const JOURNAL_ID_HEX_BYTES: usize = JOURNAL_ID_BYTES * 2;
const ACCOUNT_KEY_HEX_BYTES: usize = 64;
const MAX_ACCOUNT_SCOPE_BYTES: usize = 256;
const MAX_EVENT_OWNER_ID_BYTES: usize = 128;
const MAX_HEADER_BYTES: usize = 32 * 1_024 * 1_024;
const MAX_RECORD_OVERHEAD_BYTES: usize = 768;
const MAX_ACCOUNT_JOURNAL_FILES: usize = 64;
const TEMP_FILE_PREFIX: &str = ".agent-live-events-";
const RETIRING_FILE_PREFIX: &str = ".agent-live-retiring-v1-";
const PROCESS_TOKEN_BYTES: usize = 16;
const PROCESS_TOKEN_HEX_BYTES: usize = PROCESS_TOKEN_BYTES * 2;
const MAX_CURSOR_SEQUENCE: u64 = 9_007_199_254_740_991;
const MAX_IDEMPOTENCY_EVENT_IDS: usize = 65_536;
const MAX_IDEMPOTENCY_METADATA_BYTES: usize = 20 * 1_024 * 1_024;
const MAX_CHECKPOINT_BYTES: usize = 8 * 1_024 * 1_024;
const CHECKPOINT_SCHEMA: &str = "maple_live_projection_v1";
const DISK_SUPERBLOCK_MAGIC: &[u8; 8] = b"MPLAEJ3\0";
const DISK_ANCHOR_MAGIC: &[u8; 8] = b"MPLANCH3";
const DISK_FRAME_MAGIC: &[u8; 4] = b"MEV3";
const DISK_SUPERBLOCK_VERSION: u32 = 3;
const DISK_ANCHOR_VERSION: u32 = 1;
const DISK_FRAME_VERSION: u8 = 1;
const DISK_SUPERBLOCK_BYTES: usize = 80;
const DISK_SUPERBLOCK_BYTES_U32: u32 = 80;
const DISK_ANCHOR_SLOT_BYTES: usize = 256;
const DISK_ANCHOR_SLOT_BYTES_U32: u32 = 256;
const DISK_ANCHOR_SLOT_COUNT: usize = 2;
const DISK_PREFIX_BYTES: usize =
    DISK_SUPERBLOCK_BYTES + DISK_ANCHOR_SLOT_BYTES * DISK_ANCHOR_SLOT_COUNT;
const DISK_PREFIX_BYTES_U32: u32 = 592;
const DISK_FRAME_HEADER_BYTES: usize = 88;
const DISK_SUPERBLOCK_HASHED_BYTES: usize = 48;
const DISK_ANCHOR_HASHED_BYTES: usize = 224;
const DISK_SUPERBLOCK_CHECKSUM_DOMAIN: &[u8] = b"maple-agent-journal-v3-superblock";
const DISK_ANCHOR_CHECKSUM_DOMAIN: &[u8] = b"maple-agent-journal-v3-anchor";
const DISK_SNAPSHOT_HASH_DOMAIN: &[u8] = b"maple-agent-journal-v3-snapshot";
const DISK_FRAME_HASH_DOMAIN: &[u8] = b"maple-agent-journal-v3-frame";
const DISK_CHAIN_BASE_DOMAIN: &[u8] = b"maple-agent-journal-v3-chain-base";
const OBSERVED_FILE_DIGEST_DOMAIN: &[u8] = b"maple-agent-journal-observed-file-v1";
const PROJECTION_DIGEST_DOMAIN: &[u8] = b"maple-agent-journal-authoritative-projection-v1";
const INGRESS_EVENT_NAMESPACE_DOMAIN: &[u8] = b"maple-agent-journal-ingress-event-namespace-v1";
const AMBIGUOUS_EVENT_ID_DOMAIN: &[u8] = b"maple-agent-journal-ambiguous-event-id-v1";

/// Production defaults intentionally retain only a short live suffix.
///
/// The history pager remains the durable source of truth. These bounds cover
/// ordinary phone background/foreground gaps without allowing streaming output
/// to grow a second unbounded history store.
pub(crate) const DEFAULT_LIVE_EVENT_JOURNAL_LIMITS: LiveEventJournalLimits =
    LiveEventJournalLimits {
        max_entries: 2_048,
        max_payload_bytes: 256 * 1_024,
        max_total_payload_bytes: 8 * 1_024 * 1_024,
        max_replay_entries: 50,
        max_replay_payload_bytes: 700 * 1_024,
    };

/// Create the dedicated private directory that must directly contain the live
/// event journal root. The broader app-local-data directory may be readable by
/// other principals, but neither it nor its trusted canonical ancestry may
/// grant cross-principal rename authority.
pub(crate) fn prepare_live_event_journal_parent(path: &Path) -> Result<(), LiveEventJournalError> {
    ensure_supported_platform()?;
    let file_name = path
        .file_name()
        .filter(|name| !name.is_empty())
        .ok_or(LiveEventJournalError::StorageUnavailable)?;
    let requested_parent = path
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
        .ok_or(LiveEventJournalError::StorageUnavailable)?;
    let canonical_parent = fs::canonicalize(requested_parent)
        .map_err(|_| LiveEventJournalError::StorageUnavailable)?;
    verify_safe_directory_ancestry(&canonical_parent)?;
    let dedicated_parent = canonical_parent.join(file_name);
    match fs::symlink_metadata(&dedicated_parent) {
        Ok(metadata) => {
            if !metadata.file_type().is_dir()
                || metadata.file_type().is_symlink()
                || !metadata_owned_by_effective_user(&metadata)
            {
                return Err(LiveEventJournalError::StorageUnavailable);
            }
        }
        Err(error) if error.kind() == ErrorKind::NotFound => {
            create_owner_only_directory(&dedicated_parent)?;
        }
        Err(_) => return Err(LiveEventJournalError::StorageUnavailable),
    }
    let directory = open_directory_no_follow(&dedicated_parent)?;
    set_owner_only_directory(&directory)?;
    sync_directory_path(&canonical_parent)?;
    verify_private_parent_directory(&dedicated_parent)
}

/// A payload admitted to the durable live-event journal.
///
/// Implement this only for a Maple-owned, presentation-safe wire projection.
/// Raw Goose events, provider messages, prompts, tool contexts, credentials,
/// and arbitrary `serde_json::Value` payloads are not suitable implementations.
/// Serialization must also be canonical for equal values: payloads must not
/// contain unordered maps or serializers whose output varies between calls or
/// processes, because the bytes feed the durable event commitment.
pub(crate) trait LiveReplayPayload:
    Clone + Serialize + DeserializeOwned + Send + Sync + 'static
{
    /// Stable account-wide ID for exactly one projected event. A caller retry
    /// after an ambiguous storage error must reuse this ID. Timeline updates
    /// need an event/revision ID, not merely the timeline item's stable row ID.
    fn live_replay_event_id(&self) -> &str;

    /// Revalidate semantic bounds before both persistence and replay.
    fn validate_live_replay_payload(&self) -> Result<(), LiveEventJournalError>;
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct LiveEventJournalLimits {
    pub(crate) max_entries: usize,
    pub(crate) max_payload_bytes: usize,
    pub(crate) max_total_payload_bytes: usize,
    pub(crate) max_replay_entries: usize,
    /// Low-level response safety bound, separate from history page semantics.
    /// Metadata overhead is additionally bounded by `max_replay_entries`.
    pub(crate) max_replay_payload_bytes: usize,
}

impl LiveEventJournalLimits {
    fn validate(self) -> Result<Self, LiveEventJournalError> {
        if self.max_entries == 0
            || self.max_payload_bytes == 0
            || self.max_total_payload_bytes < self.max_payload_bytes
            || self.max_replay_entries == 0
            || self.max_replay_entries > self.max_entries
            || self.max_replay_payload_bytes < self.max_payload_bytes
            || self.max_replay_payload_bytes > self.max_total_payload_bytes
        {
            return Err(LiveEventJournalError::InvalidLimits);
        }
        self.max_disk_bytes()
            .ok_or(LiveEventJournalError::InvalidLimits)?;
        Ok(self)
    }

    fn max_disk_bytes(self) -> Option<u64> {
        let framed_entry_overhead =
            MAX_RECORD_OVERHEAD_BYTES.checked_add(DISK_FRAME_HEADER_BYTES)?;
        let entry_overhead = self.max_entries.checked_mul(framed_entry_overhead)?;
        let total = DISK_PREFIX_BYTES
            .checked_add(MAX_HEADER_BYTES)?
            .checked_add(self.max_total_payload_bytes)?
            .checked_add(entry_overhead)?
            // One append can durably reach disk immediately before a required
            // compaction on process interruption.
            .checked_add(self.max_payload_bytes)?
            .checked_add(framed_entry_overhead)?;
        u64::try_from(total).ok()
    }
}

/// Exact account owner required for every journal operation.
///
/// The raw account scope is hashed immediately and is never written to disk.
/// `account_generation` is Maple's revocable, process-local data generation; a
/// stale handle cannot append to or replay the current in-memory journal. It is
/// intentionally not persisted because Maple's current generation resets when
/// the process restarts.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct LiveEventAccountOwner {
    account_key: String,
    account_generation: u64,
}

/// Process-local authority for one exact active journal owner.
///
/// The random token is deliberately opaque, never serialized, and changes
/// across destructive owner transitions. Possessing only an account key and
/// process generation is not authority to reopen, mutate, or recreate a
/// retired journal.
#[derive(Clone, PartialEq, Eq)]
pub(crate) struct LiveEventJournalLease {
    owner: LiveEventAccountOwner,
    operation_token: [u8; PROCESS_TOKEN_BYTES],
}

/// Opaque producer capability bound to one exact active journal generation.
///
/// A coordinator may clone this for all events emitted by one admitted run,
/// but must never hand the broader activation lease to a producer. Rollover,
/// reseed, retirement, and account rotation all revoke this capability.
#[derive(Clone, PartialEq, Eq)]
pub(crate) struct LiveEventJournalIngressLease {
    owner: LiveEventAccountOwner,
    operation_token: [u8; PROCESS_TOKEN_BYTES],
    journal_id: [u8; JOURNAL_ID_BYTES],
}

impl fmt::Debug for LiveEventJournalLease {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("LiveEventJournalLease")
            .field("account_generation", &self.owner.account_generation)
            .field("operation_token", &"<redacted>")
            .finish_non_exhaustive()
    }
}

impl LiveEventJournalLease {
    pub(crate) const fn account_generation(&self) -> u64 {
        self.owner.account_generation
    }
}

impl fmt::Debug for LiveEventJournalIngressLease {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("LiveEventJournalIngressLease")
            .field("account_generation", &self.owner.account_generation)
            .field("operation_token", &"<redacted>")
            .field("journal_id", &"<redacted>")
            .finish_non_exhaustive()
    }
}

impl LiveEventJournalIngressLease {
    /// Stable, non-reversible namespace commitment for producer event IDs
    /// within this journal generation. It is read-only and never sufficient
    /// to reconstruct either the journal ID or this capability.
    pub(crate) fn event_namespace_commitment(&self) -> [u8; 32] {
        sha256_parts(
            INGRESS_EVENT_NAMESPACE_DOMAIN,
            &[
                self.owner.account_key.as_bytes(),
                &self.owner.account_generation.to_le_bytes(),
                &self.journal_id,
            ],
        )
    }
}

/// One-use, process-local proof that a FIFO actor sealed an exact journal head
/// before retirement began. The token cannot be constructed from renderer or
/// wire data and cannot authorize a later activation of the same stable key.
#[derive(Clone, PartialEq, Eq)]
pub(crate) struct LiveEventJournalRetirementToken {
    owner: LiveEventAccountOwner,
    operation_token: [u8; PROCESS_TOKEN_BYTES],
    retirement_nonce: [u8; PROCESS_TOKEN_BYTES],
    journal_id: String,
    head_sequence: u64,
}

/// Move-only proof that the coordinator sealed its FIFO and paused every
/// subscriber at one exact durable head before beginning a journal-generation
/// rollover. The preselected replacement ID makes an ambiguous atomic replace
/// retryable without ever blessing a different generation.
pub(crate) struct LiveEventJournalRolloverObligation {
    owner: LiveEventAccountOwner,
    operation_token: [u8; PROCESS_TOKEN_BYTES],
    new_operation_token: [u8; PROCESS_TOKEN_BYTES],
    rollover_nonce: [u8; PROCESS_TOKEN_BYTES],
    journal_id: String,
    head_sequence: u64,
    checkpoint_commitment: [u8; 32],
    new_journal_id: String,
}

/// Opaque observation of the exact account-file generation that could not be
/// activated. Its random process token prevents a caller from fabricating a
/// reseed request from an owner and path alone.
#[derive(PartialEq, Eq)]
pub(crate) struct LiveEventJournalReseedRequired {
    owner: LiveEventAccountOwner,
    observed: ObservedJournalGeneration,
    observation_token: [u8; PROCESS_TOKEN_BYTES],
}

#[derive(Debug, PartialEq, Eq)]
pub(crate) enum LiveEventJournalActivationError {
    Journal(LiveEventJournalError),
    ReseedRequired(Box<LiveEventJournalReseedRequired>),
}

impl From<LiveEventJournalError> for LiveEventJournalActivationError {
    fn from(error: LiveEventJournalError) -> Self {
        Self::Journal(error)
    }
}

impl fmt::Debug for LiveEventJournalReseedRequired {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("LiveEventJournalReseedRequired")
            .field("account_generation", &self.owner.account_generation)
            .field("observed", &self.observed)
            .field("observation_token", &"<redacted>")
            .finish_non_exhaustive()
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum ObservedJournalGeneration {
    Missing,
    V3 {
        file_nonce: [u8; PROCESS_TOKEN_BYTES],
        journal_id: [u8; JOURNAL_ID_BYTES],
        head_sequence: u64,
        committed_end: u64,
        digest: [u8; 32],
        file_identity: FileIdentity,
    },
    LegacyOrCorrupt {
        length: u64,
        digest: [u8; 32],
        file_identity: FileIdentity,
    },
}

impl LiveEventJournalReseedRequired {
    pub(crate) fn owner(&self) -> &LiveEventAccountOwner {
        &self.owner
    }
}

/// Two-phase reseed obligation prepared from host-only verified authority.
/// Commit remains impossible until the host has FIFO-sealed publication and
/// calls `mark_reseed_sealed` on this exact non-Clone obligation.
pub(crate) struct LiveEventJournalReseedObligation {
    owner: LiveEventAccountOwner,
    observed: ObservedJournalGeneration,
    observation_token: [u8; PROCESS_TOKEN_BYTES],
    authority_nonce: [u8; 32],
    durable_head_commitment: [u8; 32],
    projection_digest: [u8; 32],
    projection_bytes: Box<[u8]>,
    new_journal_id: String,
    sealed: bool,
}

impl fmt::Debug for LiveEventJournalReseedObligation {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("LiveEventJournalReseedObligation")
            .field("account_generation", &self.owner.account_generation)
            .field("observed", &self.observed)
            .field("authority_nonce", &"<redacted>")
            .field("durable_head_commitment", &"<redacted>")
            .field("projection_digest", &"<redacted>")
            .field("projection_bytes", &"<redacted>")
            .field("new_journal_id", &self.new_journal_id)
            .field("sealed", &self.sealed)
            .finish_non_exhaustive()
    }
}

pub(crate) struct LiveEventJournalActivation {
    pub(crate) lease: LiveEventJournalLease,
    pub(crate) cursor: LiveEventCursor,
}

impl LiveEventJournalActivation {
    pub(crate) fn into_parts(self) -> (LiveEventJournalLease, LiveEventCursor) {
        (self.lease, self.cursor)
    }
}

impl fmt::Debug for LiveEventJournalActivation {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("LiveEventJournalActivation")
            .field("lease", &self.lease)
            .field("cursor", &self.cursor)
            .finish()
    }
}

impl fmt::Debug for LiveEventJournalRetirementToken {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("LiveEventJournalRetirementToken")
            .field("account_generation", &self.owner.account_generation)
            .field("journal_id", &self.journal_id)
            .field("head_sequence", &self.head_sequence)
            .field("operation_token", &"<redacted>")
            .field("new_operation_token", &"<redacted>")
            .field("retirement_nonce", &"<redacted>")
            .finish_non_exhaustive()
    }
}

impl fmt::Debug for LiveEventJournalRolloverObligation {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("LiveEventJournalRolloverObligation")
            .field("account_generation", &self.owner.account_generation)
            .field("journal_id", &self.journal_id)
            .field("head_sequence", &self.head_sequence)
            .field("new_journal_id", &self.new_journal_id)
            .field("operation_token", &"<redacted>")
            .field("rollover_nonce", &"<redacted>")
            .field("checkpoint_commitment", &"<redacted>")
            .finish_non_exhaustive()
    }
}

mod journal_authority {
    pub(crate) trait Sealed {}
}

/// Sealed so production callers cannot manufacture a substitute for the
/// opaque lease. The owner-only implementation exists solely to keep this
/// module's white-box tests compact; it is absent from production builds.
pub(crate) trait LiveEventJournalAuthority: journal_authority::Sealed {
    fn journal_owner(&self) -> &LiveEventAccountOwner;
    fn matches_operation_token(&self, expected: &[u8; PROCESS_TOKEN_BYTES]) -> bool;
    #[cfg(test)]
    fn allows_test_auto_claim(&self) -> bool {
        false
    }
}

impl journal_authority::Sealed for LiveEventJournalLease {}

impl LiveEventJournalAuthority for LiveEventJournalLease {
    fn journal_owner(&self) -> &LiveEventAccountOwner {
        &self.owner
    }

    fn matches_operation_token(&self, expected: &[u8; PROCESS_TOKEN_BYTES]) -> bool {
        constant_time_token_eq(&self.operation_token, expected)
    }
}

#[cfg(test)]
impl journal_authority::Sealed for LiveEventAccountOwner {}

#[cfg(test)]
impl LiveEventJournalAuthority for LiveEventAccountOwner {
    fn journal_owner(&self) -> &LiveEventAccountOwner {
        self
    }

    fn matches_operation_token(&self, _expected: &[u8; PROCESS_TOKEN_BYTES]) -> bool {
        true
    }

    fn allows_test_auto_claim(&self) -> bool {
        true
    }
}

impl LiveEventAccountOwner {
    pub(crate) fn new(
        opaque_account_scope: &str,
        account_generation: u64,
    ) -> Result<Self, LiveEventJournalError> {
        validate_nonempty_bounded(
            opaque_account_scope,
            MAX_ACCOUNT_SCOPE_BYTES,
            LiveEventJournalError::InvalidAccountOwner,
        )?;
        let digest = Sha256::digest(opaque_account_scope.as_bytes());
        Ok(Self {
            account_key: encode_hex(&digest),
            account_generation,
        })
    }

    pub(crate) const fn account_generation(&self) -> u64 {
        self.account_generation
    }
}

/// Cursor for the live replay suffix only. It must never be accepted by a
/// persisted-history pagination endpoint.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct LiveEventCursor {
    journal_id: String,
    sequence: u64,
}

impl LiveEventCursor {
    fn new(journal_id: String, sequence: u64) -> Self {
        Self {
            journal_id,
            sequence,
        }
    }

    pub(crate) fn journal_id(&self) -> &str {
        &self.journal_id
    }

    pub(crate) const fn sequence(&self) -> u64 {
        self.sequence
    }

    pub(crate) fn try_from_parts(
        journal_id: String,
        sequence: u64,
    ) -> Result<Self, LiveEventJournalError> {
        let cursor = Self::new(journal_id, sequence);
        cursor.validate()?;
        Ok(cursor)
    }

    /// Return the start cursor for this exact journal generation. This is for
    /// reconstructing retained live state after a process restart; it does not
    /// weaken the private cursor constructor or cross a history-page boundary.
    pub(crate) fn beginning(&self) -> Self {
        Self::new(self.journal_id.clone(), 0)
    }

    pub(crate) fn validate(&self) -> Result<(), LiveEventJournalError> {
        if self.sequence > MAX_CURSOR_SEQUENCE
            || self.journal_id.len() != JOURNAL_ID_HEX_BYTES
            || !self
                .journal_id
                .bytes()
                .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
        {
            return Err(LiveEventJournalError::InvalidCursor);
        }
        Ok(())
    }
}

/// One account-owned replay entry. Session and optional run identifiers are
/// stored outside the payload so routing ownership cannot be omitted by a new
/// payload variant.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct LiveReplayEntry<T> {
    cursor: LiveEventCursor,
    session_id: String,
    run_id: Option<String>,
    payload: T,
}

impl<T> LiveReplayEntry<T> {
    pub(crate) fn cursor(&self) -> &LiveEventCursor {
        &self.cursor
    }

    pub(crate) fn session_id(&self) -> &str {
        &self.session_id
    }

    pub(crate) fn run_id(&self) -> Option<&str> {
        self.run_id.as_deref()
    }

    pub(crate) fn payload(&self) -> &T {
        &self.payload
    }

    pub(crate) fn into_parts(self) -> (LiveEventCursor, String, Option<String>, T) {
        (self.cursor, self.session_id, self.run_id, self.payload)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum SnapshotRequiredReason {
    JournalReplaced,
    RetentionGap,
    CursorAhead,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct SnapshotRequired {
    pub(crate) reason: SnapshotRequiredReason,
    /// A checkpoint the caller may retain only after rebuilding from paged
    /// history. It is not permission to skip the required snapshot.
    pub(crate) current_cursor: LiveEventCursor,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum LiveReplayRead<T> {
    Events {
        entries: Vec<LiveReplayEntry<T>>,
        next_cursor: LiveEventCursor,
        has_more: bool,
    },
    SnapshotRequired(SnapshotRequired),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum EventAdmission {
    New,
    Duplicate {
        event_cursor: LiveEventCursor,
        head_cursor: LiveEventCursor,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum AppendOutcome {
    Inserted(LiveEventCursor),
    Duplicate {
        event_cursor: LiveEventCursor,
        head_cursor: LiveEventCursor,
    },
}

impl AppendOutcome {
    pub(crate) fn cursor(&self) -> &LiveEventCursor {
        match self {
            Self::Inserted(cursor) => cursor,
            Self::Duplicate { event_cursor, .. } => event_cursor,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct LiveProjectionCheckpoint {
    pub(crate) through_cursor: LiveEventCursor,
    pub(crate) bytes: Vec<u8>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum LiveEventJournalError {
    InvalidLimits,
    InvalidAccountOwner,
    InvalidEventOwner,
    InvalidCursor,
    InvalidReplayLimit,
    PayloadTooLarge,
    EventIdConflict,
    JournalReplaced,
    ReseedRequired,
    HeadChanged,
    CheckpointRequired,
    InvalidCheckpoint,
    IdempotencyCapacityExceeded,
    SequenceExhausted,
    OwnerGenerationMismatch,
    JournalRetired,
    OwnerTransitionIncomplete,
    AlreadyOpen,
    UnsupportedPlatform,
    StorageCorrupt,
    StorageUnavailable,
    LockUnavailable,
}

impl fmt::Display for LiveEventJournalError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        let message = match self {
            Self::InvalidLimits => "live event journal limits are invalid",
            Self::InvalidAccountOwner => "live event account owner is invalid",
            Self::InvalidEventOwner => "live event session or run owner is invalid",
            Self::InvalidCursor => "live event cursor is invalid",
            Self::InvalidReplayLimit => "live event replay limit is invalid",
            Self::PayloadTooLarge => "live event payload exceeds the journal bound",
            Self::EventIdConflict => "live event ID was reused with different content or ownership",
            Self::JournalReplaced => "live event journal generation was replaced",
            Self::ReseedRequired => {
                "live event journal requires an authoritative absolute projection reseed"
            }
            Self::HeadChanged => "live event journal head changed before the operation",
            Self::CheckpointRequired => {
                "live event projection checkpoint must advance before retention compaction"
            }
            Self::InvalidCheckpoint => "live event projection checkpoint is invalid",
            Self::IdempotencyCapacityExceeded => {
                "live event idempotency capacity requires a FIFO-sealed journal rollover"
            }
            Self::SequenceExhausted => "live event sequence is exhausted",
            Self::OwnerGenerationMismatch => {
                "live event account generation no longer owns this journal"
            }
            Self::JournalRetired => "live event journal lease was retired or replaced",
            Self::OwnerTransitionIncomplete => {
                "live event account journal rotation requires an authorized clear"
            }
            Self::AlreadyOpen => "live event journal is already open by another host",
            Self::UnsupportedPlatform => {
                "durable live event journals are unsupported on this platform"
            }
            Self::StorageCorrupt => "live event journal storage is corrupt",
            Self::StorageUnavailable => "live event journal storage is unavailable",
            Self::LockUnavailable => "live event journal lock is unavailable",
        };
        formatter.write_str(message)
    }
}

impl std::error::Error for LiveEventJournalError {}

#[derive(Clone)]
pub(crate) struct LiveEventJournal<T: LiveReplayPayload> {
    inner: Arc<LiveEventJournalInner<T>>,
}

struct LiveEventJournalInner<T: LiveReplayPayload> {
    root: PathBuf,
    limits: LiveEventJournalLimits,
    root_guard: JournalRootGuard,
    /// Ownership transitions and journal mutation share one synchronization
    /// boundary. This prevents an old append from passing its generation check
    /// immediately before clear/rotation advances the owner.
    state: Mutex<JournalState<T>>,
    /// Test-only crash boundary immediately after an appended record reached
    /// durable storage but before the caller received an acknowledgement.
    #[cfg(test)]
    fail_next_append_after_sync: AtomicBool,
    /// Test-only ambiguous atomic-replacement boundary after rename/persist but
    /// before the root-directory durability acknowledgement.
    #[cfg(test)]
    fail_next_replace_at: AtomicU8,
    /// Test-only durable-retirement crash boundary.
    #[cfg(test)]
    fail_next_retirement_at: AtomicU8,
}

#[cfg(test)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
enum ReplaceFailureBoundary {
    None = 0,
    BeforeFileSync = 1,
    AfterFileSync = 2,
    AfterPersist = 3,
    AfterDirectorySync = 4,
}

#[cfg(test)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
enum RetirementFailureBoundary {
    None = 0,
    BeforeRename = 1,
    AfterRename = 2,
    AfterRenameDirectorySync = 3,
    AfterUnlink = 4,
    AfterFinalDirectorySync = 5,
}

struct JournalState<T> {
    /// This small process-lifetime authority map is deliberately independent
    /// from the evictable decoded payload cache. An ambiguous I/O failure must
    /// not let a stale handle claim the account again.
    owners: HashMap<String, JournalOwnerState>,
    accounts: HashMap<String, AccountJournal<T>>,
}

/// Exact append identity retained only while one storage result is ambiguous.
/// It prevents an unrelated event from using the producer capability to clear
/// the recovery fence before the original operation has been classified.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct AmbiguousAppend {
    journal_id: [u8; JOURNAL_ID_BYTES],
    expected_sequence: u64,
    event_id_commitment: [u8; 32],
    event_commitment: [u8; 32],
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum JournalOwnerState {
    ReseedRequired {
        generation: u64,
        observation_token: [u8; PROCESS_TOKEN_BYTES],
    },
    Active {
        generation: u64,
        operation_token: [u8; PROCESS_TOKEN_BYTES],
        /// A prior operation may have reached disk but failed before proving
        /// durability. The recovered file must be rewritten and synced before
        /// any cursor can be returned or replayed.
        needs_resync: bool,
        ambiguous_append: Option<AmbiguousAppend>,
    },
    /// Generation authority advanced, but journal rotation did not prove a
    /// durable empty replacement. Normal operations fail until an authorized
    /// current-generation clear or rotation retry resolves it.
    TransitionIncomplete {
        generation: u64,
        operation_token: [u8; PROCESS_TOKEN_BYTES],
    },
    /// The coordinator has sealed publication at the captured head. Ordinary
    /// journal operations remain fenced until this exact obligation commits or
    /// the process restarts and observes the atomically old-or-new file.
    RolloverPending {
        generation: u64,
        operation_token: [u8; PROCESS_TOKEN_BYTES],
        new_operation_token: [u8; PROCESS_TOKEN_BYTES],
        rollover_nonce: [u8; PROCESS_TOKEN_BYTES],
        journal_id: [u8; JOURNAL_ID_BYTES],
        head_sequence: u64,
        checkpoint_commitment: [u8; 32],
        new_journal_id: [u8; JOURNAL_ID_BYTES],
    },
    /// FIFO publication is sealed and every ordinary operation is fenced.
    /// `rename_committed` becomes true only after the pending-retirement name
    /// has been synced into the journal root.
    Retiring {
        generation: u64,
        operation_token: [u8; PROCESS_TOKEN_BYTES],
        retirement_nonce: [u8; PROCESS_TOKEN_BYTES],
        journal_id: [u8; JOURNAL_ID_BYTES],
        head_sequence: u64,
        rename_committed: bool,
    },
    Reseeding {
        generation: u64,
        observation_token: [u8; PROCESS_TOKEN_BYTES],
        authority_nonce: [u8; 32],
        durable_head_commitment: [u8; 32],
        projection_digest: [u8; 32],
        new_journal_id: [u8; JOURNAL_ID_BYTES],
        sealed: bool,
    },
}

struct JournalRootLock {
    file: File,
    identity: FileIdentity,
}

struct JournalRootGuard {
    directory: File,
    identity: FileIdentity,
    lock: JournalRootLock,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct FileIdentity {
    device: u64,
    inode: u64,
}

impl Drop for JournalRootLock {
    fn drop(&mut self) {
        let _ = FileExt::unlock(&self.file);
    }
}

impl JournalRootGuard {
    fn verify(&self, path: &Path) -> Result<(), LiveEventJournalError> {
        let path_metadata =
            fs::symlink_metadata(path).map_err(|_| LiveEventJournalError::StorageUnavailable)?;
        let descriptor_metadata = self
            .directory
            .metadata()
            .map_err(|_| LiveEventJournalError::StorageUnavailable)?;
        if !path_metadata.file_type().is_dir()
            || path_metadata.file_type().is_symlink()
            || file_identity(&path_metadata) != self.identity
            || file_identity(&descriptor_metadata) != self.identity
        {
            return Err(LiveEventJournalError::StorageUnavailable);
        }
        self.lock.verify(path)
    }

    fn sync(&self) -> Result<(), LiveEventJournalError> {
        self.directory
            .sync_all()
            .map_err(|_| LiveEventJournalError::StorageUnavailable)
    }

    fn scavenge_owned_temporary_files(
        &self,
        path: &Path,
        max_disk_bytes: u64,
    ) -> Result<(), LiveEventJournalError> {
        self.verify(path)?;
        let mut account_files = 0usize;
        let mut account_keys = HashSet::new();
        let mut pending_keys = HashSet::new();
        let mut pending_files = Vec::new();
        let mut removed_file = false;
        for entry in fs::read_dir(path).map_err(|_| LiveEventJournalError::StorageUnavailable)? {
            let entry = entry.map_err(|_| LiveEventJournalError::StorageUnavailable)?;
            let file_name = entry.file_name();
            let file_name = file_name
                .to_str()
                .ok_or(LiveEventJournalError::StorageCorrupt)?;
            if file_name == "host.lock" {
                continue;
            }
            let metadata = entry
                .path()
                .symlink_metadata()
                .map_err(|_| LiveEventJournalError::StorageUnavailable)?;
            if file_name.starts_with(TEMP_FILE_PREFIX) {
                if !metadata.file_type().is_file() || metadata.file_type().is_symlink() {
                    return Err(LiveEventJournalError::StorageCorrupt);
                }
                fs::remove_file(entry.path())
                    .map_err(|_| LiveEventJournalError::StorageUnavailable)?;
                removed_file = true;
                continue;
            }
            if is_account_journal_file_name(file_name) && metadata.file_type().is_file() {
                let account_file = open_read_no_follow(&entry.path())?;
                set_owner_only_file(&account_file)?;
                let account_key = account_key_from_journal_file_name(file_name)
                    .ok_or(LiveEventJournalError::StorageCorrupt)?;
                if !account_keys.insert(account_key.to_string()) {
                    return Err(LiveEventJournalError::StorageCorrupt);
                }
                account_files = account_files
                    .checked_add(1)
                    .ok_or(LiveEventJournalError::StorageCorrupt)?;
                continue;
            }
            if let Some((account_key, _nonce)) = parse_retiring_file_name(file_name) {
                if !metadata.file_type().is_file()
                    || metadata.file_type().is_symlink()
                    || !metadata_owned_by_effective_user(&metadata)
                    || !pending_keys.insert(account_key.clone())
                {
                    return Err(LiveEventJournalError::StorageCorrupt);
                }
                let pending_file = open_read_no_follow(&entry.path())?;
                set_owner_only_file(&pending_file)?;
                let identity = read_v3_disk_identity(&entry.path(), max_disk_bytes)?;
                if identity.account_key != account_key || identity.committed_end != metadata.len() {
                    return Err(LiveEventJournalError::StorageCorrupt);
                }
                pending_files.push(entry.path());
                continue;
            }
            return Err(LiveEventJournalError::StorageCorrupt);
        }
        if account_files
            .checked_add(pending_files.len())
            .is_none_or(|count| count > MAX_ACCOUNT_JOURNAL_FILES)
            || pending_keys
                .iter()
                .any(|account_key| account_keys.contains(account_key))
        {
            return Err(LiveEventJournalError::StorageCorrupt);
        }
        // A visible, validated pending name after restart is the durable
        // retirement commit. Finish only those bounded names; absence remains
        // non-authority and never causes an account journal to be recreated.
        for pending in pending_files {
            fs::remove_file(pending).map_err(|_| LiveEventJournalError::StorageUnavailable)?;
            removed_file = true;
        }
        if removed_file {
            self.sync()?;
        }
        self.verify(path)
    }
}

impl JournalRootLock {
    fn verify(&self, root: &Path) -> Result<(), LiveEventJournalError> {
        let path_metadata = fs::symlink_metadata(root.join("host.lock"))
            .map_err(|_| LiveEventJournalError::StorageUnavailable)?;
        let descriptor_metadata = self
            .file
            .metadata()
            .map_err(|_| LiveEventJournalError::StorageUnavailable)?;
        if !path_metadata.file_type().is_file()
            || path_metadata.file_type().is_symlink()
            || file_identity(&path_metadata) != self.identity
            || file_identity(&descriptor_metadata) != self.identity
        {
            return Err(LiveEventJournalError::StorageUnavailable);
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct JournalHeader {
    version: u8,
    journal_id: String,
    account_key: String,
    head_sequence: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    checkpoint: Option<StoredCheckpoint>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    event_ids: Vec<StoredEventId>,
    integrity: String,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct JournalHeaderIntegrity<'a> {
    version: u8,
    journal_id: &'a str,
    account_key: &'a str,
    head_sequence: u64,
    checkpoint: &'a Option<StoredCheckpoint>,
    event_ids: &'a [StoredEventId],
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct StoredCheckpoint {
    schema: String,
    through_sequence: u64,
    #[serde(with = "base64_bytes")]
    bytes: Vec<u8>,
    commitment: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct StoredEventId {
    event_id: String,
    sequence: u64,
    commitment: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(
    rename_all = "camelCase",
    deny_unknown_fields,
    bound(serialize = "T: Serialize", deserialize = "T: DeserializeOwned")
)]
struct StoredEntry<T> {
    sequence: u64,
    session_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    run_id: Option<String>,
    payload: T,
    commitment: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct DiskAnchor {
    slot_index: u8,
    revision: u64,
    file_nonce: [u8; 16],
    journal_id: String,
    account_key: String,
    snapshot_offset: u64,
    snapshot_len: u64,
    data_start: u64,
    committed_end: u64,
    snapshot_head_sequence: u64,
    committed_head_sequence: u64,
    committed_frame_count: u64,
    snapshot_hash: [u8; 32],
    committed_chain_hash: [u8; 32],
}

#[derive(Clone)]
struct AccountJournal<T> {
    journal_id: String,
    account_generation: u64,
    head_sequence: u64,
    entries: VecDeque<StoredEntry<T>>,
    total_payload_bytes: usize,
    checkpoint: Option<StoredCheckpoint>,
    event_ids: HashMap<String, StoredEventId>,
    event_id_metadata_bytes: usize,
    /// Exact durable head of the v3 account file. New accounts receive this
    /// state only after their first atomic replacement commits.
    disk_anchor: Option<DiskAnchor>,
}

impl<T: LiveReplayPayload> LiveEventJournal<T> {
    pub(crate) fn open(
        root: PathBuf,
        limits: LiveEventJournalLimits,
    ) -> Result<Self, LiveEventJournalError> {
        ensure_supported_platform()?;
        let limits = limits.validate()?;
        let root = canonical_journal_root_path(&root)?;
        ensure_private_directory(&root)?;
        let root_guard = open_and_lock_journal_root(&root)?;
        root_guard.scavenge_owned_temporary_files(
            &root,
            limits
                .max_disk_bytes()
                .ok_or(LiveEventJournalError::InvalidLimits)?,
        )?;
        Ok(Self {
            inner: Arc::new(LiveEventJournalInner {
                root,
                limits,
                root_guard,
                state: Mutex::new(JournalState {
                    owners: HashMap::new(),
                    accounts: HashMap::new(),
                }),
                #[cfg(test)]
                fail_next_append_after_sync: AtomicBool::new(false),
                #[cfg(test)]
                fail_next_replace_at: AtomicU8::new(ReplaceFailureBoundary::None as u8),
                #[cfg(test)]
                fail_next_retirement_at: AtomicU8::new(RetirementFailureBoundary::None as u8),
            }),
        })
    }

    pub(crate) fn max_replay_entries(&self) -> usize {
        self.inner.limits.max_replay_entries
    }

    pub(crate) const fn max_checkpoint_bytes(&self) -> usize {
        MAX_CHECKPOINT_BYTES
    }

    /// Activate one exact owner under the host's verified binding lifecycle
    /// lock. This is the only production path that may load or create an
    /// account file and mint its process-local operation capability.
    pub(crate) fn activate_account(
        &self,
        owner: &LiveEventAccountOwner,
    ) -> Result<LiveEventJournalLease, LiveEventJournalActivationError> {
        let mut state = self.lock_state()?;
        self.verify_storage_root()?;
        self.ensure_owner_capacity(&state, owner)?;
        self.ensure_account_file_capacity_for(owner)?;
        let operation_token = match state.owners.get(&owner.account_key).copied() {
            Some(JournalOwnerState::Active {
                generation,
                operation_token,
                ..
            }) if generation == owner.account_generation => operation_token,
            Some(JournalOwnerState::TransitionIncomplete { generation, .. })
                if generation == owner.account_generation =>
            {
                return Err(LiveEventJournalError::OwnerTransitionIncomplete.into());
            }
            Some(JournalOwnerState::RolloverPending { generation, .. })
                if generation == owner.account_generation =>
            {
                return Err(LiveEventJournalError::OwnerTransitionIncomplete.into());
            }
            Some(JournalOwnerState::Retiring { generation, .. })
                if generation == owner.account_generation =>
            {
                return Err(LiveEventJournalError::JournalRetired.into());
            }
            Some(JournalOwnerState::ReseedRequired {
                generation,
                observation_token,
            }) if generation == owner.account_generation => {
                let observed = self.observe_journal_generation(owner)?;
                return Err(LiveEventJournalActivationError::ReseedRequired(Box::new(
                    LiveEventJournalReseedRequired {
                        owner: owner.clone(),
                        observed,
                        observation_token,
                    },
                )));
            }
            Some(JournalOwnerState::Reseeding { generation, .. })
                if generation == owner.account_generation =>
            {
                return Err(LiveEventJournalError::OwnerTransitionIncomplete.into());
            }
            Some(_) => return Err(LiveEventJournalError::OwnerGenerationMismatch.into()),
            None => {
                let operation_token = new_process_token()?;
                state.owners.insert(
                    owner.account_key.clone(),
                    JournalOwnerState::Active {
                        generation: owner.account_generation,
                        operation_token,
                        needs_resync: false,
                        ambiguous_append: None,
                    },
                );
                operation_token
            }
        };
        let lease = LiveEventJournalLease {
            owner: owner.clone(),
            operation_token,
        };
        match self.prepare_account(&mut state, &lease) {
            Ok(_) => Ok(lease),
            Err(LiveEventJournalError::StorageCorrupt) => {
                state.accounts.remove(&owner.account_key);
                let observation_token = new_process_token()?;
                state.owners.insert(
                    owner.account_key.clone(),
                    JournalOwnerState::ReseedRequired {
                        generation: owner.account_generation,
                        observation_token,
                    },
                );
                let observed = self.observe_journal_generation(owner)?;
                Err(LiveEventJournalActivationError::ReseedRequired(Box::new(
                    LiveEventJournalReseedRequired {
                        owner: owner.clone(),
                        observed,
                        observation_token,
                    },
                )))
            }
            Err(error) => Err(error.into()),
        }
    }

    /// Return an account checkpoint to capture before loading paged history.
    /// Events emitted during that load can then be replayed from this cursor.
    pub(crate) fn checkpoint<A: LiveEventJournalAuthority>(
        &self,
        authority: &A,
    ) -> Result<LiveEventCursor, LiveEventJournalError> {
        let mut state = self.lock_state()?;
        self.verify_storage_root()?;
        let account = self.prepare_account(&mut state, authority)?;
        Ok(current_cursor(account))
    }

    /// Mint a producer capability for the activation lease's exact current
    /// journal generation. Coordinators call this during explicit producer/run
    /// admission and carry the returned lease with every queued event; publish
    /// paths never auto-refresh it.
    pub(crate) fn bind_ingress(
        &self,
        lease: &LiveEventJournalLease,
    ) -> Result<LiveEventJournalIngressLease, LiveEventJournalError> {
        let mut state = self.lock_state()?;
        self.verify_storage_root()?;
        let account = self.prepare_account(&mut state, lease)?;
        Ok(LiveEventJournalIngressLease {
            owner: lease.owner.clone(),
            operation_token: lease.operation_token,
            journal_id: decode_hex_array::<JOURNAL_ID_BYTES>(&account.journal_id)?,
        })
    }

    pub(crate) fn classify_event(
        &self,
        ingress: &LiveEventJournalIngressLease,
        expected_head: &LiveEventCursor,
        session_id: &str,
        run_id: Option<&str>,
        payload: &T,
    ) -> Result<EventAdmission, LiveEventJournalError> {
        expected_head.validate()?;
        validate_event_for_append(session_id, run_id, payload, self.inner.limits)?;
        let commitment = event_commitment(session_id, run_id, payload)?;
        let mut state = self.lock_state()?;
        self.verify_storage_root()?;
        let account = self.prepare_ingress_account(
            &mut state,
            ingress,
            expected_head,
            payload.live_replay_event_id(),
            &commitment,
        )?;
        ensure_expected_journal(account, expected_head)?;
        let admission =
            classify_account_event(account, payload.live_replay_event_id(), &commitment)?;
        if matches!(admission, EventAdmission::New) {
            ensure_expected_sequence(account, expected_head)?;
        }
        Ok(admission)
    }

    /// Test convenience wrapper. Production callers must carry their actor's
    /// exact expected head through [`Self::append_outcome`].
    #[cfg(test)]
    pub(crate) fn append(
        &self,
        owner: &LiveEventAccountOwner,
        session_id: &str,
        run_id: Option<&str>,
        payload: T,
    ) -> Result<LiveEventCursor, LiveEventJournalError> {
        let lease = self.activate_account(owner).map_err(|error| match error {
            LiveEventJournalActivationError::Journal(error) => error,
            LiveEventJournalActivationError::ReseedRequired(_) => {
                LiveEventJournalError::ReseedRequired
            }
        })?;
        let ingress = self.bind_ingress(&lease)?;
        loop {
            let expected_head = self.checkpoint(&lease)?;
            match self.append_outcome(
                &ingress,
                &expected_head,
                session_id,
                run_id,
                payload.clone(),
            ) {
                Err(LiveEventJournalError::HeadChanged) => continue,
                result => return result.map(|outcome| outcome.cursor().clone()),
            }
        }
    }

    /// Persist one already-sanitized event before publishing it remotely.
    /// The outcome explicitly separates a new durable insert from an exact
    /// retry that was already committed, even after its payload was compacted.
    pub(crate) fn append_outcome(
        &self,
        ingress: &LiveEventJournalIngressLease,
        expected_head: &LiveEventCursor,
        session_id: &str,
        run_id: Option<&str>,
        payload: T,
    ) -> Result<AppendOutcome, LiveEventJournalError> {
        expected_head.validate()?;
        let payload_bytes =
            validate_event_for_append(session_id, run_id, &payload, self.inner.limits)?;
        let commitment = event_commitment(session_id, run_id, &payload)?;
        let owner = &ingress.owner;

        let mut state = self.lock_state()?;
        self.verify_storage_root()?;
        let account = self.prepare_ingress_account(
            &mut state,
            ingress,
            expected_head,
            payload.live_replay_event_id(),
            &commitment,
        )?;
        let ambiguous_append = ambiguous_append_identity(
            ingress,
            expected_head,
            payload.live_replay_event_id(),
            &commitment,
        )?;
        ensure_expected_journal(account, expected_head)?;
        if let EventAdmission::Duplicate {
            event_cursor,
            head_cursor,
        } = classify_account_event(account, payload.live_replay_event_id(), &commitment)?
        {
            return Ok(AppendOutcome::Duplicate {
                event_cursor,
                head_cursor,
            });
        }
        ensure_expected_sequence(account, expected_head)?;
        if account.event_ids.len() >= MAX_IDEMPOTENCY_EVENT_IDS {
            return Err(LiveEventJournalError::IdempotencyCapacityExceeded);
        }
        let sequence = account
            .head_sequence
            .checked_add(1)
            .ok_or(LiveEventJournalError::SequenceExhausted)?;
        if sequence > MAX_CURSOR_SEQUENCE {
            return Err(LiveEventJournalError::SequenceExhausted);
        }
        let entry = StoredEntry {
            sequence,
            session_id: session_id.to_string(),
            run_id: run_id.map(str::to_string),
            payload,
            commitment: commitment.clone(),
        };
        let event_id_record = StoredEventId {
            event_id: entry.payload.live_replay_event_id().to_string(),
            sequence,
            commitment,
        };
        let event_id_metadata_bytes = encoded_event_id_bytes(&event_id_record)?;
        if account
            .event_id_metadata_bytes
            .checked_add(event_id_metadata_bytes)
            .is_none_or(|total| total > MAX_IDEMPOTENCY_METADATA_BYTES)
        {
            return Err(LiveEventJournalError::IdempotencyCapacityExceeded);
        }

        let needs_compaction = account.entries.len() >= self.inner.limits.max_entries
            || account
                .total_payload_bytes
                .checked_add(payload_bytes)
                .is_none_or(|total| total > self.inner.limits.max_total_payload_bytes);

        if needs_compaction {
            let mut retained = account.entries.clone();
            let mut retained_payload_bytes = account.total_payload_bytes;
            retained.push_back(entry.clone());
            retained_payload_bytes = retained_payload_bytes
                .checked_add(payload_bytes)
                .ok_or(LiveEventJournalError::PayloadTooLarge)?;
            let evict_through = account
                .checkpoint
                .as_ref()
                .map_or(0, |checkpoint| checkpoint.through_sequence);
            trim_compaction_low_watermark(
                &mut retained,
                &mut retained_payload_bytes,
                self.inner.limits,
                evict_through,
            )?;
            trim_retention(
                &mut retained,
                &mut retained_payload_bytes,
                self.inner.limits,
                evict_through,
            )?;
            if retained.len() > self.inner.limits.max_entries
                || retained_payload_bytes > self.inner.limits.max_total_payload_bytes
            {
                return Err(LiveEventJournalError::CheckpointRequired);
            }
            let mut replacement = account.clone();
            replacement.entries = retained;
            replacement.total_payload_bytes = retained_payload_bytes;
            replacement.head_sequence = sequence;
            replacement
                .event_ids
                .insert(event_id_record.event_id.clone(), event_id_record);
            replacement.event_id_metadata_bytes += event_id_metadata_bytes;
            if let Err(error) = self.replace_account_file(owner, &mut replacement) {
                // The atomic replacement may have reached disk before a final
                // directory sync failed. Force the next operation to reload
                // instead of appending from a possibly stale cached sequence.
                mark_ingress_owner_indeterminate(&mut state.owners, ingress, ambiguous_append)?;
                state.accounts.remove(&owner.account_key);
                return Err(error);
            }
            *account = replacement;
        } else {
            if let Err(error) = self.append_record(owner, account, &entry) {
                // A failed sync can leave an unanchored frame tail, a newly
                // committed anchor, or a torn nonzero anchor slot. Reloading
                // accepts only exact anchored EOF; every ambiguous extra byte
                // and every torn nonzero slot fails closed.
                mark_ingress_owner_indeterminate(&mut state.owners, ingress, ambiguous_append)?;
                state.accounts.remove(&owner.account_key);
                return Err(error);
            }
            account.entries.push_back(entry);
            account.total_payload_bytes += payload_bytes;
            account.head_sequence = sequence;
            account
                .event_ids
                .insert(event_id_record.event_id.clone(), event_id_record);
            account.event_id_metadata_bytes += event_id_metadata_bytes;
        }

        Ok(AppendOutcome::Inserted(LiveEventCursor::new(
            account.journal_id.clone(),
            sequence,
        )))
    }

    /// Replay the account-wide event suffix after `cursor`.
    ///
    /// Entries retain exact session/run ownership so the caller can route all
    /// background task updates without weakening account isolation. Pagination
    /// here is over the short live suffix and is unrelated to history pages.
    pub(crate) fn replay_after<A: LiveEventJournalAuthority>(
        &self,
        authority: &A,
        cursor: &LiveEventCursor,
        limit: usize,
    ) -> Result<LiveReplayRead<T>, LiveEventJournalError> {
        cursor.validate()?;
        if limit == 0 || limit > self.inner.limits.max_replay_entries {
            return Err(LiveEventJournalError::InvalidReplayLimit);
        }

        let mut state = self.lock_state()?;
        self.verify_storage_root()?;
        let account = self.prepare_account(&mut state, authority)?;
        let current = current_cursor(account);
        if cursor.journal_id != account.journal_id {
            return Ok(LiveReplayRead::SnapshotRequired(SnapshotRequired {
                reason: SnapshotRequiredReason::JournalReplaced,
                current_cursor: current,
            }));
        }
        if cursor.sequence > current.sequence {
            return Ok(LiveReplayRead::SnapshotRequired(SnapshotRequired {
                reason: SnapshotRequiredReason::CursorAhead,
                current_cursor: current,
            }));
        }
        if account.entries.front().is_some_and(|first| {
            cursor
                .sequence
                .checked_add(1)
                .is_none_or(|expected| expected < first.sequence)
        }) || (cursor.sequence < current.sequence
            && account
                .entries
                .front()
                .is_none_or(|first| first.sequence > cursor.sequence.saturating_add(1)))
        {
            return Ok(LiveReplayRead::SnapshotRequired(SnapshotRequired {
                reason: SnapshotRequiredReason::RetentionGap,
                current_cursor: current,
            }));
        }

        let mut entries = Vec::new();
        let mut replay_payload_bytes = 0usize;
        let mut has_more = false;
        for entry in account
            .entries
            .iter()
            .filter(|entry| entry.sequence > cursor.sequence)
        {
            let payload_bytes = serialized_payload_bytes(&entry.payload)?;
            entry.payload.validate_live_replay_payload()?;
            validate_event_id(entry.payload.live_replay_event_id())?;
            let exceeds_count = entries.len() == limit;
            let exceeds_bytes = replay_payload_bytes
                .checked_add(payload_bytes)
                .is_none_or(|total| total > self.inner.limits.max_replay_payload_bytes);
            if exceeds_count || exceeds_bytes {
                has_more = true;
                break;
            }
            replay_payload_bytes += payload_bytes;
            entries.push(LiveReplayEntry {
                cursor: LiveEventCursor::new(account.journal_id.clone(), entry.sequence),
                session_id: entry.session_id.clone(),
                run_id: entry.run_id.clone(),
                payload: entry.payload.clone(),
            });
        }
        let next_cursor = entries
            .last()
            .map(|entry| entry.cursor.clone())
            .unwrap_or_else(|| cursor.clone());
        Ok(LiveReplayRead::Events {
            entries,
            next_cursor,
            has_more,
        })
    }

    pub(crate) fn load_checkpoint<A: LiveEventJournalAuthority>(
        &self,
        authority: &A,
    ) -> Result<Option<LiveProjectionCheckpoint>, LiveEventJournalError> {
        let mut state = self.lock_state()?;
        self.verify_storage_root()?;
        let account = self.prepare_account(&mut state, authority)?;
        Ok(account
            .checkpoint
            .as_ref()
            .map(|checkpoint| LiveProjectionCheckpoint {
                through_cursor: LiveEventCursor::new(
                    account.journal_id.clone(),
                    checkpoint.through_sequence,
                ),
                bytes: checkpoint.bytes.clone(),
            }))
    }

    /// CAS-install an absolute projection at the exact current durable head.
    /// The coordinator prepares and validates the safe DTO before this call.
    pub(crate) fn store_checkpoint<A: LiveEventJournalAuthority>(
        &self,
        authority: &A,
        expected_head: &LiveEventCursor,
        bytes: &[u8],
    ) -> Result<LiveEventCursor, LiveEventJournalError> {
        expected_head.validate()?;
        if bytes.is_empty() || bytes.len() > MAX_CHECKPOINT_BYTES {
            return Err(LiveEventJournalError::InvalidCheckpoint);
        }
        let owner = authority.journal_owner();
        let mut state = self.lock_state()?;
        self.verify_storage_root()?;
        let account = self.prepare_account(&mut state, authority)?;
        let current = current_cursor(account);
        ensure_expected_head(account, expected_head)?;
        let mut replacement = account.clone();
        replacement.checkpoint = Some(StoredCheckpoint {
            schema: CHECKPOINT_SCHEMA.to_string(),
            through_sequence: current.sequence,
            bytes: bytes.to_vec(),
            commitment: bytes_commitment(bytes),
        });
        // Keep the useful recent replay suffix for phone reconnects. Future
        // retention compaction may evict only entries now covered by this
        // checkpoint; checkpointing itself never creates a client gap.
        if let Err(error) = self.replace_account_file(owner, &mut replacement) {
            mark_owner_indeterminate(&mut state.owners, authority)?;
            state.accounts.remove(&owner.account_key);
            return Err(error);
        }
        *account = replacement;
        Ok(current)
    }

    /// Fence ordinary operations and prepare one exact generation rollover.
    ///
    /// The coordinator may call this only after sealing its FIFO and pausing
    /// subscribers. Requiring the concrete lease, the exact current head, and
    /// the already-stored absolute checkpoint means an arbitrary owner or
    /// projection cannot manufacture a destructive rollover. The returned
    /// move-only obligation preselects the replacement journal ID for exact
    /// retry after an ambiguous atomic-replace acknowledgement.
    pub(crate) fn prepare_rollover(
        &self,
        lease: &LiveEventJournalLease,
        expected_head: &LiveEventCursor,
        bytes: &[u8],
    ) -> Result<LiveEventJournalRolloverObligation, LiveEventJournalError> {
        expected_head.validate()?;
        if bytes.is_empty() || bytes.len() > MAX_CHECKPOINT_BYTES {
            return Err(LiveEventJournalError::InvalidCheckpoint);
        }
        let mut state = self.lock_state()?;
        self.verify_storage_root()?;
        let account = self.prepare_account(&mut state, lease)?;
        ensure_expected_head(account, expected_head)?;
        let checkpoint_commitment: [u8; 32] = Sha256::digest(bytes).into();
        let Some(previous_checkpoint) = account.checkpoint.as_ref() else {
            return Err(LiveEventJournalError::InvalidCheckpoint);
        };
        if previous_checkpoint.schema != CHECKPOINT_SCHEMA
            || previous_checkpoint.through_sequence != expected_head.sequence
            || previous_checkpoint.commitment != encode_hex(&checkpoint_commitment)
            || previous_checkpoint.bytes != bytes
        {
            return Err(LiveEventJournalError::InvalidCheckpoint);
        }
        let rollover_nonce = new_process_token()?;
        let new_operation_token = new_process_token()?;
        if constant_time_token_eq(&new_operation_token, &lease.operation_token) {
            return Err(LiveEventJournalError::StorageUnavailable);
        }
        let new_journal_id = new_journal_id()?;
        if new_journal_id == account.journal_id {
            // A random collision is extraordinarily unlikely, but accepting
            // it would silently defeat the generation fence.
            return Err(LiveEventJournalError::StorageUnavailable);
        }
        let journal_id = account.journal_id.clone();
        let journal_id_bytes = decode_hex_array::<JOURNAL_ID_BYTES>(&journal_id)?;
        let new_journal_id_bytes = decode_hex_array::<JOURNAL_ID_BYTES>(&new_journal_id)?;
        let head_sequence = account.head_sequence;
        state.owners.insert(
            lease.owner.account_key.clone(),
            JournalOwnerState::RolloverPending {
                generation: lease.owner.account_generation,
                operation_token: lease.operation_token,
                new_operation_token,
                rollover_nonce,
                journal_id: journal_id_bytes,
                head_sequence,
                checkpoint_commitment,
                new_journal_id: new_journal_id_bytes,
            },
        );
        state.accounts.remove(&lease.owner.account_key);
        Ok(LiveEventJournalRolloverObligation {
            owner: lease.owner.clone(),
            operation_token: lease.operation_token,
            new_operation_token,
            rollover_nonce,
            journal_id,
            head_sequence,
            checkpoint_commitment,
            new_journal_id,
        })
    }

    /// Atomically start a fresh journal generation from one FIFO-sealed
    /// obligation. `bytes` is the exact absolute projection, never a delta.
    ///
    /// The replacement stores that projection at sequence zero and clears the
    /// prior generation's replay suffix and durable event-ID commitments. An
    /// append actor carrying its pre-rollover journal ID is therefore fenced
    /// with `JournalReplaced`. The obligation is borrowed so an ambiguous
    /// storage result can be retried. An exact replay after the internal
    /// `RolloverPending -> Active` transition returns the same preselected
    /// activation capability without performing another disk replacement.
    pub(crate) fn commit_rollover(
        &self,
        obligation: &LiveEventJournalRolloverObligation,
        bytes: &[u8],
    ) -> Result<LiveEventJournalActivation, LiveEventJournalError> {
        let supplied_commitment: [u8; 32] = Sha256::digest(bytes).into();
        if bytes.is_empty()
            || bytes.len() > MAX_CHECKPOINT_BYTES
            || !constant_time_digest_eq(&supplied_commitment, &obligation.checkpoint_commitment)
        {
            return Err(LiveEventJournalError::InvalidCheckpoint);
        }
        let expected_journal_id = decode_hex_array::<JOURNAL_ID_BYTES>(&obligation.journal_id)?;
        let expected_new_journal_id =
            decode_hex_array::<JOURNAL_ID_BYTES>(&obligation.new_journal_id)?;
        let mut state = self.lock_state()?;
        self.verify_storage_root()?;
        let already_committed = match state.owners.get(&obligation.owner.account_key).copied() {
            Some(JournalOwnerState::RolloverPending {
                generation,
                operation_token,
                new_operation_token,
                rollover_nonce,
                journal_id,
                head_sequence,
                checkpoint_commitment,
                new_journal_id,
            }) if generation == obligation.owner.account_generation
                && constant_time_token_eq(&operation_token, &obligation.operation_token)
                && constant_time_token_eq(
                    &new_operation_token,
                    &obligation.new_operation_token,
                )
                && constant_time_token_eq(&rollover_nonce, &obligation.rollover_nonce)
                && journal_id == expected_journal_id
                && head_sequence == obligation.head_sequence
                && constant_time_digest_eq(
                    &checkpoint_commitment,
                    &obligation.checkpoint_commitment,
                )
                && new_journal_id == expected_new_journal_id =>
            {
                false
            }
            Some(JournalOwnerState::Active {
                generation,
                operation_token,
                ..
            }) if generation == obligation.owner.account_generation
                && constant_time_token_eq(&operation_token, &obligation.new_operation_token) =>
            {
                true
            }
            Some(JournalOwnerState::RolloverPending { generation, .. })
            | Some(JournalOwnerState::Active { generation, .. })
            | Some(JournalOwnerState::TransitionIncomplete { generation, .. })
            | Some(JournalOwnerState::Retiring { generation, .. })
            | Some(JournalOwnerState::ReseedRequired { generation, .. })
            | Some(JournalOwnerState::Reseeding { generation, .. })
                if generation == obligation.owner.account_generation =>
            {
                return Err(LiveEventJournalError::JournalReplaced);
            }
            Some(_) => return Err(LiveEventJournalError::OwnerGenerationMismatch),
            None => return Err(LiveEventJournalError::JournalReplaced),
        };

        let current = self.load_account(&obligation.owner)?;
        let replacement =
            if !already_committed && is_exact_rollover_source(&current, obligation, bytes) {
                let checkpoint = StoredCheckpoint {
                    schema: CHECKPOINT_SCHEMA.to_string(),
                    through_sequence: 0,
                    bytes: bytes.to_vec(),
                    commitment: encode_hex(&obligation.checkpoint_commitment),
                };
                let mut replacement = AccountJournal {
                    journal_id: obligation.new_journal_id.clone(),
                    account_generation: obligation.owner.account_generation,
                    head_sequence: 0,
                    entries: VecDeque::new(),
                    total_payload_bytes: 0,
                    checkpoint: Some(checkpoint),
                    event_ids: HashMap::new(),
                    event_id_metadata_bytes: 0,
                    disk_anchor: None,
                };
                if let Err(error) = self.replace_account_file(&obligation.owner, &mut replacement) {
                    // The same obligation can distinguish the exact old file from
                    // its preselected exact replacement on a subsequent retry.
                    state.accounts.remove(&obligation.owner.account_key);
                    return Err(error);
                }
                replacement
            } else if is_exact_rollover_replacement(&current, obligation, bytes) {
                // A prior rename may be visible even though its directory-sync
                // acknowledgement was lost. Re-establish that durability barrier
                // before treating the preselected replacement as committed. An
                // already-Active exact replay performs no further disk write.
                if !already_committed {
                    self.inner.root_guard.sync()?;
                    self.verify_storage_root()?;
                }
                current
            } else {
                return Err(LiveEventJournalError::JournalReplaced);
            };
        let operation_token = obligation.new_operation_token;
        let lease = LiveEventJournalLease {
            owner: obligation.owner.clone(),
            operation_token,
        };
        let cursor = current_cursor(&replacement);
        state.owners.insert(
            obligation.owner.account_key.clone(),
            JournalOwnerState::Active {
                generation: obligation.owner.account_generation,
                operation_token,
                needs_resync: false,
                ambiguous_append: None,
            },
        );
        state
            .accounts
            .insert(obligation.owner.account_key.clone(), replacement);
        Ok(LiveEventJournalActivation { lease, cursor })
    }

    /// White-box recovery helper for corruption and format tests. Production
    /// code must use a host-authorized reseed, account-generation rotation, or
    /// FIFO-sealed rollover; an active lease alone never exposes this reset.
    #[cfg(test)]
    fn clear_account<A: LiveEventJournalAuthority>(
        &self,
        authority: &A,
    ) -> Result<LiveEventCursor, LiveEventJournalError> {
        let owner = authority.journal_owner();
        let mut state = self.lock_state()?;
        self.verify_storage_root()?;
        self.ensure_owner_capacity(&state, owner)?;
        self.ensure_account_file_capacity_for(owner)?;
        let operation_token = authorize_clear(&state.owners, authority)?;
        state.owners.insert(
            owner.account_key.clone(),
            JournalOwnerState::TransitionIncomplete {
                generation: owner.account_generation,
                operation_token,
            },
        );
        state.accounts.remove(&owner.account_key);
        let mut replacement = self.empty_account(owner)?;
        self.replace_account_file(owner, &mut replacement)?;
        let cursor = current_cursor(&replacement);
        let operation_token = new_process_token()?;
        state.owners.insert(
            owner.account_key.clone(),
            JournalOwnerState::Active {
                generation: owner.account_generation,
                operation_token,
                needs_resync: false,
                ambiguous_append: None,
            },
        );
        state
            .accounts
            .insert(owner.account_key.clone(), replacement);
        Ok(cursor)
    }

    /// Atomically rotate an account journal across Maple's one-step process
    /// generation advance. Requiring both adjacent owners lets a stale handle
    /// fail once another clear has already rebound the in-memory journal.
    pub(crate) fn rotate_account_generation<A: LiveEventJournalAuthority>(
        &self,
        previous_authority: &A,
        current_owner: &LiveEventAccountOwner,
    ) -> Result<LiveEventCursor, LiveEventJournalError> {
        let previous_owner = previous_authority.journal_owner();
        if previous_owner.account_key != current_owner.account_key
            || previous_owner
                .account_generation
                .checked_add(1)
                .is_none_or(|next| next != current_owner.account_generation)
        {
            return Err(LiveEventJournalError::InvalidAccountOwner);
        }
        let mut state = self.lock_state()?;
        self.verify_storage_root()?;
        self.ensure_owner_capacity(&state, current_owner)?;
        self.ensure_account_file_capacity_for(current_owner)?;
        let operation_token = authorize_rotation(&state.owners, previous_authority, current_owner)?;
        state.owners.insert(
            current_owner.account_key.clone(),
            JournalOwnerState::TransitionIncomplete {
                generation: current_owner.account_generation,
                operation_token,
            },
        );
        state.accounts.remove(&current_owner.account_key);
        let mut replacement = self.empty_account(current_owner)?;
        self.replace_account_file(current_owner, &mut replacement)?;
        let cursor = current_cursor(&replacement);
        let operation_token = new_process_token()?;
        state.owners.insert(
            current_owner.account_key.clone(),
            JournalOwnerState::Active {
                generation: current_owner.account_generation,
                operation_token,
                needs_resync: false,
                ambiguous_append: None,
            },
        );
        state
            .accounts
            .insert(current_owner.account_key.clone(), replacement);
        Ok(cursor)
    }

    /// Evict decoded payloads for an inactive account while retaining its
    /// process-generation fence and durable bounded suffix.
    pub(crate) fn unload_account<A: LiveEventJournalAuthority>(
        &self,
        authority: &A,
    ) -> Result<(), LiveEventJournalError> {
        let owner = authority.journal_owner();
        let mut state = self.lock_state()?;
        self.verify_storage_root()?;
        let (needs_resync, ambiguous_append) =
            authorize_active_owner(&mut state.owners, authority)?;
        if needs_resync || ambiguous_append.is_some() {
            return Err(LiveEventJournalError::OwnerTransitionIncomplete);
        }
        state.accounts.remove(&owner.account_key);
        Ok(())
    }

    /// Fence an exact active journal after its coordinator FIFO has sealed.
    /// No disk mutation happens here; once this returns, every ordinary
    /// operation through this or any cloned lease fails with `JournalRetired`.
    pub(crate) fn seal_for_retirement(
        &self,
        lease: &LiveEventJournalLease,
        expected_head: &LiveEventCursor,
    ) -> Result<LiveEventJournalRetirementToken, LiveEventJournalError> {
        expected_head.validate()?;
        let mut state = self.lock_state()?;
        self.verify_storage_root()?;
        let account = self.prepare_account(&mut state, lease)?;
        ensure_expected_head(account, expected_head)?;
        let retirement_nonce = new_process_token()?;
        let journal_id = decode_hex_array::<JOURNAL_ID_BYTES>(&account.journal_id)?;
        let journal_id_string = account.journal_id.clone();
        let head_sequence = account.head_sequence;
        let token = LiveEventJournalRetirementToken {
            owner: lease.owner.clone(),
            operation_token: lease.operation_token,
            retirement_nonce,
            journal_id: journal_id_string,
            head_sequence,
        };
        state.owners.insert(
            lease.owner.account_key.clone(),
            JournalOwnerState::Retiring {
                generation: lease.owner.account_generation,
                operation_token: lease.operation_token,
                retirement_nonce,
                journal_id,
                head_sequence,
                rename_committed: false,
            },
        );
        state.accounts.remove(&lease.owner.account_key);
        Ok(token)
    }

    /// Durably retire a FIFO-sealed account journal.
    ///
    /// Renaming to a validated pending name and syncing the root is the commit
    /// point. Startup finishes only such well-formed pending retirements. The
    /// unlink and second directory sync make quota recovery durable before
    /// success is acknowledged. Ambiguous errors keep the process-local state
    /// fenced and are retryable with this exact opaque token.
    pub(crate) fn retire_account(
        &self,
        token: &LiveEventJournalRetirementToken,
    ) -> Result<(), LiveEventJournalError> {
        let mut state = self.lock_state()?;
        self.verify_storage_root()?;
        let expected_journal_id = decode_hex_array::<JOURNAL_ID_BYTES>(&token.journal_id)?;
        let rename_committed = match state.owners.get(&token.owner.account_key).copied() {
            Some(JournalOwnerState::Retiring {
                generation,
                operation_token,
                retirement_nonce,
                journal_id,
                head_sequence,
                rename_committed,
            }) if generation == token.owner.account_generation
                && constant_time_token_eq(&operation_token, &token.operation_token)
                && constant_time_token_eq(&retirement_nonce, &token.retirement_nonce)
                && journal_id == expected_journal_id
                && head_sequence == token.head_sequence =>
            {
                rename_committed
            }
            Some(JournalOwnerState::Retiring { generation, .. })
            | Some(JournalOwnerState::Active { generation, .. })
            | Some(JournalOwnerState::TransitionIncomplete { generation, .. })
            | Some(JournalOwnerState::RolloverPending { generation, .. })
                if generation == token.owner.account_generation =>
            {
                return Err(LiveEventJournalError::JournalRetired);
            }
            Some(_) => return Err(LiveEventJournalError::OwnerGenerationMismatch),
            None => return Err(LiveEventJournalError::JournalRetired),
        };

        let source = self.journal_path(&token.owner);
        let pending = self.retirement_path(&token.owner, &token.retirement_nonce);
        let source_state = account_file_state(&source)?;
        let pending_state = account_file_state(&pending)?;
        if matches!(source_state, AccountFileState::Present)
            && matches!(pending_state, AccountFileState::Present)
        {
            return Err(LiveEventJournalError::StorageCorrupt);
        }

        let max_disk_bytes = self
            .inner
            .limits
            .max_disk_bytes()
            .ok_or(LiveEventJournalError::InvalidLimits)?;
        if matches!(source_state, AccountFileState::Present) {
            if rename_committed {
                return Err(LiveEventJournalError::StorageCorrupt);
            }
            validate_retirement_identity(&source, token, max_disk_bytes)?;
            #[cfg(test)]
            if self.take_retirement_failure(RetirementFailureBoundary::BeforeRename) {
                return Err(LiveEventJournalError::StorageUnavailable);
            }
            fs::rename(&source, &pending).map_err(|_| LiveEventJournalError::StorageUnavailable)?;
            #[cfg(test)]
            if self.take_retirement_failure(RetirementFailureBoundary::AfterRename) {
                return Err(LiveEventJournalError::StorageUnavailable);
            }
        } else if matches!(pending_state, AccountFileState::Present) {
            validate_retirement_identity(&pending, token, max_disk_bytes)?;
        } else if !rename_committed {
            return Err(LiveEventJournalError::StorageUnavailable);
        }

        let pending_exists = matches!(account_file_state(&pending)?, AccountFileState::Present);
        if pending_exists {
            self.inner.root_guard.sync()?;
            if let Some(JournalOwnerState::Retiring {
                rename_committed, ..
            }) = state.owners.get_mut(&token.owner.account_key)
            {
                *rename_committed = true;
            }
            #[cfg(test)]
            if self.take_retirement_failure(RetirementFailureBoundary::AfterRenameDirectorySync) {
                return Err(LiveEventJournalError::StorageUnavailable);
            }
            fs::remove_file(&pending).map_err(|_| LiveEventJournalError::StorageUnavailable)?;
            #[cfg(test)]
            if self.take_retirement_failure(RetirementFailureBoundary::AfterUnlink) {
                return Err(LiveEventJournalError::StorageUnavailable);
            }
        }

        self.inner.root_guard.sync()?;
        #[cfg(test)]
        if self.take_retirement_failure(RetirementFailureBoundary::AfterFinalDirectorySync) {
            return Err(LiveEventJournalError::StorageUnavailable);
        }
        self.verify_storage_root()?;
        state.accounts.remove(&token.owner.account_key);
        state.owners.remove(&token.owner.account_key);
        Ok(())
    }

    /// Prepare a one-use reseed obligation only from the host's concrete,
    /// non-forgeable durable-history authority and this exact observed broken
    /// file generation. This does not mutate disk and does not itself claim
    /// that the coordinator FIFO has been sealed.
    pub(crate) fn prepare_reseed(
        &self,
        required: LiveEventJournalReseedRequired,
        authority: VerifiedJournalReseedAuthority,
    ) -> Result<LiveEventJournalReseedObligation, LiveEventJournalError> {
        let result = self.prepare_reseed_parts(
            required,
            authority.owner(),
            authority.projection_bytes(),
            *authority.durable_head_commitment(),
            *authority.nonce(),
        );
        drop(authority);
        result
    }

    fn prepare_reseed_parts(
        &self,
        required: LiveEventJournalReseedRequired,
        authority_owner: &LiveEventAccountOwner,
        authority_projection: &[u8],
        durable_head_commitment: [u8; 32],
        authority_nonce: [u8; 32],
    ) -> Result<LiveEventJournalReseedObligation, LiveEventJournalError> {
        if authority_owner != &required.owner
            || authority_projection.is_empty()
            || authority_projection.len() > MAX_CHECKPOINT_BYTES
        {
            return Err(LiveEventJournalError::InvalidCheckpoint);
        }
        if durable_head_commitment.iter().all(|byte| *byte == 0)
            || authority_nonce.iter().all(|byte| *byte == 0)
        {
            return Err(LiveEventJournalError::InvalidCheckpoint);
        }
        let projection_digest = projection_digest(authority_projection);
        let new_journal_id = new_journal_id()?;
        let new_journal_id_bytes = decode_hex_array::<JOURNAL_ID_BYTES>(&new_journal_id)?;
        let mut state = self.lock_state()?;
        self.verify_storage_root()?;
        let observed = self.observe_journal_generation(&required.owner)?;
        if observed != required.observed {
            return Err(LiveEventJournalError::JournalReplaced);
        }
        match state.owners.get(&required.owner.account_key).copied() {
            Some(JournalOwnerState::ReseedRequired {
                generation,
                observation_token,
            }) if generation == required.owner.account_generation
                && constant_time_token_eq(&observation_token, &required.observation_token) => {}
            Some(JournalOwnerState::ReseedRequired { generation, .. })
            | Some(JournalOwnerState::Reseeding { generation, .. })
            | Some(JournalOwnerState::Active { generation, .. })
            | Some(JournalOwnerState::TransitionIncomplete { generation, .. })
            | Some(JournalOwnerState::RolloverPending { generation, .. })
            | Some(JournalOwnerState::Retiring { generation, .. })
                if generation == required.owner.account_generation =>
            {
                return Err(LiveEventJournalError::JournalReplaced);
            }
            Some(_) => return Err(LiveEventJournalError::OwnerGenerationMismatch),
            None => return Err(LiveEventJournalError::JournalReplaced),
        }
        state.owners.insert(
            required.owner.account_key.clone(),
            JournalOwnerState::Reseeding {
                generation: required.owner.account_generation,
                observation_token: required.observation_token,
                authority_nonce,
                durable_head_commitment,
                projection_digest,
                new_journal_id: new_journal_id_bytes,
                sealed: false,
            },
        );
        Ok(LiveEventJournalReseedObligation {
            owner: required.owner,
            observed,
            observation_token: required.observation_token,
            authority_nonce,
            durable_head_commitment,
            projection_digest,
            projection_bytes: authority_projection.to_vec().into_boxed_slice(),
            new_journal_id,
            sealed: false,
        })
    }

    /// Mark the obligation sealed only after the host has closed the exact
    /// coordinator and all live subscribers under its lifecycle lock.
    pub(crate) fn mark_reseed_sealed(
        &self,
        obligation: &mut LiveEventJournalReseedObligation,
    ) -> Result<(), LiveEventJournalError> {
        let mut state = self.lock_state()?;
        self.verify_storage_root()?;
        match state.owners.get_mut(&obligation.owner.account_key) {
            Some(JournalOwnerState::Reseeding {
                generation,
                observation_token,
                authority_nonce,
                durable_head_commitment,
                projection_digest,
                new_journal_id,
                sealed,
            }) if *generation == obligation.owner.account_generation
                && constant_time_token_eq(observation_token, &obligation.observation_token)
                && constant_time_digest_eq(authority_nonce, &obligation.authority_nonce)
                && constant_time_digest_eq(
                    durable_head_commitment,
                    &obligation.durable_head_commitment,
                )
                && constant_time_digest_eq(projection_digest, &obligation.projection_digest)
                && *new_journal_id
                    == decode_hex_array::<JOURNAL_ID_BYTES>(&obligation.new_journal_id)? =>
            {
                *sealed = true;
                obligation.sealed = true;
                Ok(())
            }
            Some(_) => Err(LiveEventJournalError::JournalReplaced),
            None => Err(LiveEventJournalError::JournalReplaced),
        }
    }

    /// Atomically replace the exact observed broken generation with a fresh v3
    /// journal carrying the authoritative absolute projection at sequence
    /// zero. The replacement's normal file+rename+directory durability barrier
    /// runs before a fresh process lease is returned.
    pub(crate) fn commit_reseed(
        &self,
        obligation: &LiveEventJournalReseedObligation,
    ) -> Result<LiveEventJournalActivation, LiveEventJournalError> {
        if !obligation.sealed {
            return Err(LiveEventJournalError::OwnerTransitionIncomplete);
        }
        let mut state = self.lock_state()?;
        self.verify_storage_root()?;
        match state.owners.get(&obligation.owner.account_key).copied() {
            Some(JournalOwnerState::Reseeding {
                generation,
                observation_token,
                authority_nonce,
                durable_head_commitment,
                projection_digest,
                new_journal_id,
                sealed: true,
            }) if generation == obligation.owner.account_generation
                && constant_time_token_eq(&observation_token, &obligation.observation_token)
                && constant_time_digest_eq(&authority_nonce, &obligation.authority_nonce)
                && constant_time_digest_eq(
                    &durable_head_commitment,
                    &obligation.durable_head_commitment,
                )
                && constant_time_digest_eq(&projection_digest, &obligation.projection_digest)
                && new_journal_id
                    == decode_hex_array::<JOURNAL_ID_BYTES>(&obligation.new_journal_id)? => {}
            Some(_) => return Err(LiveEventJournalError::JournalReplaced),
            None => return Err(LiveEventJournalError::JournalReplaced),
        }
        if !constant_time_digest_eq(
            &projection_digest(&obligation.projection_bytes),
            &obligation.projection_digest,
        ) {
            return Err(LiveEventJournalError::InvalidCheckpoint);
        }
        let current_observation = self.observe_journal_generation(&obligation.owner)?;
        let replacement = if current_observation == obligation.observed {
            let checkpoint_bytes = obligation.projection_bytes.to_vec();
            let mut replacement = self.empty_account(&obligation.owner)?;
            replacement
                .journal_id
                .clone_from(&obligation.new_journal_id);
            replacement.checkpoint = Some(StoredCheckpoint {
                schema: CHECKPOINT_SCHEMA.to_string(),
                through_sequence: 0,
                commitment: bytes_commitment(&checkpoint_bytes),
                bytes: checkpoint_bytes,
            });
            // Preserve the sealed obligation state for an exact retry. The
            // next commit re-observes old or new and accepts only this
            // preselected journal ID and exact absolute projection.
            self.replace_account_file(&obligation.owner, &mut replacement)?;
            replacement
        } else {
            let replacement = self.load_account(&obligation.owner)?;
            if !is_exact_reseed_replacement(&replacement, obligation) {
                return Err(LiveEventJournalError::JournalReplaced);
            }
            // The exact replacement can be visible after a lost post-rename
            // acknowledgement. Never issue a fresh lease until the directory
            // entry has crossed the durability barrier on this retry.
            self.inner.root_guard.sync()?;
            self.verify_storage_root()?;
            replacement
        };
        let operation_token = new_process_token()?;
        let lease = LiveEventJournalLease {
            owner: obligation.owner.clone(),
            operation_token,
        };
        let cursor = current_cursor(&replacement);
        state.owners.insert(
            obligation.owner.account_key.clone(),
            JournalOwnerState::Active {
                generation: obligation.owner.account_generation,
                operation_token,
                needs_resync: false,
                ambiguous_append: None,
            },
        );
        state
            .accounts
            .insert(obligation.owner.account_key.clone(), replacement);
        Ok(LiveEventJournalActivation { lease, cursor })
    }

    fn lock_state(&self) -> Result<MutexGuard<'_, JournalState<T>>, LiveEventJournalError> {
        self.inner
            .state
            .lock()
            .map_err(|_| LiveEventJournalError::LockUnavailable)
    }

    fn verify_storage_root(&self) -> Result<(), LiveEventJournalError> {
        self.inner.root_guard.verify(&self.inner.root)
    }

    fn observe_journal_generation(
        &self,
        owner: &LiveEventAccountOwner,
    ) -> Result<ObservedJournalGeneration, LiveEventJournalError> {
        self.verify_storage_root()?;
        let path = self.journal_path(owner);
        if matches!(account_file_state(&path)?, AccountFileState::Missing) {
            return Ok(ObservedJournalGeneration::Missing);
        }
        let file = open_read_no_follow(&path)?;
        let metadata = file
            .metadata()
            .map_err(|_| LiveEventJournalError::StorageUnavailable)?;
        if !metadata.file_type().is_file() || !metadata_owned_by_effective_user(&metadata) {
            return Err(LiveEventJournalError::StorageCorrupt);
        }
        let max_read = self
            .inner
            .limits
            .max_disk_bytes()
            .ok_or(LiveEventJournalError::InvalidLimits)?
            .checked_add(1)
            .ok_or(LiveEventJournalError::InvalidLimits)?;
        let capacity = usize::try_from(metadata.len().min(max_read))
            .map_err(|_| LiveEventJournalError::StorageCorrupt)?;
        let mut bytes = Vec::with_capacity(capacity);
        file.take(max_read)
            .read_to_end(&mut bytes)
            .map_err(|_| LiveEventJournalError::StorageUnavailable)?;
        self.verify_storage_root()?;
        let digest = observed_file_digest(metadata.len(), &bytes);
        let identity = file_identity(&metadata);
        if metadata.len() <= max_read.saturating_sub(1) && bytes.len() >= DISK_PREFIX_BYTES {
            if let Ok(anchor) = select_v3_anchor(&bytes) {
                if anchor.committed_end == metadata.len() {
                    return Ok(ObservedJournalGeneration::V3 {
                        file_nonce: anchor.file_nonce,
                        journal_id: decode_hex_array::<JOURNAL_ID_BYTES>(&anchor.journal_id)?,
                        head_sequence: anchor.committed_head_sequence,
                        committed_end: anchor.committed_end,
                        digest,
                        file_identity: identity,
                    });
                }
            }
        }
        Ok(ObservedJournalGeneration::LegacyOrCorrupt {
            length: metadata.len(),
            digest,
            file_identity: identity,
        })
    }

    fn prepare_account<'a, A: LiveEventJournalAuthority>(
        &self,
        state: &'a mut JournalState<T>,
        authority: &A,
    ) -> Result<&'a mut AccountJournal<T>, LiveEventJournalError> {
        let owner = authority.journal_owner();
        self.verify_storage_root()?;
        let (needs_resync, ambiguous_append) =
            authorize_active_owner(&mut state.owners, authority)?;
        if ambiguous_append.is_some() {
            // A generic activation operation cannot decide the outcome of an
            // ambiguous producer append. Only an exact retry carrying the
            // original ingress capability and event identity may reconcile it.
            return Err(LiveEventJournalError::OwnerTransitionIncomplete);
        }
        if !state.accounts.contains_key(&owner.account_key) {
            let account = match account_file_state(&self.journal_path(owner))? {
                AccountFileState::Present => self.load_account(owner),
                AccountFileState::Missing => self.create_account(owner),
            };
            let account = match account {
                Ok(account) => account,
                Err(error) => {
                    mark_owner_indeterminate(&mut state.owners, authority)?;
                    state.accounts.remove(&owner.account_key);
                    return Err(error);
                }
            };
            state.accounts.insert(owner.account_key.clone(), account);
        }

        {
            let account = state
                .accounts
                .get_mut(&owner.account_key)
                .ok_or(LiveEventJournalError::StorageUnavailable)?;
            ensure_owner_generation(account, owner)?;
            if needs_resync {
                if let Err(error) = self.replace_account_file(owner, account) {
                    state.accounts.remove(&owner.account_key);
                    return Err(error);
                }
            }
        }
        if needs_resync {
            mark_owner_resynced(&mut state.owners, authority)?;
        }
        state
            .accounts
            .get_mut(&owner.account_key)
            .ok_or(LiveEventJournalError::StorageUnavailable)
    }

    fn prepare_ingress_account<'a>(
        &self,
        state: &'a mut JournalState<T>,
        ingress: &LiveEventJournalIngressLease,
        expected_head: &LiveEventCursor,
        event_id: &str,
        event_commitment: &str,
    ) -> Result<&'a mut AccountJournal<T>, LiveEventJournalError> {
        self.verify_storage_root()?;
        // Authenticate the producer generation before consulting any durable
        // event-ID record. A stale producer therefore cannot probe or mutate a
        // replacement journal, even when it reuses an old event ID.
        let (needs_resync, pending_append) = authorize_active_ingress(&state.owners, ingress)?;
        let supplied_append =
            ambiguous_append_identity(ingress, expected_head, event_id, event_commitment)?;
        if needs_resync && pending_append != Some(supplied_append) {
            return Err(LiveEventJournalError::OwnerTransitionIncomplete);
        }
        if !state.accounts.contains_key(&ingress.owner.account_key) {
            let account = match account_file_state(&self.journal_path(&ingress.owner))? {
                AccountFileState::Present => self.load_account(&ingress.owner),
                // An ingress lease is never authority to recreate a missing
                // account file. Only activation under the host lifecycle may
                // create storage for a newly admitted owner.
                AccountFileState::Missing => Err(LiveEventJournalError::JournalReplaced),
            };
            let account = match account {
                Ok(account) => account,
                Err(error) => {
                    if needs_resync {
                        // Keep normal operations fenced. A torn or otherwise
                        // ambiguous durable image advances to the explicit
                        // host-authorized reseed path.
                        state.accounts.remove(&ingress.owner.account_key);
                        if matches!(error, LiveEventJournalError::StorageCorrupt) {
                            let observation_token = new_process_token()?;
                            state.owners.insert(
                                ingress.owner.account_key.clone(),
                                JournalOwnerState::ReseedRequired {
                                    generation: ingress.owner.account_generation,
                                    observation_token,
                                },
                            );
                        }
                    }
                    return Err(error);
                }
            };
            state
                .accounts
                .insert(ingress.owner.account_key.clone(), account);
        }
        {
            let account = state
                .accounts
                .get_mut(&ingress.owner.account_key)
                .ok_or(LiveEventJournalError::StorageUnavailable)?;
            ensure_owner_generation(account, &ingress.owner)?;
            let account_journal_id = decode_hex_array::<JOURNAL_ID_BYTES>(&account.journal_id)?;
            if account_journal_id != ingress.journal_id {
                return Err(LiveEventJournalError::JournalReplaced);
            }
            if needs_resync {
                // Rewriting the exact recovered generation re-establishes the
                // directory durability barrier for either the old head or the
                // exact committed retry. It never adopts an unanchored tail.
                if let Err(error) = self.replace_account_file(&ingress.owner, account) {
                    state.accounts.remove(&ingress.owner.account_key);
                    return Err(error);
                }
            }
        }
        if needs_resync {
            mark_ingress_owner_resynced(&mut state.owners, ingress)?;
        }
        state
            .accounts
            .get_mut(&ingress.owner.account_key)
            .ok_or(LiveEventJournalError::StorageUnavailable)
    }

    fn create_account(
        &self,
        owner: &LiveEventAccountOwner,
    ) -> Result<AccountJournal<T>, LiveEventJournalError> {
        self.ensure_account_file_capacity_for(owner)?;
        let mut account = self.empty_account(owner)?;
        self.replace_account_file(owner, &mut account)?;
        Ok(account)
    }

    fn empty_account(
        &self,
        owner: &LiveEventAccountOwner,
    ) -> Result<AccountJournal<T>, LiveEventJournalError> {
        Ok(AccountJournal {
            journal_id: new_journal_id()?,
            account_generation: owner.account_generation,
            head_sequence: 0,
            entries: VecDeque::new(),
            total_payload_bytes: 0,
            checkpoint: None,
            event_ids: HashMap::new(),
            event_id_metadata_bytes: 0,
            disk_anchor: None,
        })
    }

    fn load_account(
        &self,
        owner: &LiveEventAccountOwner,
    ) -> Result<AccountJournal<T>, LiveEventJournalError> {
        self.verify_storage_root()?;
        let path = self.journal_path(owner);
        let file = open_read_no_follow(&path)?;
        let metadata = file
            .metadata()
            .map_err(|_| LiveEventJournalError::StorageUnavailable)?;
        if !metadata.file_type().is_file()
            || metadata.len()
                > self
                    .inner
                    .limits
                    .max_disk_bytes()
                    .ok_or(LiveEventJournalError::InvalidLimits)?
        {
            return Err(LiveEventJournalError::StorageCorrupt);
        }
        let mut bytes = Vec::with_capacity(
            usize::try_from(metadata.len()).map_err(|_| LiveEventJournalError::StorageCorrupt)?,
        );
        let max_read = self
            .inner
            .limits
            .max_disk_bytes()
            .ok_or(LiveEventJournalError::InvalidLimits)?
            .checked_add(1)
            .ok_or(LiveEventJournalError::InvalidLimits)?;
        file.take(max_read)
            .read_to_end(&mut bytes)
            .map_err(|_| LiveEventJournalError::StorageUnavailable)?;
        if u64::try_from(bytes.len()).is_ok_and(|length| length == max_read) {
            return Err(LiveEventJournalError::StorageCorrupt);
        }
        if bytes.len() < DISK_PREFIX_BYTES {
            return Err(LiveEventJournalError::StorageCorrupt);
        }
        let actual_length =
            u64::try_from(bytes.len()).map_err(|_| LiveEventJournalError::StorageCorrupt)?;
        let anchor = select_v3_anchor(&bytes)?;
        if anchor.account_key != owner.account_key
            || anchor.committed_end != actual_length
            || anchor.committed_end
                > self
                    .inner
                    .limits
                    .max_disk_bytes()
                    .ok_or(LiveEventJournalError::InvalidLimits)?
        {
            return Err(LiveEventJournalError::StorageCorrupt);
        }
        let snapshot_start = usize::try_from(anchor.snapshot_offset)
            .map_err(|_| LiveEventJournalError::StorageCorrupt)?;
        let snapshot_end = usize::try_from(anchor.data_start)
            .map_err(|_| LiveEventJournalError::StorageCorrupt)?;
        let committed_end = usize::try_from(anchor.committed_end)
            .map_err(|_| LiveEventJournalError::StorageCorrupt)?;
        let snapshot = bytes
            .get(snapshot_start..snapshot_end)
            .ok_or(LiveEventJournalError::StorageCorrupt)?;
        if sha256_parts(DISK_SNAPSHOT_HASH_DOMAIN, &[snapshot]) != anchor.snapshot_hash {
            return Err(LiveEventJournalError::StorageCorrupt);
        }
        let header: JournalHeader =
            serde_json::from_slice(snapshot).map_err(|_| LiveEventJournalError::StorageCorrupt)?;
        validate_header(&header, owner)?;
        if header.journal_id != anchor.journal_id
            || header.head_sequence != anchor.snapshot_head_sequence
        {
            return Err(LiveEventJournalError::StorageCorrupt);
        }

        let mut entries = VecDeque::new();
        let mut total_payload_bytes = 0usize;
        let mut previous_sequence = None;
        let mut frame_offset = snapshot_end;
        let mut chain_hash = v3_chain_base(&anchor)?;
        while frame_offset < committed_end {
            if entries.len() == self.inner.limits.max_entries {
                return Err(LiveEventJournalError::StorageCorrupt);
            }
            let (entry, next_offset, frame_hash) = decode_v3_frame(
                &bytes,
                frame_offset,
                committed_end,
                &chain_hash,
                self.inner
                    .limits
                    .max_payload_bytes
                    .saturating_add(MAX_RECORD_OVERHEAD_BYTES),
            )?;
            validate_stored_entry(&entry, previous_sequence, self.inner.limits)?;
            let payload_bytes = serialized_payload_bytes(&entry.payload)?;
            total_payload_bytes = total_payload_bytes
                .checked_add(payload_bytes)
                .ok_or(LiveEventJournalError::StorageCorrupt)?;
            if total_payload_bytes > self.inner.limits.max_total_payload_bytes {
                return Err(LiveEventJournalError::StorageCorrupt);
            }
            previous_sequence = Some(entry.sequence);
            entries.push_back(entry);
            frame_offset = next_offset;
            chain_hash = frame_hash;
        }
        if frame_offset != committed_end
            || chain_hash != anchor.committed_chain_hash
            || u64::try_from(entries.len()).map_err(|_| LiveEventJournalError::StorageCorrupt)?
                != anchor.committed_frame_count
        {
            return Err(LiveEventJournalError::StorageCorrupt);
        }
        self.verify_storage_root()?;

        let head_sequence = anchor.committed_head_sequence;
        let snapshot_head_sequence = header.head_sequence;
        let checkpoint = header.checkpoint;
        let checkpoint_sequence = checkpoint
            .as_ref()
            .map_or(0, |checkpoint| checkpoint.through_sequence);
        let first_sequence = entries.front().map(|entry| entry.sequence);
        let last_sequence = entries.back().map(|entry| entry.sequence);
        let suffix_covers_restart = if checkpoint.is_some() {
            head_sequence == checkpoint_sequence
                || first_sequence
                    .is_some_and(|first| first <= checkpoint_sequence.saturating_add(1))
        } else {
            head_sequence == 0 || first_sequence == Some(1)
        };
        if checkpoint.as_ref().is_some_and(|checkpoint| {
            checkpoint.schema != CHECKPOINT_SCHEMA
                || checkpoint.bytes.is_empty()
                || checkpoint.bytes.len() > MAX_CHECKPOINT_BYTES
                || checkpoint.through_sequence > snapshot_head_sequence
                || checkpoint.through_sequence > MAX_CURSOR_SEQUENCE
                || checkpoint.commitment != bytes_commitment(&checkpoint.bytes)
        }) || head_sequence > MAX_CURSOR_SEQUENCE
            || head_sequence > MAX_IDEMPOTENCY_EVENT_IDS as u64
            || snapshot_head_sequence > head_sequence
            || header.event_ids.len() > MAX_IDEMPOTENCY_EVENT_IDS
            || !suffix_covers_restart
            || (entries.is_empty() && head_sequence != checkpoint_sequence)
            || last_sequence.is_some_and(|last| last != head_sequence)
            || entries
                .iter()
                .find(|entry| entry.sequence > snapshot_head_sequence)
                .is_some_and(|entry| entry.sequence != snapshot_head_sequence.saturating_add(1))
        {
            return Err(LiveEventJournalError::StorageCorrupt);
        }
        let mut event_ids = HashMap::with_capacity(header.event_ids.len());
        let mut event_sequences = vec![
            false;
            usize::try_from(head_sequence)
                .map_err(|_| LiveEventJournalError::StorageCorrupt)?
                .saturating_add(1)
        ];
        let mut event_id_metadata_bytes = 0usize;
        for record in header.event_ids {
            validate_stored_event_id(&record, snapshot_head_sequence)?;
            let sequence = usize::try_from(record.sequence)
                .map_err(|_| LiveEventJournalError::StorageCorrupt)?;
            if event_ids.contains_key(&record.event_id)
                || event_sequences.get(sequence).copied() != Some(false)
            {
                return Err(LiveEventJournalError::StorageCorrupt);
            }
            event_id_metadata_bytes = event_id_metadata_bytes
                .checked_add(encoded_event_id_bytes(&record)?)
                .ok_or(LiveEventJournalError::StorageCorrupt)?;
            event_sequences[sequence] = true;
            event_ids.insert(record.event_id.clone(), record);
        }
        if event_ids.len()
            != usize::try_from(snapshot_head_sequence)
                .map_err(|_| LiveEventJournalError::StorageCorrupt)?
        {
            return Err(LiveEventJournalError::StorageCorrupt);
        }
        for entry in &entries {
            let record = stored_event_id(entry)?;
            match event_ids.get(&record.event_id) {
                Some(existing)
                    if existing.sequence != record.sequence
                        || existing.commitment != record.commitment =>
                {
                    return Err(LiveEventJournalError::StorageCorrupt);
                }
                Some(_) => {}
                None => {
                    let sequence = usize::try_from(record.sequence)
                        .map_err(|_| LiveEventJournalError::StorageCorrupt)?;
                    if event_sequences.get(sequence).copied() != Some(false) {
                        return Err(LiveEventJournalError::StorageCorrupt);
                    }
                    event_id_metadata_bytes = event_id_metadata_bytes
                        .checked_add(encoded_event_id_bytes(&record)?)
                        .ok_or(LiveEventJournalError::StorageCorrupt)?;
                    event_sequences[sequence] = true;
                    event_ids.insert(record.event_id.clone(), record);
                }
            }
        }
        if event_ids.len() > MAX_IDEMPOTENCY_EVENT_IDS
            || event_ids.len()
                != usize::try_from(head_sequence)
                    .map_err(|_| LiveEventJournalError::StorageCorrupt)?
            || event_sequences.iter().skip(1).any(|seen| !seen)
            || event_id_metadata_bytes > MAX_IDEMPOTENCY_METADATA_BYTES
            || event_ids
                .values()
                .any(|record| validate_stored_event_id(record, head_sequence).is_err())
        {
            return Err(LiveEventJournalError::StorageCorrupt);
        }

        let account = AccountJournal {
            journal_id: header.journal_id,
            // The persisted file proves the account key, not an ephemeral
            // process generation. The caller must already hold the current
            // verified owner when this process first loads the account.
            account_generation: owner.account_generation,
            head_sequence,
            entries,
            total_payload_bytes,
            checkpoint,
            event_ids,
            event_id_metadata_bytes,
            disk_anchor: Some(anchor.clone()),
        };
        Ok(account)
    }

    fn append_record(
        &self,
        owner: &LiveEventAccountOwner,
        account: &mut AccountJournal<T>,
        entry: &StoredEntry<T>,
    ) -> Result<(), LiveEventJournalError> {
        self.verify_storage_root()?;
        let current_anchor = account
            .disk_anchor
            .as_ref()
            .ok_or(LiveEventJournalError::StorageCorrupt)?;
        if current_anchor.journal_id != account.journal_id
            || current_anchor.account_key != owner.account_key
            || current_anchor.committed_head_sequence != account.head_sequence
        {
            return Err(LiveEventJournalError::StorageCorrupt);
        }
        let (encoded, frame_hash) = encode_v3_frame(entry, &current_anchor.committed_chain_hash)?;
        let file = open_read_write_no_follow(&self.journal_path(owner))?;
        set_owner_only_file(&file)?;
        let current_length = file
            .metadata()
            .map_err(|_| LiveEventJournalError::StorageUnavailable)?
            .len();
        if current_length != current_anchor.committed_end {
            return Err(LiveEventJournalError::StorageCorrupt);
        }
        let encoded_length =
            u64::try_from(encoded.len()).map_err(|_| LiveEventJournalError::StorageUnavailable)?;
        if current_anchor
            .committed_end
            .checked_add(encoded_length)
            .is_none_or(|length| length > self.inner.limits.max_disk_bytes().unwrap_or_default())
        {
            return Err(LiveEventJournalError::StorageUnavailable);
        }
        let committed_end = current_anchor
            .committed_end
            .checked_add(encoded_length)
            .ok_or(LiveEventJournalError::StorageUnavailable)?;
        write_all_at(&file, &encoded, current_anchor.committed_end)?;
        file.sync_data()
            .map_err(|_| LiveEventJournalError::StorageUnavailable)?;

        let mut next_anchor = current_anchor.clone();
        next_anchor.revision = next_anchor
            .revision
            .checked_add(1)
            .ok_or(LiveEventJournalError::SequenceExhausted)?;
        next_anchor.slot_index = (next_anchor.revision % DISK_ANCHOR_SLOT_COUNT as u64) as u8;
        next_anchor.committed_end = committed_end;
        next_anchor.committed_head_sequence = entry.sequence;
        next_anchor.committed_frame_count = next_anchor
            .committed_frame_count
            .checked_add(1)
            .ok_or(LiveEventJournalError::SequenceExhausted)?;
        next_anchor.committed_chain_hash = frame_hash;
        let encoded_anchor = encode_v3_anchor(&next_anchor)?;
        let anchor_offset = DISK_SUPERBLOCK_BYTES
            .checked_add(usize::from(next_anchor.slot_index) * DISK_ANCHOR_SLOT_BYTES)
            .and_then(|offset| u64::try_from(offset).ok())
            .ok_or(LiveEventJournalError::StorageUnavailable)?;
        write_all_at(&file, &encoded_anchor, anchor_offset)?;
        file.sync_data()
            .map_err(|_| LiveEventJournalError::StorageUnavailable)?;
        account.disk_anchor = Some(next_anchor);
        #[cfg(test)]
        if self
            .inner
            .fail_next_append_after_sync
            .swap(false, Ordering::SeqCst)
        {
            return Err(LiveEventJournalError::StorageUnavailable);
        }
        self.verify_storage_root()
    }

    #[cfg(test)]
    fn fail_next_append_after_sync(&self) {
        self.inner
            .fail_next_append_after_sync
            .store(true, Ordering::SeqCst);
    }

    fn replace_account_file(
        &self,
        owner: &LiveEventAccountOwner,
        account: &mut AccountJournal<T>,
    ) -> Result<(), LiveEventJournalError> {
        self.verify_storage_root()?;
        let mut header = JournalHeader {
            version: JOURNAL_FORMAT_VERSION,
            journal_id: account.journal_id.clone(),
            account_key: owner.account_key.clone(),
            head_sequence: account.head_sequence,
            checkpoint: account.checkpoint.clone(),
            event_ids: {
                let mut event_ids = account.event_ids.values().cloned().collect::<Vec<_>>();
                event_ids.sort_by(|left, right| left.event_id.cmp(&right.event_id));
                event_ids
            },
            integrity: String::new(),
        };
        header.integrity = journal_header_integrity(&header)?;
        let encoded = encode_v3_journal(&header, &account.entries, new_file_nonce()?)?;
        if u64::try_from(encoded.bytes.len()).is_err()
            || u64::try_from(encoded.bytes.len())
                .is_ok_and(|length| length > self.inner.limits.max_disk_bytes().unwrap_or_default())
        {
            return Err(LiveEventJournalError::StorageUnavailable);
        }
        let mut temporary = tempfile::Builder::new()
            .prefix(TEMP_FILE_PREFIX)
            .tempfile_in(&self.inner.root)
            .map_err(|_| LiveEventJournalError::StorageUnavailable)?;
        set_owner_only_file(temporary.as_file())?;
        temporary
            .write_all(&encoded.bytes)
            .map_err(|_| LiveEventJournalError::StorageUnavailable)?;
        #[cfg(test)]
        if self.take_replace_failure(ReplaceFailureBoundary::BeforeFileSync) {
            return Err(LiveEventJournalError::StorageUnavailable);
        }
        temporary
            .as_file_mut()
            .sync_all()
            .map_err(|_| LiveEventJournalError::StorageUnavailable)?;
        #[cfg(test)]
        if self.take_replace_failure(ReplaceFailureBoundary::AfterFileSync) {
            return Err(LiveEventJournalError::StorageUnavailable);
        }
        temporary
            .persist(self.journal_path(owner))
            .map_err(|_| LiveEventJournalError::StorageUnavailable)?;
        #[cfg(test)]
        if self.take_replace_failure(ReplaceFailureBoundary::AfterPersist) {
            return Err(LiveEventJournalError::StorageUnavailable);
        }
        self.inner.root_guard.sync()?;
        #[cfg(test)]
        if self.take_replace_failure(ReplaceFailureBoundary::AfterDirectorySync) {
            return Err(LiveEventJournalError::StorageUnavailable);
        }
        self.verify_storage_root()?;
        account.disk_anchor = Some(encoded.anchor);
        Ok(())
    }

    #[cfg(test)]
    fn fail_next_replace_at(&self, boundary: ReplaceFailureBoundary) {
        self.inner
            .fail_next_replace_at
            .store(boundary as u8, Ordering::SeqCst);
    }

    #[cfg(test)]
    fn take_replace_failure(&self, boundary: ReplaceFailureBoundary) -> bool {
        self.inner
            .fail_next_replace_at
            .compare_exchange(
                boundary as u8,
                ReplaceFailureBoundary::None as u8,
                Ordering::SeqCst,
                Ordering::SeqCst,
            )
            .is_ok()
    }

    fn ensure_account_file_capacity_for(
        &self,
        owner: &LiveEventAccountOwner,
    ) -> Result<(), LiveEventJournalError> {
        self.verify_storage_root()?;
        let mut account_files = 0usize;
        for entry in
            fs::read_dir(&self.inner.root).map_err(|_| LiveEventJournalError::StorageUnavailable)?
        {
            let entry = entry.map_err(|_| LiveEventJournalError::StorageUnavailable)?;
            let name = entry.file_name();
            let name = name.to_str().ok_or(LiveEventJournalError::StorageCorrupt)?;
            if is_account_journal_file_name(name) || parse_retiring_file_name(name).is_some() {
                account_files = account_files
                    .checked_add(1)
                    .ok_or(LiveEventJournalError::StorageCorrupt)?;
            }
        }
        self.verify_storage_root()?;
        let target_exists = matches!(
            account_file_state(&self.journal_path(owner))?,
            AccountFileState::Present
        );
        if account_files >= MAX_ACCOUNT_JOURNAL_FILES && !target_exists {
            Err(LiveEventJournalError::StorageUnavailable)
        } else {
            Ok(())
        }
    }

    fn ensure_owner_capacity(
        &self,
        state: &JournalState<T>,
        owner: &LiveEventAccountOwner,
    ) -> Result<(), LiveEventJournalError> {
        if state.owners.len() >= MAX_ACCOUNT_JOURNAL_FILES
            && !state.owners.contains_key(&owner.account_key)
        {
            Err(LiveEventJournalError::StorageUnavailable)
        } else {
            Ok(())
        }
    }

    fn journal_path(&self, owner: &LiveEventAccountOwner) -> PathBuf {
        self.inner
            .root
            .join(format!("{}.events", owner.account_key))
    }

    fn retirement_path(
        &self,
        owner: &LiveEventAccountOwner,
        retirement_nonce: &[u8; PROCESS_TOKEN_BYTES],
    ) -> PathBuf {
        self.inner.root.join(format!(
            "{RETIRING_FILE_PREFIX}{}-{}",
            owner.account_key,
            encode_hex(retirement_nonce)
        ))
    }

    #[cfg(test)]
    fn fail_next_retirement_at(&self, boundary: RetirementFailureBoundary) {
        self.inner
            .fail_next_retirement_at
            .store(boundary as u8, Ordering::SeqCst);
    }

    #[cfg(test)]
    fn take_retirement_failure(&self, boundary: RetirementFailureBoundary) -> bool {
        self.inner
            .fail_next_retirement_at
            .compare_exchange(
                boundary as u8,
                RetirementFailureBoundary::None as u8,
                Ordering::SeqCst,
                Ordering::SeqCst,
            )
            .is_ok()
    }
}

fn authorize_active_owner<A: LiveEventJournalAuthority>(
    owners: &mut HashMap<String, JournalOwnerState>,
    authority: &A,
) -> Result<(bool, Option<AmbiguousAppend>), LiveEventJournalError> {
    let owner = authority.journal_owner();
    match owners.get(&owner.account_key).copied() {
        Some(JournalOwnerState::Active {
            generation,
            operation_token,
            needs_resync,
            ambiguous_append,
        }) if generation == owner.account_generation
            && authority.matches_operation_token(&operation_token) =>
        {
            Ok((needs_resync, ambiguous_append))
        }
        Some(JournalOwnerState::TransitionIncomplete { generation, .. })
            if generation == owner.account_generation =>
        {
            Err(LiveEventJournalError::OwnerTransitionIncomplete)
        }
        Some(JournalOwnerState::RolloverPending { generation, .. })
            if generation == owner.account_generation =>
        {
            Err(LiveEventJournalError::OwnerTransitionIncomplete)
        }
        Some(JournalOwnerState::Retiring { generation, .. })
            if generation == owner.account_generation =>
        {
            Err(LiveEventJournalError::JournalRetired)
        }
        Some(JournalOwnerState::ReseedRequired { generation, .. })
            if generation == owner.account_generation =>
        {
            Err(LiveEventJournalError::ReseedRequired)
        }
        Some(JournalOwnerState::Reseeding { generation, .. })
            if generation == owner.account_generation =>
        {
            Err(LiveEventJournalError::OwnerTransitionIncomplete)
        }
        Some(JournalOwnerState::Active { generation, .. })
            if generation == owner.account_generation =>
        {
            Err(LiveEventJournalError::JournalRetired)
        }
        Some(_) => Err(LiveEventJournalError::OwnerGenerationMismatch),
        #[cfg(not(test))]
        None => Err(LiveEventJournalError::JournalRetired),
        #[cfg(test)]
        None => {
            if !authority.allows_test_auto_claim() {
                return Err(LiveEventJournalError::JournalRetired);
            }
            if owners.len() >= MAX_ACCOUNT_JOURNAL_FILES {
                return Err(LiveEventJournalError::StorageUnavailable);
            }
            let operation_token = new_process_token()?;
            owners.insert(
                owner.account_key.clone(),
                JournalOwnerState::Active {
                    generation: owner.account_generation,
                    operation_token,
                    needs_resync: false,
                    ambiguous_append: None,
                },
            );
            Ok((false, None))
        }
    }
}

fn authorize_active_ingress(
    owners: &HashMap<String, JournalOwnerState>,
    ingress: &LiveEventJournalIngressLease,
) -> Result<(bool, Option<AmbiguousAppend>), LiveEventJournalError> {
    match owners.get(&ingress.owner.account_key).copied() {
        Some(JournalOwnerState::Active {
            generation,
            operation_token,
            needs_resync,
            ambiguous_append,
        }) if generation == ingress.owner.account_generation
            && constant_time_token_eq(&operation_token, &ingress.operation_token) =>
        {
            Ok((needs_resync, ambiguous_append))
        }
        Some(JournalOwnerState::Active { generation, .. })
            if generation == ingress.owner.account_generation =>
        {
            Err(LiveEventJournalError::JournalReplaced)
        }
        Some(JournalOwnerState::TransitionIncomplete { generation, .. })
        | Some(JournalOwnerState::RolloverPending { generation, .. })
        | Some(JournalOwnerState::Reseeding { generation, .. })
            if generation == ingress.owner.account_generation =>
        {
            Err(LiveEventJournalError::OwnerTransitionIncomplete)
        }
        Some(JournalOwnerState::Retiring { generation, .. })
            if generation == ingress.owner.account_generation =>
        {
            Err(LiveEventJournalError::JournalRetired)
        }
        Some(JournalOwnerState::ReseedRequired { generation, .. })
            if generation == ingress.owner.account_generation =>
        {
            Err(LiveEventJournalError::ReseedRequired)
        }
        Some(_) => Err(LiveEventJournalError::OwnerGenerationMismatch),
        None => Err(LiveEventJournalError::JournalReplaced),
    }
}

fn authorize_clear<A: LiveEventJournalAuthority>(
    owners: &HashMap<String, JournalOwnerState>,
    authority: &A,
) -> Result<[u8; PROCESS_TOKEN_BYTES], LiveEventJournalError> {
    let owner = authority.journal_owner();
    match owners.get(&owner.account_key).copied() {
        Some(JournalOwnerState::Active {
            generation,
            operation_token,
            ..
        }) if generation == owner.account_generation
            && authority.matches_operation_token(&operation_token) =>
        {
            Ok(operation_token)
        }
        Some(JournalOwnerState::TransitionIncomplete {
            generation,
            operation_token,
        }) if generation == owner.account_generation
            && authority.matches_operation_token(&operation_token) =>
        {
            Ok(operation_token)
        }
        Some(JournalOwnerState::Retiring { generation, .. })
            if generation == owner.account_generation =>
        {
            Err(LiveEventJournalError::JournalRetired)
        }
        Some(JournalOwnerState::RolloverPending { generation, .. })
            if generation == owner.account_generation =>
        {
            Err(LiveEventJournalError::OwnerTransitionIncomplete)
        }
        Some(JournalOwnerState::Active { generation, .. })
        | Some(JournalOwnerState::TransitionIncomplete { generation, .. })
            if generation == owner.account_generation =>
        {
            Err(LiveEventJournalError::JournalRetired)
        }
        Some(_) => Err(LiveEventJournalError::OwnerGenerationMismatch),
        #[cfg(not(test))]
        None => Err(LiveEventJournalError::JournalRetired),
        #[cfg(test)]
        None if authority.allows_test_auto_claim() => new_process_token(),
        #[cfg(test)]
        None => Err(LiveEventJournalError::JournalRetired),
    }
}

fn authorize_rotation<A: LiveEventJournalAuthority>(
    owners: &HashMap<String, JournalOwnerState>,
    previous_authority: &A,
    current_owner: &LiveEventAccountOwner,
) -> Result<[u8; PROCESS_TOKEN_BYTES], LiveEventJournalError> {
    let previous_owner = previous_authority.journal_owner();
    match owners.get(&previous_owner.account_key).copied() {
        Some(JournalOwnerState::Active {
            generation,
            operation_token,
            ..
        }) if generation == previous_owner.account_generation
            && previous_authority.matches_operation_token(&operation_token) =>
        {
            Ok(operation_token)
        }
        Some(JournalOwnerState::TransitionIncomplete {
            generation,
            operation_token,
        }) if generation == current_owner.account_generation
            && previous_authority.matches_operation_token(&operation_token) =>
        {
            // Retrying the exact adjacent generation transition is the only
            // normal operation admitted while rotation is incomplete.
            Ok(operation_token)
        }
        Some(JournalOwnerState::Retiring { .. }) => Err(LiveEventJournalError::JournalRetired),
        Some(JournalOwnerState::RolloverPending { generation, .. })
            if generation == previous_owner.account_generation =>
        {
            Err(LiveEventJournalError::OwnerTransitionIncomplete)
        }
        Some(JournalOwnerState::Active { generation, .. })
            if generation == previous_owner.account_generation =>
        {
            Err(LiveEventJournalError::JournalRetired)
        }
        #[cfg(not(test))]
        None => Err(LiveEventJournalError::JournalRetired),
        #[cfg(test)]
        None if previous_authority.allows_test_auto_claim() => new_process_token(),
        #[cfg(test)]
        None => Err(LiveEventJournalError::JournalRetired),
        _ => Err(LiveEventJournalError::OwnerGenerationMismatch),
    }
}

fn mark_owner_indeterminate<A: LiveEventJournalAuthority>(
    owners: &mut HashMap<String, JournalOwnerState>,
    authority: &A,
) -> Result<(), LiveEventJournalError> {
    let owner = authority.journal_owner();
    match owners.get_mut(&owner.account_key) {
        Some(JournalOwnerState::Active {
            generation,
            operation_token,
            needs_resync,
            ambiguous_append,
        }) if *generation == owner.account_generation
            && authority.matches_operation_token(operation_token) =>
        {
            *needs_resync = true;
            *ambiguous_append = None;
            Ok(())
        }
        Some(JournalOwnerState::Active { generation, .. })
            if *generation == owner.account_generation =>
        {
            Err(LiveEventJournalError::JournalRetired)
        }
        _ => Err(LiveEventJournalError::OwnerGenerationMismatch),
    }
}

fn mark_ingress_owner_indeterminate(
    owners: &mut HashMap<String, JournalOwnerState>,
    ingress: &LiveEventJournalIngressLease,
    append: AmbiguousAppend,
) -> Result<(), LiveEventJournalError> {
    match owners.get_mut(&ingress.owner.account_key) {
        Some(JournalOwnerState::Active {
            generation,
            operation_token,
            needs_resync,
            ambiguous_append,
        }) if *generation == ingress.owner.account_generation
            && constant_time_token_eq(operation_token, &ingress.operation_token) =>
        {
            *needs_resync = true;
            *ambiguous_append = Some(append);
            Ok(())
        }
        Some(JournalOwnerState::Active { generation, .. })
            if *generation == ingress.owner.account_generation =>
        {
            Err(LiveEventJournalError::JournalReplaced)
        }
        _ => Err(LiveEventJournalError::OwnerGenerationMismatch),
    }
}

fn mark_owner_resynced<A: LiveEventJournalAuthority>(
    owners: &mut HashMap<String, JournalOwnerState>,
    authority: &A,
) -> Result<(), LiveEventJournalError> {
    let owner = authority.journal_owner();
    match owners.get_mut(&owner.account_key) {
        Some(JournalOwnerState::Active {
            generation,
            operation_token,
            needs_resync,
            ambiguous_append,
        }) if *generation == owner.account_generation
            && authority.matches_operation_token(operation_token) =>
        {
            *needs_resync = false;
            *ambiguous_append = None;
            Ok(())
        }
        Some(JournalOwnerState::Active { generation, .. })
            if *generation == owner.account_generation =>
        {
            Err(LiveEventJournalError::JournalRetired)
        }
        _ => Err(LiveEventJournalError::OwnerGenerationMismatch),
    }
}

fn mark_ingress_owner_resynced(
    owners: &mut HashMap<String, JournalOwnerState>,
    ingress: &LiveEventJournalIngressLease,
) -> Result<(), LiveEventJournalError> {
    match owners.get_mut(&ingress.owner.account_key) {
        Some(JournalOwnerState::Active {
            generation,
            operation_token,
            needs_resync,
            ambiguous_append,
        }) if *generation == ingress.owner.account_generation
            && constant_time_token_eq(operation_token, &ingress.operation_token) =>
        {
            *needs_resync = false;
            *ambiguous_append = None;
            Ok(())
        }
        Some(JournalOwnerState::Active { generation, .. })
            if *generation == ingress.owner.account_generation =>
        {
            Err(LiveEventJournalError::JournalReplaced)
        }
        _ => Err(LiveEventJournalError::OwnerGenerationMismatch),
    }
}

fn validate_header(
    header: &JournalHeader,
    owner: &LiveEventAccountOwner,
) -> Result<(), LiveEventJournalError> {
    if header.version != JOURNAL_FORMAT_VERSION
        || header.account_key != owner.account_key
        || header.account_key.len() != ACCOUNT_KEY_HEX_BYTES
        || !header.account_key.bytes().all(is_lower_hex)
    {
        return Err(LiveEventJournalError::StorageCorrupt);
    }
    LiveEventCursor::new(header.journal_id.clone(), 0)
        .validate()
        .map_err(|_| LiveEventJournalError::StorageCorrupt)?;
    if header.integrity.len() != ACCOUNT_KEY_HEX_BYTES
        || !header.integrity.bytes().all(is_lower_hex)
        || header.integrity != journal_header_integrity(header)?
        || header
            .event_ids
            .windows(2)
            .any(|records| records[0].event_id >= records[1].event_id)
    {
        return Err(LiveEventJournalError::StorageCorrupt);
    }
    Ok(())
}

fn validate_stored_entry<T: LiveReplayPayload>(
    entry: &StoredEntry<T>,
    previous_sequence: Option<u64>,
    limits: LiveEventJournalLimits,
) -> Result<(), LiveEventJournalError> {
    let expected_sequence = previous_sequence.and_then(|previous| previous.checked_add(1));
    let sequence_is_invalid = entry.sequence == 0
        || entry.sequence > MAX_CURSOR_SEQUENCE
        || previous_sequence.is_some() && expected_sequence != Some(entry.sequence);
    if sequence_is_invalid {
        return Err(LiveEventJournalError::StorageCorrupt);
    }
    validate_event_owner(&entry.session_id, entry.run_id.as_deref())
        .map_err(|_| LiveEventJournalError::StorageCorrupt)?;
    entry
        .payload
        .validate_live_replay_payload()
        .map_err(|_| LiveEventJournalError::StorageCorrupt)?;
    validate_event_id(entry.payload.live_replay_event_id())
        .map_err(|_| LiveEventJournalError::StorageCorrupt)?;
    if serialized_payload_bytes(&entry.payload)? > limits.max_payload_bytes {
        return Err(LiveEventJournalError::StorageCorrupt);
    }
    if entry.commitment.len() != ACCOUNT_KEY_HEX_BYTES
        || !entry.commitment.bytes().all(is_lower_hex)
        || entry.commitment
            != event_commitment(&entry.session_id, entry.run_id.as_deref(), &entry.payload)?
    {
        return Err(LiveEventJournalError::StorageCorrupt);
    }
    Ok(())
}

fn validate_stored_event_id(
    record: &StoredEventId,
    head_sequence: u64,
) -> Result<(), LiveEventJournalError> {
    validate_event_id(&record.event_id).map_err(|_| LiveEventJournalError::StorageCorrupt)?;
    if record.sequence == 0
        || record.sequence > head_sequence
        || record.commitment.len() != ACCOUNT_KEY_HEX_BYTES
        || !record.commitment.bytes().all(is_lower_hex)
    {
        return Err(LiveEventJournalError::StorageCorrupt);
    }
    Ok(())
}

fn stored_event_id<T: LiveReplayPayload>(
    entry: &StoredEntry<T>,
) -> Result<StoredEventId, LiveEventJournalError> {
    Ok(StoredEventId {
        event_id: entry.payload.live_replay_event_id().to_string(),
        sequence: entry.sequence,
        commitment: entry.commitment.clone(),
    })
}

fn encoded_event_id_bytes(record: &StoredEventId) -> Result<usize, LiveEventJournalError> {
    encode_record(record).map(|encoded| encoded.len())
}

fn validate_event_for_append<T: LiveReplayPayload>(
    session_id: &str,
    run_id: Option<&str>,
    payload: &T,
    limits: LiveEventJournalLimits,
) -> Result<usize, LiveEventJournalError> {
    validate_event_owner(session_id, run_id)?;
    payload.validate_live_replay_payload()?;
    validate_event_id(payload.live_replay_event_id())?;
    let payload_bytes = serialized_payload_bytes(payload)?;
    if payload_bytes > limits.max_payload_bytes {
        return Err(LiveEventJournalError::PayloadTooLarge);
    }
    Ok(payload_bytes)
}

fn event_commitment<T: Serialize>(
    session_id: &str,
    run_id: Option<&str>,
    payload: &T,
) -> Result<String, LiveEventJournalError> {
    #[derive(Serialize)]
    struct Commitment<'a, T> {
        session_id: &'a str,
        run_id: Option<&'a str>,
        payload: &'a T,
    }
    let encoded = serde_json::to_vec(&Commitment {
        session_id,
        run_id,
        payload,
    })
    .map_err(|_| LiveEventJournalError::StorageCorrupt)?;
    Ok(encode_hex(&Sha256::digest(encoded)))
}

fn ambiguous_append_identity(
    ingress: &LiveEventJournalIngressLease,
    expected_head: &LiveEventCursor,
    event_id: &str,
    event_commitment: &str,
) -> Result<AmbiguousAppend, LiveEventJournalError> {
    let expected_journal_id = decode_hex_array::<JOURNAL_ID_BYTES>(&expected_head.journal_id)?;
    if expected_journal_id != ingress.journal_id {
        return Err(LiveEventJournalError::JournalReplaced);
    }
    Ok(AmbiguousAppend {
        journal_id: ingress.journal_id,
        expected_sequence: expected_head.sequence,
        event_id_commitment: sha256_parts(AMBIGUOUS_EVENT_ID_DOMAIN, &[event_id.as_bytes()]),
        event_commitment: decode_hex_array::<32>(event_commitment)?,
    })
}

fn bytes_commitment(bytes: &[u8]) -> String {
    encode_hex(&Sha256::digest(bytes))
}

fn journal_header_integrity(header: &JournalHeader) -> Result<String, LiveEventJournalError> {
    let encoded = serde_json::to_vec(&JournalHeaderIntegrity {
        version: header.version,
        journal_id: &header.journal_id,
        account_key: &header.account_key,
        head_sequence: header.head_sequence,
        checkpoint: &header.checkpoint,
        event_ids: &header.event_ids,
    })
    .map_err(|_| LiveEventJournalError::StorageCorrupt)?;
    Ok(bytes_commitment(&encoded))
}

fn classify_account_event<T>(
    account: &AccountJournal<T>,
    event_id: &str,
    commitment: &str,
) -> Result<EventAdmission, LiveEventJournalError> {
    match account.event_ids.get(event_id) {
        Some(record) if record.commitment == commitment => Ok(EventAdmission::Duplicate {
            event_cursor: LiveEventCursor::new(account.journal_id.clone(), record.sequence),
            head_cursor: current_cursor(account),
        }),
        Some(_) => Err(LiveEventJournalError::EventIdConflict),
        None => Ok(EventAdmission::New),
    }
}

fn ensure_expected_head<T>(
    account: &AccountJournal<T>,
    expected_head: &LiveEventCursor,
) -> Result<(), LiveEventJournalError> {
    ensure_expected_journal(account, expected_head)?;
    ensure_expected_sequence(account, expected_head)
}

fn ensure_expected_journal<T>(
    account: &AccountJournal<T>,
    expected_head: &LiveEventCursor,
) -> Result<(), LiveEventJournalError> {
    if expected_head.journal_id == account.journal_id {
        Ok(())
    } else {
        Err(LiveEventJournalError::JournalReplaced)
    }
}

fn ensure_expected_sequence<T>(
    account: &AccountJournal<T>,
    expected_head: &LiveEventCursor,
) -> Result<(), LiveEventJournalError> {
    if expected_head.sequence == account.head_sequence {
        Ok(())
    } else {
        Err(LiveEventJournalError::HeadChanged)
    }
}

fn validate_event_owner(
    session_id: &str,
    run_id: Option<&str>,
) -> Result<(), LiveEventJournalError> {
    validate_nonempty_bounded(
        session_id,
        MAX_EVENT_OWNER_ID_BYTES,
        LiveEventJournalError::InvalidEventOwner,
    )?;
    if let Some(run_id) = run_id {
        validate_nonempty_bounded(
            run_id,
            MAX_EVENT_OWNER_ID_BYTES,
            LiveEventJournalError::InvalidEventOwner,
        )?;
    }
    Ok(())
}

fn validate_event_id(event_id: &str) -> Result<(), LiveEventJournalError> {
    validate_nonempty_bounded(
        event_id,
        MAX_EVENT_OWNER_ID_BYTES,
        LiveEventJournalError::InvalidEventOwner,
    )
}

fn validate_nonempty_bounded(
    value: &str,
    max_bytes: usize,
    error: LiveEventJournalError,
) -> Result<(), LiveEventJournalError> {
    if value.is_empty() || value.len() > max_bytes || value.chars().any(char::is_control) {
        Err(error)
    } else {
        Ok(())
    }
}

fn ensure_owner_generation<T>(
    account: &AccountJournal<T>,
    owner: &LiveEventAccountOwner,
) -> Result<(), LiveEventJournalError> {
    if account.account_generation == owner.account_generation {
        Ok(())
    } else {
        Err(LiveEventJournalError::OwnerGenerationMismatch)
    }
}

fn is_exact_reseed_replacement<T: LiveReplayPayload>(
    account: &AccountJournal<T>,
    obligation: &LiveEventJournalReseedObligation,
) -> bool {
    account.journal_id == obligation.new_journal_id
        && account.head_sequence == 0
        && account.entries.is_empty()
        && account.total_payload_bytes == 0
        && account.event_ids.is_empty()
        && account.event_id_metadata_bytes == 0
        && account.checkpoint.as_ref().is_some_and(|checkpoint| {
            checkpoint.schema == CHECKPOINT_SCHEMA
                && checkpoint.through_sequence == 0
                && checkpoint.bytes.as_slice() == obligation.projection_bytes.as_ref()
                && checkpoint.commitment == bytes_commitment(&checkpoint.bytes)
                && constant_time_digest_eq(
                    &projection_digest(&checkpoint.bytes),
                    &obligation.projection_digest,
                )
        })
}

fn is_exact_rollover_source<T>(
    account: &AccountJournal<T>,
    obligation: &LiveEventJournalRolloverObligation,
    bytes: &[u8],
) -> bool {
    account.journal_id == obligation.journal_id
        && account.head_sequence == obligation.head_sequence
        && account.checkpoint.as_ref().is_some_and(|checkpoint| {
            checkpoint.schema == CHECKPOINT_SCHEMA
                && checkpoint.through_sequence == obligation.head_sequence
                && checkpoint.bytes == bytes
                && checkpoint.commitment == encode_hex(&obligation.checkpoint_commitment)
        })
}

fn is_exact_rollover_replacement<T>(
    account: &AccountJournal<T>,
    obligation: &LiveEventJournalRolloverObligation,
    bytes: &[u8],
) -> bool {
    account.journal_id == obligation.new_journal_id
        && account.head_sequence == 0
        && account.entries.is_empty()
        && account.total_payload_bytes == 0
        && account.event_ids.is_empty()
        && account.event_id_metadata_bytes == 0
        && account.checkpoint.as_ref().is_some_and(|checkpoint| {
            checkpoint.schema == CHECKPOINT_SCHEMA
                && checkpoint.through_sequence == 0
                && checkpoint.bytes == bytes
                && checkpoint.commitment == encode_hex(&obligation.checkpoint_commitment)
        })
}

fn current_cursor<T>(account: &AccountJournal<T>) -> LiveEventCursor {
    LiveEventCursor::new(account.journal_id.clone(), account.head_sequence)
}

fn trim_retention<T: LiveReplayPayload>(
    entries: &mut VecDeque<StoredEntry<T>>,
    total_payload_bytes: &mut usize,
    limits: LiveEventJournalLimits,
    evict_through: u64,
) -> Result<(), LiveEventJournalError> {
    while entries.len() > limits.max_entries
        || *total_payload_bytes > limits.max_total_payload_bytes
    {
        if entries
            .front()
            .is_none_or(|entry| entry.sequence > evict_through)
        {
            break;
        }
        let removed = entries
            .pop_front()
            .ok_or(LiveEventJournalError::StorageCorrupt)?;
        let removed_bytes = serialized_payload_bytes(&removed.payload)?;
        *total_payload_bytes = total_payload_bytes
            .checked_sub(removed_bytes)
            .ok_or(LiveEventJournalError::StorageCorrupt)?;
    }
    Ok(())
}

fn trim_compaction_low_watermark<T: LiveReplayPayload>(
    entries: &mut VecDeque<StoredEntry<T>>,
    total_payload_bytes: &mut usize,
    limits: LiveEventJournalLimits,
    evict_through: u64,
) -> Result<(), LiveEventJournalError> {
    let entry_low_watermark = if limits.max_entries < 4 {
        limits.max_entries
    } else {
        limits.max_entries.saturating_mul(3) / 4
    };
    let newest_payload_bytes = entries
        .back()
        .map(|entry| serialized_payload_bytes(&entry.payload))
        .transpose()?
        .unwrap_or_default();
    let payload_low_watermark = limits
        .max_total_payload_bytes
        .saturating_mul(3)
        .checked_div(4)
        .unwrap_or_default()
        .max(newest_payload_bytes);
    while entries.len() > 1
        && (entries.len() > entry_low_watermark || *total_payload_bytes > payload_low_watermark)
    {
        if entries
            .front()
            .is_none_or(|entry| entry.sequence > evict_through)
        {
            break;
        }
        let removed = entries
            .pop_front()
            .ok_or(LiveEventJournalError::StorageCorrupt)?;
        let removed_bytes = serialized_payload_bytes(&removed.payload)?;
        *total_payload_bytes = total_payload_bytes
            .checked_sub(removed_bytes)
            .ok_or(LiveEventJournalError::StorageCorrupt)?;
    }
    Ok(())
}

fn serialized_payload_bytes<T: Serialize>(payload: &T) -> Result<usize, LiveEventJournalError> {
    serialized_payload(payload).map(|encoded| encoded.len())
}

fn serialized_payload<T: Serialize>(payload: &T) -> Result<Vec<u8>, LiveEventJournalError> {
    serde_json::to_vec(payload).map_err(|_| LiveEventJournalError::StorageCorrupt)
}

fn encode_record<T: Serialize>(value: &T) -> Result<Vec<u8>, LiveEventJournalError> {
    let mut encoded =
        serde_json::to_vec(value).map_err(|_| LiveEventJournalError::StorageCorrupt)?;
    encoded.push(b'\n');
    Ok(encoded)
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct V3DiskIdentity {
    account_key: String,
    journal_id: String,
    committed_head_sequence: u64,
    committed_end: u64,
    anchor: DiskAnchor,
}

struct EncodedV3Journal {
    bytes: Vec<u8>,
    anchor: DiskAnchor,
}

fn sha256_parts(domain: &[u8], parts: &[&[u8]]) -> [u8; 32] {
    let mut hasher = Sha256::new();
    hasher.update(domain);
    for part in parts {
        hasher.update(part);
    }
    let digest = hasher.finalize();
    let mut result = [0u8; 32];
    result.copy_from_slice(&digest);
    result
}

fn observed_file_digest(length: u64, bytes: &[u8]) -> [u8; 32] {
    sha256_parts(OBSERVED_FILE_DIGEST_DOMAIN, &[&length.to_le_bytes(), bytes])
}

fn projection_digest(bytes: &[u8]) -> [u8; 32] {
    sha256_parts(PROJECTION_DIGEST_DOMAIN, &[bytes])
}

fn constant_time_digest_eq(left: &[u8; 32], right: &[u8; 32]) -> bool {
    left.iter()
        .zip(right)
        .fold(0u8, |difference, (left, right)| difference | (left ^ right))
        == 0
}

fn new_file_nonce() -> Result<[u8; 16], LiveEventJournalError> {
    let mut nonce = [0u8; 16];
    fill_random(&mut nonce).map_err(|_| LiveEventJournalError::StorageUnavailable)?;
    Ok(nonce)
}

fn put_u32(target: &mut [u8], offset: usize, value: u32) {
    target[offset..offset + 4].copy_from_slice(&value.to_le_bytes());
}

fn put_u64(target: &mut [u8], offset: usize, value: u64) {
    target[offset..offset + 8].copy_from_slice(&value.to_le_bytes());
}

fn get_u32(source: &[u8], offset: usize) -> Result<u32, LiveEventJournalError> {
    let bytes = source
        .get(offset..offset + 4)
        .ok_or(LiveEventJournalError::StorageCorrupt)?;
    Ok(u32::from_le_bytes(
        bytes
            .try_into()
            .map_err(|_| LiveEventJournalError::StorageCorrupt)?,
    ))
}

fn get_u64(source: &[u8], offset: usize) -> Result<u64, LiveEventJournalError> {
    let bytes = source
        .get(offset..offset + 8)
        .ok_or(LiveEventJournalError::StorageCorrupt)?;
    Ok(u64::from_le_bytes(
        bytes
            .try_into()
            .map_err(|_| LiveEventJournalError::StorageCorrupt)?,
    ))
}

fn array_at<const N: usize>(
    source: &[u8],
    offset: usize,
) -> Result<[u8; N], LiveEventJournalError> {
    source
        .get(offset..offset + N)
        .ok_or(LiveEventJournalError::StorageCorrupt)?
        .try_into()
        .map_err(|_| LiveEventJournalError::StorageCorrupt)
}

fn decode_hex_array<const N: usize>(value: &str) -> Result<[u8; N], LiveEventJournalError> {
    if value.len() != N * 2 || !value.bytes().all(is_lower_hex) {
        return Err(LiveEventJournalError::StorageCorrupt);
    }
    let mut decoded = [0u8; N];
    for (index, pair) in value.as_bytes().chunks_exact(2).enumerate() {
        let high = decode_hex_nibble(pair[0]).ok_or(LiveEventJournalError::StorageCorrupt)?;
        let low = decode_hex_nibble(pair[1]).ok_or(LiveEventJournalError::StorageCorrupt)?;
        decoded[index] = (high << 4) | low;
    }
    Ok(decoded)
}

fn decode_hex_nibble(byte: u8) -> Option<u8> {
    match byte {
        b'0'..=b'9' => Some(byte - b'0'),
        b'a'..=b'f' => Some(byte - b'a' + 10),
        _ => None,
    }
}

fn encode_v3_superblock(file_nonce: &[u8; 16]) -> [u8; DISK_SUPERBLOCK_BYTES] {
    let mut encoded = [0u8; DISK_SUPERBLOCK_BYTES];
    encoded[0..8].copy_from_slice(DISK_SUPERBLOCK_MAGIC);
    put_u32(&mut encoded, 8, DISK_SUPERBLOCK_VERSION);
    put_u32(&mut encoded, 12, DISK_SUPERBLOCK_BYTES_U32);
    put_u32(&mut encoded, 16, DISK_PREFIX_BYTES_U32);
    put_u32(&mut encoded, 20, DISK_ANCHOR_SLOT_BYTES_U32);
    encoded[24..40].copy_from_slice(file_nonce);
    let checksum = sha256_parts(
        DISK_SUPERBLOCK_CHECKSUM_DOMAIN,
        &[&encoded[..DISK_SUPERBLOCK_HASHED_BYTES]],
    );
    encoded[DISK_SUPERBLOCK_HASHED_BYTES..DISK_SUPERBLOCK_BYTES].copy_from_slice(&checksum);
    encoded
}

fn decode_v3_superblock(bytes: &[u8]) -> Result<[u8; 16], LiveEventJournalError> {
    let superblock = bytes
        .get(..DISK_SUPERBLOCK_BYTES)
        .ok_or(LiveEventJournalError::StorageCorrupt)?;
    if superblock.get(0..8) != Some(DISK_SUPERBLOCK_MAGIC.as_slice())
        || get_u32(superblock, 8)? != DISK_SUPERBLOCK_VERSION
        || get_u32(superblock, 12)? != DISK_SUPERBLOCK_BYTES_U32
        || get_u32(superblock, 16)? != DISK_PREFIX_BYTES_U32
        || get_u32(superblock, 20)? != DISK_ANCHOR_SLOT_BYTES_U32
        || superblock[40..48].iter().any(|byte| *byte != 0)
    {
        return Err(LiveEventJournalError::StorageCorrupt);
    }
    let expected = sha256_parts(
        DISK_SUPERBLOCK_CHECKSUM_DOMAIN,
        &[&superblock[..DISK_SUPERBLOCK_HASHED_BYTES]],
    );
    if superblock[DISK_SUPERBLOCK_HASHED_BYTES..DISK_SUPERBLOCK_BYTES] != expected {
        return Err(LiveEventJournalError::StorageCorrupt);
    }
    array_at(superblock, 24)
}

fn encode_v3_anchor(
    anchor: &DiskAnchor,
) -> Result<[u8; DISK_ANCHOR_SLOT_BYTES], LiveEventJournalError> {
    if usize::from(anchor.slot_index) >= DISK_ANCHOR_SLOT_COUNT
        || anchor.slot_index != (anchor.revision % DISK_ANCHOR_SLOT_COUNT as u64) as u8
    {
        return Err(LiveEventJournalError::StorageCorrupt);
    }
    let journal_id = decode_hex_array::<JOURNAL_ID_BYTES>(&anchor.journal_id)?;
    let account_key = decode_hex_array::<32>(&anchor.account_key)?;
    let mut encoded = [0u8; DISK_ANCHOR_SLOT_BYTES];
    encoded[0..8].copy_from_slice(DISK_ANCHOR_MAGIC);
    put_u32(&mut encoded, 8, DISK_ANCHOR_VERSION);
    encoded[12] = anchor.slot_index;
    put_u64(&mut encoded, 16, anchor.revision);
    encoded[24..40].copy_from_slice(&anchor.file_nonce);
    encoded[40..56].copy_from_slice(&journal_id);
    encoded[56..88].copy_from_slice(&account_key);
    put_u64(&mut encoded, 88, anchor.snapshot_offset);
    put_u64(&mut encoded, 96, anchor.snapshot_len);
    put_u64(&mut encoded, 104, anchor.data_start);
    put_u64(&mut encoded, 112, anchor.committed_end);
    put_u64(&mut encoded, 120, anchor.snapshot_head_sequence);
    put_u64(&mut encoded, 128, anchor.committed_head_sequence);
    put_u64(&mut encoded, 136, anchor.committed_frame_count);
    encoded[144..176].copy_from_slice(&anchor.snapshot_hash);
    encoded[176..208].copy_from_slice(&anchor.committed_chain_hash);
    let checksum = sha256_parts(
        DISK_ANCHOR_CHECKSUM_DOMAIN,
        &[&encoded[..DISK_ANCHOR_HASHED_BYTES]],
    );
    encoded[DISK_ANCHOR_HASHED_BYTES..DISK_ANCHOR_SLOT_BYTES].copy_from_slice(&checksum);
    Ok(encoded)
}

fn decode_v3_anchor_slot(
    bytes: &[u8],
    slot_index: u8,
    file_nonce: &[u8; 16],
) -> Result<Option<DiskAnchor>, LiveEventJournalError> {
    let offset = DISK_SUPERBLOCK_BYTES
        .checked_add(usize::from(slot_index) * DISK_ANCHOR_SLOT_BYTES)
        .ok_or(LiveEventJournalError::StorageCorrupt)?;
    let slot = bytes
        .get(offset..offset + DISK_ANCHOR_SLOT_BYTES)
        .ok_or(LiveEventJournalError::StorageCorrupt)?;
    if slot.iter().all(|byte| *byte == 0) {
        return Ok(None);
    }
    let expected = sha256_parts(
        DISK_ANCHOR_CHECKSUM_DOMAIN,
        &[&slot[..DISK_ANCHOR_HASHED_BYTES]],
    );
    if slot[DISK_ANCHOR_HASHED_BYTES..] != expected {
        // A nonzero slot proves that an in-place anchor write took effect, but
        // a bad checksum cannot distinguish a pre-acknowledgement tear from
        // post-acknowledgement damage. Never reinterpret it as an absent slot
        // and silently roll back to the other anchor.
        return Err(LiveEventJournalError::StorageCorrupt);
    }
    if slot.get(0..8) != Some(DISK_ANCHOR_MAGIC.as_slice())
        || get_u32(slot, 8)? != DISK_ANCHOR_VERSION
        || slot[12] != slot_index
        || slot[13..16].iter().any(|byte| *byte != 0)
        || slot[208..224].iter().any(|byte| *byte != 0)
    {
        return Err(LiveEventJournalError::StorageCorrupt);
    }
    let revision = get_u64(slot, 16)?;
    if slot_index != (revision % DISK_ANCHOR_SLOT_COUNT as u64) as u8 {
        return Err(LiveEventJournalError::StorageCorrupt);
    }
    let stored_nonce = array_at::<16>(slot, 24)?;
    if &stored_nonce != file_nonce {
        return Err(LiveEventJournalError::StorageCorrupt);
    }
    let anchor = DiskAnchor {
        slot_index,
        revision,
        file_nonce: stored_nonce,
        journal_id: encode_hex(&array_at::<JOURNAL_ID_BYTES>(slot, 40)?),
        account_key: encode_hex(&array_at::<32>(slot, 56)?),
        snapshot_offset: get_u64(slot, 88)?,
        snapshot_len: get_u64(slot, 96)?,
        data_start: get_u64(slot, 104)?,
        committed_end: get_u64(slot, 112)?,
        snapshot_head_sequence: get_u64(slot, 120)?,
        committed_head_sequence: get_u64(slot, 128)?,
        committed_frame_count: get_u64(slot, 136)?,
        snapshot_hash: array_at(slot, 144)?,
        committed_chain_hash: array_at(slot, 176)?,
    };
    validate_v3_anchor_geometry(&anchor)?;
    Ok(Some(anchor))
}

fn validate_v3_anchor_geometry(anchor: &DiskAnchor) -> Result<(), LiveEventJournalError> {
    let prefix =
        u64::try_from(DISK_PREFIX_BYTES).map_err(|_| LiveEventJournalError::StorageCorrupt)?;
    let expected_data_start = anchor
        .snapshot_offset
        .checked_add(anchor.snapshot_len)
        .ok_or(LiveEventJournalError::StorageCorrupt)?;
    if anchor.snapshot_offset != prefix
        || anchor.snapshot_len == 0
        || anchor.snapshot_len
            > u64::try_from(MAX_HEADER_BYTES).map_err(|_| LiveEventJournalError::StorageCorrupt)?
        || anchor.data_start != expected_data_start
        || anchor.committed_end < anchor.data_start
        || anchor.snapshot_head_sequence > anchor.committed_head_sequence
        || anchor.committed_head_sequence > MAX_CURSOR_SEQUENCE
        || anchor.committed_head_sequence > MAX_IDEMPOTENCY_EVENT_IDS as u64
        || anchor.committed_frame_count > MAX_CURSOR_SEQUENCE
    {
        return Err(LiveEventJournalError::StorageCorrupt);
    }
    LiveEventCursor::new(anchor.journal_id.clone(), anchor.committed_head_sequence)
        .validate()
        .map_err(|_| LiveEventJournalError::StorageCorrupt)?;
    if anchor.account_key.len() != ACCOUNT_KEY_HEX_BYTES
        || !anchor.account_key.bytes().all(is_lower_hex)
    {
        return Err(LiveEventJournalError::StorageCorrupt);
    }
    Ok(())
}

fn select_v3_anchor(bytes: &[u8]) -> Result<DiskAnchor, LiveEventJournalError> {
    let file_nonce = decode_v3_superblock(bytes)?;
    let first = decode_v3_anchor_slot(bytes, 0, &file_nonce)?;
    let second = decode_v3_anchor_slot(bytes, 1, &file_nonce)?;
    match (first, second) {
        (None, None) => Err(LiveEventJournalError::StorageCorrupt),
        (Some(anchor), None) if anchor.revision == 0 && anchor.slot_index == 0 => Ok(anchor),
        (Some(_), None) | (None, Some(_)) => Err(LiveEventJournalError::StorageCorrupt),
        (Some(left), Some(right)) => {
            let (older, newer) = if left.revision < right.revision {
                (&left, &right)
            } else if right.revision < left.revision {
                (&right, &left)
            } else {
                return Err(LiveEventJournalError::StorageCorrupt);
            };
            if newer.revision != older.revision.saturating_add(1)
                || newer.file_nonce != older.file_nonce
                || newer.journal_id != older.journal_id
                || newer.account_key != older.account_key
                || newer.snapshot_offset != older.snapshot_offset
                || newer.snapshot_len != older.snapshot_len
                || newer.data_start != older.data_start
                || newer.snapshot_head_sequence != older.snapshot_head_sequence
                || newer.snapshot_hash != older.snapshot_hash
                || newer.committed_head_sequence != older.committed_head_sequence.saturating_add(1)
                || newer.committed_frame_count != older.committed_frame_count.saturating_add(1)
                || newer.committed_end <= older.committed_end
            {
                return Err(LiveEventJournalError::StorageCorrupt);
            }
            Ok(newer.clone())
        }
    }
}

/// Payload-independent identity read for startup retirement and ownership
/// reconciliation. It proves the v3 superblock and selected terminal anchor,
/// including that the anchored end exists, without deserializing `T`. Full
/// journal loading must still verify the snapshot and frame hash chain.
fn read_v3_disk_identity(
    path: &Path,
    max_disk_bytes: u64,
) -> Result<V3DiskIdentity, LiveEventJournalError> {
    let mut file = open_read_no_follow(path)?;
    let metadata = file
        .metadata()
        .map_err(|_| LiveEventJournalError::StorageUnavailable)?;
    if !metadata.file_type().is_file()
        || metadata.len() > max_disk_bytes
        || metadata.len()
            < u64::try_from(DISK_PREFIX_BYTES).map_err(|_| LiveEventJournalError::StorageCorrupt)?
    {
        return Err(LiveEventJournalError::StorageCorrupt);
    }
    let mut prefix = vec![0u8; DISK_PREFIX_BYTES];
    file.read_exact(&mut prefix).map_err(|error| {
        if error.kind() == ErrorKind::UnexpectedEof {
            LiveEventJournalError::StorageCorrupt
        } else {
            LiveEventJournalError::StorageUnavailable
        }
    })?;
    let anchor = select_v3_anchor(&prefix)?;
    if anchor.committed_end != metadata.len() || anchor.committed_end > max_disk_bytes {
        return Err(LiveEventJournalError::StorageCorrupt);
    }
    Ok(V3DiskIdentity {
        account_key: anchor.account_key.clone(),
        journal_id: anchor.journal_id.clone(),
        committed_head_sequence: anchor.committed_head_sequence,
        committed_end: anchor.committed_end,
        anchor,
    })
}

fn validate_retirement_identity(
    path: &Path,
    token: &LiveEventJournalRetirementToken,
    max_disk_bytes: u64,
) -> Result<(), LiveEventJournalError> {
    let metadata = path
        .symlink_metadata()
        .map_err(|_| LiveEventJournalError::StorageUnavailable)?;
    if !metadata.file_type().is_file()
        || metadata.file_type().is_symlink()
        || !metadata_owned_by_effective_user(&metadata)
    {
        return Err(LiveEventJournalError::StorageCorrupt);
    }
    let identity = read_v3_disk_identity(path, max_disk_bytes)?;
    if identity.account_key != token.owner.account_key
        || identity.journal_id != token.journal_id
        || identity.committed_head_sequence != token.head_sequence
        || identity.committed_end != metadata.len()
    {
        return Err(LiveEventJournalError::StorageCorrupt);
    }
    Ok(())
}

fn v3_chain_base(anchor: &DiskAnchor) -> Result<[u8; 32], LiveEventJournalError> {
    let journal_id = decode_hex_array::<JOURNAL_ID_BYTES>(&anchor.journal_id)?;
    let account_key = decode_hex_array::<32>(&anchor.account_key)?;
    Ok(sha256_parts(
        DISK_CHAIN_BASE_DOMAIN,
        &[
            &anchor.file_nonce,
            &journal_id,
            &account_key,
            &anchor.snapshot_hash,
            &anchor.data_start.to_le_bytes(),
        ],
    ))
}

fn encode_v3_frame<T: Serialize>(
    entry: &StoredEntry<T>,
    previous_hash: &[u8; 32],
) -> Result<(Vec<u8>, [u8; 32]), LiveEventJournalError> {
    let body = serde_json::to_vec(entry).map_err(|_| LiveEventJournalError::StorageCorrupt)?;
    let body_len = u32::try_from(body.len()).map_err(|_| LiveEventJournalError::StorageCorrupt)?;
    let mut header = [0u8; DISK_FRAME_HEADER_BYTES];
    header[0..4].copy_from_slice(DISK_FRAME_MAGIC);
    header[4] = DISK_FRAME_VERSION;
    put_u32(&mut header, 8, body_len);
    put_u32(&mut header, 12, !body_len);
    put_u64(&mut header, 16, entry.sequence);
    header[24..56].copy_from_slice(previous_hash);
    let frame_hash = sha256_parts(DISK_FRAME_HASH_DOMAIN, &[&header[..56], &body]);
    header[56..88].copy_from_slice(&frame_hash);
    let mut encoded = Vec::with_capacity(DISK_FRAME_HEADER_BYTES + body.len());
    encoded.extend_from_slice(&header);
    encoded.extend_from_slice(&body);
    Ok((encoded, frame_hash))
}

fn decode_v3_frame<T: DeserializeOwned>(
    bytes: &[u8],
    offset: usize,
    committed_end: usize,
    previous_hash: &[u8; 32],
    max_body_bytes: usize,
) -> Result<(StoredEntry<T>, usize, [u8; 32]), LiveEventJournalError> {
    let header_end = offset
        .checked_add(DISK_FRAME_HEADER_BYTES)
        .ok_or(LiveEventJournalError::StorageCorrupt)?;
    let header = bytes
        .get(offset..header_end)
        .filter(|_| header_end <= committed_end)
        .ok_or(LiveEventJournalError::StorageCorrupt)?;
    if header.get(0..4) != Some(DISK_FRAME_MAGIC.as_slice())
        || header[4] != DISK_FRAME_VERSION
        || header[5..8].iter().any(|byte| *byte != 0)
    {
        return Err(LiveEventJournalError::StorageCorrupt);
    }
    let body_len = get_u32(header, 8)?;
    if get_u32(header, 12)? != !body_len
        || usize::try_from(body_len).is_err()
        || usize::try_from(body_len).is_ok_and(|length| length > max_body_bytes)
    {
        return Err(LiveEventJournalError::StorageCorrupt);
    }
    let stored_previous = array_at::<32>(header, 24)?;
    if &stored_previous != previous_hash {
        return Err(LiveEventJournalError::StorageCorrupt);
    }
    let body_end = header_end
        .checked_add(usize::try_from(body_len).map_err(|_| LiveEventJournalError::StorageCorrupt)?)
        .ok_or(LiveEventJournalError::StorageCorrupt)?;
    let body = bytes
        .get(header_end..body_end)
        .filter(|_| body_end <= committed_end)
        .ok_or(LiveEventJournalError::StorageCorrupt)?;
    let expected_hash = sha256_parts(DISK_FRAME_HASH_DOMAIN, &[&header[..56], body]);
    let stored_hash = array_at::<32>(header, 56)?;
    if stored_hash != expected_hash {
        return Err(LiveEventJournalError::StorageCorrupt);
    }
    let entry: StoredEntry<T> =
        serde_json::from_slice(body).map_err(|_| LiveEventJournalError::StorageCorrupt)?;
    if entry.sequence != get_u64(header, 16)? {
        return Err(LiveEventJournalError::StorageCorrupt);
    }
    Ok((entry, body_end, stored_hash))
}

fn encode_v3_journal<T: Serialize>(
    header: &JournalHeader,
    entries: &VecDeque<StoredEntry<T>>,
    file_nonce: [u8; 16],
) -> Result<EncodedV3Journal, LiveEventJournalError> {
    let snapshot = serde_json::to_vec(header).map_err(|_| LiveEventJournalError::StorageCorrupt)?;
    if snapshot.is_empty() || snapshot.len() > MAX_HEADER_BYTES {
        return Err(LiveEventJournalError::StorageUnavailable);
    }
    let snapshot_offset =
        u64::try_from(DISK_PREFIX_BYTES).map_err(|_| LiveEventJournalError::StorageUnavailable)?;
    let snapshot_len =
        u64::try_from(snapshot.len()).map_err(|_| LiveEventJournalError::StorageUnavailable)?;
    let data_start = snapshot_offset
        .checked_add(snapshot_len)
        .ok_or(LiveEventJournalError::StorageUnavailable)?;
    let snapshot_hash = sha256_parts(DISK_SNAPSHOT_HASH_DOMAIN, &[&snapshot]);
    let mut anchor = DiskAnchor {
        slot_index: 0,
        revision: 0,
        file_nonce,
        journal_id: header.journal_id.clone(),
        account_key: header.account_key.clone(),
        snapshot_offset,
        snapshot_len,
        data_start,
        committed_end: data_start,
        snapshot_head_sequence: header.head_sequence,
        committed_head_sequence: header.head_sequence,
        committed_frame_count: 0,
        snapshot_hash,
        committed_chain_hash: [0u8; 32],
    };
    let mut chain_hash = v3_chain_base(&anchor)?;
    let mut frames = Vec::new();
    for entry in entries {
        let (encoded, frame_hash) = encode_v3_frame(entry, &chain_hash)?;
        frames
            .len()
            .checked_add(encoded.len())
            .ok_or(LiveEventJournalError::StorageUnavailable)?;
        frames.extend_from_slice(&encoded);
        chain_hash = frame_hash;
    }
    anchor.committed_frame_count =
        u64::try_from(entries.len()).map_err(|_| LiveEventJournalError::StorageUnavailable)?;
    anchor.committed_end = data_start
        .checked_add(
            u64::try_from(frames.len()).map_err(|_| LiveEventJournalError::StorageUnavailable)?,
        )
        .ok_or(LiveEventJournalError::StorageUnavailable)?;
    anchor.committed_chain_hash = chain_hash;
    let superblock = encode_v3_superblock(&file_nonce);
    let anchor_slot = encode_v3_anchor(&anchor)?;
    let total_len = usize::try_from(anchor.committed_end)
        .map_err(|_| LiveEventJournalError::StorageUnavailable)?;
    let mut bytes = Vec::with_capacity(total_len);
    bytes.extend_from_slice(&superblock);
    bytes.extend_from_slice(&anchor_slot);
    bytes.resize(DISK_PREFIX_BYTES, 0);
    bytes.extend_from_slice(&snapshot);
    bytes.extend_from_slice(&frames);
    if bytes.len() != total_len {
        return Err(LiveEventJournalError::StorageCorrupt);
    }
    Ok(EncodedV3Journal { bytes, anchor })
}

mod base64_bytes {
    use base64::{engine::general_purpose::STANDARD, Engine as _};
    use serde::{Deserialize, Deserializer, Serializer};

    pub(super) fn serialize<S>(bytes: &[u8], serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_str(&STANDARD.encode(bytes))
    }

    pub(super) fn deserialize<'de, D>(deserializer: D) -> Result<Vec<u8>, D::Error>
    where
        D: Deserializer<'de>,
    {
        let encoded = String::deserialize(deserializer)?;
        STANDARD.decode(encoded).map_err(serde::de::Error::custom)
    }
}

fn new_journal_id() -> Result<String, LiveEventJournalError> {
    let mut bytes = [0u8; JOURNAL_ID_BYTES];
    fill_random(&mut bytes).map_err(|_| LiveEventJournalError::StorageUnavailable)?;
    Ok(encode_hex(&bytes))
}

fn new_process_token() -> Result<[u8; PROCESS_TOKEN_BYTES], LiveEventJournalError> {
    loop {
        let mut bytes = [0u8; PROCESS_TOKEN_BYTES];
        fill_random(&mut bytes).map_err(|_| LiveEventJournalError::StorageUnavailable)?;
        if bytes.iter().any(|byte| *byte != 0) {
            return Ok(bytes);
        }
    }
}

fn constant_time_token_eq(
    left: &[u8; PROCESS_TOKEN_BYTES],
    right: &[u8; PROCESS_TOKEN_BYTES],
) -> bool {
    left.iter()
        .zip(right)
        .fold(0u8, |difference, (left, right)| difference | (left ^ right))
        == 0
}

fn encode_hex(bytes: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut encoded = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        encoded.push(char::from(HEX[usize::from(byte >> 4)]));
        encoded.push(char::from(HEX[usize::from(byte & 0x0f)]));
    }
    encoded
}

fn is_lower_hex(byte: u8) -> bool {
    byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum AccountFileState {
    Present,
    Missing,
}

fn account_file_state(path: &Path) -> Result<AccountFileState, LiveEventJournalError> {
    match fs::symlink_metadata(path) {
        Ok(_) => Ok(AccountFileState::Present),
        Err(error) if error.kind() == ErrorKind::NotFound => Ok(AccountFileState::Missing),
        Err(_) => Err(LiveEventJournalError::StorageUnavailable),
    }
}

fn is_account_journal_file_name(file_name: &str) -> bool {
    account_key_from_journal_file_name(file_name).is_some()
}

fn account_key_from_journal_file_name(file_name: &str) -> Option<&str> {
    file_name.strip_suffix(".events").filter(|account_key| {
        account_key.len() == ACCOUNT_KEY_HEX_BYTES && account_key.bytes().all(is_lower_hex)
    })
}

fn parse_retiring_file_name(file_name: &str) -> Option<(String, [u8; PROCESS_TOKEN_BYTES])> {
    let suffix = file_name.strip_prefix(RETIRING_FILE_PREFIX)?;
    let separator = suffix.get(ACCOUNT_KEY_HEX_BYTES..ACCOUNT_KEY_HEX_BYTES + 1)?;
    if separator != "-" {
        return None;
    }
    let account_key = suffix.get(..ACCOUNT_KEY_HEX_BYTES)?;
    let nonce = suffix.get(ACCOUNT_KEY_HEX_BYTES + 1..)?;
    if account_key.len() != ACCOUNT_KEY_HEX_BYTES
        || !account_key.bytes().all(is_lower_hex)
        || nonce.len() != PROCESS_TOKEN_HEX_BYTES
        || !nonce.bytes().all(is_lower_hex)
    {
        return None;
    }
    let nonce = decode_hex_array::<PROCESS_TOKEN_BYTES>(nonce).ok()?;
    if nonce.iter().all(|byte| *byte == 0) {
        return None;
    }
    Some((account_key.to_string(), nonce))
}

fn open_read_no_follow(path: &Path) -> Result<File, LiveEventJournalError> {
    let mut options = OpenOptions::new();
    options.read(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.custom_flags(libc::O_NOFOLLOW);
    }
    options
        .open(path)
        .map_err(|_| LiveEventJournalError::StorageUnavailable)
}

fn open_append_no_follow(path: &Path) -> Result<File, LiveEventJournalError> {
    let mut options = OpenOptions::new();
    options.append(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.custom_flags(libc::O_NOFOLLOW);
    }
    options
        .open(path)
        .map_err(|_| LiveEventJournalError::StorageUnavailable)
}

fn open_read_write_no_follow(path: &Path) -> Result<File, LiveEventJournalError> {
    let mut options = OpenOptions::new();
    options.read(true).write(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.custom_flags(libc::O_NOFOLLOW);
    }
    options
        .open(path)
        .map_err(|_| LiveEventJournalError::StorageUnavailable)
}

#[cfg(unix)]
fn write_all_at(
    file: &File,
    mut bytes: &[u8],
    mut offset: u64,
) -> Result<(), LiveEventJournalError> {
    use std::os::unix::fs::FileExt;

    while !bytes.is_empty() {
        match file.write_at(bytes, offset) {
            Ok(0) => return Err(LiveEventJournalError::StorageUnavailable),
            Ok(written) => {
                bytes = &bytes[written..];
                offset = offset
                    .checked_add(
                        u64::try_from(written)
                            .map_err(|_| LiveEventJournalError::StorageUnavailable)?,
                    )
                    .ok_or(LiveEventJournalError::StorageUnavailable)?;
            }
            Err(error) if error.kind() == ErrorKind::Interrupted => {}
            Err(_) => return Err(LiveEventJournalError::StorageUnavailable),
        }
    }
    Ok(())
}

#[cfg(not(unix))]
fn write_all_at(_file: &File, _bytes: &[u8], _offset: u64) -> Result<(), LiveEventJournalError> {
    Err(LiveEventJournalError::UnsupportedPlatform)
}

fn open_and_lock_journal_root(path: &Path) -> Result<JournalRootGuard, LiveEventJournalError> {
    let directory = open_directory_no_follow(path)?;
    set_owner_only_directory(&directory)?;
    let directory_metadata = directory
        .metadata()
        .map_err(|_| LiveEventJournalError::StorageUnavailable)?;
    if !directory_metadata.file_type().is_dir() {
        return Err(LiveEventJournalError::StorageUnavailable);
    }
    let identity = file_identity(&directory_metadata);
    let lock = lock_journal_root(path)?;
    let guard = JournalRootGuard {
        directory,
        identity,
        lock,
    };
    guard.verify(path)?;
    // Persist the lock-file directory entry before construction succeeds. A
    // crash may release the advisory lock, but must not make this process
    // claim a path whose directory identity was never durably established.
    guard.sync()?;
    guard.verify(path)?;
    Ok(guard)
}

fn open_directory_no_follow(path: &Path) -> Result<File, LiveEventJournalError> {
    let mut options = OpenOptions::new();
    options.read(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.custom_flags(libc::O_DIRECTORY | libc::O_NOFOLLOW);
    }
    options
        .open(path)
        .map_err(|_| LiveEventJournalError::StorageUnavailable)
}

fn lock_journal_root(path: &Path) -> Result<JournalRootLock, LiveEventJournalError> {
    let lock_path = path.join("host.lock");
    let mut options = OpenOptions::new();
    options.read(true).write(true).create(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600).custom_flags(libc::O_NOFOLLOW);
    }
    let file = options
        .open(lock_path)
        .map_err(|_| LiveEventJournalError::StorageUnavailable)?;
    set_owner_only_file(&file)?;
    file.try_lock_exclusive()
        .map_err(|_| LiveEventJournalError::AlreadyOpen)?;
    let metadata = file
        .metadata()
        .map_err(|_| LiveEventJournalError::StorageUnavailable)?;
    if !metadata.file_type().is_file() {
        return Err(LiveEventJournalError::StorageUnavailable);
    }
    Ok(JournalRootLock {
        identity: file_identity(&metadata),
        file,
    })
}

fn ensure_private_directory(path: &Path) -> Result<(), LiveEventJournalError> {
    ensure_private_directory_with_parent_sync(path, sync_directory_path)
}

fn canonical_journal_root_path(path: &Path) -> Result<PathBuf, LiveEventJournalError> {
    let file_name = path
        .file_name()
        .filter(|name| !name.is_empty())
        .ok_or(LiveEventJournalError::StorageUnavailable)?;
    let parent = path
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
        .ok_or(LiveEventJournalError::StorageUnavailable)?;
    let canonical_parent =
        fs::canonicalize(parent).map_err(|_| LiveEventJournalError::StorageUnavailable)?;
    verify_private_parent_directory(&canonical_parent)?;
    verify_safe_parent_ancestry(&canonical_parent)?;
    Ok(canonical_parent.join(file_name))
}

fn ensure_private_directory_with_parent_sync(
    path: &Path,
    sync_parent: impl FnOnce(&Path) -> Result<(), LiveEventJournalError>,
) -> Result<(), LiveEventJournalError> {
    let parent = path
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
        .ok_or(LiveEventJournalError::StorageUnavailable)?;
    verify_private_parent_directory(parent)?;
    match fs::symlink_metadata(path) {
        Ok(_) => {}
        Err(error) if error.kind() == ErrorKind::NotFound => {
            create_owner_only_directory(path)?;
        }
        Err(_) => return Err(LiveEventJournalError::StorageUnavailable),
    }
    let metadata =
        fs::symlink_metadata(path).map_err(|_| LiveEventJournalError::StorageUnavailable)?;
    if !metadata.file_type().is_dir() || metadata.file_type().is_symlink() {
        return Err(LiveEventJournalError::StorageUnavailable);
    }
    // Sync on every open, including after an earlier ambiguous sync failure.
    // This re-establishes durability for the root's parent directory entry.
    sync_parent(parent)
}

#[cfg(unix)]
fn verify_private_parent_directory(path: &Path) -> Result<(), LiveEventJournalError> {
    use std::os::unix::fs::PermissionsExt;

    let directory = open_directory_no_follow(path)?;
    let metadata = directory
        .metadata()
        .map_err(|_| LiveEventJournalError::StorageUnavailable)?;
    // The caller must place this root directly under an app-private directory
    // owned by Maple's effective user. Cross-principal parent rename authority
    // would invalidate the root/lock identity contract.
    if !metadata.file_type().is_dir()
        || !metadata_owned_by_effective_user(&metadata)
        || metadata.permissions().mode() & 0o777 != 0o700
    {
        return Err(LiveEventJournalError::StorageUnavailable);
    }
    #[cfg(target_os = "macos")]
    if macos_acl::has_extended_entries(&directory)
        .map_err(|_| LiveEventJournalError::StorageUnavailable)?
    {
        return Err(LiveEventJournalError::StorageUnavailable);
    }
    Ok(())
}

#[cfg(unix)]
fn verify_safe_parent_ancestry(private_parent: &Path) -> Result<(), LiveEventJournalError> {
    verify_safe_ancestor_directories(private_parent.ancestors().skip(1))
}

#[cfg(unix)]
fn verify_safe_directory_ancestry(directory: &Path) -> Result<(), LiveEventJournalError> {
    verify_safe_ancestor_directories(directory.ancestors())
}

#[cfg(unix)]
fn verify_safe_ancestor_directories<'a>(
    ancestors: impl Iterator<Item = &'a Path>,
) -> Result<(), LiveEventJournalError> {
    use std::os::unix::fs::PermissionsExt;

    let effective_uid = unsafe { libc::geteuid() };
    for ancestor in ancestors {
        let directory = open_directory_no_follow(ancestor)?;
        let metadata = directory
            .metadata()
            .map_err(|_| LiveEventJournalError::StorageUnavailable)?;
        let mode = metadata.permissions().mode();
        let owned_by_trusted_principal =
            matches!(metadata.uid(), 0) || metadata.uid() == effective_uid;
        let cross_principal_writable = mode & 0o022 != 0;
        let sticky = mode & (libc::S_ISVTX as u32) != 0;
        if !metadata.file_type().is_dir()
            || !owned_by_trusted_principal
            || (cross_principal_writable && !sticky)
        {
            return Err(LiveEventJournalError::StorageUnavailable);
        }
        #[cfg(target_os = "macos")]
        if macos_acl::has_allow_entries(&directory)
            .map_err(|_| LiveEventJournalError::StorageUnavailable)?
        {
            return Err(LiveEventJournalError::StorageUnavailable);
        }
    }
    Ok(())
}

#[cfg(not(unix))]
fn verify_safe_parent_ancestry(_private_parent: &Path) -> Result<(), LiveEventJournalError> {
    Err(LiveEventJournalError::UnsupportedPlatform)
}

#[cfg(not(unix))]
fn verify_safe_directory_ancestry(_directory: &Path) -> Result<(), LiveEventJournalError> {
    Err(LiveEventJournalError::UnsupportedPlatform)
}

#[cfg(unix)]
fn metadata_owned_by_effective_user(metadata: &fs::Metadata) -> bool {
    metadata.uid() == unsafe { libc::geteuid() }
}

#[cfg(not(unix))]
fn metadata_owned_by_effective_user(_metadata: &fs::Metadata) -> bool {
    false
}

#[cfg(not(unix))]
fn verify_private_parent_directory(_path: &Path) -> Result<(), LiveEventJournalError> {
    Err(LiveEventJournalError::UnsupportedPlatform)
}

#[cfg(unix)]
fn create_owner_only_directory(path: &Path) -> Result<(), LiveEventJournalError> {
    use std::os::unix::fs::DirBuilderExt;
    let mut builder = fs::DirBuilder::new();
    builder.mode(0o700);
    builder
        .create(path)
        .map_err(|_| LiveEventJournalError::StorageUnavailable)
}

#[cfg(not(unix))]
fn create_owner_only_directory(_path: &Path) -> Result<(), LiveEventJournalError> {
    Err(LiveEventJournalError::UnsupportedPlatform)
}

#[cfg(unix)]
fn set_owner_only_directory(file: &File) -> Result<(), LiveEventJournalError> {
    use std::os::unix::fs::PermissionsExt;
    let mut repaired = false;
    let metadata = file
        .metadata()
        .map_err(|_| LiveEventJournalError::StorageUnavailable)?;
    if metadata.permissions().mode() & 0o7777 != 0o700 {
        file.set_permissions(fs::Permissions::from_mode(0o700))
            .map_err(|_| LiveEventJournalError::StorageUnavailable)?;
        repaired = true;
    }
    #[cfg(target_os = "macos")]
    if macos_acl::has_extended_entries(file)
        .map_err(|_| LiveEventJournalError::StorageUnavailable)?
    {
        strip_extended_acl(file)?;
        repaired = true;
    }
    if repaired {
        file.sync_all()
            .map_err(|_| LiveEventJournalError::StorageUnavailable)?;
    }
    Ok(())
}

#[cfg(not(unix))]
fn set_owner_only_directory(_file: &File) -> Result<(), LiveEventJournalError> {
    Err(LiveEventJournalError::UnsupportedPlatform)
}

#[cfg(unix)]
fn set_owner_only_file(file: &File) -> Result<(), LiveEventJournalError> {
    use std::os::unix::fs::PermissionsExt;
    let mut repaired = false;
    let metadata = file
        .metadata()
        .map_err(|_| LiveEventJournalError::StorageUnavailable)?;
    if metadata.permissions().mode() & 0o7777 != 0o600 {
        file.set_permissions(fs::Permissions::from_mode(0o600))
            .map_err(|_| LiveEventJournalError::StorageUnavailable)?;
        repaired = true;
    }
    #[cfg(target_os = "macos")]
    if macos_acl::has_extended_entries(file)
        .map_err(|_| LiveEventJournalError::StorageUnavailable)?
    {
        strip_extended_acl(file)?;
        repaired = true;
    }
    if repaired {
        file.sync_all()
            .map_err(|_| LiveEventJournalError::StorageUnavailable)?;
    }
    Ok(())
}

#[cfg(not(unix))]
fn set_owner_only_file(_file: &File) -> Result<(), LiveEventJournalError> {
    Err(LiveEventJournalError::UnsupportedPlatform)
}

fn sync_directory_path(path: &Path) -> Result<(), LiveEventJournalError> {
    let directory = open_directory_no_follow(path)?;
    directory
        .sync_all()
        .map_err(|_| LiveEventJournalError::StorageUnavailable)
}

fn ensure_supported_platform() -> Result<(), LiveEventJournalError> {
    if durable_journal_platform_supported(std::env::consts::OS) {
        Ok(())
    } else {
        Err(LiveEventJournalError::UnsupportedPlatform)
    }
}

fn durable_journal_platform_supported(target_os: &str) -> bool {
    matches!(target_os, "macos" | "linux")
}

#[cfg(unix)]
fn file_identity(metadata: &fs::Metadata) -> FileIdentity {
    FileIdentity {
        device: metadata.dev(),
        inode: metadata.ino(),
    }
}

#[cfg(not(unix))]
fn file_identity(_metadata: &fs::Metadata) -> FileIdentity {
    FileIdentity {
        device: 0,
        inode: 0,
    }
}

#[cfg(target_os = "macos")]
fn strip_extended_acl(file: &File) -> Result<(), LiveEventJournalError> {
    macos_acl::strip(file).map_err(|_| LiveEventJournalError::StorageUnavailable)
}

#[cfg(all(unix, not(target_os = "macos")))]
fn strip_extended_acl(_file: &File) -> Result<(), LiveEventJournalError> {
    Ok(())
}

#[cfg(target_os = "macos")]
mod macos_acl {
    use std::ptr;
    use std::{ffi::c_void, fs::File, io, os::fd::AsRawFd};

    type Acl = *mut c_void;
    const ACL_TYPE_EXTENDED: i32 = 0x0000_0100;

    unsafe extern "C" {
        fn acl_init(count: i32) -> Acl;
        fn acl_free(object: *mut c_void) -> i32;
        fn acl_set_fd_np(fd: i32, acl: Acl, acl_type: i32) -> i32;
        fn acl_get_fd_np(fd: i32, acl_type: i32) -> Acl;
        fn acl_get_entry(acl: Acl, entry_id: i32, entry: *mut *mut c_void) -> i32;
        fn acl_get_tag_type(entry: *mut c_void, tag_type: *mut i32) -> i32;
    }

    pub(super) fn strip(file: &File) -> io::Result<()> {
        // SAFETY: `acl_init` returns an owned ACL handle or null. The handle is
        // passed only to macOS ACL functions and is freed exactly once below.
        let acl = unsafe { acl_init(0) };
        if acl.is_null() {
            return Err(io::Error::last_os_error());
        }
        // An empty ACL removes all extended entries, including inherited
        // entries that chmod alone leaves effective on macOS.
        // SAFETY: `file` owns a valid descriptor and `acl` is live here.
        let set_result = unsafe { acl_set_fd_np(file.as_raw_fd(), acl, ACL_TYPE_EXTENDED) };
        let set_error = (set_result != 0).then(io::Error::last_os_error);
        // SAFETY: `acl` was allocated by `acl_init` and has not been freed.
        let free_result = unsafe { acl_free(acl) };
        if let Some(error) = set_error {
            return Err(error);
        }
        if free_result != 0 {
            return Err(io::Error::last_os_error());
        }
        Ok(())
    }

    pub(super) fn has_extended_entries(file: &File) -> io::Result<bool> {
        has_entry_matching(file, |_| true)
    }

    pub(super) fn has_allow_entries(file: &File) -> io::Result<bool> {
        const ACL_EXTENDED_ALLOW: i32 = 1;
        has_entry_matching(file, |tag_type| tag_type == ACL_EXTENDED_ALLOW)
    }

    fn has_entry_matching(file: &File, predicate: impl Fn(i32) -> bool) -> io::Result<bool> {
        // SAFETY: the descriptor remains valid for this call. ENOENT is the
        // native "no extended ACL exists" result and therefore means empty.
        let acl = unsafe { acl_get_fd_np(file.as_raw_fd(), ACL_TYPE_EXTENDED) };
        if acl.is_null() {
            let error = io::Error::last_os_error();
            return if error.raw_os_error() == Some(libc::ENOENT) {
                Ok(false)
            } else {
                Err(error)
            };
        }
        let mut entry = ptr::null_mut();
        let mut entry_id = 0;
        let mut matched = false;
        let mut iteration_error = None;
        loop {
            // SAFETY: `acl` is live and `entry` points to writable storage.
            let get_result = unsafe { acl_get_entry(acl, entry_id, &mut entry) };
            if get_result > 0 {
                break;
            }
            if get_result < 0 {
                iteration_error = Some(io::Error::last_os_error());
                break;
            }
            let mut tag_type = 0;
            // SAFETY: a successful `acl_get_entry` returned a live entry.
            if unsafe { acl_get_tag_type(entry, &mut tag_type) } != 0 {
                iteration_error = Some(io::Error::last_os_error());
                break;
            }
            if predicate(tag_type) {
                matched = true;
                break;
            }
            entry_id = -1;
        }
        // SAFETY: `acl` was returned by `acl_get_fd_np` and is freed once.
        let free_result = unsafe { acl_free(acl) };
        if let Some(error) = iteration_error {
            return Err(error);
        }
        if free_result != 0 {
            return Err(io::Error::last_os_error());
        }
        Ok(matched)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[cfg(unix)]
    use std::os::unix::fs::PermissionsExt;
    use std::{collections::BTreeSet, sync::Barrier, thread};

    // These capabilities are deliberately move-only. This coherence check is
    // a compile-time assertion: adding `Clone` makes the marker selection
    // ambiguous and fails the test build before a caller can duplicate either
    // a corrupt-generation observation or its prepared one-shot obligation.
    const _: fn() = || {
        struct AmbiguousIfClone;
        trait NotCloneMarker<Disambiguator> {
            fn assert_not_clone() {}
        }
        impl<T: ?Sized> NotCloneMarker<()> for T {}
        impl<T: ?Sized + Clone> NotCloneMarker<AmbiguousIfClone> for T {}

        let _ = <LiveEventJournalReseedRequired as NotCloneMarker<_>>::assert_not_clone;
        let _ = <LiveEventJournalReseedObligation as NotCloneMarker<_>>::assert_not_clone;
        let _ = <LiveEventJournalRolloverObligation as NotCloneMarker<_>>::assert_not_clone;
        let _ = <VerifiedJournalReseedAuthority as NotCloneMarker<_>>::assert_not_clone;
    };

    #[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
    #[serde(rename_all = "camelCase", deny_unknown_fields)]
    struct TestPayload {
        event_id: String,
        kind: String,
        value: String,
    }

    impl TestPayload {
        fn new(value: impl Into<String>) -> Self {
            let value = value.into();
            let event_id = format!("event-{}", encode_hex(&Sha256::digest(value.as_bytes())));
            Self {
                event_id,
                kind: "timeline_item".to_string(),
                value,
            }
        }
    }

    impl LiveReplayPayload for TestPayload {
        fn live_replay_event_id(&self) -> &str {
            &self.event_id
        }

        fn validate_live_replay_payload(&self) -> Result<(), LiveEventJournalError> {
            if self.kind == "timeline_item"
                && self.value.len() <= 1_024
                && self.event_id.len() <= MAX_EVENT_OWNER_ID_BYTES
            {
                Ok(())
            } else {
                Err(LiveEventJournalError::PayloadTooLarge)
            }
        }
    }

    fn limits(max_entries: usize) -> LiveEventJournalLimits {
        LiveEventJournalLimits {
            max_entries,
            max_payload_bytes: 2_048,
            max_total_payload_bytes: 8_192,
            max_replay_entries: max_entries,
            max_replay_payload_bytes: 8_192,
        }
    }

    fn private_tempdir() -> tempfile::TempDir {
        let directory = tempfile::tempdir().unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(directory.path(), fs::Permissions::from_mode(0o700)).unwrap();
        }
        directory
    }

    fn owner(scope: &str, generation: u64) -> LiveEventAccountOwner {
        LiveEventAccountOwner::new(scope, generation).unwrap()
    }

    fn ingress<T: LiveReplayPayload>(
        journal: &LiveEventJournal<T>,
        owner: &LiveEventAccountOwner,
    ) -> LiveEventJournalIngressLease {
        let lease = journal.activate_account(owner).unwrap();
        journal.bind_ingress(&lease).unwrap()
    }

    fn reseed_required(
        result: Result<LiveEventJournalLease, LiveEventJournalActivationError>,
    ) -> LiveEventJournalReseedRequired {
        match result {
            Err(LiveEventJournalActivationError::ReseedRequired(required)) => *required,
            result => panic!("expected authoritative reseed requirement, got {result:?}"),
        }
    }

    fn read_v3_parts(
        path: &Path,
    ) -> (
        JournalHeader,
        VecDeque<StoredEntry<TestPayload>>,
        DiskAnchor,
    ) {
        let bytes = fs::read(path).unwrap();
        let anchor = select_v3_anchor(&bytes).unwrap();
        let snapshot_start = usize::try_from(anchor.snapshot_offset).unwrap();
        let snapshot_end = usize::try_from(anchor.data_start).unwrap();
        let committed_end = usize::try_from(anchor.committed_end).unwrap();
        let header = serde_json::from_slice(&bytes[snapshot_start..snapshot_end]).unwrap();
        let mut entries = VecDeque::new();
        let mut offset = snapshot_end;
        let mut chain_hash = v3_chain_base(&anchor).unwrap();
        while offset < committed_end {
            let (entry, next, frame_hash) = decode_v3_frame(
                &bytes,
                offset,
                committed_end,
                &chain_hash,
                MAX_CHECKPOINT_BYTES,
            )
            .unwrap();
            entries.push_back(entry);
            offset = next;
            chain_hash = frame_hash;
        }
        assert_eq!(offset, committed_end);
        assert_eq!(chain_hash, anchor.committed_chain_hash);
        (header, entries, anchor)
    }

    fn write_v3_parts(
        path: &Path,
        header: &JournalHeader,
        entries: &VecDeque<StoredEntry<TestPayload>>,
    ) {
        write_v3_parts_at_head(path, header, entries, header.head_sequence);
    }

    fn write_v3_parts_at_head(
        path: &Path,
        header: &JournalHeader,
        entries: &VecDeque<StoredEntry<TestPayload>>,
        committed_head_sequence: u64,
    ) {
        let mut encoded = encode_v3_journal(header, entries, new_file_nonce().unwrap()).unwrap();
        encoded.anchor.committed_head_sequence = committed_head_sequence;
        let anchor = encode_v3_anchor(&encoded.anchor).unwrap();
        encoded.bytes[DISK_SUPERBLOCK_BYTES..DISK_SUPERBLOCK_BYTES + DISK_ANCHOR_SLOT_BYTES]
            .copy_from_slice(&anchor);
        let mut file = OpenOptions::new()
            .write(true)
            .truncate(true)
            .open(path)
            .unwrap();
        file.write_all(&encoded.bytes).unwrap();
        file.sync_all().unwrap();
    }

    fn rewrite_header(path: &Path, mutate: impl FnOnce(&mut JournalHeader)) -> JournalHeader {
        let (mut header, entries, _) = read_v3_parts(path);
        mutate(&mut header);
        header.integrity = journal_header_integrity(&header).unwrap();
        write_v3_parts(path, &header, &entries);
        header
    }

    fn events<T>(read: LiveReplayRead<T>) -> (Vec<LiveReplayEntry<T>>, LiveEventCursor, bool) {
        match read {
            LiveReplayRead::Events {
                entries,
                next_cursor,
                has_more,
            } => (entries, next_cursor, has_more),
            LiveReplayRead::SnapshotRequired(snapshot) => {
                panic!("unexpected snapshot requirement: {snapshot:?}")
            }
        }
    }

    #[test]
    fn append_is_monotonic_and_replay_keeps_session_run_ownership() {
        let root = private_tempdir();
        let journal = LiveEventJournal::open(root.path().join("journal"), limits(10)).unwrap();
        let owner = owner("opaque-account-a", 7);
        let checkpoint = journal.checkpoint(&owner).unwrap();
        assert_eq!(checkpoint.sequence(), 0);

        let first = journal
            .append(
                &owner,
                "session-a",
                Some("run-a"),
                TestPayload::new("first"),
            )
            .unwrap();
        let second = journal
            .append(&owner, "session-b", None, TestPayload::new("second"))
            .unwrap();
        assert_eq!((first.sequence(), second.sequence()), (1, 2));
        assert_eq!(first.journal_id(), second.journal_id());

        let (entries, next, has_more) =
            events(journal.replay_after(&owner, &checkpoint, 10).unwrap());
        assert!(!has_more);
        assert_eq!(next, second);
        assert_eq!(entries.len(), 2);
        assert_eq!(entries[0].session_id(), "session-a");
        assert_eq!(entries[0].run_id(), Some("run-a"));
        assert_eq!(entries[0].payload().value, "first");
        assert_eq!(entries[1].session_id(), "session-b");
        assert_eq!(entries[1].run_id(), None);
    }

    #[test]
    fn journal_survives_reopen_and_continues_the_sequence() {
        let root = private_tempdir();
        let path = root.path().join("journal");
        let owner = owner("opaque-account", 0);
        let first = {
            let journal = LiveEventJournal::open(path.clone(), limits(10)).unwrap();
            journal
                .append(&owner, "session", None, TestPayload::new("first"))
                .unwrap()
        };
        let journal = LiveEventJournal::<TestPayload>::open(path, limits(10)).unwrap();
        let second = journal
            .append(&owner, "session", None, TestPayload::new("second"))
            .unwrap();
        assert_eq!(second.sequence(), 2);
        assert_eq!(second.journal_id(), first.journal_id());
        let (entries, _, _) = events(journal.replay_after(&owner, &first, 10).unwrap());
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].payload().value, "second");
    }

    #[test]
    fn process_local_generation_is_rebound_after_restart() {
        let root = private_tempdir();
        let path = root.path().join("journal");
        let prior_process_owner = owner("opaque-account", 9);
        let first = {
            let journal = LiveEventJournal::open(path.clone(), limits(10)).unwrap();
            journal
                .append(
                    &prior_process_owner,
                    "session",
                    None,
                    TestPayload::new("first"),
                )
                .unwrap()
        };

        let current_process_owner = owner("opaque-account", 0);
        let journal = LiveEventJournal::<TestPayload>::open(path, limits(10)).unwrap();
        let checkpoint = journal.checkpoint(&current_process_owner).unwrap();
        assert_eq!(checkpoint, first);
        assert_eq!(
            journal.checkpoint(&prior_process_owner),
            Err(LiveEventJournalError::OwnerGenerationMismatch)
        );
    }

    #[test]
    fn generation_rotation_resets_sequence_and_rejects_the_old_owner() {
        let root = private_tempdir();
        let journal = LiveEventJournal::open(root.path().join("journal"), limits(10)).unwrap();
        let previous = owner("opaque-account", 0);
        let old_cursor = journal
            .append(&previous, "session", None, TestPayload::new("old"))
            .unwrap();
        let current = owner("opaque-account", 1);
        let reset = journal
            .rotate_account_generation(&previous, &current)
            .unwrap();
        assert_eq!(reset.sequence(), 0);
        assert_ne!(reset.journal_id(), old_cursor.journal_id());
        assert_eq!(
            journal.checkpoint(&previous),
            Err(LiveEventJournalError::OwnerGenerationMismatch)
        );
        assert_eq!(
            journal.replay_after(&current, &old_cursor, 10).unwrap(),
            LiveReplayRead::SnapshotRequired(SnapshotRequired {
                reason: SnapshotRequiredReason::JournalReplaced,
                current_cursor: reset,
            })
        );
        assert_eq!(
            journal
                .append(&current, "session", None, TestPayload::new("new"))
                .unwrap()
                .sequence(),
            1
        );
    }

    #[test]
    fn generation_rotation_can_be_the_accounts_first_journal_operation() {
        let root = private_tempdir();
        let journal =
            LiveEventJournal::<TestPayload>::open(root.path().join("journal"), limits(10)).unwrap();
        let previous = owner("opaque-account", 0);
        let current = owner("opaque-account", 1);
        let reset = journal
            .rotate_account_generation(&previous, &current)
            .unwrap();
        assert_eq!(reset.sequence(), 0);
        assert_eq!(journal.checkpoint(&current).unwrap(), reset);
        assert_eq!(
            journal.checkpoint(&previous),
            Err(LiveEventJournalError::OwnerGenerationMismatch)
        );
    }

    #[test]
    fn retention_gap_requires_snapshot_instead_of_silently_skipping() {
        let root = private_tempdir();
        let journal = LiveEventJournal::open(root.path().join("journal"), limits(2)).unwrap();
        let owner = owner("opaque-account", 0);
        let before = journal.checkpoint(&owner).unwrap();
        let first = journal
            .append(&owner, "session", None, TestPayload::new("one"))
            .unwrap();
        journal
            .append(&owner, "session", None, TestPayload::new("two"))
            .unwrap();
        let projection_head = journal.checkpoint(&owner).unwrap();
        journal
            .store_checkpoint(&owner, &projection_head, b"absolute projection")
            .unwrap();
        let current = journal
            .append(&owner, "session", None, TestPayload::new("three"))
            .unwrap();

        assert_eq!(
            journal.replay_after(&owner, &before, 2).unwrap(),
            LiveReplayRead::SnapshotRequired(SnapshotRequired {
                reason: SnapshotRequiredReason::RetentionGap,
                current_cursor: current.clone(),
            })
        );
        let (entries, next, _) = events(journal.replay_after(&owner, &first, 2).unwrap());
        assert_eq!(entries.len(), 2);
        assert_eq!(next, current);
    }

    #[test]
    fn clearing_an_account_rotates_the_journal_and_invalidates_old_cursor() {
        let root = private_tempdir();
        let journal = LiveEventJournal::open(root.path().join("journal"), limits(10)).unwrap();
        let owner = owner("opaque-account", 0);
        let old = journal
            .append(&owner, "session", None, TestPayload::new("old"))
            .unwrap();
        journal.clear_account(&owner).unwrap();
        let current = journal.checkpoint(&owner).unwrap();
        assert_ne!(current.journal_id(), old.journal_id());
        assert_eq!(
            journal.replay_after(&owner, &old, 10).unwrap(),
            LiveReplayRead::SnapshotRequired(SnapshotRequired {
                reason: SnapshotRequiredReason::JournalReplaced,
                current_cursor: current,
            })
        );
    }

    #[test]
    fn account_and_generation_ownership_fail_closed() {
        let root = private_tempdir();
        let journal = LiveEventJournal::open(root.path().join("journal"), limits(10)).unwrap();
        let account_a = owner("opaque-account-a", 3);
        let account_b = owner("opaque-account-b", 3);
        let a_checkpoint = journal.checkpoint(&account_a).unwrap();
        let b_checkpoint = journal.checkpoint(&account_b).unwrap();
        journal
            .append(&account_a, "session-a", None, TestPayload::new("a"))
            .unwrap();
        journal
            .append(&account_b, "session-b", None, TestPayload::new("b"))
            .unwrap();
        let (a_entries, _, _) =
            events(journal.replay_after(&account_a, &a_checkpoint, 10).unwrap());
        let (b_entries, _, _) =
            events(journal.replay_after(&account_b, &b_checkpoint, 10).unwrap());
        assert_eq!(a_entries[0].payload().value, "a");
        assert_eq!(b_entries[0].payload().value, "b");

        let stale_generation = owner("opaque-account-a", 2);
        let newer_generation = owner("opaque-account-a", 4);
        assert_eq!(
            journal.checkpoint(&stale_generation),
            Err(LiveEventJournalError::OwnerGenerationMismatch)
        );
        assert_eq!(
            journal.append(
                &newer_generation,
                "session-a",
                None,
                TestPayload::new("new")
            ),
            Err(LiveEventJournalError::OwnerGenerationMismatch)
        );

        journal.unload_account(&account_a).unwrap();
        assert_eq!(
            journal.checkpoint(&stale_generation),
            Err(LiveEventJournalError::OwnerGenerationMismatch)
        );
    }

    #[test]
    fn stable_event_id_makes_append_retry_idempotent_and_conflicts_fail_closed() {
        let root = private_tempdir();
        let journal = LiveEventJournal::open(root.path().join("journal"), limits(10)).unwrap();
        let owner = owner("opaque-account", 0);
        let payload = TestPayload::new("same event");
        let first = journal
            .append(&owner, "session", Some("run"), payload.clone())
            .unwrap();
        let retried = journal
            .append(&owner, "session", Some("run"), payload.clone())
            .unwrap();
        assert_eq!(retried, first);
        let (entries, _, _) = events(
            journal
                .replay_after(
                    &owner,
                    &LiveEventCursor::new(first.journal_id().to_string(), 0),
                    10,
                )
                .unwrap(),
        );
        assert_eq!(entries.len(), 1);

        let mut conflicting = TestPayload::new("different event");
        conflicting.event_id = payload.event_id.clone();
        assert_eq!(
            journal.append(&owner, "session", Some("run"), conflicting),
            Err(LiveEventJournalError::EventIdConflict)
        );
        assert_eq!(
            journal.append(&owner, "other-session", Some("run"), payload),
            Err(LiveEventJournalError::EventIdConflict)
        );
    }

    #[test]
    fn expected_head_fences_classification_append_and_checkpoint() {
        let root = private_tempdir();
        let journal = LiveEventJournal::open(root.path().join("journal"), limits(10)).unwrap();
        let owner = owner("opaque-account", 0);
        let stale = journal.checkpoint(&owner).unwrap();
        let first_payload = TestPayload::new("first");
        let first = match journal
            .append_outcome(
                &ingress(&journal, &owner),
                &stale,
                "session",
                None,
                first_payload.clone(),
            )
            .unwrap()
        {
            AppendOutcome::Inserted(cursor) => cursor,
            outcome => panic!("unexpected append outcome: {outcome:?}"),
        };
        assert_eq!(
            journal.classify_event(
                &ingress(&journal, &owner),
                &stale,
                "session",
                None,
                &first_payload
            ),
            Ok(EventAdmission::Duplicate {
                event_cursor: first.clone(),
                head_cursor: first.clone(),
            })
        );
        assert_eq!(
            journal.append_outcome(
                &ingress(&journal, &owner),
                &stale,
                "session",
                None,
                first_payload.clone(),
            ),
            Ok(AppendOutcome::Duplicate {
                event_cursor: first.clone(),
                head_cursor: first.clone(),
            })
        );
        assert_eq!(
            journal.classify_event(
                &ingress(&journal, &owner),
                &stale,
                "session",
                None,
                &TestPayload::new("unrelated")
            ),
            Err(LiveEventJournalError::HeadChanged)
        );
        assert_eq!(
            journal.store_checkpoint(&owner, &stale, b"stale projection"),
            Err(LiveEventJournalError::HeadChanged)
        );
        assert_eq!(
            journal.classify_event(
                &ingress(&journal, &owner),
                &first,
                "session",
                None,
                &first_payload
            ),
            Ok(EventAdmission::Duplicate {
                event_cursor: first.clone(),
                head_cursor: first,
            })
        );
    }

    #[test]
    fn post_sync_error_reopens_as_an_exact_duplicate_not_a_poisoning_head_change() {
        let root = private_tempdir();
        let path = root.path().join("journal");
        let owner = owner("opaque-account", 0);
        let payload = TestPayload::new("durable-before-ack");
        let expected = {
            let journal = LiveEventJournal::open(path.clone(), limits(10)).unwrap();
            let expected = journal.checkpoint(&owner).unwrap();
            journal.fail_next_append_after_sync();
            assert_eq!(
                journal.append_outcome(
                    &ingress(&journal, &owner),
                    &expected,
                    "session",
                    None,
                    payload.clone()
                ),
                Err(LiveEventJournalError::StorageUnavailable)
            );
            expected
        };

        let journal = LiveEventJournal::<TestPayload>::open(path, limits(10)).unwrap();
        let outcome = journal
            .append_outcome(
                &ingress(&journal, &owner),
                &expected,
                "session",
                None,
                payload.clone(),
            )
            .unwrap();
        let committed = match outcome {
            AppendOutcome::Duplicate {
                event_cursor,
                head_cursor,
            } => {
                assert_eq!(event_cursor, head_cursor);
                event_cursor
            }
            outcome => panic!("expected recovered duplicate, got {outcome:?}"),
        };
        assert_eq!(committed.sequence(), 1);
        let (entries, next, has_more) = events(
            journal
                .replay_after(&owner, &expected, journal.max_replay_entries())
                .unwrap(),
        );
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].payload(), &payload);
        assert_eq!(next, committed);
        assert!(!has_more);
    }

    #[test]
    fn same_process_ambiguous_append_only_exact_ingress_retry_can_reconcile() {
        let root = private_tempdir();
        let journal = LiveEventJournal::open(root.path().join("journal"), limits(10)).unwrap();
        let owner = owner("same-process-ambiguous-append", 0);
        let lease = journal.activate_account(&owner).unwrap();
        let ingress = journal.bind_ingress(&lease).unwrap();
        let start = journal.checkpoint(&lease).unwrap();
        let payload = TestPayload::new("durable before same-process error");

        journal.fail_next_append_after_sync();
        assert_eq!(
            journal.append_outcome(&ingress, &start, "session", Some("run"), payload.clone()),
            Err(LiveEventJournalError::StorageUnavailable)
        );
        assert_eq!(
            journal.checkpoint(&lease),
            Err(LiveEventJournalError::OwnerTransitionIncomplete)
        );
        assert_eq!(
            journal.classify_event(
                &ingress,
                &start,
                "session",
                Some("run"),
                &TestPayload::new("unrelated retry")
            ),
            Err(LiveEventJournalError::OwnerTransitionIncomplete)
        );

        let admission = journal
            .classify_event(&ingress, &start, "session", Some("run"), &payload)
            .unwrap();
        let committed = match admission {
            EventAdmission::Duplicate {
                event_cursor,
                head_cursor,
            } => {
                assert_eq!(event_cursor, head_cursor);
                event_cursor
            }
            admission => panic!("expected exact recovered duplicate, got {admission:?}"),
        };
        assert_eq!(committed.sequence(), 1);
        assert_eq!(journal.checkpoint(&lease).unwrap(), committed);
        assert!(matches!(
            journal
                .append_outcome(&ingress, &start, "session", Some("run"), payload)
                .unwrap(),
            AppendOutcome::Duplicate { .. }
        ));
    }

    #[test]
    fn rollover_revokes_old_activation_and_ingress_before_tombstones_clear() {
        let root = private_tempdir();
        let journal = LiveEventJournal::open(root.path().join("journal"), limits(10)).unwrap();
        let owner = owner("rollover-ingress-fence", 0);
        let old_lease = journal.activate_account(&owner).unwrap();
        let old_ingress = journal.bind_ingress(&old_lease).unwrap();
        let old_namespace = old_ingress.event_namespace_commitment();
        let start = journal.checkpoint(&old_lease).unwrap();
        let payload = TestPayload::new("same durable event ID across generations");
        let old_head = journal
            .append_outcome(
                &old_ingress,
                &start,
                "session",
                Some("run"),
                payload.clone(),
            )
            .unwrap()
            .cursor()
            .clone();
        journal
            .store_checkpoint(
                &old_lease,
                &old_head,
                b"absolute rollover ingress projection",
            )
            .unwrap();
        let obligation = journal
            .prepare_rollover(
                &old_lease,
                &old_head,
                b"absolute rollover ingress projection",
            )
            .unwrap();

        // In-flight commands cannot pass the FIFO seal while rollover is
        // pending, even though they still carry the exact old capability.
        assert_eq!(
            journal.classify_event(&old_ingress, &old_head, "session", Some("run"), &payload),
            Err(LiveEventJournalError::OwnerTransitionIncomplete)
        );

        let activation = journal
            .commit_rollover(&obligation, b"absolute rollover ingress projection")
            .unwrap();
        let (fresh_lease, fresh) = activation.into_parts();
        let fresh_ingress = journal.bind_ingress(&fresh_lease).unwrap();
        assert_ne!(fresh_ingress.event_namespace_commitment(), old_namespace);
        assert_eq!(
            journal.checkpoint(&old_lease),
            Err(LiveEventJournalError::JournalRetired)
        );
        assert_eq!(
            journal.bind_ingress(&old_lease),
            Err(LiveEventJournalError::JournalRetired)
        );
        assert_eq!(
            journal.append_outcome(
                &old_ingress,
                &fresh,
                "session",
                Some("run"),
                payload.clone()
            ),
            Err(LiveEventJournalError::JournalReplaced)
        );

        // Clearing the old generation's tombstone deliberately permits the
        // same stable ID only when it arrives under the newly admitted ingress
        // namespace.
        let fresh_head = journal
            .append_outcome(
                &fresh_ingress,
                &fresh,
                "session",
                Some("run"),
                payload.clone(),
            )
            .unwrap()
            .cursor()
            .clone();
        assert_eq!(fresh_head.sequence(), 1);
        assert!(matches!(
            journal
                .append_outcome(&fresh_ingress, &fresh, "session", Some("run"), payload,)
                .unwrap(),
            AppendOutcome::Duplicate { .. }
        ));
    }

    #[test]
    fn same_owner_rollover_preserves_absolute_projection_and_fences_old_actors() {
        let root = private_tempdir();
        let path = root.path().join("journal");
        let owner = owner("opaque-account", 0);
        let old_payload = TestPayload::new("old-generation-event");
        let old_head = {
            let journal = LiveEventJournal::open(path.clone(), limits(10)).unwrap();
            let old_head = journal
                .append(&owner, "session", None, old_payload.clone())
                .unwrap();
            {
                let mut state = journal.lock_state().unwrap();
                let account = state.accounts.get_mut(&owner.account_key).unwrap();
                account.event_id_metadata_bytes = MAX_IDEMPOTENCY_METADATA_BYTES;
            }
            assert_eq!(
                journal.append_outcome(
                    &ingress(&journal, &owner),
                    &old_head,
                    "session",
                    None,
                    TestPayload::new("capacity-boundary")
                ),
                Err(LiveEventJournalError::IdempotencyCapacityExceeded)
            );
            journal
                .store_checkpoint(&owner, &old_head, b"absolute projection at rollover")
                .unwrap();
            let lease = journal.activate_account(&owner).unwrap();
            assert!(matches!(
                journal.prepare_rollover(&lease, &old_head, b"different projection at rollover"),
                Err(LiveEventJournalError::InvalidCheckpoint)
            ));
            let obligation = journal
                .prepare_rollover(&lease, &old_head, b"absolute projection at rollover")
                .unwrap();
            assert_eq!(
                journal.checkpoint(&lease),
                Err(LiveEventJournalError::OwnerTransitionIncomplete)
            );
            let activation = journal
                .commit_rollover(&obligation, b"absolute projection at rollover")
                .unwrap();
            let (fresh_lease, fresh) = activation.into_parts();
            let replayed = journal
                .commit_rollover(&obligation, b"absolute projection at rollover")
                .unwrap();
            assert_eq!(replayed.lease, fresh_lease);
            assert_eq!(replayed.cursor, fresh);
            assert_eq!(fresh.sequence(), 0);
            assert_ne!(fresh.journal_id(), old_head.journal_id());
            old_head
        };

        let journal = LiveEventJournal::<TestPayload>::open(path, limits(10)).unwrap();
        let fresh = journal.checkpoint(&owner).unwrap();
        assert_eq!(fresh.sequence(), 0);
        assert_ne!(fresh.journal_id(), old_head.journal_id());
        let projection = journal.load_checkpoint(&owner).unwrap().unwrap();
        assert_eq!(projection.through_cursor, fresh);
        assert_eq!(projection.bytes, b"absolute projection at rollover");
        assert_eq!(
            journal.replay_after(&owner, &old_head, 10).unwrap(),
            LiveReplayRead::SnapshotRequired(SnapshotRequired {
                reason: SnapshotRequiredReason::JournalReplaced,
                current_cursor: fresh.clone(),
            })
        );
        assert_eq!(
            journal.classify_event(
                &ingress(&journal, &owner),
                &old_head,
                "session",
                None,
                &old_payload
            ),
            Err(LiveEventJournalError::JournalReplaced)
        );
        assert_eq!(
            journal.append_outcome(
                &ingress(&journal, &owner),
                &old_head,
                "session",
                None,
                old_payload
            ),
            Err(LiveEventJournalError::JournalReplaced)
        );
        assert_eq!(
            journal
                .append_outcome(
                    &ingress(&journal, &owner),
                    &fresh,
                    "session",
                    None,
                    TestPayload::new("new-generation-event")
                )
                .unwrap()
                .cursor()
                .sequence(),
            1
        );
    }

    #[test]
    fn rollover_crash_boundaries_reopen_as_exactly_old_or_new_generation() {
        for (boundary, replacement_committed) in [
            (ReplaceFailureBoundary::BeforeFileSync, false),
            (ReplaceFailureBoundary::AfterFileSync, false),
            (ReplaceFailureBoundary::AfterPersist, true),
            (ReplaceFailureBoundary::AfterDirectorySync, true),
        ] {
            let root = private_tempdir();
            let path = root.path().join("journal");
            let owner = owner("opaque-account", 0);
            let old_payload = TestPayload::new("old-generation-event");
            let old_head = {
                let journal = LiveEventJournal::open(path.clone(), limits(10)).unwrap();
                let old_head = journal
                    .append(&owner, "session", None, old_payload.clone())
                    .unwrap();
                journal
                    .store_checkpoint(
                        &owner,
                        &old_head,
                        b"absolute projection at ambiguous commit",
                    )
                    .unwrap();
                let lease = journal.activate_account(&owner).unwrap();
                let obligation = journal
                    .prepare_rollover(
                        &lease,
                        &old_head,
                        b"absolute projection at ambiguous commit",
                    )
                    .unwrap();
                journal.fail_next_replace_at(boundary);
                assert!(
                    matches!(
                        journal.commit_rollover(
                            &obligation,
                            b"absolute projection at ambiguous commit"
                        ),
                        Err(LiveEventJournalError::StorageUnavailable)
                    ),
                    "boundary {boundary:?}"
                );
                old_head
            };

            let journal = LiveEventJournal::<TestPayload>::open(path, limits(10)).unwrap();
            let recovered = journal.checkpoint(&owner).unwrap();
            assert_eq!(
                recovered.journal_id() != old_head.journal_id(),
                replacement_committed,
                "boundary {boundary:?}"
            );
            assert_eq!(
                journal.load_checkpoint(&owner).unwrap().unwrap().bytes,
                b"absolute projection at ambiguous commit"
            );
            if replacement_committed {
                assert_eq!(recovered.sequence(), 0);
                assert_eq!(
                    journal.append_outcome(
                        &ingress(&journal, &owner),
                        &old_head,
                        "session",
                        None,
                        old_payload
                    ),
                    Err(LiveEventJournalError::JournalReplaced),
                    "boundary {boundary:?}"
                );
            } else {
                assert_eq!(recovered, old_head);
                let lease = journal.activate_account(&owner).unwrap();
                let obligation = journal
                    .prepare_rollover(
                        &lease,
                        &old_head,
                        b"absolute projection at ambiguous commit",
                    )
                    .unwrap();
                let activation = journal
                    .commit_rollover(&obligation, b"absolute projection at ambiguous commit")
                    .unwrap();
                let (_, fresh) = activation.into_parts();
                assert_eq!(fresh.sequence(), 0);
                assert_ne!(fresh.journal_id(), old_head.journal_id());
            }
        }
    }

    #[test]
    fn rollover_requires_one_prepared_fifo_obligation_and_retries_ambiguity() {
        let root = private_tempdir();
        let path = root.path().join("journal");
        let owner = owner("rollover-obligation", 0);
        let journal = LiveEventJournal::<TestPayload>::open(path, limits(4)).unwrap();
        let lease = journal.activate_account(&owner).unwrap();
        let start = journal.checkpoint(&lease).unwrap();
        let head = journal
            .append_outcome(
                &journal.bind_ingress(&lease).unwrap(),
                &start,
                "session",
                None,
                TestPayload::new("before rollover"),
            )
            .unwrap()
            .cursor()
            .clone();
        journal
            .store_checkpoint(&lease, &head, b"absolute rollover projection")
            .unwrap();

        let forged = LiveEventJournalRolloverObligation {
            owner: owner.clone(),
            operation_token: lease.operation_token,
            new_operation_token: new_process_token().unwrap(),
            rollover_nonce: [0x44; PROCESS_TOKEN_BYTES],
            journal_id: head.journal_id().to_string(),
            head_sequence: head.sequence(),
            checkpoint_commitment: Sha256::digest(b"absolute rollover projection").into(),
            new_journal_id: new_journal_id().unwrap(),
        };
        assert!(matches!(
            journal.commit_rollover(&forged, b"absolute rollover projection"),
            Err(LiveEventJournalError::JournalReplaced)
        ));

        let obligation = journal
            .prepare_rollover(&lease, &head, b"absolute rollover projection")
            .unwrap();
        assert!(matches!(
            journal.prepare_rollover(&lease, &head, b"absolute rollover projection"),
            Err(LiveEventJournalError::OwnerTransitionIncomplete)
        ));
        assert!(matches!(
            journal.commit_rollover(&obligation, b"different projection"),
            Err(LiveEventJournalError::InvalidCheckpoint)
        ));
        journal.fail_next_replace_at(ReplaceFailureBoundary::AfterPersist);
        assert!(matches!(
            journal.commit_rollover(&obligation, b"absolute rollover projection"),
            Err(LiveEventJournalError::StorageUnavailable)
        ));
        assert_eq!(
            journal.checkpoint(&lease),
            Err(LiveEventJournalError::OwnerTransitionIncomplete)
        );
        let activation = journal
            .commit_rollover(&obligation, b"absolute rollover projection")
            .unwrap();
        let (fresh_lease, fresh) = activation.into_parts();
        assert_ne!(fresh.journal_id(), head.journal_id());
        assert_eq!(fresh.sequence(), 0);
        assert_eq!(
            journal.checkpoint(&lease),
            Err(LiveEventJournalError::JournalRetired)
        );
        assert_eq!(journal.checkpoint(&fresh_lease).unwrap(), fresh);
        let path = journal.journal_path(&owner);
        let bytes_before_replay = fs::read(&path).unwrap();
        let replayed_once = journal
            .commit_rollover(&obligation, b"absolute rollover projection")
            .unwrap();
        let replayed_twice = journal
            .commit_rollover(&obligation, b"absolute rollover projection")
            .unwrap();
        assert_eq!(replayed_once.lease, fresh_lease);
        assert_eq!(replayed_once.cursor, fresh);
        assert_eq!(replayed_twice.lease, fresh_lease);
        assert_eq!(replayed_twice.cursor, fresh);
        assert_eq!(fs::read(&path).unwrap(), bytes_before_replay);
        assert_eq!(
            journal.append_outcome(
                &LiveEventJournalIngressLease {
                    owner: owner.clone(),
                    operation_token: lease.operation_token,
                    journal_id: decode_hex_array(head.journal_id()).unwrap(),
                },
                &head,
                "session",
                None,
                TestPayload::new("old actor retry"),
            ),
            Err(LiveEventJournalError::JournalReplaced)
        );
        let next_obligation = journal
            .prepare_rollover(&fresh_lease, &fresh, b"absolute rollover projection")
            .unwrap();
        assert!(matches!(
            journal.commit_rollover(&obligation, b"absolute rollover projection"),
            Err(LiveEventJournalError::JournalReplaced)
        ));
        drop(next_obligation);
    }

    #[test]
    fn compaction_crash_boundaries_reopen_as_exactly_old_or_new_head() {
        for (boundary, replacement_committed) in [
            (ReplaceFailureBoundary::BeforeFileSync, false),
            (ReplaceFailureBoundary::AfterFileSync, false),
            (ReplaceFailureBoundary::AfterPersist, true),
            (ReplaceFailureBoundary::AfterDirectorySync, true),
        ] {
            let root = private_tempdir();
            let path = root.path().join("journal");
            let owner = owner(&format!("compact-{boundary:?}"), 0);
            let second_payload = TestPayload::new("second compacted event");
            let first = {
                let journal = LiveEventJournal::open(path.clone(), limits(1)).unwrap();
                let first = journal
                    .append(
                        &owner,
                        "session",
                        None,
                        TestPayload::new("first compacted event"),
                    )
                    .unwrap();
                journal
                    .store_checkpoint(&owner, &first, b"absolute compacted projection")
                    .unwrap();
                journal.fail_next_replace_at(boundary);
                assert_eq!(
                    journal.append_outcome(
                        &ingress(&journal, &owner),
                        &first,
                        "session",
                        None,
                        second_payload.clone()
                    ),
                    Err(LiveEventJournalError::StorageUnavailable),
                    "boundary {boundary:?}"
                );
                first
            };

            let journal = LiveEventJournal::<TestPayload>::open(path, limits(1)).unwrap();
            let recovered = journal.checkpoint(&owner).unwrap();
            assert_eq!(
                recovered.sequence(),
                if replacement_committed { 2 } else { 1 },
                "boundary {boundary:?}"
            );
            let retried = journal
                .append_outcome(
                    &ingress(&journal, &owner),
                    &first,
                    "session",
                    None,
                    second_payload.clone(),
                )
                .unwrap();
            if replacement_committed {
                assert!(matches!(retried, AppendOutcome::Duplicate { .. }));
            } else {
                assert!(matches!(retried, AppendOutcome::Inserted(_)));
            }
            assert_eq!(journal.checkpoint(&owner).unwrap().sequence(), 2);
            assert_eq!(
                journal.load_checkpoint(&owner).unwrap().unwrap().bytes,
                b"absolute compacted projection"
            );
        }
    }

    #[test]
    fn checkpoint_survives_restart_and_preserves_recent_replay_suffix() {
        let root = private_tempdir();
        let path = root.path().join("journal");
        let owner = owner("opaque-account", 0);
        let (start, second) = {
            let journal = LiveEventJournal::open(path.clone(), limits(4)).unwrap();
            let start = journal.checkpoint(&owner).unwrap();
            journal
                .append(&owner, "session", None, TestPayload::new("one"))
                .unwrap();
            let second = journal
                .append(&owner, "session", None, TestPayload::new("two"))
                .unwrap();
            assert_eq!(
                journal
                    .store_checkpoint(&owner, &second, b"absolute projection at two")
                    .unwrap(),
                second
            );
            let saved = journal.load_checkpoint(&owner).unwrap().unwrap();
            assert_eq!(saved.through_cursor, second);
            assert_eq!(saved.bytes, b"absolute projection at two");
            let (entries, _, _) = events(journal.replay_after(&owner, &start, 4).unwrap());
            assert_eq!(entries.len(), 2);
            (start, second)
        };

        let journal = LiveEventJournal::<TestPayload>::open(path, limits(4)).unwrap();
        let saved = journal.load_checkpoint(&owner).unwrap().unwrap();
        assert_eq!(saved.through_cursor, second);
        assert_eq!(saved.bytes, b"absolute projection at two");
        let (retained, _, _) = events(journal.replay_after(&owner, &start, 4).unwrap());
        assert_eq!(retained.len(), 2);
        let third = journal
            .append(&owner, "session", None, TestPayload::new("three"))
            .unwrap();
        let (delta, next, has_more) = events(journal.replay_after(&owner, &second, 4).unwrap());
        assert_eq!(delta.len(), 1);
        assert_eq!(delta[0].payload().value, "three");
        assert_eq!(next, third);
        assert!(!has_more);
    }

    #[test]
    fn compaction_requires_checkpoint_before_append_and_keeps_exact_tombstones() {
        let root = private_tempdir();
        let path = root.path().join("journal");
        let owner = owner("opaque-account", 0);
        let old_payload = TestPayload::new("one");
        let (old_cursor, head) = {
            let journal = LiveEventJournal::open(path.clone(), limits(2)).unwrap();
            let old_cursor = journal
                .append(&owner, "session", None, old_payload.clone())
                .unwrap();
            let head = journal
                .append(&owner, "session", None, TestPayload::new("two"))
                .unwrap();
            assert_eq!(
                journal.append_outcome(
                    &ingress(&journal, &owner),
                    &head,
                    "session",
                    None,
                    TestPayload::new("three"),
                ),
                Err(LiveEventJournalError::CheckpointRequired)
            );
            assert_eq!(journal.checkpoint(&owner).unwrap(), head);
            journal
                .store_checkpoint(&owner, &head, b"projection through two")
                .unwrap();
            let third = journal
                .append_outcome(
                    &ingress(&journal, &owner),
                    &head,
                    "session",
                    None,
                    TestPayload::new("three"),
                )
                .unwrap();
            assert!(matches!(third, AppendOutcome::Inserted(_)));
            (old_cursor, journal.checkpoint(&owner).unwrap())
        };

        let journal = LiveEventJournal::<TestPayload>::open(path, limits(2)).unwrap();
        assert_eq!(
            journal
                .classify_event(
                    &ingress(&journal, &owner),
                    &head,
                    "session",
                    None,
                    &old_payload
                )
                .unwrap(),
            EventAdmission::Duplicate {
                event_cursor: old_cursor.clone(),
                head_cursor: head.clone(),
            }
        );
        assert_eq!(
            journal
                .append_outcome(
                    &ingress(&journal, &owner),
                    &head,
                    "session",
                    None,
                    old_payload.clone()
                )
                .unwrap(),
            AppendOutcome::Duplicate {
                event_cursor: old_cursor.clone(),
                head_cursor: head.clone(),
            }
        );
        let mut conflict = TestPayload::new("conflict");
        conflict.event_id = old_payload.event_id;
        assert_eq!(
            journal.classify_event(
                &ingress(&journal, &owner),
                &head,
                "session",
                None,
                &conflict
            ),
            Err(LiveEventJournalError::EventIdConflict)
        );
    }

    #[test]
    fn checkpoint_validation_is_bounded_and_schema_checked_on_reopen() {
        let root = private_tempdir();
        let path = root.path().join("journal");
        let owner = owner("opaque-account", 0);
        let journal = LiveEventJournal::<TestPayload>::open(path.clone(), limits(2)).unwrap();
        let head = journal.checkpoint(&owner).unwrap();
        assert_eq!(journal.max_checkpoint_bytes(), MAX_CHECKPOINT_BYTES);
        assert_eq!(
            journal.store_checkpoint(&owner, &head, b""),
            Err(LiveEventJournalError::InvalidCheckpoint)
        );
        assert_eq!(
            journal.store_checkpoint(&owner, &head, &vec![0; MAX_CHECKPOINT_BYTES + 1]),
            Err(LiveEventJournalError::InvalidCheckpoint)
        );
        journal
            .store_checkpoint(&owner, &head, b"valid checkpoint")
            .unwrap();
        journal.unload_account(&owner).unwrap();
        rewrite_header(&journal.journal_path(&owner), |header| {
            header.checkpoint.as_mut().unwrap().schema = "unknown".to_string();
        });
        assert_eq!(
            journal.load_checkpoint(&owner),
            Err(LiveEventJournalError::StorageCorrupt)
        );
    }

    #[test]
    fn idempotency_capacity_rejects_only_unseen_events_without_advancing_head() {
        let root = private_tempdir();
        let journal = LiveEventJournal::open(root.path().join("journal"), limits(4)).unwrap();
        let owner = owner("opaque-account", 0);
        let payload = TestPayload::new("already committed");
        let head = journal
            .append(&owner, "session", None, payload.clone())
            .unwrap();
        {
            let mut state = journal.lock_state().unwrap();
            state
                .accounts
                .get_mut(&owner.account_key)
                .unwrap()
                .event_id_metadata_bytes = MAX_IDEMPOTENCY_METADATA_BYTES;
        }
        assert!(matches!(
            journal
                .append_outcome(&ingress(&journal, &owner), &head, "session", None, payload)
                .unwrap(),
            AppendOutcome::Duplicate { .. }
        ));
        assert_eq!(
            journal.append_outcome(
                &ingress(&journal, &owner),
                &head,
                "session",
                None,
                TestPayload::new("unseen"),
            ),
            Err(LiveEventJournalError::IdempotencyCapacityExceeded)
        );
        assert_eq!(journal.checkpoint(&owner).unwrap(), head);
    }

    #[test]
    fn exact_tombstone_count_boundary_rolls_over_without_losing_retry_fences() {
        let root = private_tempdir();
        let path = root.path().join("journal");
        let owner = owner("opaque-account", 0);
        let journal = LiveEventJournal::<TestPayload>::open(path.clone(), limits(1)).unwrap();
        journal.checkpoint(&owner).unwrap();
        let before_last = {
            let mut state = journal.lock_state().unwrap();
            let account = state.accounts.get_mut(&owner.account_key).unwrap();
            account.event_ids.clear();
            account.event_id_metadata_bytes = 0;
            for sequence in 1..u64::try_from(MAX_IDEMPOTENCY_EVENT_IDS).unwrap() {
                let record = StoredEventId {
                    event_id: format!("seed-{sequence}"),
                    sequence,
                    commitment: "a".repeat(ACCOUNT_KEY_HEX_BYTES),
                };
                account.event_id_metadata_bytes += encoded_event_id_bytes(&record).unwrap();
                account.event_ids.insert(record.event_id.clone(), record);
            }
            account.head_sequence = u64::try_from(MAX_IDEMPOTENCY_EVENT_IDS - 1).unwrap();
            let checkpoint_bytes = b"absolute projection before final tombstone".to_vec();
            account.checkpoint = Some(StoredCheckpoint {
                schema: CHECKPOINT_SCHEMA.to_string(),
                through_sequence: account.head_sequence,
                commitment: bytes_commitment(&checkpoint_bytes),
                bytes: checkpoint_bytes,
            });
            journal.replace_account_file(&owner, account).unwrap();
            current_cursor(account)
        };

        let final_payload = TestPayload::new("final tombstone");
        let at_capacity = match journal
            .append_outcome(
                &ingress(&journal, &owner),
                &before_last,
                "session",
                None,
                final_payload.clone(),
            )
            .unwrap()
        {
            AppendOutcome::Inserted(cursor) => cursor,
            outcome => panic!("expected final insertion, got {outcome:?}"),
        };
        assert_eq!(
            at_capacity.sequence(),
            u64::try_from(MAX_IDEMPOTENCY_EVENT_IDS).unwrap()
        );
        assert!(matches!(
            journal
                .append_outcome(
                    &ingress(&journal, &owner),
                    &before_last,
                    "session",
                    None,
                    final_payload.clone(),
                )
                .unwrap(),
            AppendOutcome::Duplicate { .. }
        ));
        assert_eq!(
            journal.append_outcome(
                &ingress(&journal, &owner),
                &at_capacity,
                "session",
                None,
                TestPayload::new("one beyond capacity"),
            ),
            Err(LiveEventJournalError::IdempotencyCapacityExceeded)
        );

        journal
            .store_checkpoint(
                &owner,
                &at_capacity,
                b"absolute projection at exact capacity",
            )
            .unwrap();
        let lease = journal.activate_account(&owner).unwrap();
        let obligation = journal
            .prepare_rollover(
                &lease,
                &at_capacity,
                b"absolute projection at exact capacity",
            )
            .unwrap();
        let activation = journal
            .commit_rollover(&obligation, b"absolute projection at exact capacity")
            .unwrap();
        let (fresh_lease, fresh) = activation.into_parts();
        assert_eq!(fresh.sequence(), 0);
        assert_ne!(fresh.journal_id(), at_capacity.journal_id());
        assert_eq!(
            journal.append_outcome(
                &ingress(&journal, &owner),
                &at_capacity,
                "session",
                None,
                final_payload,
            ),
            Err(LiveEventJournalError::JournalReplaced)
        );
        let fresh_payload = TestPayload::new("first fresh event");
        let first_fresh = journal
            .append_outcome(
                &journal.bind_ingress(&fresh_lease).unwrap(),
                &fresh,
                "session",
                None,
                fresh_payload.clone(),
            )
            .unwrap();
        assert_eq!(first_fresh.cursor().sequence(), 1);
        assert!(matches!(
            journal
                .append_outcome(
                    &ingress(&journal, &owner),
                    &fresh,
                    "session",
                    None,
                    fresh_payload
                )
                .unwrap(),
            AppendOutcome::Duplicate { .. }
        ));
        drop(journal);

        let journal = LiveEventJournal::<TestPayload>::open(path, limits(1)).unwrap();
        assert_eq!(
            journal.checkpoint(&owner).unwrap(),
            first_fresh.cursor().clone()
        );
        assert_eq!(
            journal.load_checkpoint(&owner).unwrap().unwrap().bytes,
            b"absolute projection at exact capacity"
        );
    }

    #[test]
    fn maximum_checkpoint_and_tombstone_metadata_reopen_within_disk_bound() {
        let root = private_tempdir();
        let path = root.path().join("journal");
        let owner = owner("opaque-account", 0);
        let journal = LiveEventJournal::<TestPayload>::open(path.clone(), limits(1)).unwrap();
        let mut account = journal.empty_account(&owner).unwrap();
        for index in 0..MAX_IDEMPOTENCY_EVENT_IDS {
            let prefix = format!("{index:016x}");
            let event_id = format!(
                "{prefix}{}",
                "\"".repeat(MAX_EVENT_OWNER_ID_BYTES - prefix.len())
            );
            let record = StoredEventId {
                event_id,
                sequence: u64::try_from(index + 1).unwrap(),
                commitment: "a".repeat(ACCOUNT_KEY_HEX_BYTES),
            };
            let encoded_bytes = encoded_event_id_bytes(&record).unwrap();
            if account
                .event_id_metadata_bytes
                .checked_add(encoded_bytes)
                .is_none_or(|total| total > MAX_IDEMPOTENCY_METADATA_BYTES)
            {
                break;
            }
            account.event_id_metadata_bytes += encoded_bytes;
            account.event_ids.insert(record.event_id.clone(), record);
        }
        assert!(account.event_id_metadata_bytes > MAX_IDEMPOTENCY_METADATA_BYTES - 512);
        account.head_sequence = u64::try_from(account.event_ids.len()).unwrap();
        let checkpoint_bytes = vec![0x5a; MAX_CHECKPOINT_BYTES];
        account.checkpoint = Some(StoredCheckpoint {
            schema: CHECKPOINT_SCHEMA.to_string(),
            through_sequence: account.head_sequence,
            commitment: bytes_commitment(&checkpoint_bytes),
            bytes: checkpoint_bytes,
        });
        journal.replace_account_file(&owner, &mut account).unwrap();
        let file_len = fs::metadata(journal.journal_path(&owner)).unwrap().len();
        assert!(file_len <= journal.inner.limits.max_disk_bytes().unwrap());
        drop(journal);

        let journal = LiveEventJournal::<TestPayload>::open(path, limits(1)).unwrap();
        let loaded = journal.load_checkpoint(&owner).unwrap().unwrap();
        assert_eq!(loaded.through_cursor.sequence(), account.head_sequence);
        assert_eq!(loaded.bytes.len(), MAX_CHECKPOINT_BYTES);
    }

    #[test]
    fn cursor_below_head_with_no_retained_suffix_requires_snapshot() {
        let root = private_tempdir();
        let journal = LiveEventJournal::open(root.path().join("journal"), limits(4)).unwrap();
        let owner = owner("opaque-account", 0);
        let start = journal.checkpoint(&owner).unwrap();
        let head = journal
            .append(&owner, "session", None, TestPayload::new("one"))
            .unwrap();
        journal
            .store_checkpoint(&owner, &head, b"absolute projection")
            .unwrap();
        {
            let mut state = journal.lock_state().unwrap();
            let account = state.accounts.get_mut(&owner.account_key).unwrap();
            account.entries.clear();
            account.total_payload_bytes = 0;
            journal.replace_account_file(&owner, account).unwrap();
        }
        assert_eq!(
            journal.replay_after(&owner, &start, 4).unwrap(),
            LiveReplayRead::SnapshotRequired(SnapshotRequired {
                reason: SnapshotRequiredReason::RetentionGap,
                current_cursor: head.clone(),
            })
        );
        assert_eq!(
            journal.replay_after(&owner, &head, 4).unwrap(),
            LiveReplayRead::Events {
                entries: Vec::new(),
                next_cursor: head,
                has_more: false,
            }
        );
    }

    #[test]
    fn low_water_compaction_never_evicts_the_event_being_appended() {
        let root = private_tempdir();
        let mut compact_limits = limits(2);
        compact_limits.max_payload_bytes = 2_048;
        compact_limits.max_total_payload_bytes = 2_500;
        compact_limits.max_replay_payload_bytes = 2_500;
        let journal = LiveEventJournal::open(root.path().join("journal"), compact_limits).unwrap();
        let owner = owner("opaque-account", 0);
        journal
            .append(&owner, "session", None, TestPayload::new("small"))
            .unwrap();
        journal
            .append(&owner, "session", None, TestPayload::new("other"))
            .unwrap();
        let projection_head = journal.checkpoint(&owner).unwrap();
        journal
            .store_checkpoint(&owner, &projection_head, b"absolute projection")
            .unwrap();
        let newest_payload = TestPayload::new("x".repeat(1_000));
        let newest_id = newest_payload.event_id.clone();
        let newest = journal
            .append(&owner, "session", None, newest_payload)
            .unwrap();
        let cursor_before_newest = LiveEventCursor::new(
            newest.journal_id().to_string(),
            newest.sequence().checked_sub(1).unwrap(),
        );
        let (entries, next, _) = events(
            journal
                .replay_after(&owner, &cursor_before_newest, 2)
                .unwrap(),
        );
        assert_eq!(next, newest);
        assert_eq!(entries.last().unwrap().payload().event_id, newest_id);
    }

    #[test]
    fn incomplete_rotation_blocks_new_owner_until_authorized_clear() {
        let root = private_tempdir();
        let journal = LiveEventJournal::open(root.path().join("journal"), limits(10)).unwrap();
        let previous = owner("opaque-account", 0);
        journal
            .append(&previous, "session", None, TestPayload::new("old"))
            .unwrap();
        let current = owner("opaque-account", 1);

        {
            let mut state = journal.lock_state().unwrap();
            let operation_token = authorize_rotation(&state.owners, &previous, &current).unwrap();
            state.owners.insert(
                current.account_key.clone(),
                JournalOwnerState::TransitionIncomplete {
                    generation: current.account_generation,
                    operation_token,
                },
            );
            state.accounts.remove(&current.account_key);
        }

        assert_eq!(
            journal.checkpoint(&current),
            Err(LiveEventJournalError::OwnerTransitionIncomplete)
        );
        assert_eq!(
            journal.append(&current, "session", None, TestPayload::new("new")),
            Err(LiveEventJournalError::OwnerTransitionIncomplete)
        );
        let reset = journal.clear_account(&current).unwrap();
        assert_eq!(reset.sequence(), 0);
        assert_eq!(journal.checkpoint(&current).unwrap(), reset);
    }

    #[test]
    fn owner_transition_and_append_share_one_serialization_boundary() {
        let root = private_tempdir();
        let journal = LiveEventJournal::open(root.path().join("journal"), limits(10)).unwrap();
        let previous = owner("opaque-account", 0);
        journal.checkpoint(&previous).unwrap();
        let current = owner("opaque-account", 1);

        let state_guard = journal.lock_state().unwrap();
        let rotated = Arc::new(std::sync::atomic::AtomicBool::new(false));
        let rotated_worker = Arc::clone(&rotated);
        let worker_journal = journal.clone();
        let worker_previous = previous.clone();
        let worker_current = current.clone();
        let worker = thread::spawn(move || {
            worker_journal
                .rotate_account_generation(&worker_previous, &worker_current)
                .unwrap();
            rotated_worker.store(true, std::sync::atomic::Ordering::Release);
        });
        thread::sleep(std::time::Duration::from_millis(20));
        assert!(!rotated.load(std::sync::atomic::Ordering::Acquire));
        drop(state_guard);
        worker.join().unwrap();
        assert!(rotated.load(std::sync::atomic::Ordering::Acquire));
        assert_eq!(
            journal.append(&previous, "session", None, TestPayload::new("late")),
            Err(LiveEventJournalError::OwnerGenerationMismatch)
        );
    }

    #[test]
    fn indeterminate_recovery_resyncs_before_returning_a_deduped_cursor() {
        let root = private_tempdir();
        let journal = LiveEventJournal::open(root.path().join("journal"), limits(10)).unwrap();
        let owner = owner("opaque-account", 0);
        let payload = TestPayload::new("ambiguous");
        let cursor = journal
            .append(&owner, "session", None, payload.clone())
            .unwrap();
        {
            let mut state = journal.lock_state().unwrap();
            mark_owner_indeterminate(&mut state.owners, &owner).unwrap();
            state.accounts.remove(&owner.account_key);
        }

        let retried = journal.append(&owner, "session", None, payload).unwrap();
        assert_eq!(retried, cursor);
        let state = journal.lock_state().unwrap();
        assert!(matches!(
            state.owners.get(&owner.account_key),
            Some(JournalOwnerState::Active {
                generation,
                needs_resync: false,
                ..
            }) if *generation == owner.account_generation
        ));
    }

    #[test]
    fn replay_is_bounded_and_reports_more_without_advancing_past_delivery() {
        let root = private_tempdir();
        let journal =
            LiveEventJournal::<TestPayload>::open(root.path().join("journal"), limits(4)).unwrap();
        let owner = owner("opaque-account", 0);
        let start = journal.checkpoint(&owner).unwrap();
        for value in ["one", "two", "three"] {
            journal
                .append(&owner, "session", None, TestPayload::new(value))
                .unwrap();
        }
        let (first_page, next, has_more) = events(journal.replay_after(&owner, &start, 2).unwrap());
        assert_eq!(first_page.len(), 2);
        assert!(has_more);
        assert_eq!(next.sequence(), 2);
        let (second_page, final_cursor, has_more) =
            events(journal.replay_after(&owner, &next, 2).unwrap());
        assert_eq!(second_page.len(), 1);
        assert!(!has_more);
        assert_eq!(final_cursor.sequence(), 3);
    }

    #[test]
    fn replay_response_is_also_bounded_below_the_transport_frame() {
        let root = private_tempdir();
        let mut replay_limits = limits(4);
        replay_limits.max_replay_payload_bytes = 2_048;
        let journal = LiveEventJournal::open(root.path().join("journal"), replay_limits).unwrap();
        let owner = owner("opaque-account", 0);
        let start = journal.checkpoint(&owner).unwrap();
        for marker in ["a", "b", "c"] {
            journal
                .append(
                    &owner,
                    "session",
                    None,
                    TestPayload::new(format!("{marker}{}", "x".repeat(899))),
                )
                .unwrap();
        }

        let (entries, next, has_more) = events(journal.replay_after(&owner, &start, 4).unwrap());
        assert_eq!(entries.len(), 2);
        assert!(has_more);
        assert_eq!(next.sequence(), 2);
    }

    #[test]
    fn cursor_ahead_requires_snapshot() {
        let root = private_tempdir();
        let journal =
            LiveEventJournal::<TestPayload>::open(root.path().join("journal"), limits(4)).unwrap();
        let owner = owner("opaque-account", 0);
        let current = journal.checkpoint(&owner).unwrap();
        let ahead = LiveEventCursor::new(current.journal_id().to_string(), 9);
        assert_eq!(
            journal.replay_after(&owner, &ahead, 4).unwrap(),
            LiveReplayRead::SnapshotRequired(SnapshotRequired {
                reason: SnapshotRequiredReason::CursorAhead,
                current_cursor: current,
            })
        );
    }

    #[test]
    fn malformed_owners_payloads_cursors_and_limits_are_rejected() {
        assert_eq!(
            LiveEventAccountOwner::new("", 0),
            Err(LiveEventJournalError::InvalidAccountOwner)
        );
        let root = private_tempdir();
        let journal = LiveEventJournal::open(root.path().join("journal"), limits(4)).unwrap();
        let owner = owner("opaque-account", 0);
        assert_eq!(
            journal.append(&owner, "", None, TestPayload::new("value")),
            Err(LiveEventJournalError::InvalidEventOwner)
        );
        assert_eq!(
            journal.append(&owner, "session", None, TestPayload::new("x".repeat(1_025))),
            Err(LiveEventJournalError::PayloadTooLarge)
        );
        assert_eq!(
            journal.replay_after(&owner, &LiveEventCursor::new("bad".into(), 0), 4),
            Err(LiveEventJournalError::InvalidCursor)
        );
        assert_eq!(
            journal.replay_after(&owner, &journal.checkpoint(&owner).unwrap(), 0),
            Err(LiveEventJournalError::InvalidReplayLimit)
        );
        assert!(matches!(
            LiveEventJournal::<TestPayload>::open(
                root.path().join("invalid"),
                LiveEventJournalLimits {
                    max_entries: 1,
                    max_payload_bytes: 2,
                    max_total_payload_bytes: 1,
                    max_replay_entries: 1,
                    max_replay_payload_bytes: 2,
                }
            ),
            Err(LiveEventJournalError::InvalidLimits)
        ));
    }

    #[test]
    fn platform_gate_rejects_unimplemented_durability_targets() {
        assert!(durable_journal_platform_supported("macos"));
        assert!(durable_journal_platform_supported("linux"));
        assert!(!durable_journal_platform_supported("windows"));
        assert!(!durable_journal_platform_supported("freebsd"));
        if durable_journal_platform_supported(std::env::consts::OS) {
            assert_eq!(ensure_supported_platform(), Ok(()));
        } else {
            assert_eq!(
                ensure_supported_platform(),
                Err(LiveEventJournalError::UnsupportedPlatform)
            );
        }
    }

    #[test]
    fn cursor_parts_are_checked_without_exposing_the_constructor() {
        let journal_id = "a".repeat(JOURNAL_ID_HEX_BYTES);
        let cursor = LiveEventCursor::try_from_parts(journal_id.clone(), 42).unwrap();
        assert_eq!(cursor.journal_id(), journal_id);
        assert_eq!(cursor.sequence(), 42);
        assert_eq!(cursor.beginning().sequence(), 0);
        assert_eq!(
            LiveEventCursor::try_from_parts("not-a-journal".to_string(), 0),
            Err(LiveEventJournalError::InvalidCursor)
        );
        assert_eq!(
            LiveEventCursor::try_from_parts(
                "a".repeat(JOURNAL_ID_HEX_BYTES),
                MAX_CURSOR_SEQUENCE + 1
            ),
            Err(LiveEventJournalError::InvalidCursor)
        );
    }

    #[cfg(unix)]
    #[test]
    fn dedicated_parent_helper_creates_and_normalizes_only_its_leaf() {
        use std::os::unix::fs::PermissionsExt;

        let broad_parent = private_tempdir();
        fs::set_permissions(broad_parent.path(), fs::Permissions::from_mode(0o755)).unwrap();
        let dedicated = broad_parent.path().join("agent-live-events");
        prepare_live_event_journal_parent(&dedicated).unwrap();
        assert_eq!(
            fs::metadata(&dedicated).unwrap().permissions().mode() & 0o777,
            0o700
        );

        fs::set_permissions(&dedicated, fs::Permissions::from_mode(0o755)).unwrap();
        prepare_live_event_journal_parent(&dedicated).unwrap();
        assert_eq!(
            fs::metadata(&dedicated).unwrap().permissions().mode() & 0o777,
            0o700
        );
        LiveEventJournal::<TestPayload>::open(dedicated.join("journal"), limits(4)).unwrap();
    }

    #[cfg(unix)]
    #[test]
    fn dedicated_parent_helper_rejects_symlink_and_unsafe_ancestry() {
        use std::os::unix::fs::{symlink, PermissionsExt};

        let outer = private_tempdir();
        let target = outer.path().join("target");
        create_owner_only_directory(&target).unwrap();
        let linked = outer.path().join("linked");
        symlink(&target, &linked).unwrap();
        assert_eq!(
            prepare_live_event_journal_parent(&linked),
            Err(LiveEventJournalError::StorageUnavailable)
        );

        let unsafe_ancestor = outer.path().join("unsafe");
        create_owner_only_directory(&unsafe_ancestor).unwrap();
        fs::set_permissions(&unsafe_ancestor, fs::Permissions::from_mode(0o777)).unwrap();
        let private_child = unsafe_ancestor.join("private");
        create_owner_only_directory(&private_child).unwrap();
        assert!(matches!(
            LiveEventJournal::<TestPayload>::open(private_child.join("journal"), limits(4)),
            Err(LiveEventJournalError::StorageUnavailable)
        ));
        assert_eq!(
            prepare_live_event_journal_parent(&unsafe_ancestor.join("dedicated")),
            Err(LiveEventJournalError::StorageUnavailable)
        );
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn dedicated_parent_helper_strips_acl_from_its_owned_leaf() {
        use std::process::Command;

        let broad_parent = private_tempdir();
        let dedicated = broad_parent.path().join("agent-live-events");
        prepare_live_event_journal_parent(&dedicated).unwrap();
        assert!(Command::new("/bin/chmod")
            .args(["+a", "everyone allow read"])
            .arg(&dedicated)
            .status()
            .unwrap()
            .success());
        assert!(
            macos_acl::has_extended_entries(&open_directory_no_follow(&dedicated).unwrap())
                .unwrap()
        );
        prepare_live_event_journal_parent(&dedicated).unwrap();
        assert!(
            !macos_acl::has_extended_entries(&open_directory_no_follow(&dedicated).unwrap())
                .unwrap()
        );
    }

    #[test]
    fn root_creation_must_commit_its_parent_link() {
        let parent = private_tempdir();
        let path = parent.path().join("journal");
        let mut observed_parent = None;
        let result = ensure_private_directory_with_parent_sync(&path, |candidate| {
            observed_parent = Some(candidate.to_path_buf());
            Err(LiveEventJournalError::StorageUnavailable)
        });
        assert_eq!(result, Err(LiveEventJournalError::StorageUnavailable));
        assert_eq!(observed_parent.as_deref(), Some(parent.path()));
        assert!(path.is_dir());

        // A retry performs the parent sync again and can safely adopt the
        // directory created before the indeterminate first sync.
        ensure_private_directory(&path).unwrap();
        LiveEventJournal::<TestPayload>::open(path, limits(4)).unwrap();
    }

    #[cfg(unix)]
    #[test]
    fn journal_requires_an_owner_private_parent_directory() {
        use std::os::unix::fs::PermissionsExt;

        let parent = private_tempdir();
        fs::set_permissions(parent.path(), fs::Permissions::from_mode(0o755)).unwrap();
        assert!(matches!(
            LiveEventJournal::<TestPayload>::open(parent.path().join("journal"), limits(4)),
            Err(LiveEventJournalError::StorageUnavailable)
        ));

        fs::set_permissions(parent.path(), fs::Permissions::from_mode(0o700)).unwrap();
        LiveEventJournal::<TestPayload>::open(parent.path().join("journal"), limits(4)).unwrap();
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn journal_rejects_extended_acl_on_its_parent() {
        use std::process::Command;

        let parent = private_tempdir();
        assert!(Command::new("/bin/chmod")
            .args(["+a", "everyone allow add_file,delete_child"])
            .arg(parent.path())
            .status()
            .unwrap()
            .success());
        assert!(matches!(
            LiveEventJournal::<TestPayload>::open(parent.path().join("journal"), limits(4)),
            Err(LiveEventJournalError::StorageUnavailable)
        ));

        assert!(Command::new("/bin/chmod")
            .arg("-N")
            .arg(parent.path())
            .status()
            .unwrap()
            .success());
        LiveEventJournal::<TestPayload>::open(parent.path().join("journal"), limits(4)).unwrap();
    }

    #[test]
    fn metadata_errors_are_not_misclassified_as_missing_account_files() {
        let nul_path = Path::new("\0");
        assert_eq!(
            account_file_state(nul_path),
            Err(LiveEventJournalError::StorageUnavailable)
        );
    }

    #[test]
    fn replacement_of_the_locked_root_fails_closed() {
        let parent = private_tempdir();
        let path = parent.path().join("journal");
        let moved = parent.path().join("journal-moved");
        let journal = LiveEventJournal::<TestPayload>::open(path.clone(), limits(4)).unwrap();
        let owner = owner("opaque-account", 0);
        journal.checkpoint(&owner).unwrap();

        fs::rename(&path, &moved).unwrap();
        create_owner_only_directory(&path).unwrap();
        assert_eq!(
            journal.append(&owner, "session", None, TestPayload::new("late")),
            Err(LiveEventJournalError::StorageUnavailable)
        );
    }

    #[test]
    fn replacement_of_the_lock_file_fails_closed() {
        let parent = private_tempdir();
        let path = parent.path().join("journal");
        let journal = LiveEventJournal::<TestPayload>::open(path.clone(), limits(4)).unwrap();
        let owner = owner("opaque-account", 0);
        journal.checkpoint(&owner).unwrap();

        fs::rename(path.join("host.lock"), path.join("old-host.lock")).unwrap();
        File::create(path.join("host.lock")).unwrap();
        assert_eq!(
            journal.checkpoint(&owner),
            Err(LiveEventJournalError::StorageUnavailable)
        );
    }

    #[test]
    fn v3_rejects_missing_or_aliased_tombstones_even_with_valid_snapshot_integrity() {
        let parent = private_tempdir();
        let path = parent.path().join("journal");
        let owner = owner("opaque-account", 0);
        let journal = LiveEventJournal::open(path, limits(4)).unwrap();
        journal
            .append(&owner, "session", None, TestPayload::new("one"))
            .unwrap();
        let second = journal
            .append(&owner, "session", None, TestPayload::new("two"))
            .unwrap();
        journal
            .store_checkpoint(&owner, &second, b"projection")
            .unwrap();
        journal.unload_account(&owner).unwrap();
        rewrite_header(&journal.journal_path(&owner), |header| {
            header.event_ids.remove(0);
        });
        assert_eq!(
            journal.checkpoint(&owner),
            Err(LiveEventJournalError::StorageCorrupt)
        );

        journal.clear_account(&owner).unwrap();
        let first = journal
            .append(&owner, "session", None, TestPayload::new("three"))
            .unwrap();
        let second = journal
            .append(&owner, "session", None, TestPayload::new("four"))
            .unwrap();
        journal
            .store_checkpoint(&owner, &second, b"replacement projection")
            .unwrap();
        journal.unload_account(&owner).unwrap();
        rewrite_header(&journal.journal_path(&owner), |header| {
            header.event_ids[1].sequence = header.event_ids[0].sequence;
        });
        assert_eq!(
            journal.checkpoint(&owner),
            Err(LiveEventJournalError::StorageCorrupt)
        );
        assert_eq!(first.sequence(), 1);
    }

    #[test]
    fn v3_rejects_suffix_holes_and_payload_bit_flips() {
        let parent = private_tempdir();
        let path = parent.path().join("journal");
        let owner = owner("opaque-account", 0);
        {
            let journal = LiveEventJournal::<TestPayload>::open(path.clone(), limits(4)).unwrap();
            let checkpoint_head = journal
                .append(&owner, "session", None, TestPayload::new("one"))
                .unwrap();
            journal
                .store_checkpoint(&owner, &checkpoint_head, b"projection through one")
                .unwrap();
            journal
                .append(&owner, "session", None, TestPayload::new("two"))
                .unwrap();
            let head = journal
                .append(&owner, "session", None, TestPayload::new("three"))
                .unwrap();
            journal.unload_account(&owner).unwrap();
            let (header, mut entries, _) = read_v3_parts(&journal.journal_path(&owner));
            entries.remove(1);
            write_v3_parts_at_head(
                &journal.journal_path(&owner),
                &header,
                &entries,
                head.sequence(),
            );
        }
        let journal = LiveEventJournal::<TestPayload>::open(path.clone(), limits(4)).unwrap();
        assert_eq!(
            journal.checkpoint(&owner),
            Err(LiveEventJournalError::StorageCorrupt)
        );
        journal.clear_account(&owner).unwrap();
        journal
            .append(&owner, "session", None, TestPayload::new("original"))
            .unwrap();
        journal.unload_account(&owner).unwrap();
        let mut bytes = fs::read(journal.journal_path(&owner)).unwrap();
        let marker = b"original";
        let marker_offset = bytes
            .windows(marker.len())
            .position(|window| window == marker)
            .unwrap();
        bytes[marker_offset] ^= 1;
        fs::write(journal.journal_path(&owner), bytes).unwrap();
        assert_eq!(
            journal.checkpoint(&owner),
            Err(LiveEventJournalError::StorageCorrupt)
        );
    }

    #[test]
    fn v3_rejects_checkpoint_digest_mutation_even_with_recomputed_snapshot_integrity() {
        let parent = private_tempdir();
        let path = parent.path().join("journal");
        let owner = owner("opaque-account", 0);
        let journal = LiveEventJournal::<TestPayload>::open(path, limits(4)).unwrap();
        let head = journal.checkpoint(&owner).unwrap();
        journal
            .store_checkpoint(&owner, &head, b"original projection")
            .unwrap();
        journal.unload_account(&owner).unwrap();
        rewrite_header(&journal.journal_path(&owner), |header| {
            header.checkpoint.as_mut().unwrap().bytes[0] ^= 1;
        });
        assert_eq!(
            journal.load_checkpoint(&owner),
            Err(LiveEventJournalError::StorageCorrupt)
        );
    }

    #[test]
    fn over_record_journal_fails_closed_until_explicit_clear() {
        let parent = private_tempdir();
        let path = parent.path().join("journal");
        let owner = owner("opaque-account", 0);
        {
            let journal = LiveEventJournal::<TestPayload>::open(path.clone(), limits(3)).unwrap();
            for (sequence, value) in [(1, "one"), (2, "two"), (3, "three")] {
                assert_eq!(
                    journal
                        .append(&owner, "session", None, TestPayload::new(value))
                        .unwrap()
                        .sequence(),
                    sequence
                );
            }
        }

        let journal = LiveEventJournal::<TestPayload>::open(path, limits(2)).unwrap();
        assert_eq!(
            journal.checkpoint(&owner),
            Err(LiveEventJournalError::StorageCorrupt)
        );
        assert_eq!(
            journal.checkpoint(&owner),
            Err(LiveEventJournalError::StorageCorrupt)
        );
        assert_eq!(journal.clear_account(&owner).unwrap().sequence(), 0);
    }

    #[test]
    fn complete_but_corrupt_final_record_fails_closed_until_clear() {
        let parent = private_tempdir();
        let path = parent.path().join("journal");
        let owner = owner("opaque-account", 0);
        {
            let journal = LiveEventJournal::<TestPayload>::open(path.clone(), limits(4)).unwrap();
            journal
                .append(&owner, "session", None, TestPayload::new("old"))
                .unwrap();
            journal
                .append(&owner, "session", None, TestPayload::new("corrupt-tail"))
                .unwrap();
            let mut bytes = fs::read(journal.journal_path(&owner)).unwrap();
            let marker = b"corrupt-tail";
            let marker_offset = bytes
                .windows(marker.len())
                .position(|window| window == marker)
                .unwrap();
            bytes[marker_offset] ^= 1;
            fs::write(journal.journal_path(&owner), bytes).unwrap();
        }

        let journal = LiveEventJournal::<TestPayload>::open(path, limits(4)).unwrap();
        assert_eq!(
            journal.checkpoint(&owner),
            Err(LiveEventJournalError::StorageCorrupt)
        );
        assert_eq!(
            journal.checkpoint(&owner),
            Err(LiveEventJournalError::StorageCorrupt)
        );
        journal.clear_account(&owner).unwrap();
    }

    #[test]
    fn malformed_persisted_journal_id_fails_closed_until_clear() {
        let parent = private_tempdir();
        let path = parent.path().join("journal");
        let owner = owner("opaque-account", 0);
        {
            let journal = LiveEventJournal::<TestPayload>::open(path.clone(), limits(4)).unwrap();
            journal.checkpoint(&owner).unwrap();
            let mut bytes = fs::read(journal.journal_path(&owner)).unwrap();
            let anchor = select_v3_anchor(&bytes).unwrap();
            let journal_id = anchor.journal_id.as_bytes();
            let snapshot_start = usize::try_from(anchor.snapshot_offset).unwrap();
            let snapshot_end = usize::try_from(anchor.data_start).unwrap();
            let relative = bytes[snapshot_start..snapshot_end]
                .windows(journal_id.len())
                .position(|window| window == journal_id)
                .unwrap();
            bytes[snapshot_start + relative] = b'g';
            fs::write(journal.journal_path(&owner), bytes).unwrap();
        }

        let journal = LiveEventJournal::<TestPayload>::open(path, limits(4)).unwrap();
        assert_eq!(
            journal.checkpoint(&owner),
            Err(LiveEventJournalError::StorageCorrupt)
        );
        assert_eq!(journal.clear_account(&owner).unwrap().sequence(), 0);
    }

    #[test]
    fn v3_selected_anchor_rejects_terminal_frame_truncation_without_fallback() {
        let parent = private_tempdir();
        let path = parent.path().join("journal");
        let owner = owner("opaque-account", 0);
        {
            let journal = LiveEventJournal::<TestPayload>::open(path.clone(), limits(4)).unwrap();
            journal
                .append(&owner, "session", None, TestPayload::new("acknowledged"))
                .unwrap();
            let bytes = fs::read(journal.journal_path(&owner)).unwrap();
            let selected = select_v3_anchor(&bytes).unwrap();
            let older = decode_v3_anchor_slot(&bytes, 0, &selected.file_nonce)
                .unwrap()
                .unwrap();
            assert!(selected.revision > older.revision);
            assert!(selected.committed_end > older.committed_end);
            let file = open_read_write_no_follow(&journal.journal_path(&owner)).unwrap();
            // The higher anchor remains checksum-valid in the fixed prefix.
            // Recovery must not silently fall back to the older anchor merely
            // because the bytes committed by the higher one are now missing.
            file.set_len(older.committed_end).unwrap();
            file.sync_all().unwrap();
        }
        let journal = LiveEventJournal::<TestPayload>::open(path, limits(4)).unwrap();
        assert_eq!(
            journal.checkpoint(&owner),
            Err(LiveEventJournalError::StorageCorrupt)
        );
    }

    #[test]
    fn v3_selected_anchor_rejects_one_byte_terminal_truncation() {
        let parent = private_tempdir();
        let path = parent.path().join("journal");
        let owner = owner("opaque-account", 0);
        {
            let journal = LiveEventJournal::<TestPayload>::open(path.clone(), limits(4)).unwrap();
            journal
                .append(&owner, "session", None, TestPayload::new("acknowledged"))
                .unwrap();
            let bytes = fs::read(journal.journal_path(&owner)).unwrap();
            let selected = select_v3_anchor(&bytes).unwrap();
            let file = open_read_write_no_follow(&journal.journal_path(&owner)).unwrap();
            file.set_len(selected.committed_end - 1).unwrap();
            file.sync_all().unwrap();
        }
        let journal = LiveEventJournal::<TestPayload>::open(path, limits(4)).unwrap();
        assert_eq!(
            journal.checkpoint(&owner),
            Err(LiveEventJournalError::StorageCorrupt)
        );
    }

    #[test]
    fn v3_rejects_deletion_of_every_post_checkpoint_frame() {
        let parent = private_tempdir();
        let path = parent.path().join("journal");
        let owner = owner("opaque-account", 0);
        {
            let journal = LiveEventJournal::<TestPayload>::open(path.clone(), limits(4)).unwrap();
            let checkpoint_head = journal
                .append(
                    &owner,
                    "session",
                    None,
                    TestPayload::new("checkpoint event"),
                )
                .unwrap();
            journal
                .store_checkpoint(&owner, &checkpoint_head, b"absolute checkpoint")
                .unwrap();
            journal
                .append(&owner, "session", None, TestPayload::new("tail one"))
                .unwrap();
            journal
                .append(&owner, "session", None, TestPayload::new("tail two"))
                .unwrap();
            let bytes = fs::read(journal.journal_path(&owner)).unwrap();
            let anchor = select_v3_anchor(&bytes).unwrap();
            let base = v3_chain_base(&anchor).unwrap();
            let (_, checkpoint_frame_end, _) = decode_v3_frame::<TestPayload>(
                &bytes,
                usize::try_from(anchor.data_start).unwrap(),
                usize::try_from(anchor.committed_end).unwrap(),
                &base,
                MAX_CHECKPOINT_BYTES,
            )
            .unwrap();
            let file = open_read_write_no_follow(&journal.journal_path(&owner)).unwrap();
            file.set_len(u64::try_from(checkpoint_frame_end).unwrap())
                .unwrap();
            file.sync_all().unwrap();
        }
        let journal = LiveEventJournal::<TestPayload>::open(path, limits(4)).unwrap();
        assert_eq!(
            journal.load_checkpoint(&owner),
            Err(LiveEventJournalError::StorageCorrupt)
        );
    }

    #[test]
    fn v3_complete_unanchored_frame_fails_closed_without_mutation() {
        let parent = private_tempdir();
        let path = parent.path().join("journal");
        let owner = owner("opaque-account", 0);
        let (journal_path, unanchored_len) = {
            let journal = LiveEventJournal::<TestPayload>::open(path.clone(), limits(4)).unwrap();
            journal
                .append(&owner, "session", None, TestPayload::new("committed"))
                .unwrap();
            let journal_path = journal.journal_path(&owner);
            let bytes = fs::read(&journal_path).unwrap();
            let anchor = select_v3_anchor(&bytes).unwrap();
            let payload = TestPayload::new("complete but unanchored");
            let entry = StoredEntry {
                sequence: 2,
                session_id: "session".to_string(),
                run_id: None,
                commitment: event_commitment("session", None, &payload).unwrap(),
                payload,
            };
            let (frame, _) = encode_v3_frame(&entry, &anchor.committed_chain_hash).unwrap();
            let file = open_read_write_no_follow(&journal_path).unwrap();
            write_all_at(&file, &frame, anchor.committed_end).unwrap();
            file.sync_all().unwrap();
            (
                journal_path,
                anchor.committed_end + u64::try_from(frame.len()).unwrap(),
            )
        };
        let journal = LiveEventJournal::<TestPayload>::open(path, limits(4)).unwrap();
        assert_eq!(
            journal.checkpoint(&owner),
            Err(LiveEventJournalError::StorageCorrupt)
        );
        assert_eq!(fs::metadata(&journal_path).unwrap().len(), unanchored_len);
        let bytes = fs::read(journal_path).unwrap();
        assert!(bytes
            .windows(b"complete but unanchored".len())
            .any(|window| window == b"complete but unanchored"));
    }

    #[test]
    fn v3_every_nonempty_partial_anchor_write_fails_closed() {
        let parent = private_tempdir();
        let path = parent.path().join("journal");
        let owner = owner("opaque-account", 0);
        let (journal_path, pristine, anchor, frame, encoded_anchor) = {
            let journal = LiveEventJournal::<TestPayload>::open(path.clone(), limits(4)).unwrap();
            journal.checkpoint(&owner).unwrap();
            let journal_path = journal.journal_path(&owner);
            let bytes = fs::read(&journal_path).unwrap();
            let anchor = select_v3_anchor(&bytes).unwrap();
            let payload = TestPayload::new("not acknowledged");
            let entry = StoredEntry {
                sequence: 1,
                session_id: "session".to_string(),
                run_id: None,
                commitment: event_commitment("session", None, &payload).unwrap(),
                payload,
            };
            let (frame, frame_hash) =
                encode_v3_frame(&entry, &anchor.committed_chain_hash).unwrap();
            let mut next = anchor.clone();
            next.revision = 1;
            next.slot_index = 1;
            next.committed_end += u64::try_from(frame.len()).unwrap();
            next.committed_head_sequence = 1;
            next.committed_frame_count = 1;
            next.committed_chain_hash = frame_hash;
            let encoded_anchor = encode_v3_anchor(&next).unwrap();
            (journal_path, bytes, anchor, frame, encoded_anchor)
        };
        let slot_offset = DISK_SUPERBLOCK_BYTES + DISK_ANCHOR_SLOT_BYTES;
        for partial_len in 0..DISK_ANCHOR_SLOT_BYTES {
            let mut torn = pristine.clone();
            torn.extend_from_slice(&frame);
            torn[slot_offset..slot_offset + partial_len]
                .copy_from_slice(&encoded_anchor[..partial_len]);
            let slot = &torn[slot_offset..slot_offset + DISK_ANCHOR_SLOT_BYTES];
            if slot.iter().all(|byte| *byte == 0) {
                assert_eq!(select_v3_anchor(&torn).unwrap(), anchor);
            } else if slot == encoded_anchor {
                assert_eq!(select_v3_anchor(&torn).unwrap().revision, 1);
            } else {
                assert_eq!(
                    select_v3_anchor(&torn),
                    Err(LiveEventJournalError::StorageCorrupt),
                    "partial anchor length {partial_len} must fail closed"
                );
            }
        }

        let mut complete = pristine.clone();
        complete.extend_from_slice(&frame);
        complete[slot_offset..slot_offset + DISK_ANCHOR_SLOT_BYTES]
            .copy_from_slice(&encoded_anchor);
        assert_eq!(select_v3_anchor(&complete).unwrap().revision, 1);

        let mut absent = pristine.clone();
        absent.extend_from_slice(&frame);
        fs::write(&journal_path, &absent).unwrap();
        {
            let journal = LiveEventJournal::<TestPayload>::open(path.clone(), limits(4)).unwrap();
            assert_eq!(
                journal.checkpoint(&owner),
                Err(LiveEventJournalError::StorageCorrupt)
            );
            assert_eq!(fs::read(&journal_path).unwrap(), absent);
        }

        let mut torn = pristine;
        torn.extend_from_slice(&frame);
        torn[slot_offset..slot_offset + DISK_ANCHOR_SLOT_BYTES / 2]
            .copy_from_slice(&encoded_anchor[..DISK_ANCHOR_SLOT_BYTES / 2]);
        fs::write(&journal_path, &torn).unwrap();
        let torn_len = u64::try_from(torn.len()).unwrap();
        let journal = LiveEventJournal::<TestPayload>::open(path, limits(4)).unwrap();
        assert_eq!(
            journal.checkpoint(&owner),
            Err(LiveEventJournalError::StorageCorrupt)
        );
        assert_eq!(fs::metadata(journal_path).unwrap().len(), torn_len);
    }

    #[test]
    fn v3_checksum_valid_nonadjacent_anchor_slots_fail_closed() {
        let parent = private_tempdir();
        let path = parent.path().join("journal");
        let owner = owner("opaque-account", 0);
        {
            let journal = LiveEventJournal::<TestPayload>::open(path.clone(), limits(4)).unwrap();
            journal
                .append(&owner, "session", None, TestPayload::new("one"))
                .unwrap();
            let bytes = fs::read(journal.journal_path(&owner)).unwrap();
            let selected = select_v3_anchor(&bytes).unwrap();
            let mut forged = decode_v3_anchor_slot(&bytes, 0, &selected.file_nonce)
                .unwrap()
                .unwrap();
            forged.revision = 8;
            forged.slot_index = 0;
            let encoded = encode_v3_anchor(&forged).unwrap();
            let file = open_read_write_no_follow(&journal.journal_path(&owner)).unwrap();
            write_all_at(
                &file,
                &encoded,
                u64::try_from(DISK_SUPERBLOCK_BYTES).unwrap(),
            )
            .unwrap();
            file.sync_all().unwrap();
        }
        let journal = LiveEventJournal::<TestPayload>::open(path, limits(4)).unwrap();
        assert_eq!(
            journal.checkpoint(&owner),
            Err(LiveEventJournalError::StorageCorrupt)
        );
    }

    #[test]
    fn v3_checksum_corrupt_older_slot_fails_closed() {
        let parent = private_tempdir();
        let path = parent.path().join("journal");
        let owner = owner("opaque-account", 0);
        {
            let journal = LiveEventJournal::<TestPayload>::open(path.clone(), limits(4)).unwrap();
            journal
                .append(&owner, "session", None, TestPayload::new("one"))
                .unwrap();
            let bytes = fs::read(journal.journal_path(&owner)).unwrap();
            let file = open_read_write_no_follow(&journal.journal_path(&owner)).unwrap();
            let checksum_offset =
                u64::try_from(DISK_SUPERBLOCK_BYTES + DISK_ANCHOR_HASHED_BYTES).unwrap();
            let corrupted = bytes[usize::try_from(checksum_offset).unwrap()] ^ 1;
            write_all_at(&file, &[corrupted], checksum_offset).unwrap();
            file.sync_all().unwrap();
        }
        let journal = LiveEventJournal::<TestPayload>::open(path, limits(4)).unwrap();
        assert_eq!(
            journal.checkpoint(&owner),
            Err(LiveEventJournalError::StorageCorrupt)
        );
    }

    #[test]
    fn v3_checksum_corrupt_newest_slot_fails_closed_without_truncating_its_frame() {
        for relative_offset in [112usize, DISK_ANCHOR_HASHED_BYTES] {
            let parent = private_tempdir();
            let path = parent.path().join("journal");
            let owner = owner("opaque-account", 0);
            let (journal_path, committed_len) = {
                let journal =
                    LiveEventJournal::<TestPayload>::open(path.clone(), limits(4)).unwrap();
                journal
                    .append(&owner, "session", None, TestPayload::new("acknowledged"))
                    .unwrap();
                let journal_path = journal.journal_path(&owner);
                let bytes = fs::read(&journal_path).unwrap();
                let newest_slot_offset = DISK_SUPERBLOCK_BYTES + DISK_ANCHOR_SLOT_BYTES;
                let corrupt_offset = newest_slot_offset + relative_offset;
                let file = open_read_write_no_follow(&journal_path).unwrap();
                write_all_at(
                    &file,
                    &[bytes[corrupt_offset] ^ 1],
                    u64::try_from(corrupt_offset).unwrap(),
                )
                .unwrap();
                file.sync_all().unwrap();
                (journal_path, u64::try_from(bytes.len()).unwrap())
            };
            let journal = LiveEventJournal::<TestPayload>::open(path, limits(4)).unwrap();
            assert_eq!(
                journal.checkpoint(&owner),
                Err(LiveEventJournalError::StorageCorrupt),
                "newest slot corruption at relative offset {relative_offset} must fail closed"
            );
            assert_eq!(fs::metadata(journal_path).unwrap().len(), committed_len);
        }
    }

    #[test]
    fn v3_every_committed_prefix_bit_flip_fails_closed() {
        let parent = private_tempdir();
        let path = parent.path().join("journal");
        let owner = owner("prefix-bit-matrix", 0);
        let journal = LiveEventJournal::<TestPayload>::open(path, limits(4)).unwrap();
        journal
            .append(&owner, "session", None, TestPayload::new("acknowledged"))
            .unwrap();
        let bytes = fs::read(journal.journal_path(&owner)).unwrap();
        assert_eq!(select_v3_anchor(&bytes).unwrap().revision, 1);

        for offset in 0..DISK_PREFIX_BYTES {
            for bit in 0..u8::BITS {
                let mut corrupted = bytes[..DISK_PREFIX_BYTES].to_vec();
                corrupted[offset] ^= 1u8 << bit;
                assert_eq!(
                    select_v3_anchor(&corrupted),
                    Err(LiveEventJournalError::StorageCorrupt),
                    "prefix bit {bit} at byte {offset} must not select an older anchor"
                );
            }
        }
    }

    #[test]
    fn v3_zeroed_newest_slot_with_newer_frame_fails_closed_without_rollback() {
        let parent = private_tempdir();
        let path = parent.path().join("journal");
        let owner = owner("opaque-account", 0);
        let (journal_path, committed_bytes) = {
            let journal = LiveEventJournal::<TestPayload>::open(path.clone(), limits(4)).unwrap();
            journal
                .append(&owner, "session", None, TestPayload::new("acknowledged"))
                .unwrap();
            let journal_path = journal.journal_path(&owner);
            let mut bytes = fs::read(&journal_path).unwrap();
            let newest_slot_offset = DISK_SUPERBLOCK_BYTES + DISK_ANCHOR_SLOT_BYTES;
            bytes[newest_slot_offset..newest_slot_offset + DISK_ANCHOR_SLOT_BYTES].fill(0);
            fs::write(&journal_path, &bytes).unwrap();
            (journal_path, bytes)
        };
        let journal = LiveEventJournal::<TestPayload>::open(path, limits(4)).unwrap();
        assert_eq!(
            journal.checkpoint(&owner),
            Err(LiveEventJournalError::StorageCorrupt)
        );
        assert_eq!(fs::read(journal_path).unwrap(), committed_bytes);
    }

    #[test]
    fn v3_payload_independent_identity_reads_the_selected_anchor() {
        let parent = private_tempdir();
        let path = parent.path().join("journal");
        let owner = owner("opaque-account", 0);
        let journal = LiveEventJournal::<TestPayload>::open(path, limits(4)).unwrap();
        let head = journal
            .append(&owner, "session", None, TestPayload::new("one"))
            .unwrap();
        let identity = read_v3_disk_identity(
            &journal.journal_path(&owner),
            journal.inner.limits.max_disk_bytes().unwrap(),
        )
        .unwrap();
        assert_eq!(identity.account_key, owner.account_key);
        assert_eq!(identity.journal_id.as_str(), head.journal_id());
        assert_eq!(identity.committed_head_sequence, head.sequence());
        assert_eq!(identity.committed_end, identity.anchor.committed_end);
    }

    #[test]
    fn v3_golden_prefix_layout_has_one_anchor_and_exact_eof() {
        let owner = owner("golden-layout", 0);
        let mut header = JournalHeader {
            version: JOURNAL_FORMAT_VERSION,
            journal_id: "00112233445566778899aabbccddeeff".to_string(),
            account_key: owner.account_key,
            head_sequence: 0,
            checkpoint: None,
            event_ids: Vec::new(),
            integrity: String::new(),
        };
        header.integrity = journal_header_integrity(&header).unwrap();
        let encoded = encode_v3_journal::<TestPayload>(
            &header,
            &VecDeque::new(),
            [0x5au8; PROCESS_TOKEN_BYTES],
        )
        .unwrap();
        assert_eq!(DISK_SUPERBLOCK_BYTES, 80);
        assert_eq!(DISK_ANCHOR_SLOT_BYTES, 256);
        assert_eq!(DISK_PREFIX_BYTES, 592);
        assert_eq!(&encoded.bytes[0..8], DISK_SUPERBLOCK_MAGIC);
        assert_eq!(get_u32(&encoded.bytes, 8).unwrap(), 3);
        assert_eq!(get_u32(&encoded.bytes, 12).unwrap(), 80);
        assert_eq!(get_u32(&encoded.bytes, 16).unwrap(), 592);
        assert_eq!(get_u32(&encoded.bytes, 20).unwrap(), 256);
        assert_eq!(&encoded.bytes[24..40], &[0x5a; PROCESS_TOKEN_BYTES]);
        assert_eq!(
            &encoded.bytes[DISK_SUPERBLOCK_BYTES..DISK_SUPERBLOCK_BYTES + 8],
            DISK_ANCHOR_MAGIC
        );
        assert!(
            encoded.bytes[DISK_SUPERBLOCK_BYTES + DISK_ANCHOR_SLOT_BYTES..DISK_PREFIX_BYTES]
                .iter()
                .all(|byte| *byte == 0)
        );
        assert_eq!(encoded.anchor.slot_index, 0);
        assert_eq!(encoded.anchor.revision, 0);
        assert_eq!(encoded.anchor.snapshot_offset, 592);
        assert_eq!(encoded.anchor.data_start, encoded.anchor.committed_end);
        assert_eq!(
            u64::try_from(encoded.bytes.len()).unwrap(),
            encoded.anchor.committed_end
        );
        assert_eq!(select_v3_anchor(&encoded.bytes).unwrap(), encoded.anchor);
    }

    #[test]
    fn v3_rejects_frame_length_complement_corruption() {
        let parent = private_tempdir();
        let path = parent.path().join("journal");
        let owner = owner("opaque-account", 0);
        {
            let journal = LiveEventJournal::<TestPayload>::open(path.clone(), limits(4)).unwrap();
            journal
                .append(&owner, "session", None, TestPayload::new("one"))
                .unwrap();
            let mut bytes = fs::read(journal.journal_path(&owner)).unwrap();
            let anchor = select_v3_anchor(&bytes).unwrap();
            let complement_offset = usize::try_from(anchor.data_start).unwrap() + 12;
            bytes[complement_offset] ^= 1;
            fs::write(journal.journal_path(&owner), bytes).unwrap();
        }
        let journal = LiveEventJournal::<TestPayload>::open(path, limits(4)).unwrap();
        assert_eq!(
            journal.checkpoint(&owner),
            Err(LiveEventJournalError::StorageCorrupt)
        );
    }

    #[test]
    fn v2_format_fails_closed_until_explicit_clear() {
        let parent = private_tempdir();
        let path = parent.path().join("journal");
        let owner = owner("opaque-account", 0);
        let journal = LiveEventJournal::<TestPayload>::open(path, limits(4)).unwrap();
        journal
            .append(&owner, "session", None, TestPayload::new("old"))
            .unwrap();
        journal.unload_account(&owner).unwrap();
        fs::write(
            journal.journal_path(&owner),
            br#"{"version":2,"journalId":"00000000000000000000000000000000","accountKey":"aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"}
"#,
        )
        .unwrap();

        assert_eq!(
            journal.checkpoint(&owner),
            Err(LiveEventJournalError::StorageCorrupt)
        );
        assert_eq!(journal.clear_account(&owner).unwrap().sequence(), 0);
    }

    #[test]
    fn account_file_quota_bounds_root_and_process_authority_state() {
        let parent = private_tempdir();
        let path = parent.path().join("journal");
        ensure_private_directory(&path).unwrap();
        for index in 0..MAX_ACCOUNT_JOURNAL_FILES {
            File::create(path.join(format!("{index:064x}.events"))).unwrap();
        }
        let journal = LiveEventJournal::<TestPayload>::open(path, limits(4)).unwrap();
        let owner = owner("one-account-too-many", 0);
        assert_eq!(
            journal.checkpoint(&owner),
            Err(LiveEventJournalError::StorageUnavailable)
        );
        let state = journal.lock_state().unwrap();
        assert!(state.owners.len() <= MAX_ACCOUNT_JOURNAL_FILES);
        drop(state);

        let mut owners = HashMap::new();
        for index in 0..MAX_ACCOUNT_JOURNAL_FILES {
            owners.insert(
                format!("{index:064x}"),
                JournalOwnerState::Active {
                    generation: 0,
                    operation_token: new_process_token().unwrap(),
                    needs_resync: false,
                    ambiguous_append: None,
                },
            );
        }
        assert_eq!(
            authorize_active_owner(&mut owners, &owner),
            Err(LiveEventJournalError::StorageUnavailable)
        );
    }

    #[test]
    fn account_quota_covers_public_clear_and_first_rotation_paths() {
        let parent = private_tempdir();
        let clear_path = parent.path().join("clear-journal");
        let clear_journal = LiveEventJournal::<TestPayload>::open(clear_path, limits(4)).unwrap();
        for index in 0..MAX_ACCOUNT_JOURNAL_FILES {
            clear_journal
                .clear_account(&owner(&format!("account-{index}"), 0))
                .unwrap();
        }
        assert_eq!(
            clear_journal.clear_account(&owner("clear-overflow", 0)),
            Err(LiveEventJournalError::StorageUnavailable)
        );
        assert_eq!(
            clear_journal.lock_state().unwrap().owners.len(),
            MAX_ACCOUNT_JOURNAL_FILES
        );

        let rotate_path = parent.path().join("rotate-journal");
        let rotate_journal = LiveEventJournal::<TestPayload>::open(rotate_path, limits(4)).unwrap();
        for index in 0..MAX_ACCOUNT_JOURNAL_FILES {
            let scope = format!("rotate-account-{index}");
            rotate_journal
                .rotate_account_generation(&owner(&scope, 0), &owner(&scope, 1))
                .unwrap();
        }
        assert_eq!(
            rotate_journal.rotate_account_generation(
                &owner("rotate-overflow", 0),
                &owner("rotate-overflow", 1),
            ),
            Err(LiveEventJournalError::StorageUnavailable)
        );
        assert_eq!(
            rotate_journal.lock_state().unwrap().owners.len(),
            MAX_ACCOUNT_JOURNAL_FILES
        );
    }

    #[test]
    fn startup_scavenges_only_owned_temporary_files() {
        let parent = private_tempdir();
        let path = parent.path().join("journal");
        ensure_private_directory(&path).unwrap();
        let temporary = path.join(format!("{TEMP_FILE_PREFIX}abandoned"));
        File::create(&temporary).unwrap();
        LiveEventJournal::<TestPayload>::open(path, limits(4)).unwrap();
        assert!(!temporary.exists());
    }

    #[test]
    fn unanchored_partial_tail_fails_closed_without_mutation() {
        let root = private_tempdir();
        let path = root.path().join("journal");
        let owner = owner("opaque-account", 0);
        let (journal_path, damaged_len) = {
            let journal = LiveEventJournal::open(path.clone(), limits(10)).unwrap();
            journal
                .append(&owner, "session", None, TestPayload::new("first"))
                .unwrap();
            let journal_path = journal.journal_path(&owner);
            let mut file = OpenOptions::new().append(true).open(&journal_path).unwrap();
            file.write_all(b"uncommitted-partial-v3-frame").unwrap();
            file.sync_all().unwrap();
            (journal_path, file.metadata().unwrap().len())
        };

        let journal = LiveEventJournal::<TestPayload>::open(path, limits(10)).unwrap();
        assert_eq!(
            journal.checkpoint(&owner),
            Err(LiveEventJournalError::StorageCorrupt)
        );
        assert_eq!(fs::metadata(&journal_path).unwrap().len(), damaged_len);
        let on_disk = fs::read(journal_path).unwrap();
        assert!(on_disk
            .windows(b"uncommitted-partial-v3-frame".len())
            .any(|window| window == b"uncommitted-partial-v3-frame"));
    }

    #[test]
    fn explicit_retirement_fences_stale_leases_and_token_replay() {
        let root = private_tempdir();
        let path = root.path().join("journal");
        let owner = owner("opaque-account", 0);
        let journal = LiveEventJournal::open(path, limits(10)).unwrap();
        let lease = journal.activate_account(&owner).unwrap();
        let start = journal.checkpoint(&lease).unwrap();
        let ingress = journal.bind_ingress(&lease).unwrap();
        let payload = TestPayload::new("before-retirement");
        let head = journal
            .append_outcome(&ingress, &start, "session", None, payload)
            .unwrap()
            .cursor()
            .clone();

        assert_eq!(
            journal.seal_for_retirement(&lease, &start),
            Err(LiveEventJournalError::HeadChanged)
        );
        let retirement = journal.seal_for_retirement(&lease, &head).unwrap();
        assert_eq!(
            journal.checkpoint(&lease),
            Err(LiveEventJournalError::JournalRetired)
        );
        journal.retire_account(&retirement).unwrap();
        assert!(!journal.journal_path(&owner).exists());
        assert_eq!(
            journal.checkpoint(&lease),
            Err(LiveEventJournalError::JournalRetired)
        );
        assert!(!journal.journal_path(&owner).exists());

        let replacement_lease = journal.activate_account(&owner).unwrap();
        let replacement = journal.checkpoint(&replacement_lease).unwrap();
        assert_ne!(replacement.journal_id(), head.journal_id());
        assert_ne!(replacement_lease.operation_token, lease.operation_token);
        assert_eq!(
            journal.retire_account(&retirement),
            Err(LiveEventJournalError::JournalRetired)
        );
        assert!(journal.journal_path(&owner).exists());
        assert_eq!(
            journal.replay_after(&replacement_lease, &head, 10).unwrap(),
            LiveReplayRead::SnapshotRequired(SnapshotRequired {
                reason: SnapshotRequiredReason::JournalReplaced,
                current_cursor: replacement,
            })
        );
    }

    #[test]
    fn authoritative_reseed_binds_observation_owner_projection_and_seal() {
        let root = private_tempdir();
        let path = root.path().join("journal");
        let owner = owner("reseed-account", 0);
        let journal = LiveEventJournal::<TestPayload>::open(path, limits(4)).unwrap();
        let stale_lease = journal.activate_account(&owner).unwrap();
        journal.unload_account(&stale_lease).unwrap();
        fs::write(journal.journal_path(&owner), b"legacy-or-corrupt-v2\n").unwrap();

        let required = reseed_required(journal.activate_account(&owner));
        assert_eq!(required.owner(), &owner);
        assert_eq!(
            journal.checkpoint(&stale_lease),
            Err(LiveEventJournalError::ReseedRequired)
        );
        let wrong_owner = self::owner("wrong-reseed-account", 0);
        assert!(matches!(
            journal.prepare_reseed_parts(
                required,
                &wrong_owner,
                b"absolute projection",
                [1; 32],
                [2; 32],
            ),
            Err(LiveEventJournalError::InvalidCheckpoint)
        ));

        let changed_required = reseed_required(journal.activate_account(&owner));
        fs::write(
            journal.journal_path(&owner),
            b"changed-corrupt-generation\n",
        )
        .unwrap();
        assert!(matches!(
            journal.prepare_reseed_parts(
                changed_required,
                &owner,
                b"absolute projection",
                [1; 32],
                [2; 32],
            ),
            Err(LiveEventJournalError::JournalReplaced)
        ));

        let required = reseed_required(journal.activate_account(&owner));
        let duplicate_required = reseed_required(journal.activate_account(&owner));
        let mut obligation = journal
            .prepare_reseed_parts(required, &owner, b"absolute projection", [1; 32], [2; 32])
            .unwrap();
        assert!(matches!(
            journal.prepare_reseed_parts(
                duplicate_required,
                &owner,
                b"absolute projection",
                [1; 32],
                [3; 32],
            ),
            Err(LiveEventJournalError::JournalReplaced)
        ));
        assert!(matches!(
            journal.commit_reseed(&obligation),
            Err(LiveEventJournalError::OwnerTransitionIncomplete)
        ));
        journal.mark_reseed_sealed(&mut obligation).unwrap();
        let expected_journal_id = obligation.new_journal_id.clone();
        let activation = journal.commit_reseed(&obligation).unwrap();
        let (lease, cursor) = activation.into_parts();
        assert_eq!(cursor.journal_id(), expected_journal_id);
        assert_eq!(cursor.sequence(), 0);
        assert_eq!(
            journal.load_checkpoint(&lease).unwrap().unwrap(),
            LiveProjectionCheckpoint {
                through_cursor: cursor,
                bytes: b"absolute projection".to_vec(),
            }
        );
        assert_eq!(
            journal.checkpoint(&stale_lease),
            Err(LiveEventJournalError::JournalRetired)
        );
        assert!(matches!(
            journal.commit_reseed(&obligation),
            Err(LiveEventJournalError::JournalReplaced)
        ));
    }

    #[test]
    fn malformed_persisted_event_id_requires_authoritative_reseed() {
        let root = private_tempdir();
        let path = root.path().join("journal");
        let owner = owner("malformed-persisted-event-id", 0);
        {
            let journal = LiveEventJournal::<TestPayload>::open(path.clone(), limits(4)).unwrap();
            let lease = journal.activate_account(&owner).unwrap();
            let start = journal.checkpoint(&lease).unwrap();
            let ingress = journal.bind_ingress(&lease).unwrap();
            let head = journal
                .append_outcome(
                    &ingress,
                    &start,
                    "session",
                    None,
                    TestPayload::new("persisted event"),
                )
                .unwrap()
                .cursor()
                .clone();
            journal
                .store_checkpoint(&lease, &head, b"projection before corruption")
                .unwrap();
            journal.unload_account(&lease).unwrap();
            rewrite_header(&journal.journal_path(&owner), |header| {
                header.event_ids[0].event_id.clear();
            });
        }

        let journal = LiveEventJournal::<TestPayload>::open(path, limits(4)).unwrap();
        let required = reseed_required(journal.activate_account(&owner));
        let mut obligation = journal
            .prepare_reseed_parts(
                required,
                &owner,
                b"authoritative projection after corrupt event ID",
                [7; 32],
                [8; 32],
            )
            .unwrap();
        journal.mark_reseed_sealed(&mut obligation).unwrap();
        let activation = journal.commit_reseed(&obligation).unwrap();
        let (lease, cursor) = activation.into_parts();
        assert_eq!(cursor.sequence(), 0);
        assert_eq!(
            journal.load_checkpoint(&lease).unwrap().unwrap(),
            LiveProjectionCheckpoint {
                through_cursor: cursor,
                bytes: b"authoritative projection after corrupt event ID".to_vec(),
            }
        );
    }

    #[test]
    fn reseed_replacement_boundaries_reopen_as_exactly_observed_or_fresh() {
        for (boundary, replacement_committed) in [
            (ReplaceFailureBoundary::BeforeFileSync, false),
            (ReplaceFailureBoundary::AfterFileSync, false),
            (ReplaceFailureBoundary::AfterPersist, true),
            (ReplaceFailureBoundary::AfterDirectorySync, true),
        ] {
            let root = private_tempdir();
            let path = root.path().join("journal");
            let owner = owner(&format!("reseed-boundary-{boundary:?}"), 0);
            let expected_journal_id = {
                let journal =
                    LiveEventJournal::<TestPayload>::open(path.clone(), limits(4)).unwrap();
                let lease = journal.activate_account(&owner).unwrap();
                journal.unload_account(&lease).unwrap();
                fs::write(journal.journal_path(&owner), b"broken-v2\n").unwrap();
                let required = reseed_required(journal.activate_account(&owner));
                let mut obligation = journal
                    .prepare_reseed_parts(
                        required,
                        &owner,
                        b"authoritative absolute projection",
                        [3; 32],
                        [4; 32],
                    )
                    .unwrap();
                journal.mark_reseed_sealed(&mut obligation).unwrap();
                let expected_journal_id = obligation.new_journal_id.clone();
                journal.fail_next_replace_at(boundary);
                assert!(matches!(
                    journal.commit_reseed(&obligation),
                    Err(LiveEventJournalError::StorageUnavailable)
                ));
                expected_journal_id
            };

            let journal = LiveEventJournal::<TestPayload>::open(path, limits(4)).unwrap();
            if replacement_committed {
                let lease = journal.activate_account(&owner).unwrap();
                let cursor = journal.checkpoint(&lease).unwrap();
                assert_eq!(cursor.journal_id(), expected_journal_id);
                assert_eq!(cursor.sequence(), 0);
                assert_eq!(
                    journal.load_checkpoint(&lease).unwrap().unwrap().bytes,
                    b"authoritative absolute projection"
                );
            } else {
                let required = reseed_required(journal.activate_account(&owner));
                assert_eq!(required.owner(), &owner);
            }
        }
    }

    #[test]
    fn reseed_postpersist_error_is_exactly_retryable_in_process() {
        let root = private_tempdir();
        let path = root.path().join("journal");
        let owner = owner("reseed-exact-retry", 0);
        let journal = LiveEventJournal::<TestPayload>::open(path, limits(4)).unwrap();
        let lease = journal.activate_account(&owner).unwrap();
        journal.unload_account(&lease).unwrap();
        fs::write(journal.journal_path(&owner), b"broken-v2\n").unwrap();
        let required = reseed_required(journal.activate_account(&owner));
        let mut obligation = journal
            .prepare_reseed_parts(
                required,
                &owner,
                b"authoritative absolute projection",
                [5; 32],
                [6; 32],
            )
            .unwrap();
        journal.mark_reseed_sealed(&mut obligation).unwrap();
        let expected_journal_id = obligation.new_journal_id.clone();
        journal.fail_next_replace_at(ReplaceFailureBoundary::AfterPersist);
        assert!(matches!(
            journal.commit_reseed(&obligation),
            Err(LiveEventJournalError::StorageUnavailable)
        ));
        let activation = journal.commit_reseed(&obligation).unwrap();
        assert_eq!(activation.cursor.journal_id(), expected_journal_id);
        assert_eq!(activation.cursor.sequence(), 0);
    }

    #[test]
    fn retirement_crash_boundaries_remain_fenced_and_retryable() {
        for boundary in [
            RetirementFailureBoundary::BeforeRename,
            RetirementFailureBoundary::AfterRename,
            RetirementFailureBoundary::AfterRenameDirectorySync,
            RetirementFailureBoundary::AfterUnlink,
            RetirementFailureBoundary::AfterFinalDirectorySync,
        ] {
            let root = private_tempdir();
            let path = root.path().join("journal");
            let owner = owner(&format!("retire-{boundary:?}"), 0);
            let journal = LiveEventJournal::<TestPayload>::open(path, limits(4)).unwrap();
            let lease = journal.activate_account(&owner).unwrap();
            let head = journal.checkpoint(&lease).unwrap();
            let retirement = journal.seal_for_retirement(&lease, &head).unwrap();
            let pending = journal.retirement_path(&owner, &retirement.retirement_nonce);
            journal.fail_next_retirement_at(boundary);
            assert_eq!(
                journal.retire_account(&retirement),
                Err(LiveEventJournalError::StorageUnavailable),
                "boundary {boundary:?}"
            );
            assert_eq!(
                journal.checkpoint(&lease),
                Err(LiveEventJournalError::JournalRetired),
                "boundary {boundary:?}"
            );
            match boundary {
                RetirementFailureBoundary::BeforeRename => {
                    assert!(journal.journal_path(&owner).exists());
                    assert!(!pending.exists());
                }
                RetirementFailureBoundary::AfterRename
                | RetirementFailureBoundary::AfterRenameDirectorySync => {
                    assert!(!journal.journal_path(&owner).exists());
                    assert!(pending.exists());
                }
                RetirementFailureBoundary::AfterUnlink
                | RetirementFailureBoundary::AfterFinalDirectorySync => {
                    assert!(!journal.journal_path(&owner).exists());
                    assert!(!pending.exists());
                }
                RetirementFailureBoundary::None => unreachable!(),
            }
            journal.retire_account(&retirement).unwrap();
            assert!(!journal.journal_path(&owner).exists());
            assert!(!pending.exists());
            assert!(!journal
                .lock_state()
                .unwrap()
                .owners
                .contains_key(&owner.account_key));
        }
    }

    #[test]
    fn retirement_reopen_resolves_precommit_source_or_committed_pending() {
        for boundary in [
            RetirementFailureBoundary::BeforeRename,
            RetirementFailureBoundary::AfterRename,
            RetirementFailureBoundary::AfterRenameDirectorySync,
            RetirementFailureBoundary::AfterUnlink,
            RetirementFailureBoundary::AfterFinalDirectorySync,
        ] {
            let root = private_tempdir();
            let path = root.path().join("journal");
            let owner = owner(&format!("reopen-{boundary:?}"), 0);
            let old_head = {
                let journal =
                    LiveEventJournal::<TestPayload>::open(path.clone(), limits(4)).unwrap();
                let lease = journal.activate_account(&owner).unwrap();
                let old_head = journal.checkpoint(&lease).unwrap();
                let retirement = journal.seal_for_retirement(&lease, &old_head).unwrap();
                journal.fail_next_retirement_at(boundary);
                assert_eq!(
                    journal.retire_account(&retirement),
                    Err(LiveEventJournalError::StorageUnavailable)
                );
                old_head
            };

            let journal = LiveEventJournal::<TestPayload>::open(path, limits(4)).unwrap();
            let lease = journal.activate_account(&owner).unwrap();
            let recovered = journal.checkpoint(&lease).unwrap();
            if boundary == RetirementFailureBoundary::BeforeRename {
                assert_eq!(recovered, old_head);
            } else {
                assert_ne!(recovered.journal_id(), old_head.journal_id());
                assert_eq!(recovered.sequence(), 0);
            }
        }
    }

    #[test]
    fn authorized_retirement_frees_one_of_sixty_four_account_slots() {
        let root = private_tempdir();
        let journal =
            LiveEventJournal::<TestPayload>::open(root.path().join("journal"), limits(4)).unwrap();
        let mut activated = Vec::new();
        for index in 0..MAX_ACCOUNT_JOURNAL_FILES {
            let owner = owner(&format!("quota-owner-{index}"), 0);
            let lease = journal.activate_account(&owner).unwrap();
            let head = journal.checkpoint(&lease).unwrap();
            activated.push((owner, lease, head));
        }
        let overflow_owner = owner("quota-overflow", 0);
        assert_eq!(
            journal.activate_account(&overflow_owner),
            Err(LiveEventJournalActivationError::Journal(
                LiveEventJournalError::StorageUnavailable
            ))
        );

        let (retired_owner, retired_lease, retired_head) = activated.remove(0);
        let retirement = journal
            .seal_for_retirement(&retired_lease, &retired_head)
            .unwrap();
        journal.retire_account(&retirement).unwrap();
        let overflow_lease = journal.activate_account(&overflow_owner).unwrap();
        assert_eq!(journal.checkpoint(&overflow_lease).unwrap().sequence(), 0);
        assert_eq!(
            journal.checkpoint(&retired_lease),
            Err(LiveEventJournalError::JournalRetired)
        );
        assert!(!journal.journal_path(&retired_owner).exists());
        assert_eq!(
            journal.lock_state().unwrap().owners.len(),
            MAX_ACCOUNT_JOURNAL_FILES
        );
    }

    #[test]
    fn repeated_activate_retire_cycles_do_not_accumulate_owner_tombstones() {
        let root = private_tempdir();
        let journal =
            LiveEventJournal::<TestPayload>::open(root.path().join("journal"), limits(4)).unwrap();
        let owner = owner("repeated-retirement", 0);
        let mut prior_journal_id = None;
        for _ in 0..256 {
            let lease = journal.activate_account(&owner).unwrap();
            let head = journal.checkpoint(&lease).unwrap();
            if let Some(prior) = prior_journal_id.replace(head.journal_id().to_string()) {
                assert_ne!(prior, head.journal_id());
            }
            let retirement = journal.seal_for_retirement(&lease, &head).unwrap();
            journal.retire_account(&retirement).unwrap();
            assert!(journal.lock_state().unwrap().owners.is_empty());
            assert!(!journal.journal_path(&owner).exists());
        }
        let residual_files = fs::read_dir(&journal.inner.root)
            .unwrap()
            .filter_map(Result::ok)
            .filter(|entry| {
                entry.file_name().to_str().is_some_and(|name| {
                    is_account_journal_file_name(name) || parse_retiring_file_name(name).is_some()
                })
            })
            .count();
        assert_eq!(residual_files, 0);
    }

    #[test]
    fn startup_scavenges_only_valid_pending_retirements_and_rejects_collisions() {
        let root = private_tempdir();
        let path = root.path().join("journal");
        let owner = owner("pending-account", 0);
        let (source, pending) = {
            let journal = LiveEventJournal::<TestPayload>::open(path.clone(), limits(4)).unwrap();
            let lease = journal.activate_account(&owner).unwrap();
            let head = journal.checkpoint(&lease).unwrap();
            let retirement = journal.seal_for_retirement(&lease, &head).unwrap();
            let source = journal.journal_path(&owner);
            let pending = journal.retirement_path(&owner, &retirement.retirement_nonce);
            (source, pending)
        };
        fs::copy(&source, &pending).unwrap();
        assert_eq!(
            LiveEventJournal::<TestPayload>::open(path.clone(), limits(4)).err(),
            Some(LiveEventJournalError::StorageCorrupt)
        );
        assert!(source.exists());
        assert!(pending.exists());
        fs::remove_file(&source).unwrap();
        LiveEventJournal::<TestPayload>::open(path.clone(), limits(4)).unwrap();
        assert!(!pending.exists());

        let malformed_root = private_tempdir();
        let malformed_path = malformed_root.path().join("journal");
        ensure_private_directory(&malformed_path).unwrap();
        File::create(malformed_path.join(format!("{RETIRING_FILE_PREFIX}malformed"))).unwrap();
        assert_eq!(
            LiveEventJournal::<TestPayload>::open(malformed_path, limits(4)).err(),
            Some(LiveEventJournalError::StorageCorrupt)
        );
    }

    #[test]
    fn authorized_clear_recovers_a_corrupt_journal_without_parsing_it() {
        let root = private_tempdir();
        let path = root.path().join("journal");
        let owner = owner("opaque-account", 0);
        let journal = LiveEventJournal::open(path.clone(), limits(10)).unwrap();
        let old = journal
            .append(&owner, "session", None, TestPayload::new("old"))
            .unwrap();
        journal.unload_account(&owner).unwrap();
        fs::write(journal.journal_path(&owner), b"corrupt\n").unwrap();

        let reset = journal.clear_account(&owner).unwrap();
        assert_ne!(reset.journal_id(), old.journal_id());
        assert_eq!(reset.sequence(), 0);
        assert_eq!(journal.checkpoint(&owner).unwrap(), reset);
    }

    #[test]
    fn a_second_independent_host_cannot_open_the_same_root() {
        let root = private_tempdir();
        let path = root.path().join("journal");
        let first = LiveEventJournal::<TestPayload>::open(path.clone(), limits(10)).unwrap();
        assert!(matches!(
            LiveEventJournal::<TestPayload>::open(path.clone(), limits(10)),
            Err(LiveEventJournalError::AlreadyOpen)
        ));
        drop(first);
        LiveEventJournal::<TestPayload>::open(path, limits(10)).unwrap();
    }

    #[test]
    fn cloned_journal_serializes_concurrent_appends() {
        let root = private_tempdir();
        let journal = LiveEventJournal::open(root.path().join("journal"), limits(16)).unwrap();
        let owner = owner("opaque-account", 0);
        let start = journal.checkpoint(&owner).unwrap();
        let barrier = Arc::new(Barrier::new(8));
        let mut workers = Vec::new();
        for index in 0..8 {
            let journal = journal.clone();
            let owner = owner.clone();
            let barrier = Arc::clone(&barrier);
            workers.push(thread::spawn(move || {
                barrier.wait();
                journal
                    .append(
                        &owner,
                        "session",
                        Some("run"),
                        TestPayload::new(index.to_string()),
                    )
                    .unwrap()
                    .sequence()
            }));
        }
        let sequences = workers
            .into_iter()
            .map(|worker| worker.join().unwrap())
            .collect::<BTreeSet<_>>();
        assert_eq!(sequences, (1..=8).collect());
        let (entries, _, _) = events(journal.replay_after(&owner, &start, 16).unwrap());
        assert_eq!(entries.len(), 8);
    }

    #[cfg(unix)]
    #[test]
    fn journal_directory_and_file_are_owner_only() {
        let root = private_tempdir();
        let path = root.path().join("journal");
        let journal = LiveEventJournal::open(path.clone(), limits(4)).unwrap();
        let owner = owner("opaque-account", 0);
        journal
            .append(&owner, "session", None, TestPayload::new("event"))
            .unwrap();
        assert_eq!(
            fs::metadata(&path).unwrap().permissions().mode() & 0o777,
            0o700
        );
        assert_eq!(
            fs::metadata(journal.journal_path(&owner))
                .unwrap()
                .permissions()
                .mode()
                & 0o777,
            0o600
        );
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn journal_strips_inherited_macos_acl_entries() {
        use std::process::Command;

        let parent = private_tempdir();
        let path = parent.path().join("journal");
        let owner = owner("opaque-account", 0);
        let account_path = {
            let journal = LiveEventJournal::open(path.clone(), limits(4)).unwrap();
            journal
                .append(&owner, "session", None, TestPayload::new("event"))
                .unwrap();
            journal.journal_path(&owner)
        };

        for candidate in [&path, &account_path] {
            assert!(Command::new("/bin/chmod")
                .args(["+a", "everyone allow read"])
                .arg(candidate)
                .status()
                .unwrap()
                .success());
        }
        let journal = LiveEventJournal::<TestPayload>::open(path.clone(), limits(4)).unwrap();
        journal.checkpoint(&owner).unwrap();

        for candidate in [path, account_path] {
            let output = Command::new("/bin/ls")
                .args(["-lde"])
                .arg(&candidate)
                .output()
                .unwrap();
            assert!(output.status.success());
            let listing = String::from_utf8(output.stdout).unwrap();
            assert!(
                !listing.lines().skip(1).any(|line| line.contains(" allow ")),
                "extended ACL remained on {}: {listing}",
                candidate.display()
            );
        }
    }
}
