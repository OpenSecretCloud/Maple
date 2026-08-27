//! Interactive transcript text: drag selection across paragraphs, clickable
//! links, and code blocks with a copy button.
//!
//! gpui 0.2.2 has no selection for static text, so this module provides it:
//! a shared [`TextSelection`] entity records an anchor/head position, and
//! every rendered paragraph is a [`RichText`] element that maps mouse
//! positions to byte indices through `TextLayout::index_for_position`.
//! Paragraphs are keyed by an `ordinal` (base per message + block index) so
//! a drag can span paragraphs; the text of visible paragraphs is registered
//! in the entity to rebuild the copied string.

use std::collections::HashMap;
use std::ops::Range;

use gpui::{
    AnyElement, App, ClipboardItem, Div, Element, Entity, FocusHandle, GlobalElementId, Hitbox,
    HitboxBehavior, InspectorElementId, LayoutId, MouseButton, Pixels, SharedString, StyledText,
    Window, div, prelude::*,
};

use super::icons::icon;
use super::theme;

/// Background drawn under selected text: Maple coral at ~40% alpha.
fn selection_background() -> gpui::Hsla {
    gpui::rgba(0xff977166).into()
}

/// A position inside the transcript's paragraph space.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct SelectionPos {
    /// Paragraph key: message base ordinal plus block index.
    pub ordinal: u64,
    /// Byte index into that paragraph's text.
    pub index: usize,
}

/// Shared drag-selection state, owned by the chat screen.
#[derive(Default)]
pub struct TextSelection {
    anchor: Option<SelectionPos>,
    head: Option<SelectionPos>,
    down: Option<SelectionPos>,
    selecting: bool,
    /// Text of the paragraphs rendered recently, keyed by ordinal, so the
    /// copied string can be rebuilt without touching chat state.
    registry: HashMap<u64, String>,
}

impl TextSelection {
    pub fn is_selecting(&self) -> bool {
        self.selecting
    }

    pub fn has_selection(&self) -> bool {
        self.anchor.is_some() && self.anchor != self.head
    }

    pub fn clear(&mut self) {
        self.anchor = None;
        self.head = None;
        self.down = None;
        self.selecting = false;
    }

    fn begin(&mut self, pos: SelectionPos) {
        self.anchor = Some(pos);
        self.head = Some(pos);
        self.down = Some(pos);
        self.selecting = true;
    }

    fn extend(&mut self, pos: SelectionPos) {
        if self.selecting {
            self.head = Some(pos);
        }
    }

    /// End a drag. Returns the URL when the press was a click (no drag)
    /// inside a link range.
    fn end_with_click(
        &mut self,
        ordinal: u64,
        index: usize,
        links: &[(Range<usize>, String)],
    ) -> Option<String> {
        let down = self.down.take();
        self.selecting = false;
        if self.anchor.is_some() && self.anchor == self.head {
            // A click without a drag leaves nothing selected.
            self.anchor = None;
            self.head = None;
        }
        let down = down?;
        if down.ordinal != ordinal || down.index != index {
            return None;
        }
        links
            .iter()
            .find(|(range, _)| range.contains(&index))
            .map(|(_, url)| url.clone())
    }

    /// Anchor and head in reading order, when a selection exists.
    fn bounds(&self) -> Option<(SelectionPos, SelectionPos)> {
        let anchor = self.anchor?;
        let head = self.head?;
        Some(
            if (head.ordinal, head.index) >= (anchor.ordinal, anchor.index) {
                (anchor, head)
            } else {
                (head, anchor)
            },
        )
    }

    /// The selected byte range within one paragraph, if any.
    fn range_for(&self, ordinal: u64) -> Option<Range<usize>> {
        let (lo, hi) = self.bounds()?;
        if ordinal < lo.ordinal || ordinal > hi.ordinal {
            return None;
        }
        let len = self.registry.get(&ordinal).map(String::len)?;
        let start = if ordinal == lo.ordinal { lo.index } else { 0 };
        let end = if ordinal == hi.ordinal { hi.index } else { len };
        let (start, end) = (start.min(len), end.min(len));
        (start < end).then(|| start..end)
    }

