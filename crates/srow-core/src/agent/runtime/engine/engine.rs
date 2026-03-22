// INPUT:  std::sync, tokio::sync, crate::agent::runtime::engine::context_manager, crate::domain, crate::error, crate::ports, crate::ui_message_stream, serde, futures, uuid
// OUTPUT: AgentEngine
// POS:    Core agentic loop: prompt -> LLM stream -> tool execution (parallel) -> loop, with UIMessageChunk emission and cancellation support.
use std::sync::Arc;
use tokio::sync::{mpsc, watch};
use futures::StreamExt;

use crate::{
    agent::runtime::engine::context_manager::ContextManager,
    domain::{
        agent::AgentConfig,
        message::{LLMContent, LLMMessage, llm_messages_to_provider_prompt},
        session::SessionStatus,
        tool::ToolCall,
    },
    error::EngineError,
    ports::{
        provider::{
            LanguageModel, LanguageModelCallOptions, LanguageModelStreamPart,
            LanguageModelTool, FunctionTool, UnifiedFinishReason,
        },
        storage::SessionStorage,
        tool::{ToolContext, ToolRegistry},
    },
    ui_message_stream::{FinishReason, TokenUsage, UIMessageChunk},
};

/// Core agent engine: drives the prompt -> LLM -> tool_call -> execute -> loop cycle
///
/// Emits `UIMessageChunk` events for UI consumption.
pub struct AgentEngine {
    config: AgentConfig,
    llm: Arc<dyn LanguageModel>,
    tools: Arc<ToolRegistry>,
    storage: Arc<dyn SessionStorage>,
    context_manager: ContextManager,
    chunk_tx: mpsc::Sender<UIMessageChunk>,
    cancel_rx: watch::Receiver<bool>,
}

impl AgentEngine {
    pub fn new(
        config: AgentConfig,
        llm: Arc<dyn LanguageModel>,
        tools: Arc<ToolRegistry>,
        storage: Arc<dyn SessionStorage>,
        chunk_tx: mpsc::Sender<UIMessageChunk>,
        cancel_rx: watch::Receiver<bool>,
    ) -> Self {
        let context_manager = ContextManager::new(config.compaction_threshold);
        Self {
            config,
            llm,
            tools,
            storage,
            context_manager,
            chunk_tx,
            cancel_rx,
        }
    }

