//! Text input, ported from the gpui 0.2.2 `input` example and adapted for
//! Maple: neutral theming, optional password masking, an Enter-key hook for
//! form submit and composer send, and an optional multi-line mode that wraps
//! text and grows with its content (Shift+Enter inserts a newline).

use super::theme;
use std::ops::Range;

use gpui::{
    App, Bounds, ClipboardEntry, ClipboardItem, ContentMask, Context, CursorStyle, Element,
    ElementId, ElementInputHandler, Entity, EntityInputHandler, FocusHandle, Focusable,
    GlobalElementId, InteractiveElement, KeyBinding, Keystroke, LayoutId, MouseDownEvent,
    MouseMoveEvent, MouseUpEvent, PaintQuad, Pixels, SharedString, Style, TextAlign, TextRun,
    UTF16Selection, UnderlineStyle, Window, WrappedLine, actions, div, fill, point, prelude::*, px,
    relative, rgb, rgba, size,
};
use unicode_segmentation::UnicodeSegmentation;

actions!(
    text_input,
    [
        Backspace,
        Delete,
        Left,
        Right,
        SelectLeft,
        SelectRight,
        SelectAll,
        Home,
        End,
        Up,
        Down,
        ShowCharacterPalette,
        Paste,
        Cut,
        Copy,
    ]
);

/// Register the default key bindings for every TextInput. Safe to call once
/// at startup; covers both platform modifier conventions.
pub fn register_key_bindings(cx: &mut App) {
    let context = Some("TextInput");
    cx.bind_keys([
        KeyBinding::new("backspace", Backspace, context),
        KeyBinding::new("delete", Delete, context),
        KeyBinding::new("left", Left, context),
        KeyBinding::new("right", Right, context),
        KeyBinding::new("shift-left", SelectLeft, context),
        KeyBinding::new("shift-right", SelectRight, context),
        KeyBinding::new("ctrl-a", SelectAll, context),
        KeyBinding::new("cmd-a", SelectAll, context),
        KeyBinding::new("ctrl-v", Paste, context),
        KeyBinding::new("cmd-v", Paste, context),
        KeyBinding::new("ctrl-c", Copy, context),
        KeyBinding::new("cmd-c", Copy, context),
        KeyBinding::new("ctrl-x", Cut, context),
        KeyBinding::new("cmd-x", Cut, context),
        KeyBinding::new("home", Home, context),
        KeyBinding::new("end", End, context),
        KeyBinding::new("up", Up, context),
        KeyBinding::new("down", Down, context),
        KeyBinding::new("ctrl-cmd-space", ShowCharacterPalette, context),
    ]);
}

type EnterHandler = Box<dyn Fn(String, &mut Window, &mut Context<TextInput>) + 'static>;
type PasteImageHandler = Box<dyn Fn(gpui::Image, &mut Window, &mut Context<TextInput>) + 'static>;
/// First look at a key press with the input's current text. Return true to
/// consume it. The handler runs while this input is being updated, so it
/// must not read or update this input entity; defer anything that does.
type KeyHandler = Box<
    dyn Fn(&gpui::KeyDownEvent, &SharedString, &mut Window, &mut Context<TextInput>) -> bool
        + 'static,
>;

pub struct TextInput {
    focus_handle: FocusHandle,
    content: SharedString,
    placeholder: SharedString,
    selected_range: Range<usize>,
    selection_reversed: bool,
    marked_range: Option<Range<usize>>,
    last_layout: Option<TextLayout>,
    last_bounds: Option<Bounds<Pixels>>,
    /// Horizontal scroll of a single-line input so the cursor stays in
    /// view when the text is wider than the box.
    scroll_x: Pixels,
    /// Wrap long text and accept Shift+Enter newlines; the element grows
    /// with the content up to `max_lines`.
    multiline: bool,
    /// Fill the parent's height instead of sizing to the content
    /// (expanded composer).
    fill_height: bool,
    max_lines: usize,
    /// Last (text, wrap width) -> row count, so layout does not re-shape
    /// unchanged text every frame.
    measure_cache: Option<(SharedString, Pixels, usize)>,
    is_selecting: bool,
    /// Render '*' in place of content characters (password fields).
    mask: bool,
    /// Clear the content once the Enter hook has consumed it (composer behavior).
    /// Explicit tab order for this input within its surface. Inputs without
    /// distinct indices collapse onto the same tab-stop path, which makes
    /// focus navigation a no-op.
    tab_index: Option<isize>,
    on_enter: Option<EnterHandler>,
    /// Called when the clipboard holds an image instead of text.
    on_paste_image: Option<PasteImageHandler>,
    /// First look at every key press; returning true consumes the key.
    on_key: Option<KeyHandler>,
}

