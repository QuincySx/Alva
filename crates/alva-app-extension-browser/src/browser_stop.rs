// INPUT:  alva_kernel_abi, async_trait, schemars, serde, serde_json, super::browser_manager
// OUTPUT: BrowserStopTool
// POS:    Shuts down a running Chrome browser instance and releases all resources.
//! browser_stop — shut down a Chrome instance

use alva_kernel_abi::{AgentError, Tool, ToolExecutionContext, ToolOutput};
use schemars::JsonSchema;
use serde::Deserialize;
use serde_json::json;

use super::browser_manager::SharedBrowserManager;

#[derive(Debug, Deserialize, JsonSchema)]
struct Input {
    /// Browser instance ID to stop. Default: 'default'.
    #[serde(default)]
    id: Option<String>,
}

#[derive(Tool)]
#[tool(
    name = "browser_stop",
    description = "Shut down a running Chrome browser instance and release all resources.",
    input = Input,
    destructive,
)]
pub struct BrowserStopTool {
    pub manager: SharedBrowserManager,
}

impl BrowserStopTool {
    async fn execute_impl(
        &self,
        params: Input,
        _ctx: &dyn ToolExecutionContext,
    ) -> Result<ToolOutput, AgentError> {
        let id = params.id.unwrap_or_else(|| "default".to_string());

        let mut manager = self.manager.lock().await;

        match manager.stop(&id).await {
            Ok(()) => {
                Ok(ToolOutput::text(json!({
                    "status": "stopped",
                    "id": id,
                })
                .to_string()))
            }
            Err(e) => {
                Ok(ToolOutput::error(json!({ "error": e }).to_string()))
            }
        }
    }
}
