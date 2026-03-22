// INPUT:  (re-exports submodules: root_view, sidebar, side_panel, chat_panel, agent_panel, settings_panel)
// OUTPUT: pub use root_view::* (RootView)
// POS:    Barrel module that aggregates all UI view panels and re-exports the top-level RootView.
pub mod root_view;
pub mod sidebar;
pub mod side_panel;
pub mod chat_panel;
pub mod agent_panel;
pub mod agent_detail_panel;
pub mod settings_panel;

pub use root_view::*;
