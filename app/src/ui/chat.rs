//! The agent chat surface: session sidebar, streaming transcript with tool
//! calls, permission prompts, and the composer. Pure consumer of the backend
//! facade + event stream.

use std::cell::RefCell;
use std::collections::{HashMap, HashSet};
use std::rc::Rc;
use std::sync::Arc;

use gpui::{
    AnyElement, AppContext, Context, Div, Entity, EventEmitter, Render, SharedString, Window, div,
    prelude::*, px,
};

use maple_agent::agent::{
    AgentImageUpload, AgentSendMessageRequest, AgentServiceEvent, AgentSessionMcpServer,
    AgentSessionSummary, AgentTimelineItem,
};

use crate::backend::{AgentBackend, PendingPermission, PendingQuestion};
use crate::ui::icons::{icon, wordmark};
use crate::ui::markdown;
use crate::ui::settings::{OpenSettingsSection, Section};
use crate::ui::text_input::TextInput;
use crate::ui::theme;

pub struct LoggedOut;

/// Emitted when the user opens app settings from the chat header.
pub struct OpenSettings;

/// An image staged in the composer: the thumbnail source (a file on disk
/// or pasted bytes) and the data URL the runtime stores with the message.
#[derive(Clone)]
struct DraftImage {
    name: String,
    source: DraftImageSource,
    data_url: String,
}

/// Where a draft thumbnail comes from. `gpui::ImageSource` is not `Send`,
/// so drafts hold this and convert when they render.
#[derive(Clone)]
enum DraftImageSource {
    Path(Arc<std::path::Path>),
    Pasted(Arc<gpui::Image>),
}

impl From<&DraftImageSource> for gpui::ImageSource {
    fn from(source: &DraftImageSource) -> Self {
        match source {
            DraftImageSource::Path(path) => Self::from(Arc::clone(path)),
            DraftImageSource::Pasted(image) => Self::Image(Arc::clone(image)),
        }
    }
}

/// Per-item parsed markdown. Interior mutability because the list render
/// callback only has shared access to the screen.
#[derive(Default)]
struct MarkdownCache {
    entries: RefCell<HashMap<String, (u64, Rc<markdown::Document>)>>,
}

impl MarkdownCache {
    /// Parsed document for `source`, parsed now if the cache is stale.
    fn get(&self, key: &str, source: &str) -> Rc<markdown::Document> {
        use std::hash::{Hash, Hasher};
        let mut hasher = std::collections::hash_map::DefaultHasher::new();
        source.hash(&mut hasher);
        let hash = hasher.finish();
        let mut entries = self.entries.borrow_mut();
        if let Some((cached_hash, document)) = entries.get(key) {
            if *cached_hash == hash {
                return Rc::clone(document);
            }
        }
        if entries.len() > 4096 {
            entries.clear();
        }
        let document = Rc::new(markdown::parse(source));
        entries.insert(key.to_string(), (hash, Rc::clone(&document)));
        document
    }

    fn clear(&self) {
        self.entries.borrow_mut().clear();
    }
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
    pending_question: Option<PendingQuestion>,
    /// Lazily created answer input for the question card.
    pending_question_input: Option<Entity<TextInput>>,
    /// Request id whose input currently holds focus.
    question_focus_id: Option<String>,
    /// Fraction of the context window in use for the selected session.
    context_fraction: Option<f32>,
    /// True while the per-second usage poller task is running.
    usage_poller_active: bool,
    /// Last ledger-confirmed context tokens for the selected session.
    ledger_context_tokens: i64,
    /// Context limit used with the estimate above.
    context_limit: i64,
    composer_busy: bool,
    composer: Option<Entity<TextInput>>,
    models: Vec<String>,
    selected_model: Option<String>,
    models_menu_open: bool,
    /// Approval-mode dropdown open state, anchored under the composer.
    mode_menu_open: bool,
    /// Compact usage line for the sidebar bottom (tokens used this account).
    sidebar_usage: Option<crate::settings::UsageRow>,
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
    /// Parsed markdown per timeline item (keyed by item id and a hash of
    /// the source), so visible messages are parsed once, not every frame.
    markdown_cache: MarkdownCache,
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
}

