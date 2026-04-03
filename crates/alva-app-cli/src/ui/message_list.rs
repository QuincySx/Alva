//! Conversation message list widget.
//!
//! Renders a scrollable list of conversation messages with role indicators,
//! content lines, and inline tool-use status icons.

use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::Style;
use ratatui::text::{Line, Span, Text};
use ratatui::widgets::{Block, Paragraph, Widget, Wrap};

use super::theme::Theme;

// ---------------------------------------------------------------------------
// Data types
// ---------------------------------------------------------------------------

/// Role of a conversation message.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MessageRole {
    User,
    Assistant,
    System,
    Error,
}

/// Execution status of a tool use.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ToolStatus {
    Running,
    Success,
    Error,
}

/// Visual summary of a single tool invocation.
#[derive(Debug, Clone)]
pub struct ToolUseDisplay {
    pub name: String,
    pub status: ToolStatus,
    pub input_summary: String,
    pub output_preview: String,
}

/// A single message to display in the conversation.
#[derive(Debug, Clone)]
pub struct DisplayMessage {
    pub role: MessageRole,
    pub content: String,
    pub tool_uses: Vec<ToolUseDisplay>,
    pub timestamp: Option<String>,
    pub is_streaming: bool,
}

// ---------------------------------------------------------------------------
// Widget
// ---------------------------------------------------------------------------

/// Renders a list of [`DisplayMessage`]s inside the given area.
pub struct MessageListWidget<'a> {
    messages: &'a [DisplayMessage],
    theme: &'a Theme,
    block: Option<Block<'a>>,
    /// Vertical scroll offset (number of lines to skip from top).
    scroll_offset: u16,
}

impl<'a> MessageListWidget<'a> {
    pub fn new(messages: &'a [DisplayMessage], theme: &'a Theme) -> Self {
        Self {
            messages,
            theme,
            block: None,
            scroll_offset: 0,
        }
    }

    pub fn block(mut self, block: Block<'a>) -> Self {
        self.block = block.into();
        self
    }

    pub fn scroll(mut self, offset: u16) -> Self {
        self.scroll_offset = offset;
        self
    }

    // -- internal helpers ---------------------------------------------------

    fn role_indicator(role: MessageRole, theme: &Theme) -> Span<'static> {
        match role {
            MessageRole::User => Span::styled("You ", theme.user_text),
            MessageRole::Assistant => Span::styled("Assistant ", theme.assistant_text),
            MessageRole::System => Span::styled("System ", theme.system_text),
            MessageRole::Error => Span::styled("Error ", theme.error_text),
        }
    }

    fn role_style(role: MessageRole, theme: &Theme) -> Style {
        match role {
            MessageRole::User => theme.user_text,
            MessageRole::Assistant => theme.assistant_text,
            MessageRole::System => theme.system_text,
            MessageRole::Error => theme.error_text,
        }
    }

    fn tool_status_icon(status: ToolStatus, theme: &Theme) -> Span<'static> {
        match status {
            ToolStatus::Running => Span::styled("\u{25cf} ", theme.tool_running), // ●
            ToolStatus::Success => Span::styled("\u{2713} ", theme.tool_success), // ✓
            ToolStatus::Error => Span::styled("\u{2717} ", theme.tool_error),     // ✗
        }
    }

    /// Convert all messages into a single [`Text`] block.
    fn build_text(&self) -> Text<'static> {
        let mut lines: Vec<Line<'static>> = Vec::new();

        for (idx, msg) in self.messages.iter().enumerate() {
            // Blank separator between messages (except before the first).
            if idx > 0 {
                lines.push(Line::default());
            }

            // -- Role header --
            let mut header_spans = vec![Self::role_indicator(msg.role, self.theme)];
            if let Some(ts) = &msg.timestamp {
                header_spans.push(Span::styled(
                    ts.clone(),
                    self.theme.text_dim,
                ));
            }
            if msg.is_streaming {
                header_spans.push(Span::styled(" ...", self.theme.text_dim));
            }
            lines.push(Line::from(header_spans));

            // -- Content lines --
            let content_style = Self::role_style(msg.role, self.theme);
            for content_line in msg.content.lines() {
                lines.push(Line::styled(content_line.to_owned(), content_style));
            }

            // -- Tool uses --
            for tool in &msg.tool_uses {
                let icon = Self::tool_status_icon(tool.status, self.theme);
                let name_span = Span::styled(tool.name.clone(), self.theme.tool_name);
                let input_span = Span::styled(
                    format!(" {}", tool.input_summary),
                    self.theme.text_dim,
                );
                lines.push(Line::from(vec![
                    Span::raw("  "),
                    icon,
                    name_span,
                    input_span,
                ]));

                if !tool.output_preview.is_empty() {
                    lines.push(Line::from(vec![
                        Span::raw("    "),
                        Span::styled(tool.output_preview.clone(), self.theme.text_dim),
                    ]));
                }
            }
        }

        Text::from(lines)
    }
}

impl<'a> Widget for MessageListWidget<'a> {
    fn render(self, area: Rect, buf: &mut Buffer) {
        let text = self.build_text();

        let mut paragraph = Paragraph::new(text)
            .wrap(Wrap { trim: false })
            .scroll((self.scroll_offset, 0));

        if let Some(blk) = self.block {
            paragraph = paragraph.block(blk);
        }

        paragraph.render(area, buf);
    }
}
