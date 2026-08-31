//! Text input, ported from the gpui 0.2.2 `input` example and adapted for
//! Maple: neutral theming, optional password masking, an Enter-key hook for
//! form submit and composer send, and an optional multi-line mode that wraps
//! text and grows with its content (Shift+Enter inserts a newline).

use super::{spell, theme, widgets};
use std::collections::VecDeque;
use std::ops::Range;

use gpui::{
    App, Bounds, ClipboardEntry, ClipboardItem, ContentMask, Context, CursorStyle, Element,
    ElementId, ElementInputHandler, Entity, EntityInputHandler, FocusHandle, Focusable,
    GlobalElementId, InteractiveElement, KeyBinding, LayoutId, MouseDownEvent, MouseMoveEvent,
    MouseUpEvent, PaintQuad, Pixels, SharedString, Style, TextAlign, TextRun, UTF16Selection,
    UnderlineStyle, Window, WrappedLine, actions, div, fill, point, prelude::*, px, relative, rgb,
    size,
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
        Undo,
        Redo,
    ]
);

/// Register the default key bindings for every TextInput. Safe to call
/// once at startup; `secondary-` is cmd on macOS and ctrl elsewhere.
pub fn register_key_bindings(cx: &mut App) {
    let context = Some("TextInput");
    cx.bind_keys([
        KeyBinding::new("backspace", Backspace, context),
        KeyBinding::new("delete", Delete, context),
        KeyBinding::new("left", Left, context),
        KeyBinding::new("right", Right, context),
        KeyBinding::new("shift-left", SelectLeft, context),
        KeyBinding::new("shift-right", SelectRight, context),
        KeyBinding::new("secondary-a", SelectAll, context),
        KeyBinding::new("secondary-v", Paste, context),
        KeyBinding::new("secondary-c", Copy, context),
        KeyBinding::new("secondary-x", Cut, context),
        KeyBinding::new("home", Home, context),
        KeyBinding::new("end", End, context),
        KeyBinding::new("up", Up, context),
        KeyBinding::new("down", Down, context),
        KeyBinding::new("secondary-z", Undo, context),
        KeyBinding::new("secondary-shift-z", Redo, context),
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
    /// Vertical scroll of a multi-line input so text taller than the box
    /// stays reachable. Positive values scroll down.
    scroll_y: Pixels,
    /// Scroll the cursor into view on the next prepaint. Set by edits and
    /// cursor moves; a wheel scroll leaves it unset so the view stays put.
    keep_cursor_visible: bool,
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
    /// Shaped lines from the last prepaint with the inputs they came
    /// from; reused while nothing that affects shaping has changed.
    shape_cache: Option<ShapeCache>,
    is_selecting: bool,
    /// Window position of the right-click menu while it is open.
    context_menu: Option<gpui::Point<Pixels>>,
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
    /// Underline words the dictionary rejects (composer only).
    spell_check: bool,
    /// Byte ranges of misspelled words, refreshed on every content change
    /// so prepaint only reads it.
    misspelled: Vec<Range<usize>>,
    /// `spell::generation()` at the last check. Text set before the
    /// dictionary finished loading is re-checked when this falls behind.
    spell_generation: u32,
    /// The misspelled word under the open right-click menu, with its
    /// replacement candidates.
    spell_menu: Option<(Range<usize>, Vec<String>)>,
    /// Text states before each edit, oldest first. Capped at
    /// [`UNDO_DEPTH`]; the oldest goes when the cap is reached.
    undo_stack: VecDeque<EditSnapshot>,
    /// States undone so far, newest last. An edit clears them.
    redo_stack: Vec<EditSnapshot>,
    /// Where the last edit left the cursor, so a run of typing or
    /// deleting at that point folds into one undo step.
    last_edit: Option<EditAnchor>,
}

/// How many text states one input remembers for undo.
const UNDO_DEPTH: usize = 128;

/// The content and selection before an edit.
#[derive(Clone)]
struct EditSnapshot {
    content: SharedString,
    selected_range: Range<usize>,
    selection_reversed: bool,
}

/// The kinds of edit that fold into one undo step. Everything else
/// (a paste, an edit over a selection, an IME composition, new text
/// set from code) starts its own step.
#[derive(Clone, Copy, PartialEq, Eq)]
enum EditKind {
    Insert,
    Delete,
}

/// The last edit and the cursor it left behind.
#[derive(Clone, Copy, PartialEq, Eq)]
struct EditAnchor {
    kind: EditKind,
    offset: usize,
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
            scroll_y: px(0.),
            keep_cursor_visible: false,
            multiline: false,
            fill_height: false,
            max_lines: 8,
            measure_cache: None,
            shape_cache: None,
            is_selecting: false,
            context_menu: None,
            mask: false,
            tab_index: None,
            on_enter: None,
            on_paste_image: None,
            on_key: None,
            spell_check: false,
            misspelled: Vec::new(),
            spell_generation: 0,
            spell_menu: None,
            undo_stack: VecDeque::new(),
            redo_stack: Vec::new(),
            last_edit: None,
        }
    }

    /// Underline misspelled words and offer replacements in the
    /// right-click menu.
    pub fn spell_check(mut self) -> Self {
        self.spell_check = true;
        self
    }

    /// Recompute the misspelled ranges. Call after every content change.
    fn refresh_spelling(&mut self) {
        if self.spell_check && !self.mask {
            self.spell_generation = spell::generation();
            self.misspelled = spell::misspelled_ranges(&self.content);
        }
    }

    /// Replace the word under the right-click menu with `replacement`.
    fn apply_suggestion(&mut self, range: Range<usize>, replacement: &str, cx: &mut Context<Self>) {
        if self.content.get(range.clone()).is_none() {
            return;
        }
        self.record_edit(false);
        self.last_edit = None;
        self.content =
            (self.content[..range.start].to_owned() + replacement + &self.content[range.end..])
                .into();
        let end = range.start + replacement.len();
        self.selected_range = end..end;
        self.selection_reversed = false;
        self.refresh_spelling();
        cx.notify();
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

    pub fn set_placeholder(&mut self, text: &str, cx: &mut Context<Self>) {
        self.placeholder = SharedString::from(text.to_string());
        cx.notify();
    }

    pub fn set_text(&mut self, text: &str, cx: &mut Context<Self>) {
        self.record_edit(false);
        self.last_edit = None;
        self.content = SharedString::from(text.to_string());
        self.selected_range = self.content.len()..self.content.len();
        self.keep_cursor_visible = true;
        self.forget_text_positions();
        self.refresh_spelling();
        cx.notify();
    }

    pub fn clear(&mut self, cx: &mut Context<Self>) {
        self.forget_edits();
        self.content = "".into();
        self.selected_range = 0..0;
        self.scroll_y = px(0.);
        self.forget_text_positions();
        self.misspelled.clear();
        cx.notify();
    }

    /// Drop every state that holds a byte range into the old content: an
    /// IME composition, a reversed selection, and the menus whose items
    /// point at a word.
    fn forget_text_positions(&mut self) {
        self.marked_range = None;
        self.selection_reversed = false;
        self.spell_menu = None;
        self.context_menu = None;
    }

    /// The core of an edit: swap `range` for `new_text`, record the undo
    /// step, and refresh what the old byte ranges pointed at.
    fn replace_range(&mut self, range: Range<usize>, new_text: &str, cx: &mut Context<Self>) {
        // An IME composition was already recorded when it started; a
        // single typed or deleted character continues the run the last
        // one started; anything else is its own undo step.
        let composing = self.marked_range.is_some();
        let kind = if composing {
            None
        } else if new_text.is_empty() && !range.is_empty() {
            Some(EditKind::Delete)
        } else if range.is_empty() && new_text.chars().count() == 1 && new_text != "\n" {
            Some(EditKind::Insert)
        } else {
            None
        };
        if !composing {
            let continues = match (kind, self.last_edit) {
                (Some(EditKind::Insert), Some(last)) => {
                    last.kind == EditKind::Insert && last.offset == range.start
                }
                (Some(EditKind::Delete), Some(last)) => {
                    last.kind == EditKind::Delete
                        && (last.offset == range.start || last.offset == range.end)
                }
                _ => false,
            };
            self.record_edit(continues);
        }

        self.content =
            (self.content[0..range.start].to_owned() + new_text + &self.content[range.end..])
                .into();
        self.selected_range = range.start + new_text.len()..range.start + new_text.len();
        self.last_edit = kind.map(|kind| EditAnchor {
            kind,
            offset: self.selected_range.start,
        });
        self.marked_range.take();
        // Menu items hold ranges into the old text.
        self.spell_menu = None;
        self.context_menu = None;
        self.keep_cursor_visible = true;
        self.refresh_spelling();
        cx.notify();
    }

    fn snapshot(&self) -> EditSnapshot {
        EditSnapshot {
            content: self.content.clone(),
            selected_range: self.selected_range.clone(),
            selection_reversed: self.selection_reversed,
        }
    }

    /// Keep the text as it is now, so undo can come back to it. A step
    /// that continues the run the last edit started is folded into it;
    /// every recorded step drops the redo history.
    fn record_edit(&mut self, continues_run: bool) {
        if !continues_run {
            if self.undo_stack.len() == UNDO_DEPTH {
                self.undo_stack.pop_front();
            }
            self.undo_stack.push_back(self.snapshot());
        }
        self.redo_stack.clear();
    }

    /// Forget the edit history. For content that is replaced wholesale
    /// and must not come back, such as a sent composer message.
    fn forget_edits(&mut self) {
        self.undo_stack.clear();
        self.redo_stack.clear();
        self.last_edit = None;
    }

    fn restore(&mut self, snapshot: EditSnapshot, cx: &mut Context<Self>) {
        self.content = snapshot.content;
        self.forget_text_positions();
        self.selected_range = snapshot.selected_range;
        self.selection_reversed = snapshot.selection_reversed;
        self.last_edit = None;
        self.keep_cursor_visible = true;
        self.refresh_spelling();
        cx.notify();
    }

    fn undo_last(&mut self, cx: &mut Context<Self>) {
        let Some(previous) = self.undo_stack.pop_back() else {
            return;
        };
        self.redo_stack.push(self.snapshot());
        self.restore(previous, cx);
    }

    fn redo_last(&mut self, cx: &mut Context<Self>) {
        let Some(next) = self.redo_stack.pop() else {
            return;
        };
        self.undo_stack.push_back(self.snapshot());
        self.restore(next, cx);
    }

    fn undo(&mut self, _: &Undo, _: &mut Window, cx: &mut Context<Self>) {
        self.undo_last(cx);
    }

    fn redo(&mut self, _: &Redo, _: &mut Window, cx: &mut Context<Self>) {
        self.redo_last(cx);
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
        self.context_menu = None;
        let index = self.index_for_mouse_position(event.position);
        match event.click_count {
            // A drag only follows a single click; jitter after a double
            // click must not collapse the word selection.
            2 => {
                self.is_selecting = false;
                self.select_word_at(index, cx);
            }
            count if count >= 3 => {
                self.is_selecting = false;
                self.move_to(0, cx);
                self.select_to(self.content.len(), cx);
            }
            _ => {
                self.is_selecting = true;
                if event.modifiers.shift {
                    self.select_to(index, cx);
                } else {
                    self.move_to(index, cx)
                }
            }
        }
    }

    /// Select the word (or run of spaces) that contains byte `index`.
    fn select_word_at(&mut self, index: usize, cx: &mut Context<Self>) {
        let index = index.min(self.content.len());
        let range = self
            .content
            .split_word_bound_indices()
            .map(|(start, word)| start..start + word.len())
            .find(|range| index < range.end)
            .or_else(|| {
                self.content
                    .split_word_bound_indices()
                    .next_back()
                    .map(|(start, word)| start..start + word.len())
            })
            .unwrap_or(0..0);
        self.move_to(range.start, cx);
        self.select_to(range.end, cx);
    }

    fn on_right_click(
        &mut self,
        event: &MouseDownEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.is_selecting = false;
        window.focus(&self.focus_handle);
        self.context_menu = Some(event.position);
        let index = self.index_for_mouse_position(event.position);
        self.spell_menu = self
            .misspelled
            .iter()
            .find(|range| range.start <= index && index <= range.end)
            .cloned()
            .map(|range| {
                let suggestions = spell::suggestions(&self.content[range.clone()], 4);
                (range, suggestions)
            });
        cx.notify();
    }

    /// Right-click menu: cut, copy, paste, select all.
    fn render_context_menu(&self, cx: &mut Context<Self>) -> Option<gpui::Deferred> {
        let position = self.context_menu?;
        let has_selection = !self.selected_range.is_empty();
        let suggestions: Vec<_> = self
            .spell_menu
            .as_ref()
            .map(|(range, suggestions)| {
                suggestions
                    .iter()
                    .enumerate()
                    .map(|(i, suggestion)| {
                        let range = range.clone();
                        let replacement = suggestion.clone();
                        let label = SharedString::from(suggestion.clone());
                        widgets::menu_row(
                            ElementId::NamedInteger("text-input-suggest".into(), i as u64),
                            true,
                        )
                        .on_click(cx.listener(move |this, _event, _window, cx| {
                            cx.stop_propagation();
                            this.context_menu = None;
                            this.spell_menu = None;
                            this.apply_suggestion(range.clone(), &replacement, cx);
                        }))
                        .child(label)
                    })
                    .collect()
            })
            .unwrap_or_default();
        let add_word = self.spell_menu.as_ref().map(|(range, _)| {
            let range = range.clone();
            widgets::menu_row("text-input-add-word", true)
                .on_click(cx.listener(move |this, _event, _window, cx| {
                    cx.stop_propagation();
                    this.context_menu = None;
                    this.spell_menu = None;
                    if let Some(word) = this.content.get(range.clone()) {
                        spell::add_word(word);
                    }
                    this.refresh_spelling();
                    cx.notify();
                }))
                .child("Add to dictionary")
        });
        let has_spell_items = add_word.is_some();
        let item = |id: &'static str,
                    label: &'static str,
                    enabled: bool,
                    action: fn(&mut Self, &mut Window, &mut Context<Self>)| {
            widgets::menu_row(id, enabled)
                .when(enabled, |item| {
                    item.on_click(cx.listener(move |this, _event, window, cx| {
                        cx.stop_propagation();
                        this.context_menu = None;
                        action(this, window, cx);
                        cx.notify();
                    }))
                })
                .child(label)
        };
        Some(gpui::deferred(
            gpui::anchored()
                .position(position)
                .snap_to_window_with_margin(px(8.))
                .child(
                    widgets::popup_panel("text-input-menu", px(140.))
                        .on_mouse_down_out(cx.listener(|this, _event, _window, cx| {
                            this.context_menu = None;
                            this.spell_menu = None;
                            cx.notify();
                        }))
                        .children(suggestions)
                        .children(add_word)
                        .when(has_spell_items, |menu| {
                            menu.child(div().my_1().h(px(1.)).bg(gpui::rgb(theme::border())))
                        })
                        .child(item(
                            "text-input-cut",
                            "Cut",
                            has_selection,
                            |this, window, cx| this.cut(&Cut, window, cx),
                        ))
                        .child(item(
                            "text-input-copy",
                            "Copy",
                            has_selection,
                            |this, window, cx| this.copy(&Copy, window, cx),
                        ))
                        .child(item(
                            "text-input-paste",
                            "Paste",
                            true,
                            |this, window, cx| this.paste(&Paste, window, cx),
                        ))
                        .child(item(
                            "text-input-select-all",
                            "Select all",
                            !self.content.is_empty(),
                            |this, window, cx| this.select_all(&SelectAll, window, cx),
                        )),
                ),
        ))
    }

    fn on_mouse_up(&mut self, _: &MouseUpEvent, _window: &mut Window, _: &mut Context<Self>) {
        self.is_selecting = false;
    }

    fn on_mouse_move(&mut self, event: &MouseMoveEvent, _: &mut Window, cx: &mut Context<Self>) {
        if self.is_selecting {
            self.select_to(self.index_for_mouse_position(event.position), cx);
        }
    }

    /// Wheel-scroll a multi-line input whose text is taller than its box.
    /// The event is consumed only when there is something to scroll, so a
    /// wheel over a short input still reaches the surface behind it.
    fn on_scroll_wheel(
        &mut self,
        event: &gpui::ScrollWheelEvent,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let (Some(layout), Some(bounds)) = (self.last_layout.as_ref(), self.last_bounds.as_ref())
        else {
            return;
        };
        let max_scroll = layout.height() - bounds.size.height;
        if max_scroll <= px(0.) {
            return;
        }
        cx.stop_propagation();
        let delta = event.delta.pixel_delta(layout.line_height).y;
        let next = (self.scroll_y - delta).min(max_scroll).max(px(0.));
        if next != self.scroll_y {
            self.scroll_y = next;
            cx.notify();
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
        let offset = snap_to_char_boundary(&self.content, offset);
        self.selected_range = offset..offset;
        self.keep_cursor_visible = true;
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
        if position.y < bounds.top() {
            return 0;
        }
        if position.y > bounds.bottom() || !self.layout_is_current(layout) {
            // Below the box, or the layout is from a frame before the text
            // changed: its offsets no longer describe `content`.
            return self.content.len();
        }
        let local = point(position.x - bounds.left(), position.y - bounds.top());
        let display = layout.closest_index_for_position(local);
        self.content_offset_for_display(display)
    }

    /// Whether `layout` was shaped from the current content. A masked
    /// input shapes one `*` per char, so only the lengths can be compared.
    fn layout_is_current(&self, layout: &TextLayout) -> bool {
        if self.mask {
            layout.text.len() == self.content.chars().count()
        } else {
            layout.text == self.content
        }
    }

    fn select_to(&mut self, offset: usize, cx: &mut Context<Self>) {
        let offset = snap_to_char_boundary(&self.content, offset);
        if self.selection_reversed {
            self.selected_range.start = offset
        } else {
            self.selected_range.end = offset
        };
        if self.selected_range.end < self.selected_range.start {
            self.selection_reversed = !self.selection_reversed;
            self.selected_range = self.selected_range.end..self.selected_range.start;
        }
        self.keep_cursor_visible = true;
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
        self.replace_range(range, new_text, cx);
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

        // The whole composition is one undo step: record the text as it
        // was before the first marked update.
        if self.marked_range.is_none() {
            self.record_edit(false);
        }
        self.last_edit = None;

        self.content =
            (self.content[0..range.start].to_owned() + new_text + &self.content[range.end..])
                .into();
        if !new_text.is_empty() {
            self.marked_range = Some(range.start..range.start + new_text.len());
        } else {
            self.marked_range = None;
        }
        // The IME reports the selection relative to the marked text.
        let len = self.content.len();
        self.selected_range = new_selected_range_utf16
            .as_ref()
            .map(|range_utf16| self.range_from_utf16(range_utf16))
            .map(|new_range| {
                (new_range.start + range.start).min(len)..(new_range.end + range.start).min(len)
            })
            .unwrap_or_else(|| range.start + new_text.len()..range.start + new_text.len());
        self.spell_menu = None;
        self.context_menu = None;
        self.keep_cursor_visible = true;

        self.refresh_spelling();
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
        if !self.layout_is_current(layout) {
            // Layout is from a previous frame; report no hit instead of
            // slicing with stale offsets.
            return None;
        }
        let local = gpui::point(point.x - bounds.left(), point.y - bounds.top());
        let display = layout.closest_index_for_position(local);
        Some(self.offset_to_utf16(self.content_offset_for_display(display)))
    }
}

/// `offset` clamped to `text` and moved back to the nearest char boundary,
/// so a cursor from a stale layout or a mouse hit can never split a
/// multi-byte character.
fn snap_to_char_boundary(text: &str, offset: usize) -> usize {
    let mut offset = offset.min(text.len());
    while !text.is_char_boundary(offset) {
        offset -= 1;
    }
    offset
}

/// Everything `shape_text` was given last time, and what it returned.
/// A cursor move or a blink must not re-shape the text: prepaint runs
/// on every notify, and shaping is the expensive part of it.
struct ShapeCache {
    text: SharedString,
    wrap_width: Option<Pixels>,
    font_size: Pixels,
    /// Runs carry the font, color, and underline ranges, so a theme
    /// switch or a new misspelling misses the cache as it should.
    runs: Vec<TextRun>,
    lines: Vec<WrappedLine>,
}

impl ShapeCache {
    fn matches(
        &self,
        text: &SharedString,
        wrap_width: Option<Pixels>,
        font_size: Pixels,
        runs: &[TextRun],
    ) -> bool {
        self.wrap_width == wrap_width
            && self.font_size == font_size
            && self.text == *text
            && self.runs == runs
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

/// Split one base run into pieces so each underlined range gets its own
/// run. Ranges must not overlap; they are sorted here.
fn split_runs(
    base: &TextRun,
    len: usize,
    mut underlined: Vec<(Range<usize>, UnderlineStyle)>,
) -> Vec<TextRun> {
    if underlined.is_empty() {
        return vec![base.clone()];
    }
    underlined.sort_by_key(|(range, _)| range.start);
    let mut runs = Vec::with_capacity(underlined.len() * 2 + 1);
    let mut at = 0;
    for (range, underline) in underlined {
        let start = range.start.max(at).min(len);
        let end = range.end.min(len);
        if end <= start {
            continue;
        }
        if start > at {
            runs.push(TextRun {
                len: start - at,
                ..base.clone()
            });
        }
        runs.push(TextRun {
            len: end - start,
            underline: Some(underline),
            ..base.clone()
        });
        at = end;
    }
    if at < len {
        runs.push(TextRun {
            len: len - at,
            ..base.clone()
        });
    }
    runs
}

struct TextElement {
    input: Entity<TextInput>,
}

struct PrepaintState {
    layout: Option<TextLayout>,
    scroll_x: Pixels,
    scroll_y: Pixels,
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
        {
            // Text set before the dictionary loaded has no ranges yet;
            // one atomic compare per frame, a re-check only on change.
            let input = self.input.read(cx);
            if input.spell_check && input.spell_generation != spell::generation() {
                self.input.update(cx, |input, _| input.refresh_spelling());
            }
        }
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
        // Underlines: the IME marked range (plain) and misspelled words
        // (wavy red), except the word the cursor is still inside.
        let mut underlined: Vec<(Range<usize>, UnderlineStyle)> = Vec::new();
        if let Some(marked_range) = input.marked_range.as_ref() {
            underlined.push((
                marked_range.clone(),
                UnderlineStyle {
                    color: Some(run.color),
                    thickness: px(1.0),
                    wavy: false,
                },
            ));
        }
        if !content.is_empty() && !mask {
            let cursor_in = |range: &Range<usize>| {
                selected_range.is_empty() && range.start <= cursor && cursor <= range.end
            };
            underlined.extend(
                input
                    .misspelled
                    .iter()
                    .filter(|range| range.end <= content.len() && !cursor_in(range))
                    .map(|range| {
                        (
                            range.clone(),
                            UnderlineStyle {
                                color: Some(rgb(theme::spell_error()).into()),
                                thickness: px(1.0),
                                wavy: true,
                            },
                        )
                    }),
            );
        }
        let runs = split_runs(&run, display_text.len(), underlined);

        let font_size = style.font_size.to_pixels(window.rem_size());
        let line_height = window.line_height();
        let multiline = input.multiline;
        let last_scroll_x = input.scroll_x;
        let last_scroll_y = input.scroll_y;
        let keep_cursor_visible = input.keep_cursor_visible;
        let wrap_width = multiline.then_some(bounds.size.width);
        // Reuse last frame's shaping when its inputs are unchanged. Lines
        // are Arc-backed, so the clone is a refcount per line.
        let cached = input
            .shape_cache
            .as_ref()
            .filter(|cache| cache.matches(&display_text, wrap_width, font_size, &runs))
            .map(|cache| cache.lines.clone());
        let lines = match cached {
            Some(lines) => lines,
            None => {
                let lines = window
                    .text_system()
                    .shape_text(display_text.clone(), font_size, &runs, wrap_width, None)
                    .map(|lines| lines.into_vec())
                    .unwrap_or_default();
                self.input.update(cx, |input, _| {
                    input.shape_cache = Some(ShapeCache {
                        text: display_text.clone(),
                        wrap_width,
                        font_size,
                        runs,
                        lines: lines.clone(),
                    });
                });
                lines
            }
        };
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
        let scroll_x = if multiline {
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
            let mut sx = last_scroll_x;
            if cursor_x - sx > width - px(2.) {
                sx = cursor_x - width + px(2.);
            }
            if cursor_x - sx < px(0.) {
                sx = cursor_x;
            }
            sx.min((text_width - width + px(2.)).max(px(0.)))
                .max(px(0.))
        };
        // Multi-line inputs wrap instead. Scroll vertically: follow the
        // cursor after an edit or a cursor move, keep a wheel scroll put,
        // and clamp when the content shrinks.
        let scroll_y = if multiline {
            let viewport = bounds.size.height;
            let max_scroll = (layout.height() - viewport).max(px(0.));
            let mut sy = last_scroll_y.min(max_scroll).max(px(0.));
            if keep_cursor_visible {
                let cursor_y = layout
                    .position_for_index(display_cursor)
                    .map(|p| p.y)
                    .unwrap_or_default();
                if cursor_y + line_height - sy > viewport {
                    sy = cursor_y + line_height - viewport;
                }
                if cursor_y < sy {
                    sy = cursor_y;
                }
                sy = sy.min(max_scroll).max(px(0.));
            }
            sy
        } else {
            px(0.)
        };
        let text_bounds = Bounds::new(
            point(bounds.left() - scroll_x, bounds.top() - scroll_y),
            bounds.size,
        );
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
                    rgb(theme::text_cursor()),
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
            let color = theme::text_selection();
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
                if start_pos.y + line_height < end_pos.y {
                    quads.push(fill(
                        Bounds::from_corners(
                            point(
                                text_bounds.left(),
                                text_bounds.top() + start_pos.y + line_height,
                            ),
                            point(text_bounds.right(), text_bounds.top() + end_pos.y),
                        ),
                        color,
                    ));
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
        if !multiline && last_scroll_x != scroll_x {
            self.input.update(cx, |input, _| input.scroll_x = scroll_x);
        }
        if multiline && (last_scroll_y != scroll_y || keep_cursor_visible) {
            self.input.update(cx, |input, _| {
                input.scroll_y = scroll_y;
                input.keep_cursor_visible = false;
            });
        }
        PrepaintState {
            layout: Some(layout),
            scroll_x,
            scroll_y,
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
            point(
                bounds.left() - prepaint.scroll_x,
                bounds.top() - prepaint.scroll_y,
            ),
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
                let height = line.size(layout.line_height).height;
                // A scrolled input shapes every line but paints only the
                // rows inside the box.
                if origin.y + height >= bounds.top() && origin.y <= bounds.bottom() {
                    line.paint(
                        origin,
                        layout.line_height,
                        TextAlign::Left,
                        None,
                        window,
                        cx,
                    )
                    .ok();
                }
                origin.y += height;
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
            .on_action(cx.listener(Self::undo))
            .on_action(cx.listener(Self::redo))
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
            .on_mouse_down(gpui::MouseButton::Right, cx.listener(Self::on_right_click))
            .on_mouse_up(gpui::MouseButton::Left, cx.listener(Self::on_mouse_up))
            .on_mouse_up_out(gpui::MouseButton::Left, cx.listener(Self::on_mouse_up))
            .on_mouse_move(cx.listener(Self::on_mouse_move))
            .when(self.multiline, |el| {
                el.on_scroll_wheel(cx.listener(Self::on_scroll_wheel))
            })
            .w_full()
            .when(self.multiline && self.fill_height, |el| el.h_full())
            .child(TextElement { input: cx.entity() })
            .children(self.render_context_menu(cx))
    }
}

impl Focusable for TextInput {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus_handle.clone()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use gpui::TestAppContext;

    #[test]
    fn snapping_clamps_and_respects_char_boundaries() {
        assert_eq!(snap_to_char_boundary("abc", 10), 3);
        assert_eq!(snap_to_char_boundary("abc", 1), 1);
        // "é" is two bytes; offset 1 is inside it.
        assert_eq!(snap_to_char_boundary("é", 1), 0);
        assert_eq!(snap_to_char_boundary("aé", 2), 1);
        assert_eq!(snap_to_char_boundary("", 5), 0);
    }

    #[test]
    fn shape_cache_hits_only_on_identical_inputs() {
        let run = |len: usize, underline: Option<UnderlineStyle>| TextRun {
            len,
            font: gpui::font("Sans"),
            color: gpui::black(),
            background_color: None,
            underline,
            strikethrough: None,
        };
        let text = SharedString::from("hello");
        let cache = ShapeCache {
            text: text.clone(),
            wrap_width: Some(px(200.)),
            font_size: px(14.),
            runs: vec![run(5, None)],
            lines: Vec::new(),
        };
        assert!(cache.matches(&text, Some(px(200.)), px(14.), &[run(5, None)]));
        assert!(!cache.matches(&"hellp".into(), Some(px(200.)), px(14.), &[run(5, None)]));
        assert!(!cache.matches(&text, Some(px(100.)), px(14.), &[run(5, None)]));
        assert!(!cache.matches(&text, None, px(14.), &[run(5, None)]));
        assert!(!cache.matches(&text, Some(px(200.)), px(16.), &[run(5, None)]));
        let wavy = UnderlineStyle {
            color: None,
            thickness: px(1.),
            wavy: true,
        };
        assert!(!cache.matches(
            &text,
            Some(px(200.)),
            px(14.),
            &[run(2, None), run(3, Some(wavy))]
        ));
    }

    #[gpui::test]
    fn test_typing_undoes_as_one_step(cx: &mut TestAppContext) {
        let input = cx.new(|cx| TextInput::new("", cx));
        input.update(cx, |input, cx| {
            for (offset, letter) in ["h", "i"].iter().enumerate() {
                input.replace_range(offset..offset, letter, cx);
            }
            assert_eq!(input.text(), "hi");
            input.undo_last(cx);
            assert_eq!(input.text(), "");
            input.redo_last(cx);
            assert_eq!(input.text(), "hi");
            assert_eq!(input.selected_range, 2..2);
        });
    }

    #[gpui::test]
    fn test_a_new_run_starts_its_own_undo_step(cx: &mut TestAppContext) {
        let input = cx.new(|cx| TextInput::new("", cx));
        input.update(cx, |input, cx| {
            input.replace_range(0..0, "a", cx);
            // Typing away from the last cursor is a second step.
            input.replace_range(0..0, "b", cx);
            assert_eq!(input.text(), "ba");
            input.undo_last(cx);
            assert_eq!(input.text(), "a");
            input.undo_last(cx);
            assert_eq!(input.text(), "");
        });
    }

    #[gpui::test]
    fn test_delete_and_paste_are_their_own_steps(cx: &mut TestAppContext) {
        let input = cx.new(|cx| TextInput::new("", cx));
        input.update(cx, |input, cx| {
            input.set_text("word", cx);
            // A pasted run of characters never folds into typing.
            input.replace_range(4..4, " and more", cx);
            input.replace_range(3..13, "", cx);
            assert_eq!(input.text(), "wor");
            input.undo_last(cx);
            assert_eq!(input.text(), "word and more");
            input.undo_last(cx);
            assert_eq!(input.text(), "word");
            input.undo_last(cx);
            assert_eq!(input.text(), "");
        });
    }

    #[gpui::test]
    fn test_an_edit_drops_the_redo_history(cx: &mut TestAppContext) {
        let input = cx.new(|cx| TextInput::new("", cx));
        input.update(cx, |input, cx| {
            input.set_text("one", cx);
            input.undo_last(cx);
            input.set_text("two", cx);
            input.redo_last(cx);
            assert_eq!(input.text(), "two");
        });
    }

    #[gpui::test]
    fn test_clear_forgets_the_history(cx: &mut TestAppContext) {
        let input = cx.new(|cx| TextInput::new("", cx));
        input.update(cx, |input, cx| {
            input.set_text("sent message", cx);
            input.clear(cx);
            input.undo_last(cx);
            assert_eq!(input.text(), "");
        });
    }

    #[gpui::test]
    fn test_multiline_text_taller_than_the_box_scrolls(cx: &mut TestAppContext) {
        struct Host {
            input: Entity<TextInput>,
        }
        impl Render for Host {
            fn render(
                &mut self,
                _window: &mut Window,
                _cx: &mut Context<Self>,
            ) -> impl IntoElement {
                // Fixed size: the harness does not apply the window bounds
                // to the root view.
                div()
                    .w(px(300.))
                    .h(px(400.))
                    .flex()
                    .flex_col()
                    .child(self.input.clone())
            }
        }

        let input = cx.new(|cx| TextInput::new("", cx).multiline(8));
        // Enough text to wrap far past eight rows in a 300 px box.
        input.update(cx, |input, cx| input.set_text(&"word ".repeat(400), cx));
        let (_host, cx) = cx.add_window_view(|_window, _cx| Host {
            input: input.clone(),
        });
        cx.simulate_resize(gpui::size(px(300.), px(400.)));

        // The cursor sits at the end of the text, so the first draw must
        // scroll the view down to keep it visible.
        let scrolled = cx.update(|_window, app| input.read(app).scroll_y);
        assert!(
            scrolled > px(0.),
            "the view must follow the cursor to the bottom, got {scrolled:?}"
        );

        // A wheel over the input moves the view up and stays put: prepaint
        // must not snap back to the cursor without a new edit.
        cx.simulate_event(gpui::ScrollWheelEvent {
            position: point(px(150.), px(10.)),
            delta: gpui::ScrollDelta::Pixels(gpui::point(px(0.), px(50.))),
            ..Default::default()
        });
        let wheeled = cx.update(|_window, app| input.read(app).scroll_y);
        assert!(
            wheeled < scrolled,
            "the wheel must scroll the text up, got {wheeled:?} from {scrolled:?}"
        );

        // Typing pulls the cursor back into view; in test mode the dirty
        // window redraws at the end of the update.
        cx.update(|window, app| {
            input.update(app, |input, cx| {
                input.replace_text_in_range(None, "!", window, cx)
            })
        });
        let followed = cx.update(|_window, app| input.read(app).scroll_y);
        assert!(
            followed > wheeled,
            "an edit must scroll the cursor back into view, got {followed:?}"
        );
    }

    #[gpui::test]
    fn test_cursor_moves_never_leave_the_content(cx: &mut TestAppContext) {
        let input = cx.new(|cx| TextInput::new("", cx));
        input.update(cx, |input, cx| {
            input.set_text("héllo", cx);
            input.move_to(100, cx);
            assert_eq!(input.selected_range, 6..6);
            input.move_to(0, cx);
            input.select_to(2, cx);
            assert_eq!(input.selected_range, 0..1);
            input.select_to(99, cx);
            assert_eq!(input.selected_range, 0..6);
        });
    }
}
