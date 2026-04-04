//! Typeahead / autocomplete for slash commands and file paths.
//!
//! Maintains a filtered list of candidates and tracks the selected index.

/// Typeahead autocomplete state.
#[derive(Debug, Clone)]
pub struct Typeahead {
    /// All known candidates (slash commands, file paths, etc.)
    candidates: Vec<String>,
    /// Filtered candidates matching the current query.
    filtered: Vec<String>,
    /// Currently highlighted index in `filtered`.
    selected: usize,
    /// Whether the typeahead menu is visible.
    active: bool,
    /// Current query string (without `/` prefix).
    query: String,
}

impl Typeahead {
    pub fn new(candidates: Vec<String>) -> Self {
        Self {
            candidates,
            filtered: Vec::new(),
            selected: 0,
            active: false,
            query: String::new(),
        }
    }

    /// Update candidates (e.g., when tools/commands change).
    pub fn set_candidates(&mut self, candidates: Vec<String>) {
        self.candidates = candidates;
        if self.active {
            self.refilter();
        }
    }

    /// Update the filter based on user input.
    ///
    /// Call this whenever the input text changes. If the input starts with `/`,
    /// the typeahead activates and filters commands.
    pub fn update(&mut self, input: &str) {
        if input.starts_with('/') && !input.contains(' ') {
            self.query = input[1..].to_string(); // strip leading /
            self.refilter();
            self.active = !self.filtered.is_empty();
        } else {
            self.active = false;
            self.query.clear();
            self.filtered.clear();
            self.selected = 0;
        }
    }

    fn refilter(&mut self) {
        let query_lower = self.query.to_lowercase();
        self.filtered = self
            .candidates
            .iter()
            .filter(|c| c.to_lowercase().contains(&query_lower))
            .cloned()
            .collect();
        if self.selected >= self.filtered.len() {
            self.selected = 0;
        }
    }

    /// Whether the typeahead menu is visible.
    pub fn is_active(&self) -> bool {
        self.active
    }

    /// Get the filtered candidates.
    pub fn items(&self) -> &[String] {
        &self.filtered
    }

    /// Get the selected index.
    pub fn selected(&self) -> usize {
        self.selected
    }

    /// Get the currently selected item.
    pub fn selected_item(&self) -> Option<&str> {
        self.filtered.get(self.selected).map(|s| s.as_str())
    }

    /// Move selection down.
    pub fn next(&mut self) {
        if !self.filtered.is_empty() {
            self.selected = (self.selected + 1) % self.filtered.len();
        }
    }

    /// Move selection up.
    pub fn prev(&mut self) {
        if !self.filtered.is_empty() {
            self.selected = if self.selected == 0 {
                self.filtered.len() - 1
            } else {
                self.selected - 1
            };
        }
    }

    /// Accept the current selection — returns the full command string (with `/` prefix).
    pub fn accept(&mut self) -> Option<String> {
        let item = self.selected_item().map(|s| format!("/{}", s));
        self.dismiss();
        item
    }

    /// Dismiss the typeahead without accepting.
    pub fn dismiss(&mut self) {
        self.active = false;
        self.query.clear();
        self.filtered.clear();
        self.selected = 0;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn commands() -> Vec<String> {
        vec![
            "help".into(),
            "commit".into(),
            "compact".into(),
            "config".into(),
            "cost".into(),
            "clear".into(),
            "doctor".into(),
            "exit".into(),
            "export".into(),
            "model".into(),
            "plan".into(),
            "status".into(),
            "tools".into(),
        ]
    }

    #[test]
    fn inactive_by_default() {
        let ta = Typeahead::new(commands());
        assert!(!ta.is_active());
    }

    #[test]
    fn activates_on_slash() {
        let mut ta = Typeahead::new(commands());
        ta.update("/");
        assert!(ta.is_active());
        assert_eq!(ta.items().len(), commands().len());
    }

    #[test]
    fn filters_by_query() {
        let mut ta = Typeahead::new(commands());
        ta.update("/co");
        assert!(ta.is_active());
        // Should match: commit, compact, config, cost
        assert_eq!(ta.items().len(), 4);
        assert!(ta.items().iter().all(|c| c.contains("co")));
    }

    #[test]
    fn filters_case_insensitive() {
        let mut ta = Typeahead::new(commands());
        ta.update("/CO");
        assert_eq!(ta.items().len(), 4);
    }

    #[test]
    fn deactivates_on_space() {
        let mut ta = Typeahead::new(commands());
        ta.update("/commit fix bug");
        assert!(!ta.is_active());
    }

    #[test]
    fn deactivates_on_non_slash() {
        let mut ta = Typeahead::new(commands());
        ta.update("hello");
        assert!(!ta.is_active());
    }

    #[test]
    fn next_wraps_around() {
        let mut ta = Typeahead::new(commands());
        ta.update("/ex"); // exit, export
        assert_eq!(ta.items().len(), 2);
        assert_eq!(ta.selected(), 0);
        ta.next();
        assert_eq!(ta.selected(), 1);
        ta.next();
        assert_eq!(ta.selected(), 0); // wrap
    }

    #[test]
    fn prev_wraps_around() {
        let mut ta = Typeahead::new(commands());
        ta.update("/ex");
        assert_eq!(ta.selected(), 0);
        ta.prev();
        assert_eq!(ta.selected(), 1); // wrap to end
    }

    #[test]
    fn accept_returns_full_command() {
        let mut ta = Typeahead::new(commands());
        ta.update("/he");
        assert_eq!(ta.selected_item(), Some("help"));
        let accepted = ta.accept();
        assert_eq!(accepted, Some("/help".to_string()));
        assert!(!ta.is_active());
    }

    #[test]
    fn dismiss_clears_state() {
        let mut ta = Typeahead::new(commands());
        ta.update("/co");
        assert!(ta.is_active());
        ta.dismiss();
        assert!(!ta.is_active());
        assert!(ta.items().is_empty());
    }

    #[test]
    fn empty_filter_deactivates() {
        let mut ta = Typeahead::new(commands());
        ta.update("/zzz");
        assert!(!ta.is_active()); // no matches
    }
}
