// INPUT:  std::sync, async_trait, serde_json, crate::domain::tool, crate::error, crate::ports::tool, super::orchestrator, uuid
// OUTPUT: CreateAgentTool, SendToAgentTool, GetAgentStatusTool, CancelAgentTool, ReviewResultTool, ListTemplatesTool, ListAgentsTool, register_orchestration_tools
// POS:    Seven orchestration tools registered to the brain Agent for creating, messaging, monitoring, and reviewing sub-Agents.
//! Orchestration tools — these are the tools available to the brain Agent
//! for creating and managing sub-Agents.
//!
//! Each tool implements the `Tool` trait so it can be registered in a ToolRegistry
//! and invoked by the brain Agent through normal LLM tool-use flow.

use std::sync::Arc;

use async_trait::async_trait;
use serde_json::{json, Value};

use crate::domain::tool::{ToolDefinition, ToolResult};
use crate::error::EngineError;
use crate::ports::tool::{Tool, ToolContext};

use super::orchestrator::OrchestratorHandle;

// ---------------------------------------------------------------------------
// create_agent
// ---------------------------------------------------------------------------

/// Tool: create_agent
///
/// Creates a new Agent instance from a template and assigns it a task.
/// Returns the instance ID for subsequent operations.
pub struct CreateAgentTool {
    handle: Arc<OrchestratorHandle>,
}

impl CreateAgentTool {
    pub fn new(handle: Arc<OrchestratorHandle>) -> Self {
        Self { handle }
    }
}

#[async_trait]
impl Tool for CreateAgentTool {
    fn name(&self) -> &str {
        "create_agent"
    }

    fn definition(&self) -> ToolDefinition {
        ToolDefinition {
            name: "create_agent".to_string(),
            description: "Create a new Agent instance from a template and assign it a task. \
                          Returns the instance ID."
                .to_string(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "template_id": {
                        "type": "string",
                        "description": "Template ID to instantiate (e.g. 'browser-agent', 'coding-agent', 'system-agent', 'research-agent')"
                    },
                    "task": {
                        "type": "string",
                        "description": "Task description to assign to the new Agent"
                    }
                },
                "required": ["template_id", "task"]
            }),
        }
    }

    async fn execute(&self, input: Value, _ctx: &ToolContext) -> Result<ToolResult, EngineError> {
        let start = std::time::Instant::now();
        let tool_call_id = uuid::Uuid::new_v4().to_string();

        let template_id = input["template_id"]
            .as_str()
            .ok_or_else(|| EngineError::ToolExecution("missing 'template_id'".to_string()))?;
        let task = input["task"]
            .as_str()
            .ok_or_else(|| EngineError::ToolExecution("missing 'task'".to_string()))?;

        match self.handle.create_agent(template_id, task).await {
            Ok(instance_id) => Ok(ToolResult {
                tool_call_id,
                tool_name: "create_agent".to_string(),
                output: json!({
                    "status": "created",
                    "instance_id": instance_id,
                    "template_id": template_id,
                    "task": task
                })
                .to_string(),
                is_error: false,
                duration_ms: start.elapsed().as_millis() as u64,
            }),
            Err(e) => Ok(ToolResult {
                tool_call_id,
                tool_name: "create_agent".to_string(),
                output: json!({ "error": e.to_string() }).to_string(),
                is_error: true,
                duration_ms: start.elapsed().as_millis() as u64,
            }),
        }
    }
}

// ---------------------------------------------------------------------------
// send_to_agent
// ---------------------------------------------------------------------------

/// Tool: send_to_agent
///
/// Sends a message to a running Agent instance.
pub struct SendToAgentTool {
    handle: Arc<OrchestratorHandle>,
}

impl SendToAgentTool {
    pub fn new(handle: Arc<OrchestratorHandle>) -> Self {
        Self { handle }
    }
}

#[async_trait]
impl Tool for SendToAgentTool {
    fn name(&self) -> &str {
        "send_to_agent"
    }

    fn definition(&self) -> ToolDefinition {
        ToolDefinition {
            name: "send_to_agent".to_string(),
            description: "Send a message to a running Agent instance.".to_string(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "agent_id": {
                        "type": "string",
                        "description": "Target Agent instance ID"
                    },
                    "message": {
                        "type": "string",
                        "description": "Message content to send"
                    }
                },
                "required": ["agent_id", "message"]
            }),
        }
    }

    async fn execute(&self, input: Value, _ctx: &ToolContext) -> Result<ToolResult, EngineError> {
        let start = std::time::Instant::now();
        let tool_call_id = uuid::Uuid::new_v4().to_string();

        let agent_id = input["agent_id"]
            .as_str()
            .ok_or_else(|| EngineError::ToolExecution("missing 'agent_id'".to_string()))?;
        let message = input["message"]
            .as_str()
            .ok_or_else(|| EngineError::ToolExecution("missing 'message'".to_string()))?;

        match self.handle.send_to_agent(agent_id, message).await {
            Ok(()) => Ok(ToolResult {
                tool_call_id,
                tool_name: "send_to_agent".to_string(),
                output: json!({
                    "status": "sent",
                    "agent_id": agent_id
                })
                .to_string(),
                is_error: false,
                duration_ms: start.elapsed().as_millis() as u64,
            }),
            Err(e) => Ok(ToolResult {
                tool_call_id,
                tool_name: "send_to_agent".to_string(),
                output: json!({ "error": e.to_string() }).to_string(),
                is_error: true,
                duration_ms: start.elapsed().as_millis() as u64,
            }),
        }
    }
}

