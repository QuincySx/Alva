// INPUT:  alva_types, async_trait, schemars, serde, serde_json, super::browser_manager
// OUTPUT: BrowserStatusTool
// POS:    Queries browser instance status including running state, current URL, title, and open tabs.
//! browser_status — query browser state (running?, URL, tabs)

use alva_types::{AgentError, Tool, ToolExecutionContext, ToolOutput};
use schemars::JsonSchema;
use serde::Deserialize;
use serde_json::json;

use super::browser_manager::SharedBrowserManager;

#[derive(Debug, Deserialize, JsonSchema)]
struct Input {
    /// Browser instance ID. Default: 'default'. Use '*' to list all instances.
    #[serde(default)]
    id: Option<String>,
}

#[derive(Tool)]
#[tool(
    name = "browser_status",
    description = "Query the status of browser instances. Returns whether the browser is running, the current URL, page title, and list of open tabs. Use id='*' to list all running instances.",
    input = Input,
    read_only,
)]
pub struct BrowserStatusTool {
    pub manager: SharedBrowserManager,
}

impl BrowserStatusTool {
    async fn execute_impl(
        &self,
        params: Input,
        _ctx: &dyn ToolExecutionContext,
    ) -> Result<ToolOutput, AgentError> {
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

        Ok(ToolOutput::text(output.to_string()))
    }
}
