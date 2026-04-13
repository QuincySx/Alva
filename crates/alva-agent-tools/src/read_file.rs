// INPUT:  alva_types, async_trait, base64, schemars, serde, serde_json, crate::local_fs::LocalToolFs
// OUTPUT: ReadFileTool
// POS:    Read file contents with offset/limit pagination, automatic image detection,
//         encoding detection, PDF page support, and smart truncation.
//! read_file — read text or image files with pagination and truncation

use alva_types::{AgentError, Tool, ToolContent, ToolExecutionContext, ToolOutput};
use schemars::JsonSchema;
use serde::Deserialize;
use serde_json::json;
use std::path::Path;

use crate::local_fs::LocalToolFs;
use crate::truncate::{truncate_head, MAX_BYTES, MAX_LINES};

#[derive(Debug, Deserialize, JsonSchema)]
struct Input {
    /// Path to the file (absolute or relative to workspace).
    path: String,
    /// Line number to start reading from (1-indexed). Default: 1.
    #[serde(default)]
    offset: Option<usize>,
    /// Maximum number of lines to read. Default: up to 2000 lines or 50KB.
    #[serde(default)]
    limit: Option<usize>,
    /// Page range for PDF files (e.g., "1-5", "3", "10-20"). Max 20 pages per request.
    #[serde(default)]
    pages: Option<String>,
}

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

/// Check if a file is a PDF based on magic bytes.
#[allow(unused)]
fn is_pdf(data: &[u8]) -> bool {
    data.len() >= 5 && data.starts_with(b"%PDF-")
}

/// Detect text encoding from BOM or content analysis.
/// Returns detected encoding name and the byte offset to skip BOM.
#[allow(unused)]
fn detect_encoding(data: &[u8]) -> (&'static str, usize) {
    // Check BOM
    if data.len() >= 3 && data[0] == 0xEF && data[1] == 0xBB && data[2] == 0xBF {
        return ("utf-8-bom", 3);
    }
    if data.len() >= 2 && data[0] == 0xFE && data[1] == 0xFF {
        return ("utf-16-be", 2);
    }
    if data.len() >= 2 && data[0] == 0xFF && data[1] == 0xFE {
        return ("utf-16-le", 2);
    }
    // Default: assume UTF-8 (no BOM)
    ("utf-8", 0)
}

/// Add line numbers to text content.
fn add_line_numbers(text: &str, start_line: usize) -> String {
    text.lines()
        .enumerate()
        .map(|(i, line)| format!("{:>6}\t{}", start_line + i, line))
        .collect::<Vec<_>>()
        .join("\n")
}

/// Parse a page range string like "1-5", "3", "10-20".
/// Returns (start_page, end_page) 1-indexed, clamped to max 20 pages.
#[allow(unused)]
fn parse_page_range(range: &str) -> Result<(usize, usize), String> {
    let range = range.trim();
    if range.contains('-') {
        let parts: Vec<&str> = range.splitn(2, '-').collect();
        let start: usize = parts[0].trim().parse()
            .map_err(|_| format!("Invalid page start: {}", parts[0]))?;
        let end: usize = parts[1].trim().parse()
            .map_err(|_| format!("Invalid page end: {}", parts[1]))?;
        if start == 0 || end == 0 {
            return Err("Page numbers are 1-indexed".into());
        }
        if end < start {
            return Err(format!("End page {} is before start page {}", end, start));
        }
        let clamped_end = end.min(start + 19); // max 20 pages
        Ok((start, clamped_end))
    } else {
        let page: usize = range.parse()
            .map_err(|_| format!("Invalid page number: {}", range))?;
        if page == 0 {
            return Err("Page numbers are 1-indexed".into());
        }
        Ok((page, page))
    }
}

#[derive(Tool)]
#[tool(
    name = "read_file",
    description = "Read file contents. Returns text with line numbers for code/text files, \
        or base64-encoded image data for image files. Supports offset/limit for \
        paginated reading of large files. Use pages parameter for PDF files.",
    input = Input,
    read_only,
    concurrency_safe,
)]
pub struct ReadFileTool;

impl ReadFileTool {
    async fn execute_impl(
        &self,
        params: Input,
        ctx: &dyn ToolExecutionContext,
    ) -> Result<ToolOutput, AgentError> {
        let workspace = ctx.workspace().ok_or_else(|| AgentError::ToolError {
            tool_name: "read_file".into(),
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
            tool_name: "read_file".into(),
            message: e.to_string(),
        })? {
            return Ok(ToolOutput::error(format!(
                "File not found: {}",
                params.path
            )));
        }

        // Read raw bytes
        let data = fs.read_file(path_str).await.map_err(|e| AgentError::ToolError {
            tool_name: "read_file".into(),
            message: format!("Failed to read file: {e}"),
        })?;

        // Record the read for staleness detection by FileEditTool
        crate::file_edit::record_file_read(path_str, crate::file_edit::content_hash(&data));

        // Check if it's an image (magic bytes)
        if let Some(mime) = detect_image_mime(&data) {
            return self.handle_image(&data, mime, &params.path);
        }

        // Check if it's a PDF
        if is_pdf(&data) {
            return self.handle_pdf(&params.path, &params.pages);
        }

        // Detect encoding
        let (encoding, bom_skip) = detect_encoding(&data);
        let text_data = &data[bom_skip..];

        // Handle as text
        let text = String::from_utf8_lossy(text_data);
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

        let mut details = json!({
            "path": params.path,
            "total_lines": total_lines,
            "lines_shown": lines_shown,
            "offset": offset,
            "truncated": remaining > 0,
        });

        if encoding != "utf-8" {
            details["encoding"] = json!(encoding);
        }

        Ok(ToolOutput {
            content: vec![ToolContent::text(content)],
            is_error: false,
            details: Some(details),
        })
    }

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

    /// Handle PDF files by returning metadata and page info.
    /// Full PDF text extraction would require a dedicated library;
    /// for now we provide file info and suggest the pages parameter.
    #[allow(unused)]
    fn handle_pdf(
        &self,
        path: &str,
        pages: &Option<String>,
    ) -> Result<ToolOutput, AgentError> {
        let mut content = format!("PDF file: {}", path);

        if let Some(ref page_range) = pages {
            match parse_page_range(page_range) {
                Ok((start, end)) => {
                    content.push_str(&format!(
                        "\nRequested pages {}-{}. PDF text extraction requires a dedicated library. \
                         Consider using a shell command like `pdftotext` to extract text.",
                        start, end
                    ));
                }
                Err(e) => {
                    return Ok(ToolOutput::error(format!("Invalid page range: {}", e)));
                }
            }
        } else {
            content.push_str(
                "\nThis is a PDF file. Use the `pages` parameter to specify which pages to read \
                 (e.g., pages: \"1-5\"). Consider using `pdftotext` via execute_shell for text extraction."
            );
        }

        Ok(ToolOutput {
            content: vec![ToolContent::text(content)],
            is_error: false,
            details: Some(json!({
                "path": path,
                "file_type": "pdf",
            })),
        })
    }
}
