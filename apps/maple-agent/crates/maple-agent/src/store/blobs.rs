//! Chunked new-store images. Digests and database ids never cross the host API.
//!
//! A digest lease spans both chunk preparation and the transaction that links
//! the bytes. Collectors take the same lease before deciding whether a blob is
//! shared. No filesystem attachment cache or previous application data is read.

use std::collections::{BTreeMap, HashSet};
use std::sync::{Arc, Condvar, Mutex};

use base64::{Engine as _, engine::general_purpose::STANDARD};
use pi_ai::{Content, Message};
use pi_coding_agent::{
    SessionMessage,
    session::{EntryKind, SessionEntry},
};
use rusqlite::{OptionalExtension, Transaction, params};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use super::{AccountId, Durability, Store, StoreError, StoreResult};

pub const BLOB_CHUNK_BYTES: usize = 256 * 1024;
pub const MAX_ATTACHMENT_RANGE_BYTES: usize = 1024 * 1024;
pub const MAX_UPLOAD_IMAGE_BYTES: usize = 10 * 1024 * 1024;
pub const MAX_UPLOAD_IMAGES: usize = 10;
const CHUNKS_PER_WRITE: usize = MAX_ATTACHMENT_RANGE_BYTES / BLOB_CHUNK_BYTES;
const METADATA_ROWS_PER_COLLECTION: usize = 1000;
const BLOB_MARKER: &str = "maple-blob:";
// Pi deliberately retains third-party images it cannot decode. Escape those
// strings rather than requiring them to meet the attachment-upload contract.
// Every such string is wrapped, including literal values of either marker.
const INLINE_IMAGE_MARKER: &str = "maple-inline-image:";

#[derive(Default)]
pub(crate) struct BlobLocks {
    held: Mutex<HashSet<[u8; 32]>>,
    changed: Condvar,
    orphan_cursor: Mutex<i64>,
}

impl BlobLocks {
    fn acquire(self: &Arc<Self>, digest: [u8; 32]) -> StoreResult<BlobLease> {
        let mut held = self.held.lock().map_err(|_| StoreError::InvalidData)?;
        while held.contains(&digest) {
            held = self
                .changed
                .wait(held)
                .map_err(|_| StoreError::InvalidData)?;
        }
        held.insert(digest);
        Ok(BlobLease {
            locks: Arc::clone(self),
            digest,
        })
    }
}

struct BlobLease {
    locks: Arc<BlobLocks>,
    digest: [u8; 32],
}

