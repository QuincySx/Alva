use crate::ui_message::{UIMessage, UIMessagePart, TextPartState, ToolState};
use crate::error::StreamError;
use super::{UIMessageChunk, FinishReason, TokenUsage};
use super::state::{StreamingUIMessageState, PartialToolCall};
use futures::StreamExt;
use futures::Stream;
use std::collections::HashMap;
use tokio::sync::mpsc;

/// State update events sent during stream processing
#[derive(Debug, Clone)]
pub enum UIMessageStreamUpdate {
    /// First write — status should transition Submitted -> Streaming
    FirstWrite(UIMessage),
    /// Message content has changed (need re-render)
    MessageChanged(UIMessage),
    /// Stream processing finished
    Finished {
        message: UIMessage,
        finish_reason: Option<FinishReason>,
        usage: Option<TokenUsage>,
    },
}

/// Find the index of a Tool part by its `id` field.
fn find_tool_part_index(parts: &[UIMessagePart], tool_id: &str) -> Option<usize> {
    parts.iter().position(|p| matches!(p, UIMessagePart::Tool { id, .. } if id == tool_id))
}

/// Consume a stream of `UIMessageChunk` items, building up a `UIMessage` and
/// sending state updates through the provided channel.
///
/// This is the Rust equivalent of Vercel AI SDK's `processUIMessageStream`.
pub async fn process_ui_message_stream<S>(
    mut stream: S,
    initial_message: UIMessage,
    update_tx: mpsc::UnboundedSender<UIMessageStreamUpdate>,
) -> Result<StreamingUIMessageState, StreamError>
where
    S: Stream<Item = Result<UIMessageChunk, StreamError>> + Unpin + Send,
{
    let mut state = StreamingUIMessageState {
        message: initial_message,
        active_text_parts: HashMap::new(),
        active_reasoning_parts: HashMap::new(),
        partial_tool_calls: HashMap::new(),
        finish_reason: None,
    };

    let mut first_write = true;
    let mut accumulated_usage: Option<TokenUsage> = None;

    while let Some(chunk_result) = stream.next().await {
        let chunk = chunk_result?;

        // Determine whether this chunk changes visible content.
        // If it does, we send an update after processing.
        let sends_update = matches!(
            &chunk,
            UIMessageChunk::TextStart { .. }
            | UIMessageChunk::TextDelta { .. }
            | UIMessageChunk::TextEnd { .. }
            | UIMessageChunk::ReasoningStart { .. }
            | UIMessageChunk::ReasoningDelta { .. }
            | UIMessageChunk::ReasoningEnd { .. }
            | UIMessageChunk::ToolInputStart { .. }
            | UIMessageChunk::ToolInputDelta { .. }
            | UIMessageChunk::ToolInputAvailable { .. }
            | UIMessageChunk::ToolInputError { .. }
            | UIMessageChunk::ToolApprovalRequest { .. }
            | UIMessageChunk::ToolOutputAvailable { .. }
            | UIMessageChunk::ToolOutputError { .. }
            | UIMessageChunk::ToolOutputDenied { .. }
            | UIMessageChunk::Data { .. }
            | UIMessageChunk::File { .. }
            | UIMessageChunk::SourceUrl { .. }
            | UIMessageChunk::SourceDocument { .. }
            | UIMessageChunk::Custom { .. }
            | UIMessageChunk::StartStep
            | UIMessageChunk::ReasoningFile { .. }
        );

        match chunk {
            UIMessageChunk::Start { message_id, message_metadata } => {
                if let Some(id) = message_id {
                    state.message.id = id;
                }
                if let Some(metadata) = message_metadata {
                    state.message.metadata = Some(metadata);
                }
            }

            // --- Text ---
            UIMessageChunk::TextStart { id } => {
                state.message.parts.push(UIMessagePart::Text {
                    text: String::new(),
                    state: Some(TextPartState::Streaming),
                });
                let idx = state.message.parts.len() - 1;
                state.active_text_parts.insert(id, idx);
            }
            UIMessageChunk::TextDelta { id, delta } => {
                if let Some(&idx) = state.active_text_parts.get(&id) {
                    if let Some(UIMessagePart::Text { text, .. }) = state.message.parts.get_mut(idx) {
                        text.push_str(&delta);
                    }
                }
            }
            UIMessageChunk::TextEnd { id } => {
                if let Some(&idx) = state.active_text_parts.get(&id) {
                    if let Some(UIMessagePart::Text { state: ref mut part_state, .. }) = state.message.parts.get_mut(idx) {
                        *part_state = Some(TextPartState::Done);
                    }
                }
                state.active_text_parts.remove(&id);
            }

            // --- Reasoning ---
            UIMessageChunk::ReasoningStart { id } => {
                state.message.parts.push(UIMessagePart::Reasoning {
                    text: String::new(),
                    state: Some(TextPartState::Streaming),
                });
                let idx = state.message.parts.len() - 1;
                state.active_reasoning_parts.insert(id, idx);
            }
            UIMessageChunk::ReasoningDelta { id, delta } => {
                if let Some(&idx) = state.active_reasoning_parts.get(&id) {
                    if let Some(UIMessagePart::Reasoning { text, .. }) = state.message.parts.get_mut(idx) {
                        text.push_str(&delta);
                    }
                }
            }
            UIMessageChunk::ReasoningEnd { id } => {
                if let Some(&idx) = state.active_reasoning_parts.get(&id) {
                    if let Some(UIMessagePart::Reasoning { state: ref mut part_state, .. }) = state.message.parts.get_mut(idx) {
                        *part_state = Some(TextPartState::Done);
                    }
                }
                state.active_reasoning_parts.remove(&id);
            }

            // --- Tool calls ---
            UIMessageChunk::ToolInputStart { id, tool_name, title } => {
                state.message.parts.push(UIMessagePart::Tool {
                    id: id.clone(),
                    tool_name: tool_name.clone(),
                    input: serde_json::Value::Null,
                    state: ToolState::InputStreaming,
                    output: None,
                    error: None,
                    title: title.clone(),
                });
                let idx = state.message.parts.len() - 1;
                state.partial_tool_calls.insert(id, PartialToolCall {
                    text: String::new(),
                    index: idx,
                    tool_name,
                    title,
                });
            }
            UIMessageChunk::ToolInputDelta { id, delta } => {
                if let Some(partial) = state.partial_tool_calls.get_mut(&id) {
                    partial.text.push_str(&delta);
                }
            }
            UIMessageChunk::ToolInputAvailable { id, input } => {
                if let Some(partial) = state.partial_tool_calls.remove(&id) {
                    if let Some(UIMessagePart::Tool {
                        input: ref mut tool_input,
                        state: ref mut tool_state,
                        ..
                    }) = state.message.parts.get_mut(partial.index)
                    {
                        *tool_input = input;
                        *tool_state = ToolState::InputAvailable;
                    }
                }
            }
            UIMessageChunk::ToolInputError { id, error } => {
                if let Some(partial) = state.partial_tool_calls.remove(&id) {
                    if let Some(UIMessagePart::Tool {
                        state: ref mut tool_state,
                        error: ref mut tool_error,
                        ..
                    }) = state.message.parts.get_mut(partial.index)
                    {
                        *tool_state = ToolState::OutputError;
                        *tool_error = Some(error);
                    }
                }
            }
            UIMessageChunk::ToolApprovalRequest { id } => {
                if let Some(idx) = find_tool_part_index(&state.message.parts, &id) {
                    if let Some(UIMessagePart::Tool { state: ref mut tool_state, .. }) = state.message.parts.get_mut(idx) {
                        *tool_state = ToolState::ApprovalRequested;
                    }
                }
            }
            UIMessageChunk::ToolOutputAvailable { id, output } => {
                if let Some(idx) = find_tool_part_index(&state.message.parts, &id) {
                    if let Some(UIMessagePart::Tool {
                        output: ref mut tool_output,
                        state: ref mut tool_state,
                        ..
                    }) = state.message.parts.get_mut(idx)
                    {
                        *tool_output = Some(output);
                        *tool_state = ToolState::OutputAvailable;
                    }
                }
            }
            UIMessageChunk::ToolOutputError { id, error } => {
                if let Some(idx) = find_tool_part_index(&state.message.parts, &id) {
                    if let Some(UIMessagePart::Tool {
                        error: ref mut tool_error,
                        state: ref mut tool_state,
                        ..
                    }) = state.message.parts.get_mut(idx)
                    {
                        *tool_error = Some(error);
                        *tool_state = ToolState::OutputError;
                    }
                }
            }
            UIMessageChunk::ToolOutputDenied { id } => {
                if let Some(idx) = find_tool_part_index(&state.message.parts, &id) {
                    if let Some(UIMessagePart::Tool { state: ref mut tool_state, .. }) = state.message.parts.get_mut(idx) {
                        *tool_state = ToolState::OutputDenied;
                    }
                }
            }

            // --- Data / File / Source / Custom ---
            UIMessageChunk::Data { name, data } => {
                state.message.parts.push(UIMessagePart::Data { name, data });
            }
            UIMessageChunk::File { id: _, media_type, data } => {
                state.message.parts.push(UIMessagePart::File { media_type, data });
            }
            UIMessageChunk::ReasoningFile { id: _, media_type, data } => {
                state.message.parts.push(UIMessagePart::File { media_type, data });
            }
            UIMessageChunk::SourceUrl { id: _, url, title } => {
                state.message.parts.push(UIMessagePart::SourceUrl { url, title });
            }
            UIMessageChunk::SourceDocument { id: _, title, source_type } => {
                // SourceDocument chunk has no separate `id` field for the part;
                // we reuse the chunk id as the part id.
                state.message.parts.push(UIMessagePart::SourceDocument {
                    id: String::new(),
                    title,
                    source_type,
                });
            }
            UIMessageChunk::Custom { id, data } => {
                state.message.parts.push(UIMessagePart::Custom { id, data });
            }

            // --- Steps ---
            UIMessageChunk::StartStep => {
                state.message.parts.push(UIMessagePart::StepStart);
            }
            UIMessageChunk::FinishStep => {
                state.active_text_parts.clear();
                state.active_reasoning_parts.clear();
            }

            // --- Metadata ---
            UIMessageChunk::MessageMetadata { metadata } => {
                state.message.metadata = Some(metadata);
            }

            // --- Token usage ---
            UIMessageChunk::TokenUsage { usage } => {
                accumulated_usage = Some(usage);
            }

            // --- Finish ---
            UIMessageChunk::Finish { finish_reason, usage } => {
                state.finish_reason = Some(finish_reason);
                if let Some(u) = usage {
                    accumulated_usage = Some(u);
                }
                // Don't send update here; handled after stream ends.
            }

            // --- Error ---
            UIMessageChunk::Error { error } => {
                return Err(StreamError::InvalidChunk(error));
            }

            // --- Abort ---
            UIMessageChunk::Abort => {
                return Err(StreamError::Aborted);
            }
        }

        // Send update if this chunk changed visible content.
        if sends_update {
            let update = if first_write {
                first_write = false;
                UIMessageStreamUpdate::FirstWrite(state.message.clone())
            } else {
                UIMessageStreamUpdate::MessageChanged(state.message.clone())
            };
            // Ignore send errors (receiver may have been dropped).
            let _ = update_tx.send(update);
        }
    }

    // Stream ended — send Finished update.
    let _ = update_tx.send(UIMessageStreamUpdate::Finished {
        message: state.message.clone(),
        finish_reason: state.finish_reason.clone(),
        usage: accumulated_usage,
    });

    Ok(state)
}
