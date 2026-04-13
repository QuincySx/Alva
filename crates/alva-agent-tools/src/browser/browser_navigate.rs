// INPUT:  alva_types, async_trait, schemars, serde, serde_json, super::browser_manager
// OUTPUT: BrowserNavigateTool
// POS:    Navigates the browser to a URL, waits for load, and returns the final URL and title.
//! browser_navigate — navigate to a URL

use alva_types::{AgentError, Tool, ToolExecutionContext, ToolOutput};
use schemars::JsonSchema;
use serde::Deserialize;
use serde_json::json;

use super::browser_manager::SharedBrowserManager;

#[derive(Debug, Deserialize, JsonSchema)]
struct Input {
    /// The URL to navigate to (e.g. 'https://example.com').
    url: String,
    /// Browser instance ID. Default: 'default'.
    #[serde(default)]
    id: Option<String>,
}

#[derive(Tool)]
#[tool(
    name = "browser_navigate",
    description = "Navigate the browser to a URL. Opens a new tab and waits for the page to load. Returns the page title and final URL (after any redirects).",
    input = Input,
)]
pub struct BrowserNavigateTool {
    pub manager: SharedBrowserManager,
}

impl BrowserNavigateTool {
    async fn execute_impl(
        &self,
        params: Input,
        _ctx: &dyn ToolExecutionContext,
    ) -> Result<ToolOutput, AgentError> {
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
