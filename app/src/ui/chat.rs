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

use crate::backend::{AgentBackend, PendingPermission};
use crate::ui::markdown;
use crate::ui::text_input::TextInput;
use crate::ui::theme;

pub struct LoggedOut;

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
    composer: Entity<TextInput>,
    models: Vec<String>,
    selected_model: Option<String>,
    models_menu_open: bool,
    runtime_error: Option<String>,
    notice: Option<String>,
    booting: bool,
    /// Virtualized transcript state; bottom-aligned like a chat log.
    list_state: gpui::ListState,
    /// Guards against a slow session load overwriting a newer selection.
    selection_generation: u64,
    /// Per-session count of applied timeline events; a load whose snapshot
    /// predates newer events is discarded instead of clobbering them.
    timeline_revisions: HashMap<String, u64>,
}

impl ChatScreen {
    pub fn new(backend: Arc<AgentBackend>, user_id: String, cx: &mut Context<Self>) -> Self {
        let weak = cx.entity().downgrade();
        let composer = cx.new(|cx| {
            TextInput::new("Message Maple…", cx)
                .clears_on_enter()
                .on_enter(move |text, _, cx| {
                    // The handler receives the composer text directly; clearing
                    // and sending happen without leasing the focused input.
                    if let Some(this) = weak.upgrade() {
                        this.update(cx, |chat, cx| chat.send_text(text, cx));
                    }
                })
        });
        let this = Self {
            backend,
            user_id,
            sessions: Vec::new(),
            selected_session: None,
            timeline: Vec::new(),
            active_runs: HashMap::new(),
            pending_permission: None,
            permission_responding: false,
            composer,
            models: Vec::new(),
            selected_model: None,
            models_menu_open: false,
            runtime_error: None,
            notice: None,
            booting: true,
            list_state: gpui::ListState::new(0, gpui::ListAlignment::Bottom, px(400.)),
            selection_generation: 0,
            timeline_revisions: HashMap::new(),
        };
        this.start(cx);
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
                    Ok(_) => this.runtime_error = None,
                    Err(message) => {
                        this.runtime_error = Some(format!("Runtime failed to start: {message}"))
                    }
                }
                this.booting = false;
                cx.notify();
                this.refresh_models(cx);
                this.refresh_sessions(cx);
            },
        );
    }

    fn refresh_models(&self, cx: &mut Context<Self>) {
        let backend = self.backend.clone();
        let user_id = self.user_id.clone();
        self.call(
            async move { backend.available_model_ids(&user_id).await },
            cx,
            |this, result, cx| {
                if let Ok(models) = result {
                    if this.selected_model.is_none() {
                        this.selected_model = models.first().cloned();
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
        self.call(
            async move { backend.list_sessions(&user_id, None).await },
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

    fn new_session(&self, cx: &mut Context<Self>) {
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
                    this.sessions.insert(0, session.clone());
                    let timeline = Vec::new();
                    this.set_active_session(session, timeline, cx);
                }
                Err(message) => this.notice = Some(message),
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

    fn set_active_session(
        &mut self,
        session: AgentSessionSummary,
        timeline: Vec<AgentTimelineItem>,
        cx: &mut Context<Self>,
    ) {
        self.selected_session = Some(session.id);
        self.timeline = timeline;
        self.list_state.reset(self.timeline.len());
        self.pending_permission = None;
        self.permission_responding = false;
        self.models_menu_open = false;
        cx.notify();
    }

    fn is_run_active(&self) -> bool {
        self.selected_session
            .as_ref()
            .is_some_and(|session| self.active_runs.contains_key(session))
    }

    /// Send without a Window, callable from the composer's Enter hook and
    /// from the Send button.
    /// Send without a Window, callable from the Send button.
    fn send_inner(&mut self, cx: &mut Context<Self>) {
        let text = self.composer.read(cx).text();
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
            mode: None,
            vision_capable: false,
            steer: false,
            queue_id: None,
            attachments: Vec::new(),
        };
        self.composer.update(cx, |input, cx| input.clear(cx));
        self.notice = None;
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
                    this.composer
                        .update(cx, |input, cx| input.set_text(&text, cx));
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
                backend.logout(&user_id).await
            },
            cx,
            |_this, _result, cx| {
                cx.emit(LoggedOut);
            },
        );
    }

    fn pick_model(&mut self, model: String, cx: &mut Context<Self>) {
        self.selected_model = Some(model);
        self.models_menu_open = false;
        cx.notify();
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

    fn update_session_summary(&mut self, session: AgentSessionSummary) {
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
                self.update_session_summary(session);
            }
            AgentServiceEvent::SessionUpdated { session, .. } => {
                self.update_session_summary(session);
            }
            AgentServiceEvent::TimelineItem {
                session_id, item, ..
            } => {
                if self.selected_session.as_deref() == Some(session_id.as_str()) {
                    self.apply_timeline_item(&session_id, item);
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
                self.update_session_summary(session);
            }
            AgentRunEvent::Started => {
                self.active_runs
                    .insert(session_id.to_string(), run_id.to_string());
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
            }
            AgentRunEvent::QueueChanged(_) | AgentRunEvent::QueuePromoted { .. } => {
                // Queue chips are rendered from send responses; nothing to do
                // until queue editing is exposed in the UI.
            }
        }
    }
}

impl EventEmitter<LoggedOut> for ChatScreen {}

impl Render for ChatScreen {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        div()
            .size_full()
            .flex()
            .flex_row()
            .bg(gpui::rgb(theme::BG_APP))
            .child(self.render_sidebar(cx))
            .child(
                div()
                    .flex()
                    .flex_col()
                    .flex_1()
                    .h_full()
                    .min_w_0()
                    .child(self.render_header(cx))
                    .child(self.render_transcript(cx))
                    .when_some(self.pending_permission.clone(), |container, permission| {
                        container.child(render_permission_card(
                            permission,
                            self.permission_responding,
                            cx,
                        ))
                    })
                    .child(self.render_composer(cx)),
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
    }

    fn render_header(&self, cx: &mut Context<Self>) -> Div {
        let model_label = self
            .selected_model
            .clone()
            .unwrap_or_else(|| "default model".to_string());
        let models = self.models.clone();
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
                            .id("model-picker")
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
                                this.models_menu_open = !this.models_menu_open;
                                cx.notify();
                            }))
                            .child(model_label),
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
        if self.models_menu_open {
            let menu = div()
                .occlude()
                .flex()
                .flex_col()
                .mt_6()
                .py_1()
                .rounded_md()
                .bg(gpui::rgb(theme::BG_ELEVATED))
                .border_1()
                .border_color(gpui::rgb(theme::BORDER))
                .shadow_md()
                .children(models.iter().map(|model| {
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
            header = header.child(gpui::deferred(
                div()
                    .absolute()
                    .top(gpui::px(40.))
                    .right(gpui::px(16.))
                    .child(menu),
            ));
        }
        header
    }

    fn render_transcript(&self, _cx: &mut Context<Self>) -> impl IntoElement {
        let empty = self.timeline.is_empty();
        let booting = self.booting;
        let items = self.timeline.clone();
        let list_state = self.list_state.clone();
        div()
            .id("transcript")
            .flex_1()
            .flex()
            .flex_col()
            .px_6()
            .py_4()
            .child(
                div()
                    .flex_1()
                    .min_h_0()
                    .when(empty, |container| {
                        container.child(
                            div()
                                .flex()
                                .h_full()
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
                        container.child(gpui::list(list_state, move |index, _window, _cx| {
                            items
                                .get(index)
                                .map(|item| render_timeline_item(item).into_any_element())
                                .unwrap_or_else(|| div().into_any_element())
                        }))
                    }),
            )
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

    fn render_composer(&mut self, cx: &mut Context<Self>) -> Div {
        let running = self.is_run_active();
        let disabled = self.booting;
        let has_text = !self.composer.read(cx).text().trim().is_empty();
        let can_send = !disabled && !running && has_text;
        let composer = self.composer.clone();
        div()
            .flex()
            .items_center()
            .gap_2()
            .m_4()
            .px_3()
            .py_2()
            .rounded_lg()
            .bg(gpui::rgb(theme::BG_INPUT))
            .border_1()
            .border_color(gpui::rgb(theme::BORDER))
            .when(disabled, |container| container.opacity(0.5))
            .child(
                div()
                    .flex_1()
                    .text_color(gpui::rgb(theme::TEXT_PRIMARY))
                    .child(composer),
            )
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
                        el.hover(|style| style.bg(gpui::rgb(theme::ACCENT_HOVER)).cursor_pointer())
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
            })
    }
}

fn render_timeline_item(item: &AgentTimelineItem) -> Div {
    let item = match item.item_type.as_str() {
        "message" => render_message(item),
        "thinking" | "reasoning" => render_thinking(item),
        "tool" | "toolCall" => render_tool(item),
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

fn render_tool(item: &AgentTimelineItem) -> Div {
    let (label, status_color) = tool_status_style(item.status.as_deref());
    let title = item.title.clone().unwrap_or_else(|| item.item_type.clone());
    let summary = tool_summary(item);
    div()
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
        )
        .children(summary.map(|line| {
            div()
                .text_xs()
                .text_color(gpui::rgb(theme::TEXT_MUTED))
                .font_family("monospace")
                .line_clamp(3)
                .child(line)
        }))
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

fn tool_summary(item: &AgentTimelineItem) -> Option<String> {
    let input = item
        .input
        .as_ref()
        .filter(|value| !value.is_null())
        .map(|value| format!("input: {value}"));
    let output = item
        .output
        .as_ref()
        .filter(|value| !value.is_null())
        .map(|value| format!("output: {value}"));
    match (input, output) {
        (Some(input), Some(output)) => Some(format!("{input}\n{output}")),
        (Some(input), None) => Some(input),
        (None, Some(output)) => Some(output),
        (None, None) => None,
    }
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

fn relative_time(when: i64) -> String {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|elapsed| elapsed.as_millis() as i64)
        .unwrap_or(0);
    let delta = (now - when).max(0);
    match delta {
        seconds if seconds < 60 => "just now".to_string(),
        seconds if seconds < 3600 => format!("{}m ago", seconds / 60),
        seconds if seconds < 86_400 => format!("{}h ago", seconds / 3600),
        seconds => format!("{}d ago", seconds / 86_400),
    }
}

#[allow(dead_code)]
fn _unused_any_element(assertion: AnyElement) -> AnyElement {
    assertion
}
