// INPUT:  std::sync, async_trait, serde_json, alva_kernel_abi, crate::extension::mcp::runtime, crate::extension::skills::skill_domain::mcp
// OUTPUT: McpToolAdapter, build_mcp_tools
// POS:    MCP tools as standard Tools with shared provider-safe aliases and host-access effect descriptions.
use std::sync::Arc;

use alva_kernel_abi::{AgentError, Tool, ToolExecutionContext, ToolOutput};
use async_trait::async_trait;
use serde_json::Value;

use crate::extension::mcp::runtime::McpManager;
use crate::extension::skills::skill_domain::mcp::McpToolInfo;

/// Wraps a single MCP Tool as a alva_app_core Tool trait implementation
///
/// Logical `mcp:<server_id>:<tool_name>` identity is retained in descriptions;
/// names share the protocol adapter's stable provider-safe ASCII alias.
pub struct McpToolAdapter {
    info: McpToolInfo,
    manager: Arc<McpManager>,
    /// Cached provider-safe tool alias.
    full_name: String,
    /// Cached description with server prefix
    full_description: String,
}

impl McpToolAdapter {
    pub fn new(info: McpToolInfo, manager: Arc<McpManager>) -> Self {
        let full_name = Self::tool_name(&info.server_id, &info.tool_name);
        let full_description = format!(
            "[MCP:{}:{}] {}",
            info.server_id, info.tool_name, info.description
        );
        Self {
            info,
            manager,
            full_name,
            full_description,
        }
    }

    /// Generate the engine-internal tool name
    pub fn tool_name(server_id: &str, tool_name: &str) -> String {
        alva_protocol_mcp::tool_adapter::McpToolAdapter::tool_name(server_id, tool_name)
    }

    /// Keep existing exact logical names in Agent templates compatible with
    /// provider-safe aliases. Wildcard patterns are not expanded or granted.
    pub(crate) fn resolve_tool_name(name: &str) -> String {
        if let Some((server, tool)) = name.strip_prefix("mcp:").and_then(|s| s.split_once(':')) {
            if !server.is_empty() && !tool.is_empty() && !name.contains('*') {
                return Self::tool_name(server, tool);
            }
        }
        name.into()
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

    fn parameters_schema(&self, _ctx: &alva_kernel_abi::ToolSchemaContext) -> Value {
        self.info.input_schema.clone()
    }

    fn describe_effect(&self, _input: &Value) -> alva_kernel_abi::EffectDescription {
        alva_kernel_abi::EffectDescription::of(format!(
            "Call external MCP server '{}' tool '{}'; the server runs with host access",
            self.info.server_id, self.info.tool_name
        ))
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
            .map_err(|e| AgentError::ToolError {
                tool_name: self.full_name.clone(),
                message: e.to_string(),
            })?;

        let output = serde_json::to_string_pretty(&result).unwrap_or_else(|_| result.to_string());

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
        .map(|info| -> Box<dyn Tool> { Box::new(McpToolAdapter::new(info, manager.clone())) })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn template_logical_names_resolve_without_expanding_permissions() {
        let alias = McpToolAdapter::tool_name("fixture", "echo");
        assert_eq!(McpToolAdapter::resolve_tool_name("mcp:fixture:echo"), alias);
        assert_eq!(McpToolAdapter::resolve_tool_name(&alias), alias);
        for unchanged in ["read_file", "mcp:fixture:*", "mcp::echo", "mcp:fixture:"] {
            assert_eq!(McpToolAdapter::resolve_tool_name(unchanged), unchanged);
        }
    }
}
