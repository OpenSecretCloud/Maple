//! Markdown parsing for agent messages: pulldown-cmark events resolved into
//! block-level structures that callers cache per message. Rendering is in
//! [`super::rich_text`], which turns blocks into interactive elements
//! (clickable links, drag selection, copyable code blocks).
//!
//! Parsing and element building are separate steps: [`parse`] produces a
//! [`Document`] of resolved blocks that callers cache per message, and
//! [`render`] turns it into elements each frame. Parsing is the expensive
//! part; rendering from blocks is a handful of allocations.

use std::rc::Rc;

use gpui::{Div, ElementId, SharedString, div, prelude::*, px};
use pulldown_cmark::{Event, HeadingLevel, Options, Parser, Tag, TagEnd};

use super::rich_text::{self, Highlights, Links, RenderCtx};
use super::theme;

#[derive(Clone, Copy, PartialEq, Eq, Default)]
struct InlineStyle {
    bold: bool,
    italic: bool,
    strikethrough: bool,
    code: bool,
    link: bool,
}

impl InlineStyle {
    fn is_plain(self) -> bool {
        self == Self::default()
    }

    fn highlight(self) -> gpui::HighlightStyle {
        let mut style = gpui::HighlightStyle {
            color: None,
            font_weight: None,
            font_style: None,
            background_color: None,
            underline: None,
            strikethrough: None,
            fade_out: None,
        };
        if self.bold {
            style.font_weight = Some(gpui::FontWeight::BOLD);
        }
        if self.italic {
            style.font_style = Some(gpui::FontStyle::Italic);
        }
        if self.strikethrough {
            style.strikethrough = Some(gpui::StrikethroughStyle {
                thickness: px(1.0),
                color: None,
            });
        }
        if self.code {
            style.background_color = Some(gpui::hsla(0., 0., 1., 0.08));
            style.color = Some(gpui::rgb(theme::CODE_TEXT).into());
        }
        if self.link {
            style.color = Some(gpui::rgb(theme::LINK).into());
            style.underline = Some(gpui::UnderlineStyle {
                thickness: px(1.0),
                color: Some(gpui::rgb(theme::LINK).into()),
                wavy: false,
            });
        }
        style
    }
}

#[derive(Default)]
struct Paragraph {
    text: String,
    spans: Vec<(std::ops::Range<usize>, InlineStyle)>,
    /// Link ranges with their destinations, byte ranges into `text`.
    links: Vec<(std::ops::Range<usize>, String)>,
    /// Length of the list marker ("• " or "1. ") at the start of `text`.
    /// A paragraph that holds only its marker counts as empty.
    marker_len: usize,
}

impl Paragraph {
    fn push_marker(&mut self, marker: &str) {
        self.push(marker, InlineStyle::default());
        self.marker_len = self.text.len();
    }

    fn push(&mut self, chunk: &str, style: InlineStyle) {
        let start = self.text.len();
        self.text.push_str(chunk);
        let end = self.text.len();
        if start != end {
            self.spans.push((start..end, style));
        }
    }

    /// Record that `start..end` belongs to the link at `url`, merging with
    /// the previous range when it continues the same link.
    fn extend_link(&mut self, url: &str, start: usize, end: usize) {
        if let Some((range, existing)) = self.links.last_mut()
            && existing == url
            && range.end == start
        {
            range.end = end;
            return;
        }
        self.links.push((start..end, url.to_string()));
    }
    fn is_empty(&self) -> bool {
        self.text[self.marker_len..].trim().is_empty()
    }

    fn take(&mut self) -> Paragraph {
        std::mem::take(self)
    }
}

/// One parsed block with its inline highlights already resolved.
#[derive(Clone)]
pub enum Block {
    Text {
        text: SharedString,
        highlights: Highlights,
        /// Clickable link ranges with destinations, byte ranges into `text`.
        links: Links,
        text_size: Option<gpui::Pixels>,
        weight: Option<gpui::FontWeight>,
        in_quote: bool,
        list_depth: usize,
    },
    Code {
        code: SharedString,
        /// Upper-case language tag from the fence info string, or "CODE",
        /// shown in the block header.
        label: SharedString,
        in_quote: bool,
        list_depth: usize,
    },
    Rule,
}

/// A parsed markdown message, ready to render any number of times.
#[derive(Clone, Default)]
pub struct Document {
    pub blocks: Vec<Block>,
}

