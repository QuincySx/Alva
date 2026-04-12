// INPUT:  alva_types, alva_agent_core::run_child, alva_agent_scope::blackboard, alva_agent_scope::SpawnScopeImpl
// OUTPUT: AgentSpawnTool, create_agent_spawn_tool, SubAgentExtension
// POS:    AI-driven sub-agent spawning — dynamic roles, optional Blackboard communication.

//! Agent spawn plugin — the AI primitive for creating sub-agents.
//!
//! The LLM decides when to spawn, what role to give, whether to share
//! a Blackboard. Orchestration lives in the LLM's reasoning, not in
//! code-level graph definitions.
//!
//! Exposes [`SubAgentExtension`] which wires the `agent` tool into the
//! agent using `finalize()` so the tool receives the final tool list and
//! model as its root `SpawnScopeImpl`.

use std::sync::Arc;

use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use alva_agent_core::run_child::{run_child_agent, ChildAgentParams};
use alva_types::base::cancel::CancellationToken;
use alva_types::base::error::AgentError;
use alva_types::scope::{ChildScopeConfig, ScopeError};
use alva_types::tool::Tool;
use alva_types::tool::execution::{ToolExecutionContext, ToolOutput};

use alva_agent_scope::blackboard::{AgentProfile, BoardMessage, MessageKind};
use alva_agent_scope::board_registry::BoardRegistry;
use alva_agent_scope::SpawnScopeImpl;

// ---------------------------------------------------------------------------
// Tool input
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
struct SpawnInput {
    task: String,
    role: String,
    #[serde(default)]
    system_prompt: String,
    #[serde(default)]
    inherit_tools: bool,
    #[serde(default)]
    board: Option<String>,
}

// ---------------------------------------------------------------------------
// AgentSpawnTool
// ---------------------------------------------------------------------------

pub struct AgentSpawnTool {
    scope: Arc<SpawnScopeImpl>,
    /// Board registry for inter-agent communication (optional, independent of scope).
    board_registry: Arc<BoardRegistry>,
}

impl AgentSpawnTool {
    pub fn new(scope: Arc<SpawnScopeImpl>) -> Self {
        Self {
            scope,
            board_registry: Arc::new(BoardRegistry::new()),
        }
    }

    pub fn with_board_registry(mut self, registry: Arc<BoardRegistry>) -> Self {
        self.board_registry = registry;
        self
    }
}

#[async_trait]
impl Tool for AgentSpawnTool {
    fn name(&self) -> &str {
        "agent"
    }

    fn description(&self) -> &str {
        "Spawn a sub-agent to handle a specific task. The sub-agent runs independently \
         with its own role and system prompt. Use 'board' to enable communication between \
         multiple agents via a shared workspace. Sub-agents can spawn further sub-agents \
         up to the configured depth limit."
    }

    /// Sub-agents run under their own `SpawnScopeImpl` timeout budget
    /// (enforced inside `run_child_agent`). Opting out of the generic
    /// per-tool timeout middleware prevents it from pre-empting the
    /// scope budget with a shorter default.
    fn manages_own_timeout(&self) -> bool {
        true
    }

    fn parameters_schema(&self) -> Value {
        serde_json::json!({
            "type": "object",
            "required": ["task", "role"],
            "properties": {
                "task": {
                    "type": "string",
                    "description": "Complete task description for the sub-agent"
                },
                "role": {
                    "type": "string",
                    "description": "Short role name (e.g. 'planner', 'coder', 'reviewer')"
                },
                "system_prompt": {
                    "type": "string",
                    "description": "System prompt for the sub-agent. If empty, a default is generated from the role."
                },
                "inherit_tools": {
                    "type": "boolean",
                    "description": "Whether the sub-agent inherits tools (shell, file, etc). Default: false (reasoning only)."
                },
                "board": {
                    "type": "string",
                    "description": "Shared board ID for multi-agent communication. Agents on the same board see each other's output."
                }
            }
        })
    }

