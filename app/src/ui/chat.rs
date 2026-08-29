//! The agent chat surface: session sidebar, streaming transcript with tool
//! calls, permission prompts, and the composer. Pure consumer of the backend
//! facade + event stream.

use std::cell::RefCell;
use std::collections::{HashMap, HashSet};
use std::rc::Rc;
use std::sync::Arc;

use gpui::{
    AnimationExt, AnyElement, AppContext, Div, Entity, EntityInputHandler, EventEmitter, Focusable,
    Render, SharedString, Window, div, prelude::*, px,
};
use maple_agent::agent::{
    AgentImageUpload, AgentProjectTrustStatus, AgentQueuedMessage, AgentSendMessageRequest,
    AgentServiceEvent, AgentSessionMcpServer, AgentSessionSummary, AgentSlashCommand,
    AgentTimelineItem, compaction_notice_text,
};

use crate::backend::{AgentBackend, PendingPermission, PendingQuestion};
use crate::ui::icons::{icon, spinner, wordmark};
use crate::ui::markdown;
use crate::ui::rich_text::{self, RenderCtx};
use crate::ui::settings::{OpenSettingsSection, Section};
use crate::ui::text_input::TextInput;
use crate::ui::theme;

gpui::actions!(chat, [ChatEscape, CopySelection, SelectAllTranscript]);

pub struct LoggedOut;

/// Emitted when the user opens app settings from the chat header.
pub struct OpenSettings;

/// Menu item callback with access to the chat screen.
type MenuAction = Box<dyn Fn(&mut ChatScreen, &mut Context<ChatScreen>)>;

/// A queued message pulled into the composer; it keeps its place in the
/// queue until the edit is sent or discarded.
#[derive(Debug, Clone)]
struct QueueEdit {
    queue_id: String,
    /// The composer text before the edit began, restored afterwards.
    draft: String,
}

const COMPOSER_PLACEHOLDER: &str = "Ask Maple to work in this folder...";
const QUEUE_EDIT_PLACEHOLDER: &str =
    "Edit the queued message, then send to keep its place. Escape discards.";

/// What an inline sidebar rename edits.
#[derive(Debug, Clone, PartialEq, Eq)]
enum RenameTarget {
    Task(String),
    Project(String),
}

/// Sent prompts kept for Up/Down recall.
const PROMPT_HISTORY_LIMIT: usize = 50;

/// An image staged in the composer: the data URL the runtime stores with
/// the message and a square thumbnail, cropped off the UI thread.
#[derive(Clone)]
struct DraftImage {
    /// Unique per staged draft, so a late thumbnail finds its owner even
    /// after other drafts were removed.
    id: u64,
    name: String,
    data_url: String,
    /// `None` until the crop finishes (or if the image did not decode).
    thumbnail: Option<Arc<gpui::Image>>,
}
/// Which text of a timeline item a parsed document belongs to.
#[derive(Clone, Copy)]
enum MarkdownKind {
    Body = 0,
    ToolOutput = 1,
}

/// Per-item parsed markdown. Interior mutability because the list render
/// callback only has shared access to the screen. Entries are keyed by
/// item id and kind and validated by the item's revision and text length,
/// so no frame hashes message content.
/// Cached document with the item revision and text length it was parsed at.
type MarkdownEntry = (u64, usize, Rc<markdown::Document>);

#[derive(Default)]
struct MarkdownCache {
    entries: [RefCell<HashMap<String, MarkdownEntry>>; 2],
    /// Base ordinal per item key, so paragraphs get stable selection keys.
    ordinals: RefCell<HashMap<String, u64>>,
}

impl MarkdownCache {
    /// Parsed document for `source`, parsed now if the cache is stale.
    fn get(
        &self,
        id: &str,
        kind: MarkdownKind,
        revision: u64,
        source: &str,
    ) -> Rc<markdown::Document> {
        let mut entries = self.entries[kind as usize].borrow_mut();
        if let Some((cached_revision, cached_len, document)) = entries.get(id)
            && *cached_revision == revision
            && *cached_len == source.len()
        {
            return Rc::clone(document);
        }
        if entries.len() > 4096 {
            entries.clear();
        }
        let document = Rc::new(markdown::parse(source));
        entries.insert(
            id.to_string(),
            (revision, source.len(), Rc::clone(&document)),
        );
        document
    }

    /// Selection ordinal base for an item key. Bases are spaced far apart
    /// so `base + block index` never collides across messages.
    fn ordinal_for(&self, key: &str) -> u64 {
        let mut ordinals = self.ordinals.borrow_mut();
        if let Some(base) = ordinals.get(key) {
            return *base;
        }
        let base = ordinals.values().copied().max().unwrap_or(0) + 4096;
        ordinals.insert(key.to_string(), base);
        base
    }

    fn clear(&self) {
        for entries in &self.entries {
            entries.borrow_mut().clear();
        }
        self.ordinals.borrow_mut().clear();
    }
}

/// Strings a tool or text card shows, derived once per item revision
/// instead of on every frame.
#[derive(Default)]
struct ItemDerived {
    /// Display text of thinking rows and user bubbles.
    text: SharedString,
    /// Readable tool output for the expanded card.
    output_text: Option<SharedString>,
    /// First non-empty output line for the collapsed card.
    preview: Option<SharedString>,
    /// `input: {json}` for the expanded card.
    input_line: Option<SharedString>,
    /// +/- lines of an edit or write tool, capped at `MAX_DIFF_LINES`.
    diff_lines: Rc<Vec<(char, SharedString)>>,
}

const MAX_DIFF_LINES: usize = 200;

impl ItemDerived {
    fn build(item: &AgentTimelineItem) -> Self {
        let output_text = tool_output_markdown(item).map(SharedString::from);
        let preview = output_text.as_ref().and_then(|text| {
            text.lines()
                .map(str::trim)
                .find(|line| !line.is_empty())
                .map(|line| SharedString::from(line.to_string()))
        });
        Self {
            text: SharedString::from(
                maple_display_text(item.text.as_deref().unwrap_or("")).into_owned(),
            ),
            output_text,
            preview,
            input_line: tool_input_line(item).map(SharedString::from),
            diff_lines: Rc::new(diff_lines_for(item)),
        }
    }
}

/// Lazily built `ItemDerived` per item id, validated by item revision.
#[derive(Default)]
struct DerivedCache {
    entries: RefCell<HashMap<String, (u64, Rc<ItemDerived>)>>,
}

impl DerivedCache {
    fn get(&self, item: &AgentTimelineItem, revision: u64) -> Rc<ItemDerived> {
        let mut entries = self.entries.borrow_mut();
        if let Some((cached, derived)) = entries.get(&item.id)
            && *cached == revision
        {
            return Rc::clone(derived);
        }
        if entries.len() > 4096 {
            entries.clear();
        }
        let derived = Rc::new(ItemDerived::build(item));
        entries.insert(item.id.clone(), (revision, Rc::clone(&derived)));
        derived
    }

    fn clear(&self) {
        self.entries.borrow_mut().clear();
    }
}

/// Shared read-only state the transcript rows render from.
struct TranscriptCtx<'a> {
    markdown_cache: &'a MarkdownCache,
    derived: &'a DerivedCache,
    attachment_images: &'a HashMap<String, Arc<gpui::Image>>,
    chat: &'a gpui::WeakEntity<ChatScreen>,
    tool_summaries: &'a HashMap<String, SharedString>,
    summary_requests: &'a HashSet<String>,
    render: &'a RenderCtx,
    /// Message being spoken, if any.
    speech: Option<&'a SpeechState>,
    /// The account can use text-to-speech.
    speech_available: bool,
}

/// Text-to-speech progress for one message.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct SpeechState {
    pub item_id: String,
    /// False while the first chunk is still being synthesized.
    pub playing: bool,
}

pub struct ChatScreen {
    backend: Arc<AgentBackend>,
    user_id: String,
    sessions: Vec<AgentSessionSummary>,
    selected_session: Option<String>,
    timeline: Vec<AgentTimelineItem>,
    /// Active run per session id, kept across selection changes.
    active_runs: HashMap<String, String>,
    pending_permission: Option<PendingPermission>,
    /// Pretty-printed tool arguments for the permission card, formatted
    /// once when the request arrives instead of on every frame.
    pending_permission_arguments: SharedString,
    permission_responding: bool,
    /// Suppresses duplicate session creation while one is in flight.
    session_setup_pending: bool,
    /// An ask_user question waiting for the user's text answer.
    /// Questions waiting for the user, oldest first. The model can issue
    /// several ask_user calls in one turn; every card must be answerable.
    pending_questions: Vec<PendingQuestion>,
    /// Lazily created answer input for the question card.
    pending_question_input: Option<Entity<TextInput>>,
    /// Fraction of the context window in use for the selected session.
    context_fraction: Option<f32>,
    /// True while the per-second usage poller task is running.
    usage_poller_active: bool,
    /// Last ledger-confirmed context tokens for the selected session.
    ledger_context_tokens: i64,
    /// Context limit used with the estimate above.
    context_limit: i64,
    composer: Option<Entity<TextInput>>,
    /// Composer holds non-blank text; refreshed when the composer changes.
    composer_has_text: bool,
    /// Palette rows for the composer's current "/" token; rebuilt when the
    /// composer changes, not per frame.
    slash_entries: Vec<SlashEntry>,
    models: Vec<String>,
    selected_model: Option<String>,
    models_menu_open: bool,
    /// Approval-mode dropdown open state, anchored under the composer.
    mode_menu_open: bool,
    /// Pinned project roots, ordered by pin time; first in the sidebar.
    pinned_roots: Vec<String>,
    /// Desktop notifications enabled (settings).
    notify_enabled: bool,
    /// Mirrors `window.is_window_active()` from the last render; refreshed
    /// on activation changes because they force a redraw.
    window_active: bool,
    sidebar_plan: Option<crate::billing::PlanUsage>,
    runtime_error: Option<SharedString>,
    notice: Option<SharedString>,
    booting: bool,
    /// Virtualized transcript state; bottom-aligned like a chat log.
    list_state: gpui::ListState,
    /// Independent scroll state for the sidebar session list. Sharing one
    /// handle made each container clamp the other's offset, which broke
    /// the transcript's bottom pinning.
    sidebar_scroll: gpui::ScrollHandle,
    /// Set when the transcript should jump to its newest content on the
    /// next render (session switch or send); streaming follows only while
    /// the view is already at the bottom.
    follow_transcript: bool,
    /// Whether tool cards show their input/output payloads. Toggled from
    /// the header; off gives a one-line card per tool call.
    tool_details: bool,
    /// Permission policy: "smart_approve" prompts per gated tool, "auto"
    /// approves everything (bypass). Applies to new runs.
    permission_mode: String,
    /// False once the user picks a mode for this specific session; the
    /// settings default then no longer overrides it.
    uses_default_permission_mode: bool,
    /// Root the runtime is currently serving; new tasks operate here.
    project_root: Option<String>,
    recent_roots: Vec<String>,
    root_menu_open: bool,
    /// Manual path entry for the root switcher.
    root_input: Option<Entity<TextInput>>,
    root_switching: bool,
    /// Session to open once a root switch triggered by the sidebar lands.
    pending_session_select: Option<String>,
    /// Sidebar hidden; a toggle in the main pane brings it back.
    sidebar_collapsed: bool,
    /// Images staged for the next message.
    draft_images: Vec<DraftImage>,
    /// Source of `DraftImage::id`.
    draft_counter: u64,
    /// True while the native file dialog is open.
    image_picking: bool,
    /// Catalog vision flag per model id, looked up when a model is chosen.
    model_vision: HashMap<String, bool>,
    /// MCP servers with their enabled state for the selected task.
    session_mcp: Vec<AgentSessionMcpServer>,
    mcp_menu_open: bool,
    /// Composer fills the pane (fullscreen editing).
    composer_expanded: bool,
    /// Web tools on for the selected task (mirrors the session record).
    web_enabled: bool,
    /// Settings default applied to newly created tasks.
    default_web_enabled: bool,
    /// Parsed markdown per timeline item (keyed by item id and revision),
    /// so visible messages are parsed once, not every frame.
    markdown_cache: MarkdownCache,
    /// Per-item display strings, rebuilt when the item's revision moves.
    derived: DerivedCache,
    /// Item id to `(index in timeline, revision)`; the revision counts
    /// applied updates so caches can tell a changed item from a stable one.
    timeline_index: HashMap<String, (usize, u64)>,
    /// Sidebar groups: (project root, indices into `sessions`), rebuilt
    /// when sessions or roots change instead of on every render.
    project_groups: Vec<(String, Vec<usize>)>,
    /// Indices into `sessions` of archived tasks, newest first.
    archived_indices: Vec<usize>,
    /// Roots whose task list is folded in the sidebar.
    collapsed_roots: HashSet<String>,
    /// Archived section open in the sidebar.
    archived_expanded: bool,
    /// Guards against a slow session load overwriting a newer selection.
    selection_generation: u64,
    /// Per-session count of applied timeline events; a load whose snapshot
    /// predates newer events is discarded instead of clobbering them.
    timeline_revisions: HashMap<String, u64>,
    /// Decoded images for sent attachments, keyed by attachment id, so the
    /// transcript shows the picture instead of only its file name.
    attachment_images: HashMap<String, Arc<gpui::Image>>,
    /// Attachment ids with a read in flight or already failed; never asked
    /// for twice.
    attachment_requests: HashSet<String>,
    /// Shared drag-selection state for transcript text.
    selection: Option<Entity<rich_text::TextSelection>>,
    /// Window position of the transcript's right-click menu while open.
    transcript_menu: Option<gpui::Point<gpui::Pixels>>,
    /// Focus handle of the transcript; a selection press moves focus here
    /// so the copy keybinding applies.
    transcript_focus: Option<gpui::FocusHandle>,
    /// True from send until the first agent item of the run arrives; shows
    /// the waiting indicator under the transcript.
    awaiting_first_token: bool,
    /// Item ids whose tool card was clicked; membership inverts the
    /// `tool_details` default for that card.
    toggled_tools: HashSet<String>,
    /// Ticked options of a multi-select question.
    question_selected: HashMap<usize, usize>,
    /// Index of the question being shown within the current card; a batch
    /// iterates one question at a time instead of listing them all.
    question_step: usize,
    /// Answers recorded for earlier steps of the current card, by index.
    question_step_answers: HashMap<usize, Vec<String>>,
    /// Full-size image shown over the chat until dismissed.
    lightbox: Option<Arc<gpui::Image>>,
    /// Display names for project roots, from settings.
    project_names: HashMap<String, String>,
    /// Sent prompts, oldest first, for Up/Down recall in the composer.
    prompt_history: Vec<String>,
    /// Position in `prompt_history` while browsing; `None` when typing.
    history_index: Option<usize>,
    /// The unsent draft saved when browsing started.
    history_draft: String,
    /// Inline rename in progress in the sidebar.
    rename: Option<RenameTarget>,
    rename_input: Option<Entity<TextInput>>,
    rename_focus_pending: bool,
    /// Root whose overflow menu is open.
    project_menu: Option<String>,
    /// Root waiting for the user to confirm removal.
    confirm_remove_root: Option<String>,
    /// Project that has skills or guidance and no trust decision yet.
    trust_prompt: Option<AgentProjectTrustStatus>,
    trust_saving: bool,
    /// Trust status of the project whose overflow menu is open.
    menu_trust: Option<AgentProjectTrustStatus>,
    /// Messages waiting behind the selected session's active run.
    queue: Vec<AgentQueuedMessage>,
    /// A newer release, until the banner is dismissed.
    update: Option<crate::update::UpdateInfo>,
    queue_busy: bool,
    queue_edit: Option<QueueEdit>,
    /// Sidebar task filter, lower-cased; empty shows everything.
    sidebar_filter: String,
    search_input: Option<Entity<TextInput>>,
    /// Slash commands from the installed skills of the current project root.
    slash_commands: Vec<AgentSlashCommand>,
    /// Highlighted row in the open slash palette, if any.
    slash_selected: Option<usize>,
    /// Set when a pending question should steal focus at the next render.
    question_focus_pending: bool,
    /// One-line model summaries per completed tool item id.
    tool_summaries: HashMap<String, SharedString>,
    /// Item ids with a summary request sent; never asked twice.
    summary_requests: HashSet<String>,
    /// Summary requests in flight; caps how many ride at once.
    pending_summaries: usize,
    /// Item ids waiting for a free summary slot, oldest first.
    summary_queue: std::collections::VecDeque<String>,
    /// Bumped on session switch; results from an older generation are
    /// dropped instead of landing on the wrong screen state.
    summary_generation: u64,
    /// Tool call summaries enabled (settings).
    summaries_enabled: bool,
    /// Microphone and speaker, on their own thread.
    audio: Arc<crate::audio::AudioEngine>,
    /// Voice endpoints the account offers; refreshed after runtime start.
    audio_caps: maple_agent::agent::AudioCapabilities,
    /// The microphone is open.
    recording: bool,
    /// A recording is at Whisper.
    transcribing: bool,
    /// Text-to-speech in progress.
    speech: Option<SpeechState>,
    /// Bumped on every speak or stop; stale chunks are dropped.
    speech_generation: u64,
    tts_voice: String,
    tts_speed: f32,
}

/// What a session snapshot load replaces once it lands.
#[derive(Clone, Copy)]
enum LoadMode {
    /// Make the session current (sidebar click, boot, root switch).
    Select,
    /// Swap the timeline only (mid-run history compaction).
    Reload,
}

/// How often a stale snapshot is fetched again before it is applied.
const LOAD_RETRIES: u8 = 2;

impl ChatScreen {
    pub fn new(backend: Arc<AgentBackend>, user_id: String, cx: &mut Context<Self>) -> Self {
        let weak = cx.entity().downgrade();
        let mut this = Self::new_inner(backend, user_id);
        this.attach_composer(weak, cx);
        this.selection = Some(cx.new(|_| rich_text::TextSelection::default()));
        this.transcript_focus = Some(cx.focus_handle());
        let search = cx.new(|cx| TextInput::new("Search tasks", cx).with_tab_index(2));
        cx.observe(&search, |this, input, cx| this.search_changed(&input, cx))
            .detach();
        this.search_input = Some(search);
        this.start(cx);
        this
    }

    /// Create and wire the composer; called by the real constructor.
    fn attach_composer(&mut self, weak: gpui::WeakEntity<Self>, cx: &mut Context<Self>) {
        let composer = cx.new(|cx| {
            TextInput::new(COMPOSER_PLACEHOLDER, cx)
                .multiline(8)
                .on_key({
                    let weak = weak.clone();
                    // This hook runs while the composer entity is being
                    // updated: never read or update the composer here.
                    // Text arrives as an argument; composer writes defer.
                    move |event: &gpui::KeyDownEvent, text: &SharedString, _window, cx| -> bool {
                        let Some(this) = weak.upgrade() else {
                            return false;
                        };
                        let key = event.keystroke.key.clone();
                        let steer_combo = key == "enter"
                            && !event.keystroke.modifiers.shift
                            && !event.keystroke.modifiers.alt
                            && (event.keystroke.modifiers.control
                                || event.keystroke.modifiers.platform);
                        if steer_combo {
                            let text = text.to_string();
                            let weak = weak.clone();
                            cx.defer(move |cx| {
                                weak.update(cx, |chat, cx| chat.steer_text(text, cx)).ok();
                            });
                            return true;
                        }
                        let plain = !event.keystroke.modifiers.control
                            && !event.keystroke.modifiers.alt
                            && !event.keystroke.modifiers.platform
                            && !event.keystroke.modifiers.shift;
                        if !plain {
                            return false;
                        }
                        let token = text.strip_prefix('/').unwrap_or_default();
                        let token_ok = text.starts_with('/')
                            && !token.contains(char::is_whitespace)
                            && !token.contains('/');
                        match key.as_str() {
                            "down" | "up" if token_ok => this.update(cx, |chat, cx| {
                                chat.navigate_slash_palette(&key, token, cx)
                            }),
                            "down" | "up" => {
                                let recalled = this
                                    .update(cx, |chat, _| chat.recall_prompt(&key, text.as_ref()));
                                match recalled {
                                    Some(recalled) => {
                                        let weak = weak.clone();
                                        cx.defer(move |cx| {
                                            weak.update(cx, |chat, cx| {
                                                if let Some(composer) = chat.composer.clone() {
                                                    composer.update(cx, |input, cx| {
                                                        input.set_text(&recalled, cx)
                                                    });
                                                }
                                            })
                                            .ok();
                                        });
                                        true
                                    }
                                    None => false,
                                }
                            }
                            "tab" if token_ok => {
                                let name = this.update(cx, |chat, _| {
                                    let entries = slash_entries_for(token, &chat.slash_commands);
                                    let index = chat
                                        .slash_selected
                                        .filter(|index| *index < entries.len())
                                        .unwrap_or(0);
                                    entries.get(index).map(|entry| entry.name.clone())
                                });
                                if let Some(name) = name {
                                    let weak = weak.clone();
                                    cx.defer(move |cx| {
                                        weak.update(cx, |chat, cx| {
                                            chat.complete_slash_command(&name, cx)
                                        })
                                        .ok();
                                    });
                                    true
                                } else {
                                    false
                                }
                            }
                            _ => {
                                // Typing anything else restarts selection.
                                this.update(cx, |chat, cx| {
                                    if chat.slash_selected.is_some() {
                                        chat.slash_selected = None;
                                        cx.notify();
                                    }
                                });
                                false
                            }
                        }
                    }
                })
                .on_enter({
                    let weak = weak.clone();
                    // The hook runs inside the composer's update; defer the
                    // send so it may read and clear the composer safely.
                    move |text, _, cx| {
                        let weak = weak.clone();
                        cx.defer(move |cx| {
                            if let Some(this) = weak.upgrade() {
                                this.update(cx, |chat, cx| chat.send_text(text.clone(), cx));
                            }
                        });
                    }
                })
                .on_paste_image(move |image, _, cx| {
                    if let Some(this) = weak.upgrade() {
                        this.update(cx, |chat, cx| chat.paste_image(image, cx));
                    }
                })
        });
        cx.observe(&composer, |this, composer, cx| {
            this.composer_changed(&composer, cx);
        })
        .detach();
        self.composer = Some(composer);
    }

    /// The composer text moved: refresh the derived state the render path
    /// reads (send button enablement, slash palette rows).
    fn composer_changed(&mut self, composer: &Entity<TextInput>, cx: &mut Context<Self>) {
        let input = composer.read(cx);
        let text = input.text_ref();
        let has_text = !text.trim().is_empty();
        let entries = match text.strip_prefix('/') {
            Some(token) if !token.contains(char::is_whitespace) && !token.contains('/') => {
                slash_entries_for(token, &self.slash_commands)
            }
            _ => Vec::new(),
        };
        let entries_changed = entries.len() != self.slash_entries.len()
            || entries
                .iter()
                .zip(&self.slash_entries)
                .any(|(next, previous)| next.name != previous.name);
        if has_text != self.composer_has_text || entries_changed {
            self.composer_has_text = has_text;
            self.slash_entries = entries;
            cx.notify();
        }
    }

    /// Test seam: pure state without composer wiring or runtime start.
    pub(crate) fn new_inner(backend: Arc<AgentBackend>, user_id: String) -> Self {
        Self::new_inner_with_placeholder(backend, user_id)
    }

    fn new_inner_with_placeholder(backend: Arc<AgentBackend>, user_id: String) -> Self {
        let settings = crate::settings::load_settings();
        Self {
            backend,
            user_id,
            sessions: Vec::new(),
            selected_session: None,
            timeline: Vec::new(),
            active_runs: HashMap::new(),
            pending_permission: None,
            pending_permission_arguments: SharedString::default(),
            permission_responding: false,
            session_setup_pending: false,
            pending_questions: Vec::new(),
            pending_question_input: None,
            context_fraction: None,
            usage_poller_active: false,
            ledger_context_tokens: 0,
            context_limit: 0,
            composer: None,
            composer_has_text: false,
            slash_entries: Vec::new(),
            models: Vec::new(),
            selected_model: None,
            models_menu_open: false,
            mode_menu_open: false,
            pinned_roots: settings.pinned_roots.clone(),
            notify_enabled: settings.desktop_notifications,
            window_active: true,
            sidebar_plan: None,
            runtime_error: None,
            notice: None,
            booting: true,
            list_state: gpui::ListState::new(0, gpui::ListAlignment::Bottom, px(400.)),
            sidebar_scroll: gpui::ScrollHandle::new(),
            follow_transcript: true,
            tool_details: settings.tool_details,
            permission_mode: std::env::var("MAPLE_PERMISSION_MODE")
                .ok()
                .filter(|mode| mode == "auto" || mode == "smart_approve")
                .or_else(|| {
                    Some(settings.default_permission_mode.clone())
                        .filter(|mode| mode == "auto" || mode == "smart_approve")
                })
                .unwrap_or_else(|| "smart_approve".to_string()),
            uses_default_permission_mode: std::env::var("MAPLE_PERMISSION_MODE").is_err(),
            project_root: None,
            recent_roots: Vec::new(),
            root_menu_open: false,
            pending_session_select: None,
            sidebar_collapsed: false,
            draft_images: Vec::new(),
            draft_counter: 0,
            image_picking: false,
            model_vision: HashMap::new(),
            session_mcp: Vec::new(),
            mcp_menu_open: false,
            composer_expanded: false,
            web_enabled: true,
            default_web_enabled: settings.default_web_enabled,
            markdown_cache: MarkdownCache::default(),
            derived: DerivedCache::default(),
            timeline_index: HashMap::new(),
            project_groups: Vec::new(),
            archived_indices: Vec::new(),
            collapsed_roots: HashSet::new(),
            archived_expanded: false,
            root_input: None,
            root_switching: false,
            selection_generation: 0,
            attachment_images: HashMap::new(),
            attachment_requests: HashSet::new(),
            timeline_revisions: HashMap::new(),
            selection: None,
            transcript_menu: None,
            transcript_focus: None,
            awaiting_first_token: false,
            toggled_tools: HashSet::new(),
            question_selected: HashMap::new(),
            question_step: 0,
            question_step_answers: HashMap::new(),
            lightbox: None,
            project_names: settings.project_names.clone(),
            prompt_history: Vec::new(),
            history_index: None,
            history_draft: String::new(),
            rename: None,
            rename_input: None,
            rename_focus_pending: false,
            project_menu: None,
            confirm_remove_root: None,
            trust_prompt: None,
            trust_saving: false,
            menu_trust: None,
            queue: Vec::new(),
            queue_busy: false,
            queue_edit: None,
            sidebar_filter: String::new(),
            search_input: None,
            update: None,
            slash_commands: Vec::new(),
            slash_selected: None,
            question_focus_pending: false,
            tool_summaries: HashMap::new(),
            summary_requests: HashSet::new(),
            pending_summaries: 0,
            summary_queue: std::collections::VecDeque::new(),
            summary_generation: 0,
            summaries_enabled: settings.tool_summaries,
            audio: Arc::new(crate::audio::AudioEngine::new()),
            audio_caps: maple_agent::agent::AudioCapabilities::default(),
            recording: false,
            transcribing: false,
            speech: None,
            speech_generation: 0,
            tts_voice: settings.tts_voice,
            tts_speed: settings.tts_speed,
        }
    }

    fn call<T, F>(
        &self,
        future: F,
        cx: &mut Context<Self>,
        then: impl FnOnce(&mut Self, Result<T, String>, &mut Context<Self>) + 'static,
    ) where
        T: Send + 'static,
        F: std::future::Future<Output = Result<T, String>> + Send + 'static,
    {
        let task = self.backend.spawn(future);
        cx.spawn(async move |this, cx| {
            let result = task.await.unwrap_or_else(|error| {
                log::debug!("agent task failed: {error:?}");
                Err("The agent task was cancelled".to_string())
            });
            this.update(cx, |this, cx| then(this, result, cx)).ok();
        })
        .detach();
    }

    /// Sign-in finished: boot the runtime, then load the workspace state.
    fn start(&self, cx: &mut Context<Self>) {
        let backend = self.backend.clone();
        let user_id = self.user_id.clone();
        let request = backend.default_start_request();
        self.call(
            async move { backend.start_runtime(&user_id, Some(request)).await },
            cx,
            |this, result, cx| {
                match result {
                    Ok(status) => {
                        this.runtime_error = None;
                        this.project_root = status.project_root;
                        this.check_project_trust(cx);
                    }
                    Err(message) => {
                        this.runtime_error =
                            Some(format!("Runtime failed to start: {message}").into())
                    }
                }
                this.booting = false;
                cx.notify();
                this.refresh_models(cx);
                this.refresh_roots(cx);
                this.refresh_slash_commands(cx);
                this.refresh_sessions(cx);
                this.refresh_sidebar_plan(cx);
                this.refresh_audio_capabilities(cx);
            },
        );
    }

    fn refresh_audio_capabilities(&self, cx: &mut Context<Self>) {
        let backend = self.backend.clone();
        let user_id = self.user_id.clone();
        self.call(
            async move { backend.audio_capabilities(&user_id).await },
            cx,
            |this, result, cx| match result {
                Ok(caps) => {
                    if caps != this.audio_caps {
                        this.audio_caps = caps;
                        cx.notify();
                    }
                }
                Err(message) => log::debug!("audio capabilities unavailable: {message}"),
            },
        );
    }

    // ----- Voice: microphone to Whisper -----

    /// Mic button: start a recording, or stop it and transcribe.
    fn toggle_recording(&mut self, cx: &mut Context<Self>) {
        if self.transcribing {
            return;
        }
        if self.recording {
            self.finish_recording(cx);
        } else {
            self.begin_recording(cx);
        }
    }

