//! The session: an append-only tree of entries.
//!
//! Every entry points at its parent and the leaf marks the current position. Appending
//! adds a child of the leaf; moving the leaf to an earlier entry and appending starts a
//! branch, so no history is ever rewritten. The model context is the path from the root
//! to the leaf, with the latest compaction standing in for everything it summarized and
//! context edits replacing or omitting earlier entries' content.

use std::collections::HashMap;
use std::fmt;
use std::io;
use std::path::{Path, PathBuf};

use pi_agent_core::AgentMessage;
use pi_ai::transcript::current_system_message;
use pi_ai::{
    AssistantContent, Content, Message, SystemMessage, ThinkingLevel, Timestamp, Usage, now_ms,
};
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::ids::{new_entry_id, new_session_id};
use crate::messages::{
    BranchSummaryMessage, CompactionSummaryMessage, CustomMessage, SessionMessage,
};
use crate::store::{JsonlStore, MemoryStore, SessionStore};

pub const SESSION_VERSION: u32 = 1;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionHeader {
    pub version: u32,
    pub id: String,
    pub timestamp: Timestamp,
    pub cwd: String,
    /// The file of the session this one was forked from, as Pi records it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub parent_session: Option<String>,
}

/// State immediately before the first entry of a partially loaded branch.
///
/// These are facts, not session entries: they seed projection without entering the
/// persistence retry slice. A compaction in the loaded tail supplies its own complete
/// system message and replaces `system_message`, so the prompt is never replayed twice.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct SessionBase {
    pub model: Option<(String, String)>,
    pub thinking_level: ThinkingLevel,
    pub session_name: Option<String>,
    pub system_message: Option<SystemMessage>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionEntry {
    pub id: String,
    pub parent_id: Option<String>,
    pub timestamp: Timestamp,
    #[serde(flatten)]
    pub kind: EntryKind,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(
    tag = "type",
    rename_all = "snake_case",
    rename_all_fields = "camelCase"
)]
pub enum EntryKind {
    Message {
        message: SessionMessage,
    },
    ThinkingLevelChange {
        thinking_level: ThinkingLevel,
    },
    ModelChange {
        provider: String,
        model_id: String,
    },
    /// Everything before `first_kept_entry_id` is replaced by `summary` in the context.
    Compaction {
        summary: String,
        first_kept_entry_id: String,
        tokens_before: u64,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        details: Option<Value>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        usage: Option<Usage>,
        #[serde(default)]
        from_extension: bool,
        /// The complete prompt and tool state at the boundary.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        system_message: Option<SystemMessage>,
    },
    /// A summary of the branch the conversation came back from.
    BranchSummary {
        from_id: String,
        summary: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        details: Option<Value>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        usage: Option<Usage>,
        #[serde(default)]
        from_extension: bool,
    },
    /// Extension state. Never part of the model context.
    Custom {
        custom_type: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        data: Option<Value>,
    },
    /// An extension message that is part of the model context.
    CustomMessage {
        custom_type: String,
        content: Vec<Content>,
        display: bool,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        details: Option<Value>,
    },
    /// Replace an earlier entry's content in the context, or omit it with `None`.
    ContextEdit {
        target_id: String,
        replacement: Option<Vec<Content>>,
    },
    Label {
        target_id: String,
        label: Option<String>,
    },
    SessionInfo {
        name: Option<String>,
    },
}

#[derive(Debug)]
pub enum SessionError {
    NotFound(String),
    NotOnBranch(String),
    NotEditable(String),
    Io(io::Error),
}

impl fmt::Display for SessionError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NotFound(id) => write!(formatter, "Entry {id} not found"),
            Self::NotOnBranch(id) => write!(formatter, "Entry {id} is not on the active branch"),
            Self::NotEditable(id) => {
                write!(
                    formatter,
                    "Entry {id} does not contribute editable model content"
                )
            }
            Self::Io(error) => write!(formatter, "Session storage failed: {error}"),
        }
    }
}

impl std::error::Error for SessionError {}

impl From<io::Error> for SessionError {
    fn from(error: io::Error) -> Self {
        Self::Io(error)
    }
}

/// One entry's contribution to the model context.
#[derive(Clone, Debug, PartialEq)]
pub struct ProjectedEntry {
    /// The source id, or an empty string for a partial load's system-state snapshot.
    pub entry_id: String,
    pub is_compaction: bool,
    /// Empty for state-only entries and omitted ones.
    pub messages: Vec<SessionMessage>,
}

/// The model context of a branch, with where each message came from.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct SessionProjection {
    pub entries: Vec<ProjectedEntry>,
    pub messages: Vec<SessionMessage>,
    pub thinking_level: ThinkingLevel,
    /// `(provider, model id)` of the last model selection or response.
    pub model: Option<(String, String)>,
}

/// A node of [`SessionManager::tree`].
#[derive(Clone, Debug, PartialEq)]
pub struct SessionTreeNode {
    pub entry: SessionEntry,
    pub label: Option<String>,
    pub children: Vec<SessionTreeNode>,
}

/// The messages one entry contributes to the context, before edits.
pub fn entry_messages(entry: &SessionEntry) -> Vec<SessionMessage> {
    match &entry.kind {
        EntryKind::Message { message } => vec![message.clone()],
        EntryKind::CustomMessage {
            custom_type,
            content,
            display,
            details,
        } => vec![SessionMessage::Custom(CustomMessage {
            custom_type: custom_type.clone(),
            content: content.clone(),
            display: *display,
            details: details.clone(),
            timestamp: entry.timestamp,
        })],
        EntryKind::BranchSummary {
            from_id, summary, ..
        } if !summary.is_empty() => {
            vec![SessionMessage::BranchSummary(BranchSummaryMessage {
                summary: summary.clone(),
                from_id: from_id.clone(),
                timestamp: entry.timestamp,
            })]
        }
        EntryKind::Compaction {
            summary,
            tokens_before,
            system_message,
            ..
        } => {
            let mut messages: Vec<SessionMessage> = system_message
                .iter()
                .map(|system| SessionMessage::Llm(Message::System(system.clone())))
                .collect();
            messages.push(SessionMessage::CompactionSummary(
                CompactionSummaryMessage {
                    summary: summary.clone(),
                    tokens_before: *tokens_before,
                    timestamp: entry.timestamp,
                },
            ));
            messages
        }
        _ => Vec::new(),
    }
}

fn apply_edit(message: SessionMessage, replacement: &[Content]) -> SessionMessage {
    match message {
        SessionMessage::Llm(Message::User(mut user)) => {
            user.content = replacement.to_vec();
            SessionMessage::Llm(Message::User(user))
        }
        SessionMessage::Llm(Message::ToolResult(mut result)) => {
            result.content = replacement.to_vec();
            SessionMessage::Llm(Message::ToolResult(result))
        }
        SessionMessage::Llm(Message::Assistant(mut assistant)) => {
            assistant.content = replacement
                .iter()
                .filter_map(|block| block.as_text().map(AssistantContent::text))
                .collect();
            SessionMessage::Llm(Message::Assistant(assistant))
        }
        SessionMessage::Custom(mut custom) => {
            custom.content = replacement.to_vec();
            SessionMessage::Custom(custom)
        }
        other => other,
    }
}

/// The context edits among some entries, by target; a later edit of a target wins.
pub(crate) struct ContextEdits<'a> {
    edits: HashMap<&'a str, &'a Option<Vec<Content>>>,
}

impl<'a> ContextEdits<'a> {
    pub(crate) fn of(entries: &[&'a SessionEntry]) -> Self {
        let mut edits = HashMap::new();
        for entry in entries {
            if let EntryKind::ContextEdit {
                target_id,
                replacement,
            } = &entry.kind
            {
                edits.insert(target_id.as_str(), replacement);
            }
        }
        Self { edits }
    }

    /// The messages `entry` contributes once its edit is applied.
    pub(crate) fn messages(&self, entry: &SessionEntry) -> Vec<SessionMessage> {
        match self.edits.get(entry.id.as_str()) {
            Some(None) => Vec::new(),
            Some(Some(replacement)) => entry_messages(entry)
                .into_iter()
                .map(|message| apply_edit(message, replacement))
                .collect(),
            None => entry_messages(entry),
        }
    }
}

/// The entries that make up the context: with a compaction on the path, the latest one
/// comes first, then the kept entries before it and everything after it.
pub fn context_entries<'a>(path: &[&'a SessionEntry]) -> Vec<&'a SessionEntry> {
    let Some(compaction_index) = path
        .iter()
        .rposition(|entry| matches!(entry.kind, EntryKind::Compaction { .. }))
    else {
        return path.to_vec();
    };
    let compaction = path[compaction_index];
    let EntryKind::Compaction {
        first_kept_entry_id,
        ..
    } = &compaction.kind
    else {
        return path.to_vec();
    };
    let mut entries = vec![compaction];
    let mut keeping = false;
    for entry in &path[..compaction_index] {
        keeping |= entry.id == *first_kept_entry_id;
        // The compaction carries the prompt state; older system messages would repeat it.
        let is_system = matches!(&entry.kind, EntryKind::Message { message } if matches!(message.as_message(), Some(Message::System(_))));
        if keeping && !is_system {
            entries.push(*entry);
        }
    }
    entries.extend_from_slice(&path[compaction_index + 1..]);
    entries
}

/// Project a branch path into its model context.
pub fn project(path: &[&SessionEntry]) -> SessionProjection {
    project_from_base(path, None)
}

/// Project a loaded branch using the typed facts immediately before its boundary.
/// The latest compaction supplies the prompt when present; otherwise the base prompt
/// is replayed before the loaded messages. The snapshot never becomes a stored entry.
pub fn project_with_base(path: &[&SessionEntry], base: &SessionBase) -> SessionProjection {
    project_from_base(path, Some(base))
}

