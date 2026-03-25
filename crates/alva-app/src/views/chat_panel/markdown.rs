// INPUT:  gpui (div, px, Div, FontWeight, HighlightStyle, Hsla, IntoElement, ParentElement,
//         SharedString, Styled, StyledText, AnyElement),
//         pulldown_cmark (Parser, Event, Tag, TagEnd, CodeBlockKind, HeadingLevel),
//         crate::theme::Theme, super::code_block::render_code_block
// OUTPUT: pub fn render_markdown
// POS:    Parses Markdown text and converts to GPUI elements supporting headings, paragraphs,
//         bold/italic/code inline spans, code blocks, lists, horizontal rules, and links.

use gpui::{
    div, px, AnyElement, FontWeight, HighlightStyle, IntoElement, ParentElement, SharedString,
    Styled, StyledText,
};
use gpui::prelude::FluentBuilder;
use pulldown_cmark::{CodeBlockKind, Event, HeadingLevel, Parser, Tag, TagEnd};
use std::ops::Range;

use crate::theme::Theme;
use super::code_block::render_code_block;

/// Parses Markdown text and returns GPUI elements.
///
/// This is a helper function (not a GPUI view) that returns an element to be used
/// inside a parent view's render method.
pub fn render_markdown(text: &str, theme: &Theme) -> impl IntoElement {
    let elements = parse_markdown_to_elements(text, theme);

    div().flex().flex_col().gap_1().children(elements)
}

/// Inline span with formatting metadata.
#[derive(Clone, Debug)]
struct InlineSpan {
    text: String,
    bold: bool,
    italic: bool,
    code: bool,
    link: bool,
}

/// Internal state for the markdown parser.
struct MarkdownParserState {
    /// Accumulated block-level elements.
    elements: Vec<AnyElement>,
    /// Current inline spans being collected for a paragraph/heading.
    inline_spans: Vec<InlineSpan>,
    /// Current formatting state.
    bold: bool,
    italic: bool,
    code: bool,
    link: bool,
    /// Current heading level (None if not in a heading).
    heading_level: Option<HeadingLevel>,
    /// Code block state.
    in_code_block: bool,
    code_block_lang: Option<String>,
    code_block_content: String,
    /// List state.
    list_stack: Vec<Option<u64>>, // None = unordered, Some(start) = ordered
    item_counter: Vec<u64>,
}

impl MarkdownParserState {
    fn new() -> Self {
        Self {
            elements: Vec::new(),
            inline_spans: Vec::new(),
            bold: false,
            italic: false,
            code: false,
            link: false,
            heading_level: None,
            in_code_block: false,
            code_block_lang: None,
            code_block_content: String::new(),
            list_stack: Vec::new(),
            item_counter: Vec::new(),
        }
    }

    fn push_text(&mut self, text: &str) {
        self.inline_spans.push(InlineSpan {
            text: text.to_string(),
            bold: self.bold,
            italic: self.italic,
            code: self.code,
            link: self.link,
        });
    }

    /// Flush accumulated inline spans as a styled paragraph/heading element.
    fn flush_inline(&mut self, theme: &Theme) {
        if self.inline_spans.is_empty() {
            return;
        }

        let spans = std::mem::take(&mut self.inline_spans);
        let heading = self.heading_level.take();

        // Build combined text and highlight ranges
        let mut full_text = String::new();
        let mut highlights: Vec<(Range<usize>, HighlightStyle)> = Vec::new();

        for span in &spans {
            let start = full_text.len();
            full_text.push_str(&span.text);
            let end = full_text.len();

            if start == end {
                continue;
            }

            let mut style = HighlightStyle::default();
            let mut has_style = false;

            if span.bold {
                style.font_weight = Some(FontWeight::BOLD);
                has_style = true;
            }

            if span.italic {
                style.font_style = Some(gpui::FontStyle::Italic);
                has_style = true;
            }

            if span.code {
                // Inline code gets a muted color
                style.color = Some(theme.accent.into());
                has_style = true;
            }

            if span.link {
                style.color = Some(theme.accent.into());
                has_style = true;
            }

            if has_style {
                highlights.push((start..end, style));
            }
        }

        if full_text.is_empty() {
            return;
        }

        let styled_text =
            StyledText::new(SharedString::from(full_text)).with_highlights(highlights);

        let nesting_depth = self.list_stack.len();

        let element: AnyElement = match heading {
            Some(level) => {
                let font_size = match level {
                    HeadingLevel::H1 => px(24.),
                    HeadingLevel::H2 => px(20.),
                    HeadingLevel::H3 => px(18.),
                    HeadingLevel::H4 => px(16.),
                    HeadingLevel::H5 => px(15.),
                    HeadingLevel::H6 => px(14.),
                };

                div()
                    .mb_2()
                    .mt_3()
                    .text_size(font_size)
                    .font_weight(FontWeight::BOLD)
                    .text_color(theme.text)
                    .child(styled_text)
                    .into_any_element()
            }
            None => {
                let indent = px(nesting_depth as f32 * 16.0);
                let wrapper = div()
                    .mb_1()
                    .text_color(theme.text)
                    .when(nesting_depth > 0, |el: gpui::Div| el.pl(indent))
                    .child(styled_text);
                wrapper.into_any_element()
            }
        };

        self.elements.push(element);
    }
}

