// INPUT:  (re-exports submodules: root_view, sidebar, chat_panel, agent_detail_panel, settings_panel, dialogs, welcome_view)
// OUTPUT: pub use root_view::* (RootView)
// POS:    Barrel module that aggregates all UI view panels and re-exports the top-level RootView.
pub mod root_view;
pub mod sidebar;
pub mod chat_panel;
pub mod agent_detail_panel;
pub mod settings_panel;
pub mod dialogs;
pub mod session_welcome;

pub use root_view::*;
