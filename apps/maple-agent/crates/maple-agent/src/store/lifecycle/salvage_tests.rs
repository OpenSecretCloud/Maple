use super::*;

fn fixture(directory: &Path) -> Store {
    let store = Store::unavailable(StorePaths::new(directory), StoreState::Maintenance).unwrap();
    store.initialize_recovered().unwrap();
    let worker = store.checkpoint_worker.lock().unwrap().take().unwrap();
    worker
        .sender
        .send(super::super::super::CheckpointMessage::Stop)
        .unwrap();
    worker.thread.join().unwrap();
    store.upsert_account("readable-fixture", 1).unwrap();
    assert!(
        fs::metadata(store.paths.db().join("maple.db-wal"))
            .unwrap()
            .len()
            > 32
    );
    store
}

fn quarantine(store: &Store) {
    set_state(
        store,
        StoreState::Damaged {
            reason: DamageReason::Database,
        },
    )
    .unwrap();
    store.quarantine_generation().unwrap();
}

fn copy_account(source: &Connection, destination: &mut Connection) -> StoreResult<u64> {
    let (id, user_key, user_id, created, seen): (i64, String, String, i64, i64) = source
        .query_row(
            "SELECT id,user_key,user_id,created_ms,last_seen_ms FROM accounts",
            [],
            |row| {
                Ok((
                    row.get(0)?,
                    row.get(1)?,
                    row.get(2)?,
                    row.get(3)?,
                    row.get(4)?,
                ))
            },
        )?;
    destination.execute(
        "INSERT INTO accounts(id,user_key,user_id,created_ms,last_seen_ms) VALUES (?1,?2,?3,?4,?5)",
        params![id, user_key, user_id, created, seen],
    )?;
    Ok(1)
}

#[test]
fn salvage_reads_committed_wal_then_deletes_only_its_superseded_source() {
    let directory = tempfile::tempdir().unwrap();
    let store = fixture(directory.path());
    let original = store.store_id().unwrap();
    quarantine(&store);
    let copied = store
        .install_salvaged_replacement(|source, destination| {
            // A source connection can see the committed WAL, but cannot modify it.
            assert!(source.execute("DELETE FROM accounts", []).is_err());
            copy_account(source, destination)
        })
        .unwrap();
    assert_eq!(copied, 1);
    assert_eq!(store.state(), StoreState::Open);
    assert_ne!(store.store_id().unwrap(), original);
    assert!(store.account_id("readable-fixture").unwrap().is_some());
    for name in ["db.old", "db.new", "db.tmp"] {
        assert!(!store.paths.root().join(name).exists());
    }
    assert!(
        !store
            .paths
            .root()
            .join(format!("db.damaged-{}", encode_id(&original)))
            .exists()
    );
    assert!(!store.paths.setup().exists());
}

#[test]
fn aborted_build_keeps_quarantine_across_restarts_until_explicit_reset() {
    let directory = tempfile::tempdir().unwrap();
    let store = fixture(directory.path());
    let original = store.store_id().unwrap();
    quarantine(&store);
    let before = fs::read(store.paths.database()).unwrap();
    let wal_before = fs::read(store.paths.db().join("maple.db-wal")).unwrap();
    let result = store.install_salvaged_replacement::<()>(|_, _| Err(StoreError::InvalidData));
    assert_eq!(
        result.unwrap_err(),
        LifecycleError::Store(StoreError::InvalidData)
    );
    assert!(matches!(store.state(), StoreState::Damaged { .. }));
    assert_eq!(fs::read(store.paths.database()).unwrap(), before);
    assert_eq!(
        fs::read(store.paths.db().join("maple.db-wal")).unwrap(),
        wal_before
    );
    assert!(!store.paths.root().join("db.tmp").exists());
    assert!(store.paths.setup().exists());
    store.close_connections_without_checkpoint().unwrap();
    drop(store);
    for _ in 0..2 {
        let reopened =
            Store::unavailable(StorePaths::new(directory.path()), StoreState::Maintenance).unwrap();
        reopened.initialize_recovered().unwrap();
        assert!(matches!(reopened.state(), StoreState::Damaged { .. }));
        assert_eq!(
            reopened.lock_status().unwrap().store_id.as_deref(),
            Some(encode_id(&original).as_str())
        );
        assert!(reopened.setup_store(None, &[], KdfParams::floor()).is_err());
    }
    let reopened =
        Store::unavailable(StorePaths::new(directory.path()), StoreState::Maintenance).unwrap();
    reopened.initialize_recovered().unwrap();
    reopened.start_fresh(START_FRESH_CONFIRMATION).unwrap();
    assert_eq!(reopened.state(), StoreState::Open);
    assert!(reopened.account_id("readable-fixture").unwrap().is_none());
}

#[test]
fn cancellation_before_commit_preserves_original_and_known_damage() {
    let directory = tempfile::tempdir().unwrap();
    let store = fixture(directory.path());
    let generation = store.store_id().unwrap();
    quarantine(&store);
    let error = store
        .install_salvaged_replacement(|source, destination| {
            let copied = copy_account(source, destination)?;
            assert!(store.cancel_store_operation());
            Ok(copied)
        })
        .unwrap_err();
    assert_eq!(error, LifecycleError::Cancelled);
    assert_eq!(
        read_generation(&store.paths.db())
            .unwrap()
            .unwrap()
            .store_id,
        encode_id(&generation)
    );
    assert!(
        !store
            .paths
            .root()
            .join(format!("db.damaged-{}", encode_id(&generation)))
            .exists()
    );
    assert!(matches!(store.state(), StoreState::Damaged { .. }));
}

