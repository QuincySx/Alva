// INPUT:  async_trait, crate::connection, crate::error, crate::protocol::bootstrap
// OUTPUT: AgentDelegate, AcpAgentDelegate, DelegateResult, DelegateFinishReason, DelegateToolCallSummary
// POS:    Trait for driving external Agent invocation and concrete ACP protocol implementation

use std::sync::Arc;

use async_trait::async_trait;

use crate::connection::{AcpProcessManager, ExternalAgentKind};
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
/// Body todo!()'d -- depends on event types being rebuilt.
pub struct AcpAgentDelegate {
    kind: ExternalAgentKind,
}

impl AcpAgentDelegate {
    pub fn new(
        kind: ExternalAgentKind,
        _model_config: ModelConfig,
        _process_manager: Arc<AcpProcessManager>,
    ) -> Self {
        Self { kind }
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
        _prompt: String,
        _workspace: std::path::PathBuf,
    ) -> Result<DelegateResult, AcpError> {
        todo!("Rebuild AcpAgentDelegate on alva-core event types")
    }

    async fn cancel(&self) -> Result<(), AcpError> {
        todo!("Rebuild AcpAgentDelegate on alva-core event types")
    }
}
