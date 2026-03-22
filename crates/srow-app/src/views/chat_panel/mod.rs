// INPUT:  (re-exports submodules: chat_panel, message_list, input_box, markdown, code_block)
// OUTPUT: pub use chat_panel::* (ChatPanel)
// POS:    Barrel module for the chat panel, re-exporting the ChatPanel composite view.
pub mod chat_panel;
pub mod message_list;
pub mod input_box;
pub mod markdown;
pub mod code_block;
pub mod message_bubble;
pub mod tool_call_block;
pub mod thinking_block;
pub mod agent_block;

pub use chat_panel::*;
