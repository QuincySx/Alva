//! LLM wire-format conversion: protocol-neutral types + ProtocolAdapter.
//!
//! Standalone, serde-only (plus uuid/chrono for id/timestamp generation).
//! No dependency on the agent framework — usable by external consumers.

pub mod adapter;
pub mod config;
pub mod content;
pub mod message;
pub mod stream;
pub mod tool_def;
pub mod tool_payload;

pub use adapter::{ProtocolAdapter, ToolAdapter};
pub use config::{ModelConfig, ReasoningEffort};
pub use content::ContentBlock;
pub use message::{AgentMessage, Marker, Message, MessageRole, UsageMetadata};
pub use stream::{StopReason, StreamEvent};
pub use tool_def::ToolDefinition;
pub use tool_payload::{ProgressEvent, ToolContent, ToolOutput};
