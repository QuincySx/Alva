// INPUT:  thiserror
// OUTPUT: McpError
// POS:    MCP-specific error types for the alva-protocol-mcp crate.
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn error_server_not_found_display() {
        let err = McpError::ServerNotFound("my-server".into());
        assert_eq!(err.to_string(), "MCP server not found: my-server");
    }

    #[test]
    fn error_not_connected_display() {
        let err = McpError::NotConnected("my-server".into());
        assert_eq!(err.to_string(), "MCP server not connected: my-server");
    }

    #[test]
    fn error_connect_timeout_display() {
        let err = McpError::ConnectTimeout("my-server".into());
        assert_eq!(err.to_string(), "MCP connection timeout: my-server");
    }

    #[test]
    fn error_transport_display() {
        let err = McpError::Transport("broken pipe".into());
        assert_eq!(err.to_string(), "MCP transport error: broken pipe");
    }

    #[test]
    fn error_io_display() {
        let err = McpError::Io("file not found".into());
        assert_eq!(err.to_string(), "IO error: file not found");
    }

    #[test]
    fn error_serialization_display() {
        let err = McpError::Serialization("unexpected EOF".into());
        assert_eq!(err.to_string(), "Serialization error: unexpected EOF");
    }

    #[test]
    fn error_tool_execution_display() {
        let err = McpError::ToolExecution("tool crashed".into());
        assert_eq!(err.to_string(), "Tool execution error: tool crashed");
    }

    #[test]
    fn all_variants_are_debug() {
        // Ensure Debug is implemented for all variants (compile-time + runtime check)
        let variants: Vec<McpError> = vec![
            McpError::ServerNotFound("a".into()),
            McpError::NotConnected("b".into()),
            McpError::ConnectTimeout("c".into()),
            McpError::Transport("d".into()),
            McpError::Io("e".into()),
            McpError::Serialization("f".into()),
            McpError::ToolExecution("g".into()),
        ];
        for v in &variants {
            let debug = format!("{:?}", v);
            assert!(!debug.is_empty());
        }
    }
}
