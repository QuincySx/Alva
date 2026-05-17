// INPUT:  crossterm::Event, ratatui (Frame, Rect, Block, Borders),
//         tui_textarea::TextArea, super::{Component, ComponentAction, theme}
// OUTPUT: TextField
// POS:    Single-line text input wrapping `tui-textarea`. Inherits its
//         emacs/vim keybindings, undo/redo, selection, masking. We keep
//         a thin facade so callers stay on the Component trait.

use crossterm::event::{Event, KeyCode, KeyEvent};
use ratatui::layout::Rect;
use ratatui::style::Style;
use ratatui::widgets::{Block, Borders};
use ratatui::Frame;
use tui_textarea::{CursorMove, TextArea};

use super::super::theme::Theme;
use super::{Component, ComponentAction};

/// Single-line editable text. Owns a `tui_textarea::TextArea` configured
/// for one line; we forward all editing keys to it via `input(event)`.
/// Callers get back `Submit(value)` on Enter, `Dismiss` on Esc, and
/// `Changed` on every other consumed event so live filters / previews
/// can update.
pub struct TextField {
    inner: TextArea<'static>,
    label: String,
    secret: bool,
}

impl TextField {
    pub fn new(label: impl Into<String>) -> Self {
        let mut inner = TextArea::default();
        inner.set_block(
            Block::default()
                .borders(Borders::ALL)
                .title(format!(" {} ", label.into())),
        );
        // Single-line: we explicitly drop newline events in handle_event
        // (Enter -> Submit), and tui-textarea ignores newlines if the
        // underlying lines() vector stays at len 1.
        Self {
            inner,
            label: String::new(),  // (held inside the Block above)
            secret: false,
        }
    }

    /// Set placeholder shown when the buffer is empty.
    pub fn placeholder(mut self, s: impl Into<String>) -> Self {
        self.inner.set_placeholder_text(s.into());
        self
    }

    /// Render each char as a bullet (•) — for API key entry, etc.
    pub fn secret(mut self, on: bool) -> Self {
        self.secret = on;
        self.inner.set_mask_char(if on { '•' } else { '\0' });
        self
    }

    /// Replace the current value (resets cursor to end).
    pub fn set_value(&mut self, v: impl Into<String>) {
        let v: String = v.into();
        // tui-textarea exposes a single-line constructor via from_iter.
        let lines: Vec<String> = v.lines().take(1).map(|s| s.to_string()).collect();
        let mut new = TextArea::new(if lines.is_empty() { vec![String::new()] } else { lines });
        new.set_block(self.inner.block().cloned().unwrap_or_default());
        new.set_placeholder_text(self.inner.placeholder_text().to_string());
        if self.secret { new.set_mask_char('•'); }
        new.move_cursor(CursorMove::End);
        self.inner = new;
    }

    pub fn value(&self) -> String {
        self.inner.lines().first().cloned().unwrap_or_default()
    }

    pub fn clear(&mut self) {
        self.inner.select_all();
        self.inner.cut();
    }

    /// Update the title shown in the surrounding block.
    pub fn set_label(&mut self, label: impl Into<String>) {
        let l = label.into();
        self.label = l.clone();
        self.inner.set_block(
            Block::default().borders(Borders::ALL).title(format!(" {} ", l)),
        );
    }
}

impl Component for TextField {
    fn render(&self, frame: &mut Frame<'_>, area: Rect, theme: &Theme) {
        // We can't mutate `self` here, so re-clone the textarea state into
        // a local mut for ratatui's stateful render. This is cheap (it just
        // shares lines via Cow internally for tui-textarea 0.7).
        let mut ta = self.inner.clone();
        // Restyle each render so theme changes take effect immediately.
        if let Some(block) = ta.block().cloned() {
            ta.set_block(block.border_style(theme.border));
        }
        ta.set_style(theme.text);
        ta.set_cursor_style(Style::default().add_modifier(ratatui::style::Modifier::REVERSED));
        ta.set_placeholder_style(theme.text_dim);
        frame.render_widget(&ta, area);
    }

