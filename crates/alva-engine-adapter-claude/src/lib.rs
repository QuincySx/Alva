// INPUT:  adapter, bridge, config, idle_watchdog, mapping, process, protocol (internal modules)
// OUTPUT: pub ClaudeAdapter, pub ClaudeAdapterConfig, pub PermissionMode
// POS:    Crate root that re-exports the Claude Agent SDK bridge adapter and its configuration.

mod bridge;
mod config;
mod idle_watchdog;
mod mapping;
mod process;
mod protocol;

mod adapter;

pub use adapter::ClaudeAdapter;
pub use config::{ClaudeAdapterConfig, PermissionMode};
