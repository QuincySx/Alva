use std::sync::Arc;

use async_trait::async_trait;
use tokio::sync::mpsc;

use crate::{
    application::engine::EngineEvent,
    domain::tool::{ToolDefinition, ToolResult},
    error::EngineError,
    ports::tool::{Tool, ToolContext},
};

/// Delegate execution result
#[derive(Debug, Clone)]
pub struct DelegateResult {
    /// External Agent's final text output (concatenated from all TextBlocks)
    pub output: String,
    /// Finish reason
    pub finish_reason: DelegateFinishReason,
    /// Tool call summaries from the external Agent's execution (optional)
    pub tool_calls_summary: Vec<DelegateToolCallSummary>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum DelegateFinishReason {
    Complete,
    Cancelled,
    Error { message: String },
    ProcessCrashed,
}

#[derive(Debug, Clone)]
pub struct DelegateToolCallSummary {
    pub tool_name: String,
    pub is_error: bool,
}

/// AgentDelegate — Sub-5 orchestration layer drives external Agent via this trait.
/// Implementors manage ACP process lifecycle.
#[async_trait]
pub trait AgentDelegate: Send + Sync {
    /// Delegate agent identifier ("claude-code" / "qwen-code" etc.)
    fn agent_kind(&self) -> &str;

    /// Execute task:
    ///   - Start (or reuse existing) ACP child process
    ///   - Send prompt
    ///   - Wait for TaskComplete
    ///   - Return aggregated result
    ///
    /// `event_tx` forwards EngineEvents generated during execution to UI layer
    async fn delegate(
        &self,
        prompt: String,
        workspace: std::path::PathBuf,
        event_tx: mpsc::Sender<EngineEvent>,
    ) -> Result<DelegateResult, EngineError>;

    /// Cancel an in-progress delegation
    async fn cancel(&self) -> Result<(), EngineError>;
}

/// ACP protocol concrete implementation of AgentDelegate
pub struct AcpAgentDelegate {
    kind: super::process::discovery::ExternalAgentKind,
    model_config: super::protocol::bootstrap::ModelConfig,
    process_manager: Arc<super::process::manager::AcpProcessManager>,
    permission_manager: Arc<super::session::permission_manager::PermissionManager>,
    /// Current active process (only one at a time)
    current_process_id: tokio::sync::Mutex<Option<String>>,
}

impl AcpAgentDelegate {
    pub fn new(
        kind: super::process::discovery::ExternalAgentKind,
        model_config: super::protocol::bootstrap::ModelConfig,
        process_manager: Arc<super::process::manager::AcpProcessManager>,
    ) -> Self {
        Self {
            kind,
            model_config,
            process_manager,
            permission_manager: Arc::new(
                super::session::permission_manager::PermissionManager::new(),
            ),
            current_process_id: tokio::sync::Mutex::new(None),
        }
    }
}

#[async_trait]
impl AgentDelegate for AcpAgentDelegate {
    fn agent_kind(&self) -> &str {
        match &self.kind {
            super::process::discovery::ExternalAgentKind::ClaudeCode => "claude-code",
            super::process::discovery::ExternalAgentKind::QwenCode => "qwen-code",
            super::process::discovery::ExternalAgentKind::CodexCli => "codex-cli",
            super::process::discovery::ExternalAgentKind::GeminiCli => "gemini-cli",
            super::process::discovery::ExternalAgentKind::Generic { command } => command.as_str(),
        }
    }

