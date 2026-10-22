//! Exercise damage admission with ordinary store transactions and the real
//! checkpoint worker. Faults are bounded native SQLite result codes, not file
//! mutation or a process-global VFS, so other test connections are unaffected.

use super::*;

fn sqlite_error(code: i32) -> StoreError {
    rusqlite::Error::SqliteFailure(rusqlite::ffi::Error::new(code), None).into()
}

fn fixture(keyed: bool) -> (tempfile::TempDir, StorePaths, Store) {
    let directory = tempfile::tempdir().unwrap();
    let paths = StorePaths::new(directory.path());
    let store = if keyed {
        Store::open_keyed(
            paths.clone(),
            keys::LockedKey::from_bytes([0x73; 32]).unwrap(),
        )
        .unwrap()
    } else {
        Store::open(paths.clone()).unwrap()
    };
    store.upsert_account("quarantine-owner", 1).unwrap();
    (directory, paths, store)
}

fn assert_cold_quarantine(paths: StorePaths, keyed: bool) {
    assert!(paths.setup().is_file());
    let reopened = Store::unavailable(paths, StoreState::Maintenance).unwrap();
    reopened.initialize_recovered().unwrap();
    assert_eq!(
        reopened.state(),
        if keyed {
            StoreState::Locked
        } else {
            StoreState::Damaged {
                reason: DamageReason::Database,
            }
        }
    );
    assert!(reopened.upsert_account("must-not-save", 2).is_err());
    assert!(reopened.checkpoint_worker.lock().unwrap().is_none());
}

#[test]
fn corrupt_read_and_notadb_write_persist_quarantine_before_return_and_cold_reopen() {
    for keyed in [false, true] {
        for (code, write) in [
            (rusqlite::ffi::SQLITE_CORRUPT, false),
            (rusqlite::ffi::SQLITE_NOTADB, true),
        ] {
            let (_directory, paths, store) = fixture(keyed);
            let error = sqlite_error(code);
            let result: StoreResult<()> = if write {
                store.write(Durability::Durable, |_| Err(error.clone()))
            } else {
                store.read(|_| Err(error.clone()))
            };
            assert_eq!(result, Err(error.clone()));
            assert_eq!(
                store.state(),
                StoreState::Damaged {
                    reason: DamageReason::Database
                }
            );
            assert!(
                paths.setup().is_file(),
                "the failed call must persist quarantine synchronously"
            );
            assert!(store.checkpoint_worker.lock().unwrap().is_none());
            assert_eq!(store.upsert_account("must-not-save", 2), Err(error));
            store.close().unwrap();
            drop(store);
            assert_cold_quarantine(paths, keyed);
        }
    }
}

#[test]
fn operational_read_and_write_errors_do_not_persist_database_damage() {
    for code in [
        rusqlite::ffi::SQLITE_IOERR,
        rusqlite::ffi::SQLITE_CANTOPEN,
        rusqlite::ffi::SQLITE_BUSY,
        rusqlite::ffi::SQLITE_FULL,
    ] {
        let (_directory, paths, store) = fixture(false);
        let error = sqlite_error(code);
        let read: StoreResult<()> = store.read(|_| Err(error.clone()));
        assert_eq!(read, Err(error.clone()));
        let write: StoreResult<()> = store.write(Durability::Durable, |_| Err(error.clone()));
        assert_eq!(write, Err(error));
        assert_eq!(store.state(), StoreState::Open);
        assert!(!paths.setup().exists());
        store.upsert_account("still-writable", 2).unwrap();
        store.close().unwrap();
        drop(store);
        let reopened = Store::open(paths).unwrap();
        assert!(reopened.account_id("still-writable").unwrap().is_some());
    }
}

#[test]
fn checkpoint_corruption_exits_its_worker_and_quarantines_without_joining_itself() {
    for keyed in [false, true] {
        let (_directory, paths, store) = fixture(keyed);
        let worker = store.checkpoint_worker.lock().unwrap().take().unwrap();
        worker.sender.send(CheckpointMessage::Stop).unwrap();
        worker.thread.join().unwrap();
        store
            .start_checkpointer_with(|_, _| Err(sqlite_error(rusqlite::ffi::SQLITE_CORRUPT)))
            .unwrap();
        store.upsert_account("before-checkpoint", 2).unwrap();
        let deadline = Instant::now() + Duration::from_secs(5);
        loop {
            if store
                .checkpoint_worker
                .lock()
                .unwrap()
                .as_ref()
                .unwrap()
                .thread
                .is_finished()
            {
                break;
            }
            assert!(
                Instant::now() < deadline,
                "checkpoint quarantine must not deadlock"
            );
            std::thread::sleep(Duration::from_millis(5));
        }
        assert_eq!(
            store.state(),
            StoreState::Damaged {
                reason: DamageReason::Database
            }
        );
        assert!(paths.setup().is_file());
        store.close().unwrap();
        drop(store);
        assert_cold_quarantine(paths, keyed);
    }
}

