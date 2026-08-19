//! Lower-layer, non-serializable durability capabilities shared by the Agent
//! host composition and event journal.
//!
//! These types deliberately contain no business logic and expose no general
//! constructor. The future exact Goose persistence adapter must live in the
//! private `mint` module below; until that reviewed adapter exists, authoritative
//! reseed and persisted-head acknowledgement remain impossible rather than
//! accepting renderer/provider scalars as proof.

#![allow(
    dead_code,
    reason = "the exact Goose durable-persistence adapter is a later integration slice"
)]

use crate::{
    agent::{AgentLiveEventCursor, AgentPagingError},
    agent_event_journal::LiveEventAccountOwner,
    agent_live_binding::AgentLiveBindingLease,
};

const MAX_DURABLE_STABLE_OPERATION_ID_BYTES: usize = 128;
pub(crate) const AGENT_LIVE_PROJECTION_SCHEMA_VERSION: u16 = 1;

/// Opaque identity of one native, durably recorded logical mutation.
///
/// The wire journal event ID is derived from this value plus the journal and
/// route namespace, but this capability itself is never serialized or minted
/// from a renderer/remote scalar. Reconstructing it after restart belongs to
/// the future exact Goose persistence adapter in the private `mint` module.
/// It is deliberately non-Clone so creating a new logical mutation cannot be
/// confused with copying authority; exact retries borrow the same capability.
pub(crate) struct AgentDurableStableOperationId {
    owner: AgentLiveDataOwnerKey,
    session_id: String,
    run_id: Option<String>,
    stable_id: String,
    journal_namespace_commitment: [u8; 32],
    projection_schema_version: u16,
    payload_commitment: [u8; 32],
}

impl std::fmt::Debug for AgentDurableStableOperationId {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("AgentDurableStableOperationId")
            .field("owner", &"<redacted>")
            .field("session_id", &"<redacted>")
            .field("run_id", &"<redacted>")
            .field("stable_id", &"<redacted>")
            .field("journal_namespace_commitment", &"<redacted>")
            .field("projection_schema_version", &self.projection_schema_version)
            .field("payload_commitment", &"<redacted>")
            .finish()
    }
}

impl AgentDurableStableOperationId {
    pub(crate) fn owner(&self) -> &AgentLiveDataOwnerKey {
        &self.owner
    }

    pub(crate) fn session_id(&self) -> &str {
        &self.session_id
    }

    pub(crate) fn run_id(&self) -> Option<&str> {
        self.run_id.as_deref()
    }

    pub(crate) fn as_str(&self) -> &str {
        &self.stable_id
    }

    pub(crate) const fn journal_namespace_commitment(&self) -> &[u8; 32] {
        &self.journal_namespace_commitment
    }

    pub(crate) const fn projection_schema_version(&self) -> u16 {
        self.projection_schema_version
    }

    pub(crate) const fn payload_commitment(&self) -> &[u8; 32] {
        &self.payload_commitment
    }

    #[cfg(test)]
    pub(crate) fn for_test(
        owner: AgentLiveDataOwnerKey,
        session_id: impl Into<String>,
        run_id: Option<String>,
        stable_id: impl Into<String>,
        journal_namespace_commitment: [u8; 32],
        payload_commitment: [u8; 32],
    ) -> Self {
        let session_id = session_id.into();
        let stable_id = stable_id.into();
        assert!(!session_id.is_empty());
        assert!(run_id.as_deref().is_none_or(|run_id| !run_id.is_empty()));
        assert!(is_valid_stable_operation_id(&stable_id));
        Self {
            owner,
            session_id,
            run_id,
            stable_id,
            journal_namespace_commitment,
            projection_schema_version: AGENT_LIVE_PROJECTION_SCHEMA_VERSION,
            payload_commitment,
        }
    }
}

fn is_valid_stable_operation_id(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= MAX_DURABLE_STABLE_OPERATION_ID_BYTES
        && value
            .bytes()
            .all(|byte| byte.is_ascii_graphic() && !matches!(byte, b'\\' | b'"'))
}

/// Opaque identity of one account data owner. Peer reconnect and pairing
/// lineage are intentionally absent; target or data-generation changes create
/// a different identity.
#[derive(Clone, PartialEq, Eq, Hash)]
pub(crate) struct AgentLiveDataOwnerKey {
    account_scope: String,
    account_generation: u64,
    execution_target: String,
    data_lineage_epoch: u64,
}

