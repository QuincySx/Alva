// INPUT:  blackboard, board_registry, session_tracker, scope_impl
// OUTPUT: Blackboard, BlackboardPlugin, BlackboardPluginConfig, AgentProfile, BoardMessage,
//         MessageKind, TaskPhase, BoardRegistry, SessionTracker, SessionSnapshot, SpawnScopeImpl
// POS:    Multi-agent collaboration scope — merged in from the former alva-agent-scope crate (Phase 3).

//! Multi-agent collaboration runtime — formerly `alva-agent-scope`, merged into context
//! because `BlackboardPlugin` is itself a `ContextHooks` implementation, so blackboard +
//! board registry + session tracker + spawn scope live naturally under the context plugin
//! framework.
//!
//! - [`blackboard`] — shared multi-agent communication space (Blackboard, BoardMessage,
//!   AgentProfile, BlackboardPlugin)
//! - [`board_registry`] — manages Blackboard instances scoped to SpawnScope IDs
//! - [`session_tracker`] — tracks tree-structured session relationships across the spawn tree
//! - [`scope_impl`] — concrete SpawnScope implementation (one node per spawn tree)

pub mod blackboard;
pub mod board_registry;
pub mod session_tracker;
pub mod scope_impl;

pub use blackboard::{
    AgentProfile, Blackboard, BlackboardPlugin, BlackboardPluginConfig, BoardMessage, MessageKind,
    TaskPhase,
};
pub use board_registry::BoardRegistry;
pub use scope_impl::SpawnScopeImpl;
pub use session_tracker::{SessionSnapshot, SessionTracker};