impl TextInput {
    /// Intercept key presses before the input's own handling. The composer
    /// uses this for slash-palette navigation; the handler receives the
    /// input's current text so it never reads this entity mid-update.
    pub fn on_key(
        mut self,
        handler: impl Fn(
            &gpui::KeyDownEvent,
            &SharedString,
            &mut Window,
            &mut Context<TextInput>,
        ) -> bool
        + 'static,
    ) -> Self {
        self.on_key = Some(Box::new(handler));
        self
    }

    pub fn new(placeholder: &str, cx: &mut Context<Self>) -> Self {
        Self {
            focus_handle: cx.focus_handle(),
            content: "".into(),
            placeholder: SharedString::from(placeholder.to_string()),
            selected_range: 0..0,
            selection_reversed: false,
            marked_range: None,
            last_layout: None,
            last_bounds: None,
            scroll_x: px(0.),
            multiline: false,
            fill_height: false,
            max_lines: 8,
            measure_cache: None,
            is_selecting: false,
            mask: false,
            tab_index: None,
            on_enter: None,
            on_paste_image: None,
            on_key: None,
        }
    }

    /// Give this input an explicit position in the tab order.
    pub fn with_tab_index(mut self, index: isize) -> Self {
        self.tab_index = Some(index);
        self
    }

    pub fn masked(mut self) -> Self {
        self.mask = true;
        self
    }

    /// Wrap text and grow with the content, up to `max_lines` rows.
    pub fn multiline(mut self, max_lines: usize) -> Self {
        self.multiline = true;
        self.max_lines = max_lines.max(1);
        self
    }

    /// Fill the parent's height (used while the composer is expanded).
    pub fn set_fill_height(&mut self, fill: bool, cx: &mut Context<Self>) {
        if self.fill_height != fill {
            self.fill_height = fill;
            cx.notify();
        }
    }

    pub fn on_enter(
        mut self,
        handler: impl Fn(String, &mut Window, &mut Context<TextInput>) + 'static,
    ) -> Self {
        self.on_enter = Some(Box::new(handler));
        self
    }

    /// Receive images pasted with Ctrl-V; text pastes go into the input.
    pub fn on_paste_image(
        mut self,
        handler: impl Fn(gpui::Image, &mut Window, &mut Context<TextInput>) + 'static,
    ) -> Self {
        self.on_paste_image = Some(Box::new(handler));
        self
    }

    /// Attach or replace the Enter hook after construction. The handler
    /// receives the current text so it never reads this entity re-entrantly.
    pub fn set_on_enter(
        &mut self,
        handler: impl Fn(String, &mut Window, &mut Context<TextInput>) + 'static,
    ) {
        self.on_enter = Some(Box::new(handler));
    }

    pub fn text(&self) -> String {
        self.content.to_string()
    }

    /// The content without a copy, for checks that only read it.
    pub fn text_ref(&self) -> &str {
        &self.content
    }

    pub fn set_text(&mut self, text: &str, cx: &mut Context<Self>) {
        self.content = SharedString::from(text.to_string());
        self.selected_range = self.content.len()..self.content.len();
        cx.notify();
    }

    pub fn clear(&mut self, cx: &mut Context<Self>) {
        self.content = "".into();
        self.selected_range = 0..0;
        self.selection_reversed = false;
        cx.notify();
    }

    fn enter_pressed(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        // Hand the handler the current text so it never has to read this
        // entity while it is being updated.
        let text = self.text();
        if let Some(on_enter) = self.on_enter.take() {
            on_enter(text, window, cx);
            self.on_enter = Some(on_enter);
        }
    }

    fn backspace(&mut self, _: &Backspace, window: &mut Window, cx: &mut Context<Self>) {
        if self.selected_range.is_empty() {
            self.select_to(self.previous_boundary(self.cursor_offset()), cx)
        }
        self.replace_text_in_range(None, "", window, cx)
    }

    fn delete(&mut self, _: &Delete, window: &mut Window, cx: &mut Context<Self>) {
        if self.selected_range.is_empty() {
            self.select_to(self.next_boundary(self.cursor_offset()), cx)
        }
        self.replace_text_in_range(None, "", window, cx)
    }

    fn left(&mut self, _: &Left, _: &mut Window, cx: &mut Context<Self>) {
        if self.selected_range.is_empty() {
            self.move_to(self.previous_boundary(self.cursor_offset()), cx);
        } else {
            self.move_to(self.selected_range.start, cx)
        }
    }

