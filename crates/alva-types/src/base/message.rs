// INPUT:  serde, serde_json, uuid, chrono, crate::base::content::ContentBlock
// OUTPUT: pub enum MessageRole, pub struct UsageMetadata, pub struct Message, pub enum AgentMessage
// POS:    Core message types representing LLM conversation turns, token usage, and an agent-level message wrapper.
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::base::content::ContentBlock;

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
pub struct Message {
    pub id: String,
    pub role: MessageRole,
    pub content: Vec<ContentBlock>,
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
            tool_call_id: None,
            usage: None,
            timestamp: chrono::Utc::now().timestamp_millis(),
        }
    }

    pub fn has_tool_calls(&self) -> bool {
        self.content.iter().any(|b| b.is_tool_use())
    }

    pub fn text_content(&self) -> String {
        self.content
            .iter()
            .filter_map(|b| b.as_text())
            .collect::<Vec<_>>()
            .join("")
    }
}

// ---------------------------------------------------------------------------
// AgentMessage
// ---------------------------------------------------------------------------

/// Wraps either a standard LLM message or a custom application-level message
/// that can flow through the agent event stream.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind")]
pub enum AgentMessage {
    Standard(Message),
    Custom {
        type_name: String,
        data: Value,
    },
}
