// INPUT:  std::sync, async_trait, serde_json, alva_types, crate::client, crate::types
// OUTPUT: McpToolAdapter, build_mcp_tools
// POS:    Wraps individual MCP tools as alva-types Tool trait implementations with namespaced names (mcp:server:tool).
use std::sync::Arc;

use async_trait::async_trait;
use serde_json::Value;

use alva_types::cancel::CancellationToken;
use alva_types::error::AgentError;
use alva_types::tool::{Tool, ToolContext, ToolResult};

use crate::client::McpClient;
use crate::types::McpToolInfo;

/// Wraps a single MCP Tool as an alva-types Tool trait implementation.
///
/// Tool name format: `mcp:<server_id>:<tool_name>`
/// This way MCP tools coexist with built-in tools in a ToolRegistry,
/// and Agent calls them via standard tool_call mechanism without knowing MCP details.
pub struct McpToolAdapter {
    info: McpToolInfo,
    client: Arc<McpClient>,
    /// Cached full tool name: "mcp:<server_id>:<tool_name>"
    full_name: String,
    /// Cached description
    full_description: String,
}

impl McpToolAdapter {
    pub fn new(info: McpToolInfo, client: Arc<McpClient>) -> Self {
        let full_name = format!("mcp:{}:{}", info.server_id, info.tool_name);
        let full_description = format!("[MCP:{}] {}", info.server_id, info.description);
        Self {
            info,
            client,
            full_name,
            full_description,
        }
    }

    /// Generate the engine-internal tool name.
    pub fn tool_name(server_id: &str, tool_name: &str) -> String {
        format!("mcp:{}:{}", server_id, tool_name)
    }
}

#[async_trait]
impl Tool for McpToolAdapter {
    fn name(&self) -> &str {
        &self.full_name
    }

    fn description(&self) -> &str {
        &self.full_description
    }

    fn parameters_schema(&self) -> Value {
        self.info.input_schema.clone()
    }

    async fn execute(
        &self,
        input: Value,
        _cancel: &CancellationToken,
        _ctx: &dyn ToolContext,
    ) -> Result<ToolResult, AgentError> {
        let result = self
            .client
            .call_tool(&self.info.server_id, &self.info.tool_name, input)
            .await
            .map_err(|e| AgentError::ToolError {
                tool_name: self.full_name.clone(),
                message: e.to_string(),
            })?;

        let output =
            serde_json::to_string_pretty(&result).unwrap_or_else(|_| result.to_string());

        Ok(ToolResult {
            content: output,
            is_error: false,
            details: None,
        })
    }
}

/// Convert all connected tools from McpClient into Tool list.
pub fn build_mcp_tools(
    client: Arc<McpClient>,
    tools_info: Vec<McpToolInfo>,
) -> Vec<Box<dyn Tool>> {
    tools_info
        .into_iter()
        .map(|info| -> Box<dyn Tool> {
            Box::new(McpToolAdapter::new(info, client.clone()))
        })
        .collect()
}