    fn right(&mut self, _: &Right, _: &mut Window, cx: &mut Context<Self>) {
        if self.selected_range.is_empty() {
            self.move_to(self.next_boundary(self.selected_range.end), cx)
        } else {
            self.move_to(self.selected_range.end, cx)
        }
    }

    fn select_left(&mut self, _: &SelectLeft, _: &mut Window, cx: &mut Context<Self>) {
        self.select_to(self.previous_boundary(self.cursor_offset()), cx);
    }

    fn select_right(&mut self, _: &SelectRight, _: &mut Window, cx: &mut Context<Self>) {
        self.select_to(self.next_boundary(self.cursor_offset()), cx);
    }

    fn select_all(&mut self, _: &SelectAll, _: &mut Window, cx: &mut Context<Self>) {
        self.move_to(0, cx);
        self.select_to(self.content.len(), cx)
    }

    fn home(&mut self, _: &Home, _: &mut Window, cx: &mut Context<Self>) {
        self.move_to(0, cx);
    }

    fn end(&mut self, _: &End, _: &mut Window, cx: &mut Context<Self>) {
        self.move_to(self.content.len(), cx);
    }

    fn up(&mut self, _: &Up, _: &mut Window, cx: &mut Context<Self>) {
        match self.vertical_neighbor(-1) {
            Some(offset) => self.move_to(offset, cx),
            None => self.move_to(0, cx),
        }
    }

    fn down(&mut self, _: &Down, _: &mut Window, cx: &mut Context<Self>) {
        match self.vertical_neighbor(1) {
            Some(offset) => self.move_to(offset, cx),
            None => self.move_to(self.content.len(), cx),
        }
    }

    /// Offset on the row above (-1) or below (+1) the cursor, keeping the
    /// horizontal position. None when there is no such row.
    fn vertical_neighbor(&self, direction: i32) -> Option<usize> {
        let layout = self.last_layout.as_ref()?;
        let cursor = self.to_display_offset(self.cursor_offset());
        let position = layout.position_for_index(cursor)?;
        let target_y =
            position.y + layout.line_height * (direction as f32) + layout.line_height / 2.;
        if target_y < px(0.) || target_y > layout.height() {
            return None;
        }
        let display = layout.closest_index_for_position(point(position.x, target_y));
        Some(self.content_offset_for_display(display))
    }

    fn on_mouse_down(
        &mut self,
        event: &MouseDownEvent,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.is_selecting = true;
        if event.modifiers.shift {
            self.select_to(self.index_for_mouse_position(event.position), cx);
        } else {
            self.move_to(self.index_for_mouse_position(event.position), cx)
        }
    }

    fn on_mouse_up(&mut self, _: &MouseUpEvent, _window: &mut Window, _: &mut Context<Self>) {
        self.is_selecting = false;
    }

    fn on_mouse_move(&mut self, event: &MouseMoveEvent, _: &mut Window, cx: &mut Context<Self>) {
        if self.is_selecting {
            self.select_to(self.index_for_mouse_position(event.position), cx);
        }
    }

    fn show_character_palette(
        &mut self,
        _: &ShowCharacterPalette,
        window: &mut Window,
        _: &mut Context<Self>,
    ) {
        window.show_character_palette();
    }

    fn paste(&mut self, _: &Paste, window: &mut Window, cx: &mut Context<Self>) {
        let Some(item) = cx.read_from_clipboard() else {
            return;
        };
        if let Some(on_paste_image) = self.on_paste_image.take() {
            let image = item.entries().iter().find_map(|entry| match entry {
                ClipboardEntry::Image(image) => Some(image.clone()),
                ClipboardEntry::String(_) => None,
            });
            if let Some(image) = image {
                on_paste_image(image, window, cx);
                self.on_paste_image = Some(on_paste_image);
                return;
            }
            self.on_paste_image = Some(on_paste_image);
        }
        if let Some(text) = item.text() {
            let text = if self.multiline {
                text.replace("\r\n", "\n")
            } else {
                text.replace('\n', " ")
            };
            self.replace_text_in_range(None, &text, window, cx);
        }
    }

    fn copy(&mut self, _: &Copy, _: &mut Window, cx: &mut Context<Self>) {
        if !self.selected_range.is_empty() {
            cx.write_to_clipboard(ClipboardItem::new_string(
                self.content[self.selected_range.clone()].to_string(),
            ));
        }
    }