#[test]
fn shutdown_race_does_not_checkpoint_after_a_transaction_has_detected_damage() {
    let (_directory, paths, store) = fixture(false);
    // A lifecycle operation may have already changed Open to Maintenance when
    // an admitted reader reports corruption. The independent latch survives
    // that presentation state and forbids the final checkpoint.
    store.runtime_damage.store(true, Ordering::Release);
    *store.state.write().unwrap() = StoreState::Maintenance;
    assert_eq!(
        store.close_connections(),
        Err(StoreError::Damaged {
            reason: DamageReason::Database
        })
    );
    assert!(paths.setup().is_file());
    assert!(store.writer.lock().unwrap().is_none());
    drop(store);
    assert_cold_quarantine(paths, false);
}

#[test]
fn confirmed_reset_joins_an_inflight_corrupt_checkpoint_before_publishing_its_intent() {
    for keyed in [false, true] {
        let (_directory, paths, store) = fixture(keyed);
        let old_generation = store.store_id().unwrap();
        let worker = store.checkpoint_worker.lock().unwrap().take().unwrap();
        worker.sender.send(CheckpointMessage::Stop).unwrap();
        worker.thread.join().unwrap();
        let store = Arc::new(store);
        // These owned channels are timed barriers. No global fault state or
        // another test's native connection can participate in this race.
        let (entered, at_checkpoint) = mpsc::sync_channel(1);
        let (release_checkpoint, released) = mpsc::sync_channel(1);
        store
            .start_checkpointer_with(move |connection, truncate| {
                entered.send(()).unwrap();
                released.recv_timeout(Duration::from_secs(10)).unwrap();
                // Exercise the real native call while the worker still owns its
                // connection guard, then simulate its detected corruption result.
                connection::checkpoint(connection, truncate)?;
                Err(sqlite_error(rusqlite::ffi::SQLITE_CORRUPT))
            })
            .unwrap();
        store.upsert_account("before-confirmed-erase", 2).unwrap();
        at_checkpoint.recv_timeout(Duration::from_secs(5)).unwrap();
        let resetting_store = Arc::clone(&store);
        let resetting = std::thread::spawn(move || resetting_store.erase_local_data("ERASE"));
        let deadline = Instant::now() + Duration::from_secs(5);
        loop {
            // Taking the worker out of its slot occurs immediately before
            // join. This proves reset reached that barrier, rather than merely
            // observing an early Maintenance state before intent publication.
            if store.checkpoint_worker.lock().unwrap().is_none() {
                break;
            }
            assert!(
                Instant::now() < deadline,
                "reset did not reach the worker join"
            );
            std::thread::sleep(Duration::from_millis(5));
        }
        let intent_published_before_join = paths.setup().exists();
        release_checkpoint.send(()).unwrap();
        resetting.join().unwrap().unwrap();
        assert!(
            !intent_published_before_join,
            "a confirmed reset cannot publish RetireOld while an old worker can still publish quarantine"
        );
        store.close().unwrap();
        drop(store);
        let reopened = Store::unavailable(paths, StoreState::Maintenance).unwrap();
        reopened.initialize_recovered().unwrap();
        if keyed {
            assert_eq!(reopened.state(), StoreState::NeedsSetup);
            reopened
                .setup_store(None, &[], keys::KdfParams::floor())
                .unwrap();
        }
        assert_eq!(reopened.state(), StoreState::Open);
        assert_ne!(reopened.store_id().unwrap(), old_generation);
        assert!(reopened.account_id("quarantine-owner").unwrap().is_none());
        assert!(
            reopened
                .account_id("before-confirmed-erase")
                .unwrap()
                .is_none()
        );
        assert!(!reopened.paths.setup().exists());
    }
}
