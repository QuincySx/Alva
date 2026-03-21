// INPUT:  crate::agent::agent_client::protocol::{message, bootstrap, permission, content}
// OUTPUT: AcpInboundMessage, AcpOutboundMessage, BootstrapPayload, ModelConfig, SandboxLevel, PermissionData, PermissionOption, PermissionRequest, RiskLevel, ContentBlock
// POS:    Flat re-export of ACP protocol message types for convenience.
//! ACP message types — re-exported from agent::agent_client::protocol
//! This module provides a convenient flat import path for ACP types.

pub use crate::agent::agent_client::protocol::message::{AcpInboundMessage, AcpOutboundMessage};
pub use crate::agent::agent_client::protocol::bootstrap::{BootstrapPayload, ModelConfig, SandboxLevel};
pub use crate::agent::agent_client::protocol::permission::{PermissionData, PermissionOption, PermissionRequest, RiskLevel};
pub use crate::agent::agent_client::protocol::content::ContentBlock;
