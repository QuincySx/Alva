//! browser_status — query browser state (running?, URL, tabs)

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
    /// Browser instance ID, default "default". Use "*" to list all instances.
    id: Option<String>,
}

pub struct BrowserStatusTool {
    pub manager: SharedBrowserManager,
}

#[async_trait]
impl Tool for BrowserStatusTool {
    fn name(&self) -> &str {
        "browser_status"
    }

    fn definition(&self) -> ToolDefinition {
        ToolDefinition {
            name: "browser_status".to_string(),
            description: "Query the status of browser instances. Returns whether the browser is running, the current URL, page title, and list of open tabs. Use id='*' to list all running instances.".to_string(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "id": {
                        "type": "string",
                        "description": "Browser instance ID. Default: 'default'. Use '*' to list all instances."
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

        let manager = self.manager.lock().await;

        let output = if id == "*" {
            // List all running instances
            let ids = manager.instance_ids();
            let mut instances = Vec::new();

            for instance_id in &ids {
                let tabs = manager.list_tabs(instance_id).await.unwrap_or_default();
                let instance_info = manager.get(instance_id);
                instances.push(json!({
                    "id": instance_id,
                    "headless": instance_info.map(|i| i.headless).unwrap_or(false),
                    "tabs": tabs,
                }));
            }

            json!({
                "total_instances": ids.len(),
                "instances": instances,
            })
        } else {
            // Status for a specific instance
            if !manager.is_running(&id) {
                json!({
                    "id": id,
                    "running": false,
                })
            } else {
                let tabs = manager.list_tabs(&id).await.unwrap_or_default();
                let instance_info = manager.get(&id);

                // Get current page info
                let (current_url, current_title) =
                    if let Ok(page) = manager.active_page(&id).await {
                        let url = page
                            .url()
                            .await
                            .ok()
                            .flatten()
                            .map(|u| u.to_string())
                            .unwrap_or_else(|| "about:blank".to_string());
                        let title = page
                            .get_title()
                            .await
                            .ok()
                            .flatten()
                            .unwrap_or_default();
                        (url, title)
                    } else {
                        ("unknown".to_string(), String::new())
                    };

                json!({
                    "id": id,
                    "running": true,
                    "headless": instance_info.map(|i| i.headless).unwrap_or(false),
                    "current_url": current_url,
                    "current_title": current_title,
                    "tab_count": tabs.len(),
                    "tabs": tabs,
                })
            }
        };

        let duration_ms = start.elapsed().as_millis() as u64;

        Ok(ToolResult {
            tool_call_id: String::new(),
            tool_name: "browser_status".to_string(),
            output: output.to_string(),
            is_error: false,
            duration_ms,
        })
    }
}
