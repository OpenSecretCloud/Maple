//! An external WAL snapshot must refuse conversion without destroying retry
//! authority. These use real plain/keyed SQLite handles and native exports.

use super::*;

#[derive(Clone, Copy, Debug)]
enum ExportChange {
    Set,
    Remove,
    Rotate,
}

fn apply_export(
    store: &Store,
    change: ExportChange,
    current: &Password,
    replacement: &Password,
) -> LifecycleResult<PasswordChangeResult> {
    match change {
        ExportChange::Set => store.set_store_password(current, &[], KdfParams::floor()),
        ExportChange::Remove => store.remove_store_password(current),
        ExportChange::Rotate => {
            store.change_store_password(current, replacement, &[], KdfParams::floor(), true)
        }
    }
}

fn key_mapping(store: &Store) -> Option<usize> {
    store
        .dek
        .lock()
        .unwrap()
        .as_ref()
        .map(|key| key.as_bytes().as_ptr() as usize)
}

fn optional_bytes(path: &Path) -> Option<Vec<u8>> {
    path.exists().then(|| fs::read(path).unwrap())
}

#[test]
fn held_external_wal_snapshot_preserves_export_retry_handles_key_slots_and_generation() {
    for change in [
        ExportChange::Set,
        ExportChange::Remove,
        ExportChange::Rotate,
    ] {
        let directory = tempfile::tempdir().unwrap();
        let store = Store::open(StorePaths::new(directory.path())).unwrap();
        let current = Password::normalize("Z7jv!9qP#3nMx@8Ks^2dH").unwrap();
        let replacement = Password::normalize("H5gx&2qT%8mVz!4Kd@7sJ").unwrap();
        let keyed = !matches!(change, ExportChange::Set);
        store.upsert_account("before-export-snapshot", 1).unwrap();
        if keyed {
            store
                .set_store_password(&current, &[], KdfParams::floor())
                .unwrap();
        }
        let paths = store.paths.clone();
        let before_generation = read_generation(&paths.db()).unwrap().unwrap();
        let before_salt = database_salt(&paths.database()).unwrap();
        let before_primary = optional_bytes(&paths.keys());
        let before_backup = optional_bytes(&paths.db().join("keys.backup.json"));
        let before_key_mapping = key_mapping(&store);
        let reader = connection::open(
            &paths,
            connection::Role::Interactive,
            false,
            &|connection| {
                let key = store.dek.lock().map_err(|_| StoreError::Busy)?;
                match key.as_ref() {
                    Some(key) => engine::key_connection(connection, key),
                    None => engine::guard_plain(connection),
                }
            },
        )
        .unwrap();
        reader.execute_batch("BEGIN").unwrap();
        let _: i64 = reader
            .query_row("SELECT count(*) FROM accounts", [], |row| row.get(0))
            .unwrap();
        let saved = store.upsert_account("held-wal-source", 2).unwrap();
        store
            .writer
            .lock()
            .unwrap()
            .as_ref()
            .unwrap()
            .busy_timeout(std::time::Duration::from_millis(20))
            .unwrap();
        for _ in 0..2 {
            assert_eq!(
                apply_export(&store, change, &current, &replacement),
                Err(LifecycleError::Store(StoreError::Busy)),
                "{change:?}"
            );
            assert_eq!(store.state(), StoreState::Open, "{change:?}");
            assert!(!store.runtime_damage.load(Ordering::Acquire));
            assert_eq!(store.account_id("held-wal-source").unwrap(), Some(saved));
            assert_eq!(key_mapping(&store), before_key_mapping);
            assert_eq!(
                read_generation(&paths.db()).unwrap(),
                Some(before_generation.clone())
            );
            assert_eq!(database_salt(&paths.database()).unwrap(), before_salt);
            assert_eq!(optional_bytes(&paths.keys()), before_primary);
            assert_eq!(
                optional_bytes(&paths.db().join("keys.backup.json")),
                before_backup
            );
            assert!(store.writer.lock().unwrap().is_some());
            assert!(
                store
                    .readers
                    .iter()
                    .all(|reader| reader.lock().unwrap().is_some())
            );
            assert!(store.bulk.lock().unwrap().is_some());
            assert!(store.checkpointer.lock().unwrap().is_some());
            assert!(store.checkpoint_worker.lock().unwrap().is_some());
            assert_eq!(store.start_checkpointer(), Err(StoreError::Conflict));
            assert!(!paths.setup().exists());
            for candidate in ["db.tmp", "db.new", "db.old"] {
                assert!(!paths.root().join(candidate).exists());
            }
        }
        store
            .upsert_account("saved-after-export-refusal", 3)
            .unwrap();
        reader.execute_batch("ROLLBACK").unwrap();
        drop(reader);
        let result = apply_export(&store, change, &current, &replacement).unwrap();
        let expected = if matches!(change, ExportChange::Remove) {
            StoreMode::Plain
        } else {
            StoreMode::Password
        };
        assert_eq!(result.mode, expected);
        assert_eq!(store.state(), StoreState::Open);
        assert_eq!(store.lock_status().unwrap().mode, expected);
        assert_eq!(
            read_generation(&paths.db()).unwrap(),
            Some(before_generation)
        );
        assert_eq!(store.account_id("held-wal-source").unwrap(), Some(saved));
        assert!(
            store
                .account_id("saved-after-export-refusal")
                .unwrap()
                .is_some()
        );
        if expected == StoreMode::Password {
            let pair =
                KeyFilePair::load(&paths.db(), &database_salt(&paths.database()).unwrap()).unwrap();
            let password = if matches!(change, ExportChange::Rotate) {
                &replacement
            } else {
                &current
            };
            let unwrapped = pair.primary().unlock(password).unwrap();
            let active = store.dek.lock().unwrap();
            assert!(unwrapped.as_bytes() == active.as_ref().unwrap().as_bytes());
        } else {
            assert!(!paths.keys().exists());
            assert!(store.dek.lock().unwrap().is_none());
        }
        for candidate in ["db.tmp", "db.new", "db.old"] {
            assert!(!paths.root().join(candidate).exists());
        }
        store.close().unwrap();
    }
}
