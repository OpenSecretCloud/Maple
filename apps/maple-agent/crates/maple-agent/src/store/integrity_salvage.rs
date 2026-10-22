//! Explicit recovery of completely enumerable logical source rows.
//!
//! A failed B-tree walk is not evidence of an exact loss boundary. Such damage
//! is refused here; no unknown range is silently discarded. The lifecycle
//! module remains the only owner of replacement keys, journals and renames.

use std::collections::BTreeSet;
use std::fmt;
use std::time::{Duration, Instant};

use pi_ai::{Content, Message};
use pi_coding_agent::{
    SessionMessage,
    session::{EntryKind, SessionBase, SessionEntry, SessionHeader},
};
use rusqlite::{
    Connection, OptionalExtension, Row, params, params_from_iter,
    types::{Value, ValueRef},
};
use serde::Serialize;
use sha2::{Digest, Sha256};

use super::{IntegrityCancellation, ProgressReset};
use crate::store::lifecycle::LifecycleError;
use crate::store::{Store, StoreError, StoreResult, StoreState, codec, schema};

pub const SALVAGE_CONFIRMATION: &str = "SAVE READABLE DATA";
const MAX_LOSS_ROWS: usize = 10_000;
const MAX_BAD_UNITS: usize = 10_000;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum SalvageTable {
    StoreMeta,
    Accounts,
    Credentials,
    AppSettings,
    AccountSettings,
    DictionaryWords,
    Projects,
    McpServers,
    McpServerSecrets,
    Tasks,
    Entries,
    TimelineRows,
    ToolSummaries,
    Blobs,
    BlobChunks,
    TaskBlobs,
    Attachments,
    SendReceipts,
    RemoteIdentities,
}

impl SalvageTable {
    fn name(self) -> &'static str {
        match self {
            Self::StoreMeta => "store_meta",
            Self::Accounts => "accounts",
            Self::Credentials => "credentials",
            Self::AppSettings => "app_settings",
            Self::AccountSettings => "account_settings",
            Self::DictionaryWords => "dictionary_words",
            Self::Projects => "projects",
            Self::McpServers => "mcp_servers",
            Self::McpServerSecrets => "mcp_server_secrets",
            Self::Tasks => "tasks",
            Self::Entries => "entries",
            Self::TimelineRows => "timeline_rows",
            Self::ToolSummaries => "tool_summaries",
            Self::Blobs => "blobs",
            Self::BlobChunks => "blob_chunks",
            Self::TaskBlobs => "task_blobs",
            Self::Attachments => "attachments",
            Self::SendReceipts => "send_receipts",
            Self::RemoteIdentities => "remote_identities",
        }
    }
    /// Trusted schema-owned table source. NOT INDEXED does not exclude
    /// covering secondary indexes for WITHOUT ROWID tables in SQLite.
    /// Naming their PK explicitly makes an unreadable authoritative tree fail
    /// instead of accidentally using an incomplete secondary tree.
    fn primary_source(self) -> &'static str {
        match self {
            Self::StoreMeta => "store_meta INDEXED BY sqlite_autoindex_store_meta_1",
            Self::DictionaryWords => {
                "dictionary_words INDEXED BY sqlite_autoindex_dictionary_words_1"
            }
            Self::McpServerSecrets => {
                "mcp_server_secrets INDEXED BY sqlite_autoindex_mcp_server_secrets_1"
            }
            Self::ToolSummaries => "tool_summaries INDEXED BY sqlite_autoindex_tool_summaries_1",
            Self::TaskBlobs => "task_blobs INDEXED BY sqlite_autoindex_task_blobs_1",
            Self::Attachments => "attachments INDEXED BY sqlite_autoindex_attachments_1",
            Self::Accounts => "accounts NOT INDEXED",
            Self::Credentials => "credentials NOT INDEXED",
            Self::AppSettings => "app_settings NOT INDEXED",
            Self::AccountSettings => "account_settings NOT INDEXED",
            Self::Projects => "projects NOT INDEXED",
            Self::McpServers => "mcp_servers NOT INDEXED",
            Self::Tasks => "tasks NOT INDEXED",
            Self::Entries => "entries NOT INDEXED",
            Self::TimelineRows => "timeline_rows NOT INDEXED",
            Self::Blobs => "blobs NOT INDEXED",
            Self::BlobChunks => "blob_chunks NOT INDEXED",
            Self::SendReceipts => "send_receipts NOT INDEXED",
            Self::RemoteIdentities => "remote_identities NOT INDEXED",
        }
    }
    fn key_columns(self) -> &'static [usize] {
        match self {
            Self::AccountSettings | Self::BlobChunks | Self::TaskBlobs | Self::Attachments => {
                &[0, 1]
            }
            Self::McpServerSecrets | Self::ToolSummaries => &[0, 1, 2],
            _ => &[0],
        }
    }
}

