//! browser_screenshot — capture page screenshot

use crate::domain::tool::{ToolDefinition, ToolResult};
use crate::error::EngineError;
use crate::ports::tool::{Tool, ToolContext};
use async_trait::async_trait;
use chromiumoxide::cdp::browser_protocol::page::{
    CaptureScreenshotFormat, CaptureScreenshotParams,
};
use serde::Deserialize;
use serde_json::{json, Value};
use std::time::Instant;

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

    fn definition(&self) -> ToolDefinition {
        ToolDefinition {
            name: "browser_screenshot".to_string(),
            description: "Capture a screenshot of the current page. Can capture the viewport, full page, or a specific element. Saves to a file and returns the path.".to_string(),
            parameters: json!({
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
            }),
        }
    }

    async fn execute(&self, input: Value, ctx: &ToolContext) -> Result<ToolResult, EngineError> {
        let params: Input =
            serde_json::from_value(input).map_err(|e| EngineError::ToolExecution(e.to_string()))?;

        let start = Instant::now();
        let id = params.id.unwrap_or_else(|| "default".to_string());
        let format_str = params.format.as_deref().unwrap_or("png");
        let output_path = params
            .path
            .clone()
            .unwrap_or_else(|| format!("screenshot.{}", format_str));

        let full_path = if std::path::Path::new(&output_path).is_absolute() {
            std::path::PathBuf::from(&output_path)
        } else {
            ctx.workspace.join(&output_path)
        };

        let manager = self.manager.lock().await;
        let page = manager
            .active_page(&id)
            .await
            .map_err(|e| EngineError::ToolExecution(e))?;

        let result = if let Some(ref selector) = params.selector {
            // Screenshot a specific element
            let element = page
                .find_element(selector)
                .await
                .map_err(|e| {
                    EngineError::ToolExecution(format!(
                        "Element '{}' not found: {}",
                        selector, e
                    ))
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
                        EngineError::ToolExecution(format!("Element screenshot failed: {}", e))
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
                    EngineError::ToolExecution(format!("Screenshot failed: {}", e))
                })?;

            use base64::Engine;
            base64::engine::general_purpose::STANDARD
                .decode(&screenshot_response.result.data)
                .map_err(|e| {
                    EngineError::ToolExecution(format!(
                        "Failed to decode screenshot data: {}",
                        e
                    ))
                })?
        };

        // Ensure parent directory exists
        if let Some(parent) = full_path.parent() {
            tokio::fs::create_dir_all(parent)
                .await
                .map_err(|e| EngineError::ToolExecution(format!("Failed to create directory: {}", e)))?;
        }

        // Write the screenshot file
        tokio::fs::write(&full_path, &result)
            .await
            .map_err(|e| {
                EngineError::ToolExecution(format!("Failed to write screenshot: {}", e))
            })?;

        let duration_ms = start.elapsed().as_millis() as u64;

        Ok(ToolResult {
            tool_call_id: String::new(),
            tool_name: "browser_screenshot".to_string(),
            output: json!({
                "status": "captured",
                "path": full_path.display().to_string(),
                "format": format_str,
                "size_bytes": result.len(),
            })
            .to_string(),
            is_error: false,
            duration_ms,
        })
    }
}
