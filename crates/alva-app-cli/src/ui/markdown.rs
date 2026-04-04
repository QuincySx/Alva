//! Markdown to styled [`Text`] converter.
//!
//! Handles a practical subset of Markdown that appears in LLM output:
//! headings, bold, italic, code blocks (with syntax highlighting),
//! inline code, lists, blockquotes, tables, links, and horizontal rules.

use ratatui::style::Modifier;
use ratatui::text::{Line, Span, Text};

use super::theme::Theme;

/// Convert a Markdown string into a ratatui [`Text`] with styling applied
/// according to the provided [`Theme`].
pub fn render_markdown<'a>(input: &str, theme: &Theme) -> Text<'a> {
    let mut lines: Vec<Line<'static>> = Vec::new();
    let mut in_code_block = false;
    let mut code_lang: Option<String> = None;
    let theme = theme.clone();

    for raw_line in input.lines() {
        // -- fenced code blocks -------------------------------------------
        if raw_line.trim_start().starts_with("```") {
            if in_code_block {
                // Closing fence
                lines.push(Line::styled(raw_line.to_owned(), theme.code_block_bg));
                in_code_block = false;
                code_lang = None;
            } else {
                // Opening fence — extract language
                let after = raw_line.trim_start().strip_prefix("```").unwrap_or("");
                code_lang = if after.is_empty() {
                    None
                } else {
                    Some(after.trim().to_lowercase())
                };
                in_code_block = true;
                lines.push(Line::styled(raw_line.to_owned(), theme.code_block_bg));
            }
            continue;
        }

        if in_code_block {
            let spans = highlight_code_line(raw_line, code_lang.as_deref(), &theme);
            lines.push(Line::from(spans));
            continue;
        }

        // -- horizontal rule ----------------------------------------------
        let trimmed = raw_line.trim();
        if trimmed.len() >= 3
            && trimmed.chars().all(|c| c == '-' || c == ' ')
            && trimmed.contains('-')
        {
            let rule = "\u{2500}".repeat(40); // ─
            lines.push(Line::styled(rule, theme.text_dim));
            continue;
        }

        // -- table --------------------------------------------------------
        if trimmed.starts_with('|') && trimmed.ends_with('|') {
            // Check if this is a separator row (e.g. |---|---|)
            let inner = &trimmed[1..trimmed.len() - 1];
            if inner.chars().all(|c| c == '-' || c == '|' || c == ':' || c == ' ') {
                let rule = "\u{2500}".repeat(trimmed.len().min(60));
                lines.push(Line::styled(rule, theme.text_dim));
                continue;
            }

            // Data/header row
            let spans = render_table_row(trimmed, &theme);
            lines.push(Line::from(spans));
            continue;
        }

        // -- headings -----------------------------------------------------
        if let Some(rest) = trimmed.strip_prefix("### ") {
            lines.push(Line::styled(
                format!("### {}", rest),
                theme.text_bold.add_modifier(Modifier::UNDERLINED),
            ));
            continue;
        }
        if let Some(rest) = trimmed.strip_prefix("## ") {
            lines.push(Line::styled(
                format!("## {}", rest),
                theme.text_bold.add_modifier(Modifier::UNDERLINED),
            ));
            continue;
        }
        if let Some(rest) = trimmed.strip_prefix("# ") {
            lines.push(Line::styled(
                format!("# {}", rest),
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
            lines.push(Line::from(vec![Span::styled(
                "\u{2502}",
                theme.text_dim,
            )]));
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
        // Also handle * bullets
        if let Some(rest) = trimmed.strip_prefix("* ") {
            let indent = leading_spaces(raw_line);
            let mut spans = vec![
                Span::raw(" ".repeat(indent)),
                Span::styled("\u{2022} ", theme.text),
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
// Syntax highlighting (basic keyword-based)
// ---------------------------------------------------------------------------

/// Apply basic syntax highlighting to a code line.
fn highlight_code_line(line: &str, lang: Option<&str>, theme: &Theme) -> Vec<Span<'static>> {
    // Common keywords for popular languages
    let keywords: &[&str] = match lang {
        Some("rust" | "rs") => &[
            "fn", "let", "mut", "pub", "use", "mod", "struct", "enum", "impl", "trait",
            "where", "for", "in", "if", "else", "match", "return", "self", "Self",
            "async", "await", "const", "static", "type", "crate", "super", "true", "false",
            "loop", "while", "break", "continue", "move", "ref", "as", "dyn", "Box", "Arc",
            "Vec", "Option", "Result", "Some", "None", "Ok", "Err",
        ],
        Some("python" | "py") => &[
            "def", "class", "import", "from", "return", "if", "elif", "else", "for",
            "while", "in", "not", "and", "or", "True", "False", "None", "self", "with",
            "as", "try", "except", "finally", "raise", "yield", "async", "await", "lambda",
        ],
        Some("typescript" | "ts" | "javascript" | "js" | "tsx" | "jsx") => &[
            "function", "const", "let", "var", "return", "if", "else", "for", "while",
            "class", "extends", "import", "export", "from", "default", "async", "await",
            "new", "this", "true", "false", "null", "undefined", "typeof", "interface",
            "type", "enum", "implements", "throw", "try", "catch", "finally", "yield",
        ],
        Some("go") => &[
            "func", "package", "import", "return", "if", "else", "for", "range",
            "var", "const", "type", "struct", "interface", "map", "chan", "go",
            "defer", "select", "case", "switch", "break", "continue", "true", "false", "nil",
        ],
        Some("bash" | "sh" | "shell" | "zsh") => &[
            "if", "then", "else", "elif", "fi", "for", "do", "done", "while", "until",
            "case", "esac", "function", "return", "exit", "echo", "export", "local",
            "set", "unset", "source", "true", "false",
        ],
        _ => &[], // No highlighting for unknown languages
    };

    if keywords.is_empty() {
        // No language-specific highlighting — just apply code block bg
        return vec![Span::styled(line.to_owned(), theme.code_block_bg)];
    }

    let mut spans: Vec<Span<'static>> = Vec::new();

    // Simple token-based highlighting
    let mut chars = line.char_indices().peekable();
    let mut buf = String::new();

    while let Some(&(i, ch)) = chars.peek() {
        // Comments: // or #
        if (ch == '/' && line[i..].starts_with("//"))
            || (ch == '#' && !line[..i].ends_with('$'))
        {
            if !buf.is_empty() {
                flush_buf(&mut buf, keywords, &mut spans, theme);
            }
            spans.push(Span::styled(line[i..].to_owned(), theme.code_comment));
            break;
        }

        // String literals: "..." or '...'
        if ch == '"' || ch == '\'' {
            if !buf.is_empty() {
                flush_buf(&mut buf, keywords, &mut spans, theme);
            }
            let quote = ch;
            let mut string_buf = String::new();
            string_buf.push(ch);
            chars.next(); // consume opening quote
            let mut escaped = false;
            loop {
                match chars.peek() {
                    Some(&(_, c)) if escaped => {
                        string_buf.push(c);
                        escaped = false;
                        chars.next();
                    }
                    Some(&(_, '\\')) => {
                        string_buf.push('\\');
                        escaped = true;
                        chars.next();
                    }
                    Some(&(_, c)) if c == quote => {
                        string_buf.push(c);
                        chars.next();
                        break;
                    }
                    Some(&(_, c)) => {
                        string_buf.push(c);
                        chars.next();
                    }
                    None => break,
                }
            }
            spans.push(Span::styled(string_buf, theme.code_string));
            continue;
        }

        // Word boundary — flush and check keyword
        if !ch.is_alphanumeric() && ch != '_' {
            if !buf.is_empty() {
                flush_buf(&mut buf, keywords, &mut spans, theme);
            }
            buf.push(ch);
            flush_buf(&mut buf, &[], &mut spans, theme); // non-word char, never a keyword
            chars.next();
            continue;
        }

        buf.push(ch);
        chars.next();
    }

    if !buf.is_empty() {
        flush_buf(&mut buf, keywords, &mut spans, theme);
    }

    // If no spans were produced, at least show the line
    if spans.is_empty() {
        spans.push(Span::styled(line.to_owned(), theme.code_block_bg));
    }

    spans
}

/// Flush the buffer as either a keyword span or a regular code span.
fn flush_buf(
    buf: &mut String,
    keywords: &[&str],
    spans: &mut Vec<Span<'static>>,
    theme: &Theme,
) {
    if buf.is_empty() {
        return;
    }
    let text = std::mem::take(buf);
    if keywords.contains(&text.as_str()) {
        spans.push(Span::styled(text, theme.code_keyword));
    } else {
        spans.push(Span::styled(text, theme.code_block_bg));
    }
}

// ---------------------------------------------------------------------------
// Table rendering
// ---------------------------------------------------------------------------

/// Render a markdown table row `| col1 | col2 | col3 |` into styled spans.
fn render_table_row(line: &str, theme: &Theme) -> Vec<Span<'static>> {
    let mut spans: Vec<Span<'static>> = Vec::new();
    let cells: Vec<&str> = line
        .trim_matches('|')
        .split('|')
        .map(|c| c.trim())
        .collect();

    spans.push(Span::styled("\u{2502} ", theme.text_dim)); // │
    for (i, cell) in cells.iter().enumerate() {
        if i > 0 {
            spans.push(Span::styled(" \u{2502} ", theme.text_dim)); // │
        }
        spans.push(Span::styled(cell.to_string(), theme.text));
    }
    spans.push(Span::styled(" \u{2502}", theme.text_dim)); // │

    spans
}

// ---------------------------------------------------------------------------
// Inline formatting
// ---------------------------------------------------------------------------

/// Parse inline formatting (bold `**`, italic `*`, inline code `` ` ``,
/// links `[text](url)`) and return spans.
fn inline_spans(line: &str, theme: &Theme) -> Vec<Span<'static>> {
    let mut spans: Vec<Span<'static>> = Vec::new();
    let mut chars = line.char_indices().peekable();
    let mut buf = String::new();

    while let Some((i, ch)) = chars.next() {
        // -- bold **...**
        if ch == '*' && line[i..].starts_with("**") {
            if !buf.is_empty() {
                spans.push(Span::styled(buf.clone(), theme.text));
                buf.clear();
            }
            chars.next(); // skip second *
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
            spans.push(Span::styled(bold_buf, theme.text_bold));
            continue;
        }

        // -- italic *...*  (single * not followed by *)
        if ch == '*' && !line[i..].starts_with("**") {
            if !buf.is_empty() {
                spans.push(Span::styled(buf.clone(), theme.text));
                buf.clear();
            }
            let mut italic_buf = String::new();
            for (_, c) in chars.by_ref() {
                if c == '*' {
                    break;
                }
                italic_buf.push(c);
            }
            spans.push(Span::styled(
                italic_buf,
                theme.text.add_modifier(Modifier::ITALIC),
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

        // -- link [text](url)
        if ch == '[' {
            let rest = &line[i..];
            if let Some(end) = parse_link(rest) {
                if !buf.is_empty() {
                    spans.push(Span::styled(buf.clone(), theme.text));
                    buf.clear();
                }
                spans.push(Span::styled(
                    end.text.to_owned(),
                    theme.text.add_modifier(Modifier::UNDERLINED),
                ));
                // Advance past the entire [text](url) construct
                let consumed = end.total_len - 1; // -1 because we already consumed '['
                for _ in 0..consumed {
                    chars.next();
                }
                continue;
            }
        }

        buf.push(ch);
    }

    if !buf.is_empty() {
        spans.push(Span::styled(buf, theme.text));
    }

    spans
}

// ---------------------------------------------------------------------------
// Link parsing
// ---------------------------------------------------------------------------

struct ParsedLink<'a> {
    text: &'a str,
    total_len: usize,
}

fn parse_link(s: &str) -> Option<ParsedLink<'_>> {
    // Expected: [text](url)
    if !s.starts_with('[') {
        return None;
    }
    let close_bracket = s.find(']')?;
    let text = &s[1..close_bracket];
    let after = &s[close_bracket + 1..];
    if !after.starts_with('(') {
        return None;
    }
    let close_paren = after.find(')')?;
    let total_len = close_bracket + 1 + close_paren + 1; // [text](url)
    Some(ParsedLink { text, total_len })
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn leading_spaces(s: &str) -> usize {
    s.len() - s.trim_start_matches(' ').len()
}

fn parse_ordered_list(trimmed: &str) -> Option<(u32, &str)> {
    let dot_pos = trimmed.find(". ")?;
    let num_str = &trimmed[..dot_pos];
    let num: u32 = num_str.parse().ok()?;
    let rest = &trimmed[dot_pos + 2..];
    Some((num, rest))
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    fn theme() -> Theme {
        Theme::dark()
    }

    #[test]
    fn headings_rendered() {
        let input = "# Title\n## Subtitle\n### Section";
        let text = render_markdown(input, &theme());
        assert_eq!(text.lines.len(), 3);
    }

    #[test]
    fn code_block_basic() {
        let input = "```\nhello\nworld\n```";
        let text = render_markdown(input, &theme());
        // 4 lines: opening fence, hello, world, closing fence
        assert_eq!(text.lines.len(), 4);
    }

    #[test]
    fn code_block_with_language() {
        let input = "```rust\nfn main() {}\n```";
        let text = render_markdown(input, &theme());
        assert_eq!(text.lines.len(), 3);
        // The code line should have multiple spans (keyword highlight)
        let code_line = &text.lines[1];
        assert!(code_line.spans.len() > 1, "code should be highlighted, got {:?}", code_line.spans);
    }

    #[test]
    fn rust_keywords_highlighted() {
        let input = "```rust\nlet x = 42;\n```";
        let text = render_markdown(input, &theme());
        let code_line = &text.lines[1];
        // Find the "let" span — should have keyword style
        let has_keyword = code_line.spans.iter().any(|s| {
            s.content.as_ref() == "let" && s.style.fg == theme().code_keyword.fg
        });
        assert!(has_keyword, "should highlight 'let' as keyword: {:?}", code_line.spans);
    }

    #[test]
    fn string_literals_highlighted() {
        let input = "```rust\nlet s = \"hello\";\n```";
        let text = render_markdown(input, &theme());
        let code_line = &text.lines[1];
        let has_string = code_line.spans.iter().any(|s| {
            s.content.contains("hello") && s.style.fg == theme().code_string.fg
        });
        assert!(has_string, "should highlight string literal: {:?}", code_line.spans);
    }

    #[test]
    fn comments_highlighted() {
        let input = "```rust\n// this is a comment\n```";
        let text = render_markdown(input, &theme());
        let code_line = &text.lines[1];
        let has_comment = code_line.spans.iter().any(|s| {
            s.content.contains("comment") && s.style.fg == theme().code_comment.fg
        });
        assert!(has_comment, "should highlight comment: {:?}", code_line.spans);
    }

    #[test]
    fn table_rendered() {
        let input = "| Name | Age |\n|------|-----|\n| Alice | 30 |";
        let text = render_markdown(input, &theme());
        assert_eq!(text.lines.len(), 3);
        // Separator row becomes a horizontal line
        let sep = &text.lines[1];
        let content: String = sep.spans.iter().map(|s| s.content.as_ref()).collect();
        assert!(content.contains('\u{2500}'), "separator should be horizontal line");
    }

    #[test]
    fn table_cells_extracted() {
        let input = "| foo | bar |";
        let text = render_markdown(input, &theme());
        let row = &text.lines[0];
        let content: String = row.spans.iter().map(|s| s.content.as_ref()).collect();
        assert!(content.contains("foo"), "should contain cell 'foo': {}", content);
        assert!(content.contains("bar"), "should contain cell 'bar': {}", content);
    }

    #[test]
    fn bold_text() {
        let input = "this is **bold** text";
        let text = render_markdown(input, &theme());
        let spans = &text.lines[0].spans;
        let bold_span = spans.iter().find(|s| s.content.as_ref() == "bold");
        assert!(bold_span.is_some(), "should have bold span");
    }

    #[test]
    fn italic_text() {
        let input = "this is *italic* text";
        let text = render_markdown(input, &theme());
        let spans = &text.lines[0].spans;
        let italic_span = spans.iter().find(|s| {
            s.content.as_ref() == "italic"
                && s.style.add_modifier.contains(Modifier::ITALIC)
        });
        assert!(italic_span.is_some(), "should have italic span: {:?}", spans);
    }

    #[test]
    fn inline_code() {
        let input = "use `foo::bar` here";
        let text = render_markdown(input, &theme());
        let spans = &text.lines[0].spans;
        let code_span = spans.iter().find(|s| s.content.as_ref() == "foo::bar");
        assert!(code_span.is_some(), "should have inline code span");
    }

    #[test]
    fn link_text_underlined() {
        let input = "see [docs](https://example.com) here";
        let text = render_markdown(input, &theme());
        let spans = &text.lines[0].spans;
        let link_span = spans.iter().find(|s| {
            s.content.as_ref() == "docs"
                && s.style.add_modifier.contains(Modifier::UNDERLINED)
        });
        assert!(link_span.is_some(), "should have underlined link text: {:?}", spans);
    }

    #[test]
    fn unordered_list_bullet() {
        let input = "- item one\n- item two";
        let text = render_markdown(input, &theme());
        assert_eq!(text.lines.len(), 2);
        let content: String = text.lines[0].spans.iter().map(|s| s.content.as_ref()).collect();
        assert!(content.contains('\u{2022}'), "should have bullet: {}", content);
    }

    #[test]
    fn star_bullet_list() {
        let input = "* item one";
        let text = render_markdown(input, &theme());
        let content: String = text.lines[0].spans.iter().map(|s| s.content.as_ref()).collect();
        assert!(content.contains('\u{2022}'), "should have bullet for * list: {}", content);
    }

    #[test]
    fn ordered_list() {
        let input = "1. first\n2. second";
        let text = render_markdown(input, &theme());
        assert_eq!(text.lines.len(), 2);
    }

    #[test]
    fn blockquote() {
        let input = "> quoted text";
        let text = render_markdown(input, &theme());
        let content: String = text.lines[0].spans.iter().map(|s| s.content.as_ref()).collect();
        assert!(content.contains('\u{2502}'), "should have vertical bar: {}", content);
    }

    #[test]
    fn horizontal_rule() {
        let input = "---";
        let text = render_markdown(input, &theme());
        let content: String = text.lines[0].spans.iter().map(|s| s.content.as_ref()).collect();
        assert!(content.contains('\u{2500}'), "should have horizontal rule: {}", content);
    }

    #[test]
    fn python_highlighting() {
        let input = "```python\ndef hello():\n    return True\n```";
        let text = render_markdown(input, &theme());
        let code_line = &text.lines[1];
        let has_keyword = code_line.spans.iter().any(|s| {
            s.content.as_ref() == "def" && s.style.fg == theme().code_keyword.fg
        });
        assert!(has_keyword, "should highlight 'def' in python: {:?}", code_line.spans);
    }

    #[test]
    fn javascript_highlighting() {
        let input = "```js\nconst x = 42;\n```";
        let text = render_markdown(input, &theme());
        let code_line = &text.lines[1];
        let has_keyword = code_line.spans.iter().any(|s| {
            s.content.as_ref() == "const" && s.style.fg == theme().code_keyword.fg
        });
        assert!(has_keyword, "should highlight 'const' in js: {:?}", code_line.spans);
    }
}