fn project_from_base(path: &[&SessionEntry], base: Option<&SessionBase>) -> SessionProjection {
    let mut thinking_level = base.map_or(ThinkingLevel::Off, |base| base.thinking_level);
    let mut model = base.and_then(|base| base.model.clone());
    for entry in path {
        match &entry.kind {
            EntryKind::ThinkingLevelChange {
                thinking_level: level,
            } => thinking_level = *level,
            EntryKind::ModelChange { provider, model_id } => {
                model = Some((provider.clone(), model_id.clone()));
            }
            EntryKind::Message {
                message: SessionMessage::Llm(Message::Assistant(assistant)),
            } => model = Some((assistant.provider.clone(), assistant.model.clone())),
            _ => {}
        }
    }
    let entries = context_entries(path);
    let edits = ContextEdits::of(&entries);
    let mut projected: Vec<ProjectedEntry> = entries
        .iter()
        .enumerate()
        .map(|(index, entry)| {
            let is_compaction = matches!(entry.kind, EntryKind::Compaction { .. });
            // Only the latest compaction, which leads the list, contributes its summary.
            let messages = if is_compaction && index > 0 {
                Vec::new()
            } else {
                edits.messages(entry)
            };
            ProjectedEntry {
                entry_id: entry.id.clone(),
                is_compaction,
                messages,
            }
        })
        .collect();
    if !path
        .iter()
        .any(|entry| matches!(entry.kind, EntryKind::Compaction { .. }))
        && let Some(system) = base.and_then(|base| base.system_message.as_ref())
    {
        projected.insert(
            0,
            ProjectedEntry {
                entry_id: String::new(),
                is_compaction: false,
                messages: vec![SessionMessage::Llm(Message::System(system.clone()))],
            },
        );
    }
    SessionProjection {
        messages: projected
            .iter()
            .flat_map(|entry| entry.messages.clone())
            .collect(),
        entries: projected,
        thinking_level,
        model,
    }
}

fn is_conversation(entry: &SessionEntry) -> bool {
    matches!(
        &entry.kind,
        EntryKind::Message { message } if matches!(message.as_message(), Some(Message::User(_) | Message::Assistant(_)))
    )
}

/// `ms` as Pi puts it in a file name: its UTC ISO time with `:` and `.` as `-`, such
/// as `2026-10-11T04-30-00-123Z`.
pub(crate) fn file_timestamp(ms: i64) -> String {
    let (days, rest) = (ms.div_euclid(86_400_000), ms.rem_euclid(86_400_000));
    // Days to a civil date, after Howard Hinnant's `civil_from_days`.
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let day_of_era = z.rem_euclid(146_097);
    let year_of_era =
        (day_of_era - day_of_era / 1_460 + day_of_era / 36_524 - day_of_era / 146_096) / 365;
    let day_of_year = day_of_era - (365 * year_of_era + year_of_era / 4 - year_of_era / 100);
    let shifted_month = (5 * day_of_year + 2) / 153;
    let day = day_of_year - (153 * shifted_month + 2) / 5 + 1;
    let month = if shifted_month < 10 {
        shifted_month + 3
    } else {
        shifted_month - 9
    };
    let year = year_of_era + era * 400 + i64::from(month <= 2);
    format!(
        "{year:04}-{month:02}-{day:02}T{:02}-{:02}-{:02}-{:03}Z",
        rest / 3_600_000,
        rest / 60_000 % 60,
        rest / 1_000 % 60,
        rest % 1_000
    )
}

/// The file Pi keeps the session with `header` in, in `dir`: `<time>_<id>.jsonl`.
fn session_file_in(dir: &Path, header: &SessionHeader) -> PathBuf {
    dir.join(format!(
        "{}_{}.jsonl",
        file_timestamp(header.timestamp),
        header.id
    ))
}

/// The folder Pi keeps the session files of `cwd` in: `sessions/--<cwd>--` in the agent
/// folder, the path's separators and colons as dashes, as Pi's `getDefaultSessionDir`
/// names it.
pub fn default_session_dir(cwd: &Path, agent_dir: &Path) -> PathBuf {
    let cwd = cwd.to_string_lossy();
    let cwd = cwd.strip_prefix(['/', '\\']).unwrap_or(&cwd);
    let name: String = cwd
        .chars()
        .map(|character| match character {
            '/' | '\\' | ':' => '-',
            other => other,
        })
        .collect();
    agent_dir.join("sessions").join(format!("--{name}--"))
}

/// The session tree of one conversation and where it is stored.
pub struct SessionManager {
    header: SessionHeader,
    entries: Vec<SessionEntry>,
    index: HashMap<String, usize>,
    labels: HashMap<String, String>,
    /// When each label in `labels` was set, which a fork keeps, as Pi's
    /// `labelTimestampsById`.
    label_times: HashMap<String, Timestamp>,
    leaf: Option<String>,
    base: SessionBase,
    /// The first loaded entry. A new root created with `reset_leaf` does not inherit
    /// facts from the omitted prefix of a different branch.
    base_root: Option<String>,
    store: Box<dyn SessionStore>,
    /// Whether this store has completed its first conversation write.
    flushed: bool,
    persisted_len: usize,
    needs_rewrite: bool,
    persist_error: Option<io::Error>,
}

impl SessionManager {
    /// A new, empty session.
    pub fn create(cwd: impl Into<String>, store: Box<dyn SessionStore>) -> Self {
        Self::create_with_id(new_session_id(), cwd, store)
    }

    /// A new, empty session with an id the host chose, as Pi's `create` takes an `id`
    /// option. A host that lists a session before its first write opens it again
    /// under the same id.
    pub fn create_with_id(
        id: impl Into<String>,
        cwd: impl Into<String>,
        store: Box<dyn SessionStore>,
    ) -> Self {
        let header = SessionHeader {
            version: SESSION_VERSION,
            id: id.into(),
            timestamp: now_ms(),
            cwd: cwd.into(),
            parent_session: None,
        };
        Self::from_parts(header, Vec::new(), store, false)
    }

    /// A new session id, time-ordered like the ones sessions get.
    pub fn new_id() -> String {
        new_session_id()
    }

    /// A session that is never stored.
    pub fn in_memory(cwd: impl Into<String>) -> Self {
        Self::create(cwd, Box::new(MemoryStore))
    }

    /// A new session kept in a file of its own in `dir`, named as Pi names one: the
    /// time it started and its id, `<time>_<id>.jsonl`. Like any session, the file is
    /// written once there is a conversation.
    pub fn create_in(cwd: impl Into<String>, dir: &Path) -> Self {
        let mut manager = Self::in_memory(cwd);
        manager.store = Box::new(JsonlStore::new(session_file_in(dir, &manager.header)));
        manager
    }

    /// Name `parent` as the session this one comes from, as Pi's `newSession` takes
    /// `parentSession`. It is recorded with the header, so before the first write.
    pub fn with_parent_session(mut self, parent: impl Into<String>) -> Self {
        self.header.parent_session = Some(parent.into());
        self
    }

    /// The session kept in `file`, as Pi's `SessionManager.open` opens one: read back,
    /// or a new session to be kept there when the file is missing or empty. Its folder
    /// is the one recorded, unless `cwd_override` replaces it. A file with content but
    /// no session in it is an error.
    pub fn open_file(file: &Path, cwd_override: Option<&str>) -> io::Result<Self> {
        let store = Box::new(JsonlStore::new(file));
        match JsonlStore::load(file)? {
            Some((mut header, entries)) => {
                if let Some(cwd) = cwd_override {
                    header.cwd = cwd.to_string();
                }
                Ok(Self::open(header, entries, store))
            }
            None => {
                let cwd = match cwd_override {
                    Some(cwd) => cwd.to_string(),
                    None => std::env::current_dir()?.to_string_lossy().into_owned(),
                };
                Ok(Self::create(cwd, store))
            }
        }
    }

    /// A stored session, read back. The leaf is its last entry.
    pub fn open(
        header: SessionHeader,
        entries: Vec<SessionEntry>,
        store: Box<dyn SessionStore>,
    ) -> Self {
        Self::from_parts(header, entries, store, true)
    }

    /// A stored branch loaded from a context boundary, leaving its older entries in
    /// the store. `base` records facts immediately before that boundary, and original
    /// parent ids are retained. The tail must contain all entries needed by the latest
    /// compaction and its context edits.
    ///
    /// The store must preserve the unloaded prefix when `write_all` retries this tail;
    /// a replacing store such as `JsonlStore` must instead use a complete `open` load.
    pub fn open_partial(
        header: SessionHeader,
        base: SessionBase,
        entries_from_boundary: Vec<SessionEntry>,
        store: Box<dyn SessionStore>,
    ) -> Self {
        let mut manager = Self::from_parts(header, entries_from_boundary, store, true);
        manager.base_root = manager.entries.first().map(|entry| entry.id.clone());
        manager.base = base;
        manager
    }

    fn from_parts(
        header: SessionHeader,
        entries: Vec<SessionEntry>,
        store: Box<dyn SessionStore>,
        flushed: bool,
    ) -> Self {
        let persisted_len = if flushed { entries.len() } else { 0 };
        let mut manager = Self {
            header,
            entries,
            index: HashMap::new(),
            labels: HashMap::new(),
            label_times: HashMap::new(),
            leaf: None,
            base: SessionBase::default(),
            base_root: None,
            store,
            flushed,
            persisted_len,
            needs_rewrite: !flushed,
            persist_error: None,
        };
        manager.rebuild_index();
        manager
    }

    fn rebuild_index(&mut self) {
        self.index.clear();
        self.labels.clear();
        self.label_times.clear();
        self.leaf = None;
        for position in 0..self.entries.len() {
            let entry = &self.entries[position];
            self.index.insert(entry.id.clone(), position);
            self.leaf = Some(entry.id.clone());
            if let EntryKind::Label { target_id, label } = &entry.kind {
                let (target_id, label, timestamp) =
                    (target_id.clone(), label.clone(), entry.timestamp);
                self.set_label(target_id, label, timestamp);
            }
        }
    }

    fn set_label(&mut self, target_id: String, label: Option<String>, timestamp: Timestamp) {
        match label {
            Some(label) => {
                self.label_times.insert(target_id.clone(), timestamp);
                self.labels.insert(target_id, label);
            }
            None => {
                self.label_times.remove(&target_id);
                self.labels.remove(&target_id);
            }
        }
    }

    pub fn header(&self) -> &SessionHeader {
        &self.header
    }

    pub fn id(&self) -> &str {
        &self.header.id
    }

