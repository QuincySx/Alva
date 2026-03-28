// INPUT:  alva_types, alva_agent_core, alva_agent_graph, crate::plugins::blackboard, std::sync::Arc
// OUTPUT: TeamTool, create_team_tool
// POS:    A Tool that lets the LLM dynamically assemble and run a multi-agent team via Graph + Blackboard.

//! Team tool — lets the LLM compose an agent team on the fly.
//!
//! The LLM describes agents and their relationships in the tool input,
//! and the tool internally builds a StateGraph with a shared Blackboard,
//! runs the graph, and returns the consolidated result.
//!
//! # How it works
//!
//! 1. LLM calls `team` tool with a JSON describing agents + task
//! 2. Tool creates a `Blackboard` + one `Agent` per role
//! 3. Wires them into a `StateGraph` based on `depends_on` edges
//! 4. Runs the graph (Pregel BSP) — agents communicate via Blackboard
//! 5. Returns final output to the parent agent

use std::sync::Arc;

use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use alva_agent_core::{Agent, AgentEvent, AgentHooks, AgentMessage, ConvertToLlmFn};
use alva_agent_graph::{StateGraph, END};
use alva_types::base::error::AgentError;
use alva_types::base::message::Message;
use alva_types::model::LanguageModel;
use alva_types::base::cancel::CancellationToken;
use alva_types::tool::{Tool, ToolContext, ToolResult};

use crate::plugins::blackboard::{AgentProfile, Blackboard, BlackboardPlugin, BoardMessage, MessageKind};

// ---------------------------------------------------------------------------
// Tool input schema types
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
struct TeamInput {
    /// The overall task to accomplish.
    task: String,
    /// Agent definitions.
    agents: Vec<AgentDef>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct AgentDef {
    /// Unique agent ID (e.g., "planner", "coder", "reviewer").
    id: String,
    /// What this agent does (natural language).
    role: String,
    /// System prompt for this agent.
    system_prompt: String,
    /// IDs of agents whose output this agent needs.
    #[serde(default)]
    depends_on: Vec<String>,
}

// ---------------------------------------------------------------------------
// Graph state
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
struct TeamState {
    task: String,
    outputs: std::collections::HashMap<String, String>,
    board_snapshot: Vec<String>,
}

// ---------------------------------------------------------------------------
// TeamTool
// ---------------------------------------------------------------------------

/// A tool that lets the LLM dynamically create and run a multi-agent team.
///
/// Has a built-in [`ToolGuard`] depth limiter: if a team is already running
/// (depth >= max), the tool refuses to execute and tells the LLM to handle
/// the task itself.
pub struct TeamTool {
    model: Arc<dyn LanguageModel>,
    guard: alva_types::tool::guard::ToolGuard,
}

impl TeamTool {
    pub fn new(model: Arc<dyn LanguageModel>) -> Self {
        Self {
            model,
            guard: alva_types::tool::guard::ToolGuard::max_depth(1),
        }
    }

    /// Set the maximum nesting depth (default: 1, meaning no nesting).
    pub fn with_max_depth(mut self, max: u32) -> Self {
        self.guard = alva_types::tool::guard::ToolGuard::max_depth(max);
        self
    }
}

#[async_trait]
impl Tool for TeamTool {
    fn name(&self) -> &str {
        "team"
    }

    fn description(&self) -> &str {
        "Assemble and run a team of specialized AI agents that collaborate to complete a complex task. \
         Define agents with roles and dependencies — they communicate via a shared workspace and \
         execute in dependency order."
    }

    fn parameters_schema(&self) -> Value {
        serde_json::json!({
            "type": "object",
            "required": ["task", "agents"],
            "properties": {
                "task": {
                    "type": "string",
                    "description": "The overall task for the team to accomplish"
                },
                "agents": {
                    "type": "array",
                    "description": "Agent definitions. Agents run in dependency order.",
                    "items": {
                        "type": "object",
                        "required": ["id", "role", "system_prompt"],
                        "properties": {
                            "id": {
                                "type": "string",
                                "description": "Unique agent name (e.g. 'planner', 'coder', 'reviewer')"
                            },
                            "role": {
                                "type": "string",
                                "description": "What this agent does"
                            },
                            "system_prompt": {
                                "type": "string",
                                "description": "System prompt for this agent"
                            },
                            "depends_on": {
                                "type": "array",
                                "items": { "type": "string" },
                                "description": "IDs of agents that must run before this one"
                            }
                        }
                    }
                }
            }
        })
    }

