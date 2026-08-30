//! Per-item caches the transcript renders from. Parsing markdown and
//! deriving display strings happens once per item revision, never on a
//! frame, so the list callback only reads.

use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;

use gpui::SharedString;
use maple_agent::agent::AgentTimelineItem;

use super::{diff_lines_for, maple_display_text, tool_input_line, tool_output_markdown};
use crate::ui::markdown;

/// Which text of a timeline item a parsed document belongs to.
#[derive(Clone, Copy)]
pub(super) enum MarkdownKind {
    Body = 0,
    ToolOutput = 1,
}

/// Per-item parsed markdown. Interior mutability because the list render
/// callback only has shared access to the screen. Entries are keyed by
/// item id and kind and validated by the item's revision and text length,
/// so no frame hashes message content.
/// Cached document with the item revision and text length it was parsed
/// at, and when.
pub(super) type MarkdownEntry = (u64, usize, Rc<markdown::Document>, std::time::Instant);

/// Shortest gap between two parses of a streaming message. Chunks land
/// faster than this; the previous parse stays on screen in between and a
/// deferred repaint shows the last chunk.
pub(super) const STREAM_PARSE_INTERVAL: std::time::Duration = std::time::Duration::from_millis(50);

/// Gap between selection ordinal bases of two items.
pub(super) const ORDINAL_SPACING: u64 = 4096;

#[derive(Default)]
pub(super) struct MarkdownCache {
    entries: [RefCell<HashMap<String, MarkdownEntry>>; 2],
    /// Base ordinal per item key, so paragraphs get stable selection keys.
    ordinals: RefCell<HashMap<String, u64>>,
    /// The base the next unseen key gets; bases only grow, so this is
    /// the maximum without scanning the map on every miss.
    next_base: std::cell::Cell<u64>,
    /// A throttled parse was skipped since the last `take_stale`; the
    /// caller owes a repaint once the interval has passed.
    stale: std::cell::Cell<bool>,
}

impl MarkdownCache {
    /// Parsed document for `source`, parsed now if the cache is stale.
    /// With `throttle` (a message still streaming), a parse younger than
    /// `STREAM_PARSE_INTERVAL` is served as is and the stale flag is set.
    pub(super) fn get(
        &self,
        id: &str,
        kind: MarkdownKind,
        revision: u64,
        source: &str,
        throttle: bool,
    ) -> Rc<markdown::Document> {
        let mut entries = self.entries[kind as usize].borrow_mut();
        if let Some((cached_revision, cached_len, document, parsed_at)) = entries.get(id) {
            if *cached_revision == revision && *cached_len == source.len() {
                return Rc::clone(document);
            }
            if throttle && parsed_at.elapsed() < STREAM_PARSE_INTERVAL {
                self.stale.set(true);
                return Rc::clone(document);
            }
        }
        if entries.len() > 4096 {
            entries.clear();
        }
        let document = Rc::new(markdown::parse(source));
        entries.insert(
            id.to_string(),
            (
                revision,
                source.len(),
                Rc::clone(&document),
                std::time::Instant::now(),
            ),
        );
        document
    }

    /// Whether a throttled parse was skipped since the last call.
    pub(super) fn take_stale(&self) -> bool {
        self.stale.replace(false)
    }

    /// Selection ordinal base for an item key. Bases are spaced far apart
    /// so `base + block index` never collides across messages.
    pub(super) fn ordinal_for(&self, key: &str) -> u64 {
        let mut ordinals = self.ordinals.borrow_mut();
        if let Some(base) = ordinals.get(key) {
            return *base;
        }
        let base = self.next_base.get() + ORDINAL_SPACING;
        self.next_base.set(base);
        ordinals.insert(key.to_string(), base);
        base
    }

    pub(super) fn clear(&self) {
        for entries in &self.entries {
            entries.borrow_mut().clear();
        }
        self.ordinals.borrow_mut().clear();
        self.next_base.set(0);
    }
}

/// Strings a tool or text card shows, derived once per item revision
/// instead of on every frame.
#[derive(Default)]
pub(super) struct ItemDerived {
    /// Display text of thinking rows and user bubbles.
    pub(super) text: SharedString,
    /// Readable tool output for the expanded card.
    pub(super) output_text: Option<SharedString>,
    /// First non-empty output line for the collapsed card.
    pub(super) preview: Option<SharedString>,
    /// `input: {json}` for the expanded card.
    pub(super) input_line: Option<SharedString>,
    /// +/- lines of an edit or write tool, capped at `MAX_DIFF_LINES`.
    pub(super) diff_lines: Rc<Vec<(char, SharedString)>>,
}

pub(super) const MAX_DIFF_LINES: usize = 200;

impl ItemDerived {
    fn build(item: &AgentTimelineItem) -> Self {
        let output_text = tool_output_markdown(item).map(SharedString::from);
        let preview = output_text.as_ref().and_then(|text| {
            text.lines()
                .map(str::trim)
                .find(|line| !line.is_empty())
                .map(|line| SharedString::from(line.to_string()))
        });
        Self {
            text: SharedString::from(
                maple_display_text(item.text.as_deref().unwrap_or("")).into_owned(),
            ),
            output_text,
            preview,
            input_line: tool_input_line(item).map(SharedString::from),
            diff_lines: Rc::new(diff_lines_for(item)),
        }
    }
}

/// Lazily built `ItemDerived` per item id, validated by item revision.
#[derive(Default)]
pub(super) struct DerivedCache {
    entries: RefCell<HashMap<String, (u64, Rc<ItemDerived>)>>,
}

impl DerivedCache {
    pub(super) fn get(&self, item: &AgentTimelineItem, revision: u64) -> Rc<ItemDerived> {
        let mut entries = self.entries.borrow_mut();
        if let Some((cached, derived)) = entries.get(&item.id)
            && *cached == revision
        {
            return Rc::clone(derived);
        }
        if entries.len() > 4096 {
            entries.clear();
        }
        let derived = Rc::new(ItemDerived::build(item));
        entries.insert(item.id.clone(), (revision, Rc::clone(&derived)));
        derived
    }

    pub(super) fn clear(&self) {
        self.entries.borrow_mut().clear();
    }
}