fn text_block(
    paragraph: Paragraph,
    text_size: Option<gpui::Pixels>,
    weight: Option<gpui::FontWeight>,
    in_quote: bool,
    list_depth: usize,
) -> Block {
    // Only styled spans become highlights; unstyled ranges inherit the
    // ambient text style resolved at paint time.
    let mut highlights: Vec<(std::ops::Range<usize>, gpui::HighlightStyle)> = Vec::new();
    for (range, style) in paragraph.spans {
        if style.is_plain() {
            continue;
        }
        if let Some((last_range, last_style)) = highlights.last_mut()
            && last_range.end == range.start
            && *last_style == style.highlight()
        {
            last_range.end = range.end;
            continue;
        }
        highlights.push((range, style.highlight()));
    }
    let links: Links = paragraph
        .links
        .into_iter()
        .filter(|(range, _)| !range.is_empty())
        .collect();
    Block::Text {
        text: SharedString::new(paragraph.text),
        highlights: Rc::from(highlights),
        links,
        text_size,
        weight,
        in_quote,
        list_depth,
    }
}

fn heading_size(level: HeadingLevel) -> gpui::Pixels {
    match level {
        HeadingLevel::H1 => px(21.),
        HeadingLevel::H2 => px(18.),
        HeadingLevel::H3 => px(16.),
        HeadingLevel::H4 => px(15.),
        HeadingLevel::H5 | HeadingLevel::H6 => px(14.),
    }
}

/// Build elements from a parsed document (no selection context).
pub fn render(document: &Document) -> Div {
    render_with(document, &RenderCtx::default())
}

/// Build elements from a parsed document, making text blocks selectable
/// when the context carries a selection entity.
pub fn render_with(document: &Document, ctx: &RenderCtx) -> Div {
    let mut container = div().flex().flex_col().gap_2().w_full().pr_6();
    // One shared name per message; each code block adds its index.
    let id_name = ctx.id_name();
    for (index, block) in document.blocks.iter().enumerate() {
        container = match block {
            Block::Text {
                text,
                highlights,
                links,
                text_size,
                weight,
                in_quote,
                list_depth,
            } => container.child(wrap_inline(
                rich_text::paragraph(
                    text.clone(),
                    highlights.clone(),
                    links.clone(),
                    *text_size,
                    *weight,
                    ctx.for_block(index),
                    ctx,
                ),
                *in_quote,
                *list_depth,
            )),
            Block::Code {
                code,
                label,
                in_quote,
                list_depth,
            } => container.child(wrap_inline(
                rich_text::code_block(
                    code.clone(),
                    label.clone(),
                    ElementId::NamedInteger(id_name.clone(), index as u64),
                ),
                *in_quote,
                *list_depth,
            )),
            Block::Rule => container.child(
                div()
                    .h(px(1.))
                    .w_full()
                    .my_1()
                    .bg(gpui::rgb(theme::BORDER_SUBTLE)),
            ),
        };
    }
    container
}

