// INPUT:  crossterm::Event, ratatui (Frame, Rect, Block, Borders),
//         tui_textarea::TextArea, super::theme
// OUTPUT: ChatInput, ChatInputAction
// POS:    Multi-line editor at the bottom of the chat screen. Wraps
//         `tui-textarea` for the editing surface and watches the cursor
//         word for `/` (command palette) and `@` (file picker) triggers
//         so the parent can pop those over without taking focus away.

use crossterm::event::{Event, KeyCode, KeyEvent, KeyModifiers};
use ratatui::layout::Rect;
use ratatui::widgets::{Block, Borders};
use ratatui::Frame;
use tui_textarea::TextArea;

use super::super::theme::Theme;

/// What ChatInput tells its parent after an event. `Submit` carries the
/// full message + a copy of any attachments the parent has been tracking.
/// `SlashTrigger` / `AtTrigger` carry the partial token (without the
/// leading char) so the parent can pre-filter its picker.
#[derive(Debug, Clone)]
pub enum ChatInputAction {
    None,
    Changed,
    Submit(String),
    SlashTrigger(String),
    AtTrigger(String),
    Cancel,
}

/// Multi-line chat input. Submit on Enter (Shift+Enter for newline),
/// Ctrl+C to cancel. While typing, if the cursor word starts with `/`
/// or `@`, ChatInput emits the matching trigger every keystroke so the
/// parent's command-palette / file-picker filters live.
pub struct ChatInput {
    inner: TextArea<'static>,
    placeholder: String,
}

impl ChatInput {
    pub fn new(placeholder: impl Into<String>) -> Self {
        let mut inner = TextArea::default();
        let placeholder: String = placeholder.into();
        inner.set_placeholder_text(&placeholder);
        inner.set_block(
            Block::default()
                .borders(Borders::ALL)
                .title(" Message "),
        );
        Self { inner, placeholder }
    }

    /// Replace the entire buffer (e.g. after a command-palette accept
    /// rewrites the slash word).
    pub fn set_value(&mut self, v: impl Into<String>) {
        let v: String = v.into();
        let lines: Vec<String> = if v.is_empty() {
            vec![String::new()]
        } else {
            v.lines().map(|s| s.to_string()).collect()
        };
        let mut new = TextArea::new(lines);
        new.set_block(self.inner.block().cloned().unwrap_or_default());
        new.set_placeholder_text(&self.placeholder);
        self.inner = new;
    }

    pub fn value(&self) -> String {
        self.inner.lines().join("\n")
    }

    pub fn clear(&mut self) {
        self.set_value("");
    }

    /// Inspect the word the cursor is currently inside. Returns `None`
    /// if the cursor is not in a word, otherwise the bare token (no
    /// leading `/` or `@`) and which trigger char it started with.
    fn cursor_token(&self) -> Option<(char, String)> {
        let (row, col) = self.inner.cursor();
        let line = self.inner.lines().get(row)?.as_str();
        // Walk back from the cursor to the previous space or line start.
        let mut start = col.min(line.len());
        let bytes = line.as_bytes();
        while start > 0 {
            let prev = bytes[start - 1];
            if prev == b' ' || prev == b'\t' { break; }
            start -= 1;
        }
        let head_byte = bytes.get(start).copied()?;
        if head_byte != b'/' && head_byte != b'@' { return None; }
        let trigger = head_byte as char;
        let end = col.min(line.len());
        let token = line.get(start + 1..end).unwrap_or("").to_string();
        Some((trigger, token))
    }

    pub fn render(&self, frame: &mut Frame<'_>, area: Rect, theme: &Theme) {
        let mut ta = self.inner.clone();
        if let Some(block) = ta.block().cloned() {
            ta.set_block(block.border_style(theme.border));
        }
        ta.set_style(theme.text);
        ta.set_cursor_style(
            ratatui::style::Style::default().add_modifier(ratatui::style::Modifier::REVERSED),
        );
        ta.set_placeholder_style(theme.text_dim);
        frame.render_widget(&ta, area);
    }

