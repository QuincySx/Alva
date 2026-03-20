use crate::domain::mcp::McpToolInfo;
use crate::error::SkillError;
use async_trait::async_trait;
use serde_json::Value;

/// MCP transport layer abstraction: hides stdio / SSE differences
#[async_trait]
pub trait McpTransport: Send + Sync {
    /// Establish connection and complete MCP handshake (initialize)
    async fn connect(&mut self) -> Result<(), SkillError>;

    /// Disconnect
    async fn disconnect(&mut self) -> Result<(), SkillError>;

    /// Whether connected
    fn is_connected(&self) -> bool;

    /// List all tools exposed by this Server
    async fn list_tools(&self) -> Result<Vec<McpToolInfo>, SkillError>;

    /// Call a tool
    async fn call_tool(
        &self,
        tool_name: &str,
        arguments: Value,
    ) -> Result<Value, SkillError>;
}
