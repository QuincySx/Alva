// INPUT:  ratatui (Frame, Rect, Paragraph, Block, Borders), super::theme
// OUTPUT: CollapsibleBlock — generic expand/collapse container
// POS:    Used for thinking blocks, tool-call blocks, log entries — any
//         conversation item where the header is a one-liner summary and
//         the body is a multi-line detail you usually keep folded. The
//         caller decides what's inside the body (markdown, JSON, diff).

use ratatui::layout::Rect;
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span, Text};
use ratatui::widgets::{Block, Borders, Paragraph, Wrap};
use ratatui::Frame;

use super::super::theme::Theme;

/// What sits in the `body` of a CollapsibleBlock. We keep it as a
/// pre-built `Text` so the conversation view can lay it out without
/// special-casing markdown vs raw vs JSON — caller decides.
pub type CollapsibleBody = Text<'static>;

/// One conversation item. Tracks open/closed state and the rendering
/// chrome. The actual height it occupies depends on `is_open`.
pub struct CollapsibleBlock {
    pub kind: CollapsibleKind,
    pub header: String,
    pub body: CollapsibleBody,
    pub open: bool,
    /// Optional badge after the header (e.g. "1.2K chars", "✓ done", "ERR").
    pub badge: Option<String>,
}

#[derive(Debug, Clone, Copy)]
pub enum CollapsibleKind {
    Thinking,
    ToolCall,
    Log,
}

impl CollapsibleBlock {
    pub fn thinking(header: impl Into<String>, body: impl Into<Text<'static>>) -> Self {
        Self {
            kind: CollapsibleKind::Thinking,
            header: header.into(),
            body: body.into(),
            open: false,
            badge: None,
        }
    }
    pub fn tool_call(header: impl Into<String>, body: impl Into<Text<'static>>) -> Self {
        Self {
            kind: CollapsibleKind::ToolCall,
            header: header.into(),
            body: body.into(),
            open: false,
            badge: None,
        }
    }
    pub fn log(header: impl Into<String>, body: impl Into<Text<'static>>) -> Self {
        Self {
            kind: CollapsibleKind::Log,
            header: header.into(),
            body: body.into(),
            open: false,
            badge: None,
        }
    }

    pub fn with_badge(mut self, badge: impl Into<String>) -> Self {
        self.badge = Some(badge.into());
        self
    }
    pub fn opened(mut self) -> Self {
        self.open = true;
        self
    }

    pub fn toggle(&mut self) {
        self.open = !self.open;
    }

    /// Total rows this block occupies given the rendering width.
    /// 1 row for header, plus body rows when open.
    pub fn height(&self, width: u16) -> u16 {
        let mut h = 1u16;
        if self.open {
            // Conservative estimate: count text lines and word-wrap to width.
            for line in self.body.lines.iter() {
                let line_w = line.width() as u16;
                let lines = if line_w == 0 {
                    1
                } else {
                    line_w.div_ceil(width.max(1))
                };
                h = h.saturating_add(lines);
            }
        }
        h
    }

    pub fn render(&self, frame: &mut Frame<'_>, area: Rect, theme: &Theme) {
        if area.height == 0 {
            return;
        }

        let icon = match self.kind {
            CollapsibleKind::Thinking => {
                if self.open {
                    "▼ 💭"
                } else {
                    "▶ 💭"
                }
            }
            CollapsibleKind::ToolCall => {
                if self.open {
                    "▼ 🛠 "
                } else {
                    "▶ 🛠 "
                }
            }
            CollapsibleKind::Log => {
                if self.open {
                    "▼ ≡ "
                } else {
                    "▶ ≡ "
                }
            }
        };
        let title_style = match self.kind {
            CollapsibleKind::Thinking => theme.text_dim,
            CollapsibleKind::ToolCall => theme.tool_name,
            CollapsibleKind::Log => theme.text_dim,
        };

        let mut header_spans = vec![
            Span::styled(format!("{}  ", icon), title_style),
            Span::styled(self.header.clone(), theme.text),
        ];
        if let Some(b) = &self.badge {
            header_spans.push(Span::raw("  "));
            header_spans.push(Span::styled(format!("[{}]", b), theme.text_dim));
        }
        let header_line = Line::from(header_spans);

        // Header row: 1 line at the top of `area`.
        let header_area = Rect { height: 1, ..area };
        frame.render_widget(Paragraph::new(header_line), header_area);

        if !self.open || area.height < 2 {
            return;
        }

        let body_area = Rect {
            x: area.x + 2,
            y: area.y + 1,
            width: area.width.saturating_sub(2),
            height: area.height.saturating_sub(1),
        };
        let body_block = Block::default()
            .borders(Borders::LEFT)
            .border_style(theme.border);
        let body_para = Paragraph::new(self.body.clone())
            .block(body_block)
            .wrap(Wrap { trim: false });
        frame.render_widget(body_para, body_area);
    }
}

/// Convenience helpers used by callers that pass plain strings.
impl CollapsibleBlock {
    pub fn body_text(&self) -> String {
        self.body
            .lines
            .iter()
            .map(|l| {
                l.spans
                    .iter()
                    .map(|s| s.content.as_ref())
                    .collect::<String>()
            })
            .collect::<Vec<_>>()
            .join("\n")
    }

