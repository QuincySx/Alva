// INPUT:  blackboard, board_registry, scope_impl
// OUTPUT: Blackboard, BlackboardPlugin, BlackboardPluginConfig, AgentProfile, BoardMessage,
//         MessageKind, TaskPhase, BoardRegistry, SpawnScopeImpl
// POS:    Multi-agent collaboration scope — merged in from the former alva-agent-scope crate (Phase 3).

//! Multi-agent collaboration runtime — formerly `alva-agent-scope`, merged into context
//! because `BlackboardPlugin` is itself a `ContextHooks` implementation, so blackboard +
//! board registry + spawn scope live naturally under the context plugin framework.
//!
//! - [`blackboard`] — shared multi-agent communication space (Blackboard, BoardMessage,
//!   AgentProfile, BlackboardPlugin)
//! - [`board_registry`] — manages Blackboard instances scoped to SpawnScope IDs
//! - [`scope_impl`] — concrete SpawnScope implementation (one node per spawn tree)

pub mod blackboard;
pub mod board_registry;
pub mod scope_impl;

pub use blackboard::{
    AgentProfile, Blackboard, BlackboardPlugin, BlackboardPluginConfig, BoardMessage, MessageKind,
    TaskPhase,
};
pub use board_registry::BoardRegistry;
pub use scope_impl::SpawnScopeImpl;
