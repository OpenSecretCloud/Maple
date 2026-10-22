use std::sync::{Arc, Barrier, mpsc};
use std::time::Duration;

use super::*;
use crate::store::StorePaths;

fn fixture() -> (tempfile::TempDir, Arc<Store>, AccountId, AccountId) {
    let directory = tempfile::tempdir().unwrap();
    let store = Arc::new(Store::open(StorePaths::new(directory.path())).unwrap());
    let alice = store.upsert_account("alice", 1).unwrap();
    let bob = store.upsert_account("bob", 1).unwrap();
    create_task(&store, alice, "alice-task");
    create_task(&store, bob, "bob-task");
    (directory, store, alice, bob)
}

fn create_task(store: &Store, account: AccountId, uuid: &str) -> i64 {
    store.write(Durability::Durable, |transaction| {
        transaction.execute(
            "INSERT INTO projects(account_id,root,last_used_ms) VALUES(?1,?2,1) ON CONFLICT DO NOTHING",
            params![account.0, format!("/fixture/{}",account.0)],
        )?;
        let project: i64 = transaction.query_row("SELECT id FROM projects WHERE account_id=?1",[account.0],|row| row.get(0))?;
        transaction.execute(
            "INSERT INTO tasks(uuid,account_id,project_id,kind,state,title,title_fold,created_ms,updated_ms) \
             VALUES(?1,?2,?3,'desktop','active','fixture','fixture',1,1)",
            params![uuid,account.0,project],
        )?;
        Ok(transaction.last_insert_rowid())
    }).unwrap()
}

fn png(size: usize) -> Vec<u8> {
    let mut bytes = vec![17; size.max(8)];
    bytes[..8].copy_from_slice(b"\x89PNG\r\n\x1a\n");
    bytes
}

fn upload(bytes: &[u8]) -> ImageUpload {
    ImageUpload {
        name: "../photo.png".to_string(),
        data_url: format!("data:image/png;base64,{}", STANDARD.encode(bytes)),
    }
}

fn count(store: &Store, table: &str) -> i64 {
    assert!(["blobs", "blob_chunks", "attachments", "task_blobs"].contains(&table));
    store
        .read(|transaction| {
            Ok(
                transaction.query_row(&format!("SELECT count(*) FROM {table}"), [], |row| {
                    row.get(0)
                })?,
            )
        })
        .unwrap()
}

fn mark_deleted(store: &Store, uuid: &str) {
    store
        .write(Durability::Durable, |transaction| {
            transaction.execute("UPDATE tasks SET deleted_ms=2 WHERE uuid=?1", [uuid])?;
            Ok(())
        })
        .unwrap();
}

fn collect_task(store: &Store, account: AccountId, uuid: &str) -> usize {
    for step in 0..100 {
        let progress = store.collect_task_blobs_step(account, uuid).unwrap();
        assert!(progress.chunks <= CHUNKS_PER_WRITE);
        assert!(progress.bytes <= MAX_ATTACHMENT_RANGE_BYTES);
        assert!(progress.links <= METADATA_ROWS_PER_COLLECTION + 1);
        if progress.done {
            return step;
        }
    }
    panic!("blob collector failed to finish");
}

#[test]
fn uploads_deduplicate_without_exposing_digest_or_crossing_accounts() {
    let (_directory, store, alice, bob) = fixture();
    let bytes = png(2 * BLOB_CHUNK_BYTES + 73);
    let first = store
        .store_image_uploads(alice, "alice-task", &[upload(&bytes)])
        .unwrap()
        .remove(0);
    let second = store
        .store_image_uploads(bob, "bob-task", &[upload(&bytes)])
        .unwrap()
        .remove(0);
    assert_ne!(first.id, second.id);
    assert_eq!(first.id.len(), 32);
    assert_eq!(first.name, "photo.png");
    assert_eq!(first.source, format!("maple-attachment://{}", first.id));
    assert!(
        !first
            .source
            .contains(&format!("{:x}", Sha256::digest(&bytes)))
    );
    assert_eq!(count(&store, "blobs"), 1);
    assert_eq!(count(&store, "blob_chunks"), 3);
    assert_eq!(count(&store, "task_blobs"), 2);
    assert!(matches!(
        store.read_attachment(bob, "alice-task", &first.id, 0, 1),
        Err(StoreError::NotFound)
    ));
    assert!(matches!(
        store.read_attachment(bob, "bob-task", &first.id, 0, 1),
        Err(StoreError::NotFound)
    ));
    assert_eq!(
        store
            .read_attachment(alice, "alice-task", &first.id, 0, bytes.len())
            .unwrap()
            .data,
        bytes
    );
}

