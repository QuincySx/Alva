// INPUT:  async_trait, serde_json, alva_kernel_abi::tool::{Tool, execution::{ToolExecutionContext, ToolOutput}}, alva_kernel_abi::base::error::AgentError
// OUTPUT: RawTool
// POS:    Passthrough Tool that carries a ToolDefinition (name/description/schema) but never executes.
//         The gateway forwards tool definitions upstream and relays tool-call outputs back to clients;
//         it never runs tools itself. If execute() is somehow called, it returns AgentError::ToolError.

use alva_kernel_abi::base::error::AgentError;
use alva_kernel_abi::tool::execution::{ToolExecutionContext, ToolOutput};
use alva_kernel_abi::tool::Tool;
use async_trait::async_trait;

/// A non-executable [`Tool`] that carries only a definition (name / description / schema).
///
/// The gateway forwards tool definitions to upstreams and relays tool-call outputs back to
/// clients; it never runs tools, so `execute` returns an error.  Constructing a `RawTool`
/// from a [`alva_kernel_abi::tool::types::ToolDefinition`] lets the gateway satisfy the
/// `&[&dyn Tool]` parameter of `LanguageModel::complete` / `stream` without wiring up
/// real tool implementations.
pub struct RawTool {
    name: String,
    description: String,
    schema: serde_json::Value,
}

impl RawTool {
    /// Create a new passthrough tool from raw parts.
    pub fn new(name: String, description: String, schema: serde_json::Value) -> Self {
        Self {
            name,
            description,
            schema,
        }
    }
}

#[async_trait]
impl Tool for RawTool {
    fn name(&self) -> &str {
        &self.name
    }

    fn description(&self) -> &str {
        &self.description
    }

    fn parameters_schema(&self) -> serde_json::Value {
        self.schema.clone()
    }

    /// Always returns an error — the gateway never executes tools locally.
    async fn execute(
        &self,
        _input: serde_json::Value,
        _ctx: &dyn ToolExecutionContext,
    ) -> Result<ToolOutput, AgentError> {
        Err(AgentError::ToolError {
            tool_name: self.name.clone(),
            message: "gateway RawTool is passthrough-only; execute must not be called".into(),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use alva_kernel_abi::tool::Tool;

    #[test]
    fn raw_tool_carries_definition() {
        let rt = RawTool::new(
            "read".into(),
            "desc".into(),
            serde_json::json!({"type": "object"}),
        );
        assert_eq!(rt.name(), "read");
        assert_eq!(rt.description(), "desc");
        assert_eq!(rt.parameters_schema(), serde_json::json!({"type": "object"}));
    }

    #[tokio::test]
    async fn raw_tool_execute_errors() {
        let rt = RawTool::new("read".into(), "d".into(), serde_json::json!({}));
        let ctx = alva_kernel_abi::tool::execution::MinimalExecutionContext::new();
        let res = rt.execute(serde_json::json!({}), &ctx).await;
        assert!(res.is_err(), "gateway RawTool must never execute");
    }
}
