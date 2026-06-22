// INPUT:  crossterm::Event, ratatui (Frame, Rect, Block, Borders),
//         tui_tree_widget (Tree as TwTree, TreeItem, TreeState),
//         super::{Component, ComponentAction, theme}
// OUTPUT: Tree<Id> — thin facade around tui-tree-widget
// POS:    Nested tree view for file picker / settings tree / hierarchical
//         menus. Wraps `tui-tree-widget`'s Tree + TreeItem + TreeState so
//         our callers stay on the Component trait.

use crossterm::event::{Event, KeyCode, KeyEvent};
use ratatui::layout::Rect;
use ratatui::style::{Modifier, Style};
use ratatui::widgets::{Block, Borders};
use ratatui::Frame;
use tui_tree_widget::{Tree as TwTree, TreeItem, TreeState};

use std::cell::RefCell;
use std::hash::Hash;

use super::super::theme::Theme;
use super::{Component, ComponentAction};

/// Tree component. `Id: Clone + Hash + Eq` is the key type — tui-tree-widget
/// uses it for selection paths. Construct items with `tree_item(id, label)`
/// and `with_children(...)` from the upstream crate.
pub struct Tree<Id: Clone + Hash + Eq + Send + Sync + 'static> {
    items: Vec<TreeItem<'static, Id>>,
    state: RefCell<TreeState<Id>>,
    title: String,
}

impl<Id: Clone + Hash + Eq + Send + Sync + 'static> Tree<Id> {
    pub fn new(items: Vec<TreeItem<'static, Id>>, title: impl Into<String>) -> Self {
        Self {
            items,
            state: RefCell::new(TreeState::default()),
            title: title.into(),
        }
    }

    pub fn set_items(&mut self, items: Vec<TreeItem<'static, Id>>) {
        self.items = items;
    }

    /// The currently-selected leaf path (sequence of Ids from root).
    pub fn selected(&self) -> Vec<Id> {
        self.state.borrow().selected().to_vec()
    }
}

impl<Id: Clone + Hash + Eq + Send + Sync + std::fmt::Debug + 'static> Component for Tree<Id> {
    fn render(&self, frame: &mut Frame<'_>, area: Rect, theme: &Theme) {
        let widget = TwTree::new(&self.items)
            .expect("tree items must have unique ids")
            .block(
                Block::default()
                    .borders(Borders::ALL)
                    .border_style(theme.border)
                    .title(format!(" {} ", self.title)),
            )
            .highlight_style(Style::default().add_modifier(Modifier::REVERSED));

        let mut state = self.state.borrow_mut();
        frame.render_stateful_widget(widget, area, &mut state);
    }

    fn handle_event(&mut self, event: Event) -> ComponentAction {
        let Event::Key(KeyEvent { code, .. }) = event.clone() else {
            return ComponentAction::Bubble(event);
        };
        let mut state = self.state.borrow_mut();
        let did = match code {
            KeyCode::Up => state.key_up(),
            KeyCode::Down => state.key_down(),
            KeyCode::Left => state.key_left(),
            KeyCode::Right => state.key_right(),
            KeyCode::Home => state.select_first(),
            KeyCode::End => state.select_last(),
            KeyCode::Enter => {
                let _ = state.toggle_selected();
                drop(state);
                let path = self.state.borrow().selected().to_vec();
                let label = format!("{:?}", path);
                return ComponentAction::Submit(label);
            }
            KeyCode::Esc => return ComponentAction::Dismiss,
            _ => return ComponentAction::Bubble(event),
        };
        if did {
            ComponentAction::None
        } else {
            ComponentAction::Bubble(event)
        }
    }
}

#[cfg(test)]
mod tests {
    //! Tests for Tree<Id> — the keyboard router + selection state.
    //! render() needs a Frame so is intentionally not exercised here;
    //! handle_event() is pure data-in/action-out and is the route
    //! users actually hit.
    use super::*;
    use crossterm::event::{KeyEventKind, KeyEventState, KeyModifiers};