/// Parse markdown into resolved blocks.
pub fn parse(source: &str) -> Document {
    let mut options = Options::empty();
    // Open link destinations, parallel to the `link` entries pushed onto
    // `inline_flags`.
    options.insert(Options::ENABLE_STRIKETHROUGH);
    let parser = Parser::new_ext(source, options);

    let mut blocks: Vec<Block> = Vec::new();
    let mut paragraph = Paragraph::default();
    let mut inline_flags: Vec<InlineStyle> = Vec::new();
    let mut link_stack: Vec<String> = Vec::new();
    let mut list_counters: Vec<Option<u64>> = Vec::new();
    let mut code_block_text: Option<String> = None;
    let mut code_block_language: Option<String> = None;
    let mut in_quote = false;

    let current_style = |flags: &[InlineStyle]| -> InlineStyle {
        let mut style = InlineStyle::default();
        for flag in flags {
            style.bold |= flag.bold;
            style.italic |= flag.italic;
            style.strikethrough |= flag.strikethrough;
            style.code |= flag.code;
            style.link |= flag.link;
        }
        style
    };

    // Emit the pending paragraph before a block boundary.
    macro_rules! flush_paragraph {
        () => {
            let taken = paragraph.take();
            if !taken.is_empty() {
                blocks.push(text_block(taken, None, None, in_quote, list_counters.len()));
            }
        };
    }

    for event in parser {
        match event {
            Event::Start(tag) => match tag {
                Tag::Paragraph => {}
                Tag::Heading { .. } => {
                    flush_paragraph!();
                }
                Tag::BlockQuote(_) => {
                    flush_paragraph!();
                    in_quote = true;
                }
                Tag::CodeBlock(kind) => {
                    flush_paragraph!();
                    code_block_text = Some(String::new());
                    code_block_language = match kind {
                        pulldown_cmark::CodeBlockKind::Fenced(info) => {
                            let tag = info.split_whitespace().next().unwrap_or("");
                            (!tag.is_empty()).then(|| tag.to_string())
                        }
                        pulldown_cmark::CodeBlockKind::Indented => None,
                    };
                }
                Tag::List(start) => {
                    flush_paragraph!();
                    list_counters.push(start);
                }
                Tag::Item => {
                    // A new list item starts a fresh paragraph even when the
                    // previous one has not been closed (tight lists), so
                    // nested items never merge into their parent's line.
                    flush_paragraph!();
                    if let Some(counter) = list_counters.last_mut() {
                        match counter {
                            Some(number) => {
                                paragraph.push_marker(&format!("{number}. "));
                                *number += 1;
                            }
                            None => paragraph.push_marker("• "),
                        }
                    }
                }
                Tag::Emphasis => inline_flags.push(InlineStyle {
                    italic: true,
                    ..Default::default()
                }),
                Tag::Strong => inline_flags.push(InlineStyle {
                    bold: true,
                    ..Default::default()
                }),
                Tag::Strikethrough => inline_flags.push(InlineStyle {
                    strikethrough: true,
                    ..Default::default()
                }),
                Tag::Link { dest_url, .. } => {
                    inline_flags.push(InlineStyle {
                        link: true,
                        ..Default::default()
                    });
                    link_stack.push(dest_url.to_string());
                }
                _ => {}
            },
            Event::End(tag) => match tag {
                TagEnd::Paragraph => {
                    flush_paragraph!();
                }
                TagEnd::Heading(level) => {
                    let taken = paragraph.take();
                    if !taken.is_empty() {
                        blocks.push(text_block(
                            taken,
                            Some(heading_size(level)),
                            Some(gpui::FontWeight::BOLD),
                            in_quote,
                            list_counters.len(),
                        ));
                    }
                }
                TagEnd::BlockQuote(_) => {
                    in_quote = false;
                }
                TagEnd::CodeBlock => {
                    if let Some(code) = code_block_text.take() {
                        let language = code_block_language.take();
                        let label = language.as_deref().unwrap_or("code").to_uppercase();
                        blocks.push(Block::Code {
                            code: SharedString::new(code.trim_end().to_string()),
                            label: SharedString::new(label),
                            in_quote,
                            list_depth: list_counters.len(),
                        });
                    }
                }
                TagEnd::List(_) => {
                    list_counters.pop();
                }
                TagEnd::Item | TagEnd::HtmlBlock => {
                    flush_paragraph!();
                }
                TagEnd::Emphasis | TagEnd::Strong | TagEnd::Strikethrough => {
                    inline_flags.pop();
                }
                TagEnd::Link => {
                    inline_flags.pop();
                    link_stack.pop();
                }
                _ => {}
            },
            Event::Text(chunk) => {
                if let Some(code) = code_block_text.as_mut() {
                    code.push_str(&chunk);
                } else {
                    let start = paragraph.text.len();
                    paragraph.push(&chunk, current_style(&inline_flags));
                    if let Some(url) = link_stack.last().cloned() {
                        let end = paragraph.text.len();
                        paragraph.extend_link(&url, start, end);
                    }
                }
            }
            Event::Code(chunk) => {
                let start = paragraph.text.len();
                let mut style = current_style(&inline_flags);
                style.code = true;
                paragraph.push(&chunk, style);
                if let Some(url) = link_stack.last().cloned() {
                    let end = paragraph.text.len();
                    paragraph.extend_link(&url, start, end);
                }
            }
            Event::SoftBreak | Event::HardBreak => {
                if code_block_text.is_none() {
                    paragraph.push("\n", InlineStyle::default());
                }
            }
            Event::Rule => {
                flush_paragraph!();
                blocks.push(Block::Rule);
            }
            Event::InlineHtml(html) => {
                let start = paragraph.text.len();
                paragraph.push(&html, current_style(&inline_flags));
                if let Some(url) = link_stack.last().cloned() {
                    let end = paragraph.text.len();
                    paragraph.extend_link(&url, start, end);
                }
            }
            Event::Html(html) => {
                // Block HTML is shown as its source, like inline HTML.
                if let Some(code) = code_block_text.as_mut() {
                    code.push_str(&html);
                } else {
                    paragraph.push(&html, current_style(&inline_flags));
                }
            }
            _ => {}
        }
    }

    // Flush any trailing content outside a closing tag (defensive).
    if !paragraph.is_empty() && code_block_text.is_none() {
        let taken = paragraph.take();
        blocks.push(text_block(taken, None, None, false, 0));
    }
    Document { blocks }
}

