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

#[cfg(test)]
mod tests {
    //! Tests for SpinnerWidget — the SPINNER_FRAMES constant + the
    //! `frame_index % SPINNER_FRAMES.len()` wrap that's the only
    //! protection against an index-out-of-bounds panic on a
    //! long-running animation (frame_index increments forever).
    //!
    //! Render is exercised via `Buffer::empty(...)` (the standard
    //! ratatui widget-test pattern). This is the first Buffer-based
    //! test in alva-app-cli — pattern can be reused for message_list
    //! / other Widget impls later.
    use super::*;
    use ratatui::buffer::Buffer;
    use ratatui::layout::Rect;

    fn theme() -> Theme {
        Theme::default()
    }

    /// Helper: render to a 1-row buffer of `width` cells, return the
    /// joined symbols as a String for substring assertions.
    fn render_to_string(widget: SpinnerWidget<'_>, width: u16) -> String {
        let area = Rect::new(0, 0, width, 1);
        let mut buf = Buffer::empty(area);
        widget.render(area, &mut buf);
        let mut s = String::new();
        for x in 0..width {
            s.push_str(buf[(x, 0)].symbol());
        }
        s
    }

    // -- SPINNER_FRAMES constant ------------------------------------------

    #[test]
    fn spinner_frames_has_eight_entries() {
        // 8 frames is the published animation length — UI calls
        // increment frame_index once per tick, so a change here
        // changes the spinner's perceived speed.
        assert_eq!(SPINNER_FRAMES.len(), 8);
    }

    #[test]
    fn spinner_frames_are_all_non_empty() {
        for (i, f) in SPINNER_FRAMES.iter().enumerate() {
            assert!(!f.is_empty(), "SPINNER_FRAMES[{i}] is empty");
        }
    }

    // -- Builders ----------------------------------------------------------

    #[test]
    fn tip_builder_chains_and_doesnt_panic() {
        let theme = theme();
        // Just verifying the builder chain compiles + returns a
        // usable widget. Field is private so direct assert isn't
        // possible — exercise via render in next tests.
        let _w = SpinnerWidget::new(0, "msg", &theme).tip("hint");
    }

    // -- Render: frame char at column 0 -----------------------------------

    #[test]
    fn render_places_first_frame_at_column_zero() {
        let theme = theme();
        let s = render_to_string(SpinnerWidget::new(0, "loading", &theme), 40);
        // First grapheme is the braille spinner character.
        assert!(
            s.starts_with(SPINNER_FRAMES[0]),
            "expected leading frame '{}', got '{s}'",
            SPINNER_FRAMES[0],
        );
    }

    #[test]
    fn render_message_text_appears_after_spinner_and_space() {
        let theme = theme();
        let s = render_to_string(SpinnerWidget::new(0, "loading", &theme), 40);
        // The message starts somewhere after the spinner char.
        assert!(
            s.contains("loading"),
            "message missing from rendered output: '{s}'"
        );
    }

    // -- frame_index modular wrap -----------------------------------------

    #[test]
    fn render_wraps_frame_index_at_modulus() {
        let theme = theme();
        // Index 8 should wrap to index 0 (8 % 8 == 0).
        let s = render_to_string(SpinnerWidget::new(8, "x", &theme), 40);
        assert!(s.starts_with(SPINNER_FRAMES[0]));

        // Index 10 should wrap to index 2 (10 % 8 == 2).
        let s = render_to_string(SpinnerWidget::new(10, "x", &theme), 40);
        assert!(
            s.starts_with(SPINNER_FRAMES[2]),
            "expected SPINNER_FRAMES[2]='{}', got '{s}'",
            SPINNER_FRAMES[2],
        );
    }

    #[test]
    fn render_with_usize_max_frame_index_does_not_panic() {
        // The whole point of the % SPINNER_FRAMES.len() wrap: a
        // frame_index that's been incremented for "forever" still
        // resolves to a valid index. Without the modulo, the lookup
        // would panic with index-out-of-bounds on a long session.
        let theme = theme();
        let s = render_to_string(SpinnerWidget::new(usize::MAX, "x", &theme), 40);
        // usize::MAX % 8 is the expected frame.
        let expected_idx = usize::MAX % SPINNER_FRAMES.len();
        assert!(s.starts_with(SPINNER_FRAMES[expected_idx]));
    }

    // -- Tip rendering -----------------------------------------------------

    #[test]
    fn render_with_tip_includes_em_dash_and_tip_text() {
        let theme = theme();
        let s = render_to_string(
            SpinnerWidget::new(0, "primary", &theme).tip("press q to quit"),
            60,
        );
        // The em-dash separator is U+2014; the tip text must appear
        // after it.
        assert!(
            s.contains("\u{2014}"),
            "tip separator (em-dash) missing: '{s}'"
        );
        assert!(s.contains("press q to quit"), "tip text missing: '{s}'");
    }

    #[test]
    fn render_without_tip_has_no_em_dash() {
        let theme = theme();
        let s = render_to_string(SpinnerWidget::new(0, "no tip here", &theme), 40);
        assert!(
            !s.contains("\u{2014}"),
            "rendered without tip but found em-dash: '{s}'",
        );
    }
}