    pub fn header_style_dim(theme: &Theme) -> Style {
        Style::default()
            .fg(theme.text_dim.fg.unwrap_or_default())
            .add_modifier(Modifier::ITALIC)
    }
}

#[cfg(test)]
mod tests {
    //! Tests for CollapsibleBlock — constructor presets, badge/open
    //! builders, the toggle flip, height(width) word-wrap arithmetic
    //! and body_text() plain-string export.
    //!
    //! render() needs a Frame so is intentionally not exercised. The
    //! height contract IS exercised because it feeds the conversation
    //! view's scrolling math — a wrong height makes the user see less
    //! than is actually there (or padded blank rows).
    use super::*;

    fn body_from_strings(lines: &[&str]) -> CollapsibleBody {
        Text::from(
            lines
                .iter()
                .map(|s| Line::raw(s.to_string()))
                .collect::<Vec<_>>(),
        )
    }

    // -- Constructors + defaults -------------------------------------------

    #[test]
    fn thinking_ctor_defaults_closed_no_badge() {
        let b = CollapsibleBlock::thinking("hdr", Text::raw("body"));
        assert!(matches!(b.kind, CollapsibleKind::Thinking));
        assert_eq!(b.header, "hdr");
        assert!(!b.open, "fresh blocks must start closed");
        assert!(b.badge.is_none());
    }

    #[test]
    fn tool_call_ctor_uses_tool_call_kind() {
        let b = CollapsibleBlock::tool_call("hdr", Text::raw("body"));
        assert!(matches!(b.kind, CollapsibleKind::ToolCall));
        assert!(!b.open);
    }

    #[test]
    fn log_ctor_uses_log_kind() {
        let b = CollapsibleBlock::log("hdr", Text::raw("body"));
        assert!(matches!(b.kind, CollapsibleKind::Log));
        assert!(!b.open);
    }

    // -- Builders -----------------------------------------------------------

    #[test]
    fn with_badge_sets_badge_and_chains() {
        let b = CollapsibleBlock::log("hdr", Text::raw("body")).with_badge("done");
        assert_eq!(b.badge.as_deref(), Some("done"));
        assert!(!b.open, "with_badge must not flip open");
    }

    #[test]
    fn opened_sets_open_true_and_chains() {
        let b = CollapsibleBlock::log("hdr", Text::raw("body")).opened();
        assert!(b.open);
        assert!(b.badge.is_none(), "opened must not touch badge");
    }

    #[test]
    fn toggle_flips_open_back_and_forth() {
        let mut b = CollapsibleBlock::log("hdr", Text::raw("body"));
        assert!(!b.open);
        b.toggle();
        assert!(b.open);
        b.toggle();
        assert!(!b.open, "double toggle must return to original state");
    }

    // -- height(width) ------------------------------------------------------

    #[test]
    fn height_closed_is_always_one() {
        let b = CollapsibleBlock::log("hdr", body_from_strings(&["a", "b", "c"]));
        assert_eq!(
            b.height(80),
            1,
            "closed block hides body — only header row counts"
        );
    }

    #[test]
    fn height_open_no_wrap_is_one_plus_lines() {
        // 3 short lines, width 80 → 1 (header) + 3 (body) = 4.
        let b = CollapsibleBlock::log("hdr", body_from_strings(&["a", "bb", "ccc"])).opened();
        assert_eq!(b.height(80), 4);
    }

    #[test]
    fn height_open_with_wrap_uses_div_ceil() {
        // Line of width 25 wrapped to width 10 → ceil(25/10) = 3 visual rows.
        // header (1) + 3 (one wrapped line) = 4.
        let long = "x".repeat(25);
        let b = CollapsibleBlock::log("hdr", body_from_strings(&[long.as_str()])).opened();
        assert_eq!(b.height(10), 4);
    }

    #[test]
    fn height_open_empty_line_counts_as_one_row() {
        // Empty line shouldn't be div_ceil(0/width) = 0 (that would
        // hide it). The implementation guards with `if line_w == 0 { 1 }`.
        let b = CollapsibleBlock::log("hdr", body_from_strings(&[""])).opened();
        assert_eq!(b.height(80), 2, "empty line must still occupy 1 row");
    }

    #[test]
    fn height_open_with_zero_width_does_not_divide_by_zero() {
        // Width 0 should not panic — width.max(1) is the guard inside
        // height(). Pin this regression: dropping `.max(1)` would
        // panic in a div_ceil(_, 0) call.
        let b = CollapsibleBlock::log("hdr", body_from_strings(&["abc"])).opened();
        // Just call it — assertion is "no panic". Result is best-effort.
        let _ = b.height(0);
    }

    // -- body_text ----------------------------------------------------------

    #[test]
    fn body_text_joins_lines_with_newlines() {
        let b = CollapsibleBlock::log("hdr", body_from_strings(&["first", "second", "third"]));
        assert_eq!(b.body_text(), "first\nsecond\nthird");
    }

    #[test]
    fn body_text_concatenates_multi_span_line() {
        // Each Line can have multiple Spans (styled fragments). body_text()
        // should glue them back into a single string per line — UI export /
        // copy-to-clipboard contract.
        let line = Line::from(vec![Span::raw("hello "), Span::raw("world")]);
        let body = Text::from(vec![line]);
        let b = CollapsibleBlock::log("hdr", body);
        assert_eq!(b.body_text(), "hello world");
    }
}
