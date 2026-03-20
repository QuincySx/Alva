pub mod delegate;
pub mod process;
pub mod protocol;
pub mod session;
pub mod storage;

// ---------------------------------------------------------------------------
// Error type
// ---------------------------------------------------------------------------

use thiserror::Error;

#[derive(Debug, Error)]
pub enum AcpError {
    #[error("Agent CLI not found: {kind} — {hint}")]
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

// Allow AcpError -> EngineError conversion
impl From<AcpError> for crate::error::EngineError {
    fn from(e: AcpError) -> Self {
        crate::error::EngineError::ToolExecution(e.to_string())
    }
}

// ---------------------------------------------------------------------------
// Public re-exports
// ---------------------------------------------------------------------------

pub use protocol::bootstrap::{BootstrapPayload, ModelConfig, SandboxLevel};
pub use protocol::message::{AcpInboundMessage, AcpOutboundMessage};
pub use protocol::permission::{PermissionData, PermissionOption, PermissionRequest, RiskLevel};

pub use process::discovery::ExternalAgentKind;
pub use process::handle::ProcessState;
pub use process::manager::{AcpProcessManager, ProcessManagerConfig};

pub use session::permission_manager::PermissionManager;
pub use session::session::{AcpSession, AcpSessionState};

pub use delegate::{
    AcpAgentDelegate, AcpDelegateTool, AgentDelegate, DelegateFinishReason, DelegateResult,
};
