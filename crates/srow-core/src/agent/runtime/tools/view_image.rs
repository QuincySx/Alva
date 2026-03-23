// INPUT:  agent_types, async_trait, serde, serde_json, base64, tokio::fs
// OUTPUT: ViewImageTool
// POS:    Reads image files and returns base64-encoded content with MIME type detection.
//! view_image — read an image file and return its base64-encoded content

use agent_types::{AgentError, CancellationToken, Tool, ToolContext, ToolResult};
use async_trait::async_trait;
use serde::Deserialize;
use serde_json::{json, Value};
use std::path::Path;

#[derive(Debug, Deserialize)]
struct Input {
    path: String,
}

pub struct ViewImageTool;

#[async_trait]
impl Tool for ViewImageTool {
    fn name(&self) -> &str {
        "view_image"
    }

    fn description(&self) -> &str {
        "Read an image file from disk and return its base64-encoded content with MIME type. Supports PNG, JPEG, GIF, WebP, BMP, SVG, and ICO formats."
    }

    fn parameters_schema(&self) -> Value {
        json!({
            "type": "object",
            "required": ["path"],
            "properties": {
                "path": {
                    "type": "string",
                    "description": "Absolute or workspace-relative path to the image file"
                }
            }
        })
    }

    async fn execute(&self, input: Value, _cancel: &CancellationToken, ctx: &dyn ToolContext) -> Result<ToolResult, AgentError> {
        let params: Input =
            serde_json::from_value(input).map_err(|e| AgentError::ToolError { tool_name: "view_image".into(), message: e.to_string() })?;

        // Resolve path: if absolute use directly, otherwise relative to workspace
        let file_path = if Path::new(&params.path).is_absolute() {
            std::path::PathBuf::from(&params.path)
        } else {
            ctx.workspace().join(&params.path)
        };

        // Verify file exists
        if !file_path.exists() {
            return Err(AgentError::ToolError {
                tool_name: "view_image".into(),
                message: format!("Image file not found: {}", file_path.display()),
            });
        }

        // Determine MIME type from extension
        let extension = file_path
            .extension()
            .and_then(|e| e.to_str())
            .unwrap_or("")
            .to_lowercase();

        let mime_type = match extension.as_str() {
            "png" => "image/png",
            "jpg" | "jpeg" => "image/jpeg",
            "gif" => "image/gif",
            "webp" => "image/webp",
            "bmp" => "image/bmp",
            "svg" => "image/svg+xml",
            "ico" => "image/x-icon",
            other => {
                return Err(AgentError::ToolError {
                    tool_name: "view_image".into(),
                    message: format!("Unsupported image format: .{}", other),
                });
            }
        };

        // Read file
        let data = tokio::fs::read(&file_path)
            .await
            .map_err(|e| AgentError::ToolError { tool_name: "view_image".into(), message: format!("Failed to read image: {e}") })?;

        let file_size = data.len();

        // Size guard: don't encode files > 10MB
        if file_size > 10 * 1024 * 1024 {
            return Err(AgentError::ToolError {
                tool_name: "view_image".into(),
                message: format!("Image file too large: {} bytes (max 10MB)", file_size),
            });
        }

        // Encode to base64
        use base64::Engine;
        let b64 = base64::engine::general_purpose::STANDARD.encode(&data);

        let output = json!({
            "path": file_path.display().to_string(),
            "mime_type": mime_type,
            "base64": b64,
            "file_size_bytes": file_size,
        });

        Ok(ToolResult {
            content: serde_json::to_string_pretty(&output)
                .unwrap_or_else(|_| "{}".to_string()),
            is_error: false,
            details: None,
        })
    }
}