/// Apply list indentation and blockquote chrome to an inner block.
fn wrap_inline(element: Div, in_quote: bool, list_depth: usize) -> Div {
    let mut outer = div().w_full();
    if list_depth > 1 {
        outer = outer.pl(px(16. * (list_depth - 1) as f32));
    }
    if in_quote {
        outer = outer
            .border_l_2()
            .border_color(gpui::rgb(theme::BORDER))
            .pl_3();
    }
    outer.child(element)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn link_ranges_carry_destinations() {
        let document = parse("see [the docs](https://example.com/a) now");
        let Block::Text { text, links, .. } = &document.blocks[0] else {
            panic!("expected a text block");
        };
        assert_eq!(text.as_ref(), "see the docs now");
        assert_eq!(links.len(), 1);
        assert_eq!(links[0].1, "https://example.com/a");
        assert_eq!(&text[links[0].0.clone()], "the docs");
    }

    #[test]
    fn adjacent_links_stay_separate() {
        let document = parse("[a](https://x.test) and [b](https://y.test)");
        let Block::Text { links, .. } = &document.blocks[0] else {
            panic!("expected a text block");
        };
        assert_eq!(links.len(), 2);
        assert_eq!(links[0].1, "https://x.test");
        assert_eq!(links[1].1, "https://y.test");
    }

    #[test]
    fn fenced_code_keeps_language() {
        let document = parse("```rust\nfn main() {}\n```");
        let Block::Code { code, label, .. } = &document.blocks[0] else {
            panic!("expected a code block");
        };
        assert_eq!(code.as_ref(), "fn main() {}");
        assert_eq!(label.as_ref(), "RUST");
    }

    #[test]
    fn code_block_label_is_upper_case() {
        let document = parse("```sh\nls\n```\n\n    plain\n");
        let labels: Vec<&str> = document
            .blocks
            .iter()
            .filter_map(|block| match block {
                Block::Code { label, .. } => Some(label.as_ref()),
                _ => None,
            })
            .collect();
        assert_eq!(labels, vec!["SH", "CODE"]);
    }

    #[test]
    fn list_item_with_leading_code_block_has_no_orphan_marker() {
        // Unindented lines end the item, so the fence is empty; there must
        // still be no marker-only paragraph.
        let document = parse("- ```sh\nls\n```");
        assert!(
            !document
                .blocks
                .iter()
                .any(|block| matches!(block, Block::Text { text, .. } if text.trim() == "•"))
        );

        let document = parse("- ```sh\n  ls\n  ```\n- second");
        let Block::Code {
            code, list_depth, ..
        } = &document.blocks[0]
        else {
            panic!("expected a code block first, got a marker paragraph");
        };
        assert_eq!(code.as_ref(), "ls");
        assert_eq!(*list_depth, 1);
        let Block::Text { text, .. } = &document.blocks[1] else {
            panic!("expected the second item");
        };
        assert_eq!(text.as_ref(), "• second");
        assert_eq!(document.blocks.len(), 2);
    }

    #[test]
    fn list_item_with_leading_heading_has_no_orphan_marker() {
        let document = parse("- # Title\n  body");
        assert!(
            !document
                .blocks
                .iter()
                .any(|block| matches!(block, Block::Text { text, .. } if text.trim() == "•"))
        );
    }

    #[test]
    fn block_html_is_kept_as_text() {
        let document = parse("<div>\nhi\n</div>\n\nafter");
        let Block::Text { text, .. } = &document.blocks[0] else {
            panic!("expected html text");
        };
        assert!(text.contains("<div>"), "got {text:?}");
    }

    #[test]
    fn indented_code_has_no_language() {
        let document = parse("text\n\n    indented code\n");
        assert!(
            document.blocks.iter().any(
                |block| matches!(block, Block::Code { label, .. } if label.as_ref() == "CODE")
            )
        );
    }
}
