//! Segmented presentation of one native text input. The editor still owns
//! selection, clipboard, undo, keyboard navigation and accessibility.
use super::*;

const COUNT: usize = 8;
const CELL: f32 = 40.;
const GAP: f32 = 6.;
const SEPARATOR: f32 = 16.;
const HEIGHT: f32 = 48.;
pub(super) const WIDTH: f32 = CELL * COUNT as f32 + GAP * COUNT as f32 + SEPARATOR;

fn left(index: usize) -> Pixels {
    px(index as f32 * (CELL + GAP) + if index >= 4 { SEPARATOR + GAP } else { 0. })
}

fn caret_x(index: usize) -> Pixels {
    if index >= COUNT {
        left(COUNT - 1) + px(30.)
    } else {
        left(index) + px(10.)
    }
}

pub(super) fn index_at(x: Pixels, len: usize) -> usize {
    (0..len.min(COUNT))
        .find(|&index| x < left(index) + px(CELL / 2.))
        .unwrap_or(len.min(COUNT))
}

pub(super) fn range_bounds(bounds: Bounds<Pixels>, range: Range<usize>) -> Bounds<Pixels> {
    Bounds::from_corners(
        point(bounds.left() + caret_x(range.start), bounds.top()),
        point(bounds.left() + caret_x(range.end), bounds.bottom()),
    )
}

/// Normalize only a complete optional separator and ASCII letter case. Reject
/// invalid/oversized edits rather than silently dropping or truncating data.
/// The host remains responsible for checking the actual pairing alphabet.
pub(super) fn normalize(text: &str, retained: usize) -> Option<String> {
    if !text.is_empty() && text.trim().is_empty() {
        return None;
    }
    let text = text.trim();
    let value = if text.len() == 9 && text.as_bytes()[4] == b'-' {
        format!("{}{}", &text[..4], &text[5..])
    } else {
        text.to_owned()
    };
    (value.is_ascii()
        && value.bytes().all(|b| b.is_ascii_alphanumeric())
        && retained + value.len() <= COUNT)
        .then(|| value.to_ascii_uppercase())
}

