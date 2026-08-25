//! Markdown rendering for agent messages: pulldown-cmark events mapped onto
//! paragraphs, headings, lists, code blocks, blockquotes, rules, and inline
//! bold/italic/code/strikethrough/link styling via StyledText highlights.
//!
//! Must be called during render, when a `Window` is available to resolve the
//! ambient text style.

use gpui::{Div, SharedString, StyledText, Window, div, prelude::*, px};
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

    fn highlight(self, base: gpui::HighlightStyle) -> gpui::HighlightStyle {
        let mut style = base;
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

fn base_highlight(window: &Window) -> gpui::HighlightStyle {
    let style = window.text_style();
    gpui::HighlightStyle {
        color: Some(style.color),
        font_weight: Some(style.font_weight),
        font_style: Some(style.font_style),
        background_color: None,
        underline: None,
        strikethrough: None,
        fade_out: None,
    }
}

fn styled_paragraph(
    paragraph: Paragraph,
    window: &Window,
    text_size: Option<gpui::Pixels>,
    weight: Option<gpui::FontWeight>,
    color: Option<gpui::Hsla>,
) -> Div {
    let base = base_highlight(window);
    let mut base = base;
    if let Some(weight) = weight {
        base.font_weight = Some(weight);
    }
    if let Some(color) = color {
        base.color = Some(color);
    }
    let mut highlights: Vec<(std::ops::Range<usize>, gpui::HighlightStyle)> = Vec::new();
    for (range, style) in paragraph.spans {
        let rendered = if style.is_plain() {
            base
        } else {
            style.highlight(base)
        };
        if let Some((last_range, last_style)) = highlights.last_mut() {
            if last_range.end == range.start && *last_style == rendered {
                last_range.end = range.end;
                continue;
            }
        }
        highlights.push((range, rendered));
    }
    let text = StyledText::new(SharedString::new(paragraph.text))
        .with_default_highlights(&window.text_style(), highlights);
    let mut div = div().w_full();
    if let Some(size) = text_size {
        div = div.text_size(size);
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

/// Render a markdown string into a vertical stack of elements. Call during
/// render with the ambient window.
pub fn render_markdown(source: &str, window: &Window) -> Div {
    let mut options = Options::empty();
    options.insert(Options::ENABLE_STRIKETHROUGH);
    let parser = Parser::new_ext(source, options);

    let mut container = div().flex().flex_col().gap_2().w_full();
    let mut paragraph = Paragraph::default();
    let mut inline_flags: Vec<InlineStyle> = Vec::new();
    let mut list_counters: Vec<Option<u64>> = Vec::new();
    let mut code_block: Option<String> = None;
    let mut heading: Option<HeadingLevel> = None;
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

    for event in parser {
        match event {
            Event::Start(tag) => match tag {
                Tag::Paragraph => {}
                Tag::Heading { level, .. } => {
                    heading = Some(level);
                }
                Tag::BlockQuote(_) => {
                    in_quote = true;
                }
                Tag::CodeBlock(_) => {
                    code_block = Some(String::new());
                }
                Tag::List(start) => {
                    list_counters.push(start);
                }
                Tag::Item => {
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
                    let taken = paragraph.take();
                    if !taken.is_empty() {
                        let element = styled_paragraph(taken, window, None, None, None);
                        container = container.child(if in_quote {
                            quote_wrap(element)
                        } else {
                            element
                        });
                    }
                }
                TagEnd::Heading(level) => {
                    let taken = paragraph.take();
                    if !taken.is_empty() {
                        let element = styled_paragraph(
                            taken,
                            window,
                            Some(heading_size(level)),
                            Some(gpui::FontWeight::BOLD),
                            None,
                        );
                        container = container.child(if in_quote {
                            quote_wrap(element)
                        } else {
                            element
                        });
                    }
                    heading = None;
                }
                TagEnd::BlockQuote(_) => {
                    in_quote = false;
                }
                TagEnd::CodeBlock => {
                    if let Some(code) = code_block.take() {
                        container = container.child(
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
                                .child(code.trim_end().to_string()),
                        );
                    }
                }
                TagEnd::List(_) => {
                    list_counters.pop();
                }
                TagEnd::Item => {
                    let taken = paragraph.take();
                    if !taken.is_empty() {
                        let element = styled_paragraph(taken, window, None, None, None);
                        container = container.child(if in_quote {
                            quote_wrap(element)
                        } else {
                            element
                        });
                    }
                }
                TagEnd::Emphasis | TagEnd::Strong | TagEnd::Strikethrough | TagEnd::Link => {}
                _ => {}
            },
            Event::Text(chunk) => {
                if let Some(code) = code_block.as_mut() {
                    code.push_str(&chunk);
                } else {
                    paragraph.push(&chunk, current_style(&inline_flags));
                }
            }
            Event::Code(chunk) => {
                let style = {
                    let mut style = current_style(&inline_flags);
                    style.code = true;
                    style
                };
                paragraph.push(&chunk, style);
            }
            Event::SoftBreak | Event::HardBreak => {
                if code_block.is_none() {
                    paragraph.push("\n", InlineStyle::default());
                }
            }
            Event::Rule => {
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
                if let Some(code) = code_block.as_mut() {
                    code.push_str(&html);
                }
            }
            _ => {}
        }
    }

    // Flush any trailing content outside a closing tag (defensive).
    if !paragraph.is_empty() && code_block.is_none() {
        let taken = paragraph.take();
        container = container.child(styled_paragraph(taken, window, None, None, None));
    }
    container
}

fn quote_wrap(element: Div) -> Div {
    div()
        .w_full()
        .border_l_2()
        .border_color(gpui::rgb(theme::BORDER))
        .pl_3()
        .child(element)
}
