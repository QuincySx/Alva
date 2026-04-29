// INPUT:  ratatui (Frame, Rect), throbber_widgets_tui (Throbber, ThrobberState),
//         super::theme
// OUTPUT: ProgressThrobber — animated spinner facade
// POS:    Replaces the hand-rolled `ui::spinner::SpinnerFrame` with a
//         throbber-widgets-tui Throbber. We hold a state field so callers
//         tick once per frame to advance the animation.

use std::cell::RefCell;

use ratatui::layout::Rect;
use ratatui::Frame;
use throbber_widgets_tui::{Throbber, ThrobberState};

use super::super::theme::Theme;

/// Animated spinner. `tick()` once per frame to advance it; `render()` paints
/// the current frame at `area` (1×N). Style picked from the theme's
/// `tool_running` so it matches the rest of the UI.
pub struct ProgressThrobber {
    state: RefCell<ThrobberState>,
    label: String,
}

impl ProgressThrobber {
    pub fn new(label: impl Into<String>) -> Self {
        Self {
            state: RefCell::new(ThrobberState::default()),
            label: label.into(),
        }
    }

    /// Advance the animation one frame. Call from your event/render loop —
    /// upstream uses an internal counter, so calling more often = faster spin.
    pub fn tick(&self) {
        self.state.borrow_mut().calc_next();
    }

    pub fn render(&self, frame: &mut Frame<'_>, area: Rect, theme: &Theme) {
        let throbber = Throbber::default()
            .label(self.label.clone())
            .style(theme.tool_running)
            .throbber_style(theme.tool_running)
            .throbber_set(throbber_widgets_tui::CLOCK)
            .use_type(throbber_widgets_tui::WhichUse::Spin);
        let mut state = self.state.borrow_mut();
        frame.render_stateful_widget(throbber, area, &mut state);
    }
}
