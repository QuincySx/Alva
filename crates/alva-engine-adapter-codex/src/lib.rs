// INPUT:  adapter, config, mapping, process, protocol
// OUTPUT: CodexAdapter, CodexAdapterConfig, CodexSandboxMode
// POS:    Crate root for the Codex CLI EngineRuntime adapter.

mod adapter;
mod config;
mod mapping;
mod process;
mod protocol;

pub use adapter::CodexAdapter;
pub use config::{CodexAdapterConfig, CodexSandboxMode};
