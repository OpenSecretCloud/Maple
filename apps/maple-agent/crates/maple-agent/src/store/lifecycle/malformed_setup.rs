//! Explicit reset can supersede one malformed public setup record only when
//! its source generation is verified and no staged operation is ambiguous.

use super::{unclassified_identity::DirectoryManifest, *};

impl Store {
    pub(super) fn supersede_malformed_setup_locked(
        &self,
        after: ResetAfter,
        intent_sync: fn(&Path) -> io::Result<()>,
    ) -> LifecycleResult<()> {
        check_unstaged_source(&self.paths)?;
        let generation = read_generation(&self.paths.db())?.ok_or(LifecycleError::Generation)?;
        require_malformed_setup(&self.paths)?;

        // Stop all writers before capturing file identities and sizes. A
        // failed preflight may leave the store unavailable, never partially
        // retire or replace the original source or the malformed record.
        set_state(self, StoreState::Maintenance)?;
        self.close_connections_without_checkpoint()?;
        check_unstaged_source(&self.paths)?;
        let source = DirectoryManifest::capture(&self.paths.db())?;
        let control = unclassified_identity::capture_path(&self.paths.setup(), false)?;
        let mode = conservative_reset_mode(&self.paths)?;
        let journal = SetupJournal {
            format: SETUP_FORMAT.into(),
            operation_id: random_id()?,
            after,
            previous_mode: mode,
            old_store_id: generation.store_id.clone(),
            new_store_id: random_id()?,
            phase: SetupPhase::RetireOld,
        };
        preflight_setup(&self.paths, &journal)?;
        source.verify(&self.paths.db(), false)?;
        if read_generation(&self.paths.db())?.as_ref() != Some(&generation)
            || unclassified_identity::capture_path(&self.paths.setup(), false)? != control
        {
            return Err(LifecycleError::Generation);
        }
        require_malformed_setup(&self.paths)?;
        check_unstaged_source(&self.paths)?;
        // Atomic publication leaves either the malformed control record or a
        // complete ordinary reset intent. Its source identity is the verified
        // public generation, never an invented replacement for a missing ID.
        if write_json_with_sync(&self.paths.setup(), &journal, intent_sync).is_err() {
            return Err(LifecycleError::RecoveryRequired { mode });
        }
        self.begin_reset_locked(after)
    }
}

fn check_unstaged_source(paths: &StorePaths) -> LifecycleResult<()> {
    verify_private_path(paths.root(), true)?;
    for path in [
        paths.root().join("unclassified-reset.json"),
        paths.root().join("db.tmp"),
        paths.root().join("db.new"),
        paths.root().join("db.old"),
    ] {
        if path_present(&path)? {
            return Err(LifecycleError::Generation);
        }
    }
    Ok(())
}

fn require_malformed_setup(paths: &StorePaths) -> LifecycleResult<()> {
    verify_private_path(&paths.setup(), false)?;
    let mut options = OpenOptions::new();
    options.read(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.custom_flags(libc::O_NOFOLLOW | libc::O_CLOEXEC);
    }
    let mut bytes = Vec::new();
    options
        .open(paths.setup())?
        .take(16 * 1024 + 1)
        .read_to_end(&mut bytes)?;
    if bytes.len() > 16 * 1024 {
        return Err(LifecycleError::Generation);
    }
    // RawValue validates syntax without interpreting schema, numeric range or
    // recursively materializing nested values. Valid JSON of any unsupported
    // schema must be refused, including a future operation or phase. The
    // pinned parser uses a bounded iterative stack for this raw-value path.
    match serde_json::from_slice::<&serde_json::value::RawValue>(&bytes) {
        Ok(_) => Err(LifecycleError::Generation),
        Err(error) if error.is_syntax() || error.is_eof() => Ok(()),
        Err(_) => Err(LifecycleError::Generation),
    }
}

