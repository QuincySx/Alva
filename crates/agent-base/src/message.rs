use serde::{Deserialize, Serialize};

use crate::content::ContentBlock;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "lowercase")]
pub enum MessageRole {
    User,
    Assistant,
    System,
    Tool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UsageMetadata {
    pub input_tokens: u32,
    pub output_tokens: u32,
    pub total_tokens: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolCallData {
    pub id: String,
    pub name: String,
    pub arguments: serde_json::Value,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Message {
    pub id: String,
    pub role: MessageRole,
    pub content: Vec<ContentBlock>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub tool_calls: Vec<ToolCallData>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tool_call_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub usage: Option<UsageMetadata>,
    pub timestamp: i64,
}

impl Message {
    pub fn user(text: impl Into<String>) -> Self {
        Self {
            id: uuid::Uuid::new_v4().to_string(),
            role: MessageRole::User,
            content: vec![ContentBlock::Text { text: text.into() }],
            tool_calls: vec![],
            tool_call_id: None,
            usage: None,
            timestamp: chrono::Utc::now().timestamp_millis(),
        }
    }

    pub fn system(text: impl Into<String>) -> Self {
        Self {
            id: uuid::Uuid::new_v4().to_string(),
            role: MessageRole::System,
            content: vec![ContentBlock::Text { text: text.into() }],
            tool_calls: vec![],
            tool_call_id: None,
            usage: None,
            timestamp: chrono::Utc::now().timestamp_millis(),
        }
    }

    pub fn text_content(&self) -> String {
        self.content
            .iter()
            .filter_map(|b| match b {
                ContentBlock::Text { text } => Some(text.as_str()),
                _ => None,
            })
            .collect::<Vec<_>>()
            .join("")
    }
}
