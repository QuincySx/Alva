// INPUT:  crossterm::Event, ratatui (Frame, Rect, Tabs as RatatuiTabs),
//         super::{Component, ComponentAction, theme}
// OUTPUT: Tabs (component wrapper around ratatui's Tabs widget)
// POS:    Horizontal tab strip with keyboard navigation. Settings UI uses
//         this for the "Models / Agents / Display / Hooks" rail. The body
//         (rendering of the active tab's content) is the parent's job —
//         Tabs only owns the strip + which tab is active.

use crossterm::event::{Event, KeyCode, KeyEvent, KeyModifiers};
use ratatui::layout::Rect;
use ratatui::style::{Modifier, Style};
use ratatui::text::Line;
use ratatui::widgets::{Block, Borders, Tabs as RatatuiTabs};
use ratatui::Frame;

use super::super::theme::Theme;
use super::{Component, ComponentAction};

pub struct Tabs {
    pub titles: Vec<String>,
    pub active: usize,
    pub bordered: bool,
}

impl Tabs {
    pub fn new(titles: Vec<impl Into<String>>) -> Self {
        Self {
            titles: titles.into_iter().map(Into::into).collect(),
            active: 0,
            bordered: false,
        }
    }

    pub fn bordered(mut self, on: bool) -> Self {
        self.bordered = on;
        self
    }

    pub fn next(&mut self) {
        if self.titles.is_empty() { return; }
        self.active = (self.active + 1) % self.titles.len();
    }

    pub fn prev(&mut self) {
        if self.titles.is_empty() { return; }
        self.active = if self.active == 0 { self.titles.len() - 1 } else { self.active - 1 };
    }

    pub fn set_active(&mut self, i: usize) {
        if i < self.titles.len() { self.active = i; }
    }
}

impl Component for Tabs {
    fn render(&self, frame: &mut Frame<'_>, area: Rect, theme: &Theme) {
        let titles: Vec<Line> = self.titles.iter()
            .map(|t| Line::from(t.clone()))
            .collect();
        let mut tabs = RatatuiTabs::new(titles)
            .style(theme.text_dim)
            .highlight_style(Style::default().add_modifier(Modifier::BOLD).fg(theme.text.fg.unwrap_or_default()))
            .select(self.active)
            .divider("│");
        if self.bordered {
            tabs = tabs.block(Block::default().borders(Borders::BOTTOM).border_style(theme.border));
        }
        frame.render_widget(tabs, area);
    }

    fn handle_event(&mut self, event: Event) -> ComponentAction {
        let Event::Key(KeyEvent { code, modifiers, .. }) = event.clone() else {
            return ComponentAction::Bubble(event);
        };
        match (modifiers, code) {
            (KeyModifiers::CONTROL, KeyCode::Tab) | (_, KeyCode::Right) => {
                self.next(); ComponentAction::Changed
            }
            (KeyModifiers::CONTROL | KeyModifiers::SHIFT, KeyCode::BackTab)
            | (KeyModifiers::SHIFT, KeyCode::Tab)
            | (_, KeyCode::Left) => {
                self.prev(); ComponentAction::Changed
            }
            _ => ComponentAction::Bubble(event),
        }
    }
}