impl ChatScreen {
    pub fn new(backend: Arc<AgentBackend>, user_id: String, cx: &mut Context<Self>) -> Self {
        let weak = cx.entity().downgrade();
        let mut this = Self::new_inner(backend, user_id);
        this.attach_composer(weak, cx);
        this.start(cx);
        this
    }

    /// Create and wire the composer; called by the real constructor.
    fn attach_composer(&mut self, weak: gpui::WeakEntity<Self>, cx: &mut Context<Self>) {
        let composer = cx.new(|cx| {
            TextInput::new("Ask Maple to work in this folder...", cx)
                .multiline(8)
                .clears_on_enter()
                .on_enter({
                    let weak = weak.clone();
                    move |text, _, cx| {
                        // The handler receives the composer text directly;
                        // clearing and sending happen without leasing the input.
                        if let Some(this) = weak.upgrade() {
                            this.update(cx, |chat, cx| chat.send_text(text, cx));
                        }
                    }
                })
                .on_paste_image(move |image, _, cx| {
                    if let Some(this) = weak.upgrade() {
                        this.update(cx, |chat, cx| chat.paste_image(image, cx));
                    }
                })
        });
        self.composer = Some(composer);
    }

    /// Test seam: pure state without composer wiring or runtime start.
    pub(crate) fn new_inner(backend: Arc<AgentBackend>, user_id: String) -> Self {
        Self::new_inner_with_placeholder(backend, user_id)
    }

    fn new_inner_with_placeholder(backend: Arc<AgentBackend>, user_id: String) -> Self {
        let settings = crate::settings::load_settings();
        let this = Self {
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
            pending_question: None,
            pending_question_input: None,
            question_focus_id: None,
            context_fraction: None,
            usage_poller_active: false,
            ledger_context_tokens: 0,
            context_limit: 0,
            composer_busy: false,
            composer: None,
            models: Vec::new(),
            selected_model: None,
            models_menu_open: false,
            mode_menu_open: false,
            sidebar_usage: None,
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
            image_picking: false,
            model_vision: HashMap::new(),
            session_mcp: Vec::new(),
            mcp_menu_open: false,
            composer_expanded: false,
            web_enabled: true,
            default_web_enabled: settings.default_web_enabled,
            markdown_cache: MarkdownCache::default(),
            project_groups: Vec::new(),
            archived_indices: Vec::new(),
            collapsed_roots: HashSet::new(),
            archived_expanded: false,
            root_input: None,
            root_switching: false,
            selection_generation: 0,
            timeline_revisions: HashMap::new(),
        };
        this
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
                this.refresh_sessions(cx);
                this.refresh_sidebar_usage(cx);
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
                        this.timeline.clear();
                        this.list_state.reset(0);
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
        self.selection_generation += 1;
        let generation = self.selection_generation;
        let revision = *self.timeline_revisions.get(session_id).unwrap_or(&0);
        let backend = self.backend.clone();
        let user_id = self.user_id.clone();
        let session_id = session_id.to_string();
        self.call(
            async move { backend.load_session(&user_id, &session_id).await },
            cx,
            move |this, result, cx| {
                // A newer selection (or reload) superseded this load.
                if this.selection_generation != generation {
                    return;
                }
                match result {
                    Ok(detail) => {
                        // Events applied while the snapshot was in flight are
                        // newer than the snapshot; keep them instead.
                        let current = *this
                            .timeline_revisions
                            .get(&detail.session.id)
                            .unwrap_or(&0);
                        if current != revision {
                            return;
                        }
                        if let Some(existing) = this
                            .sessions
                            .iter_mut()
                            .find(|session| session.id == detail.session.id)
                        {
                            *existing = detail.session.clone();
                        } else {
                            this.sessions.insert(0, detail.session.clone());
                        }
                        this.set_active_session(detail.session, detail.timeline, cx);
                    }
                    Err(message) => this.notice = Some(message.into()),
                }
            },
        );
    }

