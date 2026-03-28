//! Agent execution scope — blackboard communication, board isolation, session tree, depth control.
//!
//! Extracted from `alva-app-core` to be a standalone crate.
//!
//! - [`blackboard`] — shared multi-agent communication space (Blackboard, BoardMessage, AgentProfile, BlackboardPlugin)
//! - [`board_registry`] — manages Blackboard instances scoped to SpawnScope IDs
//! - [`session_tracker`] — tracks tree-structured session relationships across the spawn tree
//! - [`scope_impl`] — concrete SpawnScope implementation (one node per spawn tree)

pub mod blackboard;
pub mod board_registry;
pub mod session_tracker;
pub mod scope_impl;

pub use blackboard::{Blackboard, BlackboardPlugin, BlackboardPluginConfig, AgentProfile, BoardMessage, MessageKind, TaskPhase};
pub use board_registry::BoardRegistry;
pub use session_tracker::{SessionTracker, SessionSnapshot};
pub use scope_impl::SpawnScopeImpl;
