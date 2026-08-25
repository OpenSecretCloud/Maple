//! The agent chat surface: session sidebar, streaming transcript with tool
//! calls, permission prompts, and the composer. Pure consumer of the backend
//! facade + event stream.

use std::sync::Arc;

use gpui::{AppContext, Context, Div, Entity, Render, ScrollHandle, Window, div, prelude::*};

use maple_agent::agent::{
    AgentSendMessageRequest, AgentServiceEvent, AgentSessionSummary, AgentTimelineItem,
};

use crate::backend::{AgentBackend, PendingPermission};
use crate::ui::markdown;
use crate::ui::text_input::TextInput;
use crate::ui::theme;

pub struct ChatScreen {
    backend: Arc<AgentBackend>,
    user_id: String,
    sessions: Vec<AgentSessionSummary>,
    selected_session: Option<String>,
    timeline: Vec<AgentTimelineItem>,
    active_run: Option<String>,
    pending_permission: Option<PendingPermission>,
    composer: Entity<TextInput>,
    models: Vec<String>,
    selected_model: Option<String>,
    models_menu_open: bool,
    notice: Option<String>,
    scroll: ScrollHandle,
    booting: bool,
}

impl ChatScreen {
    pub fn new(backend: Arc<AgentBackend>, user_id: String, cx: &mut Context<Self>) -> Self {
        let composer = cx.new(|cx| TextInput::new("Message Maple…", cx));
        let this = Self {
            backend,
            user_id,
            sessions: Vec::new(),
            selected_session: None,
            timeline: Vec::new(),
            active_run: None,
            pending_permission: None,
            composer,
            models: Vec::new(),
            selected_model: None,
            models_menu_open: false,
            notice: None,
            scroll: ScrollHandle::new(),
            booting: true,
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
            let result = task.await.unwrap_or_else(|error| Err(format!("{error}")));
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
                    Ok(_) => this.notice = None,
                    Err(message) => {
                        this.notice = Some(format!("Runtime failed to start: {message}"))
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
                                this.select_session(&latest.id, cx);
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
                    this.set_active_session(session, Vec::new(), cx);
                }
                Err(message) => this.notice = Some(message),
            },
        );
    }

    fn select_session(&self, session_id: &str, cx: &mut Context<Self>) {
        let backend = self.backend.clone();
        let user_id = self.user_id.clone();
        let session_id = session_id.to_string();
        self.call(
            async move { backend.load_session(&user_id, &session_id).await },
            cx,
            |this, result, cx| match result {
                Ok(detail) => {
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
        self.active_run = None;
        self.pending_permission = None;
        self.scroll.scroll_to_bottom();
        cx.notify();
    }

    fn send(&mut self, _event: &gpui::ClickEvent, _window: &mut Window, cx: &mut Context<Self>) {
        let Some(session_id) = self.selected_session.clone() else {
            return;
        };
        let text = self.composer.read(cx).text();
        if text.trim().is_empty() || self.booting {
            return;
        }
        let backend = self.backend.clone();
        let user_id = self.user_id.clone();
        let model = self.selected_model.clone();
        let request = AgentSendMessageRequest {
            session_id,
            text,
            model,
            context_limit: None,
            mode: None,
            vision_capable: false,
            steer: false,
            queue_id: None,
            attachments: Vec::new(),
        };
        self.composer.update(cx, |input, cx| input.clear(cx));
        cx.notify();
        self.call(
            async move { backend.send_message(&user_id, request).await },
            cx,
            |this, result, cx| {
                if let Ok(run_id) = result {
                    this.active_run = Some(run_id);
                }
                cx.notify();
            },
        );
    }

    fn stop(&mut self, _event: &gpui::ClickEvent, _window: &mut Window, cx: &mut Context<Self>) {
        let Some(run_id) = self.active_run.clone() else {
            return;
        };
        let backend = self.backend.clone();
        let user_id = self.user_id.clone();
        self.call(
            async move { backend.cancel_run(&user_id, &run_id).await },
            cx,
            |_this, _result, cx| {
                cx.notify();
            },
        );
    }

    fn respond_permission(&mut self, allow: bool, cx: &mut Context<Self>) {
        let Some(permission) = self.pending_permission.clone() else {
            return;
        };
        let backend = self.backend.clone();
        let user_id = self.user_id.clone();
        self.pending_permission = None;
        cx.notify();
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
            |_this, _result, cx| {
                cx.notify();
            },
        );
    }

    fn pick_model(&mut self, model: String, cx: &mut Context<Self>) {
        self.selected_model = Some(model);
        self.models_menu_open = false;
        cx.notify();
    }

    /// Apply a timeline item using Maple's merge contract: `append` extends
    /// message/thinking text on the item with the same id; otherwise replace
    /// or insert in order.
    fn apply_timeline_item(&mut self, item: AgentTimelineItem) {
        let existing = self
            .timeline
            .iter_mut()
            .find(|candidate| candidate.id == item.id);
        match existing {
            Some(existing) => {
                let append = item.merge == "append"
                    && matches!(item.item_type.as_str(), "message" | "thinking")
                    && item.text.is_some();
                if append {
                    let incoming = item.text.clone().unwrap_or_default();
                    let text = existing.text.get_or_insert_with(String::new);
                    text.push_str(&incoming);
                } else {
                    *existing = item;
                }
            }
            None => self.timeline.push(item),
        }
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
                if !status.running {
                    self.notice = Some("Agent runtime stopped".to_string());
                } else {
                    self.notice = None;
                }
            }
            AgentServiceEvent::SessionCreated(session) => {
                self.update_session_summary(session);
            }
            AgentServiceEvent::SessionUpdated { session, .. } => {
                self.update_session_summary(session);
            }
            AgentServiceEvent::TimelineItem { item, .. } => {
                self.apply_timeline_item(item);
                self.scroll.scroll_to_bottom();
            }
            AgentServiceEvent::Run {
                session_id,
                run_id,
                event,
            } => self.handle_run_event(&session_id, &run_id, event, cx),
        }
        cx.notify();
    }

    fn handle_run_event(
        &mut self,
        session_id: &str,
        run_id: &str,
        event: maple_agent::agent::AgentRunEvent,
        cx: &mut Context<Self>,
    ) {
        use maple_agent::agent::AgentRunEvent;
        if self.selected_session.as_deref() != Some(session_id) {
            // Events for background sessions still update their summary.
            if let AgentRunEvent::SessionUpdated(session) = &event {
                self.update_session_summary(session.clone());
            }
            return;
        }
        match event {
            AgentRunEvent::SessionUpdated(session) => {
                self.update_session_summary(session);
            }
            AgentRunEvent::Started => {
                self.active_run = Some(run_id.to_string());
            }
            AgentRunEvent::TimelineItem(item) => {
                self.apply_timeline_item(item);
                self.scroll.scroll_to_bottom();
            }
            AgentRunEvent::PermissionRequested { request, .. } => {
                self.pending_permission = Some(PendingPermission {
                    session_id: session_id.to_string(),
                    run_id: run_id.to_string(),
                    request_id: request.request_id,
                    tool_name: request.tool_name,
                    prompt: request.prompt,
                    arguments: serde_json::Value::Object(request.arguments),
                });
            }
            AgentRunEvent::SetupWarning(message) => {
                self.notice = Some(message);
            }
            AgentRunEvent::HistoryReplaced => {
                let session_id = session_id.to_string();
                self.select_session(&session_id, cx);
            }
            AgentRunEvent::Error(item) => {
                self.apply_timeline_item(item);
            }
            AgentRunEvent::Finished(_) => {
                self.active_run = None;
                self.pending_permission = None;
            }
            AgentRunEvent::QueueChanged(_) | AgentRunEvent::QueuePromoted { .. } => {
                // Queue chips are rendered from send responses; nothing to do
                // until queue editing is exposed in the UI.
            }
        }
    }
}