    fn cut(&mut self, _: &Cut, window: &mut Window, cx: &mut Context<Self>) {
        if !self.selected_range.is_empty() {
            cx.write_to_clipboard(ClipboardItem::new_string(
                self.content[self.selected_range.clone()].to_string(),
            ));
            self.replace_text_in_range(None, "", window, cx)
        }
    }

    fn move_to(&mut self, offset: usize, cx: &mut Context<Self>) {
        self.selected_range = offset..offset;
        cx.notify()
    }

    fn cursor_offset(&self) -> usize {
        if self.selection_reversed {
            self.selected_range.start
        } else {
            self.selected_range.end
        }
    }

    /// Byte offset into the shaped display string for a content byte offset.
    fn to_display_offset(&self, offset: usize) -> usize {
        if self.mask {
            self.content[..offset.min(self.content.len())]
                .chars()
                .count()
        } else {
            offset
        }
    }

    /// Content byte offset for a display-string byte offset (mask aware).
    fn content_offset_for_display(&self, display: usize) -> usize {
        if self.mask {
            let chars = self.content.char_indices().collect::<Vec<_>>();
            chars
                .get(display)
                .map(|(i, _)| *i)
                .unwrap_or(self.content.len())
        } else {
            display.min(self.content.len())
        }
    }

    fn display_text(&self) -> SharedString {
        if self.mask {
            "*".repeat(self.content.chars().count()).into()
        } else {
            self.content.clone()
        }
    }

    fn index_for_mouse_position(&self, position: gpui::Point<Pixels>) -> usize {
        if self.content.is_empty() {
            return 0;
        }
        let (Some(bounds), Some(layout)) = (self.last_bounds.as_ref(), self.last_layout.as_ref())
        else {
            return 0;
        };
        let local = point(position.x - bounds.left(), position.y - bounds.top());
        let display = if position.y < bounds.top() {
            0
        } else if position.y > bounds.bottom() {
            layout.len()
        } else {
            layout.closest_index_for_position(local)
        };
        self.content_offset_for_display(display)
    }

    fn select_to(&mut self, offset: usize, cx: &mut Context<Self>) {
        if self.selection_reversed {
            self.selected_range.start = offset
        } else {
            self.selected_range.end = offset
        };
        if self.selected_range.end < self.selected_range.start {
            self.selection_reversed = !self.selection_reversed;
            self.selected_range = self.selected_range.end..self.selected_range.start;
        }
        cx.notify()
    }

    fn offset_from_utf16(&self, offset: usize) -> usize {
        let mut utf8_offset = 0;
        let mut utf16_count = 0;
        for ch in self.content.chars() {
            if utf16_count >= offset {
                break;
            }
            utf16_count += ch.len_utf16();
            utf8_offset += ch.len_utf8();
        }
        utf8_offset
    }

    fn offset_to_utf16(&self, offset: usize) -> usize {
        let mut utf16_offset = 0;
        let mut utf8_count = 0;
        for ch in self.content.chars() {
            if utf8_count >= offset {
                break;
            }
            utf8_count += ch.len_utf8();
            utf16_offset += ch.len_utf16();
        }
        utf16_offset
    }

    fn range_to_utf16(&self, range: &Range<usize>) -> Range<usize> {
        self.offset_to_utf16(range.start)..self.offset_to_utf16(range.end)
    }

    fn range_from_utf16(&self, range_utf16: &Range<usize>) -> Range<usize> {
        self.offset_from_utf16(range_utf16.start)..self.offset_from_utf16(range_utf16.end)
    }

    fn previous_boundary(&self, offset: usize) -> usize {
        self.content
            .grapheme_indices(true)
            .rev()
            .find_map(|(idx, _)| (idx < offset).then_some(idx))
            .unwrap_or(0)
    }

    fn next_boundary(&self, offset: usize) -> usize {
        self.content
            .grapheme_indices(true)
            .find_map(|(idx, _)| (idx > offset).then_some(idx))
            .unwrap_or(self.content.len())
    }
}

impl EntityInputHandler for TextInput {
    fn text_for_range(
        &mut self,
        range_utf16: Range<usize>,
        actual_range: &mut Option<Range<usize>>,
        _window: &mut Window,
        _cx: &mut Context<Self>,
    ) -> Option<String> {
        let range = self.range_from_utf16(&range_utf16);
        actual_range.replace(self.range_to_utf16(&range));
        Some(self.content[range].to_string())
    }

    fn selected_text_range(
        &mut self,
        _ignore_disabled_input: bool,
        _window: &mut Window,
        _cx: &mut Context<Self>,
    ) -> Option<UTF16Selection> {
        Some(UTF16Selection {
            range: self.range_to_utf16(&self.selected_range),
            reversed: self.selection_reversed,
        })
    }

