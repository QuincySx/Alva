//! Ctrl+R reverse history search.
//!
//! Manages the search state — query string, matched entries, and navigation.

/// Reverse history search state (Ctrl+R).
#[derive(Debug, Clone)]
pub struct HistorySearch {
    /// Whether the search overlay is active.
    active: bool,
    /// Current search query.
    query: String,
    /// All history entries (most recent first).
    entries: Vec<String>,
    /// Indices into `entries` that match the current query.
    matches: Vec<usize>,
    /// Index into `matches` for the currently highlighted result.
    match_cursor: usize,
}

impl HistorySearch {
    pub fn new() -> Self {
        Self {
            active: false,
            query: String::new(),
            entries: Vec::new(),
            matches: Vec::new(),
            match_cursor: 0,
        }
    }

    /// Load history entries (should be most-recent-first).
    pub fn set_entries(&mut self, entries: Vec<String>) {
        self.entries = entries;
    }

    /// Activate the search (Ctrl+R pressed).
    pub fn activate(&mut self) {
        self.active = true;
        self.query.clear();
        self.matches.clear();
        self.match_cursor = 0;
        // Initially all entries match
        self.matches = (0..self.entries.len()).collect();
    }

    /// Deactivate without accepting.
    pub fn cancel(&mut self) {
        self.active = false;
        self.query.clear();
        self.matches.clear();
    }

    pub fn is_active(&self) -> bool {
        self.active
    }

    pub fn query(&self) -> &str {
        &self.query
    }

    /// Add a character to the search query and re-filter.
    pub fn push_char(&mut self, ch: char) {
        self.query.push(ch);
        self.refilter();
    }

    /// Remove the last character from the search query.
    pub fn pop_char(&mut self) {
        self.query.pop();
        self.refilter();
    }

    /// Move to the next match (older entry).
    pub fn next_match(&mut self) {
        if !self.matches.is_empty() {
            self.match_cursor = (self.match_cursor + 1) % self.matches.len();
        }
    }

    /// Move to the previous match (newer entry).
    pub fn prev_match(&mut self) {
        if !self.matches.is_empty() {
            self.match_cursor = if self.match_cursor == 0 {
                self.matches.len() - 1
            } else {
                self.match_cursor - 1
            };
        }
    }

    /// Get the currently selected entry text.
    pub fn selected_entry(&self) -> Option<&str> {
        let idx = self.matches.get(self.match_cursor)?;
        self.entries.get(*idx).map(|s| s.as_str())
    }

    /// Get the visible matches (up to `max` items around the cursor).
    pub fn visible_matches(&self, max: usize) -> Vec<(usize, &str)> {
        let total = self.matches.len();
        if total == 0 {
            return Vec::new();
        }

        let start = self.match_cursor.saturating_sub(max / 2);
        let end = (start + max).min(total);

        self.matches[start..end]
            .iter()
            .enumerate()
            .map(|(i, &idx)| (start + i, self.entries[idx].as_str()))
            .collect()
    }

    /// Accept the current selection and deactivate. Returns the selected text.
    pub fn accept(&mut self) -> Option<String> {
        let result = self.selected_entry().map(|s| s.to_string());
        self.active = false;
        self.query.clear();
        self.matches.clear();
        result
    }

    /// Number of matching entries.
    pub fn match_count(&self) -> usize {
        self.matches.len()
    }

    /// Current cursor position in the match list.
    pub fn match_cursor(&self) -> usize {
        self.match_cursor
    }

