// INPUT:  crossterm::Event, ratatui (Frame, Rect, Paragraph, Block, Borders),
//         super::{Component, ComponentAction, theme}
// OUTPUT: Toggle
// POS:    Bool switch widget — Space / Enter flips, ←/→ also work. Used in
//         settings forms wherever a boolean knob is needed (auto-shell,
//         plan-mode, vim-mode, ...). Renders as `[ ] off` / `[x] on`.

use crossterm::event::{Event, KeyCode, KeyEvent};
use ratatui::layout::Rect;
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Paragraph};
use ratatui::Frame;

use super::super::theme::Theme;
use super::{Component, ComponentAction};

pub struct Toggle {
    label: String,
    value: bool,
    bordered: bool,
}

impl Toggle {
    pub fn new(label: impl Into<String>, value: bool) -> Self {
        Self { label: label.into(), value, bordered: false }
    }

    pub fn bordered(mut self, on: bool) -> Self {
        self.bordered = on;
        self
    }

    pub fn value(&self) -> bool { self.value }
    pub fn set(&mut self, v: bool) { self.value = v; }
}

impl Component for Toggle {
    fn render(&self, frame: &mut Frame<'_>, area: Rect, theme: &Theme) {
        let mark = if self.value { "[x]" } else { "[ ]" };
        let line = Line::from(vec![
            Span::styled(format!("{} ", mark), theme.text),
            Span::styled(self.label.clone(), theme.text),
            Span::styled(
                if self.value { "  on" } else { "  off" }.to_string(),
                theme.text_dim,
            ),
        ]);
        let p = if self.bordered {
            Paragraph::new(line).block(
                Block::default().borders(Borders::ALL).border_style(theme.border),
            )
        } else {
            Paragraph::new(line)
        };
        frame.render_widget(p, area);
    }

    fn handle_event(&mut self, event: Event) -> ComponentAction {
        let Event::Key(KeyEvent { code, .. }) = event.clone() else {
            return ComponentAction::Bubble(event);
        };
        match code {
            KeyCode::Char(' ') | KeyCode::Enter | KeyCode::Char('y') => {
                self.value = !self.value;
                ComponentAction::Changed
            }
            KeyCode::Left => { if self.value { self.value = false; } ComponentAction::Changed }
            KeyCode::Right => { if !self.value { self.value = true; } ComponentAction::Changed }
            KeyCode::Esc => ComponentAction::Dismiss,
            _ => ComponentAction::Bubble(event),
        }
    }
}
