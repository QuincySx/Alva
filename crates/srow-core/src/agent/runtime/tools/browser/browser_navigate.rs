// INPUT:  crate::domain::tool, crate::error, crate::ports::tool, async_trait, serde, serde_json, super::browser_manager
// OUTPUT: BrowserNavigateTool
// POS:    Navigates the browser to a URL, waits for load, and returns the final URL and title.
//! browser_navigate — navigate to a URL

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
    /// URL to navigate to
    url: String,
    /// Browser instance ID, default "default"
    id: Option<String>,
}

pub struct BrowserNavigateTool {
    pub manager: SharedBrowserManager,
}

#[async_trait]
impl Tool for BrowserNavigateTool {
    fn name(&self) -> &str {
        "browser_navigate"
    }

    fn definition(&self) -> ToolDefinition {
        ToolDefinition {
            name: "browser_navigate".to_string(),
            description: "Navigate the browser to a URL. Opens a new tab and waits for the page to load. Returns the page title and final URL (after any redirects).".to_string(),
            parameters: json!({
                "type": "object",
                "required": ["url"],
                "properties": {
                    "url": {
                        "type": "string",
                        "description": "The URL to navigate to (e.g. 'https://example.com')"
                    },
                    "id": {
                        "type": "string",
                        "description": "Browser instance ID. Default: 'default'"
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

        match manager.navigate(&id, &params.url).await {
            Ok(page) => {
                // Get final URL and title after navigation
                let final_url = page
                    .url()
                    .await
                    .ok()
                    .flatten()
                    .map(|u| u.to_string())
                    .unwrap_or_else(|| params.url.clone());

                let title = page
                    .get_title()
                    .await
                    .ok()
                    .flatten()
                    .unwrap_or_default();

                let duration_ms = start.elapsed().as_millis() as u64;

                Ok(ToolResult {
                    tool_call_id: String::new(),
                    tool_name: "browser_navigate".to_string(),
                    output: json!({
                        "status": "navigated",
                        "url": final_url,
                        "title": title,
                        "instance_id": id,
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
                    tool_name: "browser_navigate".to_string(),
                    output: json!({ "error": e }).to_string(),
                    is_error: true,
                    duration_ms,
                })
            }
        }
    }
}