// Derived timeline rows never determine whether authoritative source is readable.
const TABLES: [SalvageTable; 18] = [
    SalvageTable::StoreMeta,
    SalvageTable::Accounts,
    SalvageTable::Credentials,
    SalvageTable::AppSettings,
    SalvageTable::AccountSettings,
    SalvageTable::DictionaryWords,
    SalvageTable::Projects,
    SalvageTable::McpServers,
    SalvageTable::McpServerSecrets,
    SalvageTable::Tasks,
    SalvageTable::Entries,
    SalvageTable::ToolSummaries,
    SalvageTable::Blobs,
    SalvageTable::BlobChunks,
    SalvageTable::TaskBlobs,
    SalvageTable::Attachments,
    SalvageTable::SendReceipts,
    SalvageTable::RemoteIdentities,
];

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum SalvageRowKey {
    Integer {
        id: i64,
    },
    IntegerPair {
        first: i64,
        second: i64,
    },
    /// Hash of the complete, type-delimited primary key. Text key material,
    /// which can contain a path, account or secret name, never enters a report.
    Fingerprint {
        sha256: String,
    },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum SalvageReason {
    UnreadableTask,
    InvalidBlob,
    OrphanedRow,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct SalvageRow {
    pub table: SalvageTable,
    pub key: SalvageRowKey,
    pub reason: SalvageReason,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct SalvageReport {
    pub source_generation: String,
    pub replacement_generation: Option<String>,
    pub retained_tasks: u64,
    pub dropped_tasks: u64,
    pub retained_rows: u64,
    pub dropped_rows: Vec<SalvageRow>,
    /// Retained nonempty tasks need lazy projection from authoritative entries.
    /// The damaged derived table is neither read nor counted.
    pub tasks_to_rebuild: u64,
}

#[derive(Clone, Debug)]
pub struct SalvagePreview {
    report: SalvageReport,
    fingerprint: [u8; 32],
}
impl SalvagePreview {
    pub fn report(&self) -> &SalvageReport {
        &self.report
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SalvageError {
    Lifecycle(LifecycleError),
    UnenumerableSource,
    ReviewTooLarge,
    StalePreview,
    Cancelled,
}
impl fmt::Display for SalvageError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Lifecycle(error)=>fmt::Display::fmt(error,f),
            Self::UnenumerableSource=>f.write_str("Maple cannot determine an exact recovery boundary for this damage. The original data is preserved."),
            Self::ReviewTooLarge=>f.write_str("The recovery loss report exceeds its review limit. The original data is preserved."),
            Self::StalePreview=>f.write_str("The local data changed after this recovery preview. Review a new preview."),
            Self::Cancelled=>f.write_str("The recovery preview was cancelled or exceeded its time budget."),
        }
    }
}
impl std::error::Error for SalvageError {}
impl From<LifecycleError> for SalvageError {
    fn from(error: LifecycleError) -> Self {
        Self::Lifecycle(error)
    }
}

struct Scan {
    preview: SalvagePreview,
    bad_tasks: BTreeSet<i64>,
    bad_blobs: BTreeSet<i64>,
}

impl Store {
    /// The host freezes admission before entering this explicit damaged-store
    /// operation. Preview is read-only and may be safely abandoned.
    pub fn preview_salvage(
        &self,
        budget: Duration,
        cancel: &IntegrityCancellation,
    ) -> Result<SalvagePreview, SalvageError> {
        if budget.is_zero() || budget > Duration::from_secs(3600) {
            return Err(LifecycleError::Store(StoreError::InvalidInput).into());
        }
        if cancel.is_cancelled() {
            return Err(SalvageError::Cancelled);
        }
        if !matches!(self.state(), StoreState::Damaged { .. }) {
            return Err(LifecycleError::Store(StoreError::Conflict).into());
        }
        let expected = self
            .lock_status()?
            .store_id
            .ok_or(SalvageError::UnenumerableSource)?;
        let start = Instant::now();
        let stopped = cancel.clone();
        let result = self.with_authenticated_store_key(|key| {
            if !matches!(self.state(), StoreState::Damaged { .. }) {
                return Err(StoreError::Conflict);
            }
            let source = super::open_readonly(self.paths(), key)?;
            source.progress_handler(
                1000,
                Some(move || stopped.is_cancelled() || start.elapsed() >= budget),
            )?;
            let _progress = ProgressReset(&source);
            source.execute_batch("BEGIN")?;
            let scanned = scan(&source);
            let _ = source.execute_batch("ROLLBACK");
            let scan = scanned?;
            if scan.preview.report.source_generation != expected {
                return Err(StoreError::InvalidData);
            }
            Ok(scan.preview)
        });
        if cancel.is_cancelled() || start.elapsed() >= budget {
            return Err(SalvageError::Cancelled);
        }
        result.map_err(map_scan_error)
    }

    /// Install exactly the reviewed logical recovery through the shared,
    /// generation-qualified replacement journal. Source rows are scanned again
    /// under one read snapshot before any replacement can be committed.
    pub fn save_readable_data(
        &self,
        preview: &SalvagePreview,
        confirmation: &str,
    ) -> Result<SalvageReport, SalvageError> {
        if confirmation != SALVAGE_CONFIRMATION {
            return Err(LifecycleError::Confirmation.into());
        }
        if self.lock_status()?.store_id.as_deref()
            != Some(preview.report.source_generation.as_str())
        {
            return Err(SalvageError::StalePreview);
        }
        let mut stale = false;
        let result = self.install_salvaged_replacement(|source, destination| {
            source.execute_batch("BEGIN")?;
            let result = (|| {
                let current = scan(source)?;
                if current.preview.fingerprint != preview.fingerprint
                    || current.preview.report != preview.report
                {
                    stale = true;
                    return Err(StoreError::Conflict);
                }
                copy_rows(source, destination, &current)?;
                Ok(current.preview.report)
            })();
            let rollback = source.execute_batch("ROLLBACK");
            rollback?;
            result
        });
        if stale {
            return Err(SalvageError::StalePreview);
        }
        let mut report = result.map_err(map_scan_error)?;
        report.replacement_generation = Some(hex(&self
            .store_id()
            .map_err(|error| SalvageError::Lifecycle(error.into()))?));
        Ok(report)
    }
}

fn map_scan_error(error: LifecycleError) -> SalvageError {
    match error {
        LifecycleError::Store(StoreError::InvalidInput) => SalvageError::ReviewTooLarge,
        LifecycleError::Store(
            StoreError::InvalidData | StoreError::Damaged { .. } | StoreError::SchemaMismatch,
        ) => SalvageError::UnenumerableSource,
        LifecycleError::Cancelled => SalvageError::Cancelled,
        other => SalvageError::Lifecycle(other),
    }
}

fn scan(source: &Connection) -> StoreResult<Scan> {
    schema::validate(source)?;
    if source.pragma_query_value::<i64, _>(None, "application_id", |row| row.get(0))?
        != schema::APPLICATION_ID
        || source.pragma_query_value::<i64, _>(None, "user_version", |row| row.get(0))?
            != schema::VERSION
        || source.query_row::<i64, _, _>(
            "SELECT value FROM store_meta WHERE key='schema_epoch'",
            [],
            |row| row.get(0),
        )? != schema::SCHEMA_EPOCH
    {
        return Err(StoreError::SchemaMismatch);
    }
    let generation: Vec<u8> = source.query_row(
        "SELECT value FROM store_meta WHERE key='store_id'",
        [],
        |row| row.get(0),
    )?;
    if generation.len() != 16 {
        return Err(StoreError::InvalidData);
    }
    let mut fingerprint = Sha256::new();
    let mut table_counts = [0_u64; TABLES.len()];
    // Scan every table before deciding that any data can be omitted. A failed
    // walk leaves unknown rows and aborts the entire operation.
    for (index, table) in TABLES.into_iter().enumerate() {
        fingerprint.update(table.name().as_bytes());
        walk(source, table, |row, columns| {
            fingerprint.update([0xff]);
            let mut bytes = 0usize;
            for column in 0..columns {
                let value = row.get_ref(column)?;
                bytes = bytes.saturating_add(value_bytes(value));
                if bytes > codec::MAX_DECODED_BYTES {
                    return Err(StoreError::InvalidData);
                }
                hash_value(&mut fingerprint, value);
            }
            table_counts[index] += 1;
            Ok(())
        })?;
    }
    validate_metadata(source)?;
    let has_deleted_tasks: bool = source.query_row(
        "SELECT EXISTS(SELECT 1 FROM tasks NOT INDEXED WHERE deleted_ms IS NOT NULL)",
        [],
        |row| row.get(0),
    )?;
    let mut bad_blobs = BTreeSet::new();
    walk(source, SalvageTable::Blobs, |row, _| {
        let id = row.get(0)?;
        // A task collector can leave an incomplete blob while deleting its
        // chunks. Preserve that hidden, resumable state rather than reporting
        // already requested deletion as new corruption loss. Any live or
        // orphan ownership still requires full payload validation.
        // Do the ownership scan only for invalid payloads, avoiding extra
        // whole-link scans for the normal complete-blob case.
        if !valid_blob(source, row)?
            && !(has_deleted_tasks && blob_has_only_deleted_owners(source, id)?)
        {
            insert_bad(&mut bad_blobs, id)?;
        }
        Ok(())
    })?;
    let mut bad_tasks = BTreeSet::new();
    walk(source, SalvageTable::Tasks, |row, _| {
        let id = row.get(0)?;
        if !valid_task(source, row)? {
            insert_bad(&mut bad_tasks, id)?;
        }
        Ok(())
    })?;
    for table in [SalvageTable::TaskBlobs, SalvageTable::Attachments] {
        walk(source, table, |row, _| {
            let task = row.get(0)?;
            let blob = row.get(if table == SalvageTable::TaskBlobs {
                1
            } else {
                2
            })?;
            let deleted = task_is_deleted(source, task)?;
            let blob_exists = exists(source, "blobs", blob)?;
            if deleted && !blob_exists {
                // Normal collection removes a final link and its blob in one
                // transaction. A broken FK is additional source corruption,
                // not proof that this hidden ownership was fully collected.
                return Err(StoreError::InvalidData);
            }
            if !deleted && (bad_blobs.contains(&blob) || !blob_exists) {
                insert_bad(&mut bad_tasks, task)?;
            }
            Ok(())
        })?;
    }
    // An orphan source row is still enumerated and explicitly reported.
    for table in [
        SalvageTable::Entries,
        SalvageTable::ToolSummaries,
        SalvageTable::TaskBlobs,
        SalvageTable::Attachments,
    ] {
        walk(source, table, |row, _| {
            let task = row.get(if table == SalvageTable::Entries { 1 } else { 0 })?;
            if !exists(source, "tasks", task)? {
                insert_bad(&mut bad_tasks, task)?;
            }
            Ok(())
        })?;
    }
    let mut report = SalvageReport {
        source_generation: hex(&generation),
        replacement_generation: None,
        retained_tasks: 0,
        dropped_tasks: 0,
        retained_rows: 0,
        dropped_rows: Vec::new(),
        tasks_to_rebuild: 0,
    };
    for table in TABLES {
        walk(source, table, |row, _| {
            if table == SalvageTable::StoreMeta {
                return Ok(());
            }
            if discard_deleted_blob_link(source, table, row, &bad_blobs)? {
                return Ok(());
            }
            if let Some(reason) = drop_reason(source, table, row, &bad_tasks, &bad_blobs)? {
                if report.dropped_rows.len() >= MAX_LOSS_ROWS {
                    return Err(StoreError::InvalidInput);
                }
                report.dropped_rows.push(SalvageRow {
                    table,
                    key: row_key(table, row)?,
                    reason,
                });
                if table == SalvageTable::Tasks {
                    report.dropped_tasks += 1;
                }
            } else {
                report.retained_rows += 1;
                if table == SalvageTable::Tasks
                    && row.get::<_, Option<i64>>("deleted_ms")?.is_none()
                {
                    report.retained_tasks += 1;
                    if row.get::<_, i64>("last_seq")? > 0 {
                        report.tasks_to_rebuild += 1;
                    }
                }
            }
            Ok(())
        })?;
    }
    for count in table_counts {
        fingerprint.update(count.to_le_bytes());
    }
    Ok(Scan {
        preview: SalvagePreview {
            report,
            fingerprint: fingerprint.finalize().into(),
        },
        bad_tasks,
        bad_blobs,
    })
}

fn validate_metadata(source: &Connection) -> StoreResult<()> {
    let checks = [
        "SELECT EXISTS(SELECT 1 FROM app_settings NOT INDEXED WHERE NOT json_valid(value))",
        "SELECT EXISTS(SELECT 1 FROM account_settings NOT INDEXED WHERE NOT json_valid(value))",
        "SELECT EXISTS(SELECT 1 FROM projects NOT INDEXED WHERE NOT json_valid(mcp_defaults))",
        "SELECT EXISTS(SELECT 1 FROM mcp_servers NOT INDEXED WHERE NOT json_valid(definition))",
        "SELECT EXISTS(SELECT 1 FROM remote_identities NOT INDEXED WHERE NOT json_valid(record) OR length(secret)!=32)",
        "SELECT EXISTS(SELECT 1 FROM mcp_server_secrets INDEXED BY sqlite_autoindex_mcp_server_secrets_1 WHERE length(target)!=32)",
        "SELECT EXISTS(SELECT 1 FROM send_receipts NOT INDEXED WHERE result IS NOT NULL AND NOT json_valid(result))",
        "SELECT EXISTS(SELECT 1 FROM store_meta WHERE key NOT IN ('store_id','created_ms','schema_epoch','last_integrity_ms','setup_operation_id','integrity_incomplete_attempts','integrity_retry_after_ms'))",
    ];
    for sql in checks {
        if source.query_row(sql, [], |row| row.get::<_, bool>(0))? {
            return Err(StoreError::InvalidData);
        }
    }
    // The all-table pragma would also read the disposable derived timeline.
    // Scope each check to owned authoritative tables, keeping every other
    // metadata FK refusal unchanged.
    for table in TABLES {
        let mut statement =
            source.prepare(&format!("PRAGMA foreign_key_check({})", table.name()))?;
        let mut rows = statement.query([])?;
        while rows.next()?.is_some() {
            if !matches!(
                table,
                SalvageTable::Tasks
                    | SalvageTable::Entries
                    | SalvageTable::ToolSummaries
                    | SalvageTable::BlobChunks
                    | SalvageTable::TaskBlobs
                    | SalvageTable::Attachments
            ) {
                return Err(StoreError::InvalidData);
            }
        }
    }
    Ok(())
}

fn valid_task(source: &Connection, row: &Row<'_>) -> StoreResult<bool> {
    // GC deletes entries in bounded batches while deliberately retaining the
    // old sequence caches. Preserve the tombstone and its remaining source so
    // normal collection can resume; never reinterpret that as visible history.
    // An account's pending_server state alone is not deletion authorization.
    if row.get::<_, Option<i64>>("deleted_ms")?.is_some() {
        let account: i64 = row.get("account_id")?;
        let project: i64 = row.get("project_id")?;
        if !exists(source, "accounts", account)?
            || !source.query_row(
                "SELECT EXISTS(SELECT 1 FROM projects NOT INDEXED WHERE id=?1 AND account_id=?2)",
                params![project, account],
                |row| row.get::<_, bool>(0),
            )?
        {
            return Err(StoreError::InvalidData);
        }
        return Ok(true);
    }
    let id: i64 = row.get(0)?;
    let uuid: String = row.get(1)?;
    let account: i64 = row.get(2)?;
    let project: i64 = row.get(3)?;
    let header: Option<String> = row.get("pi_header")?;
    let last: i64 = row.get("last_seq")?;
    let count: i64 = row.get("message_count")?;
    let start: Option<i64> = row.get("context_start_seq")?;
    let base: Option<String> = row.get("context_base")?;
    if last < 0
        || count < 0
        || start.is_some_and(|start| start < 1 || start > last.saturating_add(1))
        || base.is_some_and(|base| serde_json::from_str::<SessionBase>(&base).is_err())
    {
        return Ok(false);
    }
    if !exists(source, "accounts", account)?
        || !source.query_row(
            "SELECT EXISTS(SELECT 1 FROM projects NOT INDEXED WHERE id=?1 AND account_id=?2)",
            params![project, account],
            |row| row.get::<_, bool>(0),
        )?
    {
        return Ok(false);
    }
    let header = match header {
        Some(header) => match serde_json::from_str::<SessionHeader>(&header) {
            Ok(header) => Some(header),
            Err(_) => return Ok(false),
        },
        None => None,
    };
    if header.as_ref().is_some_and(|header| {
        header.id != uuid || header.version != pi_coding_agent::session::SESSION_VERSION
    }) {
        return Ok(false);
    }
    let mut statement=source.prepare("SELECT seq,entry_id,kind,flags,codec,bytes,payload FROM entries NOT INDEXED WHERE task_id=?1 ORDER BY id")?;
    let mut entries = statement.query([id])?;
    let mut next = 1_i64;
    while let Some(row) = entries.next()? {
        let seq: i64 = row.get(0)?;
        let entry_id: String = row.get(1)?;
        let kind: String = row.get(2)?;
        let flags: i64 = row.get(3)?;
        let encoding: i64 = row.get(4)?;
        let bytes: i64 = row.get(5)?;
        let payload: Vec<u8> = row.get(6)?;
        if seq != next || flags & !1 != 0 || bytes < 0 || payload.len() != bytes as usize {
            return Ok(false);
        }
        let decoded = match codec::decode(encoding, &payload) {
            Ok(decoded) => decoded,
            Err(_) => return Ok(false),
        };
        let entry: SessionEntry = match serde_json::from_slice(&decoded) {
            Ok(entry) => entry,
            Err(_) => return Ok(false),
        };
        if entry.id != entry_id || entry_tag(&entry) != kind {
            return Ok(false);
        }
        let duplicates: i64 = source.query_row(
            "SELECT count(*) FROM entries NOT INDEXED WHERE task_id=?1 AND entry_id=?2",
            params![id, entry.id],
            |row| row.get(0),
        )?;
        if duplicates != 1 {
            return Ok(false);
        }
        if let Some(parent) = entry.parent_id.as_ref()
            && !prior_entry(source, id, parent, seq)?
        {
            return Ok(false);
        }
        let reference = match &entry.kind {
            EntryKind::Compaction {
                first_kept_entry_id,
                ..
            } if first_kept_entry_id != &entry.id => Some(first_kept_entry_id),
            EntryKind::BranchSummary { from_id, .. } if from_id != "root" => Some(from_id),
            EntryKind::ContextEdit { target_id, .. } | EntryKind::Label { target_id, .. } => {
                Some(target_id)
            }
            _ => None,
        };
        if let Some(reference) = reference
            && !prior_entry(source, id, reference, seq)?
        {
            return Ok(false);
        }
        if !valid_images(source, id, &entry, flags)? {
            return Ok(false);
        }
        next = next.checked_add(1).ok_or(StoreError::InvalidData)?;
    }
    Ok(last == next - 1 && (last == 0 || header.is_some()))
}

fn prior_entry(source: &Connection, task: i64, entry: &str, before: i64) -> StoreResult<bool> {
    Ok(source.query_row("SELECT EXISTS(SELECT 1 FROM entries NOT INDEXED WHERE task_id=?1 AND entry_id=?2 AND seq<?3)",params![task,entry,before],|row|row.get::<_,bool>(0))?)
}

fn entry_tag(entry: &SessionEntry) -> &'static str {
    match entry.kind {
        EntryKind::Message { .. } => "message",
        EntryKind::ThinkingLevelChange { .. } => "thinking_level_change",
        EntryKind::ModelChange { .. } => "model_change",
        EntryKind::Compaction { .. } => "compaction",
        EntryKind::BranchSummary { .. } => "branch_summary",
        EntryKind::Custom { .. } => "custom",
        EntryKind::CustomMessage { .. } => "custom_message",
        EntryKind::ContextEdit { .. } => "context_edit",
        EntryKind::Label { .. } => "label",
        EntryKind::SessionInfo { .. } => "session_info",
    }
}

fn valid_images(
    source: &Connection,
    task: i64,
    entry: &SessionEntry,
    flags: i64,
) -> StoreResult<bool> {
    let content = match &entry.kind {
        EntryKind::Message {
            message: SessionMessage::Llm(Message::User(message)),
        } => Some(&message.content),
        EntryKind::Message {
            message: SessionMessage::Llm(Message::ToolResult(message)),
        } => Some(&message.content),
        EntryKind::Message {
            message: SessionMessage::Custom(message),
        } => Some(&message.content),
        EntryKind::CustomMessage { content, .. } => Some(content),
        EntryKind::ContextEdit { replacement, .. } => replacement.as_ref(),
        _ => None,
    };
    let mut moved = false;
    for block in content.into_iter().flatten() {
        if let Content::Image(image) = block
            && let Some(marker) = image.data.strip_prefix("maple-blob:")
        {
            moved = true;
            let Some(blob) = marker.parse::<i64>().ok().filter(|id| *id > 0) else {
                return Ok(false);
            };
            if !task_has_blob(source, task, blob)? {
                return Ok(false);
            }
        }
    }
    Ok(moved == (flags & 1 != 0))
}

const TASK_HAS_BLOB_SQL: &str = "SELECT EXISTS(SELECT 1 FROM task_blobs INDEXED BY sqlite_autoindex_task_blobs_1 WHERE task_id=?1 AND blob_id=?2)";

fn task_has_blob(source: &Connection, task: i64, blob: i64) -> StoreResult<bool> {
    Ok(source.query_row(TASK_HAS_BLOB_SQL, params![task, blob], |row| row.get(0))?)
}

fn valid_blob(source: &Connection, row: &Row<'_>) -> StoreResult<bool> {
    let id: i64 = row.get(0)?;
    let expected: Vec<u8> = row.get(1)?;
    let size: i64 = row.get(3)?;
    let complete: i64 = row.get(4)?;
    if size < 0 || expected.len() != 32 || complete != 1 {
        return Ok(false);
    }
    let mut hash = Sha256::new();
    let mut length = 0_i64;
    let mut next = 0_i64;
    let mut statement = source
        .prepare("SELECT idx,data FROM blob_chunks NOT INDEXED WHERE blob_id=?1 ORDER BY idx")?;
    let mut chunks = statement.query([id])?;
    while let Some(row) = chunks.next()? {
        let index: i64 = row.get(0)?;
        let data: Vec<u8> = row.get(1)?;
        let remaining = size.saturating_sub(length);
        let expected_length = remaining.min(crate::store::blobs::BLOB_CHUNK_BYTES as i64);
        if index != next || data.len() as i64 != expected_length || expected_length <= 0 {
            return Ok(false);
        }
        hash.update(&data);
        length += data.len() as i64;
        next += 1;
    }
    let actual: [u8; 32] = hash.finalize().into();
    Ok(length == size && actual.as_slice() == expected)
}

fn task_is_deleted(source: &Connection, task: i64) -> StoreResult<bool> {
    Ok(source.query_row(
        "SELECT EXISTS(SELECT 1 FROM tasks NOT INDEXED WHERE id=?1 AND deleted_ms IS NOT NULL)",
        [task],
        |row| row.get(0),
    )?)
}

fn blob_has_only_deleted_owners(source: &Connection, blob: i64) -> StoreResult<bool> {
    // Both link scans name the WITHOUT ROWID primary tree. Missing task rows
    // are not proof of deletion, and shared live ownership must stay validated.
    Ok(source.query_row(
        "SELECT (EXISTS(SELECT 1 FROM task_blobs INDEXED BY sqlite_autoindex_task_blobs_1 WHERE blob_id=?1)
             OR EXISTS(SELECT 1 FROM attachments INDEXED BY sqlite_autoindex_attachments_1 WHERE blob_id=?1))
         AND NOT EXISTS(SELECT 1 FROM task_blobs AS link INDEXED BY sqlite_autoindex_task_blobs_1
             LEFT JOIN tasks AS task NOT INDEXED ON task.id=link.task_id
             WHERE link.blob_id=?1 AND (task.id IS NULL OR task.deleted_ms IS NULL))
         AND NOT EXISTS(SELECT 1 FROM attachments AS link INDEXED BY sqlite_autoindex_attachments_1
             LEFT JOIN tasks AS task NOT INDEXED ON task.id=link.task_id
             WHERE link.blob_id=?1 AND (task.id IS NULL OR task.deleted_ms IS NULL))",
        [blob],
        |row| row.get(0),
    )?)
}

