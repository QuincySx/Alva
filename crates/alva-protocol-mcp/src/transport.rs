// INPUT:  crate::types, crate::error, async_trait, serde_json
// OUTPUT: McpTransport and optional backpressured McpIoObserver contracts
// POS:    Abstract MCP transport layer hiding stdio/SSE protocol differences.
use crate::error::McpError;
use crate::types::McpToolInfo;
use async_trait::async_trait;
use serde_json::Value;

/// Optional host observer. Persistence, redaction and scope policy belong to
/// the harness, not the protocol. An error stops the interaction, never drops
/// a frame silently. Returned identity is a host-owned durable reference.
#[async_trait]
pub trait McpIoObserver: Send + Sync {
    async fn record(&self, server: &str, direction: &str, frame: Value) -> Result<String, McpError>;
}

/// MCP transport layer abstraction: hides stdio / SSE differences.
///
/// Implementations handle the wire protocol for a specific transport
/// (e.g. spawning a child process for stdio, or HTTP for SSE).
#[async_trait]
pub trait McpTransport: Send + Sync {
    /// Establish connection and complete MCP handshake (initialize).
    async fn connect(&mut self) -> Result<(), McpError>;

    /// Disconnect from the server.
    async fn disconnect(&mut self) -> Result<(), McpError>;

    /// Whether the transport is currently connected.
    fn is_connected(&self) -> bool;

    /// List all tools exposed by this Server.
    async fn list_tools(&self) -> Result<Vec<McpToolInfo>, McpError>;

    /// Call a tool by name with the given arguments.
    async fn call_tool(&self, tool_name: &str, arguments: Value) -> Result<Value, McpError>;
}