    /// The selected text across paragraphs, joined with newlines.
    pub fn selected_text(&self) -> String {
        let Some((lo, hi)) = self.bounds() else {
            return String::new();
        };
        let mut parts: Vec<&str> = Vec::new();
        for ordinal in lo.ordinal..=hi.ordinal {
            let Some(text) = self.registry.get(&ordinal) else {
                continue;
            };
            let start = if ordinal == lo.ordinal { lo.index } else { 0 };
            let end = if ordinal == hi.ordinal {
                hi.index
            } else {
                text.len()
            };
            let (start, end) = (start.min(text.len()), end.min(text.len()));
            if start < end {
                parts.push(&text[start..end]);
            }
        }
        parts.join("\n")
    }

    fn needs_register(&self, ordinal: u64, text: &str) -> bool {
        self.registry
            .get(&ordinal)
            .is_none_or(|registered| registered != text)
    }

    fn register(&mut self, ordinal: u64, text: &str) {
        if self.registry.len() > 8192 {
            // Keep only the neighborhood of the active selection.
            match self.bounds() {
                Some((lo, _)) => {
                    let floor = lo.ordinal.saturating_sub(4096);
                    self.registry.retain(|key, _| *key >= floor);
                }
                None => self.registry.clear(),
            }
        }
        self.registry.insert(ordinal, text.to_string());
    }
}

/// Context for turning parsed markdown blocks into interactive elements.
#[derive(Clone, Default)]
pub struct RenderCtx {
    /// Selection shared by every paragraph of the transcript.
    pub selection: Option<Entity<TextSelection>>,
    /// First ordinal of the message being rendered; block indexes are added.
    pub base_ordinal: Option<u64>,
    /// Focus handle moved to the transcript when a drag starts, so the
    /// copy keybinding applies while text is selected.
    pub focus: Option<FocusHandle>,
    /// Unique element-id seed for the message being rendered.
    pub id_seed: String,
}

impl RenderCtx {
    /// Ordinal of block `index` when text blocks are interactive.
    pub fn for_block(&self, index: usize) -> Option<u64> {
        self.base_ordinal.map(|base| base + index as u64)
    }
}

/// One paragraph of markdown as an interactive element: styled runs,
/// clickable links, and drag selection.
pub struct RichText {
    ordinal: Option<u64>,
    text: SharedString,
    highlights: Vec<(Range<usize>, HighlightStyleT)>,
    links: Vec<(Range<usize>, String)>,
    selection: Option<Entity<TextSelection>>,
    focus: Option<FocusHandle>,
    styled: Option<StyledText>,
}

type HighlightStyleT = gpui::HighlightStyle;

/// Overlay the selection background on the paragraph's styled runs.
fn merged_highlights(
    base: &[(Range<usize>, HighlightStyleT)],
    selection: Option<Range<usize>>,
    text_len: usize,
) -> Vec<(Range<usize>, HighlightStyleT)> {
    let Some(selection) = selection else {
        return base.to_vec();
    };
    let selection = selection.start.min(text_len)..selection.end.min(text_len);
    if selection.start >= selection.end {
        return base.to_vec();
    }
    let mut points: Vec<usize> = vec![selection.start, selection.end];
    for (range, _) in base {
        // Segment boundaries of the base styles, whether inside or outside
        // the selection, so unstyled gaps keep their original styling.
        points.push(range.start.min(text_len));
        points.push(range.end.min(text_len));
    }
    points.sort_unstable();
    points.dedup();
    let mut merged: Vec<(Range<usize>, HighlightStyleT)> = Vec::new();
    for pair in points.windows(2) {
        let (start, end) = (pair[0], pair[1]);
        if start >= end {
            continue;
        }
        let base_style = base
            .iter()
            .find(|(range, _)| range.contains(&start))
            .map(|(_, style)| *style)
            .unwrap_or_default();
        let selected = selection.start <= start && start < selection.end;
        let mut style = base_style;
        if selected {
            style.background_color = Some(selection_background());
        }
        if let Some((last_range, last_style)) = merged.last_mut() {
            if last_range.end == start && *last_style == style {
                last_range.end = end;
                continue;
            }
        }
        merged.push((start..end, style));
    }
    merged
}

