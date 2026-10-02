// INPUT:  std::sync, async_trait, serde_json, sha2, alva_kernel_abi, crate::client, crate::types
// OUTPUT: McpToolAdapter, build_mcp_tools
// POS:    MCP tool adapters with stable provider-safe aliases and explicit logical server/tool identity.
use std::sync::Arc;

use async_trait::async_trait;
use serde_json::Value;
use sha2::{Digest, Sha256};

use alva_kernel_abi::base::error::AgentError;
use alva_kernel_abi::tool::execution::{ToolExecutionContext, ToolOutput};
use alva_kernel_abi::tool::Tool;

use crate::client::McpClient;
use crate::types::McpToolInfo;

/// Wraps a single MCP Tool as an alva-kernel-abi Tool trait implementation.
///
/// Logical identity is `mcp:<server_id>:<tool_name>` in the description.
/// LLM-facing names use stable ASCII aliases (at most 64 bytes), because colon,
/// dots and unbounded concatenated names are not valid across model providers.
pub struct McpToolAdapter {
    info: McpToolInfo,
    client: Arc<McpClient>,
    /// Cached provider-safe tool alias.
    full_name: String,
    /// Cached description
    full_description: String,
}

impl McpToolAdapter {
    pub fn new(info: McpToolInfo, client: Arc<McpClient>) -> Self {
        let full_name = Self::tool_name(&info.server_id, &info.tool_name);
        let full_description = format!(
            "[MCP:{}:{}] {}",
            info.server_id, info.tool_name, info.description
        );
        Self {
            info,
            client,
            full_name,
            full_description,
        }
    }

    /// Generate the stable provider-safe alias; length-prefix identities to
    /// avoid ambiguous concatenations. The 240-bit digest fits the strictest
    /// provider's 64-byte limit, including the reserved `mcp_` prefix.
    pub fn tool_name(server_id: &str, tool_name: &str) -> String {
        let mut digest = Sha256::new();
        digest.update((server_id.len() as u64).to_be_bytes());
        digest.update(server_id.as_bytes());
        digest.update(tool_name.as_bytes());
        let mut alias = String::from("mcp_");
        for byte in &digest.finalize()[..30] {
            use std::fmt::Write;
            write!(alias, "{byte:02x}").expect("writing to String cannot fail");
        }
        alias
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

        let output = serde_json::to_string_pretty(&result).unwrap_or_else(|_| result.to_string());

        Ok(ToolOutput::text(output))
    }
}

/// Convert all connected tools from McpClient into Tool list.
pub fn build_mcp_tools(client: Arc<McpClient>, tools_info: Vec<McpToolInfo>) -> Vec<Box<dyn Tool>> {
    tools_info
        .into_iter()
        .map(|info| -> Box<dyn Tool> { Box::new(McpToolAdapter::new(info, client.clone())) })
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
        assert_eq!(
            adapter.name(),
            McpToolAdapter::tool_name("my-server", "do-stuff")
        );
    }

    #[test]
    fn adapter_description_is_prefixed() {
        let adapter = McpToolAdapter::new(sample_info(), make_client());
        assert_eq!(adapter.description(), "[MCP:my-server:do-stuff] Does stuff");
    }

    #[test]
    fn adapter_parameters_schema_returns_input_schema() {
        let info = sample_info();
        let expected = info.input_schema.clone();
        let adapter = McpToolAdapter::new(info, make_client());
        assert_eq!(
            adapter.parameters_schema(&alva_kernel_abi::ToolSchemaContext::empty()),
            expected
        );
    }

    // ── tool_name static helper ─────────────────────────────────────────

    #[test]
    fn tool_name_helper_format() {
        let name = McpToolAdapter::tool_name("srv", "my-tool");
        assert!(name.starts_with("mcp_"));
        assert_eq!(name.len(), 64);
        assert!(name
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-'));
        assert_eq!(name, McpToolAdapter::tool_name("srv", "my-tool"));
        assert_ne!(name, McpToolAdapter::tool_name("srv", "my_tool"));
        assert_ne!(
            McpToolAdapter::tool_name("a:b", "c"),
            McpToolAdapter::tool_name("a", "b:c")
        );
        let long = McpToolAdapter::tool_name(&"服务".repeat(100), &"tool.name".repeat(100));
        assert_eq!(long.len(), 64);
        assert!(long.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_'));
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
        assert_eq!(tools[0].name(), McpToolAdapter::tool_name("s1", "t1"));
        assert_eq!(tools[1].name(), McpToolAdapter::tool_name("s1", "t2"));
        assert_eq!(tools[2].name(), McpToolAdapter::tool_name("s2", "t3"));
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
        let def = alva_kernel_abi::tool_definition(
            &adapter,
            &alva_kernel_abi::ToolSchemaContext::empty(),
        );
        assert_eq!(def.name, McpToolAdapter::tool_name("my-server", "do-stuff"));
        assert_eq!(def.description, "[MCP:my-server:do-stuff] Does stuff");
    }
}