#[test]
fn ranges_cross_chunk_boundaries_and_reassemble_exact_bytes() {
    let (_directory, store, alice, _) = fixture();
    let mut bytes = png(3 * MAX_ATTACHMENT_RANGE_BYTES + 19);
    for (index, byte) in bytes.iter_mut().enumerate().skip(8) {
        *byte = (index % 251) as u8;
    }
    let image = store
        .store_image_uploads(alice, "alice-task", &[upload(&bytes)])
        .unwrap()
        .remove(0);
    let mut assembled = Vec::new();
    while assembled.len() < bytes.len() {
        let read = store
            .read_attachment(
                alice,
                "alice-task",
                &image.id,
                assembled.len() as u64,
                MAX_ATTACHMENT_RANGE_BYTES - 7,
            )
            .unwrap();
        assert_eq!(read.size, bytes.len() as u64);
        assert_eq!(read.mime, "image/png");
        assembled.extend(read.data);
    }
    assert_eq!(assembled, bytes);
    let crossing = store
        .read_attachment(
            alice,
            "alice-task",
            &image.id,
            (BLOB_CHUNK_BYTES - 3) as u64,
            11,
        )
        .unwrap();
    assert_eq!(
        crossing.data,
        bytes[BLOB_CHUNK_BYTES - 3..BLOB_CHUNK_BYTES + 8]
    );
    assert!(
        store
            .read_attachment(
                alice,
                "alice-task",
                &image.id,
                0,
                MAX_ATTACHMENT_RANGE_BYTES + 1
            )
            .is_err()
    );
    assert!(
        store
            .read_attachment(alice, "alice-task", &image.id, bytes.len() as u64 + 1, 1)
            .is_err()
    );
    assert!(
        store
            .read_attachment(alice, "alice-task", &image.id, bytes.len() as u64, 1)
            .unwrap()
            .data
            .is_empty()
    );
}

#[test]
fn upload_limits_validate_whole_batch_before_creating_any_blob() {
    let (_directory, store, alice, _) = fixture();
    let okay = upload(&png(100));
    let bad = ImageUpload {
        name: "bad.jpg".into(),
        data_url: okay.data_url.replace("image/png", "image/jpeg"),
    };
    assert!(
        store
            .store_image_uploads(alice, "alice-task", &[okay.clone(), bad])
            .is_err()
    );
    assert_eq!(count(&store, "blobs"), 0);
    assert!(
        store
            .store_image_uploads(alice, "alice-task", &vec![okay; MAX_UPLOAD_IMAGES + 1])
            .is_err()
    );
    assert!(
        store
            .store_image_uploads(
                alice,
                "alice-task",
                &[upload(&png(MAX_UPLOAD_IMAGE_BYTES + 1))]
            )
            .is_err()
    );
    assert_eq!(count(&store, "blobs"), 0);
}

#[test]
fn all_existing_upload_formats_are_preserved() {
    let (_directory, store, alice, _) = fixture();
    let jpeg = ImageUpload {
        name: "a.jpg".into(),
        data_url: format!(
            "data:image/jpg;base64,{}",
            STANDARD.encode([0xff, 0xd8, 0xff, 7])
        ),
    };
    let webp = ImageUpload {
        name: "a.webp".into(),
        data_url: format!(
            "data:image/webp;base64,{}",
            STANDARD.encode(b"RIFF\x04\0\0\0WEBP")
        ),
    };
    let result = store
        .store_image_uploads(alice, "alice-task", &[jpeg, webp])
        .unwrap();
    assert_eq!(result[0].mime_type, "image/jpeg");
    assert_eq!(result[1].mime_type, "image/webp");
}

fn tool_entry(bytes: &[u8]) -> SessionEntry {
    serde_json::from_value(serde_json::json!({
        "id":"0123abcd","parentId":null,"timestamp":1,"type":"message",
        "message":{"role":"toolResult","toolCallId":"call","toolName":"read_image",
            "content":[{"type":"image","data":STANDARD.encode(bytes),"mimeType":"image/png"}],
            "details":{"type":"image","data":"opaque-provider-payload","mimeType":"kept"},
            "isError":false,"timestamp":1}
    }))
    .unwrap()
}

