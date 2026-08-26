//! Custom window title bar. Wayland compositors draw no server-side
//! decorations, so the app renders its own: a drag region plus minimize,
//! maximize, and close controls.

use gpui::{Context, SharedString, Window, div, prelude::*, px};

use super::theme;

const BAR_HEIGHT: gpui::Pixels = px(34.);

pub struct TitleBar {
    title: SharedString,
}

impl TitleBar {
    pub fn new(title: impl Into<SharedString>) -> Self {
        Self {
            title: title.into(),
        }
    }

    /// App version for display, from the workspace manifest.
    pub fn version() -> &'static str {
        env!("CARGO_PKG_VERSION")
    }
    pub fn height() -> gpui::Pixels {
        BAR_HEIGHT
    }
}

impl Render for TitleBar {
    fn render(&mut self, window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
        let controls = window.window_controls();
        div()
            .id("title-bar")
            .flex()
            .items_center()
            .justify_between()
            .w_full()
            .h(BAR_HEIGHT)
            .pl_3()
            .bg(gpui::rgb(theme::BG_SIDEBAR))
            .border_b_1()
            .border_color(gpui::rgb(theme::BORDER_SUBTLE))
            // Any press in the bar that is not on a control starts a window
            // drag, which is the standard title-bar behavior.
            .on_mouse_down(gpui::MouseButton::Left, |_, window, _| {
                window.start_window_move();
            })
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap_2()
                    .child(
                        div()
                            .text_xs()
                            .text_color(gpui::rgb(theme::TEXT_MUTED))
                            .child(self.title.clone()),
                    )
                    .child(
                        div()
                            .text_xs()
                            .text_color(gpui::rgb(theme::TEXT_FAINT))
                            .child(format!("v{}", Self::version())),
                    ),
            )
            .child(
                div()
                    .flex()
                    .items_center()
                    .h_full()
                    .children(controls.minimize.then(|| {
                        control("title-minimize", "—", |window, _cx| {
                            window.minimize_window();
                        })
                    }))
                    .children(controls.maximize.then(|| {
                        control("title-maximize", "▢", |window, _cx| {
                            window.zoom_window();
                        })
                    }))
                    .child(control("title-close", "✕", |_window, cx| {
                        cx.quit();
                    })),
            )
    }
}

fn control(
    id: &'static str,
    label: &'static str,
    action: impl Fn(&mut Window, &mut gpui::App) + 'static,
) -> impl IntoElement {
    div()
        .id(id)
        .flex()
        .items_center()
        .justify_center()
        .size(BAR_HEIGHT)
        .text_sm()
        .text_color(gpui::rgb(theme::TEXT_SECONDARY))
        .hover(|style| {
            style
                .bg(gpui::rgb(theme::STATUS_ERROR))
                .text_color(gpui::rgb(theme::TEXT_PRIMARY))
                .cursor_pointer()
        })
        .on_mouse_down(gpui::MouseButton::Left, {
            let action = std::rc::Rc::new(action);
            move |_event, window, cx| {
                // Stop the press from also starting a window drag.
                cx.stop_propagation();
                action(window, cx);
            }
        })
        .child(label.to_string())
}
