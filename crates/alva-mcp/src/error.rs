// INPUT:  thiserror
// OUTPUT: McpError
// POS:    MCP-specific error types for the alva-mcp crate.
use thiserror::Error;

/// Errors specific to MCP operations.
#[derive(Debug, Error)]
pub enum McpError {
    #[error("MCP server not found: {0}")]
    ServerNotFound(String),

    #[error("MCP server not connected: {0}")]
    NotConnected(String),

    #[error("MCP connection timeout: {0}")]
    ConnectTimeout(String),

    #[error("MCP transport error: {0}")]
    Transport(String),

    #[error("IO error: {0}")]
    Io(String),

    #[error("Serialization error: {0}")]
    Serialization(String),

    #[error("Tool execution error: {0}")]
    ToolExecution(String),
}