    fn begin_recording(&mut self, cx: &mut Context<Self>) {
        self.stop_speech(cx);
        self.notice = None;
        let audio = Arc::clone(&self.audio);
        cx.spawn(async move |this, cx| {
            let result = audio.start_recording().await;
            this.update(cx, |this, cx| {
                match result {
                    Ok(()) => this.recording = true,
                    Err(message) => this.notice = Some(message.into()),
                }
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    fn finish_recording(&mut self, cx: &mut Context<Self>) {
        self.recording = false;
        self.transcribing = true;
        cx.notify();
        let audio = Arc::clone(&self.audio);
        let backend = self.backend.clone();
        let user_id = self.user_id.clone();
        cx.spawn(async move |this, cx| {
            let result = match audio.stop_recording().await {
                Ok(wav) => {
                    let task_backend = backend.clone();
                    backend
                        .spawn(async move { task_backend.transcribe_audio(&user_id, wav).await })
                        .await
                }
                .unwrap_or_else(|_| Err("Transcription was cancelled".to_string())),
                Err(message) => Err(message),
            };
            this.update(cx, |this, cx| {
                this.transcribing = false;
                match result {
                    Ok(text) if text.is_empty() => {
                        this.notice = Some("No speech was recognized".into());
                    }
                    Ok(text) => this.insert_transcript(&text, cx),
                    Err(message) => this.notice = Some(message.into()),
                }
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    /// Append a transcript to the composer, after a space when text is
    /// already there.
    fn insert_transcript(&mut self, transcript: &str, cx: &mut Context<Self>) {
        let Some(composer) = self.composer.clone() else {
            return;
        };
        composer.update(cx, |input, cx| {
            let current = input.text_ref().trim_end().to_string();
            let text = if current.is_empty() {
                transcript.to_string()
            } else {
                format!("{current} {transcript}")
            };
            input.set_text(&text, cx);
        });
    }

    // ----- Voice: text-to-speech -----

    /// Speak button: read a message aloud, or stop when it is the one
    /// already playing.
    fn toggle_speech(&mut self, item_id: String, text: String, cx: &mut Context<Self>) {
        if self
            .speech
            .as_ref()
            .is_some_and(|speech| speech.item_id == item_id)
        {
            self.stop_speech(cx);
            return;
        }
        self.speak(item_id, text, cx);
    }

    fn stop_speech(&mut self, cx: &mut Context<Self>) {
        self.speech_generation += 1;
        self.audio.stop_playback(self.speech_generation);
        if self.speech.take().is_some() {
            cx.notify();
        }
    }

    /// Synthesize `text` chunk by chunk and queue each one as it lands,
    /// so playback starts after the first chunk instead of the last.
    fn speak(&mut self, item_id: String, text: String, cx: &mut Context<Self>) {
        self.stop_speech(cx);
        let chunks = speech_chunks(&text);
        if chunks.is_empty() {
            self.notice = Some("There is nothing to read aloud".into());
            cx.notify();
            return;
        }
        let generation = self.speech_generation;
        self.speech = Some(SpeechState {
            item_id,
            playing: false,
        });
        self.notice = None;
        cx.notify();

        let audio = Arc::clone(&self.audio);
        let backend = self.backend.clone();
        let user_id = self.user_id.clone();
        let voice = self.tts_voice.clone();
        let speed = self.tts_speed;
        cx.spawn(async move |this, cx| {
            let is_current = |this: &gpui::WeakEntity<Self>, cx: &mut gpui::AsyncApp| {
                this.read_with(cx, |this, _| this.speech_generation == generation)
                    .unwrap_or(false)
            };
            for (ix, chunk) in chunks.into_iter().enumerate() {
                let task_backend = backend.clone();
                let user_id = user_id.clone();
                let voice = voice.clone();
                let synthesized = backend
                    .spawn(async move {
                        task_backend
                            .synthesize_speech(&user_id, chunk, voice, speed)
                            .await
                    })
                    .await
                    .unwrap_or_else(|_| Err("Text-to-speech was cancelled".to_string()));
                if !is_current(&this, cx) {
                    return;
                }
                let played = match synthesized {
                    Ok(wav) => audio.play(generation, wav).await,
                    Err(message) => Err(message),
                };
                if let Err(message) = played {
                    log::warn!("speech chunk {} failed: {message}", ix + 1);
                    this.update(cx, |this, cx| {
                        if this.speech_generation == generation {
                            this.speech = None;
                            this.notice = Some(message.into());
                            cx.notify();
                        }
                    })
                    .ok();
                    return;
                }
                if ix == 0 {
                    this.update(cx, |this, cx| {
                        if let Some(speech) = this.speech.as_mut()
                            && this.speech_generation == generation
                        {
                            speech.playing = true;
                            cx.notify();
                        }
                    })
                    .ok();
                }
            }
            audio.await_idle(generation).await;
            this.update(cx, |this, cx| {
                if this.speech_generation == generation && this.speech.take().is_some() {
                    cx.notify();
                }
            })
            .ok();
        })
        .detach();
    }

    fn refresh_roots(&self, cx: &mut Context<Self>) {
        let backend = self.backend.clone();
        let user_id = self.user_id.clone();
        self.call(
            async move { backend.recent_project_roots(&user_id).await },
            cx,
            |this, result, cx| {
                if let Ok(roots) = result {
                    this.recent_roots = roots.into_iter().map(|root| root.path).collect();
                    this.rebuild_project_groups();
                }
                cx.notify();
            },
        );
    }

    fn switch_root(&mut self, path: String, cx: &mut Context<Self>) {
        if self.root_switching {
            return;
        }
        let path = path.trim().to_string();
        if path.is_empty() || !std::path::Path::new(&path).is_absolute() {
            self.notice = Some("Enter an absolute directory path".into());
            cx.notify();
            return;
        }
        self.root_switching = true;
        self.root_menu_open = false;
        self.root_input = None;
        self.notice = None;
        cx.notify();
        let backend = self.backend.clone();
        let user_id = self.user_id.clone();
        self.call(
            async move { backend.set_project_root(&user_id, path).await },
            cx,
            |this, result, cx| {
                this.root_switching = false;
                match result {
                    Ok(status) => {
                        this.project_root = status.project_root;
                        this.check_project_trust(cx);
                        this.rebuild_project_groups();
                        this.replace_timeline(Vec::new());
                        this.refresh_slash_commands(cx);
                        this.refresh_roots(cx);
                        match this.pending_session_select.take() {
                            Some(session_id) => {
                                // Keep the auto-select in refresh_sessions
                                // from racing the explicit choice.
                                this.selected_session = Some(session_id.clone());
                                this.refresh_sessions(cx);
                                this.select_session(&session_id, cx);
                            }
                            None => {
                                this.selected_session = None;
                                this.refresh_sessions(cx);
                            }
                        }
                    }
                    Err(message) => this.notice = Some(message.into()),
                }
                cx.notify();
            },
        );
    }

    fn choose_root_dialog(&mut self, cx: &mut Context<Self>) {
        // Best-effort native directory picker on a blocking thread so the
        // window keeps painting; falls back to manual entry.
        let current = self.project_root.clone().unwrap_or_default();
        self.call(
            async move {
                tokio::task::spawn_blocking(move || {
                    let output = std::process::Command::new("zenity")
                        .arg("--file-selection")
                        .arg("--directory")
                        .arg("--filename")
                        .arg(&current)
                        .output();
                    match output {
                        Ok(output) if output.status.success() => Ok(Some(
                            String::from_utf8_lossy(&output.stdout).trim().to_string(),
                        )),
                        _ => Ok(None),
                    }
                })
                .await
                .map_err(|error| format!("Folder picker failed: {error}"))?
            },
            cx,
            |this, result, cx| match result {
                Ok(Some(path)) if !path.is_empty() => this.switch_root(path, cx),
                Ok(Some(_)) => {}
                _ => this.show_root_input(cx),
            },
        );
    }

    /// Manual path entry when the native picker is unavailable.
    fn show_root_input(&mut self, cx: &mut Context<Self>) {
        // zenity missing or cancelled: offer manual entry.
        if self.root_input.is_none() {
            let input =
                cx.new(|cx| TextInput::new("/absolute/path/to/project", cx).with_tab_index(0));
            self.root_input = Some(input);
        }
        cx.notify();
    }

    fn refresh_models(&self, cx: &mut Context<Self>) {
        let backend = self.backend.clone();
        let user_id = self.user_id.clone();
        let env_model = backend.configured_model();
        self.call(
            async move {
                let saved = backend.saved_model(&user_id).await;
                let models = backend.available_model_ids(&user_id).await?;
                Ok((models, saved))
            },
            cx,
            move |this, result, cx| {
                if let Ok((models, saved)) = result {
                    if this.selected_model.is_none() {
                        // Env override wins, then the account's saved
                        // default, then the first catalog entry.
                        this.selected_model =
                            env_model.or(saved).or_else(|| models.first().cloned());
                    }
                    this.models = models;
                    this.refresh_vision(cx);
                }
                cx.notify();
            },
        );
    }

    fn refresh_sessions(&self, cx: &mut Context<Self>) {
        let backend = self.backend.clone();
        let user_id = self.user_id.clone();
        // The sidebar groups tasks by project, so list every root. Opening
        // a task under another root switches the runtime first (see
        // open_session), which keeps tools running in the right directory.
        self.call(
            async move { backend.list_sessions(&user_id, None).await },
            cx,
            |this, result, cx| {
                match result {
                    Ok(sessions) => {
                        this.sessions = sessions;
                        this.rebuild_project_groups();
                        if this.selected_session.is_none() {
                            let root = this.project_root.clone();
                            let latest = this
                                .sessions
                                .iter()
                                .find(|session| Some(&session.project_root) == root.as_ref())
                                .cloned();
                            if let Some(latest) = latest {
                                let id = latest.id;
                                this.select_session(&id, cx);
                            } else {
                                this.new_session(cx);
                            }
                        }
                    }
                    Err(message) => this.notice = Some(message.into()),
                }
                cx.notify();
            },
        );
    }

    fn new_session(&mut self, cx: &mut Context<Self>) {
        // A boot-time auto-create and a user click can race; one only.
        if self.session_setup_pending {
            return;
        }
        self.session_setup_pending = true;
        let backend = self.backend.clone();
        let user_id = self.user_id.clone();
        self.call(
            async move {
                backend
                    .create_session(&user_id, None)
                    .await
                    .map(|detail| detail.session)
            },
            cx,
            |this, result, cx| match result {
                Ok(session) => {
                    this.session_setup_pending = false;
                    // The SessionCreated event may arrive before this
                    // callback; upsert so the sidebar never shows the task
                    // twice.
                    this.upsert_session(session.clone());
                    let timeline = Vec::new();
                    this.set_active_session(session, timeline, cx);
                    if !this.default_web_enabled {
                        this.set_web_enabled(false, cx);
                    }
                }
                Err(message) => {
                    this.session_setup_pending = false;
                    this.notice = Some(message.into());
                }
            },
        );
    }

    /// Sidebar click: switch the runtime root first when the task lives
    /// under another project, then load it.
    fn open_session(&mut self, session_id: &str, cx: &mut Context<Self>) {
        let root = self
            .sessions
            .iter()
            .find(|session| session.id == session_id)
            .map(|session| session.project_root.clone());
        match root {
            Some(root) if self.project_root.as_deref() != Some(root.as_str()) => {
                self.pending_session_select = Some(session_id.to_string());
                self.switch_root(root, cx);
            }
            _ => self.select_session(session_id, cx),
        }
    }

    fn select_session(&mut self, session_id: &str, cx: &mut Context<Self>) {
        self.load_session(session_id, LoadMode::Select, LOAD_RETRIES, cx);
    }

    /// Replace the timeline after a mid-run history compaction without
    /// disturbing the active run or a pending permission.
    fn reload_timeline(&mut self, session_id: &str, cx: &mut Context<Self>) {
        self.load_session(session_id, LoadMode::Reload, LOAD_RETRIES, cx);
    }

    /// Fetch a session snapshot and apply it. Events that land while the
    /// snapshot is in flight make it stale (the revision moved); the load
    /// is then repeated up to `retries` times and finally applied as is,
    /// so a session that streams while it is opened never stays blank.
    fn load_session(
        &mut self,
        session_id: &str,
        mode: LoadMode,
        retries: u8,
        cx: &mut Context<Self>,
    ) {
        self.selection_generation += 1;
        let generation = self.selection_generation;
        let revision = *self.timeline_revisions.get(session_id).unwrap_or(&0);
        let backend = self.backend.clone();
        let user_id = self.user_id.clone();
        let session_id = session_id.to_string();
        let target = session_id.clone();
        self.call(
            async move {
                // The stored summaries load off-thread while the runtime
                // builds the session detail.
                let store = backend.clone();
                let store_user = user_id.clone();
                let store_target = target.clone();
                let summaries = tokio::task::spawn_blocking(move || {
                    store.load_tool_summaries_blocking(&store_user, &store_target)
                });
                let (detail, summaries) =
                    tokio::join!(backend.load_session(&user_id, &target), summaries);
                let summaries = summaries
                    .map_err(|error| error.to_string())?
                    .unwrap_or_else(|error| {
                        log::warn!("Cannot load tool summaries: {error}");
                        HashMap::new()
                    });
                Ok::<_, String>((detail?, summaries))
            },
            cx,
            move |this, result, cx| {
                // A newer selection (or reload) superseded this load.
                if this.selection_generation != generation {
                    return;
                }
                let (detail, summaries) = match result {
                    Ok(detail) => detail,
                    Err(message) => {
                        this.notice = Some(message.into());
                        return;
                    }
                };
                let current = *this
                    .timeline_revisions
                    .get(&detail.session.id)
                    .unwrap_or(&0);
                if current != revision && retries > 0 {
                    this.load_session(&session_id, mode, retries - 1, cx);
                    return;
                }
                // Stored summaries stand in for the model calls the
                // timeline would otherwise request again.
                this.tool_summaries.extend(
                    summaries
                        .into_iter()
                        .map(|(id, summary)| (id, SharedString::from(summary))),
                );
                match mode {
                    LoadMode::Select => {
                        this.upsert_session(detail.session.clone());
                        this.set_active_session(detail.session, detail.timeline, cx);
                        this.queue = detail.queue.items;
                    }
                    LoadMode::Reload => {
                        this.replace_timeline(detail.timeline);
                        this.load_attachment_images(cx);
                        this.summarize_loaded_tools(cx);
                        cx.notify();
                    }
                }
            },
        );
    }

    /// Install a new timeline and reset every per-item structure that is
    /// keyed by its contents.
    fn replace_timeline(&mut self, timeline: Vec<AgentTimelineItem>) {
        self.timeline = timeline;
        self.timeline_index = self
            .timeline
            .iter()
            .enumerate()
            .map(|(index, item)| (item.id.clone(), (index, 0)))
            .collect();
        self.markdown_cache.clear();
        self.derived.clear();
        self.list_state.reset(self.timeline.len());
    }

    #[allow(dead_code)] // No delete affordance in the UI yet.
    fn delete_session(&self, session_id: &str, cx: &mut Context<Self>) {
        let backend = self.backend.clone();
        let user_id = self.user_id.clone();
        let session_id = session_id.to_string();
        let deleted_id = session_id.clone();
        self.call(
            async move { backend.delete_session(&user_id, &session_id).await },
            cx,
            move |this, result, cx| {
                if result.is_ok() {
                    this.sessions.retain(|session| session.id != deleted_id);
                    if this.selected_session.as_deref() == Some(&*deleted_id) {
                        this.selected_session = None;
                        this.replace_timeline(Vec::new());
                        // Stay in the runtime's root: a task from another
                        // project would run its tools in the wrong folder.
                        let root = this.project_root.clone();
                        let next = this
                            .sessions
                            .iter()
                            .find(|s| !s.archived && Some(&s.project_root) == root.as_ref())
                            .map(|s| s.id.clone());
                        if let Some(id) = next {
                            this.select_session(&id, cx);
                        }
                    }
                }
                cx.notify();
            },
        );
    }

    /// Apply settings-default changes when returning from the settings
    /// screen: tool verbosity updates live; the permission default only
    /// affects sessions that still follow the default.
    pub fn apply_defaults(
        &mut self,
        settings: &crate::settings::AppSettings,
        cx: &mut Context<Self>,
    ) {
        self.tool_details = settings.tool_details;
        self.default_web_enabled = settings.default_web_enabled;
        self.notify_enabled = settings.desktop_notifications;
        self.summaries_enabled = settings.tool_summaries;
        self.tts_voice.clone_from(&settings.tts_voice);
        self.tts_speed = settings.tts_speed;
        self.pinned_roots = settings.pinned_roots.clone();
        self.rebuild_project_groups();
        if self.uses_default_permission_mode
            && matches!(
                settings.default_permission_mode.as_str(),
                "auto" | "smart_approve"
            )
        {
            self.permission_mode
                .clone_from(&settings.default_permission_mode);
            self.apply_permission_mode(cx);
        }
        // Servers may have been added or removed in settings.
        self.refresh_session_mcp(cx);
        cx.notify();
    }

    fn refresh_context_usage(&self, cx: &mut Context<Self>) {
        let Some(session_id) = self.selected_session.clone() else {
            return;
        };
        let backend = self.backend.clone();
        let user_id = self.user_id.clone();
        let model = self.selected_model.clone();
        self.call(
            async move {
                backend
                    .context_usage(&user_id, &session_id, model.as_deref())
                    .await
            },
            cx,
            |this, result, cx| {
                if let Ok(Some((tokens, limit))) = result {
                    this.apply_context_usage(tokens, limit, cx);
                }
            },
        );
    }

    fn apply_context_usage(&mut self, tokens: i64, limit: i64, cx: &mut Context<Self>) {
        self.ledger_context_tokens = tokens;
        self.context_limit = limit;
        let fraction = if limit > 0 {
            tokens as f32 / limit as f32
        } else {
            0.0
        };
        if self.context_fraction != Some(fraction) {
            self.context_fraction = Some(fraction);
            cx.notify();
        }
    }

    /// Fallback refresh of context usage while the selected session runs.
    /// Turn boundaries refresh it directly (completed items, Finished);
    /// this timer only covers a missed event, so it is slow and skips
    /// ticks where the timeline did not move.
    fn start_usage_poller(&mut self, session_id: String, cx: &mut Context<Self>) {
        if self.usage_poller_active {
            return;
        }
        self.usage_poller_active = true;
        let target = session_id;
        let mut last_revision = *self.timeline_revisions.get(&target).unwrap_or(&0);
        cx.spawn(async move |this, cx| {
            loop {
                cx.background_executor()
                    .timer(std::time::Duration::from_millis(5000))
                    .await;
                let keep_going = this
                    .update(cx, |this: &mut ChatScreen, cx| {
                        let running = this
                            .selected_session
                            .as_deref()
                            .is_some_and(|selected| selected == target.as_str())
                            && this.active_runs.contains_key(&target);
                        let revision = *this.timeline_revisions.get(&target).unwrap_or(&0);
                        if running && revision != last_revision {
                            last_revision = revision;
                            this.refresh_context_usage(cx);
                        }
                        running
                    })
                    .unwrap_or(false);
                if !keep_going {
                    this.update(cx, |this: &mut ChatScreen, _cx| {
                        this.usage_poller_active = false;
                    })
                    .ok();
                    return;
                }
            }
        })
        .detach();
    }

    fn compact_now(&mut self, cx: &mut Context<Self>) {
        let Some(session_id) = self.selected_session.clone() else {
            return;
        };
        let backend = self.backend.clone();
        let user_id = self.user_id.clone();
        self.notice = Some("Compacting…".into());
        cx.notify();
        self.call(
            async move { backend.compact_session(&user_id, &session_id).await },
            cx,
            |this, result, cx| match result {
                Ok(()) => {
                    this.notice = Some("Conversation compacted".into());
                    let sid = this.selected_session.clone().unwrap_or_default();
                    this.select_session(&sid, cx);
                    this.refresh_context_usage(cx);
                }
                Err(message) => this.notice = Some(format!("Compaction failed: {message}").into()),
            },
        );
    }

    fn apply_permission_mode(&self, cx: &mut Context<Self>) {
        let Some(session_id) = self.selected_session.clone() else {
            return;
        };
        let backend = self.backend.clone();
        let user_id = self.user_id.clone();
        let mode = self.permission_mode.clone();
        self.call(
            async move {
                backend
                    .set_permission_mode(&user_id, &session_id, &mode)
                    .await
            },
            cx,
            |this, result, cx| {
                if let Err(message) = result {
                    this.notice = Some(format!("Could not set permission mode: {message}").into());
                    cx.notify();
                }
            },
        );
    }

    fn set_active_session(
        &mut self,
        session: AgentSessionSummary,
        timeline: Vec<AgentTimelineItem>,
        cx: &mut Context<Self>,
    ) {
        self.selected_session = Some(session.id);
        self.abandon_queue_edit(cx);
        self.queue.clear();
        // Adopt the session's stored policy; it persists per session in the
        // runtime.
        let mode = session.mode;
        if mode == "auto" || mode == "smart_approve" {
            self.permission_mode = mode;
        }
        self.replace_timeline(timeline);
        // Ordinals restart for the new session; drop any stale selection.
        if let Some(selection) = &self.selection {
            selection.update(cx, |selection, _| selection.clear());
        }
        // Summaries in flight belong to the previous screen state; their
        // results are dropped and the queue restarts for this session.
        self.summary_generation += 1;
        self.pending_summaries = 0;
        self.summary_queue.clear();
        self.summary_requests.clear();
        self.load_attachment_images(cx);
        self.summarize_loaded_tools(cx);
        self.follow_transcript = true;
        self.awaiting_first_token = false;
        self.lightbox = None;
        self.pending_permission = None;
        self.permission_responding = false;
        self.models_menu_open = false;
        self.root_menu_open = false;
        self.mcp_menu_open = false;
        // Questions stay queued per session; the card for this session
        // starts on its first step with nothing picked.
        self.reset_question_card(cx);
        self.refresh_session_mcp(cx);
        cx.notify();
    }

    /// The question shown for the selected session, if any. Other
    /// sessions' questions stay queued until the user switches to them.
    fn current_question(&self) -> Option<&PendingQuestion> {
        let selected = self.selected_session.as_deref()?;
        self.pending_questions
            .iter()
            .find(|question| question.session_id == selected)
    }

    /// Start the displayed card over: drop the step state and the answer
    /// input, then recreate the input when a question is showing.
    fn reset_question_card(&mut self, cx: &mut Context<Self>) {
        self.question_selected.clear();
        self.question_step = 0;
        self.question_step_answers.clear();
        if let Some(input) = self.pending_question_input.take() {
            input.update(cx, |input, cx| input.clear(cx));
        }
        if self.current_question().is_some() {
            self.ensure_question_input(cx);
            self.question_focus_pending = true;
        }
    }

    /// Look up the catalog vision flag for the selected model once.
    fn refresh_vision(&mut self, cx: &mut Context<Self>) {
        let Some(model) = self.selected_model.clone() else {
            return;
        };
        if self.model_vision.contains_key(&model) {
            return;
        }
        let backend = self.backend.clone();
        let user_id = self.user_id.clone();
        let lookup = model.clone();
        self.call(
            async move { backend.model_supports_vision(&user_id, &lookup).await },
            cx,
            move |this, result, cx| {
                if let Ok(Some(vision)) = result {
                    this.model_vision.insert(model, vision);
                    cx.notify();
                }
            },
        );
    }

    fn selected_model_supports_vision(&self) -> bool {
        self.selected_model
            .as_ref()
            .and_then(|model| self.model_vision.get(model))
            .copied()
            .unwrap_or(false)
    }
    /// Reload the skill slash commands for the current project root.
    pub fn refresh_slash_commands(&mut self, cx: &mut Context<Self>) {
        let backend = self.backend.clone();
        let working_dir = self.project_root.clone();
        self.call(
            async move { backend.list_slash_commands(working_dir).await },
            cx,
            move |this, result, cx| {
                if let Ok(commands) = result {
                    this.slash_commands = commands;
                    cx.notify();
                }
            },
        );
    }

    /// Reload the MCP server list for the selected task.
    pub fn refresh_session_mcp(&mut self, cx: &mut Context<Self>) {
        let Some(session_id) = self.selected_session.clone() else {
            self.session_mcp.clear();
            return;
        };
        let backend = self.backend.clone();
        let user_id = self.user_id.clone();
        let target = session_id.clone();
        self.call(
            async move { backend.list_session_mcp_servers(&user_id, &target).await },
            cx,
            move |this, result, cx| {
                if this.selected_session.as_deref() != Some(session_id.as_str()) {
                    return;
                }
                match result {
                    Ok(servers) => this.session_mcp = servers,
                    Err(message) => log::debug!("mcp list failed: {message}"),
                }
                cx.notify();
            },
        );
    }

    fn toggle_session_mcp(&mut self, name: String, enabled: bool, cx: &mut Context<Self>) {
        let Some(session_id) = self.selected_session.clone() else {
            return;
        };
        let backend = self.backend.clone();
        let user_id = self.user_id.clone();
        let target = session_id.clone();
        self.call(
            async move {
                backend
                    .set_session_mcp_server_enabled(&user_id, &target, &name, enabled)
                    .await
            },
            cx,
            move |this, result, cx| {
                if this.selected_session.as_deref() != Some(session_id.as_str()) {
                    return;
                }
                match result {
                    Ok(servers) => this.session_mcp = servers,
                    Err(message) => this.notice = Some(message.into()),
                }
                cx.notify();
            },
        );
    }

    /// Persist the web flag for the selected task; the runtime applies it
    /// on the next turn.
    fn set_web_enabled(&mut self, enabled: bool, cx: &mut Context<Self>) {
        let Some(session_id) = self.selected_session.clone() else {
            return;
        };
        let previous = self.web_enabled;
        self.web_enabled = enabled;
        cx.notify();
        let backend = self.backend.clone();
        let user_id = self.user_id.clone();
        let target = session_id.clone();
        self.call(
            async move {
                backend
                    .set_session_web_enabled(&user_id, &target, enabled)
                    .await
            },
            cx,
            move |this, result, cx| {
                match result {
                    Ok(session) => {
                        this.upsert_session(session);
                    }
                    Err(message) => {
                        if this.selected_session.as_deref() == Some(session_id.as_str()) {
                            this.web_enabled = previous;
                        }
                        this.notice =
                            Some(format!("Could not change web access: {message}").into());
                    }
                }
                cx.notify();
            },
        );
    }

    fn toggle_composer_expanded(&mut self, cx: &mut Context<Self>) {
        self.composer_expanded = !self.composer_expanded;
        let expanded = self.composer_expanded;
        if let Some(composer) = self.composer.clone() {
            composer.update(cx, |input, cx| input.set_fill_height(expanded, cx));
        }
        cx.notify();
    }

    /// How many more images the draft can take, or `None` with a notice
    /// set when the plan or the limit blocks attachments.
    fn remaining_image_slots(&mut self, cx: &mut Context<Self>) -> Option<usize> {
        if let Some(plan) = self.sidebar_plan.as_ref() {
            let label = plan.plan_label.to_lowercase();
            if !(label.contains("pro") || label.contains("max") || label.contains("team")) {
                self.notice = Some("Image attachments need a Pro, Max, or Team plan".into());
                cx.notify();
                return None;
            }
        }
        let remaining = MAX_DRAFT_IMAGES.saturating_sub(self.draft_images.len());
        if remaining == 0 {
            self.notice =
                Some(format!("Attach at most {MAX_DRAFT_IMAGES} images at a time").into());
            cx.notify();
            return None;
        }
        Some(remaining)
    }

    /// Stage an image pasted into the composer from the clipboard.
    fn paste_image(&mut self, image: gpui::Image, cx: &mut Context<Self>) {
        if self.remaining_image_slots(cx).is_none() {
            return;
        }
        let extension = match image.format {
            gpui::ImageFormat::Jpeg => "jpg",
            gpui::ImageFormat::Webp => "webp",
            _ => "png",
        };
        let name = format!("pasted-{}.{extension}", self.draft_images.len() + 1);
        let id = self.next_draft_id();
        match draft_image_from_bytes(id, name, &image.bytes) {
            Ok(draft) => {
                self.notice = None;
                self.draft_images.push(draft);
                self.crop_draft_thumbnail(id, image.bytes, cx);
            }
            Err(message) => self.notice = Some(message.into()),
        }
        cx.notify();
    }

    fn next_draft_id(&mut self) -> u64 {
        self.draft_counter += 1;
        self.draft_counter
    }

    /// Decode and center-crop off the UI thread, then attach the result to
    /// the draft with `id` if it is still staged.
    fn crop_draft_thumbnail(&mut self, id: u64, bytes: Vec<u8>, cx: &mut Context<Self>) {
        self.call(
            async move {
                tokio::task::spawn_blocking(move || square_thumbnail(&bytes))
                    .await
                    .map_err(|error| format!("Thumbnail task failed: {error}"))?
            },
            cx,
            move |this, result, cx| {
                let Some(draft) = this.draft_images.iter_mut().find(|draft| draft.id == id) else {
                    return;
                };
                match result {
                    Ok(image) => draft.thumbnail = Some(Arc::new(image)),
                    Err(message) => log::debug!("thumbnail for {}: {message}", draft.name),
                }
                cx.notify();
            },
        );
    }

    fn pick_images(&mut self, cx: &mut Context<Self>) {
        if self.image_picking {
            return;
        }
        let Some(remaining) = self.remaining_image_slots(cx) else {
            return;
        };
        self.image_picking = true;
        self.notice = None;
        cx.notify();
        self.call(
            async move {
                tokio::task::spawn_blocking(move || {
                    let output = std::process::Command::new("zenity")
                        .args([
                            "--file-selection",
                            "--multiple",
                            "--separator=\n",
                            "--title=Add images",
                            "--file-filter=Images | *.png *.jpg *.jpeg *.webp *.PNG *.JPG *.JPEG *.WEBP",
                        ])
                        .output()
                        .map_err(|error| format!("Could not open the file dialog: {error}"))?;
                    if !output.status.success() {
                        // Cancelled.
                        return Ok(Vec::new());
                    }
                    let mut images = Vec::new();
                    for line in String::from_utf8_lossy(&output.stdout).lines() {
                        let path = std::path::PathBuf::from(line.trim());
                        if path.as_os_str().is_empty() {
                            continue;
                        }
                        images.push(load_draft_image(&path)?);
                        if images.len() >= remaining {
                            break;
                        }
                    }
                    Ok(images)
                })
                .await
                .map_err(|error| format!("Image picker failed: {error}"))?
            },
            cx,
            |this, result, cx| {
                this.image_picking = false;
                match result {
                    Ok(images) => {
                        for mut image in images {
                            image.id = this.next_draft_id();
                            this.draft_images.push(image);
                        }
                    }
                    Err(message) => this.notice = Some(message.into()),
                }
                cx.notify();
            },
        );
    }

    pub fn set_update(&mut self, info: crate::update::UpdateInfo, cx: &mut Context<Self>) {
        self.update = Some(info);
        cx.notify();
    }

    /// Banner for a newer release, with a link to its page.
    fn render_update_banner(&self, cx: &mut Context<Self>) -> Option<Div> {
        let info = self.update.as_ref()?;
        let url = info.url.clone();
        Some(
            div()
                .flex()
                .items_center()
                .gap_3()
                .px_3()
                .py_2()
                .rounded_md()
                .bg(gpui::rgb(theme::bg_elevated()))
                .border_1()
                .border_color(gpui::rgb(theme::border()))
                .text_sm()
                .text_color(gpui::rgb(theme::text_primary()))
                .child(icon("arrow-up", px(14.), theme::accent()))
                .child(
                    div()
                        .flex_1()
                        .child(format!("Maple v{} is available", info.version)),
                )
                .child(
                    div()
                        .id("update-open")
                        .px_2()
                        .py_0p5()
                        .rounded_md()
                        .text_color(gpui::rgb(theme::accent()))
                        .hover(|style| style.bg(theme::overlay_hover()).cursor_pointer())
                        .on_click(cx.listener(move |this, _event, _window, cx| {
                            if let Err(error) = webbrowser::open(&url) {
                                this.notice =
                                    Some(format!("Could not open browser: {error}").into());
                            }
                            cx.notify();
                        }))
                        .child("Download"),
                )
                .child(
                    div()
                        .id("update-dismiss")
                        .size_5()
                        .flex()
                        .items_center()
                        .justify_center()
                        .rounded_md()
                        .hover(|style| style.bg(theme::overlay_hover()).cursor_pointer())
                        .on_click(cx.listener(|this, _event, _window, cx| {
                            this.update = None;
                            cx.notify();
                        }))
                        .child(icon("x", px(12.), theme::text_secondary())),
                ),
        )
    }

    /// Keep a sent prompt for Up/Down recall. Repeats move to the end.
    fn remember_prompt(&mut self, text: &str) {
        let text = text.trim();
        self.history_index = None;
        self.history_draft.clear();
        if text.is_empty() {
            return;
        }
        self.prompt_history.retain(|entry| entry != text);
        self.prompt_history.push(text.to_string());
        if self.prompt_history.len() > PROMPT_HISTORY_LIMIT {
            let excess = self.prompt_history.len() - PROMPT_HISTORY_LIMIT;
            self.prompt_history.drain(..excess);
        }
    }

    /// Up/Down in the composer walks sent prompts when the composer is
    /// empty or still shows a recalled prompt. Returns the text to show,
    /// or `None` when the key should move the caret instead.
    fn recall_prompt(&mut self, key: &str, current: &str) -> Option<String> {
        let browsing = self
            .history_index
            .and_then(|index| self.prompt_history.get(index))
            .is_some_and(|entry| entry == current);
        if !browsing && !current.trim().is_empty() {
            return None;
        }
        if self.prompt_history.is_empty() {
            return None;
        }
        match key {
            "up" => {
                let next = match self.history_index {
                    Some(0) => return None,
                    Some(index) if browsing => index - 1,
                    _ => {
                        self.history_draft = current.to_string();
                        self.prompt_history.len() - 1
                    }
                };
                self.history_index = Some(next);
                Some(self.prompt_history[next].clone())
            }
            "down" => {
                let index = self.history_index?;
                if !browsing {
                    return None;
                }
                if index + 1 < self.prompt_history.len() {
                    self.history_index = Some(index + 1);
                    Some(self.prompt_history[index + 1].clone())
                } else {
                    self.history_index = None;
                    Some(std::mem::take(&mut self.history_draft))
                }
            }
            _ => None,
        }
    }

    /// Stage images dropped onto the composer. Files that are not PNG,
    /// JPEG, or WebP are skipped with a notice.
    fn add_image_paths(&mut self, paths: Vec<std::path::PathBuf>, cx: &mut Context<Self>) {
        let (images, other): (Vec<_>, Vec<_>) = paths.into_iter().partition(|path| {
            path.extension()
                .and_then(|extension| extension.to_str())
                .is_some_and(|extension| {
                    matches!(
                        extension.to_ascii_lowercase().as_str(),
                        "png" | "jpg" | "jpeg" | "webp"
                    )
                })
        });
        if !other.is_empty() {
            self.notice = Some(
                format!(
                    "Only PNG, JPEG, and WebP images can be attached ({} file{} skipped)",
                    other.len(),
                    if other.len() == 1 { "" } else { "s" }
                )
                .into(),
            );
            cx.notify();
        }
        if images.is_empty() {
            return;
        }
        let Some(remaining) = self.remaining_image_slots(cx) else {
            return;
        };
        self.call(
            async move {
                tokio::task::spawn_blocking(move || {
                    images
                        .iter()
                        .take(remaining)
                        .map(|path| load_draft_image(path))
                        .collect::<Result<Vec<_>, String>>()
                })
                .await
                .map_err(|error| format!("Image load failed: {error}"))?
            },
            cx,
            |this, result, cx| {
                match result {
                    Ok(images) => {
                        for mut image in images {
                            image.id = this.next_draft_id();
                            this.draft_images.push(image);
                        }
                    }
                    Err(message) => this.notice = Some(message.into()),
                }
                cx.notify();
            },
        );
    }

    fn is_run_active(&self) -> bool {
        self.selected_session
            .as_ref()
            .is_some_and(|session| self.active_runs.contains_key(session))
    }

    /// Escape, in priority order: close the photo viewer, skip a pending
    /// question, clear a text selection, close the chip menus.
    fn chat_escape(&mut self, _: &ChatEscape, _window: &mut Window, cx: &mut Context<Self>) {
        if self.rename.is_some() {
            self.cancel_rename(cx);
            return;
        }
        if self.queue_edit.is_some() {
            self.discard_queue_edit(cx);
            return;
        }
        if !self.sidebar_filter.is_empty() {
            self.clear_search(cx);
            return;
        }
        if self.confirm_remove_root.is_some() || self.project_menu.is_some() {
            self.confirm_remove_root = None;
            self.project_menu = None;
            cx.notify();
            return;
        }
        if self.lightbox.is_some() {
            self.lightbox = None;
            cx.notify();
            return;
        }
        if self.transcript_menu.is_some() {
            self.transcript_menu = None;
            cx.notify();
            return;
        }
        if self.current_question().is_some() {
            self.skip_question(cx);
            return;
        }
        if let Some(selection) = self.selection.clone()
            && selection.read(cx).has_selection()
        {
            selection.update(cx, |selection, _| selection.clear());
            cx.notify();
            return;
        }
        self.close_menus_on_escape(cx);
    }

    /// Escape with nothing else to dismiss: close whichever chip menu is
    /// open (including the folder picker).
    fn close_menus_on_escape(&mut self, cx: &mut Context<Self>) {
        if self.models_menu_open || self.mode_menu_open || self.mcp_menu_open || self.root_menu_open
        {
            self.models_menu_open = false;
            self.mode_menu_open = false;
            self.mcp_menu_open = false;
            self.root_menu_open = false;
            cx.notify();
        }
    }

    /// Copy the transcript drag selection, when there is one.
    fn copy_selection(&mut self, _: &CopySelection, _window: &mut Window, cx: &mut Context<Self>) {
        self.copy_selected_text(cx);
    }

    fn copy_selected_text(&mut self, cx: &mut Context<Self>) {
        let Some(selection) = self.selection.clone() else {
            return;
        };
        let text = selection.read(cx).selected_text();
        if !text.is_empty() {
            cx.write_to_clipboard(gpui::ClipboardItem::new_string(text));
        }
    }

    fn select_all_transcript(
        &mut self,
        _: &SelectAllTranscript,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.select_all_text(cx);
    }

    /// Plain typing with no text input focused routes to the composer:
    /// focus it and insert the character so the first keystroke lands
    /// instead of being lost waiting for the next frame's input handler.
    fn type_into_composer(
        &mut self,
        event: &gpui::KeyDownEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(composer) = self.composer.clone() else {
            return;
        };
        // The platforms deliver a keystroke to a text input only when it
        // is unmodified text (shift alone allowed); apply the same rule
        // here so chords like ctrl-c keep their meaning. Enter and tab are
        // excluded: macOS reports a key_char for them, but they must keep
        // their focus-navigation and send meaning.
        let Some(text) = event.keystroke.key_char.clone() else {
            return;
        };
        if matches!(event.keystroke.key.as_str(), "enter" | "tab")
            || !event
                .keystroke
                .modifiers
                .is_subset_of(&gpui::Modifiers::shift())
        {
            return;
        }
        // A focused text input already receives typing.
        let focused = window.focused(cx);
        let typing_here = [
            self.composer.as_ref(),
            self.search_input.as_ref(),
            self.rename_input.as_ref(),
            self.pending_question_input.as_ref(),
        ]
        .into_iter()
        .flatten()
        .any(|input| Some(input.read(cx).focus_handle(cx)) == focused);
        if typing_here {
            return;
        }
        let handle = composer.read(cx).focus_handle(cx);
        window.focus(&handle);
        composer.update(cx, |input, cx| {
            input.replace_text_in_range(None, &text, window, cx)
        });
        cx.stop_propagation();
    }

    /// Select every message in the transcript. Paragraphs that are not on
    /// screen have never registered their text, so register them here;
    /// the ordinals mirror the ones `render_message` assigns.
    fn select_all_text(&mut self, cx: &mut Context<Self>) {
        let Some(selection) = self.selection.clone() else {
            return;
        };
        selection.update(cx, |selection, cx| {
            for item in &self.timeline {
                if item.item_type != "message" {
                    continue;
                }
                let text = item.text.as_deref().unwrap_or("");
                if text.trim().is_empty() {
                    continue;
                }
                let base = self.markdown_cache.ordinal_for(&item.id);
                let revision = self
                    .timeline_index
                    .get(&item.id)
                    .map_or(0, |(_, revision)| *revision);
                if item.role.as_deref() == Some("user") {
                    let derived = self.derived.get(item, revision);
                    selection.register(base + 2048, &derived.text);
                    continue;
                }
                let document =
                    self.markdown_cache
                        .get(&item.id, MarkdownKind::Body, revision, text);
                for (index, block) in document.blocks.iter().enumerate() {
                    if let markdown::Block::Text { text, .. } = block {
                        selection.register(base + index as u64, text);
                    }
                }
            }
            selection.select_all();
            cx.notify();
        });
        cx.notify();
    }

    /// Right-click menu over the transcript: copy the selection, select all.
    fn render_transcript_menu(&self, cx: &mut Context<Self>) -> Option<gpui::Deferred> {
        let position = self.transcript_menu?;
        let has_selection = self
            .selection
            .as_ref()
            .is_some_and(|selection| selection.read(cx).has_selection());
        let item = |id: &'static str,
                    icon_name: &'static str,
                    label: &'static str,
                    on_click: MenuAction| {
            div()
                .id(id)
                .flex()
                .items_center()
                .gap_2()
                .px_3()
                .py_1p5()
                .text_sm()
                .text_color(gpui::rgb(theme::text_primary()))
                .hover(|style| {
                    style
                        .bg(gpui::rgb(theme::bg_sidebar_row_hover()))
                        .cursor_pointer()
                })
                .on_click(cx.listener(move |this, _event, _window, cx| {
                    cx.stop_propagation();
                    this.transcript_menu = None;
                    on_click(this, cx);
                    cx.notify();
                }))
                .child(icon(icon_name, px(14.), theme::text_secondary()))
                .child(label)
        };
        Some(gpui::deferred(
            gpui::anchored()
                .position(position)
                .snap_to_window_with_margin(px(8.))
                .child(
                    div()
                        .id("transcript-menu")
                        .occlude()
                        .w(px(160.))
                        .py_1()
                        .rounded_md()
                        .bg(gpui::rgb(theme::bg_elevated()))
                        .border_1()
                        .border_color(gpui::rgb(theme::border()))
                        .shadow_md()
                        .flex()
                        .flex_col()
                        .on_mouse_down_out(cx.listener(|this, _event, _window, cx| {
                            this.transcript_menu = None;
                            cx.notify();
                        }))
                        .when(has_selection, |menu| {
                            menu.child(item(
                                "transcript-menu-copy",
                                "copy",
                                "Copy",
                                Box::new(|this, cx| this.copy_selected_text(cx)),
                            ))
                        })
                        .child(item(
                            "transcript-menu-select-all",
                            "text-select",
                            "Select all",
                            Box::new(|this, cx| this.select_all_text(cx)),
                        )),
                ),
        ))
    }

    /// Toggle one tool card's expansion and re-measure its row.
    fn toggle_tool(&mut self, item_id: &str, ix: usize, cx: &mut Context<Self>) {
        if self.toggled_tools.contains(item_id) {
            self.toggled_tools.remove(item_id);
        } else {
            self.toggled_tools.insert(item_id.to_string());
        }
        self.list_state.splice(ix..ix + 1, 1);
        cx.notify();
    }

    /// Show a clicked attachment image full size.
    fn open_lightbox(&mut self, image: Arc<gpui::Image>, cx: &mut Context<Self>) {
        self.lightbox = Some(image);
        cx.notify();
    }
    /// Send without a Window, callable from the composer's Enter hook and
    /// from the Send button.
    /// Send without a Window, callable from the Send button. The button
    /// path owns no composer lease, so it clears the input here.
    fn send_inner(&mut self, cx: &mut Context<Self>) {
        let Some(composer) = self.composer.clone() else {
            return;
        };
        let text = composer.read(cx).text();
        composer.update(cx, |input, cx| input.clear(cx));
        self.send_text(text, cx);
    }

    /// Send the given text; used by the composer Enter hook, which already
    /// holds the text and clears the input itself.
    fn send_text(&mut self, text: String, cx: &mut Context<Self>) {
        self.send_text_with(text, false, cx);
    }

    /// Ctrl/Cmd+Enter: send into the active run instead of behind it.
    /// With no run active this is a plain send.
    fn steer_text(&mut self, text: String, cx: &mut Context<Self>) {
        self.send_text_with(text, true, cx);
    }

    fn send_text_with(&mut self, text: String, steer: bool, cx: &mut Context<Self>) {
        let Some(session_id) = self.selected_session.clone() else {
            self.notice = Some("Create a task first".into());
            cx.notify();
            return;
        };
        if self.booting {
            self.notice = Some("Agent runtime is still starting".into());
            cx.notify();
            return;
        }
        if self.current_question().is_some() {
            // The run is blocked on the question; sending here would queue a
            // message nobody reads and look like a stall.
            self.notice = Some("Answer the question above first".into());
            self.question_focus_pending = true;
            cx.notify();
            return;
        }
        if text.trim().is_empty() && self.draft_images.is_empty() {
            return;
        }
        // Enter on an open palette runs the highlighted command instead of
        // sending the partial token.
        if let Some(command) = self.slash_palette_command(text.trim()) {
            self.slash_selected = None;
            if let Some(composer) = self.composer.clone() {
                composer.update(cx, |input, cx| input.clear(cx));
            }
            self.try_command(&session_id, &format!("/{command}"), cx);
            return;
        }
        if self.try_command(&session_id, text.trim(), cx) {
            // Commands never echo into the transcript; drop the typed text
            // so the palette cannot survive the execution.
            self.slash_selected = None;
            if let Some(composer) = self.composer.clone() {
                composer.update(cx, |input, cx| input.clear(cx));
            }
            return;
        }
        let queue_id = self.queue_edit.as_ref().map(|edit| edit.queue_id.clone());
        self.send_to_session(&session_id, text, steer, queue_id, cx);
    }

    /// The command Enter should run when the slash palette is open: the
    /// highlighted entry, falling back to the only or first match. `None`
    /// when the palette is closed or the text is already a full command.
    fn slash_palette_command(&self, text: &str) -> Option<String> {
        if self.current_question().is_some() {
            return None;
        }
        let entries = &self.slash_entries;
        if entries.is_empty() {
            return None;
        }
        // A full exact match runs through the normal command path so the
        // typed form (including arguments) is preserved.
        let exact = text
            .strip_prefix('/')
            .and_then(|body| body.split_whitespace().next())
            .map(|name| {
                entries
                    .iter()
                    .any(|entry| entry.name.eq_ignore_ascii_case(name))
            })
            .unwrap_or(false);
        if exact || text.contains(' ') {
            return None;
        }
        let index = self
            .slash_selected
            .filter(|index| *index < entries.len())
            .unwrap_or(0);
        Some(entries[index].name.clone())
    }

    /// Arrow/tab handling for the open slash palette; returns whether the
    /// key was consumed.
    fn navigate_slash_palette(&mut self, key: &str, token: &str, cx: &mut Context<Self>) -> bool {
        let entries = slash_entries_for(token, &self.slash_commands);
        let count = entries.len();
        if count == 0 {
            return false;
        }
        match key {
            "down" => {
                let next = self.slash_selected.map_or(0, |index| (index + 1) % count);
                self.slash_selected = Some(next);
            }
            "up" => {
                let next = self
                    .slash_selected
                    .map_or(count - 1, |index| index.max(1) - 1);
                self.slash_selected = Some(next);
            }
            _ => return false,
        }
        cx.notify();
        true
    }

    /// Replace the composer text with `/name ` and focus it.
    fn complete_slash_command(&mut self, name: &str, cx: &mut Context<Self>) {
        let completed = format!("/{name} ");
        if let Some(composer) = self.composer.clone() {
            composer.update(cx, |input, cx| input.set_text(&completed, cx));
        }
        self.slash_selected = None;
        cx.notify();
    }

    /// Execute a `/command` when it matches a built-in or a skill. Unknown
    /// commands fall through and are sent to the model as plain text.
    fn try_command(&mut self, session_id: &str, text: &str, cx: &mut Context<Self>) -> bool {
        let Some(body) = text.strip_prefix('/') else {
            return false;
        };
        let (name, args) = match body.split_once(char::is_whitespace) {
            Some((name, args)) => (name, args.trim()),
            None => (body, ""),
        };
        if name.is_empty() || name.contains('/') {
            return false;
        }
        let session_id = session_id.to_string();
        match name {
            "compact" => {
                self.compact_now(cx);
                true
            }
            "new" => {
                self.new_session(cx);
                true
            }
            "pin" => {
                if let Some(root) = self.project_root.clone() {
                    self.toggle_pin(&root, cx);
                } else {
                    self.notice = Some("No project to pin".into());
                    cx.notify();
                }
                true
            }
            "web" => {
                let next = !self.web_enabled;
                self.set_web_enabled(next, cx);
                true
            }
            "model" => {
                if args.is_empty() {
                    self.models_menu_open = true;
                    self.mode_menu_open = false;
                    self.mcp_menu_open = false;
                    self.root_menu_open = false;
                    cx.notify();
                } else {
                    let query = args.to_string();
                    match self
                        .models
                        .iter()
                        .find(|model| model.to_lowercase().contains(&query.to_lowercase()))
                    {
                        Some(model) => {
                            let model = model.clone();
                            self.pick_model(model, cx);
                        }
                        None => {
                            self.notice = Some(format!("No model matches “{query}”").into());
                            cx.notify();
                        }
                    }
                }
                true
            }
            "help" => {
                self.notice =
                    Some("Type / to list commands. Built-ins: /compact, /new, /pin, /web, /model, /help. Skills appear as /name.".into());
                cx.notify();
                true
            }
            _ => {
                // Skill commands resolve into the prompt that loads them.
                if !self
                    .slash_commands
                    .iter()
                    .any(|command| command.name.eq_ignore_ascii_case(name))
                {
                    return false;
                }
                let backend = self.backend.clone();
                let working_dir = self.project_root.clone();
                let command = name.to_string();
                let arguments = args.to_string();
                self.notice = Some("Loading skill…".into());
                cx.notify();
                self.call(
                    async move {
                        backend
                            .resolve_slash_command(working_dir, command, arguments)
                            .await
                    },
                    cx,
                    move |this, result, cx| match result {
                        Ok(Some(prompt)) => {
                            this.notice = None;
                            this.send_to_session(&session_id, prompt, false, None, cx);
                        }
                        Ok(None) => {
                            this.notice = Some("That skill is no longer installed".into());
                            cx.notify();
                        }
                        Err(message) => {
                            this.notice = Some(message.into());
                            cx.notify();
                        }
                    },
                );
                true
            }
        }
    }

    /// Dispatch a message. While a run is active the runtime queues it
    /// behind the run, or with `steer` injects it into the run. `queue_id`
    /// names an already queued chip to send in place of new text.
    fn send_to_session(
        &mut self,
        session_id: &str,
        text: String,
        steer: bool,
        queue_id: Option<String>,
        cx: &mut Context<Self>,
    ) {
        let session_id = session_id.to_string();
        let backend = self.backend.clone();
        let user_id = self.user_id.clone();
        let model = self.selected_model.clone();
        let vision_capable = self.selected_model_supports_vision();
        let run_active = self.active_runs.contains_key(&session_id);
        // A queued chip keeps its own attachments; new images stay staged.
        let drafts = if queue_id.is_some() {
            Vec::new()
        } else {
            std::mem::take(&mut self.draft_images)
        };
        let request = AgentSendMessageRequest {
            session_id: session_id.clone(),
            text: text.clone(),
            model,
            context_limit: None,
            mode: Some(self.permission_mode.clone()),
            vision_capable,
            steer: steer && run_active,
            queue_id,
            attachments: drafts
                .iter()
                .map(|image| AgentImageUpload {
                    name: image.name.clone(),
                    data_url: image.data_url.clone(),
                })
                .collect(),
        };
        self.notice = None;
        let editing = self
            .queue_edit
            .as_ref()
            .is_some_and(|edit| Some(&edit.queue_id) == request.queue_id.as_ref());
        let from_composer = request.queue_id.is_none() || editing;
        if from_composer {
            self.remember_prompt(&text);
            // Clear the composer at dispatch so no entry path can leave the
            // sent text behind; a failed send restores it below.
            if let Some(composer) = self.composer.clone() {
                composer.update(cx, |input, cx| input.clear(cx));
            }
        }
        if editing {
            // The edit is on its way; give the stashed draft back.
            self.finish_queue_edit(cx);
        }
        // Sending closes the chip menus anchored under the composer.
        self.models_menu_open = false;
        self.mode_menu_open = false;
        self.mcp_menu_open = false;
        self.root_menu_open = false;
        self.follow_transcript = true;
        if !run_active {
            self.awaiting_first_token = true;
        }
        cx.notify();
        self.call(
            async move { backend.send_message(&user_id, request).await },
            cx,
            move |this, result, cx| match result {
                Ok(run_id) => {
                    this.active_runs.insert(session_id.clone(), run_id);
                }
                Err(message) => {
                    // Show the failure in the transcript and give the draft
                    // back instead of silently dropping it. The composer
                    // belongs to the selected session: a draft from another
                    // task must not land in it.
                    this.push_local_error("Send failed", &message, cx);
                    if from_composer
                        && this.selected_session.as_deref() == Some(session_id.as_str())
                    {
                        if let Some(composer) = this.composer.clone() {
                            composer.update(cx, |input, cx| input.set_text(&text, cx));
                        }
                        let mut restored = drafts;
                        restored.append(&mut this.draft_images);
                        this.draft_images = restored;
                    } else {
                        this.notice = Some(format!("Send failed: {message}").into());
                    }
                    this.awaiting_first_token = false;
                }
            },
        );
    }

    /// Send a queued chip into the active run now.
    fn steer_queued(&mut self, queue_id: &str, cx: &mut Context<Self>) {
        let Some(session_id) = self.selected_session.clone() else {
            return;
        };
        let Some(item) = self.queue.iter().find(|item| item.queue_id == queue_id) else {
            return;
        };
        let text = item.text.clone();
        self.send_to_session(&session_id, text, true, Some(queue_id.to_string()), cx);
    }

    /// Drop a queued chip.
    fn remove_queued(&mut self, queue_id: &str, cx: &mut Context<Self>) {
        let Some(session_id) = self.selected_session.clone() else {
            return;
        };
        if self.queue_busy {
            return;
        }
        self.queue_busy = true;
        let backend = self.backend.clone();
        let user_id = self.user_id.clone();
        let queue_id = queue_id.to_string();
        let target = session_id.clone();
        self.call(
            async move {
                backend
                    .cancel_queued_message(&user_id, &session_id, &queue_id)
                    .await
            },
            cx,
            move |this, result, cx| {
                this.queue_busy = false;
                match result {
                    Ok(snapshot) => {
                        if this.is_selected(&target) {
                            this.queue = snapshot.items;
                        }
                    }
                    Err(message) => this.notice = Some(message.into()),
                }
                cx.notify();
            },
        );
    }

    /// Pull a queued chip into the composer. The runtime holds its place
    /// until the edit is sent (Enter updates it in place) or discarded.
    fn edit_queued(&mut self, queue_id: &str, cx: &mut Context<Self>) {
        let Some(session_id) = self.selected_session.clone() else {
            return;
        };
        if self.queue_busy || self.queue_edit.is_some() {
            return;
        }
        let Some(item) = self.queue.iter().find(|item| item.queue_id == queue_id) else {
            return;
        };
        let text = item.text.clone();
        self.queue_busy = true;
        let backend = self.backend.clone();
        let user_id = self.user_id.clone();
        let queue_id = queue_id.to_string();
        let target = session_id.clone();
        let held_id = queue_id.clone();
        self.call(
            async move {
                backend
                    .begin_queued_message_edit(&user_id, &session_id, &queue_id)
                    .await
            },
            cx,
            move |this, result, cx| {
                this.queue_busy = false;
                match result {
                    Ok(()) if this.is_selected(&target) => {
                        let draft = this
                            .composer
                            .as_ref()
                            .map(|composer| composer.read(cx).text())
                            .unwrap_or_default();
                        this.queue_edit = Some(QueueEdit {
                            queue_id: held_id.clone(),
                            draft,
                        });
                        if let Some(composer) = this.composer.clone() {
                            composer.update(cx, |input, cx| {
                                input.set_text(&text, cx);
                                input.set_placeholder(QUEUE_EDIT_PLACEHOLDER, cx);
                            });
                        }
                    }
                    Ok(()) => {
                        // Selection moved while the hold was requested.
                        this.release_queue_hold(&target, &held_id, cx);
                    }
                    Err(message) => this.notice = Some(message.into()),
                }
                cx.notify();
            },
        );
    }

    /// Drop the edit without changing the queued message; the stashed
    /// draft returns to the composer.
    fn discard_queue_edit(&mut self, cx: &mut Context<Self>) {
        let Some(session_id) = self.selected_session.clone() else {
            return;
        };
        if let Some(edit) = self.queue_edit.as_ref() {
            let queue_id = edit.queue_id.clone();
            self.release_queue_hold(&session_id, &queue_id, cx);
        }
        self.finish_queue_edit(cx);
    }

    /// The edit ended (sent or discarded): restore the composer.
    fn finish_queue_edit(&mut self, cx: &mut Context<Self>) {
        let Some(edit) = self.queue_edit.take() else {
            return;
        };
        if let Some(composer) = self.composer.clone() {
            composer.update(cx, |input, cx| {
                input.set_text(&edit.draft, cx);
                input.set_placeholder(COMPOSER_PLACEHOLDER, cx);
            });
        }
        cx.notify();
    }

    /// An edit cannot follow a session switch; release the hold and put the
    /// composer back.
    fn abandon_queue_edit(&mut self, cx: &mut Context<Self>) {
        if self.queue_edit.is_none() {
            return;
        }
        if let (Some(previous), Some(edit)) =
            (self.selected_session.clone(), self.queue_edit.as_ref())
        {
            let queue_id = edit.queue_id.clone();
            self.release_queue_hold(&previous, &queue_id, cx);
        }
        self.finish_queue_edit(cx);
    }

    fn release_queue_hold(&self, session_id: &str, queue_id: &str, cx: &mut Context<Self>) {
        let backend = self.backend.clone();
        let user_id = self.user_id.clone();
        let session_id = session_id.to_string();
        let queue_id = queue_id.to_string();
        self.call(
            async move {
                backend
                    .end_queued_message_edit(&user_id, &session_id, &queue_id)
                    .await
            },
            cx,
            |_this, result, _cx| {
                if let Err(message) = result {
                    log::debug!("end_queued_message_edit failed: {message}");
                }
            },
        );
    }

    /// Chips for messages waiting behind the active run.
    fn render_queue(&self, cx: &mut Context<Self>) -> Option<Div> {
        if self.queue.is_empty() {
            return None;
        }
        let busy = self.queue_busy;
        let action = |id: String, icon_name: &'static str| {
            div()
                .id(SharedString::from(id))
                .size_5()
                .flex()
                .items_center()
                .justify_center()
                .rounded_md()
                .when(busy, |button| button.opacity(0.5))
                .when(!busy, |button| {
                    button.hover(|style| {
                        style
                            .bg(gpui::rgb(theme::bg_sidebar_row_hover()))
                            .cursor_pointer()
                    })
                })
                .child(icon(icon_name, px(12.), theme::text_secondary()))
        };
        Some(
            div()
                .flex()
                .flex_col()
                .gap_1()
                .px_4()
                .pt_3()
                .child(
                    div()
                        .text_xs()
                        .text_color(gpui::rgb(theme::text_muted()))
                        .child(format!("{} queued for after this turn", self.queue.len())),
                )
                .children(self.queue.iter().map(|item| {
                    let steer_id = item.queue_id.clone();
                    let edit_id = item.queue_id.clone();
                    let remove_id = item.queue_id.clone();
                    let editing = self
                        .queue_edit
                        .as_ref()
                        .is_some_and(|edit| edit.queue_id == item.queue_id);
                    let preview: String = if editing {
                        "Editing in the composer…".to_string()
                    } else {
                        item.text.lines().next().unwrap_or("").to_string()
                    };
                    div()
                        .flex()
                        .items_center()
                        .gap_2()
                        .px_3()
                        .py_1()
                        .rounded_lg()
                        .bg(gpui::rgb(theme::bg_elevated()))
                        .border_1()
                        .border_color(gpui::rgb(theme::border_subtle()))
                        .text_sm()
                        .text_color(gpui::rgb(theme::text_secondary()))
                        .when(editing, |row| {
                            row.border_color(gpui::rgb(theme::accent()))
                                .text_color(gpui::rgb(theme::text_muted()))
                        })
                        .child(div().flex_1().min_w_0().line_clamp(1).child(preview))
                        .when(!item.attachments.is_empty(), |row| {
                            row.child(icon("paperclip", px(12.), theme::text_muted()))
                        })
                        .when(!editing, |row| {
                            row.child(
                                action(format!("queue-steer-{}", item.queue_id), "arrow-up")
                                    .on_click(cx.listener(move |this, _event, _window, cx| {
                                        this.steer_queued(&steer_id, cx);
                                    })),
                            )
                            .child(
                                action(format!("queue-edit-{}", item.queue_id), "pencil").on_click(
                                    cx.listener(move |this, _event, _window, cx| {
                                        this.edit_queued(&edit_id, cx);
                                    }),
                                ),
                            )
                            .child(
                                action(format!("queue-remove-{}", item.queue_id), "x").on_click(
                                    cx.listener(move |this, _event, _window, cx| {
                                        this.remove_queued(&remove_id, cx);
                                    }),
                                ),
                            )
                        })
                        .when(editing, |row| {
                            row.child(
                                action(format!("queue-discard-{}", item.queue_id), "x").on_click(
                                    cx.listener(|this, _event, _window, cx| {
                                        this.discard_queue_edit(cx);
                                    }),
                                ),
                            )
                        })
                })),
        )
    }

    fn push_local_error(&mut self, title: &str, message: &str, cx: &mut Context<Self>) {
        let item = AgentTimelineItem {
            id: format!(
                "local-error-{}",
                std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .map(|elapsed| elapsed.as_millis())
                    .unwrap_or(0)
            ),
            item_type: "error".to_string(),
            role: None,
            title: Some(title.to_string()),
            text: Some(message.to_string()),
            status: Some("failed".to_string()),
            input: None,
            output: None,
            created_ms: 0,
            merge: "replace".to_string(),
        };
        if let Some(session) = self.selected_session.clone() {
            self.apply_timeline_item(&session, item);
        }
        cx.notify();
    }

    fn stop(&mut self, cx: &mut Context<Self>) {
        let Some(session_id) = self.selected_session.clone() else {
            return;
        };
        let Some(run_id) = self.active_runs.get(&session_id).cloned() else {
            return;
        };
        let backend = self.backend.clone();
        let user_id = self.user_id.clone();
        self.call(
            async move { backend.cancel_run(&user_id, &run_id).await },
            cx,
            |this, result, cx| {
                if let Err(message) = result {
                    this.notice = Some(message.into());
                }
                cx.notify();
            },
        );
    }

    /// A question card without an input is unanswerable; make sure one
    /// exists whenever a question is showing.
    fn ensure_question_input(&mut self, cx: &mut Context<Self>) {
        if self.pending_question_input.is_some() {
            return;
        }
        let chat = cx.entity().downgrade();
        let input = cx.new(|cx| {
            TextInput::new("Type your answer…", cx).on_enter(move |text, _, cx| {
                let _ = text;
                let chat = chat.clone();
                // submit_question reads this input; defer out of its
                // update first.
                cx.defer(move |cx| {
                    if let Some(chat) = chat.upgrade() {
                        chat.update(cx, |chat, cx| chat.submit_question(cx));
                    }
                });
            })
        });
        self.pending_question_input = Some(input);
    }

    fn submit_question(&mut self, cx: &mut Context<Self>) {
        let Some(question) = self.current_question().cloned() else {
            return;
        };
        let step = self
            .question_step
            .min(question.questions.len().saturating_sub(1));
        // Record this step's answer: the picked option, else typed text.
        let answer = self.step_answer(&question, step, cx);
        self.question_step_answers.insert(step, answer);
        if step + 1 < question.questions.len() {
            // More questions in this batch: show the next one.
            self.question_step = step + 1;
            self.question_selected.clear();
            if let Some(input) = self.pending_question_input.clone() {
                input.update(cx, |input, cx| input.clear(cx));
            }
            self.question_focus_pending = true;
            cx.notify();
            return;
        }
        let composed = self.composed_question_answer(cx);
        self.answer_question(composed, cx);
    }

    /// The answer for one step: picked option label, else typed text,
    /// else an explicit placeholder so the model sees it was skipped.
    fn step_answer(
        &self,
        question: &crate::backend::PendingQuestion,
        step: usize,
        cx: &mut Context<Self>,
    ) -> Vec<String> {
        let Some(entry) = question.questions.get(step) else {
            return vec!["(no answer provided)".to_string()];
        };
        if let Some(option_index) = self.question_selected.get(&step)
            && let Some(option) = entry.options.get(*option_index)
        {
            return vec![option.label.clone()];
        }
        let typed = self
            .pending_question_input
            .as_ref()
            .map(|input| input.read(cx).text())
            .unwrap_or_default();
        if !typed.trim().is_empty() {
            return vec![typed.trim().to_string()];
        }
        vec!["(no answer provided)".to_string()]
    }

    /// Deliver `answer` (free text, a picked option, or joined options).
    fn answer_question(&mut self, answer: String, cx: &mut Context<Self>) {
        let Some(question) = self.current_question().cloned() else {
            return;
        };
        if answer.trim().is_empty() {
            return;
        }
        let request_id = question.request_id.clone();
        let callback_request_id = request_id.clone();
        let backend = self.backend.clone();
        let user_id = self.user_id.clone();
        // Drop the answered question and its input so the next card starts
        // fresh; a queued question's event already fired, so the input is
        // recreated right away when one is showing.
        self.pending_questions
            .retain(|queued| queued.request_id != question.request_id);
        self.reset_question_card(cx);
        cx.notify();
        self.call(
            async move { backend.answer_question(&user_id, &request_id, answer).await },
            cx,
            move |this, result, cx| match result {
                Ok(true) => {}
                Ok(false) => {
                    log::warn!(
                        "answer for question {} matched nothing pending",
                        callback_request_id
                    );
                    this.notice = Some("The task was no longer waiting for that answer".into());
                    cx.notify();
                }
                Err(message) => {
                    log::warn!(
                        "answer for question {} failed: {message}",
                        callback_request_id
                    );
                    this.notice = Some(format!("Could not deliver the answer: {message}").into());
                    cx.notify();
                }
            },
        );
    }
    /// Dismiss a question: cancel the run like the Stop button and
    /// unblock the tool with an empty answer if it is still waiting.
    fn skip_question(&mut self, cx: &mut Context<Self>) {
        let Some(question) = self.current_question().cloned() else {
            return;
        };
        self.pending_questions
            .retain(|queued| queued.request_id != question.request_id);
        self.reset_question_card(cx);
        cx.notify();
        let backend = self.backend.clone();
        let user_id = self.user_id.clone();
        {
            let request_id = question.request_id.clone();
            let answer_backend = backend.clone();
            let answer_user = user_id.clone();
            self.call(
                async move {
                    answer_backend
                        .answer_question(&answer_user, &request_id, String::new())
                        .await
                },
                cx,
                |_this, _result, _cx| {},
            );
        }
        if let Some(run_id) = self.active_runs.get(&question.session_id).cloned() {
            self.call(
                async move { backend.cancel_run(&user_id, &run_id).await },
                cx,
                |this, result, cx| {
                    if let Err(message) = result {
                        this.notice = Some(message.into());
                    }
                    cx.notify();
                },
            );
        }
    }

    /// Pick one option of one question (single-select, codex shape).
    fn select_question_option(
        &mut self,
        question_index: usize,
        option_index: usize,
        cx: &mut Context<Self>,
    ) {
        self.question_selected.insert(question_index, option_index);
        cx.notify();
    }

    /// Codex response JSON assembled from the recorded step answers.
    fn composed_question_answer(&mut self, cx: &mut Context<Self>) -> String {
        let Some(question) = self.current_question() else {
            return String::new();
        };
        let mut answers = serde_json::Map::new();
        for step in 0..question.questions.len() {
            let entry = &question.questions[step];
            let answer = self
                .question_step_answers
                .get(&step)
                .cloned()
                .unwrap_or_else(|| self.step_answer(question, step, cx));
            answers.insert(entry.id.clone(), serde_json::json!({ "answers": answer }));
        }
        let composed = serde_json::json!({ "answers": answers }).to_string();
        self.question_step_answers.clear();
        self.question_step = 0;
        composed
    }

    fn respond_permission(&mut self, allow: bool, cx: &mut Context<Self>) {
        let Some(permission) = self.pending_permission.clone() else {
            return;
        };
        if self.permission_responding {
            return;
        }
        self.permission_responding = true;
        cx.notify();
        let backend = self.backend.clone();
        let user_id = self.user_id.clone();
        self.call(
            async move {
                backend
                    .permission_respond(
                        &user_id,
                        &permission.session_id,
                        &permission.request_id,
                        allow,
                    )
                    .await
            },
            cx,
            |this, result, cx| {
                this.permission_responding = false;
                match result {
                    Ok(()) => {
                        this.pending_permission = None;
                    }
                    Err(message) => {
                        // Keep the card so the decision can be retried.
                        this.notice = Some(format!("Permission response failed: {message}").into());
                    }
                }
                cx.notify();
            },
        );
    }

    pub(crate) fn sign_out(&mut self, cx: &mut Context<Self>) {
        self.stop_speech(cx);
        if self.recording {
            self.recording = false;
            self.audio.cancel_recording();
        }
        let backend = self.backend.clone();
        let user_id = self.user_id.clone();
        self.call(
            async move {
                backend.stop_runtime(&user_id).await?;
                backend.logout_and_clear(&user_id).await
            },
            cx,
            |_this, _result, cx| {
                cx.emit(LoggedOut);
            },
        );
    }

    fn pick_model(&mut self, model: String, cx: &mut Context<Self>) {
        self.selected_model = Some(model.clone());
        self.models_menu_open = false;
        self.root_menu_open = false;
        cx.notify();
        self.refresh_vision(cx);
        // Remember the choice across launches via the agent config.
        let backend = self.backend.clone();
        let user_id = self.user_id.clone();
        self.call(
            async move { backend.save_default_model(&user_id, model).await },
            cx,
            |_this, _result, _cx| {},
        );
        self.refresh_context_usage(cx);
    }

    /// Ask the title model for a one-line summary of the completed tool
    /// call at `index` when its output is too long to skim. At most a few
    /// requests ride at once; the rest wait in `summary_queue`. Each item
    /// is only ever asked once per session visit.
    fn maybe_summarize_tool(&mut self, index: usize, cx: &mut Context<Self>) {
        let Some(item) = self.timeline.get(index) else {
            return;
        };
        if !self.summaries_enabled
            || !matches!(item.item_type.as_str(), "tool" | "toolCall")
            || item.status.as_deref() != Some("completed")
            || has_tool_input(item, "todos")
            || item.input.as_ref().is_none_or(serde_json::Value::is_null)
            || self.tool_summaries.contains_key(&item.id)
            || self.summary_requests.contains(&item.id)
        {
            return;
        }
        let Some(output) = tool_output_markdown(item) else {
            return;
        };
        if output.chars().count() < 400 {
            return;
        }
        let item_id = item.id.clone();
        self.summary_requests.insert(item_id.clone());
        if self.pending_summaries >= 3 {
            self.summary_queue.push_back(item_id);
            return;
        }
        self.start_summary(item_id, output, cx);
    }

    /// Start queued summaries while a slot is free.
    fn drain_summary_queue(&mut self, cx: &mut Context<Self>) {
        while self.pending_summaries < 3 {
            let Some(item_id) = self.summary_queue.pop_front() else {
                return;
            };
            let Some(&(index, _)) = self.timeline_index.get(&item_id) else {
                continue;
            };
            let Some(output) = self.timeline.get(index).and_then(tool_output_markdown) else {
                continue;
            };
            self.start_summary(item_id, output, cx);
        }
    }

    fn start_summary(&mut self, item_id: String, output: String, cx: &mut Context<Self>) {
        let Some(session_id) = self.selected_session.clone() else {
            return;
        };
        let Some(&(index, _)) = self.timeline_index.get(&item_id) else {
            return;
        };
        let item = &self.timeline[index];
        let Some(input) = item.input.clone() else {
            return;
        };
        let tool_name = item.title.clone().unwrap_or_else(|| item.item_type.clone());
        let backend = self.backend.clone();
        let user_id = self.user_id.clone();
        let generation = self.summary_generation;
        self.pending_summaries += 1;
        let store_id = item_id.clone();
        self.call(
            async move {
                let summary = backend
                    .summarize_tool_call(&user_id, &session_id, tool_name, Some(input), output)
                    .await?;
                if let Some(summary) = &summary {
                    let summary = summary.clone();
                    let store = backend.clone();
                    tokio::task::spawn_blocking(move || {
                        if let Err(error) = store.store_tool_summary_blocking(
                            &user_id,
                            &session_id,
                            &store_id,
                            &summary,
                        ) {
                            log::warn!("Cannot store tool summary: {error}");
                        }
                    });
                }
                Ok::<_, String>(summary)
            },
            cx,
            move |this, result, cx| {
                // The screen moved to another session meanwhile; the slot
                // count restarted and this label has nowhere to go.
                if this.summary_generation != generation {
                    return;
                }
                this.pending_summaries = this.pending_summaries.saturating_sub(1);
                if let Ok(Some(summary)) = result {
                    if let Some(&(index, _)) = this.timeline_index.get(&item_id) {
                        this.list_state.splice(index..index + 1, 1);
                    }
                    this.tool_summaries
                        .insert(item_id, SharedString::from(summary));
                    cx.notify();
                }
                this.drain_summary_queue(cx);
            },
        );
    }

    /// Request summaries for the long completed tool calls of the loaded
    /// timeline (session switch or reload).
    fn summarize_loaded_tools(&mut self, cx: &mut Context<Self>) {
        if self.selected_session.is_none() {
            return;
        }
        let first = self.timeline.len().saturating_sub(40);
        for index in (first..self.timeline.len()).rev() {
            self.maybe_summarize_tool(index, cx);
        }
    }

    /// Fetch the images behind sent attachments that are not decoded yet.
    /// Each id is requested once; the result re-measures the rows that
    /// show it. Scans the whole timeline: for a loaded snapshot only.
    fn load_attachment_images(&mut self, cx: &mut Context<Self>) {
        let wanted: Vec<String> = self
            .timeline
            .iter()
            .flat_map(attachment_refs)
            .map(|(id, _)| id.to_string())
            .filter(|id| !self.attachment_requests.contains(id))
            .collect();
        self.request_attachments(wanted, cx);
    }

    /// Same for one arriving item, so a streaming run does not rescan the
    /// timeline on every event.
    fn load_attachment_images_for(&mut self, item: &AgentTimelineItem, cx: &mut Context<Self>) {
        let wanted: Vec<String> = attachment_refs(item)
            .map(|(id, _)| id.to_string())
            .filter(|id| !self.attachment_requests.contains(id))
            .collect();
        self.request_attachments(wanted, cx);
    }

    fn request_attachments(&mut self, wanted: Vec<String>, cx: &mut Context<Self>) {
        let Some(session_id) = self.selected_session.clone() else {
            return;
        };
        for id in wanted {
            self.attachment_requests.insert(id.clone());
            let backend = self.backend.clone();
            let user_id = self.user_id.clone();
            let session = session_id.clone();
            let attachment_id = id.clone();
            self.call(
                async move {
                    backend
                        .read_image_attachment(&user_id, &session, &attachment_id)
                        .await
                },
                cx,
                move |this, result, cx| match result {
                    Ok(bytes) => this.insert_attachment_image(&id, bytes, cx),
                    Err(message) => log::debug!("attachment {id} not loaded: {message}"),
                },
            );
        }
    }

    fn insert_attachment_image(&mut self, id: &str, bytes: Vec<u8>, cx: &mut Context<Self>) {
        let Some(format) = image_format_from_bytes(&bytes) else {
            return;
        };
        self.attachment_images.insert(
            id.to_string(),
            Arc::new(gpui::Image::from_bytes(format, bytes)),
        );
        // Rows that show this image change height; tell the list.
        for (index, item) in self.timeline.iter().enumerate() {
            if attachment_refs(item).any(|(candidate, _)| candidate == id) {
                self.list_state.splice(index..index + 1, 1);
            }
        }
        cx.notify();
    }

    /// Apply a timeline item using Maple's merge contract: `append` extends
    /// message/thinking text on the item with the same id; otherwise merge
    /// fields, keeping the previous value when the incoming field is absent.
    ///
    /// Returns the item's index in the timeline.
    fn apply_timeline_item(&mut self, session_id: &str, item: AgentTimelineItem) -> usize {
        // Streaming delivers one of these per chunk; move the payload
        // rather than copy it.
        let position = match self.timeline_index.get_mut(&item.id) {
            Some((index, revision)) => {
                *revision += 1;
                Some(*index)
            }
            None => None,
        };
        let index = match position {
            Some(index) => {
                let AgentTimelineItem {
                    created_ms: incoming_created,
                    item_type: incoming_type,
                    role: incoming_role,
                    title: incoming_title,
                    text: incoming_text,
                    status: incoming_status,
                    input: incoming_input,
                    output: incoming_output,
                    merge: incoming_merge,
                    ..
                } = item;
                let is_newest = index + 1 == self.timeline.len();
                let existing = &mut self.timeline[index];
                // The virtualized list caches item heights; tell it this
                // one changed so it re-measures. The newest item is the
                // exception: a splice drops its cached height to zero until
                // the next paint, which collapses the list's scroll range.
                // A wheel event in that window clamps back to the bottom
                // and re-pins the view, so streaming would make the
                // transcript impossible to scroll up. The newest item is
                // measured on every layout while it is visible, and its
                // stale height is a better estimate than zero once the user
                // scrolls away mid-stream.
                if !is_newest {
                    self.list_state.splice(index..index + 1, 1);
                }
                let append = incoming_merge == "append"
                    && matches!(incoming_type.as_str(), "message" | "thinking")
                    && incoming_text.is_some();
                if append {
                    let addition = incoming_text.unwrap_or_default();
                    existing
                        .text
                        .get_or_insert_with(String::new)
                        .push_str(&addition);
                } else {
                    existing.created_ms = incoming_created;
                    existing.item_type = incoming_type;
                    existing.role = incoming_role.or_else(|| existing.role.take());
                    existing.title = incoming_title.or_else(|| existing.title.take());
                    existing.text = incoming_text.or_else(|| existing.text.take());
                    existing.status = incoming_status.or_else(|| existing.status.take());
                    existing.input = incoming_input.or_else(|| existing.input.take());
                    existing.output = incoming_output.or_else(|| existing.output.take());
                    existing.merge = incoming_merge;
                }
                index
            }
            None => {
                let index = self.timeline.len();
                self.timeline_index.insert(item.id.clone(), (index, 0));
                self.timeline.push(item);
                self.list_state.splice(index..index, 1);
                index
            }
        };
        *self
            .timeline_revisions
            .entry(session_id.to_string())
            .or_insert(0) += 1;
        index
    }

    /// Insert or replace a session row; returns whether anything changed.
    fn upsert_session(&mut self, session: AgentSessionSummary) -> bool {
        if let Some(existing) = self
            .sessions
            .iter_mut()
            .find(|candidate| candidate.id == session.id)
        {
            if session_summary_eq(existing, &session) {
                return false;
            }
            *existing = session;
        } else {
            self.sessions.insert(0, session);
        }
        self.rebuild_project_groups();
        true
    }

    /// One timeline item arrived for the selected session.
    fn apply_incoming_item(
        &mut self,
        session_id: &str,
        item: AgentTimelineItem,
        cx: &mut Context<Self>,
    ) {
        if item.role.as_deref() != Some("user") {
            self.awaiting_first_token = false;
        }
        // Cheap gates first so the common streaming chunk does no extra
        // work; the summary check reads the stored item after the merge.
        let completed = item.status.as_deref() == Some("completed");
        let is_tool = matches!(item.item_type.as_str(), "tool" | "toolCall");
        self.retire_decided_permission(&item);
        self.load_attachment_images_for(&item, cx);
        let index = self.apply_timeline_item(session_id, item);
        if completed {
            if is_tool && self.summaries_enabled {
                self.maybe_summarize_tool(index, cx);
            }
            // A finished item marks a turn boundary: the usage ledger has
            // a new row.
            self.refresh_context_usage(cx);
        }
    }
    /// Drop the permission card when the runtime decided its request
    /// without the card: switching the session to "Allow all" approves
    /// every pending request and replaces the permission row with a
    /// status.
    fn retire_decided_permission(&mut self, item: &AgentTimelineItem) {
        let Some(permission) = &self.pending_permission else {
            return;
        };
        if item.status.is_none() {
            return;
        }
        if item
            .id
            .strip_prefix("permission-")
            .is_some_and(|request_id| request_id == permission.request_id)
        {
            self.pending_permission = None;
            self.permission_responding = false;
        }
    }

    pub fn handle_service_events(
        &mut self,
        events: Vec<AgentServiceEvent>,
        cx: &mut Context<Self>,
    ) {
        let mut changed = false;
        for event in events {
            changed |= self.apply_service_event(event, cx);
        }
        if changed {
            cx.notify();
        }
    }

    /// Route one backend service event into UI state.
    #[cfg(test)]
    pub fn handle_service_event(&mut self, event: AgentServiceEvent, cx: &mut Context<Self>) {
        if self.apply_service_event(event, cx) {
            cx.notify();
        }
    }

    /// Apply one event; returns false when nothing visible changed.
    fn apply_service_event(&mut self, event: AgentServiceEvent, cx: &mut Context<Self>) -> bool {
        match event {
            AgentServiceEvent::RuntimeStatus(status) => {
                // The status snapshot is authoritative for active runs; an
                // idle heartbeat that repeats it changes nothing.
                if self.active_runs == status.active_runs {
                    return false;
                }
                self.active_runs = status.active_runs;
            }
            AgentServiceEvent::SessionCreated(session) => {
                return self.upsert_session(session);
            }
            AgentServiceEvent::SessionUpdated { session, .. } => {
                return self.upsert_session(session);
            }
            AgentServiceEvent::TimelineItem {
                session_id, item, ..
            } => {
                if self.selected_session.as_deref() == Some(session_id.as_str()) {
                    self.apply_incoming_item(&session_id, item, cx);
                } else {
                    return false;
                }
            }
            AgentServiceEvent::Question {
                session_id,
                request_id,
                questions,
            } => {
                if self
                    .pending_questions
                    .iter()
                    .any(|queued| queued.request_id == request_id)
                {
                    return false;
                }
                let preview: String = questions
                    .first()
                    .map(|question| question.question.chars().take(140).collect())
                    .unwrap_or_default();
                self.notify_desktop("Maple has a question", &preview);
                // Questions queue per session; one for a task that is not
                // on screen shows its card when the user switches there.
                let shows_now = self.current_question().is_none()
                    && self.selected_session.as_deref() == Some(session_id.as_str());
                self.pending_questions.push(PendingQuestion {
                    session_id,
                    request_id,
                    questions,
                });
                if shows_now {
                    // A fresh question takes the card; follow-ups queue
                    // behind it and keep the user's picks on this one.
                    self.question_selected.clear();
                    self.ensure_question_input(cx);
                    self.question_focus_pending = true;
                }
            }
            AgentServiceEvent::Run {
                session_id,
                run_id,
                event,
            } => return self.handle_run_event(&session_id, &run_id, event, cx),
        }
        true
    }

    fn is_selected(&self, session_id: &str) -> bool {
        self.selected_session.as_deref() == Some(session_id)
    }

    /// Drop every queued question of a session whose run ended.
    fn clear_session_questions(&mut self, session_id: &str, cx: &mut Context<Self>) {
        let before = self.pending_questions.len();
        self.pending_questions
            .retain(|question| question.session_id != session_id);
        if self.pending_questions.len() != before && self.is_selected(session_id) {
            self.reset_question_card(cx);
        }
    }

    fn handle_run_event(
        &mut self,
        session_id: &str,
        run_id: &str,
        event: maple_agent::agent::AgentRunEvent,
        cx: &mut Context<Self>,
    ) -> bool {
        use maple_agent::agent::AgentRunEvent;
        match event {
            AgentRunEvent::SessionUpdated(session) => {
                return self.upsert_session(session);
            }
            AgentRunEvent::Started => {
                self.active_runs
                    .insert(session_id.to_string(), run_id.to_string());
                self.start_usage_poller(session_id.to_string(), cx);
                self.refresh_context_usage(cx);
            }
            AgentRunEvent::TimelineItem(item) => {
                if self.is_selected(session_id) {
                    self.apply_incoming_item(session_id, item, cx);
                }
            }
            AgentRunEvent::PermissionRequested { request, item } => {
                if self.is_selected(session_id) {
                    // The permission row stays in the transcript so the
                    // decision is visible after the card is answered.
                    self.load_attachment_images_for(&item, cx);
                    self.apply_timeline_item(session_id, item);
                    let arguments = serde_json::Value::Object(request.arguments);
                    self.pending_permission_arguments = if arguments.is_null() {
                        SharedString::default()
                    } else {
                        serde_json::to_string_pretty(&arguments)
                            .unwrap_or_default()
                            .into()
                    };
                    let prompt = request
                        .prompt
                        .clone()
                        .unwrap_or_else(|| format!("Run tool {}?", request.tool_name));
                    self.notify_desktop("Maple needs permission", &prompt);
                    self.pending_permission = Some(PendingPermission {
                        session_id: session_id.to_string(),
                        run_id: run_id.to_string(),
                        request_id: request.request_id,
                        tool_name: request.tool_name,
                        prompt: request.prompt,
                    });
                    self.permission_responding = false;
                }
            }
            AgentRunEvent::SetupWarning(message) => {
                if self.is_selected(session_id) {
                    self.notice = Some(message.into());
                }
            }
            AgentRunEvent::HistoryReplaced => {
                if self.is_selected(session_id) {
                    // Replace history only; the run and any pending
                    // permission keep flowing.
                    let session_id = session_id.to_string();
                    self.reload_timeline(&session_id, cx);
                }
            }
            AgentRunEvent::Error(item) => {
                if self.is_selected(session_id) {
                    self.load_attachment_images_for(&item, cx);
                    self.apply_timeline_item(session_id, item);
                }
            }
            AgentRunEvent::Finished(_) => {
                // Only retire the run that actually finished; a late
                // Finished from a cancelled run must not clear a newer one.
                let owns_session = self
                    .active_runs
                    .get(session_id)
                    .is_none_or(|active| active == run_id);
                if owns_session {
                    self.active_runs.remove(session_id);
                    // The run that asked is gone (stopped or failed): its
                    // questions would block the composer forever.
                    self.clear_session_questions(session_id, cx);
                }
                if let Some(permission) = &self.pending_permission
                    && permission.run_id == run_id
                {
                    self.pending_permission = None;
                    self.permission_responding = false;
                }
                self.awaiting_first_token = false;
                if self.is_selected(session_id) {
                    self.refresh_context_usage(cx);
                }
                let title = self
                    .sessions
                    .iter()
                    .find(|session| session.id == session_id)
                    .map(|session| session.title.clone())
                    .unwrap_or_else(|| "Task".to_string());
                self.notify_desktop("Maple", &format!("“{title}” finished"));
                self.refresh_sidebar_plan(cx);
            }
            AgentRunEvent::QueueChanged(snapshot) => {
                if !self.is_selected(session_id) {
                    return false;
                }
                self.queue = snapshot.items;
            }
            AgentRunEvent::QueuePromoted { snapshot, item, .. } => {
                if !self.is_selected(session_id) {
                    return false;
                }
                self.queue = snapshot.items;
                self.apply_timeline_item(session_id, item);
            }
        }
        true
    }
}

impl EventEmitter<LoggedOut> for ChatScreen {}

impl EventEmitter<OpenSettings> for ChatScreen {}
impl EventEmitter<OpenSettingsSection> for ChatScreen {}

impl Render for ChatScreen {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        // Refreshed every frame; activation changes force a redraw, so
        // this tracks focus closely enough to gate notifications.
        self.window_active = window.is_window_active();
        if self.question_focus_pending {
            self.question_focus_pending = false;
            if self.current_question().is_some()
                && let Some(input) = self.pending_question_input.clone()
            {
                let handle = input.read(cx).focus_handle(cx);
                window.focus(&handle);
            }
        }
        if self.rename_focus_pending {
            self.rename_focus_pending = false;
            if let Some(input) = self.rename_input.clone() {
                let handle = input.read(cx).focus_handle(cx);
                window.focus(&handle);
            }
        }
        let confirm_remove = self
            .confirm_remove_root
            .clone()
            .map(|root| self.render_confirm_remove(&root, cx));
        let trust_prompt = self
            .trust_prompt
            .clone()
            .map(|status| self.render_trust_prompt(&status, cx));
        let empty = self.timeline.is_empty();
        let collapsed = self.sidebar_collapsed;
        let main = if empty {
            self.render_empty_state(cx)
        } else {
            div()
                .flex()
                .flex_col()
                .flex_1()
                .h_full()
                .min_w_0()
                .child(self.render_header())
                .child(self.render_transcript(cx))
                .when(self.awaiting_first_token && self.is_run_active(), |main| {
                    // Same gutter as transcript text so the dots line up
                    main.child(
                        div()
                            .w_full()
                            .max_w(px(900.))
                            .mx_auto()
                            .px_6()
                            .child(render_waiting_indicator()),
                    )
                })
                .when_some(self.current_question(), |container, question| {
                    let input = self.pending_question_input.clone();
                    let step = self.question_step;
                    container.child(render_question_card(
                        question,
                        step,
                        input,
                        &self.question_selected,
                        cx,
                    ))
                })
                .when_some(self.pending_permission.as_ref(), |container, permission| {
                    container.child(render_permission_card(
                        permission,
                        &self.pending_permission_arguments,
                        self.permission_responding,
                        cx,
                    ))
                })
                .child(
                    div()
                        .w_full()
                        .max_w(px(900.))
                        .mx_auto()
                        .px_4()
                        .pb_4()
                        .when(self.composer_expanded, |wrap| {
                            wrap.flex_none()
                                .h(gpui::relative(0.7))
                                .min_h_0()
                                .flex()
                                .flex_col()
                        })
                        .child(self.render_composer(cx))
                        .children(self.render_menu_panel(cx))
                        .children(self.render_slash_palette(cx)),
                )
        };
        div()
            .key_context("Chat")
            .on_action(cx.listener(Self::chat_escape))
            .on_action(cx.listener(Self::copy_selection))
            .on_action(cx.listener(Self::select_all_transcript))
            .on_key_down(cx.listener(Self::type_into_composer))
            .flex_1()
            .min_h_0()
            .flex()
            .flex_col()
            .bg(gpui::rgb(theme::bg_app()))
            .child(
                div()
                    .flex_1()
                    .min_h_0()
                    .flex()
                    .flex_row()
                    .when(!collapsed, |row| row.child(self.render_sidebar(cx)))
                    .child(
                        div()
                            .relative()
                            .flex()
                            .flex_col()
                            .flex_1()
                            .h_full()
                            .min_w_0()
                            .when(collapsed, |pane| {
                                pane.child(
                                    div()
                                        .absolute()
                                        .top_2()
                                        .left_3()
                                        .flex()
                                        .items_center()
                                        .gap_2()
                                        .child(self.render_sidebar_toggle(cx))
                                        .child(wordmark(px(14.), theme::text_primary())),
                                )
                            })
                            .child(main),
                    ),
            )
            .when_some(self.lightbox.clone(), |root, image| {
                root.child(
                    div()
                        .id("lightbox")
                        .absolute()
                        .size_full()
                        .top_0()
                        .left_0()
                        .bg(gpui::rgba(0x000000d9))
                        .flex()
                        .items_center()
                        .justify_center()
                        .hover(|style| style.cursor_pointer())
                        .on_click(cx.listener(|this, _event, _window, cx| {
                            this.lightbox = None;
                            cx.notify();
                        }))
                        .child(
                            gpui::img(gpui::ImageSource::Image(image))
                                .max_w(gpui::relative(0.9))
                                .max_h(gpui::relative(0.9))
                                .rounded_lg()
                                .border_1()
                                .border_color(gpui::rgb(theme::border())),
                        ),
                )
            })
            .children(confirm_remove)
            .children(trust_prompt)
    }
}

impl ChatScreen {
    /// Panel icon that hides or shows the sidebar.
    fn render_sidebar_toggle(&self, cx: &mut Context<Self>) -> gpui::Stateful<Div> {
        div()
            .id("sidebar-toggle")
            .flex_none()
            .size_7()
            .flex()
            .items_center()
            .justify_center()
            .rounded_md()
            .hover(|style| {
                style
                    .bg(gpui::rgb(theme::bg_sidebar_row_hover()))
                    .cursor_pointer()
            })
            .on_click(cx.listener(|this, _event, _window, cx| {
                this.sidebar_collapsed = !this.sidebar_collapsed;
                cx.notify();
            }))
            .child(icon("panel-left", px(16.), theme::text_secondary()))
    }

    /// Hero layout for a task with no messages: display heading, composer,
    /// and privacy note centered in the pane (mirrors EmptyAgentState).
    fn render_empty_state(&mut self, cx: &mut Context<Self>) -> Div {
        let expanded = self.composer_expanded;
        div()
            .flex()
            .flex_col()
            .flex_1()
            .h_full()
            .min_w_0()
            .items_center()
            .justify_center()
            .px_6()
            .when(expanded, |pane| pane.py_6())
            .child(
                div()
                    .w_full()
                    .max_w(px(if expanded { 900. } else { 650. }))
                    .flex()
                    .flex_col()
                    .items_center()
                    .gap_6()
                    .when(expanded, |column| column.h_full().min_h_0())
                    .when(!expanded, |column| {
                        column.child(
                            div()
                                .mb_6()
                                .font_family(crate::assets::FONT_DISPLAY)
                                .text_size(px(36.))
                                .line_height(px(48.))
                                .text_color(gpui::rgb(theme::display_text()))
                                .child("Work on anything..."),
                        )
                    })
                    .child(
                        div()
                            .w_full()
                            .when(expanded, |wrap| wrap.flex_1().min_h_0().flex().flex_col())
                            .child(self.render_composer(cx))
                            .children(self.render_menu_panel(cx))
                            .children(self.render_slash_palette(cx)),
                    )
                    .when(
                        self.awaiting_first_token && self.is_run_active(),
                        |column| column.child(render_waiting_indicator()),
                    )
                    .when(!expanded, |column| {
                        column.child(
                            div()
                                .flex()
                                .items_center()
                                .gap_1()
                                .text_xs()
                                .text_color(gpui::rgb(theme::text_muted()))
                                .child(icon("lock", px(12.), theme::text_muted()))
                                .child("Encrypted and private at every step"),
                        )
                    })
                    .when_some(self.runtime_error.clone(), |column, error| {
                        column.child(
                            div()
                                .px_3()
                                .py_2()
                                .rounded_md()
                                .bg(gpui::rgb(theme::status_error()))
                                .text_color(gpui::rgb(theme::bg_app()))
                                .text_sm()
                                .child(error),
                        )
                    })
                    .children(self.render_update_banner(cx))
                    .when_some(self.notice.clone(), |column, notice| {
                        column.child(
                            div()
                                .px_3()
                                .py_2()
                                .rounded_md()
                                .bg(gpui::rgb(theme::status_warning()))
                                .text_color(gpui::rgb(theme::bg_app()))
                                .text_sm()
                                .child(notice),
                        )
                    }),
            )
    }

    /// Group sessions by project root: pinned roots first (in pin order),
    /// then the current root, then the other recent roots, then any root
    /// that only appears on a stored task. Called when sessions, roots, or
    /// pins change, not per render.
    fn rebuild_project_groups(&mut self) {
        let known = |root: &str, this: &Self| {
            this.recent_roots.iter().any(|candidate| candidate == root)
                || this.project_root.as_deref() == Some(root)
                || this
                    .sessions
                    .iter()
                    .any(|session| session.project_root == root)
        };
        let mut roots: Vec<String> = Vec::new();
        for root in self
            .pinned_roots
            .iter()
            .filter(|root| known(root, self))
            .cloned()
        {
            roots.push(root);
        }
        if let Some(root) = self.project_root.clone()
            && !roots.contains(&root)
        {
            roots.push(root);
        }
        for root in self.recent_roots.iter().chain(
            self.sessions
                .iter()
                .filter(|s| !s.archived)
                .map(|s| &s.project_root),
        ) {
            if !roots.contains(root) {
                roots.push(root.clone());
            }
        }
        let filtering = !self.sidebar_filter.is_empty();
        self.project_groups = roots
            .into_iter()
            .map(|root| {
                let indices: Vec<usize> = self
                    .sessions
                    .iter()
                    .enumerate()
                    .filter(|(_, session)| {
                        !session.archived
                            && session.project_root == root
                            && self.matches_filter(session)
                    })
                    .map(|(index, _)| index)
                    .collect();
                (root, indices)
            })
            // While searching, a project with no matching task is noise.
            .filter(|(_, indices)| !filtering || !indices.is_empty())
            .collect();
        self.archived_indices = self
            .sessions
            .iter()
            .enumerate()
            .filter(|(_, session)| session.archived && self.matches_filter(session))
            .map(|(index, _)| index)
            .collect();
    }

    fn search_changed(&mut self, input: &Entity<TextInput>, cx: &mut Context<Self>) {
        let filter = input.read(cx).text_ref().trim().to_lowercase();
        if filter != self.sidebar_filter {
            self.sidebar_filter = filter;
            self.rebuild_project_groups();
            cx.notify();
        }
    }

    fn clear_search(&mut self, cx: &mut Context<Self>) {
        if let Some(input) = self.search_input.clone() {
            input.update(cx, |input, cx| input.clear(cx));
        }
        self.sidebar_filter.clear();
        self.rebuild_project_groups();
        cx.notify();
    }

    /// Whether a task row passes the sidebar filter.
    fn matches_filter(&self, session: &AgentSessionSummary) -> bool {
        self.sidebar_filter.is_empty()
            || session.title.to_lowercase().contains(&self.sidebar_filter)
            || self
                .root_name(&session.project_root)
                .to_lowercase()
                .contains(&self.sidebar_filter)
    }

    /// Pin or unpin a project root. Pinned roots sort to the top of the
    /// sidebar and persist in the app settings.
    fn toggle_pin(&mut self, root: &str, cx: &mut Context<Self>) {
        if self.pinned_roots.iter().any(|pinned| pinned == root) {
            self.pinned_roots.retain(|pinned| pinned != root);
        } else {
            self.pinned_roots.push(root.to_string());
        }
        self.rebuild_project_groups();
        cx.notify();
        let pinned = self.pinned_roots.clone();
        self.persist_settings(move |settings| settings.pinned_roots = pinned, cx);
    }

    /// Apply `update` to the settings file off the UI thread.
    fn persist_settings(
        &self,
        update: impl FnOnce(&mut crate::settings::AppSettings) + Send + 'static,
        cx: &mut Context<Self>,
    ) {
        self.call(
            async move {
                tokio::task::spawn_blocking(move || {
                    let mut settings = crate::settings::load_settings();
                    update(&mut settings);
                    crate::settings::save_settings_in_background(settings);
                    Ok(())
                })
                .await
                .map_err(|error| format!("Settings save failed: {error}"))?
            },
            cx,
            |_this, result: Result<(), String>, _cx| {
                if let Err(message) = result {
                    log::warn!("{message}");
                }
            },
        );
    }

    /// Display name for a project root: the saved name, else the folder name.
    fn root_name(&self, root: &str) -> String {
        self.project_names
            .get(root)
            .filter(|name| !name.trim().is_empty())
            .cloned()
            .unwrap_or_else(|| root_display_name(root))
    }

    /// Start an inline rename of a task or project in the sidebar.
    fn begin_rename(&mut self, target: RenameTarget, cx: &mut Context<Self>) {
        let current = match &target {
            RenameTarget::Task(id) => self
                .sessions
                .iter()
                .find(|session| &session.id == id)
                .map(|session| session.title.clone())
                .unwrap_or_default(),
            RenameTarget::Project(root) => self.root_name(root),
        };
        let chat = cx.entity().downgrade();
        let input = cx.new(|cx| {
            let mut input = TextInput::new("Name", cx).with_tab_index(0);
            input.set_text(&current, cx);
            input.on_enter(move |_text, _, cx| {
                let chat = chat.clone();
                // commit_rename reads this input; defer out of its update.
                cx.defer(move |cx| {
                    if let Some(chat) = chat.upgrade() {
                        chat.update(cx, |chat, cx| chat.commit_rename(cx));
                    }
                });
            })
        });
        self.project_menu = None;
        self.rename = Some(target);
        self.rename_input = Some(input);
        self.rename_focus_pending = true;
        cx.notify();
    }

    fn cancel_rename(&mut self, cx: &mut Context<Self>) {
        self.rename = None;
        self.rename_input = None;
        cx.notify();
    }

    fn commit_rename(&mut self, cx: &mut Context<Self>) {
        let Some(target) = self.rename.take() else {
            return;
        };
        let name = self
            .rename_input
            .take()
            .map(|input| input.read(cx).text())
            .unwrap_or_default();
        let name = name.trim().to_string();
        cx.notify();
        if name.is_empty() {
            return;
        }
        match target {
            RenameTarget::Task(session_id) => {
                let backend = self.backend.clone();
                let user_id = self.user_id.clone();
                self.call(
                    async move { backend.rename_session(&user_id, &session_id, name).await },
                    cx,
                    |this, result, cx| {
                        match result {
                            Ok(session) => {
                                this.upsert_session(session);
                                this.rebuild_project_groups();
                            }
                            Err(message) => this.notice = Some(message.into()),
                        }
                        cx.notify();
                    },
                );
            }
            RenameTarget::Project(root) => {
                if name == root_display_name(&root) {
                    self.project_names.remove(&root);
                } else {
                    self.project_names.insert(root.clone(), name);
                }
                let names = self.project_names.clone();
                self.persist_settings(move |settings| settings.project_names = names, cx);
            }
        }
    }

    /// Show the project's folder in the file manager.
    fn open_folder(&mut self, root: &str, cx: &mut Context<Self>) {
        self.project_menu = None;
        let path = root.to_string();
        cx.notify();
        self.call(
            async move {
                tokio::task::spawn_blocking(move || crate::platform::reveal_folder(&path))
                    .await
                    .map_err(|error| format!("Could not open the folder: {error}"))?
            },
            cx,
            |this, result, cx| {
                if let Err(message) = result {
                    this.notice = Some(message.into());
                    cx.notify();
                }
            },
        );
    }

    fn toggle_project_menu(&mut self, root: &str, cx: &mut Context<Self>) {
        self.menu_trust = None;
        if self.project_menu.as_deref() == Some(root) {
            self.project_menu = None;
        } else {
            self.project_menu = Some(root.to_string());
            let backend = self.backend.clone();
            let user_id = self.user_id.clone();
            let path = root.to_string();
            self.call(
                async move { backend.project_trust(&user_id, path).await },
                cx,
                |this, result, cx| {
                    if let Ok(status) = result
                        && this.project_menu.as_deref() == Some(status.path.as_str())
                    {
                        this.menu_trust = Some(status);
                        cx.notify();
                    }
                },
            );
        }
        cx.notify();
    }

    /// Ask for a trust decision when the current project provides skills
    /// or guidance and none is saved yet.
    fn check_project_trust(&mut self, cx: &mut Context<Self>) {
        self.trust_prompt = None;
        let Some(root) = self.project_root.clone() else {
            return;
        };
        let backend = self.backend.clone();
        let user_id = self.user_id.clone();
        self.call(
            async move { backend.project_trust(&user_id, root).await },
            cx,
            |this, result, cx| {
                if let Ok(status) = result
                    && this.project_root.as_deref() == Some(status.path.as_str())
                    && status.available
                    && !status.protected_features.is_empty()
                    && status.decision.is_none()
                {
                    this.trust_prompt = Some(status);
                    cx.notify();
                }
            },
        );
    }

    fn set_project_trust(&mut self, path: String, trusted: bool, cx: &mut Context<Self>) {
        if self.trust_saving {
            return;
        }
        self.trust_saving = true;
        self.project_menu = None;
        cx.notify();
        let backend = self.backend.clone();
        let user_id = self.user_id.clone();
        self.call(
            async move { backend.set_project_trust(&user_id, path, trusted).await },
            cx,
            move |this, result, cx| {
                this.trust_saving = false;
                match result {
                    Ok(status) => {
                        if this.trust_prompt.as_ref().map(|p| &p.path) == Some(&status.path) {
                            this.trust_prompt = None;
                        }
                        this.notice = Some(
                            if trusted {
                                "Project trusted: its skills and guidance are available to new tasks"
                            } else {
                                "Project kept untrusted"
                            }
                            .into(),
                        );
                    }
                    Err(message) => this.notice = Some(message.into()),
                }
                cx.notify();
            },
        );
    }

    /// Modal that asks whether project-provided guidance may be used.
    fn render_trust_prompt(
        &self,
        status: &AgentProjectTrustStatus,
        cx: &mut Context<Self>,
    ) -> gpui::Stateful<Div> {
        let name = self.root_name(&status.path);
        let saving = self.trust_saving;
        let button = |id: &'static str, label: &'static str, primary: bool| {
            div()
                .id(id)
                .px_3()
                .py_1p5()
                .rounded_md()
                .text_sm()
                .when(primary, |button| {
                    button
                        .bg(gpui::rgb(theme::accent()))
                        .text_color(gpui::rgb(theme::bg_app()))
                })
                .when(!primary, |button| {
                    button
                        .border_1()
                        .border_color(gpui::rgb(theme::border()))
                        .text_color(gpui::rgb(theme::text_secondary()))
                })
                .when(saving, |button| button.opacity(0.6))
                .when(!saving, |button| {
                    button.hover(|style| style.cursor_pointer().opacity(0.9))
                })
                .child(label)
        };
        let keep_path = status.path.clone();
        let trust_path = status.path.clone();
        div()
            .id("trust-backdrop")
            .absolute()
            .size_full()
            .top_0()
            .left_0()
            .bg(gpui::rgba(0x000000a0))
            .flex()
            .items_center()
            .justify_center()
            .child(
                div()
                    .w(px(460.))
                    .p_5()
                    .rounded_lg()
                    .bg(gpui::rgb(theme::bg_elevated()))
                    .border_1()
                    .border_color(gpui::rgb(theme::border()))
                    .flex()
                    .flex_col()
                    .gap_3()
                    .child(
                        div()
                            .text_lg()
                            .font_weight(gpui::FontWeight::SEMIBOLD)
                            .text_color(gpui::rgb(theme::text_primary()))
                            .child(format!("Trust {name}?")),
                    )
                    .child(
                        div()
                            .text_sm()
                            .text_color(gpui::rgb(theme::text_secondary()))
                            .child(
                                "Trusting a project lets Maple use project-provided guidance, \
                                 including agent skills. These instructions can influence how \
                                 agents work and use tools. Maple's tool permissions still \
                                 apply, and you can change this choice later from the \
                                 project's menu.",
                            ),
                    )
                    .child(
                        div()
                            .text_xs()
                            .font_family("monospace")
                            .text_color(gpui::rgb(theme::text_muted()))
                            .child(status.path.clone()),
                    )
                    .child(
                        div()
                            .flex()
                            .justify_end()
                            .gap_2()
                            .child(button("trust-keep", "Keep untrusted", false).on_click(
                                cx.listener(move |this, _event, _window, cx| {
                                    this.set_project_trust(keep_path.clone(), false, cx);
                                }),
                            ))
                            .child(button("trust-allow", "Trust project", true).on_click(
                                cx.listener(move |this, _event, _window, cx| {
                                    this.set_project_trust(trust_path.clone(), true, cx);
                                }),
                            )),
                    ),
            )
    }

    /// Ask before a project leaves the sidebar.
    fn request_remove_root(&mut self, root: &str, cx: &mut Context<Self>) {
        self.project_menu = None;
        self.confirm_remove_root = Some(root.to_string());
        cx.notify();
    }

    fn confirm_remove_root(&mut self, cx: &mut Context<Self>) {
        if let Some(root) = self.confirm_remove_root.take() {
            self.archive_root(&root, cx);
        }
        cx.notify();
    }

    /// The rename field for `target` when it is the one being edited.
    fn rename_field(&self, target: &RenameTarget) -> Option<gpui::Stateful<Div>> {
        if self.rename.as_ref() != Some(target) {
            return None;
        }
        let input = self.rename_input.clone()?;
        Some(
            div()
                .id("rename-field")
                .flex_1()
                .min_w_0()
                .on_click(|_event, _window, cx| cx.stop_propagation())
                .child(input),
        )
    }

    /// Overflow menu for a project row.
    fn render_project_menu(&self, root: &str, cx: &mut Context<Self>) -> gpui::Deferred {
        let menu_item =
            |id: String, icon_name: &'static str, label: &'static str, on_click: MenuAction| {
                div()
                    .id(SharedString::from(id))
                    .flex()
                    .items_center()
                    .gap_2()
                    .px_3()
                    .py_1p5()
                    .text_sm()
                    .text_color(gpui::rgb(theme::text_primary()))
                    .hover(|style| {
                        style
                            .bg(gpui::rgb(theme::bg_sidebar_row_hover()))
                            .cursor_pointer()
                    })
                    .on_click(cx.listener(move |this, _event, _window, cx| {
                        cx.stop_propagation();
                        on_click(this, cx);
                    }))
                    .child(icon(icon_name, px(14.), theme::text_secondary()))
                    .child(label)
            };
        let rename_root = root.to_string();
        let open_root = root.to_string();
        let remove_root = root.to_string();
        let trust_item = self
            .menu_trust
            .as_ref()
            .filter(|status| status.available && status.path == root)
            .map(|status| {
                let trusted = status.decision == Some(true);
                let path = status.path.clone();
                menu_item(
                    format!("trust-project-{root}"),
                    if trusted { "shield-check" } else { "lock" },
                    if trusted {
                        "Untrust project"
                    } else {
                        "Trust project"
                    },
                    Box::new(move |this, cx| this.set_project_trust(path.clone(), !trusted, cx)),
                )
            });
        gpui::deferred(
            div()
                .id(SharedString::from(format!("project-menu-{root}")))
                .absolute()
                .top(px(30.))
                .right_0()
                .w(px(180.))
                .py_1()
                .rounded_md()
                .bg(gpui::rgb(theme::bg_elevated()))
                .border_1()
                .border_color(gpui::rgb(theme::border()))
                .shadow_md()
                .flex()
                .flex_col()
                .on_mouse_down_out(cx.listener(|this, _event, _window, cx| {
                    this.project_menu = None;
                    cx.notify();
                }))
                .child(menu_item(
                    format!("rename-project-{root}"),
                    "pencil",
                    "Rename project",
                    Box::new(move |this, cx| {
                        this.begin_rename(RenameTarget::Project(rename_root.clone()), cx)
                    }),
                ))
                .child(menu_item(
                    format!("open-project-{root}"),
                    "folder-open",
                    "Open folder",
                    Box::new(move |this, cx| this.open_folder(&open_root, cx)),
                ))
                .children(trust_item)
                .child(menu_item(
                    format!("remove-project-{root}"),
                    "trash-2",
                    "Remove project",
                    Box::new(move |this, cx| this.request_remove_root(&remove_root, cx)),
                )),
        )
    }

    /// Modal that confirms a project removal.
    fn render_confirm_remove(&self, root: &str, cx: &mut Context<Self>) -> gpui::Stateful<Div> {
        let name = self.root_name(root);
        let button = |id: &'static str, label: &'static str, primary: bool| {
            div()
                .id(id)
                .px_3()
                .py_1p5()
                .rounded_md()
                .text_sm()
                .when(primary, |button| {
                    button
                        .bg(gpui::rgb(theme::status_error()))
                        .text_color(gpui::rgb(theme::text_primary()))
                })
                .when(!primary, |button| {
                    button
                        .border_1()
                        .border_color(gpui::rgb(theme::border()))
                        .text_color(gpui::rgb(theme::text_secondary()))
                })
                .hover(|style| style.cursor_pointer().opacity(0.9))
                .child(label)
        };
        div()
            .id("confirm-remove-backdrop")
            .absolute()
            .size_full()
            .top_0()
            .left_0()
            .bg(gpui::rgba(0x000000a0))
            .flex()
            .items_center()
            .justify_center()
            .on_click(cx.listener(|this, _event, _window, cx| {
                this.confirm_remove_root = None;
                cx.notify();
            }))
            .child(
                div()
                    .id("confirm-remove-card")
                    .w(px(420.))
                    .p_5()
                    .rounded_lg()
                    .bg(gpui::rgb(theme::bg_elevated()))
                    .border_1()
                    .border_color(gpui::rgb(theme::border()))
                    .flex()
                    .flex_col()
                    .gap_3()
                    .on_click(|_event, _window, cx| cx.stop_propagation())
                    .child(
                        div()
                            .text_lg()
                            .font_weight(gpui::FontWeight::SEMIBOLD)
                            .text_color(gpui::rgb(theme::text_primary()))
                            .child(format!("Remove {name}?")),
                    )
                    .child(
                        div()
                            .text_sm()
                            .text_color(gpui::rgb(theme::text_secondary()))
                            .child(
                                "The project leaves the sidebar and its tasks move to \
                                 Archived, where you can restore them. No files are deleted.",
                            ),
                    )
                    .child(
                        div()
                            .text_xs()
                            .font_family("monospace")
                            .text_color(gpui::rgb(theme::text_muted()))
                            .child(root.to_string()),
                    )
                    .child(
                        div()
                            .flex()
                            .justify_end()
                            .gap_2()
                            .child(button("confirm-remove-cancel", "Cancel", false).on_click(
                                cx.listener(|this, _event, _window, cx| {
                                    cx.stop_propagation();
                                    this.confirm_remove_root = None;
                                    cx.notify();
                                }),
                            ))
                            .child(button("confirm-remove-ok", "Remove", true).on_click(
                                cx.listener(|this, _event, _window, cx| {
                                    cx.stop_propagation();
                                    this.confirm_remove_root(cx);
                                }),
                            )),
                    ),
            )
    }

    /// Fold or unfold a project's task list. Unfolding a project that is
    /// not current also makes it current, so New Task lands in it.
    fn toggle_root_collapsed(&mut self, root: &str, cx: &mut Context<Self>) {
        if self.collapsed_roots.remove(root) {
            if self.project_root.as_deref() != Some(root) {
                self.switch_root(root.to_string(), cx);
            }
        } else {
            self.collapsed_roots.insert(root.to_string());
        }
        cx.notify();
    }

    /// Archive or restore one task. The service event updates the row;
    /// an archived selection moves to the newest task in the same root.
    fn set_session_archived(&mut self, session_id: &str, archived: bool, cx: &mut Context<Self>) {
        let backend = self.backend.clone();
        let user_id = self.user_id.clone();
        let session_id = session_id.to_string();
        let changed_id = session_id.clone();
        self.call(
            async move {
                backend
                    .set_session_archived(&user_id, &session_id, archived)
                    .await
            },
            cx,
            move |this, result, cx| {
                match result {
                    Ok(session) => {
                        let root = session.project_root.clone();
                        this.upsert_session(session);
                        if archived && this.selected_session.as_deref() == Some(&*changed_id) {
                            this.selected_session = None;
                            this.replace_timeline(Vec::new());
                            let next = this
                                .sessions
                                .iter()
                                .find(|s| !s.archived && s.project_root == root)
                                .map(|s| s.id.clone());
                            if let Some(id) = next {
                                this.select_session(&id, cx);
                            }
                        }
                    }
                    Err(error) => this.notice = Some(error.into()),
                }
                cx.notify();
            },
        );
    }

    /// Archive every task in a project and drop the project from the
    /// sidebar. The runtime moves to the next project when this one was
    /// current.
    fn archive_root(&mut self, root: &str, cx: &mut Context<Self>) {
        if self.root_switching {
            return;
        }
        let backend = self.backend.clone();
        let user_id = self.user_id.clone();
        let path = root.to_string();
        let fallback = self
            .project_groups
            .iter()
            .map(|(candidate, _)| candidate.clone())
            .find(|candidate| candidate != root);
        let task_ids: Vec<String> = self
            .sessions
            .iter()
            .filter(|s| !s.archived && s.project_root == root)
            .map(|s| s.id.clone())
            .collect();
        let removed = path.clone();
        let next_root = fallback.clone();
        self.call(
            async move {
                for id in task_ids {
                    backend.set_session_archived(&user_id, &id, true).await?;
                }
                backend.remove_project_root(&user_id, path, fallback).await
            },
            cx,
            move |this, result, cx| {
                match result {
                    Ok(()) => {
                        this.recent_roots.retain(|candidate| candidate != &removed);
                        this.collapsed_roots.remove(&removed);
                        for session in &mut this.sessions {
                            if session.project_root == removed {
                                session.archived = true;
                            }
                        }
                        let was_current = this.project_root.as_deref() == Some(&*removed);
                        if was_current {
                            this.selected_session = None;
                            this.replace_timeline(Vec::new());
                            this.project_root = next_root.clone();
                        }
                        this.rebuild_project_groups();
                        if was_current && let Some(next) = next_root {
                            this.switch_root(next, cx);
                        } else {
                            this.refresh_roots(cx);
                        }
                    }
                    Err(error) => this.notice = Some(error.into()),
                }
                cx.notify();
            },
        );
    }

    fn render_sidebar(&self, cx: &mut Context<Self>) -> Div {
        let selected = self.selected_session.as_deref();
        let current_root = self.project_root.as_deref();
        let section_label = |text: &'static str| {
            div()
                .text_xs()
                .font_weight(gpui::FontWeight::MEDIUM)
                .text_color(gpui::rgb(theme::text_secondary()))
                .child(text.to_uppercase())
        };
        div()
            .w(px(300.))
            .h_full()
            .flex()
            .flex_col()
            .bg(gpui::rgb(theme::bg_sidebar()))
            .child(
                div()
                    .flex()
                    .items_center()
                    .justify_between()
                    .pl_4()
                    .pr_3()
                    .pt_3()
                    .pb_2()
                    .child(wordmark(px(16.), theme::text_primary()))
                    .child(self.render_sidebar_toggle(cx)),
            )
            .child(
                div()
                    .id("new-task")
                    .mx_2()
                    .mt_3()
                    .px_2()
                    .py_1p5()
                    .rounded_md()
                    .flex()
                    .items_center()
                    .gap_2()
                    .text_sm()
                    .text_color(gpui::rgb(theme::accent()))
                    .hover(|style| {
                        style
                            .bg(gpui::rgb(theme::bg_sidebar_row_hover()))
                            .cursor_pointer()
                    })
                    .on_click(cx.listener(|this, _event, _window, cx| {
                        this.new_session(cx);
                    }))
                    .child(icon("square-pen", px(16.), theme::accent()))
                    .child("New Task"),
            )
            .child(
                div()
                    .id("session-list")
                    .flex_1()
                    .flex()
                    .flex_col()
                    .overflow_y_scroll()
                    .track_scroll(&self.sidebar_scroll)
                    .px_4()
                    .pt_6()
                    .children(self.search_input.clone().map(|input| {
                        let active = !self.sidebar_filter.is_empty();
                        div()
                            .flex()
                            .items_center()
                            .gap_2()
                            .mb_3()
                            .px_2()
                            .py_1()
                            .rounded_md()
                            .bg(gpui::rgb(theme::bg_sidebar_pill()))
                            .border_1()
                            .border_color(gpui::rgb(if active {
                                theme::accent()
                            } else {
                                theme::border_subtle()
                            }))
                            .text_sm()
                            .child(icon("search", px(14.), theme::text_muted()))
                            .child(div().flex_1().min_w_0().child(input))
                            .when(active, |row| {
                                row.child(
                                    div()
                                        .id("search-clear")
                                        .size_5()
                                        .flex()
                                        .items_center()
                                        .justify_center()
                                        .rounded_md()
                                        .hover(|style| style.cursor_pointer())
                                        .on_click(cx.listener(|this, _event, _window, cx| {
                                            this.clear_search(cx);
                                        }))
                                        .child(icon("x", px(12.), theme::text_secondary())),
                                )
                            })
                    }))
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .justify_between()
                            .mb_3()
                            .child(section_label("Projects"))
                            .child(
                                div()
                                    .id("new-project")
                                    .size_6()
                                    .flex()
                                    .items_center()
                                    .justify_center()
                                    .rounded_md()
                                    .hover(|style| {
                                        style
                                            .bg(gpui::rgb(theme::bg_sidebar_row_hover()))
                                            .cursor_pointer()
                                    })
                                    .on_click(cx.listener(|this, _event, _window, cx| {
                                        this.choose_root_dialog(cx);
                                    }))
                                    .child(icon("folder-plus", px(16.), theme::text_secondary())),
                            ),
                    )
                    .children(self.project_groups.iter().map(|(root, indices)| {
                        let is_current = current_root == Some(root.as_str());
                        let is_collapsed = self.collapsed_roots.contains(root);
                        let is_pinned = self.pinned_roots.iter().any(|pinned| pinned == root);
                        let name = self.root_name(root);
                        let tasks = indices.iter().filter_map(|index| self.sessions.get(*index));
                        let group_name = SharedString::from(format!("project-row-{root}"));
                        let rename_field = self.rename_field(&RenameTarget::Project(root.clone()));
                        let menu = (self.project_menu.as_deref() == Some(root.as_str()))
                            .then(|| self.render_project_menu(root, cx));
                        div()
                            .relative()
                            .flex()
                            .flex_col()
                            .mb_2()
                            .child(
                                div()
                                    .id(SharedString::from(format!("project-{root}")))
                                    .group(group_name.clone())
                                    .flex()
                                    .items_center()
                                    .gap_1p5()
                                    .px_2()
                                    .py_1()
                                    .rounded_md()
                                    .text_sm()
                                    .font_weight(gpui::FontWeight::MEDIUM)
                                    .text_color(gpui::rgb(theme::text_primary()))
                                    .hover(|style| {
                                        style
                                            .bg(gpui::rgb(theme::bg_sidebar_row_hover()))
                                            .cursor_pointer()
                                    })
                                    .on_click({
                                        let root = root.clone();
                                        cx.listener(move |this, _event, _window, cx| {
                                            this.toggle_root_collapsed(&root, cx);
                                        })
                                    })
                                    .child(icon(
                                        if is_collapsed {
                                            "chevron-right"
                                        } else {
                                            "chevron-down"
                                        },
                                        px(14.),
                                        theme::text_secondary(),
                                    ))
                                    .child(icon(
                                        if is_current { "folder-open" } else { "folder" },
                                        px(16.),
                                        theme::text_primary(),
                                    ))
                                    .when_some(rename_field, |row, field| row.child(field))
                                    .when(
                                        self.rename.as_ref()
                                            != Some(&RenameTarget::Project(root.clone())),
                                        |row| {
                                            row.child(
                                                div().flex_1().min_w_0().line_clamp(1).child(name),
                                            )
                                        },
                                    )
                                    .when(is_pinned, |row| {
                                        // Pinned: the always-visible pin is
                                        // the unpin button itself.
                                        row.child(
                                            div()
                                                .id(SharedString::from(format!(
                                                    "pin-project-{root}"
                                                )))
                                                .size_5()
                                                .flex()
                                                .items_center()
                                                .justify_center()
                                                .hover(|style| style.cursor_pointer())
                                                .on_click({
                                                    let root = root.clone();
                                                    cx.listener(move |this, _event, _window, cx| {
                                                        cx.stop_propagation();
                                                        this.toggle_pin(&root, cx);
                                                    })
                                                })
                                                .child(icon("pin", px(13.), theme::accent())),
                                        )
                                    })
                                    .when(!is_pinned, |row| {
                                        row.child(row_action(
                                            SharedString::from(format!("pin-project-{root}")),
                                            &group_name,
                                            "pin",
                                            {
                                                let root = root.clone();
                                                cx.listener(move |this, _event, _window, cx| {
                                                    cx.stop_propagation();
                                                    this.toggle_pin(&root, cx);
                                                })
                                            },
                                        ))
                                    })
                                    .child(row_action(
                                        SharedString::from(format!("menu-project-{root}")),
                                        &group_name,
                                        "ellipsis",
                                        {
                                            let root = root.clone();
                                            cx.listener(move |this, _event, _window, cx| {
                                                cx.stop_propagation();
                                                this.toggle_project_menu(&root, cx);
                                            })
                                        },
                                    )),
                            )
                            .when(!is_collapsed, |column| {
                                column.children(tasks.map(|session| {
                                    self.render_task_row(session, selected, false, cx)
                                }))
                            })
                            .children(menu)
                    }))
                    .when(!self.archived_indices.is_empty(), |list| {
                        let expanded = self.archived_expanded;
                        let count = self.archived_indices.len();
                        list.child(
                            div()
                                .mt_5()
                                .flex()
                                .flex_col()
                                .child(
                                    div()
                                        .id("archived-toggle")
                                        .flex()
                                        .items_center()
                                        .gap_1p5()
                                        .mb_1()
                                        .px_2()
                                        .py_1()
                                        .rounded_md()
                                        .hover(|style| {
                                            style
                                                .bg(gpui::rgb(theme::bg_sidebar_row_hover()))
                                                .cursor_pointer()
                                        })
                                        .on_click(cx.listener(|this, _event, _window, cx| {
                                            this.archived_expanded = !this.archived_expanded;
                                            cx.notify();
                                        }))
                                        .child(icon(
                                            if expanded {
                                                "chevron-down"
                                            } else {
                                                "chevron-right"
                                            },
                                            px(14.),
                                            theme::text_secondary(),
                                        ))
                                        .child(section_label("Archived"))
                                        .child(
                                            div()
                                                .text_xs()
                                                .text_color(gpui::rgb(theme::text_muted()))
                                                .child(count.to_string()),
                                        ),
                                )
                                .when(expanded, |column| {
                                    column.children(self.archived_indices.iter().filter_map(
                                        |index| {
                                            let session = self.sessions.get(*index)?;
                                            Some(self.render_task_row(session, selected, true, cx))
                                        },
                                    ))
                                }),
                        )
                    }),
            )
            .child(self.render_sidebar_footer(cx))
    }

    /// One task row. Archived rows show the project name under the title
    /// and a restore button; live rows show an archive button on hover.
    fn render_task_row(
        &self,
        session: &AgentSessionSummary,
        selected: Option<&str>,
        archived: bool,
        cx: &mut Context<Self>,
    ) -> gpui::Stateful<Div> {
        let is_selected = selected == Some(session.id.as_str());
        let session_id = session.id.clone();
        let action_id = session.id.clone();
        let rename_id = session.id.clone();
        let rename_target = RenameTarget::Task(session.id.clone());
        let rename_field = self.rename_field(&rename_target);
        let renaming = rename_field.is_some();
        let group_name = SharedString::from(format!("task-row-{}", session.id));
        div()
            .id(SharedString::from(format!("session-{}", session.id)))
            .group(group_name.clone())
            .flex()
            .items_center()
            .gap_1p5()
            .when(archived, |row| row.pl_2())
            .when(!archived, |row| row.pl_8())
            .pr_2()
            .py_1()
            .rounded_lg()
            .text_sm()
            .when(is_selected, |row| {
                row.bg(gpui::rgb(theme::bg_sidebar_row_selected()))
                    .font_weight(gpui::FontWeight::MEDIUM)
            })
            .text_color(gpui::rgb(theme::text_primary()))
            .hover(|style| {
                style
                    .bg(gpui::rgb(theme::bg_sidebar_row_hover()))
                    .cursor_pointer()
            })
            .on_click(cx.listener(move |this, _event, _window, cx| {
                this.open_session(&session_id, cx);
            }))
            .when_some(rename_field, |row, field| row.child(field))
            .when(!renaming, |row| {
                row.child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .flex()
                        .flex_col()
                        .child(div().line_clamp(1).child(session.title.clone()))
                        .when(archived, |column| {
                            column.child(
                                div()
                                    .text_xs()
                                    .text_color(gpui::rgb(theme::text_muted()))
                                    .line_clamp(1)
                                    .child(self.root_name(&session.project_root)),
                            )
                        }),
                )
            })
            .child(row_action(
                SharedString::from(format!("rename-session-{}", session.id)),
                &group_name,
                "pencil",
                cx.listener(move |this, _event, _window, cx| {
                    cx.stop_propagation();
                    this.begin_rename(RenameTarget::Task(rename_id.clone()), cx);
                }),
            ))
            .child(row_action(
                SharedString::from(format!("archive-session-{}", session.id)),
                &group_name,
                if archived {
                    "archive-restore"
                } else {
                    "archive"
                },
                cx.listener(move |this, _event, _window, cx| {
                    cx.stop_propagation();
                    this.set_session_archived(&action_id, !archived, cx);
                }),
            ))
    }

    /// Sidebar footer: just the settings gear. Plan usage stays loaded for
    /// gating image attachments, but is not shown here.
    fn render_sidebar_footer(&self, cx: &mut Context<Self>) -> Div {
        let gear = div()
            .id("open-settings")
            .flex_none()
            .size_6()
            .flex()
            .items_center()
            .justify_center()
            .rounded_md()
            .size_9()
            .rounded_full()
            .hover(|style| {
                style
                    .bg(gpui::rgb(theme::bg_sidebar_row_hover()))
                    .cursor_pointer()
            })
            .on_click(cx.listener(|_this, _event, _window, cx| {
                cx.emit(OpenSettings);
            }))
            .child(icon("settings", px(16.), theme::text_secondary()));
        div().flex().items_center().px_3().py_2().child(gear)
    }

    /// Raise a desktop notification when enabled and the window is not
    /// focused.
    fn notify_desktop(&self, title: &str, body: &str) {
        if !self.notify_enabled {
            log::info!("desktop notification skipped (disabled): {title}");
            return;
        }
        if self.window_active {
            log::info!("desktop notification skipped (window active): {title}");
            return;
        }
        log::info!("desktop notification sent: {title}");
        crate::notify::notify_desktop(title, &body.replace('\n', " "));
    }

    /// Load the plan card from the Maple billing API. Failures keep the
    /// previous card; the token-total fallback covers the first load.
    fn refresh_sidebar_plan(&mut self, cx: &mut Context<Self>) {
        let backend = self.backend.clone();
        let user_id = self.user_id.clone();
        self.call(
            async move { backend.plan_usage(&user_id).await },
            cx,
            |this, result: Result<Option<crate::billing::PlanUsage>, String>, cx| match result {
                Ok(plan) => {
                    this.sidebar_plan = plan;
                    cx.notify();
                }
                Err(error) => log::warn!("plan usage unavailable: {error}"),
            },
        );
    }

    fn render_header(&self) -> Div {
        let title = self
            .sessions
            .iter()
            .find(|session| Some(session.id.as_str()) == self.selected_session.as_deref())
            .map(|session| SharedString::from(session.title.clone()))
            .unwrap_or_else(|| SharedString::from("New Task"));
        div()
            .flex()
            .items_center()
            .justify_between()
            .gap_3()
            .h(px(40.))
            .flex_none()
            .pl_4()
            .pr_3()
            .when(self.sidebar_collapsed, |row| row.pl(px(220.)))
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .text_lg()
                    .line_height(px(24.))
                    .font_weight(gpui::FontWeight::SEMIBOLD)
                    .text_color(gpui::rgb(theme::text_primary()))
                    .line_clamp(1)
                    .child(title),
            )
    }

    /// The open header menu as an inline panel. Rendered in normal flow
    /// below the header; deferred/absolute anchoring proved unreliable.
    fn render_menu_panel(&self, cx: &mut Context<Self>) -> Option<Div> {
        let mut menu = div()
            .flex()
            .flex_col()
            .mt_1()
            .py_1()
            .rounded_lg()
            .bg(gpui::rgb(theme::bg_elevated()))
            .border_1()
            .border_color(gpui::rgb(theme::border()));
        if self.root_menu_open {
            for path in self.recent_roots.iter().take(6) {
                let is_current = self.project_root.as_deref() == Some(path.as_str());
                menu = menu.child(
                    div()
                        .id(gpui::SharedString::from(format!("root-{}", path)))
                        .px_3()
                        .py_1()
                        .text_sm()
                        .text_color(gpui::rgb(if is_current {
                            theme::accent()
                        } else {
                            theme::text_primary()
                        }))
                        .line_clamp(1)
                        .hover(|style| style.bg(gpui::rgb(theme::bg_input())).cursor_pointer())
                        .on_click({
                            let path = path.clone();
                            cx.listener(move |this, _event, _window, cx| {
                                this.switch_root(path.clone(), cx);
                            })
                        })
                        .child(path.clone()),
                );
            }
            menu = menu.child(
                div()
                    .id("root-choose")
                    .px_3()
                    .py_1()
                    .text_sm()
                    .text_color(gpui::rgb(theme::text_secondary()))
                    .hover(|style| style.bg(gpui::rgb(theme::bg_input())).cursor_pointer())
                    .on_click(cx.listener(|this, _event, _window, cx| {
                        this.choose_root_dialog(cx);
                    }))
                    .child("New project…"),
            );
            if let Some(input) = self.root_input.clone() {
                menu = menu
                    .child(
                        div()
                            .px_3()
                            .pb_1()
                            .text_xs()
                            .text_color(gpui::rgb(theme::text_muted()))
                            .child("Or type an absolute path:"),
                    )
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap_2()
                            .px_3()
                            .pb_2()
                            .child(div().flex_1().child(input))
                            .child(
                                div()
                                    .id("root-apply")
                                    .px_3()
                                    .py_1()
                                    .rounded_md()
                                    .bg(gpui::rgb(theme::accent()))
                                    .text_sm()
                                    .text_color(gpui::rgb(theme::text_primary()))
                                    .hover(|style| style.cursor_pointer())
                                    .on_click(cx.listener(|this, _event, _window, cx| {
                                        if let Some(path) = this
                                            .root_input
                                            .as_ref()
                                            .map(|input| input.read(cx).text())
                                        {
                                            this.switch_root(path, cx);
                                        }
                                    }))
                                    .child("Go"),
                            ),
                    );
            }
            return Some(menu);
        }
        if self.mode_menu_open {
            for (mode, label, note) in [
                (
                    "auto",
                    "Allow all",
                    "Approve every tool call without asking",
                ),
                ("smart_approve", "Ask first", "Confirm each gated tool call"),
            ] {
                let mode_icon = icon(permission_mode_icon(mode), px(14.), theme::text_secondary());
                let mode = mode.to_string();
                let is_current = self.permission_mode == mode;
                menu = menu.child(
                    div()
                        .id(gpui::SharedString::from(format!("mode-{mode}")))
                        .px_3()
                        .py_1()
                        .text_sm()
                        .text_color(gpui::rgb(if is_current {
                            theme::accent()
                        } else {
                            theme::text_primary()
                        }))
                        .hover(|style| style.bg(gpui::rgb(theme::bg_input())).cursor_pointer())
                        .on_click(cx.listener(move |this, _event, _window, cx| {
                            this.permission_mode.clone_from(&mode);
                            this.uses_default_permission_mode = false;
                            this.mode_menu_open = false;
                            this.apply_permission_mode(cx);
                            cx.notify();
                        }))
                        .child(
                            div()
                                .flex()
                                .flex_col()
                                .gap_0()
                                .child(
                                    div()
                                        .flex()
                                        .items_center()
                                        .gap_1p5()
                                        .child(mode_icon)
                                        .child(label),
                                )
                                .child(
                                    div()
                                        .text_xs()
                                        .text_color(gpui::rgb(theme::text_muted()))
                                        .child(note),
                                ),
                        ),
                );
            }
            return Some(menu);
        }
        if self.mcp_menu_open {
            menu = menu.child(
                div()
                    .px_3()
                    .pt_1()
                    .pb_2()
                    .text_xs()
                    .font_weight(gpui::FontWeight::MEDIUM)
                    .text_color(gpui::rgb(theme::text_secondary()))
                    .child("MCP servers"),
            );
            if self.session_mcp.is_empty() {
                menu = menu.child(
                    div()
                        .px_3()
                        .py_2()
                        .text_sm()
                        .text_color(gpui::rgb(theme::text_muted()))
                        .child("No MCP servers configured."),
                );
            }
            for server in &self.session_mcp {
                let name = server.name.clone();
                let enabled = server.enabled;
                let available = server.available;
                menu = menu.child(
                    div()
                        .id(gpui::SharedString::from(format!("mcp-{name}")))
                        .flex()
                        .items_center()
                        .gap_3()
                        .px_3()
                        .py_1p5()
                        .hover(|style| style.bg(gpui::rgb(theme::bg_input())).cursor_pointer())
                        .on_click(cx.listener(move |this, _event, _window, cx| {
                            this.toggle_session_mcp(name.clone(), !enabled, cx);
                        }))
                        .child(
                            div()
                                .flex_1()
                                .min_w_0()
                                .flex()
                                .flex_col()
                                .child(
                                    div()
                                        .text_sm()
                                        .font_weight(gpui::FontWeight::MEDIUM)
                                        .text_color(gpui::rgb(theme::text_primary()))
                                        .line_clamp(1)
                                        .child(server.name.clone()),
                                )
                                .when(!server.description.is_empty(), |col| {
                                    col.child(
                                        div()
                                            .text_xs()
                                            .text_color(gpui::rgb(theme::text_muted()))
                                            .line_clamp(2)
                                            .child(server.description.clone()),
                                    )
                                })
                                .when(!available, |col| {
                                    col.child(
                                        div()
                                            .text_xs()
                                            .text_color(gpui::rgb(theme::status_warning()))
                                            .child("Not available in this task"),
                                    )
                                }),
                        )
                        .child(
                            div()
                                .w(px(32.))
                                .h(px(18.))
                                .p(px(2.))
                                .rounded_full()
                                .bg(gpui::rgb(if enabled {
                                    theme::accent()
                                } else {
                                    theme::border()
                                }))
                                .flex()
                                .when(enabled, |track| track.justify_end())
                                .child(div().size(px(14.)).rounded_full().bg(gpui::rgb(
                                    if enabled {
                                        theme::bg_app()
                                    } else {
                                        theme::text_secondary()
                                    },
                                ))),
                        ),
                );
            }
            menu = menu.child(
                div()
                    .id("mcp-manage")
                    .mt_1()
                    .px_3()
                    .py_1p5()
                    .border_t_1()
                    .border_color(gpui::rgb(theme::border_subtle()))
                    .text_sm()
                    .text_color(gpui::rgb(theme::accent()))
                    .hover(|style| style.bg(gpui::rgb(theme::bg_input())).cursor_pointer())
                    .on_click(cx.listener(|this, _event, _window, cx| {
                        this.mcp_menu_open = false;
                        cx.emit(OpenSettingsSection(Section::Mcp));
                    }))
                    .child("Manage servers…"),
            );
            return Some(menu);
        }
        if self.models_menu_open {
            menu = menu.children(self.models.iter().map(|model| {
                div()
                    .id(gpui::SharedString::from(format!("model-{model}")))
                    .px_3()
                    .py_1()
                    .text_sm()
                    .text_color(gpui::rgb(theme::text_primary()))
                    .hover(|style| style.bg(gpui::rgb(theme::bg_input())).cursor_pointer())
                    .on_click({
                        let model = model.clone();
                        cx.listener(move |this, _event, _window, cx| {
                            this.pick_model(model.clone(), cx);
                        })
                    })
                    .child(model.clone())
            }));
            return Some(menu);
        }
        None
    }

    /// Command palette shown while the composer text starts with "/".
    /// Lists built-ins plus the project's skill commands, filtered by the
    /// typed prefix; clicking completes the command in the composer.
    fn render_slash_palette(&self, cx: &mut Context<Self>) -> Option<Div> {
        let entries = &self.slash_entries;
        if entries.is_empty() {
            return None;
        }
        let selected = self.slash_selected.filter(|index| *index < entries.len());
        let chat = cx.entity().downgrade();
        let mut palette = div()
            .flex()
            .flex_col()
            .mt_1()
            .py_1()
            .rounded_lg()
            .bg(gpui::rgb(theme::bg_elevated()))
            .border_1()
            .border_color(gpui::rgb(theme::border()));
        for (index, entry) in entries.iter().enumerate() {
            let is_selected = selected == Some(index);
            let name = entry.name.clone();
            let chat = chat.clone();
            palette = palette.child(
                div()
                    .id(gpui::SharedString::from(format!("slash-{}", entry.name)))
                    .flex()
                    .items_baseline()
                    .gap_2()
                    .px_3()
                    .when(is_selected, |row| {
                        row.bg(gpui::rgb(theme::bg_sidebar_row_selected()))
                    })
                    .hover(|style| style.bg(gpui::rgb(theme::bg_input())).cursor_pointer())
                    .on_click(move |_event, _window, cx: &mut gpui::App| {
                        chat.update(cx, |chat, cx| chat.complete_slash_command(&name, cx))
                            .ok();
                    })
                    .child(
                        div()
                            .font_family("monospace")
                            .text_color(gpui::rgb(theme::accent()))
                            .child(format!("/{}", entry.name)),
                    )
                    .child(
                        div()
                            .min_w_0()
                            .flex_1()
                            .text_xs()
                            .text_color(gpui::rgb(theme::text_muted()))
                            .line_clamp(1)
                            .child(entry.description.clone()),
                    ),
            );
        }
        Some(palette)
    }

    fn render_transcript(&mut self, cx: &mut Context<Self>) -> gpui::Stateful<Div> {
        // Follow the newest content while the view sits at (or near) the
        // bottom, or after an explicit jump. Checking before render keeps
        // user scrolls intact mid-stream.
        let count = self.timeline.len();
        if self.list_state.item_count() != count {
            self.list_state.reset(count);
        }
        // Pixel offsets are unreliable here: items that have never been
        // rendered count as 0 px, so an old chat looks "all visible" until
        // the user scrolls. The logical position is exact: with bottom
        // alignment, `item_ix == count` means pinned to the newest item.
        let at_bottom = self.list_state.logical_scroll_top().item_ix >= count;
        if self.follow_transcript && !at_bottom {
            self.list_state.scroll_to(gpui::ListOffset {
                item_ix: count,
                offset_in_item: px(0.),
            });
        }
        self.follow_transcript = false;
        let tool_details = self.tool_details;
        let entity = cx.entity().downgrade();
        let selection = self.selection.clone();
        let transcript_focus = self.transcript_focus.clone();
        // Only the visible items (plus a small overdraw) are built each
        // frame; the list measures and caches the rest. Everything else is
        // read through the entity so nothing is cloned per frame.
        let list = gpui::list(self.list_state.clone(), move |ix, _window, cx| {
            let Some(chat) = entity.upgrade() else {
                return div().into_any_element();
            };
            let chat = chat.read(cx);
            match chat.timeline.get(ix) {
                Some(item) => {
                    let render_ctx = RenderCtx {
                        selection: selection.clone(),
                        base_ordinal: Some(chat.markdown_cache.ordinal_for(&item.id)),
                        focus: transcript_focus.clone(),
                        id_seed: item.id.clone(),
                    };
                    let transcript = TranscriptCtx {
                        markdown_cache: &chat.markdown_cache,
                        derived: &chat.derived,
                        attachment_images: &chat.attachment_images,
                        chat: &entity,
                        tool_summaries: &chat.tool_summaries,
                        summary_requests: &chat.summary_requests,
                        render: &render_ctx,
                        speech: chat.speech.as_ref(),
                        speech_available: chat.audio_caps.speech,
                    };
                    let expanded = tool_details != chat.toggled_tools.contains(&item.id);
                    let revision = chat
                        .timeline_index
                        .get(&item.id)
                        .map_or(0, |(_, revision)| *revision);
                    render_timeline_item(item, revision, expanded, ix, &transcript)
                        .into_any_element()
                }
                None => div().into_any_element(),
            }
        })
        .size_full();
        div()
            .id("transcript")
            .key_context("Transcript")
            .when_some(self.transcript_focus.clone(), |div, focus| {
                div.track_focus(&focus)
            })
            .relative()
            .flex_1()
            .flex()
            .flex_col()
            .min_h_0()
            .on_mouse_down(
                gpui::MouseButton::Right,
                cx.listener(|this, event: &gpui::MouseDownEvent, window, cx| {
                    if let Some(focus) = &this.transcript_focus {
                        window.focus(focus);
                    }
                    this.transcript_menu = Some(event.position);
                    cx.notify();
                }),
            )
            .children(self.render_transcript_menu(cx))
            .child(
                // The list element does not apply padding itself, so the
                // gutter lives here. Same column width as the composer.
                div()
                    .flex_1()
                    .min_h_0()
                    .w_full()
                    .max_w(px(900.))
                    .mx_auto()
                    .px_6()
                    .pb_4()
                    .child(list),
            )
            .child(self.render_scrollbar())
            .when_some(self.runtime_error.clone(), |container, error| {
                container.child(
                    div()
                        .mx_6()
                        .mb_2()
                        .px_3()
                        .py_2()
                        .rounded_md()
                        .bg(gpui::rgb(theme::status_error()))
                        .text_color(gpui::rgb(theme::bg_app()))
                        .text_sm()
                        .child(error),
                )
            })
            .when_some(self.notice.clone(), |container, notice| {
                container.child(
                    div()
                        .mx_6()
                        .mb_2()
                        .px_3()
                        .py_2()
                        .rounded_md()
                        .bg(gpui::rgb(theme::status_warning()))
                        .text_color(gpui::rgb(theme::bg_app()))
                        .text_sm()
                        .child(notice),
                )
            })
    }

    /// Thin scrollbar overlay driven by the virtualized list state.
    fn render_scrollbar(&self) -> impl IntoElement {
        let state = &self.list_state;
        let track_height = state.viewport_bounds().size.height;
        let max = state.max_offset_for_scrollbar().height;
        if max <= px(1.) || track_height <= px(0.) {
            return div().opacity(0.);
        }
        let content = max + track_height;
        // ratio of visible track to total content
        let ratio = track_height / content;
        let thumb_height = (track_height * ratio).max(px(24.));
        let offset = -state.scroll_px_offset_for_scrollbar().y;
        let scrollable = (track_height - thumb_height).max(px(0.));
        let progress = (offset / max).clamp(0., 1.);
        let thumb_top = scrollable * progress;
        div()
            .absolute()
            .top(px(0.))
            .right(px(2.))
            .bottom(px(0.))
            .w(px(6.))
            .flex()
            .flex_col()
            .child(
                div()
                    .w_full()
                    .h(thumb_height)
                    .mt(thumb_top)
                    .rounded_full()
                    .bg(theme::scrollbar_thumb()),
            )
    }

    fn render_composer(&mut self, cx: &mut Context<Self>) -> Div {
        let running = self.is_run_active();
        let disabled = self.booting;
        let has_text = self.composer_has_text;
        let has_images = !self.draft_images.is_empty();
        let can_send = !disabled && (has_text || has_images);
        let queue_chips = self.render_queue(cx);
        let expanded = self.composer_expanded;
        let composer = self.composer.clone();
        let mcp_enabled = self.session_mcp.iter().filter(|s| s.enabled).count();
        let drafts = &self.draft_images;
        let model_label = self
            .selected_model
            .clone()
            .unwrap_or_else(|| "Model".to_string());
        let bypass = self.permission_mode == "auto";
        let root_label = self
            .project_root
            .as_deref()
            .map(root_display_name)
            .unwrap_or_else(|| "Choose folder".to_string());
        div()
            .w_full()
            .flex()
            .flex_col()
            .when(expanded, |container| container.flex_1().min_h_0())
            .rounded(px(24.))
            .bg(gpui::rgb(theme::bg_app()))
            .border_1()
            .border_color(gpui::rgb(theme::accent()))
            .when(disabled, |container| container.opacity(0.5))
            // Files dragged from the desktop land as image attachments.
            .can_drop(|dragged, _window, _cx| {
                dragged.downcast_ref::<gpui::ExternalPaths>().is_some()
            })
            .drag_over::<gpui::ExternalPaths>(|style, _paths, _window, _cx| {
                style.bg(gpui::rgb(theme::bg_elevated()))
            })
            .on_drop(
                cx.listener(|this, paths: &gpui::ExternalPaths, _window, cx| {
                    this.add_image_paths(paths.paths().to_vec(), cx);
                }),
            )
            .children(queue_chips)
            .when(!drafts.is_empty(), |container| {
                container.child(div().flex().flex_wrap().gap_2().px_4().pt_4().children(
                    drafts.iter().enumerate().map(|(index, image)| {
                        div()
                            .relative()
                            .size_16()
                            .rounded_xl()
                            .border_1()
                            .border_color(gpui::rgb(theme::border()))
                            .bg(gpui::rgb(theme::bg_elevated()))
                            .flex()
                            .items_center()
                            .justify_center()
                            .map(|frame| match &image.thumbnail {
                                // The thumbnail is already a square crop, so
                                // it fills the frame and the corners round.
                                Some(thumbnail) => frame.child(
                                    gpui::img(gpui::ImageSource::Image(Arc::clone(thumbnail)))
                                        .size_full()
                                        .rounded_xl(),
                                ),
                                None => {
                                    frame.child(icon("image", px(20.), theme::text_secondary()))
                                }
                            })
                            .child(
                                div()
                                    .id(gpui::SharedString::from(format!("draft-remove-{index}")))
                                    .absolute()
                                    .top(px(-4.))
                                    .right(px(-4.))
                                    .size_5()
                                    .flex()
                                    .items_center()
                                    .justify_center()
                                    .rounded_full()
                                    .bg(gpui::rgb(theme::bg_elevated()))
                                    .border_1()
                                    .border_color(gpui::rgb(theme::border()))
                                    .hover(|style| style.cursor_pointer())
                                    .on_click(cx.listener(move |this, _event, _window, cx| {
                                        if index < this.draft_images.len() {
                                            this.draft_images.remove(index);
                                        }
                                        cx.notify();
                                    }))
                                    .child(icon("x", px(10.), theme::text_primary())),
                            )
                    }),
                ))
            })
            .child(
                div()
                    .flex()
                    .items_start()
                    .px_4()
                    .pt_4()
                    .pb_2()
                    .when(expanded, |row| row.flex_1().min_h_0())
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .when(expanded, |cell| cell.h_full())
                            .text_color(gpui::rgb(theme::text_primary()))
                            .children(composer),
                    )
                    .child(
                        div()
                            .id("composer-expand")
                            .size_6()
                            .flex()
                            .items_center()
                            .justify_center()
                            .rounded_md()
                            .hover(|style| {
                                style.bg(gpui::rgb(theme::bg_elevated())).cursor_pointer()
                            })
                            .on_click(cx.listener(|this, _event, _window, cx| {
                                this.toggle_composer_expanded(cx);
                            }))
                            .child(icon(
                                if expanded { "minimize-2" } else { "maximize-2" },
                                px(14.),
                                theme::text_muted(),
                            )),
                    ),
            )
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap_1()
                    .px_2()
                    .pb_2()
                    .pt_1()
                    .child(
                        chip(
                            "model-picker",
                            None,
                            model_label,
                            true,
                            self.models_menu_open,
                        )
                        .on_click(cx.listener(
                            |this, _event, _window, cx| {
                                this.root_menu_open = false;
                                this.mode_menu_open = false;
                                this.mcp_menu_open = false;
                                this.models_menu_open = !this.models_menu_open;
                                cx.notify();
                            },
                        )),
                    )
                    .child(
                        chip(
                            "permission-mode-toggle",
                            Some(permission_mode_icon(&self.permission_mode)),
                            if bypass { "Allow all" } else { "Read only" }.to_string(),
                            true,
                            self.mode_menu_open,
                        )
                        .on_click(cx.listener(
                            |this, _event, _window, cx| {
                                this.root_menu_open = false;
                                this.models_menu_open = false;
                                this.mcp_menu_open = false;
                                this.mode_menu_open = !this.mode_menu_open;
                                cx.notify();
                            },
                        )),
                    )
                    .child(
                        chip(
                            "mcp-menu",
                            Some("puzzle"),
                            mcp_enabled.to_string(),
                            false,
                            self.mcp_menu_open,
                        )
                        .on_click(cx.listener(
                            |this, _event, _window, cx| {
                                this.root_menu_open = false;
                                this.models_menu_open = false;
                                this.mode_menu_open = false;
                                this.mcp_menu_open = !this.mcp_menu_open;
                                if this.mcp_menu_open {
                                    this.refresh_session_mcp(cx);
                                }
                                cx.notify();
                            },
                        )),
                    )
                    .child(
                        chip(
                            "web-toggle",
                            Some("globe"),
                            if self.web_enabled { "Web" } else { "Web off" }.to_string(),
                            false,
                            self.web_enabled,
                        )
                        .on_click(cx.listener(
                            |this, _event, _window, cx| {
                                let next = !this.web_enabled;
                                this.set_web_enabled(next, cx);
                            },
                        )),
                    )
                    .child(
                        div()
                            .id("add-images")
                            .size_8()
                            .flex()
                            .items_center()
                            .justify_center()
                            .rounded_md()
                            .hover(|style| {
                                style.bg(gpui::rgb(theme::bg_elevated())).cursor_pointer()
                            })
                            .when(self.image_picking, |el| el.opacity(0.5))
                            .on_click(cx.listener(|this, _event, _window, cx| {
                                this.pick_images(cx);
                            }))
                            .child(icon("image", px(16.), theme::text_secondary())),
                    )
                    .when(self.audio_caps.transcription, |row| {
                        let recording = self.recording;
                        let transcribing = self.transcribing;
                        row.child(
                            div()
                                .id("record-voice")
                                .size_8()
                                .flex()
                                .items_center()
                                .justify_center()
                                .rounded_md()
                                .when(recording, |el| el.bg(gpui::rgb(theme::status_error())))
                                .hover(|style| {
                                    style.bg(gpui::rgb(theme::bg_elevated())).cursor_pointer()
                                })
                                .when(transcribing, |el| el.opacity(0.5))
                                .on_click(cx.listener(|this, _event, _window, cx| {
                                    this.toggle_recording(cx);
                                }))
                                .child(if transcribing {
                                    spinner("transcribing", px(16.), theme::text_secondary())
                                } else if recording {
                                    icon("square", px(14.), theme::bg_app()).into_any_element()
                                } else {
                                    icon("mic", px(16.), theme::text_secondary()).into_any_element()
                                }),
                        )
                    })
                    .child(
                        chip(
                            "root-picker",
                            Some("folder-open"),
                            root_label,
                            true,
                            self.root_menu_open,
                        )
                        .on_click(cx.listener(
                            |this, _event, _window, cx| {
                                this.models_menu_open = false;
                                this.mode_menu_open = false;
                                this.mcp_menu_open = false;
                                this.root_menu_open = !this.root_menu_open;
                                cx.notify();
                            },
                        )),
                    )
                    .child(div().flex_1())
                    .child(
                        div()
                            .id("context-indicator")
                            .flex()
                            .items_center()
                            .mr_1()
                            .child(crate::ui::context_ring::ContextRing::new(
                                self.context_fraction.unwrap_or(0.0),
                            )),
                    )
                    .when(running, |row| {
                        row.child(
                            div()
                                .id("stop-run")
                                .size_8()
                                .flex()
                                .items_center()
                                .justify_center()
                                .rounded_xl()
                                .bg(gpui::rgb(theme::status_error()))
                                .hover(|style| style.cursor_pointer())
                                .on_click(cx.listener(|this, _event, _window, cx| {
                                    this.stop(cx);
                                }))
                                .child(div().size_3().rounded_md().bg(gpui::rgb(theme::bg_app()))),
                        )
                    })
                    .child(
                        div()
                            .id("send-message")
                            .size_8()
                            .flex()
                            .items_center()
                            .justify_center()
                            .rounded_full()
                            .bg(gpui::linear_gradient(
                                180.,
                                gpui::linear_color_stop(gpui::rgb(theme::send_top()), 0.),
                                gpui::linear_color_stop(gpui::rgb(theme::send_bottom()), 1.),
                            ))
                            .when(!can_send, |el| el.opacity(0.4))
                            .when(can_send, |el| {
                                el.hover(|style| style.cursor_pointer())
                                    .on_click(cx.listener(|this, _event, _window, cx| {
                                        this.send_inner(cx);
                                    }))
                            })
                            .child(if disabled {
                                spinner("send-booting", px(16.), theme::bg_app())
                            } else {
                                icon("arrow-up", px(16.), theme::bg_app()).into_any_element()
                            }),
                    ),
            )
    }
}
/// One row of the slash command palette.
struct SlashEntry {
    name: String,
    description: String,
}

/// Built-in and skill commands matching a "/" token, capped for the popup.
fn slash_entries_for(token: &str, skills: &[AgentSlashCommand]) -> Vec<SlashEntry> {
    let query = token.to_lowercase();
    [
        ("compact", "Summarize the conversation to free context"),
        ("new", "Start a new task"),
        ("pin", "Pin or unpin this project"),
        ("web", "Toggle web tools for this task"),
        ("model", "Pick the model; add a name to filter"),
        ("help", "Show the available commands"),
    ]
    .into_iter()
    .map(|(name, description)| SlashEntry {
        name: name.to_string(),
        description: description.to_string(),
    })
    .chain(skills.iter().map(|command| SlashEntry {
        name: command.name.clone(),
        description: command.description.clone(),
    }))
    .filter(|entry| entry.name.to_lowercase().starts_with(&query))
    .take(8)
    .collect()
}

fn chip(
    id: &'static str,
    leading: Option<&'static str>,
    label: String,
    chevron: bool,
    active: bool,
) -> gpui::Stateful<Div> {
    let color = if active {
        theme::text_primary()
    } else {
        theme::text_secondary()
    };
    div()
        .id(id)
        .h_8()
        .flex()
        .items_center()
        .gap_1()
        .px_2()
        .rounded_md()
        .text_xs()
        .font_weight(gpui::FontWeight::MEDIUM)
        .text_color(gpui::rgb(color))
        .when(active, |el| el.bg(gpui::rgb(theme::bg_elevated())))
        .hover(|style| style.bg(gpui::rgb(theme::bg_elevated())).cursor_pointer())
        .children(leading.map(|name| icon(name, px(16.), color)))
        .child(div().whitespace_nowrap().child(label))
        .when(chevron, |el| el.child(icon("chevron-down", px(14.), color)))
}

/// Last path component of a project root, for chips and the sidebar.
/// Small icon button that shows only while the pointer is over its row.
fn row_action(
    id: SharedString,
    group: &SharedString,
    icon_name: &str,
    on_click: impl Fn(&gpui::ClickEvent, &mut Window, &mut gpui::App) + 'static,
) -> gpui::Stateful<Div> {
    div()
        .id(id)
        .flex_none()
        .size_5()
        .flex()
        .items_center()
        .justify_center()
        .rounded_md()
        .opacity(0.)
        .group_hover(group.clone(), |style| style.opacity(1.))
        .hover(|style| style.bg(gpui::rgb(theme::bg_sidebar_row_selected())))
        .on_click(on_click)
        .child(icon(icon_name, px(14.), theme::text_secondary()))
}

/// Field-wise equality for session rows; the summary type has no
/// `PartialEq` of its own.
fn session_summary_eq(a: &AgentSessionSummary, b: &AgentSessionSummary) -> bool {
    a.id == b.id
        && a.title == b.title
        && a.project_root == b.project_root
        && a.created_ms == b.created_ms
        && a.updated_ms == b.updated_ms
        && a.message_count == b.message_count
        && a.model == b.model
        && a.mode == b.mode
        && a.web_enabled == b.web_enabled
        && a.archived == b.archived
}

fn root_display_name(root: &str) -> String {
    std::path::Path::new(root)
        .file_name()
        .map(|name| name.to_string_lossy().to_string())
        .unwrap_or_else(|| root.to_string())
}

/// Icon name for a permission mode: bolt for allow all, shield for read only.
fn permission_mode_icon(mode: &str) -> &'static str {
    if mode == "auto" {
        "zap"
    } else {
        "shield-check"
    }
}

