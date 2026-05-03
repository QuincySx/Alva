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
            "quit", "resume", "fork", "rewind", "sessions", "setup",
            "auto", "locks", "model",
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
        if line.contains(' ') { return vec![]; }
        self.matches(line).into_iter().map(Self::make).collect()
    }

    fn partial_complete(
        &mut self,
        line: &str,
        _pos: usize,
        start: usize,
        offset: usize,
    ) -> Vec<Suggestion> {
        if line.contains(' ') { return vec![]; }
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
        if line.contains(' ') { return 0; }
        self.matches(line).len()
    }
}
