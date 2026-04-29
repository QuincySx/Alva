// INPUT:  crossterm::Event, ratatui (List, ListState, Frame, Rect),
//         super::{Component, ComponentAction, theme}
// OUTPUT: Picker<T>
// POS:    Generic single-select list with type-to-filter, paging, and
//         keyboard nav. The slash-command typeahead, model picker,
//         session picker, and any future "pick one of N" dialog should
//         use this instead of building from List + state by hand.

use crossterm::event::{Event, KeyCode, KeyEvent, KeyModifiers};
use ratatui::layout::Rect;
use ratatui::style::{Modifier, Style};
use ratatui::widgets::{Block, Borders, List, ListItem, ListState};
use ratatui::Frame;

use super::super::theme::Theme;
use super::{Component, ComponentAction};

/// Generic single-select list. `T` is the value carried with each entry;
/// `label_for` decides how it's rendered. Items are filtered by substring
/// match against the label (case-insensitive).
pub struct Picker<T: Clone> {
    items: Vec<(T, String)>,        // (value, display label)
    filtered: Vec<usize>,            // indices into `items`, post-filter
    selected: usize,                 // index into `filtered`
    query: String,                   // current filter
    title: String,                   // shown in border
    page_size: usize,                // visible rows per page
    page: usize,                     // 0-based current page index
    show_query: bool,                // render `query` in title?
}

impl<T: Clone> Picker<T> {
    pub fn new(items: Vec<(T, String)>, title: impl Into<String>) -> Self {
        let n = items.len();
        Self {
            items,
            filtered: (0..n).collect(),
            selected: 0,
            query: String::new(),
            title: title.into(),
            page_size: 15,
            page: 0,
            show_query: false,
        }
    }

    /// Set max visible rows per page. Default 15.
    pub fn page_size(mut self, n: usize) -> Self {
        self.page_size = n.max(1);
        self
    }

    /// Show the active query string in the title (useful for picker-as-search).
    pub fn show_query(mut self, on: bool) -> Self {
        self.show_query = on;
        self
    }

    /// Replace candidates and reset the cursor.
    pub fn set_items(&mut self, items: Vec<(T, String)>) {
        self.items = items;
        self.refilter();
    }

    /// Update the filter query — called by parent after user typed something.
    pub fn set_query(&mut self, q: &str) {
        self.query = q.to_string();
        self.refilter();
    }

    fn refilter(&mut self) {
        let q = self.query.to_lowercase();
        self.filtered = self
            .items
            .iter()
            .enumerate()
            .filter(|(_, (_, label))| q.is_empty() || label.to_lowercase().contains(&q))
            .map(|(i, _)| i)
            .collect();
        if self.selected >= self.filtered.len() { self.selected = 0; }
        self.page = self.selected / self.page_size;
    }

    pub fn selected_value(&self) -> Option<&T> {
        self.filtered.get(self.selected).map(|i| &self.items[*i].0)
    }

    pub fn selected_label(&self) -> Option<&str> {
        self.filtered.get(self.selected).map(|i| self.items[*i].1.as_str())
    }

    pub fn is_empty(&self) -> bool { self.filtered.is_empty() }

    /// Move selection down by 1 (wraps).
    pub fn next(&mut self) {
        if self.filtered.is_empty() { return; }
        self.selected = (self.selected + 1) % self.filtered.len();
        self.page = self.selected / self.page_size;
    }

    /// Move selection up by 1 (wraps).
    pub fn prev(&mut self) {
        if self.filtered.is_empty() { return; }
        self.selected = if self.selected == 0 { self.filtered.len() - 1 } else { self.selected - 1 };
        self.page = self.selected / self.page_size;
    }

    pub fn page_next(&mut self) {
        if self.filtered.is_empty() { return; }
        let pages = (self.filtered.len() + self.page_size - 1) / self.page_size;
        self.page = (self.page + 1) % pages;
        self.selected = (self.page * self.page_size).min(self.filtered.len() - 1);
    }

    pub fn page_prev(&mut self) {
        if self.filtered.is_empty() { return; }
        let pages = (self.filtered.len() + self.page_size - 1) / self.page_size;
        self.page = if self.page == 0 { pages - 1 } else { self.page - 1 };
        self.selected = (self.page * self.page_size).min(self.filtered.len() - 1);
    }
}

impl<T: Clone> Component for Picker<T> {
    fn render(&self, frame: &mut Frame<'_>, area: Rect, theme: &Theme) {
        let total = self.filtered.len();
        let pages = (total + self.page_size - 1) / self.page_size.max(1);
        let title = if self.show_query && !self.query.is_empty() {
            format!(" {} · {} · {}/{} ", self.title, self.query,
                if total == 0 { 0 } else { self.selected + 1 }, total)
        } else if total == 0 {
            format!(" {} (no matches) ", self.title)
        } else {
            format!(" {} · {}/{} (p {}/{}) ", self.title, self.selected + 1, total,
                self.page + 1, pages.max(1))
        };

        let block = Block::default()
            .borders(Borders::ALL)
            .border_style(theme.border)
            .title(title);

        let start = self.page * self.page_size;
        let end = (start + self.page_size).min(total);
        let visible: Vec<ListItem> = (start..end)
            .map(|i| {
                let label = &self.items[self.filtered[i]].1;
                ListItem::new(format!("  {}", label))
            })
            .collect();

        let list = List::new(visible)
            .block(block)
            .highlight_style(Style::default().add_modifier(Modifier::REVERSED));

        let mut state = ListState::default();
        // selected is global; convert to within-page index for ListState.
        if total > 0 && self.selected >= start && self.selected < end {
            state.select(Some(self.selected - start));
        }

        frame.render_stateful_widget(list, area, &mut state);
    }

    fn handle_event(&mut self, event: Event) -> ComponentAction {
        let Event::Key(KeyEvent { code, modifiers, .. }) = event.clone() else {
            return ComponentAction::Bubble(event);
        };
        match (modifiers, code) {
            (_, KeyCode::Up) => { self.prev(); ComponentAction::None }
            (_, KeyCode::Down) => { self.next(); ComponentAction::None }
            (_, KeyCode::PageUp) => { self.page_prev(); ComponentAction::None }
            (_, KeyCode::PageDown) => { self.page_next(); ComponentAction::None }
            (KeyModifiers::CONTROL, KeyCode::Char('p')) => { self.prev(); ComponentAction::None }
            (KeyModifiers::CONTROL, KeyCode::Char('n')) => { self.next(); ComponentAction::None }
            (_, KeyCode::Enter) | (_, KeyCode::Tab) => match self.selected_label() {
                Some(s) => ComponentAction::Submit(s.to_string()),
                None => ComponentAction::None,
            },
            (_, KeyCode::Esc) => ComponentAction::Dismiss,
            _ => ComponentAction::Bubble(event),
        }
    }
}