    /// Replace the timeline after a mid-run history compaction without
    /// disturbing the active run or a pending permission.
    fn reload_timeline(&mut self, session_id: &str, cx: &mut Context<Self>) {
        self.selection_generation += 1;
        let generation = self.selection_generation;
        let revision = *self.timeline_revisions.get(session_id).unwrap_or(&0);
        let backend = self.backend.clone();
        let user_id = self.user_id.clone();
        let session_id = session_id.to_string();
        self.call(
            async move { backend.load_session(&user_id, &session_id).await },
            cx,
            move |this, result, _cx| {
                if this.selection_generation != generation {
                    return;
                }
                if let Ok(detail) = result {
                    let current = *this
                        .timeline_revisions
                        .get(&detail.session.id)
                        .unwrap_or(&0);
                    if current != revision {
                        return;
                    }
                    this.timeline = detail.timeline;
                    this.list_state.reset(this.timeline.len());
                }
            },
        );
    }

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
                        this.timeline.clear();
                        this.list_state.reset(0);
                        if let Some(next) = this.sessions.first().cloned() {
                            let id = next.id;
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
                    this.ledger_context_tokens = tokens;
                    this.context_limit = limit;
                    let fraction = if limit > 0 {
                        tokens as f32 / limit as f32
                    } else {
                        0.0
                    };
                    this.context_fraction = Some(fraction);
                    cx.notify();
                }
            },
        );
    }

    /// Poll context usage once per second while the selected session runs.
    /// The goose usage ledger gains a row on every inference call, so this
    /// tracks the ring in real time at each turn boundary.
    fn start_usage_poller(&mut self, session_id: String, cx: &mut Context<Self>) {
        if self.usage_poller_active {
            return;
        }
        self.usage_poller_active = true;
        let backend = self.backend.clone();
        let user_id = self.user_id.clone();
        let target = session_id;
        cx.spawn(async move |this, cx| {
            loop {
                cx.background_executor()
                    .timer(std::time::Duration::from_millis(1000))
                    .await;
                let keep_going = this
                    .update(cx, |this: &mut ChatScreen, cx| {
                        let running = this
                            .selected_session
                            .as_deref()
                            .is_some_and(|selected| selected == target.as_str())
                            && this.active_runs.contains_key(&target);
                        if running {
                            let backend = backend.clone();
                            let user_id = user_id.clone();
                            let target = target.clone();
                            let model = this.selected_model.clone();
                            this.call(
                                async move {
                                    backend
                                        .context_usage(&user_id, &target, model.as_deref())
                                        .await
                                },
                                cx,
                                |this, result, cx| {
                                    if let Ok(Some((tokens, limit))) = result {
                                        this.ledger_context_tokens = tokens;
                                        this.context_limit = limit;
                                        let fraction = if limit > 0 {
                                            tokens as f32 / limit as f32
                                        } else {
                                            0.0
                                        };
                                        if this.context_fraction != Some(fraction) {
                                            this.context_fraction = Some(fraction);
                                            cx.notify();
                                        }
                                    }
                                },
                            );
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
        self.web_enabled = session.web_enabled;
        self.apply_permission_mode(cx);
        self.timeline = timeline;
        self.markdown_cache.clear();
        self.list_state.reset(self.timeline.len());
        self.follow_transcript = true;
        self.pending_permission = None;
        self.permission_responding = false;
        self.models_menu_open = false;
        self.root_menu_open = false;
        self.mcp_menu_open = false;
        self.refresh_session_mcp(cx);
        cx.notify();
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
                    Ok(session) => this.upsert_session(session),
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
        let image = Arc::new(image);
        match draft_image_from_bytes(
            name,
            &image.bytes,
            DraftImageSource::Pasted(Arc::clone(&image)),
        ) {
            Ok(draft) => {
                self.notice = None;
                self.draft_images.push(draft);
            }
            Err(message) => self.notice = Some(message.into()),
        }
        cx.notify();
    }

    /// Open the native image picker and stage the chosen files.
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
                    Ok(images) => this.draft_images.extend(images),
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
        if text.trim().is_empty() && self.draft_images.is_empty() {
            return;
        }
        if text.trim() == "/compact" {
            self.compact_now(cx);
            return;
        }
        self.send_to_session(&session_id, text, cx);
    }

    fn send_to_session(&mut self, session_id: &str, text: String, cx: &mut Context<Self>) {
        let session_id = session_id.to_string();
        let backend = self.backend.clone();
        let user_id = self.user_id.clone();
        let model = self.selected_model.clone();
        let vision_capable = self.selected_model_supports_vision();
        let drafts = std::mem::take(&mut self.draft_images);
        let request = AgentSendMessageRequest {
            session_id: session_id.to_string(),
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
        // The caller already cleared the composer (the Enter path clears
        // inside the input itself); clearing here would double-lease it.
        self.notice = None;
        self.follow_transcript = true;
        cx.notify();
        self.call(
            async move { backend.send_message(&user_id, request).await },
            cx,
            move |this, result, cx| match result {
                Ok(run_id) => {
                    this.active_runs.insert(session_id.to_string(), run_id);
                }
                Err(message) => {
                    // Show the failure in the transcript and give the draft
                    // back instead of silently dropping it.
                    this.push_local_error("Send failed", &message, cx);
                    if let Some(composer) = this.composer.clone() {
                        composer.update(cx, |input, cx| input.set_text(&text, cx));
                    }
                    this.draft_images = drafts;
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

    fn submit_question(&mut self, cx: &mut Context<Self>) {
        let Some(question) = self.pending_question.clone() else {
            return;
        };
        let Some(input) = self.pending_question_input.clone() else {
            return;
        };
        let answer = input.read(cx).text();
        if answer.trim().is_empty() {
            return;
        }
        let backend = self.backend.clone();
        let user_id = self.user_id.clone();
        self.pending_question = None;
        cx.notify();
        self.call(
            async move {
                backend
                    .answer_question(&user_id, &question.request_id, answer)
                    .await
            },
            cx,
            |_this, _result, _cx| {},
        );
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

    /// Apply a timeline item using Maple's merge contract: `append` extends
    /// message/thinking text on the item with the same id; otherwise merge
    /// fields, keeping the previous value when the incoming field is absent.
    fn apply_timeline_item(&mut self, session_id: &str, item: AgentTimelineItem) {
        // Streaming delivers one of these per chunk; move the payload
        // rather than copy it.
        let position = self
            .timeline
            .iter()
            .position(|candidate| candidate.id == item.id);
        match position {
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
            }
            None => {
                self.timeline.push(item);
                self.list_state
                    .splice(self.timeline.len() - 1..self.timeline.len() - 1, 1);
            }
        }
        *self
            .timeline_revisions
            .entry(session_id.to_string())
            .or_insert(0) += 1;
    }

    fn upsert_session(&mut self, session: AgentSessionSummary) {
        if let Some(existing) = self
            .sessions
            .iter_mut()
            .find(|candidate| candidate.id == session.id)
        {
            *existing = session;
        } else {
            self.sessions.insert(0, session);
        }
        self.rebuild_project_groups();
    }

    /// Route a batch of backend service events into UI state with a single
    /// render at the end.
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
    pub fn handle_service_event(&mut self, event: AgentServiceEvent, cx: &mut Context<Self>) {
        if self.apply_service_event(event, cx) {
            cx.notify();
        }
    }

    /// Apply one event; returns false when nothing visible changed.
    fn apply_service_event(&mut self, event: AgentServiceEvent, cx: &mut Context<Self>) -> bool {
        match event {
            AgentServiceEvent::RuntimeStatus(status) => {
                // The status snapshot is authoritative for active runs.
                self.active_runs = status.active_runs;
            }
            AgentServiceEvent::SessionCreated(session) => {
                self.upsert_session(session);
            }
            AgentServiceEvent::SessionUpdated { session, .. } => {
                self.upsert_session(session);
            }
            AgentServiceEvent::TimelineItem {
                session_id, item, ..
            } => {
                if self.selected_session.as_deref() == Some(session_id.as_str()) {
                    self.apply_timeline_item(&session_id, item);
                } else {
                    return false;
                }
            }
            AgentServiceEvent::Question {
                session_id,
                request_id,
                question,
            } => {
                if self.selected_session.as_deref() == Some(session_id.as_str()) {
                    if self.pending_question_input.is_none() {
                        let chat = cx.entity().downgrade();
                        let input = cx.new(|cx| {
                            TextInput::new("Type your answer…", cx).on_enter(move |text, _, cx| {
                                let _ = text;
                                if let Some(chat) = chat.upgrade() {
                                    chat.update(cx, |chat, cx| chat.submit_question(cx));
                                }
                            })
                        });
                        self.pending_question_input = Some(input);
                    }
                    self.pending_question = Some(PendingQuestion {
                        request_id,
                        question,
                    });
                }
            }
            AgentServiceEvent::Run {
                session_id,
                run_id,
                event,
            } => self.handle_run_event(&session_id, &run_id, event, cx),
        }
        true
    }

    fn is_selected(&self, session_id: &str) -> bool {
        self.selected_session.as_deref() == Some(session_id)
    }

    fn handle_run_event(
        &mut self,
        session_id: &str,
        run_id: &str,
        event: maple_agent::agent::AgentRunEvent,
        cx: &mut Context<Self>,
    ) {
        use maple_agent::agent::AgentRunEvent;
        match event {
            AgentRunEvent::SessionUpdated(session) => {
                self.upsert_session(session);
            }
            AgentRunEvent::Started => {
                self.active_runs
                    .insert(session_id.to_string(), run_id.to_string());
                self.start_usage_poller(session_id.to_string(), cx);
                self.refresh_context_usage(cx);
            }
            AgentRunEvent::TimelineItem(item) => {
                if self.is_selected(session_id) {
                    self.apply_timeline_item(session_id, item);
                }
            }
            AgentRunEvent::PermissionRequested { request, item } => {
                if self.is_selected(session_id) {
                    // The permission row stays in the transcript so the
                    // decision is visible after the card is answered.
                    self.apply_timeline_item(session_id, item);
                    let arguments = serde_json::Value::Object(request.arguments);
                    self.pending_permission_arguments = if arguments.is_null() {
                        SharedString::default()
                    } else {
                        serde_json::to_string_pretty(&arguments)
                            .unwrap_or_default()
                            .into()
                    };
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
                    self.apply_timeline_item(session_id, item);
                }
            }
            AgentRunEvent::Finished(_) => {
                // Only retire the run that actually finished; a late
                // Finished from a cancelled run must not clear a newer one.
                if self.active_runs.get(session_id).map(String::as_str) == Some(run_id) {
                    self.active_runs.remove(session_id);
                }
                if let Some(permission) = &self.pending_permission {
                    if permission.run_id == run_id {
                        self.pending_permission = None;
                        self.permission_responding = false;
                    }
                }
                self.refresh_sidebar_usage(cx);
                self.refresh_sidebar_plan(cx);
            }
            AgentRunEvent::QueueChanged(_) | AgentRunEvent::QueuePromoted { .. } => {
                // Queue chips are rendered from send responses; nothing to do
                // until queue editing is exposed in the UI.
            }
        }
    }
}

impl EventEmitter<LoggedOut> for ChatScreen {}

impl EventEmitter<OpenSettings> for ChatScreen {}
impl EventEmitter<OpenSettingsSection> for ChatScreen {}

impl Render for ChatScreen {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
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
                .when_some(self.pending_question.as_ref(), |container, question| {
                    let input = self.pending_question_input.clone();
                    container.child(render_question_card(question, input, cx))
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
                        .children(self.render_menu_panel(cx)),
                )
        };
        div()
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
                            .children(self.render_menu_panel(cx)),
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

    /// Group sessions by project root: current root first, then the other
    /// recent roots, then any root that only appears on a stored task.
    /// Called when sessions or roots change, not per render.
    fn rebuild_project_groups(&mut self) {
        let mut roots: Vec<String> = Vec::new();
        if let Some(root) = self.project_root.clone() {
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
                            this.timeline.clear();
                            this.list_state.reset(0);
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
                            this.timeline.clear();
                            this.list_state.reset(0);
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

    /// Sidebar footer: settings gear on the left and the plan usage card on
    /// the right. The card shows the plan pill, percent used, reset date,
    /// and a progress bar. Without plan data it shows token totals instead.
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

        let card = div()
            .flex_1()
            .min_w_0()
            .flex()
            .flex_col()
            .gap_2()
            .px_2p5()
            .py_2p5()
            .rounded_lg()
            .bg(gpui::rgb(theme::BG_SIDEBAR_CARD));

        let card = match &self.sidebar_plan {
            Some(plan) => {
                let fraction = f32::from(plan.percent_used) / 100.0;
                card.child(
                    div()
                        .flex()
                        .items_center()
                        .justify_between()
                        .gap_2()
                        .child(
                            div()
                                .flex()
                                .items_center()
                                .gap_2()
                                .flex_none()
                                .child(
                                    div()
                                        .px_1p5()
                                        .py_0p5()
                                        .rounded_md()
                                        .bg(gpui::rgb(theme::BG_SIDEBAR_PILL))
                                        .text_size(px(10.))
                                        .font_weight(gpui::FontWeight::BOLD)
                                        .text_color(gpui::rgb(theme::ACCENT))
                                        .whitespace_nowrap()
                                        .child(plan.plan_label.to_uppercase()),
                                )
                                .child(
                                    div()
                                        .text_xs()
                                        .font_weight(gpui::FontWeight::BOLD)
                                        .text_color(gpui::rgb(theme::TEXT_PRIMARY))
                                        .whitespace_nowrap()
                                        .child(format!("{}% used", plan.percent_used)),
                                ),
                        )
                        .child(
                            div()
                                .min_w_0()
                                .text_size(px(10.))
                                .text_color(gpui::rgb(theme::TEXT_MUTED))
                                .whitespace_nowrap()
                                .overflow_hidden()
                                .child(format!("· Resets {}", plan.resets_label)),
                        ),
                )
                .child(
                    div()
                        .w_full()
                        .h(px(3.))
                        .rounded_full()
                        .bg(gpui::rgb(theme::BORDER))
                        .child(
                            div()
                                .h_full()
                                .w(gpui::relative(fraction))
                                .rounded_full()
                                .bg(gpui::rgb(theme::ACCENT)),
                        ),
                )
            }
            None => card.child(
                div()
                    .text_xs()
                    .text_color(gpui::rgb(theme::TEXT_MUTED))
                    .line_clamp(1)
                    .child(match &self.sidebar_usage {
                        Some(row) => format!(
                            "{} tokens · ${:.2}",
                            format_usage_tokens(row.total_tokens),
                            row.cost
                        ),
                        None => "Usage unavailable".to_string(),
                    }),
            ),
        };

        div()
            .flex()
            .items_center()
            .gap_2()
            .px_3()
            .py_3()
            .child(gear)
            .child(card)
    }

    /// Load the compact sidebar usage line from the goose usage ledger.
    fn refresh_sidebar_usage(&mut self, cx: &mut Context<Self>) {
        let backend = self.backend.clone();
        let user_id = self.user_id.clone();
        self.call(
            async move {
                let scope = backend.account_scope(&user_id);
                Ok(scope.map(|scope| crate::settings::load_usage(&scope).totals))
            },
            cx,
            |this, result: Result<Option<crate::settings::UsageRow>, String>, cx| {
                if let Ok(totals) = result {
                    this.sidebar_usage = totals;
                    cx.notify();
                }
            },
        );
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
        // Only the visible items (plus a small overdraw) are built each
        // frame; the list measures and caches the rest.
        let list = gpui::list(self.list_state.clone(), move |ix, _window, cx| {
            let Some(chat) = entity.upgrade() else {
                return div().into_any_element();
            };
            let chat = chat.read(cx);
            match chat.timeline.get(ix) {
                Some(item) => render_timeline_item(item, tool_details, &chat.markdown_cache)
                    .into_any_element(),
                None => div().into_any_element(),
            }
        })
        .size_full();
        div()
            .id("transcript")
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
        let has_text = self
            .composer
            .as_ref()
            .map(|composer| !composer.read(cx).text().trim().is_empty())
            .unwrap_or(false);
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
                            .child(
                                gpui::img(gpui::ImageSource::from(&image.source))
                                    .size_16()
                                    .rounded_xl()
                                    .object_fit(gpui::ObjectFit::Cover)
                                    .border_1()
                                    .border_color(gpui::rgb(theme::BORDER)),
                            )
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

/// Borderless toolbar chip: optional leading icon, label, optional chevron.
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

/// Read an image file into a data URL, checking size and format the same
/// way the runtime does so errors surface before the send.
fn load_draft_image(path: &std::path::Path) -> Result<DraftImage, String> {
    let name = path
        .file_name()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_else(|| "image".to_string());
    let bytes = std::fs::read(path).map_err(|error| format!("Could not read {name}: {error}"))?;
    draft_image_from_bytes(name, &bytes, DraftImageSource::Path(Arc::from(path)))
}

/// Build a draft from raw image bytes, checking size and format the same
/// way the runtime does so errors surface before the send.
fn draft_image_from_bytes(
    name: String,
    bytes: &[u8],
    source: DraftImageSource,
) -> Result<DraftImage, String> {
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
        name,
        source,
        data_url: format!("data:{mime};base64,{}", base64_encode(bytes)),
    })
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

/// Compact token count for the sidebar usage line (k/M).
fn format_usage_tokens(tokens: i64) -> String {
    if tokens >= 1_000_000 {
        format!("{:.1}M", tokens as f64 / 1_000_000.0)
    } else if tokens >= 1_000 {
        format!("{:.1}k", tokens as f64 / 1_000.0)
    } else {
        tokens.to_string()
    }
}

fn render_timeline_item(
    item: &AgentTimelineItem,
    tool_details: bool,
    markdown_cache: &MarkdownCache,
) -> Div {
    let item = match item.item_type.as_str() {
        "message" => render_message(item, markdown_cache),
        "thinking" | "reasoning" => render_thinking(item),
        "tool" | "toolCall" => {
            // Dispatch on payload shape; runtime titles are humanized
            // ("todo write", "ask user") and vary by detail suffix.
            if has_tool_input(item, "todos") {
                render_todo(item)
            } else if has_tool_input(item, "edits")
                || (has_tool_input(item, "content") && has_tool_input(item, "path"))
            {
                render_tool_with_diff(item, tool_details, markdown_cache)
            } else {
                render_tool(item, tool_details, markdown_cache)
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

fn render_message(item: &AgentTimelineItem, markdown_cache: &MarkdownCache) -> Div {
    let is_user = item.role.as_deref() == Some("user");
    let text = item.text.as_deref().unwrap_or("");
    let has_images = item
        .input
        .as_ref()
        .is_some_and(|input| input.get("imageAttachments").is_some());
    if text.trim().is_empty() && !(is_user && has_images) {
        return div();
    }
    if is_user {
        let attachments: Vec<String> = item
            .input
            .as_ref()
            .and_then(|input| input.get("imageAttachments"))
            .and_then(|items| items.as_array())
            .map(|items| {
                items
                    .iter()
                    .filter_map(|entry| entry.get("name").and_then(|n| n.as_str()))
                    .map(ToOwned::to_owned)
                    .collect()
            })
            .unwrap_or_default();
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
                .when(!attachments.is_empty(), |bubble| {
                    bubble.child(div().flex().flex_wrap().gap_2().mb_1().children(
                        attachments.into_iter().map(|name| {
                            div()
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
                                .child(name)
                        }),
                    ))
                })
                .when(!text.trim().is_empty(), |bubble| {
                    bubble.child(text.to_string())
                }),
        )
    } else {
        div()
            .max_w_full()
            .pr_2()
            .text_color(gpui::rgb(theme::TEXT_PRIMARY))
            .child(markdown::render(&markdown_cache.get(&item.id, text)))
    }
}
fn render_thinking(item: &AgentTimelineItem) -> Div {
    let text = item.text.clone().unwrap_or_default();
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
    if let Some(serde_json::Value::Object(map)) = item.input.as_ref() {
        if let Some(serde_json::Value::Array(todos)) = map.get("todos") {
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
    }
    card
}

/// Tool card whose payload renders as a colored diff when it carries
/// edit/write replacements.
fn render_tool_with_diff(
    item: &AgentTimelineItem,
    details: bool,
    markdown_cache: &MarkdownCache,
) -> Div {
    let card = render_tool(item, details, markdown_cache);
    if !details {
        return card;
    }
    // Build a +/- view from the edit set when present.
    let mut diff_lines: Vec<(char, String)> = Vec::new();
    if let Some(serde_json::Value::Object(map)) = item.input.as_ref() {
        if let Some(path) = map.get("path").and_then(|v| v.as_str()) {
            diff_lines.push((' ', path.to_string()));
        }
        if let Some(serde_json::Value::Array(edits)) = map.get("edits") {
            for edit in edits {
                if let Some(old) = edit.get("oldText").and_then(|v| v.as_str()) {
                    for line in old.lines() {
                        diff_lines.push(('-', line.to_string()));
                    }
                }
                if let Some(new) = edit.get("newText").and_then(|v| v.as_str()) {
                    for line in new.lines() {
                        diff_lines.push(('+', line.to_string()));
                    }
                }
            }
        }
        if let Some(content) = map.get("content").and_then(|v| v.as_str()) {
            for line in content.lines() {
                diff_lines.push(('+', line.to_string()));
            }
        }
    }
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
    for (sign, line) in diff_lines.into_iter().take(200) {
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
                        .child(line),
                ),
        );
    }
    card.child(diff)
}

fn render_tool(item: &AgentTimelineItem, details: bool, markdown_cache: &MarkdownCache) -> Div {
    let (label, status_color) = tool_status_style(item.status.as_deref());
    let title = item.title.clone().unwrap_or_else(|| item.item_type.clone());
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
                ),
        );
    if !details {
        // Compact: tool name and status only, no payload.
        return card;
    }
    // Input stays monospace JSON; the output renders as markdown when it
    // carries readable text, falling back to the raw JSON line.
    if let Some(input) = tool_input_line(item) {
        card = card.child(
            div()
                .text_xs()
                .text_color(gpui::rgb(theme::TEXT_MUTED))
                .font_family("monospace")
                .line_clamp(2)
                .overflow_x_hidden()
                .child(input),
        );
    }
    if let Some(output) = tool_output_markdown(item) {
        card = card.child(
            div()
                .mt_1()
                .w_full()
                .text_sm()
                .text_color(gpui::rgb(theme::TEXT_SECONDARY))
                .child(markdown::render(
                    &markdown_cache.get(&format!("{}#output", item.id), &output),
                )),
        );
    }
    card
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
                if let Some(inner) = map.get(key) {
                    if let Some(text) = extract_output_text(inner) {
                        return Some(text);
                    }
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
    input: Option<Entity<TextInput>>,
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
        )
        .child(
            div()
                .text_sm()
                .text_color(gpui::rgb(theme::TEXT_SECONDARY))
                .child(question.question.clone()),
        );
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
                        .child("Answer"),
                ),
        );
    }
    card
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

    #[gpui::test]
    fn test_question_event_sets_pending_card(cx: &mut TestAppContext) {
        let screen = screen(cx);
        let event = AgentServiceEvent::Question {
            session_id: "s1".to_string(),
            request_id: "q1".to_string(),
            question: "Favorite color?".to_string(),
        };
        screen.update(cx, |this, cx| {
            assert!(this.pending_question.is_none());
            this.handle_service_event(event, cx);
            let question = this.pending_question.as_ref().expect("question set");
            assert_eq!(question.request_id, "q1");
            assert!(this.pending_question_input.is_some());
        });
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
            assert!(matches!(
                this.draft_images[0].source,
                DraftImageSource::Pasted(_)
            ));
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
