use srow_core::domain::message::{LLMContent, LLMMessage};
use srow_core::domain::tool::{ToolCall, ToolResult};
use srow_core::error::{ChatError, EngineError};
use srow_core::ports::llm_provider::{LLMRequest, StopReason};
use srow_core::ports::tool::ToolContext;
use srow_core::ui_message::convert::ui_messages_to_llm_messages;
use srow_core::ui_message_stream::{FinishReason, TokenUsage};

use super::types::*;

/// Non-streaming text generation with an agentic tool-use loop.
///
/// Converts the prompt to LLM messages, then enters a loop that:
/// 1. Sends messages to the model
/// 2. Extracts text, reasoning, and tool calls from the response
/// 3. If the model requests tool use, executes tools and appends results to history
/// 4. Repeats until the model stops or a stop condition is met
pub async fn generate_text(
    settings: CallSettings,
    prompt: Prompt,
) -> Result<GenerateTextResult, ChatError> {
    // 1. Convert prompt to LLM messages
    let mut history: Vec<LLMMessage> = match prompt {
        Prompt::Text(s) => vec![LLMMessage::user(s)],
        Prompt::Messages(msgs) => ui_messages_to_llm_messages(&msgs),
    };

    let mut steps: Vec<StepResult> = Vec::new();

    // 2. Agent loop
    loop {
        // a. Build LLM request
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

        // b. Call model with retry
        let response = with_retry(settings.max_retries, || {
            settings.model.complete(request.clone())
        })
        .await
        .map_err(|e| ChatError::Engine(e.to_string()))?;

        // c. Extract text, reasoning, tool calls from response content
        let text = extract_text(&response.content);
        let reasoning = extract_reasoning(&response.content);
        let tool_calls = extract_tool_calls(&response.content);

        // d. Build step usage (convert from llm_provider::TokenUsage to ui_message_stream::TokenUsage)
        let step_usage = TokenUsage {
            input_tokens: response.usage.input_tokens,
            output_tokens: response.usage.output_tokens,
        };

        // e. If ToolUse stop reason and we have tools, execute them
        let tool_results =
            if response.stop_reason == StopReason::ToolUse && settings.tools.is_some() {
                let results = execute_tools(
                    settings.tools.as_ref().unwrap(),
                    &tool_calls,
                    &settings.workspace,
                )
                .await;

                // Append assistant message + tool results to history
                let assistant_msg = LLMMessage::assistant(response.content.clone());
                history.push(assistant_msg);
                for result in &results {
                    history.push(LLMMessage::tool_result(
                        &result.tool_call_id,
                        &result.output,
                        result.is_error,
                    ));
                }

                results
            } else {
                // Append final assistant message
                let assistant_msg = LLMMessage::assistant(response.content.clone());
                history.push(assistant_msg);
                vec![]
            };

        let step = StepResult {
            text: text.clone(),
            reasoning: reasoning.clone(),
            tool_calls: tool_calls.clone(),
            tool_results: tool_results.clone(),
            finish_reason: convert_stop_reason(&response.stop_reason),
            usage: step_usage,
        };
        steps.push(step);

        // f. Check stop condition
        if response.stop_reason != StopReason::ToolUse {
            break; // Natural stop
        }
        if tool_calls.is_empty() {
            break; // No tool calls despite ToolUse reason
        }
        if let Some(ref stop_when) = settings.stop_when {
            if stop_when.should_stop(&steps) {
                break;
            }
        }
    }

    // 3. Build final result
    let last_step = steps.last().unwrap();
    let total_usage = TokenUsage {
        input_tokens: steps.iter().map(|s| s.usage.input_tokens).sum(),
        output_tokens: steps.iter().map(|s| s.usage.output_tokens).sum(),
    };

    Ok(GenerateTextResult {
        text: last_step.text.clone(),
        reasoning: last_step.reasoning.clone(),
        tool_calls: last_step.tool_calls.clone(),
        tool_results: last_step.tool_results.clone(),
        finish_reason: last_step.finish_reason.clone(),
        usage: last_step.usage.clone(),
        total_usage,
        steps,
        response_messages: history,
        output: None,
    })
}

/// Extract all text content blocks from an LLM response, joined together.
pub(crate) fn extract_text(content: &[LLMContent]) -> String {
    content
        .iter()
        .filter_map(|c| match c {
            LLMContent::Text { text } => Some(text.as_str()),
            _ => None,
        })
        .collect::<Vec<_>>()
        .join("")
}

/// Extract reasoning text from content blocks.
///
/// Extracts reasoning/thinking blocks from the response content.
/// These are produced by models with extended thinking capabilities.
fn extract_reasoning(content: &[LLMContent]) -> Option<String> {
    let reasoning: String = content
        .iter()
        .filter_map(|c| match c {
            LLMContent::Reasoning { text } => Some(text.as_str()),
            _ => None,
        })
        .collect::<Vec<_>>()
        .join("");
    if reasoning.is_empty() {
        None
    } else {
        Some(reasoning)
    }
}

/// Extract tool calls from LLM response content blocks.
pub(crate) fn extract_tool_calls(content: &[LLMContent]) -> Vec<ToolCall> {
    content
        .iter()
        .filter_map(|c| match c {
            LLMContent::ToolUse { id, name, input } => Some(ToolCall {
                id: id.clone(),
                name: name.clone(),
                input: input.clone(),
            }),
            _ => None,
        })
        .collect()
}

/// Convert LLM StopReason to UI FinishReason.
pub(crate) fn convert_stop_reason(reason: &StopReason) -> FinishReason {
    match reason {
        StopReason::EndTurn => FinishReason::Stop,
        StopReason::ToolUse => FinishReason::ToolCalls,
        StopReason::MaxTokens => FinishReason::MaxTokens,
        StopReason::StopSequence => FinishReason::Stop,
    }
}

/// Retry an async operation up to `max_retries` times.
///
/// Attempts the operation once, then retries up to `max_retries` additional times on failure.
async fn with_retry<F, Fut, T>(max_retries: u32, f: F) -> Result<T, EngineError>
where
    F: Fn() -> Fut,
    Fut: std::future::Future<Output = Result<T, EngineError>>,
{
    let mut last_err = None;
    for _ in 0..=max_retries {
        match f().await {
            Ok(v) => return Ok(v),
            Err(e) => last_err = Some(e),
        }
    }
    Err(last_err.unwrap())
}

/// Execute tool calls in parallel and collect results.
pub(crate) async fn execute_tools(
    tools: &srow_core::ports::tool::ToolRegistry,
    calls: &[ToolCall],
    workspace: &std::path::Path,
) -> Vec<ToolResult> {
    let ctx = ToolContext {
        session_id: String::new(),
        workspace: workspace.to_path_buf(),
        allow_dangerous: false,
    };

    let mut results = Vec::with_capacity(calls.len());
    for call in calls {
        let start = std::time::Instant::now();
        let result = match tools.get(&call.name) {
            Some(tool) => match tool.execute(call.input.clone(), &ctx).await {
                Ok(r) => r,
                Err(e) => ToolResult {
                    tool_call_id: call.id.clone(),
                    tool_name: call.name.clone(),
                    output: format!("Error: {e}"),
                    is_error: true,
                    duration_ms: start.elapsed().as_millis() as u64,
                },
            },
            None => ToolResult {
                tool_call_id: call.id.clone(),
                tool_name: call.name.clone(),
                output: format!("Tool '{}' not found", call.name),
                is_error: true,
                duration_ms: 0,
            },
        };
        results.push(result);
    }
    results
}
