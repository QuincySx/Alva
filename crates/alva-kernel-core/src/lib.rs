// INPUT:  alva_kernel_abi, tokio, tokio_stream, tracing, uuid, chrono, async_trait, thiserror, serde, serde_json
// OUTPUT: AgentState, AgentConfig, AgentEvent, AgentMessage, Middleware, MiddlewareStack, Extensions, run_agent, builtins
// POS:    Crate root — declares modules and re-exports the public API for the agent engine.

pub mod agent_session;
pub mod builtins;
pub mod context_runtime;
pub mod event;
pub mod middleware;
pub mod run;
pub mod run_child;
pub mod runtime_context;
pub(crate) mod session_events;
pub mod shared;
pub mod state;
pub mod tool_batch;

// Re-exports — shared types
pub use shared::{Extensions, MiddlewareError, MiddlewarePriority};

// Re-exports — state
pub use state::{AgentConfig, AgentState};

// Re-exports — middleware
pub use middleware::{LlmCallFn, Middleware, MiddlewareStack, ToolCallFn};

// Re-exports — run
pub use run::run_agent;
pub use run_child::{run_child_agent, ChildAgentOutput, ChildAgentParams};
pub use tool_batch::{CommittedToolCall, ToolBatchCoordinator};

// Re-exports — event
pub use event::AgentEvent;

// Re-exports — runtime context
pub use runtime_context::RuntimeExecutionContext;

// Re-exports — builtins
pub use builtins::{DanglingToolCallMiddleware, LoopDetectionMiddleware};
pub use context_runtime::ContextRuntime;

// Re-exports — concrete AgentSession backends (contract stays in alva-kernel-abi,
// re-exported through agent_session for one-path consumer imports).
pub use agent_session::{InMemoryAgentSession, ListenableInMemorySession};

// Re-export AgentMessage from alva-kernel-abi
pub use alva_kernel_abi::AgentMessage;

// Context types are available directly from `alva_kernel_abi::context` — no re-export needed.