pub(super) fn render(
    input: &TextInput,
    window: &mut Window,
    cx: &mut Context<TextInput>,
) -> gpui::Div {
    let focused = input.focus_handle.is_focused(window);
    let cursor = input.cursor_offset();
    let mut row = div()
        .relative()
        .flex()
        .items_center()
        .gap(px(GAP))
        .w(px(WIDTH))
        .flex_shrink_0()
        .h(px(HEIGHT))
        .font_family(crate::assets::FONT_MONO)
        .text_xl()
        .text_color(rgb(theme::text_primary()));
    for index in 0..COUNT {
        if index == 4 {
            row = row.child(
                div()
                    .w(px(SEPARATOR))
                    .flex_shrink_0()
                    .text_center()
                    .text_color(rgb(theme::text_muted()))
                    .child("–"),
            );
        }
        let selected = input.selected_range.contains(&index);
        let active = focused && input.selected_range.is_empty() && index == cursor.min(COUNT - 1);
        let character = input
            .content
            .as_bytes()
            .get(index)
            .map(|b| char::from(*b).to_string());
        row = row.child(
            div()
                .relative()
                .w(px(CELL))
                .h(px(HEIGHT))
                .flex_shrink_0()
                .flex()
                .items_center()
                .justify_center()
                .rounded(theme::RADIUS_MD)
                .border_1()
                .border_color(rgb(if active {
                    theme::accent()
                } else {
                    theme::border()
                }))
                .bg(rgb(theme::bg_input()))
                .when(selected, |cell| cell.bg(theme::text_selection()))
                .children(character)
                .when(active, |cell| {
                    cell.child(
                        div()
                            .absolute()
                            .top(px(13.))
                            .left(if cursor == COUNT { px(30.) } else { px(10.) })
                            .w(px(2.))
                            .h(px(22.))
                            .bg(rgb(theme::text_cursor())),
                    )
                }),
        );
    }
    let entity = cx.entity();
    // Register the whole row as ONE native field, with matching slot geometry
    // for mouse selection and OS text-range queries. No hidden off-screen input.
    row.child(
        gpui::canvas(
            |_, _, _| (),
            move |bounds, _, window, cx| {
                let focus = entity.read(cx).focus_handle.clone();
                window.handle_input(&focus, ElementInputHandler::new(bounds, entity.clone()), cx);
                entity.update(cx, |input, _| input.last_bounds = Some(bounds));
            },
        )
        .absolute()
        .top_0()
        .left_0()
        .size_full(),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn code_edits_normalize_format_without_truncating_invalid_input() {
        assert_eq!(normalize(" abcd-efgh ", 0).as_deref(), Some("ABCDEFGH"));
        assert_eq!(normalize("efgh", 4).as_deref(), Some("EFGH"));
        assert_eq!(normalize("", 7).as_deref(), Some(""));
        for text in [
            "ABCDEFGHI",
            "AB-CD-EFGH",
            "ABCD–EFGH",
            "abcd!efg",
            "éé-EFGH",
        ] {
            assert_eq!(normalize(text, 0), None);
        }
        assert_eq!(normalize("Z", 8), None);
    }

    #[gpui::test]
    fn code_slots_keep_native_paste_selection_navigation_and_undo(cx: &mut gpui::TestAppContext) {
        cx.executor().allow_parking();
        struct Host(Entity<TextInput>);
        impl Render for Host {
            fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
                div().w(px(WIDTH)).child(self.0.clone())
            }
        }
        let input = cx.new(|cx| {
            crate::desktop::register_key_bindings(cx);
            TextInput::new("Pairing code", cx).pairing_code()
        });
        let (_, cx) = cx.add_window_view(|window, cx| {
            window.activate_window();
            window.focus(&input.read(cx).focus_handle(cx), cx);
            Host(input.clone())
        });
        cx.simulate_resize(size(px(500.), px(200.)));
        cx.run_until_parked();
        cx.update(|window, cx| {
            cx.write_to_clipboard(ClipboardItem::new_string("abcd-efgh".into()));
            input.update(cx, |input, cx| input.paste(&Paste, window, cx));
        });
        assert_eq!(input.read_with(cx, |input, _| input.text()), "ABCDEFGH");
        cx.simulate_input("Z");
        assert_eq!(
            input.read_with(cx, |input, _| input.text()),
            "ABCDEFGH",
            "ninth character rejected"
        );
        cx.simulate_keystrokes("left backspace");
        cx.simulate_input("z");
        assert_eq!(input.read_with(cx, |input, _| input.text()), "ABCDEFZH");
        cx.simulate_keystrokes("shift-left");
        cx.simulate_input("g");
        assert_eq!(input.read_with(cx, |input, _| input.text()), "ABCDEFGH");
        cx.update(|window, cx| input.update(cx, |input, cx| input.undo(&Undo, window, cx)));
        assert_eq!(input.read_with(cx, |input, _| input.text()), "ABCDEFZH");
        cx.run_until_parked();
        cx.update(|window, cx| {
            input.update(cx, |input, cx| {
                let bounds = input.last_bounds.expect("native code field bounds");
                let click = point(bounds.left() + left(4) + px(12.), bounds.center().y);
                assert_eq!(input.character_index_for_point(click, window, cx), Some(4));
                assert_eq!(input.index_for_mouse_position(click), 4);
                let caret = input.bounds_for_range(4..4, bounds, window, cx).unwrap();
                assert_eq!(caret.left(), bounds.left() + caret_x(4));
                input.select_all(&SelectAll, window, cx);
            })
        });
        cx.simulate_input("wxyz-2345");
        assert_eq!(input.read_with(cx, |input, _| input.text()), "WXYZ2345");
        cx.simulate_keystrokes("enter");
        assert_eq!(
            input.read_with(cx, |input, _| input.text()),
            "WXYZ2345",
            "no implicit submission"
        );
    }
}
