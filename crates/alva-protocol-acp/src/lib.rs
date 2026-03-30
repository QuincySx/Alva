// INPUT:  protocol, connection, session, delegate, error (sub-modules)
// OUTPUT: AcpError, BootstrapPayload, ModelConfig, SandboxLevel, AcpInboundMessage, AcpOutboundMessage, PermissionData, AcpSession, AgentDelegate, AcpAgentDelegate (+ more re-exports)
// POS:    Crate root for alva-protocol-acp — re-exports protocol messages, process management, session handling, and delegate trait for external Agent CLI communication
//
// ⚠️ DEPRECATED: This crate uses a custom protocol that does NOT conform to the
// official ACP (Agent Client Protocol) specification by Zed/JetBrains.
// See: https://github.com/agentclientprotocol/agent-client-protocol
//
// The official ACP uses JSON-RPC 2.0 with methods like `initialize`, `session/prompt`,
// `session/update`, `fs/read_text_file`, etc. This crate uses a custom envelope format
// (`acp_event_type` tag) and a BootstrapPayload approach designed for parent→child
// agent spawning, not editor↔agent communication.
//
// Plan: Rewrite this crate to implement the official ACP spec with:
//   - `acp::server` — Engine side (accepts editor connections)
//   - `acp::client` — Agent side (calls other ACP-compatible agents)
// Until then, do not build new features on this crate.

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