// ---------------------------------------------------------------------------
// get_agent_status
// ---------------------------------------------------------------------------

/// Tool: get_agent_status
///
/// Queries the current status of an Agent instance, including its result if completed.
pub struct GetAgentStatusTool {
    handle: Arc<OrchestratorHandle>,
}

impl GetAgentStatusTool {
    pub fn new(handle: Arc<OrchestratorHandle>) -> Self {
        Self { handle }
    }
}

#[async_trait]
impl Tool for GetAgentStatusTool {
    fn name(&self) -> &str {
        "get_agent_status"
    }

    fn definition(&self) -> ToolDefinition {
        ToolDefinition {
            name: "get_agent_status".to_string(),
            description: "Get the current status of an Agent instance. Returns status, task, \
                          result (if completed), and error (if failed)."
                .to_string(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "agent_id": {
                        "type": "string",
                        "description": "Agent instance ID to query"
                    }
                },
                "required": ["agent_id"]
            }),
        }
    }

    async fn execute(&self, input: Value, _ctx: &ToolContext) -> Result<ToolResult, EngineError> {
        let start = std::time::Instant::now();
        let tool_call_id = uuid::Uuid::new_v4().to_string();

        let agent_id = input["agent_id"]
            .as_str()
            .ok_or_else(|| EngineError::ToolExecution("missing 'agent_id'".to_string()))?;

        match self.handle.get_agent_status(agent_id).await {
            Ok(instance) => Ok(ToolResult {
                tool_call_id,
                tool_name: "get_agent_status".to_string(),
                output: serde_json::to_string(&instance)
                    .unwrap_or_else(|_| "serialization error".to_string()),
                is_error: false,
                duration_ms: start.elapsed().as_millis() as u64,
            }),
            Err(e) => Ok(ToolResult {
                tool_call_id,
                tool_name: "get_agent_status".to_string(),
                output: json!({ "error": e.to_string() }).to_string(),
                is_error: true,
                duration_ms: start.elapsed().as_millis() as u64,
            }),
        }
    }
}

// ---------------------------------------------------------------------------
// cancel_agent
// ---------------------------------------------------------------------------

/// Tool: cancel_agent
///
/// Cancels a running Agent instance.
pub struct CancelAgentTool {
    handle: Arc<OrchestratorHandle>,
}

impl CancelAgentTool {
    pub fn new(handle: Arc<OrchestratorHandle>) -> Self {
        Self { handle }
    }
}

#[async_trait]
impl Tool for CancelAgentTool {
    fn name(&self) -> &str {
        "cancel_agent"
    }

    fn definition(&self) -> ToolDefinition {
        ToolDefinition {
            name: "cancel_agent".to_string(),
            description: "Cancel a running Agent instance. The Agent will stop execution.".to_string(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "agent_id": {
                        "type": "string",
                        "description": "Agent instance ID to cancel"
                    }
                },
                "required": ["agent_id"]
            }),
        }
    }

    async fn execute(&self, input: Value, _ctx: &ToolContext) -> Result<ToolResult, EngineError> {
        let start = std::time::Instant::now();
        let tool_call_id = uuid::Uuid::new_v4().to_string();

        let agent_id = input["agent_id"]
            .as_str()
            .ok_or_else(|| EngineError::ToolExecution("missing 'agent_id'".to_string()))?;

        match self.handle.cancel_agent(agent_id).await {
            Ok(()) => Ok(ToolResult {
                tool_call_id,
                tool_name: "cancel_agent".to_string(),
                output: json!({
                    "status": "cancelled",
                    "agent_id": agent_id
                })
                .to_string(),
                is_error: false,
                duration_ms: start.elapsed().as_millis() as u64,
            }),
            Err(e) => Ok(ToolResult {
                tool_call_id,
                tool_name: "cancel_agent".to_string(),
                output: json!({ "error": e.to_string() }).to_string(),
                is_error: true,
                duration_ms: start.elapsed().as_millis() as u64,
            }),
        }
    }
}

// ---------------------------------------------------------------------------
// review_result
// ---------------------------------------------------------------------------

/// Tool: review_result
///
/// Submits an Agent's result to the reviewer Agent for quality assessment.
pub struct ReviewResultTool {
    handle: Arc<OrchestratorHandle>,
}

impl ReviewResultTool {
    pub fn new(handle: Arc<OrchestratorHandle>) -> Self {
        Self { handle }
    }
}

#[async_trait]
impl Tool for ReviewResultTool {
    fn name(&self) -> &str {
        "review_result"
    }

