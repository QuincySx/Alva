// INPUT:  adapter, config, mapping, process, protocol, retry_escalation
// OUTPUT: CodexAdapter, CodexAdapterConfig, CodexSandboxMode
// POS:    Crate root for the Codex CLI EngineRuntime adapter.

mod adapter;
mod config;
mod mapping;
mod process;
mod protocol;
mod retry_escalation;

pub use adapter::CodexAdapter;
pub use config::{CodexAdapterConfig, CodexSandboxMode};