    /// Run the agent loop for a session with an initial user message.
    #[tracing::instrument(name = "agent_turn", skip(self, initial_message), fields(session_id = %session_id))]
    pub async fn run(
        &mut self,
        session_id: &str,
        initial_message: LLMMessage,
    ) -> Result<(), EngineError> {
        // 1. Update session status -> Running
        self.storage
            .update_session_status(session_id, SessionStatus::Running)
            .await?;
        self.storage
            .append_message(session_id, &initial_message)
            .await?;

        // 2. Load full message history
        let mut history = self.storage.get_messages(session_id).await?;

        let tool_ctx = ToolContext {
            session_id: session_id.to_string(),
            workspace: self.config.workspace.clone(),
            allow_dangerous: false,
        };

        let mut iteration = 0u32;

        // Emit Start at the beginning of the agent loop
        let _ = self
            .chunk_tx
            .send(UIMessageChunk::Start {
                message_id: Some(session_id.to_string()),
                message_metadata: None,
            })
            .await;

        loop {
            // Cancel check
            if *self.cancel_rx.borrow() {
                self.storage
                    .update_session_status(session_id, SessionStatus::Cancelled)
                    .await?;
                let _ = self
                    .chunk_tx
                    .send(UIMessageChunk::Finish {
                        finish_reason: FinishReason::Stop,
                        usage: None,
                    })
                    .await;
                return Ok(());
            }

            // Max iterations guard
            if iteration >= self.config.max_iterations {
                let _ = self
                    .storage
                    .update_session_status(session_id, SessionStatus::Error)
                    .await;
                let _ = self
                    .chunk_tx
                    .send(UIMessageChunk::Error {
                        error: format!(
                            "max iterations ({}) reached",
                            self.config.max_iterations
                        ),
                    })
                    .await;
                return Err(EngineError::MaxIterationsReached(
                    self.config.max_iterations,
                ));
            }

            // 3. Context compaction (if threshold exceeded)
            if self
                .context_manager
                .needs_compaction(&history, &self.config.system_prompt)
            {
                history = self
                    .context_manager
                    .compact(history, &self.config.system_prompt)
                    .await?;
            }

            // 4. Get filtered tool definitions and convert to V4 LanguageModelTool
            let tool_defs = self.filtered_tool_definitions();
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

            // 5. Convert LLMMessage history -> LanguageModelMessage prompt
            let prompt = llm_messages_to_provider_prompt(
                &Some(self.config.system_prompt.clone()),
                &history,
            );

            // 6. Build LanguageModelCallOptions
            let options = LanguageModelCallOptions {
                prompt,
                max_output_tokens: Some(self.config.llm.max_tokens),
                temperature: self.config.llm.temperature,
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

            // 7. Stream LLM response via Provider V4
            let llm_span = tracing::info_span!("llm_request");
            let _llm_guard = llm_span.enter();

            let stream_result = self
                .llm
                .do_stream(options)
                .await
                .map_err(|e| EngineError::LLMProvider(e.to_string()))?;

            let mut stream = stream_result.stream;

            // Track text/reasoning streaming state
            let mut text_part_id: Option<String> = None;
            let mut reasoning_part_id: Option<String> = None;

            // Accumulate content from the stream
            let mut accumulated_text = String::new();
            let mut accumulated_reasoning = String::new();
            let mut accumulated_tool_calls: Vec<(String, String, String)> = Vec::new(); // (id, name, input_json)
            let mut tool_input_buffers: std::collections::HashMap<String, (String, String)> =
                std::collections::HashMap::new(); // id -> (name, accumulated_input)

            let mut finish_reason = UnifiedFinishReason::Stop;
            let mut usage_input: u32 = 0;
            let mut usage_output: u32 = 0;
            let mut got_finish = false;

            while let Some(part) = stream.next().await {
                match part {
                    LanguageModelStreamPart::TextStart { id } => {
                        text_part_id = Some(id.clone());
                        let _ = self
                            .chunk_tx
                            .send(UIMessageChunk::TextStart { id })
                            .await;
                    }
                    LanguageModelStreamPart::TextDelta { id, delta } => {
                        if text_part_id.is_none() {
                            // Auto-emit TextStart if not yet started
                            let new_id = id.clone();
                            let _ = self
                                .chunk_tx
                                .send(UIMessageChunk::TextStart { id: new_id.clone() })
                                .await;
                            text_part_id = Some(new_id);
                        }
                        accumulated_text.push_str(&delta);
                        let _ = self
                            .chunk_tx
                            .send(UIMessageChunk::TextDelta { id, delta })
                            .await;
                    }
                    LanguageModelStreamPart::TextEnd { id } => {
                        text_part_id = None;
                        let _ = self
                            .chunk_tx
                            .send(UIMessageChunk::TextEnd { id })
                            .await;
                    }
                    LanguageModelStreamPart::ReasoningStart { id } => {
                        reasoning_part_id = Some(id.clone());
                        let _ = self
                            .chunk_tx
                            .send(UIMessageChunk::ReasoningStart { id })
                            .await;
                    }
                    LanguageModelStreamPart::ReasoningDelta { id, delta } => {
                        if reasoning_part_id.is_none() {
                            let new_id = id.clone();
                            let _ = self
                                .chunk_tx
                                .send(UIMessageChunk::ReasoningStart { id: new_id.clone() })
                                .await;
                            reasoning_part_id = Some(new_id);
                        }
                        accumulated_reasoning.push_str(&delta);
                        let _ = self
                            .chunk_tx
                            .send(UIMessageChunk::ReasoningDelta { id, delta })
                            .await;
                    }
                    LanguageModelStreamPart::ReasoningEnd { id } => {
                        reasoning_part_id = None;
                        let _ = self
                            .chunk_tx
                            .send(UIMessageChunk::ReasoningEnd { id })
                            .await;
                    }
                    LanguageModelStreamPart::ToolInputStart { id, tool_name, title } => {
                        tool_input_buffers.insert(id.clone(), (tool_name, String::new()));
                        // Don't emit UIMessageChunk::ToolInputStart yet —
                        // we emit them during execute_tools below
                        let _ = id;
                        let _ = title;
                    }
                    LanguageModelStreamPart::ToolInputDelta { id, delta } => {
                        if let Some(entry) = tool_input_buffers.get_mut(&id) {
                            entry.1.push_str(&delta);
                        }
                    }
                    LanguageModelStreamPart::ToolInputEnd { id } => {
                        if let Some((name, input_json)) = tool_input_buffers.remove(&id) {
                            accumulated_tool_calls.push((id, name, input_json));
                        }
                    }
                    LanguageModelStreamPart::ToolCall { content } => {
                        if let crate::ports::provider::content::LanguageModelContent::ToolCall {
                            tool_call_id,
                            tool_name,
                            input,
                            ..
                        } = content
                        {
                            accumulated_tool_calls.push((tool_call_id, tool_name, input));
                        }
                    }
                    LanguageModelStreamPart::Finish {
                        usage,
                        finish_reason: fr,
                        ..
                    } => {
                        finish_reason = fr.unified;
                        usage_input = usage.input_tokens.total.unwrap_or(0);
                        usage_output = usage.output_tokens.total.unwrap_or(0);
                        got_finish = true;
                    }
                    LanguageModelStreamPart::Error { error } => {
                        tracing::error!(error_type = "llm_stream", error = %error, "LLM stream error");
                    }
                    _ => {
                        // Other stream parts (StreamStart, Metadata, Source, etc.) — ignored
                    }
                }
            }

            // Close any still-open text/reasoning parts
            if let Some(id) = text_part_id.take() {
                let _ = self
                    .chunk_tx
                    .send(UIMessageChunk::TextEnd { id })
                    .await;
            }
            if let Some(id) = reasoning_part_id.take() {
                let _ = self
                    .chunk_tx
                    .send(UIMessageChunk::ReasoningEnd { id })
                    .await;
            }

            if !got_finish {
                return Err(EngineError::LLMStreamInterrupted);
            }

            // Emit token usage
            let _ = self
                .chunk_tx
                .send(UIMessageChunk::TokenUsage {
                    usage: TokenUsage {
                        input_tokens: usage_input,
                        output_tokens: usage_output,
                    },
                })
                .await;

            // 8. Build LLMContent from accumulated data
            let mut response_content: Vec<LLMContent> = Vec::new();
            if !accumulated_reasoning.is_empty() {
                response_content.push(LLMContent::Reasoning {
                    text: accumulated_reasoning,
                });
            }
            if !accumulated_text.is_empty() {
                response_content.push(LLMContent::Text {
                    text: accumulated_text,
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

            // 9. Persist assistant message
            let assistant_msg = LLMMessage::assistant(response_content.clone());
            self.storage
                .append_message(session_id, &assistant_msg)
                .await?;
            history.push(assistant_msg);

            // 10. Check stop reason
            match finish_reason {
                UnifiedFinishReason::Stop | UnifiedFinishReason::ContentFilter | UnifiedFinishReason::Other => {
                    self.storage
                        .update_session_status(session_id, SessionStatus::Completed)
                        .await?;
                    let _ = self
                        .chunk_tx
                        .send(UIMessageChunk::Finish {
                            finish_reason: FinishReason::Stop,
                            usage: None,
                        })
                        .await;
                    return Ok(());
                }
                UnifiedFinishReason::Length => {
                    self.storage
                        .update_session_status(session_id, SessionStatus::Error)
                        .await?;
                    let _ = self
                        .chunk_tx
                        .send(UIMessageChunk::Finish {
                            finish_reason: FinishReason::MaxTokens,
                            usage: None,
                        })
                        .await;
                    return Err(EngineError::MaxTokensReached);
                }
                UnifiedFinishReason::ToolCalls => {
                    // 11. Extract all tool calls and execute in parallel
                    let tool_calls: Vec<ToolCall> = response_content
                        .iter()
                        .filter_map(|c| {
                            if let LLMContent::ToolUse { id, name, input } = c {
                                Some(ToolCall {
                                    id: id.clone(),
                                    name: name.clone(),
                                    input: input.clone(),
                                })
                            } else {
                                None
                            }
                        })
                        .collect();

                    let tool_results =
                        self.execute_tools(&tool_calls, &tool_ctx, session_id).await?;

                    // 12. Append tool result messages to history
                    for result in &tool_results {
                        let msg = LLMMessage::tool_result(
                            &result.tool_call_id,
                            &result.output,
                            result.is_error,
                        );
                        self.storage.append_message(session_id, &msg).await?;
                        history.push(msg);
                    }

                    iteration += 1;
                    // Continue loop
                }
                UnifiedFinishReason::Error => {
                    self.storage
                        .update_session_status(session_id, SessionStatus::Error)
                        .await?;
                    let _ = self
                        .chunk_tx
                        .send(UIMessageChunk::Finish {
                            finish_reason: FinishReason::Error,
                            usage: None,
                        })
                        .await;
                    return Err(EngineError::LLMProvider("Model returned error finish reason".to_string()));
                }
            }
        }
    }

    /// Execute all tool calls in parallel using `futures::future::join_all`.
    #[tracing::instrument(name = "tool_execution", skip(self, calls, ctx), fields(tool_count = calls.len()))]
    async fn execute_tools(
        &self,
        calls: &[ToolCall],
        ctx: &ToolContext,
        _session_id: &str,
    ) -> Result<Vec<crate::domain::tool::ToolResult>, EngineError> {
        // Emit ToolInputStart + ToolInputAvailable events for all calls first
        for call in calls {
            let _ = self
                .chunk_tx
                .send(UIMessageChunk::ToolInputStart {
                    id: call.id.clone(),
                    tool_name: call.name.clone(),
                    title: None,
                })
                .await;
            let _ = self
                .chunk_tx
                .send(UIMessageChunk::ToolInputAvailable {
                    id: call.id.clone(),
                    input: call.input.clone(),
                })
                .await;
        }

        // Execute all tool calls in parallel
        let futures: Vec<_> = calls
            .iter()
            .map(|call| {
                tracing::info!(tool_name = %call.name, "executing tool");
                let tools = self.tools.clone();
                let ctx = ctx.clone();
                let call = call.clone();
                async move {
                    let start = std::time::Instant::now();
                    let result = match tools.get(&call.name) {
                        Some(tool) => tool.execute(call.input.clone(), &ctx).await,
                        None => Err(EngineError::ToolNotFound(call.name.clone())),
                    };
                    let duration_ms = start.elapsed().as_millis() as u64;

                    match result {
                        Ok(r) => r,
                        Err(e) => crate::domain::tool::ToolResult {
                            tool_call_id: call.id.clone(),
                            tool_name: call.name.clone(),
                            output: format!("Error: {e}"),
                            is_error: true,
                            duration_ms,
                        },
                    }
                }
            })
            .collect();

        let results = futures::future::join_all(futures).await;

        // Emit ToolOutput events for all results
        for tool_result in &results {
            if tool_result.is_error {
                let _ = self
                    .chunk_tx
                    .send(UIMessageChunk::ToolOutputError {
                        id: tool_result.tool_call_id.clone(),
                        error: tool_result.output.clone(),
                    })
                    .await;
            } else {
                // Try to parse output as JSON, fallback to string value
                let output = serde_json::from_str(&tool_result.output)
                    .unwrap_or(serde_json::Value::String(tool_result.output.clone()));
                let _ = self
                    .chunk_tx
                    .send(UIMessageChunk::ToolOutputAvailable {
                        id: tool_result.tool_call_id.clone(),
                        output,
                    })
                    .await;
            }
        }

        Ok(results)
    }

    fn filtered_tool_definitions(&self) -> Vec<crate::domain::tool::ToolDefinition> {
        match &self.config.allowed_tools {
            None => self.tools.definitions(),
            Some(allowed) => self
                .tools
                .definitions()
                .into_iter()
                .filter(|d| allowed.contains(&d.name))
                .collect(),
        }
    }

    /// Sub-1 backward compatibility: mock execution that simulates a streaming reply.
    ///
    /// Sends UIMessageChunk events character by character, then a Finish event.
    /// Replace calls to this with `AgentEngine::run()` when ready.
    pub fn run_mock(
        session_id: &str,
        prompt: &str,
        event_tx: std::sync::mpsc::Sender<UIMessageChunk>,
    ) {
        let reply = format!("Mock reply: I received your message -- \"{}\"", prompt);

        let _ = event_tx.send(UIMessageChunk::Start {
            message_id: Some(session_id.to_string()),
            message_metadata: None,
        });

        let text_id = uuid::Uuid::new_v4().to_string();
        let _ = event_tx.send(UIMessageChunk::TextStart {
            id: text_id.clone(),
        });

        for ch in reply.chars() {
            std::thread::sleep(std::time::Duration::from_millis(25));
            let _ = event_tx.send(UIMessageChunk::TextDelta {
                id: text_id.clone(),
                delta: ch.to_string(),
            });
        }

        let _ = event_tx.send(UIMessageChunk::TextEnd { id: text_id });
        let _ = event_tx.send(UIMessageChunk::Finish {
            finish_reason: FinishReason::Stop,
            usage: None,
        });
    }
}
