// INPUT:  alva_kernel_core, alva_kernel_abi, alva_agent_extension_builtin, alva_agent_security, alva_agent_memory (native)
// OUTPUT: AgentRuntime, AgentRuntimeBuilder, model, AgentState, AgentConfig, AgentEvent, AgentMessage, run_agent, Middleware, MiddlewareStack, SecurityMiddleware
// POS:    Native platform capability crate; legacy AgentRuntimeBuilder is deprecated.
//! Native platform capabilities for Alva.
//!
//! New app harnesses should use `alva_app_core::BaseAgentBuilder`; SDK callers
//! should use `alva_agent_core::AgentBuilder`. This crate keeps native model
//! init, sleeper, middleware, graph re-exports, and the deprecated legacy
//! `AgentRuntimeBuilder`.

pub mod builder;
pub mod graph;
pub mod init;
pub mod middleware;
pub mod sleeper;

pub use builder::AgentRuntime;
#[allow(deprecated)]
pub use builder::AgentRuntimeBuilder;
pub use init::model;
pub use sleeper::TokioSleeper;

// Re-export key types for convenience.
// These are **host-layer** re-exports intended for first-party code
// that wires kernels and runtimes together. External Extension /
// plugin authors should NOT import from this crate — they go through
// `alva-app-core::{Plugin, Registrar}` instead.
pub use alva_agent_extension_builtin::register_builtin_tools;
#[cfg(feature = "native")]
pub use alva_agent_memory::MemoryService;
pub use alva_agent_security::{SandboxMode, SecurityGuard};
pub use alva_kernel_abi::{
    LanguageModel, Provider, ProviderRegistry, Tool, ToolExecutionContext, ToolRegistry,
};
pub use alva_kernel_core::{run_agent, AgentConfig, AgentEvent, AgentMessage, AgentState};
pub use alva_kernel_core::{Middleware, MiddlewareStack};
pub use middleware::SecurityMiddleware;
