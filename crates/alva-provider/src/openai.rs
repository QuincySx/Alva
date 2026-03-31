//! OpenAI-compatible Chat Completions provider.
//!
//! Implements `LanguageModel` by calling POST /chat/completions with
//! tool definitions. Works with OpenAI, DeepSeek, Ollama, vLLM, etc.

use std::pin::Pin;
use std::sync::Arc;

use async_trait::async_trait;
use futures_core::Stream;
use reqwest::Client;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use alva_types::base::error::AgentError;
use alva_types::base::message::{Message, MessageRole, UsageMetadata};
use alva_types::model::{LanguageModel, ModelConfig};
use alva_types::base::stream::StreamEvent;
use alva_types::provider::credential::{CredentialSource, StaticCredential};
use alva_types::tool::Tool;
use alva_types::ContentBlock;

use crate::config::ProviderConfig;

/// OpenAI-compatible LLM provider.
pub struct OpenAIProvider {
    credential: Arc<dyn CredentialSource>,
    model: String,
    base_url: String,
    max_tokens: u32,
    client: Client,
}

impl OpenAIProvider {
    /// Create from config (backward compatible — wraps api_key in StaticCredential).
    pub fn new(config: ProviderConfig) -> Self {
        Self {
            credential: Arc::new(StaticCredential::new(&config.api_key)),
            model: config.model,
            base_url: config.base_url,
            max_tokens: config.max_tokens,
            client: Client::new(),
        }
    }

    /// Create with a custom credential source (for OAuth, vault, etc.).
    pub fn with_credential(credential: Arc<dyn CredentialSource>, config: ProviderConfig) -> Self {
        Self {
            credential,
            model: config.model,
            base_url: config.base_url,
            max_tokens: config.max_tokens,
            client: Client::new(),
        }
    }
}

// ---------------------------------------------------------------------------
// LanguageModel implementation
// ---------------------------------------------------------------------------

#[async_trait]
impl LanguageModel for OpenAIProvider {
    async fn complete(
        &self,
        messages: &[Message],
        tools: &[&dyn Tool],
        config: &ModelConfig,
    ) -> Result<Message, AgentError> {
        let url = format!("{}/chat/completions", self.base_url.trim_end_matches('/'));

        let api_key = self.credential.get_api_key().await
            .map_err(|e| AgentError::LlmError(format!("credential error: {}", e)))?;

        let oai_messages = to_oai_messages(messages);
        let oai_tools = to_oai_tools(tools);

        let max_tokens = config
            .max_tokens
            .unwrap_or(self.max_tokens);

        let mut body = serde_json::json!({
            "model": self.model,
            "messages": oai_messages,
            "max_tokens": max_tokens,
        });

        if let Some(t) = config.temperature {
            body["temperature"] = serde_json::json!(t);
        }
        if let Some(p) = config.top_p {
            body["top_p"] = serde_json::json!(p);
        }
        if !config.stop_sequences.is_empty() {
            body["stop"] = serde_json::json!(config.stop_sequences);
        }
        if !oai_tools.is_empty() {
            body["tools"] = serde_json::json!(oai_tools);
        }

        tracing::debug!(
            model = %self.model,
            messages = oai_messages.len(),
            tools = oai_tools.len(),
            "calling chat completions"
        );

        let resp = self
            .client
            .post(&url)
            .header("Authorization", format!("Bearer {}", api_key))
            .header("Content-Type", "application/json")
            .json(&body)
            .send()
            .await
            .map_err(|e| AgentError::LlmError(format!("HTTP request failed: {}", e)))?;

        let status = resp.status();
        let resp_text = resp
            .text()
            .await
            .map_err(|e| AgentError::LlmError(format!("read response body: {}", e)))?;

        if !status.is_success() {
            return Err(AgentError::LlmError(format!(
                "API returned {}: {}",
                status, resp_text
            )));
        }

        let oai_resp: OaiResponse = serde_json::from_str(&resp_text)
            .map_err(|e| AgentError::LlmError(format!("parse response: {} — raw: {}", e, resp_text)))?;

        from_oai_response(oai_resp)
    }

    fn stream(
        &self,
        _messages: &[Message],
        _tools: &[&dyn Tool],
        _config: &ModelConfig,
    ) -> Pin<Box<dyn Stream<Item = StreamEvent> + Send>> {
        // Streaming not implemented yet — return an empty stream.
        Box::pin(futures::stream::empty())
    }

    fn model_id(&self) -> &str {
        &self.model
    }
}

// ---------------------------------------------------------------------------
// OpenAI API types (request)
// ---------------------------------------------------------------------------

#[derive(Serialize)]
struct OaiMessage {
    role: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    content: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    tool_calls: Option<Vec<OaiToolCall>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    tool_call_id: Option<String>,
}

#[derive(Serialize, Deserialize, Clone)]
struct OaiToolCall {
    id: String,
    #[serde(rename = "type")]
    call_type: String,
    function: OaiFunction,
}

