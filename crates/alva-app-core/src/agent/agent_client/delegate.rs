// INPUT:  std::sync, async_trait, tokio::sync, alva_types, crate::error
// OUTPUT: DelegateResult, DelegateFinishReason, AgentDelegate (trait), AcpAgentDelegate, AcpDelegateTool
// POS:    Wraps ACP external Agent invocation as both a trait (AgentDelegate) and a Tool implementation (AcpDelegateTool).
//         AcpAgentDelegate bodies stubbed with todo!() — awaiting full ACP delegate rebuild on alva-core event types.
use std::sync::Arc;

use async_trait::async_trait;

use alva_types::{AgentError, Tool, ToolExecutionContext, ToolOutput};
use crate::error::EngineError;

/// Delegate execution result
#[derive(Debug, Clone)]
pub struct DelegateResult {
    pub output: String,
    pub finish_reason: DelegateFinishReason,
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

/// AgentDelegate -- Sub-5 orchestration layer drives external Agent via this trait.
#[async_trait]
pub trait AgentDelegate: Send + Sync {
    fn agent_kind(&self) -> &str;

    async fn delegate(
        &self,
        prompt: String,
        workspace: std::path::PathBuf,
    ) -> Result<DelegateResult, EngineError>;

    async fn cancel(&self) -> Result<(), EngineError>;
}

/// ACP protocol concrete implementation of AgentDelegate.
///
/// Uses the app-level `AcpProcessManager` to spawn and communicate
/// with external agent processes.
pub struct AcpAgentDelegate {
    kind: super::connection::discovery::ExternalAgentKind,
    model_config: super::protocol::bootstrap::ModelConfig,
    process_manager: Arc<super::connection::factory::AcpProcessManager>,
}

impl AcpAgentDelegate {
    pub fn new(
        kind: super::connection::discovery::ExternalAgentKind,
        model_config: super::protocol::bootstrap::ModelConfig,
        process_manager: Arc<super::connection::factory::AcpProcessManager>,
    ) -> Self {
        Self {
            kind,
            model_config,
            process_manager,
        }
    }
}

#[async_trait]
impl AgentDelegate for AcpAgentDelegate {
    fn agent_kind(&self) -> &str {
        match &self.kind {
            super::connection::discovery::ExternalAgentKind::Named { id, .. } => id.as_str(),
            super::connection::discovery::ExternalAgentKind::Generic { command } => command.as_str(),
        }
    }

    async fn delegate(
        &self,
        prompt: String,
        workspace: std::path::PathBuf,
    ) -> Result<DelegateResult, EngineError> {
        use super::protocol::bootstrap::{BootstrapPayload, SandboxLevel};
        use super::protocol::content::ContentBlock;
        use super::protocol::message::{AcpInboundMessage, AcpOutboundMessage};

        let ws = workspace.to_string_lossy().to_string();
        let bootstrap = BootstrapPayload {
            workspace: ws.clone(),
            model_config: self.model_config.clone(),
            authorized_roots: vec![ws],
            sandbox_level: SandboxLevel::None,
            attachment_paths: Vec::new(),
            protocol_version: env!("CARGO_PKG_VERSION").to_string(),
        };

        let process_id = self
            .process_manager
            .spawn(self.kind.clone(), bootstrap)
            .await
            .map_err(|e| EngineError::ToolExecution(e.to_string()))?;

        // Send prompt
        self.process_manager
            .send(
                &process_id,
                AcpOutboundMessage::Prompt {
                    content: prompt,
                    resume: None,
                },
            )
            .await
            .map_err(|e| EngineError::ToolExecution(e.to_string()))?;

        // Collect output
        let mut rx = self.process_manager.subscribe();
        let mut output = String::new();
        let mut tool_calls = Vec::new();
        let mut finish_reason = DelegateFinishReason::Complete;

        loop {
            match tokio::time::timeout(std::time::Duration::from_secs(900), rx.recv()).await {
                Ok(Ok((pid, msg))) if pid == process_id => match msg {
                    AcpInboundMessage::SessionUpdate { content, .. }
                    | AcpInboundMessage::MessageUpdate { content, .. } => {
                        for block in &content {
                            if let ContentBlock::Text { text, .. } = block {
                                output.push_str(text);
                            }
                        }
                    }
                    AcpInboundMessage::ToolCallData { data } => {
                        tool_calls.push(DelegateToolCallSummary {
                            tool_name: data.tool_name.clone(),
                            is_error: false,
                        });
                    }
                    AcpInboundMessage::TaskComplete { .. }
                    | AcpInboundMessage::FinishData { .. } => break,
                    AcpInboundMessage::ErrorData { data } => {
                        finish_reason = DelegateFinishReason::Error {
                            message: data.message.clone(),
                        };
                        break;
                    }
                    AcpInboundMessage::PingPong { data } => {
                        let _ = self
                            .process_manager
                            .send(&process_id, AcpOutboundMessage::Pong { id: data.id })
                            .await;
                    }
                    _ => {}
                },
                Ok(Ok(_)) => continue,
                Ok(Err(_)) => {
                    finish_reason = DelegateFinishReason::ProcessCrashed;
                    break;
                }
                Err(_) => {
                    finish_reason = DelegateFinishReason::Error {
                        message: "delegate timed out".to_string(),
                    };
                    break;
                }
            }
        }

        self.process_manager.shutdown(&process_id).await;

        Ok(DelegateResult {
            output,
            finish_reason,
            tool_calls_summary: tool_calls,
        })
    }

    async fn cancel(&self) -> Result<(), EngineError> {
        // Cancel is best-effort — if no active process, silently succeed.
        Ok(())
    }
}

/// Wraps AgentDelegate as Tool trait.
/// Body commented out — depends on deleted UIMessageChunk.
pub struct AcpDelegateTool {
    delegate: Arc<dyn AgentDelegate>,
    tool_name: String,
    tool_description: String,
}

impl AcpDelegateTool {
    pub fn new(
        delegate: Arc<dyn AgentDelegate>,
        tool_name: impl Into<String>,
        description: impl Into<String>,
    ) -> Self {
        Self {
            delegate,
            tool_name: tool_name.into(),
            tool_description: description.into(),
        }
    }
}

#[async_trait]
impl Tool for AcpDelegateTool {
    fn name(&self) -> &str {
        &self.tool_name
    }

    fn description(&self) -> &str {
        &self.tool_description
    }

    fn parameters_schema(&self) -> serde_json::Value {
        serde_json::json!({
            "type": "object",
            "required": ["task"],
            "properties": {
                "task": {
                    "type": "string",
                    "description": "Complete task description for the external agent"
                }
            }
        })
    }

    async fn execute(
        &self,
        input: serde_json::Value,
        ctx: &dyn ToolExecutionContext,
    ) -> Result<ToolOutput, AgentError> {
        let task = input["task"]
            .as_str()
            .ok_or_else(|| AgentError::ToolError { tool_name: self.tool_name.clone(), message: "missing 'task' field".to_string() })?
            .to_string();

        let workspace_path = ctx.workspace().map(|w| w.to_path_buf()).unwrap_or_else(|| std::path::PathBuf::from("."));
        let result = self
            .delegate
            .delegate(task, workspace_path)
            .await
            .map_err(|e| AgentError::ToolError { tool_name: self.tool_name.clone(), message: e.to_string() })?;

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

        if is_error {
            Ok(ToolOutput::error(output))
        } else {
            Ok(ToolOutput::text(output))
        }
    }
}
