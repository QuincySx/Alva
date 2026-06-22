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
    items: Vec<(T, String)>, // (value, display label)
    filtered: Vec<usize>,    // indices into `items`, post-filter
    selected: usize,         // index into `filtered`
    query: String,           // current filter
    title: String,           // shown in border
    page_size: usize,        // visible rows per page
    page: usize,             // 0-based current page index
    show_query: bool,        // render `query` in title?
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
        if self.selected >= self.filtered.len() {
            self.selected = 0;
        }
        self.page = self.selected / self.page_size;
    }

    pub fn selected_value(&self) -> Option<&T> {
        self.filtered.get(self.selected).map(|i| &self.items[*i].0)
    }

    pub fn selected_label(&self) -> Option<&str> {
        self.filtered
            .get(self.selected)
            .map(|i| self.items[*i].1.as_str())
    }

    pub fn is_empty(&self) -> bool {
        self.filtered.is_empty()
    }

    /// Move selection down by 1 (wraps).
    pub fn next(&mut self) {
        if self.filtered.is_empty() {
            return;
        }
        self.selected = (self.selected + 1) % self.filtered.len();
        self.page = self.selected / self.page_size;
    }

    /// Move selection up by 1 (wraps).
    pub fn prev(&mut self) {
        if self.filtered.is_empty() {
            return;
        }
        self.selected = if self.selected == 0 {
            self.filtered.len() - 1
        } else {
            self.selected - 1
        };
        self.page = self.selected / self.page_size;
    }

    pub fn page_next(&mut self) {
        if self.filtered.is_empty() {
            return;
        }
        let pages = (self.filtered.len() + self.page_size - 1) / self.page_size;
        self.page = (self.page + 1) % pages;
        self.selected = (self.page * self.page_size).min(self.filtered.len() - 1);
    }

    pub fn page_prev(&mut self) {
        if self.filtered.is_empty() {
            return;
        }
        let pages = (self.filtered.len() + self.page_size - 1) / self.page_size;
        self.page = if self.page == 0 {
            pages - 1
        } else {
            self.page - 1
        };
        self.selected = (self.page * self.page_size).min(self.filtered.len() - 1);
    }
}

impl<T: Clone> Component for Picker<T> {
    fn render(&self, frame: &mut Frame<'_>, area: Rect, theme: &Theme) {
        let total = self.filtered.len();
        let pages = (total + self.page_size - 1) / self.page_size.max(1);
        let title = if self.show_query && !self.query.is_empty() {
            format!(
                " {} · {} · {}/{} ",
                self.title,
                self.query,
                if total == 0 { 0 } else { self.selected + 1 },
                total
            )
        } else if total == 0 {
            format!(" {} (no matches) ", self.title)
        } else {
            format!(
                " {} · {}/{} (p {}/{}) ",
                self.title,
                self.selected + 1,
                total,
                self.page + 1,
                pages.max(1)
            )
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
        let Event::Key(KeyEvent {
            code, modifiers, ..
        }) = event.clone()
        else {
            return ComponentAction::Bubble(event);
        };
        match (modifiers, code) {
            (_, KeyCode::Up) => {
                self.prev();
                ComponentAction::None
            }
            (_, KeyCode::Down) => {
                self.next();
                ComponentAction::None
            }
            (_, KeyCode::PageUp) => {
                self.page_prev();
                ComponentAction::None
            }
            (_, KeyCode::PageDown) => {
                self.page_next();
                ComponentAction::None
            }
            (KeyModifiers::CONTROL, KeyCode::Char('p')) => {
                self.prev();
                ComponentAction::None
            }
            (KeyModifiers::CONTROL, KeyCode::Char('n')) => {
                self.next();
                ComponentAction::None
            }
            (_, KeyCode::Enter) | (_, KeyCode::Tab) => match self.selected_label() {
                Some(s) => ComponentAction::Submit(s.to_string()),
                None => ComponentAction::None,
            },
            (_, KeyCode::Esc) => ComponentAction::Dismiss,
            _ => ComponentAction::Bubble(event),
        }
    }
}

#[cfg(test)]
mod tests {
    //! Tests for Picker<T> — filter + paging + wrap + handle_event
    //! routing. render() needs a Frame so is not exercised; the
    //! state mutations covered here are what actually decide which
    //! item ends up selected when the user hits Enter.
    use super::*;
    use crossterm::event::{KeyEventKind, KeyEventState};

