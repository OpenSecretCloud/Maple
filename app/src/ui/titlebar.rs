//! Custom window title bar, drawn only when the system draws none (see
//! `super::decorations`). That is the case on GNOME, whose compositor
//! leaves every window undecorated: a drag region with a centered title
//! plus round minimize, maximize, and close controls.

use gpui::{Context, Pixels, SharedString, Window, div, prelude::*, px};

use super::decorations::system_draws_titlebar;
use super::theme;

/// Window title. The system bar and the app's own bar show the same text,
/// and only one of them is ever on screen (see `super::decorations`).
pub const WINDOW_TITLE: &str = "Maple - Private AI Chat";

const BAR_HEIGHT: Pixels = px(40.);
const CONTROL_SIZE: Pixels = px(24.);

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
}

impl Render for TitleBar {
    fn render(&mut self, window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
        if system_draws_titlebar(window) {
            // The window already has a title bar. A second one is the bug
            // this check exists to prevent.
            return div().into_any_element();
        }

        let controls = window.window_controls();
        div()
            .id("title-bar")
            .relative()
            .flex()
            .items_center()
            .justify_end()
            .w_full()
            .h(BAR_HEIGHT)
            .pr_3()
            .bg(gpui::rgb(theme::bg_sidebar()))
            // Any press in the bar that is not on a control starts a window
            // drag, which is the standard title-bar behavior. A double
            // press toggles maximize instead.
            .on_mouse_down(gpui::MouseButton::Left, |event, window, _| {
                if event.click_count >= 2 {
                    window.zoom_window();
                } else {
                    window.start_window_move();
                }
            })
            .child(
                div()
                    .absolute()
                    .inset_0()
                    .flex()
                    .items_center()
                    .justify_center()
                    .text_sm()
                    .font_weight(gpui::FontWeight::BOLD)
                    .text_color(gpui::rgb(theme::text_primary()))
                    .child(self.title.clone()),
            )
            .child(
                div()
                    .relative()
                    .flex()
                    .items_center()
                    .gap_2()
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
            .into_any_element()
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
        .size(CONTROL_SIZE)
        .rounded_full()
        .bg(gpui::rgb(theme::bg_title_control()))
        .text_xs()
        .text_color(gpui::rgb(theme::text_primary()))
        .hover(|style| {
            style
                .bg(gpui::rgb(theme::bg_title_control_hover()))
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
