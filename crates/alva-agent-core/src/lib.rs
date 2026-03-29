// INPUT:  alva_types, tokio, tokio_stream, tracing, uuid, chrono, async_trait, thiserror, serde, serde_json
// OUTPUT: AgentState, AgentConfig, AgentEvent, AgentMessage, Middleware, MiddlewareStack, Extensions, run_agent, builtins
// POS:    Crate root — declares modules and re-exports the public API for the V2 agent engine.

// V1 middleware module kept for shared types (Extensions, MiddlewareError, MiddlewarePriority,
// CompressionMiddleware etc.) — v2 middleware re-uses these.
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

/// Re-export context types so downstream crates don't need a direct dependency on alva-types::context.
pub use alva_types::context::{
    ContextHooks, ContextHandle, ContextSystem, ContextError,
    NoopContextHooks, NoopContextHandle,
    SessionAccess, SessionEvent, IngestAction, EventQuery, EventMatch,
    ContextEntry, ContextMetadata, ContextLayer, ContextSnapshot,
    EntryOrigin, Priority, BudgetInfo,
    Injection, InjectionContent, CompressAction,
    MemoryFact, MemoryCategory, ToolPattern, MessageRange, MessageSelector,
    PromptSection, RuntimeContext, EntrySnapshot, LayerStats,
};