fn discard_deleted_blob_link(
    source: &Connection,
    table: SalvageTable,
    row: &Row<'_>,
    bad_blobs: &BTreeSet<i64>,
) -> StoreResult<bool> {
    let blob_column = match table {
        SalvageTable::TaskBlobs => 1,
        SalvageTable::Attachments => 2,
        _ => return Ok(false),
    };
    let blob = row.get(blob_column)?;
    // A shared corrupt blob is omitted for its live owners. Its hidden owner
    // keeps its tombstone/history, but cannot retain an FK to the omitted blob.
    // That link removal is already authorized deletion, not a new loss row.
    Ok(bad_blobs.contains(&blob) && task_is_deleted(source, row.get(0)?)?)
}

fn drop_reason(
    source: &Connection,
    table: SalvageTable,
    row: &Row<'_>,
    tasks: &BTreeSet<i64>,
    blobs: &BTreeSet<i64>,
) -> StoreResult<Option<SalvageReason>> {
    use SalvageTable::*;
    let task_column = match table {
        Tasks => Some(0),
        Entries => Some(1),
        ToolSummaries | TaskBlobs | Attachments => Some(0),
        _ => None,
    };
    if let Some(column) = task_column {
        let id = row.get(column)?;
        if tasks.contains(&id) {
            return Ok(Some(SalvageReason::UnreadableTask));
        }
    }
    if matches!(table, Blobs | BlobChunks) {
        let id = row.get(0)?;
        if blobs.contains(&id) {
            return Ok(Some(SalvageReason::InvalidBlob));
        }
        if table == BlobChunks && !exists(source, "blobs", id)? {
            return Ok(Some(SalvageReason::OrphanedRow));
        }
    }
    Ok(None)
}