    async fn execute(
        &self,
        input: Value,
        cancel: &CancellationToken,
        _ctx: &dyn ToolContext,
    ) -> Result<ToolResult, AgentError> {
        // Depth guard: refuse if already at max nesting
        let _token = match self.guard.try_acquire("team") {
            Ok(token) => token,
            Err(e) => {
                return Ok(ToolResult {
                    content: e.message,
                    is_error: true,
                    details: None,
                });
            }
        };

        let team_input: TeamInput = serde_json::from_value(input).map_err(|e| {
            AgentError::ToolError {
                tool_name: "team".into(),
                message: format!("invalid input: {}", e),
            }
        })?;

        if team_input.agents.is_empty() {
            return Err(AgentError::ToolError {
                tool_name: "team".into(),
                message: "at least one agent is required".into(),
            });
        }

        // Build shared blackboard
        let board = Arc::new(Blackboard::new());

        // Register all agents on the board
        for def in &team_input.agents {
            let provides_to: Vec<String> = team_input
                .agents
                .iter()
                .filter(|other| other.depends_on.contains(&def.id))
                .map(|other| other.id.clone())
                .collect();

            board
                .register(AgentProfile::new(&def.id, &def.role)
                    .depends_on(def.depends_on.clone())
                    .provides_to(provides_to))
                .await;
        }

        // Build graph
        let mut graph = StateGraph::<TeamState>::new();

        // Topological sort: agents with no dependencies first
        let sorted = topological_sort(&team_input.agents)?;

        // Create nodes
        for agent_id in &sorted {
            let def = team_input
                .agents
                .iter()
                .find(|d| d.id == *agent_id)
                .unwrap()
                .clone();

            let model = self.model.clone();
            let board_clone = board.clone();
            let task = team_input.task.clone();

            graph.add_node(agent_id, move |mut state: TeamState| {
                let def = def.clone();
                let model = model.clone();
                let board = board_clone.clone();
                let task = task.clone();

                Box::pin(async move {
                    let output = run_single_agent(&def, &model, &board, &task, &state).await;
                    state.outputs.insert(def.id.clone(), output.clone());

                    // Post result to blackboard
                    let provides_to: Vec<String> = state
                        .outputs
                        .keys()
                        .filter(|k| *k != &def.id)
                        .cloned()
                        .collect();

                    let mut msg = BoardMessage::new(&def.id, &output)
                        .with_kind(MessageKind::Artifact {
                            name: format!("{}-output", def.id),
                        });
                    // Don't error on this — provides_to may not be accurate here
                    let _ = msg;
                    board.post(
                        BoardMessage::new(&def.id, &output)
                            .with_kind(MessageKind::Artifact {
                                name: format!("{}-output", def.id),
                            }),
                    ).await;

                    state
                })
            });
        }

        // Wire edges based on dependency order
        if sorted.len() == 1 {
            graph.set_entry_point(&sorted[0]);
            graph.add_edge(&sorted[0], END);
        } else {
            graph.set_entry_point(&sorted[0]);
            for i in 0..sorted.len() - 1 {
                graph.add_edge(&sorted[i], &sorted[i + 1]);
            }
            graph.add_edge(&sorted[sorted.len() - 1], END);
        }

        let compiled = graph.compile().map_err(|e| AgentError::ToolError {
            tool_name: "team".into(),
            message: format!("graph compile error: {}", e),
        })?;

        // Run with timeout
        let initial_state = TeamState {
            task: team_input.task.clone(),
            outputs: std::collections::HashMap::new(),
            board_snapshot: Vec::new(),
        };

        let result = tokio::time::timeout(
            std::time::Duration::from_secs(600),
            compiled.invoke(initial_state),
        )
        .await;

        match result {
            Ok(Ok(final_state)) => {
                // Build consolidated output
                let mut output = String::new();
                for agent_id in &sorted {
                    if let Some(agent_output) = final_state.outputs.get(agent_id) {
                        output.push_str(&format!("## {} ({})\n\n", agent_id,
                            team_input.agents.iter().find(|d| d.id == *agent_id)
                                .map(|d| d.role.as_str()).unwrap_or("")));
                        output.push_str(agent_output);
                        output.push_str("\n\n");
                    }
                }
                Ok(ToolResult {
                    content: output,
                    is_error: false,
                    details: None,
                })
            }
            Ok(Err(e)) => Err(e),
            Err(_) => Ok(ToolResult {
                content: "Team execution timed out after 10 minutes.".into(),
                is_error: true,
                details: None,
            }),
        }
    }
}

// ---------------------------------------------------------------------------
// Run a single agent within the team
// ---------------------------------------------------------------------------

async fn run_single_agent(
    def: &AgentDef,
    model: &Arc<dyn LanguageModel>,
    board: &Arc<Blackboard>,
    task: &str,
    state: &TeamState,
) -> String {
    // Build context from dependencies' outputs + blackboard
    let mut context = format!("## Task\n{}\n\n", task);

    for dep_id in &def.depends_on {
        if let Some(dep_output) = state.outputs.get(dep_id) {
            context.push_str(&format!("## Output from {}\n{}\n\n", dep_id, dep_output));
        }
    }

    // Include recent board messages
    let (board_log, _) = board.render_chat_log(20).await;
    if !board_log.is_empty() {
        context.push_str(&format!("## Team Chat\n{}\n\n", board_log));
    }

    context.push_str("Based on the above context, complete your part of the task.");

    // Create a lightweight agent (no tools — team agents focus on reasoning)
    let convert_fn: ConvertToLlmFn = Arc::new(|ctx| {
        ctx.messages
            .iter()
            .filter_map(|m| match m {
                AgentMessage::Standard(msg) => Some(msg.clone()),
                _ => None,
            })
            .collect()
    });

    let hooks = AgentHooks::new(convert_fn);
    let session_id = format!("team-{}-{}", def.id, uuid::Uuid::new_v4());
    let agent = Agent::new(model.clone(), session_id, &def.system_prompt, hooks);

    let user_msg = AgentMessage::Standard(Message::user(&context));
    let mut rx = agent.prompt(vec![user_msg]);

    let mut output = String::new();
    while let Some(event) = rx.recv().await {
        match event {
            AgentEvent::MessageEnd { message } => {
                if let AgentMessage::Standard(msg) = &message {
                    output.push_str(&msg.text_content());
                }
            }
            AgentEvent::AgentEnd { .. } => break,
            _ => {}
        }
    }

    output
}

// ---------------------------------------------------------------------------
// Topological sort
// ---------------------------------------------------------------------------

fn topological_sort(agents: &[AgentDef]) -> Result<Vec<String>, AgentError> {
    let ids: Vec<&str> = agents.iter().map(|a| a.id.as_str()).collect();
    let mut sorted = Vec::new();
    let mut visited = std::collections::HashSet::new();
    let mut visiting = std::collections::HashSet::new();

    fn visit<'a>(
        id: &'a str,
        agents: &'a [AgentDef],
        visited: &mut std::collections::HashSet<&'a str>,
        visiting: &mut std::collections::HashSet<&'a str>,
        sorted: &mut Vec<String>,
    ) -> Result<(), AgentError> {
        if visited.contains(id) {
            return Ok(());
        }
        if visiting.contains(id) {
            return Err(AgentError::ConfigError(format!(
                "circular dependency involving '{}'",
                id
            )));
        }

        visiting.insert(id);

        if let Some(def) = agents.iter().find(|a| a.id == id) {
            for dep in &def.depends_on {
                visit(dep, agents, visited, visiting, sorted)?;
            }
        }

        visiting.remove(id);
        visited.insert(id);
        sorted.push(id.to_string());
        Ok(())
    }

