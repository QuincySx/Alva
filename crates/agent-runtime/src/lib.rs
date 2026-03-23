// INPUT:  agent_core, agent_types, agent_tools, agent_security, agent_memory, builder, init
// OUTPUT: AgentRuntime, AgentRuntimeBuilder, model, Agent, AgentEvent, AgentMessage, AgentHooks, Tool, ToolContext, ToolRegistry, LanguageModel, Provider, ProviderRegistry, SecurityGuard, SandboxMode, SecurityMiddleware, MemoryService
// POS:    Crate root — composes all agent subsystems and re-exports a batteries-included API.
//! Batteries-included agent runtime.
//!
//! Composes agent-core + agent-tools + agent-security + agent-memory
//! into a ready-to-use agent with a builder API.

pub mod builder;
pub mod init;
pub mod middleware;

pub use builder::{AgentRuntime, AgentRuntimeBuilder};
pub use init::model;

// Re-export key types for convenience
pub use agent_core::{Agent, AgentEvent, AgentMessage, AgentHooks, ConvertToLlmFn};
pub use agent_core::middleware::{Middleware, MiddlewareStack};
pub use agent_types::{Tool, ToolContext, ToolRegistry, LanguageModel, Provider, ProviderRegistry};
pub use agent_tools::{register_builtin_tools, register_all_tools};
pub use agent_security::{SecurityGuard, SandboxMode};
pub use middleware::SecurityMiddleware;
pub use agent_memory::MemoryService;
