// INPUT:  ratatui (Frame, Rect, Paragraph, Block, Borders), super::theme
// OUTPUT: PinnedHeader — current question stuck at top of conversation
// POS:    Top stripe of the chat screen. Always shows the user's most
//         recent prompt verbatim so they can see what the agent is
//         answering even after a lot of streaming/scrolling pushes the
//         original message far up. Multi-line wraps inside a fixed
//         max-height box.

use ratatui::layout::Rect;
use ratatui::style::Modifier;
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Paragraph, Wrap};
use ratatui::Frame;

use super::super::theme::Theme;

pub struct PinnedHeader<'a> {
    pub question: &'a str,
    /// Hard cap on how many rows the header may occupy (still wraps
    /// internally). Callers usually pass 3-5.
    pub max_rows: u16,
}

impl<'a> PinnedHeader<'a> {
    pub fn new(question: &'a str) -> Self {
        Self { question, max_rows: 4 }
    }

    pub fn max_rows(mut self, n: u16) -> Self {
        self.max_rows = n.max(1);
        self
    }

    pub fn render(&self, frame: &mut Frame<'_>, area: Rect, theme: &Theme) {
        if area.height == 0 || self.question.is_empty() { return; }
        let h = area.height.min(self.max_rows + 2); // +2 for borders
        let area = Rect { height: h, ..area };

        let line = Line::from(vec![
            Span::styled("➤ ",
                ratatui::style::Style::default()
                    .fg(theme.user_text.fg.unwrap_or_default())
                    .add_modifier(Modifier::BOLD)),
            Span::styled(self.question.to_string(), theme.user_text),
        ]);
        let para = Paragraph::new(line)
            .wrap(Wrap { trim: false })
            .block(
                Block::default()
                    .borders(Borders::ALL)
                    .title(" You ")
                    .border_style(theme.border),
            );
        frame.render_widget(para, area);
    }
}
