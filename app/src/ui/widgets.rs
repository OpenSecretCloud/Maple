//! Shared chrome for the gpui screens: popup panels, menu rows, buttons,
//! switches, tooltips, card rows, input frames, and banners.
//!
//! Every helper returns a bare `div()` builder, so it costs the same as
//! the inline chain it replaces: no allocation, no parsing, nothing
//! cached. Callers keep their own ids, children, and click handlers.
//!
//! Shapes follow the brand kit: buttons are pills, small controls use
//! `theme::RADIUS_SM`, popups and inputs `theme::RADIUS_MD`, cards
//! `theme::RADIUS_LG`. Every clickable helper has a hover plate and a
//! pressed state, so no control is silent under the pointer.

use std::cell::RefCell;
use std::time::{Duration, Instant};

use gpui::{
    AnyView, App, Div, ElementId, Pixels, SharedString, Stateful, Window, div, prelude::*, px,
};

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
        .p_1()
        .rounded(theme::RADIUS_MD)
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
        .rounded(theme::RADIUS_SM)
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
            .active(|style| style.bg(gpui::rgb(theme::bg_sidebar_row_selected())))
        })
}

/// Shared pill shape for [`primary_button`], [`secondary_button`], and
/// [`ghost_button`]: the brand's button sizing (8 px vertical, 20 px
/// horizontal padding) with a medium-weight label.
fn pill(id: impl Into<ElementId>) -> Stateful<Div> {
    div()
        .id(id)
        .flex()
        .items_center()
        .justify_center()
        .gap_1p5()
        .px(theme::SPACE_MD)
        .py_2()
        .rounded_full()
        .text_sm()
        .font_weight(gpui::FontWeight::MEDIUM)
}

/// Accent-filled action button: coral pill with a light label.
pub fn primary_button(id: impl Into<ElementId>) -> Stateful<Div> {
    pill(id)
        .bg(gpui::rgb(theme::accent()))
        .text_color(gpui::rgb(theme::on_accent()))
        .hover(|style| style.bg(gpui::rgb(theme::accent_hover())).cursor_pointer())
        .active(|style| style.bg(gpui::rgb(theme::send_bottom())))
}

/// Neutral pill for the second action beside a [`primary_button`]
/// (Cancel, Back, alternate sign-in): a pebble fill that darkens on hover.
pub fn secondary_button(id: impl Into<ElementId>) -> Stateful<Div> {
    pill(id)
        .bg(gpui::rgb(theme::bg_sidebar_pill()))
        .text_color(gpui::rgb(theme::text_primary()))
        .hover(|style| {
            style
                .bg(gpui::rgb(theme::bg_sidebar_row_selected()))
                .cursor_pointer()
        })
        .active(|style| style.bg(gpui::rgb(theme::border())))
}

/// Text-only button for low-emphasis actions: no fill, no border, and
/// the label brightens on hover.
pub fn ghost_button(id: impl Into<ElementId>) -> Stateful<Div> {
    pill(id)
        .text_color(gpui::rgb(theme::text_secondary()))
        .hover(|style| {
            style
                .text_color(gpui::rgb(theme::text_primary()))
                .bg(theme::overlay_hover())
                .cursor_pointer()
        })
        .active(|style| style.bg(gpui::rgb(theme::bg_sidebar_pill())))
}

/// Destructive pill (Remove, Deny): error fill with a light label.
pub fn danger_button(id: impl Into<ElementId>) -> Stateful<Div> {
    pill(id)
        .bg(gpui::rgb(theme::status_error()))
        .text_color(gpui::rgb(theme::on_accent()))
        .hover(|style| style.opacity(0.9).cursor_pointer())
        .active(|style| style.opacity(0.8))
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
        .rounded(theme::RADIUS_SM)
        .hover(|style| {
            style
                .bg(gpui::rgb(theme::bg_sidebar_row_hover()))
                .cursor_pointer()
        })
        .active(|style| style.bg(gpui::rgb(theme::bg_sidebar_row_selected())))
        .child(icon(name, size, color))
}

