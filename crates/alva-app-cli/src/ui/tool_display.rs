//! Tool execution status widget.
//!
//! Renders a compact one-line summary of a tool invocation showing an icon,
//! tool name, input summary, and elapsed time.

use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::text::{Line, Span};
use ratatui::widgets::{Paragraph, Widget};

use super::theme::Theme;

// ---------------------------------------------------------------------------
// Types
// ---------------------------------------------------------------------------

/// Lifecycle state of a tool execution.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ToolExecutionStatus {
    Starting,
    Running,
    Completed,
    Failed,
}

impl ToolExecutionStatus {
    /// Unicode icon representing the status.
    pub fn icon(self) -> &'static str {
        match self {
            Self::Starting => "\u{25cb}",  // ○
            Self::Running => "\u{25cf}",   // ●
            Self::Completed => "\u{2713}", // ✓
            Self::Failed => "\u{2717}",    // ✗
        }
    }
}

// ---------------------------------------------------------------------------
// Widget
// ---------------------------------------------------------------------------

/// Single-line tool execution status display.
pub struct ToolStatusWidget<'a> {
    /// Tool name (e.g. "Bash", "Edit").
    name: &'a str,
    /// Current execution status.
    status: ToolExecutionStatus,
    /// Brief summary of the tool input.
    input_summary: &'a str,
    /// Elapsed wall-clock time as a human string (e.g. "1.2s").
    elapsed: Option<&'a str>,
    theme: &'a Theme,
}

impl<'a> ToolStatusWidget<'a> {
    pub fn new(
        name: &'a str,
        status: ToolExecutionStatus,
        input_summary: &'a str,
        theme: &'a Theme,
    ) -> Self {
        Self {
            name,
            status,
            input_summary,
            elapsed: None,
            theme,
        }
    }

    pub fn elapsed(mut self, elapsed: &'a str) -> Self {
        self.elapsed = Some(elapsed);
        self
    }

    fn status_style(&self) -> ratatui::style::Style {
        match self.status {
            ToolExecutionStatus::Starting => self.theme.text_dim,
            ToolExecutionStatus::Running => self.theme.tool_running,
            ToolExecutionStatus::Completed => self.theme.tool_success,
            ToolExecutionStatus::Failed => self.theme.tool_error,
        }
    }
}

impl<'a> Widget for ToolStatusWidget<'a> {
    fn render(self, area: Rect, buf: &mut Buffer) {
        let mut spans: Vec<Span<'_>> = Vec::with_capacity(6);

        // Icon
        spans.push(Span::styled(self.status.icon(), self.status_style()));
        spans.push(Span::raw(" "));

        // Tool name
        spans.push(Span::styled(self.name, self.theme.tool_name));
        spans.push(Span::raw(" "));

        // Input summary (truncated to fit). Uses byte-safe truncation
        // because tool args often contain CJK paths / emoji that would
        // panic if `max_summary_len-3` landed mid-char.
        let max_summary_len = area.width.saturating_sub(20) as usize;
        let summary = truncate_with_ellipsis(self.input_summary, max_summary_len);
        spans.push(Span::styled(summary, self.theme.text_dim));

        // Elapsed time
        if let Some(elapsed) = self.elapsed {
            spans.push(Span::styled(format!(" ({})", elapsed), self.theme.text_dim));
        }

        let line = Line::from(spans);
        Paragraph::new(line).render(area, buf);
    }
}

/// Truncate `s` so the resulting String is **at most `max_bytes` bytes**
/// when accounting for a trailing `"..."` marker. Backs off to the
/// previous UTF-8 char boundary to avoid panicking on multi-byte chars.
///
/// Contract: `result.len() <= max_bytes` (strict). If `s` already fits,
/// returns it unchanged. If `max_bytes < 3` the budget for kept content
/// is 0, so output is just `"..."` (possibly itself >max_bytes if
/// max_bytes < 3 — pathological terminal width, accepted).
///
/// Note: this is intentionally distinct from `output.rs::safe_preview`
/// which uses `kept ≤ max_bytes` semantics (marker added on top, so
/// total can exceed max_bytes by 3). For the TUI render here the total
/// width is the hard constraint.
fn truncate_with_ellipsis(s: &str, max_bytes: usize) -> String {
    if s.len() <= max_bytes {
        return s.to_string();
    }
    let budget = max_bytes.saturating_sub(3);
    let mut end = budget;
    while end > 0 && !s.is_char_boundary(end) {
        end -= 1;
    }
    format!("{}...", &s[..end])
}

