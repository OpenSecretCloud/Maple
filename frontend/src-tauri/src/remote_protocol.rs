//! Maple-owned wire types for remote Agent Mode.
//!
//! These types intentionally contain no Goose or Tauri values. They form the
//! stable seam shared by every Maple Tauri platform and the desktop host.
#![allow(
    dead_code,
    reason = "bounded foundation is wired in later vertical slices"
)]

use std::{
    collections::{BTreeMap, BTreeSet},
    fmt,
    marker::PhantomData,
};

use serde::{
    de::{self, IgnoredAny, SeqAccess, Visitor},
    Deserialize, Deserializer, Serialize,
};

pub const PROTOCOL_VERSION: u16 = 1;
pub const ALPN: &[u8] = b"cloud.opensecret.maple/agent/1";
pub const MAX_FRAME_BYTES: u32 = 1_048_576;
/// Presentation cap shared by embedded and remote history adapters. The
/// reserved envelope margin ensures any record accepted locally also fits in
/// one correlated remote response frame.
pub const MAX_HISTORY_RECORD_PRESENTATION_BYTES: usize = MAX_FRAME_BYTES as usize - 8_192;
/// A live item is framed independently inside a correlated response envelope.
/// This mirrors the native 192 KiB presentation bound while retaining ample
/// headroom for IDs, labels, cursor metadata, and encoding overhead.
pub const MAX_LIVE_ITEM_PRESENTATION_BYTES: usize = 192 * 1_024;
pub const DEFAULT_PAGE_SIZE: u16 = 25;
pub const MAX_PAGE_SIZE: u16 = 50;
pub const MAX_ID_BYTES: usize = 128;
pub const MAX_CURSOR_BYTES: usize = 512;
pub const MAX_ERROR_MESSAGE_BYTES: usize = 512;
pub const MAX_PROJECT_ROOT_BYTES: usize = 4_096;
pub const MAX_MODEL_ID_BYTES: usize = 256;
pub const MAX_AGENT_MODE_BYTES: usize = 64;
pub const MAX_SESSION_TITLE_BYTES: usize = 1_024;
pub const MAX_ACTIVE_RUNS: usize = 64;
pub const MAX_HISTORY_ITEMS_PER_RECORD: usize = 200;
pub const MAX_LIVE_SESSIONS_PER_ACCOUNT: usize = 64;
pub const MAX_LIVE_ITEMS_PER_SESSION: usize = 200;
pub const MAX_LIVE_ITEMS_PER_ACCOUNT: usize = 512;
/// Matches the native coordinator's account projection checkpoint bound. C0
/// consumers charge this conservative bound incrementally so a malicious but
/// authenticated host cannot turn individually valid item frames into an
/// unexpectedly large retained mobile snapshot.
pub const MAX_LIVE_PROJECTION_BYTES_PER_ACCOUNT: usize = 8 * 1_024 * 1_024;
pub(crate) const LIVE_PROJECTION_OUTER_OVERHEAD_BYTES: usize = 4 * 1_024;
const LIVE_PROJECTION_SESSION_OVERHEAD_BYTES: usize = 256;
const LIVE_PROJECTION_ITEM_OVERHEAD_BYTES: usize = 256;
pub const MAX_HISTORY_ITEM_LABEL_BYTES: usize = 1_024;
pub(crate) const SAFE_REMOTE_SETUP_WARNING: &str =
    "Some Agent integrations could not connect. Review Agent settings on the host.";
pub(crate) const SAFE_REMOTE_AGENT_ERROR: &str =
    "The Agent task failed. Open the host for additional diagnostic details.";
pub(crate) const SAFE_REMOTE_TOOL_TITLE: &str = "Tool activity";
pub(crate) const SAFE_REMOTE_TOOL_FAILED: &str =
    "The tool failed. Open the host for additional diagnostic details.";
pub(crate) const SAFE_REMOTE_TOOL_CANCELLED: &str = "The tool was cancelled.";
pub(crate) const SAFE_REMOTE_PERMISSION_TITLE: &str = "Tool permission";
const MAX_JAVASCRIPT_SAFE_INTEGER: i64 = 9_007_199_254_740_991;
const MAX_JAVASCRIPT_SAFE_UNSIGNED_INTEGER: u64 = 9_007_199_254_740_991;

/// Identifies one live host epoch and one connection attempt within it.
/// `host_epoch` increases whenever the host process starts a new resumability
/// epoch; `generation` increases for every replacement connection in that
/// epoch. Zero is reserved as an invalid/uninitialized value for both fields.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ConnectionStamp {
    host_epoch: u64,
    generation: u64,
}

impl ConnectionStamp {
    pub fn new(host_epoch: u64, generation: u64) -> Result<Self, ProtocolError> {
        let stamp = Self {
            host_epoch,
            generation,
        };
        stamp.validate()?;
        Ok(stamp)
    }

    pub const fn host_epoch(self) -> u64 {
        self.host_epoch
    }

    pub const fn generation(self) -> u64 {
        self.generation
    }

    pub fn validate(self) -> Result<(), ProtocolError> {
        if self.host_epoch == 0 || self.generation == 0 {
            Err(ProtocolError::new(
                ErrorCode::StaleGeneration,
                "connection stamp epoch and generation must be positive",
                true,
            ))
        } else {
            Ok(())
        }
    }
}

/// Every request/response body must opt into validation. This prevents a new
/// DTO from being placed on the wire without an explicit boundedness review.
pub trait WireBody {
    fn stream_kind(&self) -> StreamKind;

    fn validate_body(&self) -> Result<(), ProtocolError>;

    /// Validate only fields whose meaning depends on the envelope's current
    /// connection stamp. Envelopes always call [`WireBody::validate_body`]
    /// first, so an override cannot accidentally bypass ordinary bounds.
    fn validate_body_for_stamp(
        &self,
        _connection_stamp: ConnectionStamp,
    ) -> Result<(), ProtocolError> {
        Ok(())
    }
}

/// Explicitly admits a DTO as a request and fixes the one direction in which
/// that operation may be initiated. There is deliberately no blanket impl:
/// adding a request operation requires a direction review.
pub trait RequestBody: WireBody {
    fn allowed_direction(&self) -> PeerDirection;
}

/// Explicitly pairs a successful response DTO with the request DTO whose
/// context it must validate against. There is deliberately no blanket impl:
/// adding a new request/response operation requires a pairing review.
pub trait ResponseBody<TRequest: RequestBody>: WireBody {
    fn validate_response_to(&self, request: &TRequest) -> Result<(), ProtocolError>;
}

/// Explicitly admits a bounded DTO as an item in a paged response. There is no
/// blanket implementation: each concrete page item requires an operation and
/// pagination review before `Page<T>` can satisfy a response pairing.
pub trait PageItem: WireBody {}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PeerDirection {
    ControllerToHost,
    HostToController,
}