    fn marked_text_range(
        &self,
        _window: &mut Window,
        _cx: &mut Context<Self>,
    ) -> Option<Range<usize>> {
        self.marked_range
            .as_ref()
            .map(|range| self.range_to_utf16(range))
    }

    fn unmark_text(&mut self, _window: &mut Window, _cx: &mut Context<Self>) {
        self.marked_range = None;
    }

    fn replace_text_in_range(
        &mut self,
        range_utf16: Option<Range<usize>>,
        new_text: &str,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let range = range_utf16
            .as_ref()
            .map(|range_utf16| self.range_from_utf16(range_utf16))
            .or(self.marked_range.clone())
            .unwrap_or(self.selected_range.clone());

        self.content =
            (self.content[0..range.start].to_owned() + new_text + &self.content[range.end..])
                .into();
        self.selected_range = range.start + new_text.len()..range.start + new_text.len();
        self.marked_range.take();
        cx.notify();
    }

    fn replace_and_mark_text_in_range(
        &mut self,
        range_utf16: Option<Range<usize>>,
        new_text: &str,
        new_selected_range_utf16: Option<Range<usize>>,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let range = range_utf16
            .as_ref()
            .map(|range_utf16| self.range_from_utf16(range_utf16))
            .or(self.marked_range.clone())
            .unwrap_or(self.selected_range.clone());

        self.content =
            (self.content[0..range.start].to_owned() + new_text + &self.content[range.end..])
                .into();
        if !new_text.is_empty() {
            self.marked_range = Some(range.start..range.start + new_text.len());
        } else {
            self.marked_range = None;
        }
        self.selected_range = new_selected_range_utf16
            .as_ref()
            .map(|range_utf16| self.range_from_utf16(range_utf16))
            .map(|new_range| new_range.start + range.start..new_range.end + range.end)
            .unwrap_or_else(|| range.start + new_text.len()..range.start + new_text.len());

        cx.notify();
    }

    fn bounds_for_range(
        &mut self,
        range_utf16: Range<usize>,
        bounds: Bounds<Pixels>,
        _window: &mut Window,
        _cx: &mut Context<Self>,
    ) -> Option<Bounds<Pixels>> {
        let layout = self.last_layout.as_ref()?;
        let range = self.range_from_utf16(&range_utf16);
        let start = layout.position_for_index(self.to_display_offset(range.start))?;
        let end = layout.position_for_index(self.to_display_offset(range.end))?;
        Some(Bounds::from_corners(
            point(bounds.left() + start.x, bounds.top() + start.y),
            point(
                bounds.left() + end.x,
                bounds.top() + end.y + layout.line_height,
            ),
        ))
    }

    fn character_index_for_point(
        &mut self,
        point: gpui::Point<Pixels>,
        _window: &mut Window,
        _cx: &mut Context<Self>,
    ) -> Option<usize> {
        let bounds = self.last_bounds?;
        let layout = self.last_layout.as_ref()?;
        if !self.mask && layout.text != self.content {
            // Layout is from a previous frame; report no hit instead of
            // slicing with stale offsets.
            return None;
        }
        let local = gpui::point(point.x - bounds.left(), point.y - bounds.top());
        let display = layout.closest_index_for_position(local);
        Some(self.offset_to_utf16(self.content_offset_for_display(display)))
    }
}

/// Shaped paragraphs of one input plus the offsets needed to map between
/// byte indices and pixel positions across newlines and wrap rows.
struct TextLayout {
    lines: Vec<WrappedLine>,
    line_height: Pixels,
    text: SharedString,
}

impl TextLayout {
    fn len(&self) -> usize {
        self.text.len()
    }

    fn height(&self) -> Pixels {
        self.lines
            .iter()
            .map(|line| line.size(self.line_height).height)
            .fold(px(0.), |acc, h| acc + h)
            .max(self.line_height)
    }

    fn position_for_index(&self, index: usize) -> Option<gpui::Point<Pixels>> {
        let mut y = px(0.);
        let mut start = 0;
        for line in &self.lines {
            let end = start + line.len();
            if index <= end {
                let local = line.position_for_index(index - start, self.line_height)?;
                return Some(point(local.x, local.y + y));
            }
            y += line.size(self.line_height).height;
            start = end + 1;
        }
        Some(point(px(0.), y))
    }

