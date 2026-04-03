//! Agent plugins — optional capabilities that plug into the agent's
//! extension points (ContextHooks, Middleware, Graph nodes, Tools).
//!
//! Each plugin is self-contained and only touches its own data.
//! Multiple plugins compose cleanly via `ContextHooksChain` and
//! `MiddlewareStack`.
//!
//! - [`blackboard`] — shared multi-agent communication space (re-exported from `alva-agent-scope`)
//! - [`evaluation`] — QA evaluation loop, sprint contracts, grading criteria
//! - [`types`] — Plugin definition, scope, status, installation types
//! - [`manager`] — Plugin manager for discovery, install, enable/disable lifecycle

pub mod agent_spawn;
pub mod task_spawn;
pub mod evaluation;
pub mod team;
pub mod types;
pub mod manager;

pub use types::*;
pub use manager::*;

// Re-export blackboard from alva-agent-scope for backward compatibility
pub use alva_agent_scope::blackboard;
