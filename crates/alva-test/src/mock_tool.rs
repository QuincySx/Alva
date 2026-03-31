// INPUT:  alva_types::{Tool, ToolOutput, ToolExecutionContext, MinimalExecutionContext, AgentError}, Arc, Mutex, serde_json::Value
// OUTPUT: MockTool — configurable mock for Tool with preset result/error and call recording
// POS:    alva-test crate — provides a test double for Tool used in unit and integration tests

use std::sync::{Arc, Mutex};

use async_trait::async_trait;
use serde_json::Value;

use alva_types::base::error::AgentError;
use alva_types::tool::Tool;
use alva_types::tool::execution::{ToolExecutionContext, ToolOutput};

// ---------------------------------------------------------------------------
// MockTool
// ---------------------------------------------------------------------------

/// A mock implementation of [`Tool`] for use in tests.
///
/// Supports:
/// - Returning a preset [`ToolOutput`] via [`with_result`].
/// - Returning a preset error via [`with_error`].
/// - Recording every `execute` call's input via [`calls`].
///
/// Uses `Arc<Mutex<...>>` internally so the struct can be cloned and still
/// share state across handles, which is required because the trait takes `&self`.
#[derive(Clone)]
pub struct MockTool {
    name: String,
    preset: Arc<Mutex<Option<Result<ToolOutput, AgentError>>>>,
    recorded_calls: Arc<Mutex<Vec<Value>>>,
}

impl MockTool {
    /// Create a new mock with no preset result (calling `execute` without
    /// configuring a result or error will return an error).
    pub fn new(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            preset: Arc::new(Mutex::new(None)),
            recorded_calls: Arc::new(Mutex::new(Vec::new())),
        }
    }

    /// Configure the mock to return a successful [`ToolOutput`] (builder pattern).
    pub fn with_result(self, result: ToolOutput) -> Self {
        *self.preset.lock().unwrap() = Some(Ok(result));
        self
    }

    /// Configure the mock to return a [`AgentError::ToolError`] (builder pattern).
    pub fn with_error(self, message: impl Into<String>) -> Self {
        let name = self.name.clone();
        let msg = message.into();
        *self.preset.lock().unwrap() = Some(Err(AgentError::ToolError {
            tool_name: name,
            message: msg,
        }));
        self
    }

    /// Return all recorded `execute` input values (one per call).
    pub fn calls(&self) -> Vec<Value> {
        self.recorded_calls.lock().unwrap().clone()
    }
}

#[async_trait]
impl Tool for MockTool {
    fn name(&self) -> &str {
        &self.name
    }

    fn description(&self) -> &str {
        "MockTool"
    }

    fn parameters_schema(&self) -> serde_json::Value {
        serde_json::json!({ "type": "object" })
    }

    async fn execute(
        &self,
        input: serde_json::Value,
        _ctx: &dyn ToolExecutionContext,
    ) -> Result<ToolOutput, AgentError> {
        // Record the call.
        self.recorded_calls.lock().unwrap().push(input);

        // Return the preset or a generic error if none was configured.
        match self.preset.lock().unwrap().as_ref() {
            Some(Ok(result)) => Ok(result.clone()),
            Some(Err(err)) => Err(AgentError::ToolError {
                tool_name: self.name.clone(),
                message: err.to_string(),
            }),
            None => Err(AgentError::ToolError {
                tool_name: self.name.clone(),
                message: "MockTool: no preset result configured".into(),
            }),
        }
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use alva_types::tool::execution::MinimalExecutionContext;

    #[tokio::test]
    async fn test_mock_tool_returns_preset() {
        let tool = MockTool::new("test_tool")
            .with_result(ToolOutput::text("done"));

        let ctx = MinimalExecutionContext::new();
        let result = tool.execute(serde_json::json!({}), &ctx).await.unwrap();
        assert_eq!(result.model_text(), "done");
        assert!(!result.is_error);
    }

    #[tokio::test]
    async fn test_mock_tool_records_calls() {
        let tool = MockTool::new("recorder")
            .with_result(ToolOutput::text("ok"));

        let ctx = MinimalExecutionContext::new();
        let input = serde_json::json!({"path": "/tmp/test"});
        let _ = tool.execute(input.clone(), &ctx).await;
        let calls = tool.calls();
        assert_eq!(calls.len(), 1);
        assert_eq!(calls[0], input);
    }

    #[tokio::test]
    async fn test_mock_tool_error() {
        let tool = MockTool::new("failing")
            .with_error("tool exploded");

        let ctx = MinimalExecutionContext::new();
        let result = tool.execute(serde_json::json!({}), &ctx).await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_mock_tool_no_preset_returns_error() {
        let tool = MockTool::new("unconfigured");
        let ctx = MinimalExecutionContext::new();
        let result = tool.execute(serde_json::json!({}), &ctx).await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_mock_tool_records_multiple_calls() {
        let tool = MockTool::new("multi")
            .with_result(ToolOutput::text("ok"));

        let ctx = MinimalExecutionContext::new();
        let _ = tool.execute(serde_json::json!({"n": 1}), &ctx).await;
        let _ = tool.execute(serde_json::json!({"n": 2}), &ctx).await;
        let calls = tool.calls();
        assert_eq!(calls.len(), 2);
        assert_eq!(calls[0], serde_json::json!({"n": 1}));
        assert_eq!(calls[1], serde_json::json!({"n": 2}));
    }

    #[tokio::test]
    async fn test_mock_tool_name() {
        let tool = MockTool::new("my_tool");
        assert_eq!(tool.name(), "my_tool");
    }
}
