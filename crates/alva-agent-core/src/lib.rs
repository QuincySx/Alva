// INPUT:  alva_types, tokio, tokio_stream, tracing, uuid, chrono, async_trait, thiserror, serde, serde_json, alva_agent_context
// OUTPUT: Agent, AgentEvent, AgentMessage, AgentHooks, AgentState, AgentContext, Middleware, MiddlewareStack, Extensions, CompressionMiddleware, ConvertToLlmFn
// POS:    Crate root — declares modules and re-exports the public API for the agent engine.
pub mod types;
pub mod event;
pub mod middleware;
pub mod agent;
mod agent_loop;
mod tool_executor;

pub use types::{
    AgentMessage, AgentHooks, AgentState, AgentContext, ToolCallDecision,
    ToolExecutionMode, HookFuture, ConvertToLlmFn,
};
pub use event::AgentEvent;
pub use agent::Agent;
pub use middleware::{Middleware, MiddlewareStack, MiddlewareContext, MiddlewareError, MiddlewarePriority, Extensions, CompressionMiddleware, CompressionConfig};

/// Re-export context types so downstream crates don't need a direct dependency.
pub use alva_agent_context::{
    ContextHooks, ContextHandle, ContextHandleImpl, ContextStore,
    RulesContextHooks, DefaultContextHooks, DefaultHooksConfig,
    SessionAccess, SessionEvent, InMemorySession, EventQuery, EventMatch,
    MessageStore, InMemoryMessageStore, Turn,
};
