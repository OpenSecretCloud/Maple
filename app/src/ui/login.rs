//! Sign-in screen: email and password against the OpenSecret backend.

use std::sync::Arc;

use gpui::{AppContext, Context, Div, Entity, EventEmitter, Render, Window, div, prelude::*};

use crate::backend::AgentBackend;
use crate::ui::text_input::TextInput;
use crate::ui::theme;

/// Emitted after the backend validated the credentials. Only the account id
/// crosses the event boundary; the token snapshot stays inside the backend.
pub struct LoginSucceeded(pub String);

pub struct LoginScreen {
    backend: Arc<AgentBackend>,
    email_input: Entity<TextInput>,
    password_input: Entity<TextInput>,
    error: Option<String>,
    busy: bool,
}

impl LoginScreen {
    pub fn new(backend: Arc<AgentBackend>, cx: &mut Context<Self>) -> Self {
        let email = cx.new(|cx| TextInput::new("Email", cx));
        let password = cx.new(|cx| TextInput::new("Password", cx).masked());
        // Enter handlers receive their own field's text and read the sibling
        // through its entity; neither path leases the focused input.
        let (email_handle, password_handle) = (email.clone(), password.clone());
        let weak = cx.entity().downgrade();
        let email_weak = weak.clone();
        let password_weak = weak.clone();
        email.update(cx, |input, _| {
            input.set_on_enter(move |email_text, _, cx| {
                let password_text = password_handle.read(cx).text();
                if let Some(this) = email_weak.upgrade() {
                    this.update(cx, |screen, cx| {
                        screen.submit_values(email_text, password_text, cx)
                    });
                }
            });
        });
        password.update(cx, |input, _| {
            input.set_on_enter(move |password_text, _, cx| {
                let email_text = email_handle.read(cx).text();
                if let Some(this) = password_weak.upgrade() {
                    this.update(cx, |screen, cx| {
                        screen.submit_values(email_text, password_text, cx)
                    });
                }
            });
        });
        Self {
            backend,
            email_input: email,
            password_input: password,
            error: None,
            busy: false,
        }
    }

    fn submit_values(&mut self, email: String, password: String, cx: &mut Context<Self>) {
        if self.busy {
            return;
        }
        if email.trim().is_empty() || password.is_empty() {
            self.error = Some("Enter your email and password".to_string());
            cx.notify();
            return;
        }
        self.busy = true;
        self.error = None;
        cx.notify();
        let (spawn_backend, login_backend) = (self.backend.clone(), self.backend.clone());
        let task = spawn_backend.spawn(async move { login_backend.login(email, password).await });
        cx.spawn(async move |this, cx| {
            let result = task.await.unwrap_or_else(|error| {
                log::debug!("login task failed: {error:?}");
                Err("Sign in failed. Try again.".to_string())
            });
            this.update(cx, |this, cx| {
                this.busy = false;
                match result {
                    Ok(session) => cx.emit(LoginSucceeded(session.user_id)),
                    // The backend already sanitizes its own error strings;
                    // anything unexpected still gets a fixed message.
                    Err(message) => this.error = Some(message),
                }
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    fn submit(&mut self, cx: &mut Context<Self>) {
        let email = self.email_input.read(cx).text();
        let password = self.password_input.read(cx).text();
        self.submit_values(email, password, cx);
    }

    fn submit_clicked(
        &mut self,
        _event: &gpui::ClickEvent,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.submit(cx);
    }
}

impl EventEmitter<LoginSucceeded> for LoginScreen {}

impl Render for LoginScreen {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let busy = self.busy;
        div()
            .size_full()
            .flex()
            .justify_center()
            .items_center()
            .bg(gpui::rgb(theme::BG_APP))
            .child(
                div()
                    .flex()
                    .flex_col()
                    .gap_3()
                    .w(gpui::px(360.))
                    .p_6()
                    .rounded_lg()
                    .bg(gpui::rgb(theme::BG_ELEVATED))
                    .border_1()
                    .border_color(gpui::rgb(theme::BORDER))
                    .when(busy, |container| container.opacity(0.7))
                    .child(
                        div()
                            .text_xl()
                            .font_weight(gpui::FontWeight::BOLD)
                            .text_color(gpui::rgb(theme::TEXT_PRIMARY))
                            .child("Maple"),
                    )
                    .child(
                        div()
                            .text_sm()
                            .text_color(gpui::rgb(theme::TEXT_SECONDARY))
                            .child(format!("Sign in to {}", self.backend.api_url())),
                    )
                    .child(field("Email", self.email_input.clone()))
                    .child(field("Password", self.password_input.clone()))
                    .when_some(self.error.clone(), |container, message| {
                        container.child(
                            div()
                                .text_sm()
                                .text_color(gpui::rgb(theme::STATUS_ERROR))
                                .child(message),
                        )
                    })
                    .child(
                        div()
                            .id("login-submit")
                            .flex()
                            .justify_center()
                            .py_2()
                            .rounded_md()
                            .bg(gpui::rgb(if busy { theme::BORDER } else { theme::ACCENT }))
                            .text_color(gpui::rgb(theme::TEXT_PRIMARY))
                            .when(!busy, |el| {
                                el.hover(|style| {
                                    style.bg(gpui::rgb(theme::ACCENT_HOVER)).cursor_pointer()
                                })
                            })
                            .when(!busy, |el| el.on_click(cx.listener(Self::submit_clicked)))
                            .child(if busy {
                                "Signing in…".to_string()
                            } else {
                                "Sign in".to_string()
                            }),
                    ),
            )
    }
}

fn field(label: &str, input: Entity<TextInput>) -> Div {
    div()
        .flex()
        .flex_col()
        .gap_1()
        .child(
            div()
                .text_sm()
                .text_color(gpui::rgb(theme::TEXT_SECONDARY))
                .child(label.to_string()),
        )
        .child(
            div()
                .px_3()
                .py_2()
                .rounded_md()
                .bg(gpui::rgb(theme::BG_INPUT))
                .border_1()
                .border_color(gpui::rgb(theme::BORDER))
                .text_color(gpui::rgb(theme::TEXT_PRIMARY))
                .child(input),
        )
}
