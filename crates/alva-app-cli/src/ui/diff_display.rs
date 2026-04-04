//! Unified diff rendering — green additions, red deletions.
//!
//! Renders `unified diff` format into styled ratatui [`Text`] suitable for
//! permission dialogs (FileEdit preview) or `/diff` output.

use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span, Text};

use super::theme::Theme;

/// Render a unified diff string into styled [`Text`].
///
/// Lines starting with `+` are green, `-` are red, `@@` are cyan,
/// and everything else is dimmed.
pub fn render_diff<'a>(diff: &str, theme: &Theme) -> Text<'a> {
    let mut lines: Vec<Line<'static>> = Vec::new();

    for raw_line in diff.lines() {
        let line = if raw_line.starts_with("+++") || raw_line.starts_with("---") {
            // File header lines
            Line::styled(raw_line.to_owned(), Style::default().fg(Color::White).add_modifier(Modifier::BOLD))
        } else if raw_line.starts_with('+') {
            Line::styled(raw_line.to_owned(), Style::default().fg(Color::Green))
        } else if raw_line.starts_with('-') {
            Line::styled(raw_line.to_owned(), Style::default().fg(Color::Red))
        } else if raw_line.starts_with("@@") {
            Line::styled(raw_line.to_owned(), Style::default().fg(Color::Cyan))
        } else {
            // Context lines
            Line::styled(raw_line.to_owned(), theme.text_dim)
        };
        lines.push(line);
    }

    Text::from(lines)
}

/// Render an old_string → new_string inline diff (for FileEdit permission dialogs).
///
/// Shows the old text in red and the new text in green, separated by a divider.
pub fn render_inline_diff<'a>(old: &str, new: &str, theme: &Theme) -> Text<'a> {
    let mut lines: Vec<Line<'static>> = Vec::new();

    // Old section
    lines.push(Line::styled(
        "── old ──".to_owned(),
        Style::default().fg(Color::Red).add_modifier(Modifier::DIM),
    ));
    for line in old.lines() {
        lines.push(Line::from(vec![
            Span::styled("- ", Style::default().fg(Color::Red)),
            Span::styled(line.to_owned(), Style::default().fg(Color::Red)),
        ]));
    }

    // Divider
    lines.push(Line::styled(
        "── new ──".to_owned(),
        Style::default().fg(Color::Green).add_modifier(Modifier::DIM),
    ));

    // New section
    for line in new.lines() {
        lines.push(Line::from(vec![
            Span::styled("+ ", Style::default().fg(Color::Green)),
            Span::styled(line.to_owned(), Style::default().fg(Color::Green)),
        ]));
    }

    Text::from(lines)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn theme() -> Theme {
        Theme::dark()
    }

    #[test]
    fn diff_colors_additions_green() {
        let diff = "+added line";
        let text = render_diff(diff, &theme());
        assert_eq!(text.lines.len(), 1);
        let style = text.lines[0].style;
        assert_eq!(style.fg, Some(Color::Green));
    }

    #[test]
    fn diff_colors_deletions_red() {
        let diff = "-removed line";
        let text = render_diff(diff, &theme());
        assert_eq!(text.lines.len(), 1);
        let style = text.lines[0].style;
        assert_eq!(style.fg, Some(Color::Red));
    }

    #[test]
    fn diff_colors_hunk_header_cyan() {
        let diff = "@@ -1,3 +1,5 @@";
        let text = render_diff(diff, &theme());
        assert_eq!(text.lines.len(), 1);
        let style = text.lines[0].style;
        assert_eq!(style.fg, Some(Color::Cyan));
    }

    #[test]
    fn diff_file_headers_bold() {
        let diff = "--- a/file.rs\n+++ b/file.rs";
        let text = render_diff(diff, &theme());
        assert_eq!(text.lines.len(), 2);
        assert!(text.lines[0].style.add_modifier.contains(Modifier::BOLD));
        assert!(text.lines[1].style.add_modifier.contains(Modifier::BOLD));
    }

    #[test]
    fn diff_context_lines_dim() {
        let diff = " context line";
        let text = render_diff(diff, &theme());
        assert_eq!(text.lines.len(), 1);
        // Context uses theme.text_dim which is DarkGray
        assert_eq!(text.lines[0].style.fg, Some(Color::DarkGray));
    }

    #[test]
    fn inline_diff_old_new() {
        let text = render_inline_diff("old line", "new line", &theme());
        // Should have: header + old line + divider + new line = 4 lines
        assert_eq!(text.lines.len(), 4);
    }

    #[test]
    fn inline_diff_multiline() {
        let old = "line1\nline2";
        let new = "line1\nline2\nline3";
        let text = render_inline_diff(old, new, &theme());
        // header(1) + old(2) + divider(1) + new(3) = 7
        assert_eq!(text.lines.len(), 7);
    }

    #[test]
    fn full_unified_diff() {
        let diff = "--- a/src/main.rs\n+++ b/src/main.rs\n@@ -1,3 +1,4 @@\n fn main() {\n-    println!(\"hello\");\n+    println!(\"hello world\");\n+    println!(\"goodbye\");\n }";
        let text = render_diff(diff, &theme());
        assert_eq!(text.lines.len(), 8);
    }
}
