// INPUT:  alva_types, tokio, tokio_stream, tracing, uuid, chrono, async_trait, thiserror, serde, serde_json
// OUTPUT: AgentState, AgentConfig, AgentEvent, AgentMessage, Middleware, MiddlewareStack, Extensions, run_agent, builtins
// POS:    Crate root — declares modules and re-exports the public API for the agent engine.

pub mod shared;
pub mod state;
pub mod middleware;
pub mod run;
pub mod builtins;
pub mod event;

// Re-exports — shared types
pub use shared::{Extensions, MiddlewareError, MiddlewarePriority};

// Re-exports — state
pub use state::{AgentState, AgentConfig};

// Re-exports — middleware
pub use middleware::{Middleware, MiddlewareStack, LlmCallFn, ToolCallFn};

// Re-exports — run
pub use run::run_agent;

// Re-exports — event
pub use event::AgentEvent;

// Re-exports — builtins
pub use builtins::{LoopDetectionMiddleware, DanglingToolCallMiddleware};

// Re-export AgentMessage from alva-types
pub use alva_types::AgentMessage;

// Context types are available directly from `alva_types::context` — no re-export needed.
