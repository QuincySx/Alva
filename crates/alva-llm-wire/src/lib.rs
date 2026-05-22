//! LLM wire-format conversion: protocol-neutral types + ProtocolAdapter.
//!
//! Standalone, serde-only (plus uuid/chrono for id/timestamp generation).
//! No dependency on the agent framework — usable by external consumers.

pub mod content;
pub mod message;
pub mod stream;
pub mod config;
pub mod tool_payload;
pub mod tool_def;

pub use content::ContentBlock;
pub use message::{AgentMessage, Marker, Message, MessageRole, UsageMetadata};
pub use stream::{StreamEvent, StopReason};
pub use config::{ModelConfig, ReasoningEffort};
pub use tool_payload::{ProgressEvent, ToolContent, ToolOutput};
pub use tool_def::ToolDefinition;