impl RichText {
    fn styled_text(&mut self, cx: &App) -> StyledText {
        let selection_range = self
            .selection
            .as_ref()
            .zip(self.ordinal)
            .and_then(|(entity, ordinal)| entity.read(cx).range_for(ordinal));
        let highlights = merged_highlights(&self.highlights, selection_range, self.text.len());
        StyledText::new(self.text.clone()).with_highlights(highlights)
    }
}

impl Element for RichText {
    type RequestLayoutState = ();
    type PrepaintState = Hitbox;

    fn id(&self) -> Option<gpui::ElementId> {
        None
    }

    fn source_location(&self) -> Option<&'static core::panic::Location<'static>> {
        None
    }

    fn request_layout(
        &mut self,
        _id: Option<&GlobalElementId>,
        inspector_id: Option<&InspectorElementId>,
        window: &mut Window,
        cx: &mut App,
    ) -> (LayoutId, Self::RequestLayoutState) {
        let mut styled = self.styled_text(cx);
        let result = styled.request_layout(None, inspector_id, window, cx);
        self.styled = Some(styled);
        result
    }

    fn prepaint(
        &mut self,
        _id: Option<&GlobalElementId>,
        inspector_id: Option<&InspectorElementId>,
        bounds: gpui::Bounds<Pixels>,
        state: &mut Self::RequestLayoutState,
        window: &mut Window,
        cx: &mut App,
    ) -> Self::PrepaintState {
        if self.styled.is_none() {
            self.styled = Some(self.styled_text(cx));
        }
        let styled = self.styled.as_mut().expect("styled text built in layout");
        styled.prepaint(None, inspector_id, bounds, state, window, cx);
        window.insert_hitbox(bounds, HitboxBehavior::Normal)
    }

