//! Markdown to styled [`Text`] converter.
//!
//! Handles a practical subset of Markdown that appears in LLM output:
//! headings, bold, code blocks, inline code, lists, blockquotes, and
//! horizontal rules. Does *not* aim for full CommonMark compliance.

use ratatui::style::Modifier;
use ratatui::text::{Line, Span, Text};

use super::theme::Theme;

/// Convert a Markdown string into a ratatui [`Text`] with styling applied
/// according to the provided [`Theme`].
pub fn render_markdown<'a>(input: &str, theme: &Theme) -> Text<'a> {
    let mut lines: Vec<Line<'static>> = Vec::new();
    let mut in_code_block = false;
    let theme = theme.clone();

    for raw_line in input.lines() {
        // -- fenced code blocks -------------------------------------------
        if raw_line.trim_start().starts_with("```") {
            in_code_block = !in_code_block;
            // Render the fence line itself in code-block style.
            lines.push(Line::styled(raw_line.to_owned(), theme.code_block_bg));
            continue;
        }

        if in_code_block {
            lines.push(Line::styled(raw_line.to_owned(), theme.code_block_bg));
            continue;
        }

        // -- horizontal rule ----------------------------------------------
        let trimmed = raw_line.trim();
        if trimmed.len() >= 3 && trimmed.chars().all(|c| c == '-' || c == ' ') && trimmed.contains('-') {
            let rule = "\u{2500}".repeat(40); // ─ repeated
            lines.push(Line::styled(rule, theme.text_dim));
            continue;
        }

        // -- headings -----------------------------------------------------
        if let Some(rest) = trimmed.strip_prefix("### ") {
            lines.push(Line::styled(
                rest.to_owned(),
                theme.text_bold.add_modifier(Modifier::UNDERLINED),
            ));
            continue;
        }
        if let Some(rest) = trimmed.strip_prefix("## ") {
            lines.push(Line::styled(
                rest.to_owned(),
                theme.text_bold.add_modifier(Modifier::UNDERLINED),
            ));
            continue;
        }
        if let Some(rest) = trimmed.strip_prefix("# ") {
            lines.push(Line::styled(
                rest.to_owned(),
                theme
                    .text_bold
                    .add_modifier(Modifier::UNDERLINED | Modifier::BOLD),
            ));
            continue;
        }

        // -- blockquote ---------------------------------------------------
        if let Some(rest) = trimmed.strip_prefix("> ") {
            lines.push(Line::from(vec![
                Span::styled("\u{2502} ", theme.text_dim), // │
                Span::styled(rest.to_owned(), theme.text_dim),
            ]));
            continue;
        }
        if trimmed == ">" {
            lines.push(Line::from(vec![
                Span::styled("\u{2502}", theme.text_dim),
            ]));
            continue;
        }

        // -- unordered list -----------------------------------------------
        if let Some(rest) = trimmed.strip_prefix("- ") {
            let indent = leading_spaces(raw_line);
            let mut spans = vec![
                Span::raw(" ".repeat(indent)),
                Span::styled("\u{2022} ", theme.text), // •
            ];
            spans.extend(inline_spans(rest, &theme));
            lines.push(Line::from(spans));
            continue;
        }

        // -- ordered list -------------------------------------------------
        if let Some((num, rest)) = parse_ordered_list(trimmed) {
            let indent = leading_spaces(raw_line);
            let mut spans = vec![
                Span::raw(" ".repeat(indent)),
                Span::styled(format!("{}. ", num), theme.text),
            ];
            spans.extend(inline_spans(rest, &theme));
            lines.push(Line::from(spans));
            continue;
        }

        // -- plain / inline-formatted line --------------------------------
        let spans = inline_spans(raw_line, &theme);
        lines.push(Line::from(spans));
    }

    Text::from(lines)
}

// ---------------------------------------------------------------------------
// Inline formatting
// ---------------------------------------------------------------------------

/// Parse inline formatting (bold `**`, inline code `` ` ``) and return spans.
fn inline_spans(line: &str, theme: &Theme) -> Vec<Span<'static>> {
    let mut spans: Vec<Span<'static>> = Vec::new();
    let mut chars = line.char_indices().peekable();
    let mut buf = String::new();

    while let Some((i, ch)) = chars.next() {
        // -- bold **...**
        if ch == '*' && line[i..].starts_with("**") {
            // flush buffer
            if !buf.is_empty() {
                spans.push(Span::styled(buf.clone(), theme.text));
                buf.clear();
            }
            // skip second *
            chars.next();
            // collect until closing **
            let mut bold_buf = String::new();
            loop {
                match chars.next() {
                    Some((j, c)) if c == '*' && line[j..].starts_with("**") => {
                        chars.next(); // skip second *
                        break;
                    }
                    Some((_, c)) => bold_buf.push(c),
                    None => break,
                }
            }
            spans.push(Span::styled(
                bold_buf,
                theme.text_bold,
            ));
            continue;
        }

        // -- inline code `...`
        if ch == '`' {
            if !buf.is_empty() {
                spans.push(Span::styled(buf.clone(), theme.text));
                buf.clear();
            }
            let mut code_buf = String::new();
            for (_, c) in chars.by_ref() {
                if c == '`' {
                    break;
                }
                code_buf.push(c);
            }
            spans.push(Span::styled(
                code_buf,
                theme.code_block_bg.add_modifier(Modifier::BOLD),
            ));
            continue;
        }

        buf.push(ch);
    }

    if !buf.is_empty() {
        spans.push(Span::styled(buf, theme.text));
    }

    spans
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Count leading spaces.
fn leading_spaces(s: &str) -> usize {
    s.len() - s.trim_start_matches(' ').len()
}

/// Try to parse `"1. rest"` style ordered-list prefix.
fn parse_ordered_list(trimmed: &str) -> Option<(u32, &str)> {
    let dot_pos = trimmed.find(". ")?;
    let num_str = &trimmed[..dot_pos];
    let num: u32 = num_str.parse().ok()?;
    let rest = &trimmed[dot_pos + 2..];
    Some((num, rest))
}
