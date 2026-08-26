//! The agent chat surface: session sidebar, streaming transcript with tool
//! calls, permission prompts, and the composer. Pure consumer of the backend
//! facade + event stream.

use std::collections::HashMap;
use std::sync::Arc;

use gpui::{
    AnyElement, AppContext, Context, Div, Entity, EventEmitter, Render, Window, div, prelude::*, px,
};

use maple_agent::agent::{
    AgentSendMessageRequest, AgentServiceEvent, AgentSessionSummary, AgentTimelineItem,
};

use crate::backend::{AgentBackend, PendingPermission, PendingQuestion};
use crate::ui::markdown;
use crate::ui::text_input::TextInput;
use crate::ui::theme;

pub struct LoggedOut;

/// Emitted when the user opens app settings from the chat header.
pub struct OpenSettings;

pub struct ChatScreen {
    backend: Arc<AgentBackend>,
    user_id: String,
    sessions: Vec<AgentSessionSummary>,
    selected_session: Option<String>,
    timeline: Vec<AgentTimelineItem>,
    /// Active run per session id, kept across selection changes.
    active_runs: HashMap<String, String>,
    pending_permission: Option<PendingPermission>,
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
    runtime_error: Option<String>,
    notice: Option<String>,
    booting: bool,
    /// Virtualized transcript state; bottom-aligned like a chat log.
    list_state: gpui::ListState,
    /// Drives the transcript scroll; pinned to the bottom on new items.
    transcript_scroll: gpui::ScrollHandle,
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
            TextInput::new("Message Maple…", cx)
                .clears_on_enter()
                .on_enter(move |text, _, cx| {
                    // The handler receives the composer text directly;
                    // clearing and sending happen without leasing the input.
                    if let Some(this) = weak.upgrade() {
                        this.update(cx, |chat, cx| chat.send_text(text, cx));
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
        let this = Self {
            backend,
            user_id,
            sessions: Vec::new(),
            selected_session: None,
            timeline: Vec::new(),
            active_runs: HashMap::new(),
            pending_permission: None,
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
            runtime_error: None,
            notice: None,
            booting: true,
            list_state: gpui::ListState::new(0, gpui::ListAlignment::Bottom, px(400.)),
            transcript_scroll: gpui::ScrollHandle::new(),
            sidebar_scroll: gpui::ScrollHandle::new(),
            follow_transcript: true,
            tool_details: true,
            permission_mode: std::env::var("MAPLE_PERMISSION_MODE")
                .ok()
                .filter(|mode| mode == "auto" || mode == "smart_approve")
                .or_else(|| {
                    crate::settings::load_settings()
                        .default_permission_mode
                        .into()
                })
                .unwrap_or_else(|| "smart_approve".to_string()),
            uses_default_permission_mode: std::env::var("MAPLE_PERMISSION_MODE").is_err(),
            project_root: None,
            recent_roots: Vec::new(),
            root_menu_open: false,
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
                        this.runtime_error = Some(format!("Runtime failed to start: {message}"))
                    }
                }
                this.booting = false;
                cx.notify();
                this.refresh_models(cx);
                this.refresh_roots(cx);
                this.refresh_sessions(cx);
                this.refresh_sidebar_usage(cx);
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
            self.notice = Some("Enter an absolute directory path".to_string());
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
                        this.selected_session = None;
                        this.timeline.clear();
                        this.list_state.reset(0);
                        this.refresh_roots(cx);
                        this.refresh_sessions(cx);
                    }
                    Err(message) => this.notice = Some(message),
                }
                cx.notify();
            },
        );
    }

    fn choose_root_dialog(&mut self, cx: &mut Context<Self>) {
        // Best-effort native directory picker; falls back to manual entry.
        let current = self.project_root.clone().unwrap_or_default();
        let output = std::process::Command::new("zenity")
            .arg("--file-selection")
            .arg("--directory")
            .arg("--filename")
            .arg(&current)
            .output();
        match output {
            Ok(output) if output.status.success() => {
                let path = String::from_utf8_lossy(&output.stdout).trim().to_string();
                if !path.is_empty() {
                    self.switch_root(path, cx);
                }
            }
            _ => {
                // zenity missing or cancelled: offer manual entry.
                if self.root_input.is_none() {
                    let input = cx.new(|cx| {
                        TextInput::new("/absolute/path/to/project", cx).with_tab_index(0)
                    });
                    self.root_input = Some(input);
                }
                cx.notify();
            }
        }
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
                }
                cx.notify();
            },
        );
    }

    fn refresh_sessions(&self, cx: &mut Context<Self>) {
        let backend = self.backend.clone();
        let user_id = self.user_id.clone();
        // Scope the list to the runtime's project root so sessions created
        // under other roots never leak in (their working directories would
        // run tools in the wrong place).
        let root = self.project_root.clone();
        self.call(
            async move { backend.list_sessions(&user_id, root).await },
            cx,
            |this, result, cx| {
                match result {
                    Ok(sessions) => {
                        this.sessions = sessions;
                        if this.selected_session.is_none() {
                            if let Some(latest) = this.sessions.first().cloned() {
                                let id = latest.id.clone();
                                this.select_session(&id, cx);
                            } else {
                                this.new_session(cx);
                            }
                        }
                    }
                    Err(message) => this.notice = Some(message),
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
                }
                Err(message) => {
                    this.session_setup_pending = false;
                    this.notice = Some(message);
                }
            },
        );
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
                    Err(message) => this.notice = Some(message),
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
                            let id = next.id.clone();
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
        if self.uses_default_permission_mode {
            self.permission_mode = settings.default_permission_mode.clone();
            self.apply_permission_mode(cx);
        }
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
        let target = session_id.clone();
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
                                        this.context_fraction = Some(fraction);
                                        cx.notify();
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
        self.notice = Some("Compacting…".to_string());
        cx.notify();
        self.call(
            async move { backend.compact_session(&user_id, &session_id).await },
            cx,
            |this, result, cx| match result {
                Ok(()) => {
                    this.notice = Some("Conversation compacted".to_string());
                    let sid = this.selected_session.clone().unwrap_or_default();
                    this.select_session(&sid, cx);
                    this.refresh_context_usage(cx);
                }
                Err(message) => this.notice = Some(format!("Compaction failed: {message}")),
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
                    this.notice = Some(format!("Could not set permission mode: {message}"));
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
        let mode = session.mode.clone();
        if mode == "auto" || mode == "smart_approve" {
            self.permission_mode = mode;
        }
        self.apply_permission_mode(cx);
        self.timeline = timeline;
        self.list_state.reset(self.timeline.len());
        self.follow_transcript = true;
        self.pending_permission = None;
        self.permission_responding = false;
        self.models_menu_open = false;
        self.root_menu_open = false;
        cx.notify();
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
            self.notice = Some("Create a task first".to_string());
            cx.notify();
            return;
        };
        if self.booting {
            self.notice = Some("Agent runtime is still starting".to_string());
            cx.notify();
            return;
        }
        if text.trim().is_empty() {
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
        let request = AgentSendMessageRequest {
            session_id: session_id.to_string(),
            text: text.clone(),
            model,
            context_limit: None,
            mode: Some(self.permission_mode.clone()),
            vision_capable: false,
            steer: false,
            queue_id: None,
            attachments: Vec::new(),
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
                    this.notice = Some(message);
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
                        this.notice = Some(format!("Permission response failed: {message}"));
                    }
                }
                cx.notify();
            },
        );
    }

    fn sign_out(&mut self, cx: &mut Context<Self>) {
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
        let incoming_merge = item.merge.clone();
        let incoming_created = item.created_ms;
        let incoming_type = item.item_type.clone();
        let incoming_role = item.role.clone();
        let incoming_title = item.title.clone();
        let incoming_text = item.text.clone();
        let incoming_status = item.status.clone();
        let incoming_input = item.input.clone();
        let incoming_output = item.output.clone();
        let existing = self
            .timeline
            .iter_mut()
            .find(|candidate| candidate.id == item.id);
        match existing {
            Some(existing) => {
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
    }

    /// Route one backend service event into UI state.
    pub fn handle_service_event(&mut self, event: AgentServiceEvent, cx: &mut Context<Self>) {
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
        cx.notify();
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
                    self.pending_permission = Some(PendingPermission {
                        session_id: session_id.to_string(),
                        run_id: run_id.to_string(),
                        request_id: request.request_id,
                        tool_name: request.tool_name,
                        prompt: request.prompt,
                        arguments: serde_json::Value::Object(request.arguments),
                    });
                    self.permission_responding = false;
                }
            }
            AgentRunEvent::SetupWarning(message) => {
                if self.is_selected(session_id) {
                    self.notice = Some(message);
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

impl Render for ChatScreen {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
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
                    .child(self.render_sidebar(cx))
                    .child(
                        div()
                            .flex()
                            .flex_col()
                            .flex_1()
                            .h_full()
                            .min_w_0()
                            .child(self.render_header(cx))
                            .child(self.render_transcript(window, cx))
                            .when_some(self.pending_question.clone(), |container, question| {
                                let input = self.pending_question_input.clone();
                                container.child(render_question_card(question, input, cx))
                            })
                            .when_some(self.pending_permission.clone(), |container, permission| {
                                container.child(render_permission_card(
                                    permission,
                                    self.permission_responding,
                                    cx,
                                ))
                            })
                            .child(self.render_composer(cx))
                            .children(self.render_menu_panel(cx)),
                    ),
            )
    }
}

impl ChatScreen {
    fn render_sidebar(&self, cx: &mut Context<Self>) -> Div {
        let sessions = self.sessions.clone();
        let selected = self.selected_session.clone();
        div()
            .w(gpui::px(260.))
            .h_full()
            .flex()
            .flex_col()
            .bg(gpui::rgb(theme::BG_SIDEBAR))
            .border_r_1()
            .border_color(gpui::rgb(theme::BORDER))
            .child(
                div()
                    .flex()
                    .items_center()
                    .justify_between()
                    .px_4()
                    .py_3()
                    .child(
                        div()
                            .font_weight(gpui::FontWeight::BOLD)
                            .text_color(gpui::rgb(theme::TEXT_PRIMARY))
                            .child("Maple"),
                    )
                    .child(
                        div()
                            .id("new-task")
                            .px_3()
                            .py_1()
                            .rounded_md()
                            .bg(gpui::rgb(theme::ACCENT))
                            .text_sm()
                            .text_color(gpui::rgb(theme::TEXT_PRIMARY))
                            .hover(|style| {
                                style.bg(gpui::rgb(theme::ACCENT_HOVER)).cursor_pointer()
                            })
                            .on_click(cx.listener(|this, _event, _window, cx| {
                                this.new_session(cx);
                            }))
                            .child("New task"),
                    ),
            )
            .child(
                div()
                    .id("session-list")
                    .flex_1()
                    .flex()
                    .flex_col()
                    .overflow_y_scroll()
                    .track_scroll(&self.sidebar_scroll)
                    .px_2()
                    .gap_1()
                    .children(sessions.iter().map(|session| {
                        let is_selected = selected.as_deref() == Some(session.id.as_str());
                        let session_id = session.id.clone();
                        div()
                            .id(gpui::SharedString::from(format!("session-{}", session.id)))
                            .px_2()
                            .py_2()
                            .rounded_md()
                            .bg(gpui::rgb(if is_selected {
                                theme::BG_ELEVATED
                            } else {
                                theme::BG_SIDEBAR
                            }))
                            .hover(|style| style.bg(gpui::rgb(theme::BG_ELEVATED)).cursor_pointer())
                            .on_click({
                                let session_id = session_id.clone();
                                cx.listener(move |this, _event, _window, cx| {
                                    this.select_session(&session_id, cx);
                                })
                            })
                            .child(
                                div()
                                    .text_sm()
                                    .text_color(gpui::rgb(theme::TEXT_PRIMARY))
                                    .line_clamp(1)
                                    .child(session.title.clone()),
                            )
                            .child(
                                div()
                                    .text_xs()
                                    .text_color(gpui::rgb(theme::TEXT_MUTED))
                                    .child(relative_time(session.updated_ms)),
                            )
                    })),
            )
            .child(
                div()
                    .flex()
                    .items_center()
                    .justify_between()
                    .gap_2()
                    .px_4()
                    .py_3()
                    .border_t_1()
                    .border_color(gpui::rgb(theme::BORDER))
                    .child(
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
                    )
                    .child(
                        div()
                            .id("open-settings")
                            .px_2()
                            .py_1()
                            .rounded_md()
                            .text_sm()
                            .text_color(gpui::rgb(theme::TEXT_MUTED))
                            .hover(|style| {
                                style
                                    .text_color(gpui::rgb(theme::TEXT_PRIMARY))
                                    .cursor_pointer()
                            })
                            .on_click(cx.listener(|_this, _event, _window, cx| {
                                cx.emit(OpenSettings);
                            }))
                            .child("⚙"),
                    ),
            )
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

    fn render_header(&self, cx: &mut Context<Self>) -> Div {
        let mut header = div()
            .flex()
            .items_center()
            .justify_between()
            .px_4()
            .py_2()
            .border_b_1()
            .border_color(gpui::rgb(theme::BORDER))
            .child(
                div()
                    .text_sm()
                    .text_color(gpui::rgb(theme::TEXT_SECONDARY))
                    .child(
                        self.sessions
                            .iter()
                            .find(|session| Some(session.id.clone()) == self.selected_session)
                            .map(|session| session.title.clone())
                            .unwrap_or_else(|| "New task".to_string()),
                    ),
            )
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap_2()
                    .child(
                        div()
                            .id("root-picker")
                            .flex()
                            .items_center()
                            .gap_1()
                            .px_2()
                            .py_1()
                            .rounded_md()
                            .border_1()
                            .border_color(gpui::rgb(theme::BORDER))
                            .text_sm()
                            .text_color(gpui::rgb(theme::TEXT_SECONDARY))
                            .hover(|style| style.cursor_pointer())
                            .on_click(cx.listener(|this, _event, _window, cx| {
                                this.models_menu_open = false;
                                this.root_menu_open = !this.root_menu_open;
                                cx.notify();
                            }))
                            .child(format!(
                                "📁 {}",
                                self.project_root
                                    .as_deref()
                                    .and_then(|path| {
                                        std::path::Path::new(path)
                                            .file_name()
                                            .map(|name| name.to_string_lossy().to_string())
                                    })
                                    .unwrap_or_else(|| "project".to_string())
                            )),
                    )
                    .child(
                        div()
                            .id("compact-now")
                            .flex()
                            .items_center()
                            .px_2()
                            .py_1()
                            .rounded_md()
                            .border_1()
                            .border_color(gpui::rgb(theme::BORDER))
                            .text_sm()
                            .text_color(gpui::rgb(theme::TEXT_SECONDARY))
                            .hover(|style| style.cursor_pointer())
                            .on_click(cx.listener(|this, _event, _window, cx| {
                                this.compact_now(cx);
                            }))
                            .child("⇲"),
                    )
                    .child(
                        div()
                            .id("sign-out")
                            .px_2()
                            .py_1()
                            .rounded_md()
                            .text_sm()
                            .text_color(gpui::rgb(theme::TEXT_MUTED))
                            .hover(|style| {
                                style
                                    .text_color(gpui::rgb(theme::STATUS_ERROR))
                                    .cursor_pointer()
                            })
                            .on_click(cx.listener(|this, _event, _window, cx| {
                                this.sign_out(cx);
                            }))
                            .child("Sign out"),
                    ),
            );
        header
    }

    /// The open header menu as an inline panel. Rendered in normal flow
    /// below the header; deferred/absolute anchoring proved unreliable.
    fn render_menu_panel(&self, cx: &mut Context<Self>) -> Option<Div> {
        let mut menu = div()
            .flex()
            .flex_col()
            .mx_4()
            .my_1()
            .py_1()
            .rounded_md()
            .bg(gpui::rgb(theme::BG_ELEVATED))
            .border_1()
            .border_color(gpui::rgb(theme::BORDER));
        if self.root_menu_open {
            for path in self.recent_roots.iter().take(6) {
                let path = path.clone();
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
                        .child(path),
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
                    .child("Choose folder…"),
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
                    "Full access",
                    "Approve every tool call without asking",
                ),
                ("smart_approve", "Ask first", "Confirm each gated tool call"),
            ] {
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
                            this.permission_mode = mode.clone();
                            this.uses_default_permission_mode = false;
                            this.mode_menu_open = false;
                            this.apply_permission_mode(cx);
                            cx.notify();
                        }))
                        .child(
                            div().flex().flex_col().gap_0().child(label).child(
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
        if self.models_menu_open {
            menu = menu.children(self.models.iter().map(|model| {
                let model = model.clone();
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
                    .child(model)
            }));
            return Some(menu);
        }
        None
    }

    fn render_transcript(
        &mut self,
        _window: &mut Window,
        _cx: &mut Context<Self>,
    ) -> impl IntoElement {
        // Follow the newest content while the view sits at (or near) the
        // bottom, or after an explicit jump. Checking before render keeps
        // user scrolls intact mid-stream.
        let offset = self.transcript_scroll.offset();
        let max = self.transcript_scroll.max_offset();
        let at_bottom = max.height <= gpui::px(1.) || offset.y <= -(max.height - gpui::px(48.));
        if at_bottom || self.follow_transcript {
            self.follow_transcript = false;
            self.transcript_scroll.scroll_to_bottom();
        }
        let empty = self.timeline.is_empty();
        let booting = self.booting;
        let items = self.timeline.clone();
        div()
            .id("transcript")
            .relative()
            .flex_1()
            .flex()
            .flex_col()
            .min_h_0()
            .px_6()
            .py_4()
            .overflow_y_scroll()
            .track_scroll(&self.transcript_scroll)
            .overflow_x_hidden()
            .when(empty, |container| {
                container.child(
                    div()
                        .flex()
                        .flex_1()
                        .min_h_0()
                        .justify_center()
                        .items_center()
                        .text_color(gpui::rgb(theme::TEXT_FAINT))
                        .child(if booting {
                            "Starting agent runtime…".to_string()
                        } else {
                            "Ask Maple anything about this project".to_string()
                        }),
                )
            })
            .when(!empty, |container| {
                // Standard chat order: oldest at the top, newest at the
                // bottom; the container follows the bottom while the user
                // stays there (see the pinning logic above).
                let tool_details = self.tool_details;
                container.children(
                    items
                        .iter()
                        .map(move |item| render_timeline_item(item, tool_details)),
                )
            })
            .child(self.render_scrollbar())
            .when_some(self.runtime_error.clone(), |container, error| {
                container.child(
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
            .when_some(self.notice.clone(), |container, notice| {
                container.child(
                    div()
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

    /// Thin scrollbar overlay driven by the transcript scroll handle.
    fn render_scrollbar(&self) -> impl IntoElement {
        let handle = &self.transcript_scroll;
        let bounds = handle.bounds();
        let max = handle.max_offset();
        let track_height = bounds.size.height;
        if max.height <= gpui::px(1.) || track_height <= gpui::px(0.) {
            return div().opacity(0.);
        }
        let content = max.height + track_height;
        // ratio of visible track to total content
        let ratio = track_height / content;
        let thumb_height = (track_height * ratio).max(gpui::px(24.));
        let offset = -handle.offset().y;
        let scrollable = (track_height - thumb_height).max(gpui::px(0.));
        let progress = offset / max.height;
        let thumb_top = scrollable * progress;
        div()
            .absolute()
            .top(gpui::px(0.))
            .right(gpui::px(2.))
            .bottom(gpui::px(0.))
            .w(gpui::px(6.))
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
        let can_send = !disabled && !running && has_text;
        let composer = self.composer.clone();
        let model_label = self
            .selected_model
            .clone()
            .unwrap_or_else(|| "default model".to_string());
        let bypass = self.permission_mode == "auto";
        div()
            .m_4()
            .flex()
            .flex_col()
            .gap_2()
            .px_3()
            .py_2()
            .rounded_lg()
            .bg(gpui::rgb(theme::BG_INPUT))
            .border_1()
            .border_color(gpui::rgb(theme::BORDER))
            .when(disabled, |container| container.opacity(0.5))
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap_2()
                    .child(
                        div()
                            .flex_1()
                            .text_color(gpui::rgb(theme::TEXT_PRIMARY))
                            .children(composer),
                    )
                    .child(div().id("context-indicator").flex().items_center().child(
                        crate::ui::context_ring::ContextRing::new(
                            self.context_fraction.unwrap_or(0.0),
                        ),
                    ))
                    .child(if running {
                        div()
                            .id("stop-run")
                            .px_4()
                            .py_2()
                            .rounded_md()
                            .bg(gpui::rgb(theme::STATUS_ERROR))
                            .text_sm()
                            .text_color(gpui::rgb(theme::BG_APP))
                            .hover(|style| style.cursor_pointer())
                            .on_click(cx.listener(|this, _event, _window, cx| {
                                this.stop(cx);
                            }))
                            .child("Stop")
                    } else {
                        div()
                            .id("send-message")
                            .px_4()
                            .py_2()
                            .rounded_md()
                            .bg(gpui::rgb(if can_send {
                                theme::ACCENT
                            } else {
                                theme::BORDER
                            }))
                            .text_sm()
                            .text_color(gpui::rgb(theme::TEXT_PRIMARY))
                            .when(can_send, |el| {
                                el.hover(|style| {
                                    style.bg(gpui::rgb(theme::ACCENT_HOVER)).cursor_pointer()
                                })
                            })
                            .when(can_send, |el| {
                                el.on_click(cx.listener(|this, _event, _window, cx| {
                                    this.send_inner(cx);
                                }))
                            })
                            .child(if disabled {
                                "Starting…".to_string()
                            } else {
                                "Send".to_string()
                            })
                    }),
            )
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap_2()
                    .child(
                        div()
                            .id("model-picker")
                            .flex()
                            .items_center()
                            .gap_1()
                            .px_2()
                            .py_1()
                            .rounded_md()
                            .border_1()
                            .border_color(gpui::rgb(if self.models_menu_open {
                                theme::ACCENT
                            } else {
                                theme::BORDER
                            }))
                            .text_sm()
                            .text_color(gpui::rgb(theme::TEXT_SECONDARY))
                            .hover(|style| style.cursor_pointer())
                            .on_click(cx.listener(|this, _event, _window, cx| {
                                this.root_menu_open = false;
                                this.mode_menu_open = false;
                                this.models_menu_open = !this.models_menu_open;
                                cx.notify();
                            }))
                            .child(model_label)
                            .child("▾"),
                    )
                    .child(
                        div()
                            .id("permission-mode-toggle")
                            .flex()
                            .items_center()
                            .gap_1()
                            .px_2()
                            .py_1()
                            .rounded_md()
                            .border_1()
                            .border_color(gpui::rgb(if bypass {
                                theme::STATUS_WARNING
                            } else {
                                theme::BORDER
                            }))
                            .text_sm()
                            .text_color(gpui::rgb(if bypass {
                                theme::STATUS_WARNING
                            } else {
                                theme::TEXT_SECONDARY
                            }))
                            .hover(|style| style.cursor_pointer())
                            .on_click(cx.listener(|this, _event, _window, cx| {
                                this.root_menu_open = false;
                                this.models_menu_open = false;
                                this.mode_menu_open = !this.mode_menu_open;
                                cx.notify();
                            }))
                            .child(if bypass {
                                "🛡 Full access".to_string()
                            } else {
                                "🛡 Ask first".to_string()
                            })
                            .child("▾"),
                    ),
            )
    }
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

fn render_timeline_item(item: &AgentTimelineItem, tool_details: bool) -> Div {
    let item = match item.item_type.as_str() {
        "message" => render_message(item),
        "thinking" | "reasoning" => render_thinking(item),
        "tool" | "toolCall" => {
            // Dispatch on payload shape; runtime titles are humanized
            // ("todo write", "ask user") and vary by detail suffix.
            if has_tool_input(item, "todos") {
                render_todo(item)
            } else if has_tool_input(item, "edits")
                || (has_tool_input(item, "content") && has_tool_input(item, "path"))
            {
                render_tool_with_diff(item, tool_details)
            } else {
                render_tool(item, tool_details)
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

fn render_message(item: &AgentTimelineItem) -> Div {
    let is_user = item.role.as_deref() == Some("user");
    let text = item.text.clone().unwrap_or_default();
    if text.trim().is_empty() {
        return div();
    }
    if is_user {
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
                .child(text),
        )
    } else {
        div()
            .max_w_full()
            .pr_2()
            .text_color(gpui::rgb(theme::TEXT_PRIMARY))
            .child(markdown::render_markdown(&text))
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
fn render_tool_with_diff(item: &AgentTimelineItem, details: bool) -> Div {
    let card = render_tool(item, details);
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

fn render_tool(item: &AgentTimelineItem, details: bool) -> Div {
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
                .child(crate::ui::markdown::render_markdown(&output)),
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
    question: crate::backend::PendingQuestion,
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
    permission: PendingPermission,
    responding: bool,
    cx: &mut Context<ChatScreen>,
) -> Div {
    let description = permission
        .prompt
        .clone()
        .unwrap_or_else(|| format!("Run tool {}?", permission.tool_name));
    let arguments = if permission.arguments.is_null() {
        String::new()
    } else {
        serde_json::to_string_pretty(&permission.arguments).unwrap_or_default()
    };
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

fn relative_time(when_ms: i64) -> String {
    let now_ms = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|elapsed| elapsed.as_millis() as i64)
        .unwrap_or(0);
    relative_time_at(when_ms, now_ms)
}

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

    fn screen(cx: &mut TestAppContext) -> Entity<ChatScreen> {
        let backend = std::sync::Arc::new(
            crate::backend::AgentBackend::new("http://127.0.0.1:9".to_string()).expect("backend"),
        );
        cx.new(|_cx| {
            let mut screen = ChatScreen::new_inner(backend, "user".to_string());
            screen.selected_session = Some("s1".to_string());
            screen
        })
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
}