fn copy_rows(source: &Connection, destination: &mut Connection, scan: &Scan) -> StoreResult<()> {
    // FK order differs from DDL order because task blob links follow blobs.
    let order = [
        SalvageTable::Accounts,
        SalvageTable::Credentials,
        SalvageTable::AppSettings,
        SalvageTable::AccountSettings,
        SalvageTable::DictionaryWords,
        SalvageTable::Projects,
        SalvageTable::McpServers,
        SalvageTable::McpServerSecrets,
        SalvageTable::Tasks,
        SalvageTable::Entries,
        SalvageTable::ToolSummaries,
        SalvageTable::Blobs,
        SalvageTable::BlobChunks,
        SalvageTable::TaskBlobs,
        SalvageTable::Attachments,
        SalvageTable::SendReceipts,
        SalvageTable::RemoteIdentities,
    ];
    let transaction = destination.transaction()?;
    for table in order {
        let mut insert = None;
        walk(source, table, |row, columns| {
            if discard_deleted_blob_link(source, table, row, &scan.bad_blobs)?
                || drop_reason(source, table, row, &scan.bad_tasks, &scan.bad_blobs)?.is_some()
            {
                return Ok(());
            }
            let mut values = (0..columns)
                .map(|column| row.get::<_, Value>(column))
                .collect::<rusqlite::Result<Vec<_>>>()?;
            if table == SalvageTable::Tasks && row.get::<_, Option<i64>>("deleted_ms")?.is_none() {
                // Canonical lazy rebuild resumes from the retained source tree.
                for name in [
                    "timeline_format",
                    "rows_epoch",
                    "last_row_seq",
                    "rebuild_item_index",
                ] {
                    values[row.as_ref().column_index(name)?] = Value::Integer(0);
                }
                for name in ["rebuild_epoch", "rebuild_seq"] {
                    values[row.as_ref().column_index(name)?] = Value::Null;
                }
            }
            let statement = insert.get_or_insert_with(|| {
                format!(
                    "INSERT INTO {} VALUES ({})",
                    table.name(),
                    vec!["?"; columns].join(",")
                )
            });
            transaction.execute(statement, params_from_iter(values))?;
            Ok(())
        })?;
    }
    let created: Option<Value> = source
        .query_row(
            "SELECT value FROM store_meta WHERE key='created_ms'",
            [],
            |row| row.get(0),
        )
        .optional()?;
    if let Some(created) = created {
        transaction.execute(
            "UPDATE store_meta SET value=?1 WHERE key='created_ms'",
            [created],
        )?;
    }
    transaction.commit()?;
    Ok(())
}