#[cfg(test)]
mod tests {
    //! Tests for `truncate_with_ellipsis`, the TUI render helper that
    //! shapes tool-call input summaries to fit the available column
    //! width. The previous inline byte-slice would panic if `max_bytes
    //! - 3` landed mid-char — common for tool args containing CJK paths
    //! or emoji.
    use super::*;

    #[test]
    fn truncate_with_ellipsis_empty_returns_empty() {
        assert_eq!(truncate_with_ellipsis("", 100), "");
    }

    #[test]
    fn truncate_with_ellipsis_short_string_returned_unchanged() {
        assert_eq!(truncate_with_ellipsis("hello", 100), "hello");
    }

    #[test]
    fn truncate_with_ellipsis_exact_length_returned_unchanged() {
        let s = "a".repeat(50);
        assert_eq!(truncate_with_ellipsis(&s, 50), s);
    }

    #[test]
    fn truncate_with_ellipsis_ascii_over_budget_respects_total_max_bytes() {
        // 100 'a' with max=20 → budget = 17 → "aaa...aaa..." = 20 bytes.
        let s = "a".repeat(100);
        let out = truncate_with_ellipsis(&s, 20);
        assert_eq!(out.len(), 20, "TOTAL output must be ≤ max_bytes");
        assert!(out.ends_with("..."));
        let kept = out.strip_suffix("...").unwrap();
        assert_eq!(kept.len(), 17);
    }

    #[test]
    fn truncate_with_ellipsis_backs_off_from_mid_emoji_at_budget_boundary() {
        // Regression for the panic this loop fixes: input_summary that
        // includes an emoji at `max_bytes - 3` would panic on the
        // naive `&s[..max_bytes.saturating_sub(3)]`.
        //
        // 15 ASCII + 🦀 + 80 ASCII = 99 bytes. max_bytes=20 → budget=17.
        // Byte 17 lands inside the emoji (bytes 15..19) → must back off
        // to byte 15.
        let s = format!("{}{}{}", "a".repeat(15), "🦀", "b".repeat(80));
        assert_eq!(s.len(), 99);
        assert!(!s.is_char_boundary(17), "test premise: byte 17 mid-emoji");
        let out = truncate_with_ellipsis(&s, 20);
        // Must not panic. Output must be ≤ 20 bytes and end on a
        // char boundary in the kept portion.
        assert!(out.len() <= 20);
        let kept = out.strip_suffix("...").unwrap();
        assert!(kept.is_char_boundary(kept.len()));
        // Kept content backs off to byte 15 (just before the emoji),
        // so the entire 15 ASCII 'a's are present, emoji isn't.
        assert_eq!(kept, "a".repeat(15));
    }

    #[test]
    fn truncate_with_ellipsis_realistic_cjk_path_no_crash() {
        // Real scenario: tool input summary contains a CJK path
        // (e.g., `{"path": "/Users/张三/项目/test.rs"}`).
        let s = r#"{"path":"/Users/张三/项目/源代码/超级长的文件名.rs"}"#;
        // Must not panic at any reasonable terminal width.
        for max in [10, 20, 30, 40, 50] {
            let out = truncate_with_ellipsis(s, max);
            assert!(
                out.len() <= max.max(3),
                "max={}: output len {} > max",
                max,
                out.len()
            );
            // Result must be valid UTF-8 (always true if it didn't panic)
            assert!(out.chars().count() > 0 || s.is_empty());
        }
    }
}