    fn picker_of(items: &[&str]) -> Picker<&'static str> {
        let owned: Vec<(&'static str, String)> = items
            .iter()
            .map(|s| {
                let leaked: &'static str = Box::leak(s.to_string().into_boxed_str());
                (leaked, leaked.to_string())
            })
            .collect();
        Picker::new(owned, "Pick")
    }

    fn key(code: KeyCode, modifiers: KeyModifiers) -> Event {
        Event::Key(KeyEvent {
            code,
            modifiers,
            kind: KeyEventKind::Press,
            state: KeyEventState::empty(),
        })
    }

    // -- Construction / defaults ------------------------------------------

    #[test]
    fn new_picks_first_item_by_default() {
        let p = picker_of(&["alpha", "beta", "gamma"]);
        assert_eq!(p.selected_label(), Some("alpha"));
        assert_eq!(p.selected_value().copied(), Some("alpha"));
        assert!(!p.is_empty());
    }

    #[test]
    fn empty_items_yields_no_selection() {
        let p: Picker<&'static str> = Picker::new(vec![], "Empty");
        assert!(p.is_empty());
        assert_eq!(p.selected_label(), None);
        assert_eq!(p.selected_value(), None);
    }

    // -- Builders -----------------------------------------------------------

    #[test]
    fn page_size_clamps_zero_to_one() {
        // page_size(0) would cause div-by-zero in refilter() and
        // render() (see `self.selected / self.page_size`). The
        // `.max(1)` guard is the only protection.
        let p = picker_of(&["a", "b"]).page_size(0);
        // Smoke: advance through items without panic.
        let mut p = p;
        p.next();
        p.next();
        assert!(p.selected_label().is_some());
    }

    #[test]
    fn show_query_chains_returns_self_state() {
        // Builder pattern smoke — just verify chain doesn't drop state.
        let p = picker_of(&["a", "b"]).show_query(true).page_size(3);
        assert_eq!(p.selected_label(), Some("a"));
    }

    // -- set_query / filter -----------------------------------------------

    #[test]
    fn set_query_filters_case_insensitive_substring() {
        let mut p = picker_of(&["Apple", "banana", "Cherry"]);
        p.set_query("an");
        // "Apple" matches "ap"? no — "an" matches "banana".
        assert_eq!(p.selected_label(), Some("banana"));
    }

    #[test]
    fn set_query_uppercase_still_matches_lowercase_labels() {
        let mut p = picker_of(&["banana", "apple"]);
        p.set_query("BAN");
        assert_eq!(p.selected_label(), Some("banana"));
    }

    #[test]
    fn set_query_empty_restores_full_list() {
        let mut p = picker_of(&["a", "b", "c"]);
        p.set_query("a");
        p.set_query("");
        // First item once again selectable + total restored.
        assert_eq!(p.selected_label(), Some("a"));
        assert!(!p.is_empty());
    }

    #[test]
    fn set_query_no_match_yields_empty() {
        let mut p = picker_of(&["a", "b"]);
        p.set_query("zzz");
        assert!(p.is_empty());
        assert_eq!(p.selected_label(), None);
    }

    #[test]
    fn set_query_resets_out_of_bounds_selection() {
        // Move selection to last, then filter so list shrinks below
        // the prior selected index. selected must reset to 0 rather
        // than indexing past the end.
        let mut p = picker_of(&["alpha", "beta", "gamma"]);
        p.next();
        p.next(); // selected = 2 ("gamma")
        assert_eq!(p.selected_label(), Some("gamma"));
        p.set_query("al"); // only "alpha" survives — selected was 2, out of bounds.
        assert_eq!(p.selected_label(), Some("alpha"));
    }

    // -- set_items ---------------------------------------------------------

    #[test]
    fn set_items_replaces_and_refilters() {
        let mut p = picker_of(&["old1", "old2"]);
        p.next(); // selected=1
        let owned: Vec<(&'static str, String)> = vec![
            ("new-a", "new-a".into()),
            ("new-b", "new-b".into()),
            ("new-c", "new-c".into()),
        ];
        p.set_items(owned);
        // refilter ran — selected stays within new bounds.
        assert_eq!(p.selected_label(), Some("new-b"));
    }

    // -- next / prev wrap --------------------------------------------------