    fn definition(&self) -> ToolDefinition {
        ToolDefinition {
            name: "review_result".to_string(),
            description: "Submit an Agent's completed result to the reviewer Agent for quality \
                          assessment. Returns the reviewer's verdict."
                .to_string(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "agent_id": {
                        "type": "string",
                        "description": "Agent instance ID whose result should be reviewed"
                    }
                },
                "required": ["agent_id"]
            }),
        }
    }

    async fn execute(&self, input: Value, _ctx: &ToolContext) -> Result<ToolResult, EngineError> {
        let start = std::time::Instant::now();
        let tool_call_id = uuid::Uuid::new_v4().to_string();

        let agent_id = input["agent_id"]
            .as_str()
            .ok_or_else(|| EngineError::ToolExecution("missing 'agent_id'".to_string()))?;

        match self.handle.review_result(agent_id).await {
            Ok(review) => Ok(ToolResult {
                tool_call_id,
                tool_name: "review_result".to_string(),
                output: review,
                is_error: false,
                duration_ms: start.elapsed().as_millis() as u64,
            }),
            Err(e) => Ok(ToolResult {
                tool_call_id,
                tool_name: "review_result".to_string(),
                output: json!({ "error": e.to_string() }).to_string(),
                is_error: true,
                duration_ms: start.elapsed().as_millis() as u64,
            }),
        }
    }
}

// ---------------------------------------------------------------------------
// list_templates
// ---------------------------------------------------------------------------

/// Tool: list_templates
///
/// Lists all available Agent templates that can be instantiated.
pub struct ListTemplatesTool {
    handle: Arc<OrchestratorHandle>,
}

impl ListTemplatesTool {
    pub fn new(handle: Arc<OrchestratorHandle>) -> Self {
        Self { handle }
    }
}

#[async_trait]
impl Tool for ListTemplatesTool {
    fn name(&self) -> &str {
        "list_templates"
    }

    fn definition(&self) -> ToolDefinition {
        ToolDefinition {
            name: "list_templates".to_string(),
            description: "List all available Agent templates with their IDs and descriptions."
                .to_string(),
            parameters: json!({
                "type": "object",
                "properties": {},
                "required": []
            }),
        }
    }

    async fn execute(&self, _input: Value, _ctx: &ToolContext) -> Result<ToolResult, EngineError> {
        let start = std::time::Instant::now();
        let tool_call_id = uuid::Uuid::new_v4().to_string();

        let templates = self.handle.list_templates().await;
        let summary: Vec<Value> = templates
            .iter()
            .map(|t| {
                json!({
                    "id": t.id,
                    "name": t.name,
                    "description": t.description
                })
            })
            .collect();

        Ok(ToolResult {
            tool_call_id,
            tool_name: "list_templates".to_string(),
            output: serde_json::to_string(&summary)
                .unwrap_or_else(|_| "[]".to_string()),
            is_error: false,
            duration_ms: start.elapsed().as_millis() as u64,
        })
    }
}

// ---------------------------------------------------------------------------
// list_agents
// ---------------------------------------------------------------------------

/// Tool: list_agents
///
/// Lists all active Agent instances with their current status.
pub struct ListAgentsTool {
    handle: Arc<OrchestratorHandle>,
}

impl ListAgentsTool {
    pub fn new(handle: Arc<OrchestratorHandle>) -> Self {
        Self { handle }
    }
}

#[async_trait]
impl Tool for ListAgentsTool {
    fn name(&self) -> &str {
        "list_agents"
    }

    fn definition(&self) -> ToolDefinition {
        ToolDefinition {
            name: "list_agents".to_string(),
            description: "List all active Agent instances with their IDs, template, status, and task."
                .to_string(),
            parameters: json!({
                "type": "object",
                "properties": {},
                "required": []
            }),
        }
    }

    async fn execute(&self, _input: Value, _ctx: &ToolContext) -> Result<ToolResult, EngineError> {
        let start = std::time::Instant::now();
        let tool_call_id = uuid::Uuid::new_v4().to_string();

        let agents = self.handle.list_agents().await;
        let output = serde_json::to_string(&agents)
            .unwrap_or_else(|_| "[]".to_string());

        Ok(ToolResult {
            tool_call_id,
            tool_name: "list_agents".to_string(),
            output,
            is_error: false,
            duration_ms: start.elapsed().as_millis() as u64,
        })
    }
}

/// Register all orchestration tools into a ToolRegistry.
///
/// Call this when building the brain Agent's ToolRegistry so it has access
/// to all orchestration capabilities.
pub fn register_orchestration_tools(
    registry: &mut crate::ports::tool::ToolRegistry,
    handle: Arc<OrchestratorHandle>,
) {
    registry.register(Box::new(CreateAgentTool::new(handle.clone())));
    registry.register(Box::new(SendToAgentTool::new(handle.clone())));
    registry.register(Box::new(GetAgentStatusTool::new(handle.clone())));
    registry.register(Box::new(CancelAgentTool::new(handle.clone())));
    registry.register(Box::new(ReviewResultTool::new(handle.clone())));
    registry.register(Box::new(ListTemplatesTool::new(handle.clone())));
    registry.register(Box::new(ListAgentsTool::new(handle)));
}