#[test]
fn inline_images_over_upload_limit_round_trip_exactly_without_touching_opaque_details() {
    let (_directory, store, alice, bob) = fixture();
    let original = tool_entry(&png(20 * 1024 * 1024));
    let prepared = store
        .prepare_entry_images(alice, "alice-task", &original)
        .unwrap();
    assert!(prepared.has_images());
    assert_ne!(prepared.entry, original);
    let encoded = serde_json::to_value(&prepared.entry).unwrap();
    assert_eq!(
        encoded["message"]["details"]["data"],
        "opaque-provider-payload"
    );
    assert!(
        encoded["message"]["content"][0]["data"]
            .as_str()
            .unwrap()
            .starts_with(BLOB_MARKER)
    );
    store
        .write(Durability::Durable, |transaction| {
            let task = active_task(transaction, alice, "alice-task")?;
            prepared.link(transaction, task)
        })
        .unwrap();
    let mut stored = prepared.entry.clone();
    drop(prepared);
    assert!(
        store
            .rehydrate_entry_images(bob, "bob-task", &mut stored)
            .is_err()
    );
    store
        .rehydrate_entry_images(alice, "alice-task", &mut stored)
        .unwrap();
    assert_eq!(stored, original);
}

#[test]
fn third_party_images_escape_losslessly_without_aliasing_internal_markers() {
    let (_directory, store, alice, _) = fixture();
    let bytes = png(103);
    let canonical = STANDARD.encode(&bytes);
    // Existing unencoded inline data is still opaque to both decoding and
    // metadata projection; a flag-zero source row does not need a rewrite.
    let original_inline = tool_entry(&bytes);
    let mut unchanged_inline = original_inline.clone();
    store
        .rehydrate_entry_images(alice, "alice-task", &mut unchanged_inline)
        .unwrap();
    assert_eq!(unchanged_inline, original_inline);
    store
        .read(|transaction| {
            let task = active_task(transaction, alice, "alice-task")?;
            assert_eq!(
                entry_image_sizes_tx(
                    transaction,
                    super::super::tasks::TaskKey(task),
                    &original_inline,
                )?,
                vec![canonical.len()]
            );
            Ok(())
        })
        .unwrap();
    let non_blob_images = [
        ("not base64!".to_string(), "image/png"),
        (String::new(), "image/png"),
        (format!("data:image/png;base64,{canonical}"), "image/png"),
        (
            format!("{}\n{}\n", &canonical[..76], &canonical[76..]),
            "image/png",
        ),
        (canonical.trim_end_matches('=').to_string(), "image/png"),
        (
            STANDARD
                .encode([0xfb, 0xff])
                .replace('+', "-")
                .replace('/', "_"),
            "image/png",
        ),
        (canonical.clone(), ""),
        ("maple-blob:1".to_string(), "image/png"),
        ("maple-blob:not-an-id".to_string(), "image/png"),
        ("maple-inline-image:".to_string(), "image/png"),
        (
            "maple-inline-image:bWFwbGUtYmxvYjox".to_string(),
            "image/png",
        ),
        ("opaque image 🦊".to_string(), "image/png"),
    ];
    for (data, mime) in non_blob_images {
        for mixed in [false, true] {
            let mut original = tool_entry(&bytes);
            let EntryKind::Message {
                message: SessionMessage::Llm(Message::ToolResult(result)),
            } = &mut original.kind
            else {
                unreachable!()
            };
            result.content = vec![Content::image(data.clone(), mime)];
            if mixed {
                result.content.extend([
                    Content::image(canonical.clone(), "image/png"),
                    Content::image(canonical.clone(), "image/png"),
                ]);
            }
            let prepared = store
                .prepare_entry_images(alice, "alice-task", &original)
                .unwrap();
            assert!(prepared.has_images(), "escaped-only entries need decoding");
            assert!(prepared.has_encoded_inline_images);
            assert_eq!(prepared.blobs.len(), usize::from(mixed));
            let mut expected_sizes = vec![data.len()];
            if mixed {
                expected_sizes.extend([canonical.len(); 2]);
            }
            store
                .write(Durability::Durable, |transaction| {
                    let task = active_task(transaction, alice, "alice-task")?;
                    prepared.link(transaction, task)?;
                    assert_eq!(
                        entry_image_sizes_tx(
                            transaction,
                            super::super::tasks::TaskKey(task),
                            &prepared.entry
                        )?,
                        expected_sizes
                    );
                    Ok(())
                })
                .unwrap();
            let mut restored = prepared.entry.clone();
            drop(prepared);
            store
                .rehydrate_entry_images(alice, "alice-task", &mut restored)
                .unwrap();
            assert_eq!(restored, original);
        }
    }
    assert_eq!(count(&store, "blobs"), 1);
    assert_eq!(count(&store, "task_blobs"), 1);
}

