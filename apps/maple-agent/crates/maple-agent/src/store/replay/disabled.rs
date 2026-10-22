//! No ACP replay resources exist in a proxy-only runtime.

use super::{AccountId, Store, StoreResult};

#[derive(Default)]
pub(super) struct ReplayRegistry {
    _private: (),
}

impl ReplayRegistry {
    pub(super) fn seal(&self) -> StoreResult<()> {
        Ok(())
    }
}

impl Store {
    pub(super) fn resume_replays(&self) {}
    pub(crate) fn revoke_all_replays(&self) {
        let _ = self.cancel_all_replays();
    }
    pub(crate) fn cancel_all_replays(&self) -> StoreResult<()> {
        Ok(())
    }
    pub(crate) fn cancel_account_replays(&self, _account: AccountId) -> StoreResult<()> {
        Ok(())
    }
    pub(crate) fn cancel_task_replays(&self, _account: AccountId, _task: &str) -> StoreResult<()> {
        Ok(())
    }
}
