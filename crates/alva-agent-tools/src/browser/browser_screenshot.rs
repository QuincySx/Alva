// INPUT:  alva_kernel_abi, async_trait, base64, chromiumoxide::cdp, schemars, serde, serde_json, super::browser_manager
// OUTPUT: BrowserScreenshotTool
// POS:    Captures page screenshots (viewport, full-page, or element) and saves to file.
//! browser_screenshot — capture page screenshot

use alva_kernel_abi::{AgentError, Tool, ToolExecutionContext, ToolOutput};
use chromiumoxide::cdp::browser_protocol::page::{
    CaptureScreenshotFormat, CaptureScreenshotParams,
};
use schemars::JsonSchema;
use serde::Deserialize;
use serde_json::json;

use super::browser_manager::SharedBrowserManager;

/// Image format for the screenshot.
#[derive(Debug, Deserialize, JsonSchema)]
#[serde(rename_all = "lowercase")]
enum ImageFormat {
    Png,
    Jpeg,
}

#[derive(Debug, Deserialize, JsonSchema)]
struct Input {
    /// Output file path (relative to workspace). Default: 'screenshot.png'.
    #[serde(default)]
    path: Option<String>,
    /// Capture the full scrollable page, not just the viewport. Default: false.
    #[serde(default)]
    full_page: Option<bool>,
    /// CSS selector to screenshot a specific element instead of the page.
    #[serde(default)]
    selector: Option<String>,
    /// Image format. Default: 'png'.
    #[serde(default)]
    format: Option<ImageFormat>,
    /// JPEG quality (0-100). Only used with format='jpeg'. Default: 80.
    #[serde(default)]
    quality: Option<i64>,
    /// Browser instance ID. Default: 'default'.
    #[serde(default)]
    id: Option<String>,
}

#[derive(Tool)]
#[tool(
    name = "browser_screenshot",
    description = "Capture a screenshot of the current page. Can capture the viewport, full page, or a specific element. Saves to a file and returns the path.",
    input = Input,
    read_only,
)]
pub struct BrowserScreenshotTool {
    pub manager: SharedBrowserManager,
}

impl BrowserScreenshotTool {
    async fn execute_impl(
        &self,
        params: Input,
        ctx: &dyn ToolExecutionContext,
    ) -> Result<ToolOutput, AgentError> {
        let id = params.id.unwrap_or_else(|| "default".to_string());
        let format_str = match params.format {
            Some(ImageFormat::Jpeg) => "jpeg",
            Some(ImageFormat::Png) | None => "png",
        };
        let output_path = params
            .path
            .clone()
            .unwrap_or_else(|| format!("screenshot.{}", format_str));

        let workspace = ctx.workspace().ok_or_else(|| AgentError::ToolError {
            tool_name: "browser_screenshot".into(),
            message: "local filesystem context required".into(),
        })?;
        let full_path = if std::path::Path::new(&output_path).is_absolute() {
            std::path::PathBuf::from(&output_path)
        } else {
            workspace.join(&output_path)
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

        Ok(ToolOutput::text(json!({
            "status": "captured",
            "path": full_path.display().to_string(),
            "format": format_str,
            "size_bytes": result.len(),
        })
        .to_string()))
    }
}
