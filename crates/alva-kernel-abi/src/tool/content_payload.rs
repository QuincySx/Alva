// INPUT:  serde, serde_json
// OUTPUT: ToolContent, ToolOutput, ProgressEvent
// POS:    Pure-serde tool payload types, split out of execution.rs so they carry
//         zero bus/runtime coupling and can be re-exported by alva-llm-wire later.
use serde::{Deserialize, Serialize};

// ---------------------------------------------------------------------------
// ProgressEvent — intermediate progress from tool execution
// ---------------------------------------------------------------------------

/// Intermediate progress events emitted during tool execution.
///
/// Consumers (e.g., the run loop or UI layer) can subscribe to these
/// for real-time feedback without waiting for the final `ToolOutput`.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum ProgressEvent {
    /// A line written to stdout by a subprocess.
    #[serde(rename = "stdout_line")]
    StdoutLine { line: String },
    /// A line written to stderr by a subprocess.
    #[serde(rename = "stderr_line")]
    StderrLine { line: String },
    /// A human-readable status message (e.g., "compiling...", "downloading 3/10").
    #[serde(rename = "status")]
    Status { message: String },
    /// Arbitrary structured data for tool-specific progress.
    #[serde(rename = "custom")]
    Custom { data: serde_json::Value },
}

// ---------------------------------------------------------------------------
// ToolContent — multi-modal content returned to the model
// ---------------------------------------------------------------------------

/// A single content block in a tool response.
///
/// Supports text and image payloads so tools like browser screenshots
/// or diagram generators can return rich content to the model.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum ToolContent {
    /// Plain text content.
    #[serde(rename = "text")]
    Text { text: String },
    /// Base64-encoded image with its MIME type.
    #[serde(rename = "image")]
    Image { data: String, media_type: String },
}

impl ToolContent {
    /// Create a text content block.
    pub fn text(s: impl Into<String>) -> Self {
        ToolContent::Text { text: s.into() }
    }

    /// Create an image content block.
    pub fn image(data: impl Into<String>, media_type: impl Into<String>) -> Self {
        ToolContent::Image {
            data: data.into(),
            media_type: media_type.into(),
        }
    }

    /// If this is a text block, return its text.
    pub fn as_text(&self) -> Option<&str> {
        match self {
            ToolContent::Text { text } => Some(text),
            _ => None,
        }
    }

    /// Collapse this content block to a string suitable for the model.
    ///
    /// Text blocks return their text; image blocks return a placeholder
    /// (the actual image data is sent via the content block mechanism).
    pub fn to_model_string(&self) -> String {
        match self {
            ToolContent::Text { text } => text.clone(),
            ToolContent::Image { media_type, .. } => {
                format!("[image: {}]", media_type)
            }
        }
    }
}

// ---------------------------------------------------------------------------
// ToolOutput — replaces ToolResult with multi-modal support
// ---------------------------------------------------------------------------

/// The result of executing a tool — multi-modal content plus optional details.
///
/// Replaces the old `ToolResult` which only supported a single string.
/// `content` can hold multiple blocks (text, images, etc.) that are sent
/// back to the model. `details` carries structured metadata for the UI
/// or middleware but is not shown to the model.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolOutput {
    /// Content blocks returned to the model.
    pub content: Vec<ToolContent>,
    /// Whether this result represents an error.
    pub is_error: bool,
    /// Structured metadata for the UI/middleware (not sent to model).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub details: Option<serde_json::Value>,
}

impl ToolOutput {
    /// Create a successful text-only output.
    pub fn text(s: impl Into<String>) -> Self {
        ToolOutput {
            content: vec![ToolContent::text(s)],
            is_error: false,
            details: None,
        }
    }

    /// Create an error text-only output.
    pub fn error(s: impl Into<String>) -> Self {
        ToolOutput {
            content: vec![ToolContent::text(s)],
            is_error: true,
            details: None,
        }
    }

    /// Concatenate all content blocks into a single string for the model.
    ///
    /// Useful when you need a flat string representation (e.g., for
    /// backward-compatible code paths that still expect `ToolResult.content`).
    pub fn model_text(&self) -> String {
        self.content
            .iter()
            .map(|c| c.to_model_string())
            .collect::<Vec<_>>()
            .join("\n")
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tool_content_text_helpers() {
        let c = ToolContent::text("hello");
        assert_eq!(c.as_text(), Some("hello"));
        assert_eq!(c.to_model_string(), "hello");
    }

    #[test]
    fn tool_content_image_helpers() {
        let c = ToolContent::image("base64data", "image/png");
        assert_eq!(c.as_text(), None);
        assert_eq!(c.to_model_string(), "[image: image/png]");
    }

    #[test]
    fn tool_output_text_convenience() {
        let out = ToolOutput::text("ok");
        assert!(!out.is_error);
        assert_eq!(out.model_text(), "ok");
        assert!(out.details.is_none());
    }

    #[test]
    fn tool_output_error_convenience() {
        let out = ToolOutput::error("fail");
        assert!(out.is_error);
        assert_eq!(out.model_text(), "fail");
    }

    #[test]
    fn tool_output_model_text_multimodal() {
        let out = ToolOutput {
            content: vec![
                ToolContent::text("Description:"),
                ToolContent::image("data", "image/jpeg"),
            ],
            is_error: false,
            details: None,
        };
        assert_eq!(out.model_text(), "Description:\n[image: image/jpeg]");
    }

    #[test]
    fn progress_event_serde_roundtrip() {
        let events = vec![
            ProgressEvent::StdoutLine {
                line: "hello".into(),
            },
            ProgressEvent::StderrLine {
                line: "warn".into(),
            },
            ProgressEvent::Status {
                message: "compiling".into(),
            },
            ProgressEvent::Custom {
                data: serde_json::json!({"step": 3}),
            },
        ];

        for event in events {
            let json = serde_json::to_string(&event).unwrap();
            let roundtrip: ProgressEvent = serde_json::from_str(&json).unwrap();
            // Verify the tag is present in JSON
            assert!(json.contains("\"type\""));
            // Verify roundtrip produces equivalent debug output
            assert_eq!(format!("{:?}", event), format!("{:?}", roundtrip));
        }
    }

    #[test]
    fn tool_output_serde_roundtrip() {
        let out = ToolOutput {
            content: vec![
                ToolContent::text("hello"),
                ToolContent::image("data", "image/png"),
            ],
            is_error: false,
            details: Some(serde_json::json!({"exit_code": 0})),
        };
        let json = serde_json::to_string(&out).unwrap();
        let roundtrip: ToolOutput = serde_json::from_str(&json).unwrap();
        assert_eq!(out.model_text(), roundtrip.model_text());
        assert_eq!(out.is_error, roundtrip.is_error);
        assert_eq!(out.details, roundtrip.details);
    }
}
