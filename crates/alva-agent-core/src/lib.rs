//! Agent-layer core: the Plugin assembly system and test-grade ToolFs.
//!
//! This crate holds the pure agent-internal plugin machinery that used
//! to live inside `alva-app-core/src/extension/`, plus `MockToolFs` which
//! used to live in `alva-agent-tools`. It deliberately does NOT depend on
//! any protocol crate, LLM provider, persistence, or host-specific code.

pub mod mock_fs;
pub use mock_fs::MockToolFs;

// 具体内存会话后端（ticket 09 自 kernel-core 迁入）：契约在 alva-kernel-abi，
// 实现在 SDK 装配层——内核只剩会话契约，不含任何具体后端。
pub mod agent_session;

pub mod extension;
pub use extension::{PluginHost, RegisteredCommand};

pub mod agent;
pub mod agent_builder;
pub mod tool_lock;

pub use tool_lock::{
    AcquireError, LockSnapshot, ToolLockGuards, ToolLockMiddleware, ToolLockRegistry,
    DEFAULT_ACQUIRE_TIMEOUT, GLOBAL_SERIAL_KEY,
};

pub use agent::{Agent, AgentAssemblySnapshot, PluginAssemblySnapshot};
pub use agent_builder::AgentBuilder;
