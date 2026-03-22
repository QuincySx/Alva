// protocol-agent-client: Standalone ACP (Agent Client Protocol) crate.
//
// Provides protocol message types, process management, session handling, and delegate trait
// for communicating with external Agent CLI processes over stdin/stdout JSON lines.

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
