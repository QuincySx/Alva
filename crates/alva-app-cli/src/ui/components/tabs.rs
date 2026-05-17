// INPUT:  crossterm::Event, ratatui (Frame, Rect, Tabs as RatatuiTabs),
//         super::{Component, ComponentAction, theme}
// OUTPUT: Tabs (component wrapper around ratatui's Tabs widget)
// POS:    Horizontal tab strip with keyboard navigation. Settings UI uses
//         this for the "Models / Agents / Display / Hooks" rail. The body
//         (rendering of the active tab's content) is the parent's job —
//         Tabs only owns the strip + which tab is active.

use crossterm::event::{Event, KeyCode, KeyEvent, KeyModifiers};
use ratatui::layout::Rect;
use ratatui::style::{Modifier, Style};
use ratatui::text::Line;
use ratatui::widgets::{Block, Borders, Tabs as RatatuiTabs};
use ratatui::Frame;

use super::super::theme::Theme;
use super::{Component, ComponentAction};

pub struct Tabs {
    pub titles: Vec<String>,
    pub active: usize,
    pub bordered: bool,
}

impl Tabs {
    pub fn new(titles: Vec<impl Into<String>>) -> Self {
        Self {
            titles: titles.into_iter().map(Into::into).collect(),
            active: 0,
            bordered: false,
        }
    }

    pub fn bordered(mut self, on: bool) -> Self {
        self.bordered = on;
        self
    }

    pub fn next(&mut self) {
        if self.titles.is_empty() { return; }
        self.active = (self.active + 1) % self.titles.len();
    }

    pub fn prev(&mut self) {
        if self.titles.is_empty() { return; }
        self.active = if self.active == 0 { self.titles.len() - 1 } else { self.active - 1 };
    }

    pub fn set_active(&mut self, i: usize) {
        if i < self.titles.len() { self.active = i; }
    }
}

impl Component for Tabs {
    fn render(&self, frame: &mut Frame<'_>, area: Rect, theme: &Theme) {
        let titles: Vec<Line> = self.titles.iter()
            .map(|t| Line::from(t.clone()))
            .collect();
        let mut tabs = RatatuiTabs::new(titles)
            .style(theme.text_dim)
            .highlight_style(Style::default().add_modifier(Modifier::BOLD).fg(theme.text.fg.unwrap_or_default()))
            .select(self.active)
            .divider("│");
        if self.bordered {
            tabs = tabs.block(Block::default().borders(Borders::BOTTOM).border_style(theme.border));
        }
        frame.render_widget(tabs, area);
    }

    fn handle_event(&mut self, event: Event) -> ComponentAction {
        let Event::Key(KeyEvent { code, modifiers, .. }) = event.clone() else {
            return ComponentAction::Bubble(event);
        };
        match (modifiers, code) {
            (KeyModifiers::CONTROL, KeyCode::Tab) | (_, KeyCode::Right) => {
                self.next(); ComponentAction::Changed
            }
            (KeyModifiers::CONTROL | KeyModifiers::SHIFT, KeyCode::BackTab)
            | (KeyModifiers::SHIFT, KeyCode::Tab)
            | (_, KeyCode::Left) => {
                self.prev(); ComponentAction::Changed
            }
            _ => ComponentAction::Bubble(event),
        }
    }
}

#[cfg(test)]
mod tests {
    //! Unit tests for the Tabs state machine. We exercise next/prev
    //! wrap-around, empty-vec safety, set_active bounds, and the four
    //! key bindings that drive Settings-style tabbed navigation. No
    //! ratatui Frame needed — render() is a pure widget passthrough.
    use super::*;
    use crossterm::event::{
        KeyEvent, KeyEventKind, KeyEventState, MouseButton, MouseEvent, MouseEventKind,
    };

    fn key_mod(code: KeyCode, modifiers: KeyModifiers) -> Event {
        Event::Key(KeyEvent {
            code,
            modifiers,
            kind: KeyEventKind::Press,
            state: KeyEventState::NONE,
        })
    }
    fn key(code: KeyCode) -> Event {
        key_mod(code, KeyModifiers::NONE)
    }

    fn three_tabs() -> Tabs {
        Tabs::new(vec!["Models", "Agents", "Display"])
    }

    #[test]
    fn new_starts_active_at_zero() {
        let t = three_tabs();
        assert_eq!(t.active, 0, "fresh Tabs must start with active = 0");
        assert_eq!(t.titles.len(), 3);
        assert!(!t.bordered, "default not bordered");
    }

    #[test]
    fn next_advances_and_wraps_around() {
        let mut t = three_tabs();
        t.next();
        assert_eq!(t.active, 1);
        t.next();
        assert_eq!(t.active, 2);
        // Wrap: from last back to first
        t.next();
        assert_eq!(t.active, 0, "next() at end must wrap to 0");
    }

    #[test]
    fn prev_decrements_and_wraps_around() {
        let mut t = three_tabs();
        // Wrap from 0 to len-1 (the off-by-one footgun: underflow on 0-1
        // would panic in release mode)
        t.prev();
        assert_eq!(t.active, 2, "prev() at 0 must wrap to len-1");
        t.prev();
        assert_eq!(t.active, 1);
        t.prev();
        assert_eq!(t.active, 0);
    }

