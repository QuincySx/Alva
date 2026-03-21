// INPUT:  crate::domain::message, crate::ui_message, crate::ports::llm_provider, crate::ui_message_stream, futures, uuid
// OUTPUT: ui_messages_to_llm_messages, llm_stream_to_ui_chunks
// POS:    Bidirectional conversion between UI-layer message types and LLM-layer message types.

use std::collections::HashMap;

use futures::stream::unfold;
use futures::{Stream, StreamExt};
use uuid::Uuid;

use crate::domain::message::{LLMContent, LLMMessage, Role};
use crate::ports::llm_provider::{StopReason, StreamChunk};
use crate::ui_message::{UIMessage, UIMessagePart, UIMessageRole, ToolState};
use crate::ui_message_stream::{FinishReason, UIMessageChunk};

/// Convert UI messages to LLM messages for API calls.
///
/// Rules:
/// - User messages: Text parts become a single LLMMessage with Text content blocks
/// - System messages: Text parts become a single LLMMessage with Text content
/// - Assistant messages:
///   1. Text parts become LLMContent::Text in an assistant message
///   2. Tool parts become LLMContent::ToolUse in the same assistant message
///   3. For each Tool part with state = OutputAvailable:
///      a separate LLMMessage { role: Tool, content: [ToolResult] } is generated
///   4. Tool parts with OutputError produce a ToolResult with is_error = true
pub fn ui_messages_to_llm_messages(messages: &[UIMessage]) -> Vec<LLMMessage> {
    let mut result = Vec::new();

    for (turn_index, msg) in messages.iter().enumerate() {
        match msg.role {
            UIMessageRole::User => {
                let content: Vec<LLMContent> = msg
                    .parts
                    .iter()
                    .filter_map(|part| match part {
                        UIMessagePart::Text { text, .. } => {
                            Some(LLMContent::Text { text: text.clone() })
                        }
                        _ => None,
                    })
                    .collect();

                if !content.is_empty() {
                    result.push(LLMMessage {
                        id: msg.id.clone(),
                        role: Role::User,
                        content,
                        turn_index: turn_index as u32,
                        token_count: None,
                    });
                }
            }
            UIMessageRole::System => {
                let content: Vec<LLMContent> = msg
                    .parts
                    .iter()
                    .filter_map(|part| match part {
                        UIMessagePart::Text { text, .. } => {
                            Some(LLMContent::Text { text: text.clone() })
                        }
                        _ => None,
                    })
                    .collect();

                if !content.is_empty() {
                    result.push(LLMMessage {
                        id: msg.id.clone(),
                        role: Role::System,
                        content,
                        turn_index: turn_index as u32,
                        token_count: None,
                    });
                }
            }
            UIMessageRole::Assistant => {
                // Collect assistant content blocks (text + tool use)
                let mut assistant_content: Vec<LLMContent> = Vec::new();
                // Collect tool results to emit as separate Tool-role messages
                let mut tool_results: Vec<LLMMessage> = Vec::new();

                for part in &msg.parts {
                    match part {
                        UIMessagePart::Text { text, .. } => {
                            assistant_content.push(LLMContent::Text { text: text.clone() });
                        }
                        UIMessagePart::Tool {
                            id,
                            tool_name,
                            input,
                            state,
                            output,
                            error,
                            ..
                        } => {
                            // Always add ToolUse to the assistant message
                            assistant_content.push(LLMContent::ToolUse {
                                id: id.clone(),
                                name: tool_name.clone(),
                                input: input.clone(),
                            });

                            // Generate ToolResult messages for completed tool calls
                            match state {
                                ToolState::OutputAvailable => {
                                    let content_str = match output {
                                        Some(v) => serde_json::to_string(v)
                                            .unwrap_or_else(|_| "null".to_string()),
                                        None => String::new(),
                                    };
                                    tool_results.push(LLMMessage {
                                        id: Uuid::new_v4().to_string(),
                                        role: Role::Tool,
                                        content: vec![LLMContent::ToolResult {
                                            tool_use_id: id.clone(),
                                            content: content_str,
                                            is_error: false,
                                        }],
                                        turn_index: turn_index as u32,
                                        token_count: None,
                                    });
                                }
                                ToolState::OutputError => {
                                    let content_str = error
                                        .as_deref()
                                        .unwrap_or("Tool execution error")
                                        .to_string();
                                    tool_results.push(LLMMessage {
                                        id: Uuid::new_v4().to_string(),
                                        role: Role::Tool,
                                        content: vec![LLMContent::ToolResult {
                                            tool_use_id: id.clone(),
                                            content: content_str,
                                            is_error: true,
                                        }],
                                        turn_index: turn_index as u32,
                                        token_count: None,
                                    });
                                }
                                ToolState::OutputDenied => {
                                    tool_results.push(LLMMessage {
                                        id: Uuid::new_v4().to_string(),
                                        role: Role::Tool,
                                        content: vec![LLMContent::ToolResult {
                                            tool_use_id: id.clone(),
                                            content: "Tool execution was denied by the user"
                                                .to_string(),
                                            is_error: true,
                                        }],
                                        turn_index: turn_index as u32,
                                        token_count: None,
                                    });
                                }
                                // Other states (InputStreaming, InputAvailable, ApprovalRequested,
                                // ApprovalResponded) don't produce a ToolResult yet
                                _ => {}
                            }
                        }
                        // Reasoning, File, SourceUrl, etc. are not sent to the LLM
                        _ => {}
                    }
                }

                if !assistant_content.is_empty() {
                    result.push(LLMMessage {
                        id: msg.id.clone(),
                        role: Role::Assistant,
                        content: assistant_content,
                        turn_index: turn_index as u32,
                        token_count: None,
                    });
                }

                // Tool result messages come after the assistant message
                result.extend(tool_results);
            }
        }
    }

    result
}