/// On/off switch: a pill track with a knob that sits right when `on`.
/// The caller attaches `on_click`. The track is the brand accent when on
/// and the hairline colour when off, so state reads without a label.
pub fn switch(id: impl Into<ElementId>, on: bool) -> Stateful<Div> {
    div()
        .id(id)
        .flex_none()
        .w(px(34.))
        .h(px(20.))
        .p(px(3.))
        .rounded_full()
        .bg(gpui::rgb(if on {
            theme::accent()
        } else {
            theme::bg_sidebar_row_selected()
        }))
        .flex()
        .items_center()
        .when(on, |track| track.justify_end())
        .hover(|style| style.cursor_pointer().opacity(0.9))
        .active(|style| style.opacity(0.8))
        .child(
            div()
                .size(px(14.))
                .rounded_full()
                .bg(gpui::rgb(theme::on_accent()))
                .shadow_2xs(),
        )
}

/// Card chrome for a settings-style row: padded elevated surface with a
/// subtle border.
pub fn card_row() -> Div {
    div()
        .p_4()
        .rounded(theme::RADIUS_LG)
        .bg(gpui::rgb(theme::bg_elevated()))
        .border_1()
        .border_color(gpui::rgb(theme::border_subtle()))
}

/// Frame around a [`super::text_input::TextInput`]. The explicit text
/// color matters: the input inherits the ambient color, which renders
/// near-black on the dark field without it. Pass `focused` so the frame
/// takes the accent border while the field has keyboard focus.
pub fn input_frame(focused: bool) -> Div {
    div()
        .px_3()
        .py_2()
        .rounded(theme::RADIUS_MD)
        .bg(gpui::rgb(theme::bg_input()))
        .border_1()
        .border_color(gpui::rgb(if focused {
            theme::accent()
        } else {
            theme::border()
        }))
        .text_color(gpui::rgb(theme::text_primary()))
}

/// Full-width status message on a solid fill, such as
/// `theme::status_error()` or `theme::status_warning()`. The label takes
/// the on-accent white, which reads on every status fill.
pub fn banner(background: u32) -> Div {
    div()
        .px_3()
        .py_2()
        .rounded(theme::RADIUS_MD)
        .bg(gpui::rgb(background))
        .text_color(gpui::rgb(theme::on_accent()))
        .text_sm()
}

/// Transient notice: a soft surface with a leading icon and a close
/// button. Softer than [`banner`], which is for errors that block.
pub fn notice(
    id: impl Into<ElementId>,
    text: SharedString,
    on_close: impl Fn(&gpui::ClickEvent, &mut Window, &mut App) + 'static,
) -> Div {
    div()
        .flex()
        .items_center()
        .gap_2()
        .pl_3()
        .pr_1p5()
        .py_1p5()
        .rounded(theme::RADIUS_MD)
        .bg(gpui::rgb(theme::bg_elevated()))
        .border_1()
        .border_color(gpui::rgb(theme::border()))
        .shadow_sm()
        .text_sm()
        .text_color(gpui::rgb(theme::text_primary()))
        .child(icon("zap", ROW_ICON, theme::status_warning()))
        .child(div().flex_1().min_w_0().line_clamp(2).child(text))
        .child(icon_button(id, "x", px(12.), theme::text_muted()).on_click(on_close))
}

/// Icon size inside a [`menu_row`] or an [`icon_button`] in a list row.
pub const ROW_ICON: Pixels = px(14.);

// ---- Tooltips -------------------------------------------------------------

/// Small label shown under the pointer after gpui's hover delay, with an
/// optional keyboard shortcut. Attach with `.tooltip(widgets::tooltip(..))`.
pub fn tooltip(
    label: impl Into<SharedString>,
    shortcut: Option<&'static str>,
) -> impl Fn(&mut Window, &mut App) -> AnyView + 'static {
    let label = label.into();
    move |_window, cx| {
        let label = label.clone();
        cx.new(|_| Tooltip { label, shortcut }).into()
    }
}

