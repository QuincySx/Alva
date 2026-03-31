// INPUT:  alva_types, async_trait, serde, serde_json, super::browser_manager
// OUTPUT: BrowserNavigateTool
// POS:    Navigates the browser to a URL, waits for load, and returns the final URL and title.
//! browser_navigate — navigate to a URL

use alva_types::{AgentError, Tool, ToolExecutionContext, ToolOutput};
use async_trait::async_trait;
use serde::Deserialize;
use serde_json::{json, Value};

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

    fn description(&self) -> &str {
        "Navigate the browser to a URL. Opens a new tab and waits for the page to load. Returns the page title and final URL (after any redirects)."
    }

    fn parameters_schema(&self) -> Value {
        json!({
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
        })
    }

    async fn execute(&self, input: Value, _ctx: &dyn ToolExecutionContext) -> Result<ToolOutput, AgentError> {
        let params: Input =
            serde_json::from_value(input).map_err(|e| AgentError::ToolError { tool_name: "browser_navigate".into(), message: e.to_string() })?;

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

                Ok(ToolOutput::text(json!({
                    "status": "navigated",
                    "url": final_url,
                    "title": title,
                    "instance_id": id,
                })
                .to_string()))
            }
            Err(e) => {
                Ok(ToolOutput::error(json!({ "error": e }).to_string()))
            }
        }
    }
}
