// INPUT:  (re-exports submodules: chat_panel, message_list, input_box)
// OUTPUT: pub use chat_panel::* (ChatPanel)
// POS:    Barrel module for the chat panel, re-exporting the ChatPanel composite view.
pub mod chat_panel;
pub mod message_list;
pub mod input_box;

pub use chat_panel::*;
