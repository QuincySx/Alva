// INPUT:  serde, std::collections
// OUTPUT: McpTransportConfig, McpServerConfig, McpServerState, McpToolInfo
// POS:    Core MCP protocol types — server configuration, runtime state, and tool descriptions.
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// MCP Server transport protocol type.
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

/// Complete configuration for a single MCP Server.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct McpServerConfig {
    /// Unique identifier, kebab-case, e.g. "builtin-browser"
    pub id: String,
    /// Human-readable name for UI display
    pub display_name: String,
    pub transport: McpTransportConfig,
    /// Whether to auto-connect on startup
    pub auto_connect: bool,
    /// Connection timeout in seconds
    pub connect_timeout_secs: u32,
}

/// MCP Server runtime state.
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

/// MCP Tool description (enumerated from server).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct McpToolInfo {
    pub server_id: String,
    pub tool_name: String,
    pub description: String,
    /// Parameter definition in JSON Schema format
    pub input_schema: serde_json::Value,
}

#[cfg(test)]
mod tests {
    use super::*;

    // ── McpTransportConfig ──────────────────────────────────────────────

    #[test]
    fn transport_stdio_construction_and_serde_roundtrip() {
        let transport = McpTransportConfig::Stdio {
            command: "npx".into(),
            args: vec!["-y".into(), "@modelcontextprotocol/server".into()],
            env: HashMap::from([("NODE_ENV".into(), "production".into())]),
        };

        let json = serde_json::to_string(&transport).unwrap();
        let parsed: McpTransportConfig = serde_json::from_str(&json).unwrap();

        assert_eq!(transport, parsed);
        assert!(json.contains("\"type\":\"stdio\""));
    }

    #[test]
    fn transport_sse_construction_and_serde_roundtrip() {
        let transport = McpTransportConfig::Sse {
            url: "http://127.0.0.1:3000/sse".into(),
            headers: HashMap::from([("Authorization".into(), "Bearer tok".into())]),
        };

        let json = serde_json::to_string(&transport).unwrap();
        let parsed: McpTransportConfig = serde_json::from_str(&json).unwrap();

        assert_eq!(transport, parsed);
        assert!(json.contains("\"type\":\"sse\""));
    }

    #[test]
    fn transport_stdio_empty_env_and_args() {
        let transport = McpTransportConfig::Stdio {
            command: "echo".into(),
            args: vec![],
            env: HashMap::new(),
        };

        let json = serde_json::to_string(&transport).unwrap();
        let parsed: McpTransportConfig = serde_json::from_str(&json).unwrap();
        assert_eq!(transport, parsed);
    }

    // ── McpServerConfig ─────────────────────────────────────────────────

    #[test]
    fn server_config_construction_and_serde_roundtrip() {
        let cfg = McpServerConfig {
            id: "builtin-browser".into(),
            display_name: "Browser MCP".into(),
            transport: McpTransportConfig::Stdio {
                command: "node".into(),
                args: vec!["server.js".into()],
                env: HashMap::new(),
            },
            auto_connect: true,
            connect_timeout_secs: 30,
        };

        let json = serde_json::to_string_pretty(&cfg).unwrap();
        let parsed: McpServerConfig = serde_json::from_str(&json).unwrap();

        assert_eq!(parsed.id, "builtin-browser");
        assert_eq!(parsed.display_name, "Browser MCP");
        assert!(parsed.auto_connect);
        assert_eq!(parsed.connect_timeout_secs, 30);
    }

    // ── McpServerState ──────────────────────────────────────────────────

    #[test]
    fn server_state_disconnected() {
        let state = McpServerState::Disconnected;
        let json = serde_json::to_string(&state).unwrap();
        let parsed: McpServerState = serde_json::from_str(&json).unwrap();
        assert_eq!(state, parsed);
    }

    #[test]
    fn server_state_connecting() {
        let state = McpServerState::Connecting;
        let json = serde_json::to_string(&state).unwrap();
        let parsed: McpServerState = serde_json::from_str(&json).unwrap();
        assert_eq!(state, parsed);
    }

    #[test]
    fn server_state_connected_with_tool_count() {
        let state = McpServerState::Connected { tool_count: 5 };
        let json = serde_json::to_string(&state).unwrap();
        let parsed: McpServerState = serde_json::from_str(&json).unwrap();
        assert_eq!(state, parsed);
    }

    #[test]
    fn server_state_failed_with_reason() {
        let state = McpServerState::Failed {
            reason: "connection refused".into(),
        };
        let json = serde_json::to_string(&state).unwrap();
        let parsed: McpServerState = serde_json::from_str(&json).unwrap();
        assert_eq!(state, parsed);
    }

    // ── McpToolInfo ─────────────────────────────────────────────────────

    #[test]
    fn tool_info_construction_and_serde_roundtrip() {
        let info = McpToolInfo {
            server_id: "browser".into(),
            tool_name: "screenshot".into(),
            description: "Take a screenshot of the current page".into(),
            input_schema: serde_json::json!({
                "type": "object",
                "properties": {
                    "url": { "type": "string" }
                }
            }),
        };

        let json = serde_json::to_string(&info).unwrap();
        let parsed: McpToolInfo = serde_json::from_str(&json).unwrap();

        assert_eq!(parsed.server_id, "browser");
        assert_eq!(parsed.tool_name, "screenshot");
        assert_eq!(parsed.description, "Take a screenshot of the current page");
        assert!(parsed.input_schema["properties"]["url"]["type"]
            .as_str()
            .unwrap()
            == "string");
    }
}
