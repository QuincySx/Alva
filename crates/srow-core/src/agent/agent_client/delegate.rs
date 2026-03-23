// INPUT:  std::sync, async_trait, tokio::sync, agent_types, crate::error
// OUTPUT: DelegateResult, DelegateFinishReason, AgentDelegate (trait), AcpAgentDelegate, AcpDelegateTool
// POS:    Wraps ACP external Agent invocation as both a trait (AgentDelegate) and a Tool implementation (AcpDelegateTool).
//         AcpAgentDelegate bodies stubbed with todo!() — awaiting full ACP delegate rebuild on agent-core event types.
use std::sync::Arc;

use async_trait::async_trait;

use agent_types::{AgentError, CancellationToken, Tool, ToolContext, ToolResult};
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
/// Body commented out — depends on deleted UIMessageChunk.
pub struct AcpAgentDelegate {
    kind: super::connection::discovery::ExternalAgentKind,
}

impl AcpAgentDelegate {
    pub fn new(
        kind: super::connection::discovery::ExternalAgentKind,
        _model_config: super::protocol::bootstrap::ModelConfig,
        _process_manager: Arc<super::connection::factory::AcpProcessManager>,
    ) -> Self {
        Self { kind }
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
        _prompt: String,
        _workspace: std::path::PathBuf,
    ) -> Result<DelegateResult, EngineError> {
        todo!("Rebuild AcpAgentDelegate on agent-core event types")
    }

    async fn cancel(&self) -> Result<(), EngineError> {
        todo!("Rebuild AcpAgentDelegate on agent-core event types")
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
        _cancel: &CancellationToken,
        ctx: &dyn ToolContext,
    ) -> Result<ToolResult, AgentError> {
        let task = input["task"]
            .as_str()
            .ok_or_else(|| AgentError::ToolError { tool_name: self.tool_name.clone(), message: "missing 'task' field".to_string() })?
            .to_string();

        let result = self
            .delegate
            .delegate(task, ctx.local().map(|l| l.workspace().to_path_buf()).unwrap_or_else(|| std::path::PathBuf::from(".")))
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

        Ok(ToolResult {
            content: output,
            is_error,
            details: None,
        })
    }
}
