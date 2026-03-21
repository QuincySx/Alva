// INPUT:  serde, uuid
// OUTPUT: Role, LLMContent, LLMMessage
// POS:    Defines conversation message types including role, content blocks (text/tool_use/tool_result), and factory methods.
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
    /// Image content (user sends image or assistant generates image)
    Image {
        /// How the image data is encoded
        source: ImageSource,
        /// MIME type (e.g. "image/png", "image/jpeg")
        media_type: Option<String>,
        /// Base64 data (when source=Base64) or URL string (when source=Url)
        data: String,
    },
    /// Reasoning/thinking content from extended thinking models
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

// ---------------------------------------------------------------------------
// Conversion: LLMMessage <-> LanguageModelMessage (Provider V4)
// ---------------------------------------------------------------------------

use crate::ports::provider::prompt::{
    AssistantContentPart, LanguageModelMessage, ToolContentPart, UserContentPart,
};
use crate::ports::provider::content::LanguageModelContent;
use crate::ports::provider::tool_types::ToolResultOutput;

/// Convert internal LLMMessage history + optional system prompt into
/// Provider V4 `LanguageModelMessage` prompt for provider calls.
pub fn llm_messages_to_provider_prompt(
    system: &Option<String>,
    messages: &[LLMMessage],
) -> Vec<LanguageModelMessage> {
    let mut prompt: Vec<LanguageModelMessage> = Vec::new();

    // System message
    if let Some(sys) = system {
        if !sys.is_empty() {
            prompt.push(LanguageModelMessage::System {
                content: sys.clone(),
                provider_options: None,
            });
        }
    }

    for msg in messages {
        match msg.role {
            Role::System => {
                // Already handled above; skip duplicates unless there are
                // system messages stored inline in the history.
            }
            Role::User => {
                let parts: Vec<UserContentPart> = msg
                    .content
                    .iter()
                    .filter_map(|c| match c {
                        LLMContent::Text { text } => Some(UserContentPart::Text {
                            text: text.clone(),
                            provider_options: None,
                        }),
                        LLMContent::Image { source, media_type, data } => {
                            let mime = media_type.as_deref().unwrap_or("image/png").to_string();
                            match source {
                                ImageSource::Url => Some(UserContentPart::File {
                                    data: crate::ports::provider::prompt::DataContent::Url {
                                        url: data.clone(),
                                    },
                                    media_type: mime,
                                    filename: None,
                                    provider_options: None,
                                }),
                                ImageSource::Base64 => Some(UserContentPart::File {
                                    data: crate::ports::provider::prompt::DataContent::Base64 {
                                        data: data.clone(),
                                    },
                                    media_type: mime,
                                    filename: None,
                                    provider_options: None,
                                }),
                            }
                        }
                        _ => None,
                    })
                    .collect();
                if !parts.is_empty() {
                    prompt.push(LanguageModelMessage::User {
                        content: parts,
                        provider_options: None,
                    });
                }
            }
            Role::Assistant => {
                let parts: Vec<AssistantContentPart> = msg
                    .content
                    .iter()
                    .filter_map(|c| match c {
                        LLMContent::Text { text } => Some(AssistantContentPart::Text {
                            text: text.clone(),
                            provider_options: None,
                        }),
                        LLMContent::ToolUse { id, name, input } => {
                            Some(AssistantContentPart::ToolCall {
                                tool_call_id: id.clone(),
                                tool_name: name.clone(),
                                input: input.clone(),
                                provider_options: None,
                            })
                        }
                        LLMContent::Reasoning { text } => Some(AssistantContentPart::Reasoning {
                            text: text.clone(),
                            provider_options: None,
                        }),
                        _ => None,
                    })
                    .collect();
                if !parts.is_empty() {
                    prompt.push(LanguageModelMessage::Assistant {
                        content: parts,
                        provider_options: None,
                    });
                }
            }
            Role::Tool => {
                let parts: Vec<ToolContentPart> = msg
                    .content
                    .iter()
                    .filter_map(|c| match c {
                        LLMContent::ToolResult {
                            tool_use_id,
                            content,
                            is_error,
                        } => {
                            let output = if *is_error {
                                ToolResultOutput::ErrorText {
                                    value: content.clone(),
                                }
                            } else {
                                ToolResultOutput::Text {
                                    value: content.clone(),
                                }
                            };
                            Some(ToolContentPart::ToolResult {
                                tool_call_id: tool_use_id.clone(),
                                tool_name: String::new(),
                                output,
                                provider_options: None,
                            })
                        }
                        _ => None,
                    })
                    .collect();
                if !parts.is_empty() {
                    prompt.push(LanguageModelMessage::Tool {
                        content: parts,
                        provider_options: None,
                    });
                }
            }
        }
    }

    prompt
}

/// Convert Provider V4 `LanguageModelContent` from a generate result into
/// internal `LLMContent` blocks for storage.
pub fn provider_content_to_llm_content(
    content: &[LanguageModelContent],
) -> Vec<LLMContent> {
    content
        .iter()
        .filter_map(|c| match c {
            LanguageModelContent::Text { text, .. } => Some(LLMContent::Text {
                text: text.clone(),
            }),
            LanguageModelContent::ToolCall {
                tool_call_id,
                tool_name,
                input,
                ..
            } => {
                // input is a JSON string in the V4 model — parse to Value
                let parsed: serde_json::Value =
                    serde_json::from_str(input).unwrap_or(serde_json::Value::Object(
                        serde_json::Map::new(),
                    ));
                Some(LLMContent::ToolUse {
                    id: tool_call_id.clone(),
                    name: tool_name.clone(),
                    input: parsed,
                })
            }
            LanguageModelContent::Reasoning { text, .. } => Some(LLMContent::Reasoning {
                text: text.clone(),
            }),
            _ => None,
        })
        .collect()
}