    #[test]
    fn next_wraps_at_end() {
        let mut p = picker_of(&["a", "b"]);
        p.next(); // selected=1
        p.next(); // wrap back to 0
        assert_eq!(p.selected_label(), Some("a"));
    }

    #[test]
    fn prev_wraps_at_start() {
        let mut p = picker_of(&["a", "b", "c"]);
        // From 0, prev should wrap to last (2).
        p.prev();
        assert_eq!(p.selected_label(), Some("c"));
    }

    #[test]
    fn next_and_prev_on_empty_filter_are_no_op() {
        // Pin: 0-len list. next/prev must NOT panic (index out of
        // bounds in modulo or subtraction).
        let mut p = picker_of(&["a"]);
        p.set_query("zz");
        assert!(p.is_empty());
        p.next();
        p.prev();
        // Still empty, still no selection.
        assert_eq!(p.selected_label(), None);
    }

    // -- page nav ----------------------------------------------------------

    #[test]
    fn page_next_advances_by_page_size_and_cycles() {
        // page_size=2, items: 0..5 → pages of [0,1] [2,3] [4]
        let mut p = picker_of(&["0", "1", "2", "3", "4"]).page_size(2);
        assert_eq!(p.selected_label(), Some("0"));
        p.page_next();
        // page 1 starts at index 2.
        assert_eq!(p.selected_label(), Some("2"));
        p.page_next();
        // page 2 starts at index 4.
        assert_eq!(p.selected_label(), Some("4"));
        p.page_next();
        // wrap to page 0.
        assert_eq!(p.selected_label(), Some("0"));
    }

    #[test]
    fn page_next_last_page_clamps_to_last_index_not_overshoot() {
        // page_size=3, items: 4 → pages [0,1,2] [3]. Last page only
        // has one item; selected must clamp to 3, not overshoot.
        let mut p = picker_of(&["0", "1", "2", "3"]).page_size(3);
        p.page_next();
        assert_eq!(p.selected_label(), Some("3"));
    }

    // -- handle_event ------------------------------------------------------

    #[test]
    fn down_advances_selection() {
        let mut p = picker_of(&["a", "b"]);
        let act = p.handle_event(key(KeyCode::Down, KeyModifiers::NONE));
        assert!(matches!(act, ComponentAction::None));
        assert_eq!(p.selected_label(), Some("b"));
    }

    #[test]
    fn ctrl_n_advances_like_down() {
        let mut p = picker_of(&["a", "b"]);
        p.handle_event(key(KeyCode::Char('n'), KeyModifiers::CONTROL));
        assert_eq!(p.selected_label(), Some("b"));
    }

    #[test]
    fn enter_submits_current_label() {
        let mut p = picker_of(&["only-choice"]);
        let act = p.handle_event(key(KeyCode::Enter, KeyModifiers::NONE));
        match act {
            ComponentAction::Submit(s) => assert_eq!(s, "only-choice"),
            other => panic!("expected Submit, got {other:?}"),
        }
    }

    #[test]
    fn tab_acts_like_enter() {
        // Tab is the typeahead-confirm key in command palettes.
        let mut p = picker_of(&["only-choice"]);
        let act = p.handle_event(key(KeyCode::Tab, KeyModifiers::NONE));
        match act {
            ComponentAction::Submit(s) => assert_eq!(s, "only-choice"),
            other => panic!("expected Submit, got {other:?}"),
        }
    }

    #[test]
    fn enter_on_empty_filter_returns_none() {
        // No selection to submit — must NOT panic on selected_label()
        // returning None.
        let mut p = picker_of(&["a"]);
        p.set_query("zz");
        let act = p.handle_event(key(KeyCode::Enter, KeyModifiers::NONE));
        assert!(matches!(act, ComponentAction::None));
    }

    #[test]
    fn esc_returns_dismiss() {
        let mut p = picker_of(&["a"]);
        let act = p.handle_event(key(KeyCode::Esc, KeyModifiers::NONE));
        assert!(matches!(act, ComponentAction::Dismiss));
    }

    #[test]
    fn unknown_key_bubbles() {
        let mut p = picker_of(&["a"]);
        let ev = key(KeyCode::Char('x'), KeyModifiers::NONE);
        let act = p.handle_event(ev);
        assert!(matches!(act, ComponentAction::Bubble(_)));
    }
}
