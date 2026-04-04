//! Minimal Vim-mode state machine.
//!
//! Supports normal/insert mode switching and basic motions in normal mode:
//! `h`/`l` (left/right), `w`/`b` (word forward/back), `0`/`$` (line start/end),
//! `x` (delete char), `dd` (clear line), `i`/`a`/`A`/`I` (enter insert), `u` (undo).

/// Vim editing mode.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VimState {
    /// Insert mode — keys go into the input buffer.
    Insert,
    /// Normal mode — keys are motions/commands.
    Normal,
}

/// Result of processing a key in Vim mode.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum VimAction {
    /// No effect — key was consumed but doesn't change anything visible.
    None,
    /// Switch to insert mode.
    EnterInsert,
    /// Switch to normal mode.
    EnterNormal,
    /// Move cursor to an absolute position.
    MoveTo(usize),
    /// Delete the character at the cursor.
    DeleteChar,
    /// Clear the entire line.
    ClearLine,
    /// Undo the last edit.
    Undo,
    /// Pass the key through to normal input handling (in insert mode).
    Passthrough,
}

/// Vim mode state machine.
#[derive(Debug, Clone)]
pub struct VimMode {
    state: VimState,
    enabled: bool,
    /// Pending operator (e.g., `d` waiting for second `d`).
    pending: Option<char>,
}

impl VimMode {
    pub fn new() -> Self {
        Self {
            state: VimState::Insert, // Start in insert mode
            enabled: false,
            pending: None,
        }
    }

    pub fn is_enabled(&self) -> bool {
        self.enabled
    }

    pub fn toggle(&mut self) {
        self.enabled = !self.enabled;
        if !self.enabled {
            self.state = VimState::Insert;
            self.pending = None;
        }
    }

    pub fn enable(&mut self) {
        self.enabled = true;
    }

    pub fn disable(&mut self) {
        self.enabled = false;
        self.state = VimState::Insert;
        self.pending = None;
    }

    pub fn state(&self) -> VimState {
        if self.enabled {
            self.state
        } else {
            VimState::Insert
        }
    }

    /// Process a key event. Returns the action to perform.
    ///
    /// `cursor`: current cursor position.
    /// `line_len`: length of the current input line.
    /// `ch`: the character pressed.
    pub fn process_key(&mut self, ch: char, cursor: usize, line: &str) -> VimAction {
        if !self.enabled {
            return VimAction::Passthrough;
        }

        match self.state {
            VimState::Insert => {
                if ch == '\x1b' {
                    // ESC → normal mode
                    self.state = VimState::Normal;
                    self.pending = None;
                    return VimAction::EnterNormal;
                }
                VimAction::Passthrough
            }
            VimState::Normal => self.process_normal(ch, cursor, line),
        }
    }

    fn process_normal(&mut self, ch: char, cursor: usize, line: &str) -> VimAction {
        let line_len = line.len();

        // Check for pending operator
        if let Some(op) = self.pending.take() {
            if op == 'd' && ch == 'd' {
                return VimAction::ClearLine;
            }
            // Unknown operator combo — ignore
            return VimAction::None;
        }

        match ch {
            // -- Mode switching --
            'i' => {
                self.state = VimState::Insert;
                VimAction::EnterInsert
            }
            'a' => {
                self.state = VimState::Insert;
                let pos = (cursor + 1).min(line_len);
                VimAction::MoveTo(pos) // then enter insert
            }
            'I' => {
                self.state = VimState::Insert;
                VimAction::MoveTo(0)
            }
            'A' => {
                self.state = VimState::Insert;
                VimAction::MoveTo(line_len)
            }

            // -- Movement --
            'h' => {
                if cursor > 0 {
                    VimAction::MoveTo(cursor - 1)
                } else {
                    VimAction::None
                }
            }
            'l' => {
                if cursor < line_len.saturating_sub(1) {
                    VimAction::MoveTo(cursor + 1)
                } else {
                    VimAction::None
                }
            }
            '0' => VimAction::MoveTo(0),
            '$' => VimAction::MoveTo(line_len.saturating_sub(1).max(0)),
            'w' => {
                // Move to start of next word
                let pos = next_word_start(line, cursor);
                VimAction::MoveTo(pos)
            }
            'b' => {
                // Move to start of previous word
                let pos = prev_word_start(line, cursor);
                VimAction::MoveTo(pos)
            }
            'e' => {
                // Move to end of current/next word
                let pos = word_end(line, cursor);
                VimAction::MoveTo(pos)
            }

            // -- Editing --
            'x' => VimAction::DeleteChar,
            'd' => {
                self.pending = Some('d');
                VimAction::None
            }
            'u' => VimAction::Undo,

            // -- Ignore other keys --
            _ => VimAction::None,
        }
    }
}