impl Drop for BlobLease {
    fn drop(&mut self) {
        // Poisoning cannot strand other waiters during shutdown/error unwinding.
        let mut held = self
            .locks
            .held
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        held.remove(&self.digest);
        self.locks.changed.notify_all();
    }
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ImageUpload {
    pub name: String,
    pub data_url: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct StoredAttachment {
    pub id: String,
    pub name: String,
    pub mime_type: String,
    pub source: String,
}

/// Raw bounded bytes; transports choose their own base64 encoding.
#[derive(Clone, PartialEq, Eq)]
pub struct AttachmentRange {
    pub mime: String,
    pub size: u64,
    pub data: Vec<u8>,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct BlobCollectionProgress {
    pub done: bool,
    pub chunks: usize,
    pub bytes: usize,
    pub links: usize,
}

/// Keep this value alive until the source-entry transaction has committed. Its
/// typed entry has only actual Pi image blocks encoded, never opaque JSON.
pub(super) struct PreparedEntryImages {
    pub(super) entry: SessionEntry,
    account: AccountId,
    task_uuid: String,
    blobs: Vec<i64>,
    has_encoded_inline_images: bool,
    _leases: Vec<BlobLease>,
}

/// A retry/write-all batch owns the union of its digest leases exactly once.
/// Preparing individual entries independently would deadlock when they share
/// an image, because the first prepared entry deliberately retains its lease.
pub(super) struct PreparedEntriesImages {
    pub(super) entries: Vec<PreparedEntryImages>,
    _leases: Vec<BlobLease>,
}

impl PreparedEntryImages {
    /// The entry flag means image data needs decoding, including escaped inline
    /// strings which intentionally have no blob or task-blob link.
    pub(super) fn has_images(&self) -> bool {
        !self.blobs.is_empty() || self.has_encoded_inline_images
    }

    pub(super) fn link(&self, transaction: &Transaction<'_>, task_id: i64) -> StoreResult<()> {
        let actual = active_task(transaction, self.account, &self.task_uuid)?;
        if actual != task_id {
            return Err(StoreError::NotFound);
        }
        for id in &self.blobs {
            if !transaction.query_row(
                "SELECT EXISTS(SELECT 1 FROM blobs WHERE id=?1 AND complete=1)",
                [id],
                |row| row.get::<_, bool>(0),
            )? {
                return Err(StoreError::InvalidData);
            }
            transaction.execute(
                "INSERT INTO task_blobs(task_id,blob_id) VALUES(?1,?2) ON CONFLICT DO NOTHING",
                params![task_id, id],
            )?;
        }
        Ok(())
    }
}

impl Store {
    /// Validate the complete batch before writing. Public ids are random and
    /// unrelated to digest, account or task identifiers. All links commit together.
    pub fn store_image_uploads(
        &self,
        account: AccountId,
        task_uuid: &str,
        uploads: &[ImageUpload],
    ) -> StoreResult<Vec<StoredAttachment>> {
        if uploads.len() > MAX_UPLOAD_IMAGES {
            return Err(StoreError::InvalidInput);
        }
        self.read(|transaction| active_task(transaction, account, task_uuid))?;
        let parsed = uploads
            .iter()
            .map(parse_upload)
            .collect::<StoreResult<Vec<_>>>()?;
        let mut unique = BTreeMap::new();
        for image in &parsed {
            unique.entry(image.digest).or_insert(image);
        }
        // A total ordering prevents deadlocks when two batches overlap.
        let _leases = unique
            .keys()
            .map(|digest| self.blob_locks.acquire(*digest))
            .collect::<StoreResult<Vec<_>>>()?;
        let mut ids = BTreeMap::new();
        for (digest, image) in unique {
            ids.insert(digest, self.prepare_blob(&image.bytes, image.mime, digest)?);
        }
        let attachments = parsed
            .iter()
            .map(|image| {
                let id = format!("{:032x}", rand::random::<u128>());
                StoredAttachment {
                    source: format!("maple-attachment://{id}"),
                    id,
                    name: image.name.clone(),
                    mime_type: image.mime.to_string(),
                }
            })
            .collect::<Vec<_>>();
        self.write(Durability::Durable, |transaction| {
            let task = active_task(transaction, account, task_uuid)?;
            for (image, attachment) in parsed.iter().zip(&attachments) {
                let id = ids[&image.digest];
                transaction.execute("UPDATE blobs SET complete=1,mime=?2 WHERE id=?1", params![id,image.mime])?;
                transaction.execute(
                    "INSERT INTO task_blobs(task_id,blob_id) VALUES(?1,?2) ON CONFLICT DO NOTHING",
                    params![task, id],
                )?;
                transaction.execute(
                    "INSERT INTO attachments(task_id,public_id,blob_id,created_ms) VALUES(?1,?2,?3,?4)",
                    params![task, attachment.id, id, pi_ai::now_ms()],
                )?;
            }
            Ok(())
        })?;
        Ok(attachments)
    }

    /// Metadata authorization and chunk reads share one short snapshot. No caller
    /// may read a digest/id directly, even if the same bytes belong to its account.
    pub fn read_attachment(
        &self,
        account: AccountId,
        task_uuid: &str,
        public_id: &str,
        offset: u64,
        len: usize,
    ) -> StoreResult<AttachmentRange> {
        if public_id.len() != 32
            || !public_id.bytes().all(|byte| byte.is_ascii_hexdigit())
            || len > MAX_ATTACHMENT_RANGE_BYTES
        {
            return Err(StoreError::InvalidInput);
        }
        self.read(|transaction| {
            let task = active_task(transaction, account, task_uuid)?;
            let (id, mime, size): (i64, String, i64) = transaction.query_row(
                "SELECT b.id,b.mime,b.size FROM attachments a JOIN blobs b ON b.id=a.blob_id \
                 JOIN task_blobs tb ON tb.task_id=a.task_id AND tb.blob_id=b.id \
                 WHERE a.task_id=?1 AND a.public_id=?2 AND b.complete=1",
                params![task, public_id],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
            )?;
            let size = u64::try_from(size).map_err(|_| StoreError::InvalidData)?;
            let data = read_range(transaction, id, size, offset, len)?;
            Ok(AttachmentRange { mime, size, data })
        })
    }

    /// Prepare chunks without changing source data. Source insertion and `link`
    /// must be the same Durable transaction; dropping on error leaves only an
    /// unlinked blob, safely collected on next startup.
    pub(super) fn prepare_entry_images(
        &self,
        account: AccountId,
        task_uuid: &str,
        entry: &SessionEntry,
    ) -> StoreResult<PreparedEntryImages> {
        let mut prepared =
            self.prepare_entries_images(account, task_uuid, std::slice::from_ref(entry))?;
        let mut entry = prepared.entries.pop().ok_or(StoreError::InvalidData)?;
        entry._leases = prepared._leases;
        Ok(entry)
    }

    pub(super) fn prepare_entries_images(
        &self,
        account: AccountId,
        task_uuid: &str,
        entries: &[SessionEntry],
    ) -> StoreResult<PreparedEntriesImages> {
        self.read(|transaction| active_task(transaction, account, task_uuid))?;
        let mut images = BTreeMap::new();
        let mut entries = entries.to_vec();
        for entry in &mut entries {
            visit_images(entry, |image| {
                let Some(bytes) = blob_image_bytes(image) else {
                    return Ok(());
                };
                let digest: [u8; 32] = Sha256::digest(&bytes).into();
                images
                    .entry(digest)
                    .or_insert((bytes, image.mime_type.clone()));
                Ok(())
            })?;
        }
        let leases = images
            .keys()
            .map(|digest| self.blob_locks.acquire(*digest))
            .collect::<StoreResult<Vec<_>>>()?;
        let mut ids = BTreeMap::new();
        for (digest, (bytes, mime)) in &images {
            let id = self.prepare_blob(bytes, mime, *digest)?;
            // Completion is not externally observable until link; the source
            // commit's FULL sync covers these preceding chunk transactions.
            self.write(Durability::Normal, |transaction| {
                transaction.execute("UPDATE blobs SET complete=1 WHERE id=?1", [id])?;
                Ok(())
            })?;
            ids.insert(*digest, id);
        }
        let mut prepared = Vec::with_capacity(entries.len());
        for mut entry in entries {
            let mut entry_ids = HashSet::new();
            let mut has_encoded_inline_images = false;
            visit_images(&mut entry, |image| {
                let Some(bytes) = blob_image_bytes(image) else {
                    has_encoded_inline_images = true;
                    image.data = format!(
                        "{INLINE_IMAGE_MARKER}{}",
                        STANDARD.encode(image.data.as_bytes())
                    );
                    return Ok(());
                };
                let digest: [u8; 32] = Sha256::digest(&bytes).into();
                let id = ids[&digest];
                entry_ids.insert(id);
                image.data = format!("{BLOB_MARKER}{id}");
                Ok(())
            })?;
            prepared.push(PreparedEntryImages {
                entry,
                account,
                task_uuid: task_uuid.to_string(),
                blobs: entry_ids.into_iter().collect(),
                has_encoded_inline_images,
                _leases: Vec::new(),
            });
        }
        Ok(PreparedEntriesImages {
            entries: prepared,
            _leases: leases,
        })
    }

    /// Model-context loading only. Timeline pages never call this method.
    pub(super) fn rehydrate_entry_images(
        &self,
        account: AccountId,
        task_uuid: &str,
        entry: &mut SessionEntry,
    ) -> StoreResult<()> {
        // Work on a clone so a missing/corrupt later image cannot leave a partly
        // decoded entry in the caller's session.
        let mut restored = entry.clone();
        let mut cache = BTreeMap::<i64, String>::new();
        visit_images(&mut restored, |image| {
            if let Some(data) = decode_inline_image(&image.data)? {
                // Decode exactly one layer: the original is allowed to look
                // like either internal marker and must remain opaque.
                image.data = data;
                return Ok(());
            }
            let Some(marker) = image.data.strip_prefix(BLOB_MARKER) else {
                return Ok(());
            };
            let id = marker
                .parse::<i64>()
                .ok()
                .filter(|id| *id > 0)
                .ok_or(StoreError::InvalidData)?;
            if let Some(data) = cache.get(&id) {
                image.data.clone_from(data);
                return Ok(());
            }
            let size = self.read_bulk(|transaction| {
                let task = active_task(transaction, account, task_uuid)?;
                linked_blob_size(transaction, task, id)
            })?;
            let mut bytes = Vec::new();
            let mut offset = 0;
            while offset < size {
                // Each range releases its snapshot before base64 encoding
                // and the next read, allowing checkpoints/foreground writes.
                let data = self.read_bulk(|transaction| {
                    let task = active_task(transaction, account, task_uuid)?;
                    if linked_blob_size(transaction, task, id)? != size {
                        return Err(StoreError::InvalidData);
                    }
                    read_range(transaction, id, size, offset, MAX_ATTACHMENT_RANGE_BYTES)
                })?;
                offset += data.len() as u64;
                bytes.extend(data);
            }
            image.data = STANDARD.encode(bytes);
            cache.insert(id, image.data.clone());
            Ok(())
        })?;
        *entry = restored;
        Ok(())
    }

    /// One startup/idle orphan collection step. Upload and source preparation
    /// hold the same digest lease through their link commit, so the final link
    /// recheck also protects an upload which was in progress during discovery.
    /// Metadata is removed after its chunks, keeping the final cascade bounded.
    pub(crate) fn collect_orphan_blobs_step(&self) -> StoreResult<BlobCollectionProgress> {
        // Scan each linked prefix once, at most 1,000 metadata rows per read
        // snapshot. A restart simply starts from zero and safely rediscovers any
        // partially collected row. The host runs one resumable pass per open.
        let mut cursor = self
            .blob_locks
            .orphan_cursor
            .lock()
            .map_err(|_| StoreError::InvalidData)?;
        let (candidate, last, scanned) = self.read_bulk(|transaction| {
            let mut statement = transaction.prepare_cached(
                "SELECT b.id,b.sha256, \
                 NOT EXISTS(SELECT 1 FROM task_blobs tb WHERE tb.blob_id=b.id) \
                 AND NOT EXISTS(SELECT 1 FROM attachments a WHERE a.blob_id=b.id) \
                 FROM blobs b WHERE b.id>=?1 ORDER BY b.id LIMIT ?2",
            )?;
            let mut rows =
                statement.query(params![*cursor, METADATA_ROWS_PER_COLLECTION as i64])?;
            let mut last = *cursor;
            let mut scanned = 0;
            while let Some(row) = rows.next()? {
                last = row.get(0)?;
                scanned += 1;
                if row.get::<_, bool>(2)? {
                    return Ok((Some((last, row.get::<_, Vec<u8>>(1)?)), last, scanned));
                }
            }
            Ok((None, last, scanned))
        })?;
        let Some((id, digest)) = candidate else {
            let done = scanned < METADATA_ROWS_PER_COLLECTION;
            *cursor = if done { 0 } else { last.saturating_add(1) };
            return Ok(BlobCollectionProgress {
                done,
                ..Default::default()
            });
        };
        *cursor = id;
        let digest = digest.try_into().map_err(|_| StoreError::InvalidData)?;
        let _lease = self.blob_locks.acquire(digest)?;
        let mut removed = false;
        let progress = self.write_idle(|transaction| {
            // INTEGER PRIMARY KEY ids can be reused after another collector
            // removes the candidate while we wait for its digest lease.
            if !transaction.query_row(
                "SELECT EXISTS(SELECT 1 FROM blobs WHERE id=?1 AND sha256=?2)",
                params![id,digest.as_slice()],|row| row.get::<_,bool>(0),
            )? { return Ok(BlobCollectionProgress::default()); }
            let linked: bool = transaction.query_row(
                "SELECT EXISTS(SELECT 1 FROM task_blobs WHERE blob_id=?1) OR EXISTS(SELECT 1 FROM attachments WHERE blob_id=?1)",
                [id], |row| row.get(0),
            )?;
            if linked { return Ok(BlobCollectionProgress::default()); }
            transaction.execute("UPDATE blobs SET complete=0 WHERE id=?1", [id])?;
            let mut progress = delete_chunks(transaction,id)?;
            if progress.chunks == 0 {
                transaction.execute("DELETE FROM blobs WHERE id=?1", [id])?;
                removed = true;
            }
            progress.done = false;
            Ok(progress)
        })?;
        if removed {
            *cursor = id.saturating_add(1);
        }
        Ok(progress)
    }

    /// Task collector hook; the task must already be durably hidden. It also
    /// precollects attachments, preventing a large final task cascade.
    pub(super) fn collect_task_blobs_step(
        &self,
        account: AccountId,
        task_uuid: &str,
    ) -> StoreResult<BlobCollectionProgress> {
        let (task, candidate) = self.read_bulk(|transaction| {
            let task = deleting_task(transaction, account, task_uuid)?;
            let candidate = transaction
                .query_row(
                    "SELECT b.id,b.sha256 FROM blobs b WHERE b.id=coalesce( \
                 (SELECT blob_id FROM task_blobs WHERE task_id=?1 LIMIT 1), \
                 (SELECT blob_id FROM attachments WHERE task_id=?1 LIMIT 1))",
                    [task],
                    |row| Ok((row.get::<_, i64>(0)?, row.get::<_, Vec<u8>>(1)?)),
                )
                .optional()?;
            Ok((task, candidate))
        })?;
        let Some((id, digest)) = candidate else {
            return Ok(BlobCollectionProgress {
                done: true,
                ..Default::default()
            });
        };
        let digest = digest.try_into().map_err(|_| StoreError::InvalidData)?;
        let _lease = self.blob_locks.acquire(digest)?;
        self.write_idle(|transaction| {
            if deleting_task(transaction, account, task_uuid)? != task {
                return Err(StoreError::NotFound);
            }
            if !transaction.query_row(
                "SELECT EXISTS(SELECT 1 FROM blobs b WHERE b.id=?1 AND b.sha256=?2 AND \
                 (EXISTS(SELECT 1 FROM task_blobs tb WHERE tb.task_id=?3 AND tb.blob_id=b.id) \
                  OR EXISTS(SELECT 1 FROM attachments a WHERE a.task_id=?3 AND a.blob_id=b.id)))",
                params![id, digest.as_slice(), task],
                |row| row.get::<_, bool>(0),
            )? {
                return Ok(BlobCollectionProgress::default());
            }
            let links = transaction.execute(
                "DELETE FROM attachments WHERE task_id=?1 AND public_id IN \
                 (SELECT public_id FROM attachments WHERE task_id=?1 AND blob_id=?2 LIMIT ?3)",
                params![task, id, METADATA_ROWS_PER_COLLECTION as i64],
            )?;
            // Metadata batches and chunk batches are separate transactions: a
            // 1 MiB chunk delete must not also carry 1,000 attachment records.
            if links > 0 {
                return Ok(BlobCollectionProgress {
                    links,
                    ..Default::default()
                });
            }
            if transaction.query_row(
                "SELECT EXISTS(SELECT 1 FROM attachments WHERE task_id=?1 AND blob_id=?2)",
                params![task, id],
                |row| row.get::<_, bool>(0),
            )? {
                return Ok(BlobCollectionProgress {
                    links,
                    ..Default::default()
                });
            }
            let shared: bool = transaction.query_row(
                "SELECT EXISTS(SELECT 1 FROM task_blobs WHERE blob_id=?1 AND task_id<>?2) \
                 OR EXISTS(SELECT 1 FROM attachments WHERE blob_id=?1 AND task_id<>?2)",
                params![id, task],
                |row| row.get(0),
            )?;
            if shared {
                let links = links
                    + transaction.execute(
                        "DELETE FROM task_blobs WHERE task_id=?1 AND blob_id=?2",
                        params![task, id],
                    )?;
                return Ok(BlobCollectionProgress {
                    links,
                    ..Default::default()
                });
            }
            transaction.execute("UPDATE blobs SET complete=0 WHERE id=?1", [id])?;
            let mut progress = delete_chunks(transaction, id)?;
            progress.links = links;
            if progress.chunks == 0 {
                progress.links += transaction.execute(
                    "DELETE FROM task_blobs WHERE task_id=?1 AND blob_id=?2",
                    params![task, id],
                )?;
                transaction.execute("DELETE FROM blobs WHERE id=?1", [id])?;
            }
            Ok(progress)
        })
    }

    /// Caller owns the digest lease. Incomplete rows left by an interrupted
    /// preparation are rewritten in capped transactions, never trusted blindly.
    fn prepare_blob(&self, bytes: &[u8], mime: &str, digest: [u8; 32]) -> StoreResult<i64> {
        let size = i64::try_from(bytes.len()).map_err(|_| StoreError::InvalidInput)?;
        let (id,complete) = self.write(Durability::Normal,|transaction| {
            let existing = transaction.query_row("SELECT id,complete,size FROM blobs WHERE sha256=?1",[digest.as_slice()],
                |row| Ok((row.get::<_,i64>(0)?,row.get::<_,bool>(1)?,row.get::<_,i64>(2)?))).optional()?;
            if let Some((id,complete,stored_size)) = existing {
                if stored_size != size { return Err(StoreError::InvalidData); }
                if !complete && transaction.query_row(
                    "SELECT EXISTS(SELECT 1 FROM task_blobs WHERE blob_id=?1) OR EXISTS(SELECT 1 FROM attachments WHERE blob_id=?1)",
                    [id],|row| row.get::<_,bool>(0),
                )? { return Err(StoreError::Busy); }
                return Ok((id,complete));
            }
            transaction.execute("INSERT INTO blobs(sha256,mime,size,complete,created_ms) VALUES(?1,?2,?3,0,?4)",
                params![digest.as_slice(),mime,size,pi_ai::now_ms()])?;
            Ok((transaction.last_insert_rowid(),false))
        })?;
        if complete {
            return Ok(id);
        }
        loop {
            if self
                .write(Durability::Normal, |transaction| {
                    delete_chunks(transaction, id)
                })?
                .chunks
                == 0
            {
                break;
            }
        }
        for (batch, chunks) in bytes
            .chunks(BLOB_CHUNK_BYTES)
            .collect::<Vec<_>>()
            .chunks(CHUNKS_PER_WRITE)
            .enumerate()
        {
            self.write(Durability::Normal, |transaction| {
                for (index, chunk) in chunks.iter().enumerate() {
                    let index = i64::try_from(batch * CHUNKS_PER_WRITE + index)
                        .map_err(|_| StoreError::InvalidInput)?;
                    transaction.execute(
                        "INSERT INTO blob_chunks(blob_id,idx,data) VALUES(?1,?2,?3)",
                        params![id, index, *chunk],
                    )?;
                }
                Ok(())
            })?;
        }
        Ok(id)
    }
}

fn active_task(transaction: &Transaction<'_>, account: AccountId, uuid: &str) -> StoreResult<i64> {
    Ok(transaction.query_row(
        "SELECT t.id FROM tasks t JOIN accounts a ON a.id=t.account_id \
         WHERE t.uuid=?1 AND t.account_id=?2 AND t.deleted_ms IS NULL AND a.deleting_ms IS NULL",
        params![uuid, account.0],
        |row| row.get(0),
    )?)
}

fn deleting_task(
    transaction: &Transaction<'_>,
    account: AccountId,
    uuid: &str,
) -> StoreResult<i64> {
    Ok(transaction.query_row(
        "SELECT t.id FROM tasks t JOIN accounts a ON a.id=t.account_id \
         WHERE t.uuid=?1 AND t.account_id=?2 AND t.deleted_ms IS NOT NULL \
         AND (a.deletion_phase IS NULL OR a.deletion_phase='confirmed')",
        params![uuid, account.0],
        |row| row.get(0),
    )?)
}

fn linked_blob_size(transaction: &Transaction<'_>, task: i64, id: i64) -> StoreResult<u64> {
    let size: i64 = transaction.query_row(
        "SELECT b.size FROM blobs b JOIN task_blobs tb ON tb.blob_id=b.id \
         WHERE b.id=?1 AND tb.task_id=?2 AND b.complete=1",
        params![id, task],
        |row| row.get(0),
    )?;
    u64::try_from(size).map_err(|_| StoreError::InvalidData)
}

/// Metadata-only projector support, in typed image-block order. The caller
/// already authorized the TaskKey in this same transaction. No image bytes are
/// read and no base64 string is fabricated for history rendering/rebuilds.
pub(super) fn entry_image_sizes_tx(
    transaction: &Transaction<'_>,
    task: super::tasks::TaskKey,
    entry: &SessionEntry,
) -> StoreResult<Vec<usize>> {
    let mut entry = entry.clone();
    let mut sizes = Vec::new();
    visit_images(&mut entry, |image| {
        let chars = if let Some(data) = decode_inline_image(&image.data)? {
            data.len()
        } else if let Some(marker) = image.data.strip_prefix(BLOB_MARKER) {
            let id = marker
                .parse::<i64>()
                .ok()
                .filter(|id| *id > 0)
                .ok_or(StoreError::InvalidData)?;
            let size = usize::try_from(linked_blob_size(transaction, task.0, id)?)
                .map_err(|_| StoreError::InvalidData)?;
            size.div_ceil(3)
                .checked_mul(4)
                .ok_or(StoreError::InvalidData)?
        } else {
            image.data.len()
        };
        sizes.push(chars);
        Ok(())
    })?;
    Ok(sizes)
}

fn delete_chunks(transaction: &Transaction<'_>, id: i64) -> StoreResult<BlobCollectionProgress> {
    let chunks = {
        let mut statement = transaction.prepare_cached(
            "SELECT idx,length(data) FROM blob_chunks WHERE blob_id=?1 ORDER BY idx LIMIT ?2",
        )?;
        statement
            .query_map(params![id, CHUNKS_PER_WRITE as i64], |row| {
                Ok((row.get::<_, i64>(0)?, row.get::<_, i64>(1)?))
            })?
            .collect::<Result<Vec<_>, _>>()?
    };
    let mut progress = BlobCollectionProgress::default();
    for (index, size) in chunks {
        let size = usize::try_from(size).map_err(|_| StoreError::InvalidData)?;
        if size > BLOB_CHUNK_BYTES {
            return Err(StoreError::InvalidData);
        }
        transaction.execute(
            "DELETE FROM blob_chunks WHERE blob_id=?1 AND idx=?2",
            params![id, index],
        )?;
        progress.chunks += 1;
        progress.bytes += size;
    }
    Ok(progress)
}

fn read_range(
    transaction: &Transaction<'_>,
    id: i64,
    size: u64,
    offset: u64,
    len: usize,
) -> StoreResult<Vec<u8>> {
    if offset > size || len > MAX_ATTACHMENT_RANGE_BYTES {
        return Err(StoreError::InvalidInput);
    }
    let end = offset.saturating_add(len as u64).min(size);
    if offset == end {
        return Ok(Vec::new());
    }
    let first = offset / BLOB_CHUNK_BYTES as u64;
    let last = (end - 1) / BLOB_CHUNK_BYTES as u64;
    let mut statement = transaction.prepare_cached(
        "SELECT idx,data FROM blob_chunks WHERE blob_id=?1 AND idx BETWEEN ?2 AND ?3 ORDER BY idx",
    )?;
    let mut rows = statement.query(params![id, first as i64, last as i64])?;
    let mut next = first;
    let mut data = Vec::with_capacity((end - offset) as usize);
    while let Some(row) = rows.next()? {
        let index: i64 = row.get(0)?;
        let bytes: Vec<u8> = row.get(1)?;
        if index != next as i64 {
            return Err(StoreError::InvalidData);
        }
        let start = next * BLOB_CHUNK_BYTES as u64;
        let expected = (size - start).min(BLOB_CHUNK_BYTES as u64) as usize;
        if bytes.len() != expected {
            return Err(StoreError::InvalidData);
        }
        let from = offset.saturating_sub(start) as usize;
        let to = (end - start).min(bytes.len() as u64) as usize;
        data.extend_from_slice(&bytes[from..to]);
        next += 1;
    }
    if next != last + 1 || data.len() != (end - offset) as usize {
        return Err(StoreError::InvalidData);
    }
    Ok(data)
}

fn blob_image_bytes(image: &pi_ai::ImageContent) -> Option<Vec<u8>> {
    if image.mime_type.is_empty() {
        return None;
    }
    let bytes = STANDARD.decode(&image.data).ok()?;
    (!bytes.is_empty()).then_some(bytes)
}

fn decode_inline_image(data: &str) -> StoreResult<Option<String>> {
    data.strip_prefix(INLINE_IMAGE_MARKER)
        .map(|encoded| {
            let bytes = STANDARD
                .decode(encoded)
                .map_err(|_| StoreError::InvalidData)?;
            String::from_utf8(bytes).map_err(|_| StoreError::InvalidData)
        })
        .transpose()
}

fn visit_images(
    entry: &mut SessionEntry,
    mut visit: impl FnMut(&mut pi_ai::ImageContent) -> StoreResult<()>,
) -> StoreResult<()> {
    let content = match &mut entry.kind {
        EntryKind::Message {
            message: SessionMessage::Llm(Message::User(message)),
        } => Some(&mut message.content),
        EntryKind::Message {
            message: SessionMessage::Llm(Message::ToolResult(message)),
        } => Some(&mut message.content),
        EntryKind::Message {
            message: SessionMessage::Custom(message),
        } => Some(&mut message.content),
        EntryKind::CustomMessage { content, .. } => Some(content),
        EntryKind::ContextEdit { replacement, .. } => replacement.as_mut(),
        _ => None,
    };
    if let Some(content) = content {
        for block in content {
            if let Content::Image(image) = block {
                visit(image)?;
            }
        }
    }
    Ok(())
}

struct ParsedUpload {
    name: String,
    mime: &'static str,
    bytes: Vec<u8>,
    digest: [u8; 32],
}

fn parse_upload(upload: &ImageUpload) -> StoreResult<ParsedUpload> {
    let (header, data) = upload
        .data_url
        .split_once(',')
        .ok_or(StoreError::InvalidInput)?;
    let declared = header
        .strip_prefix("data:")
        .and_then(|header| header.strip_suffix(";base64"))
        .ok_or(StoreError::InvalidInput)?;
    if data.is_empty() || data.len() > MAX_UPLOAD_IMAGE_BYTES.div_ceil(3) * 4 {
        return Err(StoreError::InvalidInput);
    }
    let bytes = STANDARD
        .decode(data)
        .map_err(|_| StoreError::InvalidInput)?;
    if bytes.len() > MAX_UPLOAD_IMAGE_BYTES {
        return Err(StoreError::InvalidInput);
    }
    let mime = if bytes.starts_with(b"\x89PNG\r\n\x1a\n") {
        "image/png"
    } else if bytes.starts_with(&[0xff, 0xd8, 0xff]) {
        "image/jpeg"
    } else if bytes.len() >= 12 && bytes.starts_with(b"RIFF") && &bytes[8..12] == b"WEBP" {
        "image/webp"
    } else {
        return Err(StoreError::InvalidInput);
    };
    let declared = if declared.eq_ignore_ascii_case("image/jpg") {
        "image/jpeg"
    } else {
        declared
    };
    if declared != mime {
        return Err(StoreError::InvalidInput);
    }
    let name = upload
        .name
        .rsplit(['/', '\\'])
        .next()
        .unwrap_or_default()
        .trim()
        .chars()
        .filter(|character| !character.is_control())
        .take(255)
        .collect::<String>();
    let name = if name.is_empty() {
        "image".to_string()
    } else {
        name
    };
    let digest = Sha256::digest(&bytes).into();
    Ok(ParsedUpload {
        name,
        mime,
        bytes,
        digest,
    })
}

#[cfg(test)]
#[path = "blobtests.rs"]
mod tests;