#[test]
fn normal_close_of_a_quarantined_store_does_not_checkpoint_the_source() {
    let directory = tempfile::tempdir().unwrap();
    let store = fixture(directory.path());
    quarantine(&store);
    let database_before = fs::read(store.paths.database()).unwrap();
    let wal = store.paths.db().join("maple.db-wal");
    let wal_before = fs::read(&wal).unwrap();
    store.close().unwrap();
    assert_eq!(fs::read(store.paths.database()).unwrap(), database_before);
    assert_eq!(fs::read(&wal).unwrap(), wal_before);
    drop(store);
    assert_eq!(
        fs::read(StorePaths::new(directory.path()).database()).unwrap(),
        database_before
    );
    assert_eq!(fs::read(&wal).unwrap(), wal_before);
}

#[test]
fn published_install_intent_recovers_even_when_parent_sync_reports_failure() {
    fn fail_sync(_: &Path) -> io::Result<()> {
        Err(io::Error::from(io::ErrorKind::Other))
    }
    let directory = tempfile::tempdir().unwrap();
    let store = fixture(directory.path());
    let old_id = store.store_id().unwrap();
    quarantine(&store);
    let error = store
        .install_salvaged_with_commit_sync(copy_account, fail_sync)
        .unwrap_err();
    assert_eq!(
        error,
        LifecycleError::RecoveryRequired {
            mode: StoreMode::Plain
        }
    );
    assert_eq!(store.state(), StoreState::Maintenance);
    assert!(!store.cancel_store_operation());
    assert_eq!(
        read_json_optional::<SetupJournal>(&store.paths.setup())
            .unwrap()
            .unwrap()
            .phase,
        SetupPhase::InstallNew
    );
    assert!(store.paths.root().join("db.tmp").exists());
    drop(store);
    let recovered =
        Store::unavailable(StorePaths::new(directory.path()), StoreState::Maintenance).unwrap();
    recovered.initialize_recovered().unwrap();
    assert_eq!(recovered.state(), StoreState::Open);
    assert_ne!(recovered.store_id().unwrap(), old_id);
    assert!(recovered.account_id("readable-fixture").unwrap().is_some());
    assert!(!recovered.paths.root().join("db.old").exists());
    assert!(
        !recovered
            .paths
            .root()
            .join(format!("db.damaged-{}", encode_id(&old_id)))
            .exists()
    );
}

#[test]
fn schema_reserved_identity_and_foreign_key_tampering_cannot_commit() {
    for kind in 0..3 {
        let directory = tempfile::tempdir().unwrap();
        let store = fixture(directory.path());
        let original = store.store_id().unwrap();
        quarantine(&store);
        let result = store.install_salvaged_replacement(|_, destination| {
            match kind {
                0 => {destination.execute("UPDATE store_meta SET value=zeroblob(16) WHERE key='store_id'", [])?;},
                1 => {destination.execute("CREATE TABLE untrusted_schema(secret TEXT)", [])?;},
                _ => {
                    destination.pragma_update(None, "foreign_keys", false)?;
                    destination.execute("INSERT INTO account_settings(account_id,key,value,updated_ms) VALUES (987,'test','{}',1)", [])?;
                },
            }
            Ok(())
        });
        assert!(result.is_err());
        assert_eq!(
            read_generation(&store.paths.db())
                .unwrap()
                .unwrap()
                .store_id,
            encode_id(&original)
        );
        assert!(matches!(store.state(), StoreState::Damaged { .. }));
        assert!(!store.paths.root().join("db.new").exists());
    }
}

#[test]
fn password_salvage_requires_current_auth_and_preserves_encryption_on_restart() {
    let directory = tempfile::tempdir().unwrap();
    let store = fixture(directory.path());
    let password = Password::normalize("Z7jv!9qP#3nMx@8Ks^2dH").unwrap();
    store
        .set_store_password(&password, &[], KdfParams::floor())
        .unwrap();
    let old_salt = database_salt(&store.paths.database()).unwrap();
    quarantine(&store);
    assert!(matches!(
        store.install_salvaged_replacement(copy_account),
        Err(LifecycleError::Store(StoreError::Locked))
    ));
    store.authenticate_repair_password(&password).unwrap();
    store.install_salvaged_replacement(copy_account).unwrap();
    assert_eq!(store.lock_status().unwrap().mode, StoreMode::Password);
    assert_ne!(database_salt(&store.paths.database()).unwrap(), old_salt);
    assert!(store.account_id("readable-fixture").unwrap().is_some());
    store.close().unwrap();
    drop(store);
    let reopened =
        Store::unavailable(StorePaths::new(directory.path()), StoreState::Maintenance).unwrap();
    reopened.initialize_recovered().unwrap();
    assert_eq!(reopened.state(), StoreState::Locked);
    reopened.validate_unlock_slot().unwrap();
    reopened.unlock_store(&password, None).unwrap();
    assert!(reopened.account_id("readable-fixture").unwrap().is_some());
}
