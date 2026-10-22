//! Real SQLite transactions with a page limit or an isolated WAL-write fault
//! exercise SQLITE_FULL. These regressions use no replacement session store.

use super::*;

type WriteFn = unsafe extern "C" fn(
    *mut rusqlite::ffi::sqlite3_file,
    *const std::ffi::c_void,
    i32,
    i64,
) -> i32;

thread_local! {
    static ORIGINAL_WRITE: std::cell::Cell<Option<WriteFn>> = const { std::cell::Cell::new(None) };
    static FAIL_NEXT_WRITE: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
    static WRITE_FAILED: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
}

unsafe extern "C" fn full_on_next_wal_write(
    file: *mut rusqlite::ffi::sqlite3_file,
    bytes: *const std::ffi::c_void,
    count: i32,
    offset: i64,
) -> i32 {
    if FAIL_NEXT_WRITE.with(|armed| armed.replace(false)) {
        WRITE_FAILED.with(|failed| failed.set(true));
        return rusqlite::ffi::SQLITE_FULL;
    }
    match ORIGINAL_WRITE.with(std::cell::Cell::get) {
        // SAFETY: the wrapper changes no file/data pointers or lengths. This
        // is the exact xWrite saved from this connection's live WAL handle.
        Some(write) => unsafe { write(file, bytes, count, offset) },
        None => rusqlite::ffi::SQLITE_IOERR,
    }
}

/// Only the calling test's writer is affected. Its mutex is held throughout;
/// readers/checkpointers own separate WAL handles. No process-global SQLite
/// VFS registration or engine setting changes, even with parallel tests.
fn with_commit_full<T>(
    writer: &mut Connection,
    operation: impl FnOnce(&mut Connection) -> T,
) -> (T, bool) {
    struct Restore {
        file: *mut rusqlite::ffi::sqlite3_file,
        original: *const rusqlite::ffi::sqlite3_io_methods,
        _replacement: Box<rusqlite::ffi::sqlite3_io_methods>,
    }
    impl Drop for Restore {
        fn drop(&mut self) {
            // SAFETY: `operation` cannot consume/close the borrowed Connection.
            // The writer mutex remains held; restore before freeing the copied
            // methods or allowing another operation to use this WAL handle.
            unsafe {
                (*self.file).pMethods = self.original;
            }
            ORIGINAL_WRITE.with(|original| original.set(None));
            FAIL_NEXT_WRITE.with(|armed| armed.set(false));
        }
    }
    let mut file = std::ptr::null_mut::<rusqlite::ffi::sqlite3_file>();
    // SAFETY: SQLite documents JOURNAL_POINTER as writing one sqlite3_file*;
    // WAL is already open from the store's migration/initial commits.
    let code = unsafe {
        rusqlite::ffi::sqlite3_file_control(
            writer.handle(),
            c"main".as_ptr(),
            rusqlite::ffi::SQLITE_FCNTL_JOURNAL_POINTER,
            (&mut file as *mut *mut rusqlite::ffi::sqlite3_file).cast(),
        )
    };
    assert_eq!(code, rusqlite::ffi::SQLITE_OK);
    assert!(!file.is_null());
    // SAFETY: the preceding file-control returned this live connection's WAL
    // handle. Its method table is copied; the shared original is never mutated.
    let original = unsafe { (*file).pMethods };
    assert!(!original.is_null());
    // SAFETY: sqlite3_io_methods is a Copy table at the checked live pointer.
    let mut replacement = Box::new(unsafe { *original });
    ORIGINAL_WRITE.with(|saved| {
        assert!(saved.get().is_none(), "fault scopes must not nest");
        saved.set(replacement.xWrite);
    });
    assert!(replacement.xWrite.is_some());
    replacement.xWrite = Some(full_on_next_wal_write);
    WRITE_FAILED.with(|failed| failed.set(false));
    FAIL_NEXT_WRITE.with(|armed| armed.set(true));
    // SAFETY: replacement stays allocated until Restore reinstalls original.
    unsafe {
        (*file).pMethods = &*replacement;
    }
    let restore = Restore {
        file,
        original,
        _replacement: replacement,
    };
    let result = operation(writer);
    let failed = WRITE_FAILED.with(std::cell::Cell::get);
    drop(restore);
    (result, failed)
}

#[test]
fn wal_write_full_during_commit_rolls_back_without_publishing_saved_change() {
    let directory = tempfile::tempdir().unwrap();
    let store = Store::open(StorePaths::new(directory.path())).unwrap();
    let mut changes = store.subscribe_changes();
    let closure_completed = std::cell::Cell::new(false);
    let (result, fault_hit): (StoreResult<()>, bool) = {
        let mut guard = store.writer.lock().unwrap();
        with_commit_full(guard.as_mut().unwrap(), |writer| {
            store.write_locked(writer, Durability::Durable, |tx| {
                tx.execute(
                    "INSERT INTO dictionary_words(word) VALUES ('commit failure fixture')",
                    [],
                )?;
                closure_completed.set(true);
                Ok((
                    (),
                    Some(StoreChange {
                        listed_changed: true,
                        ..Default::default()
                    }),
                ))
            })
        })
    };
    assert!(closure_completed.get(), "statement succeeded before COMMIT");
    assert!(fault_hit, "COMMIT reached the failing real WAL xWrite");
    assert_eq!(result, Err(StoreError::DiskFull));
    assert_eq!(store.state(), StoreState::Open);
    assert!(matches!(
        changes.try_recv(),
        Err(tokio::sync::broadcast::error::TryRecvError::Empty)
    ));
    assert!(store.dictionary_words(None, 10).unwrap().words.is_empty());
    assert_eq!(
        store.persistence_failure().unwrap().reason,
        PersistenceFailureReason::DiskFull
    );
    // Removing the VFS fault permits a genuine retry of the same mutation.
    assert!(store.add_dictionary_word("commit failure fixture").unwrap());
    assert_eq!(
        store.dictionary_words(None, 10).unwrap().words,
        ["commit failure fixture"]
    );
}

