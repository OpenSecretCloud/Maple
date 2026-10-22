//! Durable scheduling for bounded idle checks. An interrupted or incomplete
//! check must not become either a clean receipt or a minute-by-minute scan.

use std::time::Duration;

use rusqlite::{OptionalExtension, params};

use super::{Durability, Store, StoreError, StoreResult};

const INITIAL_DELAY_MS: i64 = 5 * 60 * 1000;
const MAX_DELAY_MS: i64 = 24 * 60 * 60 * 1000;
const MAX_ATTEMPTS: i64 = 16;

impl Store {
    pub(crate) fn idle_integrity_retry_ready(&self, now_ms: i64) -> StoreResult<bool> {
        if now_ms < 0 {
            return Err(StoreError::InvalidInput);
        }
        self.read(|tx| {
            let next: Option<i64> = tx
                .query_row(
                    "SELECT value FROM store_meta WHERE key='integrity_retry_after_ms'",
                    [],
                    |row| row.get(0),
                )
                .optional()?;
            Ok(next.is_none_or(|next| next <= now_ms))
        })
    }

    /// Persist admission before starting the child. A host crash or cancellation
    /// therefore retains the retry delay without claiming successful validation.
    pub(crate) fn begin_idle_integrity_attempt(
        &self,
        now_ms: i64,
    ) -> StoreResult<Option<Duration>> {
        if now_ms < 0 {
            return Err(StoreError::InvalidInput);
        }
        self.write(Durability::Durable, |tx| {
            let next: Option<i64> = tx
                .query_row(
                    "SELECT value FROM store_meta WHERE key='integrity_retry_after_ms'",
                    [],
                    |row| row.get(0),
                )
                .optional()?;
            if next.is_some_and(|next| next > now_ms) {
                return Ok(None);
            }
            let previous: Option<i64> = tx
                .query_row(
                    "SELECT value FROM store_meta WHERE key='integrity_incomplete_attempts'",
                    [],
                    |row| row.get(0),
                )
                .optional()?;
            let attempt = previous.unwrap_or(0).clamp(0, MAX_ATTEMPTS - 1) + 1;
            let delay = INITIAL_DELAY_MS
                .saturating_mul(4_i64.pow((attempt - 1).min(8) as u32))
                .min(MAX_DELAY_MS);
            for (key, value) in [
                ("integrity_incomplete_attempts", attempt),
                ("integrity_retry_after_ms", now_ms.saturating_add(delay)),
            ] {
                tx.execute(
                    "INSERT INTO store_meta(key,value) VALUES (?1,?2) ON CONFLICT(key) DO UPDATE SET value=excluded.value",
                    params![key, value],
                )?;
            }
            // Larger stores receive more time after an incomplete check, but
            // every child remains cancellable and has a finite five-minute cap.
            Ok(Some(Duration::from_secs(
                30_u64.saturating_mul(1_u64 << (attempt - 1).min(4)).min(300),
            )))
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::store::StorePaths;

    #[test]
    fn interrupted_attempt_survives_cold_reopen_without_recording_a_clean_check() {
        let directory = tempfile::tempdir().unwrap();
        let paths = StorePaths::new(directory.path());
        let store = Store::open(paths.clone()).unwrap();
        let now = 1_000_000;
        assert_eq!(
            store.begin_idle_integrity_attempt(now).unwrap(),
            Some(Duration::from_secs(30))
        );
        store.close().unwrap();
        drop(store);
        let reopened = Store::open(paths).unwrap();
        assert!(reopened.integrity_schedule(now, false).unwrap().full);
        assert!(!reopened.idle_integrity_retry_ready(now + 60_000).unwrap());
        assert_eq!(
            reopened.begin_idle_integrity_attempt(now + 60_000).unwrap(),
            None
        );
        assert_eq!(
            reopened
                .begin_idle_integrity_attempt(now + INITIAL_DELAY_MS)
                .unwrap(),
            Some(Duration::from_secs(60))
        );
        assert!(
            !reopened
                .idle_integrity_retry_ready(now + 2 * INITIAL_DELAY_MS)
                .unwrap()
        );
        assert!(
            reopened
                .integrity_schedule(now, false)
                .unwrap()
                .last_full_ms
                .is_none()
        );
    }

    #[test]
    fn repeated_incomplete_attempts_back_off_with_a_finite_budget_and_delay() {
        let directory = tempfile::tempdir().unwrap();
        let store = Store::open(StorePaths::new(directory.path())).unwrap();
        let mut now = 1_000_000;
        for attempt in 1..=20 {
            let budget = store.begin_idle_integrity_attempt(now).unwrap().unwrap();
            assert!(budget <= Duration::from_secs(300));
            assert!(budget >= Duration::from_secs(30));
            let next = now
                + INITIAL_DELAY_MS
                    .saturating_mul(4_i64.pow((attempt - 1).min(8)))
                    .min(MAX_DELAY_MS);
            assert!(!store.idle_integrity_retry_ready(next - 1).unwrap());
            assert!(store.idle_integrity_retry_ready(next).unwrap());
            now = next;
        }
    }
}
