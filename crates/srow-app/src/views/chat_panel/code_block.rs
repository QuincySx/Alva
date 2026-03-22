// INPUT:  gpui (div, px, hsla, HighlightStyle, StyledText, SharedString, ClipboardItem,
//         IntoElement, InteractiveElement, ParentElement, Styled, StyleRefinement),
//         once_cell, syntect, crate::theme::Theme
// OUTPUT: pub fn render_code_block
// POS:    Helper function that renders a syntax-highlighted code block with language label and copy button.

use gpui::{
    div, hsla, px, App, ClipboardItem, ClickEvent, HighlightStyle, InteractiveElement,
    IntoElement, ParentElement, SharedString, StatefulInteractiveElement, StyleRefinement, Styled,
    StyledText, Window,
};
use once_cell::sync::Lazy;
use std::ops::Range;
use syntect::easy::HighlightLines;
use syntect::highlighting::ThemeSet;
use syntect::parsing::SyntaxSet;
use syntect::util::LinesWithEndings;

use crate::theme::Theme;

static SYNTAX_SET: Lazy<SyntaxSet> = Lazy::new(|| SyntaxSet::load_defaults_newlines());
static THEME_SET: Lazy<ThemeSet> = Lazy::new(|| ThemeSet::load_defaults());

/// Renders a code block with syntax highlighting, a language label, and a copy button.
///
/// This is a helper function (not a GPUI view) that returns an element to be used
/// inside a parent view's render method.
pub fn render_code_block(
    code: &str,
    language: Option<&str>,
    _theme: &Theme,
) -> impl IntoElement {
    let lang_label = language.unwrap_or("text").to_string();
    let code_owned = code.to_string();
    let code_for_clipboard = code_owned.clone();

    // Dark background for the code block
    let code_bg = hsla(240. / 360., 0.21, 0.15, 1.0); // ~#1e1e2e
    let border_color = hsla(220. / 360., 0.13, 0.28, 1.0); // subtle border
    let header_bg = hsla(220. / 360., 0.13, 0.20, 1.0); // slightly lighter header
    let label_color = hsla(0., 0., 0.65, 1.0); // muted text for label
    let button_color = hsla(0., 0., 0.55, 1.0);
    let button_hover_bg = hsla(0., 0., 1.0, 0.08);

    // Build syntax-highlighted lines
    let highlighted_lines = build_highlighted_lines(&code_owned, language);

    div()
        .w_full()
        .rounded_lg()
        .border_1()
        .border_color(border_color)
        .bg(code_bg)
        .overflow_hidden()
        .mb_2()
        // Header bar: language label + copy button
        .child(
            div()
                .flex()
                .flex_row()
                .items_center()
                .justify_between()
                .px_3()
                .py_1()
                .bg(header_bg)
                .border_b_1()
                .border_color(border_color)
                .child(
                    div()
                        .text_xs()
                        .text_color(label_color)
                        .child(lang_label),
                )
                .child(
                    div()
                        .id("copy-btn")
                        .text_xs()
                        .text_color(button_color)
                        .px_2()
                        .py(px(2.))
                        .rounded_md()
                        .cursor_pointer()
                        .hover(|s: StyleRefinement| s.bg(button_hover_bg))
                        .child("Copy")
                        .on_click(
                            move |_event: &ClickEvent, _window: &mut Window, cx: &mut App| {
                                cx.write_to_clipboard(ClipboardItem::new_string(
                                    code_for_clipboard.clone(),
                                ));
                            },
                        ),
                ),
        )
        // Code content area
        .child(
            div()
                .p_3()
                .overflow_x_hidden()
                .text_sm()
                .font_family("Monaco, Menlo, Consolas, monospace")
                .children(highlighted_lines),
        )
}

/// Build highlighted lines from code using syntect.
fn build_highlighted_lines(code: &str, language: Option<&str>) -> Vec<StyledText> {
    let syntax = language
        .and_then(|lang| SYNTAX_SET.find_syntax_by_token(lang))
        .unwrap_or_else(|| SYNTAX_SET.find_syntax_plain_text());

    let syntect_theme = &THEME_SET.themes["base16-ocean.dark"];
    let mut highlighter = HighlightLines::new(syntax, syntect_theme);

    let mut lines: Vec<StyledText> = Vec::new();

    for line in LinesWithEndings::from(code) {
        match highlighter.highlight_line(line, &SYNTAX_SET) {
            Ok(ranges) => {
                // Build a single string for this line and collect highlight ranges
                let line_text: String = ranges.iter().map(|(_, text)| *text).collect();
                let line_text = line_text.trim_end_matches('\n').to_string();

                let mut highlights: Vec<(Range<usize>, HighlightStyle)> = Vec::new();
                let mut byte_offset = 0usize;

                for (style, text) in &ranges {
                    let text_trimmed = if text.ends_with('\n') {
                        &text[..text.len() - 1]
                    } else {
                        text
                    };

                    if text_trimmed.is_empty() {
                        continue;
                    }

                    let start = byte_offset;
                    let end = start + text_trimmed.len();

                    let fg = style.foreground;

                    // Convert syntect RGBA color to GPUI Hsla
                    let r = fg.r as f32 / 255.0;
                    let g = fg.g as f32 / 255.0;
                    let b = fg.b as f32 / 255.0;
                    let a = fg.a as f32 / 255.0;

                    // RGB to HSL conversion
                    let max = r.max(g).max(b);
                    let min = r.min(g).min(b);
                    let l = (max + min) / 2.0;

                    let (h, s) = if (max - min).abs() < f32::EPSILON {
                        (0.0, 0.0)
                    } else {
                        let d = max - min;
                        let s = if l > 0.5 {
                            d / (2.0 - max - min)
                        } else {
                            d / (max + min)
                        };
                        let h = if (max - r).abs() < f32::EPSILON {
                            (g - b) / d + (if g < b { 6.0 } else { 0.0 })
                        } else if (max - g).abs() < f32::EPSILON {
                            (b - r) / d + 2.0
                        } else {
                            (r - g) / d + 4.0
                        };
                        (h / 6.0, s)
                    };

                    let color = hsla(h, s, l, a);

                    highlights.push((
                        start..end,
                        HighlightStyle {
                            color: Some(color),
                            ..Default::default()
                        },
                    ));

                    byte_offset = end;
                }

                if line_text.is_empty() {
                    // Empty line — render a space to preserve line height
                    lines.push(StyledText::new(SharedString::from(" ".to_string())));
                } else {
                    let styled = StyledText::new(SharedString::from(line_text))
                        .with_highlights(highlights);
                    lines.push(styled);
                }
            }
            Err(_) => {
                // Fallback: render line as plain text
                let plain = line.trim_end_matches('\n').to_string();
                if plain.is_empty() {
                    lines.push(StyledText::new(SharedString::from(" ".to_string())));
                } else {
                    lines.push(StyledText::new(SharedString::from(plain)));
                }
            }
        }
    }

    // If the code was empty, show at least one empty line
    if lines.is_empty() {
        lines.push(StyledText::new(SharedString::from(" ".to_string())));
    }

    lines
}
