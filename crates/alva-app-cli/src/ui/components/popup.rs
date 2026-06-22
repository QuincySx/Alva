// INPUT:  ratatui (Frame, Rect), tui_popup::Popup, super::theme
// OUTPUT: ScrollablePopup — facade over tui-popup
// POS:    When `ModalFrame` isn't enough — e.g. you need a popup whose
//         body scrolls, or you want movable / draggable popups in the
//         future — use this. ModalFrame is still the default for static
//         "draw a bordered region" cases.

use ratatui::layout::Rect;
use ratatui::text::Text;
use ratatui::widgets::Block;
use ratatui::Frame;

use super::super::theme::Theme;

/// Scrollable popup using `tui-popup`. Title shown in the border, body is
/// the `Text` you pass in. Scrolling is handled by the underlying widget;
/// keyboard routing is the parent's job (parent sets scroll offset via
/// `Popup::scroll_offset` upstream API if needed — kept simple here).
pub struct ScrollablePopup<'a> {
    title: &'a str,
    body: Text<'a>,
}

impl<'a> ScrollablePopup<'a> {
    pub fn new(title: &'a str, body: impl Into<Text<'a>>) -> Self {
        Self {
            title,
            body: body.into(),
        }
    }

    pub fn render(&self, frame: &mut Frame<'_>, area: Rect, theme: &Theme) {
        let popup = tui_popup::Popup::new(self.body.clone())
            .title(self.title)
            .style(theme.text)
            .borders(ratatui::widgets::Borders::ALL);
        frame.render_widget(&popup, area);
        let _ = Block::default(); // suppress unused-import lint when ratatui rev shifts
    }
}
