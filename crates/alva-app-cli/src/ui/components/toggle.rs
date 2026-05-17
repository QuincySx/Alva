// INPUT:  crossterm::Event, ratatui (Frame, Rect, Paragraph, Block, Borders),
//         super::{Component, ComponentAction, theme}
// OUTPUT: Toggle
// POS:    Bool switch widget — Space / Enter flips, ←/→ also work. Used in
//         settings forms wherever a boolean knob is needed (auto-shell,
//         plan-mode, vim-mode, ...). Renders as `[ ] off` / `[x] on`.

use crossterm::event::{Event, KeyCode, KeyEvent};
use ratatui::layout::Rect;
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Paragraph};
use ratatui::Frame;

use super::super::theme::Theme;
use super::{Component, ComponentAction};

pub struct Toggle {
    label: String,
    value: bool,
    bordered: bool,
}

impl Toggle {
    pub fn new(label: impl Into<String>, value: bool) -> Self {
        Self { label: label.into(), value, bordered: false }
    }

    pub fn bordered(mut self, on: bool) -> Self {
        self.bordered = on;
        self
    }

    pub fn value(&self) -> bool { self.value }
    pub fn set(&mut self, v: bool) { self.value = v; }
}

impl Component for Toggle {
    fn render(&self, frame: &mut Frame<'_>, area: Rect, theme: &Theme) {
        let mark = if self.value { "[x]" } else { "[ ]" };
        let line = Line::from(vec![
            Span::styled(format!("{} ", mark), theme.text),
            Span::styled(self.label.clone(), theme.text),
            Span::styled(
                if self.value { "  on" } else { "  off" }.to_string(),
                theme.text_dim,
            ),
        ]);
        let p = if self.bordered {
            Paragraph::new(line).block(
                Block::default().borders(Borders::ALL).border_style(theme.border),
            )
        } else {
            Paragraph::new(line)
        };
        frame.render_widget(p, area);
    }

    fn handle_event(&mut self, event: Event) -> ComponentAction {
        let Event::Key(KeyEvent { code, .. }) = event.clone() else {
            return ComponentAction::Bubble(event);
        };
        match code {
            KeyCode::Char(' ') | KeyCode::Enter | KeyCode::Char('y') => {
                self.value = !self.value;
                ComponentAction::Changed
            }
            KeyCode::Left => { if self.value { self.value = false; } ComponentAction::Changed }
            KeyCode::Right => { if !self.value { self.value = true; } ComponentAction::Changed }
            KeyCode::Esc => ComponentAction::Dismiss,
            _ => ComponentAction::Bubble(event),
        }
    }
}

#[cfg(test)]
mod tests {
    //! Unit tests for the Toggle state machine. We exercise
    //! `handle_event` directly — no ratatui Frame needed, so no test
    //! backend setup. Each test asserts the returned action variant
    //! AND the post-event value, since callers read value via `value()`.
    use super::*;
    use crossterm::event::{KeyCode, KeyEvent, KeyEventKind, KeyEventState, KeyModifiers, MouseButton, MouseEvent, MouseEventKind};

    fn key(code: KeyCode) -> Event {
        Event::Key(KeyEvent {
            code,
            modifiers: KeyModifiers::NONE,
            kind: KeyEventKind::Press,
            state: KeyEventState::NONE,
        })
    }

    fn assert_changed(action: ComponentAction) {
        assert!(
            matches!(action, ComponentAction::Changed),
            "expected ComponentAction::Changed, got {action:?}",
        );
    }

    #[test]
    fn new_preserves_initial_value_and_label() {
        let on = Toggle::new("plan-mode", true);
        assert!(on.value(), "explicit true must be preserved");

        let off = Toggle::new("vim-mode", false);
        assert!(!off.value(), "explicit false must be preserved");
    }

    #[test]
    fn space_toggles_value() {
        let mut t = Toggle::new("x", false);
        let action = t.handle_event(key(KeyCode::Char(' ')));
        assert_changed(action);
        assert!(t.value(), "space on false → true");

        // Idempotent toggle: second space flips back
        let action = t.handle_event(key(KeyCode::Char(' ')));
        assert_changed(action);
        assert!(!t.value(), "space on true → false");
    }

    #[test]
    fn enter_and_y_are_aliases_for_space() {
        let mut t = Toggle::new("x", false);
        assert_changed(t.handle_event(key(KeyCode::Enter)));
        assert!(t.value(), "Enter must flip false→true");

        assert_changed(t.handle_event(key(KeyCode::Char('y'))));
        assert!(!t.value(), "y must flip true→false");
    }

    #[test]
    fn left_arrow_forces_off_and_is_idempotent_when_already_off() {
        // From ON → ←: forces OFF
        let mut t = Toggle::new("x", true);
        assert_changed(t.handle_event(key(KeyCode::Left)));
        assert!(!t.value(), "Left must force off from on");

        // From OFF → ←: stays OFF but still returns Changed
        // (this is the documented "force one-way" semantic — caller
        // should treat the keypress as acknowledged even if value
        // didn't move)
        assert_changed(t.handle_event(key(KeyCode::Left)));
        assert!(!t.value(), "Left on already-off must stay off");
    }

    #[test]
    fn right_arrow_forces_on_and_is_idempotent_when_already_on() {
        let mut t = Toggle::new("x", false);
        assert_changed(t.handle_event(key(KeyCode::Right)));
        assert!(t.value(), "Right must force on from off");

        assert_changed(t.handle_event(key(KeyCode::Right)));
        assert!(t.value(), "Right on already-on must stay on");
    }

    #[test]
    fn esc_returns_dismiss_without_changing_value() {
        let mut t = Toggle::new("x", true);
        let action = t.handle_event(key(KeyCode::Esc));
        assert!(
            matches!(action, ComponentAction::Dismiss),
            "expected Dismiss, got {action:?}"
        );
        assert!(t.value(), "Esc must not mutate value");
    }

    #[test]
    fn unknown_key_bubbles_event_unmodified() {
        let mut t = Toggle::new("x", false);
        let ev = key(KeyCode::Tab);
        let action = t.handle_event(ev.clone());
        match action {
            ComponentAction::Bubble(bubbled) => {
                // Bubbled event must be the same event we passed in
                // (verbatim re-emission, not a synthesized one)
                assert!(
                    matches!(bubbled, Event::Key(KeyEvent { code: KeyCode::Tab, .. })),
                    "bubbled event lost identity: {bubbled:?}"
                );
            }
            other => panic!("expected Bubble, got {other:?}"),
        }
        assert!(!t.value(), "unknown key must not mutate value");
    }

    #[test]
    fn non_key_event_bubbles_unchanged() {
        let mut t = Toggle::new("x", false);
        let mouse_ev = Event::Mouse(MouseEvent {
            kind: MouseEventKind::Down(MouseButton::Left),
            column: 0,
            row: 0,
            modifiers: KeyModifiers::NONE,
        });
        let action = t.handle_event(mouse_ev.clone());
        assert!(
            matches!(action, ComponentAction::Bubble(Event::Mouse(_))),
            "non-key event must bubble: {action:?}"
        );
        assert!(!t.value(), "non-key event must not mutate value");
    }

    #[test]
    fn set_overrides_value_imperatively() {
        let mut t = Toggle::new("x", false);
        t.set(true);
        assert!(t.value(), "set(true) must take effect");
        t.set(false);
        assert!(!t.value(), "set(false) must take effect");
    }
}