    fn paint(
        &mut self,
        _id: Option<&GlobalElementId>,
        _inspector_id: Option<&InspectorElementId>,
        _bounds: gpui::Bounds<Pixels>,
        _state: &mut Self::RequestLayoutState,
        hitbox: &mut Self::PrepaintState,
        window: &mut Window,
        cx: &mut App,
    ) {
        let Some(styled) = self.styled.as_mut() else {
            return;
        };
        let mut styled_prepaint = ();
        styled.paint(
            None,
            None,
            _bounds,
            _state,
            &mut styled_prepaint,
            window,
            cx,
        );

        let layout = styled.layout().clone();
        let hitbox = hitbox.clone();

        // Keep the paragraph text available for copy.
        if let Some(entity) = &self.selection
            && let Some(ordinal) = self.ordinal
        {
            let text = self.text.clone();
            let needs = entity.read(cx).needs_register(ordinal, &text);
            if needs {
                entity.update(cx, |state, _| state.register(ordinal, &text));
            }
        }

        // IBeam over selectable text; the pointing hand wins on links.
        if self.selection.is_some() {
            let over_link = !self.links.is_empty()
                && layout
                    .index_for_position(window.mouse_position())
                    .map(|ix| self.links.iter().any(|(range, _)| range.contains(&ix)))
                    .unwrap_or(false);
            let style = if over_link {
                gpui::CursorStyle::PointingHand
            } else {
                gpui::CursorStyle::IBeam
            };
            window.set_cursor_style(style, &hitbox);
        }

        let entity = self.selection.clone();
        let ordinal = self.ordinal;
        let focus = self.focus.clone();

        // Press: start a selection at the character under the pointer.
        {
            let entity = entity.clone();
            let ordinal = ordinal;
            let layout = layout.clone();
            let hitbox = hitbox.clone();
            window.on_mouse_event(
                move |event: &gpui::MouseDownEvent, phase, window: &mut Window, cx: &mut App| {
                    if phase != gpui::DispatchPhase::Bubble
                        || event.button != MouseButton::Left
                        || !hitbox.is_hovered(window)
                    {
                        return;
                    }
                    if let Some(focus) = &focus {
                        window.focus(focus);
                    }
                    let Some(entity) = &entity else {
                        return;
                    };
                    if let (Some(ordinal), Ok(index)) =
                        (ordinal, layout.index_for_position(event.position))
                    {
                        entity.update(cx, |state, cx| {
                            state.begin(SelectionPos { ordinal, index });
                            cx.notify();
                        });
                    }
                },
            );
        }

        // Drag: move the head while the pointer stays over this paragraph.
        {
            let entity = entity.clone();
            let layout = layout.clone();
            let hitbox = hitbox.clone();
            window.on_mouse_event(
                move |event: &gpui::MouseMoveEvent, phase, window: &mut Window, cx: &mut App| {
                    if phase != gpui::DispatchPhase::Bubble || !hitbox.is_hovered(window) {
                        return;
                    }
                    let Some(entity) = &entity else {
                        return;
                    };
                    if !entity.read(cx).is_selecting() {
                        return;
                    }
                    if let (Some(ordinal), Ok(index)) =
                        (ordinal, layout.index_for_position(event.position))
                    {
                        entity.update(cx, |state, cx| {
                            state.extend(SelectionPos { ordinal, index });
                            cx.notify();
                        });
                    }
                },
            );
        }

        // Release: finish the selection; a no-drag press on a link opens it.
        {
            let entity = entity.clone();
            let links = self.links.clone();
            window.on_mouse_event(
                move |event: &gpui::MouseUpEvent, phase, window: &mut Window, cx: &mut App| {
                    if phase != gpui::DispatchPhase::Bubble || event.button != MouseButton::Left {
                        return;
                    }
                    let Some(entity) = &entity else {
                        return;
                    };
                    if !hitbox.is_hovered(window) {
                        entity.update(cx, |state, cx| {
                            state.end_with_click(0, 0, &[]);
                            cx.notify();
                        });
                        return;
                    }
                    let index = layout.index_for_position(event.position).ok();
                    if let (Some(ordinal), Some(index)) = (ordinal, index) {
                        let url = entity.update(cx, |state, cx| {
                            let url = state.end_with_click(ordinal, index, &links);
                            cx.notify();
                            url
                        });
                        if let Some(url) = url {
                            open_link(&url);
                        }
                    } else {
                        entity.update(cx, |state, cx| {
                            state.end_with_click(0, 0, &[]);
                            cx.notify();
                        });
                    }
                },
            );
        }
    }
}

impl IntoElement for RichText {
    type Element = Self;

    fn into_element(self) -> Self {
        self
    }
}

/// Open a URL in the system browser off the UI thread.
pub fn open_link(url: &str) {
    let url = url.to_string();
    std::thread::spawn(move || {
        if let Err(error) = webbrowser::open(&url) {
            log::debug!("failed to open {url}: {error}");
        }
    });
}

/// A styled, selectable paragraph. `ordinal` enables selection when set.
pub fn paragraph(
    text: SharedString,
    highlights: Vec<(Range<usize>, HighlightStyleT)>,
    links: Vec<(Range<usize>, String)>,
    text_size: Option<Pixels>,
    weight: Option<gpui::FontWeight>,
    ordinal: Option<u64>,
    ctx: &RenderCtx,
) -> Div {
    let mut container = div().w_full();
    if let Some(size) = text_size {
        container = container.text_size(size);
    }
    if let Some(weight) = weight {
        container = container.font_weight(weight);
    }
    container.child(RichText {
        ordinal,
        text,
        highlights,
        links,
        selection: ctx.selection.clone(),
        focus: ctx.focus.clone(),
        styled: None,
    })
}

/// Plain (unstyled) selectable text, used for user message bubbles.
pub fn plain_paragraph(text: SharedString, ordinal: Option<u64>, ctx: &RenderCtx) -> Div {
    paragraph(text, Vec::new(), Vec::new(), None, None, ordinal, ctx)
}

