use srow_core::domain::message::{LLMContent, LLMMessage, llm_messages_to_provider_prompt, provider_content_to_llm_content};
use srow_core::domain::tool::{ToolCall, ToolResult};
use srow_core::error::ChatError;
use srow_core::ports::provider::language_model::{
    LanguageModelCallOptions, UnifiedFinishReason,
};
use srow_core::ports::provider::tool_types::{LanguageModelTool, FunctionTool};
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
        // a. Call model with retry
        let response = with_retry(settings.max_retries, || {
            let opts = LanguageModelCallOptions {
                prompt: llm_messages_to_provider_prompt(&settings.system, &history),
                max_output_tokens: settings.max_output_tokens,
                temperature: settings.temperature,
                stop_sequences: None,
                top_p: None,
                top_k: None,
                presence_penalty: None,
                frequency_penalty: None,
                response_format: None,
                seed: None,
                tools: if settings.tools.is_some() {
                    let defs = settings.tools.as_ref().unwrap().definitions();
                    let tools: Vec<LanguageModelTool> = defs.iter().map(|td| {
                        LanguageModelTool::Function(FunctionTool {
                            name: td.name.clone(),
                            description: Some(td.description.clone()),
                            input_schema: td.parameters.clone(),
                            strict: None,
                            provider_options: None,
                        })
                    }).collect();
                    if tools.is_empty() { None } else { Some(tools) }
                } else { None },
                tool_choice: None,
                reasoning: None,
                provider_options: None,
                headers: None,
            };
            settings.model.do_generate(opts)
        })
        .await
        .map_err(|e| ChatError::Engine(e.to_string()))?;

        // d. Convert response content to LLMContent for storage
        let llm_content = provider_content_to_llm_content(&response.content);

        // e. Extract text, reasoning, tool calls from converted content
        let text = extract_text(&llm_content);
        let reasoning = extract_reasoning(&llm_content);
        let tool_calls = extract_tool_calls(&llm_content);

        // f. Build step usage
        let step_usage = TokenUsage {
            input_tokens: response.usage.input_tokens.total.unwrap_or(0),
            output_tokens: response.usage.output_tokens.total.unwrap_or(0),
        };

        let unified_reason = response.finish_reason.unified.clone();

        // g. If ToolCalls stop reason and we have tools, execute them
        let tool_results =
            if unified_reason == UnifiedFinishReason::ToolCalls && settings.tools.is_some() {
                let results = execute_tools(
                    settings.tools.as_ref().unwrap(),
                    &tool_calls,
                    &settings.workspace,
                )
                .await;

                // Append assistant message + tool results to history
                let assistant_msg = LLMMessage::assistant(llm_content.clone());
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
                let assistant_msg = LLMMessage::assistant(llm_content.clone());
                history.push(assistant_msg);
                vec![]
            };

        let step = StepResult {
            text: text.clone(),
            reasoning: reasoning.clone(),
            tool_calls: tool_calls.clone(),
            tool_results: tool_results.clone(),
            finish_reason: convert_unified_reason(&unified_reason),
            usage: step_usage,
        };
        steps.push(step);

        // h. Check stop condition
        if unified_reason != UnifiedFinishReason::ToolCalls {
            break; // Natural stop
        }
        if tool_calls.is_empty() {
            break; // No tool calls despite ToolCalls reason
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

/// Extract all text content blocks from LLM content, joined together.
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

/// Extract tool calls from LLM content blocks.
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

/// Convert Provider V4 UnifiedFinishReason to UI FinishReason.
pub(crate) fn convert_unified_reason(reason: &UnifiedFinishReason) -> FinishReason {
    match reason {
        UnifiedFinishReason::Stop => FinishReason::Stop,
        UnifiedFinishReason::ToolCalls => FinishReason::ToolCalls,
        UnifiedFinishReason::Length => FinishReason::MaxTokens,
        UnifiedFinishReason::Error => FinishReason::Error,
        _ => FinishReason::Stop,
    }
}

/// Retry an async operation up to `max_retries` times.
async fn with_retry<F, Fut, T, E>(max_retries: u32, f: F) -> Result<T, E>
where
    F: Fn() -> Fut,
    Fut: std::future::Future<Output = Result<T, E>>,
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
