//! Process manager infrastructure
//! Re-exports from agent::agent_client::connection for convenience.

pub use crate::agent::agent_client::connection::factory::{AcpProcessManager, ProcessManagerConfig};
pub use crate::agent::agent_client::connection::processes::ProcessState;
