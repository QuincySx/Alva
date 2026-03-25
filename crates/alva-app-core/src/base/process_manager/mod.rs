// INPUT:  crate::agent::agent_client::connection::{factory, processes}
// OUTPUT: AcpProcessManager, ProcessManagerConfig, ProcessState
// POS:    Re-exports ACP process management types from agent_client for convenience.
//! Process manager infrastructure
//! Re-exports from agent::agent_client::connection for convenience.

pub use crate::agent::agent_client::connection::factory::{AcpProcessManager, ProcessManagerConfig};
pub use crate::agent::agent_client::connection::processes::ProcessState;
