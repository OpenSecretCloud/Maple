//! Durable, account-bound send admission in the host's one profile store.
//! A reservation commits before dispatch; uncertain effects are never replayed.

use std::sync::Arc;

pub use maple_agent::store::records::SendOutcome as Outcome;
use maple_agent::store::{AccountId, Store};

pub struct Receipts(Arc<Store>);

impl Receipts {
    pub fn new(store: Arc<Store>) -> Self {
        Self(store)
    }

    fn account(&self, user: &str) -> Result<AccountId, String> {
        self.0
            .account_id(user)
            .map_err(|error| error.to_string())?
            .ok_or_else(|| "The send account is unavailable".into())
    }

    pub fn reserve(&self, id: &str, params: &serde_json::Value) -> Result<Option<Outcome>, String> {
        let user = params
            .pointer("/0")
            .and_then(serde_json::Value::as_str)
            .ok_or("Invalid send account")?;
        let task = params
            .pointer("/1/sessionId")
            .and_then(serde_json::Value::as_str)
            .ok_or("Invalid send task")?;
        self.0
            .reserve_send(self.account(user)?, id, task, params, super::store_now_ms())
            .map_err(|error| error.to_string())
    }

    pub fn finish(
        &self,
        user: &str,
        id: &str,
        result: &Result<serde_json::Value, String>,
    ) -> Result<(), String> {
        self.0
            .finish_send(self.account(user)?, id, result)
            .map_err(|error| error.to_string())
    }

    /// Looking up another account's identity has exactly the missing outcome.
    pub fn authorize(&self, id: &str, user: &str) -> Result<(), String> {
        self.outcome(user, id).map(|_| ())
    }

    pub fn outcome(&self, user: &str, id: &str) -> Result<Outcome, String> {
        self.0
            .send_outcome(self.account(user)?, id)
            .map_err(|error| error.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use maple_agent::store::{NewTask, StorePaths, TaskKind};

    fn fixture() -> (tempfile::TempDir, Arc<Store>, String, serde_json::Value) {
        let root = tempfile::tempdir().unwrap();
        let store = Arc::new(Store::open(StorePaths::new(root.path())).unwrap());
        let account = store.upsert_account("alice", 1).unwrap();
        store.upsert_account("bob", 1).unwrap();
        let task = uuid::Uuid::now_v7().to_string();
        store
            .create_task(
                account,
                &NewTask {
                    uuid: task.clone(),
                    project_root: root.path().to_string_lossy().into(),
                    kind: TaskKind::Desktop,
                    title: "fixture".into(),
                    provisional: false,
                    model: None,
                    created_ms: 1,
                },
            )
            .unwrap();
        let params = serde_json::json!(["alice", {"sessionId":task,"text":"private fixture"}]);
        (root, store, uuid::Uuid::new_v4().to_string(), params)
    }

    #[test]
    fn lost_reply_is_durable_and_account_bound() {
        let (_root, store, id, params) = fixture();
        let receipts = Receipts::new(store.clone());
        assert_eq!(receipts.reserve(&id, &params).unwrap(), None);
        assert_eq!(receipts.outcome("bob", &id).unwrap(), Outcome::Missing);
        let result = Ok(serde_json::json!("run-1"));
        receipts.finish("alice", &id, &result).unwrap();
        assert_eq!(
            receipts.reserve(&id, &params).unwrap(),
            Some(Outcome::Finished(result.clone()))
        );
        drop(receipts);
        store.close().unwrap();
        store.initialize_plain().unwrap();
        assert_eq!(
            Receipts::new(store).outcome("alice", &id).unwrap(),
            Outcome::Finished(result)
        );
    }

    #[test]
    fn restart_reserves_uncertain_identity_without_dispatching_again() {
        let (_root, store, id, params) = fixture();
        let receipts = Receipts::new(store.clone());
        assert_eq!(receipts.reserve(&id, &params).unwrap(), None);
        drop(receipts);
        store.close().unwrap();
        store.initialize_plain().unwrap();
        let receipts = Receipts::new(store);
        assert_eq!(
            receipts.reserve(&id, &params).unwrap(),
            Some(Outcome::Interrupted)
        );
        assert!(
            receipts
                .finish("alice", &id, &Ok(serde_json::Value::Null))
                .is_err()
        );
    }

    #[test]
    fn reuse_with_different_content_and_failed_admission_do_not_authorize_dispatch() {
        let (_root, store, id, params) = fixture();
        let receipts = Receipts::new(store.clone());
        receipts.reserve(&id, &params).unwrap();
        let mut changed = params.clone();
        changed[1]["text"] = serde_json::json!("different");
        assert!(receipts.reserve(&id, &changed).is_err());
        store.close().unwrap();
        assert!(
            receipts
                .reserve(&uuid::Uuid::new_v4().to_string(), &params)
                .is_err()
        );
    }
}
