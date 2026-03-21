use futures::StreamExt;
use srow_core::domain::message::{LLMContent, LLMMessage, llm_messages_to_provider_prompt};
use srow_core::error::ChatError;
use srow_core::ports::provider::language_model::{
    LanguageModelCallOptions, LanguageModelStreamPart, UnifiedFinishReason,
};
use srow_core::ports::provider::tool_types::{LanguageModelTool, FunctionTool};
use srow_core::ui_message_stream::{FinishReason, TokenUsage, UIMessageChunk};
use tokio::sync::{mpsc, oneshot};

use super::generate_text::{convert_unified_reason, execute_tools, extract_text, extract_tool_calls};
use super::types::*;

/// Streaming text generation with an agentic tool-use loop.
///
/// Returns immediately with a `StreamTextResult` containing channels for:
/// - `chunk_rx`: real-time UI message chunks as they arrive
/// - `text`: final accumulated text (available after stream completes)
/// - `steps`: all step results (available after stream completes)
/// - `total_usage`: aggregated token usage (available after stream completes)
/// - `finish_reason`: why the generation stopped (available after stream completes)
///
/// The actual LLM interaction runs in a background tokio task.
pub fn stream_text(settings: CallSettings, prompt: Prompt) -> StreamTextResult {
    let (chunk_tx, chunk_rx) = mpsc::unbounded_channel();
    let (text_tx, text_rx) = oneshot::channel();
    let (steps_tx, steps_rx) = oneshot::channel();
    let (usage_tx, usage_rx) = oneshot::channel();
    let (reason_tx, reason_rx) = oneshot::channel();

    tokio::spawn(async move {
        let result = run_stream_loop(settings, prompt, chunk_tx.clone()).await;

        match result {
            Ok((final_text, steps, total_usage, finish_reason)) => {
                let _ = text_tx.send(final_text);
                let _ = steps_tx.send(steps);
                let _ = usage_tx.send(total_usage);
                let _ = reason_tx.send(finish_reason);
            }
            Err(e) => {
                let _ = chunk_tx.send(UIMessageChunk::Error {
                    error: e.to_string(),
                });
                let _ = text_tx.send(String::new());
                let _ = steps_tx.send(vec![]);
                let _ = usage_tx.send(TokenUsage {
                    input_tokens: 0,
                    output_tokens: 0,
                });
                let _ = reason_tx.send(FinishReason::Error);
            }
        }
    });

    StreamTextResult {
        chunk_rx,
        text: text_rx,
        steps: steps_rx,
        total_usage: usage_rx,
        finish_reason: reason_rx,
    }
}

