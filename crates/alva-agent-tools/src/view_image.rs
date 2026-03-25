// INPUT:  alva_types, async_trait, serde, serde_json, base64, crate::local_fs::LocalToolFs
// OUTPUT: ViewImageTool
// POS:    Reads image files and returns base64-encoded content with MIME type detection.
//! view_image — read an image file and return its base64-encoded content

use alva_types::{AgentError, CancellationToken, Tool, ToolContext, ToolResult};
use async_trait::async_trait;
use serde::Deserialize;
use serde_json::{json, Value};
use std::path::Path;

use crate::local_fs::LocalToolFs;

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

        let local = ctx.local().ok_or_else(|| AgentError::ToolError {
            tool_name: "view_image".into(),
            message: "local filesystem context required".into(),
        })?;

        // Resolve path: if absolute use directly, otherwise relative to workspace
        let file_path = if Path::new(&params.path).is_absolute() {
            std::path::PathBuf::from(&params.path)
        } else {
            local.workspace().join(&params.path)
        };

        let fallback = LocalToolFs::new(local.workspace());
        let fs = ctx.tool_fs().unwrap_or(&fallback);

        // Verify file exists
        let path_str = file_path.to_str().unwrap_or_default();
        if !fs.exists(path_str).await.map_err(|e| AgentError::ToolError { tool_name: "view_image".into(), message: e.to_string() })? {
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
        let data = fs
            .read_file(path_str)
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
