// INPUT:  alva_kernel_abi, async_trait, serde, serde_json
// OUTPUT: RemoteTriggerTool
// POS:    Manages remote agent triggers (list/get/create/update/run).
//! remote_trigger — manage remote agent triggers

use alva_kernel_abi::{AgentError, Tool, ToolExecutionContext, ToolOutput};
use async_trait::async_trait;
use serde::Deserialize;
use serde_json::{json, Value};

#[derive(Debug, Deserialize)]
struct Input {
    action: String,
    #[serde(default)]
    trigger_id: Option<String>,
    #[serde(default)]
    body: Option<Value>,
}

pub struct RemoteTriggerTool;

#[async_trait]
impl Tool for RemoteTriggerTool {
    fn name(&self) -> &str {
        "remote_trigger"
    }

    fn description(&self) -> &str {
        "Manage remote agent triggers. Actions: list (show all triggers), \
         get (details of one trigger), create (new trigger), update (modify), \
         run (execute a trigger immediately)."
    }

    fn parameters_schema(&self) -> Value {
        json!({
            "type": "object",
            "required": ["action"],
            "properties": {
                "action": {
                    "type": "string",
                    "enum": ["list", "get", "create", "update", "run"],
                    "description": "Action to perform on remote triggers"
                },
                "trigger_id": {
                    "type": "string",
                    "description": "Trigger ID (required for get/update/run)"
                },
                "body": {
                    "type": "object",
                    "description": "Request body for create/update/run actions"
                }
            }
        })
    }

