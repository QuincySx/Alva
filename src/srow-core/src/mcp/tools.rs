//! MCP runtime meta-tool: unified interface for listing servers, listing tools, and calling tools.

use std::sync::Arc;

use crate::domain::tool::{ToolDefinition, ToolResult};
use crate::error::EngineError;
use crate::mcp::runtime::McpManager;
use crate::ports::tool::{Tool, ToolContext};
use async_trait::async_trait;
use serde::Deserialize;
use serde_json::{json, Value};
use std::time::Instant;

#[derive(Debug, Deserialize)]
struct McpRuntimeInput {
    action: String,
    server_id: Option<String>,
    tool_name: Option<String>,
    arguments: Option<Value>,
}

/// Unified MCP operations tool.
///
/// Actions:
/// - `list_servers`: list all registered MCP servers and their states
/// - `list_tools`: list all tools from connected servers (optionally filter by server_id)
/// - `call_tool`: call a tool on a specific server
pub struct McpRuntimeTool {
    pub manager: Arc<McpManager>,
}

#[async_trait]
impl Tool for McpRuntimeTool {
    fn name(&self) -> &str {
        "mcp_runtime"
    }

    fn definition(&self) -> ToolDefinition {
        ToolDefinition {
            name: "mcp_runtime".to_string(),
            description: "Interact with MCP (Model Context Protocol) servers. Actions: 'list_servers' to see available servers, 'list_tools' to see tools from connected servers, 'call_tool' to invoke a tool on a server.".to_string(),
            parameters: json!({
                "type": "object",
                "required": ["action"],
                "properties": {
                    "action": {
                        "type": "string",
                        "enum": ["list_servers", "list_tools", "call_tool"],
                        "description": "The MCP action to perform"
                    },
                    "server_id": {
                        "type": "string",
                        "description": "Server ID (required for call_tool, optional filter for list_tools)"
                    },
                    "tool_name": {
                        "type": "string",
                        "description": "Tool name to call (required for call_tool)"
                    },
                    "arguments": {
                        "type": "object",
                        "description": "Arguments to pass to the tool (for call_tool)"
                    }
                }
            }),
        }
    }

    async fn execute(&self, input: Value, _ctx: &ToolContext) -> Result<ToolResult, EngineError> {
        let params: McpRuntimeInput =
            serde_json::from_value(input).map_err(|e| EngineError::ToolExecution(e.to_string()))?;

        let start = Instant::now();

        let output = match params.action.as_str() {
            "list_servers" => {
                let states = self.manager.server_states().await;
                let servers: Vec<Value> = states
                    .into_iter()
                    .map(|(id, state)| {
                        json!({
                            "server_id": id,
                            "state": format!("{:?}", state),
                        })
                    })
                    .collect();
                serde_json::to_string_pretty(&servers)
                    .unwrap_or_else(|_| "[]".to_string())
            }
            "list_tools" => {
                let all_tools = self.manager.list_all_tools().await;
                let filtered: Vec<Value> = all_tools
                    .iter()
                    .filter(|t| {
                        params
                            .server_id
                            .as_ref()
                            .map_or(true, |sid| t.server_id == *sid)
                    })
                    .map(|t| {
                        json!({
                            "server_id": t.server_id,
                            "tool_name": t.tool_name,
                            "description": t.description,
                        })
                    })
                    .collect();
                serde_json::to_string_pretty(&filtered)
                    .unwrap_or_else(|_| "[]".to_string())
            }
            "call_tool" => {
                let server_id = params.server_id.ok_or_else(|| {
                    EngineError::ToolExecution("server_id is required for call_tool".to_string())
                })?;
                let tool_name = params.tool_name.ok_or_else(|| {
                    EngineError::ToolExecution("tool_name is required for call_tool".to_string())
                })?;
                let arguments = params.arguments.unwrap_or(json!({}));

                let result = self
                    .manager
                    .call_tool(&server_id, &tool_name, arguments)
                    .await
                    .map_err(|e| EngineError::ToolExecution(e.to_string()))?;

                serde_json::to_string_pretty(&result)
                    .unwrap_or_else(|_| result.to_string())
            }
            other => {
                return Err(EngineError::ToolExecution(format!(
                    "Unknown action '{}'. Valid actions: list_servers, list_tools, call_tool",
                    other
                )));
            }
        };

        let duration_ms = start.elapsed().as_millis() as u64;

        Ok(ToolResult {
            tool_call_id: String::new(),
            tool_name: "mcp_runtime".to_string(),
            output,
            is_error: false,
            duration_ms,
        })
    }
}