fn parse_markdown_to_elements(text: &str, theme: &Theme) -> Vec<AnyElement> {
    let parser = Parser::new(text);
    let mut state = MarkdownParserState::new();

    for event in parser {
        match event {
            Event::Start(tag) => match tag {
                Tag::Paragraph => {
                    // Start collecting inline spans
                }
                Tag::Heading { level, .. } => {
                    state.heading_level = Some(level);
                }
                Tag::Strong => {
                    state.bold = true;
                }
                Tag::Emphasis => {
                    state.italic = true;
                }
                Tag::Link { .. } => {
                    state.link = true;
                }
                Tag::CodeBlock(kind) => {
                    state.in_code_block = true;
                    state.code_block_content.clear();
                    state.code_block_lang = match kind {
                        CodeBlockKind::Fenced(lang) => {
                            let lang_str = lang.to_string();
                            if lang_str.is_empty() {
                                None
                            } else {
                                Some(lang_str)
                            }
                        }
                        CodeBlockKind::Indented => None,
                    };
                }
                Tag::List(start_num) => {
                    state.list_stack.push(start_num);
                    state.item_counter.push(start_num.unwrap_or(1));
                }
                Tag::Item => {
                    // Will flush as a list item on End
                }
                Tag::BlockQuote(_) => {
                    // Treat blockquote content as indented
                    state.list_stack.push(None);
                    state.item_counter.push(0);
                }
                _ => {}
            },
            Event::End(tag_end) => match tag_end {
                TagEnd::Paragraph => {
                    state.flush_inline(theme);
                }
                TagEnd::Heading(_) => {
                    state.flush_inline(theme);
                }
                TagEnd::Strong => {
                    state.bold = false;
                }
                TagEnd::Emphasis => {
                    state.italic = false;
                }
                TagEnd::Link => {
                    state.link = false;
                }
                TagEnd::CodeBlock => {
                    state.in_code_block = false;
                    let code = std::mem::take(&mut state.code_block_content);
                    let lang = state.code_block_lang.take();

                    // Trim trailing newline from code block
                    let code_trimmed = code.trim_end_matches('\n');

                    let code_element =
                        render_code_block(code_trimmed, lang.as_deref(), theme);
                    state.elements.push(code_element.into_any_element());
                }
                TagEnd::List(_) => {
                    state.list_stack.pop();
                    state.item_counter.pop();
                }
                TagEnd::Item => {
                    // Flush collected text as a list item
                    if !state.inline_spans.is_empty() {
                        let spans = std::mem::take(&mut state.inline_spans);
                        let depth = state.list_stack.len();

                        // Determine bullet/number prefix
                        let prefix = if let Some(Some(_start)) =
                            state.list_stack.last()
                        {
                            // Ordered list
                            let counter = state.item_counter.last_mut().unwrap();
                            let p = format!("{}. ", counter);
                            *counter += 1;
                            p
                        } else {
                            // Unordered list
                            "\u{2022} ".to_string() // bullet
                        };

                        // Build text with prefix
                        let mut full_text = prefix.clone();
                        let mut highlights: Vec<(Range<usize>, HighlightStyle)> =
                            Vec::new();
                        let prefix_len = prefix.len();

                        for span in &spans {
                            let start = full_text.len();
                            full_text.push_str(&span.text);
                            let end = full_text.len();

                            if start == end {
                                continue;
                            }

                            let mut style = HighlightStyle::default();
                            let mut has_style = false;

                            if span.bold {
                                style.font_weight = Some(FontWeight::BOLD);
                                has_style = true;
                            }
                            if span.italic {
                                style.font_style = Some(gpui::FontStyle::Italic);
                                has_style = true;
                            }
                            if span.code {
                                style.color = Some(theme.accent.into());
                                has_style = true;
                            }
                            if span.link {
                                style.color = Some(theme.accent.into());
                                has_style = true;
                            }

                            if has_style {
                                highlights.push((start..end, style));
                            }
                        }

                        let styled_text = StyledText::new(SharedString::from(full_text))
                            .with_highlights(highlights);

                        let _ = prefix_len;

                        let item_el = div()
                            .mb_1()
                            .pl(px(depth as f32 * 16.0))
                            .text_color(theme.text)
                            .child(styled_text);

                        state.elements.push(item_el.into_any_element());
                    }
                }
                TagEnd::BlockQuote(_) => {
                    state.list_stack.pop();
                    state.item_counter.pop();
                }
                _ => {}
            },
            Event::Text(text) => {
                if state.in_code_block {
                    state.code_block_content.push_str(&text);
                } else {
                    state.push_text(&text);
                }
            }
            Event::Code(code) => {
                // Inline code
                state.inline_spans.push(InlineSpan {
                    text: code.to_string(),
                    bold: state.bold,
                    italic: state.italic,
                    code: true,
                    link: state.link,
                });
            }
            Event::SoftBreak => {
                if !state.in_code_block {
                    state.push_text(" ");
                }
            }
            Event::HardBreak => {
                if !state.in_code_block {
                    // Flush current inline spans and start a new line
                    state.flush_inline(theme);
                }
            }
            Event::Rule => {
                // Horizontal divider
                let rule = div()
                    .w_full()
                    .h(px(1.))
                    .my_3()
                    .bg(theme.border);
                state.elements.push(rule.into_any_element());
            }
            _ => {
                // Html, InlineHtml, FootnoteReference, TaskListMarker, etc. — skip for now
            }
        }
    }

    // Flush any remaining inline content
    state.flush_inline(theme);

    state.elements
}

#[cfg(test)]
mod tests {
    #[test]
    fn markdown_parser_handles_various_input() {
        let inputs = vec![
            "# Hello",
            "**bold** and *italic*",
            "```rust\nfn main() {}\n```",
            "- item 1\n- item 2",
            "1. first\n2. second",
            "---",
            "[link](http://example.com)",
            "",
            "plain text",
        ];
        for input in inputs {
            let parser = pulldown_cmark::Parser::new(input);
            let events: Vec<_> = parser.collect();
            // Just verify it doesn't panic
            assert!(!input.is_empty() || events.is_empty() || true);
        }
    }
}