    fn refilter(&mut self) {
        let query_lower = self.query.to_lowercase();
        self.matches = self
            .entries
            .iter()
            .enumerate()
            .filter(|(_, e)| {
                if query_lower.is_empty() {
                    true
                } else {
                    e.to_lowercase().contains(&query_lower)
                }
            })
            .map(|(i, _)| i)
            .collect();

        // Clamp cursor
        if self.match_cursor >= self.matches.len() {
            self.match_cursor = 0;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_entries() -> Vec<String> {
        vec![
            "fix authentication bug".into(),
            "add user registration endpoint".into(),
            "refactor database layer".into(),
            "fix CSS alignment issue".into(),
            "add unit tests for auth".into(),
            "update README".into(),
        ]
    }

    #[test]
    fn inactive_by_default() {
        let hs = HistorySearch::new();
        assert!(!hs.is_active());
    }

    #[test]
    fn activate_shows_all_entries() {
        let mut hs = HistorySearch::new();
        hs.set_entries(sample_entries());
        hs.activate();
        assert!(hs.is_active());
        assert_eq!(hs.match_count(), 6);
    }

    #[test]
    fn typing_filters_entries() {
        let mut hs = HistorySearch::new();
        hs.set_entries(sample_entries());
        hs.activate();

        hs.push_char('f');
        hs.push_char('i');
        hs.push_char('x');
        // Should match "fix authentication bug" and "fix CSS alignment issue"
        assert_eq!(hs.match_count(), 2);
    }

    #[test]
    fn case_insensitive_search() {
        let mut hs = HistorySearch::new();
        hs.set_entries(sample_entries());
        hs.activate();
        hs.push_char('R');
        hs.push_char('E');
        hs.push_char('A');
        hs.push_char('D');
        // Should match "update README"
        assert_eq!(hs.match_count(), 1);
        assert_eq!(hs.selected_entry(), Some("update README"));
    }

    #[test]
    fn backspace_widens_filter() {
        let mut hs = HistorySearch::new();
        hs.set_entries(sample_entries());
        hs.activate();
        hs.push_char('f');
        hs.push_char('i');
        hs.push_char('x');
        assert_eq!(hs.match_count(), 2);

        hs.pop_char(); // "fi"
        hs.pop_char(); // "f"
        // "f" matches: fix auth, refactor, fix CSS
        assert!(hs.match_count() >= 2);

        hs.pop_char(); // empty
        assert_eq!(hs.match_count(), 6); // all
    }

    #[test]
    fn next_match_wraps() {
        let mut hs = HistorySearch::new();
        hs.set_entries(sample_entries());
        hs.activate();
        hs.push_char('f');
        hs.push_char('i');
        hs.push_char('x');
        assert_eq!(hs.match_count(), 2);

        assert_eq!(hs.match_cursor(), 0);
        hs.next_match();
        assert_eq!(hs.match_cursor(), 1);
        hs.next_match();
        assert_eq!(hs.match_cursor(), 0); // wrap
    }

    #[test]
    fn prev_match_wraps() {
        let mut hs = HistorySearch::new();
        hs.set_entries(sample_entries());
        hs.activate();
        assert_eq!(hs.match_cursor(), 0);
        hs.prev_match();
        assert_eq!(hs.match_cursor(), 5); // wrap to end
    }

    #[test]
    fn accept_returns_selected() {
        let mut hs = HistorySearch::new();
        hs.set_entries(sample_entries());
        hs.activate();
        hs.push_char('a');
        hs.push_char('u');
        hs.push_char('t');
        hs.push_char('h');
        // Matches: "fix authentication bug", "add unit tests for auth"
        let accepted = hs.accept();
        assert!(accepted.is_some());
        assert!(accepted.unwrap().contains("auth"));
        assert!(!hs.is_active());
    }

    #[test]
    fn cancel_clears_state() {
        let mut hs = HistorySearch::new();
        hs.set_entries(sample_entries());
        hs.activate();
        hs.push_char('f');
        assert!(hs.is_active());
        hs.cancel();
        assert!(!hs.is_active());
        assert_eq!(hs.query(), "");
    }

    #[test]
    fn visible_matches_limited() {
        let mut hs = HistorySearch::new();
        hs.set_entries(sample_entries());
        hs.activate();
        let visible = hs.visible_matches(3);
        assert_eq!(visible.len(), 3);
    }

    #[test]
    fn no_match_returns_none() {
        let mut hs = HistorySearch::new();
        hs.set_entries(sample_entries());
        hs.activate();
        hs.push_char('z');
        hs.push_char('z');
        hs.push_char('z');
        assert_eq!(hs.match_count(), 0);
        assert!(hs.selected_entry().is_none());
    }
}