/// Internal streaming loop that drives the LLM and emits UI chunks.
async fn run_stream_loop(
    settings: CallSettings,
    prompt: Prompt,
    chunk_tx: mpsc::UnboundedSender<UIMessageChunk>,
) -> Result<(String, Vec<StepResult>, TokenUsage, FinishReason), ChatError> {
    // 1. Convert prompt to LLM messages
    let mut history: Vec<LLMMessage> = match prompt {
        Prompt::Text(s) => vec![LLMMessage::user(s)],
        Prompt::Messages(msgs) => {
            srow_core::ui_message::convert::ui_messages_to_llm_messages(&msgs)
        }
    };

    let mut steps = Vec::new();
    let mut total_usage = TokenUsage {
        input_tokens: 0,
        output_tokens: 0,
    };

    // Emit Start chunk
    let _ = chunk_tx.send(UIMessageChunk::Start {
        message_id: Some(uuid::Uuid::new_v4().to_string()),
        message_metadata: None,
    });

    loop {
        let tool_defs = settings
            .tools
            .as_ref()
            .map(|t| t.definitions())
            .unwrap_or_default();

        let v4_tools: Vec<LanguageModelTool> = tool_defs
            .iter()
            .map(|td| {
                LanguageModelTool::Function(FunctionTool {
                    name: td.name.clone(),
                    description: Some(td.description.clone()),
                    input_schema: td.parameters.clone(),
                    strict: None,
                    provider_options: None,
                })
            })
            .collect();

        let prompt_messages = llm_messages_to_provider_prompt(&settings.system, &history);

        let options = LanguageModelCallOptions {
            prompt: prompt_messages,
            max_output_tokens: settings.max_output_tokens,
            temperature: settings.temperature,
            stop_sequences: None,
            top_p: None,
            top_k: None,
            presence_penalty: None,
            frequency_penalty: None,
            response_format: None,
            seed: None,
            tools: if v4_tools.is_empty() { None } else { Some(v4_tools) },
            tool_choice: None,
            reasoning: None,
            provider_options: None,
            headers: None,
        };

        // Stream from LLM
        let stream_result = settings
            .model
            .do_stream(options)
            .await
            .map_err(|e| ChatError::Engine(e.to_string()))?;

        let mut stream = stream_result.stream;

        // Accumulate response
        let mut accumulated_text = String::new();
        let mut accumulated_tool_calls: Vec<(String, String, String)> = Vec::new();
        let mut tool_input_buffers: std::collections::HashMap<String, (String, String)> =
            std::collections::HashMap::new();
        let mut step_usage = TokenUsage {
            input_tokens: 0,
            output_tokens: 0,
        };
        let mut unified_reason = UnifiedFinishReason::Stop;

        while let Some(part) = stream.next().await {
            match &part {
                LanguageModelStreamPart::TextDelta { delta, .. } => {
                    let _ = chunk_tx.send(UIMessageChunk::TextDelta {
                        id: "stream".to_string(),
                        delta: delta.clone(),
                    });
                    accumulated_text.push_str(delta);
                }
                LanguageModelStreamPart::ReasoningDelta { delta, .. } => {
                    let _ = chunk_tx.send(UIMessageChunk::ReasoningDelta {
                        id: "reasoning".to_string(),
                        delta: delta.clone(),
                    });
                }
                LanguageModelStreamPart::ToolInputStart { id, tool_name, .. } => {
                    tool_input_buffers.insert(id.clone(), (tool_name.clone(), String::new()));
                }
                LanguageModelStreamPart::ToolInputDelta { id, delta } => {
                    if let Some(entry) = tool_input_buffers.get_mut(id) {
                        entry.1.push_str(delta);
                    }
                }
                LanguageModelStreamPart::ToolInputEnd { id } => {
                    if let Some((name, input_json)) = tool_input_buffers.remove(id) {
                        accumulated_tool_calls.push((id.clone(), name, input_json));
                    }
                }
                LanguageModelStreamPart::ToolCall { content } => {
                    if let srow_core::ports::provider::content::LanguageModelContent::ToolCall {
                        tool_call_id,
                        tool_name,
                        input,
                        ..
                    } = content
                    {
                        accumulated_tool_calls.push((tool_call_id.clone(), tool_name.clone(), input.clone()));
                    }
                }
                LanguageModelStreamPart::Finish { usage, finish_reason, .. } => {
                    step_usage = TokenUsage {
                        input_tokens: usage.input_tokens.total.unwrap_or(0),
                        output_tokens: usage.output_tokens.total.unwrap_or(0),
                    };
                    unified_reason = finish_reason.unified.clone();

                    let _ = chunk_tx.send(UIMessageChunk::TokenUsage {
                        usage: step_usage.clone(),
                    });
                }
                _ => {
                    // Other stream parts — ignored
                }
            }
        }

        // Build LLMContent from accumulated data
        let mut response_content: Vec<LLMContent> = Vec::new();
        if !accumulated_text.is_empty() {
            response_content.push(LLMContent::Text {
                text: accumulated_text.clone(),
            });
        }
        for (tc_id, tc_name, tc_input) in &accumulated_tool_calls {
            let parsed: serde_json::Value =
                serde_json::from_str(tc_input).unwrap_or(serde_json::Value::Object(
                    serde_json::Map::new(),
                ));
            response_content.push(LLMContent::ToolUse {
                id: tc_id.clone(),
                name: tc_name.clone(),
                input: parsed,
            });
        }

        let text = extract_text(&response_content);
        let tool_calls = extract_tool_calls(&response_content);
        let finish_reason = convert_unified_reason(&unified_reason);

        // Execute tools if needed
        let tool_results = if unified_reason == UnifiedFinishReason::ToolCalls
            && settings.tools.is_some()
            && !tool_calls.is_empty()
        {
            // Emit tool input chunks
            for call in &tool_calls {
                let _ = chunk_tx.send(UIMessageChunk::ToolInputStart {
                    id: call.id.clone(),
                    tool_name: call.name.clone(),
                    title: None,
                });
                let _ = chunk_tx.send(UIMessageChunk::ToolInputAvailable {
                    id: call.id.clone(),
                    input: call.input.clone(),
                });
            }

            let results = execute_tools(
                settings.tools.as_ref().unwrap(),
                &tool_calls,
                &settings.workspace,
            )
            .await;

            // Emit tool output chunks
            for result in &results {
                if result.is_error {
                    let _ = chunk_tx.send(UIMessageChunk::ToolOutputError {
                        id: result.tool_call_id.clone(),
                        error: result.output.clone(),
                    });
                } else {
                    let output = serde_json::from_str(&result.output)
                        .unwrap_or(serde_json::Value::String(result.output.clone()));
                    let _ = chunk_tx.send(UIMessageChunk::ToolOutputAvailable {
                        id: result.tool_call_id.clone(),
                        output,
                    });
                }
            }

            // Update history for next loop iteration
            let assistant_msg = LLMMessage::assistant(response_content.clone());
            history.push(assistant_msg);
            for result in &results {
                history.push(LLMMessage::tool_result(
                    &result.tool_call_id,
                    &result.output,
                    result.is_error,
                ));
            }

            // Emit FinishStep
            let _ = chunk_tx.send(UIMessageChunk::FinishStep);

            results
        } else {
            let assistant_msg = LLMMessage::assistant(response_content);
            history.push(assistant_msg);
            vec![]
        };

        let step = StepResult {
            text: text.clone(),
            reasoning: None,
            tool_calls,
            tool_results,
            finish_reason: finish_reason.clone(),
            usage: step_usage.clone(),
        };

        total_usage.input_tokens += step_usage.input_tokens;
        total_usage.output_tokens += step_usage.output_tokens;
        steps.push(step);

        // Check stop conditions
        if unified_reason != UnifiedFinishReason::ToolCalls {
            break;
        }
        if let Some(ref stop_when) = settings.stop_when {
            if stop_when.should_stop(&steps) {
                break;
            }
        }
    }

    // Emit Finish
    let final_reason = steps
        .last()
        .map(|s| s.finish_reason.clone())
        .unwrap_or(FinishReason::Stop);
    let _ = chunk_tx.send(UIMessageChunk::Finish {
        finish_reason: final_reason.clone(),
        usage: Some(total_usage.clone()),
    });

    let final_text = steps.last().map(|s| s.text.clone()).unwrap_or_default();
    Ok((final_text, steps, total_usage, final_reason))
}
