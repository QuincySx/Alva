// INPUT:  agent_types, async_trait, serde, serde_json, super::browser_manager
// OUTPUT: BrowserStopTool
// POS:    Shuts down a running Chrome browser instance and releases all resources.
//! browser_stop — shut down a Chrome instance

use agent_types::{AgentError, CancellationToken, Tool, ToolContext, ToolResult};
use async_trait::async_trait;
use serde::Deserialize;
use serde_json::{json, Value};

use super::browser_manager::SharedBrowserManager;

#[derive(Debug, Deserialize)]
struct Input {
    /// Instance ID to stop, default "default"
    id: Option<String>,
}

pub struct BrowserStopTool {
    pub manager: SharedBrowserManager,
}

#[async_trait]
impl Tool for BrowserStopTool {
    fn name(&self) -> &str {
        "browser_stop"
    }

    fn description(&self) -> &str {
        "Shut down a running Chrome browser instance and release all resources."
    }

    fn parameters_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "id": {
                    "type": "string",
                    "description": "Browser instance ID to stop. Default: 'default'"
                }
            }
        })
    }

    async fn execute(&self, input: Value, _cancel: &CancellationToken, _ctx: &dyn ToolContext) -> Result<ToolResult, AgentError> {
        let params: Input =
            serde_json::from_value(input).map_err(|e| AgentError::ToolError { tool_name: "browser_stop".into(), message: e.to_string() })?;

        let id = params.id.unwrap_or_else(|| "default".to_string());

        let mut manager = self.manager.lock().await;

        match manager.stop(&id).await {
            Ok(()) => {
                Ok(ToolResult {
                    content: json!({
                        "status": "stopped",
                        "id": id,
                    })
                    .to_string(),
                    is_error: false,
                    details: None,
                })
            }
            Err(e) => {
                Ok(ToolResult {
                    content: json!({ "error": e }).to_string(),
                    is_error: true,
                    details: None,
                })
            }
        }
    }
}