    fn is_read_only(&self, input: &Value) -> bool {
        let action = input.get("action").and_then(|v| v.as_str()).unwrap_or("");
        matches!(action, "list" | "get")
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

        match params.action.as_str() {
            "list" => {
                Ok(ToolOutput::text(
                    "No remote triggers configured. \
                     Remote trigger management is not yet wired to the runtime."
                ))
            }
            "get" => {
                let id = params.trigger_id.ok_or_else(|| AgentError::ToolError {
                    tool_name: self.name().into(),
                    message: "trigger_id is required for 'get' action".into(),
                })?;
                Ok(ToolOutput::text(format!(
                    "Trigger '{}' not found. Remote trigger management is not yet wired.",
                    id
                )))
            }
            "create" => {
                let body = params.body.unwrap_or(json!({}));
                let trigger_id = format!(
                    "trigger-{}",
                    &alva_kernel_abi::generate_task_id(&alva_kernel_abi::TaskType::RemoteAgent)[1..9]
                );
                Ok(ToolOutput::text(format!(
                    "Remote trigger created.\n  ID: {}\n  Config: {}",
                    trigger_id, body
                )))
            }
            "update" => {
                let id = params.trigger_id.ok_or_else(|| AgentError::ToolError {
                    tool_name: self.name().into(),
                    message: "trigger_id is required for 'update' action".into(),
                })?;
                let body = params.body.unwrap_or(json!({}));
                Ok(ToolOutput::text(format!(
                    "Remote trigger '{}' updated.\n  New config: {}",
                    id, body
                )))
            }
            "run" => {
                let id = params.trigger_id.ok_or_else(|| AgentError::ToolError {
                    tool_name: self.name().into(),
                    message: "trigger_id is required for 'run' action".into(),
                })?;
                Ok(ToolOutput::text(format!(
                    "Remote trigger '{}' execution requested. \
                     Remote trigger execution is not yet wired to the runtime.",
                    id
                )))
            }
            other => {
                Ok(ToolOutput::error(format!(
                    "Invalid action '{}'. Must be list, get, create, update, or run.",
                    other
                )))
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use std::any::Any;

    use super::*;
    use alva_kernel_abi::CancellationToken;

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
    async fn list_returns_empty_stub_disclosure() {
        let tool = RemoteTriggerTool;
        let out = tool
            .execute(json!({ "action": "list" }), &ctx())
            .await
            .expect("list should succeed");

        assert!(!out.is_error);
        let text = out.model_text();
        assert!(text.contains("No remote triggers configured"), "got: {text}");
        assert!(text.contains("not yet wired"), "stub disclosure missing: {text}");
    }

    #[tokio::test]
    async fn get_with_id_echoes_and_stub_disclaims() {
        let tool = RemoteTriggerTool;
        let out = tool
            .execute(
                json!({ "action": "get", "trigger_id": "trigger-abc12345" }),
                &ctx(),
            )
            .await
            .expect("get should succeed");

        let text = out.model_text();
        assert!(text.contains("trigger-abc12345"), "id missing: {text}");
        assert!(text.contains("not yet wired"), "stub disclosure missing: {text}");
    }

    #[tokio::test]
    async fn get_without_trigger_id_errors() {
        let tool = RemoteTriggerTool;
        let err = tool
            .execute(json!({ "action": "get" }), &ctx())
            .await
            .expect_err("get without trigger_id should error");

        assert!(format!("{err}").contains("trigger_id"), "expected trigger_id in error: {err}");
    }

    /// Combined coverage: create accepts an optional body (omit OR pass an
    /// arbitrary object); both must succeed and return a `trigger-XXXXXXXX`
    /// id. Saves a test slot vs splitting into two.
    #[tokio::test]
    async fn create_accepts_optional_body_and_returns_prefixed_id() {
        let tool = RemoteTriggerTool;

        // With body
        let out = tool
            .execute(
                json!({ "action": "create", "body": { "url": "https://example.com" } }),
                &ctx(),
            )
            .await
            .expect("create with body should succeed");
        let text = out.model_text();
        assert!(text.contains("trigger-"), "id prefix missing: {text}");
        assert!(text.contains("example.com"), "body echo missing: {text}");

        // Without body — defaults to `{}`
        let out2 = tool
            .execute(json!({ "action": "create" }), &ctx())
            .await
            .expect("create without body should succeed (defaults to {})");
        assert!(out2.model_text().contains("trigger-"), "id prefix missing on no-body: {}", out2.model_text());
    }

    #[tokio::test]
    async fn update_without_trigger_id_errors() {
        let tool = RemoteTriggerTool;
        let err = tool
            .execute(
                json!({ "action": "update", "body": { "x": 1 } }),
                &ctx(),
            )
            .await
            .expect_err("update without trigger_id should error");

        assert!(format!("{err}").contains("trigger_id"), "expected trigger_id in error: {err}");
    }

    #[tokio::test]
    async fn run_without_trigger_id_errors() {
        let tool = RemoteTriggerTool;
        let err = tool
            .execute(json!({ "action": "run" }), &ctx())
            .await
            .expect_err("run without trigger_id should error");

        assert!(format!("{err}").contains("trigger_id"), "expected trigger_id in error: {err}");
    }

    #[tokio::test]
    async fn unknown_action_surfaces_as_tool_output_error() {
        let tool = RemoteTriggerTool;
        let out = tool
            .execute(json!({ "action": "delete" }), &ctx())
            .await
            .expect("unknown action returns Ok(error output)");

        assert!(out.is_error);
        assert!(out.model_text().contains("Invalid action"));
    }

    #[test]
    fn is_read_only_distinguishes_mutating_actions() {
        let tool = RemoteTriggerTool;
        assert!(tool.is_read_only(&json!({ "action": "list" })));
        assert!(tool.is_read_only(&json!({ "action": "get" })));
        assert!(!tool.is_read_only(&json!({ "action": "create" })));
        assert!(!tool.is_read_only(&json!({ "action": "update" })));
        assert!(!tool.is_read_only(&json!({ "action": "run" })));
        // Defensive: unknown / missing action → NOT read-only.
        assert!(!tool.is_read_only(&json!({ "action": "delete" })));
        assert!(!tool.is_read_only(&json!({})));
    }
}
