// INPUT:  crate::domain, crate::error, crate::ports::llm_provider, async_trait, futures, rig (openai, completion), tokio::sync::mpsc
// OUTPUT: OpenAICompatProvider
// POS:    OpenAI-compatible LLM adapter using rig-core, supporting sync and streaming completions for OpenAI/DeepSeek/Qwen APIs.
//! OpenAI-compatible LLM adapter using rig-core.
//!
//! Supports: OpenAI, DeepSeek, Qwen, and any OpenAI API-compatible provider.
//! Uses rig's CompletionModel trait for the actual API calls.

use crate::domain::message::{LLMContent, LLMMessage, Role};
use crate::domain::tool::ToolDefinition;
use crate::error::EngineError;
use crate::ports::llm_provider::{
    LLMProvider, LLMRequest, LLMResponse, ResponseFormat, StopReason, StreamChunk, ToolChoice,
    TokenUsage,
};
use async_trait::async_trait;
use futures::StreamExt;
use rig::completion::{
    message::{AssistantContent, Message, UserContent},
    request::CompletionModel,
};
use rig::client::CompletionClient;
use rig::providers::openai;
use tokio::sync::mpsc;

/// OpenAI-compatible LLM provider backed by rig-core.
pub struct OpenAICompatProvider {
    model: Box<dyn CompletionModelDyn>,
    model_id: String,
}

/// We need a trait-object-safe wrapper around rig's CompletionModel
/// because CompletionModel is not object-safe (has associated types + impl Trait returns).
#[async_trait]
trait CompletionModelDyn: Send + Sync {
    async fn complete_dyn(
        &self,
        request: rig::completion::request::CompletionRequest,
    ) -> Result<DynCompletionResponse, EngineError>;

    async fn stream_dyn(
        &self,
        request: rig::completion::request::CompletionRequest,
    ) -> Result<DynStreamResponse, EngineError>;
}

struct DynCompletionResponse {
    pub choice: Vec<AssistantContent>,
    pub usage: rig::completion::request::Usage,
}

struct DynStreamResponse {
    pub stream: std::pin::Pin<
        Box<
            dyn futures::Stream<
                    Item = Result<
                        rig::streaming::StreamedAssistantContent<serde_json::Value>,
                        rig::completion::request::CompletionError,
                    >,
                > + Send,
        >,
    >,
}

/// Concrete wrapper for rig's OpenAI completion model
struct OpenAIModelWrapper {
    model: openai::completion::CompletionModel,
}

#[async_trait]
impl CompletionModelDyn for OpenAIModelWrapper {
    async fn complete_dyn(
        &self,
        request: rig::completion::request::CompletionRequest,
    ) -> Result<DynCompletionResponse, EngineError> {
        let resp = self
            .model
            .completion(request)
            .await
            .map_err(|e| EngineError::LLMProvider(e.to_string()))?;

        let choices: Vec<AssistantContent> = resp.choice.into_iter().collect();

        Ok(DynCompletionResponse {
            choice: choices,
            usage: resp.usage,
        })
    }

    async fn stream_dyn(
        &self,
        request: rig::completion::request::CompletionRequest,
    ) -> Result<DynStreamResponse, EngineError> {
        let stream_resp = self
            .model
            .stream(request)
            .await
            .map_err(|e| EngineError::LLMProvider(e.to_string()))?;

        // Map the provider-specific streaming response to a generic Value-based stream
        let mapped = stream_resp.map(|item| {
            item.map(|content| match content {
                rig::streaming::StreamedAssistantContent::Text(text) => {
                    rig::streaming::StreamedAssistantContent::Text(text)
                }
                rig::streaming::StreamedAssistantContent::ToolCall {
                    tool_call,
                    internal_call_id,
                } => rig::streaming::StreamedAssistantContent::ToolCall {
                    tool_call,
                    internal_call_id,
                },
                rig::streaming::StreamedAssistantContent::ToolCallDelta {
                    id,
                    internal_call_id,
                    content,
                } => rig::streaming::StreamedAssistantContent::ToolCallDelta {
                    id,
                    internal_call_id,
                    content,
                },
                rig::streaming::StreamedAssistantContent::Reasoning(r) => {
                    rig::streaming::StreamedAssistantContent::Reasoning(r)
                }
                rig::streaming::StreamedAssistantContent::ReasoningDelta { id, reasoning } => {
                    rig::streaming::StreamedAssistantContent::ReasoningDelta { id, reasoning }
                }
                rig::streaming::StreamedAssistantContent::Final(_raw) => {
                    // Convert raw response to Value
                    rig::streaming::StreamedAssistantContent::Final(serde_json::Value::Null)
                }
            })
        });

        Ok(DynStreamResponse {
            stream: Box::pin(mapped),
        })
    }
}

