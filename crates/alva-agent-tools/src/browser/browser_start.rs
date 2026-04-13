// INPUT:  alva_kernel_abi, async_trait, schemars, serde, serde_json, super::browser_manager
// OUTPUT: BrowserStartTool
// POS:    Launches a Chrome browser instance with configurable headless mode, profile, and proxy.
//! browser_start — launch a Chrome instance

use alva_kernel_abi::{AgentError, Tool, ToolExecutionContext, ToolOutput};
use schemars::JsonSchema;
use serde::Deserialize;
use serde_json::json;
use std::path::PathBuf;

use super::browser_manager::SharedBrowserManager;

#[derive(Debug, Deserialize, JsonSchema)]
struct Input {
    /// Browser instance ID. Default: 'default'. Use different IDs for multiple browsers.
    #[serde(default)]
    id: Option<String>,
    /// Run in headless mode (no visible window). Default: true.
    #[serde(default)]
    headless: Option<bool>,
    /// Path to Chrome user data directory for persistent profiles (cookies, storage, etc.).
    #[serde(default)]
    profile_dir: Option<String>,
    /// Proxy server URL, e.g. 'socks5://127.0.0.1:1080' or 'http://proxy:8080'.
    #[serde(default)]
    proxy: Option<String>,
}

#[derive(Tool)]
#[tool(
    name = "browser_start",
    description = "Launch a Chrome browser instance. Returns immediately when the browser is ready. Use browser_navigate to open a URL after starting.",
    input = Input,
)]
pub struct BrowserStartTool {
    pub manager: SharedBrowserManager,
}

impl BrowserStartTool {
    async fn execute_impl(
        &self,
        params: Input,
        _ctx: &dyn ToolExecutionContext,
    ) -> Result<ToolOutput, AgentError> {
        let id = params.id.unwrap_or_else(|| "default".to_string());
        let headless = params.headless.unwrap_or(true);
        let profile_dir = params.profile_dir.map(PathBuf::from);
        let proxy = params.proxy;

        let mut manager = self.manager.lock().await;

        match manager.start(&id, headless, profile_dir, proxy).await {
            Ok(()) => {
                Ok(ToolOutput::text(json!({
                    "status": "started",
                    "id": id,
                    "headless": headless,
                })
                .to_string()))
            }
            Err(e) => {
                Ok(ToolOutput::error(json!({ "error": e }).to_string()))
            }
        }
    }
}