/// Tracking state for the LLM-to-UI stream converter.
struct LlmToUiState {
    started: bool,
    text_id: Option<String>,
    reasoning_id: Option<String>,
    /// tool_call_id -> (name, accumulated_input_json)
    tool_calls: HashMap<String, (String, String)>,
    stream: std::pin::Pin<Box<dyn Stream<Item = StreamChunk> + Send>>,
    /// Buffered chunks to emit (when a single StreamChunk produces multiple UIMessageChunks)
    pending: std::collections::VecDeque<UIMessageChunk>,
    done: bool,
}

/// Convert LLM streaming chunks to UI message chunks.
///
/// Used by DirectChatTransport to bridge the engine output to the UI protocol.
///
/// State tracking:
/// - Auto-emits `Start` on the first chunk
/// - Emits `TextStart`/`ReasoningStart` on the first delta of each kind
/// - Emits `TextEnd`/`ReasoningEnd` + `ToolInputStart`/`ToolInputAvailable` on `Done`
pub fn llm_stream_to_ui_chunks(
    stream: impl Stream<Item = StreamChunk> + Send + Unpin + 'static,
) -> impl Stream<Item = UIMessageChunk> + Send {
    let state = LlmToUiState {
        started: false,
        text_id: None,
        reasoning_id: None,
        tool_calls: HashMap::new(),
        stream: Box::pin(stream),
        pending: std::collections::VecDeque::new(),
        done: false,
    };

    unfold(state, |mut state| async move {
        loop {
            // Drain pending queue first
            if let Some(chunk) = state.pending.pop_front() {
                return Some((chunk, state));
            }

            if state.done {
                return None;
            }

            // Read next chunk from the LLM stream
            let llm_chunk = match state.stream.next().await {
                Some(chunk) => chunk,
                None => {
                    // Stream ended without Done — emit cleanup
                    if let Some(id) = state.text_id.take() {
                        state.pending.push_back(UIMessageChunk::TextEnd { id });
                    }
                    if let Some(id) = state.reasoning_id.take() {
                        state.pending.push_back(UIMessageChunk::ReasoningEnd { id });
                    }
                    state.done = true;
                    if let Some(chunk) = state.pending.pop_front() {
                        return Some((chunk, state));
                    }
                    return None;
                }
            };

            match llm_chunk {
                StreamChunk::TextDelta(text) => {
                    if !state.started {
                        state.started = true;
                        state.pending.push_back(UIMessageChunk::Start {
                            message_id: None,
                            message_metadata: None,
                        });
                    }
                    if state.text_id.is_none() {
                        let id = Uuid::new_v4().to_string();
                        state.pending.push_back(UIMessageChunk::TextStart {
                            id: id.clone(),
                        });
                        state.text_id = Some(id);
                    }
                    let id = state.text_id.clone().unwrap();
                    state.pending.push_back(UIMessageChunk::TextDelta {
                        id,
                        delta: text,
                    });
                }
                StreamChunk::ThinkingDelta(text) => {
                    if !state.started {
                        state.started = true;
                        state.pending.push_back(UIMessageChunk::Start {
                            message_id: None,
                            message_metadata: None,
                        });
                    }
                    if state.reasoning_id.is_none() {
                        let id = Uuid::new_v4().to_string();
                        state.pending.push_back(UIMessageChunk::ReasoningStart {
                            id: id.clone(),
                        });
                        state.reasoning_id = Some(id);
                    }
                    let id = state.reasoning_id.clone().unwrap();
                    state.pending.push_back(UIMessageChunk::ReasoningDelta {
                        id,
                        delta: text,
                    });
                }
                StreamChunk::ToolCallDelta {
                    id,
                    name,
                    input_delta,
                } => {
                    if !state.started {
                        state.started = true;
                        state.pending.push_back(UIMessageChunk::Start {
                            message_id: None,
                            message_metadata: None,
                        });
                    }
                    let entry = state
                        .tool_calls
                        .entry(id.clone())
                        .or_insert_with(|| (name.clone(), String::new()));
                    entry.1.push_str(&input_delta);
                }
                StreamChunk::Done(response) => {
                    if !state.started {
                        state.started = true;
                        state.pending.push_back(UIMessageChunk::Start {
                            message_id: None,
                            message_metadata: None,
                        });
                    }

                    // Close active text/reasoning parts
                    if let Some(id) = state.text_id.take() {
                        state.pending.push_back(UIMessageChunk::TextEnd { id });
                    }
                    if let Some(id) = state.reasoning_id.take() {
                        state.pending.push_back(UIMessageChunk::ReasoningEnd { id });
                    }

                    // Emit tool calls
                    for (id, (name, accumulated_input)) in state.tool_calls.drain() {
                        // Parse the accumulated JSON input
                        let input = serde_json::from_str(&accumulated_input)
                            .unwrap_or(serde_json::Value::Null);
                        state.pending.push_back(UIMessageChunk::ToolInputStart {
                            id: id.clone(),
                            tool_name: name,
                            title: None,
                        });
                        state.pending.push_back(UIMessageChunk::ToolInputAvailable {
                            id,
                            input,
                        });
                    }

                    // Convert stop reason to finish reason
                    let finish_reason = match response.stop_reason {
                        StopReason::EndTurn => FinishReason::Stop,
                        StopReason::ToolUse => FinishReason::ToolCalls,
                        StopReason::MaxTokens => FinishReason::MaxTokens,
                        StopReason::StopSequence => FinishReason::Stop,
                    };

                    let usage = Some(crate::ui_message_stream::TokenUsage {
                        input_tokens: response.usage.input_tokens,
                        output_tokens: response.usage.output_tokens,
                    });

                    state.pending.push_back(UIMessageChunk::Finish {
                        finish_reason,
                        usage,
                    });

                    state.done = true;
                }
            }

            // Emit the first pending item
            if let Some(chunk) = state.pending.pop_front() {
                return Some((chunk, state));
            }
        }
    })
}
