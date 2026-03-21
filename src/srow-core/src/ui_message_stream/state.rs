use std::collections::HashMap;
use crate::ui_message::UIMessage;
use super::FinishReason;

#[derive(Debug)]
pub struct StreamingUIMessageState {
    pub message: UIMessage,
    pub active_text_parts: HashMap<String, usize>,
    pub active_reasoning_parts: HashMap<String, usize>,
    pub partial_tool_calls: HashMap<String, PartialToolCall>,
    pub finish_reason: Option<FinishReason>,
}

#[derive(Debug)]
pub struct PartialToolCall {
    pub text: String,
    pub index: usize,
    pub tool_name: String,
    pub title: Option<String>,
}
