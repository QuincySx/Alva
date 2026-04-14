// INPUT:  async_trait, serde, serde_json, CancellationToken, ToolFs, alva_kernel_bus::BusHandle
// OUTPUT: ProgressEvent, ToolContent, ToolOutput, ToolExecutionContext (trait), MinimalExecutionContext
// POS:    Unified execution context and multi-modal output types — ToolExecutionContext exposes bus() for tool-level capability discovery.
use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use std::any::Any;
use std::path::Path;

use crate::base::cancel::CancellationToken;
use super::types::ToolFs;
use alva_kernel_bus::BusHandle;

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
// ToolExecutionContext — unified context trait
// ---------------------------------------------------------------------------

/// Unified execution context passed to `Tool::execute`.
///
/// Merges the old `CancellationToken` + `ToolContext` + `LocalToolContext`
/// into a single trait. Every tool receives one object that provides
/// cancellation, progress reporting, configuration, filesystem access,
/// and downcast support.
#[async_trait]
pub trait ToolExecutionContext: Send + Sync {
    /// Cooperative cancellation token for this execution.
    fn cancel_token(&self) -> &CancellationToken;

    /// Report intermediate progress (no-op by default).
    fn report_progress(&self, _event: ProgressEvent) {}

    /// Session identifier for the current agent session.
    fn session_id(&self) -> &str;

    /// ID of the `ToolCall` currently being executed, if available.
    ///
    /// Used by tools that need to correlate side-channel state with a
    /// specific dispatched call (e.g. sub-agent spawning tools keying
    /// their child run records by the parent tool_call's id so the
    /// parent recorder can attach them later).
    ///
    /// Returns `None` when the context does not track tool call identity
    /// (e.g. `MinimalExecutionContext` used in tests).
    fn tool_call_id(&self) -> Option<&str> {
        None
    }

    /// Read a configuration value by key.
    fn get_config(&self, _key: &str) -> Option<String> {
        None
    }

    /// Workspace / project root path (None for remote or sessionless contexts).
    fn workspace(&self) -> Option<&Path> {
        None
    }

    /// Whether the tool is allowed to perform dangerous operations.
    fn allow_dangerous(&self) -> bool {
        false
    }

    /// Abstract filesystem interface (sandbox, remote, or mock).
    /// When None, tools fall back to direct local operations.
    fn tool_fs(&self) -> Option<&dyn ToolFs> {
        None
    }

    /// Cross-layer coordination bus handle.
    /// Returns None when bus is not wired (e.g., in tests using MinimalExecutionContext).
    fn bus(&self) -> Option<&BusHandle> {
        None
    }

    /// Scoped session handle for this tool invocation.
    ///
    /// Returns `Some` when the runtime has wired an `AgentSession` into
    /// this execution context; events appended through the returned
    /// `ScopedSession` are automatically stamped with
    /// `EmitterKind::Tool` and this tool's registered id.
    ///
    /// Returns `None` for contexts that do not carry a session (tests,
    /// `MinimalExecutionContext`, standalone tool runners).
    fn session(&self) -> Option<&crate::agent_session::ScopedSession> {
        None
    }

    /// Downcast support for application-specific extensions.
    fn as_any(&self) -> &dyn Any;
}

// ---------------------------------------------------------------------------
// MinimalExecutionContext — replaces EmptyToolContext
// ---------------------------------------------------------------------------

/// Minimal execution context for tools that don't need runtime information.
///
/// Provides a cancellation token and empty/no-op defaults for everything else.
/// Useful in tests and for tools that are self-contained.
pub struct MinimalExecutionContext {
    cancel: CancellationToken,
}

impl MinimalExecutionContext {
    /// Create a new minimal context with a fresh cancellation token.
    pub fn new() -> Self {
        Self {
            cancel: CancellationToken::new(),
        }
    }

    /// Create a minimal context wrapping an existing cancellation token.
    pub fn with_cancel(cancel: CancellationToken) -> Self {
        Self { cancel }
    }
}

impl Default for MinimalExecutionContext {
    fn default() -> Self {
        Self::new()
    }
}

impl ToolExecutionContext for MinimalExecutionContext {
    fn cancel_token(&self) -> &CancellationToken {
        &self.cancel
    }

    fn session_id(&self) -> &str {
        ""
    }

    fn as_any(&self) -> &dyn Any {
        self
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
    fn minimal_context_defaults() {
        let ctx = MinimalExecutionContext::new();
        assert_eq!(ctx.session_id(), "");
        assert!(!ctx.cancel_token().is_cancelled());
        assert!(ctx.get_config("any").is_none());
        assert!(ctx.workspace().is_none());
        assert!(!ctx.allow_dangerous());
        assert!(ctx.tool_fs().is_none());
    }

    #[test]
    fn minimal_context_with_cancel() {
        let cancel = CancellationToken::new();
        cancel.cancel();
        let ctx = MinimalExecutionContext::with_cancel(cancel);
        assert!(ctx.cancel_token().is_cancelled());
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
