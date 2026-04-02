// INPUT:  discovery, process, manager, orphan (sub-modules)
// OUTPUT: re-exports all public types from sub-modules
// POS:    Module root for connection — agent discovery, process handles, process manager, and orphan cleanup

mod discovery;
mod manager;
mod orphan;
mod process;

pub use discovery::{AgentCliCommand, AgentDiscovery, ExternalAgentKind};
pub use manager::{AcpProcessManager, ProcessManagerConfig};
pub use orphan::{cleanup_orphan_processes, parent_pid_env_value, PARENT_PID_ENV};
pub use process::{AcpProcessHandle, ProcessState};
