// INPUT:  ratatui (Frame, Rect, Paragraph, Block), super::theme,
//         std::time::Instant
// OUTPUT: Toast, ToastKind, ToastStack
// POS:    Transient status messages (info / success / warn / error). Drawn
//         non-blocking at a corner of the screen for `duration`, then
//         auto-expire. ToastStack lets multiple toasts queue.

use std::time::{Duration, Instant};

use ratatui::layout::Rect;
use ratatui::style::Style;
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Clear, Paragraph};
use ratatui::Frame;

use super::super::theme::Theme;
use super::layout::{anchored_popup, Anchor};

#[derive(Debug, Clone, Copy)]
pub enum ToastKind {
    Info,
    Success,
    Warn,
    Error,
}

#[derive(Debug, Clone)]
pub struct Toast {
    pub message: String,
    pub kind: ToastKind,
    pub expires_at: Instant,
}

impl Toast {
    pub fn info(msg: impl Into<String>) -> Self {
        Self::new(msg, ToastKind::Info, Duration::from_secs(3))
    }
    pub fn success(msg: impl Into<String>) -> Self {
        Self::new(msg, ToastKind::Success, Duration::from_secs(3))
    }
    pub fn warn(msg: impl Into<String>) -> Self {
        Self::new(msg, ToastKind::Warn, Duration::from_secs(4))
    }
    pub fn error(msg: impl Into<String>) -> Self {
        Self::new(msg, ToastKind::Error, Duration::from_secs(5))
    }

    pub fn new(msg: impl Into<String>, kind: ToastKind, dur: Duration) -> Self {
        Self {
            message: msg.into(),
            kind,
            expires_at: Instant::now() + dur,
        }
    }

    pub fn is_alive(&self) -> bool {
        Instant::now() < self.expires_at
    }

    fn icon(&self) -> &'static str {
        match self.kind {
            ToastKind::Info => "i",
            ToastKind::Success => "✓",
            ToastKind::Warn => "!",
            ToastKind::Error => "✖",
        }
    }

    fn style(&self, theme: &Theme) -> Style {
        // Theme already has tool_success / tool_error / status — repurpose
        // those rather than introduce new semantic colors. text_dim covers warn.
        match self.kind {
            ToastKind::Info => theme.text,
            ToastKind::Success => theme.tool_success,
            ToastKind::Warn => theme.text_dim,
            ToastKind::Error => theme.tool_error,
        }
    }
}

/// Vertical stack of toasts at one corner of the screen. Call `tick()`
/// each frame to GC expired entries; `render` draws all live toasts.
#[derive(Default)]
pub struct ToastStack {
    queue: Vec<Toast>,
    pub anchor: Option<Anchor>,
}

impl ToastStack {
    pub fn new() -> Self {
        Self { queue: Vec::new(), anchor: Some(Anchor::TopRight) }
    }

    pub fn push(&mut self, t: Toast) {
        self.queue.push(t);
    }

    /// Drop expired toasts. Returns true if anything was removed.
    pub fn tick(&mut self) -> bool {
        let n = self.queue.len();
        self.queue.retain(|t| t.is_alive());
        n != self.queue.len()
    }

    pub fn is_empty(&self) -> bool { self.queue.is_empty() }

    pub fn render(&self, frame: &mut Frame<'_>, area: Rect, theme: &Theme) {
        if self.queue.is_empty() { return; }
        let anchor = self.anchor.unwrap_or(Anchor::TopRight);
        let max_w = (area.width / 3).max(20).min(60);
        let mut y_offset = 0u16;
        for t in &self.queue {
            let h = 3u16;
            let mut rect = anchored_popup(area, anchor, max_w, h);
            // Offset successive toasts vertically based on the anchor side.
            match anchor {
                Anchor::TopLeft | Anchor::TopRight | Anchor::Top => rect.y += y_offset,
                Anchor::BottomLeft | Anchor::BottomRight | Anchor::Bottom => {
                    rect.y = rect.y.saturating_sub(y_offset);
                }
                _ => rect.y += y_offset,
            }
            y_offset += h;
            if rect.y + rect.height > area.y + area.height { break; }

            frame.render_widget(Clear, rect);
            let line = Line::from(vec![
                Span::styled(format!(" {} ", t.icon()), t.style(theme)),
                Span::styled(t.message.clone(), theme.text),
            ]);
            let p = Paragraph::new(line).block(
                Block::default()
                    .borders(Borders::ALL)
                    .border_style(t.style(theme)),
            );
            frame.render_widget(p, rect);
        }
    }
}