    pub fn cwd(&self) -> &str {
        &self.header.cwd
    }

    /// Write the current branch to `path` as a session file of its own, as Pi's
    /// `exportSessionToJsonl` does: a header with the session's id and folder and the
    /// time now, then the branch's entries, each the parent of the next.
    pub fn export_branch(&self, path: &Path) -> io::Result<()> {
        self.require_complete_history()?;
        let header = SessionHeader {
            version: SESSION_VERSION,
            id: self.header.id.clone(),
            timestamp: now_ms(),
            cwd: self.header.cwd.clone(),
            parent_session: None,
        };
        let mut parent_id = None;
        let entries: Vec<SessionEntry> = self
            .branch()
            .into_iter()
            .map(|entry| SessionEntry {
                parent_id: parent_id.replace(entry.id.clone()),
                ..entry.clone()
            })
            .collect();
        JsonlStore::new(path).write_all(&header, &entries)
    }

    /// The file the session is kept in, when its store keeps one.
    pub fn session_file(&self) -> Option<&Path> {
        self.store.file()
    }

    pub fn entries(&self) -> &[SessionEntry] {
        &self.entries
    }

    pub fn entry(&self, id: &str) -> Option<&SessionEntry> {
        self.index.get(id).map(|position| &self.entries[*position])
    }

    pub fn leaf_id(&self) -> Option<&str> {
        self.leaf.as_deref()
    }

    pub fn label(&self, id: &str) -> Option<&str> {
        self.labels.get(id).map(String::as_str)
    }

    pub fn children(&self, id: &str) -> Vec<&SessionEntry> {
        self.entries
            .iter()
            .filter(|entry| entry.parent_id.as_deref() == Some(id))
            .collect()
    }

    /// The latest session name, if one was set.
    pub fn session_name(&self) -> Option<&str> {
        self.entries
            .iter()
            .rev()
            .find_map(|entry| match &entry.kind {
                EntryKind::SessionInfo { name } => {
                    Some(name.as_deref().filter(|name| !name.trim().is_empty()))
                }
                _ => None,
            })
            .unwrap_or(self.base.session_name.as_deref())
    }

    /// A storage failure since the last call. The tree in memory stays correct either way.
    pub fn take_persist_error(&mut self) -> Option<io::Error> {
        self.persist_error.take()
    }

    /// Whether entries in memory still need a successful store write. Consuming a
    /// persistence error does not clear this state. State-only entries before the
    /// first conversation are deliberately provisional and do not block eviction.
    pub fn has_unpersisted(&self) -> bool {
        self.persisted_len < self.entries.len()
            && (self.flushed || self.entries.iter().any(is_conversation))
    }

