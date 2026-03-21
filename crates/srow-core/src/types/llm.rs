// INPUT:  crate::ports::llm_provider, crate::domain::message, crate::domain::tool
// OUTPUT: LLMProvider, LLMRequest, LLMResponse, StopReason, StreamChunk, TokenUsage, LLMContent, LLMMessage, Role, ToolCall, ToolDefinition, ToolResult
// POS:    Flat re-export of all LLM-related types from ports and domain for convenience.
//! LLM-related types — re-exported from ports and domain
//! This module provides a convenient flat import path for LLM types.

// Provider V4 types — re-exported from ports::provider
pub use crate::ports::provider::{
    LanguageModel, LanguageModelCallOptions, LanguageModelGenerateResult,
    LanguageModelStreamResult, LanguageModelStreamPart, LanguageModelContent,
    LanguageModelUsage, UnifiedFinishReason,
    LanguageModelMessage, LanguageModelTool, FunctionTool, ToolChoice, ResponseFormat,
    ProviderError,
};
pub use crate::domain::message::{ImageSource, LLMContent, LLMMessage, Role};
pub use crate::domain::tool::{ToolCall, ToolDefinition, ToolResult};
