// INPUT:  protocol, connection, session, delegate, error (sub-modules)
// OUTPUT: AcpError, BootstrapPayload, ModelConfig, SandboxLevel, AcpInboundMessage, AcpOutboundMessage, PermissionData, AcpSession, AgentDelegate, AcpAgentDelegate (+ more re-exports)
// POS:    Crate root for alva-protocol-acp — re-exports protocol messages, process management, session handling, and delegate trait for external Agent CLI communication

pub mod protocol;
pub mod connection;
pub mod session;
pub mod delegate;
pub mod error;

// ---------------------------------------------------------------------------
// Public re-exports
// ---------------------------------------------------------------------------

pub use error::AcpError;

pub use protocol::bootstrap::{BootstrapPayload, ModelConfig, SandboxLevel};
pub use protocol::message::{AcpInboundMessage, AcpOutboundMessage};
pub use protocol::permission::{PermissionData, PermissionOption, PermissionRequest, RiskLevel};

pub use connection::{
    AgentCliCommand, AgentDiscovery, AcpProcessHandle, AcpProcessManager,
    ExternalAgentKind, ProcessManagerConfig, ProcessState,
};

pub use session::{AcpSession, AcpSessionState, PermissionManager};

pub use delegate::{
    AcpAgentDelegate, AgentDelegate, DelegateFinishReason, DelegateResult, DelegateToolCallSummary,
};
