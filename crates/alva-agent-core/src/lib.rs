// INPUT:  alva_types, tokio, tokio_stream, tracing, uuid, chrono, async_trait, thiserror, serde, serde_json
// OUTPUT: AgentState, AgentConfig, AgentEvent, AgentMessage, Middleware, MiddlewareStack, Extensions, run_agent, builtins
// POS:    Crate root — declares modules and re-exports the public API for the V2 agent engine.

// V1 middleware module kept for shared types (Extensions, MiddlewareError, MiddlewarePriority,
// MiddlewareContext, LlmCallFn, ToolCallFn etc.) — v2 middleware re-uses these.
pub mod middleware;

pub mod event;
pub mod v2;

// Re-export V2 types at top level
pub use v2::state::{AgentState, AgentConfig};
pub use v2::middleware::{
    Middleware, MiddlewareStack, LlmCallFn, ToolCallFn,
};
pub use v2::run::run_agent;
pub use v2::builtins::{LoopDetectionMiddleware, DanglingToolCallMiddleware};

// Shared types
pub use event::AgentEvent;
pub use middleware::{MiddlewareError, MiddlewarePriority, Extensions};

// Re-export AgentMessage from alva-types
pub use alva_types::AgentMessage;

// Context types are available directly from `alva_types::context` — no re-export needed.