struct Tooltip {
    label: SharedString,
    shortcut: Option<&'static str>,
}

impl Render for Tooltip {
    fn render(&mut self, _window: &mut Window, _cx: &mut gpui::Context<Self>) -> impl IntoElement {
        div()
            .mt_2()
            .flex()
            .items_center()
            .gap_2()
            .px_2()
            .py_1()
            .rounded(theme::RADIUS_SM)
            .bg(gpui::rgb(theme::bg_elevated()))
            .border_1()
            .border_color(gpui::rgb(theme::border()))
            .shadow_md()
            .text_xs()
            .text_color(gpui::rgb(theme::text_primary()))
            .child(self.label.clone())
            .children(self.shortcut.map(|keys| {
                div()
                    .px_1()
                    .rounded(px(4.))
                    .bg(gpui::rgb(theme::bg_sidebar_pill()))
                    .font_family(crate::assets::FONT_MONO)
                    .text_color(gpui::rgb(theme::text_secondary()))
                    .child(keys)
            }))
    }
}

// ---- Copy feedback --------------------------------------------------------

/// How long a copy button reads "Copied".
const COPIED_FOR: Duration = Duration::from_millis(1400);

thread_local! {
    /// The copy button pressed most recently and when. One slot is enough:
    /// a second copy replaces the first, which is what the eye expects.
    static COPIED: RefCell<Option<(ElementId, Instant)>> = const { RefCell::new(None) };
}

fn copied_recently(id: &ElementId) -> bool {
    COPIED.with(|slot| {
        slot.borrow()
            .as_ref()
            .is_some_and(|(copied, at)| copied == id && at.elapsed() < COPIED_FOR)
    })
}

/// Small "Copy" button that writes `text` to the clipboard and reads
/// "Copied" with a check for a moment afterwards. The swap keeps the same
/// height, so a transcript row does not need re-measuring. The button
/// paints itself hidden until its `group` is hovered when `reveal` is set.
pub fn copy_button(
    id: impl Into<ElementId>,
    text: SharedString,
    reveal: Option<&SharedString>,
) -> Stateful<Div> {
    let id = id.into();
    let copied = copied_recently(&id);
    let click_id = id.clone();
    div()
        .id(id)
        .flex()
        .items_center()
        .gap_1()
        .px_1p5()
        .py_0p5()
        .rounded(theme::RADIUS_SM)
        .text_xs()
        .text_color(gpui::rgb(if copied {
            theme::status_success()
        } else {
            theme::text_muted()
        }))
        .when_some(reveal, |button, group| {
            button
                .opacity(if copied { 1. } else { 0. })
                .group_hover(group.clone(), |style| style.opacity(1.))
        })
        .hover(|style| {
            style
                .bg(theme::overlay_hover())
                .text_color(gpui::rgb(theme::text_secondary()))
                .cursor_pointer()
        })
        .active(|style| style.bg(gpui::rgb(theme::bg_sidebar_pill())))
        .on_click(move |_event, window, cx: &mut App| {
            cx.stop_propagation();
            cx.write_to_clipboard(gpui::ClipboardItem::new_string(text.to_string()));
            COPIED.with(|slot| *slot.borrow_mut() = Some((click_id.clone(), Instant::now())));
            window.refresh();
            // One more paint after the label reverts; no polling between.
            window
                .spawn(cx, async move |cx| {
                    cx.background_executor().timer(COPIED_FOR).await;
                    cx.update(|window, _| window.refresh()).ok();
                })
                .detach();
        })
        .child(icon(
            if copied { "check" } else { "copy" },
            px(12.),
            if copied {
                theme::status_success()
            } else {
                theme::text_secondary()
            },
        ))
        .child(if copied { "Copied" } else { "Copy" })
}
