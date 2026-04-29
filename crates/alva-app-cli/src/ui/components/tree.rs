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
        if did { ComponentAction::None } else { ComponentAction::Bubble(event) }
    }
}
