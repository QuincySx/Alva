//! read_url — fetch a web page and return its plain-text content (HTML tags stripped)

use crate::domain::tool::{ToolDefinition, ToolResult};
use crate::error::EngineError;
use crate::ports::tool::{Tool, ToolContext};
use async_trait::async_trait;
use serde::Deserialize;
use serde_json::{json, Value};
use std::time::Instant;

#[derive(Debug, Deserialize)]
struct Input {
    url: String,
    /// Maximum content length in characters (default: 50000)
    max_length: Option<usize>,
}

pub struct ReadUrlTool;

#[async_trait]
impl Tool for ReadUrlTool {
    fn name(&self) -> &str {
        "read_url"
    }

    fn definition(&self) -> ToolDefinition {
        ToolDefinition {
            name: "read_url".to_string(),
            description: "Fetch a web page URL and return its plain-text content with HTML tags removed. Useful for reading articles, documentation, or any web content.".to_string(),
            parameters: json!({
                "type": "object",
                "required": ["url"],
                "properties": {
                    "url": {
                        "type": "string",
                        "description": "The URL to fetch"
                    },
                    "max_length": {
                        "type": "integer",
                        "description": "Maximum content length in characters (default: 50000)"
                    }
                }
            }),
        }
    }

    async fn execute(&self, input: Value, _ctx: &ToolContext) -> Result<ToolResult, EngineError> {
        let params: Input =
            serde_json::from_value(input).map_err(|e| EngineError::ToolExecution(e.to_string()))?;

        let start = Instant::now();
        let max_length = params.max_length.unwrap_or(50_000);

        let client = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(30))
            .user_agent("SrowAgent/0.1 (compatible; bot)")
            .redirect(reqwest::redirect::Policy::limited(5))
            .build()
            .map_err(|e| EngineError::ToolExecution(format!("HTTP client error: {e}")))?;

        let resp = client
            .get(&params.url)
            .send()
            .await
            .map_err(|e| EngineError::ToolExecution(format!("HTTP request failed: {e}")))?;

        let status = resp.status();
        let content_type = resp
            .headers()
            .get("content-type")
            .and_then(|v| v.to_str().ok())
            .unwrap_or("")
            .to_string();

        if !status.is_success() {
            return Err(EngineError::ToolExecution(format!(
                "HTTP {} for URL: {}",
                status, params.url
            )));
        }

        let body = resp
            .text()
            .await
            .map_err(|e| EngineError::ToolExecution(format!("Failed to read response body: {e}")))?;

        // Strip HTML tags and extract text
        let plain_text = if content_type.contains("text/html") || content_type.contains("application/xhtml") {
            strip_html_tags(&body)
        } else {
            // Already plain text (JSON, text/plain, etc.)
            body
        };

        // Truncate if needed
        let truncated = plain_text.len() > max_length;
        let content = if truncated {
            // Truncate at a char boundary
            let end = plain_text
                .char_indices()
                .nth(max_length)
                .map(|(i, _)| i)
                .unwrap_or(plain_text.len());
            format!("{}...\n\n[Truncated: content exceeded {} characters]", &plain_text[..end], max_length)
        } else {
            plain_text
        };

        let output = json!({
            "url": params.url,
            "content": content,
            "content_type": content_type,
            "truncated": truncated,
        });

        let duration_ms = start.elapsed().as_millis() as u64;

        Ok(ToolResult {
            tool_call_id: String::new(),
            tool_name: "read_url".to_string(),
            output: serde_json::to_string_pretty(&output)
                .unwrap_or_else(|_| "{}".to_string()),
            is_error: false,
            duration_ms,
        })
    }
}

/// Simple HTML tag stripper.
///
/// Removes all HTML tags, decodes common entities, and collapses whitespace.
/// This is intentionally simple (no headless browser needed).
fn strip_html_tags(html: &str) -> String {
    let mut result = String::with_capacity(html.len() / 2);
    let mut in_tag = false;
    let mut in_script = false;
    let mut in_style = false;
    let mut tag_name = String::new();
    let mut collecting_tag_name = false;

    for ch in html.chars() {
        if ch == '<' {
            in_tag = true;
            tag_name.clear();
            collecting_tag_name = true;
            continue;
        }
        if ch == '>' {
            in_tag = false;
            collecting_tag_name = false;
            let tag_lower = tag_name.to_lowercase();
            if tag_lower == "script" {
                in_script = true;
            } else if tag_lower == "/script" {
                in_script = false;
            } else if tag_lower == "style" {
                in_style = true;
            } else if tag_lower == "/style" {
                in_style = false;
            } else if tag_lower == "br" || tag_lower == "br/" || tag_lower == "p" || tag_lower == "/p"
                || tag_lower == "div" || tag_lower == "/div" || tag_lower == "li"
                || tag_lower == "h1" || tag_lower == "h2" || tag_lower == "h3"
                || tag_lower == "h4" || tag_lower == "h5" || tag_lower == "h6"
                || tag_lower == "/h1" || tag_lower == "/h2" || tag_lower == "/h3"
                || tag_lower == "/h4" || tag_lower == "/h5" || tag_lower == "/h6"
                || tag_lower == "tr" || tag_lower == "/tr"
            {
                result.push('\n');
            } else if tag_lower == "td" || tag_lower == "th" {
                result.push('\t');
            }
            continue;
        }
        if in_tag {
            if collecting_tag_name && (ch.is_alphanumeric() || ch == '/') {
                tag_name.push(ch);
            } else {
                collecting_tag_name = false;
            }
            continue;
        }
        if in_script || in_style {
            continue;
        }
        result.push(ch);
    }

    // Decode common HTML entities
    let result = result
        .replace("&amp;", "&")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .replace("&#39;", "'")
        .replace("&apos;", "'")
        .replace("&nbsp;", " ");

    // Collapse multiple blank lines into at most two newlines
    let mut collapsed = String::with_capacity(result.len());
    let mut consecutive_newlines = 0u32;
    for ch in result.chars() {
        if ch == '\n' {
            consecutive_newlines += 1;
            if consecutive_newlines <= 2 {
                collapsed.push('\n');
            }
        } else if ch == '\r' {
            // skip CR
        } else {
            consecutive_newlines = 0;
            collapsed.push(ch);
        }
    }

    // Trim leading/trailing whitespace on each line
    collapsed
        .lines()
        .map(|l| l.trim())
        .collect::<Vec<_>>()
        .join("\n")
        .trim()
        .to_string()
}
