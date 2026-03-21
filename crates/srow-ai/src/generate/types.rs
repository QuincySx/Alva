use std::sync::Arc;

use srow_core::domain::tool::{ToolCall, ToolResult};
use srow_core::ports::provider::language_model::LanguageModel;
use srow_core::ports::tool::ToolRegistry;
use srow_core::ui_message::UIMessage;
use srow_core::ui_message_stream::{FinishReason, TokenUsage, UIMessageChunk};

use super::stop_condition::StopCondition;

pub struct CallSettings {
    pub model: Arc<dyn LanguageModel>,
    pub system: Option<String>,
    pub tools: Option<Arc<ToolRegistry>>,
    pub max_output_tokens: Option<u32>,
    pub temperature: Option<f32>,
    pub stop_when: Option<Arc<dyn StopCondition>>,
    pub max_retries: u32,
    pub workspace: std::path::PathBuf,
}

pub enum Prompt {
    Text(String),
    Messages(Vec<UIMessage>),
}

#[derive(Debug, Clone)]
pub struct StepResult {
    pub text: String,
    pub reasoning: Option<String>,
    pub tool_calls: Vec<ToolCall>,
    pub tool_results: Vec<ToolResult>,
    pub finish_reason: FinishReason,
    pub usage: TokenUsage,
}

pub struct GenerateTextResult<T = String> {
    pub text: String,
    pub reasoning: Option<String>,
    pub tool_calls: Vec<ToolCall>,
    pub tool_results: Vec<ToolResult>,
    pub finish_reason: FinishReason,
    pub usage: TokenUsage,
    pub total_usage: TokenUsage,
    pub steps: Vec<StepResult>,
    pub response_messages: Vec<srow_core::domain::message::LLMMessage>,
    pub output: Option<T>,
}

pub struct StreamTextResult {
    pub chunk_rx: tokio::sync::mpsc::UnboundedReceiver<UIMessageChunk>,
    pub text: tokio::sync::oneshot::Receiver<String>,
    pub steps: tokio::sync::oneshot::Receiver<Vec<StepResult>>,
    pub total_usage: tokio::sync::oneshot::Receiver<TokenUsage>,
    pub finish_reason: tokio::sync::oneshot::Receiver<FinishReason>,
}
