// INPUT:  alva_kernel_abi, async_trait, schemars, serde
// OUTPUT: ToolSearchTool
// POS:    Searches available tools by query string.
//! tool_search — search available tools

use alva_kernel_abi::{AgentError, Tool, ToolExecutionContext, ToolOutput};
use schemars::JsonSchema;
use serde::Deserialize;

#[derive(Debug, Deserialize, JsonSchema)]
struct Input {
    /// Search query — matches against tool names and descriptions.
    query: String,
    /// Maximum number of results to return (default 5).
    #[serde(default = "default_max_results")]
    max_results: usize,
}

fn default_max_results() -> usize {
    5
}

#[derive(Tool)]
#[tool(
    name = "tool_search",
    description = "Search for available tools by keyword or name. Returns matching tools with their \
        descriptions. Use this to discover what tools are available.",
    input = Input,
    read_only,
    concurrency_safe,
)]
pub struct ToolSearchTool;

impl ToolSearchTool {
    async fn execute_impl(
        &self,
        params: Input,
        _ctx: &dyn ToolExecutionContext,
    ) -> Result<ToolOutput, AgentError> {
        // In a full implementation, this would search the ToolRegistry
        // (accessed via ctx or a shared reference) for matching tools.
        Ok(ToolOutput::text(format!(
            "Searching for tools matching '{}' (max {})...\n\
             Tool registry search is not yet wired. The tool registry must be \
             accessible from ToolExecutionContext to enable this.",
            params.query, params.max_results
        )))
    }
}

#[cfg(test)]
mod tests {
    use std::any::Any;

    use super::*;
    use alva_kernel_abi::{CancellationToken, Tool};
    use serde_json::json;

    struct TestContext {
        cancel: CancellationToken,
    }

    impl ToolExecutionContext for TestContext {
        fn cancel_token(&self) -> &CancellationToken {
            &self.cancel
        }
        fn session_id(&self) -> &str {
            "test-session"
        }
        fn as_any(&self) -> &dyn Any {
            self
        }
    }

    fn ctx() -> TestContext {
        TestContext {
            cancel: CancellationToken::new(),
        }
    }

    #[tokio::test]
    async fn echoes_query_and_max_results() {
        let tool = ToolSearchTool;
        let out = tool
            .execute(
                json!({ "query": "file", "max_results": 12usize }),
                &ctx(),
            )
            .await
            .expect("execute should succeed");

        assert!(!out.is_error);
        let text = out.model_text();
        assert!(text.contains("file"), "query missing: {text}");
        assert!(text.contains("12"), "max_results missing: {text}");
    }

    #[tokio::test]
    async fn max_results_defaults_to_five_when_omitted() {
        let tool = ToolSearchTool;
        let out = tool
            .execute(json!({ "query": "any" }), &ctx())
            .await
            .expect("execute should succeed");

        let text = out.model_text();
        assert!(
            text.contains("max 5"),
            "expected default max_results=5 to appear: {text}"
        );
    }

    #[tokio::test]
    async fn missing_query_field_returns_input_error() {
        let tool = ToolSearchTool;
        let err = tool
            .execute(json!({ "max_results": 3usize }), &ctx())
            .await
            .expect_err("missing required `query` should error");

        let msg = format!("{err}");
        assert!(
            msg.contains("invalid input") || msg.contains("query"),
            "expected invalid-input error mentioning `query`, got: {msg}"
        );
    }

    /// Stub-output contract guard (mirrors skill_tool's pattern): when
    /// ToolRegistry wiring lands, this string changes and the test gets
    /// updated in lockstep — preventing accidental silent rewires.
    #[tokio::test]
    async fn stub_text_advertises_unwired_registry() {
        let tool = ToolSearchTool;
        let out = tool
            .execute(json!({ "query": "x" }), &ctx())
            .await
            .expect("execute should succeed");
        assert!(
            out.model_text().contains("not yet wired"),
            "stub disclosure missing — if you wired the registry, update this test"
        );
    }
}