const MAX_DRAFT_IMAGES: usize = 10;
const MAX_DRAFT_IMAGE_BYTES: usize = 10 * 1024 * 1024;

/// Read an image file into a draft with its thumbnail, checking size and
/// format the same way the runtime does so errors surface before the send.
/// Runs on a blocking thread; the caller assigns the id.
fn load_draft_image(path: &std::path::Path) -> Result<DraftImage, String> {
    let name = path
        .file_name()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_else(|| "image".to_string());
    let bytes = std::fs::read(path).map_err(|error| format!("Could not read {name}: {error}"))?;
    let mut draft = draft_image_from_bytes(0, name, &bytes)?;
    draft.thumbnail = square_thumbnail(&bytes).ok().map(Arc::new);
    Ok(draft)
}

/// Build a draft from raw image bytes, checking size and format the same
/// way the runtime does so errors surface before the send.
fn draft_image_from_bytes(id: u64, name: String, bytes: &[u8]) -> Result<DraftImage, String> {
    if bytes.len() > MAX_DRAFT_IMAGE_BYTES {
        return Err(format!("{name} is larger than 10 MB"));
    }
    let mime = if bytes.starts_with(&[0x89, b'P', b'N', b'G']) {
        "image/png"
    } else if bytes.starts_with(&[0xFF, 0xD8, 0xFF]) {
        "image/jpeg"
    } else if bytes.len() > 12 && &bytes[0..4] == b"RIFF" && &bytes[8..12] == b"WEBP" {
        "image/webp"
    } else {
        return Err(format!("{name} is not a PNG, JPEG, or WebP image"));
    };
    Ok(DraftImage {
        id,
        name,
        data_url: format!("data:{mime};base64,{}", base64_encode(bytes)),
        thumbnail: None,
    })
}

