// INPUT:  alva_types, alva_agent_core, crate::plugins::blackboard, std::sync::Arc
// OUTPUT: AgentSpawnTool, create_agent_spawn_tool
// POS:    Single primitive for spawning sub-agents. Replaces Task Tool and Team Tool.

//! Agent spawn tool — the ONE primitive for creating sub-agents.
//!
//! The LLM decides when to spawn, what role to give, whether to share
//! a Blackboard. Orchestration lives in the LLM's reasoning, not in
//! code-level graph definitions.
//!
//! Depth is controlled by a shared `ToolGuard` — the same instance is
//! passed to children, so the atomic counter tracks depth across the
//! entire agent tree. The specific limit is set at the app layer.

use std::sync::Arc;

use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use alva_agent_core::{Agent, AgentEvent, AgentHooks, AgentMessage, ConvertToLlmFn};
use alva_types::cancel::CancellationToken;
use alva_types::error::AgentError;
use alva_types::message::Message;
use alva_types::model::LanguageModel;
use alva_types::tool::{Tool, ToolContext, ToolResult};
use alva_types::tool_guard::ToolGuard;

use crate::plugins::blackboard::{AgentProfile, Blackboard, BoardMessage, MessageKind};

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
/// Shared across all levels of the agent tree. The `guard` ensures
/// depth never exceeds the configured maximum.
pub struct AgentSpawnTool {
    model: Arc<dyn LanguageModel>,
    parent_tools: Arc<Vec<Arc<dyn Tool>>>,
    guard: ToolGuard,
    boards: Arc<tokio::sync::Mutex<std::collections::HashMap<String, Arc<Blackboard>>>>,
}

impl AgentSpawnTool {
    pub fn new(
        model: Arc<dyn LanguageModel>,
        parent_tools: Vec<Arc<dyn Tool>>,
        guard: ToolGuard,
    ) -> Self {
        Self {
            model,
            parent_tools: Arc::new(parent_tools),
            guard,
            boards: Arc::new(tokio::sync::Mutex::new(std::collections::HashMap::new())),
        }
    }

    /// Get or create a named Blackboard.
    async fn get_or_create_board(&self, board_id: &str) -> Arc<Blackboard> {
        let mut boards = self.boards.lock().await;
        boards
            .entry(board_id.to_string())
            .or_insert_with(|| Arc::new(Blackboard::new()))
            .clone()
    }

    /// Build a child's tool list — optionally including a new AgentSpawnTool
    /// that shares the SAME guard (so depth is tracked globally).
    fn child_tools(&self, inherit: bool) -> Vec<Arc<dyn Tool>> {
        let mut tools = Vec::new();

        if inherit {
            // Copy parent tools, excluding the spawn tool itself to avoid duplication
            for t in self.parent_tools.iter() {
                if t.name() != "agent" {
                    tools.push(t.clone());
                }
            }
        }

        // Always give the child its own spawn tool — sharing the same guard
        tools.push(Arc::new(AgentSpawnTool {
            model: self.model.clone(),
            parent_tools: self.parent_tools.clone(),
            guard: self.guard.clone(),
            boards: self.boards.clone(),
        }));

        tools
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
        // Depth check
        let _token = match self.guard.try_acquire("agent") {
            Ok(t) => t,
            Err(e) => {
                return Ok(ToolResult {
                    content: e.message,
                    is_error: true,
                    details: None,
                });
            }
        };

        let input: SpawnInput = serde_json::from_value(input).map_err(|e| {
            AgentError::ToolError {
                tool_name: "agent".into(),
                message: format!("invalid input: {}", e),
            }
        })?;

        let system_prompt = if input.system_prompt.is_empty() {
            format!("You are a {} agent. Complete the task given to you.", input.role)
        } else {
            input.system_prompt
        };

        // Build context with board messages if applicable
        let mut task_context = input.task.clone();
        if let Some(board_id) = &input.board {
            let board = self.get_or_create_board(board_id).await;

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

        // Build child agent
        let child_tools = self.child_tools(input.inherit_tools);

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
        hooks.max_iterations = 50;

        let session_id = format!(
            "spawn-{}-{}",
            input.role,
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_millis()
        );

        let agent = Agent::new(self.model.clone(), session_id, &system_prompt, hooks);
        agent.set_tools(child_tools).await;

        // Run with timeout
        let user_msg = AgentMessage::Standard(Message::user(&task_context));
        let mut rx = agent.prompt(vec![user_msg]);
        let mut output = String::new();

        let result = tokio::time::timeout(
            std::time::Duration::from_secs(300),
            async {
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
            },
        )
        .await;

        // Post result to board if applicable
        if let Some(board_id) = &input.board {
            let board = self.get_or_create_board(board_id).await;
            board
                .post(
                    BoardMessage::new(&input.role, &output)
                        .with_kind(MessageKind::Artifact {
                            name: format!("{}-output", input.role),
                        }),
                )
                .await;
        }

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
                Ok(ToolResult {
                    content: format!(
                        "[Agent '{}' timed out after 5 minutes]\n{}",
                        input.role, output
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

/// Create the `agent` spawn tool with the given depth guard.
///
/// The guard is shared across all spawned children, so depth is
/// tracked globally across the entire agent tree.
///
/// ```rust,ignore
/// use alva_types::tool_guard::ToolGuard;
///
/// // App layer decides the limit
/// let guard = ToolGuard::max_depth(3);
/// let tool = create_agent_spawn_tool(model, parent_tools, guard);
/// ```
pub fn create_agent_spawn_tool(
    model: Arc<dyn LanguageModel>,
    parent_tools: Vec<Arc<dyn Tool>>,
    guard: ToolGuard,
) -> Box<dyn Tool> {
    Box::new(AgentSpawnTool::new(model, parent_tools, guard))
}