    async fn execute(
        &self,
        input: Value,
        ctx: &dyn ToolExecutionContext,
    ) -> Result<ToolOutput, AgentError> {
        let input: SpawnInput = serde_json::from_value(input).map_err(|e| {
            AgentError::ToolError {
                tool_name: "agent".into(),
                message: format!("invalid input: {}", e),
            }
        })?;

        let system_prompt = if input.system_prompt.is_empty() {
            format!(
                "You are a {} agent. Complete the task given to you.",
                input.role
            )
        } else {
            input.system_prompt
        };

        // Build child scope — spawn_child() enforces the depth limit
        let child_config = ChildScopeConfig::new(&input.role)
            .with_system_prompt(&system_prompt)
            .inherit_tools(input.inherit_tools);

        let child_scope = match self.scope.spawn_child(child_config).await {
            Ok(s) => s,
            Err(ScopeError::DepthExceeded { current, max }) => {
                return Ok(ToolOutput::error(format!(
                    "Cannot spawn: depth {}/{} exceeded. Handle the task directly.",
                    current, max
                )));
            }
            Err(e) => {
                return Err(AgentError::ToolError {
                    tool_name: "agent".into(),
                    message: e.to_string(),
                });
            }
        };

        // Build context with board messages if applicable.
        // Board is managed by board_registry (independent of scope).
        let mut task_context = input.task.clone();
        if let Some(board_id) = &input.board {
            let scope_key = self.scope.id();
            let board = self.board_registry.get_or_create(scope_key, board_id).await;

            board
                .register(AgentProfile::new(&input.role, &input.role))
                .await;

            let (log, count) = board.render_chat_log(30).await;
            if count > 0 {
                task_context = format!(
                    "{}\n\n## Team Communication\n{}\n\nYou are '{}'. Respond based on the above context.",
                    input.task, log, input.role,
                );
            }
        }

        // Build child tools — from scope + give child its own spawn tool (sharing board_registry)
        let mut child_tools = child_scope.tools(input.inherit_tools);
        child_tools.push(Arc::new(AgentSpawnTool {
            scope: child_scope.clone(),
            board_registry: self.board_registry.clone(),
        }));

        tracing::info!(
            sub_agent_task = %input.task,
            sub_agent_role = %input.role,
            depth = child_scope.depth(),
            parent_scope_id = %self.scope.id(),
            inherit_tools = input.inherit_tools,
            tool_count = child_tools.len(),
            "sub-agent spawned"
        );

        // Run child agent using the shared helper
        let result = run_child_agent(ChildAgentParams {
            model: child_scope.model(),
            tools: child_tools,
            system_prompt,
            task: task_context,
            max_iterations: child_scope.max_iterations(),
            timeout: child_scope.timeout(),
            parent_session_id: Some(self.scope.session_id().to_string()),
            cancel: CancellationToken::new(),
            middleware: None, // TODO: accept parent middleware for security/timeout propagation
            model_config: None,
            context_window: 0,
            workspace: ctx.workspace().map(|p| p.to_path_buf()),
            bus: ctx.bus().cloned(),
        })
        .await;

        tracing::info!(
            sub_agent_role = %input.role,
            depth = child_scope.depth(),
            parent_scope_id = %self.scope.id(),
            output_len = result.text.len(),
            success = !result.is_error,
            error = result.error.as_deref().unwrap_or(""),
            "sub-agent completed"
        );

        // Post result to board if applicable
        if let Some(board_id) = &input.board {
            let scope_key = self.scope.id();
            let board = self.board_registry.get_or_create(scope_key, board_id).await;
            board
                .post(
                    BoardMessage::new(&input.role, &result.text).with_kind(MessageKind::Artifact {
                        name: format!("{}-output", input.role),
                    }),
                )
                .await;
        }

        child_scope.mark_completed(&result.text);

        if result.is_error {
            Ok(ToolOutput::error(format!(
                "[Agent '{}' error: {}]\n{}",
                input.role,
                result.error.unwrap_or_default(),
                result.text
            )))
        } else {
            Ok(ToolOutput::text(result.text))
        }
    }
}

// ---------------------------------------------------------------------------
// Factory
// ---------------------------------------------------------------------------

pub fn create_agent_spawn_tool(scope: Arc<SpawnScopeImpl>) -> Box<dyn Tool> {
    Box::new(AgentSpawnTool::new(scope))
}

// ---------------------------------------------------------------------------
// Extension
// ---------------------------------------------------------------------------

use crate::extension::{Extension, FinalizeContext};

/// Sub-agent spawning via the `agent` tool.
///
/// Uses `finalize()` because it needs the final tool list and model to
/// construct the `SpawnScopeImpl` root scope.
pub struct SubAgentExtension {
    max_depth: u32,
}

impl SubAgentExtension {
    pub fn new(max_depth: u32) -> Self {
        Self { max_depth }
    }
}

#[async_trait]
impl Extension for SubAgentExtension {
    fn name(&self) -> &str { "sub-agents" }
    fn description(&self) -> &str { "Sub-agent spawning via the agent tool" }

    async fn finalize(&self, ctx: &FinalizeContext) -> Vec<Arc<dyn Tool>> {
        // Build a clean tool list without any placeholder agent tool
        let tools_without_agent: Vec<Arc<dyn Tool>> = ctx.tools.iter()
            .filter(|t| t.name() != "agent")
            .cloned()
            .collect();

        let root_scope = Arc::new(alva_agent_scope::SpawnScopeImpl::root(
            ctx.model.clone(),
            tools_without_agent,
            // 15-minute budget per sub-agent. The parent's ToolTimeoutMiddleware
            // exempts the `agent` tool, so this scope timeout is the single
            // authoritative cap on sub-agent execution.
            std::time::Duration::from_secs(900),
            ctx.max_iterations,
            self.max_depth,
        ));
        let spawn_tool = create_agent_spawn_tool(root_scope);
        vec![Arc::from(spawn_tool)]
    }
}
