// INPUT:  std::sync, async_trait, serde_json, alva_types, crate::client, crate::types
// OUTPUT: McpToolAdapter, build_mcp_tools
// POS:    Wraps individual MCP tools as alva-types Tool trait implementations with namespaced names (mcp:server:tool).
use std::sync::Arc;

use async_trait::async_trait;
use serde_json::Value;

use alva_types::base::error::AgentError;
use alva_types::tool::Tool;
use alva_types::tool::execution::{ToolExecutionContext, ToolOutput};

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
        _ctx: &dyn ToolExecutionContext,
    ) -> Result<ToolOutput, AgentError> {
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

        Ok(ToolOutput::text(output))
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::client::McpTransportFactory;
    use crate::transport::McpTransport;
    use crate::types::McpServerConfig;

    // Dummy factory that is never actually used to connect
    struct DummyFactory;
    impl McpTransportFactory for DummyFactory {
        fn create(&self, _config: &McpServerConfig) -> Box<dyn McpTransport> {
            unimplemented!("not needed for adapter tests")
        }
    }

    fn sample_info() -> McpToolInfo {
        McpToolInfo {
            server_id: "my-server".into(),
            tool_name: "do-stuff".into(),
            description: "Does stuff".into(),
            input_schema: serde_json::json!({
                "type": "object",
                "properties": {
                    "input": { "type": "string" }
                }
            }),
        }
    }

    fn make_client() -> Arc<McpClient> {
        Arc::new(McpClient::new(Arc::new(DummyFactory)))
    }

    // ── McpToolAdapter name prefixing ───────────────────────────────────

    #[test]
    fn adapter_name_is_prefixed() {
        let adapter = McpToolAdapter::new(sample_info(), make_client());
        assert_eq!(adapter.name(), "mcp:my-server:do-stuff");
    }

    #[test]
    fn adapter_description_is_prefixed() {
        let adapter = McpToolAdapter::new(sample_info(), make_client());
        assert_eq!(adapter.description(), "[MCP:my-server] Does stuff");
    }

    #[test]
    fn adapter_parameters_schema_returns_input_schema() {
        let info = sample_info();
        let expected = info.input_schema.clone();
        let adapter = McpToolAdapter::new(info, make_client());
        assert_eq!(adapter.parameters_schema(), expected);
    }

    // ── tool_name static helper ─────────────────────────────────────────

    #[test]
    fn tool_name_helper_format() {
        let name = McpToolAdapter::tool_name("srv", "my-tool");
        assert_eq!(name, "mcp:srv:my-tool");
    }

    // ── build_mcp_tools ─────────────────────────────────────────────────

    #[test]
    fn build_mcp_tools_creates_correct_count() {
        let client = make_client();
        let infos = vec![
            McpToolInfo {
                server_id: "s1".into(),
                tool_name: "t1".into(),
                description: "d1".into(),
                input_schema: serde_json::json!({}),
            },
            McpToolInfo {
                server_id: "s1".into(),
                tool_name: "t2".into(),
                description: "d2".into(),
                input_schema: serde_json::json!({}),
            },
            McpToolInfo {
                server_id: "s2".into(),
                tool_name: "t3".into(),
                description: "d3".into(),
                input_schema: serde_json::json!({}),
            },
        ];

        let tools = build_mcp_tools(client, infos);
        assert_eq!(tools.len(), 3);
        assert_eq!(tools[0].name(), "mcp:s1:t1");
        assert_eq!(tools[1].name(), "mcp:s1:t2");
        assert_eq!(tools[2].name(), "mcp:s2:t3");
    }

    #[test]
    fn build_mcp_tools_empty_input() {
        let tools = build_mcp_tools(make_client(), vec![]);
        assert!(tools.is_empty());
    }

    // ── Tool trait impl ─────────────────────────────────────────────────

    #[test]
    fn adapter_implements_tool_definition() {
        let adapter = McpToolAdapter::new(sample_info(), make_client());
        let def = adapter.definition();
        assert_eq!(def.name, "mcp:my-server:do-stuff");
        assert_eq!(def.description, "[MCP:my-server] Does stuff");
    }
}
