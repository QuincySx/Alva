// INPUT:  alva_kernel_core, alva_kernel_abi, alva_agent_tools, alva_agent_security, alva_agent_memory (native)
// OUTPUT: AgentRuntime, AgentRuntimeBuilder, model, AgentState, AgentConfig, AgentEvent, AgentMessage, run_agent, Middleware, MiddlewareStack, SecurityMiddleware
// POS:    Crate root — composes all agent subsystems and re-exports a batteries-included API.
//! Batteries-included agent runtime.
//!
//! Composes alva-kernel-core + alva-agent-tools + alva-agent-security + alva-agent-memory
//! into a ready-to-use agent with a builder API.

pub mod builder;
pub mod graph;
pub mod init;
pub mod middleware;

pub use builder::{AgentRuntime, AgentRuntimeBuilder};
pub use init::model;

// Re-export key types for convenience
pub use alva_kernel_core::{AgentState, AgentConfig, AgentEvent, AgentMessage, run_agent};
pub use alva_kernel_core::{Middleware, MiddlewareStack};
pub use alva_kernel_abi::{Tool, ToolExecutionContext, ToolRegistry, LanguageModel, Provider, ProviderRegistry};
pub use alva_agent_tools::{register_builtin_tools, register_all_tools};
pub use alva_agent_security::{SecurityGuard, SandboxMode};
pub use middleware::SecurityMiddleware;
#[cfg(feature = "native")]
pub use alva_agent_memory::MemoryService;
