use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// MCP Server transport protocol type
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", tag = "type")]
pub enum McpTransportConfig {
    /// stdio transport (child process stdin/stdout)
    Stdio {
        /// Executable path
        command: String,
        /// Command-line arguments
        args: Vec<String>,
        /// Environment variables to inject
        env: HashMap<String, String>,
    },
    /// SSE transport (HTTP Server-Sent Events)
    Sse {
        /// SSE endpoint URL, e.g. "http://127.0.0.1:3000/sse"
        url: String,
        /// Extra HTTP headers (e.g. Authorization)
        headers: HashMap<String, String>,
    },
}

/// Complete configuration for a single MCP Server
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct McpServerConfig {
    /// Unique identifier, kebab-case, e.g. "builtin-browser"
    pub id: String,
    /// Human-readable name for UI display
    pub display_name: String,
    pub transport: McpTransportConfig,
    /// Whether to auto-connect on Agent startup
    pub auto_connect: bool,
    /// Connection timeout in seconds
    pub connect_timeout_secs: u32,
}

/// MCP Server runtime state
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum McpServerState {
    /// Not connected (initial state)
    Disconnected,
    /// Connecting in progress
    Connecting,
    /// Connected, tools have been enumerated
    Connected {
        /// Number of tools exposed by this Server
        tool_count: usize,
    },
    /// Connection failed
    Failed { reason: String },
}

/// MCP Tool description (enumerated from server)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct McpToolInfo {
    pub server_id: String,
    pub tool_name: String,
    pub description: String,
    /// Parameter definition in JSON Schema format
    pub input_schema: serde_json::Value,
}