fn constrain_growth(store: &Store) -> i64 {
    let writer = store.writer.lock().unwrap();
    let connection = writer.as_ref().unwrap();
    let pages = connection
        .pragma_query_value::<i64, _>(None, "page_count", |row| row.get(0))
        .unwrap();
    connection
        .pragma_update(None, "max_page_count", pages)
        .unwrap();
    assert_eq!(
        connection
            .pragma_query_value::<i64, _>(None, "max_page_count", |row| row.get(0))
            .unwrap(),
        pages
    );
    pages
}

#[test]
fn sqlite_full_rolls_back_all_effects_without_acknowledging_changes_or_damage() {
    let directory = tempfile::tempdir().unwrap();
    let store = Store::open(StorePaths::new(directory.path())).unwrap();
    store
        .write(Durability::Durable, |tx| {
            tx.execute_batch("CREATE TABLE disk_full_fixture(value BLOB)")?;
            Ok(())
        })
        .unwrap();
    let mut changes = store.subscribe_changes();
    let pages = constrain_growth(&store);
    let result: StoreResult<()> = store.write_with_change(Durability::Durable, |tx| {
        tx.execute(
            "INSERT INTO dictionary_words(word) VALUES ('uncommitted fixture')",
            [],
        )?;
        tx.execute(
            "INSERT INTO disk_full_fixture(value) VALUES (zeroblob(1048576))",
            [],
        )?;
        Ok((
            (),
            Some(StoreChange {
                listed_changed: true,
                ..Default::default()
            }),
        ))
    });
    assert_eq!(result, Err(StoreError::DiskFull));
    assert_eq!(store.state(), StoreState::Open);
    assert!(matches!(
        changes.try_recv(),
        Err(tokio::sync::broadcast::error::TryRecvError::Empty)
    ));
    let (words, payloads) = store
        .read(|tx| {
            Ok((
                tx.query_row::<i64, _, _>("SELECT count(*) FROM dictionary_words", [], |row| {
                    row.get(0)
                })?,
                tx.query_row::<i64, _, _>("SELECT count(*) FROM disk_full_fixture", [], |row| {
                    row.get(0)
                })?,
            ))
        })
        .unwrap();
    assert_eq!((words, payloads), (0, 0));
    let failed = store.persistence_failure().unwrap();
    assert_eq!(failed.reason, PersistenceFailureReason::DiskFull);
    assert_eq!(
        failed.store_id,
        Some(
            uuid::Uuid::from_bytes(store.store_id().unwrap())
                .simple()
                .to_string()
        )
    );
    // Unrelated successful writes must not hide unsaved source elsewhere.
    store.add_dictionary_word("unrelated").unwrap();
    assert_eq!(store.persistence_failure().as_ref(), Some(&failed));
    let writer = store.writer.lock().unwrap();
    writer
        .as_ref()
        .unwrap()
        .pragma_update(None, "max_page_count", pages + 1024)
        .unwrap();
    drop(writer);
    store
        .write_with_change(Durability::Durable, |tx| {
            tx.execute(
                "INSERT INTO disk_full_fixture(value) VALUES (zeroblob(1048576))",
                [],
            )?;
            Ok((
                (),
                Some(StoreChange {
                    listed_changed: true,
                    ..Default::default()
                }),
            ))
        })
        .unwrap();
    assert!(changes.try_recv().unwrap().listed_changed);
    assert_eq!(store.persistence_failure().as_ref(), Some(&failed));
    assert!(store.clear_persistence_failure(failed.revision));
    assert!(store.persistence_failure().is_none());
    assert!(store.persistence_status().revision > failed.revision);
    assert!(!store.clear_persistence_failure(failed.revision));
}

#[test]
fn initialization_disk_full_stays_retryable_and_never_becomes_damage() {
    let directory = tempfile::tempdir().unwrap();
    let store =
        Store::unavailable(StorePaths::new(directory.path()), StoreState::Maintenance).unwrap();
    store.record_initialization_error(&StoreError::DiskFull);
    assert_eq!(store.state(), StoreState::Maintenance);
    assert_eq!(
        store.persistence_failure().unwrap().reason,
        PersistenceFailureReason::DiskFull
    );
    store.initialize_plain().unwrap();
    assert_eq!(store.state(), StoreState::Open);
    assert!(store.persistence_failure().is_none());
}