    #[test]
    fn next_and_prev_are_no_op_on_empty_titles() {
        // Empty Tabs is constructible (e.g., before async data loads).
        // next() and prev() must not panic on % 0 / underflow on 0-1.
        let mut t: Tabs = Tabs::new(Vec::<&str>::new());
        assert!(t.titles.is_empty());
        t.next(); // must not panic on % 0
        assert_eq!(t.active, 0);
        t.prev(); // must not panic on underflow
        assert_eq!(t.active, 0);
    }

    #[test]
    fn set_active_in_range_takes_effect() {
        let mut t = three_tabs();
        t.set_active(2);
        assert_eq!(t.active, 2);
        t.set_active(0);
        assert_eq!(t.active, 0);
    }

    #[test]
    fn set_active_out_of_range_is_silently_ignored() {
        // Treating OOB as a no-op is the documented contract — caller
        // doesn't get a result back, so panicking would surprise them.
        let mut t = three_tabs();
        t.set_active(1);
        t.set_active(99);
        assert_eq!(t.active, 1, "OOB set_active must NOT mutate");
        t.set_active(3); // exactly len → still OOB (0-indexed)
        assert_eq!(t.active, 1, "i == len must NOT mutate");
    }

    #[test]
    fn right_arrow_advances_and_left_arrow_decrements() {
        let mut t = three_tabs();
        assert!(matches!(t.handle_event(key(KeyCode::Right)), ComponentAction::Changed));
        assert_eq!(t.active, 1, "Right must call next()");

        assert!(matches!(t.handle_event(key(KeyCode::Left)), ComponentAction::Changed));
        assert_eq!(t.active, 0, "Left must call prev()");

        // Left at 0 wraps via prev()
        assert!(matches!(t.handle_event(key(KeyCode::Left)), ComponentAction::Changed));
        assert_eq!(t.active, 2, "Left at 0 wraps to len-1");
    }

    #[test]
    fn ctrl_tab_advances_and_shift_tab_decrements() {
        let mut t = three_tabs();
        let ctrl_tab = key_mod(KeyCode::Tab, KeyModifiers::CONTROL);
        let shift_tab = key_mod(KeyCode::Tab, KeyModifiers::SHIFT);

        assert!(matches!(t.handle_event(ctrl_tab), ComponentAction::Changed));
        assert_eq!(t.active, 1, "Ctrl+Tab must advance");

        assert!(matches!(t.handle_event(shift_tab), ComponentAction::Changed));
        assert_eq!(t.active, 0, "Shift+Tab must go back");
    }

    #[test]
    fn backtab_with_ctrl_or_shift_alone_decrements_but_not_combined() {
        // T10 (wont-fix per user decision 2026-05-17): the handler's pattern
        //   `(KeyModifiers::CONTROL | KeyModifiers::SHIFT, KeyCode::BackTab)`
        // is a *pattern-OR* (matches CTRL alone OR SHIFT alone), NOT a
        // bitwise OR (CTRL+SHIFT combined). SHIFT+Tab covers 99% of
        // terminals' reverse-tab needs, so we accept the gap. This test
        // documents the actual behavior as a wont-fix decision.
        let mut t = three_tabs();
        t.set_active(2);

        // SHIFT alone on BackTab → prev() ✓
        let shift_backtab = key_mod(KeyCode::BackTab, KeyModifiers::SHIFT);
        assert!(matches!(t.handle_event(shift_backtab), ComponentAction::Changed));
        assert_eq!(t.active, 1, "Shift+BackTab must call prev()");

        // CTRL alone on BackTab → prev() ✓
        let ctrl_backtab = key_mod(KeyCode::BackTab, KeyModifiers::CONTROL);
        assert!(matches!(t.handle_event(ctrl_backtab), ComponentAction::Changed));
        assert_eq!(t.active, 0, "Ctrl+BackTab must call prev()");

        // CTRL+SHIFT combined on BackTab is NOT matched today — bubbles
        let combo = key_mod(KeyCode::BackTab, KeyModifiers::CONTROL | KeyModifiers::SHIFT);
        match t.handle_event(combo) {
            ComponentAction::Bubble(_) => {}
            other => panic!("T10: CTRL+SHIFT+BackTab currently bubbles, got {other:?}"),
        }
        assert_eq!(t.active, 0, "T10: CTRL+SHIFT+BackTab must NOT change active until T10 is fixed");
    }

    #[test]
    fn unknown_key_and_non_key_event_bubble() {
        let mut t = three_tabs();

        // Unknown key (Char): not consumed → Bubble
        match t.handle_event(key(KeyCode::Char('x'))) {
            ComponentAction::Bubble(Event::Key(KeyEvent { code: KeyCode::Char('x'), .. })) => {}
            other => panic!("unknown key must bubble, got {other:?}"),
        }
        assert_eq!(t.active, 0, "unknown key must not mutate");

        // Non-key event (Mouse): not consumed → Bubble verbatim
        let mouse_ev = Event::Mouse(MouseEvent {
            kind: MouseEventKind::Down(MouseButton::Left),
            column: 0,
            row: 0,
            modifiers: KeyModifiers::NONE,
        });
        match t.handle_event(mouse_ev) {
            ComponentAction::Bubble(Event::Mouse(_)) => {}
            other => panic!("non-Key event must bubble, got {other:?}"),
        }
        assert_eq!(t.active, 0, "mouse event must not mutate");
    }
}
