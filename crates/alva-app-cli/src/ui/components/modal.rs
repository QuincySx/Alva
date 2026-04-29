// INPUT:  ratatui (Frame, Rect, widgets::Block/Borders/Clear), super::theme
// OUTPUT: ModalFrame
// POS:    Renders the chrome of a modal popup (clear backdrop + bordered
//         block + optional title) and returns the inner Rect for content.
//         Components stack INSIDE this frame; ModalFrame doesn't own them.

use ratatui::layout::Rect;
use ratatui::widgets::{Block, Borders, Clear};
use ratatui::Frame;

use super::super::theme::Theme;

/// Pop-up dialog chrome — clears the underlying region, draws a border,
/// optional title, returns the inner Rect for the caller to render
/// arbitrary content into.
///
/// ```ignore
/// let frame_helper = ModalFrame::new(" Settings ");
/// let inner = frame_helper.render(frame, popup_rect, theme);
/// my_picker.render(frame, inner, theme);
/// ```
pub struct ModalFrame<'a> {
    title: &'a str,
    /// When true, shows a "press Esc to close" hint in the bottom border.
    pub esc_hint: bool,
}

impl<'a> ModalFrame<'a> {
    pub fn new(title: &'a str) -> Self {
        Self { title, esc_hint: false }
    }

    pub fn with_esc_hint(mut self) -> Self {
        self.esc_hint = true;
        self
    }

    /// Draw the modal chrome and return the inner content Rect.
    pub fn render(&self, frame: &mut Frame<'_>, area: Rect, theme: &Theme) -> Rect {
        // Clear the backdrop so anything painted earlier doesn't bleed through.
        frame.render_widget(Clear, area);

        let mut block = Block::default()
            .borders(Borders::ALL)
            .border_style(theme.border)
            .title(format!(" {} ", self.title));

        if self.esc_hint {
            block = block.title_bottom(" Esc to close ");
        }

        let inner = block.inner(area);
        frame.render_widget(block, area);
        inner
    }
}
