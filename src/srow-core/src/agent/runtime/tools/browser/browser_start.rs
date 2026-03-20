//! browser_start — launch a Chrome instance

use crate::domain::tool::{ToolDefinition, ToolResult};
use crate::error::EngineError;
use crate::ports::tool::{Tool, ToolContext};
use async_trait::async_trait;
use serde::Deserialize;
use serde_json::{json, Value};
use std::path::PathBuf;
use std::time::Instant;

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

    fn definition(&self) -> ToolDefinition {
        ToolDefinition {
            name: "browser_start".to_string(),
            description: "Launch a Chrome browser instance. Returns immediately when the browser is ready. Use browser_navigate to open a URL after starting.".to_string(),
            parameters: json!({
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
            }),
        }
    }

    async fn execute(&self, input: Value, _ctx: &ToolContext) -> Result<ToolResult, EngineError> {
        let params: Input =
            serde_json::from_value(input).map_err(|e| EngineError::ToolExecution(e.to_string()))?;

        let start = Instant::now();
        let id = params.id.unwrap_or_else(|| "default".to_string());
        let headless = params.headless.unwrap_or(true);
        let profile_dir = params.profile_dir.map(PathBuf::from);
        let proxy = params.proxy;

        let mut manager = self.manager.lock().await;

        match manager.start(&id, headless, profile_dir, proxy).await {
            Ok(()) => {
                let duration_ms = start.elapsed().as_millis() as u64;
                Ok(ToolResult {
                    tool_call_id: String::new(),
                    tool_name: "browser_start".to_string(),
                    output: json!({
                        "status": "started",
                        "id": id,
                        "headless": headless,
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
                    tool_name: "browser_start".to_string(),
                    output: json!({ "error": e }).to_string(),
                    is_error: true,
                    duration_ms,
                })
            }
        }
    }
}
