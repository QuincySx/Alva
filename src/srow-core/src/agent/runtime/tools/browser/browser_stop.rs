//! browser_stop — shut down a Chrome instance

use crate::domain::tool::{ToolDefinition, ToolResult};
use crate::error::EngineError;
use crate::ports::tool::{Tool, ToolContext};
use async_trait::async_trait;
use serde::Deserialize;
use serde_json::{json, Value};
use std::time::Instant;

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

    fn definition(&self) -> ToolDefinition {
        ToolDefinition {
            name: "browser_stop".to_string(),
            description: "Shut down a running Chrome browser instance and release all resources.".to_string(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "id": {
                        "type": "string",
                        "description": "Browser instance ID to stop. Default: 'default'"
                    }
                }
            }),
        }
    }

    async fn execute(&self, input: Value, _ctx: &ToolContext) -> Result<ToolResult, EngineError> {
        let params: Input =
            serde_json::from_value(input).map_err(|e| EngineError::ToolExecution(e.to_string()))?;

        let start = Instant::now();
        let id = params.id.unwrap_or_else(|| "default".to_string());

        let mut manager = self.manager.lock().await;

        match manager.stop(&id).await {
            Ok(()) => {
                let duration_ms = start.elapsed().as_millis() as u64;
                Ok(ToolResult {
                    tool_call_id: String::new(),
                    tool_name: "browser_stop".to_string(),
                    output: json!({
                        "status": "stopped",
                        "id": id,
                    })
                    .to_string(),
                    is_error: false,
                    duration_ms,
                })
            }
            Err(e) => {
                let duration_ms = start.elapsed().as_millis() as u64;
                Ok(ToolResult {
                    tool_call_id: String::new(),
                    tool_name: "browser_stop".to_string(),
                    output: json!({ "error": e }).to_string(),
                    is_error: true,
                    duration_ms,
                })
            }
        }
    }
}