/// With existing generation metadata, only a positive plaintext header can
/// authorize automatic plain setup. Empty/short/unreadable bytes require the
/// same explicit mode choice as an encrypted source, even without key files.
pub(super) fn conservative_reset_mode(paths: &StorePaths) -> LifecycleResult<StoreMode> {
    if path_present(&paths.keys())? || path_present(&paths.db().join("keys.backup.json"))? {
        return Ok(StoreMode::Password);
    }
    let metadata = match fs::symlink_metadata(paths.database()) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(StoreMode::Password),
        Err(error) => return Err(error.into()),
    };
    verify_private_path(&paths.database(), false)?;
    if metadata.len() < 16 {
        return Ok(StoreMode::Password);
    }
    Ok(
        if &database_salt(&paths.database())? == b"SQLite format 3\0" {
            StoreMode::Plain
        } else {
            StoreMode::Password
        },
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn malformed_source(path: &Path) -> Store {
        let store = Store::unavailable(StorePaths::new(path), StoreState::Maintenance).unwrap();
        store.initialize_recovered().unwrap();
        store.upsert_account("current-owned-history", 1).unwrap();
        store.close().unwrap();
        let mut control = private_new_file(&store.paths.setup()).unwrap();
        control
            .write_all(b"{malformed current setup record")
            .unwrap();
        keys::sync_key_file(&control).unwrap();
        drop(control);
        sync_directory(store.paths.root()).unwrap();
        drop(store);
        let store = Store::unavailable(StorePaths::new(path), StoreState::Maintenance).unwrap();
        assert_eq!(
            store.initialize_recovered().unwrap_err(),
            StoreError::Damaged {
                reason: DamageReason::Generation
            }
        );
        assert!(store.lock_status().unwrap().store_id.is_none());
        store
    }

    #[test]
    fn explicit_start_fresh_supersedes_one_malformed_record_and_discards_current_source() {
        let directory = tempfile::tempdir().unwrap();
        let outside = directory.path().join("previous-data-canary");
        fs::write(&outside, b"never touch previous namespaces").unwrap();
        let store = malformed_source(directory.path());
        let before = fs::read(store.paths.database()).unwrap();
        let control = fs::read(store.paths.setup()).unwrap();
        let original = read_generation(&store.paths.db()).unwrap().unwrap();
        assert_eq!(
            store.start_fresh("wrong confirmation").unwrap_err(),
            LifecycleError::Confirmation
        );
        assert_eq!(fs::read(store.paths.setup()).unwrap(), control);
        assert_eq!(fs::read(store.paths.database()).unwrap(), before);
        store.start_fresh(START_FRESH_CONFIRMATION).unwrap();
        assert_eq!(store.state(), StoreState::Open);
        assert!(store.account_id("current-owned-history").unwrap().is_none());
        assert_ne!(
            store.store_id().unwrap(),
            decode_id(&original.store_id).unwrap()
        );
        assert!(!store.paths.root().join("db.old").exists());
        assert!(
            !store
                .paths
                .root()
                .join(format!("db.damaged-{}", original.store_id))
                .exists()
        );
        assert!(!store.paths.setup().exists());
        assert_eq!(
            fs::read(outside).unwrap(),
            b"never touch previous namespaces"
        );
    }

    #[test]
    fn superseding_intent_sync_failure_is_fenced_and_both_reset_choices_resume() {
        fn fail(_: &Path) -> io::Result<()> {
            Err(io::Error::from(io::ErrorKind::Other))
        }
        for after in [ResetAfter::Erase, ResetAfter::StartFresh] {
            let directory = tempfile::tempdir().unwrap();
            let store = malformed_source(directory.path());
            let before = fs::read(store.paths.database()).unwrap();
            let original = read_generation(&store.paths.db()).unwrap().unwrap();
            {
                let _gate = store.lifecycle.lock().unwrap();
                assert_eq!(
                    store
                        .begin_reset_with_intent_sync_locked(after, fail)
                        .unwrap_err(),
                    LifecycleError::RecoveryRequired {
                        mode: StoreMode::Plain
                    }
                );
            }
            assert_eq!(store.state(), StoreState::Maintenance);
            assert_eq!(fs::read(store.paths.database()).unwrap(), before);
            assert!(store.upsert_account("must-not-be-acknowledged", 2).is_err());
            let intent: SetupJournal = read_json_optional(&store.paths.setup()).unwrap().unwrap();
            assert_eq!(intent.old_store_id, original.store_id);
            assert_eq!(intent.after, after);
            drop(store);
            let store =
                Store::unavailable(StorePaths::new(directory.path()), StoreState::Maintenance)
                    .unwrap();
            store.initialize_recovered().unwrap();
            assert_eq!(store.state(), StoreState::Open);
            assert!(store.account_id("current-owned-history").unwrap().is_none());
            assert!(
                store
                    .account_id("must-not-be-acknowledged")
                    .unwrap()
                    .is_none()
            );
            assert!(!store.paths.root().join("db.old").exists());
            assert!(
                !store
                    .paths
                    .root()
                    .join(format!("db.damaged-{}", original.store_id))
                    .exists()
            );
            assert!(!store.paths.setup().exists());
        }
    }

    #[test]
    fn malformed_setup_never_selects_a_staged_or_competing_operation() {
        for artifact in ["db.tmp", "db.new", "db.old", "unclassified-reset.json"] {
            let directory = tempfile::tempdir().unwrap();
            let store = malformed_source(directory.path());
            let path = store.paths.root().join(artifact);
            if artifact.ends_with(".json") {
                let mut file = private_new_file(&path).unwrap();
                file.write_all(b"{another malformed control record")
                    .unwrap();
            } else {
                private_directory(&path).unwrap();
            }
            let before = fs::read(store.paths.database()).unwrap();
            let control = fs::read(store.paths.setup()).unwrap();
            assert!(store.erase_local_data(ERASE_CONFIRMATION).is_err());
            assert_eq!(fs::read(store.paths.database()).unwrap(), before);
            assert_eq!(fs::read(store.paths.setup()).unwrap(), control);
            assert!(path.exists());
        }
    }

    #[test]
    fn valid_conflicting_setup_and_unverified_source_ids_are_never_superseded() {
        for conflict in ["operation", "generation"] {
            let directory = tempfile::tempdir().unwrap();
            let store = malformed_source(directory.path());
            if conflict == "operation" {
                write_json(
                    &store.paths.setup(),
                    &SetupJournal {
                        format: SETUP_FORMAT.into(),
                        operation_id: encode_id(&[51; 16]),
                        after: ResetAfter::Erase,
                        previous_mode: StoreMode::Plain,
                        old_store_id: encode_id(&[52; 16]),
                        new_store_id: encode_id(&[53; 16]),
                        phase: SetupPhase::RetireOld,
                    },
                )
                .unwrap();
            } else {
                fs::write(store.paths.generation(), b"{unverified generation").unwrap();
            }
            let before = fs::read(store.paths.database()).unwrap();
            let control = fs::read(store.paths.setup()).unwrap();
            let generation = fs::read(store.paths.generation()).unwrap();
            assert!(store.erase_local_data(ERASE_CONFIRMATION).is_err());
            assert_eq!(fs::read(store.paths.database()).unwrap(), before);
            assert_eq!(fs::read(store.paths.setup()).unwrap(), control);
            assert_eq!(fs::read(store.paths.generation()).unwrap(), generation);
        }
    }

    #[test]
    fn valid_unknown_json_and_oversized_controls_are_never_treated_as_malformed() {
        let controls = [
            br#"{"phase":"a_future_phase"}"#.to_vec(),
            br#"{"format":"maple-store-setup/2"}"#.to_vec(),
            b"null".to_vec(),
            br#"{"future_counter":1e999999}"#.to_vec(),
            format!("{}0{}", "[".repeat(256), "]".repeat(256)).into_bytes(),
            format!("{{\"future\":\"{}\"}}", "x".repeat(16 * 1024)).into_bytes(),
        ];
        for control in controls {
            let directory = tempfile::tempdir().unwrap();
            let store = malformed_source(directory.path());
            fs::write(store.paths.setup(), &control).unwrap();
            let before = fs::read(store.paths.database()).unwrap();
            let generation = fs::read(store.paths.generation()).unwrap();
            assert!(store.erase_local_data(ERASE_CONFIRMATION).is_err());
            assert_eq!(fs::read(store.paths.database()).unwrap(), before);
            assert_eq!(fs::read(store.paths.setup()).unwrap(), control);
            assert_eq!(fs::read(store.paths.generation()).unwrap(), generation);
        }
    }

    #[test]
    fn unknown_or_short_source_bytes_with_valid_metadata_never_auto_select_plain() {
        for bytes in [
            b"".as_slice(),
            b"short".as_slice(),
            b"unrecognized 16!".as_slice(),
        ] {
            for malformed in [true, false] {
                let directory = tempfile::tempdir().unwrap();
                let store = malformed_source(directory.path());
                if !malformed {
                    fs::remove_file(store.paths.setup()).unwrap();
                    set_state(
                        &store,
                        StoreState::Damaged {
                            reason: DamageReason::Database,
                        },
                    )
                    .unwrap();
                }
                fs::write(store.paths.database(), bytes).unwrap();
                let generation = read_generation(&store.paths.db()).unwrap().unwrap();
                let status = store.erase_local_data(ERASE_CONFIRMATION).unwrap();
                assert_eq!(status.state, StoreState::NeedsSetup);
                assert_eq!(status.mode, StoreMode::Password);
                assert!(!store.paths.database().exists());
                let intent: SetupJournal =
                    read_json_optional(&store.paths.setup()).unwrap().unwrap();
                assert_eq!(intent.previous_mode, StoreMode::Password);
                assert_eq!(intent.old_store_id, generation.store_id);
                assert!(!store.paths.root().join("db.old").exists());
                store.setup_store(None, &[], KdfParams::floor()).unwrap();
                assert_eq!(store.state(), StoreState::Open);
                assert_eq!(store.lock_status().unwrap().mode, StoreMode::Plain);
            }
        }
    }

    #[test]
    fn startup_does_not_initialize_short_existing_generation_bytes_as_plaintext() {
        for bytes in [b"".as_slice(), b"short".as_slice()] {
            let directory = tempfile::tempdir().unwrap();
            let store = malformed_source(directory.path());
            fs::remove_file(store.paths.setup()).unwrap();
            fs::write(store.paths.database(), bytes).unwrap();
            let generation = fs::read(store.paths.generation()).unwrap();
            drop(store);
            let store =
                Store::unavailable(StorePaths::new(directory.path()), StoreState::Maintenance)
                    .unwrap();
            store.initialize_recovered().unwrap();
            assert_eq!(store.state(), StoreState::Locked);
            assert_eq!(store.lock_status().unwrap().mode, StoreMode::Password);
            assert_eq!(fs::read(store.paths.database()).unwrap(), bytes);
            assert_eq!(fs::read(store.paths.generation()).unwrap(), generation);
            assert!(!store.paths.keys().exists());
        }
    }
}
