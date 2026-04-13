// INPUT:  alva_types, async_trait, base64, schemars, serde, crate::local_fs::LocalToolFs
// OUTPUT: ViewImageTool
// POS:    Reads image files and returns base64-encoded content with MIME type detection.
//! view_image — read an image file and return its base64-encoded content

use alva_types::{AgentError, Tool, ToolContent, ToolExecutionContext, ToolOutput};
use schemars::JsonSchema;
use serde::Deserialize;
use std::path::Path;

use crate::local_fs::LocalToolFs;

#[derive(Debug, Deserialize, JsonSchema)]
struct Input {
    /// Absolute or workspace-relative path to the image file.
    path: String,
}

#[derive(Tool)]
#[tool(
    name = "view_image",
    description = "Read an image file from disk and return its base64-encoded content with MIME type. Supports PNG, JPEG, GIF, WebP, BMP, SVG, and ICO formats.",
    input = Input,
    read_only,
    concurrency_safe,
)]
pub struct ViewImageTool;

impl ViewImageTool {
    async fn execute_impl(
        &self,
        params: Input,
        ctx: &dyn ToolExecutionContext,
    ) -> Result<ToolOutput, AgentError> {
        let workspace = ctx.workspace().ok_or_else(|| AgentError::ToolError {
            tool_name: "view_image".into(),
            message: "local filesystem context required".into(),
        })?;

        // Resolve path: if absolute use directly, otherwise relative to workspace
        let file_path = if Path::new(&params.path).is_absolute() {
            std::path::PathBuf::from(&params.path)
        } else {
            workspace.join(&params.path)
        };

        let fallback = LocalToolFs::new(workspace);
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

        // Return multi-modal: text description + image content block
        Ok(ToolOutput {
            content: vec![
                ToolContent::text(format!(
                    "Image: {} ({}, {} bytes)",
                    file_path.display(),
                    mime_type,
                    file_size
                )),
                ToolContent::image(b64, mime_type),
            ],
            is_error: false,
            details: None,
        })
    }
}