impl Render for ChatScreen {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
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
                    .child(self.render_transcript(window, cx))
                    .when_some(self.pending_permission.clone(), |container, permission| {
                        container.child(render_permission_card(permission, cx))
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
                    .id("model-picker")
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
                        this.models_menu_open = !this.models_menu_open;
                        cx.notify();
                    }))
                    .child(model_label),
            );
        if self.models_menu_open {
            header = header.child(
                div()
                    .absolute()
                    .flex()
                    .flex_col()
                    .mt_6()
                    .right_4()
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
                    })),
            );
        }
        header
    }

    fn render_transcript(
        &self,
        window: &mut Window,
        _cx: &mut Context<Self>,
    ) -> gpui::Stateful<Div> {
        div()
            .id("transcript")
            .flex_1()
            .flex()
            .flex_col()
            .overflow_y_scroll()
            .track_scroll(&self.scroll)
            .px_6()
            .py_4()
            .gap_3()
            .when(self.timeline.is_empty(), |container| {
                container.child(
                    div()
                        .flex_1()
                        .flex()
                        .justify_center()
                        .items_center()
                        .text_color(gpui::rgb(theme::TEXT_FAINT))
                        .child(if self.booting {
                            "Starting agent runtime…".to_string()
                        } else {
                            "Ask Maple anything about this project".to_string()
                        }),
                )
            })
            .children(
                self.timeline
                    .iter()
                    .map(|item| render_timeline_item(item, window)),
            )
            .when_some(self.notice.clone(), |container, notice| {
                container.child(
                    div()
                        .px_3()
                        .py_2()
                        .rounded_md()
                        .bg(gpui::rgb(theme::STATUS_ERROR))
                        .text_color(gpui::rgb(theme::BG_APP))
                        .text_sm()
                        .child(notice),
                )
            })
    }

    fn render_composer(&self, cx: &mut Context<Self>) -> Div {
        let running = self.active_run.is_some();
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
                    .on_click(cx.listener(Self::stop))
                    .child("Stop")
            } else {
                div()
                    .id("send-message")
                    .px_4()
                    .py_2()
                    .rounded_md()
                    .bg(gpui::rgb(theme::ACCENT))
                    .text_sm()
                    .text_color(gpui::rgb(theme::TEXT_PRIMARY))
                    .hover(|style| style.bg(gpui::rgb(theme::ACCENT_HOVER)).cursor_pointer())
                    .on_click(cx.listener(Self::send))
                    .child("Send")
            })
    }
}

