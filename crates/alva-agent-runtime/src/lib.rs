// INPUT:  alva_agent_core, alva_types, alva_agent_tools, alva_agent_security, alva_agent_memory, builder, init
// OUTPUT: AgentRuntime, AgentRuntimeBuilder, model, Agent, AgentEvent, AgentMessage, AgentHooks, Tool, ToolContext, ToolRegistry, LanguageModel, Provider, ProviderRegistry, SecurityGuard, SandboxMode, SecurityMiddleware, MemoryService
// POS:    Crate root — composes all agent subsystems and re-exports a batteries-included API.
//! Batteries-included agent runtime.
//!
//! Composes alva-agent-core + alva-agent-tools + alva-agent-security + alva-agent-memory
//! into a ready-to-use agent with a builder API.

pub mod builder;
pub mod graph;
pub mod init;
pub mod middleware;

pub use builder::{AgentRuntime, AgentRuntimeBuilder};
pub use init::model;

// Re-export key types for convenience
pub use alva_agent_core::{Agent, AgentEvent, AgentMessage, AgentHooks, ConvertToLlmFn};
pub use alva_agent_core::middleware::{Middleware, MiddlewareStack};
pub use alva_types::{Tool, ToolContext, ToolRegistry, LanguageModel, Provider, ProviderRegistry};
pub use alva_agent_tools::{register_builtin_tools, register_all_tools};
pub use alva_agent_security::{SecurityGuard, SandboxMode};
pub use middleware::SecurityMiddleware;
pub use alva_agent_memory::MemoryService;