    fn closest_index_for_position(&self, position: gpui::Point<Pixels>) -> usize {
        let mut y = px(0.);
        let mut start = 0;
        let last = self.lines.len().saturating_sub(1);
        for (i, line) in self.lines.iter().enumerate() {
            let height = line.size(self.line_height).height;
            if position.y < y + height || i == last {
                let local = point(position.x, (position.y - y).max(px(0.)));
                let index = match line.closest_index_for_position(local, self.line_height) {
                    Ok(index) | Err(index) => index,
                };
                return start + index.min(line.len());
            }
            y += height;
            start += line.len() + 1;
        }
        self.text.len()
    }
}

struct TextElement {
    input: Entity<TextInput>,
}

struct PrepaintState {
    layout: Option<TextLayout>,
    scroll_x: Pixels,
    cursor: Option<PaintQuad>,
    selection: Vec<PaintQuad>,
}

impl IntoElement for TextElement {
    type Element = Self;

    fn into_element(self) -> Self::Element {
        self
    }
}

impl Element for TextElement {
    type RequestLayoutState = ();
    type PrepaintState = PrepaintState;

    fn id(&self) -> Option<ElementId> {
        None
    }

    fn source_location(&self) -> Option<&'static core::panic::Location<'static>> {
        None
    }

    fn request_layout(
        &mut self,
        _id: Option<&GlobalElementId>,
        _inspector_id: Option<&gpui::InspectorElementId>,
        window: &mut Window,
        cx: &mut App,
    ) -> (LayoutId, Self::RequestLayoutState) {
        let mut style = Style::default();
        style.size.width = relative(1.).into();
        let input = self.input.read(cx);
        if !input.multiline {
            style.size.height = window.line_height().into();
            return (window.request_layout(style, [], cx), ());
        }
        if input.fill_height {
            style.size.height = relative(1.).into();
            style.min_size.height = window.line_height().into();
            return (window.request_layout(style, [], cx), ());
        }
        // Grow with the wrapped content, one to `max_lines` rows.
        let entity = self.input.clone();
        let layout = window.request_measured_layout(style, move |known, available, window, cx| {
            let input = entity.read(cx);
            let text = if input.content.is_empty() {
                input.placeholder.clone()
            } else {
                input.display_text()
            };
            let line_height = window.line_height();
            let width = known.width.or(match available.width {
                gpui::AvailableSpace::Definite(width) => Some(width),
                _ => None,
            });
            let cache_width = width.unwrap_or(px(0.));
            if let Some((cached_text, cached_width, rows)) = input.measure_cache.as_ref()
                && *cached_text == text
                && *cached_width == cache_width
            {
                return size(cache_width, line_height * *rows as f32);
            }
            let style = window.text_style();
            let font_size = style.font_size.to_pixels(window.rem_size());
            let run = TextRun {
                len: text.len(),
                font: style.font(),
                color: style.color,
                background_color: None,
                underline: None,
                strikethrough: None,
            };
            let rows = window
                .text_system()
                .shape_text(text.clone(), font_size, &[run], width, None)
                .map(|lines| {
                    lines
                        .iter()
                        .map(|line| line.wrap_boundaries().len() + 1)
                        .sum::<usize>()
                })
                .unwrap_or(1)
                .clamp(1, input.max_lines);
            entity.update(cx, |input, _| {
                input.measure_cache = Some((text, cache_width, rows));
            });
            size(cache_width, line_height * rows as f32)
        });
        (layout, ())
    }

