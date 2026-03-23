//! Batteries-included agent runtime.
//!
//! Composes agent-core + agent-tools + agent-security + agent-memory
//! into a ready-to-use agent with a builder API.

pub mod builder;
pub mod init;

pub use builder::{AgentRuntime, AgentRuntimeBuilder};
pub use init::model;

// Re-export key types for convenience
pub use agent_core::{Agent, AgentEvent, AgentMessage, AgentHooks, ConvertToLlmFn};
pub use agent_core::middleware::{Middleware, MiddlewareStack};
pub use agent_types::{Tool, ToolContext, ToolRegistry, LanguageModel, Provider, ProviderRegistry};
pub use agent_tools::{register_builtin_tools, register_all_tools};
pub use agent_security::{SecurityGuard, SandboxMode};
pub use agent_memory::MemoryService;