    for id in &ids {
        visit(id, agents, &mut visited, &mut visiting, &mut sorted)?;
    }

    Ok(sorted)
}

// ---------------------------------------------------------------------------
// Factory function
// ---------------------------------------------------------------------------

/// Create a `team` tool that the LLM can use to assemble agent teams on the fly.
pub fn create_team_tool(model: Arc<dyn LanguageModel>) -> Box<dyn Tool> {
    Box::new(TeamTool::new(model))
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_agents() -> Vec<AgentDef> {
        vec![
            AgentDef {
                id: "planner".into(),
                role: "planning".into(),
                system_prompt: "You plan.".into(),
                depends_on: vec![],
            },
            AgentDef {
                id: "coder".into(),
                role: "coding".into(),
                system_prompt: "You code.".into(),
                depends_on: vec!["planner".into()],
            },
            AgentDef {
                id: "reviewer".into(),
                role: "review".into(),
                system_prompt: "You review.".into(),
                depends_on: vec!["coder".into()],
            },
        ]
    }

    #[test]
    fn topological_sort_linear() {
        let agents = sample_agents();
        let sorted = topological_sort(&agents).unwrap();
        assert_eq!(sorted, vec!["planner", "coder", "reviewer"]);
    }

    #[test]
    fn topological_sort_no_deps() {
        let agents = vec![
            AgentDef {
                id: "a".into(),
                role: "".into(),
                system_prompt: "".into(),
                depends_on: vec![],
            },
            AgentDef {
                id: "b".into(),
                role: "".into(),
                system_prompt: "".into(),
                depends_on: vec![],
            },
        ];
        let sorted = topological_sort(&agents).unwrap();
        assert_eq!(sorted.len(), 2);
    }

    #[test]
    fn topological_sort_detects_cycle() {
        let agents = vec![
            AgentDef {
                id: "a".into(),
                role: "".into(),
                system_prompt: "".into(),
                depends_on: vec!["b".into()],
            },
            AgentDef {
                id: "b".into(),
                role: "".into(),
                system_prompt: "".into(),
                depends_on: vec!["a".into()],
            },
        ];
        let result = topological_sort(&agents);
        assert!(result.is_err());
    }
}
