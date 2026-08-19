//! Fail-closed binding of synchronized Agent state to one installed Maple
//! authorization context, product host registration, controller pairing, and
//! complete transport connection stamp.
//!
//! Persisted history paging deliberately remains usable without this state.
//! Live attach/resume must retain an exact [`AgentLiveBindingLease`] and
//! revalidate it after every asynchronous boundary.

#![allow(
    dead_code,
    reason = "the verified pairing adapter is wired by the remote Agent slice"
)]

use crate::{
    remote_protocol::ConnectionStamp,
    remote_transport::{
        AuthorizationTransitionReceipt, InstalledAuthorizationContext,
        InstalledAuthorizationDomain, PairingFence, VerifiedIncomingPeerAuthorization,
    },
};
use std::{cmp::Ordering, collections::HashMap, fmt, sync::Arc};
use tokio::sync::Mutex;

const MAX_ACCOUNT_SCOPE_BYTES: usize = 256;
const MAX_HOST_REGISTRATION_ID_BYTES: usize = 128;

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub(crate) struct AgentExecutionTargetId(Arc<str>);

impl AgentExecutionTargetId {
    fn from_verified_registration(value: String) -> Result<Self, AgentLiveBindingError> {
        validate_bounded_id(&value, MAX_HOST_REGISTRATION_ID_BYTES)
            .map_err(|_| AgentLiveBindingError::InvalidVerifiedBinding)?;
        // The product target is the stable host-registration UUID, never an
        // endpoint key, hostname, or friendly/local alias.
        if !looks_like_non_nil_uuid(&value) {
            return Err(AgentLiveBindingError::InvalidVerifiedBinding);
        }
        Ok(Self(Arc::from(value)))
    }

    pub(crate) fn as_str(&self) -> &str {
        &self.0
    }
}

/// Installation-local authorization version. Independent installations never
/// compare or exchange these values; the pairing incarnation is the directed
/// shared wire fence.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub(crate) struct LocalAuthorizationContext {
    account_epoch: u64,
    snapshot_revision: u64,
    snapshot_digest: [u8; 32],
}

impl LocalAuthorizationContext {
    fn from_installed(
        installed: &InstalledAuthorizationContext,
    ) -> Result<Self, AgentLiveBindingError> {
        if installed.account_epoch() == 0 || installed.snapshot_revision() == 0 {
            return Err(AgentLiveBindingError::InvalidVerifiedBinding);
        }
        Ok(Self {
            account_epoch: installed.account_epoch(),
            snapshot_revision: installed.snapshot_revision(),
            snapshot_digest: installed.snapshot_digest(),
        })
    }

    pub(crate) const fn account_epoch(&self) -> u64 {
        self.account_epoch
    }

