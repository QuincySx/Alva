// INPUT:  reedline (Completer trait, Suggestion, Span)
// OUTPUT: SlashCompleter — pops a list as soon as the user types `/`, then
//         filters live as more characters come in.
// POS:    REPL UX layer for `alva` interactive mode. Completer is invoked
//         by the menu/event handler we configure in repl.rs (`/` keypress
//         binds to `Edit(InsertChar /) + Menu(completion_menu)`).

use reedline::{Completer, Span, Suggestion};

pub struct SlashCompleter {
    /// All known slash commands (no leading `/`).
    commands: Vec<String>,
}

impl SlashCompleter {
    pub fn new(registry_names: Vec<String>) -> Self {
        // REPL-side commands handled directly in repl.rs's match arms (not
        // registered in CommandRegistry). Keep in lockstep with repl.rs.
        let extras = [
            "quit", "resume", "fork", "rewind", "sessions", "setup", "auto", "locks", "model",
        ];
        let mut all: Vec<String> = registry_names;
        for e in extras {
            if !all.iter().any(|c| c == e) {
                all.push(e.to_string());
            }
        }
        all.sort();
        all.dedup();
        Self { commands: all }
    }
}

impl SlashCompleter {
    /// Filter the command list against `q` (the portion the user has typed
    /// after `/`). Empty `q` matches all. Used by both `complete` and
    /// `partial_complete` so they stay in lockstep.
    fn matches(&self, q: &str) -> Vec<&str> {
        self.commands
            .iter()
            .filter(|c| q.is_empty() || c.starts_with(q))
            .map(|s| s.as_str())
            .collect()
    }

    /// Build a Suggestion for one command name. `replace_span` is `0..0`:
    /// when ListMenu accepts a candidate it replaces the *delta* portion
    /// (post-`/`) entirely with `value`. The `/` itself is already in the
    /// buffer from the keybinding, so the suggestion only contributes the
    /// command name.
    fn make(name: &str) -> Suggestion {
        Suggestion {
            value: name.to_string(),
            description: None,
            style: None,
            extra: None,
            span: Span::new(0, 0),
            append_whitespace: false,
        }
    }
}

impl Completer for SlashCompleter {
    fn complete(&mut self, line: &str, _pos: usize) -> Vec<Suggestion> {
        // We're consulted by ListMenu after the user typed `/`. Because the
        // menu is configured `only_buffer_difference: true`, `line` here is
        // the *post-`/` chars* (empty right after `/`, then "c", "co", …).
        // No leading `/` to strip; we filter command names directly.
        if line.contains(' ') {
            return vec![];
        }
        self.matches(line).into_iter().map(Self::make).collect()
    }

    fn partial_complete(
        &mut self,
        line: &str,
        _pos: usize,
        start: usize,
        offset: usize,
    ) -> Vec<Suggestion> {
        if line.contains(' ') {
            return vec![];
        }
        // True paged fetch: ListMenu asks for `[start, start+offset)` based on
        // the active page. We slice the filtered list — actual cost is trivial
        // for in-memory commands, but the *interface* matches what an
        // expensive remote completer would expose.
        self.matches(line)
            .into_iter()
            .skip(start)
            .take(offset)
            .map(Self::make)
            .collect()
    }

    fn total_completions(&mut self, line: &str, _pos: usize) -> usize {
        if line.contains(' ') {
            return 0;
        }
        self.matches(line).len()
    }
}

#[cfg(test)]
mod tests {
    //! Pure-logic tests for SlashCompleter. No reedline runtime — we
    //! construct directly and assert via the Completer trait methods.

    use super::*;

    fn registry() -> Vec<String> {
        // Realistic-shaped fixture; `model` overlaps with the
        // `extras` list inside `new()` to exercise the dedup branch.
        vec!["clear", "help", "model", "status"]
            .into_iter()
            .map(String::from)
            .collect()
    }

    #[test]
    fn new_merges_extras_dedups_and_sorts() {
        let c = SlashCompleter::new(registry());
        // `model` exists in both registry + extras — must appear ONCE
        let model_count = c.commands.iter().filter(|s| s == &"model").count();
        assert_eq!(model_count, 1, "duplicate `model` not dedup'd");
        // Sorted alphabetically — `auto` (from extras) before `clear`
        let auto_idx = c.commands.iter().position(|s| s == "auto").unwrap();
        let clear_idx = c.commands.iter().position(|s| s == "clear").unwrap();
        assert!(
            auto_idx < clear_idx,
            "commands should be sorted alphabetically"
        );
    }

    #[test]
    fn matches_empty_query_returns_all() {
        let c = SlashCompleter::new(registry());
        let m = c.matches("");
        assert_eq!(m.len(), c.commands.len(), "empty query matches all");
    }

    #[test]
    fn matches_prefix_filter_works() {
        let c = SlashCompleter::new(registry());
        // Prefix `c` should match `clear` (from registry)
        let m = c.matches("c");
        assert!(m.iter().any(|s| *s == "clear"), "missing `clear`: {m:?}");
        // Prefix `re` should match `resume`+`rewind` (extras) but not `model`
        let m = c.matches("re");
        assert!(m.iter().any(|s| *s == "resume"), "missing `resume`: {m:?}");
        assert!(m.iter().any(|s| *s == "rewind"), "missing `rewind`: {m:?}");
        assert!(
            !m.iter().any(|s| *s == "model"),
            "model should not match `re`"
        );
    }

    #[test]
    fn matches_no_match_returns_empty() {
        let c = SlashCompleter::new(registry());
        let m = c.matches("zzz-nope");
        assert!(m.is_empty(), "no-match should be empty: {m:?}");
    }

    #[test]
    fn complete_total_partial_are_consistent() {
        let mut c = SlashCompleter::new(registry());
        let complete = c.complete("re", 0);
        let total = c.total_completions("re", 0);
        let partial_all = c.partial_complete("re", 0, 0, 100);
        assert_eq!(complete.len(), total, "complete and total disagree");
        assert_eq!(
            partial_all.len(),
            total,
            "partial(0..100) and total disagree"
        );

        // partial_complete with start=1 should skip first match
        let partial_skip = c.partial_complete("re", 0, 1, 100);
        assert_eq!(
            partial_skip.len(),
            total.saturating_sub(1),
            "partial skip wrong"
        );
        // Returned suggestions carry the command name in `.value`
        for s in &complete {
            assert!(
                s.value.starts_with("re"),
                "value not re-prefixed: {}",
                s.value
            );
        }
    }

    #[test]
    fn space_in_input_short_circuits_all_methods() {
        // Once the user types a space, completion is over (arg to a
        // command, not a command name). All three Completer entry points
        // must short-circuit to empty / 0.
        let mut c = SlashCompleter::new(registry());
        assert!(c.complete("clear hi", 0).is_empty());
        assert_eq!(c.total_completions("clear hi", 0), 0);
        assert!(c.partial_complete("clear hi", 0, 0, 10).is_empty());
    }
}