    /// Retry pending writes without adding an entry. A failure leaves the same retry
    /// slice available and never makes the session safe to evict. The lazy first-write
    /// rule still applies: a state-only session is not written before a conversation.
    pub fn flush(&mut self) -> io::Result<()> {
        if !self.has_unpersisted() {
            return Ok(());
        }
        if !self.flushed && !self.entries.iter().any(is_conversation) {
            return Ok(());
        }
        let result = if self.needs_rewrite || self.entries.len() != self.persisted_len + 1 {
            self.store.write_all_with_base(
                &self.header,
                self.base_root.as_ref().map(|_| &self.base),
                &self.entries,
            )
        } else {
            let entry = self.entries.last().expect("pending entry");
            let context_base = if let EntryKind::Compaction {
                first_kept_entry_id,
                ..
            } = &entry.kind
            {
                // A missing kept id means the compaction keeps no older entries, as
                // context_entries also interprets it. The boundary is the compaction.
                Some(
                    self.base_before(first_kept_entry_id)
                        .or_else(|_| self.base_before(&entry.id))
                        .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))?,
                )
            } else {
                None
            };
            self.store.append_with_context(entry, context_base.as_ref())
        };
        match result {
            Ok(()) => {
                self.flushed = true;
                self.persisted_len = self.entries.len();
                self.needs_rewrite = false;
                Ok(())
            }
            Err(error) => {
                self.needs_rewrite = true;
                Err(error)
            }
        }
    }

    fn persist(&mut self) {
        if let Err(error) = self.flush() {
            self.persist_error = Some(error);
        }
    }

    fn fresh_id(&self) -> String {
        new_entry_id(|candidate| {
            self.index.contains_key(candidate)
                || self.store.contains_entry(candidate).unwrap_or(false)
        })
    }

    fn append(&mut self, kind: EntryKind) -> String {
        let id = self.fresh_id();
        self.append_with_id(id, kind)
    }

    fn append_with_id(&mut self, id: String, kind: EntryKind) -> String {
        let timestamp = now_ms();
        if let EntryKind::Label { target_id, label } = &kind {
            self.set_label(target_id.clone(), label.clone(), timestamp);
        }
        self.entries.push(SessionEntry {
            id: id.clone(),
            parent_id: self.leaf.clone(),
            timestamp,
            kind,
        });
        self.index.insert(id.clone(), self.entries.len() - 1);
        self.leaf = Some(id.clone());
        self.persist();
        id
    }

    pub fn append_message(&mut self, message: SessionMessage) -> String {
        self.append(EntryKind::Message { message })
    }

    pub fn append_thinking_level_change(&mut self, thinking_level: ThinkingLevel) -> String {
        self.append(EntryKind::ThinkingLevelChange { thinking_level })
    }

    pub fn append_model_change(&mut self, provider: &str, model_id: &str) -> String {
        self.append(EntryKind::ModelChange {
            provider: provider.into(),
            model_id: model_id.into(),
        })
    }

    /// Record a compaction. `first_kept_entry_id: None` keeps nothing before it. The
    /// entry records the current prompt and tool state, which the summary replaces.
    pub fn append_compaction(
        &mut self,
        summary: String,
        first_kept_entry_id: Option<String>,
        tokens_before: u64,
        details: Option<Value>,
        usage: Option<Usage>,
        from_extension: bool,
    ) -> String {
        let projection = self.projection();
        let messages: Vec<&Message> = projection
            .messages
            .iter()
            .filter_map(AgentMessage::as_message)
            .collect();
        let system_message = current_system_message(messages.iter().copied()).map(|mut system| {
            system.timestamp = now_ms();
            system
        });
        let id = self.fresh_id();
        let kind = EntryKind::Compaction {
            summary,
            first_kept_entry_id: first_kept_entry_id.unwrap_or_else(|| id.clone()),
            tokens_before,
            details,
            usage,
            from_extension,
            system_message,
        };
        self.append_with_id(id, kind)
    }

    pub fn append_custom_entry(&mut self, custom_type: &str, data: Option<Value>) -> String {
        self.append(EntryKind::Custom {
            custom_type: custom_type.into(),
            data,
        })
    }

    pub fn append_custom_message(
        &mut self,
        custom_type: &str,
        content: Vec<Content>,
        display: bool,
        details: Option<Value>,
    ) -> String {
        self.append(EntryKind::CustomMessage {
            custom_type: custom_type.into(),
            content,
            display,
            details,
        })
    }

    /// Replace an entry's content in the context from here on, or omit it with `None`.
    pub fn append_context_edit(
        &mut self,
        target_id: &str,
        replacement: Option<Vec<Content>>,
    ) -> Result<String, SessionError> {
        let target = self
            .entry(target_id)
            .ok_or_else(|| SessionError::NotFound(target_id.into()))?;
        let editable = match &target.kind {
            EntryKind::CustomMessage { .. } => true,
            EntryKind::Message { message } => matches!(
                message.as_message(),
                Some(Message::User(_) | Message::Assistant(_) | Message::ToolResult(_))
            ),
            _ => false,
        };
        if !editable {
            return Err(SessionError::NotEditable(target_id.into()));
        }
        if !self.branch().iter().any(|entry| entry.id == target_id) {
            return Err(SessionError::NotOnBranch(target_id.into()));
        }
        Ok(self.append(EntryKind::ContextEdit {
            target_id: target_id.into(),
            replacement,
        }))
    }

    pub fn append_label(
        &mut self,
        target_id: &str,
        label: Option<String>,
    ) -> Result<String, SessionError> {
        if !self.index.contains_key(target_id) {
            return Err(SessionError::NotFound(target_id.into()));
        }
        Ok(self.append(EntryKind::Label {
            target_id: target_id.into(),
            label,
        }))
    }

    pub fn append_session_info(&mut self, name: Option<String>) -> String {
        self.append(EntryKind::SessionInfo { name })
    }

    /// The path from the root to the leaf.
    pub fn branch(&self) -> Vec<&SessionEntry> {
        match &self.leaf {
            Some(leaf) => self.branch_to(leaf),
            None => Vec::new(),
        }
    }

    /// The path from the root to `id`.
    pub fn branch_to(&self, id: &str) -> Vec<&SessionEntry> {
        let mut path = Vec::new();
        let mut current = self.entry(id);
        while let Some(entry) = current {
            path.push(entry);
            current = entry
                .parent_id
                .as_deref()
                .and_then(|parent| self.entry(parent));
        }
        path.reverse();
        path
    }

    /// Move the leaf to `id`; the next entry starts a branch there.
    pub fn set_leaf(&mut self, id: &str) -> Result<(), SessionError> {
        if !self.index.contains_key(id) {
            return Err(SessionError::NotFound(id.into()));
        }
        self.leaf = Some(id.into());
        Ok(())
    }

    /// Move the leaf before the first entry; the next entry is a new root.
    pub fn reset_leaf(&mut self) {
        self.leaf = None;
    }

    /// Move the leaf to `from` and record a summary of the branch being left.
    pub fn branch_with_summary(
        &mut self,
        from: Option<&str>,
        summary: String,
        details: Option<Value>,
        usage: Option<Usage>,
        from_extension: bool,
    ) -> Result<String, SessionError> {
        if let Some(from) = from
            && !self.index.contains_key(from)
        {
            return Err(SessionError::NotFound(from.into()));
        }
        let from_id = self.leaf.clone().unwrap_or_else(|| "root".into());
        self.leaf = from.map(str::to_string);
        Ok(self.append(EntryKind::BranchSummary {
            from_id,
            summary,
            details,
            usage,
            from_extension,
        }))
    }

    pub fn projection(&self) -> SessionProjection {
        let branch = self.branch();
        project_from_base(&branch, self.base_for_branch(&branch))
    }

    /// Whether the active branch inherits resolved facts from an unloaded prefix.
    pub fn has_context_base(&self) -> bool {
        self.base_for_branch(&self.branch()).is_some()
    }

    fn base_for_branch(&self, branch: &[&SessionEntry]) -> Option<&SessionBase> {
        self.base_root.as_deref().and_then(|root| {
            branch
                .first()
                .filter(|entry| entry.id == root)
                .map(|_| &self.base)
        })
    }

    /// Snapshot the typed facts immediately before `entry_id`, without manufacturing
    /// entries. Hosts persist this with the compaction boundary, in the same transaction.
    /// The id must be part of the loaded active branch.
    pub fn base_before(&self, entry_id: &str) -> Result<SessionBase, SessionError> {
        if !self.index.contains_key(entry_id) {
            return Err(SessionError::NotFound(entry_id.into()));
        }
        let branch = self.branch();
        let position = branch
            .iter()
            .position(|entry| entry.id == entry_id)
            .ok_or_else(|| SessionError::NotOnBranch(entry_id.into()))?;
        let inherited = self.base_for_branch(&branch);
        let prefix = &branch[..position];
        let projection = project_from_base(prefix, inherited);
        // Session names, unlike model context, follow append order across branches.
        let before = &self.entries[..self.index[entry_id]];
        let session_name = before
            .iter()
            .rev()
            .find_map(|entry| match &entry.kind {
                EntryKind::SessionInfo { name } => {
                    Some(name.clone().filter(|name| !name.trim().is_empty()))
                }
                _ => None,
            })
            .unwrap_or_else(|| self.base.session_name.clone());
        let system_message = current_system_message(
            projection
                .messages
                .iter()
                .filter_map(AgentMessage::as_message),
        );
        Ok(SessionBase {
            model: projection.model,
            thinking_level: projection.thinking_level,
            session_name,
            system_message,
        })
    }

    /// The whole tree, children ordered oldest first.
    pub fn tree(&self) -> Vec<SessionTreeNode> {
        let mut children: HashMap<Option<&str>, Vec<&SessionEntry>> = HashMap::new();
        for entry in &self.entries {
            let parent = entry
                .parent_id
                .as_deref()
                .filter(|parent| self.index.contains_key(*parent) && *parent != entry.id);
            children.entry(parent).or_default().push(entry);
        }
        fn build(
            manager: &SessionManager,
            children: &HashMap<Option<&str>, Vec<&SessionEntry>>,
            parent: Option<&str>,
        ) -> Vec<SessionTreeNode> {
            let mut nodes: Vec<&SessionEntry> = children.get(&parent).cloned().unwrap_or_default();
            nodes.sort_by_key(|entry| entry.timestamp);
            nodes
                .into_iter()
                .map(|entry| SessionTreeNode {
                    entry: entry.clone(),
                    label: manager.labels.get(&entry.id).cloned(),
                    children: build(manager, children, Some(&entry.id)),
                })
                .collect()
        }
        build(self, &children, None)
    }

    /// Turn this into a new session holding only the path to `leaf_id`, or nothing for
    /// `None`, stored in `store`, as Pi's `createBranchedSession`. Labels on the path are
    /// carried over with the time they were set. The new session names this one's file
    /// as its parent, as Pi does, when this one was kept in a file. A failed write is
    /// reported by [`Self::take_persist_error`], like any other.
    pub fn fork(
        &mut self,
        leaf_id: Option<&str>,
        store: Box<dyn SessionStore>,
    ) -> Result<(), SessionError> {
        self.fork_with(leaf_id, self.parent_name(), |_| store)
    }

    /// How a fork names this session as its parent: by its file, as Pi does.
    fn parent_name(&self) -> Option<String> {
        self.session_file()
            .map(|file| file.to_string_lossy().into_owned())
    }

    /// The current branch alone, in memory: where drafts are tried before they are
    /// added, as Pi previews a boundary's entries.
    pub(crate) fn branch_copy(&self) -> SessionManager {
        let branch = self.branch();
        let mut copy = Self::from_parts(
            self.header.clone(),
            branch.iter().map(|entry| (*entry).clone()).collect(),
            Box::new(MemoryStore),
            true,
        );
        if let Some(base) = self.base_for_branch(&branch) {
            copy.base = base.clone();
            copy.base_root = copy.entries.first().map(|entry| entry.id.clone());
        }
        copy
    }

    /// [`Self::fork`] into a file of its own in `dir`, named as [`Self::create_in`]
    /// names one, as Pi's `createBranchedSession` keeps a fork in the session's folder.
    pub fn fork_in(&mut self, leaf_id: Option<&str>, dir: &Path) -> Result<(), SessionError> {
        self.fork_with(leaf_id, self.parent_name(), |header| {
            Box::new(JsonlStore::new(session_file_in(dir, header)))
        })
    }

    /// A new session holding only the path to `leaf_id`, as [`Self::fork`] makes one,
    /// leaving this one as it is. It is kept in `dir`, or in memory for `None`.
    pub fn branched(
        &self,
        leaf_id: Option<&str>,
        dir: Option<&Path>,
    ) -> Result<SessionManager, SessionError> {
        self.require_complete_history()?;
        let mut copy = Self::from_parts(
            self.header.clone(),
            self.entries.clone(),
            Box::new(MemoryStore),
            true,
        );
        copy.fork_with(leaf_id, self.parent_name(), |header| match dir {
            Some(dir) => Box::new(JsonlStore::new(session_file_in(dir, header))),
            None => Box::new(MemoryStore),
        })?;
        Ok(copy)
    }

    fn fork_with(
        &mut self,
        leaf_id: Option<&str>,
        parent_session: Option<String>,
        store_for: impl FnOnce(&SessionHeader) -> Box<dyn SessionStore>,
    ) -> Result<(), SessionError> {
        self.require_complete_history()?;
        if let Some(leaf_id) = leaf_id
            && !self.index.contains_key(leaf_id)
        {
            return Err(SessionError::NotFound(leaf_id.into()));
        }
        let path = leaf_id.map_or_else(Vec::new, |leaf_id| self.branch_to(leaf_id));
        let mut entries: Vec<SessionEntry> = Vec::new();
        // Label entries are rewritten at the end. A compaction that keeps entries from a
        // label on keeps them from the next entry instead.
        let mut dropped_labels: Vec<String> = Vec::new();
        let mut kept_from: HashMap<String, String> = HashMap::new();
        for entry in path {
            if matches!(entry.kind, EntryKind::Label { .. }) {
                dropped_labels.push(entry.id.clone());
                continue;
            }
            for label in dropped_labels.drain(..) {
                kept_from.insert(label, entry.id.clone());
            }
            let mut entry = entry.clone();
            if let EntryKind::Compaction {
                first_kept_entry_id,
                ..
            } = &mut entry.kind
                && let Some(next) = kept_from.get(first_kept_entry_id)
            {
                *first_kept_entry_id = next.clone();
            }
            entry.parent_id = entries.last().map(|previous| previous.id.clone());
            entries.push(entry);
        }
        let mut parent = entries.last().map(|entry| entry.id.clone());
        let mut labels: Vec<(&String, &String)> = self
            .labels
            .iter()
            .filter(|(target, _)| entries.iter().any(|entry| entry.id == **target))
            .collect();
        labels.sort();
        let label_entries: Vec<SessionEntry> = labels
            .into_iter()
            .map(|(target, label)| {
                let id =
                    new_entry_id(|candidate| entries.iter().any(|entry| entry.id == candidate));
                SessionEntry {
                    id: id.clone(),
                    parent_id: parent.replace(id),
                    timestamp: self.label_times.get(target).copied().unwrap_or_else(now_ms),
                    kind: EntryKind::Label {
                        target_id: target.clone(),
                        label: Some(label.clone()),
                    },
                }
            })
            .collect();
        entries.extend(label_entries);
        self.header = SessionHeader {
            version: SESSION_VERSION,
            id: new_session_id(),
            timestamp: now_ms(),
            cwd: self.header.cwd.clone(),
            parent_session,
        };
        self.entries = entries;
        self.store = store_for(&self.header);
        self.flushed = false;
        self.persisted_len = 0;
        self.needs_rewrite = true;
        self.base = SessionBase::default();
        self.base_root = None;
        self.rebuild_index();
        self.persist();
        Ok(())
    }

    fn require_complete_history(&self) -> io::Result<()> {
        if self.base_root.is_some() {
            return Err(io::Error::new(
                io::ErrorKind::Unsupported,
                "This operation requires a complete session load; reload the full history first",
            ));
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::store::JsonlStore;
    use pi_ai::{AssistantMessage, StopReason, UserMessage};
    use serde_json::json;
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::{Arc, Mutex};

    fn user(text: &str) -> SessionMessage {
        SessionMessage::Llm(Message::User(UserMessage::text(text)))
    }

    fn assistant(text: &str) -> SessionMessage {
        let model = pi_ai::faux::FauxProvider::default_model();
        SessionMessage::Llm(Message::Assistant(AssistantMessage {
            content: vec![AssistantContent::text(text)],
            stop_reason: StopReason::Stop,
            ..AssistantMessage::empty(&model)
        }))
    }

    fn texts(manager: &SessionManager) -> Vec<String> {
        manager
            .projection()
            .messages
            .iter()
            .map(SessionMessage::text)
            .collect()
    }

    #[test]
    fn entries_chain_from_the_leaf_and_build_the_context() {
        let mut session = SessionManager::in_memory("/work");
        let first = session.append_message(user("hello"));
        session.append_thinking_level_change(ThinkingLevel::High);
        let reply = session.append_message(assistant("hi"));
        let path: Vec<String> = session
            .branch()
            .iter()
            .map(|entry| entry.id.clone())
            .collect();
        assert_eq!(path.len(), 3);
        assert_eq!(
            (path[0].as_str(), path[2].as_str()),
            (first.as_str(), reply.as_str())
        );

        let projection = session.projection();
        assert_eq!(texts(&session), ["hello", "hi"]);
        assert_eq!(projection.thinking_level, ThinkingLevel::High);
        assert_eq!(projection.model, Some(("faux".into(), "faux-1".into())));
    }

    #[test]
    fn branching_keeps_the_old_path_and_starts_a_new_one() {
        let mut session = SessionManager::in_memory("/work");
        let question = session.append_message(user("question"));
        let first_answer = session.append_message(assistant("first answer"));
        session.set_leaf(&question).unwrap();
        session.append_message(assistant("second answer"));

        assert_eq!(texts(&session), ["question", "second answer"]);
        assert_eq!(session.children(&question).len(), 2);
        assert!(session.entry(&first_answer).is_some());
        let tree = session.tree();
        assert_eq!(tree.len(), 1);
        assert_eq!(tree[0].children.len(), 2);

        let summary = session
            .branch_with_summary(
                Some(&question),
                "tried the second answer".into(),
                None,
                None,
                false,
            )
            .unwrap();
        assert_eq!(texts(&session)[1], "tried the second answer");
        assert_eq!(session.leaf_id(), Some(summary.as_str()));
    }

    #[test]
    fn compaction_replaces_what_it_summarized() {
        let mut session = SessionManager::in_memory("/work");
        session.append_message(user("old question"));
        session.append_message(assistant("old answer"));
        let kept = session.append_message(user("recent question"));
        session.append_message(assistant("recent answer"));
        session.append_compaction("summary of old".into(), Some(kept), 1234, None, None, false);
        session.append_message(user("next"));

        let messages = session.projection().messages;
        assert_eq!(messages[0].role(), "compactionSummary");
        let texts: Vec<String> = messages.iter().map(SessionMessage::text).collect();
        assert_eq!(
            texts,
            ["summary of old", "recent question", "recent answer", "next"]
        );
    }

    #[test]
    fn context_edits_replace_or_omit_content_on_the_branch() {
        let mut session = SessionManager::in_memory("/work");
        let question = session.append_message(user("my password is hunter2"));
        let answer = session.append_message(assistant("noted"));
        session
            .append_context_edit(
                &question,
                Some(vec![Content::text("my password is [redacted]")]),
            )
            .unwrap();
        session.append_context_edit(&answer, None).unwrap();
        assert_eq!(texts(&session), ["my password is [redacted]"]);

        let info = session.append_session_info(Some("named".into()));
        assert!(matches!(
            session.append_context_edit(&info, None),
            Err(SessionError::NotEditable(_))
        ));
        assert!(matches!(
            session.append_context_edit("nope", None),
            Err(SessionError::NotFound(_))
        ));
    }

    #[test]
    fn labels_and_names_resolve_to_the_latest_value() {
        let mut session = SessionManager::in_memory("/work");
        let question = session.append_message(user("q"));
        session
            .append_label(&question, Some("start".into()))
            .unwrap();
        assert_eq!(session.label(&question), Some("start"));
        session.append_label(&question, None).unwrap();
        assert_eq!(session.label(&question), None);
        session.append_session_info(Some("first".into()));
        session.append_session_info(Some("second".into()));
        assert_eq!(session.session_name(), Some("second"));
    }

    #[test]
    fn a_file_is_written_only_once_there_is_a_conversation() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("session.jsonl");
        let mut session = SessionManager::create("/work", Box::new(JsonlStore::new(&path)));
        session.append_model_change("faux", "faux-1");
        assert!(!path.exists());
        session.append_message(user("hello"));
        session.append_custom_entry("ext", Some(json!({ "state": 1 })));
        assert!(path.exists());

        let (header, entries) = JsonlStore::load(&path).unwrap().unwrap();
        assert_eq!(header.id, session.id());
        assert_eq!(entries, session.entries());
        let reopened = SessionManager::open(header, entries, Box::new(JsonlStore::new(&path)));
        assert_eq!(reopened.leaf_id(), session.leaf_id());
        assert_eq!(texts(&reopened), ["hello"]);
    }

    #[test]
    fn loading_rejects_files_that_are_not_sessions_and_skips_bad_lines() {
        let dir = tempfile::tempdir().unwrap();
        let foreign = dir.path().join("notes.jsonl");
        std::fs::write(&foreign, "{\"hello\":1}\n").unwrap();
        assert!(JsonlStore::load(&foreign).is_err());
        assert!(
            JsonlStore::load(&dir.path().join("missing.jsonl"))
                .unwrap()
                .is_none()
        );

        let path = dir.path().join("s.jsonl");
        let mut session = SessionManager::create("/work", Box::new(JsonlStore::new(&path)));
        session.append_message(user("hello"));
        let mut text = std::fs::read_to_string(&path).unwrap();
        text.push_str("{not json\n");
        std::fs::write(&path, text).unwrap();
        let (_, entries) = JsonlStore::load(&path).unwrap().unwrap();
        assert_eq!(entries.len(), 1);
    }

    #[test]
    fn file_names_carry_pis_timestamps() {
        assert_eq!(file_timestamp(0), "1970-01-01T00-00-00-000Z");
        assert_eq!(
            file_timestamp(1_445_412_480_123),
            "2015-10-21T07-28-00-123Z"
        );
        assert_eq!(file_timestamp(951_782_400_000), "2000-02-29T00-00-00-000Z");
    }

    #[test]
    fn session_files_are_named_and_kept_as_pis() {
        let agent = Path::new("/home/me/.agent");
        assert_eq!(
            default_session_dir(Path::new("/work/my app"), agent),
            agent.join("sessions").join("--work-my app--")
        );
        assert_eq!(
            default_session_dir(Path::new("C:\\Users\\me\\proj"), agent),
            agent.join("sessions").join("--C--Users-me-proj--")
        );

        let dir = tempfile::tempdir().unwrap();
        let mut session = SessionManager::create_in("/work", dir.path());
        let file = session.session_file().unwrap().to_path_buf();
        assert_eq!(file.parent(), Some(dir.path()));
        assert_eq!(
            file.file_name().unwrap().to_string_lossy(),
            format!(
                "{}_{}.jsonl",
                file_timestamp(session.header().timestamp),
                session.id()
            )
        );
        session.append_message(user("hello"));
        let reopened = SessionManager::open_file(&file, None).unwrap();
        assert_eq!(reopened.header(), session.header());
        assert_eq!(texts(&reopened), ["hello"]);
        assert_eq!(
            SessionManager::open_file(&file, Some("/elsewhere"))
                .unwrap()
                .cwd(),
            "/elsewhere"
        );

        // A missing file is a new session to be kept there.
        let missing = dir.path().join("later.jsonl");
        let fresh = SessionManager::open_file(&missing, Some("/work")).unwrap();
        assert!(fresh.entries().is_empty());
        assert_eq!(fresh.session_file(), Some(missing.as_path()));
        // Content that is no session is refused.
        std::fs::write(&missing, "{\"type\":\"message\"}\n").unwrap();
        assert!(SessionManager::open_file(&missing, None).is_err());
    }

    #[test]
    fn forking_keeps_only_the_path_and_its_labels() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("original.jsonl");
        let mut session = SessionManager::create("/work", Box::new(JsonlStore::new(&file)));
        let question = session.append_message(user("question"));
        let label = session
            .append_label(&question, Some("here".into()))
            .unwrap();
        let labelled_at = session.entry(&label).unwrap().timestamp;
        let answer = session.append_message(assistant("answer"));
        session.set_leaf(&question).unwrap();
        session.append_message(assistant("other answer"));
        let original = session.id().to_string();

        std::thread::sleep(std::time::Duration::from_millis(2));
        let forked = dir.path().join("forked.jsonl");
        session
            .fork(Some(&answer), Box::new(JsonlStore::new(&forked)))
            .unwrap();
        assert_ne!(session.id(), original);
        // The parent is named by its file, as in Pi.
        assert_eq!(
            session.header().parent_session.as_deref(),
            Some(&*file.to_string_lossy())
        );
        assert_eq!(texts(&session), ["question", "answer"]);
        assert_eq!(session.label(&question), Some("here"));
        let label_entry = session
            .entries()
            .iter()
            .find(|entry| matches!(entry.kind, EntryKind::Label { .. }))
            .unwrap();
        assert_eq!(label_entry.timestamp, labelled_at);
        assert!(session.entries().iter().all(|entry| !matches!(&entry.kind, EntryKind::Message { message } if message.text() == "other answer")));
        assert_eq!(
            JsonlStore::load(&forked).unwrap().unwrap().0,
            *session.header()
        );

        // A session kept nowhere has no file to name.
        session.fork(Some(&answer), Box::new(MemoryStore)).unwrap();
        session.fork(Some(&answer), Box::new(MemoryStore)).unwrap();
        assert_eq!(session.header().parent_session, None);
    }

    #[test]
    fn a_fork_keeps_what_a_compaction_kept_from_a_label() {
        let mut session = SessionManager::in_memory("/work");
        session.append_message(user("old question"));
        let old_answer = session.append_message(assistant("old answer"));
        let label = session
            .append_label(&old_answer, Some("checkpoint".into()))
            .unwrap();
        session.append_message(user("recent question"));
        session.append_message(assistant("recent answer"));
        session.append_compaction("summary".into(), Some(label), 1, None, None, false);
        let leaf = session.append_message(user("next"));
        let before = texts(&session);
        assert_eq!(
            before,
            ["summary", "recent question", "recent answer", "next"]
        );

        session.fork(Some(&leaf), Box::new(MemoryStore)).unwrap();
        assert_eq!(texts(&session), before);
    }

    /// A store that fails while `failing` is set, and otherwise records entry ids.
    struct FlakyStore {
        failing: Arc<AtomicBool>,
        stored: Arc<Mutex<Vec<String>>>,
    }

    impl SessionStore for FlakyStore {
        fn write_all(
            &mut self,
            _header: &SessionHeader,
            entries: &[SessionEntry],
        ) -> io::Result<()> {
            if self.failing.load(Ordering::SeqCst) {
                return Err(io::Error::other("disk full"));
            }
            *self.stored.lock().unwrap() = entries.iter().map(|entry| entry.id.clone()).collect();
            Ok(())
        }

        fn append(&mut self, entry: &SessionEntry) -> io::Result<()> {
            if self.failing.load(Ordering::SeqCst) {
                return Err(io::Error::other("disk full"));
            }
            self.stored.lock().unwrap().push(entry.id.clone());
            Ok(())
        }
    }

    #[test]
    fn after_a_failed_write_the_next_one_stores_everything() {
        let failing = Arc::new(AtomicBool::new(true));
        let stored = Arc::new(Mutex::new(Vec::new()));
        let mut session = SessionManager::create(
            "/work",
            Box::new(FlakyStore {
                failing: failing.clone(),
                stored: stored.clone(),
            }),
        );
        let ids = |session: &SessionManager| -> Vec<String> {
            session
                .entries()
                .iter()
                .map(|entry| entry.id.clone())
                .collect()
        };

        session.append_message(user("hello"));
        assert!(session.take_persist_error().is_some());
        failing.store(false, Ordering::SeqCst);
        session.append_message(assistant("hi"));
        assert_eq!(*stored.lock().unwrap(), ids(&session));

        failing.store(true, Ordering::SeqCst);
        session.append_message(user("again"));
        assert!(session.take_persist_error().is_some());
        failing.store(false, Ordering::SeqCst);
        session.append_message(assistant("ok"));
        assert_eq!(*stored.lock().unwrap(), ids(&session));
        assert!(session.take_persist_error().is_none());
    }

    #[test]
    fn a_line_cut_off_by_a_crash_costs_only_itself() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("s.jsonl");
        let mut session = SessionManager::create("/work", Box::new(JsonlStore::new(&path)));
        session.append_message(user("hello"));
        // Cut inside a two-byte character, so the line is not even valid UTF-8.
        let partial = "{\"id\":\"x\",\"text\":\"é";
        let mut bytes = std::fs::read(&path).unwrap();
        bytes.extend_from_slice(&partial.as_bytes()[..partial.len() - 1]);
        std::fs::write(&path, bytes).unwrap();

        session.append_message(assistant("hi"));
        let (_, entries) = JsonlStore::load(&path).unwrap().unwrap();
        assert_eq!(entries, session.entries());
    }

    #[test]
    fn flush_retries_failed_writes_after_the_error_was_consumed() {
        let failing = Arc::new(AtomicBool::new(false));
        let stored = Arc::new(Mutex::new(Vec::new()));
        let mut session = SessionManager::create(
            "/work",
            Box::new(FlakyStore {
                failing: failing.clone(),
                stored: stored.clone(),
            }),
        );
        assert!(!session.has_unpersisted());
        session.append_message(user("first"));
        assert!(!session.has_unpersisted());
        failing.store(true, Ordering::SeqCst);
        session.append_message(assistant("second"));
        assert!(session.take_persist_error().is_some());
        assert!(session.has_unpersisted());
        assert!(session.flush().is_err());
        assert!(session.has_unpersisted());
        failing.store(false, Ordering::SeqCst);
        session.flush().unwrap();
        assert!(!session.has_unpersisted());
        let expected: Vec<_> = session
            .entries()
            .iter()
            .map(|entry| entry.id.clone())
            .collect();
        assert_eq!(*stored.lock().unwrap(), expected);
        session.flush().unwrap();
        assert_eq!(*stored.lock().unwrap(), expected);
    }

    #[test]
    fn explicit_flush_keeps_the_lazy_first_conversation_contract() {
        let stored = Arc::new(Mutex::new(Vec::new()));
        let mut session = SessionManager::create(
            "/work",
            Box::new(FlakyStore {
                failing: Arc::new(AtomicBool::new(false)),
                stored: stored.clone(),
            }),
        );
        session.append_model_change("provider", "model");
        session.append_session_info(Some("pending".into()));
        assert!(!session.has_unpersisted());
        session.flush().unwrap();
        assert!(stored.lock().unwrap().is_empty());
        assert!(!session.has_unpersisted());
        session.append_message(user("start"));
        assert!(!session.has_unpersisted());
        assert_eq!(stored.lock().unwrap().len(), 3);
    }

    #[test]
    fn fresh_ids_consult_store_history_outside_the_loaded_index() {
        struct ClaimingStore(Arc<Mutex<Vec<String>>>);
        impl SessionStore for ClaimingStore {
            fn write_all(&mut self, _: &SessionHeader, _: &[SessionEntry]) -> io::Result<()> {
                Ok(())
            }
            fn append(&mut self, _: &SessionEntry) -> io::Result<()> {
                Ok(())
            }
            fn contains_entry(&self, id: &str) -> io::Result<bool> {
                let mut queried = self.0.lock().unwrap();
                queried.push(id.into());
                Ok(queried.len() <= 32)
            }
        }
        let queries = Arc::new(Mutex::new(Vec::new()));
        let session = SessionManager::create("/work", Box::new(ClaimingStore(queries.clone())));
        let id = session.fresh_id();
        let queries = queries.lock().unwrap();
        assert_eq!(queries.len(), 33);
        assert!(queries[..32].iter().all(|claimed| claimed != &id));
        assert_eq!(queries.last(), Some(&id));
    }

    #[test]
    fn a_store_id_lookup_error_does_not_block_entry_creation() {
        struct UnavailableStore;
        impl SessionStore for UnavailableStore {
            fn write_all(&mut self, _: &SessionHeader, _: &[SessionEntry]) -> io::Result<()> {
                Ok(())
            }
            fn append(&mut self, _: &SessionEntry) -> io::Result<()> {
                Ok(())
            }
            fn contains_entry(&self, _: &str) -> io::Result<bool> {
                Err(io::Error::other("reader unavailable"))
            }
        }
        let mut session = SessionManager::create("/work", Box::new(UnavailableStore));
        assert_eq!(session.append_message(user("kept")).len(), 8);
        assert!(!session.has_unpersisted());
    }

    #[test]
    fn partial_retry_sends_only_real_tail_entries_and_keeps_the_stored_prefix() {
        struct RetainingStore {
            stored: Arc<Mutex<Vec<SessionEntry>>>,
            retries: Arc<Mutex<Vec<Vec<SessionEntry>>>>,
            retry_initial_bases: Arc<Mutex<Vec<Option<SessionBase>>>>,
            failing: Arc<AtomicBool>,
        }
        impl SessionStore for RetainingStore {
            fn write_all(&mut self, _: &SessionHeader, entries: &[SessionEntry]) -> io::Result<()> {
                self.retries.lock().unwrap().push(entries.to_vec());
                if self.failing.load(Ordering::SeqCst) {
                    return Err(io::Error::other("full"));
                }
                let mut stored = self.stored.lock().unwrap();
                for entry in entries {
                    if !stored.iter().any(|old| old.id == entry.id) {
                        stored.push(entry.clone());
                    }
                }
                Ok(())
            }
            fn write_all_with_base(
                &mut self,
                header: &SessionHeader,
                initial_base: Option<&SessionBase>,
                entries: &[SessionEntry],
            ) -> io::Result<()> {
                self.retry_initial_bases
                    .lock()
                    .unwrap()
                    .push(initial_base.cloned());
                self.write_all(header, entries)
            }
            fn append(&mut self, entry: &SessionEntry) -> io::Result<()> {
                if self.failing.load(Ordering::SeqCst) {
                    return Err(io::Error::other("full"));
                }
                self.stored.lock().unwrap().push(entry.clone());
                Ok(())
            }
            fn contains_entry(&self, id: &str) -> io::Result<bool> {
                Ok(self
                    .stored
                    .lock()
                    .unwrap()
                    .iter()
                    .any(|entry| entry.id == id))
            }
        }
        let mut original = SessionManager::in_memory("/work");
        original.append_model_change("original", "model");
        original.append_thinking_level_change(ThinkingLevel::High);
        original.append_message(user("omitted prefix"));
        let boundary = original.append_message(user("kept tail"));
        original.append_compaction(
            "prefix summary".into(),
            Some(boundary.clone()),
            100,
            None,
            None,
            false,
        );
        let base = original.base_before(&boundary).unwrap();
        let tail = original.entries()[3..].to_vec();
        let prefix = original.entries()[..3].to_vec();
        let stored = Arc::new(Mutex::new(original.entries().to_vec()));
        let retries = Arc::new(Mutex::new(Vec::new()));
        let retry_initial_bases = Arc::new(Mutex::new(Vec::new()));
        let failing = Arc::new(AtomicBool::new(true));
        let mut partial = SessionManager::open_partial(
            original.header().clone(),
            base.clone(),
            tail.clone(),
            Box::new(RetainingStore {
                stored: stored.clone(),
                retries: retries.clone(),
                retry_initial_bases: retry_initial_bases.clone(),
                failing: failing.clone(),
            }),
        );
        partial.append_message(assistant("not yet written"));
        assert!(partial.take_persist_error().is_some());
        assert!(partial.has_unpersisted());
        assert!(partial.flush().is_err());
        failing.store(false, Ordering::SeqCst);
        partial.flush().unwrap();
        assert!(!partial.has_unpersisted());
        let stored = stored.lock().unwrap();
        assert_eq!(&stored[..3], prefix);
        assert_eq!(&stored[3..], partial.entries());
        assert_eq!(retries.lock().unwrap().len(), 2);
        assert_eq!(
            *retry_initial_bases.lock().unwrap(),
            vec![Some(base.clone()), Some(base)]
        );
        for retry in retries.lock().unwrap().iter() {
            assert_eq!(&retry[..tail.len()], tail);
            assert_eq!(retry.len(), tail.len() + 1);
            assert!(retry.iter().all(|entry| !entry.id.is_empty()));
        }
    }

    #[test]
    fn jsonl_id_lookup_tracks_replacements_and_appends() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("ids.jsonl");
        let mut store = JsonlStore::new(&path);
        assert!(!store.contains_entry("missing").unwrap());
        let mut session = SessionManager::in_memory("/work");
        session.append_message(user("first"));
        store
            .write_all(session.header(), session.entries())
            .unwrap();
        let first = session.entries()[0].id.clone();
        assert!(store.contains_entry(&first).unwrap());
        session.append_message(user("second"));
        let second = session.entries()[1].clone();
        store.append(&second).unwrap();
        assert!(store.contains_entry(&second.id).unwrap());
        store.write_all(session.header(), &[]).unwrap();
        assert!(!store.contains_entry(&first).unwrap());
        assert!(!store.contains_entry(&second.id).unwrap());
    }

    fn partial_fixture_header() -> SessionHeader {
        SessionHeader {
            version: SESSION_VERSION,
            id: "partial-fixture".into(),
            timestamp: 1,
            cwd: "/work/partial-fixture".into(),
            parent_session: None,
        }
    }

    fn partial_fixture_open(entries: Vec<SessionEntry>) -> SessionManager {
        SessionManager::open(partial_fixture_header(), entries, Box::new(MemoryStore))
    }

    fn partial_fixture_push(entries: &mut Vec<SessionEntry>, mut kind: EntryKind) -> String {
        let id = format!("entry-{:04}", entries.len());
        let timestamp = entries.len() as i64 + 10;
        if let EntryKind::Message { message } = &mut kind {
            match message {
                SessionMessage::Llm(Message::System(message)) => message.timestamp = timestamp,
                SessionMessage::Llm(Message::User(message)) => message.timestamp = timestamp,
                SessionMessage::Llm(Message::Assistant(message)) => message.timestamp = timestamp,
                _ => {}
            }
        }
        entries.push(SessionEntry {
            id: id.clone(),
            parent_id: entries.last().map(|entry| entry.id.clone()),
            timestamp,
            kind,
        });
        id
    }

    fn partial_fixture_message(entries: &mut Vec<SessionEntry>, message: SessionMessage) -> String {
        partial_fixture_push(entries, EntryKind::Message { message })
    }

    fn partial_fixture_reply(text: &str, context_tokens: u64, model: &str) -> SessionMessage {
        let mut message = assistant(text);
        if let SessionMessage::Llm(Message::Assistant(reply)) = &mut message {
            reply.provider = "fixture-provider".into();
            reply.model = model.into();
            reply.usage.total_tokens = context_tokens;
        }
        message
    }

    fn partial_fixture_system(session: &SessionManager) -> Option<SystemMessage> {
        let projection = session.projection();
        current_system_message(
            projection
                .messages
                .iter()
                .filter_map(AgentMessage::as_message),
        )
    }

    fn partial_fixture_compact(entries: &mut Vec<SessionEntry>, first_kept: &str, summary: &str) {
        let mut system_message = partial_fixture_system(&partial_fixture_open(entries.clone()));
        if let Some(system) = &mut system_message {
            system.timestamp = entries.len() as i64 + 10;
        }
        partial_fixture_push(
            entries,
            EntryKind::Compaction {
                summary: summary.into(),
                first_kept_entry_id: first_kept.into(),
                tokens_before: 900_000,
                details: Some(json!({ "fixture": summary })),
                usage: None,
                from_extension: false,
                system_message,
            },
        );
    }

    fn assert_partial_fixture_equal(full: &SessionManager, partial: &SessionManager) {
        let full_projection = full.projection();
        let partial_projection = partial.projection();
        let conversations = |projection: &SessionProjection| -> Vec<SessionMessage> {
            projection
                .messages
                .iter()
                .filter(|message| !matches!(message.as_message(), Some(Message::System(_))))
                .cloned()
                .collect()
        };
        assert_eq!(
            conversations(&partial_projection),
            conversations(&full_projection)
        );
        assert_eq!(
            partial_fixture_system(partial),
            partial_fixture_system(full)
        );
        assert_eq!(partial_projection.model, full_projection.model);
        assert_eq!(
            partial_projection.thinking_level,
            full_projection.thinking_level
        );
        assert_eq!(partial.session_name(), full.session_name());
        assert_eq!(
            crate::compaction::estimate_projected_context_tokens(
                &partial_projection,
                &partial.branch()
            ),
            crate::compaction::estimate_projected_context_tokens(&full_projection, &full.branch()),
        );
        assert_eq!(partial.leaf_id(), full.leaf_id());
    }

    /// Old prefixes deliberately include changing prompt sections, tool definitions,
    /// model choices, names (including cleared names), and reasoning state. The first
    /// kept boundary is state-only in two cases, which catches snapshots taken after
    /// the boundary instead of immediately before it.
    fn partial_fixture_history(
        prefix_turns: usize,
        boundary_kind: usize,
    ) -> (Vec<SessionEntry>, Vec<usize>) {
        let mut entries = Vec::new();
        partial_fixture_push(
            &mut entries,
            EntryKind::ModelChange {
                provider: "initial-provider".into(),
                model_id: "initial-model".into(),
            },
        );
        partial_fixture_push(
            &mut entries,
            EntryKind::ThinkingLevelChange {
                thinking_level: ThinkingLevel::High,
            },
        );
        partial_fixture_push(
            &mut entries,
            EntryKind::SessionInfo {
                name: Some("initial name".into()),
            },
        );
        let mut system = SystemMessage {
            content: "initial instructions".into(),
            tools_added: vec![
                pi_ai::Tool::new(
                    "read",
                    "initial reader",
                    json!({"type":"object","properties":{}}),
                ),
                pi_ai::Tool::new(
                    "write",
                    "initial writer",
                    json!({"type":"object","properties":{}}),
                ),
            ],
            ..SystemMessage::default()
        };
        system
            .sections
            .insert("project".into(), Some("old project".into()));
        system
            .sections
            .insert("temporary".into(), Some("remove me".into()));
        partial_fixture_message(&mut entries, SessionMessage::Llm(Message::System(system)));
        for turn in 0..prefix_turns {
            partial_fixture_push(
                &mut entries,
                EntryKind::ModelChange {
                    provider: format!("provider-{turn}"),
                    model_id: format!("model-{turn}"),
                },
            );
            partial_fixture_push(
                &mut entries,
                EntryKind::ThinkingLevelChange {
                    thinking_level: ThinkingLevel::ALL[turn % ThinkingLevel::ALL.len()],
                },
            );
            partial_fixture_push(
                &mut entries,
                EntryKind::SessionInfo {
                    name: (turn % 3 != 0).then(|| format!("name-{turn}")),
                },
            );
            let mut update = SystemMessage {
                content: format!("added instruction {turn}"),
                ..SystemMessage::default()
            };
            update
                .sections
                .insert("project".into(), Some(format!("project-{turn}")));
            update.sections.insert("temporary".into(), None);
            update.tools_added.push(pi_ai::Tool::new(
                "read",
                format!("reader-{turn}"),
                json!({"type":"object","properties":{"path":{"type":"string"}}}),
            ));
            partial_fixture_message(&mut entries, SessionMessage::Llm(Message::System(update)));
            partial_fixture_message(&mut entries, user(&format!("discarded question {turn}")));
            partial_fixture_message(
                &mut entries,
                partial_fixture_reply(
                    "discarded answer",
                    700_000 + turn as u64,
                    &format!("answered-{turn}"),
                ),
            );
        }

        let first_cut = entries.len();
        let first_kept = partial_fixture_push(
            &mut entries,
            match boundary_kind {
                0 => EntryKind::Message {
                    message: user("first kept question"),
                },
                1 => EntryKind::ModelChange {
                    provider: "boundary-provider".into(),
                    model_id: "boundary-model".into(),
                },
                _ => EntryKind::ThinkingLevelChange {
                    thinking_level: ThinkingLevel::Max,
                },
            },
        );
        let edited = partial_fixture_message(&mut entries, user("kept input to edit"));
        partial_fixture_message(
            &mut entries,
            partial_fixture_reply("kept answer", 500_000, "kept-model"),
        );
        partial_fixture_compact(&mut entries, &first_kept, "first compaction");
        partial_fixture_push(
            &mut entries,
            EntryKind::ContextEdit {
                target_id: edited,
                replacement: Some(vec![Content::text("short edited input")]),
            },
        );
        partial_fixture_message(&mut entries, user("next turn"));

        let second_cut = entries.len();
        let second_kept = partial_fixture_push(
            &mut entries,
            EntryKind::SessionInfo {
                // Test both inherited names and explicit clears after the snapshot.
                name: if prefix_turns.is_multiple_of(2) {
                    None
                } else {
                    Some("second-compaction name".into())
                },
            },
        );
        partial_fixture_message(&mut entries, user("second kept question"));
        partial_fixture_message(
            &mut entries,
            partial_fixture_reply("second kept answer", 800_000, "second-kept-model"),
        );
        let mut update = SystemMessage {
            content: "post-boundary instructions".into(),
            tools_removed: vec![pi_ai::ToolReference {
                name: "write".into(),
            }],
            ..SystemMessage::default()
        };
        update
            .sections
            .insert("project".into(), Some("current project".into()));
        update.tools_added.push(pi_ai::Tool::new(
            "search",
            "search current project",
            json!({"type":"object","properties":{"query":{"type":"string"}}}),
        ));
        partial_fixture_message(&mut entries, SessionMessage::Llm(Message::System(update)));
        partial_fixture_compact(&mut entries, &second_kept, "second compaction");
        partial_fixture_message(
            &mut entries,
            partial_fixture_reply("fresh measured answer", 41_000, "latest-model"),
        );
        partial_fixture_message(&mut entries, user("tail"));
        (entries, vec![first_cut, second_cut])
    }

    #[test]
    fn partial_sessions_match_full_replay_across_prefix_sizes_and_repeated_compactions() {
        for prefix_turns in [0, 1, 2, 3, 7, 16, 31] {
            for boundary_kind in 0..3 {
                let (entries, cuts) = partial_fixture_history(prefix_turns, boundary_kind);
                for end in [cuts[1], entries.len()] {
                    let entries = &entries[..end];
                    let full = partial_fixture_open(entries.to_vec());
                    for cut in cuts.iter().copied().filter(|cut| *cut < end) {
                        let boundary = &entries[cut];
                        let base = full.base_before(&boundary.id).unwrap();
                        let prefix = partial_fixture_open(entries[..cut].to_vec());
                        assert_eq!(base.model, prefix.projection().model);
                        assert_eq!(base.thinking_level, prefix.projection().thinking_level);
                        assert_eq!(base.session_name.as_deref(), prefix.session_name());
                        assert_eq!(base.system_message, partial_fixture_system(&prefix));
                        let tail = entries[cut..].to_vec();
                        let partial = SessionManager::open_partial(
                            partial_fixture_header(),
                            base,
                            tail.clone(),
                            Box::new(MemoryStore),
                        );
                        assert_eq!(partial.entries(), tail);
                        assert_eq!(
                            partial.entries()[0].parent_id,
                            Some(entries[cut - 1].id.clone())
                        );
                        assert!(partial.entry(&entries[cut - 1].id).is_none());
                        assert_partial_fixture_equal(&full, &partial);
                        // AgentSession uses a branch copy for boundary previews. It must
                        // carry the base without converting it into synthetic entries.
                        let copy = partial.branch_copy();
                        assert_eq!(copy.entries(), partial.entries());
                        assert_partial_fixture_equal(&full, &copy);
                    }
                }
            }
        }
    }

    #[test]
    fn a_partial_session_inherits_state_when_the_kept_tail_never_reselects_it() {
        let mut entries = Vec::new();
        partial_fixture_push(
            &mut entries,
            EntryKind::ModelChange {
                provider: "inherited-provider".into(),
                model_id: "inherited-model".into(),
            },
        );
        partial_fixture_push(
            &mut entries,
            EntryKind::ThinkingLevelChange {
                thinking_level: ThinkingLevel::Xhigh,
            },
        );
        partial_fixture_push(
            &mut entries,
            EntryKind::SessionInfo {
                name: Some("inherited name".into()),
            },
        );
        partial_fixture_message(
            &mut entries,
            SessionMessage::Llm(Message::System(SystemMessage {
                content: "inherited prompt".into(),
                tools_added: vec![pi_ai::Tool::new(
                    "read",
                    "reader",
                    json!({"type":"object","properties":{}}),
                )],
                ..SystemMessage::default()
            })),
        );
        let cut = entries.len();
        let first_kept = partial_fixture_message(&mut entries, user("kept question"));
        partial_fixture_compact(&mut entries, &first_kept, "summarized prefix");
        partial_fixture_message(&mut entries, user("next question"));
        let full = partial_fixture_open(entries.clone());
        let partial = SessionManager::open_partial(
            partial_fixture_header(),
            full.base_before(&first_kept).unwrap(),
            entries[cut..].to_vec(),
            Box::new(MemoryStore),
        );
        assert_eq!(
            partial.projection().model,
            Some(("inherited-provider".into(), "inherited-model".into()))
        );
        assert_eq!(partial.projection().thinking_level, ThinkingLevel::Xhigh);
        assert_eq!(partial.session_name(), Some("inherited name"));
        assert_partial_fixture_equal(&full, &partial);
        // Further bounded snapshots must start from the inherited base, too.
        let leaf = full.leaf_id().unwrap();
        assert_eq!(
            serde_json::to_value(partial.base_before(leaf).unwrap()).unwrap(),
            serde_json::to_value(full.base_before(leaf).unwrap()).unwrap(),
        );
    }

    #[test]
    fn a_partial_session_invalidates_old_usage_after_an_edit_and_trusts_new_usage() {
        let (mut entries, cuts) = partial_fixture_history(7, 1);
        let edited_id = entries
            .iter()
            .rev()
            .find_map(|entry| match &entry.kind {
                EntryKind::Message {
                    message: SessionMessage::Llm(Message::User(_)),
                } => Some(entry.id.clone()),
                _ => None,
            })
            .unwrap();
        partial_fixture_push(
            &mut entries,
            EntryKind::ContextEdit {
                target_id: edited_id,
                replacement: Some(vec![Content::text("edited tail")]),
            },
        );
        let mut full = partial_fixture_open(entries.clone());
        let cut = *cuts.last().unwrap();
        let mut partial = SessionManager::open_partial(
            partial_fixture_header(),
            full.base_before(&entries[cut].id).unwrap(),
            entries[cut..].to_vec(),
            Box::new(MemoryStore),
        );
        assert_partial_fixture_equal(&full, &partial);
        assert!(
            crate::compaction::estimate_projected_context_tokens(
                &partial.projection(),
                &partial.branch()
            ) < 1_000
        );
        let fresh = partial_fixture_reply("new measured answer", 12_345, "measured-model");
        // Clone the same message: its timestamp and usage are part of the comparison.
        full.append_message(fresh.clone());
        partial.append_message(fresh);
        let projection = partial.projection();
        assert_eq!(
            crate::compaction::estimate_projected_context_tokens(&projection, &partial.branch()),
            12_345
        );
        // Appended IDs differ, so compare only effective state, not leaf identity.
        assert_eq!(partial.projection().messages, full.projection().messages);
        assert_eq!(partial.projection().model, full.projection().model);
        assert_eq!(
            partial_fixture_system(&partial),
            partial_fixture_system(&full)
        );
    }

    #[test]
    fn partial_snapshot_excludes_boundary_state_and_roundtrips_without_entries() {
        let (entries, cuts) = partial_fixture_history(2, 1);
        let full = partial_fixture_open(entries.clone());
        let first_cut = cuts[0];
        let base = full.base_before(&entries[first_cut].id).unwrap();
        assert_ne!(
            base.model,
            Some(("boundary-provider".into(), "boundary-model".into()))
        );
        let encoded = serde_json::to_vec(&base).unwrap();
        let decoded: SessionBase = serde_json::from_slice(&encoded).unwrap();
        assert_eq!(decoded.model, base.model);
        assert_eq!(decoded.thinking_level, base.thinking_level);
        assert_eq!(decoded.session_name, base.session_name);
        assert_eq!(decoded.system_message, base.system_message);
        assert!(matches!(
            full.base_before("missing-entry"),
            Err(SessionError::NotFound(_))
        ));

        let default: SessionBase = serde_json::from_str("{}").unwrap();
        assert_eq!(default.model, None);
        assert_eq!(default.thinking_level, ThinkingLevel::Off);
        assert_eq!(default.session_name, None);
        assert_eq!(default.system_message, None);
        let partial = SessionManager::open_partial(
            partial_fixture_header(),
            default,
            entries.clone(),
            Box::new(MemoryStore),
        );
        assert_eq!(partial.entries(), entries);
        assert_partial_fixture_equal(&full, &partial);
    }

    #[test]
    fn compaction_append_passes_its_preboundary_state_to_the_store_atomically() {
        #[derive(Default)]
        struct ContextCapture {
            append_bases: Mutex<Vec<Option<SessionBase>>>,
            retry_bases: Mutex<Vec<Option<SessionBase>>>,
        }
        struct CaptureStore(Arc<ContextCapture>);
        impl SessionStore for CaptureStore {
            fn write_all(&mut self, _: &SessionHeader, _: &[SessionEntry]) -> io::Result<()> {
                Ok(())
            }
            fn append(&mut self, _: &SessionEntry) -> io::Result<()> {
                Ok(())
            }
            fn append_with_context(
                &mut self,
                _: &SessionEntry,
                base: Option<&SessionBase>,
            ) -> io::Result<()> {
                self.0.append_bases.lock().unwrap().push(base.cloned());
                Ok(())
            }
            fn write_all_with_base(
                &mut self,
                _: &SessionHeader,
                base: Option<&SessionBase>,
                _: &[SessionEntry],
            ) -> io::Result<()> {
                self.0.retry_bases.lock().unwrap().push(base.cloned());
                Ok(())
            }
        }
        let captured = Arc::new(ContextCapture::default());
        let mut session = SessionManager::create("/work", Box::new(CaptureStore(captured.clone())));
        session.append_model_change("selected-provider", "selected-model");
        session.append_thinking_level_change(ThinkingLevel::High);
        session.append_session_info(Some("saved name".into()));
        session.append_message(SessionMessage::Llm(Message::System(SystemMessage {
            content: "full prompt".into(),
            ..SystemMessage::default()
        })));
        session.append_message(user("old question"));
        let boundary = session.append_model_change("kept-provider", "kept-model");
        session.append_message(user("kept question"));
        let expected = session.base_before(&boundary).unwrap();
        session.append_compaction("summary".into(), Some(boundary), 100, None, None, false);
        assert_eq!(
            captured.append_bases.lock().unwrap().last(),
            Some(&Some(expected.clone()))
        );
        assert_eq!(
            expected.model,
            Some(("selected-provider".into(), "selected-model".into()))
        );
        assert_eq!(expected.session_name.as_deref(), Some("saved name"));
        assert_eq!(
            expected.system_message.as_ref().unwrap().content,
            "full prompt"
        );
        assert_eq!(*captured.retry_bases.lock().unwrap(), vec![None]);

        // A new compaction that keeps no preceding entries snapshots right before
        // itself. It does not accidentally reuse the older compaction boundary.
        let expected = SessionBase {
            model: session.projection().model,
            thinking_level: session.projection().thinking_level,
            session_name: session.session_name().map(str::to_owned),
            system_message: partial_fixture_system(&session),
        };
        session.append_compaction("all summarized".into(), None, 100, None, None, false);
        assert_eq!(
            captured.append_bases.lock().unwrap().last(),
            Some(&Some(expected))
        );
    }

    #[test]
    fn partial_export_and_fork_require_a_complete_reload_and_reset_starts_a_new_context() {
        let (entries, cuts) = partial_fixture_history(2, 1);
        let full = partial_fixture_open(entries.clone());
        let cut = cuts[1];
        let base = full.base_before(&entries[cut].id).unwrap();
        let mut partial = SessionManager::open_partial(
            full.header().clone(),
            base,
            entries[cut..].to_vec(),
            Box::new(MemoryStore),
        );
        let leaf = partial.leaf_id().unwrap().to_string();
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("incomplete.jsonl");
        assert_eq!(
            partial.export_branch(&path).unwrap_err().kind(),
            io::ErrorKind::Unsupported
        );
        assert!(!path.exists());
        assert!(partial.branched(Some(&leaf), None).is_err());
        assert!(partial.fork(Some(&leaf), Box::new(MemoryStore)).is_err());
        partial.reset_leaf();
        partial.append_message(user("a fresh root"));
        let projection = partial.projection();
        assert_eq!(projection.model, None);
        assert_eq!(projection.thinking_level, ThinkingLevel::Off);
        assert_eq!(texts(&partial), ["a fresh root"]);
        assert!(partial_fixture_system(&partial).is_none());
    }
}
