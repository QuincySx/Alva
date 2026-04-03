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
        spans.push(Span::styled(
            self.status.icon(),
            self.status_style(),
        ));
        spans.push(Span::raw(" "));

        // Tool name
        spans.push(Span::styled(self.name, self.theme.tool_name));
        spans.push(Span::raw(" "));

        // Input summary (truncated to fit)
        let max_summary_len = area.width.saturating_sub(20) as usize;
        let summary: String = if self.input_summary.len() > max_summary_len {
            format!("{}...", &self.input_summary[..max_summary_len.saturating_sub(3)])
        } else {
            self.input_summary.to_owned()
        };
        spans.push(Span::styled(summary, self.theme.text_dim));

        // Elapsed time
        if let Some(elapsed) = self.elapsed {
            spans.push(Span::styled(
                format!(" ({})", elapsed),
                self.theme.text_dim,
            ));
        }

        let line = Line::from(spans);
        Paragraph::new(line).render(area, buf);
    }
}
