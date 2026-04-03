//! Multi-agent swarm infrastructure — team lifecycle, inter-agent messaging,
//! and agent spawning across different execution backends.
//!
//! This module provides the coordination layer for running multiple agents
//! as a team (swarm). It is independent of the graph-based team tool in
//! [`super::plugins::team`]; that tool wires agents into a static DAG,
//! while swarm supports dynamic, runtime-managed teams with a mailbox
//! system for async agent-to-agent communication.
//!
//! # Architecture
//!
//! - [`types`] — Core types: `TeamDefinition`, `TeamMember`, `SwarmContext`, `AgentSpawnConfig`
//! - [`mailbox`] — Channel-based agent-to-agent messaging (register, send, broadcast)
//! - [`coordinator`] — Team lifecycle management (create, delete, member tracking, summaries)
//! - [`spawn`] — Agent spawning across backends (in-process, subprocess, tmux)
//! - [`backends`] — Backend trait + auto-selection (tmux preferred, in-process fallback)

pub mod backends;
pub mod coordinator;
pub mod mailbox;
pub mod spawn;
pub mod types;

pub use backends::*;
pub use coordinator::*;
pub use mailbox::*;
pub use spawn::*;
pub use types::*;