impl OpenAICompatProvider {
    /// Create a provider for standard OpenAI API (uses Completions API, not Responses API)
    pub fn new(api_key: &str, model: &str) -> Self {
        let client = openai::CompletionsClient::new(api_key)
            .expect("Failed to create OpenAI client");
        let completion_model = client.completion_model(model);
        Self {
            model: Box::new(OpenAIModelWrapper {
                model: completion_model,
            }),
            model_id: model.to_string(),
        }
    }

    /// Create a provider with a custom base URL (for DeepSeek, Qwen, etc.)
    pub fn with_base_url(api_key: &str, base_url: &str, model: &str) -> Self {
        let client = openai::CompletionsClient::builder()
            .base_url(base_url)
            .api_key(api_key)
            .build()
            .expect("Failed to create OpenAI client with custom base URL");
        let completion_model = client.completion_model(model);
        Self {
            model: Box::new(OpenAIModelWrapper {
                model: completion_model,
            }),
            model_id: model.to_string(),
        }
    }

    /// Convert our LLMMessage list + system prompt into rig's Message list
    fn convert_messages(system: &Option<String>, messages: &[LLMMessage]) -> Vec<Message> {
        let mut rig_messages = Vec::new();

        // Add system message
        if let Some(sys) = system {
            if !sys.is_empty() {
                rig_messages.push(Message::system(sys));
            }
        }

        for msg in messages {
            match msg.role {
                Role::System => {
                    // Already handled above; skip duplicates
                }
                Role::User => {
                    for content in &msg.content {
                        match content {
                            LLMContent::Text { text } => {
                                if !text.is_empty() {
                                    rig_messages.push(Message::user(text));
                                }
                            }
                            LLMContent::Image { source, media_type, data } => {
                                // TODO: Convert to rig's UserContent::Image when image support
                                // is fully plumbed. For now, send image URL/data as text placeholder.
                                let desc = match source {
                                    crate::domain::message::ImageSource::Url => {
                                        format!("[Image: {}]", data)
                                    }
                                    crate::domain::message::ImageSource::Base64 => {
                                        format!(
                                            "[Image: base64, type={}]",
                                            media_type.as_deref().unwrap_or("unknown")
                                        )
                                    }
                                };
                                rig_messages.push(Message::user(desc));
                            }
                            _ => {}
                        }
                    }
                }
                Role::Assistant => {
                    for content in &msg.content {
                        match content {
                            LLMContent::Text { text } => {
                                rig_messages.push(Message::assistant(text));
                            }
                            LLMContent::ToolUse { id, name, input } => {
                                // Build assistant message with tool call
                                let tc = AssistantContent::tool_call(
                                    id,
                                    name,
                                    input.clone(),
                                );
                                rig_messages.push(Message::Assistant {
                                    id: None,
                                    content: rig::one_or_many::OneOrMany::one(tc),
                                });
                            }
                            _ => {}
                        }
                    }
                }
                Role::Tool => {
                    for content in &msg.content {
                        if let LLMContent::ToolResult {
                            tool_use_id,
                            content: output,
                            is_error,
                        } = content
                        {
                            let result_content = if *is_error {
                                format!("[ERROR] {}", output)
                            } else {
                                output.clone()
                            };
                            rig_messages.push(Message::User {
                                content: rig::one_or_many::OneOrMany::one(
                                    UserContent::tool_result(
                                        tool_use_id,
                                        rig::one_or_many::OneOrMany::one(
                                            rig::completion::message::ToolResultContent::Text(
                                                result_content.into(),
                                            ),
                                        ),
                                    ),
                                ),
                            });
                        }
                    }
                }
            }
        }

        rig_messages
    }

    /// Convert our ToolDefinition list to rig's ToolDefinition
    fn convert_tool_defs(tools: &[ToolDefinition]) -> Vec<rig::completion::request::ToolDefinition> {
        tools
            .iter()
            .map(|t| rig::completion::request::ToolDefinition {
                name: t.name.clone(),
                description: t.description.clone(),
                parameters: t.parameters.clone(),
            })
            .collect()
    }

    /// Convert rig's AssistantContent to our LLMContent
    fn convert_assistant_content(content: &AssistantContent) -> Option<LLMContent> {
        match content {
            AssistantContent::Text(text) => Some(LLMContent::Text {
                text: text.to_string(),
            }),
            AssistantContent::ToolCall(tc) => Some(LLMContent::ToolUse {
                id: tc.id.clone(),
                name: tc.function.name.clone(),
                input: tc.function.arguments.clone(),
            }),
            AssistantContent::Reasoning(reasoning) => {
                use rig::completion::message::ReasoningContent;
                let text: String = reasoning
                    .content
                    .iter()
                    .filter_map(|c| match c {
                        ReasoningContent::Text { text, .. } => Some(text.as_str()),
                        ReasoningContent::Summary(s) => Some(s.as_str()),
                        _ => None,
                    })
                    .collect::<Vec<_>>()
                    .join("");
                if text.is_empty() {
                    None
                } else {
                    Some(LLMContent::Reasoning { text })
                }
            }
            _ => None,
        }
    }

