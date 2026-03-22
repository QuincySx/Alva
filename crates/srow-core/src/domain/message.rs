// INPUT:  serde, uuid
// OUTPUT: Role, LLMContent, LLMMessage
// POS:    Defines conversation message types including role, content blocks, and factory methods.
//         Kept during migration because storage, persistence, and types modules depend on these.
//         The conversion functions (llm_messages_to_provider_prompt, provider_content_to_llm_content)
//         have been removed — they depended on deleted Provider V4 prompt/content types.
//         TODO: Replace with agent-base Message type when storage layer is migrated.
use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// Message role in the conversation
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Role {
    System,
    User,
    Assistant,
    Tool,
}

/// Content block within a message (models Anthropic-style content blocks)
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum LLMContent {
    Text {
        text: String,
    },
    ToolUse {
        id: String,
        name: String,
        input: serde_json::Value,
    },
    ToolResult {
        tool_use_id: String,
        content: String,
        is_error: bool,
    },
    Image {
        source: ImageSource,
        media_type: Option<String>,
        data: String,
    },
    Reasoning {
        text: String,
    },
}

/// Image source type
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ImageSource {
    Base64,
    Url,
}

/// A single message in the conversation history
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LLMMessage {
    pub id: String,
    pub role: Role,
    pub content: Vec<LLMContent>,
    pub turn_index: u32,
    pub token_count: Option<u32>,
}

impl LLMMessage {
    /// Create a user message with plain text
    pub fn user(text: impl Into<String>) -> Self {
        Self {
            id: Uuid::new_v4().to_string(),
            role: Role::User,
            content: vec![LLMContent::Text { text: text.into() }],
            turn_index: 0,
            token_count: None,
        }
    }

    /// Create an assistant message from content blocks
    pub fn assistant(content: Vec<LLMContent>) -> Self {
        Self {
            id: Uuid::new_v4().to_string(),
            role: Role::Assistant,
            content,
            turn_index: 0,
            token_count: None,
        }
    }

    /// Create a tool result message
    pub fn tool_result(
        tool_use_id: impl Into<String>,
        content: impl Into<String>,
        is_error: bool,
    ) -> Self {
        Self {
            id: Uuid::new_v4().to_string(),
            role: Role::Tool,
            content: vec![LLMContent::ToolResult {
                tool_use_id: tool_use_id.into(),
                content: content.into(),
                is_error,
            }],
            turn_index: 0,
            token_count: None,
        }
    }

    /// Extract plain text from this message (joining all Text blocks)
    pub fn text(&self) -> String {
        self.content
            .iter()
            .filter_map(|c| match c {
                LLMContent::Text { text } => Some(text.as_str()),
                _ => None,
            })
            .collect::<Vec<_>>()
            .join("")
    }
}
