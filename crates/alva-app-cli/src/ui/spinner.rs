//! Animated spinner widget with optional tip text.
//!
//! Uses braille animation frames to show activity while the agent is thinking
//! or a tool is executing.

use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::text::{Line, Span};
use ratatui::widgets::{Paragraph, Widget};

use super::theme::Theme;

// ---------------------------------------------------------------------------
// Constants
// ---------------------------------------------------------------------------

/// Braille-pattern animation frames for a smooth spinner.
pub const SPINNER_FRAMES: &[&str] = &[
    "\u{2801}", // ⠁
    "\u{2802}", // ⠂
    "\u{2804}", // ⠄
    "\u{2840}", // ⡀
    "\u{2880}", // ⢀
    "\u{2820}", // ⠠
    "\u{2810}", // ⠐
    "\u{2808}", // ⠈
];

// ---------------------------------------------------------------------------
// Widget
// ---------------------------------------------------------------------------

/// A single-line spinner with a message and optional tip.
pub struct SpinnerWidget<'a> {
    /// Index into [`SPINNER_FRAMES`] (caller increments on each tick).
    frame_index: usize,
    /// Primary status message shown next to the spinner.
    message: &'a str,
    /// Optional hint or tip displayed in dimmed text.
    tip: Option<&'a str>,
    theme: &'a Theme,
}

impl<'a> SpinnerWidget<'a> {
    pub fn new(frame_index: usize, message: &'a str, theme: &'a Theme) -> Self {
        Self {
            frame_index,
            message,
            tip: None,
            theme,
        }
    }

    pub fn tip(mut self, tip: &'a str) -> Self {
        self.tip = Some(tip);
        self
    }

    /// Current frame character (wraps automatically).
    fn frame_char(&self) -> &'static str {
        SPINNER_FRAMES[self.frame_index % SPINNER_FRAMES.len()]
    }
}

impl<'a> Widget for SpinnerWidget<'a> {
    fn render(self, area: Rect, buf: &mut Buffer) {
        let mut spans: Vec<Span<'_>> = Vec::with_capacity(4);

        // Spinner character
        spans.push(Span::styled(self.frame_char(), self.theme.tool_running));
        spans.push(Span::raw(" "));

        // Message
        spans.push(Span::styled(self.message, self.theme.text));

        // Optional tip
        if let Some(tip) = self.tip {
            spans.push(Span::styled(
                format!("  \u{2014} {}", tip), // — tip
                self.theme.text_dim,
            ));
        }

        let line = Line::from(spans);
        Paragraph::new(line).render(area, buf);
    }
}
