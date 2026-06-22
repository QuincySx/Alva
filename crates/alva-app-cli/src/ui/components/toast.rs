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
        Self {
            queue: Vec::new(),
            anchor: Some(Anchor::TopRight),
        }
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

    pub fn is_empty(&self) -> bool {
        self.queue.is_empty()
    }

    pub fn render(&self, frame: &mut Frame<'_>, area: Rect, theme: &Theme) {
        if self.queue.is_empty() {
            return;
        }
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
            if rect.y + rect.height > area.y + area.height {
                break;
            }

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

#[cfg(test)]
mod tests {
    //! Tests for Toast lifecycle + ToastStack GC. render() needs a
    //! Frame so is not exercised; the lifetime + queue logic is what
    //! actually decides what the user sees (and for how long).
    //!
    //! Time-based assertions avoid `std::thread::sleep` — Instants are
    //! constructed directly via `checked_sub` so tests stay fast and
    //! deterministic.
    use super::*;

    /// Build a Toast that's already expired by constructing expires_at
    /// in the past via Instant::now().checked_sub. Falls back to plain
    /// Instant::now() in the impossibly-rare case where the system
    /// uptime is < 1 sec (the test would then be flaky for that one
    /// run, never seen on macOS / Linux CI).
    fn expired_toast(msg: &str, kind: ToastKind) -> Toast {
        Toast {
            message: msg.into(),
            kind,
            expires_at: Instant::now()
                .checked_sub(Duration::from_secs(1))
                .unwrap_or_else(Instant::now),
        }
    }

    // -- Toast constructors: kind + default duration -----------------------

    #[test]
    fn info_uses_three_second_duration() {
        let t = Toast::info("hi");
        assert!(matches!(t.kind, ToastKind::Info));
        // Window: Instant::now() created just before should land within
        // ~ a few ms of construction. expires_at - now should be ~ 3s.
        let remaining = t.expires_at.saturating_duration_since(Instant::now());
        assert!(remaining <= Duration::from_secs(3));
        assert!(
            remaining >= Duration::from_millis(2_500),
            "expected ~3s, got {:?}",
            remaining
        );
    }

    #[test]
    fn success_uses_three_second_duration() {
        let t = Toast::success("hi");
        assert!(matches!(t.kind, ToastKind::Success));
        let remaining = t.expires_at.saturating_duration_since(Instant::now());
        assert!(remaining <= Duration::from_secs(3));
        assert!(remaining >= Duration::from_millis(2_500));
    }

    #[test]
    fn warn_uses_four_second_duration() {
        // UX contract: warn is shown LONGER than info/success.
        let t = Toast::warn("hi");
        assert!(matches!(t.kind, ToastKind::Warn));
        let remaining = t.expires_at.saturating_duration_since(Instant::now());
        assert!(remaining <= Duration::from_secs(4));
        assert!(remaining >= Duration::from_millis(3_500));
    }

    #[test]
    fn error_uses_five_second_duration() {
        // UX contract: error is shown longest (users should not miss it).
        // If a future refactor makes error shorter than warn, this test
        // breaks loudly.
        let t = Toast::error("hi");
        assert!(matches!(t.kind, ToastKind::Error));
        let remaining = t.expires_at.saturating_duration_since(Instant::now());
        assert!(remaining <= Duration::from_secs(5));
        assert!(remaining >= Duration::from_millis(4_500));
    }

    #[test]
    fn new_respects_custom_duration() {
        let t = Toast::new("custom", ToastKind::Info, Duration::from_millis(100));
        let remaining = t.expires_at.saturating_duration_since(Instant::now());
        assert!(remaining <= Duration::from_millis(100));
    }

    // -- is_alive ----------------------------------------------------------

    #[test]
    fn is_alive_true_for_fresh_toast() {
        let t = Toast::info("fresh");
        assert!(t.is_alive());
    }

    #[test]
    fn is_alive_false_after_expiry() {
        let t = expired_toast("dead", ToastKind::Info);
        assert!(
            !t.is_alive(),
            "toast whose expires_at is in the past must NOT be alive"
        );
    }

    // -- ToastStack: defaults ---------------------------------------------

    #[test]
    fn new_starts_empty_with_top_right_anchor() {
        let s = ToastStack::new();
        assert!(s.is_empty());
        assert!(matches!(s.anchor, Some(Anchor::TopRight)));
    }

    #[test]
    fn default_starts_empty_with_no_anchor() {
        // The derived Default differs from ::new — no anchor preset.
        // This matters because render() falls back to TopRight when
        // anchor is None; pinning both code paths exist.
        let s = ToastStack::default();
        assert!(s.is_empty());
        assert!(s.anchor.is_none());
    }

    // -- ToastStack: queue + GC -------------------------------------------

    #[test]
    fn push_then_is_empty_returns_false() {
        let mut s = ToastStack::new();
        assert!(s.is_empty());
        s.push(Toast::info("first"));
        assert!(!s.is_empty());
        s.push(Toast::info("second"));
        assert!(!s.is_empty());
    }

    #[test]
    fn tick_removes_expired_returns_true() {
        // Pin: tick() returns true if ANY toast was removed. Without
        // this signal, callers can't tell when to redraw the stack
        // after expiry (subtle UI lag).
        let mut s = ToastStack::new();
        s.push(expired_toast("old", ToastKind::Info));
        s.push(Toast::info("still alive"));
        assert_eq!(s.is_empty(), false);
        let removed = s.tick();
        assert!(
            removed,
            "tick must report removal when an expired toast was dropped"
        );
        assert!(!s.is_empty(), "live toast must remain");
    }

    #[test]
    fn tick_returns_false_when_nothing_to_remove() {
        // Two paths: all alive, OR already empty. Both must return
        // false so callers don't trigger spurious redraws.
        let mut s = ToastStack::new();
        assert!(!s.tick(), "empty stack tick must report false");
        s.push(Toast::info("alive"));
        assert!(!s.tick(), "all-alive stack tick must report false");
    }

    #[test]
    fn tick_clears_all_expired_in_one_pass() {
        let mut s = ToastStack::new();
        s.push(expired_toast("a", ToastKind::Info));
        s.push(expired_toast("b", ToastKind::Warn));
        s.push(expired_toast("c", ToastKind::Error));
        assert!(s.tick());
        assert!(s.is_empty(), "all expired toasts must be GC'd in one pass");
    }
}
