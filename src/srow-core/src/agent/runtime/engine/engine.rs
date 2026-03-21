// INPUT:  std::sync, tokio::sync, crate::agent::runtime::engine::context_manager, crate::domain, crate::error, crate::ports, crate::ui_message_stream, serde, futures, uuid
// OUTPUT: AgentEngine
// POS:    Core agentic loop: prompt -> LLM stream -> tool execution (parallel) -> loop, with UIMessageChunk emission and cancellation support.
use std::sync::Arc;
use tokio::sync::{mpsc, watch};

use crate::{
    agent::runtime::engine::context_manager::ContextManager,
    domain::{
        agent::AgentConfig,
        message::{LLMContent, LLMMessage},
        session::SessionStatus,
        tool::ToolCall,
    },
    error::EngineError,
    ports::{
        llm_provider::{LLMProvider, LLMRequest, StopReason, StreamChunk},
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
    llm: Arc<dyn LLMProvider>,
    tools: Arc<ToolRegistry>,
    storage: Arc<dyn SessionStorage>,
    context_manager: ContextManager,
    chunk_tx: mpsc::Sender<UIMessageChunk>,
    cancel_rx: watch::Receiver<bool>,
}

impl AgentEngine {
    pub fn new(
        config: AgentConfig,
        llm: Arc<dyn LLMProvider>,
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
                    .compact(history, &self.config.system_prompt, self.llm.as_ref())
                    .await?;
            }

            // 4. Get filtered tool definitions
            let tool_defs = self.filtered_tool_definitions();

            // 5. Build LLM request
            let request = LLMRequest {
                messages: history.clone(),
                tools: tool_defs,
                system: Some(self.config.system_prompt.clone()),
                max_tokens: self.config.llm.max_tokens,
                temperature: self.config.llm.temperature,
            };

            // 6. Stream LLM response
            let (stream_tx, mut stream_rx) = mpsc::channel(256);
            let llm = self.llm.clone();
            let req = request;
            tokio::spawn(async move {
                if let Err(e) = llm.complete_stream(req, stream_tx).await {
                    tracing::error!("LLM stream error: {}", e);
                }
            });

            // Track text/reasoning streaming state
            let mut text_part_id: Option<String> = None;
            let mut reasoning_part_id: Option<String> = None;

            let mut llm_response = None;
            while let Some(chunk) = stream_rx.recv().await {
                match chunk {
                    StreamChunk::TextDelta(text) => {
                        // If text_part_id is None, generate one and emit TextStart first
                        if text_part_id.is_none() {
                            let id = uuid::Uuid::new_v4().to_string();
                            let _ = self
                                .chunk_tx
                                .send(UIMessageChunk::TextStart { id: id.clone() })
                                .await;
                            text_part_id = Some(id);
                        }
                        let _ = self
                            .chunk_tx
                            .send(UIMessageChunk::TextDelta {
                                id: text_part_id.as_ref().unwrap().clone(),
                                delta: text,
                            })
                            .await;
                    }
                    StreamChunk::ThinkingDelta(text) => {
                        // If reasoning_part_id is None, generate one and emit ReasoningStart first
                        if reasoning_part_id.is_none() {
                            let id = uuid::Uuid::new_v4().to_string();
                            let _ = self
                                .chunk_tx
                                .send(UIMessageChunk::ReasoningStart { id: id.clone() })
                                .await;
                            reasoning_part_id = Some(id);
                        }
                        let _ = self
                            .chunk_tx
                            .send(UIMessageChunk::ReasoningDelta {
                                id: reasoning_part_id.as_ref().unwrap().clone(),
                                delta: text,
                            })
                            .await;
                    }
                    StreamChunk::ToolCallDelta { .. } => {
                        // Accumulated in Done; delta events ignored for now
                    }
                    StreamChunk::Done(resp) => {
                        // Close any open text/reasoning parts
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

                        // Emit token usage
                        let _ = self
                            .chunk_tx
                            .send(UIMessageChunk::TokenUsage {
                                usage: TokenUsage {
                                    input_tokens: resp.usage.input_tokens,
                                    output_tokens: resp.usage.output_tokens,
                                },
                            })
                            .await;
                        llm_response = Some(resp);
                    }
                }
            }

            let response = llm_response.ok_or(EngineError::LLMStreamInterrupted)?;

            // 7. Persist assistant message
            let assistant_msg = LLMMessage::assistant(response.content.clone());
            self.storage
                .append_message(session_id, &assistant_msg)
                .await?;
            history.push(assistant_msg);

            // 8. Check stop reason
            match response.stop_reason {
                StopReason::EndTurn | StopReason::StopSequence => {
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
                StopReason::MaxTokens => {
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
                StopReason::ToolUse => {
                    // 9. Extract all tool calls and execute in parallel
                    let tool_calls: Vec<ToolCall> = response
                        .content
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

                    // 10. Append tool result messages to history
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
            }
        }
    }

    /// Execute all tool calls in parallel using `futures::future::join_all`.
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
