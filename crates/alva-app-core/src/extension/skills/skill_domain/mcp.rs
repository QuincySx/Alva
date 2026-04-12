// INPUT:  alva_protocol_mcp::types
// OUTPUT: pub McpServerConfig, McpServerState, McpToolInfo, McpTransportConfig
// POS:    Re-exports MCP server and tool types from alva-protocol-mcp.
// Re-export from protocol crate — single source of truth
pub use alva_protocol_mcp::types::{McpServerConfig, McpServerState, McpToolInfo, McpTransportConfig};
