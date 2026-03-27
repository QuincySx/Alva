// INPUT:  thiserror
// OUTPUT: AcpError
// POS:    Unified error enum for all ACP protocol operations (spawn, permission, IO, serialization)
use thiserror::Error;

#[derive(Debug, Error)]
pub enum AcpError {
    #[error("Agent CLI not found: {kind} -- {hint}")]
    AgentNotFound { kind: String, hint: String },

    #[error("Failed to spawn agent process '{agent}': {reason}")]
    SpawnFailed { agent: String, reason: String },

    #[error("Process {pid} is no longer alive")]
    ProcessDead { pid: u32 },

    #[error("Process '{0}' not found in manager")]
    ProcessNotFound(String),

    #[error("Permission request '{0}' not found (already resolved or expired)")]
    PermissionRequestNotFound(String),

    #[error("Invalid ACP configuration: {0}")]
    InvalidConfig(String),

    #[error("Serialization error: {0}")]
    Serialization(String),

    #[error("I/O error: {0}")]
    Io(String),

    #[error("Storage error: {0}")]
    Storage(String),

    #[error("Protocol error: {0}")]
    Protocol(String),
}