impl PeerDirection {
    pub fn opposite(self) -> Self {
        match self {
            Self::ControllerToHost => Self::HostToController,
            Self::HostToController => Self::ControllerToHost,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum StreamKind {
    Control,
    Events,
    Bulk,
}

impl StreamKind {
    /// Iroh/noq sends larger values first. Keep interactive control work ahead
    /// of events, and both ahead of paged history or attachment transfer.
    pub const fn priority(self) -> i32 {
        match self {
            Self::Control => 100,
            Self::Events => 50,
            Self::Bulk => 0,
        }
    }
}

/// The operation marker is deliberately closed and unrelated to Tauri command
/// names. Adding another remote operation requires a new reviewed wire body and
/// host adapter; callers cannot submit an arbitrary native command string.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RuntimeStatusOperation {
    GetRuntimeStatus,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct GetRuntimeStatusRequest {
    pub operation: RuntimeStatusOperation,
}

impl GetRuntimeStatusRequest {
    pub const fn new() -> Self {
        Self {
            operation: RuntimeStatusOperation::GetRuntimeStatus,
        }
    }
}

impl Default for GetRuntimeStatusRequest {
    fn default() -> Self {
        Self::new()
    }
}

impl WireBody for GetRuntimeStatusRequest {
    fn stream_kind(&self) -> StreamKind {
        StreamKind::Control
    }

    fn validate_body(&self) -> Result<(), ProtocolError> {
        match self.operation {
            RuntimeStatusOperation::GetRuntimeStatus => Ok(()),
        }
    }
}

impl RequestBody for GetRuntimeStatusRequest {
    fn allowed_direction(&self) -> PeerDirection {
        PeerDirection::ControllerToHost
    }
}

/// Bounded, transport-neutral projection of Maple's local runtime status.
///
/// The desktop adapter converts `AgentRuntimeStatus` into this type. Keeping
/// this DTO in the shared protocol module lets mobile controllers compile the
/// wire contract without compiling Goose or Maple's desktop Agent runtime.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct RemoteAgentRuntimeStatus {
    pub running: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub project_root: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mode: Option<String>,
    #[serde(default)]
    pub active_runs: BTreeMap<String, String>,
}

impl RemoteAgentRuntimeStatus {
    pub fn validate(&self) -> Result<(), ProtocolError> {
        let runtime_fields = [
            self.project_root.is_some(),
            self.model.is_some(),
            self.mode.is_some(),
        ];
        let fields_match_state = if self.running {
            runtime_fields.into_iter().all(|present| present)
        } else {
            runtime_fields.into_iter().all(|present| !present)
        };
        if !fields_match_state {
            return Err(ProtocolError::new(
                ErrorCode::InvalidFrame,
                "runtime status fields do not match the running state",
                false,
            ));
        }
        if !self.running && !self.active_runs.is_empty() {
            return Err(ProtocolError::new(
                ErrorCode::InvalidFrame,
                "a stopped runtime cannot contain active runs",
                false,
            ));
        }
        validate_optional_status_field(
            "project root",
            self.project_root.as_deref(),
            MAX_PROJECT_ROOT_BYTES,
        )?;
        validate_optional_status_field("model", self.model.as_deref(), MAX_MODEL_ID_BYTES)?;
        validate_optional_status_field("mode", self.mode.as_deref(), MAX_AGENT_MODE_BYTES)?;
        if self.active_runs.len() > MAX_ACTIVE_RUNS {
            return Err(ProtocolError::new(
                ErrorCode::InvalidFrame,
                "runtime status contains too many active runs",
                false,
            ));
        }
        for (session_id, run_id) in &self.active_runs {
            validate_id("active run session id", session_id)?;
            validate_id("active run id", run_id)?;
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct GetRuntimeStatusResponse {
    pub status: RemoteAgentRuntimeStatus,
}

impl GetRuntimeStatusResponse {
    pub fn new(status: RemoteAgentRuntimeStatus) -> Result<Self, ProtocolError> {
        status.validate()?;
        Ok(Self { status })
    }
}

impl WireBody for GetRuntimeStatusResponse {
    fn stream_kind(&self) -> StreamKind {
        StreamKind::Control
    }

    fn validate_body(&self) -> Result<(), ProtocolError> {
        self.status.validate()
    }
}

impl ResponseBody<GetRuntimeStatusRequest> for GetRuntimeStatusResponse {
    fn validate_response_to(&self, request: &GetRuntimeStatusRequest) -> Result<(), ProtocolError> {
        request.validate_body()?;
        self.validate_body()
    }
}

impl ResponseBody<RemoteAgentControlRequest> for GetRuntimeStatusResponse {
    fn validate_response_to(
        &self,
        request: &RemoteAgentControlRequest,
    ) -> Result<(), ProtocolError> {
        match request {
            RemoteAgentControlRequest::GetRuntimeStatus => self.validate_body(),
            _ => Err(ProtocolError::new(
                ErrorCode::InvalidFrame,
                "runtime status response was sent for another Control operation",
                false,
            )),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AgentHistoryOperation {
    ListSessionRecords,
}

/// Concrete count-based request for Goose's native persisted message records.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ListAgentHistoryRecordsRequest {
    pub operation: AgentHistoryOperation,
    pub session_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cursor: Option<String>,
    #[serde(default = "default_page_size")]
    pub limit: u16,
}

impl ListAgentHistoryRecordsRequest {
    pub fn new(
        session_id: impl Into<String>,
        cursor: Option<String>,
        limit: u16,
    ) -> Result<Self, ProtocolError> {
        let request = Self {
            operation: AgentHistoryOperation::ListSessionRecords,
            session_id: session_id.into(),
            cursor,
            limit,
        };
        request.validate()?;
        Ok(request)
    }

    pub fn validate(&self) -> Result<(), ProtocolError> {
        match self.operation {
            AgentHistoryOperation::ListSessionRecords => {}
        }
        validate_id("Agent session id", &self.session_id)?;
        validate_page_limit_and_cursor(self.limit, self.cursor.as_deref())
    }
}

impl WireBody for ListAgentHistoryRecordsRequest {
    fn stream_kind(&self) -> StreamKind {
        StreamKind::Bulk
    }

    fn validate_body(&self) -> Result<(), ProtocolError> {
        self.validate()
    }
}

impl RequestBody for ListAgentHistoryRecordsRequest {
    fn allowed_direction(&self) -> PeerDirection {
        PeerDirection::ControllerToHost
    }
}

/// Mobile-compilable projection of Maple's existing timeline item contract.
/// Goose/provider values never appear directly on the wire.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct RemoteAgentTimelineItem {
    pub id: String,
    pub item_type: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub role: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub text: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub status: Option<String>,
    pub created_ms: u64,
    pub merge: String,
}

impl RemoteAgentTimelineItem {
    pub fn validate(&self) -> Result<(), ProtocolError> {
        validate_id("timeline item id", &self.id)?;
        if self.created_ms > MAX_JAVASCRIPT_SAFE_UNSIGNED_INTEGER {
            return Err(invalid_history_record(
                "timeline item timestamp is outside the safe wire range",
            ));
        }
        if !matches!(
            self.item_type.as_str(),
            "message" | "thinking" | "tool" | "permission" | "system" | "error"
        ) {
            return Err(invalid_history_record("invalid timeline item type"));
        }
        if self
            .role
            .as_deref()
            .is_some_and(|role| !matches!(role, "user" | "assistant" | "thought" | "system"))
        {
            return Err(invalid_history_record("invalid timeline item role"));
        }
        validate_optional_history_label(
            "timeline item title",
            self.title.as_deref(),
            MAX_HISTORY_ITEM_LABEL_BYTES,
        )?;
        validate_optional_content_text("timeline item text", self.text.as_deref())?;
        validate_optional_history_label("timeline item status", self.status.as_deref(), 64)?;
        if !matches!(self.merge.as_str(), "append" | "replace") {
            return Err(invalid_history_record("invalid timeline merge mode"));
        }
        match self.item_type.as_str() {
            "tool" => {
                let expected_text = match self.status.as_deref() {
                    None | Some("pending" | "running" | "completed") => None,
                    Some("failed" | "error") => Some(SAFE_REMOTE_TOOL_FAILED),
                    Some("cancelled") => Some(SAFE_REMOTE_TOOL_CANCELLED),
                    Some(_) => return Err(invalid_history_record("invalid safe tool status")),
                };
                if self.role.as_deref() != Some("assistant")
                    || self.title.as_deref() != Some(SAFE_REMOTE_TOOL_TITLE)
                    || self.text.as_deref() != expected_text
                {
                    return Err(invalid_history_record("unsafe tool presentation"));
                }
            }
            "permission" => {
                if self.role.as_deref() != Some("system")
                    || self.title.as_deref() != Some(SAFE_REMOTE_PERMISSION_TITLE)
                    || self.text.is_some()
                    || !matches!(
                        self.status.as_deref(),
                        Some("allow_once" | "deny_once" | "completed" | "cancelled")
                    )
                {
                    return Err(invalid_history_record("unsafe permission presentation"));
                }
            }
            "error" => {
                if self.role.as_deref() != Some("system")
                    || self.title.as_deref() != Some("Agent error")
                    || self.text.as_deref() != Some(SAFE_REMOTE_AGENT_ERROR)
                    || self.status.as_deref() != Some("failed")
                {
                    return Err(invalid_history_record("unsafe error presentation"));
                }
            }
            _ => {}
        }
        Ok(())
    }

    fn validate_live_presentation(&self) -> Result<(), ProtocolError> {
        self.validate()?;
        validate_optional_content_text_bounded(
            "live timeline item text",
            self.text.as_deref(),
            MAX_LIVE_ITEM_PRESENTATION_BYTES,
        )
    }
}

/// Conservative JSON checkpoint charge shared with the native projection
/// owner. Fixed enum/numeric/object syntax is covered by the per-item
/// overhead; attacker-controlled strings are charged at their escaped size.
pub(crate) fn remote_live_projection_item_wire_bytes(
    item: &RemoteAgentTimelineItem,
) -> Result<usize, ProtocolError> {
    [
        Some(item.id.as_str()),
        item.title.as_deref(),
        item.text.as_deref(),
        item.status.as_deref(),
    ]
    .into_iter()
    .flatten()
    .try_fold(LIVE_PROJECTION_ITEM_OVERHEAD_BYTES, |bytes, value| {
        bytes
            .checked_add(json_string_wire_bytes(value)?)
            .ok_or_else(|| invalid_live_frame("live projection byte count overflow"))
    })
}

pub(crate) fn remote_live_projection_session_wire_bytes(
    session_id: &str,
) -> Result<usize, ProtocolError> {
    LIVE_PROJECTION_SESSION_OVERHEAD_BYTES
        .checked_add(json_string_wire_bytes(session_id)?)
        .ok_or_else(|| invalid_live_frame("live projection byte count overflow"))
}

/// Upper-bound a JSON string without allocating. ASCII controls may use the
/// six-byte `\u00XX` form; quotes and backslashes use two bytes; every other
/// scalar uses its UTF-8 width. The surrounding quote bytes are included.
fn json_string_wire_bytes(value: &str) -> Result<usize, ProtocolError> {
    value.chars().try_fold(2usize, |bytes, character| {
        let encoded = if character.is_ascii_control() {
            6
        } else if matches!(character, '"' | '\\') {
            2
        } else {
            character.len_utf8()
        };
        bytes
            .checked_add(encoded)
            .ok_or_else(|| invalid_live_frame("live projection byte count overflow"))
    })
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct RemoteAgentHistoryRecord {
    pub record_id: String,
    pub role: String,
    pub created_ms: u64,
    pub items: Vec<RemoteAgentTimelineItem>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct RemoteLiveEventCursor {
    pub journal_id: String,
    pub sequence: u64,
}

impl RemoteLiveEventCursor {
    pub fn validate(&self) -> Result<(), ProtocolError> {
        if self.journal_id.len() != 32
            || !self
                .journal_id
                .bytes()
                .all(|byte| byte.is_ascii_digit() || matches!(byte, b'a'..=b'f'))
        {
            return Err(ProtocolError::new(
                ErrorCode::InvalidPage,
                "live event cursor journal ID is invalid",
                false,
            ));
        }
        if self.sequence > MAX_JAVASCRIPT_SAFE_UNSIGNED_INTEGER {
            return Err(ProtocolError::new(
                ErrorCode::InvalidPage,
                "live event cursor sequence is outside the safe wire range",
                false,
            ));
        }
        Ok(())
    }
}

impl RemoteAgentHistoryRecord {
    pub fn validate(&self) -> Result<(), ProtocolError> {
        if !is_safe_cursor(&self.record_id) {
            return Err(invalid_history_record("invalid Agent history record id"));
        }
        // Goose owns its persisted Message role vocabulary. Maple renders the
        // safe projected items and treats this source-row label as opaque
        // metadata, so a future native role must not make local and remote
        // paging diverge. Keep the label bounded printable ASCII rather than
        // hard-coding today's user/assistant subset.
        if self.role.is_empty()
            || self.role.len() > MAX_ID_BYTES
            || !self
                .role
                .bytes()
                .all(|byte| byte.is_ascii_graphic() || byte == b' ')
        {
            return Err(invalid_history_record("invalid Agent history record role"));
        }
        if self.created_ms > MAX_JAVASCRIPT_SAFE_UNSIGNED_INTEGER {
            return Err(invalid_history_record(
                "Agent history record timestamp is outside the safe wire range",
            ));
        }
        if self.items.len() > MAX_HISTORY_ITEMS_PER_RECORD {
            return Err(invalid_history_record(
                "Agent history record contains too many timeline items",
            ));
        }
        for item in &self.items {
            item.validate()?;
        }
        if serialized_cbor_len(self)? > MAX_HISTORY_RECORD_PRESENTATION_BYTES {
            return Err(ProtocolError::new(
                ErrorCode::HistoryRecordTooLarge,
                "one Agent history record exceeds Maple's presentation limit",
                false,
            ));
        }
        Ok(())
    }
}

/// Multi-frame persisted-only Bulk response. A page is Start, exactly
/// `record_count` Record frames, then End. Synchronized live state has no
/// representation in this operation and can be disclosed only through the
/// separately authorized Events-lane attach protocol below.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "frame", rename_all = "snake_case", deny_unknown_fields)]
pub enum AgentHistoryPageFrame {
    Start {
        record_count: u16,
    },
    Record {
        index: u16,
        record: RemoteAgentHistoryRecord,
    },
    End {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        next_cursor: Option<String>,
        history_revision: String,
    },
}

impl AgentHistoryPageFrame {
    pub fn validate(&self) -> Result<(), ProtocolError> {
        match self {
            Self::Start { record_count } => {
                if *record_count > MAX_PAGE_SIZE {
                    return Err(ProtocolError::new(
                        ErrorCode::InvalidPage,
                        "history page contains too many records",
                        false,
                    ));
                }
            }
            Self::Record { index, record } => {
                if *index >= MAX_PAGE_SIZE {
                    return Err(ProtocolError::new(
                        ErrorCode::InvalidPage,
                        "history record index is out of range",
                        false,
                    ));
                }
                record.validate()?;
            }
            Self::End {
                next_cursor,
                history_revision,
            } => {
                validate_optional_cursor(next_cursor.as_deref())?;
                if !is_safe_cursor(history_revision) {
                    return Err(ProtocolError::new(
                        ErrorCode::InvalidPage,
                        "history revision is empty, unsafe, or too large",
                        false,
                    ));
                }
            }
        }
        Ok(())
    }
}

impl WireBody for AgentHistoryPageFrame {
    fn stream_kind(&self) -> StreamKind {
        StreamKind::Bulk
    }

    fn validate_body(&self) -> Result<(), ProtocolError> {
        self.validate()
    }
}

impl ResponseBody<ListAgentHistoryRecordsRequest> for AgentHistoryPageFrame {
    fn validate_response_to(
        &self,
        request: &ListAgentHistoryRecordsRequest,
    ) -> Result<(), ProtocolError> {
        request.validate()?;
        self.validate()?;
        match self {
            Self::Start { record_count, .. } if *record_count > request.limit => {
                Err(ProtocolError::new(
                    ErrorCode::InvalidPage,
                    "history page contains more records than requested",
                    false,
                ))
            }
            Self::Record { index, .. } if *index >= request.limit => Err(ProtocolError::new(
                ErrorCode::InvalidPage,
                "history record index exceeds the requested limit",
                false,
            )),
            Self::End { next_cursor, .. }
                if next_cursor.is_some() && next_cursor.as_ref() == request.cursor.as_ref() =>
            {
                Err(ProtocolError::new(
                    ErrorCode::InvalidPage,
                    "history continuation cursor did not advance",
                    false,
                ))
            }
            _ => Ok(()),
        }
    }
}

/// Fully assembled controller result. It is intentionally not a WireBody: its
/// records travel as individually bounded frames above.
#[derive(Debug, Clone, PartialEq)]
pub struct RemoteAgentHistoryPage {
    pub records: Vec<RemoteAgentHistoryRecord>,
    pub next_cursor: Option<String>,
    pub history_revision: String,
}

/// Remote synchronized-history operations are deliberately separate from
/// persisted-only paging. Begin and resume own long-lived Events streams;
/// activation and cancellation are correlated Control operations.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AgentLiveStreamOperation {
    BeginAttach,
    Resume,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct BeginAgentLiveAttachRequest {
    pub operation: AgentLiveStreamOperation,
    pub session_id: String,
    #[serde(default = "default_page_size")]
    pub limit: u16,
}

impl BeginAgentLiveAttachRequest {
    pub fn new(session_id: impl Into<String>, limit: u16) -> Result<Self, ProtocolError> {
        let request = Self {
            operation: AgentLiveStreamOperation::BeginAttach,
            session_id: session_id.into(),
            limit,
        };
        request.validate()?;
        Ok(request)
    }

    pub fn validate(&self) -> Result<(), ProtocolError> {
        if self.operation != AgentLiveStreamOperation::BeginAttach {
            return Err(invalid_live_frame("invalid live begin operation"));
        }
        validate_id("Agent session id", &self.session_id)?;
        validate_page_limit_and_cursor(self.limit, None)
    }
}

impl WireBody for BeginAgentLiveAttachRequest {
    fn stream_kind(&self) -> StreamKind {
        StreamKind::Events
    }

    fn validate_body(&self) -> Result<(), ProtocolError> {
        self.validate()
    }
}

impl RequestBody for BeginAgentLiveAttachRequest {
    fn allowed_direction(&self) -> PeerDirection {
        PeerDirection::ControllerToHost
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ResumeAgentLiveEventsRequest {
    pub operation: AgentLiveStreamOperation,
    pub cursor: RemoteLiveEventCursor,
    /// Host restart epoch which minted `cursor`. Reconnect generations may
    /// change within this epoch, but a cursor must never cross a host restart.
    pub origin_host_epoch: u64,
}

impl ResumeAgentLiveEventsRequest {
    pub fn new(
        cursor: RemoteLiveEventCursor,
        origin_host_epoch: u64,
    ) -> Result<Self, ProtocolError> {
        let request = Self {
            operation: AgentLiveStreamOperation::Resume,
            cursor,
            origin_host_epoch,
        };
        request.validate()?;
        Ok(request)
    }

    pub fn validate(&self) -> Result<(), ProtocolError> {
        if self.operation != AgentLiveStreamOperation::Resume {
            return Err(invalid_live_frame("invalid live resume operation"));
        }
        self.cursor.validate()?;
        if self.origin_host_epoch == 0 {
            return Err(invalid_live_frame(
                "live resume origin host epoch must be positive",
            ));
        }
        Ok(())
    }

    /// A live cursor is scoped to the host epoch which minted it. Reconnects
    /// within that epoch may advance the connection generation, but a host
    /// restart must force an authoritative head reload before any replay is
    /// attempted.
    pub fn validate_for_connection_stamp(
        &self,
        connection_stamp: ConnectionStamp,
    ) -> Result<(), ProtocolError> {
        self.validate()?;
        connection_stamp.validate()?;
        if self.origin_host_epoch != connection_stamp.host_epoch() {
            return Err(ProtocolError::new(
                ErrorCode::StaleGeneration,
                "live resume cursor belongs to another host epoch",
                true,
            ));
        }
        Ok(())
    }
}

impl WireBody for ResumeAgentLiveEventsRequest {
    fn stream_kind(&self) -> StreamKind {
        StreamKind::Events
    }

    fn validate_body(&self) -> Result<(), ProtocolError> {
        self.validate()
    }

    fn validate_body_for_stamp(
        &self,
        connection_stamp: ConnectionStamp,
    ) -> Result<(), ProtocolError> {
        self.validate_for_connection_stamp(connection_stamp)
    }
}

impl RequestBody for ResumeAgentLiveEventsRequest {
    fn allowed_direction(&self) -> PeerDirection {
        PeerDirection::ControllerToHost
    }
}

/// The Events lane has exactly one decoder. A central dispatcher accepts the
/// stream once and then routes by this closed operation union, preventing two
/// independent handlers from consuming each other's queued requests.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(
    tag = "operation",
    rename_all = "snake_case",
    rename_all_fields = "camelCase",
    deny_unknown_fields
)]
pub enum RemoteAgentLiveEventsRequest {
    BeginAttach {
        session_id: String,
        #[serde(default = "default_page_size")]
        limit: u16,
    },
    Resume {
        cursor: RemoteLiveEventCursor,
        origin_host_epoch: u64,
    },
}

impl From<BeginAgentLiveAttachRequest> for RemoteAgentLiveEventsRequest {
    fn from(request: BeginAgentLiveAttachRequest) -> Self {
        Self::BeginAttach {
            session_id: request.session_id,
            limit: request.limit,
        }
    }
}

impl From<ResumeAgentLiveEventsRequest> for RemoteAgentLiveEventsRequest {
    fn from(request: ResumeAgentLiveEventsRequest) -> Self {
        Self::Resume {
            cursor: request.cursor,
            origin_host_epoch: request.origin_host_epoch,
        }
    }
}

impl WireBody for RemoteAgentLiveEventsRequest {
    fn stream_kind(&self) -> StreamKind {
        StreamKind::Events
    }

    fn validate_body(&self) -> Result<(), ProtocolError> {
        match self {
            Self::BeginAttach { session_id, limit } => {
                BeginAgentLiveAttachRequest::new(session_id.clone(), *limit).map(|_| ())
            }
            Self::Resume {
                cursor,
                origin_host_epoch,
            } => ResumeAgentLiveEventsRequest::new(cursor.clone(), *origin_host_epoch).map(|_| ()),
        }
    }

    fn validate_body_for_stamp(
        &self,
        connection_stamp: ConnectionStamp,
    ) -> Result<(), ProtocolError> {
        match self {
            Self::BeginAttach { .. } => Ok(()),
            Self::Resume {
                cursor,
                origin_host_epoch,
            } => ResumeAgentLiveEventsRequest::new(cursor.clone(), *origin_host_epoch)?
                .validate_for_connection_stamp(connection_stamp),
        }
    }
}

impl RequestBody for RemoteAgentLiveEventsRequest {
    fn allowed_direction(&self) -> PeerDirection {
        PeerDirection::ControllerToHost
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AgentLiveControlOperation {
    ActivateAttach,
    Cancel,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ActivateAgentLiveAttachRequest {
    pub operation: AgentLiveControlOperation,
    pub attach_id: String,
}

impl ActivateAgentLiveAttachRequest {
    pub fn new(attach_id: impl Into<String>) -> Result<Self, ProtocolError> {
        let request = Self {
            operation: AgentLiveControlOperation::ActivateAttach,
            attach_id: attach_id.into(),
        };
        request.validate()?;
        Ok(request)
    }

    pub fn validate(&self) -> Result<(), ProtocolError> {
        if self.operation != AgentLiveControlOperation::ActivateAttach {
            return Err(invalid_live_frame("invalid live activation operation"));
        }
        validate_id("Agent live attachment id", &self.attach_id)
    }
}

impl WireBody for ActivateAgentLiveAttachRequest {
    fn stream_kind(&self) -> StreamKind {
        StreamKind::Control
    }

    fn validate_body(&self) -> Result<(), ProtocolError> {
        self.validate()
    }
}

impl RequestBody for ActivateAgentLiveAttachRequest {
    fn allowed_direction(&self) -> PeerDirection {
        PeerDirection::ControllerToHost
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AgentLiveCancelKind {
    PendingAttach,
    ActiveStream,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct CancelAgentLiveRequest {
    pub operation: AgentLiveControlOperation,
    pub kind: AgentLiveCancelKind,
    pub live_id: String,
}

impl CancelAgentLiveRequest {
    pub fn new(
        kind: AgentLiveCancelKind,
        live_id: impl Into<String>,
    ) -> Result<Self, ProtocolError> {
        let request = Self {
            operation: AgentLiveControlOperation::Cancel,
            kind,
            live_id: live_id.into(),
        };
        request.validate()?;
        Ok(request)
    }

    pub fn validate(&self) -> Result<(), ProtocolError> {
        if self.operation != AgentLiveControlOperation::Cancel {
            return Err(invalid_live_frame("invalid live cancellation operation"));
        }
        validate_id("Agent live lifecycle id", &self.live_id)
    }
}

impl WireBody for CancelAgentLiveRequest {
    fn stream_kind(&self) -> StreamKind {
        StreamKind::Control
    }

    fn validate_body(&self) -> Result<(), ProtocolError> {
        self.validate()
    }
}

impl RequestBody for CancelAgentLiveRequest {
    fn allowed_direction(&self) -> PeerDirection {
        PeerDirection::ControllerToHost
    }
}

/// The live Control subset has one explicitly tagged decoder. The peer-wide
/// Control union below additionally includes ordinary runtime status.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(
    tag = "operation",
    rename_all = "snake_case",
    rename_all_fields = "camelCase",
    deny_unknown_fields
)]
pub enum RemoteAgentLiveControlRequest {
    ActivateAttach {
        attach_id: String,
    },
    Cancel {
        kind: AgentLiveCancelKind,
        live_id: String,
    },
}

impl From<ActivateAgentLiveAttachRequest> for RemoteAgentLiveControlRequest {
    fn from(request: ActivateAgentLiveAttachRequest) -> Self {
        Self::ActivateAttach {
            attach_id: request.attach_id,
        }
    }
}

impl From<CancelAgentLiveRequest> for RemoteAgentLiveControlRequest {
    fn from(request: CancelAgentLiveRequest) -> Self {
        Self::Cancel {
            kind: request.kind,
            live_id: request.live_id,
        }
    }
}

impl WireBody for RemoteAgentLiveControlRequest {
    fn stream_kind(&self) -> StreamKind {
        StreamKind::Control
    }

    fn validate_body(&self) -> Result<(), ProtocolError> {
        match self {
            Self::ActivateAttach { attach_id } => {
                ActivateAgentLiveAttachRequest::new(attach_id.clone()).map(|_| ())
            }
            Self::Cancel { kind, live_id } => {
                CancelAgentLiveRequest::new(*kind, live_id.clone()).map(|_| ())
            }
        }
    }
}

impl RequestBody for RemoteAgentLiveControlRequest {
    fn allowed_direction(&self) -> PeerDirection {
        PeerDirection::ControllerToHost
    }
}

/// Closed peer-wide Control lane request set. Every host worker decodes this
/// union after accepting one authenticated Control stream, so runtime-status
/// and live lifecycle handlers cannot steal one another's requests.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(
    tag = "operation",
    rename_all = "snake_case",
    rename_all_fields = "camelCase",
    deny_unknown_fields
)]
pub enum RemoteAgentControlRequest {
    GetRuntimeStatus,
    ActivateAttach {
        attach_id: String,
    },
    Cancel {
        kind: AgentLiveCancelKind,
        live_id: String,
    },
}

impl WireBody for RemoteAgentControlRequest {
    fn stream_kind(&self) -> StreamKind {
        StreamKind::Control
    }

    fn validate_body(&self) -> Result<(), ProtocolError> {
        match self {
            Self::GetRuntimeStatus => GetRuntimeStatusRequest::new().validate_body(),
            Self::ActivateAttach { attach_id } => {
                ActivateAgentLiveAttachRequest::new(attach_id.clone()).map(|_| ())
            }
            Self::Cancel { kind, live_id } => {
                CancelAgentLiveRequest::new(*kind, live_id.clone()).map(|_| ())
            }
        }
    }
}

impl RequestBody for RemoteAgentControlRequest {
    fn allowed_direction(&self) -> PeerDirection {
        PeerDirection::ControllerToHost
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RemoteAgentLiveSnapshotReason {
    PausedSubscriberOverflow,
    SlowSubscriber,
    JournalReplaced,
    RetentionGap,
    CursorAhead,
    OwnerChanged,
    OrderingLost,
    JournalUnavailable,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RemoteAgentLiveRunTerminal {
    Completed,
    Cancelled,
    Failed,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RemoteAgentLiveClearReason {
    RunStarted,
    HistoryReplaced,
    ExplicitReload,
}

/// Closed v1 presentation set. It has no tool input/output, raw diagnostic,
/// provider JSON, prompt, credential, or actionable permission variant.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "eventType", rename_all = "camelCase", deny_unknown_fields)]
pub enum RemoteAgentPresentedLiveEvent {
    RunStarted,
    TimelineUpsert {
        item: RemoteAgentTimelineItem,
    },
    TimelineCleared {
        reason: RemoteAgentLiveClearReason,
    },
    HistoryReplaced,
    CursorAdvanced,
    SessionUpdated {
        session: RemoteAgentSessionSummary,
    },
    RunFinished {
        terminal: RemoteAgentLiveRunTerminal,
    },
    SessionDeleted,
    UserFacingError {
        item: RemoteAgentTimelineItem,
    },
}

impl RemoteAgentPresentedLiveEvent {
    fn validate(&self) -> Result<(), ProtocolError> {
        match self {
            // `RemoteAgentTimelineItem::validate` admits only the fixed,
            // terminal permission presentation set. Pending/actionable
            // controls therefore remain unrepresentable here.
            Self::TimelineUpsert { item } => item.validate_live_presentation(),
            Self::SessionUpdated { session } => session.validate(),
            Self::UserFacingError { item } => {
                item.validate_live_presentation()?;
                let safe_warning = item.item_type == "system"
                    && item.role.as_deref() == Some("system")
                    && item.title.as_deref() == Some("Agent warning")
                    && item.text.as_deref() == Some(SAFE_REMOTE_SETUP_WARNING)
                    && item.status.as_deref() == Some("warning")
                    && item.merge == "replace";
                let safe_error = item.item_type == "error";
                if safe_warning || safe_error {
                    Ok(())
                } else {
                    Err(invalid_live_frame("unsafe live error presentation"))
                }
            }
            Self::RunStarted
            | Self::TimelineCleared { .. }
            | Self::HistoryReplaced
            | Self::CursorAdvanced
            | Self::RunFinished { .. }
            | Self::SessionDeleted => Ok(()),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct RemoteAgentLiveDelivery {
    pub cursor: RemoteLiveEventCursor,
    pub session_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub run_id: Option<String>,
    pub event: RemoteAgentPresentedLiveEvent,
}

impl RemoteAgentLiveDelivery {
    pub fn validate(&self) -> Result<(), ProtocolError> {
        self.cursor.validate()?;
        validate_id("Agent live session id", &self.session_id)?;
        if let Some(run_id) = self.run_id.as_deref() {
            validate_id("Agent live run id", run_id)?;
        }
        self.event.validate()?;
        match &self.event {
            RemoteAgentPresentedLiveEvent::SessionUpdated { session }
                if session.id != self.session_id =>
            {
                Err(invalid_live_frame(
                    "live session update route is inconsistent",
                ))
            }
            RemoteAgentPresentedLiveEvent::RunStarted
            | RemoteAgentPresentedLiveEvent::RunFinished { .. }
            | RemoteAgentPresentedLiveEvent::HistoryReplaced
            | RemoteAgentPresentedLiveEvent::UserFacingError { .. }
                if self.run_id.is_none() =>
            {
                Err(invalid_live_frame("live run event is missing its run id"))
            }
            RemoteAgentPresentedLiveEvent::CursorAdvanced
            | RemoteAgentPresentedLiveEvent::SessionDeleted
                if self.run_id.is_some() =>
            {
                Err(invalid_live_frame(
                    "session event unexpectedly contains a run id",
                ))
            }
            RemoteAgentPresentedLiveEvent::TimelineCleared {
                reason:
                    RemoteAgentLiveClearReason::RunStarted | RemoteAgentLiveClearReason::HistoryReplaced,
            } if self.run_id.is_none() => Err(invalid_live_frame(
                "run-scoped live clear is missing its run id",
            )),
            RemoteAgentPresentedLiveEvent::TimelineCleared {
                reason: RemoteAgentLiveClearReason::ExplicitReload,
            } if self.run_id.is_some() => Err(invalid_live_frame(
                "session-scoped live clear unexpectedly contains a run id",
            )),
            _ => Ok(()),
        }
    }
}

/// One complete account-wide absolute live overlay entry captured at C0.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct RemoteAgentLiveSessionSnapshot {
    pub session_id: String,
    pub live_items: Vec<RemoteAgentTimelineItem>,
}

impl RemoteAgentLiveSessionSnapshot {
    pub fn validate(&self) -> Result<(), ProtocolError> {
        validate_id("Agent live snapshot session id", &self.session_id)?;
        if self.live_items.len() > MAX_LIVE_ITEMS_PER_SESSION {
            return Err(invalid_live_frame(
                "live session snapshot contains too many items",
            ));
        }
        let mut item_ids = BTreeSet::new();
        for item in &self.live_items {
            item.validate_live_presentation()?;
            if item.merge != "replace" || !item_ids.insert(item.id.as_str()) {
                return Err(invalid_live_frame(
                    "live session snapshot is not a unique absolute projection",
                ));
            }
        }
        Ok(())
    }
}

/// Ordered frames on a Begin/Resume Events stream. A Begin snapshot is fully
/// consumed before activation. Replay and later live events remain on this
/// same correlated response stream.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "frame", rename_all = "snake_case", deny_unknown_fields)]
pub enum AgentLiveStreamFrame {
    SnapshotStart {
        attach_id: String,
        record_count: u16,
        live_session_count: u16,
        live_sessions_complete: bool,
        through_event_cursor: RemoteLiveEventCursor,
    },
    HistoryRecord {
        index: u16,
        record: RemoteAgentHistoryRecord,
    },
    LiveSessionStart {
        index: u16,
        session_id: String,
        item_count: u16,
    },
    LiveSessionItem {
        session_index: u16,
        item_index: u16,
        item: RemoteAgentTimelineItem,
    },
    SnapshotEnd {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        next_cursor: Option<String>,
        history_revision: String,
    },
    StreamStart {
        live_stream_id: String,
        from_event_cursor: RemoteLiveEventCursor,
        through_event_cursor: RemoteLiveEventCursor,
    },
    Event {
        delivery: RemoteAgentLiveDelivery,
    },
    ReplayComplete {
        through_event_cursor: RemoteLiveEventCursor,
    },
    SnapshotRequired {
        reason: RemoteAgentLiveSnapshotReason,
        last_event_cursor: RemoteLiveEventCursor,
    },
}

impl AgentLiveStreamFrame {
    pub fn validate(&self) -> Result<(), ProtocolError> {
        match self {
            Self::SnapshotStart {
                attach_id,
                record_count,
                live_session_count,
                live_sessions_complete,
                through_event_cursor,
            } => {
                validate_id("Agent live attachment id", attach_id)?;
                if *record_count > MAX_PAGE_SIZE
                    || usize::from(*live_session_count) > MAX_LIVE_SESSIONS_PER_ACCOUNT
                    || !live_sessions_complete
                {
                    return Err(invalid_live_frame("invalid live snapshot header"));
                }
                through_event_cursor.validate()
            }
            Self::HistoryRecord { index, record } => {
                if *index >= MAX_PAGE_SIZE {
                    return Err(invalid_live_frame("live history record index is invalid"));
                }
                record.validate()
            }
            Self::LiveSessionStart {
                index,
                session_id,
                item_count,
            } => {
                if usize::from(*index) >= MAX_LIVE_SESSIONS_PER_ACCOUNT {
                    return Err(invalid_live_frame("live session index is invalid"));
                }
                validate_id("Agent live snapshot session id", session_id)?;
                if usize::from(*item_count) > MAX_LIVE_ITEMS_PER_SESSION {
                    return Err(invalid_live_frame(
                        "live session snapshot contains too many items",
                    ));
                }
                Ok(())
            }
            Self::LiveSessionItem {
                session_index,
                item_index,
                item,
            } => {
                if usize::from(*session_index) >= MAX_LIVE_SESSIONS_PER_ACCOUNT
                    || usize::from(*item_index) >= MAX_LIVE_ITEMS_PER_SESSION
                {
                    return Err(invalid_live_frame("live session item index is invalid"));
                }
                item.validate_live_presentation()?;
                if item.merge != "replace" {
                    return Err(invalid_live_frame(
                        "live session snapshot item is not an absolute projection",
                    ));
                }
                Ok(())
            }
            Self::SnapshotEnd {
                next_cursor,
                history_revision,
            } => {
                validate_optional_cursor(next_cursor.as_deref())?;
                if !is_safe_cursor(history_revision) {
                    return Err(invalid_live_frame("invalid live history revision"));
                }
                Ok(())
            }
            Self::StreamStart {
                live_stream_id,
                from_event_cursor,
                through_event_cursor,
            } => {
                validate_id("Agent live stream id", live_stream_id)?;
                validate_cursor_range(from_event_cursor, through_event_cursor)
            }
            Self::Event { delivery } => delivery.validate(),
            Self::ReplayComplete {
                through_event_cursor,
            } => through_event_cursor.validate(),
            Self::SnapshotRequired {
                last_event_cursor, ..
            } => last_event_cursor.validate(),
        }
    }
}

impl WireBody for AgentLiveStreamFrame {
    fn stream_kind(&self) -> StreamKind {
        StreamKind::Events
    }

    fn validate_body(&self) -> Result<(), ProtocolError> {
        self.validate()
    }
}

impl ResponseBody<BeginAgentLiveAttachRequest> for AgentLiveStreamFrame {
    fn validate_response_to(
        &self,
        request: &BeginAgentLiveAttachRequest,
    ) -> Result<(), ProtocolError> {
        request.validate()?;
        self.validate()?;
        match self {
            Self::SnapshotStart { record_count, .. } if *record_count > request.limit => Err(
                invalid_live_frame("live history snapshot exceeds the requested row limit"),
            ),
            Self::HistoryRecord { index, .. } if *index >= request.limit => Err(
                invalid_live_frame("live history row index exceeds the requested limit"),
            ),
            _ => Ok(()),
        }
    }
}

impl ResponseBody<ResumeAgentLiveEventsRequest> for AgentLiveStreamFrame {
    fn validate_response_to(
        &self,
        request: &ResumeAgentLiveEventsRequest,
    ) -> Result<(), ProtocolError> {
        request.validate()?;
        self.validate()?;
        match self {
            Self::SnapshotStart { .. }
            | Self::HistoryRecord { .. }
            | Self::LiveSessionStart { .. }
            | Self::LiveSessionItem { .. }
            | Self::SnapshotEnd { .. } => Err(invalid_live_frame(
                "resume stream cannot disclose a fresh history snapshot",
            )),
            Self::StreamStart {
                from_event_cursor, ..
            } if from_event_cursor != &request.cursor => Err(invalid_live_frame(
                "resume stream does not start at the requested cursor",
            )),
            _ => Ok(()),
        }
    }
}

impl ResponseBody<RemoteAgentLiveEventsRequest> for AgentLiveStreamFrame {
    fn validate_response_to(
        &self,
        request: &RemoteAgentLiveEventsRequest,
    ) -> Result<(), ProtocolError> {
        match request {
            RemoteAgentLiveEventsRequest::BeginAttach { session_id, limit } => {
                let request = BeginAgentLiveAttachRequest::new(session_id.clone(), *limit)?;
                <Self as ResponseBody<BeginAgentLiveAttachRequest>>::validate_response_to(
                    self, &request,
                )
            }
            RemoteAgentLiveEventsRequest::Resume {
                cursor,
                origin_host_epoch,
            } => {
                let request =
                    ResumeAgentLiveEventsRequest::new(cursor.clone(), *origin_host_epoch)?;
                <Self as ResponseBody<ResumeAgentLiveEventsRequest>>::validate_response_to(
                    self, &request,
                )
            }
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "disposition", rename_all = "snake_case", deny_unknown_fields)]
pub enum AgentLiveActivationDisposition {
    Activated {
        live_stream_id: String,
        through_event_cursor: RemoteLiveEventCursor,
    },
    SnapshotRequired {
        reason: RemoteAgentLiveSnapshotReason,
        last_event_cursor: RemoteLiveEventCursor,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ActivateAgentLiveAttachResponse {
    pub attach_id: String,
    pub result: AgentLiveActivationDisposition,
}

impl WireBody for ActivateAgentLiveAttachResponse {
    fn stream_kind(&self) -> StreamKind {
        StreamKind::Control
    }

    fn validate_body(&self) -> Result<(), ProtocolError> {
        validate_id("Agent live attachment id", &self.attach_id)?;
        match &self.result {
            AgentLiveActivationDisposition::Activated {
                live_stream_id,
                through_event_cursor,
            } => {
                validate_id("Agent live stream id", live_stream_id)?;
                through_event_cursor.validate()
            }
            AgentLiveActivationDisposition::SnapshotRequired {
                last_event_cursor, ..
            } => last_event_cursor.validate(),
        }
    }
}

impl ResponseBody<ActivateAgentLiveAttachRequest> for ActivateAgentLiveAttachResponse {
    fn validate_response_to(
        &self,
        request: &ActivateAgentLiveAttachRequest,
    ) -> Result<(), ProtocolError> {
        request.validate()?;
        self.validate_body()?;
        if self.attach_id == request.attach_id {
            Ok(())
        } else {
            Err(invalid_live_frame(
                "live activation response names another attachment",
            ))
        }
    }
}

impl ResponseBody<RemoteAgentLiveControlRequest> for ActivateAgentLiveAttachResponse {
    fn validate_response_to(
        &self,
        request: &RemoteAgentLiveControlRequest,
    ) -> Result<(), ProtocolError> {
        match request {
            RemoteAgentLiveControlRequest::ActivateAttach { attach_id } => {
                let request = ActivateAgentLiveAttachRequest::new(attach_id.clone())?;
                <Self as ResponseBody<ActivateAgentLiveAttachRequest>>::validate_response_to(
                    self, &request,
                )
            }
            RemoteAgentLiveControlRequest::Cancel { .. } => Err(invalid_live_frame(
                "live activation response was sent for a cancellation request",
            )),
        }
    }
}

impl ResponseBody<RemoteAgentControlRequest> for ActivateAgentLiveAttachResponse {
    fn validate_response_to(
        &self,
        request: &RemoteAgentControlRequest,
    ) -> Result<(), ProtocolError> {
        match request {
            RemoteAgentControlRequest::ActivateAttach { attach_id } => {
                let request = ActivateAgentLiveAttachRequest::new(attach_id.clone())?;
                <Self as ResponseBody<ActivateAgentLiveAttachRequest>>::validate_response_to(
                    self, &request,
                )
            }
            _ => Err(invalid_live_frame(
                "live activation response was sent for another Control operation",
            )),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct CancelAgentLiveResponse {
    pub kind: AgentLiveCancelKind,
    pub live_id: String,
}

#[derive(Debug, Clone, PartialEq)]
pub struct RemoteAgentLiveHeadSnapshot {
    pub attach_id: String,
    pub records: Vec<RemoteAgentHistoryRecord>,
    pub next_cursor: Option<String>,
    pub history_revision: String,
    pub live_sessions: Vec<RemoteAgentLiveSessionSnapshot>,
    pub through_event_cursor: RemoteLiveEventCursor,
    pub origin_host_epoch: u64,
}

#[derive(Debug, Clone, PartialEq)]
pub struct RemoteAgentLiveStreamStart {
    pub live_stream_id: String,
    pub from_event_cursor: RemoteLiveEventCursor,
    pub through_event_cursor: RemoteLiveEventCursor,
}

impl WireBody for CancelAgentLiveResponse {
    fn stream_kind(&self) -> StreamKind {
        StreamKind::Control
    }

    fn validate_body(&self) -> Result<(), ProtocolError> {
        validate_id("Agent live lifecycle id", &self.live_id)
    }
}

impl ResponseBody<CancelAgentLiveRequest> for CancelAgentLiveResponse {
    fn validate_response_to(&self, request: &CancelAgentLiveRequest) -> Result<(), ProtocolError> {
        request.validate()?;
        self.validate_body()?;
        if self.kind == request.kind && self.live_id == request.live_id {
            Ok(())
        } else {
            Err(invalid_live_frame(
                "live cancellation response does not match its request",
            ))
        }
    }
}

impl ResponseBody<RemoteAgentLiveControlRequest> for CancelAgentLiveResponse {
    fn validate_response_to(
        &self,
        request: &RemoteAgentLiveControlRequest,
    ) -> Result<(), ProtocolError> {
        match request {
            RemoteAgentLiveControlRequest::Cancel { kind, live_id } => {
                let request = CancelAgentLiveRequest::new(*kind, live_id.clone())?;
                <Self as ResponseBody<CancelAgentLiveRequest>>::validate_response_to(self, &request)
            }
            RemoteAgentLiveControlRequest::ActivateAttach { .. } => Err(invalid_live_frame(
                "live cancellation response was sent for an activation request",
            )),
        }
    }
}

impl ResponseBody<RemoteAgentControlRequest> for CancelAgentLiveResponse {
    fn validate_response_to(
        &self,
        request: &RemoteAgentControlRequest,
    ) -> Result<(), ProtocolError> {
        match request {
            RemoteAgentControlRequest::Cancel { kind, live_id } => {
                let request = CancelAgentLiveRequest::new(*kind, live_id.clone())?;
                <Self as ResponseBody<CancelAgentLiveRequest>>::validate_response_to(self, &request)
            }
            _ => Err(invalid_live_frame(
                "live cancellation response was sent for another Control operation",
            )),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AgentSessionListOperation {
    ListSessions,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ListAgentSessionsRequest {
    pub operation: AgentSessionListOperation,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub project_root: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cursor: Option<String>,
    #[serde(default = "default_page_size")]
    pub limit: u16,
}

impl ListAgentSessionsRequest {
    pub fn validate(&self) -> Result<(), ProtocolError> {
        match self.operation {
            AgentSessionListOperation::ListSessions => {}
        }
        validate_optional_status_field(
            "project root",
            self.project_root.as_deref(),
            MAX_PROJECT_ROOT_BYTES,
        )?;
        validate_page_limit_and_cursor(self.limit, self.cursor.as_deref())
    }
}

impl WireBody for ListAgentSessionsRequest {
    fn stream_kind(&self) -> StreamKind {
        StreamKind::Bulk
    }

    fn validate_body(&self) -> Result<(), ProtocolError> {
        self.validate()
    }
}

impl RequestBody for ListAgentSessionsRequest {
    fn allowed_direction(&self) -> PeerDirection {
        PeerDirection::ControllerToHost
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct RemoteAgentSessionSummary {
    pub id: String,
    pub title: String,
    pub project_root: String,
    pub created_ms: i64,
    pub updated_ms: i64,
    pub page_sort_ms: i64,
    pub message_count: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
    pub mode: String,
}

impl RemoteAgentSessionSummary {
    pub fn validate(&self) -> Result<(), ProtocolError> {
        validate_id("Agent session id", &self.id)?;
        validate_bounded_display_text(
            "Agent session title",
            &self.title,
            MAX_SESSION_TITLE_BYTES,
            false,
        )?;
        validate_bounded_display_text(
            "Agent project root",
            &self.project_root,
            MAX_PROJECT_ROOT_BYTES,
            false,
        )?;
        validate_optional_status_field("model", self.model.as_deref(), MAX_MODEL_ID_BYTES)?;
        validate_bounded_display_text("Agent mode", &self.mode, MAX_AGENT_MODE_BYTES, false)?;
        for (field, timestamp) in [
            ("created timestamp", self.created_ms),
            ("updated timestamp", self.updated_ms),
            ("page sort timestamp", self.page_sort_ms),
        ] {
            if !(0..=MAX_JAVASCRIPT_SAFE_INTEGER).contains(&timestamp) {
                return Err(ProtocolError::new(
                    ErrorCode::InvalidFrame,
                    format!("Agent session {field} is outside the safe wire range"),
                    false,
                ));
            }
        }
        if self.message_count > MAX_JAVASCRIPT_SAFE_UNSIGNED_INTEGER {
            return Err(ProtocolError::new(
                ErrorCode::InvalidFrame,
                "Agent session message count is outside the safe wire range",
                false,
            ));
        }
        Ok(())
    }
}

impl WireBody for RemoteAgentSessionSummary {
    fn stream_kind(&self) -> StreamKind {
        StreamKind::Bulk
    }

    fn validate_body(&self) -> Result<(), ProtocolError> {
        self.validate()
    }
}

impl PageItem for RemoteAgentSessionSummary {}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ListAgentSessionsResponse {
    pub items: Vec<RemoteAgentSessionSummary>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub next_cursor: Option<String>,
}

impl ListAgentSessionsResponse {
    pub fn validate_for(&self, request: &ListAgentSessionsRequest) -> Result<(), ProtocolError> {
        request.validate()?;
        if self.items.len() > usize::from(request.limit) {
            return Err(ProtocolError::new(
                ErrorCode::InvalidPage,
                "session page contains more items than requested",
                false,
            ));
        }
        if self.items.is_empty() && self.next_cursor.is_some() {
            return Err(ProtocolError::new(
                ErrorCode::InvalidPage,
                "empty session page cannot contain a continuation cursor",
                false,
            ));
        }
        if self.next_cursor.as_ref() == request.cursor.as_ref() && self.next_cursor.is_some() {
            return Err(ProtocolError::new(
                ErrorCode::InvalidPage,
                "session continuation cursor did not advance",
                false,
            ));
        }
        validate_optional_cursor(self.next_cursor.as_deref())?;
        for item in &self.items {
            item.validate()?;
        }
        Ok(())
    }
}

impl WireBody for ListAgentSessionsResponse {
    fn stream_kind(&self) -> StreamKind {
        StreamKind::Bulk
    }

    fn validate_body(&self) -> Result<(), ProtocolError> {
        if self.items.len() > usize::from(MAX_PAGE_SIZE) {
            return Err(ProtocolError::new(
                ErrorCode::InvalidPage,
                "session page contains too many items",
                false,
            ));
        }
        validate_optional_cursor(self.next_cursor.as_deref())?;
        for item in &self.items {
            item.validate()?;
        }
        Ok(())
    }
}

impl ResponseBody<ListAgentSessionsRequest> for ListAgentSessionsResponse {
    fn validate_response_to(
        &self,
        request: &ListAgentSessionsRequest,
    ) -> Result<(), ProtocolError> {
        self.validate_for(request)
    }
}

/// Closed peer-wide Bulk lane request set. History and task-list workers all
/// decode this union, preventing one Bulk operation from being consumed by a
/// handler for the other.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(
    tag = "operation",
    rename_all = "snake_case",
    rename_all_fields = "camelCase",
    deny_unknown_fields
)]
pub enum RemoteAgentBulkRequest {
    ListSessionRecords {
        session_id: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        cursor: Option<String>,
        #[serde(default = "default_page_size")]
        limit: u16,
    },
    ListSessions {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        project_root: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        cursor: Option<String>,
        #[serde(default = "default_page_size")]
        limit: u16,
    },
}

impl WireBody for RemoteAgentBulkRequest {
    fn stream_kind(&self) -> StreamKind {
        StreamKind::Bulk
    }

    fn validate_body(&self) -> Result<(), ProtocolError> {
        match self {
            Self::ListSessionRecords {
                session_id,
                cursor,
                limit,
            } => ListAgentHistoryRecordsRequest::new(session_id.clone(), cursor.clone(), *limit)
                .map(|_| ()),
            Self::ListSessions {
                project_root,
                cursor,
                limit,
            } => ListAgentSessionsRequest {
                operation: AgentSessionListOperation::ListSessions,
                project_root: project_root.clone(),
                cursor: cursor.clone(),
                limit: *limit,
            }
            .validate(),
        }
    }
}

impl RequestBody for RemoteAgentBulkRequest {
    fn allowed_direction(&self) -> PeerDirection {
        PeerDirection::ControllerToHost
    }
}

impl ResponseBody<RemoteAgentBulkRequest> for AgentHistoryPageFrame {
    fn validate_response_to(&self, request: &RemoteAgentBulkRequest) -> Result<(), ProtocolError> {
        match request {
            RemoteAgentBulkRequest::ListSessionRecords {
                session_id,
                cursor,
                limit,
            } => {
                let request = ListAgentHistoryRecordsRequest::new(
                    session_id.clone(),
                    cursor.clone(),
                    *limit,
                )?;
                <Self as ResponseBody<ListAgentHistoryRecordsRequest>>::validate_response_to(
                    self, &request,
                )
            }
            RemoteAgentBulkRequest::ListSessions { .. } => Err(ProtocolError::new(
                ErrorCode::InvalidFrame,
                "history page frame was sent for the task-list operation",
                false,
            )),
        }
    }
}

impl ResponseBody<RemoteAgentBulkRequest> for ListAgentSessionsResponse {
    fn validate_response_to(&self, request: &RemoteAgentBulkRequest) -> Result<(), ProtocolError> {
        match request {
            RemoteAgentBulkRequest::ListSessions {
                project_root,
                cursor,
                limit,
            } => {
                let request = ListAgentSessionsRequest {
                    operation: AgentSessionListOperation::ListSessions,
                    project_root: project_root.clone(),
                    cursor: cursor.clone(),
                    limit: *limit,
                };
                <Self as ResponseBody<ListAgentSessionsRequest>>::validate_response_to(
                    self, &request,
                )
            }
            RemoteAgentBulkRequest::ListSessionRecords { .. } => Err(ProtocolError::new(
                ErrorCode::InvalidFrame,
                "task-list response was sent for the history operation",
                false,
            )),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RequestEnvelope<T> {
    pub protocol_version: u16,
    pub request_id: String,
    pub execution_target_id: String,
    pub direction: PeerDirection,
    pub connection_stamp: ConnectionStamp,
    pub body: T,
}

impl<T: WireBody> RequestEnvelope<T> {
    pub fn validate(
        &self,
        expected_direction: PeerDirection,
        expected_execution_target: &str,
        expected_connection_stamp: ConnectionStamp,
        expected_stream_kind: StreamKind,
    ) -> Result<(), ProtocolError> {
        validate_version(self.protocol_version)?;
        validate_id("request_id", &self.request_id)?;
        validate_id("execution_target_id", &self.execution_target_id)?;
        if self.execution_target_id != expected_execution_target {
            return Err(ProtocolError::new(
                ErrorCode::WrongEndpoint,
                "request execution target does not match this host",
                false,
            ));
        }
        if self.direction != expected_direction {
            return Err(ProtocolError::new(
                ErrorCode::WrongDirection,
                "request direction is not allowed on this peer",
                false,
            ));
        }
        validate_connection_stamp(self.connection_stamp, expected_connection_stamp)?;
        validate_stream_kind(self.body.stream_kind(), expected_stream_kind)?;
        self.body.validate_body()?;
        self.body.validate_body_for_stamp(self.connection_stamp)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ResponseEnvelope<T> {
    pub protocol_version: u16,
    pub request_id: String,
    pub execution_target_id: String,
    pub connection_stamp: ConnectionStamp,
    pub result: Result<T, ProtocolError>,
}

impl<T: WireBody> ResponseEnvelope<T> {
    pub fn validate(
        &self,
        expected_request_id: &str,
        expected_execution_target: &str,
        expected_connection_stamp: ConnectionStamp,
        expected_stream_kind: StreamKind,
    ) -> Result<(), ProtocolError> {
        validate_version(self.protocol_version)?;
        validate_id("request_id", &self.request_id)?;
        validate_id("execution_target_id", &self.execution_target_id)?;
        if self.request_id != expected_request_id {
            return Err(ProtocolError::new(
                ErrorCode::InvalidFrame,
                "response request id does not match the outstanding request",
                false,
            ));
        }
        if self.execution_target_id != expected_execution_target {
            return Err(ProtocolError::new(
                ErrorCode::WrongEndpoint,
                "response execution target does not match the selected host",
                false,
            ));
        }
        validate_connection_stamp(self.connection_stamp, expected_connection_stamp)?;
        match &self.result {
            Ok(body) => {
                validate_stream_kind(body.stream_kind(), expected_stream_kind)?;
                body.validate_body()?;
                body.validate_body_for_stamp(self.connection_stamp)?;
            }
            // An error has no success body from which to derive a lane. Its
            // request id and the already-validated stream header bind it to
            // the outstanding operation instead.
            Err(error) => error.validate()?,
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct StreamHeader {
    pub protocol_version: u16,
    pub stream_kind: StreamKind,
    pub direction: PeerDirection,
    pub connection_stamp: ConnectionStamp,
}

impl StreamHeader {
    pub fn validate(&self, expected_direction: PeerDirection) -> Result<(), ProtocolError> {
        validate_version(self.protocol_version)?;
        self.connection_stamp.validate()?;
        if self.direction != expected_direction {
            return Err(ProtocolError::new(
                ErrorCode::WrongDirection,
                "stream direction is not allowed on this peer",
                false,
            ));
        }
        Ok(())
    }
}

/// Bounded paging foundation used by the transport harness. This is not a
/// product operation: session, timeline, and other resources must introduce
/// concrete resource-discriminated request/response pairs before going live.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PageRequest {
    #[serde(default)]
    pub cursor: Option<String>,
    #[serde(default = "default_page_size")]
    pub limit: u16,
}

impl Default for PageRequest {
    fn default() -> Self {
        Self {
            cursor: None,
            limit: DEFAULT_PAGE_SIZE,
        }
    }
}

impl PageRequest {
    pub fn validate(&self) -> Result<(), ProtocolError> {
        if self.limit == 0 || self.limit > MAX_PAGE_SIZE {
            return Err(ProtocolError::new(
                ErrorCode::InvalidPage,
                format!("page limit must be between 1 and {MAX_PAGE_SIZE}"),
                false,
            ));
        }
        if self
            .cursor
            .as_ref()
            .is_some_and(|cursor| !is_safe_cursor(cursor))
        {
            return Err(ProtocolError::new(
                ErrorCode::InvalidPage,
                "cursor is empty, unsafe, or too large",
                false,
            ));
        }
        Ok(())
    }
}

impl WireBody for PageRequest {
    fn stream_kind(&self) -> StreamKind {
        StreamKind::Bulk
    }

    fn validate_body(&self) -> Result<(), ProtocolError> {
        self.validate()
    }
}

impl RequestBody for PageRequest {
    fn allowed_direction(&self) -> PeerDirection {
        PeerDirection::ControllerToHost
    }
}

/// Generic bounded page used only with an explicitly reviewed [`PageItem`].
/// Real resources still require concrete, resource-discriminated operations;
/// bare `PageRequest` must not ambiguously route multiple page kinds.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, bound(deserialize = "T: Deserialize<'de>"))]
pub struct Page<T> {
    #[serde(deserialize_with = "deserialize_page_items")]
    pub items: Vec<T>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub next_cursor: Option<String>,
}

impl<T> Page<T> {
    pub fn validate(&self) -> Result<(), ProtocolError> {
        if self.items.len() > usize::from(MAX_PAGE_SIZE) {
            return Err(ProtocolError::new(
                ErrorCode::InvalidPage,
                "page contains too many items",
                false,
            ));
        }
        if self
            .next_cursor
            .as_ref()
            .is_some_and(|cursor| !is_safe_cursor(cursor))
        {
            return Err(ProtocolError::new(
                ErrorCode::InvalidPage,
                "next cursor is empty, unsafe, or too large",
                false,
            ));
        }
        Ok(())
    }

    /// Validate progress and sizing against the request that produced this
    /// page. Global wire bounds alone cannot prove that a continuation makes
    /// progress or that the host honored the caller's requested limit.
    pub fn validate_for_request(&self, request: &PageRequest) -> Result<(), ProtocolError> {
        request.validate()?;
        self.validate()?;
        if self.items.len() > usize::from(request.limit) {
            return Err(ProtocolError::new(
                ErrorCode::InvalidPage,
                "page contains more items than requested",
                false,
            ));
        }
        if self.items.is_empty() && self.next_cursor.is_some() {
            return Err(ProtocolError::new(
                ErrorCode::InvalidPage,
                "empty page cannot contain a continuation cursor",
                false,
            ));
        }
        if self.next_cursor.as_ref() == request.cursor.as_ref() && self.next_cursor.is_some() {
            return Err(ProtocolError::new(
                ErrorCode::InvalidPage,
                "page continuation cursor did not advance",
                false,
            ));
        }
        Ok(())
    }
}

fn deserialize_page_items<'de, D, T>(deserializer: D) -> Result<Vec<T>, D::Error>
where
    D: Deserializer<'de>,
    T: Deserialize<'de>,
{
    struct PageItemsVisitor<T>(PhantomData<fn() -> T>);

    impl<'de, T> Visitor<'de> for PageItemsVisitor<T>
    where
        T: Deserialize<'de>,
    {
        type Value = Vec<T>;

        fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
            write!(
                formatter,
                "a sequence containing at most {MAX_PAGE_SIZE} page items"
            )
        }

        fn visit_seq<A>(self, mut sequence: A) -> Result<Self::Value, A::Error>
        where
            A: SeqAccess<'de>,
        {
            let maximum = usize::from(MAX_PAGE_SIZE);
            if sequence.size_hint().is_some_and(|length| length > maximum) {
                return Err(de::Error::custom("page contains too many items"));
            }
            let mut items = Vec::with_capacity(sequence.size_hint().unwrap_or(0).min(maximum));
            while items.len() < maximum {
                match sequence.next_element()? {
                    Some(item) => items.push(item),
                    None => return Ok(items),
                }
            }
            // A sequence with no trustworthy size hint needs one non-allocating
            // look-ahead to distinguish exactly MAX_PAGE_SIZE items from an
            // oversized page. Never deserialize an extra `T` or grow the Vec.
            if sequence.next_element::<IgnoredAny>()?.is_some() {
                Err(de::Error::custom("page contains too many items"))
            } else {
                Ok(items)
            }
        }
    }

    deserializer.deserialize_seq(PageItemsVisitor(PhantomData))
}

impl<T: WireBody> WireBody for Page<T> {
    fn stream_kind(&self) -> StreamKind {
        StreamKind::Bulk
    }

    fn validate_body(&self) -> Result<(), ProtocolError> {
        self.validate()?;
        for item in &self.items {
            validate_stream_kind(item.stream_kind(), StreamKind::Bulk)?;
            item.validate_body()?;
        }
        Ok(())
    }

    fn validate_body_for_stamp(
        &self,
        connection_stamp: ConnectionStamp,
    ) -> Result<(), ProtocolError> {
        for item in &self.items {
            item.validate_body_for_stamp(connection_stamp)?;
        }
        Ok(())
    }
}

impl<T: PageItem> ResponseBody<PageRequest> for Page<T> {
    fn validate_response_to(&self, request: &PageRequest) -> Result<(), ProtocolError> {
        self.validate_for_request(request)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
pub enum ResumeRequest {
    Fresh,
    Resume {
        previous_connection_stamp: ConnectionStamp,
        last_received_event_sequence: u64,
    },
}

impl WireBody for ResumeRequest {
    fn stream_kind(&self) -> StreamKind {
        StreamKind::Control
    }

    fn validate_body(&self) -> Result<(), ProtocolError> {
        match self {
            Self::Fresh => Ok(()),
            Self::Resume {
                previous_connection_stamp,
                ..
            } => previous_connection_stamp.validate(),
        }
    }

    fn validate_body_for_stamp(
        &self,
        connection_stamp: ConnectionStamp,
    ) -> Result<(), ProtocolError> {
        if let Self::Resume {
            previous_connection_stamp,
            ..
        } = self
        {
            // A controller may have missed multiple generations or even a
            // whole host epoch while suspended. A strictly older stamp remains
            // a valid resume request; the host can answer SnapshotRequired if
            // the referenced event window no longer exists.
            if *previous_connection_stamp >= connection_stamp {
                return Err(ProtocolError::new(
                    ErrorCode::StaleGeneration,
                    "resume source stamp must precede the current connection",
                    true,
                ));
            }
        }
        Ok(())
    }
}

impl RequestBody for ResumeRequest {
    fn allowed_direction(&self) -> PeerDirection {
        PeerDirection::ControllerToHost
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ResumeDisposition {
    Fresh,
    Resumed,
    SnapshotRequired,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ResumeResponse {
    pub connection_stamp: ConnectionStamp,
    pub disposition: ResumeDisposition,
    pub first_available_event_sequence: u64,
}

impl WireBody for ResumeResponse {
    fn stream_kind(&self) -> StreamKind {
        StreamKind::Control
    }

    fn validate_body(&self) -> Result<(), ProtocolError> {
        self.connection_stamp.validate()
    }

    fn validate_body_for_stamp(
        &self,
        connection_stamp: ConnectionStamp,
    ) -> Result<(), ProtocolError> {
        validate_connection_stamp(self.connection_stamp, connection_stamp)
    }
}

impl ResponseBody<ResumeRequest> for ResumeResponse {
    fn validate_response_to(&self, request: &ResumeRequest) -> Result<(), ProtocolError> {
        match (request, &self.disposition) {
            (ResumeRequest::Fresh, ResumeDisposition::Fresh)
            | (ResumeRequest::Resume { .. }, ResumeDisposition::SnapshotRequired) => Ok(()),
            (
                ResumeRequest::Resume {
                    last_received_event_sequence,
                    ..
                },
                ResumeDisposition::Resumed,
            ) => {
                let next_expected = last_received_event_sequence
                    .checked_add(1)
                    .unwrap_or(u64::MAX);
                if self.first_available_event_sequence <= next_expected {
                    Ok(())
                } else {
                    Err(ProtocolError::new(
                        ErrorCode::InvalidFrame,
                        "resumed response would skip unavailable events",
                        false,
                    ))
                }
            }
            _ => Err(ProtocolError::new(
                ErrorCode::InvalidFrame,
                "resume response disposition does not match its request",
                false,
            )),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ErrorCode {
    InvalidFrame,
    FrameTooLarge,
    UnsupportedVersion,
    WrongEndpoint,
    WrongDirection,
    Unauthorized,
    Revoked,
    InvalidPage,
    HistoryRecordTooLarge,
    StaleHistory,
    AgentLiveUnavailable,
    SnapshotRequired,
    StaleGeneration,
    SecureStorageUnavailable,
    TransportUnavailable,
    Internal,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProtocolError {
    pub code: ErrorCode,
    pub message: String,
    pub retryable: bool,
}

impl ProtocolError {
    pub fn new(code: ErrorCode, message: impl Into<String>, retryable: bool) -> Self {
        let mut message = sanitize_error_message(&message.into());
        if message.is_empty() {
            message = "remote protocol error".into();
        }
        truncate_utf8(&mut message, MAX_ERROR_MESSAGE_BYTES);
        Self {
            code,
            message,
            retryable,
        }
    }

    /// Validate a structured error received from the wire. `ErrorCode` and
    /// `retryable` are fixed-size serde values; the human-readable message is
    /// the only variable-size field.
    pub fn validate(&self) -> Result<(), ProtocolError> {
        if self.message.is_empty()
            || self.message.len() > MAX_ERROR_MESSAGE_BYTES
            || self.message != sanitize_error_message(&self.message)
        {
            Err(ProtocolError::new(
                ErrorCode::InvalidFrame,
                "protocol error message is empty or too large",
                false,
            ))
        } else {
            Ok(())
        }
    }
}

impl std::fmt::Display for ProtocolError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(formatter, "{:?}: {}", self.code, self.message)
    }
}

impl std::error::Error for ProtocolError {}

pub fn validate_version(version: u16) -> Result<(), ProtocolError> {
    if version == PROTOCOL_VERSION {
        Ok(())
    } else {
        Err(ProtocolError::new(
            ErrorCode::UnsupportedVersion,
            format!("unsupported protocol version {version}"),
            false,
        ))
    }
}

pub fn validate_frame_len(len: usize) -> Result<(), ProtocolError> {
    if len <= MAX_FRAME_BYTES as usize {
        Ok(())
    } else {
        Err(ProtocolError::new(
            ErrorCode::FrameTooLarge,
            format!("frame exceeds {MAX_FRAME_BYTES} bytes"),
            false,
        ))
    }
}

fn validate_connection_stamp(
    actual: ConnectionStamp,
    expected: ConnectionStamp,
) -> Result<(), ProtocolError> {
    actual.validate()?;
    expected.validate()?;
    if actual == expected {
        Ok(())
    } else {
        Err(ProtocolError::new(
            ErrorCode::StaleGeneration,
            "message belongs to a stale connection stamp",
            true,
        ))
    }
}

fn validate_stream_kind(actual: StreamKind, expected: StreamKind) -> Result<(), ProtocolError> {
    if actual == expected {
        Ok(())
    } else {
        Err(ProtocolError::new(
            ErrorCode::InvalidFrame,
            "message body is not allowed on this stream kind",
            false,
        ))
    }
}

fn validate_id(field: &str, value: &str) -> Result<(), ProtocolError> {
    if !value.is_empty()
        && value.len() <= MAX_ID_BYTES
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.' | b':'))
    {
        Ok(())
    } else {
        Err(ProtocolError::new(
            ErrorCode::InvalidFrame,
            format!("invalid {field}"),
            false,
        ))
    }
}

fn validate_page_limit_and_cursor(limit: u16, cursor: Option<&str>) -> Result<(), ProtocolError> {
    if limit == 0 || limit > MAX_PAGE_SIZE {
        return Err(ProtocolError::new(
            ErrorCode::InvalidPage,
            format!("page limit must be between 1 and {MAX_PAGE_SIZE}"),
            false,
        ));
    }
    validate_optional_cursor(cursor)
}

fn validate_optional_cursor(cursor: Option<&str>) -> Result<(), ProtocolError> {
    if cursor.is_some_and(|cursor| !is_safe_cursor(cursor)) {
        Err(ProtocolError::new(
            ErrorCode::InvalidPage,
            "cursor is empty, unsafe, or too large",
            false,
        ))
    } else {
        Ok(())
    }
}

fn validate_bounded_display_text(
    field: &str,
    value: &str,
    max_bytes: usize,
    allow_empty: bool,
) -> Result<(), ProtocolError> {
    if (allow_empty || !value.is_empty())
        && value.len() <= max_bytes
        && !value
            .chars()
            .any(|character| character.is_control() || is_bidi_control(character))
    {
        Ok(())
    } else {
        Err(invalid_history_record(format!("invalid {field}")))
    }
}

fn validate_optional_history_label(
    field: &str,
    value: Option<&str>,
    max_bytes: usize,
) -> Result<(), ProtocolError> {
    value.map_or(Ok(()), |value| {
        validate_bounded_display_text(field, value, max_bytes, true)
    })
}

fn validate_optional_content_text(field: &str, value: Option<&str>) -> Result<(), ProtocolError> {
    validate_optional_content_text_bounded(field, value, MAX_FRAME_BYTES as usize)
}

fn validate_optional_content_text_bounded(
    field: &str,
    value: Option<&str>,
    max_bytes: usize,
) -> Result<(), ProtocolError> {
    let Some(value) = value else {
        return Ok(());
    };
    if value.len() <= max_bytes && !value.contains('\0') {
        Ok(())
    } else {
        Err(ProtocolError::new(
            ErrorCode::HistoryRecordTooLarge,
            format!("{field} exceeds Maple's history record limit"),
            false,
        ))
    }
}

fn invalid_history_record(message: impl Into<String>) -> ProtocolError {
    ProtocolError::new(ErrorCode::InvalidFrame, message, false)
}

fn invalid_live_frame(message: impl Into<String>) -> ProtocolError {
    ProtocolError::new(ErrorCode::InvalidFrame, message, false)
}

fn validate_cursor_range(
    from: &RemoteLiveEventCursor,
    through: &RemoteLiveEventCursor,
) -> Result<(), ProtocolError> {
    from.validate()?;
    through.validate()?;
    if from.journal_id != through.journal_id || from.sequence > through.sequence {
        Err(ProtocolError::new(
            ErrorCode::SnapshotRequired,
            "live event cursor range requires an authoritative snapshot",
            true,
        ))
    } else {
        Ok(())
    }
}

fn serialized_cbor_len<T: Serialize>(value: &T) -> Result<usize, ProtocolError> {
    #[derive(Default)]
    struct Counter(usize);

    impl std::io::Write for Counter {
        fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
            self.0 = self
                .0
                .checked_add(bytes.len())
                .ok_or_else(|| std::io::Error::other("CBOR length overflow"))?;
            Ok(bytes.len())
        }

        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }

    let mut counter = Counter::default();
    ciborium::ser::into_writer(value, &mut counter).map_err(|_| {
        ProtocolError::new(
            ErrorCode::InvalidFrame,
            "failed to measure Agent history record",
            false,
        )
    })?;
    Ok(counter.0)
}

fn validate_optional_status_field(
    field: &str,
    value: Option<&str>,
    max_bytes: usize,
) -> Result<(), ProtocolError> {
    let Some(value) = value else {
        return Ok(());
    };
    if !value.is_empty()
        && value.len() <= max_bytes
        && !value
            .chars()
            .any(|character| character.is_control() || is_bidi_control(character))
    {
        Ok(())
    } else {
        Err(ProtocolError::new(
            ErrorCode::InvalidFrame,
            format!("invalid runtime status {field}"),
            false,
        ))
    }
}

fn is_safe_cursor(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= MAX_CURSOR_BYTES
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.' | b':'))
}

fn truncate_utf8(value: &mut String, max_bytes: usize) {
    if value.len() <= max_bytes {
        return;
    }
    let mut end = max_bytes;
    while !value.is_char_boundary(end) {
        end -= 1;
    }
    value.truncate(end);
}

fn sanitize_error_message(value: &str) -> String {
    value
        .chars()
        .map(|character| {
            if character.is_control()
                || is_bidi_control(character)
                || matches!(character, '\u{2028}' | '\u{2029}')
            {
                ' '
            } else {
                character
            }
        })
        .collect::<String>()
        .trim()
        .to_owned()
}

fn is_bidi_control(character: char) -> bool {
    matches!(
        character,
        '\u{061c}'
            | '\u{200e}'
            | '\u{200f}'
            | '\u{202a}'..='\u{202e}'
            | '\u{2066}'..='\u{2069}'
    )
}

const fn default_page_size() -> u16 {
    DEFAULT_PAGE_SIZE
}

#[cfg(test)]
mod tests {
    use super::*;

    fn stamp(host_epoch: u64, generation: u64) -> ConnectionStamp {
        ConnectionStamp::new(host_epoch, generation).unwrap()
    }

    #[test]
    fn stream_priorities_keep_control_ahead_of_events_and_bulk() {
        assert!(StreamKind::Control.priority() > StreamKind::Events.priority());
        assert!(StreamKind::Events.priority() > StreamKind::Bulk.priority());
    }

    #[test]
    fn request_operations_are_explicitly_controller_to_host() {
        assert_eq!(
            GetRuntimeStatusRequest::new().allowed_direction(),
            PeerDirection::ControllerToHost
        );
        assert_eq!(
            GetRuntimeStatusRequest::new().stream_kind(),
            StreamKind::Control
        );
        assert_eq!(
            PageRequest::default().allowed_direction(),
            PeerDirection::ControllerToHost
        );
        assert_eq!(
            ResumeRequest::Fresh.allowed_direction(),
            PeerDirection::ControllerToHost
        );
    }

    #[test]
    fn runtime_status_wire_body_is_bounded_and_state_consistent() {
        let running = RemoteAgentRuntimeStatus {
            running: true,
            project_root: Some("/tmp/maple-project".into()),
            model: Some("glm-5-2".into()),
            mode: Some("smart_approve".into()),
            active_runs: BTreeMap::from([("session-01".into(), "run-01".into())]),
        };
        GetRuntimeStatusResponse::new(running.clone()).expect("valid running status");

        let mut inconsistent = running.clone();
        inconsistent.model = None;
        assert_eq!(
            inconsistent.validate().unwrap_err().code,
            ErrorCode::InvalidFrame
        );

        let stopped_with_run = RemoteAgentRuntimeStatus {
            running: false,
            project_root: None,
            model: None,
            mode: None,
            active_runs: BTreeMap::from([("session-01".into(), "run-01".into())]),
        };
        assert_eq!(
            stopped_with_run.validate().unwrap_err().code,
            ErrorCode::InvalidFrame
        );

        let stopped_with_one_field = RemoteAgentRuntimeStatus {
            running: false,
            project_root: Some("/tmp/stale-project".into()),
            model: None,
            mode: None,
            active_runs: BTreeMap::new(),
        };
        assert_eq!(
            stopped_with_one_field.validate().unwrap_err().code,
            ErrorCode::InvalidFrame
        );

        let stopped_with_two_fields = RemoteAgentRuntimeStatus {
            running: false,
            project_root: Some("/tmp/stale-project".into()),
            model: Some("stale-model".into()),
            mode: None,
            active_runs: BTreeMap::new(),
        };
        assert_eq!(
            stopped_with_two_fields.validate().unwrap_err().code,
            ErrorCode::InvalidFrame
        );

        let mut oversized = running;
        oversized.project_root = Some("p".repeat(MAX_PROJECT_ROOT_BYTES + 1));
        assert_eq!(
            oversized.validate().unwrap_err().code,
            ErrorCode::InvalidFrame
        );
    }

    #[test]
    fn runtime_status_request_does_not_accept_a_command_name() {
        let value = serde_json::json!({
            "operation": "agent_clear_user_data",
            "command": "agent_clear_user_data"
        });
        assert!(serde_json::from_value::<GetRuntimeStatusRequest>(value).is_err());
    }

    const fn invalid_stamp(host_epoch: u64, generation: u64) -> ConnectionStamp {
        ConnectionStamp {
            host_epoch,
            generation,
        }
    }

    #[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
    #[serde(deny_unknown_fields)]
    struct BoundedTestItem {
        value: String,
    }

    #[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
    struct ContextOnlyTestItem {
        base_is_valid: bool,
    }

    #[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
    struct NonBulkPageItem;

    impl BoundedTestItem {
        const MAX_VALUE_BYTES: usize = 8;
    }

    impl WireBody for BoundedTestItem {
        fn stream_kind(&self) -> StreamKind {
            StreamKind::Bulk
        }

        fn validate_body(&self) -> Result<(), ProtocolError> {
            if !self.value.is_empty() && self.value.len() <= Self::MAX_VALUE_BYTES {
                Ok(())
            } else {
                Err(ProtocolError::new(
                    ErrorCode::InvalidFrame,
                    "test item value is empty or too large",
                    false,
                ))
            }
        }
    }

    impl PageItem for BoundedTestItem {}

    impl WireBody for NonBulkPageItem {
        fn stream_kind(&self) -> StreamKind {
            StreamKind::Control
        }

        fn validate_body(&self) -> Result<(), ProtocolError> {
            Ok(())
        }
    }

    impl PageItem for NonBulkPageItem {}

    impl WireBody for ContextOnlyTestItem {
        fn stream_kind(&self) -> StreamKind {
            StreamKind::Control
        }

        fn validate_body(&self) -> Result<(), ProtocolError> {
            if self.base_is_valid {
                Ok(())
            } else {
                Err(ProtocolError::new(
                    ErrorCode::InvalidFrame,
                    "base validation was enforced",
                    false,
                ))
            }
        }

        fn validate_body_for_stamp(
            &self,
            _connection_stamp: ConnectionStamp,
        ) -> Result<(), ProtocolError> {
            // Deliberately does not call validate_body. Envelope validation must
            // enforce the base invariant independently of this override.
            Ok(())
        }
    }

    #[test]
    fn request_requires_current_version_and_direction() {
        let mut request = RequestEnvelope {
            protocol_version: PROTOCOL_VERSION,
            request_id: "request-01".into(),
            execution_target_id: "macbook-pro".into(),
            direction: PeerDirection::ControllerToHost,
            connection_stamp: stamp(2, 9),
            body: PageRequest::default(),
        };
        request
            .validate(
                PeerDirection::ControllerToHost,
                "macbook-pro",
                stamp(2, 9),
                StreamKind::Bulk,
            )
            .expect("valid request");
        assert_eq!(
            request
                .validate(
                    PeerDirection::ControllerToHost,
                    "macbook-pro",
                    stamp(2, 9),
                    StreamKind::Control,
                )
                .unwrap_err()
                .code,
            ErrorCode::InvalidFrame
        );

        request.protocol_version += 1;
        assert_eq!(
            request
                .validate(
                    PeerDirection::ControllerToHost,
                    "macbook-pro",
                    stamp(2, 9),
                    StreamKind::Bulk,
                )
                .unwrap_err()
                .code,
            ErrorCode::UnsupportedVersion
        );
        request.protocol_version = PROTOCOL_VERSION;
        assert_eq!(
            request
                .validate(
                    PeerDirection::HostToController,
                    "macbook-pro",
                    stamp(2, 9),
                    StreamKind::Bulk,
                )
                .unwrap_err()
                .code,
            ErrorCode::WrongDirection
        );
        request.direction = PeerDirection::ControllerToHost;
        assert_eq!(
            request
                .validate(
                    PeerDirection::ControllerToHost,
                    "other-host",
                    stamp(2, 9),
                    StreamKind::Bulk,
                )
                .unwrap_err()
                .code,
            ErrorCode::WrongEndpoint
        );
        assert_eq!(
            request
                .validate(
                    PeerDirection::ControllerToHost,
                    "macbook-pro",
                    stamp(2, 10),
                    StreamKind::Bulk,
                )
                .unwrap_err()
                .code,
            ErrorCode::StaleGeneration
        );
    }

    #[test]
    fn contextual_validation_cannot_bypass_base_body_validation() {
        let current = stamp(3, 7);
        let request = RequestEnvelope {
            protocol_version: PROTOCOL_VERSION,
            request_id: "request-contextual".into(),
            execution_target_id: "host-01".into(),
            direction: PeerDirection::ControllerToHost,
            connection_stamp: current,
            body: ContextOnlyTestItem {
                base_is_valid: false,
            },
        };
        assert_eq!(
            request
                .validate(
                    PeerDirection::ControllerToHost,
                    "host-01",
                    current,
                    StreamKind::Control,
                )
                .unwrap_err()
                .code,
            ErrorCode::InvalidFrame
        );

        let response = ResponseEnvelope {
            protocol_version: PROTOCOL_VERSION,
            request_id: "request-contextual".into(),
            execution_target_id: "host-01".into(),
            connection_stamp: current,
            result: Ok(ContextOnlyTestItem {
                base_is_valid: false,
            }),
        };
        assert_eq!(
            response
                .validate(
                    "request-contextual",
                    "host-01",
                    current,
                    StreamKind::Control,
                )
                .unwrap_err()
                .code,
            ErrorCode::InvalidFrame
        );

        let page = Page {
            items: vec![ContextOnlyTestItem {
                base_is_valid: false,
            }],
            next_cursor: None,
        };
        assert_eq!(
            page.validate_body().unwrap_err().code,
            ErrorCode::InvalidFrame
        );
    }

    #[test]
    fn envelope_connection_stamp_rejects_zero_epoch_or_generation() {
        assert_eq!(
            ConnectionStamp::new(0, 1).unwrap_err().code,
            ErrorCode::StaleGeneration
        );
        assert_eq!(
            ConnectionStamp::new(1, 0).unwrap_err().code,
            ErrorCode::StaleGeneration
        );
        for connection_stamp in [invalid_stamp(0, 1), invalid_stamp(1, 0)] {
            let request = RequestEnvelope {
                protocol_version: PROTOCOL_VERSION,
                request_id: "request-01".into(),
                execution_target_id: "host-01".into(),
                direction: PeerDirection::ControllerToHost,
                connection_stamp,
                body: PageRequest::default(),
            };
            assert_eq!(
                request
                    .validate(
                        PeerDirection::ControllerToHost,
                        "host-01",
                        stamp(1, 1),
                        StreamKind::Bulk,
                    )
                    .unwrap_err()
                    .code,
                ErrorCode::StaleGeneration
            );
        }
    }

    #[test]
    fn page_request_and_response_validation_are_bound_to_request_context() {
        let request = RequestEnvelope {
            protocol_version: PROTOCOL_VERSION,
            request_id: "request-01".into(),
            execution_target_id: "host-01".into(),
            direction: PeerDirection::ControllerToHost,
            connection_stamp: stamp(1, 3),
            body: PageRequest {
                cursor: None,
                limit: MAX_PAGE_SIZE + 1,
            },
        };
        assert_eq!(
            request
                .validate(
                    PeerDirection::ControllerToHost,
                    "host-01",
                    stamp(1, 3),
                    StreamKind::Bulk,
                )
                .unwrap_err()
                .code,
            ErrorCode::InvalidPage
        );

        let response: ResponseEnvelope<Page<BoundedTestItem>> = ResponseEnvelope {
            protocol_version: PROTOCOL_VERSION,
            request_id: "request-01".into(),
            execution_target_id: "host-01".into(),
            connection_stamp: stamp(1, 3),
            result: Ok(Page {
                items: vec![BoundedTestItem {
                    value: "bounded".into(),
                }],
                next_cursor: None,
            }),
        };
        response
            .validate("request-01", "host-01", stamp(1, 3), StreamKind::Bulk)
            .unwrap();
        assert_eq!(
            response
                .validate("request-01", "host-01", stamp(1, 3), StreamKind::Control,)
                .unwrap_err()
                .code,
            ErrorCode::InvalidFrame
        );
        assert_eq!(
            response
                .validate("other-request", "host-01", stamp(1, 3), StreamKind::Bulk)
                .unwrap_err()
                .code,
            ErrorCode::InvalidFrame
        );
        assert_eq!(
            response
                .validate("request-01", "other-host", stamp(1, 3), StreamKind::Bulk)
                .unwrap_err()
                .code,
            ErrorCode::WrongEndpoint
        );
        assert_eq!(
            response
                .validate("request-01", "host-01", stamp(1, 4), StreamKind::Bulk)
                .unwrap_err()
                .code,
            ErrorCode::StaleGeneration
        );

        let invalid_item_response = ResponseEnvelope {
            protocol_version: PROTOCOL_VERSION,
            request_id: "request-01".into(),
            execution_target_id: "host-01".into(),
            connection_stamp: stamp(1, 3),
            result: Ok(Page {
                items: vec![BoundedTestItem {
                    value: "too-large".into(),
                }],
                next_cursor: None,
            }),
        };
        assert_eq!(
            invalid_item_response
                .validate("request-01", "host-01", stamp(1, 3), StreamKind::Bulk)
                .unwrap_err()
                .code,
            ErrorCode::InvalidFrame
        );

        let non_bulk_item_response = ResponseEnvelope {
            protocol_version: PROTOCOL_VERSION,
            request_id: "request-01".into(),
            execution_target_id: "host-01".into(),
            connection_stamp: stamp(1, 3),
            result: Ok(Page {
                items: vec![NonBulkPageItem],
                next_cursor: None,
            }),
        };
        assert_eq!(
            non_bulk_item_response
                .validate("request-01", "host-01", stamp(1, 3), StreamKind::Bulk)
                .unwrap_err()
                .code,
            ErrorCode::InvalidFrame
        );

        let page_request = PageRequest {
            cursor: Some("cursor-1".into()),
            limit: 2,
        };
        let valid_page = Page {
            items: vec![
                BoundedTestItem {
                    value: "one".into(),
                },
                BoundedTestItem {
                    value: "two".into(),
                },
            ],
            next_cursor: Some("cursor-2".into()),
        };
        ResponseBody::<PageRequest>::validate_response_to(&valid_page, &page_request).unwrap();
        assert_eq!(
            Page {
                items: vec![
                    BoundedTestItem {
                        value: "one".into(),
                    },
                    BoundedTestItem {
                        value: "two".into(),
                    },
                    BoundedTestItem {
                        value: "three".into(),
                    },
                ],
                next_cursor: None,
            }
            .validate_for_request(&page_request)
            .unwrap_err()
            .code,
            ErrorCode::InvalidPage
        );
        assert_eq!(
            Page::<BoundedTestItem> {
                items: Vec::new(),
                next_cursor: Some("cursor-2".into()),
            }
            .validate_for_request(&page_request)
            .unwrap_err()
            .code,
            ErrorCode::InvalidPage
        );
        assert_eq!(
            Page {
                items: vec![BoundedTestItem {
                    value: "one".into(),
                }],
                next_cursor: Some("cursor-1".into()),
            }
            .validate_for_request(&page_request)
            .unwrap_err()
            .code,
            ErrorCode::InvalidPage
        );
        Page::<BoundedTestItem> {
            items: Vec::new(),
            next_cursor: None,
        }
        .validate_for_request(&page_request)
        .unwrap();
    }

    #[test]
    fn page_cursors_reject_log_control_characters() {
        for cursor in ["line\nbreak", "terminal\u{1b}escape", "space cursor"] {
            assert_eq!(
                PageRequest {
                    cursor: Some(cursor.into()),
                    limit: 1,
                }
                .validate()
                .unwrap_err()
                .code,
                ErrorCode::InvalidPage
            );
            assert_eq!(
                Page::<BoundedTestItem> {
                    items: Vec::new(),
                    next_cursor: Some(cursor.into()),
                }
                .validate()
                .unwrap_err()
                .code,
                ErrorCode::InvalidPage
            );
        }
        PageRequest {
            cursor: Some("base64url-safe:value_01.test".into()),
            limit: 1,
        }
        .validate()
        .unwrap();
    }

    #[test]
    fn page_item_limit_is_enforced_during_deserialization() {
        use std::sync::atomic::{AtomicUsize, Ordering};

        static DECODED_ITEMS: AtomicUsize = AtomicUsize::new(0);

        #[derive(Debug)]
        struct CountedItem;

        impl<'de> Deserialize<'de> for CountedItem {
            fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
            where
                D: Deserializer<'de>,
            {
                DECODED_ITEMS.fetch_add(1, Ordering::SeqCst);
                let _ = u16::deserialize(deserializer)?;
                Ok(Self)
            }
        }

        #[derive(Serialize)]
        struct EncodedPage {
            items: Vec<u16>,
            next_cursor: Option<String>,
        }

        DECODED_ITEMS.store(0, Ordering::SeqCst);
        let mut encoded = Vec::new();
        ciborium::ser::into_writer(
            &EncodedPage {
                items: vec![0; usize::from(MAX_PAGE_SIZE) + 1],
                next_cursor: None,
            },
            &mut encoded,
        )
        .unwrap();
        assert!(ciborium::de::from_reader::<Page<CountedItem>, _>(encoded.as_slice()).is_err());
        assert_eq!(
            DECODED_ITEMS.load(Ordering::SeqCst),
            0,
            "a declared oversized sequence must fail before decoding page items"
        );
    }

    #[test]
    fn response_errors_are_validated_as_untrusted_wire_data() {
        let response = |error| ResponseEnvelope::<BoundedTestItem> {
            protocol_version: PROTOCOL_VERSION,
            request_id: "request-01".into(),
            execution_target_id: "host-01".into(),
            connection_stamp: stamp(1, 3),
            result: Err(error),
        };

        response(ProtocolError::new(
            ErrorCode::TransportUnavailable,
            "host is reconnecting",
            true,
        ))
        .validate("request-01", "host-01", stamp(1, 3), StreamKind::Bulk)
        .unwrap();

        for message in [String::new(), "x".repeat(MAX_ERROR_MESSAGE_BYTES + 1)] {
            let error = ProtocolError {
                code: ErrorCode::Internal,
                message,
                retryable: false,
            };
            assert_eq!(
                response(error)
                    .validate("request-01", "host-01", stamp(1, 3), StreamKind::Bulk)
                    .unwrap_err()
                    .code,
                ErrorCode::InvalidFrame
            );
        }

        assert!(serde_json::from_str::<ProtocolError>(
            r#"{"code":"future_code","message":"bounded","retryable":false}"#,
        )
        .is_err());
        assert!(serde_json::from_str::<ProtocolError>(
            r#"{"code":"internal","message":"bounded","retryable":false,"future":true}"#,
        )
        .is_err());
    }

    #[test]
    fn protocol_error_constructor_truncates_at_utf8_boundary() {
        let error = ProtocolError::new(
            ErrorCode::Internal,
            format!("{}💥", "x".repeat(MAX_ERROR_MESSAGE_BYTES - 2)),
            false,
        );
        assert!(error.message.len() <= MAX_ERROR_MESSAGE_BYTES);
        assert_eq!(error.message, "x".repeat(MAX_ERROR_MESSAGE_BYTES - 2));
        error.validate().unwrap();
    }

    #[test]
    fn protocol_error_messages_reject_log_and_bidi_injection() {
        let constructed =
            ProtocolError::new(ErrorCode::Internal, "first\r\nforged\u{202e}entry", false);
        assert_eq!(constructed.message, "first  forged entry");
        constructed.validate().unwrap();

        let unicode_separators = ProtocolError::new(
            ErrorCode::Internal,
            "first\u{2028}second\u{2029}third",
            false,
        );
        assert_eq!(unicode_separators.message, "first second third");
        unicode_separators.validate().unwrap();

        for message in [
            "first\nforged",
            "first\u{202e}forged",
            "first\u{2028}forged",
            "first\u{2029}forged",
        ] {
            let inbound = ProtocolError {
                code: ErrorCode::Internal,
                message: message.into(),
                retryable: false,
            };
            assert_eq!(
                inbound.validate().unwrap_err().code,
                ErrorCode::InvalidFrame
            );
        }
    }

    #[test]
    fn resume_stamps_are_bound_to_the_current_envelope_stamp() {
        let current = stamp(7, 5);
        let request = |previous_connection_stamp| RequestEnvelope {
            protocol_version: PROTOCOL_VERSION,
            request_id: "resume-request".into(),
            execution_target_id: "host-01".into(),
            direction: PeerDirection::ControllerToHost,
            connection_stamp: current,
            body: ResumeRequest::Resume {
                previous_connection_stamp,
                last_received_event_sequence: 41,
            },
        };
        request(stamp(7, 4))
            .validate(
                PeerDirection::ControllerToHost,
                "host-01",
                current,
                StreamKind::Control,
            )
            .unwrap();
        // A suspended controller may skip connection generations.
        request(stamp(7, 2))
            .validate(
                PeerDirection::ControllerToHost,
                "host-01",
                current,
                StreamKind::Control,
            )
            .unwrap();
        // A prior host epoch is a valid request, although the host may answer
        // SnapshotRequired when its event window no longer exists.
        request(stamp(6, 99))
            .validate(
                PeerDirection::ControllerToHost,
                "host-01",
                current,
                StreamKind::Control,
            )
            .unwrap();
        for previous_connection_stamp in [
            invalid_stamp(0, 4),
            invalid_stamp(7, 0),
            current,
            stamp(7, 6),
            stamp(8, 1),
        ] {
            assert_eq!(
                request(previous_connection_stamp)
                    .validate(
                        PeerDirection::ControllerToHost,
                        "host-01",
                        current,
                        StreamKind::Control,
                    )
                    .unwrap_err()
                    .code,
                ErrorCode::StaleGeneration
            );
        }

        let response = |body_stamp| ResponseEnvelope {
            protocol_version: PROTOCOL_VERSION,
            request_id: "resume-request".into(),
            execution_target_id: "host-01".into(),
            connection_stamp: current,
            result: Ok(ResumeResponse {
                connection_stamp: body_stamp,
                disposition: ResumeDisposition::Resumed,
                first_available_event_sequence: 42,
            }),
        };
        response(current)
            .validate("resume-request", "host-01", current, StreamKind::Control)
            .unwrap();

        let fresh_response = ResumeResponse {
            connection_stamp: current,
            disposition: ResumeDisposition::Fresh,
            first_available_event_sequence: 0,
        };
        fresh_response
            .validate_response_to(&ResumeRequest::Fresh)
            .unwrap();
        assert_eq!(
            fresh_response
                .validate_response_to(&ResumeRequest::Resume {
                    previous_connection_stamp: stamp(7, 4),
                    last_received_event_sequence: 41,
                })
                .unwrap_err()
                .code,
            ErrorCode::InvalidFrame
        );
        let resumed_response = ResumeResponse {
            connection_stamp: current,
            disposition: ResumeDisposition::Resumed,
            first_available_event_sequence: 42,
        };
        let resume_request = ResumeRequest::Resume {
            previous_connection_stamp: stamp(7, 4),
            last_received_event_sequence: 41,
        };
        resumed_response
            .validate_response_to(&resume_request)
            .unwrap();
        let replay_gap = ResumeResponse {
            first_available_event_sequence: 43,
            ..resumed_response.clone()
        };
        assert_eq!(
            replay_gap
                .validate_response_to(&resume_request)
                .unwrap_err()
                .code,
            ErrorCode::InvalidFrame
        );
        let max_sequence_request = ResumeRequest::Resume {
            previous_connection_stamp: stamp(7, 4),
            last_received_event_sequence: u64::MAX,
        };
        ResumeResponse {
            first_available_event_sequence: u64::MAX,
            ..resumed_response.clone()
        }
        .validate_response_to(&max_sequence_request)
        .unwrap();
        ResumeResponse {
            disposition: ResumeDisposition::SnapshotRequired,
            first_available_event_sequence: 43,
            ..resumed_response.clone()
        }
        .validate_response_to(&resume_request)
        .unwrap();
        assert_eq!(
            resumed_response
                .validate_response_to(&ResumeRequest::Fresh)
                .unwrap_err()
                .code,
            ErrorCode::InvalidFrame
        );
        for body_stamp in [
            invalid_stamp(0, 5),
            invalid_stamp(7, 0),
            stamp(7, 4),
            stamp(7, 6),
        ] {
            assert_eq!(
                response(body_stamp)
                    .validate("resume-request", "host-01", current, StreamKind::Control)
                    .unwrap_err()
                    .code,
                ErrorCode::StaleGeneration
            );
        }

        let paged_response = ResponseEnvelope {
            protocol_version: PROTOCOL_VERSION,
            request_id: "resume-request".into(),
            execution_target_id: "host-01".into(),
            connection_stamp: current,
            result: Ok(Page {
                items: vec![ResumeResponse {
                    connection_stamp: stamp(7, 4),
                    disposition: ResumeDisposition::Resumed,
                    first_available_event_sequence: 42,
                }],
                next_cursor: None,
            }),
        };
        assert_eq!(
            paged_response
                .validate("resume-request", "host-01", current, StreamKind::Bulk)
                .unwrap_err()
                .code,
            ErrorCode::InvalidFrame
        );
    }

    #[test]
    fn stream_header_rejects_wrong_direction_and_version() {
        let mut header = StreamHeader {
            protocol_version: PROTOCOL_VERSION,
            stream_kind: StreamKind::Control,
            direction: PeerDirection::ControllerToHost,
            connection_stamp: stamp(1, 1),
        };
        assert_eq!(
            header
                .validate(PeerDirection::HostToController)
                .unwrap_err()
                .code,
            ErrorCode::WrongDirection
        );
        header.direction = PeerDirection::HostToController;
        header.protocol_version += 1;
        assert_eq!(
            header
                .validate(PeerDirection::HostToController)
                .unwrap_err()
                .code,
            ErrorCode::UnsupportedVersion
        );

        header.protocol_version = PROTOCOL_VERSION;
        header.connection_stamp = invalid_stamp(1, 0);
        assert_eq!(
            header
                .validate(PeerDirection::HostToController)
                .unwrap_err()
                .code,
            ErrorCode::StaleGeneration
        );
    }

    #[test]
    fn page_request_is_bounded() {
        assert!(PageRequest::default().validate().is_ok());
        assert_eq!(
            PageRequest {
                cursor: None,
                limit: MAX_PAGE_SIZE + 1,
            }
            .validate()
            .unwrap_err()
            .code,
            ErrorCode::InvalidPage
        );
        assert_eq!(
            PageRequest {
                cursor: Some("x".repeat(MAX_CURSOR_BYTES + 1)),
                limit: 1,
            }
            .validate()
            .unwrap_err()
            .code,
            ErrorCode::InvalidPage
        );
    }

    fn history_item(id: &str, text: impl Into<String>, merge: &str) -> RemoteAgentTimelineItem {
        RemoteAgentTimelineItem {
            id: id.to_string(),
            item_type: "message".to_string(),
            role: Some("assistant".to_string()),
            title: None,
            text: Some(text.into()),
            status: None,
            created_ms: 1_700_000_000_000,
            merge: merge.to_string(),
        }
    }

    fn history_record(items: Vec<RemoteAgentTimelineItem>) -> RemoteAgentHistoryRecord {
        RemoteAgentHistoryRecord {
            record_id: "epoch-record-01".to_string(),
            role: "assistant".to_string(),
            created_ms: 1_700_000_000_000,
            items,
        }
    }

    #[test]
    fn native_history_record_count_is_independent_of_projected_item_count() {
        let request = ListAgentHistoryRecordsRequest::new("session-01", None, 1)
            .expect("valid history request");
        AgentHistoryPageFrame::Start { record_count: 1 }
            .validate_response_to(&request)
            .expect("one native row satisfies a one-record request");
        AgentHistoryPageFrame::Record {
            index: 0,
            record: history_record(vec![
                history_item("message-01", "answer", "replace"),
                RemoteAgentTimelineItem {
                    id: "tool-01".to_string(),
                    item_type: "tool".to_string(),
                    role: Some("assistant".to_string()),
                    title: Some(SAFE_REMOTE_TOOL_TITLE.to_string()),
                    text: None,
                    status: Some("completed".to_string()),
                    created_ms: 1_700_000_000_000,
                    merge: "replace".to_string(),
                },
            ]),
        }
        .validate_response_to(&request)
        .expect("multiple projected cards remain inside one native record");

        AgentHistoryPageFrame::Record {
            index: 0,
            record: history_record(Vec::new()),
        }
        .validate_response_to(&request)
        .expect("a hidden native row remains an empty record container");
    }

    #[test]
    fn native_history_role_is_bounded_opaque_metadata() {
        let mut record = history_record(Vec::new());
        record.role = "tool_result".to_string();
        record
            .validate()
            .expect("a future native role stays pageable");

        for invalid in [
            String::new(),
            "role\nspoof".to_string(),
            "rôle".to_string(),
            "x".repeat(MAX_ID_BYTES + 1),
        ] {
            record.role = invalid;
            assert_eq!(
                record.validate().expect_err("unsafe role must fail").code,
                ErrorCode::InvalidFrame
            );
        }
    }

    #[test]
    fn history_request_defaults_and_bounds_match_goose_record_paging() {
        let request: ListAgentHistoryRecordsRequest = serde_json::from_value(serde_json::json!({
            "operation": "list_session_records",
            "sessionId": "session-01"
        }))
        .expect("request uses the record-page default");
        assert_eq!(request.limit, DEFAULT_PAGE_SIZE);
        request
            .validate()
            .expect("default history request is valid");

        let mut oversized = request.clone();
        oversized.limit = MAX_PAGE_SIZE + 1;
        assert_eq!(
            oversized.validate().unwrap_err().code,
            ErrorCode::InvalidPage
        );
        assert!(
            serde_json::from_value::<ListAgentHistoryRecordsRequest>(serde_json::json!({
                "operation": "agent_clear_user_data",
                "sessionId": "session-01",
                "limit": 1
            }))
            .is_err()
        );
    }

    #[test]
    fn shared_history_presentation_cap_rejects_one_oversized_record() {
        let half = MAX_HISTORY_RECORD_PRESENTATION_BYTES / 2;
        let record = history_record(vec![
            history_item("message-a", "a".repeat(half), "replace"),
            history_item("message-b", "b".repeat(half), "replace"),
        ]);
        assert_eq!(
            record.validate().unwrap_err().code,
            ErrorCode::HistoryRecordTooLarge
        );
    }

    #[test]
    fn synchronized_live_snapshot_items_are_absolute_and_cursor_is_camel_case() {
        let mut item = history_item("live-01", "streaming", "append");
        assert_eq!(
            RemoteAgentLiveSessionSnapshot {
                session_id: "session-01".to_string(),
                live_items: vec![item.clone()],
            }
            .validate()
            .unwrap_err()
            .code,
            ErrorCode::InvalidFrame
        );
        item.merge = "replace".to_string();
        RemoteAgentLiveSessionSnapshot {
            session_id: "session-01".to_string(),
            live_items: vec![item],
        }
        .validate()
        .expect("absolute live item is valid");

        assert!(
            serde_json::from_value::<AgentHistoryPageFrame>(serde_json::json!({
                "frame": "live_item",
                "index": 0,
                "item": {}
            }))
            .is_err()
        );

        let cursor = RemoteLiveEventCursor {
            journal_id: "0123456789abcdef0123456789abcdef".to_string(),
            sequence: 7,
        };
        assert_eq!(
            serde_json::to_value(cursor).expect("serialize live cursor"),
            serde_json::json!({
                "journalId": "0123456789abcdef0123456789abcdef",
                "sequence": 7
            })
        );

        assert_eq!(
            RemoteLiveEventCursor {
                journal_id: "0123456789abcdef0123456789abcdef".to_string(),
                sequence: MAX_JAVASCRIPT_SAFE_UNSIGNED_INTEGER + 1,
            }
            .validate()
            .unwrap_err()
            .code,
            ErrorCode::InvalidPage
        );
    }

    #[test]
    fn paged_timestamps_are_bounded_to_javascript_safe_integers() {
        let mut item = history_item("message-01", "answer", "replace");
        item.created_ms = MAX_JAVASCRIPT_SAFE_UNSIGNED_INTEGER + 1;
        assert_eq!(item.validate().unwrap_err().code, ErrorCode::InvalidFrame);

        let mut record = history_record(Vec::new());
        record.created_ms = MAX_JAVASCRIPT_SAFE_UNSIGNED_INTEGER + 1;
        assert_eq!(record.validate().unwrap_err().code, ErrorCode::InvalidFrame);

        let mut session = RemoteAgentSessionSummary {
            id: "session-01".to_string(),
            title: "Task".to_string(),
            project_root: "/tmp/maple".to_string(),
            created_ms: MAX_JAVASCRIPT_SAFE_INTEGER + 1,
            updated_ms: 1,
            page_sort_ms: 1,
            message_count: 0,
            model: None,
            mode: "smart_approve".to_string(),
        };
        assert_eq!(
            session.validate().unwrap_err().code,
            ErrorCode::InvalidFrame
        );
        session.created_ms = MAX_JAVASCRIPT_SAFE_INTEGER;
        session.message_count = MAX_JAVASCRIPT_SAFE_UNSIGNED_INTEGER;
        session.validate().expect("maximum JS-safe count is valid");
        session.message_count += 1;
        assert_eq!(
            session.validate().unwrap_err().code,
            ErrorCode::InvalidFrame
        );
    }

    #[test]
    fn live_lane_unions_route_once_and_resume_is_host_epoch_scoped() {
        let cursor = RemoteLiveEventCursor {
            journal_id: "0123456789abcdef0123456789abcdef".to_string(),
            sequence: 7,
        };
        let current = stamp(41, 9);
        let resume = ResumeAgentLiveEventsRequest::new(cursor.clone(), 41)
            .expect("same-host-epoch resume request");
        resume
            .validate_for_connection_stamp(current)
            .expect("connection generation may advance inside one host epoch");
        assert_eq!(
            resume
                .validate_for_connection_stamp(stamp(42, 1))
                .expect_err("host restart must fence the old cursor")
                .code,
            ErrorCode::StaleGeneration
        );

        let events = RemoteAgentLiveEventsRequest::from(resume);
        let request = RequestEnvelope {
            protocol_version: PROTOCOL_VERSION,
            request_id: "live-resume-01".to_string(),
            execution_target_id: "host-01".to_string(),
            direction: PeerDirection::ControllerToHost,
            connection_stamp: current,
            body: events.clone(),
        };
        request
            .validate(
                PeerDirection::ControllerToHost,
                "host-01",
                current,
                StreamKind::Events,
            )
            .expect("central Events union validates the exact resume body");
        assert!(matches!(
            serde_json::from_value::<RemoteAgentLiveEventsRequest>(
                serde_json::to_value(&events).expect("serialize Events union")
            )
            .expect("decode Events union"),
            RemoteAgentLiveEventsRequest::Resume { .. }
        ));

        let activate = RemoteAgentLiveControlRequest::from(
            ActivateAgentLiveAttachRequest::new("attach-01").expect("activation request"),
        );
        assert!(matches!(
            serde_json::from_value::<RemoteAgentLiveControlRequest>(
                serde_json::to_value(&activate).expect("serialize Control union")
            )
            .expect("decode Control union"),
            RemoteAgentLiveControlRequest::ActivateAttach { .. }
        ));
        assert_eq!(
            CancelAgentLiveResponse {
                kind: AgentLiveCancelKind::PendingAttach,
                live_id: "attach-01".to_string(),
            }
            .validate_response_to(&activate)
            .expect_err("central dispatcher forbids a mismatched response variant")
            .code,
            ErrorCode::InvalidFrame
        );
    }

    #[test]
    fn direct_peer_wire_shapes_decode_only_through_their_exact_tagged_lane() {
        let cursor = RemoteLiveEventCursor {
            journal_id: "0123456789abcdef0123456789abcdef".to_string(),
            sequence: 7,
        };
        let cases = [
            serde_json::to_value(
                BeginAgentLiveAttachRequest::new("session-01", 25).expect("begin request"),
            )
            .expect("serialize begin"),
            serde_json::to_value(
                ResumeAgentLiveEventsRequest::new(cursor, 41).expect("resume request"),
            )
            .expect("serialize resume"),
        ];
        assert!(matches!(
            serde_json::from_value::<RemoteAgentLiveEventsRequest>(cases[0].clone())
                .expect("direct Begin body decodes through Events union"),
            RemoteAgentLiveEventsRequest::BeginAttach { .. }
        ));
        assert!(matches!(
            serde_json::from_value::<RemoteAgentLiveEventsRequest>(cases[1].clone())
                .expect("direct Resume body decodes through Events union"),
            RemoteAgentLiveEventsRequest::Resume { .. }
        ));

        let activate = serde_json::to_value(
            ActivateAgentLiveAttachRequest::new("attach-01").expect("activate request"),
        )
        .expect("serialize activate");
        let cancel = serde_json::to_value(
            CancelAgentLiveRequest::new(AgentLiveCancelKind::ActiveStream, "live-01")
                .expect("cancel request"),
        )
        .expect("serialize cancel");
        assert!(matches!(
            serde_json::from_value::<RemoteAgentControlRequest>(activate)
                .expect("direct Activate body decodes through peer Control union"),
            RemoteAgentControlRequest::ActivateAttach { .. }
        ));
        assert!(matches!(
            serde_json::from_value::<RemoteAgentControlRequest>(cancel)
                .expect("direct Cancel body decodes through peer Control union"),
            RemoteAgentControlRequest::Cancel { .. }
        ));
        assert!(matches!(
            serde_json::from_value::<RemoteAgentControlRequest>(
                serde_json::to_value(GetRuntimeStatusRequest::new())
                    .expect("serialize runtime status request")
            )
            .expect("direct status body decodes through peer Control union"),
            RemoteAgentControlRequest::GetRuntimeStatus
        ));

        let history = ListAgentHistoryRecordsRequest::new(
            "session-01",
            Some("history-cursor-01".to_string()),
            25,
        )
        .expect("history request");
        assert!(matches!(
            serde_json::from_value::<RemoteAgentBulkRequest>(
                serde_json::to_value(history).expect("serialize history request")
            )
            .expect("direct history body decodes through peer Bulk union"),
            RemoteAgentBulkRequest::ListSessionRecords { .. }
        ));
        let sessions = ListAgentSessionsRequest {
            operation: AgentSessionListOperation::ListSessions,
            project_root: Some("/tmp/maple".to_string()),
            cursor: Some("session-cursor-01".to_string()),
            limit: 25,
        };
        assert!(matches!(
            serde_json::from_value::<RemoteAgentBulkRequest>(
                serde_json::to_value(sessions).expect("serialize task-list request")
            )
            .expect("direct task-list body decodes through peer Bulk union"),
            RemoteAgentBulkRequest::ListSessions { .. }
        ));

        for wrong in [
            serde_json::json!({
                "operation": "resume",
                "sessionId": "session-01",
                "limit": 25
            }),
            serde_json::json!({
                "operation": "begin_attach",
                "cursor": {"journalId": "0123456789abcdef0123456789abcdef", "sequence": 7},
                "originHostEpoch": 41
            }),
        ] {
            assert!(serde_json::from_value::<RemoteAgentLiveEventsRequest>(wrong).is_err());
        }
        for wrong in [
            serde_json::json!({"operation": "cancel", "attachId": "attach-01"}),
            serde_json::json!({
                "operation": "activate_attach",
                "kind": "active_stream",
                "liveId": "live-01"
            }),
        ] {
            assert!(serde_json::from_value::<RemoteAgentControlRequest>(wrong).is_err());
        }
    }

    #[test]
    fn terminal_permission_presentations_are_safe_but_actionable_state_is_not() {
        let mut permission = RemoteAgentTimelineItem {
            id: "permission-01".to_string(),
            item_type: "permission".to_string(),
            role: Some("system".to_string()),
            title: Some(SAFE_REMOTE_PERMISSION_TITLE.to_string()),
            text: None,
            status: Some("allow_once".to_string()),
            created_ms: 1_700_000_000_000,
            merge: "replace".to_string(),
        };
        permission
            .validate()
            .expect("resolved safe permission audit row is displayable");
        RemoteAgentPresentedLiveEvent::TimelineUpsert {
            item: permission.clone(),
        }
        .validate()
        .expect("terminal permission upsert remains in the closed live presentation");

        for actionable in [
            None,
            Some("pending"),
            Some("running"),
            Some("requested"),
            Some("requires_approval"),
            Some("allow_always"),
            Some("future_state"),
        ] {
            permission.status = actionable.map(str::to_string);
            assert_eq!(
                permission
                    .validate()
                    .expect_err("actionable permission state must stay off the wire")
                    .code,
                ErrorCode::InvalidFrame
            );
        }
    }

    #[test]
    fn serde_rejects_unknown_fields() {
        let input = r#"{
            "protocol_version":1,
            "request_id":"request-01",
            "execution_target_id":"host-01",
            "direction":"controller_to_host",
            "connection_stamp":{"host_epoch":1,"generation":1},
            "body":{"limit":10},
            "future_privilege":true
        }"#;
        assert!(serde_json::from_str::<RequestEnvelope<PageRequest>>(input).is_err());

        #[derive(Serialize)]
        struct FutureEnvelope<'a> {
            protocol_version: u16,
            request_id: &'a str,
            execution_target_id: &'a str,
            direction: PeerDirection,
            connection_stamp: ConnectionStamp,
            body: PageRequest,
            future_privilege: bool,
        }
        let mut wire = Vec::new();
        ciborium::ser::into_writer(
            &FutureEnvelope {
                protocol_version: PROTOCOL_VERSION,
                request_id: "request-01",
                execution_target_id: "host-01",
                direction: PeerDirection::ControllerToHost,
                connection_stamp: stamp(1, 1),
                body: PageRequest::default(),
                future_privilege: true,
            },
            &mut wire,
        )
        .unwrap();
        assert!(
            ciborium::de::from_reader::<RequestEnvelope<PageRequest>, _>(wire.as_slice()).is_err()
        );
    }

    #[test]
    fn frame_limit_is_enforced_before_allocation() {
        assert!(validate_frame_len(MAX_FRAME_BYTES as usize).is_ok());
        assert_eq!(
            validate_frame_len(MAX_FRAME_BYTES as usize + 1)
                .unwrap_err()
                .code,
            ErrorCode::FrameTooLarge
        );
    }
}