// ---------------------------------------------------------------------------
// Word motion helpers
// ---------------------------------------------------------------------------

fn next_word_start(line: &str, cursor: usize) -> usize {
    let bytes = line.as_bytes();
    let len = bytes.len();
    if cursor >= len {
        return cursor;
    }

    // Skip current word (non-whitespace)
    let mut pos = cursor;
    while pos < len && !bytes[pos].is_ascii_whitespace() {
        pos += 1;
    }
    // Skip whitespace
    while pos < len && bytes[pos].is_ascii_whitespace() {
        pos += 1;
    }
    pos.min(len)
}

fn prev_word_start(line: &str, cursor: usize) -> usize {
    let bytes = line.as_bytes();
    if cursor == 0 {
        return 0;
    }

    let mut pos = cursor - 1;
    // Skip whitespace backwards
    while pos > 0 && bytes[pos].is_ascii_whitespace() {
        pos -= 1;
    }
    // Skip word backwards
    while pos > 0 && !bytes[pos - 1].is_ascii_whitespace() {
        pos -= 1;
    }
    pos
}

fn word_end(line: &str, cursor: usize) -> usize {
    let bytes = line.as_bytes();
    let len = bytes.len();
    if cursor >= len.saturating_sub(1) {
        return cursor;
    }

    let mut pos = cursor + 1;
    // Skip whitespace
    while pos < len && bytes[pos].is_ascii_whitespace() {
        pos += 1;
    }
    // Move to end of word
    while pos < len && !bytes[pos].is_ascii_whitespace() {
        pos += 1;
    }
    (pos - 1).max(cursor)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_state_is_insert() {
        let vm = VimMode::new();
        assert!(!vm.is_enabled());
        assert_eq!(vm.state(), VimState::Insert);
    }

    #[test]
    fn passthrough_when_disabled() {
        let mut vm = VimMode::new();
        assert_eq!(vm.process_key('j', 0, "hello"), VimAction::Passthrough);
    }

    #[test]
    fn esc_enters_normal_mode() {
        let mut vm = VimMode::new();
        vm.enable();
        let action = vm.process_key('\x1b', 0, "hello");
        assert_eq!(action, VimAction::EnterNormal);
        assert_eq!(vm.state(), VimState::Normal);
    }

    #[test]
    fn i_enters_insert_mode() {
        let mut vm = VimMode::new();
        vm.enable();
        vm.process_key('\x1b', 0, "hello"); // go to normal
        let action = vm.process_key('i', 0, "hello");
        assert_eq!(action, VimAction::EnterInsert);
        assert_eq!(vm.state(), VimState::Insert);
    }

    #[test]
    fn h_moves_left() {
        let mut vm = VimMode::new();
        vm.enable();
        vm.process_key('\x1b', 3, "hello"); // normal mode
        let action = vm.process_key('h', 3, "hello");
        assert_eq!(action, VimAction::MoveTo(2));
    }

    #[test]
    fn h_at_start_does_nothing() {
        let mut vm = VimMode::new();
        vm.enable();
        vm.process_key('\x1b', 0, "hello");
        let action = vm.process_key('h', 0, "hello");
        assert_eq!(action, VimAction::None);
    }

    #[test]
    fn l_moves_right() {
        let mut vm = VimMode::new();
        vm.enable();
        vm.process_key('\x1b', 2, "hello");
        let action = vm.process_key('l', 2, "hello");
        assert_eq!(action, VimAction::MoveTo(3));
    }

    #[test]
    fn zero_goes_to_start() {
        let mut vm = VimMode::new();
        vm.enable();
        vm.process_key('\x1b', 3, "hello");
        let action = vm.process_key('0', 3, "hello");
        assert_eq!(action, VimAction::MoveTo(0));
    }

    #[test]
    fn dollar_goes_to_end() {
        let mut vm = VimMode::new();
        vm.enable();
        vm.process_key('\x1b', 0, "hello");
        let action = vm.process_key('$', 0, "hello");
        assert_eq!(action, VimAction::MoveTo(4)); // len-1
    }

    #[test]
    fn w_moves_to_next_word() {
        let mut vm = VimMode::new();
        vm.enable();
        vm.process_key('\x1b', 0, "hello world");
        let action = vm.process_key('w', 0, "hello world");
        assert_eq!(action, VimAction::MoveTo(6)); // 'w' of "world"
    }

    #[test]
    fn b_moves_to_prev_word() {
        let mut vm = VimMode::new();
        vm.enable();
        vm.process_key('\x1b', 6, "hello world");
        let action = vm.process_key('b', 6, "hello world");
        assert_eq!(action, VimAction::MoveTo(0)); // 'h' of "hello"
    }

    #[test]
    fn x_deletes_char() {
        let mut vm = VimMode::new();
        vm.enable();
        vm.process_key('\x1b', 0, "hello");
        let action = vm.process_key('x', 0, "hello");
        assert_eq!(action, VimAction::DeleteChar);
    }

    #[test]
    fn dd_clears_line() {
        let mut vm = VimMode::new();
        vm.enable();
        vm.process_key('\x1b', 0, "hello");
        let action1 = vm.process_key('d', 0, "hello");
        assert_eq!(action1, VimAction::None); // pending
        let action2 = vm.process_key('d', 0, "hello");
        assert_eq!(action2, VimAction::ClearLine);
    }

    #[test]
    fn u_undoes() {
        let mut vm = VimMode::new();
        vm.enable();
        vm.process_key('\x1b', 0, "hello");
        let action = vm.process_key('u', 0, "hello");
        assert_eq!(action, VimAction::Undo);
    }

    #[test]
    fn a_enters_insert_after_cursor() {
        let mut vm = VimMode::new();
        vm.enable();
        vm.process_key('\x1b', 2, "hello");
        let action = vm.process_key('a', 2, "hello");
        assert_eq!(action, VimAction::MoveTo(3));
        assert_eq!(vm.state(), VimState::Insert);
    }

    #[test]
    fn big_a_enters_insert_at_end() {
        let mut vm = VimMode::new();
        vm.enable();
        vm.process_key('\x1b', 0, "hello");
        let action = vm.process_key('A', 0, "hello");
        assert_eq!(action, VimAction::MoveTo(5));
        assert_eq!(vm.state(), VimState::Insert);
    }

    #[test]
    fn big_i_enters_insert_at_start() {
        let mut vm = VimMode::new();
        vm.enable();
        vm.process_key('\x1b', 3, "hello");
        let action = vm.process_key('I', 3, "hello");
        assert_eq!(action, VimAction::MoveTo(0));
        assert_eq!(vm.state(), VimState::Insert);
    }

    #[test]
    fn toggle_enables_and_disables() {
        let mut vm = VimMode::new();
        assert!(!vm.is_enabled());
        vm.toggle();
        assert!(vm.is_enabled());
        vm.toggle();
        assert!(!vm.is_enabled());
        assert_eq!(vm.state(), VimState::Insert); // reset to insert
    }

    // -- Word motion helpers --

    #[test]
    fn next_word_start_basic() {
        assert_eq!(next_word_start("hello world", 0), 6);
        assert_eq!(next_word_start("hello world", 6), 11);
    }

    #[test]
    fn prev_word_start_basic() {
        assert_eq!(prev_word_start("hello world", 6), 0);
        assert_eq!(prev_word_start("hello world", 11), 6);
    }

    #[test]
    fn word_end_basic() {
        assert_eq!(word_end("hello world", 0), 4);
        assert_eq!(word_end("hello world", 6), 10);
    }

    #[test]
    fn next_word_at_end() {
        assert_eq!(next_word_start("hello", 5), 5);
    }

    #[test]
    fn prev_word_at_start() {
        assert_eq!(prev_word_start("hello", 0), 0);
    }
}
