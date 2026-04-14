//! Agent-layer core: the Extension system and test-grade ToolFs.
//!
//! This crate holds the pure agent-internal extension machinery that used
//! to live inside `alva-app-core/src/extension/`, plus `MockToolFs` which
//! used to live in `alva-agent-tools`. It deliberately does NOT depend on
//! any protocol crate, LLM provider, persistence, or host-specific code.

pub mod mock_fs;
pub use mock_fs::MockToolFs;

pub mod extension;
pub use extension::{
    Extension, ExtensionBridgeMiddleware, ExtensionContext, ExtensionEvent, ExtensionHost,
    EventResult, FinalizeContext, HostAPI, RegisteredCommand,
};

pub mod agent;
pub mod agent_builder;

pub use agent::Agent;
pub use agent_builder::AgentBuilder;