impl std::fmt::Debug for AgentLiveDataOwnerKey {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("AgentLiveDataOwnerKey")
            .field("account_scope", &"<redacted>")
            .field("account_generation", &self.account_generation)
            .field("execution_target", &"<redacted>")
            .field("data_lineage_epoch", &self.data_lineage_epoch)
            .finish()
    }
}

impl AgentLiveDataOwnerKey {
    pub(crate) fn from_binding_lease(lease: &AgentLiveBindingLease) -> Self {
        Self {
            account_scope: lease.account_scope().to_string(),
            account_generation: lease.account_generation(),
            execution_target: lease.execution_target().as_str().to_string(),
            data_lineage_epoch: lease.lineage_epoch(),
        }
    }

    pub(crate) const fn account_generation(&self) -> u64 {
        self.account_generation
    }

    pub(crate) fn execution_target(&self) -> &str {
        &self.execution_target
    }

    #[cfg(test)]
    pub(crate) fn for_test(
        account_scope: impl Into<String>,
        account_generation: u64,
        execution_target: impl Into<String>,
        data_lineage_epoch: u64,
    ) -> Self {
        Self {
            account_scope: account_scope.into(),
            account_generation,
            execution_target: execution_target.into(),
            data_lineage_epoch,
        }
    }
}

/// Exact durable Goose-head receipt. Non-Clone and non-serializable; no sibling
/// module can mint it from a session/revision/cursor tuple.
pub(crate) struct AgentDurableHeadCommitReceipt {
    stable_operation: AgentDurableStableOperationId,
    history_revision: String,
    through_event_cursor: AgentLiveEventCursor,
}

impl std::fmt::Debug for AgentDurableHeadCommitReceipt {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("AgentDurableHeadCommitReceipt")
            .field("stable_operation", &self.stable_operation)
            .field("history_revision", &"<redacted>")
            .field("through_event_cursor", &"<redacted>")
            .finish()
    }
}

impl AgentDurableHeadCommitReceipt {
    pub(crate) fn stable_operation(&self) -> &AgentDurableStableOperationId {
        &self.stable_operation
    }

    pub(crate) fn history_revision(&self) -> &str {
        &self.history_revision
    }

    pub(crate) fn through_event_cursor(&self) -> &AgentLiveEventCursor {
        &self.through_event_cursor
    }

    #[cfg(test)]
    pub(crate) fn for_test(
        stable_operation: AgentDurableStableOperationId,
        history_revision: impl Into<String>,
        through_event_cursor: AgentLiveEventCursor,
    ) -> Self {
        Self {
            stable_operation,
            history_revision: history_revision.into(),
            through_event_cursor,
        }
    }
}

/// One-use proof that an exact, currently bound data owner has an absolute
/// projection derived from a durably committed Goose head.
pub(crate) struct VerifiedJournalReseedAuthority {
    owner: LiveEventAccountOwner,
    binding: AgentLiveDataOwnerKey,
    projection_bytes: Box<[u8]>,
    durable_head_commitment: [u8; 32],
    nonce: [u8; 32],
}

impl std::fmt::Debug for VerifiedJournalReseedAuthority {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("VerifiedJournalReseedAuthority")
            .field("owner", &self.owner)
            .field("binding", &self.binding)
            .field(
                "projection_bytes",
                &format_args!("<{} bytes>", self.projection_bytes.len()),
            )
            .field("durable_head_commitment", &"<redacted>")
            .field("nonce", &"<redacted>")
            .finish()
    }
}

impl VerifiedJournalReseedAuthority {
    pub(crate) fn owner(&self) -> &LiveEventAccountOwner {
        &self.owner
    }

    pub(crate) fn binding_key(&self) -> &AgentLiveDataOwnerKey {
        &self.binding
    }

    pub(crate) fn projection_bytes(&self) -> &[u8] {
        &self.projection_bytes
    }

    pub(crate) const fn durable_head_commitment(&self) -> &[u8; 32] {
        &self.durable_head_commitment
    }

    pub(crate) const fn nonce(&self) -> &[u8; 32] {
        &self.nonce
    }
}

/// Only the reviewed Goose persistence adapter belongs here. Keeping minting
/// private prevents other crate siblings from turning copied scalars into a
/// durability capability. The placeholder makes the intended ownership seam
/// explicit without exposing a constructor before that adapter is implemented.
mod mint {
    use super::*;

    #[allow(unused_imports)]
    use crate::agent::AgentRuntimeHandle;

    #[allow(dead_code)]
    fn exact_goose_adapter_not_yet_integrated(
        _: &AgentRuntimeHandle,
    ) -> Result<(), AgentPagingError> {
        Err(AgentPagingError::Unavailable)
    }
}
