//! The agent chat surface: session sidebar, streaming transcript with tool
//! calls, permission prompts, and the composer. Pure consumer of the backend
//! facade + event stream.

use std::cell::RefCell;
use std::collections::{HashMap, HashSet};
use std::rc::Rc;
use std::sync::Arc;

use gpui::{
    AnimationExt, AnyElement, AppContext, Div, Entity, EventEmitter, Focusable, Render,
    SharedString, Window, div, prelude::*, px,
};
use maple_agent::agent::{
    AgentImageUpload, AgentSendMessageRequest, AgentServiceEvent, AgentSessionMcpServer,
    AgentSessionSummary, AgentSlashCommand, AgentTimelineItem,
};

use crate::backend::{AgentBackend, PendingPermission, PendingQuestion};
use crate::ui::icons::{icon, wordmark};
use crate::ui::markdown;
use crate::ui::rich_text::{self, RenderCtx};
use crate::ui::settings::{OpenSettingsSection, Section};
use crate::ui::text_input::TextInput;
use crate::ui::theme;

gpui::actions!(chat, [ChatEscape, CopySelection]);

pub struct LoggedOut;

/// Emitted when the user opens app settings from the chat header.
pub struct OpenSettings;

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
    tool_summaries: &'a HashMap<String, String>,
    summary_requests: &'a HashSet<String>,
    render: &'a RenderCtx,
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
    /// Slash commands from the installed skills of the current project root.
    slash_commands: Vec<AgentSlashCommand>,
    /// Highlighted row in the open slash palette, if any.
    slash_selected: Option<usize>,
    /// Set when a pending question should steal focus at the next render.
    question_focus_pending: bool,
    /// One-line model summaries per completed tool item id.
    tool_summaries: HashMap<String, String>,
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
        this.start(cx);
        this
    }

    /// Create and wire the composer; called by the real constructor.
    fn attach_composer(&mut self, weak: gpui::WeakEntity<Self>, cx: &mut Context<Self>) {
        let composer = cx.new(|cx| {
            TextInput::new("Ask Maple to work in this folder...", cx)
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
                        let plain = !event.keystroke.modifiers.control
                            && !event.keystroke.modifiers.alt
                            && !event.keystroke.modifiers.platform
                            && !event.keystroke.modifiers.shift;
                        if !plain {
                            return false;
                        }
                        let key = event.keystroke.key.clone();
                        let token = text.strip_prefix('/').unwrap_or_default();
                        let token_ok = text.starts_with('/')
                            && !token.contains(char::is_whitespace)
                            && !token.contains('/');
                        match key.as_str() {
                            "down" | "up" if token_ok => this.update(cx, |chat, cx| {
                                chat.navigate_slash_palette(&key, token, cx)
                            }),
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
            transcript_focus: None,
            awaiting_first_token: false,
            toggled_tools: HashSet::new(),
            question_selected: HashMap::new(),
            question_step: 0,
            question_step_answers: HashMap::new(),
            lightbox: None,
            slash_commands: Vec::new(),
            slash_selected: None,
            question_focus_pending: false,
            tool_summaries: HashMap::new(),
            summary_requests: HashSet::new(),
            pending_summaries: 0,
            summary_queue: std::collections::VecDeque::new(),
            summary_generation: 0,
            summaries_enabled: settings.tool_summaries,
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
            },
        );
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
            async move { backend.load_session(&user_id, &target).await },
            cx,
            move |this, result, cx| {
                // A newer selection (or reload) superseded this load.
                if this.selection_generation != generation {
                    return;
                }
                let detail = match result {
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
                match mode {
                    LoadMode::Select => {
                        this.upsert_session(detail.session.clone());
                        this.set_active_session(detail.session, detail.timeline, cx);
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

    fn is_run_active(&self) -> bool {
        self.selected_session
            .as_ref()
            .is_some_and(|session| self.active_runs.contains_key(session))
    }

    /// Escape, in priority order: close the photo viewer, skip a pending
    /// question, clear a text selection, close the chip menus.
    fn chat_escape(&mut self, _: &ChatEscape, _window: &mut Window, cx: &mut Context<Self>) {
        if self.lightbox.is_some() {
            self.lightbox = None;
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
        let Some(selection) = self.selection.clone() else {
            return;
        };
        let text = selection.read(cx).selected_text();
        if !text.is_empty() {
            cx.write_to_clipboard(gpui::ClipboardItem::new_string(text));
        }
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
        self.send_to_session(&session_id, text, cx);
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
                            this.send_to_session(&session_id, prompt, cx);
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

    fn send_to_session(&mut self, session_id: &str, text: String, cx: &mut Context<Self>) {
        let session_id = session_id.to_string();
        let backend = self.backend.clone();
        let user_id = self.user_id.clone();
        let model = self.selected_model.clone();
        let vision_capable = self.selected_model_supports_vision();
        let drafts = std::mem::take(&mut self.draft_images);
        let request = AgentSendMessageRequest {
            session_id: session_id.clone(),
            text: text.clone(),
            model,
            context_limit: None,
            mode: Some(self.permission_mode.clone()),
            vision_capable,
            steer: false,
            queue_id: None,
            attachments: drafts
                .iter()
                .map(|image| AgentImageUpload {
                    name: image.name.clone(),
                    data_url: image.data_url.clone(),
                })
                .collect(),
        };
        self.notice = None;
        // Clear the composer at dispatch so no entry path can leave the
        // sent text behind; a failed send restores it below.
        if let Some(composer) = self.composer.clone() {
            composer.update(cx, |input, cx| input.clear(cx));
        }
        // Sending closes the chip menus anchored under the composer.
        self.models_menu_open = false;
        self.mode_menu_open = false;
        self.mcp_menu_open = false;
        self.root_menu_open = false;
        self.follow_transcript = true;
        self.awaiting_first_token = true;
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
                    if this.selected_session.as_deref() == Some(session_id.as_str()) {
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
        self.call(
            async move {
                backend
                    .summarize_tool_call(&user_id, &session_id, tool_name, Some(input), output)
                    .await
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
                    this.tool_summaries.insert(item_id, summary);
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
                let existing = &mut self.timeline[index];
                // The virtualized list caches item heights; tell it this
                // one changed so it re-measures.
                self.list_state.splice(index..index + 1, 1);
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
                        arguments,
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
            AgentRunEvent::QueueChanged(_) | AgentRunEvent::QueuePromoted { .. } => {
                // Queue chips are rendered from send responses; nothing to do
                // until queue editing is exposed in the UI.
                return false;
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
                .child(self.render_header(cx))
                .child(self.render_transcript(window, cx))
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
            .flex_1()
            .min_h_0()
            .flex()
            .flex_col()
            .bg(gpui::rgb(theme::BG_APP))
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
                                        .child(wordmark(px(14.), theme::TEXT_PRIMARY)),
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
                                .border_color(gpui::rgb(theme::BORDER)),
                        ),
                )
            })
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
                    .bg(gpui::rgb(theme::BG_SIDEBAR_ROW_HOVER))
                    .cursor_pointer()
            })
            .on_click(cx.listener(|this, _event, _window, cx| {
                this.sidebar_collapsed = !this.sidebar_collapsed;
                cx.notify();
            }))
            .child(icon("panel-left", px(16.), theme::TEXT_SECONDARY))
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
                                .text_color(gpui::rgb(theme::DISPLAY_TEXT))
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
                                .text_color(gpui::rgb(theme::TEXT_MUTED))
                                .child(icon("lock", px(12.), theme::TEXT_MUTED))
                                .child("Encrypted and private at every step"),
                        )
                    })
                    .when_some(self.runtime_error.clone(), |column, error| {
                        column.child(
                            div()
                                .px_3()
                                .py_2()
                                .rounded_md()
                                .bg(gpui::rgb(theme::STATUS_ERROR))
                                .text_color(gpui::rgb(theme::BG_APP))
                                .text_sm()
                                .child(error),
                        )
                    })
                    .when_some(self.notice.clone(), |column, notice| {
                        column.child(
                            div()
                                .px_3()
                                .py_2()
                                .rounded_md()
                                .bg(gpui::rgb(theme::STATUS_WARNING))
                                .text_color(gpui::rgb(theme::BG_APP))
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
        self.project_groups = roots
            .into_iter()
            .map(|root| {
                let indices = self
                    .sessions
                    .iter()
                    .enumerate()
                    .filter(|(_, session)| !session.archived && session.project_root == root)
                    .map(|(index, _)| index)
                    .collect();
                (root, indices)
            })
            .collect();
        self.archived_indices = self
            .sessions
            .iter()
            .enumerate()
            .filter(|(_, session)| session.archived)
            .map(|(index, _)| index)
            .collect();
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
        // The settings file is read and written off the UI thread.
        let pinned = self.pinned_roots.clone();
        self.call(
            async move {
                tokio::task::spawn_blocking(move || {
                    let mut settings = crate::settings::load_settings();
                    settings.pinned_roots = pinned;
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
                .text_color(gpui::rgb(theme::TEXT_SECONDARY))
                .child(text.to_uppercase())
        };
        div()
            .w(px(300.))
            .h_full()
            .flex()
            .flex_col()
            .bg(gpui::rgb(theme::BG_SIDEBAR))
            .child(
                div()
                    .flex()
                    .items_center()
                    .justify_between()
                    .pl_4()
                    .pr_3()
                    .pt_3()
                    .pb_2()
                    .child(wordmark(px(16.), theme::TEXT_PRIMARY))
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
                    .text_color(gpui::rgb(theme::ACCENT))
                    .hover(|style| {
                        style
                            .bg(gpui::rgb(theme::BG_SIDEBAR_ROW_HOVER))
                            .cursor_pointer()
                    })
                    .on_click(cx.listener(|this, _event, _window, cx| {
                        this.new_session(cx);
                    }))
                    .child(icon("square-pen", px(16.), theme::ACCENT))
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
                                            .bg(gpui::rgb(theme::BG_SIDEBAR_ROW_HOVER))
                                            .cursor_pointer()
                                    })
                                    .on_click(cx.listener(|this, _event, _window, cx| {
                                        this.choose_root_dialog(cx);
                                    }))
                                    .child(icon("folder-plus", px(16.), theme::TEXT_SECONDARY)),
                            ),
                    )
                    .children(self.project_groups.iter().map(|(root, indices)| {
                        let is_current = current_root == Some(root.as_str());
                        let is_collapsed = self.collapsed_roots.contains(root);
                        let is_pinned = self.pinned_roots.iter().any(|pinned| pinned == root);
                        let name = root_display_name(root);
                        let tasks = indices.iter().filter_map(|index| self.sessions.get(*index));
                        let group_name = SharedString::from(format!("project-row-{root}"));
                        div()
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
                                    .text_color(gpui::rgb(theme::TEXT_PRIMARY))
                                    .hover(|style| {
                                        style
                                            .bg(gpui::rgb(theme::BG_SIDEBAR_ROW_HOVER))
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
                                        theme::TEXT_SECONDARY,
                                    ))
                                    .child(icon(
                                        if is_current { "folder-open" } else { "folder" },
                                        px(16.),
                                        theme::TEXT_PRIMARY,
                                    ))
                                    .child(div().flex_1().min_w_0().line_clamp(1).child(name))
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
                                                .child(icon("pin", px(13.), theme::ACCENT)),
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
                                        SharedString::from(format!("archive-project-{root}")),
                                        &group_name,
                                        "archive",
                                        {
                                            let root = root.clone();
                                            cx.listener(move |this, _event, _window, cx| {
                                                cx.stop_propagation();
                                                this.archive_root(&root, cx);
                                            })
                                        },
                                    )),
                            )
                            .when(!is_collapsed, |column| {
                                column.children(tasks.map(|session| {
                                    self.render_task_row(session, selected, false, cx)
                                }))
                            })
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
                                                .bg(gpui::rgb(theme::BG_SIDEBAR_ROW_HOVER))
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
                                            theme::TEXT_SECONDARY,
                                        ))
                                        .child(section_label("Archived"))
                                        .child(
                                            div()
                                                .text_xs()
                                                .text_color(gpui::rgb(theme::TEXT_MUTED))
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
                row.bg(gpui::rgb(theme::BG_SIDEBAR_ROW_SELECTED))
                    .font_weight(gpui::FontWeight::MEDIUM)
            })
            .text_color(gpui::rgb(theme::TEXT_PRIMARY))
            .hover(|style| {
                style
                    .bg(gpui::rgb(theme::BG_SIDEBAR_ROW_HOVER))
                    .cursor_pointer()
            })
            .on_click(cx.listener(move |this, _event, _window, cx| {
                this.open_session(&session_id, cx);
            }))
            .child(
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
                                .text_color(gpui::rgb(theme::TEXT_MUTED))
                                .line_clamp(1)
                                .child(root_display_name(&session.project_root)),
                        )
                    }),
            )
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
                    .bg(gpui::rgb(theme::BG_SIDEBAR_ROW_HOVER))
                    .cursor_pointer()
            })
            .on_click(cx.listener(|_this, _event, _window, cx| {
                cx.emit(OpenSettings);
            }))
            .child(icon("settings", px(16.), theme::TEXT_SECONDARY));
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

    fn render_header(&self, cx: &mut Context<Self>) -> Div {
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
            .pl_4()
            .pr_3()
            .py_2()
            .when(self.sidebar_collapsed, |row| row.pl(px(220.)))
            .child(
                div()
                    .min_w_0()
                    .text_sm()
                    .font_weight(gpui::FontWeight::MEDIUM)
                    .text_color(gpui::rgb(theme::TEXT_PRIMARY))
                    .line_clamp(1)
                    .child(title),
            )
            .child(
                div().flex().items_center().gap_1().child(
                    div()
                        .id("header-new-task")
                        .flex()
                        .items_center()
                        .gap_1p5()
                        .px_2()
                        .py_1()
                        .rounded_md()
                        .text_sm()
                        .text_color(gpui::rgb(theme::TEXT_SECONDARY))
                        .hover(|style| {
                            style
                                .bg(gpui::rgb(theme::BG_ELEVATED))
                                .text_color(gpui::rgb(theme::TEXT_PRIMARY))
                                .cursor_pointer()
                        })
                        .on_click(cx.listener(|this, _event, _window, cx| {
                            this.new_session(cx);
                        }))
                        .child(icon("square-pen", px(14.), theme::TEXT_SECONDARY))
                        .child("New Task"),
                ),
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
            .bg(gpui::rgb(theme::BG_ELEVATED))
            .border_1()
            .border_color(gpui::rgb(theme::BORDER));
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
                            theme::ACCENT
                        } else {
                            theme::TEXT_PRIMARY
                        }))
                        .line_clamp(1)
                        .hover(|style| style.bg(gpui::rgb(theme::BG_INPUT)).cursor_pointer())
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
                    .text_color(gpui::rgb(theme::TEXT_SECONDARY))
                    .hover(|style| style.bg(gpui::rgb(theme::BG_INPUT)).cursor_pointer())
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
                            .text_color(gpui::rgb(theme::TEXT_MUTED))
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
                                    .bg(gpui::rgb(theme::ACCENT))
                                    .text_sm()
                                    .text_color(gpui::rgb(theme::TEXT_PRIMARY))
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
                let mode_icon = icon(permission_mode_icon(mode), px(14.), theme::TEXT_SECONDARY);
                let mode = mode.to_string();
                let is_current = self.permission_mode == mode;
                menu = menu.child(
                    div()
                        .id(gpui::SharedString::from(format!("mode-{mode}")))
                        .px_3()
                        .py_1()
                        .text_sm()
                        .text_color(gpui::rgb(if is_current {
                            theme::ACCENT
                        } else {
                            theme::TEXT_PRIMARY
                        }))
                        .hover(|style| style.bg(gpui::rgb(theme::BG_INPUT)).cursor_pointer())
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
                                        .text_color(gpui::rgb(theme::TEXT_MUTED))
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
                    .text_color(gpui::rgb(theme::TEXT_SECONDARY))
                    .child("MCP servers"),
            );
            if self.session_mcp.is_empty() {
                menu = menu.child(
                    div()
                        .px_3()
                        .py_2()
                        .text_sm()
                        .text_color(gpui::rgb(theme::TEXT_MUTED))
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
                        .hover(|style| style.bg(gpui::rgb(theme::BG_INPUT)).cursor_pointer())
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
                                        .text_color(gpui::rgb(theme::TEXT_PRIMARY))
                                        .line_clamp(1)
                                        .child(server.name.clone()),
                                )
                                .when(!server.description.is_empty(), |col| {
                                    col.child(
                                        div()
                                            .text_xs()
                                            .text_color(gpui::rgb(theme::TEXT_MUTED))
                                            .line_clamp(2)
                                            .child(server.description.clone()),
                                    )
                                })
                                .when(!available, |col| {
                                    col.child(
                                        div()
                                            .text_xs()
                                            .text_color(gpui::rgb(theme::STATUS_WARNING))
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
                                    theme::ACCENT
                                } else {
                                    theme::BORDER
                                }))
                                .flex()
                                .when(enabled, |track| track.justify_end())
                                .child(div().size(px(14.)).rounded_full().bg(gpui::rgb(
                                    if enabled {
                                        theme::BG_APP
                                    } else {
                                        theme::TEXT_SECONDARY
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
                    .border_color(gpui::rgb(theme::BORDER_SUBTLE))
                    .text_sm()
                    .text_color(gpui::rgb(theme::ACCENT))
                    .hover(|style| style.bg(gpui::rgb(theme::BG_INPUT)).cursor_pointer())
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
                    .text_color(gpui::rgb(theme::TEXT_PRIMARY))
                    .hover(|style| style.bg(gpui::rgb(theme::BG_INPUT)).cursor_pointer())
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
            .bg(gpui::rgb(theme::BG_ELEVATED))
            .border_1()
            .border_color(gpui::rgb(theme::BORDER));
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
                        row.bg(gpui::rgb(theme::BG_SIDEBAR_ROW_SELECTED))
                    })
                    .hover(|style| style.bg(gpui::rgb(theme::BG_INPUT)).cursor_pointer())
                    .on_click(move |_event, _window, cx: &mut gpui::App| {
                        chat.update(cx, |chat, cx| chat.complete_slash_command(&name, cx))
                            .ok();
                    })
                    .child(
                        div()
                            .font_family("monospace")
                            .text_color(gpui::rgb(theme::ACCENT))
                            .child(format!("/{}", entry.name)),
                    )
                    .child(
                        div()
                            .min_w_0()
                            .flex_1()
                            .text_xs()
                            .text_color(gpui::rgb(theme::TEXT_MUTED))
                            .line_clamp(1)
                            .child(entry.description.clone()),
                    ),
            );
        }
        Some(palette)
    }

    fn render_transcript(
        &mut self,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
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
                    .py_4()
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
                        .bg(gpui::rgb(theme::STATUS_ERROR))
                        .text_color(gpui::rgb(theme::BG_APP))
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
                        .bg(gpui::rgb(theme::STATUS_WARNING))
                        .text_color(gpui::rgb(theme::BG_APP))
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
                    .bg(gpui::rgba(0xffffff26)),
            )
    }

    fn render_composer(&mut self, cx: &mut Context<Self>) -> Div {
        let running = self.is_run_active();
        let disabled = self.booting;
        let has_text = self.composer_has_text;
        let has_images = !self.draft_images.is_empty();
        let can_send = !disabled && !running && (has_text || has_images);
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
            .bg(gpui::rgb(theme::BG_APP))
            .border_1()
            .border_color(gpui::rgb(theme::ACCENT))
            .when(disabled, |container| container.opacity(0.5))
            .when(!drafts.is_empty(), |container| {
                container.child(div().flex().flex_wrap().gap_2().px_4().pt_4().children(
                    drafts.iter().enumerate().map(|(index, image)| {
                        div()
                            .relative()
                            .size_16()
                            .rounded_xl()
                            .border_1()
                            .border_color(gpui::rgb(theme::BORDER))
                            .bg(gpui::rgb(theme::BG_ELEVATED))
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
                                None => frame.child(icon("image", px(20.), theme::TEXT_SECONDARY)),
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
                                    .bg(gpui::rgb(theme::BG_ELEVATED))
                                    .border_1()
                                    .border_color(gpui::rgb(theme::BORDER))
                                    .hover(|style| style.cursor_pointer())
                                    .on_click(cx.listener(move |this, _event, _window, cx| {
                                        if index < this.draft_images.len() {
                                            this.draft_images.remove(index);
                                        }
                                        cx.notify();
                                    }))
                                    .child(icon("x", px(10.), theme::TEXT_PRIMARY)),
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
                            .text_color(gpui::rgb(theme::TEXT_PRIMARY))
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
                            .hover(|style| style.bg(gpui::rgb(theme::BG_ELEVATED)).cursor_pointer())
                            .on_click(cx.listener(|this, _event, _window, cx| {
                                this.toggle_composer_expanded(cx);
                            }))
                            .child(icon(
                                if expanded { "minimize-2" } else { "maximize-2" },
                                px(14.),
                                theme::TEXT_MUTED,
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
                            .hover(|style| style.bg(gpui::rgb(theme::BG_ELEVATED)).cursor_pointer())
                            .when(self.image_picking, |el| el.opacity(0.5))
                            .on_click(cx.listener(|this, _event, _window, cx| {
                                this.pick_images(cx);
                            }))
                            .child(icon("image", px(16.), theme::TEXT_SECONDARY)),
                    )
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
                                .bg(gpui::rgb(theme::STATUS_ERROR))
                                .hover(|style| style.cursor_pointer())
                                .on_click(cx.listener(|this, _event, _window, cx| {
                                    this.stop(cx);
                                }))
                                .child(div().size_3().rounded_md().bg(gpui::rgb(theme::BG_APP))),
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
                                gpui::linear_color_stop(gpui::rgb(theme::SEND_TOP), 0.),
                                gpui::linear_color_stop(gpui::rgb(theme::SEND_BOTTOM), 1.),
                            ))
                            .when(!can_send, |el| el.opacity(0.4))
                            .when(can_send, |el| {
                                el.hover(|style| style.cursor_pointer())
                                    .on_click(cx.listener(|this, _event, _window, cx| {
                                        this.send_inner(cx);
                                    }))
                            })
                            .child(if disabled {
                                icon("loader-circle", px(16.), theme::BG_APP)
                            } else {
                                icon("arrow-up", px(16.), theme::BG_APP)
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
        theme::TEXT_PRIMARY
    } else {
        theme::TEXT_SECONDARY
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
        .when(active, |el| el.bg(gpui::rgb(theme::BG_ELEVATED)))
        .hover(|style| style.bg(gpui::rgb(theme::BG_ELEVATED)).cursor_pointer())
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
        .hover(|style| style.bg(gpui::rgb(theme::BG_SIDEBAR_ROW_SELECTED)))
        .on_click(on_click)
        .child(icon(icon_name, px(14.), theme::TEXT_SECONDARY))
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
    if is_user {
        let user_ctx = RenderCtx {
            selection: ctx.selection.clone(),
            base_ordinal: ctx.base_ordinal.map(|base| base + 2048),
            focus: ctx.focus.clone(),
            id_seed: format!("{}#user", ctx.id_seed),
        };
        let ordinal = user_ctx.base_ordinal;
        div().flex().justify_end().child(
            div()
                .max_w(gpui::relative(0.75))
                .px_4()
                .py_2()
                .rounded_lg()
                .bg(gpui::rgb(theme::BG_USER_BUBBLE))
                .border_1()
                .border_color(gpui::rgb(theme::USER_BUBBLE_BORDER))
                .text_color(gpui::rgb(theme::TEXT_PRIMARY))
                .when(has_images, |bubble| {
                    bubble.child(
                        div()
                            .flex()
                            .flex_wrap()
                            .gap_2()
                            .mb_1()
                            .children(attachments.map(|(id, name)| {
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
                                                    .border_color(gpui::rgb(theme::BORDER)),
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
                                        .bg(gpui::rgb(theme::BG_ELEVATED))
                                        .text_xs()
                                        .text_color(gpui::rgb(theme::TEXT_SECONDARY))
                                        .child(icon("paperclip", px(12.), theme::TEXT_SECONDARY))
                                        .child(name.to_string()),
                                }
                            })),
                    )
                })
                .when(!text.trim().is_empty(), |bubble| {
                    bubble.child(rich_text::plain_paragraph(
                        transcript.derived.get(item, revision).text.clone(),
                        ordinal,
                        &user_ctx,
                    ))
                }),
        )
    } else {
        div()
            .max_w_full()
            .pr_2()
            .text_color(gpui::rgb(theme::TEXT_PRIMARY))
            .child(markdown::render_with(
                &transcript
                    .markdown_cache
                    .get(&item.id, MarkdownKind::Body, revision, text),
                ctx,
            ))
    }
}

fn render_thinking(item: &AgentTimelineItem, revision: u64, transcript: &TranscriptCtx) -> Div {
    let text = transcript.derived.get(item, revision).text.clone();
    if text.trim().is_empty() {
        div().child(
            div()
                .text_color(gpui::rgb(theme::STATUS_RUNNING))
                .text_sm()
                .child("Thinking…"),
        )
    } else {
        div()
            .px_3()
            .py_2()
            .rounded_md()
            .bg(gpui::rgb(theme::BG_ELEVATED))
            .text_sm()
            .text_color(gpui::rgb(theme::TEXT_SECONDARY))
            .child(text)
    }
}

/// Goose's runtime strings rebranded for Maple users, who never see goose.
fn maple_display_text(text: &str) -> std::borrow::Cow<'_, str> {
    match text.trim() {
        "goose is compacting the conversation..." => {
            std::borrow::Cow::Borrowed("Compacting the conversation…")
        }
        "Context limit reached. Compacting to continue conversation..." => {
            std::borrow::Cow::Borrowed("Context limit reached — compacting to continue…")
        }
        _ => std::borrow::Cow::Borrowed(text),
    }
}

fn tool_status_style(status: Option<&str>) -> (&'static str, u32) {
    match status {
        Some("completed") => ("completed", theme::STATUS_SUCCESS),
        Some("failed") | Some("error") => ("failed", theme::STATUS_ERROR),
        Some("cancelled") | Some("controlled_externally") => ("stopped", theme::TEXT_MUTED),
        _ => ("running", theme::STATUS_RUNNING),
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
        .bg(gpui::rgb(theme::BG_TOOL_CARD))
        .border_1()
        .border_color(gpui::rgb(theme::BORDER_SUBTLE))
        .child(
            div()
                .text_sm()
                .font_weight(gpui::FontWeight::SEMIBOLD)
                .text_color(gpui::rgb(theme::TEXT_PRIMARY))
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
                "completed" => ("[x]", theme::STATUS_SUCCESS),
                "in_progress" => ("[~]", theme::STATUS_RUNNING),
                _ => ("[ ]", theme::TEXT_MUTED),
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
                                theme::TEXT_MUTED
                            } else {
                                theme::TEXT_PRIMARY
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
        .bg(gpui::rgb(theme::BG_CODE_BLOCK))
        .border_1()
        .border_color(gpui::rgb(theme::BORDER_SUBTLE))
        .overflow_x_hidden();
    for (sign, line) in diff_lines.iter() {
        let color = match sign {
            '+' => theme::STATUS_SUCCESS,
            '-' => theme::STATUS_ERROR,
            _ => theme::TEXT_SECONDARY,
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
    let title = item.title.clone().unwrap_or_else(|| item.item_type.clone());
    let item_id = item.id.clone();
    let chat_header = transcript.chat.clone();
    let summary = transcript.tool_summaries.get(&item.id).cloned();
    let has_summary = summary.is_some();
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
        .bg(gpui::rgb(theme::BG_TOOL_CARD))
        .border_1()
        .border_color(gpui::rgb(theme::BORDER_SUBTLE))
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
                        .text_color(gpui::rgb(theme::TEXT_PRIMARY))
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
                    theme::TEXT_MUTED,
                )),
        );
    // The payload region swallows clicks so selecting output text or
    // opening links does not collapse the card.
    let mut payload = div()
        .id(gpui::SharedString::from(format!(
            "tool-payload-{}",
            item.id
        )))
        .flex()
        .flex_col()
        .gap_1()
        .on_click(
            |_event: &gpui::ClickEvent, _window: &mut Window, cx: &mut gpui::App| {
                cx.stop_propagation();
            },
        );
    if let Some(summary) = summary {
        payload = payload.child(
            div()
                .text_sm()
                .text_color(gpui::rgb(theme::TEXT_SECONDARY))
                .line_clamp(2)
                .child(summary),
        );
    }
    if !details {
        // Compact: the model summary when present, otherwise a one-line
        // raw output preview.
        if !has_summary && summary_requested {
            // A summary is on the way; do not flash the raw call first.
            payload = payload.child(
                div()
                    .text_xs()
                    .text_color(gpui::rgb(theme::TEXT_MUTED))
                    .child("Summarizing…"),
            );
        } else if !has_summary && let Some(preview) = &derived.preview {
            payload = payload.child(
                div()
                    .text_xs()
                    .text_color(gpui::rgb(theme::TEXT_MUTED))
                    .line_clamp(1)
                    .child(preview.clone()),
            );
        }
        return div().child(card.child(payload));
    }
    // Input stays monospace JSON; the model summary replaces the raw
    // output once it arrives.
    if let Some(input) = &derived.input_line {
        payload = payload.child(
            div()
                .text_xs()
                .text_color(gpui::rgb(theme::TEXT_MUTED))
                .font_family("monospace")
                .line_clamp(2)
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
                .text_color(gpui::rgb(theme::TEXT_SECONDARY))
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

fn tool_input_line(item: &AgentTimelineItem) -> Option<String> {
    item.input
        .as_ref()
        .filter(|value| !value.is_null())
        .map(|value| format!("input: {value}"))
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
        .bg(gpui::rgb(theme::STATUS_ERROR))
        .text_color(gpui::rgb(theme::BG_APP))
        .text_sm()
        .child(text)
}

fn render_permission_row(item: &AgentTimelineItem) -> Div {
    let title = item
        .title
        .clone()
        .unwrap_or_else(|| "Permission".to_string());
    let status = match item.status.as_deref() {
        Some("completed") => ("allowed", theme::STATUS_SUCCESS),
        Some("denied") | Some("cancelled") => ("denied", theme::TEXT_MUTED),
        _ => ("waiting", theme::STATUS_WARNING),
    };
    div()
        .flex()
        .gap_2()
        .items_center()
        .px_3()
        .py_2()
        .rounded_md()
        .bg(gpui::rgb(theme::PERMISSION_FILL))
        .border_1()
        .border_color(gpui::rgb(theme::PERMISSION_BORDER))
        .child(
            div()
                .text_sm()
                .text_color(gpui::rgb(theme::TEXT_PRIMARY))
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
        .text_color(gpui::rgb(theme::TEXT_MUTED))
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
        .bg(gpui::rgb(theme::BG_ELEVATED))
        .border_1()
        .border_color(gpui::rgb(theme::STATUS_RUNNING))
        .flex()
        .flex_col()
        .gap_2()
        .child(
            div()
                .font_weight(gpui::FontWeight::SEMIBOLD)
                .text_color(gpui::rgb(theme::TEXT_PRIMARY))
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
                    .text_color(gpui::rgb(theme::TEXT_MUTED))
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
                .text_color(gpui::rgb(theme::TEXT_SECONDARY))
                .child(entry.header.clone()),
        );
        block = block.child(
            div()
                .text_sm()
                .text_color(gpui::rgb(theme::TEXT_SECONDARY))
                .child(entry.question.clone()),
        );
        for (option_index, option) in entry.options.iter().enumerate() {
            let is_picked = selected.get(&question_index) == Some(&option_index);
            let marker = div()
                .size_3()
                .rounded_full()
                .border_1()
                .border_color(gpui::rgb(if is_picked {
                    theme::ACCENT
                } else {
                    theme::BORDER
                }))
                .when(is_picked, |dot| dot.bg(gpui::rgb(theme::ACCENT)));
            // flex_1 is load-bearing: without it the row squeezes this
            // block to a character wide and the label wraps vertically.
            let label_element = if option.description.is_empty() {
                div()
                    .flex_1()
                    .min_w_0()
                    .text_sm()
                    .text_color(gpui::rgb(theme::TEXT_PRIMARY))
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
                            .text_color(gpui::rgb(theme::TEXT_PRIMARY))
                            .child(option.label.clone()),
                    )
                    .child(
                        div()
                            .text_xs()
                            .text_color(gpui::rgb(theme::TEXT_MUTED))
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
                            .bg(gpui::rgb(theme::BG_SIDEBAR_ROW_HOVER))
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
                        .bg(gpui::rgb(theme::BG_INPUT))
                        .border_1()
                        .border_color(gpui::rgb(theme::BORDER))
                        // The input inherits ambient color; without this the
                        // typed answer renders near-black on the dark field.
                        .text_color(gpui::rgb(theme::TEXT_PRIMARY))
                        .child(input),
                )
                .child(
                    div()
                        .id("question-submit")
                        .px_4()
                        .py_2()
                        .rounded_md()
                        .bg(gpui::rgb(theme::ACCENT))
                        .text_sm()
                        .text_color(gpui::rgb(theme::TEXT_PRIMARY))
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
            .text_color(gpui::rgb(theme::TEXT_MUTED))
            .hover(|style| {
                style
                    .text_color(gpui::rgb(theme::TEXT_SECONDARY))
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
            .bg(gpui::rgb(theme::TEXT_SECONDARY))
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
            .text_color(gpui::rgb(theme::TEXT_MUTED))
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
        .bg(gpui::rgb(theme::PERMISSION_FILL))
        .border_1()
        .border_color(gpui::rgb(theme::PERMISSION_BORDER))
        .flex()
        .flex_col()
        .gap_2()
        .child(
            div()
                .font_weight(gpui::FontWeight::SEMIBOLD)
                .text_color(gpui::rgb(theme::TEXT_PRIMARY))
                .child("Permission required"),
        )
        .child(
            div()
                .text_sm()
                .text_color(gpui::rgb(theme::TEXT_SECONDARY))
                .child(description),
        );
    if !arguments.is_empty() {
        card = card.child(
            div()
                .text_xs()
                .text_color(gpui::rgb(theme::TEXT_MUTED))
                .font_family("monospace")
                .max_h(gpui::px(120.))
                .overflow_hidden()
                .child(arguments),
        );
    }
    let mut buttons = div().flex().gap_2();
    for (label, allow, color) in [
        ("Allow once", true, theme::STATUS_SUCCESS),
        ("Deny", false, theme::STATUS_ERROR),
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
                .bg(gpui::rgb(if responding { theme::BORDER } else { color }))
                .text_sm()
                .text_color(gpui::rgb(theme::BG_APP))
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
                .text_color(gpui::rgb(theme::TEXT_MUTED))
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
            crate::backend::AgentBackend::new("http://127.0.0.1:9".to_string()).expect("backend"),
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
                    crate::backend::AgentBackend::new("http://127.0.0.1:9".to_string())
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
    fn test_skip_question_clears_card(cx: &mut TestAppContext) {
        let screen = screen(cx);
        let event = one_question("skip", "Skip me");
        screen.update(cx, |this, cx| {
            this.handle_service_event(event, cx);
            assert!(this.pending_questions.first().is_some());
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
                crate::backend::AgentBackend::new("http://127.0.0.1:9".to_string())
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
}