    fn handle_event(&mut self, event: Event) -> ComponentAction {
        let Event::Key(KeyEvent { code, .. }) = event.clone() else {
            return ComponentAction::Bubble(event);
        };
        match code {
            KeyCode::Enter => ComponentAction::Submit(self.value()),
            KeyCode::Esc => ComponentAction::Dismiss,
            // Forward everything else (chars, backspace, arrows, ctrl-a/e/u/k,
            // ctrl-z undo, etc.) to tui-textarea — it handles all editing.
            _ => {
                if self.inner.input(event) {
                    ComponentAction::Changed
                } else {
                    ComponentAction::None
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    //! Unit tests for the TextField facade. The underlying editing
    //! engine (`tui_textarea::TextArea`) has its own tests upstream, so
    //! we only cover this thin layer's contract: Enter→Submit, Esc→
    //! Dismiss, other keys forwarded with Changed/None, set_value's
    //! first-line-only truncation, and clear().
    use super::*;
    use crossterm::event::{
        KeyEvent, KeyEventKind, KeyEventState, KeyModifiers, MouseButton, MouseEvent,
        MouseEventKind,
    };

    fn key(code: KeyCode) -> Event {
        Event::Key(KeyEvent {
            code,
            modifiers: KeyModifiers::NONE,
            kind: KeyEventKind::Press,
            state: KeyEventState::NONE,
        })
    }

    #[test]
    fn new_starts_with_empty_value() {
        let f = TextField::new("name");
        assert_eq!(f.value(), "", "fresh TextField must have empty value");
    }

    #[test]
    fn set_value_and_value_roundtrip() {
        let mut f = TextField::new("name");
        f.set_value("hello world");
        assert_eq!(f.value(), "hello world");

        // Overwrite is total: old buffer must be replaced, not appended
        f.set_value("xyz");
        assert_eq!(f.value(), "xyz", "set_value must replace, not append");
    }

    #[test]
    fn set_value_truncates_multi_line_input_to_first_line() {
        // Pasting multi-line text (or whatever the caller hands in) must
        // collapse to the first line — this is the implicit contract that
        // keeps the single-line invariant of TextField. Without this test,
        // a regression would silently lose lines 2+.
        let mut f = TextField::new("name");
        f.set_value("line-one\nline-two\nline-three");
        assert_eq!(f.value(), "line-one", "multi-line input must truncate");
    }

    #[test]
    fn set_value_empty_string_yields_empty_value() {
        // The fallback `vec![String::new()]` branch in set_value must keep
        // the buffer in a valid (single-empty-line) state — value() should
        // not panic and must return "".
        let mut f = TextField::new("name");
        f.set_value("first");
        f.set_value("");
        assert_eq!(f.value(), "", "empty set_value must yield empty value");
    }

    #[test]
    fn clear_empties_the_buffer() {
        let mut f = TextField::new("name");
        f.set_value("something");
        f.clear();
        assert_eq!(f.value(), "", "clear() must empty buffer");
    }

    #[test]
    fn enter_returns_submit_with_current_value() {
        let mut f = TextField::new("name");
        f.set_value("payload");
        match f.handle_event(key(KeyCode::Enter)) {
            ComponentAction::Submit(v) => assert_eq!(v, "payload"),
            other => panic!("expected Submit, got {other:?}"),
        }
        // Submitting must not clear the buffer — caller decides what to do
        assert_eq!(f.value(), "payload", "Submit must not mutate buffer");
    }

    #[test]
    fn esc_returns_dismiss_without_changing_value() {
        let mut f = TextField::new("name");
        f.set_value("draft");
        match f.handle_event(key(KeyCode::Esc)) {
            ComponentAction::Dismiss => {}
            other => panic!("expected Dismiss, got {other:?}"),
        }
        assert_eq!(f.value(), "draft", "Esc must not mutate value");
    }

    #[test]
    fn char_input_appends_and_returns_changed() {
        let mut f = TextField::new("name");
        match f.handle_event(key(KeyCode::Char('a'))) {
            ComponentAction::Changed => {}
            other => panic!("expected Changed on 'a', got {other:?}"),
        }
        match f.handle_event(key(KeyCode::Char('b'))) {
            ComponentAction::Changed => {}
            other => panic!("expected Changed on 'b', got {other:?}"),
        }
        assert_eq!(f.value(), "ab", "consecutive chars must concat");
    }

    #[test]
    fn backspace_removes_last_char() {
        let mut f = TextField::new("name");
        f.set_value("abc");
        // tui-textarea backspace moves cursor (already at end after set_value)
        // and removes the char before it
        let action = f.handle_event(key(KeyCode::Backspace));
        // Backspace at non-empty buffer must mutate → Changed
        assert!(
            matches!(action, ComponentAction::Changed),
            "expected Changed on backspace, got {action:?}"
        );
        assert_eq!(f.value(), "ab", "backspace must remove last char");
    }

    #[test]
    fn non_key_event_bubbles_unchanged() {
        let mut f = TextField::new("name");
        let mouse_ev = Event::Mouse(MouseEvent {
            kind: MouseEventKind::Down(MouseButton::Left),
            column: 0,
            row: 0,
            modifiers: KeyModifiers::NONE,
        });
        match f.handle_event(mouse_ev) {
            ComponentAction::Bubble(Event::Mouse(_)) => {}
            other => panic!("expected Bubble(Mouse), got {other:?}"),
        }
    }
}
