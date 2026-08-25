//! Markdown rendering for agent messages: pulldown-cmark events mapped onto
//! gpui elements. Covers the subset that model output actually produces:
//! paragraphs, headings, lists, code blocks, blockquotes, rules, and inline
//! bold/italic/code/strikethrough/link styling.
//!
//! Inline styles use `StyledText::with_highlights`, which resolves the
//! ambient text style at paint time. Block styles (size, weight, monospace)
//! are set on the wrapping divs so they compose with the parent element.
//!
use gpui::{Div, SharedString, StyledText, div, prelude::*, px};
use pulldown_cmark::{Event, HeadingLevel, Options, Parser, Tag, TagEnd};

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
}

impl Paragraph {
    fn push(&mut self, chunk: &str, style: InlineStyle) {
        let start = self.text.len();
        self.text.push_str(chunk);
        let end = self.text.len();
        if start != end {
            self.spans.push((start..end, style));
        }
    }

    fn is_empty(&self) -> bool {
        self.text.trim().is_empty()
    }

    fn take(&mut self) -> Paragraph {
        std::mem::take(self)
    }
}

fn styled_paragraph(
    paragraph: Paragraph,
    text_size: Option<gpui::Pixels>,
    weight: Option<gpui::FontWeight>,
) -> Div {
    // Only styled spans become highlights; unstyled ranges inherit the
    // ambient text style resolved at paint time.
    let mut highlights: Vec<(std::ops::Range<usize>, gpui::HighlightStyle)> = Vec::new();
    for (range, style) in paragraph.spans {
        if style.is_plain() {
            continue;
        }
        if let Some((last_range, last_style)) = highlights.last_mut() {
            if last_range.end == range.start && *last_style == style.highlight() {
                last_range.end = range.end;
                continue;
            }
        }
        highlights.push((range, style.highlight()));
    }
    let text = StyledText::new(SharedString::new(paragraph.text)).with_highlights(highlights);
    let mut div = div().w_full();
    if let Some(size) = text_size {
        div = div.text_size(size);
    }
    if let Some(weight) = weight {
        div = div.font_weight(weight);
    }
    div.child(text)
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

fn code_block(code: String) -> Div {
    div()
        .w_full()
        .my_1()
        .px_3()
        .py_2()
        .rounded_md()
        .bg(gpui::rgb(theme::BG_CODE_BLOCK))
        .border_1()
        .border_color(gpui::rgb(theme::BORDER_SUBTLE))
        .font_family("monospace")
        .text_size(px(13.))
        .text_color(gpui::rgb(theme::CODE_TEXT))
        .child(code.trim_end().to_string())
}

/// Render a markdown string into a vertical stack of elements. Call during
/// render with the ambient window.
pub fn render_markdown(source: &str) -> Div {
    let mut options = Options::empty();
    options.insert(Options::ENABLE_STRIKETHROUGH);
    let parser = Parser::new_ext(source, options);

    let mut container = div().flex().flex_col().gap_2().w_full();
    let mut paragraph = Paragraph::default();
    let mut inline_flags: Vec<InlineStyle> = Vec::new();
    let mut list_counters: Vec<Option<u64>> = Vec::new();
    let mut code_block_text: Option<String> = None;
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
            if !paragraph.is_empty() {
                let taken = paragraph.take();
                let element = styled_paragraph(taken, None, None);
                container = container.child(wrap_inline(element, in_quote, list_counters.len()));
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
                Tag::CodeBlock(_) => {
                    flush_paragraph!();
                    code_block_text = Some(String::new());
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
                                paragraph.push(&format!("{number}. "), InlineStyle::default());
                                *number += 1;
                            }
                            None => paragraph.push("• ", InlineStyle::default()),
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
                Tag::Link { .. } => inline_flags.push(InlineStyle {
                    link: true,
                    ..Default::default()
                }),
                _ => {}
            },
            Event::End(tag) => match tag {
                TagEnd::Paragraph => {
                    flush_paragraph!();
                }
                TagEnd::Heading(level) => {
                    let taken = paragraph.take();
                    if !taken.is_empty() {
                        let element = styled_paragraph(
                            taken,
                            Some(heading_size(level)),
                            Some(gpui::FontWeight::BOLD),
                        );
                        container =
                            container.child(wrap_inline(element, in_quote, list_counters.len()));
                    }
                }
                TagEnd::BlockQuote(_) => {
                    in_quote = false;
                }
                TagEnd::CodeBlock => {
                    if let Some(code) = code_block_text.take() {
                        container = container.child(wrap_inline(
                            code_block(code),
                            in_quote,
                            list_counters.len(),
                        ));
                    }
                }
                TagEnd::List(_) => {
                    list_counters.pop();
                }
                TagEnd::Item => {
                    flush_paragraph!();
                }
                TagEnd::Emphasis | TagEnd::Strong | TagEnd::Strikethrough | TagEnd::Link => {
                    inline_flags.pop();
                }
                _ => {}
            },
            Event::Text(chunk) => {
                if let Some(code) = code_block_text.as_mut() {
                    code.push_str(&chunk);
                } else {
                    paragraph.push(&chunk, current_style(&inline_flags));
                }
            }
            Event::Code(chunk) => {
                let mut style = current_style(&inline_flags);
                style.code = true;
                paragraph.push(&chunk, style);
            }
            Event::SoftBreak | Event::HardBreak => {
                if code_block_text.is_none() {
                    paragraph.push("\n", InlineStyle::default());
                }
            }
            Event::Rule => {
                flush_paragraph!();
                container = container.child(
                    div()
                        .h(px(1.))
                        .w_full()
                        .my_1()
                        .bg(gpui::rgb(theme::BORDER_SUBTLE)),
                );
            }
            Event::InlineHtml(html) => {
                paragraph.push(&html, current_style(&inline_flags));
            }
            Event::Html(html) => {
                if let Some(code) = code_block_text.as_mut() {
                    code.push_str(&html);
                }
            }
            _ => {}
        }
    }

    // Flush any trailing content outside a closing tag (defensive).
    if !paragraph.is_empty() && code_block_text.is_none() {
        let taken = paragraph.take();
        container = container.child(styled_paragraph(taken, None, None));
    }
    container
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
