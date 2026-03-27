//! Blackboard — shared communication space for multi-agent collaboration.
//!
//! Implements the "shared chat room" pattern: all agents read/write to a
//! common message stream, each knowing who they are and who else is in
//! the room. Communication happens through natural-language messages
//! with @mentions, not point-to-point channels.
//!
//! # Architecture
//!
//! - [`Blackboard`] — the shared data structure (one per collaboration)
//! - [`AgentProfile`] — an agent's self-description and relationships
//! - [`BoardMessage`] / [`MessageKind`] — what gets posted to the board
//! - [`BlackboardPlugin`] — a `ContextHooks` impl that bridges an agent to the board
//!
//! # Usage
//!
//! ```rust,ignore
//! use std::sync::Arc;
//! use alva_app_core::blackboard::{Blackboard, BlackboardPlugin, AgentProfile};
//!
//! // One shared board for the team
//! let board = Arc::new(Blackboard::new());
//!
//! // Each agent gets its own plugin, sharing the same board
//! let planner_plugin = BlackboardPlugin::new(
//!     AgentProfile::new("planner", "需求分析与规划")
//!         .provides_to(["generator", "evaluator"])
//!         .with_capability("拆解需求")
//!         .with_capability("撰写技术 spec"),
//!     board.clone(),
//! );
//!
//! let generator_plugin = BlackboardPlugin::new(
//!     AgentProfile::new("generator", "代码实现")
//!         .depends_on(["planner"])
//!         .provides_to(["evaluator"])
//!         .with_capability("编写代码"),
//!     board.clone(),
//! );
//! ```

mod board;
mod message;
mod plugin;
mod profile;

pub use board::Blackboard;
pub use message::{BoardMessage, MessageKind, TaskPhase};
pub use plugin::BlackboardPlugin;
pub use profile::AgentProfile;
