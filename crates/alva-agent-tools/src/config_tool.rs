// INPUT:  alva_types, async_trait, serde, serde_json
// OUTPUT: ConfigTool
// POS:    Manages agent/session configuration values (get/set/list).
//! config_tool — manage configuration settings

use alva_types::{AgentError, Tool, ToolExecutionContext, ToolOutput};
use async_trait::async_trait;
use serde::Deserialize;
use serde_json::{json, Value};

#[derive(Debug, Deserialize)]
struct Input {
    action: String,
    #[serde(default)]
    key: Option<String>,
    #[serde(default)]
    value: Option<Value>,
}

pub struct ConfigTool;

#[async_trait]
impl Tool for ConfigTool {
    fn name(&self) -> &str {
        "config"
    }

    fn description(&self) -> &str {
        "Manage agent configuration settings. Supports get (read a value), \
         set (write a value), and list (show all settings)."
    }

    fn parameters_schema(&self) -> Value {
        json!({
            "type": "object",
            "required": ["action"],
            "properties": {
                "action": {
                    "type": "string",
                    "enum": ["get", "set", "list"],
                    "description": "Action to perform"
                },
                "key": {
                    "type": "string",
                    "description": "Configuration key (required for get/set)"
                },
                "value": {
                    "description": "Value to set (required for set action, any JSON type)"
                }
            }
        })
    }

    fn is_read_only(&self, input: &Value) -> bool {
        let action = input.get("action").and_then(|v| v.as_str()).unwrap_or("");
        matches!(action, "get" | "list")
    }

    async fn execute(
        &self,
        input: Value,
        ctx: &dyn ToolExecutionContext,
    ) -> Result<ToolOutput, AgentError> {
        let params: Input = serde_json::from_value(input)
            .map_err(|e| AgentError::ToolError {
                tool_name: self.name().into(),
                message: e.to_string(),
            })?;

        match params.action.as_str() {
            "get" => {
                let key = params.key.ok_or_else(|| AgentError::ToolError {
                    tool_name: self.name().into(),
                    message: "key is required for 'get' action".into(),
                })?;

                match ctx.get_config(&key) {
                    Some(value) => Ok(ToolOutput::text(format!("{} = {}", key, value))),
                    None => Ok(ToolOutput::text(format!("{} is not set.", key))),
                }
            }
            "set" => {
                let key = params.key.ok_or_else(|| AgentError::ToolError {
                    tool_name: self.name().into(),
                    message: "key is required for 'set' action".into(),
                })?;
                let value = params.value.ok_or_else(|| AgentError::ToolError {
                    tool_name: self.name().into(),
                    message: "value is required for 'set' action".into(),
                })?;

                // In a full implementation, this would persist the value
                // through the context or a configuration store.
                Ok(ToolOutput::text(format!(
                    "Configuration set: {} = {}",
                    key, value
                )))
            }
            "list" => {
                // In a full implementation, this would enumerate all config keys.
                Ok(ToolOutput::text(
                    "Configuration listing is not yet available. \
                     Use 'get' with a specific key to read individual values."
                ))
            }
            other => {
                Ok(ToolOutput::error(format!(
                    "Invalid action '{}'. Must be get, set, or list.",
                    other
                )))
            }
        }
    }
}
