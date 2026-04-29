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