#[test]
fn retry_batch_holds_shared_digest_once_and_projector_reads_metadata_only() {
    let (_directory, store, alice, _) = fixture();
    let bytes = png(103);
    let first = tool_entry(&bytes);
    let mut second = first.clone();
    second.id = "0123abce".to_string();
    let originals = vec![first, second];
    let prepared = store
        .prepare_entries_images(alice, "alice-task", &originals)
        .unwrap();
    assert_eq!(prepared.entries.len(), 2);
    store
        .write(Durability::Durable, |transaction| {
            let task = active_task(transaction, alice, "alice-task")?;
            for entry in &prepared.entries {
                entry.link(transaction, task)?;
            }
            Ok(())
        })
        .unwrap();
    assert_eq!(count(&store, "blobs"), 1);
    assert_eq!(count(&store, "task_blobs"), 1);
    let stored = prepared
        .entries
        .iter()
        .map(|entry| entry.entry.clone())
        .collect::<Vec<_>>();
    drop(prepared);
    for (mut entry, original) in stored.iter().cloned().zip(originals) {
        store
            .rehydrate_entry_images(alice, "alice-task", &mut entry)
            .unwrap();
        assert_eq!(entry, original);
    }
    // A projector requires metadata even if bytes have been damaged; it must
    // not silently load/decode them just to compute the image summary length.
    store
        .write(Durability::Normal, |transaction| {
            transaction.execute("DELETE FROM blob_chunks", [])?;
            let task = super::super::tasks::TaskKey(active_task(transaction, alice, "alice-task")?);
            let sizes = entry_image_sizes_tx(transaction, task, &stored[0])?;
            assert_eq!(sizes, vec![STANDARD.encode(&bytes).len()]);
            Ok(())
        })
        .unwrap();
    let mut damaged = stored[0].clone();
    assert!(
        store
            .rehydrate_entry_images(alice, "alice-task", &mut damaged)
            .is_err()
    );
    assert_eq!(damaged, stored[0]);
}

#[test]
fn shared_blob_survives_task_collection_and_final_exclusive_collection_is_bounded() {
    let (_directory, store, alice, bob) = fixture();
    let bytes = png(3 * MAX_ATTACHMENT_RANGE_BYTES + 1);
    let first = store
        .store_image_uploads(alice, "alice-task", &[upload(&bytes)])
        .unwrap()
        .remove(0);
    let second = store
        .store_image_uploads(bob, "bob-task", &[upload(&bytes)])
        .unwrap()
        .remove(0);
    mark_deleted(&store, "alice-task");
    assert!(
        store
            .read_attachment(alice, "alice-task", &first.id, 0, 1)
            .is_err()
    );
    collect_task(&store, alice, "alice-task");
    assert_eq!(count(&store, "blobs"), 1);
    assert_eq!(
        store
            .read_attachment(bob, "bob-task", &second.id, 0, 1)
            .unwrap()
            .data,
        bytes[..1]
    );
    mark_deleted(&store, "bob-task");
    assert!(collect_task(&store, bob, "bob-task") >= 5);
    assert_eq!(count(&store, "blobs"), 0);
    assert_eq!(count(&store, "blob_chunks"), 0);
    assert_eq!(count(&store, "attachments"), 0);
    assert_eq!(count(&store, "task_blobs"), 0);
}

#[test]
fn concurrent_same_digest_uploads_write_one_complete_blob() {
    let (_directory, store, alice, bob) = fixture();
    let barrier = Arc::new(Barrier::new(3));
    let mut handles = Vec::new();
    for (account, task) in [(alice, "alice-task"), (bob, "bob-task")] {
        let store = Arc::clone(&store);
        let barrier = Arc::clone(&barrier);
        handles.push(std::thread::spawn(move || {
            let image = upload(&png(2 * MAX_ATTACHMENT_RANGE_BYTES + 9));
            barrier.wait();
            store.store_image_uploads(account, task, &[image]).unwrap()
        }));
    }
    barrier.wait();
    for handle in handles {
        handle.join().unwrap();
    }
    assert_eq!(count(&store, "blobs"), 1);
    assert_eq!(count(&store, "attachments"), 2);
    assert_eq!(count(&store, "blob_chunks"), 9);
}

