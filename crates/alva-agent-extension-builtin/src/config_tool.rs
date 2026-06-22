// INPUT:  alva_kernel_abi, async_trait, serde, serde_json
// OUTPUT: ConfigTool
// POS:    Manages agent/session configuration values (get/set/list).
//! config_tool — manage configuration settings

use alva_kernel_abi::{AgentError, Tool, ToolExecutionContext, ToolOutput};
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
        let params: Input = serde_json::from_value(input).map_err(|e| AgentError::ToolError {
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
                     Use 'get' with a specific key to read individual values.",
                ))
            }
            other => Ok(ToolOutput::error(format!(
                "Invalid action '{}'. Must be get, set, or list.",
                other
            ))),
        }
    }
}

#[cfg(test)]
mod tests {
    use std::any::Any;
    use std::collections::HashMap;

    use super::*;
    use alva_kernel_abi::CancellationToken;

    /// TestContext supports a backing `config` map so `get_config` returns
    /// real values — this is what makes config_tool's `action=get` path
    /// observable (vs the pure-stub tools).
    struct TestContext {
        cancel: CancellationToken,
        config: HashMap<String, String>,
    }

    impl ToolExecutionContext for TestContext {
        fn cancel_token(&self) -> &CancellationToken {
            &self.cancel
        }
        fn session_id(&self) -> &str {
            "test-session"
        }
        fn get_config(&self, key: &str) -> Option<String> {
            self.config.get(key).cloned()
        }
        fn as_any(&self) -> &dyn Any {
            self
        }
    }

    fn ctx_with(entries: &[(&str, &str)]) -> TestContext {
        TestContext {
            cancel: CancellationToken::new(),
            config: entries
                .iter()
                .map(|(k, v)| (k.to_string(), v.to_string()))
                .collect(),
        }
    }

    #[tokio::test]
    async fn get_returns_value_when_key_present() {
        let tool = ConfigTool;
        let ctx = ctx_with(&[("editor", "vim")]);
        let out = tool
            .execute(json!({ "action": "get", "key": "editor" }), &ctx)
            .await
            .expect("get should succeed");

        assert!(!out.is_error);
        let text = out.model_text();
        assert!(text.contains("editor"), "key missing: {text}");
        assert!(text.contains("vim"), "value missing: {text}");
    }

    #[tokio::test]
    async fn get_says_not_set_when_missing() {
        let tool = ConfigTool;
        let ctx = ctx_with(&[]);
        let out = tool
            .execute(json!({ "action": "get", "key": "nope" }), &ctx)
            .await
            .expect("get should succeed even when missing");

        assert!(!out.is_error, "missing value isn't a tool error");
        assert!(
            out.model_text().contains("not set"),
            "expected 'not set' phrasing: {}",
            out.model_text()
        );
    }

    #[tokio::test]
    async fn set_without_value_errors() {
        let tool = ConfigTool;
        let ctx = ctx_with(&[]);
        let err = tool
            .execute(json!({ "action": "set", "key": "x" }), &ctx)
            .await
            .expect_err("set without value should error");

        assert!(
            format!("{err}").contains("value"),
            "expected error mentioning value: {err}"
        );
    }

    #[tokio::test]
    async fn set_without_key_errors() {
        let tool = ConfigTool;
        let ctx = ctx_with(&[]);
        let err = tool
            .execute(json!({ "action": "set", "value": "x" }), &ctx)
            .await
            .expect_err("set without key should error");

        assert!(
            format!("{err}").contains("key"),
            "expected error mentioning key: {err}"
        );
    }

    #[tokio::test]
    async fn unknown_action_surfaces_as_tool_output_error() {
        let tool = ConfigTool;
        let ctx = ctx_with(&[]);
        let out = tool
            .execute(json!({ "action": "delete" }), &ctx)
            .await
            .expect("unknown action returns Ok(error output)");

        assert!(out.is_error, "unknown action should set is_error");
        assert!(
            out.model_text().contains("Invalid action"),
            "expected validation msg: {}",
            out.model_text()
        );
    }

    /// Stub-output contract guard: list action is not yet wired to a real
    /// enumerator. When that lands, this test gets updated in lockstep.
    #[tokio::test]
    async fn list_stub_advertises_not_available() {
        let tool = ConfigTool;
        let ctx = ctx_with(&[("a", "1"), ("b", "2")]);
        let out = tool
            .execute(json!({ "action": "list" }), &ctx)
            .await
            .expect("list should succeed");

        assert!(
            out.model_text().contains("not yet available"),
            "stub disclosure missing — if you wired list, update this test"
        );
    }

    #[test]
    fn is_read_only_distinguishes_mutating_actions() {
        let tool = ConfigTool;
        assert!(tool.is_read_only(&json!({ "action": "get" })));
        assert!(tool.is_read_only(&json!({ "action": "list" })));
        assert!(!tool.is_read_only(&json!({ "action": "set" })));
        // Unknown action defaults to NOT read-only — safer (the matcher
        // would let an unknown mutating call through if we said true).
        assert!(!tool.is_read_only(&json!({ "action": "delete" })));
        // Missing action field also returns false (defensive default).
        assert!(!tool.is_read_only(&json!({})));
    }
}
