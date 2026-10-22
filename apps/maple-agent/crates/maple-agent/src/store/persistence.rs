//! A content-free, memory-only save-failure latch. Successful unrelated writes
//! cannot dismiss it while a loaded Pi session still owns unsaved source.

use std::sync::Mutex;

use serde::{Deserialize, Serialize};
use tokio::sync::watch;

use super::{Store, StoreError};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PersistenceFailureReason {
    DiskFull,
    SaveFailed,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct PersistenceFailure {
    pub store_id: Option<String>,
    pub revision: u64,
    pub reason: PersistenceFailureReason,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct PersistenceStatus {
    pub store_id: Option<String>,
    pub revision: u64,
    pub failure: Option<PersistenceFailure>,
}

impl PersistenceFailure {
    pub fn message(&self) -> &'static str {
        match self.reason {
            PersistenceFailureReason::DiskFull => {
                "The disk is full. Recent changes could not be saved. Keep Maple open, free disk space, then retry saving."
            }
            PersistenceFailureReason::SaveFailed => {
                "Recent changes could not be saved. Keep Maple open and retry saving before quitting."
            }
        }
    }
}

#[derive(Default)]
struct State {
    generation: Option<String>,
    revision: u64,
}

pub(super) struct PersistenceState {
    state: Mutex<State>,
    notice: watch::Sender<PersistenceStatus>,
}

impl Default for PersistenceState {
    fn default() -> Self {
        Self {
            state: Mutex::new(State::default()),
            notice: watch::channel(PersistenceStatus::default()).0,
        }
    }
}

impl PersistenceState {
    pub(super) fn set_generation(&self, generation: [u8; 16]) {
        let generation = uuid::Uuid::from_bytes(generation).simple().to_string();
        let mut state = self
            .state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if state.generation.as_deref() != Some(&generation) {
            state.generation = Some(generation);
            state.revision = state.revision.wrapping_add(1);
            self.notice.send_replace(PersistenceStatus {
                store_id: state.generation.clone(),
                revision: state.revision,
                failure: None,
            });
        }
    }

    fn record(&self, error: &StoreError) {
        let mut state = self
            .state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        state.revision = state.revision.wrapping_add(1);
        let reason = if matches!(error, StoreError::DiskFull)
            || self
                .notice
                .borrow()
                .failure
                .as_ref()
                .is_some_and(|notice| notice.reason == PersistenceFailureReason::DiskFull)
        {
            PersistenceFailureReason::DiskFull
        } else {
            PersistenceFailureReason::SaveFailed
        };
        let failure = PersistenceFailure {
            store_id: state.generation.clone(),
            revision: state.revision,
            reason,
        };
        self.notice.send_replace(PersistenceStatus {
            store_id: state.generation.clone(),
            revision: state.revision,
            failure: Some(failure),
        });
    }
}

impl Store {
    /// Requires no SQL or filesystem access, including while a store is locked
    /// or temporarily unavailable. The generation changes only after a checked
    /// database is installed on this same host-owned handle.
    pub fn persistence_failure(&self) -> Option<PersistenceFailure> {
        self.persistence.notice.borrow().failure.clone()
    }

    pub fn persistence_status(&self) -> PersistenceStatus {
        self.persistence.notice.borrow().clone()
    }

    pub(crate) fn subscribe_persistence_failures(&self) -> watch::Receiver<PersistenceStatus> {
        self.persistence.notice.subscribe()
    }

    pub(crate) fn record_persistence_failure(&self, error: &StoreError) {
        self.persistence.record(error);
    }

    /// A successful global retry can clear only the exact failure it observed.
    /// A newer concurrent failure must remain visible. Ordinary successful
    /// writes, including derived maintenance, never call this method.
    pub fn clear_persistence_failure(&self, revision: u64) -> bool {
        let mut state = self
            .persistence
            .state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if state.revision != revision {
            return false;
        }
        state.revision = state.revision.wrapping_add(1);
        self.persistence.notice.send_replace(PersistenceStatus {
            store_id: state.generation.clone(),
            revision: state.revision,
            failure: None,
        });
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn latch_is_generation_bound_and_only_exact_retry_clears_it() {
        let state = PersistenceState::default();
        state.set_generation([1; 16]);
        state.record(&StoreError::DiskFull);
        let first = state.notice.borrow().failure.clone().unwrap();
        state.record(&StoreError::InvalidData);
        let second = state.notice.borrow().failure.clone().unwrap();
        assert_ne!(first.revision, second.revision);
        assert_eq!(second.reason, PersistenceFailureReason::DiskFull);
        assert_eq!(first.store_id, second.store_id);
        state.set_generation([1; 16]);
        assert_eq!(state.notice.borrow().failure.as_ref(), Some(&second));
        state.set_generation([2; 16]);
        assert!(state.notice.borrow().failure.is_none());
    }
}
