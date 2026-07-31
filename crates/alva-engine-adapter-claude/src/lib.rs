// INPUT:  adapter, bridge, config, idle_watchdog, mapping, process, protocol, print_* (internal modules)
// OUTPUT: ClaudeAdapter (full SDK), ClaudePrintAdapter (light CLI), configs, ClaudeRuntimeMode
// POS:    Claude engine adapters for Hub — full (Agent SDK) and light (`claude -p`).

mod bridge;
mod config;
mod idle_watchdog;
mod mapping;
mod print_adapter;
mod print_config;
mod print_process;
mod process;
mod protocol;

mod adapter;

pub use adapter::ClaudeAdapter;
pub use config::{ClaudeAdapterConfig, PermissionMode};
pub use print_adapter::ClaudePrintAdapter;
pub use print_config::{ClaudePrintConfig, ClaudePrintPermissionMode, ClaudeRuntimeMode};