    fn key(code: KeyCode) -> Event {
        Event::Key(KeyEvent {
            code,
            modifiers: KeyModifiers::NONE,
            kind: KeyEventKind::Press,
            state: KeyEventState::empty(),
        })
    }

    fn leaf(id: &'static str) -> TreeItem<'static, &'static str> {
        TreeItem::new_leaf(id, id)
    }

    // -- Construction / state ----------------------------------------------

    #[test]
    fn new_empty_tree_has_no_selection() {
        let t: Tree<&'static str> = Tree::new(vec![], "Empty");
        assert!(t.selected().is_empty());
    }

    #[test]
    fn set_items_replaces_items_without_panicking() {
        // Caller may swap the underlying tree while the component is
        // live (e.g. file picker repopulates on cd). selected() must
        // still be callable afterwards.
        let mut t: Tree<&'static str> = Tree::new(vec![leaf("old")], "T");
        t.set_items(vec![leaf("new-a"), leaf("new-b")]);
        // Selection state is independent of items — still empty here.
        assert!(t.selected().is_empty());
    }

    // -- handle_event routing ---------------------------------------------

    #[test]
    fn esc_returns_dismiss() {
        let mut t: Tree<&'static str> = Tree::new(vec![leaf("a")], "T");
        let action = t.handle_event(key(KeyCode::Esc));
        assert!(matches!(action, ComponentAction::Dismiss));
    }

    #[test]
    fn enter_returns_submit_with_selection_label() {
        // Pin: Enter always returns Submit (no fall-through to None or
        // Bubble), even on an empty selection. The label is a Debug
        // render of the selected path.
        let mut t: Tree<&'static str> = Tree::new(vec![leaf("a")], "T");
        let action = t.handle_event(key(KeyCode::Enter));
        match action {
            ComponentAction::Submit(label) => {
                // On a tree with one un-selected leaf, the path is "[]".
                // We don't pin the exact value of label since toggle_selected
                // behavior depends on upstream — just that we got Submit.
                assert!(label.starts_with('['));
            }
            other => panic!("expected Submit, got {other:?}"),
        }
    }

    #[test]
    fn unknown_keycode_bubbles_event() {
        // Pin the default arm: keys not in the routing match must
        // bubble the original event so parent components can keep
        // routing (e.g. 'q' to close the modal).
        let mut t: Tree<&'static str> = Tree::new(vec![leaf("a")], "T");
        let ev = key(KeyCode::Char('q'));
        let action = t.handle_event(ev.clone());
        match action {
            ComponentAction::Bubble(returned) => {
                // The bubbled event must be the SAME event, not a synthesized one.
                assert!(matches!(returned, Event::Key(k) if k.code == KeyCode::Char('q')));
            }
            other => panic!("expected Bubble, got {other:?}"),
        }
    }

    #[test]
    fn non_key_event_bubbles_immediately() {
        // FocusGained is not a KeyEvent — must hit the `let-else`
        // fallback and bubble without touching TreeState.
        let mut t: Tree<&'static str> = Tree::new(vec![leaf("a")], "T");
        let ev = Event::FocusGained;
        let action = t.handle_event(ev);
        assert!(matches!(
            action,
            ComponentAction::Bubble(Event::FocusGained)
        ));
    }

    #[test]
    fn arrow_key_on_empty_tree_bubbles_when_state_does_not_move() {
        // Pin: when state.key_down() returns false (no items / can't
        // move further), the action is Bubble — that's how the parent
        // gets a chance to route arrows for itself (e.g. switching
        // panels). Without this, an empty tree would silently absorb
        // all arrow keys.
        let mut t: Tree<&'static str> = Tree::new(vec![], "Empty");
        let action = t.handle_event(key(KeyCode::Down));
        assert!(matches!(action, ComponentAction::Bubble(_)));
    }
}
