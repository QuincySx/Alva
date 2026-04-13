// INPUT:  std::sync, async_trait, serde_json, alva_kernel_abi, crate::extension::mcp::runtime, crate::extension::skills::skill_domain::mcp
// OUTPUT: McpToolAdapter, build_mcp_tools
// POS:    Wraps individual MCP tools as standard Tool trait implementations with namespaced names (mcp:server:tool).
use std::sync::Arc;

use alva_kernel_abi::{AgentError, Tool, ToolExecutionContext, ToolOutput};
use async_trait::async_trait;
use serde_json::Value;

use crate::extension::mcp::runtime::McpManager;
use crate::extension::skills::skill_domain::mcp::McpToolInfo;

/// Wraps a single MCP Tool as a alva_app_core Tool trait implementation
///
/// Tool name format: `mcp:<server_id>:<tool_name>`
/// This way MCP tools coexist with built-in tools in AgentEngine's ToolRegistry,
/// and Agent calls them via standard tool_call mechanism without knowing MCP details.
pub struct McpToolAdapter {
    info: McpToolInfo,
    manager: Arc<McpManager>,
    /// Cached full tool name: "mcp:<server_id>:<tool_name>"
    full_name: String,
    /// Cached description with server prefix
    full_description: String,
}

impl McpToolAdapter {
    pub fn new(info: McpToolInfo, manager: Arc<McpManager>) -> Self {
        let full_name = format!("mcp:{}:{}", info.server_id, info.tool_name);
        let full_description = format!("[MCP:{}] {}", info.server_id, info.description);
        Self {
            info,
            manager,
            full_name,
            full_description,
        }
    }

    /// Generate the engine-internal tool name
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
        _ctx: &dyn ToolExecutionContext,
    ) -> Result<ToolOutput, AgentError> {
        let result = self
            .manager
            .call_tool(&self.info.server_id, &self.info.tool_name, input)
            .await
            .map_err(|e| AgentError::ToolError { tool_name: self.full_name.clone(), message: e.to_string() })?;

        let output =
            serde_json::to_string_pretty(&result).unwrap_or_else(|_| result.to_string());

        Ok(ToolOutput::text(output))
    }
}

/// Convert all connected tools from McpManager into Tool list
pub fn build_mcp_tools(
    manager: Arc<McpManager>,
    tools_info: Vec<McpToolInfo>,
) -> Vec<Box<dyn Tool>> {
    tools_info
        .into_iter()
        .map(|info| -> Box<dyn Tool> {
            Box::new(McpToolAdapter::new(info, manager.clone()))
        })
        .collect()
}
