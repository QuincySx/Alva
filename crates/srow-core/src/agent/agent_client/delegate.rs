// INPUT:  std::sync, async_trait, tokio::sync, crate::domain::tool, crate::error, crate::ports::tool
// OUTPUT: DelegateResult, DelegateFinishReason, AgentDelegate (trait), AcpAgentDelegate, AcpDelegateTool
// POS:    Wraps ACP external Agent invocation as both a trait (AgentDelegate) and a Tool implementation (AcpDelegateTool).
//         Bodies commented out during migration — depends on deleted UIMessageChunk.
//         TODO: Rebuild using agent-core event types.
use std::sync::Arc;

use async_trait::async_trait;

use crate::{
    domain::tool::{ToolDefinition, ToolResult},
    error::EngineError,
    ports::tool::{Tool, ToolContext},
};

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
    description: String,
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
            description: description.into(),
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
            .delegate(task, ctx.workspace.clone())
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
