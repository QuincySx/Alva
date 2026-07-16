// INPUT:  crate modules for protocol adapters, messages, streams, tools, accumulation, and wasm proxy DTOs
// OUTPUT: Public protocol-neutral LLM wire types, adapters, stream accumulator, and versioned wasm proxy ABI
// POS:    Dependency-light root for all serialized LLM contracts shared across SDK and host layers.

//! LLM wire-format conversion: protocol-neutral types + ProtocolAdapter.
//!
//! Standalone, serde-only (plus uuid/chrono for id/timestamp generation).
//! No dependency on the agent framework — usable by external consumers.

pub mod accumulate;
pub mod adapter;
pub mod config;
pub mod content;
pub mod message;
pub mod stream;
pub mod tool_def;
pub mod tool_payload;
pub mod wasm_proxy;

pub use accumulate::{message_from_events, StreamMessageAccumulator, StreamMessageError};
pub use adapter::{ProtocolAdapter, ToolAdapter};
pub use config::{ModelConfig, ReasoningEffort};
pub use content::ContentBlock;
pub use message::{AgentMessage, Marker, Message, MessageRole, UsageMetadata};
pub use stream::{StopReason, StreamEvent};
pub use tool_def::ToolDefinition;
pub use tool_payload::{ProgressEvent, ToolContent, ToolOutput};
pub use wasm_proxy::{
    LlmProxyRequest, LlmProxyResponse, LLM_PROXY_ABI_VERSION, MAX_LLM_PROXY_REQUEST_BYTES,
    MAX_LLM_PROXY_RESPONSE_BYTES,
};
