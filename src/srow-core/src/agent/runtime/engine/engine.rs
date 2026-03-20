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
};
use serde::{Deserialize, Serialize};

/// Events emitted by the engine during agent execution (UI layer subscribes to these)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum EngineEvent {
    TextDelta {
        session_id: String,
        text: String,
    },
    ToolCallStarted {
        session_id: String,
        tool_name: String,
        tool_call_id: String,
    },
    ToolCallCompleted {
        session_id: String,
        tool_call_id: String,
        output: String,
        is_error: bool,
    },
    WaitingForHuman {
        session_id: String,
        question: String,
        ask_id: String,
    },
    Completed {
        session_id: String,
    },
    Error {
        session_id: String,
        error: String,
    },
    TokenUsage {
        session_id: String,
        input: u32,
        output: u32,
        total: u32,
    },
}

/// Core agent engine: drives the prompt -> LLM -> tool_call -> execute -> loop cycle
///
/// For Sub-1 backward compatibility, `AgentEngine::run_mock()` is also available.
pub struct AgentEngine {
    config: AgentConfig,
    llm: Arc<dyn LLMProvider>,
    tools: Arc<ToolRegistry>,
    storage: Arc<dyn SessionStorage>,
    context_manager: ContextManager,
    event_tx: mpsc::Sender<EngineEvent>,
    cancel_rx: watch::Receiver<bool>,
}

impl AgentEngine {
    pub fn new(
        config: AgentConfig,
        llm: Arc<dyn LLMProvider>,
        tools: Arc<ToolRegistry>,
        storage: Arc<dyn SessionStorage>,
        event_tx: mpsc::Sender<EngineEvent>,
        cancel_rx: watch::Receiver<bool>,
    ) -> Self {
        let context_manager = ContextManager::new(config.compaction_threshold);
        Self {
            config,
            llm,
            tools,
            storage,
            context_manager,
            event_tx,
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

        loop {
            // Cancel check
            if *self.cancel_rx.borrow() {
                self.storage
                    .update_session_status(session_id, SessionStatus::Cancelled)
                    .await?;
                let _ = self
                    .event_tx
                    .send(EngineEvent::Completed {
                        session_id: session_id.to_string(),
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
                    .event_tx
                    .send(EngineEvent::Error {
                        session_id: session_id.to_string(),
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
            let (chunk_tx, mut chunk_rx) = mpsc::channel(256);
            let llm = self.llm.clone();
            let req = request;
            tokio::spawn(async move {
                if let Err(e) = llm.complete_stream(req, chunk_tx).await {
                    tracing::error!("LLM stream error: {}", e);
                }
            });

            let mut llm_response = None;
            while let Some(chunk) = chunk_rx.recv().await {
                match chunk {
                    StreamChunk::TextDelta(text) => {
                        let _ = self
                            .event_tx
                            .send(EngineEvent::TextDelta {
                                session_id: session_id.to_string(),
                                text,
                            })
                            .await;
                    }
                    StreamChunk::ToolCallDelta { .. } => {
                        // Accumulated in Done; delta events ignored for now
                    }
                    StreamChunk::Done(resp) => {
                        let _ = self
                            .event_tx
                            .send(EngineEvent::TokenUsage {
                                session_id: session_id.to_string(),
                                input: resp.usage.input_tokens,
                                output: resp.usage.output_tokens,
                                total: resp.usage.input_tokens + resp.usage.output_tokens,
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
                        .event_tx
                        .send(EngineEvent::Completed {
                            session_id: session_id.to_string(),
                        })
                        .await;
                    return Ok(());
                }
                StopReason::MaxTokens => {
                    self.storage
                        .update_session_status(session_id, SessionStatus::Error)
                        .await?;
                    let _ = self
                        .event_tx
                        .send(EngineEvent::Error {
                            session_id: session_id.to_string(),
                            error: "max_tokens reached".to_string(),
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

    /// Execute all tool calls (sequentially for now; parallel in future with join_all)
    async fn execute_tools(
        &self,
        calls: &[ToolCall],
        ctx: &ToolContext,
        session_id: &str,
    ) -> Result<Vec<crate::domain::tool::ToolResult>, EngineError> {
        let mut results = Vec::new();

        for call in calls {
            let _ = self
                .event_tx
                .send(EngineEvent::ToolCallStarted {
                    session_id: session_id.to_string(),
                    tool_name: call.name.clone(),
                    tool_call_id: call.id.clone(),
                })
                .await;

            let start = std::time::Instant::now();
            let result = match self.tools.get(&call.name) {
                Some(tool) => tool.execute(call.input.clone(), ctx).await,
                None => Err(EngineError::ToolNotFound(call.name.clone())),
            };
            let duration_ms = start.elapsed().as_millis() as u64;

            let tool_result = match result {
                Ok(r) => r,
                Err(e) => crate::domain::tool::ToolResult {
                    tool_call_id: call.id.clone(),
                    tool_name: call.name.clone(),
                    output: format!("Error: {e}"),
                    is_error: true,
                    duration_ms,
                },
            };

            let _ = self
                .event_tx
                .send(EngineEvent::ToolCallCompleted {
                    session_id: session_id.to_string(),
                    tool_call_id: tool_result.tool_call_id.clone(),
                    output: tool_result.output.clone(),
                    is_error: tool_result.is_error,
                })
                .await;

            results.push(tool_result);
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
    /// Sends TextDelta events character by character, then a Completed event.
    /// Replace calls to this with `AgentEngine::run()` when ready.
    pub fn run_mock(
        session_id: &str,
        prompt: &str,
        event_tx: std::sync::mpsc::Sender<EngineEvent>,
    ) {
        let reply = format!("Mock reply: I received your message -- \"{}\"", prompt);
        let sid = session_id.to_string();

        for ch in reply.chars() {
            std::thread::sleep(std::time::Duration::from_millis(25));
            let _ = event_tx.send(EngineEvent::TextDelta {
                session_id: sid.clone(),
                text: ch.to_string(),
            });
        }
        let _ = event_tx.send(EngineEvent::Completed {
            session_id: sid,
        });
    }
}
