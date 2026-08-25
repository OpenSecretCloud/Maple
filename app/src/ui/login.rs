//! Sign-in screen: email and password against the OpenSecret backend.

use std::sync::Arc;

use gpui::{AppContext, Context, Div, Entity, EventEmitter, Render, Window, div, prelude::*};

use crate::backend::{AgentBackend, AuthSession};
use crate::ui::text_input::TextInput;
use crate::ui::theme;

pub struct LoginSucceeded(pub AuthSession);

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
        Self {
            backend,
            email_input: email,
            password_input: password,
            error: None,
            busy: false,
        }
    }

    fn submit(&mut self, _event: &gpui::ClickEvent, _window: &mut Window, cx: &mut Context<Self>) {
        if self.busy {
            return;
        }
        let email = self.email_input.read(cx).text();
        let password = self.password_input.read(cx).text();
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
            let result = task.await.unwrap_or_else(|error| Err(format!("{error}")));
            this.update(cx, |this, cx| {
                this.busy = false;
                match result {
                    Ok(session) => cx.emit(LoginSucceeded(session)),
                    Err(message) => this.error = Some(message),
                }
                cx.notify();
            })
            .ok();
        })
        .detach();
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
                            .on_click(cx.listener(Self::submit))
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