    /// Convert our ToolChoice to rig's ToolChoice
    fn convert_tool_choice(
        tc: &ToolChoice,
    ) -> rig::completion::message::ToolChoice {
        match tc {
            ToolChoice::Auto => rig::completion::message::ToolChoice::Auto,
            ToolChoice::None => rig::completion::message::ToolChoice::None,
            ToolChoice::Required => rig::completion::message::ToolChoice::Required,
            ToolChoice::Tool(name) => rig::completion::message::ToolChoice::Specific {
                function_names: vec![name.clone()],
            },
        }
    }
}

#[async_trait]
impl LLMProvider for OpenAICompatProvider {
    fn model_id(&self) -> &str {
        &self.model_id
    }

    async fn complete(&self, request: LLMRequest) -> Result<LLMResponse, EngineError> {
        let messages = Self::convert_messages(&request.system, &request.messages);
        let tool_defs = Self::convert_tool_defs(&request.tools);

        // Build rig CompletionRequest manually
        let rig_request = rig::completion::request::CompletionRequest {
            model: None,
            preamble: None,
            chat_history: rig::one_or_many::OneOrMany::many(messages)
                .map_err(|_| EngineError::LLMProvider("empty message list".to_string()))?,
            documents: vec![],
            tools: tool_defs,
            temperature: request.temperature.map(|t| t as f64),
            max_tokens: Some(request.max_tokens as u64),
            tool_choice: request
                .tool_choice
                .as_ref()
                .map(Self::convert_tool_choice),
            additional_params: request.provider_options.clone(),
            output_schema: request.response_format.as_ref().and_then(|rf| match rf {
                ResponseFormat::Json { schema, .. } => schema.clone().map(|s| {
                    serde_json::from_value(s).unwrap_or_default()
                }),
                _ => None,
            }),
        };

        let resp = self.model.complete_dyn(rig_request).await?;

        // Determine stop reason
        let has_tool_calls = resp
            .choice
            .iter()
            .any(|c| matches!(c, AssistantContent::ToolCall(_)));

        let stop_reason = if has_tool_calls {
            StopReason::ToolUse
        } else {
            StopReason::EndTurn
        };

        let content: Vec<LLMContent> = resp
            .choice
            .iter()
            .filter_map(Self::convert_assistant_content)
            .collect();

        Ok(LLMResponse {
            content,
            stop_reason,
            usage: TokenUsage {
                input_tokens: resp.usage.input_tokens as u32,
                output_tokens: resp.usage.output_tokens as u32,
                total_tokens: Some(resp.usage.total_tokens as u32),
                cached_input_tokens: if resp.usage.cached_input_tokens > 0 {
                    Some(resp.usage.cached_input_tokens as u32)
                } else {
                    None
                },
            },
        })
    }

