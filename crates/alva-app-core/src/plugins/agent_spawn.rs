// INPUT:  alva_types, alva_agent_core, crate::plugins::blackboard, crate::scope::SpawnScopeImpl
// OUTPUT: AgentSpawnTool, create_agent_spawn_tool
// POS:    Single primitive for spawning sub-agents. Uses SpawnScopeImpl for state management.

//! Agent spawn tool — the ONE primitive for creating sub-agents.
//!
//! The LLM decides when to spawn, what role to give, whether to share
//! a Blackboard. Orchestration lives in the LLM's reasoning, not in
//! code-level graph definitions.
//!
//! Depth is controlled by the `SpawnScopeImpl` — the scope enforces
//! depth limits when `spawn_child()` is called. Each child scope shares
//! the same tree-wide BoardRegistry and SessionTracker.

use std::sync::Arc;

use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use alva_agent_core::{Agent, AgentEvent, AgentHooks, AgentMessage, ConvertToLlmFn};
use alva_types::base::cancel::CancellationToken;
use alva_types::base::error::AgentError;
use alva_types::base::message::Message;
use alva_types::scope::{ChildScopeConfig, ScopeError};
use alva_types::tool::{Tool, ToolContext, ToolResult};

use crate::plugins::blackboard::{AgentProfile, BoardMessage, MessageKind};
use crate::scope::SpawnScopeImpl;

// ---------------------------------------------------------------------------
// Tool input
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
struct SpawnInput {
    /// What the sub-agent should do.
    task: String,
    /// Short role name (e.g., "planner", "coder", "reviewer").
    role: String,
    /// System prompt for the sub-agent.
    #[serde(default)]
    system_prompt: String,
    /// Whether the sub-agent inherits the parent's tools. Default: false.
    #[serde(default)]
    inherit_tools: bool,
    /// Optional shared board ID. If provided, the sub-agent joins this board
    /// and can see messages from other agents on the same board.
    #[serde(default)]
    board: Option<String>,
}

// ---------------------------------------------------------------------------
// AgentSpawnTool
// ---------------------------------------------------------------------------

/// The single primitive for sub-agent creation.
///
/// Holds a reference to the current scope. When a child is spawned,
/// `scope.spawn_child()` enforces depth limits and creates a new scope
/// that shares the tree-wide BoardRegistry and SessionTracker.
pub struct AgentSpawnTool {
    scope: Arc<SpawnScopeImpl>,
}

impl AgentSpawnTool {
    pub fn new(scope: Arc<SpawnScopeImpl>) -> Self {
        Self { scope }
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
        _cancel: &CancellationToken,
        _ctx: &dyn ToolContext,
    ) -> Result<ToolResult, AgentError> {
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

        // Build child scope config — spawn_child() enforces the depth limit
        let mut child_config = ChildScopeConfig::new(&input.role)
            .with_system_prompt(&system_prompt)
            .inherit_tools(input.inherit_tools);

        if let Some(board_id) = &input.board {
            child_config = child_config.with_board(board_id);
        }

        let child_scope = match self.scope.spawn_child(child_config).await {
            Ok(s) => s,
            Err(ScopeError::DepthExceeded { current, max }) => {
                return Ok(ToolResult {
                    content: format!(
                        "Cannot spawn: depth {}/{} exceeded. Handle the task directly.",
                        current, max
                    ),
                    is_error: true,
                    details: None,
                });
            }
            Err(e) => {
                return Err(AgentError::ToolError {
                    tool_name: "agent".into(),
                    message: e.to_string(),
                });
            }
        };

        // Build context with board messages if applicable
        let mut task_context = input.task.clone();
        if let Some(board_id) = &input.board {
            let board = child_scope.board(board_id).await;

            // Register on board
            board
                .register(AgentProfile::new(&input.role, &input.role))
                .await;

            // Include board history in context
            let (log, count) = board.render_chat_log(30).await;
            if count > 0 {
                task_context = format!(
                    "{}\n\n## Team Communication\n{}\n\nYou are '{}'. Respond based on the above context.",
                    input.task, log, input.role,
                );
            }
        }

        // Build child agent — tools come from the child scope
        let mut child_tools = child_scope.tools(input.inherit_tools);
        // Give the child its own spawn tool backed by the child scope
        child_tools.push(Arc::new(AgentSpawnTool {
            scope: child_scope.clone(),
        }));

        let convert_fn: ConvertToLlmFn = Arc::new(|ctx| {
            ctx.messages
                .iter()
                .filter_map(|m| match m {
                    AgentMessage::Standard(msg) => Some(msg.clone()),
                    _ => None,
                })
                .collect()
        });

        let mut hooks = AgentHooks::new(convert_fn);
        hooks.max_iterations = child_scope.max_iterations();

        let agent = Agent::new(
            child_scope.model(),
            child_scope.session_id().to_string(),
            &system_prompt,
            hooks,
        );
        agent.set_tools(child_tools).await;

        // Run with timeout from the child scope
        let user_msg = AgentMessage::Standard(Message::user(&task_context));
        let mut rx = agent.prompt(vec![user_msg]);
        let mut output = String::new();

        let result = tokio::time::timeout(child_scope.timeout(), async {
            while let Some(event) = rx.recv().await {
                match event {
                    AgentEvent::MessageEnd { message } => {
                        if let AgentMessage::Standard(msg) = &message {
                            output.push_str(&msg.text_content());
                        }
                    }
                    AgentEvent::AgentEnd { error: Some(e) } => {
                        return Err(e);
                    }
                    AgentEvent::AgentEnd { error: None } => break,
                    _ => {}
                }
            }
            Ok(())
        })
        .await;

        // Post result to board if applicable
        if let Some(board_id) = &input.board {
            let board = child_scope.board(board_id).await;
            board
                .post(
                    BoardMessage::new(&input.role, &output).with_kind(MessageKind::Artifact {
                        name: format!("{}-output", input.role),
                    }),
                )
                .await;
        }

        // Mark child scope as completed
        child_scope.mark_completed(&output);

        match result {
            Ok(Ok(())) => Ok(ToolResult {
                content: output,
                is_error: false,
                details: None,
            }),
            Ok(Err(e)) => Ok(ToolResult {
                content: format!("[Agent '{}' error: {}]\n{}", input.role, e, output),
                is_error: true,
                details: None,
            }),
            Err(_) => {
                agent.cancel();
                let timeout_secs = child_scope.timeout().as_secs();
                Ok(ToolResult {
                    content: format!(
                        "[Agent '{}' timed out after {} seconds]\n{}",
                        input.role, timeout_secs, output
                    ),
                    is_error: true,
                    details: None,
                })
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Factory
// ---------------------------------------------------------------------------

/// Create the `agent` spawn tool backed by a SpawnScope.
///
/// The scope manages depth limits, boards, session tracking, model, and
/// tools — so the tool itself becomes stateless (just a scope reference).
///
/// ```rust,ignore
/// use crate::scope::SpawnScopeImpl;
///
/// let root_scope = Arc::new(SpawnScopeImpl::root(model, tools, timeout, 50, 3));
/// let tool = create_agent_spawn_tool(root_scope);
/// ```
pub fn create_agent_spawn_tool(scope: Arc<SpawnScopeImpl>) -> Box<dyn Tool> {
    Box::new(AgentSpawnTool::new(scope))
}