/// Thumbnail edge in physical pixels: 2x the 64pt box so it stays sharp
/// on HiDPI screens.
const DRAFT_THUMBNAIL_PX: u32 = 128;

/// Center-crop to a square and scale down, so the composer can show a
/// rounded square that is the picture itself. gpui clips with rectangular
/// masks only, so cropping the pixels is the one way to get round corners
/// on a cover-fit thumbnail.
fn square_thumbnail(bytes: &[u8]) -> Result<gpui::Image, String> {
    let decoded = image::load_from_memory(bytes).map_err(|error| error.to_string())?;
    let (width, height) = (decoded.width(), decoded.height());
    let edge = width.min(height);
    if edge == 0 {
        return Err("empty image".to_string());
    }
    let cropped = decoded.crop_imm((width - edge) / 2, (height - edge) / 2, edge, edge);
    let scaled = if edge > DRAFT_THUMBNAIL_PX {
        cropped.resize_exact(
            DRAFT_THUMBNAIL_PX,
            DRAFT_THUMBNAIL_PX,
            image::imageops::FilterType::Triangle,
        )
    } else {
        cropped
    };
    let mut png = std::io::Cursor::new(Vec::new());
    scaled
        .write_to(&mut png, image::ImageFormat::Png)
        .map_err(|error| error.to_string())?;
    Ok(gpui::Image::from_bytes(
        gpui::ImageFormat::Png,
        png.into_inner(),
    ))
}