    async fn complete_stream(
        &self,
        request: LLMRequest,
        tx: mpsc::Sender<StreamChunk>,
    ) -> Result<(), EngineError> {
        let messages = Self::convert_messages(&request.system, &request.messages);
        let tool_defs = Self::convert_tool_defs(&request.tools);

        let rig_request = rig::completion::request::CompletionRequest {
            model: None,
            preamble: None,
            chat_history: rig::one_or_many::OneOrMany::many(messages)
                .map_err(|_| EngineError::LLMProvider("empty message list".to_string()))?,
            documents: vec![],
            tools: tool_defs,
            temperature: request.temperature.map(|t| t as f64),
            max_tokens: Some(request.max_tokens as u64),
            tool_choice: request
                .tool_choice
                .as_ref()
                .map(Self::convert_tool_choice),
            additional_params: request.provider_options.clone(),
            output_schema: request.response_format.as_ref().and_then(|rf| match rf {
                ResponseFormat::Json { schema, .. } => schema.clone().map(|s| {
                    serde_json::from_value(s).unwrap_or_default()
                }),
                _ => None,
            }),
        };

        let mut stream_resp = self.model.stream_dyn(rig_request).await?;

        // Accumulate full response for the Done event
        let mut all_content: Vec<LLMContent> = Vec::new();
        let mut has_tool_calls = false;
        // Track tool call argument accumulation
        let mut tool_call_args: std::collections::HashMap<String, (String, String)> =
            std::collections::HashMap::new();

        while let Some(item) = stream_resp.stream.next().await {
            match item {
                Ok(streamed) => {
                    use rig::streaming::StreamedAssistantContent;
                    match streamed {
                        StreamedAssistantContent::Text(text) => {
                            let text_str = text.to_string();
                            let _ = tx.send(StreamChunk::TextDelta(text_str.clone())).await;
                            // Check if we already have a text content block to append to
                            if let Some(LLMContent::Text { text: existing }) = all_content.last_mut()
                            {
                                existing.push_str(&text_str);
                            } else {
                                all_content.push(LLMContent::Text { text: text_str });
                            }
                        }
                        StreamedAssistantContent::ToolCall {
                            tool_call,
                            internal_call_id: _,
                        } => {
                            has_tool_calls = true;
                            all_content.push(LLMContent::ToolUse {
                                id: tool_call.id.clone(),
                                name: tool_call.function.name.clone(),
                                input: tool_call.function.arguments.clone(),
                            });
                            let _ = tx
                                .send(StreamChunk::ToolCallDelta {
                                    id: tool_call.id,
                                    name: tool_call.function.name,
                                    input_delta: tool_call.function.arguments.to_string(),
                                })
                                .await;
                        }
                        StreamedAssistantContent::ToolCallDelta {
                            id,
                            internal_call_id,
                            content,
                        } => {
                            // Accumulate tool call deltas
                            let delta_str = match &content {
                                rig::streaming::ToolCallDeltaContent::Name(n) => {
                                    tool_call_args
                                        .entry(internal_call_id.clone())
                                        .or_insert_with(|| (String::new(), String::new()))
                                        .0 = n.clone();
                                    String::new()
                                }
                                _ => {
                                    // Handle argument deltas or other delta types
                                    let a = format!("{:?}", content);
                                    tool_call_args
                                        .entry(internal_call_id.clone())
                                        .or_insert_with(|| (String::new(), String::new()))
                                        .1
                                        .push_str(&a);
                                    a
                                }
                            };
                            if !delta_str.is_empty() {
                                let name = tool_call_args
                                    .get(&internal_call_id)
                                    .map(|(n, _)| n.clone())
                                    .unwrap_or_default();
                                let _ = tx
                                    .send(StreamChunk::ToolCallDelta {
                                        id: id.clone(),
                                        name,
                                        input_delta: delta_str,
                                    })
                                    .await;
                            }
                        }
                        StreamedAssistantContent::Reasoning(reasoning) => {
                            // Extract text from Reasoning content blocks
                            use rig::completion::message::ReasoningContent;
                            let text: String = reasoning
                                .content
                                .iter()
                                .filter_map(|c| match c {
                                    ReasoningContent::Text { text, .. } => Some(text.as_str()),
                                    ReasoningContent::Summary(s) => Some(s.as_str()),
                                    _ => None,
                                })
                                .collect::<Vec<_>>()
                                .join("");
                            if !text.is_empty() {
                                let _ = tx.send(StreamChunk::ThinkingDelta(text)).await;
                            }
                        }
                        StreamedAssistantContent::ReasoningDelta { id: _, reasoning } => {
                            if !reasoning.is_empty() {
                                let _ = tx.send(StreamChunk::ThinkingDelta(reasoning)).await;
                            }
                        }
                        StreamedAssistantContent::Final(_) => {
                            // Final raw response, we already accumulated content
                        }
                    }
                }
                Err(e) => {
                    tracing::error!("Stream error: {}", e);
                    return Err(EngineError::LLMProvider(e.to_string()));
                }
            }
        }

        // Build tool call content from accumulated deltas (if not already added via ToolCall events)
        for (internal_id, (name, args_str)) in &tool_call_args {
            // Only add if we haven't already added this tool call via a ToolCall event
            let already_exists = all_content.iter().any(|c| {
                if let LLMContent::ToolUse {
                    name: existing_name,
                    ..
                } = c
                {
                    existing_name == name
                } else {
                    false
                }
            });
            if !already_exists && !name.is_empty() {
                has_tool_calls = true;
                let parsed_args: serde_json::Value =
                    serde_json::from_str(args_str).unwrap_or(serde_json::Value::Object(
                        serde_json::Map::new(),
                    ));
                all_content.push(LLMContent::ToolUse {
                    id: internal_id.clone(),
                    name: name.clone(),
                    input: parsed_args,
                });
            }
        }

        let stop_reason = if has_tool_calls {
            StopReason::ToolUse
        } else {
            StopReason::EndTurn
        };

        // TODO: Extract usage from rig's Final stream event when available.
        // Currently rig's streaming API does not surface token usage in the stream;
        // usage will be zero here. The non-streaming `complete()` path returns accurate usage.
        let _ = tx
            .send(StreamChunk::Done(LLMResponse {
                content: all_content,
                stop_reason,
                usage: TokenUsage::default(),
            }))
            .await;

        Ok(())
    }
}