fn walk(
    source: &Connection,
    table: SalvageTable,
    mut visit: impl FnMut(&Row<'_>, usize) -> StoreResult<()>,
) -> StoreResult<()> {
    // No secondary index is trusted for enumeration. WITHOUT ROWID tables use
    // their authoritative primary B-tree; a failed walk propagates unchanged.
    let mut statement = source.prepare(&format!("SELECT * FROM {}", table.primary_source()))?;
    let columns = statement.column_count();
    let mut rows = statement.query([])?;
    while let Some(row) = rows.next()? {
        visit(row, columns)?;
    }
    Ok(())
}
fn exists(source: &Connection, table: &str, id: i64) -> StoreResult<bool> {
    Ok(source.query_row(
        &format!("SELECT EXISTS(SELECT 1 FROM {table} NOT INDEXED WHERE id=?1)"),
        [id],
        |row| row.get(0),
    )?)
}
fn insert_bad(set: &mut BTreeSet<i64>, id: i64) -> StoreResult<()> {
    if set.len() >= MAX_BAD_UNITS && !set.contains(&id) {
        return Err(StoreError::InvalidInput);
    }
    set.insert(id);
    Ok(())
}
fn row_key(table: SalvageTable, row: &Row<'_>) -> StoreResult<SalvageRowKey> {
    let keys = table.key_columns();
    if keys.len() == 1
        && let ValueRef::Integer(id) = row.get_ref(keys[0])?
    {
        return Ok(SalvageRowKey::Integer { id });
    }
    if keys.len() == 2
        && let (ValueRef::Integer(first), ValueRef::Integer(second)) =
            (row.get_ref(keys[0])?, row.get_ref(keys[1])?)
    {
        return Ok(SalvageRowKey::IntegerPair { first, second });
    }
    let mut hash = Sha256::new();
    for &column in keys {
        hash_value(&mut hash, row.get_ref(column)?);
    }
    Ok(SalvageRowKey::Fingerprint {
        sha256: hex(&hash.finalize()),
    })
}
fn value_bytes(value: ValueRef<'_>) -> usize {
    match value {
        ValueRef::Text(bytes) | ValueRef::Blob(bytes) => bytes.len(),
        _ => 8,
    }
}
fn hash_value(hash: &mut Sha256, value: ValueRef<'_>) {
    match value {
        ValueRef::Null => hash.update([0]),
        ValueRef::Integer(value) => {
            hash.update([1]);
            hash.update(value.to_le_bytes());
        }
        ValueRef::Real(value) => {
            hash.update([2]);
            hash.update(value.to_bits().to_le_bytes());
        }
        ValueRef::Text(bytes) => {
            hash.update([3]);
            hash.update((bytes.len() as u64).to_le_bytes());
            hash.update(bytes);
        }
        ValueRef::Blob(bytes) => {
            hash.update([4]);
            hash.update((bytes.len() as u64).to_le_bytes());
            hash.update(bytes);
        }
    }
}
fn hex(bytes: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    bytes
        .iter()
        .flat_map(|byte| {
            [
                HEX[(byte >> 4) as usize] as char,
                HEX[(byte & 15) as usize] as char,
            ]
        })
        .collect()
}

#[cfg(test)]
#[path = "integrity_salvage_tests.rs"]
mod tests;
