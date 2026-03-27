// INPUT:  async_trait, crate::connection, crate::error, crate::protocol::bootstrap
// OUTPUT: AgentDelegate, AcpAgentDelegate, DelegateResult, DelegateFinishReason, DelegateToolCallSummary
// POS:    Trait for driving external Agent invocation and concrete ACP protocol implementation

use std::sync::Arc;

use async_trait::async_trait;

use crate::connection::{AgentDiscovery, AcpProcessManager, ExternalAgentKind};
use crate::error::AcpError;
use crate::protocol::bootstrap::ModelConfig;

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

/// AgentDelegate -- orchestration layer drives external Agent via this trait.
#[async_trait]
pub trait AgentDelegate: Send + Sync {
    fn agent_kind(&self) -> &str;

    async fn delegate(
        &self,
        prompt: String,
        workspace: std::path::PathBuf,
    ) -> Result<DelegateResult, AcpError>;

    async fn cancel(&self) -> Result<(), AcpError>;
}

/// ACP protocol concrete implementation of AgentDelegate.
///
/// Spawns an external agent process, sends a prompt, collects inbound
/// messages until completion, and returns the aggregated result.
pub struct AcpAgentDelegate {
    kind: ExternalAgentKind,
    model_config: ModelConfig,
    process_manager: Arc<AcpProcessManager>,
    /// Active process_id (set after first delegation).
    active_process_id: tokio::sync::Mutex<Option<String>>,
}

impl AcpAgentDelegate {
    pub fn new(
        kind: ExternalAgentKind,
        model_config: ModelConfig,
        process_manager: Arc<AcpProcessManager>,
    ) -> Self {
        Self {
            kind,
            model_config,
            process_manager,
            active_process_id: tokio::sync::Mutex::new(None),
        }
    }
}

#[async_trait]
impl AgentDelegate for AcpAgentDelegate {
    fn agent_kind(&self) -> &str {
        match &self.kind {
            ExternalAgentKind::Named { id, .. } => id.as_str(),
            ExternalAgentKind::Generic { command } => command.as_str(),
        }
    }

    async fn delegate(
        &self,
        prompt: String,
        workspace: std::path::PathBuf,
    ) -> Result<DelegateResult, AcpError> {
        use crate::protocol::bootstrap::BootstrapPayload;
        use crate::protocol::message::{AcpInboundMessage, AcpOutboundMessage};

        let discovery = AgentDiscovery::new("alva");
        let bootstrap = BootstrapPayload {
            workspace: workspace.to_string_lossy().to_string(),
            model_config: self.model_config.clone(),
            authorized_roots: vec![workspace.to_string_lossy().to_string()],
            sandbox_level: crate::protocol::bootstrap::SandboxLevel::default(),
            attachment_paths: Vec::new(),
            protocol_version: env!("CARGO_PKG_VERSION").to_string(),
        };

        let process_id = self
            .process_manager
            .spawn(&discovery, self.kind.clone(), bootstrap)
            .await?;

        *self.active_process_id.lock().await = Some(process_id.clone());

        // Send prompt
        self.process_manager
            .send(
                &process_id,
                AcpOutboundMessage::Prompt {
                    content: prompt,
                    resume: None,
                },
            )
            .await?;

        // Collect output from broadcast
        let mut rx = self.process_manager.subscribe();
        let mut output = String::new();
        let mut tool_calls = Vec::new();
        let mut finish_reason = DelegateFinishReason::Complete;

        loop {
            match tokio::time::timeout(std::time::Duration::from_secs(900), rx.recv()).await {
                Ok(Ok((pid, msg))) if pid == process_id => match msg {
                    AcpInboundMessage::SessionUpdate { content, .. }
                    | AcpInboundMessage::MessageUpdate { content, .. } => {
                        use crate::protocol::content::ContentBlock;
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
                    | AcpInboundMessage::FinishData { .. } => {
                        break;
                    }
                    AcpInboundMessage::ErrorData { data } => {
                        finish_reason = DelegateFinishReason::Error {
                            message: data.message.clone(),
                        };
                        break;
                    }
                    AcpInboundMessage::PingPong { data } => {
                        let _ = self
                            .process_manager
                            .send(
                                &process_id,
                                AcpOutboundMessage::Pong { id: data.id },
                            )
                            .await;
                    }
                    _ => {}
                },
                Ok(Ok(_)) => continue, // Different process
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
        *self.active_process_id.lock().await = None;

        Ok(DelegateResult {
            output,
            finish_reason,
            tool_calls_summary: tool_calls,
        })
    }

    async fn cancel(&self) -> Result<(), AcpError> {
        if let Some(process_id) = self.active_process_id.lock().await.as_ref() {
            self.process_manager
                .send(
                    process_id,
                    crate::protocol::message::AcpOutboundMessage::Cancel,
                )
                .await?;
        }
        Ok(())
    }
}
