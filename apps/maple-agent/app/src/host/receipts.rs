//! A send gets an identity before it crosses IPC. Persist admission before
//! invoking the runtime, then its result. A lost reply can be queried; a host
//! crash between admission and completion is explicitly uncertain, never replayed.
//! This contains no prompts, images, credentials, or token events.

use rusqlite::{Connection, OptionalExtension, params};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{path::Path, sync::Mutex};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum Outcome {
    Missing,
    Pending,
    Finished(Result<serde_json::Value, String>),
}

pub struct Receipts(Mutex<Connection>);

impl Receipts {
    pub fn open(path: &Path) -> Result<Self, String> {
        let connection = Connection::open(path).map_err(|e| e.to_string())?;
        maple_agent::private_file::set_owner_only_file(path).map_err(|e| e.to_string())?;
        connection
            .execute_batch(
                "PRAGMA journal_mode=WAL;
            CREATE TABLE IF NOT EXISTS sends (
                id TEXT PRIMARY KEY, fingerprint TEXT NOT NULL,
                task TEXT NOT NULL, result TEXT
            );",
            )
            .map_err(|e| e.to_string())?;
        Ok(Self(Mutex::new(connection)))
    }

    /// Returns None only to the first caller. Reusing an ID with different
    /// content is a caller error, not another prompt.
    pub fn reserve(&self, id: &str, params: &serde_json::Value) -> Result<Option<Outcome>, String> {
        uuid::Uuid::parse_str(id).map_err(|_| "Invalid send identity".to_string())?;
        let bytes = serde_json::to_vec(params).map_err(|e| e.to_string())?;
        let fingerprint = format!("{:x}", Sha256::digest(bytes));
        let task = params
            .pointer("/1/sessionId")
            .and_then(serde_json::Value::as_str)
            .ok_or("Invalid send parameters")?;
        let connection = self.0.lock().unwrap();
        let existing: Option<(String, Option<String>)> = connection
            .query_row(
                "SELECT fingerprint, result FROM sends WHERE id=?1",
                [id],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .optional()
            .map_err(|e| e.to_string())?;
        if let Some((saved, result)) = existing {
            if saved != fingerprint {
                return Err("Send identity was reused for different content".into());
            }
            return result
                .map(|result| {
                    serde_json::from_str(&result)
                        .map(Outcome::Finished)
                        .map(Some)
                        .map_err(|e| e.to_string())
                })
                .unwrap_or(Ok(Some(Outcome::Pending)));
        }
        connection
            .execute(
                "INSERT INTO sends(id,fingerprint,task) VALUES (?1,?2,?3)",
                params![id, fingerprint, task],
            )
            .map_err(|e| e.to_string())?;
        Ok(None)
    }

    pub fn finish(
        &self,
        id: &str,
        result: &Result<serde_json::Value, String>,
    ) -> Result<(), String> {
        let result = serde_json::to_string(result).map_err(|e| e.to_string())?;
        self.0
            .lock()
            .unwrap()
            .execute(
                "UPDATE sends SET result=?2 WHERE id=?1",
                params![id, result],
            )
            .map_err(|e| e.to_string())?;
        Ok(())
    }

    pub fn outcome(&self, id: &str) -> Result<Outcome, String> {
        let result: Option<Option<String>> = self
            .0
            .lock()
            .unwrap()
            .query_row("SELECT result FROM sends WHERE id=?1", [id], |row| {
                row.get(0)
            })
            .optional()
            .map_err(|e| e.to_string())?;
        match result {
            None => Ok(Outcome::Missing),
            Some(None) => Ok(Outcome::Pending),
            Some(Some(result)) => serde_json::from_str(&result)
                .map(Outcome::Finished)
                .map_err(|e| e.to_string()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lost_reply_and_incomplete_admission_survive_reopening_without_resending() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("receipts.db");
        let id = uuid::Uuid::new_v4().to_string();
        let params = serde_json::json!(["user", {"sessionId":"task", "text":"hello"}]);
        let receipts = Receipts::open(&path).unwrap();
        assert_eq!(receipts.reserve(&id, &params).unwrap(), None);
        drop(receipts);
        let receipts = Receipts::open(&path).unwrap();
        assert_eq!(
            receipts.reserve(&id, &params).unwrap(),
            Some(Outcome::Pending)
        );
        let result = Ok(serde_json::json!("run-1"));
        receipts.finish(&id, &result).unwrap();
        drop(receipts);
        let receipts = Receipts::open(&path).unwrap();
        assert_eq!(
            receipts.outcome(&id).unwrap(),
            Outcome::Finished(result.clone())
        );
        assert_eq!(
            receipts.reserve(&id, &params).unwrap(),
            Some(Outcome::Finished(result))
        );
        assert!(
            receipts
                .reserve(
                    &id,
                    &serde_json::json!(["user", {"sessionId":"task", "text":"different"}])
                )
                .is_err()
        );
    }
}
