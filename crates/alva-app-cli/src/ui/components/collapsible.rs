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
    pub fn opened(mut self) -> Self { self.open = true; self }

    pub fn toggle(&mut self) { self.open = !self.open; }

    /// Total rows this block occupies given the rendering width.
    /// 1 row for header, plus body rows when open.
    pub fn height(&self, width: u16) -> u16 {
        let mut h = 1u16;
        if self.open {
            // Conservative estimate: count text lines and word-wrap to width.
            for line in self.body.lines.iter() {
                let line_w = line.width() as u16;
                let lines = if line_w == 0 { 1 } else { line_w.div_ceil(width.max(1)) };
                h = h.saturating_add(lines);
            }
        }
        h
    }

    pub fn render(&self, frame: &mut Frame<'_>, area: Rect, theme: &Theme) {
        if area.height == 0 { return; }

        let icon = match self.kind {
            CollapsibleKind::Thinking => if self.open { "▼ 💭" } else { "▶ 💭" },
            CollapsibleKind::ToolCall => if self.open { "▼ 🛠 " } else { "▶ 🛠 " },
            CollapsibleKind::Log      => if self.open { "▼ ≡ " } else { "▶ ≡ " },
        };
        let title_style = match self.kind {
            CollapsibleKind::Thinking => theme.text_dim,
            CollapsibleKind::ToolCall => theme.tool_name,
            CollapsibleKind::Log      => theme.text_dim,
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

        if !self.open || area.height < 2 { return; }

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
        self.body.lines.iter().map(|l| {
            l.spans.iter().map(|s| s.content.as_ref()).collect::<String>()
        }).collect::<Vec<_>>().join("\n")
    }

    pub fn header_style_dim(theme: &Theme) -> Style {
        Style::default()
            .fg(theme.text_dim.fg.unwrap_or_default())
            .add_modifier(Modifier::ITALIC)
    }
}
