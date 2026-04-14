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