    /// Insert raw text at the cursor (used by the parent's paste fallback).
    /// Multi-line pastes are passed through verbatim.
    pub fn insert_text(&mut self, text: &str) {
        for (i, line) in text.split('\n').enumerate() {
            if i > 0 { self.inner.insert_newline(); }
            self.inner.insert_str(line);
        }
    }

    pub fn handle_event(&mut self, event: Event) -> ChatInputAction {
        // Forward bracketed-paste chunks to the textarea (multi-line aware).
        if let Event::Paste(s) = &event {
            self.insert_text(s);
            return ChatInputAction::Changed;
        }
        let Event::Key(KeyEvent { code, modifiers, .. }) = event.clone() else {
            return ChatInputAction::None;
        };
        match (modifiers, code) {
            // Submit: Enter alone (Shift+Enter inserts newline)
            (m, KeyCode::Enter) if !m.contains(KeyModifiers::SHIFT) => {
                let v = self.value();
                if v.trim().is_empty() { return ChatInputAction::None; }
                ChatInputAction::Submit(v)
            }
            (KeyModifiers::CONTROL, KeyCode::Char('c')) => ChatInputAction::Cancel,
            _ => {
                // Forward to tui-textarea (handles editing, navigation,
                // selection, undo/redo, Shift+Enter newline, etc.)
                let consumed = self.inner.input(event);
                // After every consumed edit, emit the right trigger so the
                // parent's palette / file picker stays in sync with the
                // word at the cursor.
                if consumed {
                    if let Some((c, token)) = self.cursor_token() {
                        match c {
                            '/' => return ChatInputAction::SlashTrigger(token),
                            '@' => return ChatInputAction::AtTrigger(token),
                            _ => {}
                        }
                    }
                    ChatInputAction::Changed
                } else {
                    ChatInputAction::None
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    //! Tests for ChatInput value/insert + handle_event routing +
    //! slash/@ trigger emission. render() needs a Frame so is not
    //! exercised; the routing + trigger logic is the route every
    //! keystroke actually takes.
    use super::*;
    use crossterm::event::{KeyEventKind, KeyEventState};

    fn key(code: KeyCode, modifiers: KeyModifiers) -> Event {
        Event::Key(KeyEvent {
            code,
            modifiers,
            kind: KeyEventKind::Press,
            state: KeyEventState::empty(),
        })
    }

    fn ch(c: char) -> Event {
        key(KeyCode::Char(c), KeyModifiers::NONE)
    }

    // -- value / set_value / clear ----------------------------------------

    #[test]
    fn new_starts_empty() {
        let ci = ChatInput::new("type here");
        assert_eq!(ci.value(), "");
    }

    #[test]
    fn set_value_then_value_round_trips() {
        let mut ci = ChatInput::new("ph");
        ci.set_value("hello world");
        assert_eq!(ci.value(), "hello world");
    }

    #[test]
    fn set_value_preserves_multiline() {
        // Pin: lines split + rejoin must preserve the newline structure
        // (set_value uses v.lines() then value() joins with "\n").
        let mut ci = ChatInput::new("ph");
        ci.set_value("first\nsecond\nthird");
        assert_eq!(ci.value(), "first\nsecond\nthird");
    }

    #[test]
    fn set_value_empty_string_is_normalized_to_single_empty_line() {
        // Pin: set_value("") still leaves a usable buffer (one empty
        // line, NOT zero lines — TextArea::new(vec![]) would panic).
        let mut ci = ChatInput::new("ph");
        ci.set_value("");
        assert_eq!(ci.value(), "");
        // Smoke that it's still usable: setting a real value afterwards.
        ci.set_value("recovered");
        assert_eq!(ci.value(), "recovered");
    }

    #[test]
    fn clear_resets_value_to_empty() {
        let mut ci = ChatInput::new("ph");
        ci.set_value("anything");
        ci.clear();
        assert_eq!(ci.value(), "");
    }

    // -- insert_text -------------------------------------------------------

    #[test]
    fn insert_text_single_line_appends_at_cursor() {
        let mut ci = ChatInput::new("ph");
        ci.insert_text("hello");
        assert_eq!(ci.value(), "hello");
    }

    #[test]
    fn insert_text_multiline_inserts_newlines() {
        let mut ci = ChatInput::new("ph");
        ci.insert_text("a\nb\nc");
        assert_eq!(ci.value(), "a\nb\nc");
    }

    // -- handle_event: Enter + Ctrl+C -------------------------------------

    #[test]
    fn enter_on_empty_buffer_returns_none() {
        // Pin: empty (and whitespace-only) submits must NOT fire. The
        // guard is `if v.trim().is_empty()`.
        let mut ci = ChatInput::new("ph");
        let act = ci.handle_event(key(KeyCode::Enter, KeyModifiers::NONE));
        assert!(matches!(act, ChatInputAction::None));
    }

    #[test]
    fn enter_on_whitespace_only_buffer_returns_none() {
        let mut ci = ChatInput::new("ph");
        ci.set_value("   \t  ");
        let act = ci.handle_event(key(KeyCode::Enter, KeyModifiers::NONE));
        assert!(matches!(act, ChatInputAction::None));
    }

    #[test]
    fn enter_on_nonempty_buffer_returns_submit_with_value() {
        let mut ci = ChatInput::new("ph");
        ci.set_value("ship it");
        let act = ci.handle_event(key(KeyCode::Enter, KeyModifiers::NONE));
        match act {
            ChatInputAction::Submit(v) => assert_eq!(v, "ship it"),
            other => panic!("expected Submit, got {other:?}"),
        }
    }

    #[test]
    fn shift_enter_does_not_submit() {
        // Pin: Shift+Enter is the "insert newline" affordance; must
        // fall through to the editor, NOT trigger Submit. Without the
        // `!m.contains(SHIFT)` guard, users couldn't compose multiline
        // messages.
        let mut ci = ChatInput::new("ph");
        ci.set_value("line one");
        let act = ci.handle_event(key(KeyCode::Enter, KeyModifiers::SHIFT));
        assert!(
            !matches!(act, ChatInputAction::Submit(_)),
            "Shift+Enter must NOT submit, got {act:?}"
        );
    }

    #[test]
    fn ctrl_c_returns_cancel() {
        let mut ci = ChatInput::new("ph");
        let act = ci.handle_event(key(KeyCode::Char('c'), KeyModifiers::CONTROL));
        assert!(matches!(act, ChatInputAction::Cancel));
    }

    // -- slash / @ trigger emission ---------------------------------------

    #[test]
    fn typing_slash_emits_slash_trigger_with_empty_token() {
        // After typing a single '/' the cursor is right after it,
        // so the token portion is "" — the parent uses this to open
        // an empty-filter command palette.
        let mut ci = ChatInput::new("ph");
        let act = ci.handle_event(ch('/'));
        match act {
            ChatInputAction::SlashTrigger(token) => assert_eq!(token, ""),
            other => panic!("expected SlashTrigger, got {other:?}"),
        }
    }

    #[test]
    fn typing_slash_then_letters_emits_slash_trigger_with_growing_token() {
        // Live-filter contract: every keystroke after '/' must re-emit
        // with the current partial token so the palette stays in sync.
        let mut ci = ChatInput::new("ph");
        ci.handle_event(ch('/'));
        ci.handle_event(ch('h'));
        ci.handle_event(ch('e'));
        let act = ci.handle_event(ch('l'));
        match act {
            ChatInputAction::SlashTrigger(token) => assert_eq!(token, "hel"),
            other => panic!("expected SlashTrigger('hel'), got {other:?}"),
        }
    }

    #[test]
    fn typing_at_emits_at_trigger() {
        let mut ci = ChatInput::new("ph");
        ci.handle_event(ch('@'));
        let act = ci.handle_event(ch('s'));
        match act {
            ChatInputAction::AtTrigger(token) => assert_eq!(token, "s"),
            other => panic!("expected AtTrigger, got {other:?}"),
        }
    }

    #[test]
    fn typing_plain_text_emits_changed_not_trigger() {
        // No leading '/' or '@' → just `Changed`. Pin so the cursor_token
        // heuristic doesn't accidentally fire on plain words.
        let mut ci = ChatInput::new("ph");
        let act = ci.handle_event(ch('h'));
        assert!(matches!(act, ChatInputAction::Changed));
    }
}
