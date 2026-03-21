// INPUT:  (re-exports submodules: gpui_chat_state, gpui_chat)
// OUTPUT: pub use gpui_chat::{GpuiChat, GpuiChatConfig, GpuiChatEvent}
// POS:    Barrel module for the GPUI chat binding layer.
pub mod gpui_chat_state;
pub mod gpui_chat;

pub use gpui_chat::{GpuiChat, GpuiChatConfig, GpuiChatEvent, SharedRuntime};
