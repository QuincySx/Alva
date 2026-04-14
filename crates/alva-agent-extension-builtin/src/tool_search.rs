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
