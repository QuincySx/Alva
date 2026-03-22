// INPUT:  crate::domain::message, crate::domain::tool
// OUTPUT: LLMContent, LLMMessage, Role, ImageSource, ToolCall, ToolDefinition, ToolResult
// POS:    Flat re-export of LLM-related types from domain for convenience.
//         Provider V4 re-exports removed (LanguageModel, LanguageModelMessage, etc.) — replaced by agent-base.
//! LLM-related types — re-exported from domain

pub use crate::domain::message::{ImageSource, LLMContent, LLMMessage, Role};
pub use crate::domain::tool::{ToolCall, ToolDefinition, ToolResult};