    pub(crate) const fn snapshot_revision(&self) -> u64 {
        self.snapshot_revision
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

/// Capability minted only after the endpoint revalidates an authenticated
/// controller against its currently installed authorization snapshot.
///
/// This constructor never accepts pairing payload lifecycle revisions or
/// renderer-supplied target/stamp fields.
pub(crate) struct VerifiedAgentTargetBinding {
    remote_authority: Option<VerifiedIncomingPeerAuthorization>,
    account_scope: String,
    account_generation: u64,
    execution_target: AgentExecutionTargetId,
    controller_endpoint: iroh::EndpointId,
    authorization: LocalAuthorizationContext,
    pairing_fence: PairingFence,
    connection_stamp: ConnectionStamp,
}

impl VerifiedAgentTargetBinding {
    pub(crate) fn from_verified_remote_adapter(
        account_scope: String,
        account_generation: u64,
        installed: VerifiedIncomingPeerAuthorization,
    ) -> Result<Self, AgentLiveBindingError> {
        installed
            .revalidate_current()
            .map_err(|_| AgentLiveBindingError::StaleBinding)?;
        validate_bounded_id(&account_scope, MAX_ACCOUNT_SCOPE_BYTES)
            .map_err(|_| AgentLiveBindingError::InvalidVerifiedBinding)?;
        let execution_target_id = installed.execution_target_id().to_string();
        let controller_endpoint = installed.controller_endpoint();
        let authorization = LocalAuthorizationContext::from_installed(installed.authorization())?;
        let pairing_fence = installed.pairing_fence();
        let connection_stamp = installed.connection_stamp();
        Ok(Self {
            remote_authority: Some(installed),
            account_scope,
            account_generation,
            execution_target: AgentExecutionTargetId::from_verified_registration(
                // Fields were copied above only after the opaque native
                // capability revalidated its current admission record.
                execution_target_id.to_string(),
            )?,
            controller_endpoint,
            authorization,
            pairing_fence,
            connection_stamp,
        })
    }

    fn revalidate_current(&self) -> Result<(), AgentLiveBindingError> {
        match self.remote_authority.as_ref() {
            Some(authority) => authority
                .revalidate_current()
                .map_err(|_| AgentLiveBindingError::StaleBinding),
            #[cfg(test)]
            None => Ok(()),
            #[cfg(not(test))]
            None => Err(AgentLiveBindingError::InvalidVerifiedBinding),
        }
    }
}

/// Exact peer access lease. Data, pairing, and transport lineages are retained
/// independently so an ordinary reconnect never rotates persisted history.
#[derive(Debug, Clone)]
pub(crate) struct AgentLiveBindingLease {
    account_scope: Arc<str>,
    account_generation: u64,
    execution_target: AgentExecutionTargetId,
    controller_endpoint: iroh::EndpointId,
    authorization: LocalAuthorizationContext,
    pairing_fence: PairingFence,
    connection_stamp: ConnectionStamp,
    data_lineage_epoch: u64,
    peer_lineage_epoch: u64,
    remote_authority: Option<VerifiedIncomingPeerAuthorization>,
}

impl PartialEq for AgentLiveBindingLease {
    fn eq(&self, other: &Self) -> bool {
        self.account_scope == other.account_scope
            && self.account_generation == other.account_generation
            && self.execution_target == other.execution_target
            && self.controller_endpoint == other.controller_endpoint
            && self.authorization == other.authorization
            && self.pairing_fence == other.pairing_fence
            && self.connection_stamp == other.connection_stamp
            && self.data_lineage_epoch == other.data_lineage_epoch
            && self.peer_lineage_epoch == other.peer_lineage_epoch
            && same_remote_authority_instance(
                self.remote_authority.as_ref(),
                other.remote_authority.as_ref(),
            )
    }
}

impl Eq for AgentLiveBindingLease {}

impl AgentLiveBindingLease {
    pub(crate) fn account_scope(&self) -> &str {
        &self.account_scope
    }

    pub(crate) const fn account_generation(&self) -> u64 {
        self.account_generation
    }

    pub(crate) fn execution_target(&self) -> &AgentExecutionTargetId {
        &self.execution_target
    }

    pub(crate) const fn controller_endpoint(&self) -> iroh::EndpointId {
        self.controller_endpoint
    }

    pub(crate) fn authorization(&self) -> &LocalAuthorizationContext {
        &self.authorization
    }

    pub(crate) const fn pairing_fence(&self) -> PairingFence {
        self.pairing_fence
    }

    pub(crate) const fn connection_stamp(&self) -> ConnectionStamp {
        self.connection_stamp
    }

    /// Backward-compatible name used by the host context key. This is the data
    /// lineage only; pairing and reconnect refreshes preserve it.
    pub(crate) const fn lineage_epoch(&self) -> u64 {
        self.data_lineage_epoch
    }

    pub(crate) const fn peer_lineage_epoch(&self) -> u64 {
        self.peer_lineage_epoch
    }

    pub(crate) fn revalidate_current_authority(&self) -> Result<(), AgentLiveBindingError> {
        match self.remote_authority.as_ref() {
            Some(authority) => authority
                .revalidate_current()
                .map_err(|_| AgentLiveBindingError::StaleBinding),
            #[cfg(test)]
            None => Ok(()),
            #[cfg(not(test))]
            None => Err(AgentLiveBindingError::InvalidVerifiedBinding),
        }
    }

    pub(crate) fn remote_authority(&self) -> Option<&VerifiedIncomingPeerAuthorization> {
        self.remote_authority.as_ref()
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct AgentLiveRotationObligation {
    previous: AgentLiveBindingLease,
    proposed: AgentLiveBindingLease,
    transition_epoch: u64,
}

impl AgentLiveRotationObligation {
    pub(crate) fn previous(&self) -> &AgentLiveBindingLease {
        &self.previous
    }

    pub(crate) fn proposed(&self) -> &AgentLiveBindingLease {
        &self.proposed
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum AgentLiveBindOutcome {
    Bound(AgentLiveBindingLease),
    RotationRequired(AgentLiveRotationObligation),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum AgentLiveBindingError {
    InvalidVerifiedBinding,
    Unbound,
    WrongAccount,
    StaleBinding,
    AuthorizationConflict,
    TransitionInProgress,
    TransitionMismatch,
    NonAdjacentGeneration,
    EpochExhausted,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct RevokedAgentLivePeer {
    pub(crate) lease: AgentLiveBindingLease,
}

#[derive(Debug)]
pub(crate) struct AppliedAgentAuthorizationTransition {
    revoked_peers: Vec<RevokedAgentLivePeer>,
    account_epoch_changed: bool,
}

impl AppliedAgentAuthorizationTransition {
    pub(crate) fn revoked_peers(&self) -> &[RevokedAgentLivePeer] {
        &self.revoked_peers
    }

    pub(crate) const fn account_epoch_changed(&self) -> bool {
        self.account_epoch_changed
    }
}

impl fmt::Display for AgentLiveBindingError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        let message = match self {
            Self::InvalidVerifiedBinding => "verified Agent host binding is invalid",
            Self::Unbound => "synchronized Agent history requires a verified host registration",
            Self::WrongAccount => "Agent host registration belongs to another account",
            Self::StaleBinding => "Agent host registration binding is stale",
            Self::AuthorizationConflict => "Agent authorization state is conflicting",
            Self::TransitionInProgress => "Agent host registration is rotating",
            Self::TransitionMismatch => "Agent host registration rotation does not match",
            Self::NonAdjacentGeneration => "Agent account data generation requires a full reset",
            Self::EpochExhausted => "Agent host registration lineage is exhausted",
        };
        formatter.write_str(message)
    }
}

impl std::error::Error for AgentLiveBindingError {}

#[derive(Debug, Clone)]
struct PeerBinding {
    pairing_fence: PairingFence,
    connection_stamp: ConnectionStamp,
    peer_lineage_epoch: u64,
    remote_authority: Option<VerifiedIncomingPeerAuthorization>,
}

#[derive(Debug, Clone)]
struct ActiveBinding {
    account_scope: Arc<str>,
    account_generation: u64,
    execution_target: AgentExecutionTargetId,
    authorization: LocalAuthorizationContext,
    data_lineage_epoch: u64,
    peers: HashMap<iroh::EndpointId, PeerBinding>,
}

impl ActiveBinding {
    fn lease_for(&self, controller_endpoint: iroh::EndpointId) -> Option<AgentLiveBindingLease> {
        let peer = self.peers.get(&controller_endpoint)?;
        Some(AgentLiveBindingLease {
            account_scope: Arc::clone(&self.account_scope),
            account_generation: self.account_generation,
            execution_target: self.execution_target.clone(),
            controller_endpoint,
            authorization: self.authorization.clone(),
            pairing_fence: peer.pairing_fence,
            connection_stamp: peer.connection_stamp,
            data_lineage_epoch: self.data_lineage_epoch,
            peer_lineage_epoch: peer.peer_lineage_epoch,
            remote_authority: peer.remote_authority.clone(),
        })
    }
}

#[derive(Clone)]
enum RegistryBindingState {
    Unbound,
    Active(ActiveBinding),
    Transition {
        previous: ActiveBinding,
        proposed: ActiveBinding,
        previous_lease: AgentLiveBindingLease,
        proposed_lease: AgentLiveBindingLease,
        transition_epoch: u64,
    },
    /// A verified newer account epoch was observed before the durable data
    /// generation advanced. The old owner is immediately unusable.
    Fenced {
        previous: ActiveBinding,
        authorization_floor: LocalAuthorizationContext,
    },
    /// Equal version with a different digest is impossible under one installed
    /// authority. Stay blocked until a strictly newer account epoch arrives.
    Poisoned {
        previous: Option<ActiveBinding>,
        account_epoch: u64,
    },
}

impl Default for RegistryBindingState {
    fn default() -> Self {
        Self::Unbound
    }
}

#[derive(Clone, Default)]
struct BindingRegistryState {
    binding: RegistryBindingState,
    authorization_domain: Option<InstalledAuthorizationDomain>,
    authorization_epoch_floor: u64,
    account_revocation: Option<LocalAuthorizationContext>,
    peer_revocations: HashMap<iroh::EndpointId, LocalAuthorizationContext>,
    next_data_lineage_epoch: u64,
    next_peer_lineage_epoch: u64,
    next_transition_epoch: u64,
}

#[derive(Clone, Default)]
pub(crate) struct AgentLiveBindingRegistry {
    state: Arc<Mutex<BindingRegistryState>>,
}

impl AgentLiveBindingRegistry {
    pub(crate) fn new() -> Self {
        Self::default()
    }

    pub(crate) async fn bind_or_refresh(
        &self,
        verified: VerifiedAgentTargetBinding,
    ) -> Result<AgentLiveBindOutcome, AgentLiveBindingError> {
        let mut state = self.state.lock().await;
        let authority = verified.remote_authority.clone();
        with_current_remote_authority(authority.as_ref(), || {
            validate_authorization_domain(
                &mut state,
                authority
                    .as_ref()
                    .map(|authority| authority.authorization_domain()),
            )?;
            if let Err(error) = validate_authorization_floor(&mut state, &verified.authorization) {
                return Err(error);
            }
            if verified.authorization.account_epoch < state.authorization_epoch_floor {
                return Err(AgentLiveBindingError::StaleBinding);
            }
            validate_revocation_tombstones(
                &mut state,
                &verified.authorization,
                verified.controller_endpoint,
            )?;
            let binding = std::mem::take(&mut state.binding);
            let (next, result) = bind_state(binding, verified, &mut state);
            state.binding = next;
            result
        })?
    }

    pub(crate) async fn require_bound(
        &self,
        account_scope: &str,
        account_generation: u64,
        controller_endpoint: iroh::EndpointId,
    ) -> Result<AgentLiveBindingLease, AgentLiveBindingError> {
        let state = self.state.lock().await;
        let current = required_lease(
            &state.binding,
            account_scope,
            account_generation,
            controller_endpoint,
        )?;
        let authority = current.remote_authority.clone();
        with_current_remote_authority(authority.as_ref(), || current)
    }

    pub(crate) async fn revalidate(
        &self,
        account_scope: &str,
        account_generation: u64,
        lease: &AgentLiveBindingLease,
    ) -> Result<(), AgentLiveBindingError> {
        if account_scope != lease.account_scope.as_ref() {
            return Err(AgentLiveBindingError::WrongAccount);
        }
        let state = self.state.lock().await;
        let authority = lease.remote_authority.clone();
        with_current_remote_authority(authority.as_ref(), || {
            let current = required_lease(
                &state.binding,
                account_scope,
                account_generation,
                lease.controller_endpoint,
            )?;
            if current == *lease {
                Ok(())
            } else {
                Err(AgentLiveBindingError::StaleBinding)
            }
        })?
    }

    /// Consume the unforgeable receipt minted by the endpoint admission swap.
    /// This is the production revocation path, including removal of the final
    /// controller; it never relies on a cloneable snapshot context retained by
    /// a peer which is no longer admitted.
    pub(crate) async fn apply_authorization_transition(
        &self,
        receipt: AuthorizationTransitionReceipt,
    ) -> Result<AppliedAgentAuthorizationTransition, AgentLiveBindingError> {
        let (authorization_domain, previous, current, removed_peers, account_epoch_changed) =
            receipt.into_parts();
        let current = LocalAuthorizationContext::from_installed(&current)?;
        let previous = previous
            .as_ref()
            .map(LocalAuthorizationContext::from_installed)
            .transpose()?;
        let mut state = self.state.lock().await;
        let mut candidate = state.clone();
        validate_authorization_domain(&mut candidate, Some(authorization_domain))?;
        if let Err(error) = validate_authorization_floor(&mut candidate, &current) {
            // Equal-version/different-digest authority is an equivocation, not
            // a stale request. `validate_authorization_floor` has already
            // poisoned the candidate, so publish that terminal fence before
            // returning. Other validation failures leave the live registry
            // byte-for-byte unchanged.
            if error == AgentLiveBindingError::AuthorizationConflict {
                *state = candidate;
            }
            return Err(error);
        }

        if account_epoch_changed {
            if previous
                .as_ref()
                .is_some_and(|previous| previous.account_epoch >= current.account_epoch)
            {
                return Err(AgentLiveBindingError::AuthorizationConflict);
            }
            let revoked_peers = committed_binding_leases(&candidate.binding)
                .into_iter()
                .map(|lease| RevokedAgentLivePeer { lease })
                .collect();
            if let Some(previous) = previous {
                record_account_revocation(&mut candidate, previous)?;
            }
            candidate.authorization_epoch_floor = current.account_epoch;
            candidate.peer_revocations.clear();
            let binding = std::mem::take(&mut candidate.binding);
            candidate.binding = match binding {
                RegistryBindingState::Active(previous)
                | RegistryBindingState::Transition { previous, .. }
                | RegistryBindingState::Fenced { previous, .. } => RegistryBindingState::Fenced {
                    previous,
                    authorization_floor: current,
                },
                RegistryBindingState::Poisoned {
                    previous: Some(previous),
                    ..
                } => RegistryBindingState::Fenced {
                    previous,
                    authorization_floor: current,
                },
                RegistryBindingState::Poisoned { previous: None, .. }
                | RegistryBindingState::Unbound => RegistryBindingState::Unbound,
            };
            *state = candidate;
            return Ok(AppliedAgentAuthorizationTransition {
                revoked_peers,
                account_epoch_changed: true,
            });
        }

        // A retained controller's QUIC connection may remain open across an
        // authorization revision, but every lease minted from the previous
        // context is immediately stale. Return all such leases to the
        // privileged transition cleanup path so idle subscriptions are woken
        // and actor-acknowledged rather than waiting for another event. An
        // exact idempotent snapshot replacement tears down nothing.
        let authorization_changed = previous
            .as_ref()
            .is_some_and(|previous| previous != &current);
        let mut revoked_peers = if authorization_changed {
            committed_binding_leases(&candidate.binding)
                .into_iter()
                .map(|lease| RevokedAgentLivePeer { lease })
                .collect::<Vec<_>>()
        } else {
            Vec::new()
        };
        for controller_endpoint in removed_peers {
            record_peer_revocation(&mut candidate, controller_endpoint, current.clone())?;
            let binding = std::mem::take(&mut candidate.binding);
            let (next, revoked) = revoke_peer_state(
                binding,
                controller_endpoint,
                current.clone(),
                &mut candidate,
            )?;
            candidate.binding = next;
            if let Some(lease) = revoked {
                if !revoked_peers.iter().any(|revoked| revoked.lease == lease) {
                    revoked_peers.push(RevokedAgentLivePeer { lease });
                }
            }
        }
        candidate.authorization_epoch_floor = candidate
            .authorization_epoch_floor
            .max(current.account_epoch);
        *state = candidate;
        Ok(AppliedAgentAuthorizationTransition {
            revoked_peers,
            account_epoch_changed: false,
        })
    }

    /// Remove one exact controller after the endpoint has installed an
    /// authorization snapshot which no longer grants it. Other peer leases and
    /// the account journal remain live.
    #[cfg(test)]
    pub(crate) async fn revoke_peer(
        &self,
        controller_endpoint: iroh::EndpointId,
        installed: &InstalledAuthorizationContext,
    ) -> Result<Option<RevokedAgentLivePeer>, AgentLiveBindingError> {
        let authorization = LocalAuthorizationContext::from_installed(installed)?;
        let mut state = self.state.lock().await;
        // A stale or unknown revocation capability must leave every lineage,
        // tombstone, and active lease unchanged. Work on a candidate snapshot
        // and publish it only after the whole state transition validates.
        let mut candidate = state.clone();
        if let Err(error) = validate_authorization_floor(&mut candidate, &authorization) {
            if error == AgentLiveBindingError::AuthorizationConflict {
                *state = candidate;
            }
            return Err(error);
        }
        if !revocation_matches_known_peer(&candidate.binding, controller_endpoint)
            && !candidate
                .peer_revocations
                .contains_key(&controller_endpoint)
            && !matches!(&candidate.binding, RegistryBindingState::Unbound)
        {
            return Err(AgentLiveBindingError::StaleBinding);
        }
        if let Err(error) =
            record_peer_revocation(&mut candidate, controller_endpoint, authorization.clone())
        {
            if error == AgentLiveBindingError::AuthorizationConflict {
                *state = candidate;
            }
            return Err(error);
        }
        let binding = std::mem::take(&mut candidate.binding);
        let transition =
            revoke_peer_state(binding, controller_endpoint, authorization, &mut candidate);
        let (next, revoked) = match transition {
            Ok(result) => result,
            Err(error) => {
                if error == AgentLiveBindingError::AuthorizationConflict {
                    *state = candidate;
                }
                return Err(error);
            }
        };
        candidate.binding = next;
        *state = candidate;
        Ok(revoked.map(|lease| RevokedAgentLivePeer { lease }))
    }

    /// Fence every synchronized peer for the current account. This is called
    /// from the native authorization lifecycle before logout/account switch;
    /// no renderer value can invoke it as authority.
    #[cfg(test)]
    pub(crate) async fn revoke_account(
        &self,
        installed: &InstalledAuthorizationContext,
    ) -> Result<(), AgentLiveBindingError> {
        let authorization = LocalAuthorizationContext::from_installed(installed)?;
        let mut state = self.state.lock().await;
        validate_authorization_floor(&mut state, &authorization)?;
        if authorization.account_epoch < state.authorization_epoch_floor {
            return Err(AgentLiveBindingError::StaleBinding);
        }
        state.authorization_epoch_floor = authorization.account_epoch;
        record_account_revocation(&mut state, authorization.clone())?;
        let binding = std::mem::take(&mut state.binding);
        state.binding = match binding {
            RegistryBindingState::Active(previous) => RegistryBindingState::Fenced {
                previous,
                authorization_floor: authorization,
            },
            RegistryBindingState::Transition { previous, .. }
            | RegistryBindingState::Fenced { previous, .. } => RegistryBindingState::Fenced {
                previous,
                authorization_floor: authorization,
            },
            RegistryBindingState::Poisoned { previous, .. } => RegistryBindingState::Poisoned {
                previous,
                account_epoch: authorization.account_epoch,
            },
            RegistryBindingState::Unbound => RegistryBindingState::Unbound,
        };
        Ok(())
    }

    pub(crate) async fn commit_rotation(
        &self,
        obligation: AgentLiveRotationObligation,
        reverified: VerifiedAgentTargetBinding,
    ) -> Result<AgentLiveBindingLease, AgentLiveBindingError> {
        let mut state = self.state.lock().await;
        let authority = reverified.remote_authority.clone();
        with_current_remote_authority(authority.as_ref(), || {
            // Never consume the retryable Transition until the fresh native
            // authority and every tombstone check have succeeded. A stale or
            // mismatched commit attempt leaves the obligation byte-for-byte
            // retryable; an authorization equivocation alone is published as
            // the fail-closed poisoned candidate.
            let mut candidate = state.clone();
            if let Err(error) =
                validate_authorization_floor(&mut candidate, &reverified.authorization)
            {
                if error == AgentLiveBindingError::AuthorizationConflict {
                    *state = candidate;
                }
                return Err(error);
            }
            if let Err(error) = validate_revocation_tombstones(
                &mut candidate,
                &reverified.authorization,
                reverified.controller_endpoint,
            ) {
                if error == AgentLiveBindingError::AuthorizationConflict {
                    *state = candidate;
                }
                return Err(error);
            }
            if !verified_matches_lease(&reverified, &obligation.proposed) {
                return Err(AgentLiveBindingError::StaleBinding);
            }
            let binding = std::mem::take(&mut candidate.binding);
            match binding {
                RegistryBindingState::Transition {
                    previous: _,
                    mut proposed,
                    previous_lease,
                    proposed_lease,
                    transition_epoch,
                } if previous_lease == obligation.previous
                    && proposed_lease == obligation.proposed
                    && transition_epoch == obligation.transition_epoch =>
                {
                    // The scalar proposal is only a durable-rotation identity.
                    // Install the freshly guarded native admission capability,
                    // never the clone captured before the asynchronous journal
                    // rotation.
                    replace_proposed_authority(&mut proposed, &reverified)?;
                    let committed = proposed
                        .lease_for(reverified.controller_endpoint)
                        .ok_or(AgentLiveBindingError::TransitionMismatch)?;
                    candidate.binding = RegistryBindingState::Active(proposed);
                    *state = candidate;
                    Ok(committed)
                }
                _ => Err(AgentLiveBindingError::TransitionMismatch),
            }
        })?
    }

    /// Validate an exact retryable obligation while deliberately retaining the
    /// fail-closed Transition state.
    pub(crate) async fn abort_rotation(
        &self,
        obligation: &AgentLiveRotationObligation,
    ) -> Result<(), AgentLiveBindingError> {
        let state = self.state.lock().await;
        match &state.binding {
            RegistryBindingState::Transition {
                previous_lease,
                proposed_lease,
                transition_epoch,
                ..
            } if previous_lease == &obligation.previous
                && proposed_lease == &obligation.proposed
                && *transition_epoch == obligation.transition_epoch =>
            {
                Ok(())
            }
            _ => Err(AgentLiveBindingError::TransitionMismatch),
        }
    }
}

fn with_current_remote_authority<R>(
    authority: Option<&VerifiedIncomingPeerAuthorization>,
    operation: impl FnOnce() -> R,
) -> Result<R, AgentLiveBindingError> {
    match authority {
        Some(authority) => authority
            .with_current(operation)
            .map_err(|_| AgentLiveBindingError::StaleBinding),
        #[cfg(test)]
        None => Ok(operation()),
        #[cfg(not(test))]
        None => Err(AgentLiveBindingError::InvalidVerifiedBinding),
    }
}

fn required_lease(
    binding: &RegistryBindingState,
    account_scope: &str,
    account_generation: u64,
    controller_endpoint: iroh::EndpointId,
) -> Result<AgentLiveBindingLease, AgentLiveBindingError> {
    match binding {
        RegistryBindingState::Active(active)
            if active.account_scope.as_ref() == account_scope
                && active.account_generation == account_generation =>
        {
            active
                .lease_for(controller_endpoint)
                .ok_or(AgentLiveBindingError::Unbound)
        }
        RegistryBindingState::Active(active) if active.account_scope.as_ref() != account_scope => {
            Err(AgentLiveBindingError::WrongAccount)
        }
        RegistryBindingState::Active(_) => Err(AgentLiveBindingError::StaleBinding),
        RegistryBindingState::Transition { .. } | RegistryBindingState::Fenced { .. } => {
            Err(AgentLiveBindingError::TransitionInProgress)
        }
        RegistryBindingState::Poisoned { .. } => Err(AgentLiveBindingError::AuthorizationConflict),
        RegistryBindingState::Unbound => Err(AgentLiveBindingError::Unbound),
    }
}

fn validate_authorization_floor(
    state: &mut BindingRegistryState,
    authorization: &LocalAuthorizationContext,
) -> Result<(), AgentLiveBindingError> {
    if authorization.account_epoch < state.authorization_epoch_floor {
        return Err(AgentLiveBindingError::StaleBinding);
    }
    let mut conflicting = false;
    match &state.binding {
        RegistryBindingState::Active(active) => {
            conflicting = matches!(
                compare_authorization(authorization, &active.authorization),
                Err(AgentLiveBindingError::AuthorizationConflict)
            );
        }
        RegistryBindingState::Transition { proposed, .. } => {
            conflicting = matches!(
                compare_authorization(authorization, &proposed.authorization),
                Err(AgentLiveBindingError::AuthorizationConflict)
            );
        }
        RegistryBindingState::Fenced {
            authorization_floor,
            ..
        } => {
            conflicting = matches!(
                compare_authorization(authorization, authorization_floor),
                Err(AgentLiveBindingError::AuthorizationConflict)
            );
        }
        RegistryBindingState::Poisoned { .. } | RegistryBindingState::Unbound => {}
    }
    if conflicting {
        poison_registry_for_conflicting_authority(state, authorization.account_epoch);
        return Err(AgentLiveBindingError::AuthorizationConflict);
    }
    Ok(())
}

fn validate_authorization_domain(
    state: &mut BindingRegistryState,
    proposed: Option<InstalledAuthorizationDomain>,
) -> Result<(), AgentLiveBindingError> {
    match (state.authorization_domain.as_ref(), proposed) {
        (Some(current), Some(proposed)) if current.same_instance(&proposed) => Ok(()),
        (Some(_), Some(_)) => Err(AgentLiveBindingError::StaleBinding),
        (None, Some(proposed)) => {
            state.authorization_domain = Some(proposed);
            Ok(())
        }
        #[cfg(test)]
        (_, None) => Ok(()),
        #[cfg(not(test))]
        (_, None) => Err(AgentLiveBindingError::InvalidVerifiedBinding),
    }
}

fn bind_state(
    binding: RegistryBindingState,
    verified: VerifiedAgentTargetBinding,
    counters: &mut BindingRegistryState,
) -> (
    RegistryBindingState,
    Result<AgentLiveBindOutcome, AgentLiveBindingError>,
) {
    match binding {
        RegistryBindingState::Unbound => {
            counters.authorization_epoch_floor = verified.authorization.account_epoch;
            let active = match new_active(verified, counters) {
                Ok(active) => active,
                Err(error) => return (RegistryBindingState::Unbound, Err(error)),
            };
            let endpoint = *active.peers.keys().next().expect("one peer is inserted");
            let lease = active
                .lease_for(endpoint)
                .expect("inserted peer has a lease");
            (
                RegistryBindingState::Active(active),
                Ok(AgentLiveBindOutcome::Bound(lease)),
            )
        }
        RegistryBindingState::Active(mut active) => {
            let authorization_order =
                match compare_authorization(&verified.authorization, &active.authorization) {
                    Ok(order) => order,
                    Err(AgentLiveBindingError::AuthorizationConflict) => {
                        let epoch = active
                            .authorization
                            .account_epoch
                            .max(verified.authorization.account_epoch);
                        counters.authorization_epoch_floor =
                            counters.authorization_epoch_floor.max(epoch);
                        return (
                            RegistryBindingState::Poisoned {
                                previous: Some(active),
                                account_epoch: epoch,
                            },
                            Err(AgentLiveBindingError::AuthorizationConflict),
                        );
                    }
                    Err(error) => return (RegistryBindingState::Active(active), Err(error)),
                };

            if authorization_order == AuthorizationOrder::NewerEpoch {
                counters.authorization_epoch_floor = verified.authorization.account_epoch;
                if Some(verified.account_generation) != active.account_generation.checked_add(1) {
                    let error = if verified.account_generation > active.account_generation {
                        AgentLiveBindingError::NonAdjacentGeneration
                    } else {
                        AgentLiveBindingError::TransitionInProgress
                    };
                    let floor = verified.authorization;
                    return (
                        RegistryBindingState::Fenced {
                            previous: active,
                            authorization_floor: floor,
                        },
                        Err(error),
                    );
                }
                return begin_rotation(active, verified, counters);
            }

            if verified.account_scope != active.account_scope.as_ref() {
                return (
                    RegistryBindingState::Active(active),
                    Err(AgentLiveBindingError::WrongAccount),
                );
            }
            if verified.account_generation < active.account_generation {
                return (
                    RegistryBindingState::Active(active),
                    Err(AgentLiveBindingError::StaleBinding),
                );
            }
            let data_changed = verified.account_generation != active.account_generation
                || verified.execution_target != active.execution_target;
            if data_changed {
                if Some(verified.account_generation) != active.account_generation.checked_add(1) {
                    return (
                        RegistryBindingState::Active(active),
                        Err(AgentLiveBindingError::NonAdjacentGeneration),
                    );
                }
                // The installed authorization digest does not include the
                // product registration ID. Require a fresh installed snapshot
                // before accepting a target switch.
                if verified.execution_target != active.execution_target
                    && authorization_order != AuthorizationOrder::NewerRevision
                {
                    return (
                        RegistryBindingState::Active(active),
                        Err(AgentLiveBindingError::StaleBinding),
                    );
                }
                return begin_rotation(active, verified, counters);
            }

            let endpoint = verified.controller_endpoint;
            let peer = active.peers.get(&endpoint).cloned();
            let peer_lineage_epoch = match peer {
                None => match allocate_peer_epoch(counters) {
                    Ok(epoch) => epoch,
                    Err(error) => return (RegistryBindingState::Active(active), Err(error)),
                },
                Some(current) if current.pairing_fence != verified.pairing_fence => {
                    if authorization_order != AuthorizationOrder::NewerRevision
                        || verified.connection_stamp <= current.connection_stamp
                    {
                        return (
                            RegistryBindingState::Active(active),
                            Err(AgentLiveBindingError::StaleBinding),
                        );
                    }
                    match allocate_peer_epoch(counters) {
                        Ok(epoch) => epoch,
                        Err(error) => return (RegistryBindingState::Active(active), Err(error)),
                    }
                }
                Some(current) => {
                    if verified.connection_stamp < current.connection_stamp {
                        return (
                            RegistryBindingState::Active(active),
                            Err(AgentLiveBindingError::StaleBinding),
                        );
                    }
                    current.peer_lineage_epoch
                }
            };
            active.authorization = verified.authorization;
            counters.authorization_epoch_floor = active.authorization.account_epoch;
            active.peers.insert(
                endpoint,
                PeerBinding {
                    pairing_fence: verified.pairing_fence,
                    connection_stamp: verified.connection_stamp,
                    peer_lineage_epoch,
                    remote_authority: verified.remote_authority.clone(),
                },
            );
            let lease = active
                .lease_for(endpoint)
                .expect("refreshed peer has a lease");
            (
                RegistryBindingState::Active(active),
                Ok(AgentLiveBindOutcome::Bound(lease)),
            )
        }
        RegistryBindingState::Transition {
            previous,
            proposed,
            previous_lease,
            proposed_lease,
            transition_epoch,
        } => {
            if verified_matches_lease(&verified, &proposed_lease) {
                let obligation = AgentLiveRotationObligation {
                    previous: previous_lease.clone(),
                    proposed: proposed_lease.clone(),
                    transition_epoch,
                };
                (
                    RegistryBindingState::Transition {
                        previous,
                        proposed,
                        previous_lease,
                        proposed_lease,
                        transition_epoch,
                    },
                    Ok(AgentLiveBindOutcome::RotationRequired(obligation)),
                )
            } else {
                (
                    RegistryBindingState::Transition {
                        previous,
                        proposed,
                        previous_lease,
                        proposed_lease,
                        transition_epoch,
                    },
                    Err(AgentLiveBindingError::TransitionInProgress),
                )
            }
        }
        RegistryBindingState::Fenced {
            previous,
            authorization_floor,
        } => {
            let order = match compare_authorization(&verified.authorization, &authorization_floor) {
                Ok(order) => order,
                Err(error) => {
                    return (
                        RegistryBindingState::Fenced {
                            previous,
                            authorization_floor,
                        },
                        Err(error),
                    )
                }
            };
            if verified.authorization.account_epoch < authorization_floor.account_epoch
                || verified.account_generation <= previous.account_generation
            {
                return (
                    RegistryBindingState::Fenced {
                        previous,
                        authorization_floor,
                    },
                    Err(AgentLiveBindingError::TransitionInProgress),
                );
            }
            if Some(verified.account_generation) != previous.account_generation.checked_add(1) {
                return (
                    RegistryBindingState::Fenced {
                        previous,
                        authorization_floor,
                    },
                    Err(AgentLiveBindingError::NonAdjacentGeneration),
                );
            }
            if order == AuthorizationOrder::Same
                && verified.authorization.snapshot_digest != authorization_floor.snapshot_digest
            {
                return (
                    RegistryBindingState::Poisoned {
                        previous: Some(previous),
                        account_epoch: authorization_floor.account_epoch,
                    },
                    Err(AgentLiveBindingError::AuthorizationConflict),
                );
            }
            begin_rotation(previous, verified, counters)
        }
        RegistryBindingState::Poisoned {
            previous,
            account_epoch,
        } => {
            if verified.authorization.account_epoch <= account_epoch {
                return (
                    RegistryBindingState::Poisoned {
                        previous,
                        account_epoch,
                    },
                    Err(AgentLiveBindingError::AuthorizationConflict),
                );
            }
            counters.authorization_epoch_floor = verified.authorization.account_epoch;
            let Some(previous) = previous else {
                let active = match new_active(verified, counters) {
                    Ok(active) => active,
                    Err(error) => {
                        return (
                            RegistryBindingState::Poisoned {
                                previous: None,
                                account_epoch,
                            },
                            Err(error),
                        )
                    }
                };
                let endpoint = *active.peers.keys().next().expect("new peer");
                let lease = active.lease_for(endpoint).expect("new peer lease");
                return (
                    RegistryBindingState::Active(active),
                    Ok(AgentLiveBindOutcome::Bound(lease)),
                );
            };
            if Some(verified.account_generation) != previous.account_generation.checked_add(1) {
                return (
                    RegistryBindingState::Poisoned {
                        previous: Some(previous),
                        account_epoch,
                    },
                    Err(AgentLiveBindingError::NonAdjacentGeneration),
                );
            }
            begin_rotation(previous, verified, counters)
        }
    }
}

fn new_active(
    verified: VerifiedAgentTargetBinding,
    counters: &mut BindingRegistryState,
) -> Result<ActiveBinding, AgentLiveBindingError> {
    let data_lineage_epoch = allocate_data_epoch(counters)?;
    let peer_lineage_epoch = allocate_peer_epoch(counters)?;
    let mut peers = HashMap::new();
    peers.insert(
        verified.controller_endpoint,
        PeerBinding {
            pairing_fence: verified.pairing_fence,
            connection_stamp: verified.connection_stamp,
            peer_lineage_epoch,
            remote_authority: verified.remote_authority.clone(),
        },
    );
    Ok(ActiveBinding {
        account_scope: Arc::from(verified.account_scope),
        account_generation: verified.account_generation,
        execution_target: verified.execution_target,
        authorization: verified.authorization,
        data_lineage_epoch,
        peers,
    })
}

fn begin_rotation(
    previous: ActiveBinding,
    verified: VerifiedAgentTargetBinding,
    counters: &mut BindingRegistryState,
) -> (
    RegistryBindingState,
    Result<AgentLiveBindOutcome, AgentLiveBindingError>,
) {
    let previous_lease = previous
        .lease_for(verified.controller_endpoint)
        .or_else(|| {
            previous
                .peers
                .keys()
                .next()
                .and_then(|endpoint| previous.lease_for(*endpoint))
        });
    let Some(previous_lease) = previous_lease else {
        return (
            RegistryBindingState::Active(previous),
            Err(AgentLiveBindingError::Unbound),
        );
    };
    let proposed = match new_active(verified, counters) {
        Ok(active) => active,
        Err(error) => return (RegistryBindingState::Active(previous), Err(error)),
    };
    let proposed_endpoint = *proposed.peers.keys().next().expect("new peer");
    let proposed_lease = proposed
        .lease_for(proposed_endpoint)
        .expect("new peer lease");
    let transition_epoch = match allocate_transition_epoch(counters) {
        Ok(epoch) => epoch,
        Err(error) => return (RegistryBindingState::Active(previous), Err(error)),
    };
    let obligation = AgentLiveRotationObligation {
        previous: previous_lease.clone(),
        proposed: proposed_lease.clone(),
        transition_epoch,
    };
    (
        RegistryBindingState::Transition {
            previous,
            proposed,
            previous_lease,
            proposed_lease,
            transition_epoch,
        },
        Ok(AgentLiveBindOutcome::RotationRequired(obligation)),
    )
}

fn revoke_peer_state(
    binding: RegistryBindingState,
    controller_endpoint: iroh::EndpointId,
    authorization: LocalAuthorizationContext,
    counters: &mut BindingRegistryState,
) -> Result<(RegistryBindingState, Option<AgentLiveBindingLease>), AgentLiveBindingError> {
    let mut active = match binding {
        RegistryBindingState::Active(active) => active,
        RegistryBindingState::Transition {
            previous,
            proposed: _,
            previous_lease,
            proposed_lease,
            ..
        } => {
            let revoked = [previous_lease, proposed_lease]
                .into_iter()
                .find(|lease| lease.controller_endpoint == controller_endpoint);
            match compare_authorization(&authorization, &previous.authorization) {
                Ok(_) => {}
                Err(AgentLiveBindingError::AuthorizationConflict) => {
                    counters.authorization_epoch_floor = counters
                        .authorization_epoch_floor
                        .max(authorization.account_epoch);
                    return Ok((
                        RegistryBindingState::Poisoned {
                            previous: Some(previous),
                            account_epoch: authorization.account_epoch,
                        },
                        None,
                    ));
                }
                Err(error) => return Err(error),
            }
            // A proposal is not a committed data owner. Revocation invalidates
            // the obligation and fences only the last committed owner, even if
            // the revoked endpoint appeared solely in the proposed binding.
            return Ok((
                RegistryBindingState::Fenced {
                    previous,
                    authorization_floor: authorization,
                },
                revoked,
            ));
        }
        RegistryBindingState::Fenced {
            previous,
            authorization_floor,
        } => {
            return Ok((
                RegistryBindingState::Fenced {
                    previous,
                    authorization_floor,
                },
                None,
            ));
        }
        other => return Ok((other, None)),
    };
    match compare_authorization(&authorization, &active.authorization) {
        Ok(_) => {}
        Err(AgentLiveBindingError::AuthorizationConflict) => {
            counters.authorization_epoch_floor = counters
                .authorization_epoch_floor
                .max(authorization.account_epoch);
            return Ok((
                RegistryBindingState::Poisoned {
                    previous: Some(active),
                    account_epoch: authorization.account_epoch,
                },
                None,
            ));
        }
        Err(error) => return Err(error),
    }
    let revoked = active.lease_for(controller_endpoint);
    active.authorization = authorization;
    counters.authorization_epoch_floor = active.authorization.account_epoch;
    active.peers.remove(&controller_endpoint);
    Ok((RegistryBindingState::Active(active), revoked))
}

fn validate_revocation_tombstones(
    state: &mut BindingRegistryState,
    proposed: &LocalAuthorizationContext,
    controller_endpoint: iroh::EndpointId,
) -> Result<(), AgentLiveBindingError> {
    if let Some(tombstone) = state.account_revocation.as_ref() {
        if proposed.account_epoch <= tombstone.account_epoch {
            if proposed.account_epoch == tombstone.account_epoch
                && proposed.snapshot_revision == tombstone.snapshot_revision
                && proposed.snapshot_digest != tombstone.snapshot_digest
            {
                poison_registry_for_conflicting_authority(state, proposed.account_epoch);
                return Err(AgentLiveBindingError::AuthorizationConflict);
            }
            // Logout/account reset is an account-epoch boundary. A same-epoch
            // snapshot revision can never resurrect the revoked account.
            return Err(AgentLiveBindingError::StaleBinding);
        }
        state.account_revocation = None;
        state.peer_revocations.clear();
    }

    let peer_tombstone = state.peer_revocations.get(&controller_endpoint).cloned();
    if let Some(tombstone) = peer_tombstone.as_ref() {
        match compare_authorization(proposed, tombstone) {
            Ok(AuthorizationOrder::NewerEpoch | AuthorizationOrder::NewerRevision) => {
                state.peer_revocations.remove(&controller_endpoint);
            }
            Ok(AuthorizationOrder::Same) | Err(AgentLiveBindingError::StaleBinding) => {
                return Err(AgentLiveBindingError::StaleBinding);
            }
            Err(AgentLiveBindingError::AuthorizationConflict) => {
                poison_registry_for_conflicting_authority(state, proposed.account_epoch);
                return Err(AgentLiveBindingError::AuthorizationConflict);
            }
            Err(error) => return Err(error),
        }
    }
    Ok(())
}

fn revocation_matches_known_peer(
    binding: &RegistryBindingState,
    controller_endpoint: iroh::EndpointId,
) -> bool {
    match binding {
        RegistryBindingState::Active(active)
        | RegistryBindingState::Fenced {
            previous: active, ..
        } => active.peers.contains_key(&controller_endpoint),
        RegistryBindingState::Transition {
            previous, proposed, ..
        } => {
            previous.peers.contains_key(&controller_endpoint)
                || proposed.peers.contains_key(&controller_endpoint)
        }
        RegistryBindingState::Poisoned { previous, .. } => previous
            .as_ref()
            .is_some_and(|active| active.peers.contains_key(&controller_endpoint)),
        RegistryBindingState::Unbound => false,
    }
}

fn committed_binding_leases(binding: &RegistryBindingState) -> Vec<AgentLiveBindingLease> {
    let active = match binding {
        RegistryBindingState::Active(active)
        | RegistryBindingState::Transition {
            previous: active, ..
        }
        | RegistryBindingState::Fenced {
            previous: active, ..
        } => Some(active),
        RegistryBindingState::Poisoned { previous, .. } => previous.as_ref(),
        RegistryBindingState::Unbound => None,
    };
    active
        .into_iter()
        .flat_map(|active| {
            active
                .peers
                .keys()
                .filter_map(|endpoint| active.lease_for(*endpoint))
        })
        .collect()
}

fn replace_proposed_authority(
    proposed: &mut ActiveBinding,
    reverified: &VerifiedAgentTargetBinding,
) -> Result<(), AgentLiveBindingError> {
    if proposed.authorization != reverified.authorization {
        return Err(AgentLiveBindingError::TransitionMismatch);
    }
    let peer = proposed
        .peers
        .get_mut(&reverified.controller_endpoint)
        .ok_or(AgentLiveBindingError::TransitionMismatch)?;
    if peer.pairing_fence != reverified.pairing_fence
        || peer.connection_stamp != reverified.connection_stamp
    {
        return Err(AgentLiveBindingError::TransitionMismatch);
    }
    peer.remote_authority = reverified.remote_authority.clone();
    Ok(())
}

fn record_peer_revocation(
    state: &mut BindingRegistryState,
    endpoint: iroh::EndpointId,
    authorization: LocalAuthorizationContext,
) -> Result<(), AgentLiveBindingError> {
    if let Some(current) = state.peer_revocations.get(&endpoint) {
        match compare_authorization(&authorization, current) {
            Err(AgentLiveBindingError::AuthorizationConflict) => {
                poison_registry_for_conflicting_authority(state, authorization.account_epoch);
                return Err(AgentLiveBindingError::AuthorizationConflict);
            }
            Err(error) => return Err(error),
            Ok(AuthorizationOrder::Same) => return Ok(()),
            Ok(AuthorizationOrder::NewerEpoch | AuthorizationOrder::NewerRevision) => {}
        }
    }
    state.authorization_epoch_floor = state
        .authorization_epoch_floor
        .max(authorization.account_epoch);
    state.peer_revocations.insert(endpoint, authorization);
    Ok(())
}

fn record_account_revocation(
    state: &mut BindingRegistryState,
    authorization: LocalAuthorizationContext,
) -> Result<(), AgentLiveBindingError> {
    if let Some(current) = state.account_revocation.as_ref() {
        match compare_authorization(&authorization, current) {
            Err(AgentLiveBindingError::AuthorizationConflict) => {
                poison_registry_for_conflicting_authority(state, authorization.account_epoch);
                return Err(AgentLiveBindingError::AuthorizationConflict);
            }
            Err(error) => return Err(error),
            Ok(AuthorizationOrder::Same) => return Ok(()),
            Ok(AuthorizationOrder::NewerEpoch | AuthorizationOrder::NewerRevision) => {}
        }
    }
    state.authorization_epoch_floor = state
        .authorization_epoch_floor
        .max(authorization.account_epoch);
    state.account_revocation = Some(authorization);
    Ok(())
}

fn poison_registry_for_conflicting_authority(state: &mut BindingRegistryState, epoch: u64) {
    let previous = match std::mem::take(&mut state.binding) {
        RegistryBindingState::Active(active)
        | RegistryBindingState::Transition {
            previous: active, ..
        }
        | RegistryBindingState::Fenced {
            previous: active, ..
        } => Some(active),
        RegistryBindingState::Poisoned { previous, .. } => previous,
        RegistryBindingState::Unbound => None,
    };
    state.authorization_epoch_floor = state.authorization_epoch_floor.max(epoch);
    state.binding = RegistryBindingState::Poisoned {
        previous,
        account_epoch: epoch,
    };
}

fn verified_matches_lease(
    verified: &VerifiedAgentTargetBinding,
    lease: &AgentLiveBindingLease,
) -> bool {
    verified.account_scope == lease.account_scope.as_ref()
        && verified.account_generation == lease.account_generation
        && verified.execution_target == lease.execution_target
        && verified.controller_endpoint == lease.controller_endpoint
        && verified.authorization == lease.authorization
        && verified.pairing_fence == lease.pairing_fence
        && verified.connection_stamp == lease.connection_stamp
        && same_remote_authority_instance(
            verified.remote_authority.as_ref(),
            lease.remote_authority.as_ref(),
        )
}

fn same_remote_authority_instance(
    left: Option<&VerifiedIncomingPeerAuthorization>,
    right: Option<&VerifiedIncomingPeerAuthorization>,
) -> bool {
    match (left, right) {
        (Some(left), Some(right)) => left.same_admission_instance(right),
        #[cfg(test)]
        (None, None) => true,
        _ => false,
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum AuthorizationOrder {
    Same,
    NewerRevision,
    NewerEpoch,
}

fn compare_authorization(
    proposed: &LocalAuthorizationContext,
    current: &LocalAuthorizationContext,
) -> Result<AuthorizationOrder, AgentLiveBindingError> {
    match proposed.account_epoch.cmp(&current.account_epoch) {
        Ordering::Less => return Err(AgentLiveBindingError::StaleBinding),
        Ordering::Greater => return Ok(AuthorizationOrder::NewerEpoch),
        Ordering::Equal => {}
    }
    match proposed.snapshot_revision.cmp(&current.snapshot_revision) {
        Ordering::Less => Err(AgentLiveBindingError::StaleBinding),
        Ordering::Greater => Ok(AuthorizationOrder::NewerRevision),
        Ordering::Equal if proposed.snapshot_digest == current.snapshot_digest => {
            Ok(AuthorizationOrder::Same)
        }
        Ordering::Equal => Err(AgentLiveBindingError::AuthorizationConflict),
    }
}

fn allocate_data_epoch(state: &mut BindingRegistryState) -> Result<u64, AgentLiveBindingError> {
    state.next_data_lineage_epoch = state
        .next_data_lineage_epoch
        .checked_add(1)
        .ok_or(AgentLiveBindingError::EpochExhausted)?;
    Ok(state.next_data_lineage_epoch)
}

fn allocate_peer_epoch(state: &mut BindingRegistryState) -> Result<u64, AgentLiveBindingError> {
    state.next_peer_lineage_epoch = state
        .next_peer_lineage_epoch
        .checked_add(1)
        .ok_or(AgentLiveBindingError::EpochExhausted)?;
    Ok(state.next_peer_lineage_epoch)
}

fn allocate_transition_epoch(
    state: &mut BindingRegistryState,
) -> Result<u64, AgentLiveBindingError> {
    state.next_transition_epoch = state
        .next_transition_epoch
        .checked_add(1)
        .ok_or(AgentLiveBindingError::EpochExhausted)?;
    Ok(state.next_transition_epoch)
}

fn validate_bounded_id(value: &str, max_bytes: usize) -> Result<(), ()> {
    if value.is_empty()
        || value.len() > max_bytes
        || value.chars().any(|character| {
            character.is_control()
                || matches!(character, '\u{202a}'..='\u{202e}' | '\u{2066}'..='\u{2069}')
        })
    {
        Err(())
    } else {
        Ok(())
    }
}

fn looks_like_non_nil_uuid(value: &str) -> bool {
    let bytes = value.as_bytes();
    if bytes.len() != 36 || bytes.get(8) != Some(&b'-') {
        return false;
    }
    for index in [13usize, 18, 23] {
        if bytes.get(index) != Some(&b'-') {
            return false;
        }
    }
    let mut has_nonzero = false;
    for (index, byte) in bytes.iter().copied().enumerate() {
        if matches!(index, 8 | 13 | 18 | 23) {
            continue;
        }
        if !byte.is_ascii_hexdigit() {
            return false;
        }
        has_nonzero |= byte != b'0';
    }
    has_nonzero
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::remote_transport::{InstalledAuthorizationDomain, PairingIncarnation};

    const TARGET_A: &str = "11111111-1111-4111-8111-111111111111";
    const TARGET_B: &str = "22222222-2222-4222-8222-222222222222";

    fn endpoint(seed: u8) -> iroh::EndpointId {
        iroh::SecretKey::from_bytes(&[seed; 32]).public()
    }

    #[allow(clippy::too_many_arguments)]
    fn verified(
        account: &str,
        generation: u64,
        target: &str,
        controller: u8,
        account_epoch: u64,
        snapshot_revision: u64,
        digest_byte: u8,
        pairing_incarnation: u64,
        host_epoch: u64,
        connection_generation: u64,
    ) -> VerifiedAgentTargetBinding {
        VerifiedAgentTargetBinding {
            remote_authority: None,
            account_scope: account.to_string(),
            account_generation: generation,
            execution_target: AgentExecutionTargetId::from_verified_registration(
                target.to_string(),
            )
            .unwrap(),
            controller_endpoint: endpoint(controller),
            authorization: LocalAuthorizationContext::for_test(
                account_epoch,
                snapshot_revision,
                [digest_byte; 32],
            ),
            pairing_fence: PairingFence::new(PairingIncarnation::new(pairing_incarnation).unwrap())
                .unwrap(),
            connection_stamp: ConnectionStamp::new(host_epoch, connection_generation).unwrap(),
        }
    }

    fn initial(controller: u8) -> VerifiedAgentTargetBinding {
        verified("account-a", 7, TARGET_A, controller, 17, 1, 1, 3, 41, 1)
    }

    #[tokio::test]
    async fn synchronized_access_is_unavailable_before_verified_binding() {
        let registry = AgentLiveBindingRegistry::new();
        assert_eq!(
            registry.require_bound("account-a", 7, endpoint(1)).await,
            Err(AgentLiveBindingError::Unbound)
        );
        assert!(AgentExecutionTargetId::from_verified_registration("local".into()).is_err());
    }

    #[tokio::test]
    async fn host_restart_full_stamp_prevents_generation_aba() {
        let registry = AgentLiveBindingRegistry::new();
        let AgentLiveBindOutcome::Bound(old) = registry.bind_or_refresh(initial(1)).await.unwrap()
        else {
            panic!("first binding")
        };
        let AgentLiveBindOutcome::Bound(restarted) = registry
            .bind_or_refresh(verified("account-a", 7, TARGET_A, 1, 17, 1, 1, 3, 42, 1))
            .await
            .unwrap()
        else {
            panic!("restart refresh")
        };
        assert_eq!(
            restarted.connection_stamp(),
            ConnectionStamp::new(42, 1).unwrap()
        );
        assert_eq!(restarted.lineage_epoch(), old.lineage_epoch());
        assert_eq!(
            registry.revalidate("account-a", 7, &old).await,
            Err(AgentLiveBindingError::StaleBinding)
        );
        assert_eq!(
            registry
                .bind_or_refresh(verified("account-a", 7, TARGET_A, 1, 17, 1, 1, 3, 41, 99,))
                .await,
            Err(AgentLiveBindingError::StaleBinding)
        );
    }

    #[tokio::test]
    async fn pairing_replacement_requires_new_auth_and_newer_stamp() {
        let registry = AgentLiveBindingRegistry::new();
        let AgentLiveBindOutcome::Bound(old) = registry.bind_or_refresh(initial(1)).await.unwrap()
        else {
            panic!("first binding")
        };
        assert_eq!(
            registry
                .bind_or_refresh(verified("account-a", 7, TARGET_A, 1, 17, 1, 1, 4, 42, 1,))
                .await,
            Err(AgentLiveBindingError::StaleBinding)
        );
        assert_eq!(
            registry
                .bind_or_refresh(verified("account-a", 7, TARGET_A, 1, 17, 2, 2, 4, 41, 1,))
                .await,
            Err(AgentLiveBindingError::StaleBinding)
        );
        let AgentLiveBindOutcome::Bound(repaired) = registry
            .bind_or_refresh(verified("account-a", 7, TARGET_A, 1, 17, 2, 2, 4, 42, 1))
            .await
            .unwrap()
        else {
            panic!("pair repair")
        };
        assert_eq!(old.lineage_epoch(), repaired.lineage_epoch());
        assert_ne!(old.peer_lineage_epoch(), repaired.peer_lineage_epoch());
    }

    #[tokio::test]
    async fn two_controllers_with_equal_pairing_numbers_remain_independent() {
        let registry = AgentLiveBindingRegistry::new();
        let AgentLiveBindOutcome::Bound(first) =
            registry.bind_or_refresh(initial(1)).await.unwrap()
        else {
            panic!("first")
        };
        let AgentLiveBindOutcome::Bound(second) =
            registry.bind_or_refresh(initial(2)).await.unwrap()
        else {
            panic!("second")
        };
        assert_ne!(first.controller_endpoint(), second.controller_endpoint());
        registry.revalidate("account-a", 7, &first).await.unwrap();
        registry.revalidate("account-a", 7, &second).await.unwrap();
    }

    #[tokio::test]
    async fn same_version_digest_conflict_poisons_existing_access() {
        let registry = AgentLiveBindingRegistry::new();
        let AgentLiveBindOutcome::Bound(old) = registry.bind_or_refresh(initial(1)).await.unwrap()
        else {
            panic!("first")
        };
        assert_eq!(
            registry
                .bind_or_refresh(verified("account-a", 7, TARGET_A, 1, 17, 1, 9, 3, 41, 1,))
                .await,
            Err(AgentLiveBindingError::AuthorizationConflict)
        );
        assert_eq!(
            registry.revalidate("account-a", 7, &old).await,
            Err(AgentLiveBindingError::AuthorizationConflict)
        );
    }

    #[tokio::test]
    async fn transition_receipt_digest_conflict_poisons_existing_access() {
        let registry = AgentLiveBindingRegistry::new();
        let AgentLiveBindOutcome::Bound(old) = registry.bind_or_refresh(initial(1)).await.unwrap()
        else {
            panic!("first")
        };
        let receipt = AuthorizationTransitionReceipt::for_test(
            Some(InstalledAuthorizationContext::for_test(17, 1, [1; 32])),
            InstalledAuthorizationContext::for_test(17, 1, [9; 32]),
            Vec::new(),
            false,
        );

        assert!(matches!(
            registry.apply_authorization_transition(receipt).await,
            Err(AgentLiveBindingError::AuthorizationConflict)
        ));
        assert_eq!(
            registry.revalidate("account-a", 7, &old).await,
            Err(AgentLiveBindingError::AuthorizationConflict)
        );
    }

    #[tokio::test]
    async fn newer_account_epoch_immediately_fences_old_before_data_advances() {
        let registry = AgentLiveBindingRegistry::new();
        let AgentLiveBindOutcome::Bound(old) = registry.bind_or_refresh(initial(1)).await.unwrap()
        else {
            panic!("first")
        };
        assert_eq!(
            registry
                .bind_or_refresh(verified("account-b", 7, TARGET_A, 2, 18, 1, 2, 3, 42, 1,))
                .await,
            Err(AgentLiveBindingError::TransitionInProgress)
        );
        assert_eq!(
            registry.revalidate("account-a", 7, &old).await,
            Err(AgentLiveBindingError::TransitionInProgress)
        );
        assert_eq!(
            registry.bind_or_refresh(initial(1)).await,
            Err(AgentLiveBindingError::StaleBinding)
        );
    }

    #[tokio::test]
    async fn account_epoch_floor_blocks_a_b_a_replay() {
        let registry = AgentLiveBindingRegistry::new();
        registry.bind_or_refresh(initial(1)).await.unwrap();
        registry
            .bind_or_refresh(verified("account-b", 7, TARGET_A, 2, 18, 1, 2, 3, 42, 1))
            .await
            .unwrap_err();
        assert_eq!(
            registry.bind_or_refresh(initial(1)).await,
            Err(AgentLiveBindingError::StaleBinding)
        );
    }

    #[tokio::test]
    async fn target_a_b_a_uses_fresh_data_lineage_and_transition_retry_is_exact() {
        let registry = AgentLiveBindingRegistry::new();
        let AgentLiveBindOutcome::Bound(first_a) =
            registry.bind_or_refresh(initial(1)).await.unwrap()
        else {
            panic!("first A")
        };
        let to_b_request = verified("account-a", 8, TARGET_B, 1, 17, 2, 2, 3, 42, 1);
        let AgentLiveBindOutcome::RotationRequired(to_b) =
            registry.bind_or_refresh(to_b_request).await.unwrap()
        else {
            panic!("A to B")
        };
        let AgentLiveBindOutcome::RotationRequired(retry) = registry
            .bind_or_refresh(verified("account-a", 8, TARGET_B, 1, 17, 2, 2, 3, 42, 1))
            .await
            .unwrap()
        else {
            panic!("exact retry")
        };
        assert_eq!(to_b, retry);
        let b = registry
            .commit_rotation(
                to_b,
                verified("account-a", 8, TARGET_B, 1, 17, 2, 2, 3, 42, 1),
            )
            .await
            .unwrap();
        let AgentLiveBindOutcome::RotationRequired(to_a) = registry
            .bind_or_refresh(verified("account-a", 9, TARGET_A, 1, 17, 3, 3, 3, 43, 1))
            .await
            .unwrap()
        else {
            panic!("B to A")
        };
        let second_a = registry
            .commit_rotation(
                to_a,
                verified("account-a", 9, TARGET_A, 1, 17, 3, 3, 3, 43, 1),
            )
            .await
            .unwrap();
        assert_ne!(first_a.lineage_epoch(), b.lineage_epoch());
        assert_ne!(first_a.lineage_epoch(), second_a.lineage_epoch());
        assert_eq!(second_a.execution_target().as_str(), TARGET_A);
    }

    #[tokio::test]
    async fn peer_revocation_does_not_remove_other_controller() {
        let registry = AgentLiveBindingRegistry::new();
        let AgentLiveBindOutcome::Bound(first) =
            registry.bind_or_refresh(initial(1)).await.unwrap()
        else {
            panic!("first")
        };
        let AgentLiveBindOutcome::Bound(second) =
            registry.bind_or_refresh(initial(2)).await.unwrap()
        else {
            panic!("second")
        };
        let revoked = registry
            .revoke_peer(
                first.controller_endpoint(),
                &InstalledAuthorizationContext::for_test(17, 2, [2; 32]),
            )
            .await
            .unwrap()
            .unwrap();
        assert_eq!(
            revoked.lease.controller_endpoint(),
            first.controller_endpoint()
        );
        assert_eq!(
            registry.revalidate("account-a", 7, &first).await,
            Err(AgentLiveBindingError::Unbound)
        );
        // The authorization revision advanced, so recover the other peer's
        // refreshed lease through a fresh verified capability.
        let AgentLiveBindOutcome::Bound(second_refreshed) = registry
            .bind_or_refresh(verified("account-a", 7, TARGET_A, 2, 17, 3, 3, 3, 41, 2))
            .await
            .unwrap()
        else {
            panic!("refresh retained peer")
        };
        assert_eq!(
            second_refreshed.controller_endpoint(),
            second.controller_endpoint()
        );
    }

    #[tokio::test]
    async fn authoritative_transition_receipt_revokes_last_peer_without_live_capability() {
        let registry = AgentLiveBindingRegistry::new();
        let AgentLiveBindOutcome::Bound(old) = registry.bind_or_refresh(initial(1)).await.unwrap()
        else {
            panic!("first binding")
        };
        let transition = AuthorizationTransitionReceipt::for_test(
            Some(InstalledAuthorizationContext::for_test(17, 1, [1; 32])),
            InstalledAuthorizationContext::for_test(17, 2, [2; 32]),
            vec![old.controller_endpoint()],
            false,
        );
        let applied = registry
            .apply_authorization_transition(transition)
            .await
            .unwrap();
        assert!(!applied.account_epoch_changed());
        assert_eq!(applied.revoked_peers().len(), 1);
        assert_eq!(
            registry.revalidate("account-a", 7, &old).await,
            Err(AgentLiveBindingError::Unbound)
        );
        assert_eq!(
            registry.bind_or_refresh(initial(1)).await,
            Err(AgentLiveBindingError::StaleBinding)
        );
    }

    #[tokio::test]
    async fn authorization_refresh_wakes_all_old_context_leases_but_exact_retry_does_not() {
        let registry = AgentLiveBindingRegistry::new();
        let AgentLiveBindOutcome::Bound(first) =
            registry.bind_or_refresh(initial(1)).await.unwrap()
        else {
            panic!("first binding")
        };
        let AgentLiveBindOutcome::Bound(second) =
            registry.bind_or_refresh(initial(2)).await.unwrap()
        else {
            panic!("second binding")
        };
        let domain = InstalledAuthorizationDomain::for_test();
        let initial_context = InstalledAuthorizationContext::for_test(17, 1, [1; 32]);

        let exact_retry = AuthorizationTransitionReceipt::for_test_in_domain(
            domain.clone(),
            Some(initial_context.clone()),
            initial_context.clone(),
            Vec::new(),
            false,
        );
        let applied = registry
            .apply_authorization_transition(exact_retry)
            .await
            .unwrap();
        assert!(applied.revoked_peers().is_empty());

        let refreshed = AuthorizationTransitionReceipt::for_test_in_domain(
            domain,
            Some(initial_context),
            InstalledAuthorizationContext::for_test(17, 2, [2; 32]),
            Vec::new(),
            false,
        );
        let applied = registry
            .apply_authorization_transition(refreshed)
            .await
            .unwrap();
        assert_eq!(applied.revoked_peers().len(), 2);
        assert!(applied
            .revoked_peers()
            .iter()
            .any(|revoked| revoked.lease == first));
        assert!(applied
            .revoked_peers()
            .iter()
            .any(|revoked| revoked.lease == second));
    }

    #[tokio::test]
    async fn authoritative_account_transition_fences_committed_owner_and_discards_proposal() {
        let registry = AgentLiveBindingRegistry::new();
        let AgentLiveBindOutcome::Bound(old) = registry.bind_or_refresh(initial(1)).await.unwrap()
        else {
            panic!("first binding")
        };
        let AgentLiveBindOutcome::RotationRequired(_proposal) = registry
            .bind_or_refresh(verified("account-a", 8, TARGET_B, 1, 17, 2, 2, 3, 42, 1))
            .await
            .unwrap()
        else {
            panic!("rotation proposal")
        };
        let transition = AuthorizationTransitionReceipt::for_test(
            Some(InstalledAuthorizationContext::for_test(17, 2, [2; 32])),
            InstalledAuthorizationContext::for_test(18, 1, [3; 32]),
            vec![old.controller_endpoint()],
            true,
        );
        let applied = registry
            .apply_authorization_transition(transition)
            .await
            .unwrap();
        assert!(applied.account_epoch_changed());
        assert_eq!(applied.revoked_peers().len(), 1);
        let state = registry.state.lock().await;
        let RegistryBindingState::Fenced { previous, .. } = &state.binding else {
            panic!("account transition must fence the committed owner")
        };
        assert_eq!(previous.account_generation, 7);
        assert_eq!(previous.execution_target.as_str(), TARGET_A);
    }

    #[tokio::test]
    async fn stale_or_unknown_peer_revoke_preserves_active_binding_exactly() {
        let registry = AgentLiveBindingRegistry::new();
        let AgentLiveBindOutcome::Bound(first) = registry
            .bind_or_refresh(verified("account-a", 7, TARGET_A, 1, 17, 2, 2, 3, 41, 1))
            .await
            .unwrap()
        else {
            panic!("first binding")
        };

        assert_eq!(
            registry
                .revoke_peer(
                    first.controller_endpoint(),
                    &InstalledAuthorizationContext::for_test(17, 1, [1; 32]),
                )
                .await,
            Err(AgentLiveBindingError::StaleBinding)
        );
        registry.revalidate("account-a", 7, &first).await.unwrap();

        assert_eq!(
            registry
                .revoke_peer(
                    endpoint(9),
                    &InstalledAuthorizationContext::for_test(17, 3, [3; 32]),
                )
                .await,
            Err(AgentLiveBindingError::StaleBinding)
        );
        registry.revalidate("account-a", 7, &first).await.unwrap();
    }

    #[tokio::test]
    async fn revoke_before_first_bind_tombstones_old_capability_until_newer_authority() {
        let registry = AgentLiveBindingRegistry::new();
        registry
            .revoke_peer(
                endpoint(1),
                &InstalledAuthorizationContext::for_test(17, 2, [2; 32]),
            )
            .await
            .unwrap();
        assert_eq!(
            registry
                .bind_or_refresh(verified("account-a", 7, TARGET_A, 1, 17, 2, 2, 3, 41, 1,))
                .await,
            Err(AgentLiveBindingError::StaleBinding)
        );
        let AgentLiveBindOutcome::Bound(fresh) = registry
            .bind_or_refresh(verified("account-a", 7, TARGET_A, 1, 17, 3, 3, 3, 42, 1))
            .await
            .unwrap()
        else {
            panic!("new installed authorization must clear the tombstone")
        };
        assert_eq!(fresh.controller_endpoint(), endpoint(1));
    }

    #[tokio::test]
    async fn revoke_between_rotation_proposal_and_commit_invalidates_obligation() {
        let registry = AgentLiveBindingRegistry::new();
        registry.bind_or_refresh(initial(1)).await.unwrap();
        let AgentLiveBindOutcome::RotationRequired(obligation) = registry
            .bind_or_refresh(verified("account-a", 8, TARGET_B, 1, 17, 2, 2, 4, 42, 1))
            .await
            .unwrap()
        else {
            panic!("rotation proposal")
        };
        registry
            .revoke_peer(
                endpoint(1),
                &InstalledAuthorizationContext::for_test(17, 3, [3; 32]),
            )
            .await
            .unwrap();
        assert_eq!(
            registry
                .commit_rotation(
                    obligation,
                    verified("account-a", 8, TARGET_B, 1, 17, 3, 3, 4, 43, 1),
                )
                .await,
            Err(AgentLiveBindingError::StaleBinding)
        );
        assert_eq!(
            registry.require_bound("account-a", 8, endpoint(1)).await,
            Err(AgentLiveBindingError::TransitionInProgress)
        );
        let state = registry.state.lock().await;
        let RegistryBindingState::Fenced { previous, .. } = &state.binding else {
            panic!("revocation must discard the uncommitted proposal")
        };
        assert_eq!(previous.account_generation, 7);
        assert_eq!(previous.execution_target.as_str(), TARGET_A);
    }

    #[tokio::test]
    async fn account_revoke_equality_never_rebinds() {
        let registry = AgentLiveBindingRegistry::new();
        registry
            .revoke_account(&InstalledAuthorizationContext::for_test(17, 1, [1; 32]))
            .await
            .unwrap();
        assert_eq!(
            registry.bind_or_refresh(initial(1)).await,
            Err(AgentLiveBindingError::StaleBinding)
        );
        assert_eq!(
            registry
                .bind_or_refresh(verified("account-a", 7, TARGET_A, 1, 17, 2, 2, 3, 42, 1,))
                .await,
            Err(AgentLiveBindingError::StaleBinding)
        );
    }

    #[tokio::test]
    async fn authorization_receipt_from_another_admission_cannot_revoke_binding() {
        let registry = AgentLiveBindingRegistry::new();
        let domain = InstalledAuthorizationDomain::for_test();
        {
            let mut state = registry.state.lock().await;
            state.authorization_domain = Some(domain.clone());
        }
        let AgentLiveBindOutcome::Bound(old) = registry.bind_or_refresh(initial(1)).await.unwrap()
        else {
            panic!("first binding")
        };
        let receipt = AuthorizationTransitionReceipt::for_test_in_domain(
            InstalledAuthorizationDomain::for_test(),
            Some(InstalledAuthorizationContext::for_test(17, 1, [1; 32])),
            InstalledAuthorizationContext::for_test(17, 2, [2; 32]),
            vec![old.controller_endpoint()],
            false,
        );
        assert!(matches!(
            registry.apply_authorization_transition(receipt).await,
            Err(AgentLiveBindingError::StaleBinding)
        ));
        registry.revalidate("account-a", 7, &old).await.unwrap();
    }

    #[test]
    fn identical_scalars_from_another_admission_are_not_the_same_capability() {
        let mut proposed = initial(1);
        let authority = VerifiedIncomingPeerAuthorization::for_admission_identity_test(
            InstalledAuthorizationContext::for_test(17, 1, [1; 32]),
            endpoint(1),
            Arc::from(TARGET_A),
            proposed.pairing_fence,
            proposed.connection_stamp,
        );
        proposed.remote_authority = Some(authority.clone());
        let lease = AgentLiveBindingLease {
            account_scope: Arc::from(proposed.account_scope.as_str()),
            account_generation: proposed.account_generation,
            execution_target: proposed.execution_target.clone(),
            controller_endpoint: proposed.controller_endpoint,
            authorization: proposed.authorization.clone(),
            pairing_fence: proposed.pairing_fence,
            connection_stamp: proposed.connection_stamp,
            data_lineage_epoch: 1,
            peer_lineage_epoch: 1,
            remote_authority: Some(authority),
        };
        assert!(verified_matches_lease(&proposed, &lease));

        proposed.remote_authority = Some(
            VerifiedIncomingPeerAuthorization::for_admission_identity_test(
                InstalledAuthorizationContext::for_test(17, 1, [1; 32]),
                endpoint(1),
                Arc::from(TARGET_A),
                proposed.pairing_fence,
                proposed.connection_stamp,
            ),
        );
        assert!(!verified_matches_lease(&proposed, &lease));
    }
}