fn render_timeline_item(item: &AgentTimelineItem, window: &Window) -> Div {
    match item.item_type.as_str() {
        "message" => {
            let is_user = item.role.as_deref() == Some("user");
            let text = item.text.clone().unwrap_or_default();
            if is_user {
                div().flex().justify_end().child(
                    div()
                        .max_w(gpui::relative(0.75))
                        .px_4()
                        .py_2()
                        .rounded_lg()
                        .bg(gpui::rgb(theme::BG_USER_BUBBLE))
                        .text_color(gpui::rgb(theme::TEXT_PRIMARY))
                        .child(text),
                )
            } else {
                div()
                    .max_w_full()
                    .text_color(gpui::rgb(theme::TEXT_PRIMARY))
                    .child(markdown::render_markdown(&text, window))
            }
        }
        "thinking" | "reasoning" => {
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
        "tool" | "toolCall" => {
            let (label, status_color) = match item.status.as_deref() {
                Some("completed") => ("completed", theme::STATUS_SUCCESS),
                Some("failed") => ("failed", theme::STATUS_ERROR),
                _ => ("running", theme::STATUS_RUNNING),
            };
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
        "error" => div()
            .px_3()
            .py_2()
            .rounded_md()
            .bg(gpui::rgb(theme::STATUS_ERROR))
            .text_color(gpui::rgb(theme::BG_APP))
            .text_sm()
            .child(
                item.text
                    .clone()
                    .or_else(|| item.title.clone())
                    .unwrap_or_default(),
            ),
        "system" | _ => {
            let text = item
                .text
                .clone()
                .or_else(|| item.title.clone())
                .unwrap_or_default();
            if text.trim().is_empty() {
                div()
            } else {
                div()
                    .text_sm()
                    .text_color(gpui::rgb(theme::TEXT_MUTED))
                    .child(text)
            }
        }
    }
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

fn render_permission_card(permission: PendingPermission, cx: &mut Context<ChatScreen>) -> Div {
    let description = permission
        .prompt
        .clone()
        .unwrap_or_else(|| format!("Run tool {}?", permission.tool_name));
    let arguments = if permission.arguments.is_null() {
        String::new()
    } else {
        serde_json::to_string_pretty(&permission.arguments).unwrap_or_default()
    };
    div()
        .m_4()
        .px_4()
        .py_3()
        .rounded_lg()
        .bg(gpui::rgb(theme::BG_ELEVATED))
        .border_1()
        .border_color(gpui::rgb(theme::STATUS_WARNING))
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
        )
        .when(!arguments.is_empty(), |container| {
            container.child(
                div()
                    .text_xs()
                    .text_color(gpui::rgb(theme::TEXT_MUTED))
                    .font_family("monospace")
                    .max_h(gpui::px(120.))
                    .overflow_hidden()
                    .child(arguments),
            )
        })
        .child(
            div()
                .flex()
                .gap_2()
                .child(
                    div()
                        .id("permission-allow")
                        .px_4()
                        .py_1()
                        .rounded_md()
                        .bg(gpui::rgb(theme::STATUS_SUCCESS))
                        .text_sm()
                        .text_color(gpui::rgb(theme::BG_APP))
                        .hover(|style| style.cursor_pointer())
                        .on_click(cx.listener(move |this, _event, _window, cx| {
                            this.respond_permission(true, cx);
                        }))
                        .child("Allow once"),
                )
                .child(
                    div()
                        .id("permission-deny")
                        .px_4()
                        .py_1()
                        .rounded_md()
                        .bg(gpui::rgb(theme::STATUS_ERROR))
                        .text_sm()
                        .text_color(gpui::rgb(theme::BG_APP))
                        .hover(|style| style.cursor_pointer())
                        .on_click(cx.listener(move |this, _event, _window, cx| {
                            this.respond_permission(false, cx);
                        }))
                        .child("Deny"),
                ),
        )
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
