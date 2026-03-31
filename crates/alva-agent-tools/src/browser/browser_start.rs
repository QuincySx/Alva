// INPUT:  alva_types, async_trait, serde, serde_json, super::browser_manager
// OUTPUT: BrowserStartTool
// POS:    Launches a Chrome browser instance with configurable headless mode, profile, and proxy.
//! browser_start — launch a Chrome instance

use alva_types::{AgentError, Tool, ToolExecutionContext, ToolOutput};
use async_trait::async_trait;
use serde::Deserialize;
use serde_json::{json, Value};
use std::path::PathBuf;

use super::browser_manager::SharedBrowserManager;

#[derive(Debug, Deserialize)]
struct Input {
    /// Instance ID, default "default"
    id: Option<String>,
    /// Run in headless mode, default true
    headless: Option<bool>,
    /// User data directory for persistent profile
    profile_dir: Option<String>,
    /// Proxy server (e.g. "socks5://127.0.0.1:1080")
    proxy: Option<String>,
}

pub struct BrowserStartTool {
    pub manager: SharedBrowserManager,
}

#[async_trait]
impl Tool for BrowserStartTool {
    fn name(&self) -> &str {
        "browser_start"
    }

    fn description(&self) -> &str {
        "Launch a Chrome browser instance. Returns immediately when the browser is ready. Use browser_navigate to open a URL after starting."
    }

    fn parameters_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "id": {
                    "type": "string",
                    "description": "Browser instance ID. Default: 'default'. Use different IDs for multiple browsers."
                },
                "headless": {
                    "type": "boolean",
                    "description": "Run in headless mode (no visible window). Default: true"
                },
                "profile_dir": {
                    "type": "string",
                    "description": "Path to Chrome user data directory for persistent profiles (cookies, storage, etc.)"
                },
                "proxy": {
                    "type": "string",
                    "description": "Proxy server URL, e.g. 'socks5://127.0.0.1:1080' or 'http://proxy:8080'"
                }
            }
        })
    }

    async fn execute(&self, input: Value, _ctx: &dyn ToolExecutionContext) -> Result<ToolOutput, AgentError> {
        let params: Input =
            serde_json::from_value(input).map_err(|e| AgentError::ToolError { tool_name: "browser_start".into(), message: e.to_string() })?;

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