    fn prepaint(
        &mut self,
        _id: Option<&GlobalElementId>,
        _inspector_id: Option<&gpui::InspectorElementId>,
        bounds: Bounds<Pixels>,
        _request_layout: &mut Self::RequestLayoutState,
        window: &mut Window,
        cx: &mut App,
    ) -> Self::PrepaintState {
        let input = self.input.read(cx);
        let content = input.content.clone();
        let selected_range = input.selected_range.clone();
        let cursor = input.cursor_offset();
        let mask = input.mask;
        let style = window.text_style();

        let (display_text, text_color) = if content.is_empty() {
            (input.placeholder.clone(), theme::placeholder())
        } else {
            (input.display_text(), style.color)
        };

        let run = TextRun {
            len: display_text.len(),
            font: style.font(),
            color: text_color,
            background_color: None,
            underline: None,
            strikethrough: None,
        };
        let runs = if let Some(marked_range) = input.marked_range.as_ref() {
            vec![
                TextRun {
                    len: marked_range.start,
                    ..run.clone()
                },
                TextRun {
                    len: marked_range.end - marked_range.start,
                    underline: Some(UnderlineStyle {
                        color: Some(run.color),
                        thickness: px(1.0),
                        wavy: false,
                    }),
                    ..run.clone()
                },
                TextRun {
                    len: display_text.len() - marked_range.end,
                    ..run
                },
            ]
            .into_iter()
            .filter(|run| run.len > 0)
            .collect()
        } else {
            vec![run]
        };

        let font_size = style.font_size.to_pixels(window.rem_size());
        let line_height = window.line_height();
        let wrap_width = input.multiline.then_some(bounds.size.width);
        let lines = window
            .text_system()
            .shape_text(display_text.clone(), font_size, &runs, wrap_width, None)
            .map(|lines| lines.into_vec())
            .unwrap_or_default();
        let layout = TextLayout {
            lines,
            line_height,
            text: display_text,
        };

        let display_cursor = if mask {
            content[..cursor.min(content.len())].chars().count()
        } else {
            cursor
        };
        // Single-line inputs do not wrap. Scroll so the cursor stays in
        // view and clip the paint to the box.
        let scroll_x = if input.multiline {
            px(0.)
        } else {
            let cursor_x = layout
                .position_for_index(display_cursor)
                .map(|p| p.x)
                .unwrap_or_default();
            let text_width = layout
                .lines
                .iter()
                .map(|line| line.size(line_height).width)
                .fold(px(0.), |acc, w| acc.max(w));
            let width = bounds.size.width;
            let mut sx = input.scroll_x;
            if cursor_x - sx > width - px(2.) {
                sx = cursor_x - width + px(2.);
            }
            if cursor_x - sx < px(0.) {
                sx = cursor_x;
            }
            sx.min((text_width - width + px(2.)).max(px(0.)))
                .max(px(0.))
        };
        let text_bounds = Bounds::new(point(bounds.left() - scroll_x, bounds.top()), bounds.size);
        let (selection, cursor) = if selected_range.is_empty() {
            let cursor_pos = layout
                .position_for_index(display_cursor)
                .unwrap_or_default();
            (
                Vec::new(),
                Some(fill(
                    Bounds::new(
                        point(
                            text_bounds.left() + cursor_pos.x,
                            text_bounds.top() + cursor_pos.y,
                        ),
                        size(px(2.), line_height),
                    ),
                    rgb(0xe7e7ea),
                )),
            )
        } else {
            let start = if mask {
                content[..selected_range.start.min(content.len())]
                    .chars()
                    .count()
            } else {
                selected_range.start
            };
            let end = if mask {
                content[..selected_range.end.min(content.len())]
                    .chars()
                    .count()
            } else {
                selected_range.end
            };
            let start_pos = layout.position_for_index(start).unwrap_or_default();
            let end_pos = layout.position_for_index(end).unwrap_or_default();
            let color = rgba(0x4a7dff40);
            let mut quads = Vec::new();
            if start_pos.y == end_pos.y {
                quads.push(fill(
                    Bounds::from_corners(
                        point(
                            text_bounds.left() + start_pos.x,
                            text_bounds.top() + start_pos.y,
                        ),
                        point(
                            text_bounds.left() + end_pos.x,
                            text_bounds.top() + start_pos.y + line_height,
                        ),
                    ),
                    color,
                ));
            } else {
                // First row to the right edge, full middle rows, then the
                // last row from the left edge.
                quads.push(fill(
                    Bounds::from_corners(
                        point(
                            text_bounds.left() + start_pos.x,
                            text_bounds.top() + start_pos.y,
                        ),
                        point(
                            text_bounds.right(),
                            text_bounds.top() + start_pos.y + line_height,
                        ),
                    ),
                    color,
                ));
                let mut y = start_pos.y + line_height;
                while y < end_pos.y {
                    quads.push(fill(
                        Bounds::from_corners(
                            point(text_bounds.left(), text_bounds.top() + y),
                            point(text_bounds.right(), text_bounds.top() + y + line_height),
                        ),
                        color,
                    ));
                    y += line_height;
                }
                quads.push(fill(
                    Bounds::from_corners(
                        point(text_bounds.left(), text_bounds.top() + end_pos.y),
                        point(
                            text_bounds.left() + end_pos.x,
                            text_bounds.top() + end_pos.y + line_height,
                        ),
                    ),
                    color,
                ));
            }
            (quads, None)
        };
        if !input.multiline && input.scroll_x != scroll_x {
            self.input.update(cx, |input, _| input.scroll_x = scroll_x);
        }
        PrepaintState {
            layout: Some(layout),
            scroll_x,
            cursor,
            selection,
        }
    }

