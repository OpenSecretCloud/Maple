//! Shared chrome for the gpui screens: popup panels, menu rows, buttons,
//! card rows, input frames, and banners.
//!
//! Every helper returns a bare `div()` builder, so it costs the same as
//! the inline chain it replaces: no allocation, no parsing, nothing
//! cached. Callers keep their own ids, children, and click handlers.

use gpui::{Div, ElementId, Pixels, Stateful, div, prelude::*, px};

use super::icons::icon;
use super::theme;

/// Floating panel behind a context menu or dropdown: an elevated surface
/// with a border and a shadow, stacked as a column.
///
/// The caller adds the anchoring (`gpui::anchored` or `absolute`), the
/// dismiss handler, and the rows.
pub fn popup_panel(id: impl Into<ElementId>, width: Pixels) -> Stateful<Div> {
    div()
        .id(id)
        .occlude()
        .w(width)
        .py_1()
        .rounded_md()
        .bg(gpui::rgb(theme::bg_elevated()))
        .border_1()
        .border_color(gpui::rgb(theme::border()))
        .shadow_md()
        .flex()
        .flex_col()
}

/// One row inside a [`popup_panel`]. A disabled row is dimmed and takes
/// no hover, so the caller only attaches `on_click` when `enabled`.
pub fn menu_row(id: impl Into<ElementId>, enabled: bool) -> Stateful<Div> {
    div()
        .id(id)
        .px_3()
        .py_1p5()
        .text_sm()
        .text_color(gpui::rgb(if enabled {
            theme::text_primary()
        } else {
            theme::text_muted()
        }))
        .when(enabled, |row| {
            row.hover(|style| {
                style
                    .bg(gpui::rgb(theme::bg_sidebar_row_hover()))
                    .cursor_pointer()
            })
        })
}

/// Accent-filled action button. The label sits on the accent fill, so it
/// takes the app background color rather than the primary text color.
pub fn primary_button(id: impl Into<ElementId>) -> Stateful<Div> {
    div()
        .id(id)
        .flex()
        .items_center()
        .gap_1p5()
        .px_3()
        .py_1p5()
        .rounded_md()
        .bg(gpui::rgb(theme::accent()))
        .text_sm()
        .text_color(gpui::rgb(theme::bg_app()))
        .hover(|style| style.bg(gpui::rgb(theme::accent_hover())).cursor_pointer())
}

/// Text-only button for secondary actions (Back, Cancel): no fill, no
/// border, and the label brightens on hover.
pub fn ghost_button(id: impl Into<ElementId>) -> Stateful<Div> {
    div()
        .id(id)
        .px_3()
        .py_1p5()
        .rounded_md()
        .text_sm()
        .text_color(gpui::rgb(theme::text_secondary()))
        .hover(|style| {
            style
                .text_color(gpui::rgb(theme::text_primary()))
                .cursor_pointer()
        })
}

/// Square icon-only button with a hover plate, used in list rows and
/// toolbars.
pub fn icon_button(
    id: impl Into<ElementId>,
    name: &'static str,
    size: Pixels,
    color: u32,
) -> Stateful<Div> {
    div()
        .id(id)
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
        .child(icon(name, size, color))
}

/// Card chrome for a settings-style row: padded elevated surface with a
/// subtle border.
pub fn card_row() -> Div {
    div()
        .p_4()
        .rounded_lg()
        .bg(gpui::rgb(theme::bg_elevated()))
        .border_1()
        .border_color(gpui::rgb(theme::border_subtle()))
}

/// Frame around a [`super::text_input::TextInput`]. The explicit text
/// color matters: the input inherits the ambient color, which renders
/// near-black on the dark field without it.
pub fn input_frame() -> Div {
    div()
        .px_3()
        .py_2()
        .rounded_md()
        .bg(gpui::rgb(theme::bg_input()))
        .border_1()
        .border_color(gpui::rgb(theme::border()))
        .text_color(gpui::rgb(theme::text_primary()))
}

/// Full-width status message on a solid fill, such as
/// `theme::status_error()` or `theme::status_warning()`. The text takes
/// the app background color so it reads on any status fill.
pub fn banner(background: u32) -> Div {
    div()
        .px_3()
        .py_2()
        .rounded_md()
        .bg(gpui::rgb(background))
        .text_color(gpui::rgb(theme::bg_app()))
        .text_sm()
}

/// Icon size inside a [`menu_row`] or an [`icon_button`] in a list row.
pub const ROW_ICON: Pixels = px(14.);
