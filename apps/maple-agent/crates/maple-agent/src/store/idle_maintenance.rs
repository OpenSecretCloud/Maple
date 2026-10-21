//! Small physical-storage and planner maintenance chunks, admitted only while idle.

use super::{Store, StoreResult};

const VACUUM_PAGE_CAP: u16 = 64;

impl Store {
    /// Reclaim at most 64 freelist pages per call; physical truncation follows
    /// the normal checkpoint policy. Admission uses the same nonblocking writer
    /// gate as collectors, so queued foreground writes take precedence. The
    /// caller schedules planner maintenance less frequently than vacuum chunks.
    pub(crate) fn idle_storage_maintenance(&self, optimize: bool) -> StoreResult<()> {
        self.write_idle(|transaction| {
            // incremental_vacuum returns one row after each page. Executing only
            // its first step would reclaim one page regardless of the limit.
            let mut vacuum =
                transaction.prepare(&format!("PRAGMA incremental_vacuum({VACUUM_PAGE_CAP})"))?;
            let mut rows = vacuum.query([])?;
            while rows.next()?.is_some() {}
            drop(rows);
            drop(vacuum);

            if optimize {
                // The linked SQLite's default optimize mask applies its bounded
                // analysis_limit; do not substitute an unbounded ANALYZE.
                transaction.execute_batch("PRAGMA optimize")?;
            }
            Ok(())
        })
    }
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::Ordering;

    use rusqlite::params;

    use super::*;
    use crate::store::{Durability, StoreError, StorePaths, keys::LockedKey};

    fn fixture(keyed: bool) -> (tempfile::TempDir, Store) {
        let directory = tempfile::tempdir().unwrap();
        let paths = StorePaths::new(directory.path());
        let store = if keyed {
            Store::open_keyed(paths, LockedKey::from_bytes([0x36; 32]).unwrap()).unwrap()
        } else {
            Store::open(paths).unwrap()
        };
        (directory, store)
    }

    fn free_pages(store: &Store) -> i64 {
        store
            .read(|transaction| {
                Ok(transaction.pragma_query_value(None, "freelist_count", |row| row.get(0))?)
            })
            .unwrap()
    }

    fn seed_deleted_blob(store: &Store) -> Vec<u8> {
        let live = vec![0xa6; 64 * 1024];
        store
            .write(Durability::Durable, |transaction| {
                transaction.execute(
                    "INSERT INTO blobs(id,sha256,mime,size,complete,created_ms)
                     VALUES(1,?1,'application/octet-stream',1048576,1,1),
                           (2,?2,'application/octet-stream',65536,1,1)",
                    params![[1_u8; 32].as_slice(), [2_u8; 32].as_slice()],
                )?;
                for index in 0..4 {
                    transaction.execute(
                        "INSERT INTO blob_chunks(blob_id,idx,data) VALUES(1,?1,zeroblob(262144))",
                        [index],
                    )?;
                }
                transaction.execute(
                    "INSERT INTO blob_chunks(blob_id,idx,data) VALUES(2,0,?1)",
                    [&live],
                )?;
                Ok(())
            })
            .unwrap();
        store
            .write(Durability::Durable, |transaction| {
                transaction.execute("DELETE FROM blobs WHERE id=1", [])?;
                Ok(())
            })
            .unwrap();
        live
    }

    fn assert_live_blob(store: &Store, expected: &[u8]) {
        let actual: Vec<u8> = store
            .read(|transaction| {
                Ok(transaction.query_row(
                    "SELECT data FROM blob_chunks WHERE blob_id=2 AND idx=0",
                    [],
                    |row| row.get(0),
                )?)
            })
            .unwrap();
        assert_eq!(actual, expected);
    }

    #[test]
    fn incremental_chunks_reclaim_multiple_pages_with_a_fixed_bound_and_preserve_live_blobs() {
        for keyed in [false, true] {
            let (_directory, store) = fixture(keyed);
            let live = seed_deleted_blob(&store);
            let mut before = free_pages(&store);
            assert!(before > i64::from(VACUUM_PAGE_CAP) * 3);

            store.idle_storage_maintenance(false).unwrap();
            let after = free_pages(&store);
            assert_eq!(before - after, i64::from(VACUUM_PAGE_CAP));
            assert_live_blob(&store, &live);
            before = after;

            for _ in 0..16 {
                store.idle_storage_maintenance(false).unwrap();
                let after = free_pages(&store);
                assert!((0..=i64::from(VACUUM_PAGE_CAP)).contains(&(before - after)));
                assert_live_blob(&store, &live);
                if after == 0 {
                    break;
                }
                assert!(after < before);
                before = after;
            }
            assert_eq!(free_pages(&store), 0);
            // Repeated idle scheduling remains harmless once reclamation ends.
            store.idle_storage_maintenance(true).unwrap();
            assert_live_blob(&store, &live);
            store.close().unwrap();
        }
    }

    #[test]
    fn maintenance_yields_to_foreground_and_refuses_a_busy_or_closed_writer() {
        let (_directory, store) = fixture(false);
        store.foreground_waiters.store(1, Ordering::Release);
        assert_eq!(store.idle_storage_maintenance(true), Err(StoreError::Busy));
        store.foreground_waiters.store(0, Ordering::Release);

        let writer = store.writer.lock().unwrap();
        assert_eq!(store.idle_storage_maintenance(true), Err(StoreError::Busy));
        drop(writer);

        store.close().unwrap();
        assert_eq!(store.idle_storage_maintenance(true), Err(StoreError::Busy));
    }
}