    async fn delegate(
        &self,
        prompt: String,
        workspace: std::path::PathBuf,
        event_tx: mpsc::Sender<EngineEvent>,
    ) -> Result<DelegateResult, EngineError> {
        use super::protocol::{
            bootstrap::BootstrapPayload, content::ContentBlock, message::AcpInboundMessage,
        };
        use super::session::session::{AcpSession, AcpSessionState};

        // 1. Build Bootstrap payload
        let bootstrap = BootstrapPayload {
            workspace: workspace.to_string_lossy().to_string(),
            authorized_roots: vec![workspace.to_string_lossy().to_string()],
            sandbox_level: Default::default(),
            model_config: self.model_config.clone(),
            attachment_paths: vec![],
            srow_version: env!("CARGO_PKG_VERSION").to_string(),
        };

        // 2. Spawn child process
        let process_id = self
            .process_manager
            .spawn(self.kind.clone(), bootstrap)
            .await
            .map_err(|e| EngineError::ToolExecution(e.to_string()))?;

        *self.current_process_id.lock().await = Some(process_id.clone());

        // 3. Create session
        let session_id = uuid::Uuid::new_v4().to_string();
        let session = AcpSession::new(
            session_id.clone(),
            process_id.clone(),
            self.permission_manager.clone(),
            self.process_manager.clone(),
            event_tx.clone(),
        );

        // 4. Subscribe to process messages
        let mut rx = self.process_manager.subscribe();

        // 5. Send prompt
        session
            .send_prompt(prompt, false)
            .await
            .map_err(|e| EngineError::ToolExecution(e.to_string()))?;

        // 6. Drive message loop until TaskComplete / Error / Crash
        let mut output_buffer = String::new();
        let mut tool_calls_summary = vec![];

        loop {
            match tokio::time::timeout(std::time::Duration::from_secs(300), rx.recv()).await {
                Ok(Ok((pid, msg))) if pid == process_id => {
                    // Collect output
                    if let AcpInboundMessage::SessionUpdate { ref content, .. }
                    | AcpInboundMessage::MessageUpdate { ref content, .. } = msg
                    {
                        for block in content {
                            if let ContentBlock::Text { ref text, .. } = block {
                                output_buffer.push_str(text);
                            }
                        }
                    }
                    if let AcpInboundMessage::PostToolUse { ref data } = msg {
                        tool_calls_summary.push(DelegateToolCallSummary {
                            tool_name: data.tool_name.clone(),
                            is_error: data.is_error,
                        });
                    }

                    // Process session state changes
                    session.handle_inbound(msg.clone()).await;

                    // Check termination conditions
                    match session.state.lock().await.clone() {
                        AcpSessionState::Completed => {
                            self.process_manager.shutdown(&process_id).await;
                            return Ok(DelegateResult {
                                output: output_buffer,
                                finish_reason: DelegateFinishReason::Complete,
                                tool_calls_summary,
                            });
                        }
                        AcpSessionState::Cancelled => {
                            self.process_manager.shutdown(&process_id).await;
                            return Ok(DelegateResult {
                                output: output_buffer,
                                finish_reason: DelegateFinishReason::Cancelled,
                                tool_calls_summary,
                            });
                        }
                        AcpSessionState::Error { message } => {
                            self.process_manager.shutdown(&process_id).await;
                            return Ok(DelegateResult {
                                output: output_buffer,
                                finish_reason: DelegateFinishReason::Error { message },
                                tool_calls_summary,
                            });
                        }
                        AcpSessionState::Crashed => {
                            return Ok(DelegateResult {
                                output: output_buffer,
                                finish_reason: DelegateFinishReason::ProcessCrashed,
                                tool_calls_summary,
                            });
                        }
                        _ => {} // Continue waiting
                    }
                }
                Ok(Ok(_)) => continue, // Other process's message, ignore
                Ok(Err(_)) => {
                    // Broadcast channel lagged — skip
                    continue;
                }
                Err(_) => {
                    // Timeout
                    self.process_manager.shutdown(&process_id).await;
                    return Err(EngineError::ToolExecution(
                        "ACP delegate timeout (300s)".to_string(),
                    ));
                }
            }
        }
    }

    async fn cancel(&self) -> Result<(), EngineError> {
        if let Some(pid) = self.current_process_id.lock().await.as_deref() {
            self.process_manager
                .send(
                    pid,
                    super::protocol::message::AcpOutboundMessage::Cancel,
                )
                .await
                .map_err(|e| EngineError::ToolExecution(e.to_string()))
        } else {
            Ok(())
        }
    }
}

/// Wraps AcpAgentDelegate as Tool trait,
/// letting Sub-2's AgentEngine invoke external Agent via tool_call mechanism.
///
/// Function calling schema:
/// {
///   "name": "delegate_to_claude_code",
///   "description": "Delegate coding task to Claude Code",
///   "parameters": {
///     "type": "object",
///     "required": ["task"],
///     "properties": {
///       "task": { "type": "string", "description": "Complete task description" }
///     }
///   }
/// }
pub struct AcpDelegateTool {
    delegate: Arc<dyn AgentDelegate>,
    /// Tool name (e.g. "delegate_to_claude_code")
    tool_name: String,
    /// Tool description (shown to decision Agent's LLM)
    description: String,
    /// Event forwarding sender (injected from ToolContext or externally)
    event_tx: mpsc::Sender<EngineEvent>,
}

impl AcpDelegateTool {
    pub fn new(
        delegate: Arc<dyn AgentDelegate>,
        tool_name: impl Into<String>,
        description: impl Into<String>,
        event_tx: mpsc::Sender<EngineEvent>,
    ) -> Self {
        Self {
            delegate,
            tool_name: tool_name.into(),
            description: description.into(),
            event_tx,
        }
    }
}

#[async_trait]
impl Tool for AcpDelegateTool {
    fn name(&self) -> &str {
        &self.tool_name
    }

    fn definition(&self) -> ToolDefinition {
        ToolDefinition {
            name: self.tool_name.clone(),
            description: self.description.clone(),
            parameters: serde_json::json!({
                "type": "object",
                "required": ["task"],
                "properties": {
                    "task": {
                        "type": "string",
                        "description": "Complete task description for the external agent"
                    }
                }
            }),
        }
    }

    async fn execute(
        &self,
        input: serde_json::Value,
        ctx: &ToolContext,
    ) -> Result<ToolResult, EngineError> {
        let task = input["task"]
            .as_str()
            .ok_or_else(|| EngineError::ToolExecution("missing 'task' field".to_string()))?
            .to_string();

        let start = std::time::Instant::now();
        let result = self
            .delegate
            .delegate(task, ctx.workspace.clone(), self.event_tx.clone())
            .await?;
        let duration_ms = start.elapsed().as_millis() as u64;

        let output = match result.finish_reason {
            DelegateFinishReason::Complete => result.output,
            DelegateFinishReason::Cancelled => format!("[Cancelled]\n{}", result.output),
            DelegateFinishReason::Error { ref message } => {
                format!("[Error: {}]\n{}", message, result.output)
            }
            DelegateFinishReason::ProcessCrashed => {
                format!("[Process Crashed]\n{}", result.output)
            }
        };

        let is_error = !matches!(result.finish_reason, DelegateFinishReason::Complete);

        Ok(ToolResult {
            tool_call_id: uuid::Uuid::new_v4().to_string(),
            tool_name: self.tool_name.clone(),
            output,
            is_error,
            duration_ms,
        })
    }
}
