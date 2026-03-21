use srow_core::domain::message::LLMMessage;
use srow_core::error::ChatError;
use srow_core::ports::llm_provider::{LLMRequest, StopReason, StreamChunk};
use srow_core::ui_message_stream::{FinishReason, TokenUsage, UIMessageChunk};
use tokio::sync::{mpsc, oneshot};

use super::generate_text::{convert_stop_reason, execute_tools, extract_text, extract_tool_calls};
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
///
/// Works the same as the generate_text loop but streams chunks in real time:
/// 1. Emits `Start` at the beginning
/// 2. For each LLM call, forwards `TextDelta` / `ReasoningDelta` chunks as they arrive
/// 3. On tool use, emits tool input/output chunks and loops back
/// 4. Emits `Finish` when done
async fn run_stream_loop(
    settings: CallSettings,
    prompt: Prompt,
    chunk_tx: mpsc::UnboundedSender<UIMessageChunk>,
) -> Result<(String, Vec<StepResult>, TokenUsage, FinishReason), ChatError> {
    // 1. Convert prompt to LLM messages (same as generate_text)
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
        let request = LLMRequest {
            messages: history.clone(),
            tools: tool_defs,
            system: settings.system.clone(),
            max_tokens: settings.max_output_tokens.unwrap_or(8192),
            temperature: settings.temperature,
            tool_choice: None,
            response_format: None,
            top_p: None,
            top_k: None,
            presence_penalty: None,
            frequency_penalty: None,
            stop_sequences: None,
            seed: None,
            provider_options: None,
        };

        // Stream from LLM
        let (stream_tx, mut stream_rx) = tokio::sync::mpsc::channel::<StreamChunk>(256);
        let model = settings.model.clone();

        let stream_handle = tokio::spawn(async move {
            model.complete_stream(request, stream_tx).await
        });

        // Forward StreamChunks as UIMessageChunks and accumulate response
        let mut response_content = Vec::new();
        let mut step_usage = TokenUsage {
            input_tokens: 0,
            output_tokens: 0,
        };
        let mut stop_reason = StopReason::EndTurn;

        while let Some(chunk) = stream_rx.recv().await {
            match &chunk {
                StreamChunk::TextDelta(text) => {
                    let _ = chunk_tx.send(UIMessageChunk::TextDelta {
                        id: "stream".to_string(),
                        delta: text.clone(),
                    });
                }
                StreamChunk::ThinkingDelta(text) => {
                    let _ = chunk_tx.send(UIMessageChunk::ReasoningDelta {
                        id: "reasoning".to_string(),
                        delta: text.clone(),
                    });
                }
                StreamChunk::ToolCallDelta { .. } => {
                    // Tool call deltas are accumulated in the Done response
                }
                StreamChunk::Done(response) => {
                    response_content = response.content.clone();
                    step_usage = TokenUsage {
                        input_tokens: response.usage.input_tokens,
                        output_tokens: response.usage.output_tokens,
                    };
                    stop_reason = response.stop_reason.clone();

                    let _ = chunk_tx.send(UIMessageChunk::TokenUsage {
                        usage: step_usage.clone(),
                    });
                }
            }
        }

        // Wait for the stream producer to finish
        let _ = stream_handle.await;

        let text = extract_text(&response_content);
        let tool_calls = extract_tool_calls(&response_content);
        let finish_reason = convert_stop_reason(&stop_reason);

        // Execute tools if needed
        let tool_results = if stop_reason == StopReason::ToolUse
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
        if stop_reason != StopReason::ToolUse {
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
