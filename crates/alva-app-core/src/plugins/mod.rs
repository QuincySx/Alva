//! Agent plugins — optional capabilities that plug into the agent's
//! extension points (ContextHooks, Middleware, Graph nodes, Tools).
//!
//! Each plugin is self-contained and only touches its own data.
//! Multiple plugins compose cleanly via `ContextHooksChain` and
//! `MiddlewareStack`.
//!
//! - [`blackboard`] — shared multi-agent communication space
//! - [`evaluation`] — QA evaluation loop, sprint contracts, grading criteria

pub mod blackboard;
pub mod evaluation;
pub mod team;