/// Code block with a language label and a copy button.
pub fn code_block(
    code: SharedString,
    language: Option<String>,
    block_ix: usize,
    ctx: &RenderCtx,
) -> Div {
    let copy_code = code.clone();
    let seed = if ctx.id_seed.is_empty() {
        "md".to_string()
    } else {
        ctx.id_seed.clone()
    };
    div()
        .w_full()
        .my_1()
        .rounded_md()
        .bg(gpui::rgb(theme::BG_CODE_BLOCK))
        .border_1()
        .border_color(gpui::rgb(theme::BORDER_SUBTLE))
        .overflow_hidden()
        .child(
            div()
                .flex()
                .items_center()
                .justify_between()
                .px_3()
                .py_1p5()
                .border_b_1()
                .border_color(gpui::rgb(theme::BORDER_SUBTLE))
                .child(
                    div()
                        .text_xs()
                        .text_color(gpui::rgb(theme::TEXT_MUTED))
                        .child(
                            language
                                .clone()
                                .unwrap_or_else(|| "code".to_string())
                                .to_uppercase(),
                        ),
                )
                .child(
                    div()
                        .id(gpui::SharedString::from(format!("{seed}-copy-{block_ix}")))
                        .flex()
                        .items_center()
                        .gap_1()
                        .px_1p5()
                        .py_0p5()
                        .rounded_md()
                        .text_xs()
                        .text_color(gpui::rgb(theme::TEXT_SECONDARY))
                        .hover(|style| style.bg(gpui::hsla(0., 0., 1., 0.08)).cursor_pointer())
                        .on_click(move |_, _, cx: &mut App| {
                            cx.write_to_clipboard(ClipboardItem::new_string(copy_code.to_string()));
                        })
                        .child(icon("copy", gpui::px(12.), theme::TEXT_SECONDARY))
                        .child("Copy"),
                ),
        )
        .child(
            div()
                .px_3()
                .py_2()
                .w_full()
                .font_family("monospace")
                .text_size(gpui::px(13.))
                .text_color(gpui::rgb(theme::CODE_TEXT))
                .child(code),
        )
}

#[allow(dead_code)]
fn _unused(element: AnyElement) -> AnyElement {
    element
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pos(ordinal: u64, index: usize) -> SelectionPos {
        SelectionPos { ordinal, index }
    }

    #[test]
    fn click_without_drag_clears_and_opens_link() {
        let mut selection = TextSelection::default();
        selection.begin(pos(10, 4));
        let links = vec![(3..7, "https://example.test".to_string())];
        let url = selection.end_with_click(10, 4, &links);
        assert_eq!(url.as_deref(), Some("https://example.test"));
        assert!(!selection.has_selection());
    }

    #[test]
    fn drag_keeps_selection() {
        let mut selection = TextSelection::default();
        selection.begin(pos(10, 2));
        selection.extend(pos(12, 5));
        assert!(selection.has_selection());
        assert_eq!(selection.bounds(), Some((pos(10, 2), pos(12, 5))));
    }

    #[test]
    fn range_for_spans_partial_and_whole_paragraphs() {
        let mut selection = TextSelection::default();
        selection.registry.insert(10, "hello world".to_string());
        selection.registry.insert(11, "second".to_string());
        selection.registry.insert(12, "third".to_string());
        selection.begin(pos(10, 6));
        selection.extend(pos(12, 2));
        assert_eq!(selection.range_for(10), Some(6..11));
        assert_eq!(selection.range_for(11), Some(0..6));
        assert_eq!(selection.range_for(12), Some(0..2));
        assert_eq!(selection.range_for(13), None);
        assert_eq!(selection.selected_text(), "world\nsecond\nth");
    }

    #[test]
    fn merged_highlights_overlay_selection() {
        let base = vec![(0..3, HighlightStyleT::default())];
        let merged = merged_highlights(&base, Some(2..6), 10);
        assert_eq!(merged.len(), 2);
        assert_eq!(merged[0].0, 0..2);
        assert!(merged[0].1.background_color.is_none());
        assert_eq!(merged[1].0, 2..6);
        assert!(merged[1].1.background_color.is_some());
    }
}