/// Standard base64 with padding; small enough to avoid another dependency.
fn base64_encode(bytes: &[u8]) -> String {
    const TABLE: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity(bytes.len().div_ceil(3) * 4);
    for chunk in bytes.chunks(3) {
        let b = [
            chunk[0],
            *chunk.get(1).unwrap_or(&0),
            *chunk.get(2).unwrap_or(&0),
        ];
        let n = (u32::from(b[0]) << 16) | (u32::from(b[1]) << 8) | u32::from(b[2]);
        out.push(TABLE[(n >> 18) as usize & 63] as char);
        out.push(TABLE[(n >> 12) as usize & 63] as char);
        out.push(if chunk.len() > 1 {
            TABLE[(n >> 6) as usize & 63] as char
        } else {
            '='
        });
        out.push(if chunk.len() > 2 {
            TABLE[n as usize & 63] as char
        } else {
            '='
        });
    }
    out
}

fn render_timeline_item(
    item: &AgentTimelineItem,
    revision: u64,
    expanded: bool,
    ix: usize,
    transcript: &TranscriptCtx,
) -> Div {
    let item = match item.item_type.as_str() {
        "message" => render_message(item, revision, transcript),
        "thinking" | "reasoning" => render_thinking(item, revision, transcript),
        "tool" | "toolCall" => {
            // Dispatch on payload shape; runtime titles are humanized
            // ("todo write", "ask user") and vary by detail suffix.
            if has_tool_input(item, "todos") {
                render_todo(item)
            } else if has_tool_input(item, "edits")
                || (has_tool_input(item, "content") && has_tool_input(item, "path"))
            {
                render_tool_with_diff(item, revision, expanded, ix, transcript)
            } else {
                render_tool(item, revision, expanded, ix, transcript)
            }
        }
        "error" => render_error(item),
        "permission" => render_permission_row(item),
        _ => render_system(item),
    };
    // Per-item spacing (instead of a container gap) keeps non-renderable
    // items from producing phantom gaps.
    div().pb_2().child(item)
}

