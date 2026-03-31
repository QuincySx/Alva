// INPUT:  alva_types, async_trait, serde, serde_json, base64, crate::local_fs::LocalToolFs
// OUTPUT: ReadFileTool
// POS:    Read file contents with offset/limit pagination, automatic image detection, and smart truncation.
//! read_file — read text or image files with pagination and truncation

use alva_types::{AgentError, Tool, ToolContent, ToolExecutionContext, ToolOutput};
use async_trait::async_trait;
use serde::Deserialize;
use serde_json::{json, Value};
use std::path::Path;

use crate::local_fs::LocalToolFs;
use crate::truncate::{truncate_head, MAX_BYTES, MAX_LINES};

#[derive(Debug, Deserialize)]
struct Input {
    path: String,
    #[serde(default)]
    offset: Option<usize>,
    #[serde(default)]
    limit: Option<usize>,
}

pub struct ReadFileTool;

/// Detect image MIME type from file header bytes (magic bytes).
fn detect_image_mime(data: &[u8]) -> Option<&'static str> {
    if data.len() < 4 {
        return None;
    }
    // PNG: 89 50 4E 47
    if data.starts_with(&[0x89, 0x50, 0x4E, 0x47]) {
        return Some("image/png");
    }
    // JPEG: FF D8 FF
    if data.starts_with(&[0xFF, 0xD8, 0xFF]) {
        return Some("image/jpeg");
    }
    // GIF: GIF87a or GIF89a
    if data.starts_with(b"GIF8") {
        return Some("image/gif");
    }
    // WebP: RIFF....WEBP
    if data.len() >= 12 && data.starts_with(b"RIFF") && &data[8..12] == b"WEBP" {
        return Some("image/webp");
    }
    // BMP: BM
    if data.starts_with(b"BM") {
        return Some("image/bmp");
    }
    None
}

/// Add line numbers to text content.
fn add_line_numbers(text: &str, start_line: usize) -> String {
    text.lines()
        .enumerate()
        .map(|(i, line)| format!("{:>6}\t{}", start_line + i, line))
        .collect::<Vec<_>>()
        .join("\n")
}

#[async_trait]
impl Tool for ReadFileTool {
    fn name(&self) -> &str {
        "read_file"
    }

    fn description(&self) -> &str {
        "Read file contents. Returns text with line numbers for code/text files, \
         or base64-encoded image data for image files. Supports offset/limit for \
         paginated reading of large files."
    }

    fn parameters_schema(&self) -> Value {
        json!({
            "type": "object",
            "required": ["path"],
            "properties": {
                "path": {
                    "type": "string",
                    "description": "Path to the file (absolute or relative to workspace)"
                },
                "offset": {
                    "type": "integer",
                    "description": "Line number to start reading from (1-indexed). Default: 1"
                },
                "limit": {
                    "type": "integer",
                    "description": "Maximum number of lines to read. Default: up to 2000 lines or 50KB"
                }
            }
        })
    }

    async fn execute(
        &self,
        input: Value,
        ctx: &dyn ToolExecutionContext,
    ) -> Result<ToolOutput, AgentError> {
        let params: Input = serde_json::from_value(input)
            .map_err(|e| AgentError::ToolError {
                tool_name: self.name().into(),
                message: e.to_string(),
            })?;

        let workspace = ctx.workspace().ok_or_else(|| AgentError::ToolError {
            tool_name: self.name().into(),
            message: "workspace required".into(),
        })?;

        // Resolve path
        let file_path = if Path::new(&params.path).is_absolute() {
            std::path::PathBuf::from(&params.path)
        } else {
            workspace.join(&params.path)
        };

        let fallback = LocalToolFs::new(workspace);
        let fs = ctx.tool_fs().unwrap_or(&fallback);
        let path_str = file_path.to_str().unwrap_or_default();

        // Check file exists
        if !fs.exists(path_str).await.map_err(|e| AgentError::ToolError {
            tool_name: self.name().into(),
            message: e.to_string(),
        })? {
            return Ok(ToolOutput::error(format!(
                "File not found: {}",
                params.path
            )));
        }

        // Read raw bytes
        let data = fs.read_file(path_str).await.map_err(|e| AgentError::ToolError {
            tool_name: self.name().into(),
            message: format!("Failed to read file: {e}"),
        })?;

        // Check if it's an image (magic bytes)
        if let Some(mime) = detect_image_mime(&data) {
            return self.handle_image(&data, mime, &params.path);
        }

        // Handle as text
        let text = String::from_utf8_lossy(&data);
        let all_lines: Vec<&str> = text.lines().collect();
        let total_lines = all_lines.len();

        // Apply offset (1-indexed)
        let offset = params.offset.unwrap_or(1).max(1);
        if offset > total_lines {
            return Ok(ToolOutput::text(format!(
                "Offset {} is beyond end of file ({} lines total)",
                offset, total_lines
            )));
        }
        let start_idx = offset - 1;

        // Apply user limit, then delegate to shared truncation
        let user_limit = params.limit.unwrap_or(usize::MAX);
        let effective_limit = user_limit.min(MAX_LINES);
        let end_idx = (start_idx + effective_limit).min(total_lines);
        let slice_text = all_lines[start_idx..end_idx].join("\n");

        let tr = truncate_head(&slice_text, effective_limit, MAX_BYTES);
        let lines_shown = tr.shown_lines;

        // Build output
        let numbered = add_line_numbers(&tr.text, offset);
        let remaining = total_lines - (start_idx + lines_shown);

        // Build continuation hint
        let mut content = numbered;
        if remaining > 0 {
            let truncation_reason = if tr.truncated {
                "byte limit"
            } else if effective_limit < user_limit {
                "line limit"
            } else {
                "limit"
            };
            content.push_str(&format!(
                "\n\n[Showing lines {}-{} of {}. Truncated by {}. Use offset={} to continue.]",
                offset,
                offset + lines_shown - 1,
                total_lines,
                truncation_reason,
                offset + lines_shown,
            ));
        }

        Ok(ToolOutput {
            content: vec![ToolContent::text(content)],
            is_error: false,
            details: Some(json!({
                "path": params.path,
                "total_lines": total_lines,
                "lines_shown": lines_shown,
                "offset": offset,
                "truncated": remaining > 0,
            })),
        })
    }
}

impl ReadFileTool {
    fn handle_image(
        &self,
        data: &[u8],
        mime: &str,
        path: &str,
    ) -> Result<ToolOutput, AgentError> {
        let file_size = data.len();

        // Size guard: 10MB max
        if file_size > 10 * 1024 * 1024 {
            return Ok(ToolOutput::error(format!(
                "Image too large: {} bytes (max 10MB). Use a smaller image or resize first.",
                file_size
            )));
        }

        use base64::Engine;
        let b64 = base64::engine::general_purpose::STANDARD.encode(data);

        Ok(ToolOutput {
            content: vec![
                ToolContent::text(format!(
                    "Image file: {} ({}, {} bytes)",
                    path, mime, file_size
                )),
                ToolContent::image(b64, mime),
            ],
            is_error: false,
            details: Some(json!({
                "path": path,
                "mime_type": mime,
                "size_bytes": file_size,
            })),
        })
    }
}
