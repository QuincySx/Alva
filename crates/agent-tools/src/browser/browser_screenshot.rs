// INPUT:  agent_types, async_trait, chromiumoxide::cdp, serde, serde_json, base64, super::browser_manager
// OUTPUT: BrowserScreenshotTool
// POS:    Captures page screenshots (viewport, full-page, or element) and saves to file.
//! browser_screenshot — capture page screenshot

use agent_types::{AgentError, CancellationToken, Tool, ToolContext, ToolResult};
use async_trait::async_trait;
use chromiumoxide::cdp::browser_protocol::page::{
    CaptureScreenshotFormat, CaptureScreenshotParams,
};
use serde::Deserialize;
use serde_json::{json, Value};

use super::browser_manager::SharedBrowserManager;

#[derive(Debug, Deserialize)]
struct Input {
    /// Output path for the screenshot file (relative to workspace or absolute)
    path: Option<String>,
    /// Whether to capture the full scrollable page (default: false = viewport only)
    full_page: Option<bool>,
    /// CSS selector to screenshot a specific element
    selector: Option<String>,
    /// Image format: "png" (default) or "jpeg"
    format: Option<String>,
    /// JPEG quality (0-100), only for jpeg format
    quality: Option<i64>,
    /// Browser instance ID, default "default"
    id: Option<String>,
}

pub struct BrowserScreenshotTool {
    pub manager: SharedBrowserManager,
}

#[async_trait]
impl Tool for BrowserScreenshotTool {
    fn name(&self) -> &str {
        "browser_screenshot"
    }

    fn description(&self) -> &str {
        "Capture a screenshot of the current page. Can capture the viewport, full page, or a specific element. Saves to a file and returns the path."
    }

    fn parameters_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "path": {
                    "type": "string",
                    "description": "Output file path (relative to workspace). Default: 'screenshot.png'"
                },
                "full_page": {
                    "type": "boolean",
                    "description": "Capture the full scrollable page, not just the viewport. Default: false"
                },
                "selector": {
                    "type": "string",
                    "description": "CSS selector to screenshot a specific element instead of the page"
                },
                "format": {
                    "type": "string",
                    "enum": ["png", "jpeg"],
                    "description": "Image format. Default: 'png'"
                },
                "quality": {
                    "type": "integer",
                    "description": "JPEG quality (0-100). Only used with format='jpeg'. Default: 80"
                },
                "id": {
                    "type": "string",
                    "description": "Browser instance ID. Default: 'default'"
                }
            }
        })
    }

    async fn execute(&self, input: Value, _cancel: &CancellationToken, ctx: &dyn ToolContext) -> Result<ToolResult, AgentError> {
        let params: Input =
            serde_json::from_value(input).map_err(|e| AgentError::ToolError { tool_name: "browser_screenshot".into(), message: e.to_string() })?;

        let id = params.id.unwrap_or_else(|| "default".to_string());
        let format_str = params.format.as_deref().unwrap_or("png");
        let output_path = params
            .path
            .clone()
            .unwrap_or_else(|| format!("screenshot.{}", format_str));

        let local = ctx.local().ok_or_else(|| AgentError::ToolError {
            tool_name: "browser_screenshot".into(),
            message: "local filesystem context required".into(),
        })?;
        let full_path = if std::path::Path::new(&output_path).is_absolute() {
            std::path::PathBuf::from(&output_path)
        } else {
            local.workspace().join(&output_path)
        };

        let manager = self.manager.lock().await;
        let page = manager
            .active_page(&id)
            .await
            .map_err(|e| AgentError::ToolError { tool_name: "browser_screenshot".into(), message: e })?;

        let result = if let Some(ref selector) = params.selector {
            // Screenshot a specific element
            let element = page
                .find_element(selector)
                .await
                .map_err(|e| {
                    AgentError::ToolError {
                        tool_name: "browser_screenshot".into(),
                        message: format!("Element '{}' not found: {}", selector, e),
                    }
                })?;

            {
                let img_format = match format_str {
                    "jpeg" | "jpg" => CaptureScreenshotFormat::Jpeg,
                    _ => CaptureScreenshotFormat::Png,
                };
                element
                    .screenshot(img_format)
                    .await
                    .map_err(|e| {
                        AgentError::ToolError {
                            tool_name: "browser_screenshot".into(),
                            message: format!("Element screenshot failed: {}", e),
                        }
                    })?
            }
        } else {
            // Screenshot the page (viewport or full page)
            let mut screenshot_params = CaptureScreenshotParams::builder();

            let img_format = match format_str {
                "jpeg" | "jpg" => CaptureScreenshotFormat::Jpeg,
                _ => CaptureScreenshotFormat::Png,
            };
            screenshot_params = screenshot_params.format(img_format);

            if format_str == "jpeg" || format_str == "jpg" {
                let quality = params.quality.unwrap_or(80);
                screenshot_params = screenshot_params.quality(quality);
            }

            if params.full_page.unwrap_or(false) {
                screenshot_params = screenshot_params.capture_beyond_viewport(true);
            }

            let cdp_params = screenshot_params
                .build();

            let screenshot_response = page
                .execute(cdp_params)
                .await
                .map_err(|e| {
                    AgentError::ToolError {
                        tool_name: "browser_screenshot".into(),
                        message: format!("Screenshot failed: {}", e),
                    }
                })?;

            use base64::Engine;
            base64::engine::general_purpose::STANDARD
                .decode(&screenshot_response.result.data)
                .map_err(|e| {
                    AgentError::ToolError {
                        tool_name: "browser_screenshot".into(),
                        message: format!("Failed to decode screenshot data: {}", e),
                    }
                })?
        };

        // Ensure parent directory exists
        if let Some(parent) = full_path.parent() {
            tokio::fs::create_dir_all(parent)
                .await
                .map_err(|e| AgentError::ToolError { tool_name: "browser_screenshot".into(), message: format!("Failed to create directory: {}", e) })?;
        }

        // Write the screenshot file
        tokio::fs::write(&full_path, &result)
            .await
            .map_err(|e| {
                AgentError::ToolError {
                    tool_name: "browser_screenshot".into(),
                    message: format!("Failed to write screenshot: {}", e),
                }
            })?;

        Ok(ToolResult {
            content: json!({
                "status": "captured",
                "path": full_path.display().to_string(),
                "format": format_str,
                "size_bytes": result.len(),
            })
            .to_string(),
            is_error: false,
            details: None,
        })
    }
}