/// Attachment `(id, name)` pairs stored on a user message.
fn attachment_refs(item: &AgentTimelineItem) -> impl Iterator<Item = (&str, &str)> {
    item.input
        .as_ref()
        .and_then(|input| input.get("imageAttachments"))
        .and_then(|items| items.as_array())
        .into_iter()
        .flatten()
        .filter_map(|entry| {
            let id = entry.get("id").and_then(|id| id.as_str())?;
            let name = entry.get("name").and_then(|name| name.as_str())?;
            Some((id, name))
        })
}

/// Image format from the file signature; `None` for unsupported data.
fn image_format_from_bytes(bytes: &[u8]) -> Option<gpui::ImageFormat> {
    if bytes.starts_with(&[0x89, b'P', b'N', b'G']) {
        Some(gpui::ImageFormat::Png)
    } else if bytes.starts_with(&[0xFF, 0xD8, 0xFF]) {
        Some(gpui::ImageFormat::Jpeg)
    } else if bytes.len() > 12 && &bytes[0..4] == b"RIFF" && &bytes[8..12] == b"WEBP" {
        Some(gpui::ImageFormat::Webp)
    } else {
        None
    }
}
/// Hover-revealed button that copies one message's text.
fn copy_message_button(item_id: &str, group: &SharedString, text: &str) -> gpui::Stateful<Div> {
    let text = text.to_string();
    div()
        .id(SharedString::from(format!("copy-message-{item_id}")))
        .flex()
        .items_center()
        .gap_1()
        .px_1p5()
        .py_0p5()
        .rounded_md()
        .text_xs()
        .text_color(gpui::rgb(theme::text_muted()))
        .opacity(0.)
        .group_hover(group.clone(), |style| style.opacity(1.))
        .hover(|style| {
            style
                .bg(gpui::rgb(theme::bg_elevated()))
                .text_color(gpui::rgb(theme::text_secondary()))
                .cursor_pointer()
        })
        .on_click(move |_event, _window, cx: &mut gpui::App| {
            cx.stop_propagation();
            cx.write_to_clipboard(gpui::ClipboardItem::new_string(text.clone()));
        })
        .child(icon("copy", px(12.), theme::text_secondary()))
        .child("Copy")
}

fn render_message(item: &AgentTimelineItem, revision: u64, transcript: &TranscriptCtx) -> Div {
    let ctx = transcript.render;
    let attachment_images = transcript.attachment_images;
    let chat = transcript.chat;
    let is_user = item.role.as_deref() == Some("user");
    let text = item.text.as_deref().unwrap_or("");
    let mut attachments = attachment_refs(item).peekable();
    let has_images = attachments.peek().is_some();
    if text.trim().is_empty() && !(is_user && has_images) {
        return div();
    }
    let group = SharedString::from(format!("message-{}", item.id));
    let copy = (!text.trim().is_empty()).then(|| copy_message_button(&item.id, &group, text));
    if is_user {
        let user_ctx = RenderCtx {
            selection: ctx.selection.clone(),
            base_ordinal: ctx.base_ordinal.map(|base| base + 2048),
            focus: ctx.focus.clone(),
            id_seed: format!("{}#user", ctx.id_seed),
        };
        let ordinal = user_ctx.base_ordinal;
        div()
            .group(group)
            .flex()
            .flex_col()
            .items_end()
            .gap_0p5()
            .child(
                div()
                    .max_w(gpui::relative(0.75))
                    .px_4()
                    .py_2()
                    .rounded_lg()
                    .bg(gpui::rgb(theme::bg_user_bubble()))
                    .border_1()
                    .border_color(gpui::rgb(theme::user_bubble_border()))
                    .text_color(gpui::rgb(theme::text_primary()))
                    .when(has_images, |bubble| {
                        bubble.child(div().flex().flex_wrap().gap_2().mb_1().children(
                            attachments.map(|(id, name)| {
                                match attachment_images.get(id) {
                                    // The picture itself, scaled to fit; click
                                    // opens it full size. gpui keeps the aspect
                                    // ratio from the decoded size.
                                    Some(image) => {
                                        let click_image = Arc::clone(image);
                                        let chat = chat.clone();
                                        div().child(
                                            div()
                                                .id(gpui::SharedString::from(format!(
                                                    "attachment-{id}"
                                                )))
                                                .hover(|style| style.cursor_pointer())
                                                .on_click(
                                                    move |_event, _window, cx: &mut gpui::App| {
                                                        chat.update(cx, |chat, cx| {
                                                            chat.open_lightbox(
                                                                Arc::clone(&click_image),
                                                                cx,
                                                            );
                                                        })
                                                        .ok();
                                                    },
                                                )
                                                .child(
                                                    gpui::img(gpui::ImageSource::Image(
                                                        Arc::clone(image),
                                                    ))
                                                    .max_w(px(320.))
                                                    .max_h(px(240.))
                                                    .rounded_md()
                                                    .overflow_hidden()
                                                    .object_fit(gpui::ObjectFit::Contain)
                                                    .border_1()
                                                    .border_color(gpui::rgb(theme::border())),
                                                ),
                                        )
                                    }
                                    // Name chip until the bytes arrive (or if they
                                    // never do, such as a deleted attachment).
                                    None => div()
                                        .flex()
                                        .items_center()
                                        .gap_1()
                                        .px_2()
                                        .py_0p5()
                                        .rounded_md()
                                        .bg(gpui::rgb(theme::bg_elevated()))
                                        .text_xs()
                                        .text_color(gpui::rgb(theme::text_secondary()))
                                        .child(icon("paperclip", px(12.), theme::text_secondary()))
                                        .child(name.to_string()),
                                }
                            }),
                        ))
                    })
                    .when(!text.trim().is_empty(), |bubble| {
                        bubble.child(rich_text::plain_paragraph(
                            transcript.derived.get(item, revision).text.clone(),
                            ordinal,
                            &user_ctx,
                        ))
                    }),
            )
            .children(copy)
    } else {
        div()
            .group(group.clone())
            .max_w_full()
            .pr_2()
            .flex()
            .flex_col()
            .gap_0p5()
            .text_color(gpui::rgb(theme::text_primary()))
            .child(markdown::render_with(
                &transcript
                    .markdown_cache
                    .get(&item.id, MarkdownKind::Body, revision, text),
                ctx,
            ))
            .children(copy.map(|button| {
                div()
                    .flex()
                    .gap_1()
                    .child(button)
                    .when(transcript.speech_available, |row| {
                        let speech = transcript.speech.filter(|speech| speech.item_id == item.id);
                        row.child(speak_message_button(
                            &item.id,
                            &group,
                            text,
                            speech,
                            chat.clone(),
                        ))
                    })
            }))
    }
}

/// Hover-revealed button that reads one message aloud; stays visible and
/// turns into Stop while that message plays.
fn speak_message_button(
    item_id: &str,
    group: &SharedString,
    text: &str,
    speech: Option<&SpeechState>,
    chat: gpui::WeakEntity<ChatScreen>,
) -> gpui::Stateful<Div> {
    let text = text.to_string();
    let item_id = item_id.to_string();
    let (glyph, label) = match speech {
        None => (
            icon("volume-2", px(12.), theme::text_secondary()).into_any_element(),
            "Speak",
        ),
        Some(SpeechState { playing: false, .. }) => (
            spinner(
                &format!("speak-{item_id}"),
                px(12.),
                theme::text_secondary(),
            ),
            "Preparing…",
        ),
        Some(SpeechState { playing: true, .. }) => (
            icon("square", px(12.), theme::text_secondary()).into_any_element(),
            "Stop",
        ),
    };
    let active = speech.is_some();
    div()
        .id(SharedString::from(format!("speak-message-{item_id}")))
        .flex()
        .items_center()
        .gap_1()
        .px_1p5()
        .py_0p5()
        .rounded_md()
        .text_xs()
        .text_color(gpui::rgb(theme::text_muted()))
        .opacity(if active { 1. } else { 0. })
        .group_hover(group.clone(), |style| style.opacity(1.))
        .hover(|style| {
            style
                .bg(gpui::rgb(theme::bg_elevated()))
                .text_color(gpui::rgb(theme::text_secondary()))
                .cursor_pointer()
        })
        .on_click(move |_event, _window, cx: &mut gpui::App| {
            cx.stop_propagation();
            let item_id = item_id.clone();
            let text = text.clone();
            chat.update(cx, |chat, cx| chat.toggle_speech(item_id, text, cx))
                .ok();
        })
        .child(glyph)
        .child(label)
}

/// Longest chunk sent to text-to-speech, in words. Mirrors the Maple web
/// app; the model handles short passages best.
const SPEECH_CHUNK_MAX_WORDS: usize = 300;

/// Split markdown into plain-text chunks for text-to-speech: fenced code
/// and rules are dropped, inline markup is stripped, and paragraphs are
/// grouped up to [`SPEECH_CHUNK_MAX_WORDS`].
pub(crate) fn speech_chunks(text: &str) -> Vec<String> {
    let mut paragraphs: Vec<String> = Vec::new();
    let mut current = String::new();
    let mut in_fence = false;
    for line in text.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with("```") || trimmed.starts_with("~~~") {
            in_fence = !in_fence;
            continue;
        }
        if in_fence {
            continue;
        }
        let compact: String = trimmed.chars().filter(|c| !c.is_whitespace()).collect();
        let is_rule = compact.len() >= 3
            && (compact.chars().all(|c| c == '-')
                || compact.chars().all(|c| c == '*')
                || compact.chars().all(|c| c == '_'));
        let spoken = if is_rule {
            String::new()
        } else {
            strip_inline_markdown(trimmed)
        };
        if spoken.is_empty() {
            if !current.is_empty() {
                paragraphs.push(std::mem::take(&mut current));
            }
            continue;
        }
        if !current.is_empty() {
            current.push(' ');
        }
        current.push_str(&spoken);
    }
    if !current.is_empty() {
        paragraphs.push(current);
    }

    let mut chunks: Vec<String> = Vec::new();
    let mut chunk = String::new();
    let mut chunk_words = 0;
    for paragraph in paragraphs {
        let words = paragraph.split_whitespace().count();
        if chunk_words > 0 && chunk_words + words > SPEECH_CHUNK_MAX_WORDS {
            chunks.push(std::mem::take(&mut chunk));
            chunk_words = 0;
        }
        if words > SPEECH_CHUNK_MAX_WORDS {
            // One very long paragraph: split by sentence-ish boundaries.
            let mut piece = String::new();
            let mut piece_words = 0;
            for word in paragraph.split_whitespace() {
                if !piece.is_empty() {
                    piece.push(' ');
                }
                piece.push_str(word);
                piece_words += 1;
                let ends_sentence = word.ends_with(['.', '!', '?', ';', ':']);
                if piece_words >= SPEECH_CHUNK_MAX_WORDS
                    || (piece_words >= SPEECH_CHUNK_MAX_WORDS / 2 && ends_sentence)
                {
                    chunks.push(std::mem::take(&mut piece));
                    piece_words = 0;
                }
            }
            if !piece.is_empty() {
                chunk = piece;
                chunk_words = piece_words;
            }
            continue;
        }
        if !chunk.is_empty() {
            chunk.push(' ');
        }
        chunk.push_str(&paragraph);
        chunk_words += words;
    }
    if !chunk.is_empty() {
        chunks.push(chunk);
    }
    split_first_chunk(chunks)
}