#[derive(Serialize, Deserialize, Clone)]
struct OaiFunction {
    name: String,
    arguments: String,
}

#[derive(Serialize)]
struct OaiToolDef {
    #[serde(rename = "type")]
    tool_type: String,
    function: OaiFunctionDef,
}

#[derive(Serialize)]
struct OaiFunctionDef {
    name: String,
    description: String,
    parameters: Value,
}

// ---------------------------------------------------------------------------
// OpenAI API types (response)
// ---------------------------------------------------------------------------

#[derive(Deserialize)]
struct OaiResponse {
    choices: Vec<OaiChoice>,
    #[serde(default)]
    usage: Option<OaiUsage>,
}

#[derive(Deserialize)]
struct OaiChoice {
    message: OaiResponseMessage,
}

#[derive(Deserialize)]
struct OaiResponseMessage {
    #[serde(default)]
    content: Option<String>,
    #[serde(default)]
    tool_calls: Option<Vec<OaiToolCall>>,
}

#[derive(Deserialize)]
struct OaiUsage {
    prompt_tokens: u32,
    completion_tokens: u32,
    total_tokens: u32,
}

// ---------------------------------------------------------------------------
// Conversion helpers
// ---------------------------------------------------------------------------

fn to_oai_messages(messages: &[Message]) -> Vec<OaiMessage> {
    messages.iter().map(|m| {
        match m.role {
            MessageRole::Tool => {
                // Tool result → send as role=tool with tool_call_id
                // Extract text from ToolResult content blocks (Vec<ToolContent>),
                // falling back to text_content() for any plain Text blocks.
                let content = {
                    let mut parts: Vec<String> = Vec::new();
                    for block in &m.content {
                        if let ContentBlock::ToolResult { content, .. } = block {
                            for tc in content {
                                parts.push(tc.to_model_string());
                            }
                        } else if let Some(text) = block.as_text() {
                            parts.push(text.to_string());
                        }
                    }
                    parts.join("\n")
                };
                OaiMessage {
                    role: "tool".to_string(),
                    content: Some(Value::String(content)),
                    tool_calls: None,
                    tool_call_id: m.tool_call_id.clone(),
                }
            }
            MessageRole::Assistant if m.has_tool_calls() => {
                // Assistant with tool calls
                let text = m.text_content();
                let tool_calls: Vec<OaiToolCall> = m.content.iter().filter_map(|b| {
                    if let ContentBlock::ToolUse { id, name, input } = b {
                        Some(OaiToolCall {
                            id: id.clone(),
                            call_type: "function".to_string(),
                            function: OaiFunction {
                                name: name.clone(),
                                arguments: input.to_string(),
                            },
                        })
                    } else {
                        None
                    }
                }).collect();

                OaiMessage {
                    role: "assistant".to_string(),
                    content: if text.is_empty() { None } else { Some(Value::String(text)) },
                    tool_calls: Some(tool_calls),
                    tool_call_id: None,
                }
            }
            _ => {
                let role = match m.role {
                    MessageRole::User => "user",
                    MessageRole::Assistant => "assistant",
                    MessageRole::System => "system",
                    MessageRole::Tool => "tool",
                };
                OaiMessage {
                    role: role.to_string(),
                    content: Some(Value::String(m.text_content())),
                    tool_calls: None,
                    tool_call_id: None,
                }
            }
        }
    }).collect()
}

fn to_oai_tools(tools: &[&dyn Tool]) -> Vec<OaiToolDef> {
    tools.iter().map(|t| OaiToolDef {
        tool_type: "function".to_string(),
        function: OaiFunctionDef {
            name: t.name().to_string(),
            description: t.description().to_string(),
            parameters: t.parameters_schema(),
        },
    }).collect()
}

fn from_oai_response(resp: OaiResponse) -> Result<Message, AgentError> {
    let choice = resp.choices.into_iter().next()
        .ok_or_else(|| AgentError::LlmError("no choices in response".to_string()))?;

    let mut content_blocks = Vec::new();

    // Text content
    if let Some(text) = choice.message.content {
        if !text.is_empty() {
            content_blocks.push(ContentBlock::Text { text });
        }
    }

    // Tool calls
    if let Some(tool_calls) = choice.message.tool_calls {
        for tc in tool_calls {
            let input: Value = serde_json::from_str(&tc.function.arguments)
                .unwrap_or(Value::Object(serde_json::Map::new()));
            content_blocks.push(ContentBlock::ToolUse {
                id: tc.id,
                name: tc.function.name,
                input,
            });
        }
    }

    let usage = resp.usage.map(|u| UsageMetadata {
        input_tokens: u.prompt_tokens,
        output_tokens: u.completion_tokens,
        total_tokens: u.total_tokens,
    });

    Ok(Message {
        id: uuid::Uuid::new_v4().to_string(),
        role: MessageRole::Assistant,
        content: content_blocks,
        tool_call_id: None,
        usage,
        timestamp: chrono::Utc::now().timestamp_millis(),
    })
}
