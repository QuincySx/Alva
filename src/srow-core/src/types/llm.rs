//! LLM-related types — re-exported from ports and domain
//! This module provides a convenient flat import path for LLM types.

pub use crate::ports::llm_provider::{LLMProvider, LLMRequest, LLMResponse, StopReason, StreamChunk, TokenUsage};
pub use crate::domain::message::{LLMContent, LLMMessage, Role};
pub use crate::domain::tool::{ToolCall, ToolDefinition, ToolResult};
