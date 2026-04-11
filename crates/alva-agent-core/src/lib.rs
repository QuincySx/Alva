// INPUT:  alva_types, tokio, tokio_stream, tracing, uuid, chrono, async_trait, thiserror, serde, serde_json
// OUTPUT: AgentState, AgentConfig, AgentEvent, AgentMessage, Middleware, MiddlewareStack, Extensions, run_agent, builtins
// POS:    Crate root — declares modules and re-exports the public API for the agent engine.

pub mod builtins;
pub mod event;
pub mod extension;
pub mod middleware;
pub mod pending_queue;
pub mod run;
pub mod run_child;
pub mod runtime_context;
pub mod shared;
pub mod state;

// Re-exports — shared types
pub use shared::{Extensions, MiddlewareError, MiddlewarePriority};

// Re-exports — state
pub use state::{AgentConfig, AgentState};

// Re-exports — extension
pub use extension::Extension;

// Re-exports — middleware
pub use middleware::{LlmCallFn, Middleware, MiddlewareStack, ToolCallFn};

// Re-exports — run
pub use run::run_agent;
pub use run_child::{run_child_agent, ChildAgentOutput, ChildAgentParams};

// Re-exports — event
pub use event::AgentEvent;

// Re-exports — pending_queue
pub use pending_queue::{AgentLoopHook, PendingMessageQueue};

// Re-exports — runtime context
pub use runtime_context::RuntimeExecutionContext;

// Re-exports — builtins
pub use builtins::{DanglingToolCallMiddleware, LoopDetectionMiddleware};

// Re-export AgentMessage from alva-types
pub use alva_types::AgentMessage;

// Context types are available directly from `alva_types::context` — no re-export needed.
