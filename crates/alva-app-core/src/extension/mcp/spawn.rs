// INPUT: Kernel AgentSession/Tool contracts and app-owned MCP snapshot assembly
// OUTPUT: McpSpawnPolicy and a session-bound McpGatewayBinding
// POS: Optional product MCP rebinding for explicitly granted recursive child gateways.
use std::sync::Arc;

use alva_kernel_abi::{bus_cap, AgentError, AgentSession, Tool};
use async_trait::async_trait;

pub struct McpGatewayBinding {
    pub gateway: Arc<dyn Tool>,
    pub system_prompt: String,
}

/// Bus Capability: create an own-session recorded MCP gateway for a granted child.
///
/// **Provider**: the product's managed MCP recipe, during root registration.
/// **Consumers**: app-core AgentSpawnTool after selecting the direct parent's grants.
/// **Why bus**: the native journal authority lives above app-core; recursive
/// spawn discovers its optional factory without reverse app-to-engine dependencies.
/// This is not a general child builder or runtime capability registration.
#[bus_cap]
#[async_trait]
pub trait McpSpawnPolicy: Send + Sync {
    /// Caller must already grant mcp_runtime and have the shared SecurityGuard.
    /// Implementations use only the approved snapshot/cwd, never ambient config.
    async fn bind(&self, session: Arc<dyn AgentSession>) -> Result<McpGatewayBinding, AgentError>;
}