#[test]
fn prepare_to_link_lease_prevents_deletion_race() {
    let (_directory, store, alice, bob) = fixture();
    let bytes = png(MAX_ATTACHMENT_RANGE_BYTES + 9);
    store
        .store_image_uploads(alice, "alice-task", &[upload(&bytes)])
        .unwrap();
    let prepared = store
        .prepare_entry_images(bob, "bob-task", &tool_entry(&bytes))
        .unwrap();
    mark_deleted(&store, "alice-task");
    let (started_tx, started_rx) = mpsc::channel();
    let (done_tx, done_rx) = mpsc::channel();
    let collector = Arc::clone(&store);
    let handle = std::thread::spawn(move || {
        started_tx.send(()).unwrap();
        done_tx
            .send(collector.collect_task_blobs_step(alice, "alice-task"))
            .unwrap();
    });
    started_rx.recv().unwrap();
    assert!(matches!(
        done_rx.recv_timeout(Duration::from_millis(30)),
        Err(mpsc::RecvTimeoutError::Timeout)
    ));
    store
        .write(Durability::Durable, |transaction| {
            prepared.link(transaction, active_task(transaction, bob, "bob-task")?)
        })
        .unwrap();
    drop(prepared);
    done_rx
        .recv_timeout(Duration::from_secs(5))
        .unwrap()
        .unwrap();
    handle.join().unwrap();
    assert_eq!(count(&store, "blobs"), 1);
    let mut image = tool_entry(&bytes);
    let prepared = store.prepare_entry_images(bob, "bob-task", &image).unwrap();
    image = prepared.entry.clone();
    drop(prepared);
    store
        .rehydrate_entry_images(bob, "bob-task", &mut image)
        .unwrap();
    assert_eq!(image, tool_entry(&bytes));
}

#[test]
fn orphan_collection_resumes_after_reopen_without_touching_previous_files() {
    let (directory, store, alice, _) = fixture();
    let sentinel = directory.path().join("previous-attachments");
    std::fs::write(&sentinel, b"leave previous data untouched").unwrap();
    let bytes = png(3 * MAX_ATTACHMENT_RANGE_BYTES + 7);
    let prepared = store
        .prepare_entry_images(alice, "alice-task", &tool_entry(&bytes))
        .unwrap();
    drop(prepared); // Source transaction never happened.
    let first = store.collect_orphan_blobs_step().unwrap();
    assert_eq!(first.chunks, 4);
    assert_eq!(first.bytes, MAX_ATTACHMENT_RANGE_BYTES);
    drop(store);
    let reopened = Store::open(StorePaths::new(directory.path())).unwrap();
    for _ in 0..100 {
        let progress = reopened.collect_orphan_blobs_step().unwrap();
        assert!(progress.bytes <= MAX_ATTACHMENT_RANGE_BYTES);
        if progress.done {
            break;
        }
    }
    assert_eq!(count(&reopened, "blobs"), 0);
    assert_eq!(count(&reopened, "blob_chunks"), 0);
    assert_eq!(
        std::fs::read(sentinel).unwrap(),
        b"leave previous data untouched"
    );
}

#[test]
fn interrupted_preparation_is_rewritten_and_pending_account_deletion_never_collects() {
    let (_directory, store, alice, _) = fixture();
    let bytes = png(MAX_ATTACHMENT_RANGE_BYTES + 3);
    let digest: [u8; 32] = Sha256::digest(&bytes).into();
    let id = {
        let _lease = store.blob_locks.acquire(digest).unwrap();
        store.prepare_blob(&bytes, "image/png", digest).unwrap()
    };
    // Simulate chunks torn between transactions: the retry cannot treat them
    // as a completed deduplication hit.
    store
        .write(Durability::Normal, |transaction| {
            transaction.execute("DELETE FROM blob_chunks WHERE blob_id=?1 AND idx>0", [id])?;
            transaction.execute("UPDATE blob_chunks SET data=x'00' WHERE blob_id=?1", [id])?;
            Ok(())
        })
        .unwrap();
    let attachment = store
        .store_image_uploads(alice, "alice-task", &[upload(&bytes)])
        .unwrap()
        .remove(0);
    let read = store
        .read_attachment(
            alice,
            "alice-task",
            &attachment.id,
            0,
            MAX_ATTACHMENT_RANGE_BYTES,
        )
        .unwrap();
    assert_eq!(read.data, bytes[..MAX_ATTACHMENT_RANGE_BYTES]);
    mark_deleted(&store, "alice-task");
    store
        .write(Durability::Durable, |transaction| {
            transaction.execute(
                "UPDATE accounts SET deleting_ms=2,deletion_phase='pending_server' WHERE id=?1",
                [alice.0],
            )?;
            Ok(())
        })
        .unwrap();
    assert!(store.collect_task_blobs_step(alice, "alice-task").is_err());
    assert_eq!(count(&store, "blobs"), 1);
    assert_eq!(count(&store, "attachments"), 1);
}
