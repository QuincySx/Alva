use std::sync::Arc;

use async_trait::async_trait;
use serde_json::Value;

use crate::domain::tool::ToolResult;
use crate::error::EngineError;
use crate::domain::tool::ToolDefinition;
use crate::ports::tool::{Tool, ToolContext};

use crate::mcp::runtime::McpManager;
use crate::skills::skill_domain::mcp::McpToolInfo;

/// Wraps a single MCP Tool as a srow_core Tool trait implementation
///
/// Tool name format: `mcp:<server_id>:<tool_name>`
/// This way MCP tools coexist with built-in tools in AgentEngine's ToolRegistry,
/// and Agent calls them via standard tool_call mechanism without knowing MCP details.
pub struct McpToolAdapter {
    info: McpToolInfo,
    manager: Arc<McpManager>,
    /// Cached full tool name: "mcp:<server_id>:<tool_name>"
    full_name: String,
}

impl McpToolAdapter {
    pub fn new(info: McpToolInfo, manager: Arc<McpManager>) -> Self {
        let full_name = format!("mcp:{}:{}", info.server_id, info.tool_name);
        Self {
            info,
            manager,
            full_name,
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

    fn definition(&self) -> ToolDefinition {
        ToolDefinition {
            name: self.full_name.clone(),
            description: format!(
                "[MCP:{}] {}",
                self.info.server_id, self.info.description
            ),
            parameters: self.info.input_schema.clone(),
        }
    }

    async fn execute(
        &self,
        input: Value,
        _ctx: &ToolContext,
    ) -> Result<ToolResult, EngineError> {
        let start = std::time::Instant::now();

        let result = self
            .manager
            .call_tool(&self.info.server_id, &self.info.tool_name, input)
            .await
            .map_err(|e| EngineError::ToolExecution(e.to_string()))?;

        let output =
            serde_json::to_string_pretty(&result).unwrap_or_else(|_| result.to_string());

        Ok(ToolResult {
            tool_call_id: String::new(), // Filled by engine layer
            tool_name: self.full_name.clone(),
            output,
            is_error: false,
            duration_ms: start.elapsed().as_millis() as u64,
        })
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