/// Words in the opening chunk before the first sentence end that closes
/// it. Synthesis takes seconds per chunk, so a short opener starts
/// playback sooner while the rest is still on the way.
const SPEECH_FIRST_CHUNK_WORDS: usize = 40;

fn split_first_chunk(mut chunks: Vec<String>) -> Vec<String> {
    let Some(first) = chunks.first() else {
        return chunks;
    };
    let words: Vec<&str> = first.split_whitespace().collect();
    if words.len() <= SPEECH_FIRST_CHUNK_WORDS * 2 {
        return chunks;
    }
    let Some(split_at) = words
        .iter()
        .enumerate()
        .skip(SPEECH_FIRST_CHUNK_WORDS)
        .take(SPEECH_FIRST_CHUNK_WORDS)
        .find(|(_, word)| word.ends_with(['.', '!', '?', ';', ':']))
        .map(|(ix, _)| ix + 1)
    else {
        return chunks;
    };
    let opener = words[..split_at].join(" ");
    let rest = words[split_at..].join(" ");
    chunks[0] = rest;
    chunks.insert(0, opener);
    chunks
}

/// Drop heading marks, list markers, emphasis, inline code ticks, and
/// link targets from one markdown line.
fn strip_inline_markdown(line: &str) -> String {
    let mut rest = line.trim_start_matches('#').trim_start();
    rest = rest.trim_start_matches('>').trim_start();
    if let Some(stripped) = rest
        .strip_prefix("- ")
        .or_else(|| rest.strip_prefix("* "))
        .or_else(|| rest.strip_prefix("+ "))
    {
        rest = stripped;
    } else if let Some(dot) = rest.find(". ")
        && dot <= 3
        && rest[..dot].chars().all(|c| c.is_ascii_digit())
    {
        rest = &rest[dot + 2..];
    }
    if let Some(stripped) = rest
        .strip_prefix("[ ] ")
        .or_else(|| rest.strip_prefix("[x] "))
    {
        rest = stripped;
    }

    // Links: keep the label, drop the target. Images: drop entirely.
    let mut out = String::with_capacity(rest.len());
    let mut chars = rest.chars().peekable();
    while let Some(c) = chars.next() {
        match c {
            '!' if chars.peek() == Some(&'[') => {
                let tail: String = chars.clone().collect();
                if let Some(skip) = link_end(&tail) {
                    for _ in 0..skip {
                        chars.next();
                    }
                }
            }
            '[' => {
                let tail: String = chars.clone().collect();
                if let Some(close) = tail.find("](")
                    && let Some(end) = tail[close..].find(')')
                {
                    out.push_str(&tail[..close]);
                    for _ in 0..close + end + 1 {
                        chars.next();
                    }
                } else {
                    out.push(c);
                }
            }
            '*' | '_' | '`' | '~' => {}
            _ => out.push(c),
        }
    }
    out.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// Characters to skip after `!` for an image `[alt](src)`.
fn link_end(tail: &str) -> Option<usize> {
    let close = tail.find("](")?;
    let end = tail[close..].find(')')?;
    Some(close + end + 1)
}

#[cfg(test)]
mod speech_tests {
    use super::*;

    #[test]
    fn chunks_drop_code_and_markup() {
        let text = "# Title\n\nSee [the docs](https://x.y) for **bold** `code`.\n\n```rs\nfn x() {}\n```\n\n---\n\n- item one\n1. item two\n![alt](img.png)";
        assert_eq!(
            speech_chunks(text),
            vec!["Title See the docs for bold code. item one item two".to_string()]
        );
    }

    #[test]
    fn chunks_group_paragraphs_up_to_the_word_cap() {
        let paragraph = "word ".repeat(200).trim().to_string();
        let text = format!("{paragraph}\n\n{paragraph}\n\n{paragraph}");
        let chunks = speech_chunks(&text);
        // No sentence end in the opener, so it is not split off.
        assert_eq!(chunks.len(), 3);
        assert!(
            chunks
                .iter()
                .all(|chunk| chunk.split_whitespace().count() == 200)
        );
    }

    #[test]
    fn a_short_opener_is_split_off_at_a_sentence() {
        let sentence = "one two three four five six seven eight nine ten. ";
        let chunks = speech_chunks(&sentence.repeat(12));
        assert_eq!(chunks[0].split_whitespace().count(), 50);
        assert!(chunks[0].ends_with("ten."));
        assert_eq!(chunks[1].split_whitespace().count(), 70);
    }

    #[test]
    fn a_long_paragraph_splits_at_sentences() {
        let sentence = "one two three four five six seven eight nine ten. ";
        let text = sentence.repeat(50);
        let chunks = speech_chunks(&text);
        assert!(chunks.len() >= 2);
        assert!(
            chunks
                .iter()
                .all(|chunk| chunk.split_whitespace().count() <= SPEECH_CHUNK_MAX_WORDS)
        );
        assert!(chunks.iter().all(|chunk| chunk.ends_with('.')));
    }

    #[test]
    fn blank_text_has_no_chunks() {
        assert!(speech_chunks("```\ncode only\n```").is_empty());
        assert!(speech_chunks("   \n\n").is_empty());
    }
}

fn render_thinking(item: &AgentTimelineItem, revision: u64, transcript: &TranscriptCtx) -> Div {
    let text = transcript.derived.get(item, revision).text.clone();
    if text.trim().is_empty() {
        div().child(
            div()
                .text_color(gpui::rgb(theme::status_running()))
                .text_sm()
                .child("Thinking…"),
        )
    } else {
        div()
            .px_3()
            .py_2()
            .rounded_md()
            .bg(gpui::rgb(theme::bg_elevated()))
            .text_sm()
            .text_color(gpui::rgb(theme::text_secondary()))
            .child(text)
    }
}

/// Goose's runtime strings rebranded for Maple users, who never see goose.
fn maple_display_text(text: &str) -> std::borrow::Cow<'_, str> {
    std::borrow::Cow::Borrowed(compaction_notice_text(text).unwrap_or(text))
}

fn tool_status_style(status: Option<&str>) -> (&'static str, u32) {
    match status {
        Some("completed") => ("completed", theme::status_success()),
        Some("failed") | Some("error") => ("failed", theme::status_error()),
        Some("cancelled") | Some("controlled_externally") => ("stopped", theme::text_muted()),
        _ => ("running", theme::status_running()),
    }
}

/// True when the tool call input has the given top-level key.
fn has_tool_input(item: &AgentTimelineItem, key: &str) -> bool {
    item.input
        .as_ref()
        .and_then(|value| value.as_object())
        .is_some_and(|map| map.contains_key(key))
}

/// Checklist card for todo_write tool calls.
fn render_todo(item: &AgentTimelineItem) -> Div {
    let mut card = div()
        .flex()
        .flex_col()
        .gap_1()
        .px_3()
        .py_2()
        .rounded_md()
        .bg(gpui::rgb(theme::bg_tool_card()))
        .border_1()
        .border_color(gpui::rgb(theme::border_subtle()))
        .child(
            div()
                .text_sm()
                .font_weight(gpui::FontWeight::SEMIBOLD)
                .text_color(gpui::rgb(theme::text_primary()))
                .child("Plan"),
        );
    if let Some(serde_json::Value::Object(map)) = item.input.as_ref()
        && let Some(serde_json::Value::Array(todos)) = map.get("todos")
    {
        for todo in todos {
            let content = todo
                .get("content")
                .and_then(|value| value.as_str())
                .unwrap_or("");
            let status = todo
                .get("status")
                .and_then(|value| value.as_str())
                .unwrap_or("pending");
            let (marker, color) = match status {
                "completed" => ("[x]", theme::status_success()),
                "in_progress" => ("[~]", theme::status_running()),
                _ => ("[ ]", theme::text_muted()),
            };
            card = card.child(
                div()
                    .flex()
                    .gap_2()
                    .child(
                        div()
                            .text_xs()
                            .font_family("monospace")
                            .text_color(gpui::rgb(color))
                            .child(marker.to_string()),
                    )
                    .child(
                        div()
                            .text_sm()
                            .text_color(gpui::rgb(if status == "completed" {
                                theme::text_muted()
                            } else {
                                theme::text_primary()
                            }))
                            .line_clamp(1)
                            .child(content.to_string()),
                    ),
            );
        }
    }
    card
}

/// +/- lines of an edit or write tool input, stopping at the display cap.
fn diff_lines_for(item: &AgentTimelineItem) -> Vec<(char, SharedString)> {
    let mut lines: Vec<(char, SharedString)> = Vec::new();
    let Some(serde_json::Value::Object(map)) = item.input.as_ref() else {
        return lines;
    };
    let push = |lines: &mut Vec<(char, SharedString)>, sign: char, text: &str| {
        for line in text.lines() {
            if lines.len() >= MAX_DIFF_LINES {
                return false;
            }
            lines.push((sign, SharedString::from(line.to_string())));
        }
        true
    };
    if let Some(path) = map.get("path").and_then(|v| v.as_str()) {
        push(&mut lines, ' ', path);
    }
    if let Some(serde_json::Value::Array(edits)) = map.get("edits") {
        for edit in edits {
            if let Some(old) = edit.get("oldText").and_then(|v| v.as_str())
                && !push(&mut lines, '-', old)
            {
                return lines;
            }
            if let Some(new) = edit.get("newText").and_then(|v| v.as_str())
                && !push(&mut lines, '+', new)
            {
                return lines;
            }
        }
    }
    if let Some(content) = map.get("content").and_then(|v| v.as_str()) {
        push(&mut lines, '+', content);
    }
    lines
}

/// Tool card whose payload renders as a colored diff when it carries
/// edit/write replacements.
fn render_tool_with_diff(
    item: &AgentTimelineItem,
    revision: u64,
    details: bool,
    ix: usize,
    transcript: &TranscriptCtx,
) -> Div {
    let card = render_tool(item, revision, details, ix, transcript);
    if !details {
        return card;
    }
    let diff_lines = Rc::clone(&transcript.derived.get(item, revision).diff_lines);
    if diff_lines.is_empty() {
        return card;
    }
    let mut diff = div()
        .flex()
        .flex_col()
        .mt_1()
        .rounded_md()
        .bg(gpui::rgb(theme::bg_code_block()))
        .border_1()
        .border_color(gpui::rgb(theme::border_subtle()))
        .overflow_x_hidden();
    for (sign, line) in diff_lines.iter() {
        let color = match sign {
            '+' => theme::status_success(),
            '-' => theme::status_error(),
            _ => theme::text_secondary(),
        };
        diff = diff.child(
            div()
                .flex()
                .gap_1()
                .px_2()
                .text_xs()
                .font_family("monospace")
                .child(
                    div()
                        .w(gpui::px(10.))
                        .text_color(gpui::rgb(color))
                        .child(sign.to_string()),
                )
                .child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .text_color(gpui::rgb(color))
                        .line_clamp(1)
                        .child(line.clone()),
                ),
        );
    }
    // The diff is content: swallow clicks so selecting it does not toggle.
    card.child(
        div()
            .id(gpui::SharedString::from(format!("tool-diff-{}", item.id)))
            .on_click(
                |_event: &gpui::ClickEvent, _window: &mut Window, cx: &mut gpui::App| {
                    cx.stop_propagation();
                },
            )
            .child(diff),
    )
}

fn render_tool(
    item: &AgentTimelineItem,
    revision: u64,
    details: bool,
    ix: usize,
    transcript: &TranscriptCtx,
) -> Div {
    let (label, status_color) = tool_status_style(item.status.as_deref());
    let item_id = item.id.clone();
    let chat_header = transcript.chat.clone();
    let summary = transcript.tool_summaries.get(&item.id).cloned();
    let has_summary = summary.is_some();
    // The model summary stands in for the raw `tool: args` title.
    let title = summary.clone().unwrap_or_else(|| {
        SharedString::from(item.title.clone().unwrap_or_else(|| item.item_type.clone()))
    });
    let summary_requested = transcript.summary_requests.contains(&item.id);
    let derived = transcript.derived.get(item, revision);
    let card = div()
        .id(gpui::SharedString::from(format!("tool-toggle-{item_id}")))
        .flex()
        .flex_col()
        .gap_1()
        .px_3()
        .py_2()
        .rounded_md()
        .bg(gpui::rgb(theme::bg_tool_card()))
        .border_1()
        .border_color(gpui::rgb(theme::border_subtle()))
        .hover(|style| style.cursor_pointer())
        .on_click(move |_event, _window, cx: &mut gpui::App| {
            chat_header
                .update(cx, |chat, cx| {
                    chat.toggle_tool(&item_id, ix, cx);
                })
                .ok();
        })
        .child(
            div()
                .flex()
                .gap_2()
                .items_center()
                .child(
                    div()
                        .text_sm()
                        .font_weight(gpui::FontWeight::SEMIBOLD)
                        .text_color(gpui::rgb(theme::text_primary()))
                        .line_clamp(1)
                        .child(title),
                )
                .child(
                    div()
                        .text_xs()
                        .text_color(gpui::rgb(status_color))
                        .child(label),
                )
                .child(div().flex_1())
                .child(icon(
                    if details {
                        "chevron-down"
                    } else {
                        "chevron-right"
                    },
                    px(14.),
                    theme::text_muted(),
                )),
        );
    // A click anywhere on the card, payload included, toggles it.
    let mut payload = div().flex().flex_col().gap_1();
    if !details {
        // Compact: the model summary when present, otherwise a one-line
        // raw output preview.
        if !has_summary && summary_requested {
            // A summary is on the way; do not flash the raw call first.
            payload = payload.child(
                div()
                    .text_xs()
                    .text_color(gpui::rgb(theme::text_muted()))
                    .child("Summarizing…"),
            );
        } else if !has_summary && let Some(preview) = &derived.preview {
            payload = payload.child(
                div()
                    .text_xs()
                    .text_color(gpui::rgb(theme::text_muted()))
                    .line_clamp(1)
                    .child(preview.clone()),
            );
        }
        return div().child(card.child(payload));
    }
    // Expanded: the call arguments and, until a summary exists, the raw
    // output.
    if let Some(input) = &derived.input_line {
        payload = payload.child(
            div()
                .text_xs()
                .text_color(gpui::rgb(theme::text_muted()))
                .font_family("monospace")
                .overflow_x_hidden()
                .child(input.clone()),
        );
    }
    if !has_summary && let Some(output) = &derived.output_text {
        payload = payload.child(
            div()
                .mt_1()
                .w_full()
                .text_sm()
                .text_color(gpui::rgb(theme::text_secondary()))
                .child(markdown::render(&transcript.markdown_cache.get(
                    &item.id,
                    MarkdownKind::ToolOutput,
                    revision,
                    output,
                ))),
        );
    }
    div().child(card.child(payload))
}

/// Readable form of the call arguments: one `key: value` line per
/// field, strings shown as-is, nested values as pretty JSON.
fn tool_input_line(item: &AgentTimelineItem) -> Option<String> {
    let value = item.input.as_ref().filter(|value| !value.is_null())?;
    Some(format_tool_input(value))
}

fn format_tool_input(value: &serde_json::Value) -> String {
    use serde_json::Value;
    let scalar = |value: &Value| match value {
        Value::String(text) => text.clone(),
        Value::Null => "null".to_string(),
        Value::Bool(_) | Value::Number(_) => value.to_string(),
        Value::Array(_) | Value::Object(_) => {
            serde_json::to_string_pretty(value).unwrap_or_else(|_| value.to_string())
        }
    };
    match value {
        Value::Object(map) if !map.is_empty() => map
            .iter()
            .map(|(key, value)| {
                let text = scalar(value);
                if text.contains('\n') {
                    format!("{key}:\n{text}")
                } else {
                    format!("{key}: {text}")
                }
            })
            .collect::<Vec<_>>()
            .join("\n"),
        _ => scalar(value),
    }
}

/// Extract readable text from a tool output for markdown rendering.
fn tool_output_markdown(item: &AgentTimelineItem) -> Option<String> {
    let value = item.output.as_ref().filter(|value| !value.is_null())?;
    let text = extract_output_text(value)?;
    let trimmed = text.trim();
    (!trimmed.is_empty()).then(|| trimmed.to_string())
}

fn extract_output_text(value: &serde_json::Value) -> Option<String> {
    match value {
        serde_json::Value::String(text) => Some(text.clone()),
        serde_json::Value::Object(map) => {
            // Common tool result shapes: {"text": ...}, {"stdout": ...},
            // {"content": [{"type": "text", "text": ...}, ...]}.
            for key in ["text", "stdout", "stderr", "output"] {
                if let Some(inner) = map.get(key)
                    && let Some(text) = extract_output_text(inner)
                {
                    return Some(text);
                }
            }
            if let Some(serde_json::Value::Array(items)) = map.get("content") {
                let mut joined = String::new();
                for entry in items {
                    if let Some(text) = entry.get("text").and_then(|t| t.as_str()) {
                        joined.push_str(text);
                        joined.push('\n');
                    }
                }
                if !joined.is_empty() {
                    return Some(joined);
                }
            }
            None
        }
        _ => None,
    }
}

fn render_error(item: &AgentTimelineItem) -> Div {
    let text = item
        .text
        .clone()
        .or_else(|| item.title.clone())
        .unwrap_or_default();
    if text.trim().is_empty() {
        return div();
    }
    div()
        .px_3()
        .py_2()
        .rounded_md()
        .bg(gpui::rgb(theme::status_error()))
        .text_color(gpui::rgb(theme::bg_app()))
        .text_sm()
        .child(text)
}

fn render_permission_row(item: &AgentTimelineItem) -> Div {
    let title = item
        .title
        .clone()
        .unwrap_or_else(|| "Permission".to_string());
    let status = match item.status.as_deref() {
        Some("completed") => ("allowed", theme::status_success()),
        Some("denied") | Some("cancelled") => ("denied", theme::text_muted()),
        _ => ("waiting", theme::status_warning()),
    };
    div()
        .flex()
        .gap_2()
        .items_center()
        .px_3()
        .py_2()
        .rounded_md()
        .bg(gpui::rgb(theme::permission_fill()))
        .border_1()
        .border_color(gpui::rgb(theme::permission_border()))
        .child(
            div()
                .text_sm()
                .text_color(gpui::rgb(theme::text_primary()))
                .child(title),
        )
        .child(
            div()
                .text_xs()
                .text_color(gpui::rgb(status.1))
                .child(status.0),
        )
}

fn render_system(item: &AgentTimelineItem) -> Div {
    let text = item
        .text
        .clone()
        .or_else(|| item.title.clone())
        .unwrap_or_default();
    if text.trim().is_empty() {
        return div();
    }
    div()
        .text_sm()
        .text_color(gpui::rgb(theme::text_muted()))
        .child(text)
}

fn render_question_card(
    question: &crate::backend::PendingQuestion,
    step: usize,
    input: Option<Entity<TextInput>>,
    selected: &HashMap<usize, usize>,
    cx: &mut Context<ChatScreen>,
) -> Div {
    let mut card = div()
        .m_4()
        .px_4()
        .py_3()
        .rounded_lg()
        .bg(gpui::rgb(theme::bg_elevated()))
        .border_1()
        .border_color(gpui::rgb(theme::status_running()))
        .flex()
        .flex_col()
        .gap_2()
        .child(
            div()
                .font_weight(gpui::FontWeight::SEMIBOLD)
                .text_color(gpui::rgb(theme::text_primary()))
                .child("Question from Maple"),
        );
    let step = step.min(question.questions.len().saturating_sub(1));
    let has_more = step + 1 < question.questions.len();
    for (question_index, entry) in question
        .questions
        .iter()
        .enumerate()
        .filter(|(i, _)| *i == step)
    {
        let mut block = div().flex().flex_col().gap_1();
        if question.questions.len() > 1 {
            block = block.child(
                div()
                    .text_xs()
                    .text_color(gpui::rgb(theme::text_muted()))
                    .child(format!(
                        "Question {} of {}",
                        step + 1,
                        question.questions.len()
                    )),
            );
        }
        block = block.child(
            div()
                .text_xs()
                .font_weight(gpui::FontWeight::MEDIUM)
                .text_color(gpui::rgb(theme::text_secondary()))
                .child(entry.header.clone()),
        );
        block = block.child(
            div()
                .text_sm()
                .text_color(gpui::rgb(theme::text_secondary()))
                .child(entry.question.clone()),
        );
        for (option_index, option) in entry.options.iter().enumerate() {
            let is_picked = selected.get(&question_index) == Some(&option_index);
            let marker = div()
                .size_3()
                .rounded_full()
                .border_1()
                .border_color(gpui::rgb(if is_picked {
                    theme::accent()
                } else {
                    theme::border()
                }))
                .when(is_picked, |dot| dot.bg(gpui::rgb(theme::accent())));
            // flex_1 is load-bearing: without it the row squeezes this
            // block to a character wide and the label wraps vertically.
            let label_element = if option.description.is_empty() {
                div()
                    .flex_1()
                    .min_w_0()
                    .text_sm()
                    .text_color(gpui::rgb(theme::text_primary()))
                    .child(option.label.clone())
            } else {
                div()
                    .flex_1()
                    .min_w_0()
                    .flex()
                    .flex_col()
                    .child(
                        div()
                            .text_sm()
                            .text_color(gpui::rgb(theme::text_primary()))
                            .child(option.label.clone()),
                    )
                    .child(
                        div()
                            .text_xs()
                            .text_color(gpui::rgb(theme::text_muted()))
                            .child(option.description.clone()),
                    )
            };
            block = block.child(
                div()
                    .id(gpui::SharedString::from(format!(
                        "question-option-{question_index}-{option_index}"
                    )))
                    .flex()
                    .items_center()
                    .gap_2()
                    .px_2()
                    .py_1p5()
                    .rounded_md()
                    .hover(|style| {
                        style
                            .bg(gpui::rgb(theme::bg_sidebar_row_hover()))
                            .cursor_pointer()
                    })
                    .on_click({
                        cx.listener(move |this, _event, _window, cx| {
                            this.select_question_option(question_index, option_index, cx);
                        })
                    })
                    .child(marker)
                    .child(label_element),
            );
        }
        card = card.child(block);
    }
    // "Other (type your own)": one shared free-form answer per card; it
    // stands in for any question left without a picked option.
    if let Some(input) = input {
        card = card.child(
            div()
                .flex()
                .items_center()
                .gap_2()
                .child(
                    div()
                        .flex_1()
                        .px_3()
                        .py_2()
                        .rounded_md()
                        .bg(gpui::rgb(theme::bg_input()))
                        .border_1()
                        .border_color(gpui::rgb(theme::border()))
                        // The input inherits ambient color; without this the
                        // typed answer renders near-black on the dark field.
                        .text_color(gpui::rgb(theme::text_primary()))
                        .child(input),
                )
                .child(
                    div()
                        .id("question-submit")
                        .px_4()
                        .py_2()
                        .rounded_md()
                        .bg(gpui::rgb(theme::accent()))
                        .text_sm()
                        .text_color(gpui::rgb(theme::text_primary()))
                        .hover(|style| style.cursor_pointer())
                        .on_click(cx.listener(|this, _event, _window, cx| {
                            this.submit_question(cx);
                        }))
                        .child(if has_more { "Next" } else { "Answer" }),
                ),
        );
    }
    card.child(
        div()
            .id("question-skip")
            .flex()
            .items_center()
            .gap_1p5()
            .px_2()
            .py_1()
            .rounded_md()
            .text_xs()
            .text_color(gpui::rgb(theme::text_muted()))
            .hover(|style| {
                style
                    .text_color(gpui::rgb(theme::text_secondary()))
                    .cursor_pointer()
            })
            .on_click(cx.listener(|this, _event, _window, cx| {
                this.skip_question(cx);
            }))
            .child("Skip (Esc)"),
    )
}

/// Pulsing dots shown between send and the first streamed content.
fn render_waiting_indicator() -> Div {
    let dots: [gpui::Pixels; 3] = [px(7.), px(7.), px(7.)];
    let mut row = div().flex().items_center().gap_1p5().px_4().py_2();
    for (index, size) in dots.into_iter().enumerate() {
        let duration = match index {
            0 => std::time::Duration::from_millis(900),
            1 => std::time::Duration::from_millis(1200),
            _ => std::time::Duration::from_millis(1500),
        };
        let dot = div()
            .size(size)
            .rounded_full()
            .bg(gpui::rgb(theme::text_secondary()))
            .with_animation(
                gpui::ElementId::Name(format!("waiting-dot-{index}").into()),
                gpui::Animation::new(duration).repeat(),
                |el, delta| {
                    let wave = (delta * std::f32::consts::PI).sin();
                    el.opacity(0.2 + 0.7 * wave)
                },
            );
        row = row.child(dot);
    }
    row.child(
        div()
            .text_sm()
            .text_color(gpui::rgb(theme::text_muted()))
            .child("Maple is thinking"),
    )
}

fn render_permission_card(
    permission: &PendingPermission,
    arguments: &SharedString,
    responding: bool,
    cx: &mut Context<ChatScreen>,
) -> Div {
    let description: SharedString = match permission.prompt.as_deref() {
        Some(prompt) => prompt.to_string().into(),
        None => format!("Run tool {}?", permission.tool_name).into(),
    };
    let arguments = arguments.clone();
    let mut card = div()
        .m_4()
        .px_4()
        .py_3()
        .rounded_lg()
        .bg(gpui::rgb(theme::permission_fill()))
        .border_1()
        .border_color(gpui::rgb(theme::permission_border()))
        .flex()
        .flex_col()
        .gap_2()
        .child(
            div()
                .font_weight(gpui::FontWeight::SEMIBOLD)
                .text_color(gpui::rgb(theme::text_primary()))
                .child("Permission required"),
        )
        .child(
            div()
                .text_sm()
                .text_color(gpui::rgb(theme::text_secondary()))
                .child(description),
        );
    if !arguments.is_empty() {
        card = card.child(
            div()
                .text_xs()
                .text_color(gpui::rgb(theme::text_muted()))
                .font_family("monospace")
                .max_h(gpui::px(120.))
                .overflow_hidden()
                .child(arguments),
        );
    }
    let mut buttons = div().flex().gap_2();
    for (label, allow, color) in [
        ("Allow once", true, theme::status_success()),
        ("Deny", false, theme::status_error()),
    ] {
        buttons = buttons.child(
            div()
                .id(gpui::SharedString::from(format!(
                    "permission-{}",
                    label.to_lowercase().replace(' ', "-")
                )))
                .px_4()
                .py_1()
                .rounded_md()
                .bg(gpui::rgb(if responding { theme::border() } else { color }))
                .text_sm()
                .text_color(gpui::rgb(theme::bg_app()))
                .when(!responding, |el| {
                    el.hover(|style| style.cursor_pointer())
                        .on_click(cx.listener(move |this, _event, _window, cx| {
                            this.respond_permission(allow, cx);
                        }))
                })
                .child(label.to_string()),
        );
    }
    if responding {
        card = card.child(
            div()
                .text_xs()
                .text_color(gpui::rgb(theme::text_muted()))
                .child("Sending decision…"),
        );
    }
    card.child(buttons)
}

#[cfg(test)]
fn relative_time_at(when_ms: i64, now_ms: i64) -> String {
    // Session timestamps are epoch milliseconds; compare in seconds.
    let seconds = ((now_ms - when_ms).max(0)) / 1000;
    match seconds {
        seconds if seconds < 60 => "just now".to_string(),
        seconds if seconds < 3600 => format!("{}m ago", seconds / 60),
        seconds if seconds < 86_400 => format!("{}h ago", seconds / 3600),
        seconds => format!("{}d ago", seconds / 86_400),
    }
}

#[cfg(test)]
mod tests {
    use super::relative_time_at;

    #[test]
    fn relative_time_interprets_epoch_milliseconds() {
        let now_ms = 1_787_713_997_000_i64;
        assert_eq!(relative_time_at(now_ms - 30_000, now_ms), "just now");
        assert_eq!(relative_time_at(now_ms - 14 * 60 * 1000, now_ms), "14m ago");
        assert_eq!(
            relative_time_at(now_ms - 2 * 3_600 * 1000, now_ms),
            "2h ago"
        );
        assert_eq!(
            relative_time_at(now_ms - 3 * 86_400 * 1000, now_ms),
            "3d ago"
        );
    }
}

#[allow(dead_code)]
fn _unused_any_element(assertion: AnyElement) -> AnyElement {
    assertion
}

#[cfg(test)]
mod state_tests {
    use super::*;
    use gpui::TestAppContext;

    fn summary(id: &str, title: &str) -> AgentSessionSummary {
        AgentSessionSummary {
            web_enabled: true,
            archived: false,
            acp: false,
            id: id.to_string(),
            title: title.to_string(),
            project_root: "/tmp/proj".to_string(),
            created_ms: 0,
            updated_ms: 0,
            message_count: 0,
            model: None,
            mode: "smart_approve".to_string(),
        }
    }

    fn item(id: &str, item_type: &str, text: Option<&str>) -> AgentTimelineItem {
        AgentTimelineItem {
            id: id.to_string(),
            item_type: item_type.to_string(),
            role: None,
            title: None,
            text: text.map(str::to_string),
            status: None,
            input: None,
            output: None,
            created_ms: 0,
            merge: "replace".to_string(),
        }
    }

    fn user_item(id: &str, text: &str) -> AgentTimelineItem {
        AgentTimelineItem {
            role: Some("user".to_string()),
            ..item(id, "message", Some(text))
        }
    }

    /// Serializes constructions that read the settings file: the
    /// persisted-defaults test swaps XDG_CONFIG_HOME process-wide, so no
    /// other test may read settings while the swap is live.
    static SETTINGS_LOCK: parking_lot::Mutex<()> = parking_lot::Mutex::new(());

    fn screen(cx: &mut TestAppContext) -> Entity<ChatScreen> {
        let _guard = SETTINGS_LOCK.lock();
        let backend = std::sync::Arc::new(
            crate::backend::AgentBackend::new("http://127.0.0.1:9".to_string(), String::new())
                .expect("backend"),
        );
        cx.new(|_cx| {
            let mut screen = ChatScreen::new_inner(backend, "user".to_string());
            screen.selected_session = Some("s1".to_string());
            screen
        })
    }

