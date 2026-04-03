// INPUT:  alva_types, async_trait, serde, serde_json
// OUTPUT: ToolSearchTool
// POS:    Searches available tools by query string.
//! tool_search — search available tools

use alva_types::{AgentError, Tool, ToolExecutionContext, ToolOutput};
use async_trait::async_trait;
use serde::Deserialize;
use serde_json::{json, Value};

#[derive(Debug, Deserialize)]
struct Input {
    query: String,
    #[serde(default = "default_max_results")]
    max_results: usize,
}

fn default_max_results() -> usize {
    5
}

pub struct ToolSearchTool;

#[async_trait]
impl Tool for ToolSearchTool {
    fn name(&self) -> &str {
        "tool_search"
    }

    fn description(&self) -> &str {
        "Search for available tools by keyword or name. Returns matching tools with their \
         descriptions. Use this to discover what tools are available."
    }

    fn parameters_schema(&self) -> Value {
        json!({
            "type": "object",
            "required": ["query"],
            "properties": {
                "query": {
                    "type": "string",
                    "description": "Search query — matches against tool names and descriptions"
                },
                "max_results": {
                    "type": "integer",
                    "description": "Maximum number of results to return (default 5)",
                    "default": 5
                }
            }
        })
    }

    fn is_read_only(&self, _input: &Value) -> bool {
        true
    }

    fn is_concurrency_safe(&self, _input: &Value) -> bool {
        true
    }

    async fn execute(
        &self,
        input: Value,
        _ctx: &dyn ToolExecutionContext,
    ) -> Result<ToolOutput, AgentError> {
        let params: Input = serde_json::from_value(input)
            .map_err(|e| AgentError::ToolError {
                tool_name: self.name().into(),
                message: e.to_string(),
            })?;

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
