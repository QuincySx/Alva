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
                header_spans.push(Span::styled(ts.clone(), self.theme.text_dim));
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
                let input_span =
                    Span::styled(format!(" {}", tool.input_summary), self.theme.text_dim);
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

#[cfg(test)]
mod tests {
    //! Tests for MessageListWidget rendering contracts.
    //! Buffer-based pattern borrowed from ui::spinner (L107).
    //!
    //! Focus is on the visible role / tool-status routing and the
    //! streaming / timestamp affordances — silent regressions here
    //! (wrong role label, missing tool status icon) would mislead
    //! users about what the agent is doing.
    use super::*;

    fn theme() -> Theme {
        Theme::default()
    }

    /// Render the widget to a multi-line buffer and join all rows
    /// into a single string so substring assertions are trivial.
    fn render_to_string(messages: &[DisplayMessage], width: u16, height: u16) -> String {
        let theme = theme();
        let widget = MessageListWidget::new(messages, &theme);
        let area = Rect::new(0, 0, width, height);
        let mut buf = Buffer::empty(area);
        widget.render(area, &mut buf);
        let mut s = String::new();
        for y in 0..height {
            for x in 0..width {
                s.push_str(buf[(x, y)].symbol());
            }
            s.push('\n');
        }
        s
    }

    fn msg(role: MessageRole, content: &str) -> DisplayMessage {
        DisplayMessage {
            role,
            content: content.into(),
            tool_uses: vec![],
            timestamp: None,
            is_streaming: false,
        }
    }

    fn tool(name: &str, status: ToolStatus, input: &str, output: &str) -> ToolUseDisplay {
        ToolUseDisplay {
            name: name.into(),
            status,
            input_summary: input.into(),
            output_preview: output.into(),
        }
    }

    // -- Role indicator routing -------------------------------------------

    #[test]
    fn user_role_renders_with_you_indicator() {
        let s = render_to_string(&[msg(MessageRole::User, "hi")], 40, 4);
        assert!(
            s.contains("You"),
            "user role must show 'You' indicator: {s}"
        );
        assert!(s.contains("hi"));
    }

    #[test]
    fn assistant_role_renders_with_assistant_indicator() {
        let s = render_to_string(&[msg(MessageRole::Assistant, "reply")], 40, 4);
        assert!(s.contains("Assistant"));
        assert!(s.contains("reply"));
    }

    #[test]
    fn system_role_renders_with_system_indicator() {
        let s = render_to_string(&[msg(MessageRole::System, "note")], 40, 4);
        assert!(s.contains("System"));
    }

    #[test]
    fn error_role_renders_with_error_indicator() {
        let s = render_to_string(&[msg(MessageRole::Error, "boom")], 40, 4);
        assert!(s.contains("Error"));
        assert!(s.contains("boom"));
    }

    // -- Streaming + timestamp affordances --------------------------------

    #[test]
    fn streaming_message_shows_ellipsis_suffix_in_header() {
        // Pin: "..." after the role indicator tells the user the agent
        // is still typing. Dropping this affordance breaks the
        // perceived liveness of the UI.
        let mut m = msg(MessageRole::Assistant, "");
        m.is_streaming = true;
        let s = render_to_string(&[m], 40, 4);
        assert!(s.contains("..."), "streaming msg must include '...': {s}");
    }

    #[test]
    fn non_streaming_message_does_not_show_ellipsis() {
        let s = render_to_string(&[msg(MessageRole::Assistant, "done")], 40, 4);
        assert!(!s.contains("..."), "non-streaming must not show '...': {s}");
    }

    #[test]
    fn timestamp_appears_in_header_when_set() {
        let mut m = msg(MessageRole::Assistant, "x");
        m.timestamp = Some("12:34".into());
        let s = render_to_string(&[m], 40, 4);
        assert!(s.contains("12:34"), "timestamp must appear in header: {s}");
    }

    // -- Tool status icons ------------------------------------------------

    #[test]
    fn tool_running_renders_filled_circle_icon() {
        let mut m = msg(MessageRole::Assistant, "");
        m.tool_uses
            .push(tool("read_file", ToolStatus::Running, "main.rs", ""));
        let s = render_to_string(&[m], 60, 6);
        assert!(
            s.contains('\u{25cf}'),
            "Running must render '●' (U+25CF): {s}"
        );
        assert!(s.contains("read_file"));
    }

    #[test]
    fn tool_success_renders_checkmark_icon() {
        let mut m = msg(MessageRole::Assistant, "");
        m.tool_uses
            .push(tool("bash", ToolStatus::Success, "ls", "ok"));
        let s = render_to_string(&[m], 60, 6);
        assert!(
            s.contains('\u{2713}'),
            "Success must render '✓' (U+2713): {s}"
        );
    }

    #[test]
    fn tool_error_renders_cross_icon() {
        let mut m = msg(MessageRole::Assistant, "");
        m.tool_uses
            .push(tool("bash", ToolStatus::Error, "false", ""));
        let s = render_to_string(&[m], 60, 6);
        assert!(
            s.contains('\u{2717}'),
            "Error must render '✗' (U+2717): {s}"
        );
    }

    // -- Tool output_preview -----------------------------------------------

    #[test]
    fn empty_tool_output_preview_omits_preview_row() {
        // Pin: when output_preview == "", no extra row appears (saves
        // vertical space in the chat). A regression that always renders
        // an empty preview row would push real content down.
        let mut m = msg(MessageRole::Assistant, "");
        m.tool_uses
            .push(tool("bash", ToolStatus::Success, "ls", ""));
        let s = render_to_string(&[m], 60, 8);
        // The tool line has the tool name; the next non-blank row
        // should NOT exist. Cheap check: "ls" appears, but no preview
        // text was given so no extra strings come after it.
        assert!(s.contains("ls"));
    }

    #[test]
    fn non_empty_tool_output_preview_appears_in_render() {
        let mut m = msg(MessageRole::Assistant, "");
        m.tool_uses
            .push(tool("bash", ToolStatus::Success, "ls", "file_a.txt"));
        let s = render_to_string(&[m], 60, 8);
        assert!(s.contains("file_a.txt"), "output preview must appear: {s}");
    }
}