    fn paint(
        &mut self,
        _id: Option<&GlobalElementId>,
        _inspector_id: Option<&gpui::InspectorElementId>,
        bounds: Bounds<Pixels>,
        _request_layout: &mut Self::RequestLayoutState,
        prepaint: &mut Self::PrepaintState,
        window: &mut Window,
        cx: &mut App,
    ) {
        let focus_handle = self.input.read(cx).focus_handle.clone();
        window.handle_input(
            &focus_handle,
            ElementInputHandler::new(bounds, self.input.clone()),
            cx,
        );
        let layout = prepaint.layout.take().unwrap();
        let text_bounds = Bounds::new(
            point(bounds.left() - prepaint.scroll_x, bounds.top()),
            bounds.size,
        );
        let focused = focus_handle.is_focused(window);
        let selection = std::mem::take(&mut prepaint.selection);
        let cursor = prepaint.cursor.take();
        window.with_content_mask(Some(ContentMask { bounds }), |window| {
            for quad in selection {
                window.paint_quad(quad)
            }
            let mut origin = text_bounds.origin;
            for line in &layout.lines {
                line.paint(
                    origin,
                    layout.line_height,
                    TextAlign::Left,
                    None,
                    window,
                    cx,
                )
                .ok();
                origin.y += line.size(layout.line_height).height;
            }
            if focused && let Some(cursor) = cursor {
                window.paint_quad(cursor);
            }
        });

        self.input.update(cx, |input, _cx| {
            input.last_layout = Some(layout);
            input.last_bounds = Some(text_bounds);
        });
    }
}

impl Render for TextInput {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        div()
            .flex()
            .key_context("TextInput")
            .track_focus(&self.focus_handle(cx))
            .cursor(CursorStyle::IBeam)
            .on_action(cx.listener(Self::backspace))
            .on_action(cx.listener(Self::delete))
            .on_action(cx.listener(Self::left))
            .on_action(cx.listener(Self::right))
            .on_action(cx.listener(Self::select_left))
            .on_action(cx.listener(Self::select_right))
            .on_action(cx.listener(Self::select_all))
            .on_action(cx.listener(Self::home))
            .on_action(cx.listener(Self::end))
            .on_action(cx.listener(Self::up))
            .on_action(cx.listener(Self::down))
            .on_action(cx.listener(Self::show_character_palette))
            .on_action(cx.listener(Self::paste))
            .on_action(cx.listener(Self::cut))
            .on_action(cx.listener(Self::copy))
            .on_key_down(cx.listener(|this, event: &gpui::KeyDownEvent, window, cx| {
                if let Some(on_key) = this.on_key.take() {
                    let consumed = on_key(event, &this.content.clone(), window, cx);
                    this.on_key = Some(on_key);
                    if consumed {
                        cx.stop_propagation();
                        return;
                    }
                }
                let no_modifiers = !event.keystroke.modifiers.control
                    && !event.keystroke.modifiers.alt
                    && !event.keystroke.modifiers.platform;
                if event.keystroke.key.eq_ignore_ascii_case("tab")
                    && no_modifiers
                    && this.marked_range.is_none()
                {
                    if event.keystroke.modifiers.shift {
                        window.focus_prev();
                    } else {
                        window.focus_next();
                    }
                    cx.stop_propagation();
                }
                let is_enter = event.keystroke.key == "enter" && this.marked_range.is_none();
                if is_enter && no_modifiers && event.keystroke.modifiers.shift && this.multiline {
                    this.replace_text_in_range(None, "\n", window, cx);
                    cx.stop_propagation();
                    return;
                }
                let plain_enter = is_enter && no_modifiers && !event.keystroke.modifiers.shift;
                if plain_enter {
                    this.enter_pressed(window, cx);
                    // Stop the platform text input from also inserting a
                    // newline for the unhandled Return keystroke.
                    cx.stop_propagation();
                }
            }))
            .when_some(self.tab_index, |el, index| el.tab_index(index))
            .on_mouse_down(gpui::MouseButton::Left, cx.listener(Self::on_mouse_down))
            .on_mouse_up(gpui::MouseButton::Left, cx.listener(Self::on_mouse_up))
            .on_mouse_up_out(gpui::MouseButton::Left, cx.listener(Self::on_mouse_up))
            .on_mouse_move(cx.listener(Self::on_mouse_move))
            .w_full()
            .when(self.multiline && self.fill_height, |el| el.h_full())
            .child(TextElement { input: cx.entity() })
    }
}

impl Focusable for TextInput {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus_handle.clone()
    }
}

#[allow(dead_code)]
fn _unused_keystroke_assert(_: &Keystroke) {}