    /// A persisted settings file must shape a freshly built chat screen:
    /// tool cards collapsed and web off for new tasks without visiting
    /// the settings screen first.
    #[gpui::test]
    fn test_constructor_reads_persisted_defaults(cx: &mut TestAppContext) {
        let _guard = SETTINGS_LOCK.lock();
        let dir = std::env::temp_dir().join(format!("maple-gpui-test-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let config = dir.join("maple-gpui");
        std::fs::create_dir_all(&config).unwrap();
        std::fs::write(
            config.join("settings.json"),
            r#"{"tool_details":false,"default_web_enabled":false}"#,
        )
        .unwrap();
        let previous = std::env::var_os("XDG_CONFIG_HOME");
        unsafe { std::env::set_var("XDG_CONFIG_HOME", &dir) };
        let screen = cx.new(|_cx| {
            ChatScreen::new_inner(
                std::sync::Arc::new(
                    crate::backend::AgentBackend::new(
                        "http://127.0.0.1:9".to_string(),
                        String::new(),
                    )
                    .expect("backend"),
                ),
                "user".to_string(),
            )
        });
        match previous {
            Some(value) => unsafe { std::env::set_var("XDG_CONFIG_HOME", value) },
            None => unsafe { std::env::remove_var("XDG_CONFIG_HOME") },
        }
        let _ = std::fs::remove_dir_all(&dir);
        screen.update(cx, |this, _cx| {
            assert!(!this.tool_details);
            assert!(!this.default_web_enabled);
        });
    }

    #[gpui::test]
    fn test_archived_tasks_leave_project_groups(cx: &mut TestAppContext) {
        let screen = screen(cx);
        screen.update(cx, |this, _cx| {
            this.sessions = vec![summary("s1", "Live"), summary("s2", "Old")];
            this.sessions[1].archived = true;
            this.rebuild_project_groups();
            assert_eq!(this.project_groups.len(), 1);
            assert_eq!(this.project_groups[0].1, vec![0]);
            assert_eq!(this.archived_indices, vec![1]);

            // A root with only archived tasks does not appear as a project.
            this.sessions[0].archived = true;
            this.rebuild_project_groups();
            assert!(this.project_groups.is_empty());
            assert_eq!(this.archived_indices, vec![0, 1]);
        });
    }

    #[gpui::test]
    fn test_timeline_appends_streamed_text(cx: &mut TestAppContext) {
        let screen = screen(cx);
        screen.update(cx, |this, _cx| {
            this.apply_timeline_item("s1", item("m1", "message", Some("Hel")));
            this.apply_timeline_item(
                "s1",
                AgentTimelineItem {
                    merge: "append".to_string(),
                    ..item("m1", "message", Some("lo"))
                },
            );
            assert_eq!(this.timeline.len(), 1);
            assert_eq!(this.timeline[0].text.as_deref(), Some("Hello"));
        });
    }

    #[gpui::test]
    fn test_timeline_field_merge_keeps_prior_payloads(cx: &mut TestAppContext) {
        let screen = screen(cx);
        screen.update(cx, |this, _cx| {
            let mut tool = item("t1", "tool", None);
            tool.title = Some("edit".to_string());
            tool.input = Some(serde_json::json!({"edits": []}));
            this.apply_timeline_item("s1", tool);
            // Completion row carries only a status; fields must survive.
            this.apply_timeline_item(
                "s1",
                AgentTimelineItem {
                    status: Some("completed".to_string()),
                    ..item("t1", "tool", None)
                },
            );
            assert_eq!(this.timeline.len(), 1);
            assert_eq!(this.timeline[0].title.as_deref(), Some("edit"));
            assert!(this.timeline[0].input.is_some());
            assert_eq!(this.timeline[0].status.as_deref(), Some("completed"));
        });
    }

    #[gpui::test]
    fn test_events_from_other_sessions_are_ignored(cx: &mut TestAppContext) {
        let screen = screen(cx);
        let event = AgentServiceEvent::TimelineItem {
            session_id: "other".to_string(),
            run_id: None,
            item: item("m9", "message", Some("alien")),
        };
        screen.update(cx, |this, cx| {
            this.handle_service_event(event, cx);
            assert!(this.timeline.is_empty());
        });
    }

    #[gpui::test]
    fn test_decided_permission_row_clears_the_card(cx: &mut TestAppContext) {
        let screen = screen(cx);
        screen.update(cx, |this, cx| {
            this.selected_session = Some("s1".to_string());
            this.pending_permission = Some(PendingPermission {
                session_id: "s1".to_string(),
                run_id: "r1".to_string(),
                request_id: "req-1".to_string(),
                tool_name: "shell".to_string(),
                prompt: None,
            });
            this.permission_responding = true;
            // A row for another request must not clear the card.
            let mut other = item("permission-req-2", "permission", None);
            other.status = Some("allow_once".to_string());
            this.handle_service_event(
                AgentServiceEvent::TimelineItem {
                    session_id: "s1".to_string(),
                    run_id: None,
                    item: other,
                },
                cx,
            );
            assert!(this.pending_permission.is_some());
            // The runtime approved the request (Allow all) and replaced
            // its row with a decision.
            let mut decided = item("permission-req-1", "permission", None);
            decided.status = Some("allow_once".to_string());
            this.handle_service_event(
                AgentServiceEvent::TimelineItem {
                    session_id: "s1".to_string(),
                    run_id: None,
                    item: decided,
                },
                cx,
            );
            assert!(this.pending_permission.is_none());
            assert!(!this.permission_responding);
        });
    }

    #[gpui::test]
    fn test_session_upsert_never_duplicates(cx: &mut TestAppContext) {
        let screen = screen(cx);
        screen.update(cx, |this, _cx| {
            this.upsert_session(summary("s1", "A"));
            this.upsert_session(summary("s1", "A2"));
            this.upsert_session(summary("s2", "B"));
            assert_eq!(this.sessions.len(), 2);
            // New sessions prepend; updates happen in place.
            assert_eq!(this.sessions[0].title, "B");
            assert_eq!(this.sessions[1].title, "A2");
        });
    }

    fn one_question(id: &str, text: &str) -> AgentServiceEvent {
        AgentServiceEvent::Question {
            session_id: "s1".to_string(),
            request_id: format!("req-{id}"),
            questions: vec![maple_agent::agent::AgentQuestion {
                id: id.to_string(),
                header: "Question".to_string(),
                question: text.to_string(),
                options: Vec::new(),
            }],
        }
    }

    #[gpui::test]
    fn test_question_event_sets_pending_card(cx: &mut TestAppContext) {
        let screen = screen(cx);
        screen.update(cx, |this, cx| {
            assert!(this.pending_questions.is_empty());
            this.handle_service_event(one_question("color", "Favorite color?"), cx);
            let question = this.pending_questions.first().expect("question set");
            assert_eq!(question.questions.len(), 1);
            assert_eq!(question.questions[0].id, "color");
            assert!(this.pending_question_input.is_some());
        });
    }

    #[gpui::test]
    fn test_question_options_select_and_compose(cx: &mut TestAppContext) {
        let screen = screen(cx);
        let event = AgentServiceEvent::Question {
            session_id: "s1".to_string(),
            request_id: "q2".to_string(),
            questions: vec![maple_agent::agent::AgentQuestion {
                id: "pick".to_string(),
                header: "Pick".to_string(),
                question: "Pick one".to_string(),
                options: vec![
                    maple_agent::agent::AgentQuestionOption {
                        label: "A".to_string(),
                        description: "First".to_string(),
                    },
                    maple_agent::agent::AgentQuestionOption {
                        label: "B".to_string(),
                        description: "Second".to_string(),
                    },
                ],
            }],
        };
        screen.update(cx, |this, cx| {
            this.handle_service_event(event, cx);
            // Repicking replaces the selection (single-select).
            this.select_question_option(0, 1, cx);
            this.select_question_option(0, 0, cx);
            let answer = this.composed_question_answer(cx);
            let parsed: serde_json::Value = serde_json::from_str(&answer).unwrap();
            assert_eq!(parsed["answers"]["pick"]["answers"][0], "A");
        });
    }

    #[gpui::test]
    fn test_sidebar_filter_hides_non_matching_tasks_and_empty_projects(cx: &mut TestAppContext) {
        let screen = screen(cx);
        screen.update(cx, |this, _| {
            let mut a = summary("s1", "Fix login bug");
            a.project_root = "/work/alpha".to_string();
            let mut b = summary("s2", "Write docs");
            b.project_root = "/work/beta".to_string();
            let mut c = summary("s3", "Old login task");
            c.project_root = "/work/beta".to_string();
            c.archived = true;
            this.sessions = vec![a, b, c];
            this.rebuild_project_groups();
            assert_eq!(this.project_groups.len(), 2);
            assert_eq!(this.archived_indices, vec![2]);

            this.sidebar_filter = "login".to_string();
            this.rebuild_project_groups();
            // Only alpha has a live match; beta drops out while searching.
            assert_eq!(this.project_groups.len(), 1);
            assert_eq!(this.project_groups[0].0, "/work/alpha");
            assert_eq!(this.project_groups[0].1, vec![0]);
            // Archived rows are searched too.
            assert_eq!(this.archived_indices, vec![2]);

            // A project name matches all of its tasks.
            this.sidebar_filter = "beta".to_string();
            this.rebuild_project_groups();
            assert_eq!(this.project_groups.len(), 1);
            assert_eq!(this.project_groups[0].1, vec![1]);

            this.sidebar_filter.clear();
            this.rebuild_project_groups();
            assert_eq!(this.project_groups.len(), 2);
        });
    }

    #[gpui::test]
    fn test_prompt_history_recall(cx: &mut TestAppContext) {
        let screen = screen(cx);
        screen.update(cx, |this, _| {
            this.remember_prompt("first");
            this.remember_prompt("second");
            // Up from an empty composer walks back; Down returns to the draft.
            assert_eq!(this.recall_prompt("up", ""), Some("second".into()));
            assert_eq!(this.recall_prompt("up", "second"), Some("first".into()));
            assert_eq!(this.recall_prompt("up", "first"), None);
            assert_eq!(this.recall_prompt("down", "first"), Some("second".into()));
            assert_eq!(this.recall_prompt("down", "second"), Some(String::new()));
            // Typed text that is not a recalled prompt keeps the caret keys.
            assert_eq!(this.recall_prompt("up", "typing"), None);
            // Re-sending an old prompt moves it to the end.
            this.remember_prompt("first");
            assert_eq!(this.recall_prompt("up", ""), Some("first".into()));
        });
    }

    #[gpui::test]
    fn test_skip_question_clears_card(cx: &mut TestAppContext) {
        let screen = screen(cx);
        let event = one_question("skip", "Skip me");
        screen.update(cx, |this, cx| {
            this.handle_service_event(event, cx);
            assert!(!this.pending_questions.is_empty());
            this.skip_question(cx);
            assert!(this.pending_questions.is_empty());
            assert!(this.question_selected.is_empty());
        });
    }

    #[gpui::test]
    fn test_first_agent_item_clears_waiting_state(cx: &mut TestAppContext) {
        let screen = screen(cx);
        screen.update(cx, |this, cx| {
            this.awaiting_first_token = true;
            this.handle_run_event(
                "s1",
                "run-1",
                maple_agent::agent::AgentRunEvent::TimelineItem(user_item("u1", "hi")),
                cx,
            );
            // The user's own echo does not stop the waiting indicator.
            assert!(this.awaiting_first_token);
            this.handle_run_event(
                "s1",
                "run-1",
                maple_agent::agent::AgentRunEvent::TimelineItem(item("t1", "thinking", None)),
                cx,
            );
            assert!(!this.awaiting_first_token);
        });
    }

    #[gpui::test]
    fn test_send_closes_chip_menus(cx: &mut TestAppContext) {
        let screen = screen(cx);
        screen.update(cx, |this, cx| {
            this.selected_session = Some("s1".to_string());
            this.booting = false;
            this.models_menu_open = true;
            this.mode_menu_open = true;
            this.mcp_menu_open = true;
            this.root_menu_open = true;
            this.send_text("hello".to_string(), cx);
            assert!(!this.models_menu_open);
            assert!(!this.mode_menu_open);
            assert!(!this.mcp_menu_open);
            assert!(!this.root_menu_open);
        });
    }

    #[gpui::test]
    fn test_multi_question_batch_steps_through(cx: &mut TestAppContext) {
        let screen = screen(cx);
        let event = AgentServiceEvent::Question {
            session_id: "s1".to_string(),
            request_id: "batch".to_string(),
            questions: vec![
                maple_agent::agent::AgentQuestion {
                    id: "first".to_string(),
                    header: "One".to_string(),
                    question: "First?".to_string(),
                    options: vec![maple_agent::agent::AgentQuestionOption {
                        label: "Yes".to_string(),
                        description: String::new(),
                    }],
                },
                maple_agent::agent::AgentQuestion {
                    id: "second".to_string(),
                    header: "Two".to_string(),
                    question: "Second?".to_string(),
                    options: vec![maple_agent::agent::AgentQuestionOption {
                        label: "No".to_string(),
                        description: String::new(),
                    }],
                },
            ],
        };
        screen.update(cx, |this, cx| {
            this.handle_service_event(event, cx);
            assert_eq!(this.question_step, 0);
            // Answer step one: the batch stays on the card, advanced.
            this.select_question_option(0, 0, cx);
            this.submit_question(cx);
            assert_eq!(this.pending_questions.len(), 1);
            assert_eq!(this.question_step, 1);
            // Answer step two: the queue pops and both answers are recorded.
            this.select_question_option(1, 0, cx);
            this.submit_question(cx);
            assert!(this.pending_questions.is_empty());
            assert_eq!(this.question_step, 0);
            assert!(this.question_step_answers.is_empty());
        });
    }

    #[gpui::test]
    fn test_parallel_questions_queue_and_advance(cx: &mut TestAppContext) {
        let screen = screen(cx);
        let question = |id: &str| AgentServiceEvent::Question {
            session_id: "s1".to_string(),
            request_id: id.to_string(),
            questions: vec![maple_agent::agent::AgentQuestion {
                id: id.to_string(),
                header: "Question".to_string(),
                question: format!("Question {id}"),
                options: Vec::new(),
            }],
        };
        screen.update(cx, |this, cx| {
            this.handle_service_event(question("q1"), cx);
            this.handle_service_event(question("q2"), cx);
            this.handle_service_event(question("q3"), cx);
            // Duplicate delivery must not double-queue.
            this.handle_service_event(question("q2"), cx);
            assert_eq!(this.pending_questions.len(), 3);
            assert_eq!(this.pending_questions[0].request_id, "q1");
            // Answering pops the head and leaves the rest queued.
            this.answer_question("first".to_string(), cx);
            assert_eq!(this.pending_questions.len(), 2);
            assert_eq!(this.pending_questions[0].request_id, "q2");
            // Skipping pops the head too.
            this.skip_question(cx);
            assert_eq!(this.pending_questions.len(), 1);
            assert_eq!(this.pending_questions[0].request_id, "q3");
            // Every surfaced question must have an answer input; a card
            // without one is unanswerable.
            assert!(this.pending_question_input.is_some());
            this.answer_question("third".to_string(), cx);
            assert!(this.pending_questions.is_empty());
            assert!(this.pending_question_input.is_none());
        });
    }

    #[gpui::test]
    fn test_send_blocked_while_question_pending(cx: &mut TestAppContext) {
        let screen = screen(cx);
        screen.update(cx, |this, cx| {
            this.booting = false;
            this.pending_questions = vec![crate::backend::PendingQuestion {
                session_id: "s1".to_string(),
                request_id: "q9".to_string(),
                questions: vec![maple_agent::agent::AgentQuestion {
                    id: "paused".to_string(),
                    header: "Question".to_string(),
                    question: "Paused?".to_string(),
                    options: Vec::new(),
                }],
            }];
            this.send_text("a stray reply".to_string(), cx);
            assert_eq!(
                this.notice.as_ref().map(SharedString::as_ref),
                Some("Answer the question above first")
            );
            assert!(this.question_focus_pending);
        });
    }

    #[test]
    fn test_slash_entries_filter_and_cap() {
        let skills = vec![AgentSlashCommand {
            name: "deploy".to_string(),
            description: "Deploy".to_string(),
            input_hint: None,
        }];
        let entries = slash_entries_for("de", &skills);
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].name, "deploy");
        assert!(slash_entries_for("zzz", &skills).is_empty());
        assert_eq!(slash_entries_for("", &skills).len(), 7);
    }

    #[gpui::test]
    fn test_builtin_commands_execute(cx: &mut TestAppContext) {
        let screen = screen(cx);
        screen.update(cx, |this, cx| {
            this.booting = false;
            this.web_enabled = true;
            assert!(this.try_command("s1", "/web", cx));
            assert!(!this.web_enabled);
            assert!(this.try_command("s1", "/model", cx));
            assert!(this.models_menu_open);
            // Unknown commands fall through to a normal send.
            assert!(!this.try_command("s1", "/definitely-not-a-command", cx));
            // Paths that merely start with a slash are not commands.
            assert!(!this.try_command("s1", "/etc/hosts is a path", cx));
        });
    }

    #[gpui::test]
    fn test_skill_command_resolves_via_backend(cx: &mut TestAppContext) {
        let screen = screen(cx);
        screen.update(cx, |this, cx| {
            this.slash_commands = vec![AgentSlashCommand {
                name: "deploy".to_string(),
                description: "Deploy the app".to_string(),
                input_hint: None,
            }];
            assert!(this.try_command("s1", "/deploy staging", cx));
            assert_eq!(
                this.notice.as_ref().map(SharedString::as_ref),
                Some("Loading skill…")
            );
        });
    }

    fn long_tool(id: &str, status: &str) -> AgentTimelineItem {
        let mut tool = item(id, "tool", None);
        tool.title = Some("shell".to_string());
        tool.input = Some(serde_json::json!({"command": "ls"}));
        tool.status = Some(status.to_string());
        tool.output = Some(serde_json::json!({"stdout": "x".repeat(600)}));
        tool
    }

    #[gpui::test]
    fn test_tool_summary_gating(cx: &mut TestAppContext) {
        let screen = screen(cx);
        screen.update(cx, |this, cx| {
            this.summaries_enabled = true;
            // Running tools are not summarized.
            let index = this.apply_timeline_item("s1", long_tool("tool-1", "running"));
            this.maybe_summarize_tool(index, cx);
            assert!(this.summary_requests.is_empty());
            // Completed tools with long output are queued once.
            let index = this.apply_timeline_item("s1", long_tool("tool-1", "completed"));
            this.maybe_summarize_tool(index, cx);
            assert!(this.summary_requests.contains("tool-1"));
            assert_eq!(this.pending_summaries, 1);
            this.maybe_summarize_tool(index, cx);
            assert_eq!(this.pending_summaries, 1);
            // Short outputs never queue.
            let mut short = item("tool-2", "tool", None);
            short.status = Some("completed".to_string());
            short.input = Some(serde_json::json!({"q": 1}));
            short.output = Some(serde_json::json!({"stdout": "ok"}));
            let index = this.apply_timeline_item("s1", short);
            this.maybe_summarize_tool(index, cx);
            assert!(!this.summary_requests.contains("tool-2"));
        });
    }

    #[gpui::test]
    fn test_tool_summaries_queue_past_the_slot_cap(cx: &mut TestAppContext) {
        let screen = screen(cx);
        screen.update(cx, |this, cx| {
            this.summaries_enabled = true;
            for n in 0..5 {
                let index =
                    this.apply_timeline_item("s1", long_tool(&format!("t{n}"), "completed"));
                this.maybe_summarize_tool(index, cx);
            }
            // Three ride at once; the rest wait instead of being dropped.
            assert_eq!(this.pending_summaries, 3);
            assert_eq!(this.summary_queue.len(), 2);
            assert_eq!(this.summary_requests.len(), 5);
            // A session switch restarts the slots and drops the queue.
            let before = this.summary_generation;
            this.set_active_session(summary("s2", "B"), Vec::new(), cx);
            assert_eq!(this.summary_generation, before + 1);
            assert_eq!(this.pending_summaries, 0);
            assert!(this.summary_queue.is_empty());
            assert!(this.summary_requests.is_empty());
        });
    }

    #[gpui::test]
    fn test_finished_run_drops_its_questions(cx: &mut TestAppContext) {
        let screen = screen(cx);
        screen.update(cx, |this, cx| {
            this.active_runs
                .insert("s1".to_string(), "run-1".to_string());
            this.handle_service_event(one_question("q", "Still there?"), cx);
            this.select_question_option(0, 0, cx);
            assert!(this.current_question().is_some());
            this.handle_run_event(
                "s1",
                "run-1",
                maple_agent::agent::AgentRunEvent::Finished(
                    maple_agent::agent::AgentRunTerminal::Cancelled,
                ),
                cx,
            );
            assert!(this.pending_questions.is_empty());
            assert!(this.pending_question_input.is_none());
            assert!(this.question_selected.is_empty());
            assert_eq!(this.question_step, 0);
            // The composer is unblocked again.
            this.booting = false;
            this.send_text("next".to_string(), cx);
            assert_ne!(
                this.notice.as_ref().map(SharedString::as_ref),
                Some("Answer the question above first")
            );
        });
    }

    #[gpui::test]
    fn test_questions_are_scoped_to_their_session(cx: &mut TestAppContext) {
        let screen = screen(cx);
        let other = AgentServiceEvent::Question {
            session_id: "s2".to_string(),
            request_id: "req-other".to_string(),
            questions: vec![maple_agent::agent::AgentQuestion {
                id: "other".to_string(),
                header: "Question".to_string(),
                question: "From another task".to_string(),
                options: Vec::new(),
            }],
        };
        screen.update(cx, |this, cx| {
            this.active_runs
                .insert("s2".to_string(), "run-2".to_string());
            this.handle_service_event(other, cx);
            // Queued for later, but not shown and not blocking this task.
            assert_eq!(this.pending_questions.len(), 1);
            assert!(this.current_question().is_none());
            assert!(this.pending_question_input.is_none());
            this.booting = false;
            this.send_text("still typing here".to_string(), cx);
            assert_ne!(
                this.notice.as_ref().map(SharedString::as_ref),
                Some("Answer the question above first")
            );
            // Escape must not cancel the other session's run.
            this.skip_question(cx);
            assert_eq!(this.pending_questions.len(), 1);
            // A second question for the shown session keeps the pick made
            // on the first card.
            this.handle_service_event(one_question("a", "First?"), cx);
            this.select_question_option(0, 0, cx);
            this.handle_service_event(one_question("b", "Second?"), cx);
            assert_eq!(this.question_selected.get(&0), Some(&0));
            // Switching to the other task shows its card.
            this.set_active_session(summary("s2", "B"), Vec::new(), cx);
            assert_eq!(
                this.current_question().map(|q| q.request_id.as_str()),
                Some("req-other")
            );
            assert!(this.pending_question_input.is_some());
            assert!(this.question_selected.is_empty());
            // Switching back still shows the first task's card.
            this.set_active_session(summary("s1", "A"), Vec::new(), cx);
            assert_eq!(
                this.current_question().map(|q| q.request_id.as_str()),
                Some("req-a")
            );
        });
    }

    #[gpui::test]
    fn test_escape_closes_root_menu(cx: &mut TestAppContext) {
        let screen = screen(cx);
        screen.update(cx, |this, cx| {
            this.root_menu_open = true;
            this.close_menus_on_escape(cx);
            assert!(!this.root_menu_open);
        });
    }

    #[gpui::test]
    fn test_idle_events_do_not_redraw(cx: &mut TestAppContext) {
        let screen = screen(cx);
        screen.update(cx, |this, cx| {
            let status = maple_agent::agent::AgentRuntimeStatus {
                running: true,
                project_root: None,
                model: None,
                mode: None,
                active_runs: HashMap::new(),
            };
            assert!(
                !this.apply_service_event(AgentServiceEvent::RuntimeStatus(status.clone()), cx)
            );
            assert!(
                this.apply_service_event(AgentServiceEvent::SessionCreated(summary("s1", "A")), cx)
            );
            assert!(
                !this
                    .apply_service_event(AgentServiceEvent::SessionCreated(summary("s1", "A")), cx)
            );
            assert!(this.apply_service_event(
                AgentServiceEvent::SessionCreated(summary("s1", "A renamed")),
                cx
            ));
        });
    }

    #[gpui::test]
    fn test_timeline_index_tracks_items(cx: &mut TestAppContext) {
        let screen = screen(cx);
        screen.update(cx, |this, _cx| {
            assert_eq!(
                this.apply_timeline_item("s1", item("a", "message", Some("1"))),
                0
            );
            assert_eq!(
                this.apply_timeline_item("s1", item("b", "message", Some("2"))),
                1
            );
            assert_eq!(
                this.apply_timeline_item("s1", item("a", "message", Some("3"))),
                0
            );
            assert_eq!(this.timeline_index.get("a"), Some(&(0, 1)));
            assert_eq!(this.timeline_index.get("b"), Some(&(1, 0)));
            this.replace_timeline(vec![item("z", "message", Some("9"))]);
            assert_eq!(this.timeline_index.get("z"), Some(&(0, 0)));
            assert!(!this.timeline_index.contains_key("a"));
        });
    }

    #[gpui::test]
    fn test_attachments_requested_from_the_arriving_item(cx: &mut TestAppContext) {
        let screen = screen(cx);
        screen.update(cx, |this, cx| {
            let mut sent = user_item("u1", "see this");
            sent.input = Some(serde_json::json!({
                "imageAttachments": [{"id": "att-1", "name": "a.png"}]
            }));
            this.apply_incoming_item("s1", sent, cx);
            assert!(this.attachment_requests.contains("att-1"));
        });
    }

    #[test]
    fn test_diff_lines_stop_at_the_cap() {
        let mut tool = item("edit", "tool", None);
        tool.input = Some(serde_json::json!({
            "path": "big.txt",
            "content": (0..1000).map(|n| n.to_string()).collect::<Vec<_>>().join("\n"),
        }));
        let lines = diff_lines_for(&tool);
        assert_eq!(lines.len(), MAX_DIFF_LINES);
        assert_eq!(lines[0], (' ', SharedString::from("big.txt")));
        assert_eq!(lines[1], ('+', SharedString::from("0")));
    }

    #[test]
    fn test_markdown_cache_keys_on_revision() {
        let cache = MarkdownCache::default();
        let first = cache.get("m", MarkdownKind::Body, 0, "hello");
        assert!(Rc::ptr_eq(
            &first,
            &cache.get("m", MarkdownKind::Body, 0, "hello")
        ));
        // Same length, new revision: parsed again.
        assert!(!Rc::ptr_eq(
            &first,
            &cache.get("m", MarkdownKind::Body, 1, "jello")
        ));
        // Kinds do not share entries.
        assert!(!Rc::ptr_eq(
            &first,
            &cache.get("m", MarkdownKind::ToolOutput, 0, "hello")
        ));
    }

    #[gpui::test]
    fn test_composer_change_updates_slash_entries(cx: &mut TestAppContext) {
        let screen = screen(cx);
        let input = cx.new(|cx| TextInput::new("", cx));
        screen.update(cx, |this, cx| {
            assert!(!this.composer_has_text);
            input.update(cx, |input, cx| input.set_text("/co", cx));
            this.composer_changed(&input, cx);
            assert!(this.composer_has_text);
            assert_eq!(this.slash_entries.len(), 1);
            assert_eq!(this.slash_entries[0].name, "compact");
            input.update(cx, |input, cx| input.set_text("/compact now", cx));
            this.composer_changed(&input, cx);
            assert!(this.slash_entries.is_empty());
            input.update(cx, |input, cx| input.clear(cx));
            this.composer_changed(&input, cx);
            assert!(!this.composer_has_text);
        });
    }
    #[test]
    fn test_maple_display_text_rebrands_compaction() {
        assert_eq!(
            maple_display_text("goose is compacting the conversation..."),
            "Compacting the conversation…"
        );
        assert_eq!(
            maple_display_text("Context limit reached. Compacting to continue conversation..."),
            "Context limit reached — compacting to continue…"
        );
        assert_eq!(maple_display_text("Anything else"), "Anything else");
    }

    #[test]
    fn test_pinned_roots_sort_first() {
        let _guard = SETTINGS_LOCK.lock();
        let mut this = ChatScreen::new_inner(
            std::sync::Arc::new(
                crate::backend::AgentBackend::new("http://127.0.0.1:9".to_string(), String::new())
                    .expect("backend"),
            ),
            "user".to_string(),
        );
        this.recent_roots = vec!["/a".to_string(), "/b".to_string(), "/c".to_string()];
        this.pinned_roots = vec!["/c".to_string(), "/a".to_string()];
        this.rebuild_project_groups();
        let roots: Vec<&String> = this.project_groups.iter().map(|(root, _)| root).collect();
        assert_eq!(roots, vec!["/c", "/a", "/b"]);
    }

    #[gpui::test]
    fn test_finished_only_clears_its_own_run(cx: &mut TestAppContext) {
        let screen = screen(cx);
        screen.update(cx, |this, cx| {
            this.active_runs
                .insert("s1".to_string(), "run-old".to_string());
            this.active_runs
                .insert("s1".to_string(), "run-new".to_string());
            this.handle_run_event(
                "s1",
                "run-old",
                maple_agent::agent::AgentRunEvent::Finished(
                    maple_agent::agent::AgentRunTerminal::Completed,
                ),
                cx,
            );
            assert_eq!(
                this.active_runs.get("s1").map(String::as_str),
                Some("run-new")
            );
        });
    }

    fn png_bytes() -> Vec<u8> {
        // Only the signature matters: the draft checks the magic bytes.
        let mut bytes = vec![0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A];
        bytes.extend_from_slice(&[0; 16]);
        bytes
    }

    #[gpui::test]
    fn test_paste_image_stages_a_draft(cx: &mut TestAppContext) {
        let screen = screen(cx);
        screen.update(cx, |this, cx| {
            let image = gpui::Image::from_bytes(gpui::ImageFormat::Png, png_bytes());
            this.paste_image(image, cx);
            assert_eq!(this.draft_images.len(), 1);
            assert_eq!(this.draft_images[0].name, "pasted-1.png");
            assert!(
                this.draft_images[0]
                    .data_url
                    .starts_with("data:image/png;base64,iVBORw0KGgo")
            );
            assert!(this.notice.is_none());
        });
    }

    #[gpui::test]
    fn test_paste_image_rejects_unsupported_format(cx: &mut TestAppContext) {
        let screen = screen(cx);
        screen.update(cx, |this, cx| {
            let image = gpui::Image::from_bytes(gpui::ImageFormat::Gif, b"GIF89a".to_vec());
            this.paste_image(image, cx);
            assert!(this.draft_images.is_empty());
            assert!(
                this.notice
                    .as_deref()
                    .is_some_and(|notice| notice.contains("not a PNG, JPEG, or WebP"))
            );
        });
    }

    #[gpui::test]
    fn test_paste_image_respects_limit(cx: &mut TestAppContext) {
        let screen = screen(cx);
        screen.update(cx, |this, cx| {
            for _ in 0..=MAX_DRAFT_IMAGES {
                let image = gpui::Image::from_bytes(gpui::ImageFormat::Png, png_bytes());
                this.paste_image(image, cx);
            }
            assert_eq!(this.draft_images.len(), MAX_DRAFT_IMAGES);
            assert!(
                this.notice
                    .as_deref()
                    .is_some_and(|notice| notice.contains("at most"))
            );
        });
    }

    #[gpui::test]
    fn test_streaming_chunk_keeps_wheel_scrolling_up(cx: &mut TestAppContext) {
        /// A paragraph long enough to need real vertical space when rendered.
        const PARA: &str = "The quick brown fox jumps over the lazy dog. \
            Pack my box with five dozen liquor jugs. How vexingly quick daft zebras jump! ";

        // Hosts only the transcript so the list gets a realistic viewport,
        // independent of the rest of the screen's layout.
        struct TranscriptHost {
            chat: Entity<ChatScreen>,
        }
        impl Render for TranscriptHost {
            fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
                // Fixed size: the test window's own bounds are not applied
                // to the root view in the harness, and a percentage height
                // collapses to zero.
                div()
                    .w(px(1200.))
                    .h(px(800.))
                    .flex()
                    .flex_col()
                    .child(self.chat.update(cx, |chat, cx| chat.render_transcript(cx)))
            }
        }

        let chat = cx.new(|_| {
            let _guard = SETTINGS_LOCK.lock();
            let backend = std::sync::Arc::new(
                crate::backend::AgentBackend::new("http://127.0.0.1:9".to_string(), String::new())
                    .expect("backend"),
            );
            let mut this = ChatScreen::new_inner(backend, "user".to_string());
            this.selected_session = Some("s1".to_string());
            this
        });

        // A long history, then a streaming answer that is already tall
        // while the view is pinned to the newest item.
        chat.update(cx, |this, _cx| {
            let mut timeline = Vec::new();
            for i in 0..20 {
                timeline.push(item(
                    &format!("u{i}"),
                    "message",
                    Some(&format!("User {i} {PARA}")),
                ));
                timeline.push(item(
                    &format!("a{i}"),
                    "message",
                    Some(&format!("Reply {i} {PARA}{PARA}")),
                ));
            }
            this.replace_timeline(timeline);
            this.apply_timeline_item("s1", item("stream", "message", Some("Answer: ")));
            for _ in 0..30 {
                this.apply_timeline_item(
                    "s1",
                    AgentTimelineItem {
                        merge: "append".to_string(),
                        text: Some(PARA.repeat(3)),
                        ..item("stream", "message", None)
                    },
                );
            }
        });

        let (_host, cx) = cx.add_window_view(|_window, _cx| TranscriptHost { chat: chat.clone() });
        cx.simulate_resize(gpui::size(px(1200.), px(800.)));
        let center = cx.update(|window, _cx| window.bounds().center());

        let pinned = cx.update(|_window, app| chat.read(app).list_state.logical_scroll_top());
        assert_eq!(
            (pinned.item_ix, pinned.offset_in_item),
            (41, px(0.)),
            "the transcript must start pinned to the newest item, got {pinned:?}"
        );

        let wheel_up = |cx: &mut gpui::VisualTestContext| {
            cx.simulate_event(gpui::ScrollWheelEvent {
                position: center,
                delta: gpui::ScrollDelta::Pixels(gpui::point(px(0.), px(100.))),
                ..Default::default()
            });
        };

        // Control: an idle wheel moves the viewport up.
        wheel_up(cx);
        let moved = cx.update(|_window, app| chat.read(app).list_state.logical_scroll_top());
        assert_ne!(
            (moved.item_ix, moved.offset_in_item),
            (pinned.item_ix, pinned.offset_in_item),
            "an idle wheel must move the viewport, got {moved:?}"
        );

        // A streamed chunk lands. No repaint runs before the user's wheel
        // event, exactly like a frame that is still pending.
        cx.update(|_window, app| {
            chat.update(app, |this, _cx| {
                this.apply_timeline_item(
                    "s1",
                    AgentTimelineItem {
                        merge: "append".to_string(),
                        text: Some(PARA.to_string()),
                        ..item("stream", "message", None)
                    },
                );
            })
        });
        wheel_up(cx);

        let after = cx.update(|_window, app| chat.read(app).list_state.logical_scroll_top());
        assert_ne!(
            (after.item_ix, after.offset_in_item),
            (pinned.item_ix, pinned.offset_in_item),
            "a wheel between a streamed chunk and the next paint must move the viewport up, got {after:?}"
        );
    }

    /// Plain typing while the transcript holds focus must land in the
    /// composer, including the first character, without a click first.
    /// Chords and enter/tab keep their meaning instead of stealing focus.
    #[gpui::test]
    fn test_typing_with_transcript_focused_lands_in_composer(cx: &mut TestAppContext) {
        // Hosts the whole screen so the chat root's key listener is on the
        // dispatch path, exactly like the real window.
        struct ChatHost {
            chat: Entity<ChatScreen>,
        }
        impl Render for ChatHost {
            fn render(
                &mut self,
                _window: &mut Window,
                _cx: &mut Context<Self>,
            ) -> impl IntoElement {
                div().w(px(1200.)).h(px(800.)).child(self.chat.clone())
            }
        }

        let chat = cx.new(|cx| {
            let _guard = SETTINGS_LOCK.lock();
            let backend = std::sync::Arc::new(
                crate::backend::AgentBackend::new("http://127.0.0.1:9".to_string(), String::new())
                    .expect("backend"),
            );
            ChatScreen::new(backend, "user".to_string(), cx)
        });
        chat.update(cx, |this, _cx| {
            this.selected_session = Some("s1".to_string());
            this.replace_timeline(vec![user_item("u1", "hello")]);
        });

        let (_host, cx) = cx.add_window_view(|_window, _cx| ChatHost { chat: chat.clone() });
        cx.simulate_resize(gpui::size(px(1200.), px(800.)));

        let transcript_focus =
            cx.update(|_window, app| chat.read(app).transcript_focus.clone().unwrap());
        let composer_handle =
            cx.update(|_window, app| chat.read(app).composer.clone().unwrap().focus_handle(app));

        // Focus the transcript the way a text-selection press does.
        cx.update(|window, _| window.focus(&transcript_focus));

        // Typing routes straight into the composer, first character
        // included; the second arrives through the normal input path.
        cx.simulate_input("hi");
        cx.update(|_window, app| {
            chat.update(app, |this, cx| {
                assert_eq!(this.composer.as_ref().unwrap().read(cx).text(), "hi");
            })
        });
        assert_eq!(
            cx.update(|window, app| window.focused(app)),
            Some(composer_handle.clone()),
            "typing must move focus to the composer"
        );

        // A modifier chord keeps focus where it is.
        cx.update(|window, _| window.focus(&transcript_focus));
        cx.simulate_keystrokes("alt-h");
        cx.update(|_window, app| {
            chat.update(app, |this, cx| {
                assert_eq!(this.composer.as_ref().unwrap().read(cx).text(), "hi");
            })
        });
        assert_eq!(
            cx.update(|window, app| window.focused(app)),
            Some(transcript_focus.clone()),
            "a chord must not steal focus from the transcript"
        );

        // Enter and tab never route to the composer, even where the
        // platform reports a key_char for them.
        cx.simulate_keystrokes("enter tab");
        cx.update(|_window, app| {
            chat.update(app, |this, cx| {
                assert_eq!(this.composer.as_ref().unwrap().read(cx).text(), "hi");
            })
        });
        assert_eq!(
            cx.update(|window, app| window.focused(app)),
            Some(transcript_focus),
            "enter and tab must not steal focus from the transcript"
        );
    }
}
