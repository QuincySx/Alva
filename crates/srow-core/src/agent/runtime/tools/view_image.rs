// INPUT:  crate::domain::tool, crate::error, crate::ports::tool, async_trait, serde, serde_json, base64, tokio::fs
// OUTPUT: ViewImageTool
// POS:    Reads image files and returns base64-encoded content with MIME type detection.
//! view_image — read an image file and return its base64-encoded content

use crate::domain::tool::{ToolDefinition, ToolResult};
use crate::error::EngineError;
use crate::ports::tool::{Tool, ToolContext};
use async_trait::async_trait;
use serde::Deserialize;
use serde_json::{json, Value};
use std::path::Path;
use std::time::Instant;

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

    fn definition(&self) -> ToolDefinition {
        ToolDefinition {
            name: "view_image".to_string(),
            description: "Read an image file from disk and return its base64-encoded content with MIME type. Supports PNG, JPEG, GIF, WebP, BMP, SVG, and ICO formats.".to_string(),
            parameters: json!({
                "type": "object",
                "required": ["path"],
                "properties": {
                    "path": {
                        "type": "string",
                        "description": "Absolute or workspace-relative path to the image file"
                    }
                }
            }),
        }
    }

    async fn execute(&self, input: Value, ctx: &ToolContext) -> Result<ToolResult, EngineError> {
        let params: Input =
            serde_json::from_value(input).map_err(|e| EngineError::ToolExecution(e.to_string()))?;

        let start = Instant::now();

        // Resolve path: if absolute use directly, otherwise relative to workspace
        let file_path = if Path::new(&params.path).is_absolute() {
            std::path::PathBuf::from(&params.path)
        } else {
            ctx.workspace.join(&params.path)
        };

        // Verify file exists
        if !file_path.exists() {
            return Err(EngineError::ToolExecution(format!(
                "Image file not found: {}",
                file_path.display()
            )));
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
                return Err(EngineError::ToolExecution(format!(
                    "Unsupported image format: .{}",
                    other
                )));
            }
        };

        // Read file
        let data = tokio::fs::read(&file_path)
            .await
            .map_err(|e| EngineError::ToolExecution(format!("Failed to read image: {e}")))?;

        let file_size = data.len();

        // Size guard: don't encode files > 10MB
        if file_size > 10 * 1024 * 1024 {
            return Err(EngineError::ToolExecution(format!(
                "Image file too large: {} bytes (max 10MB)",
                file_size
            )));
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

        let duration_ms = start.elapsed().as_millis() as u64;

        Ok(ToolResult {
            tool_call_id: String::new(),
            tool_name: "view_image".to_string(),
            output: serde_json::to_string_pretty(&output)
                .unwrap_or_else(|_| "{}".to_string()),
            is_error: false,
            duration_ms,
        })
    }
}
