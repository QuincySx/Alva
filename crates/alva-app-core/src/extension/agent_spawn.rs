// INPUT:  alva_kernel_abi, alva_kernel_core::run_child, alva_agent_context::scope::blackboard, alva_agent_context::scope::SpawnScopeImpl
// OUTPUT: AgentSpawnTool, create_agent_spawn_tool, SubAgentExtension, ChildRunRecording
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
//!
//! Also exposes the [`ChildRunRecording`] contract — the bus-registered
//! hook point that lets observers (e.g. an eval recorder) record each
//! child agent run as a structured nested record. See the trait's docs
//! for the flow.

use std::sync::Arc;

use async_trait::async_trait;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use alva_kernel_core::middleware::MiddlewareStack;
use alva_kernel_core::run_child::{run_child_agent, ChildAgentParams};
use alva_kernel_abi::agent_session::{AgentSession, ListenableInMemorySession, SessionEvent, SessionEventListener};
use alva_kernel_abi::base::cancel::CancellationToken;
use alva_kernel_abi::base::error::AgentError;
use alva_kernel_abi::scope::{ChildScopeConfig, ScopeError};
use alva_kernel_abi::tool::Tool;
use alva_kernel_abi::tool::execution::{ToolExecutionContext, ToolOutput};

use alva_agent_context::scope::blackboard::{AgentProfile, BoardMessage, MessageKind};
use alva_agent_context::scope::board_registry::BoardRegistry;
use alva_agent_context::scope::SpawnScopeImpl;

// ---------------------------------------------------------------------------
// Child-run recording contract (optional observer hook)
// ---------------------------------------------------------------------------

/// Optional observer hook for capturing sub-agent runs as structured records.
///
/// This trait is the **agent_spawn plugin's** contract with any external
/// observer that wants to record what a child agent did. It is not part
/// of the core agent loop — `AgentSpawnTool` works fine without an
/// implementation registered.
///
/// When a `ChildRunRecording` service is present on the bus,
/// `AgentSpawnTool::execute`:
///
/// 1. Calls [`begin_child_run`](Self::begin_child_run) with the parent
///    tool call's id to get a `MiddlewareStack` to drive the child with.
/// 2. Runs the child agent with that stack installed.
/// 3. Calls [`finalize_child_run`](Self::finalize_child_run) to tell the
///    service the run is done — the service drains its internal recorder
///    into a stored record.
///
/// Whoever records the parent run (typically a parent-side middleware)
/// then calls [`take_child_record`](Self::take_child_record) with the
/// same tool call id to harvest the nested record and attach it wherever
/// it likes (e.g. onto the parent's per-tool record struct).
///
/// The record is passed as `serde_json::Value` deliberately: the concrete
/// record type lives in whichever crate implements the service, so this
/// plugin stays free of any observer-crate dependency.
///
/// Register one on the bus with:
/// ```ignore
/// bus_writer.provide::<dyn ChildRunRecording>(Arc::new(MyImpl::new()));
/// ```
///
/// # Recursion
///
/// If a child agent itself spawns grandchild agents, the same service
/// handles them too — the child run inherits the parent bus
/// (see `ChildAgentParams::bus`), so its own `AgentSpawnTool` finds the
/// same service, calls `begin_child_run` with the grandchild tool call id,
/// and the recording nests automatically.
pub trait ChildRunRecording: Send + Sync {
    /// Start recording a child run keyed by the parent tool call id.
    /// Returns the middleware stack that must be installed on the child
    /// agent run (via `ChildAgentParams::middleware`).
    fn begin_child_run(&self, parent_tool_call_id: &str) -> MiddlewareStack;

    /// Tell the service the child run for `parent_tool_call_id` is done.
    /// The implementation drains its active recorder for that id into a
    /// stored record, ready for [`take_child_record`](Self::take_child_record).
    ///
    /// Calling this twice for the same id is a no-op.
    fn finalize_child_run(&self, parent_tool_call_id: &str);

    /// Consume and return the stored child record as JSON, keyed by the
    /// parent tool call id. Returns `None` if no record was produced or
    /// it was already taken.
    fn take_child_record(&self, parent_tool_call_id: &str) -> Option<serde_json::Value>;
}

// ---------------------------------------------------------------------------
// ForwardToSession listener
// ---------------------------------------------------------------------------

/// `SessionEventListener` that mirrors each event into a target session.
/// Used by `AgentSpawnTool` to forward child events into the parent session
/// with their original emitter preserved.
struct ForwardToSession {
    target: Arc<dyn AgentSession>,
}

#[async_trait]
impl SessionEventListener for ForwardToSession {
    async fn on_event(&self, event: &SessionEvent) {
        self.target.append(event.clone()).await;
    }
}

// ---------------------------------------------------------------------------
// Tool input
// ---------------------------------------------------------------------------

/// Input arguments for the `agent` spawn tool.
///
/// `schemars` derives the JSON Schema from this struct's fields and
/// their doc comments — the tool's `parameters_schema` just calls
/// `schema_for!(SpawnInput)` and post-processes the result to inject
/// the runtime-dependent `tools.items.enum` list (see
/// `AgentSpawnTool::parameters_schema`).
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
struct SpawnInput {
    /// Complete task description for the sub-agent.
    task: String,
    /// Short role name (e.g. 'planner', 'coder', 'reviewer').
    role: String,
    /// System prompt for the sub-agent. If empty, a default is
    /// generated from the role.
    #[serde(default)]
    system_prompt: String,
    /// Tool names to grant to the sub-agent. Pick exactly what the
    /// sub-task needs from the parent's own tool set (the exact valid
    /// names are enumerated at runtime in this field's `items.enum`).
    /// Empty means the sub-agent can only reason and spawn further
    /// sub-agents.
    #[serde(default)]
    tools: Vec<String>,
    /// Shared board ID for multi-agent communication. Agents on the
    /// same board see each other's output.
    #[serde(default)]
    board: Option<String>,
}

// ---------------------------------------------------------------------------
// AgentSpawnTool
// ---------------------------------------------------------------------------

#[derive(alva_kernel_abi::Tool)]
#[tool(
    name = "agent",
    description = "Spawn a sub-agent to handle a specific task. The sub-agent runs independently \
        with its own role and system prompt. Use 'board' to enable communication between \
        multiple agents via a shared workspace. Sub-agents can spawn further sub-agents \
        up to the configured depth limit.",
    input = SpawnInput,
    manages_own_timeout,
)]
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

impl AgentSpawnTool {
    /// Inject the runtime-dependent enum for `tools.items`: the exact
    /// set of tool names the parent agent can hand down. schemars can't
    /// know this at compile time — it's per-spawn.
    ///
    /// Picked up automatically by the `#[derive(Tool)]`-generated
    /// `parameters_schema`: it calls `self.apply_schema_overrides(...)`
    /// unqualified, which Rust's method resolution binds to this
    /// inherent method (winning over `Tool::apply_schema_overrides`'s
    /// trait default).
    fn apply_schema_overrides(&self, schema: &mut Value) {
        let available_tools = self.scope.parent_tool_names();
        if let Some(items) = schema
            .pointer_mut("/properties/tools/items")
            .and_then(Value::as_object_mut)
        {
            items.insert(
                "enum".into(),
                Value::Array(available_tools.into_iter().map(Value::String).collect()),
            );
        }
    }

    /// Core execution, with the input already deserialized. Called by
    /// the `#[derive(Tool)]`-generated `execute` after it parses the
    /// JSON input into `SpawnInput`.
    async fn execute_impl(
        &self,
        input: SpawnInput,
        ctx: &dyn ToolExecutionContext,
    ) -> Result<ToolOutput, AgentError> {
        let system_prompt = if input.system_prompt.is_empty() {
            format!(
                "You are a {} agent. Complete the task given to you.",
                input.role
            )
        } else {
            input.system_prompt
        };

        // Build child scope — spawn_child() enforces the depth limit.
        // Tool whitelisting happens below via `tools_by_names`; the scope
        // itself no longer carries an inherit_tools flag.
        let child_config = ChildScopeConfig::new(&input.role)
            .with_system_prompt(&system_prompt);

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

        // Build child tools = whitelisted parent tools + a freshly
        // built spawn tool bound to child_scope.
        //
        // Recursive spawning is **allowed** — the child agent gets its
        // own `agent` tool and can spawn grandchildren up to
        // `max_depth`. The new instance is pushed below, bound to
        // `child_scope` so its depth starts at child.depth+1.
        //
        // `tools_by_names` drops any `agent` entry from the whitelist
        // so the parent's own spawn tool (bound to parent scope,
        // wrong depth) doesn't end up in the list alongside ours.
        // Without that, dispatch's first-match find would route the
        // child's recursive spawn calls to the parent-scoped instance,
        // creating siblings at parent-depth instead of grandchildren
        // at child-depth — silently bypassing max_depth.
        let mut child_tools = child_scope.tools_by_names(&input.tools);
        child_tools.push(Arc::new(AgentSpawnTool {
            scope: child_scope.clone(),
            board_registry: self.board_registry.clone(),
        }));

        tracing::info!(
            sub_agent_task = %input.task,
            sub_agent_role = %input.role,
            depth = child_scope.depth(),
            parent_scope_id = %self.scope.id(),
            granted_tools = ?input.tools,
            tool_count = child_tools.len(),
            "sub-agent spawned"
        );

        // If a ChildRunRecording service is registered on the bus, install a
        // per-child recorder middleware so the parent-side observer can
        // later attach the resulting record to this tool call. The service
        // is keyed by the parent tool_call_id — both ends of the correlation
        // live on that id.
        let recording = ctx
            .bus()
            .and_then(|b| b.get::<dyn ChildRunRecording>())
            .zip(ctx.tool_call_id())
            .map(|(svc, id)| (svc, id.to_string()));
        let child_middleware = recording
            .as_ref()
            .map(|(svc, id)| svc.begin_child_run(id));

        // Retrieve the raw parent session (bypassing emitter stamping) so we
        // can write start/end markers and forward child events directly.
        let parent_raw: Option<Arc<dyn AgentSession>> = ctx
            .session()
            .map(|s| s.inner());

        // Write subagent_run_start marker into parent session.
        let tool_call_id = ctx.tool_call_id().unwrap_or("").to_string();
        if let Some(ref raw) = parent_raw {
            let mut start = SessionEvent::new_runtime("subagent_run_start");
            start.data = Some(serde_json::json!({ "tool_call_id": tool_call_id.clone() }));
            raw.append(start).await;
        }

        // Create a listenable child session and attach a forwarder to the parent.
        let child_session = Arc::new(ListenableInMemorySession::with_parent(
            self.scope.session_id(),
        ));
        if let Some(ref raw) = parent_raw {
            child_session.subscribe(Arc::new(ForwardToSession {
                target: raw.clone(),
            })).await;
        }

        // Run child agent using the shared helper, supplying the listenable session.
        let result = run_child_agent(ChildAgentParams {
            model: child_scope.model(),
            tools: child_tools,
            system_prompt,
            task: task_context,
            max_iterations: child_scope.max_iterations(),
            timeout: child_scope.timeout(),
            parent_session_id: Some(self.scope.session_id().to_string()),
            cancel: CancellationToken::new(),
            middleware: child_middleware,
            model_config: None,
            context_window: 0,
            workspace: ctx.workspace().map(|p| p.to_path_buf()),
            bus: ctx.bus().cloned(),
            sleeper: None,
            session: Some(child_session as Arc<dyn AgentSession>),
        })
        .await;

        // Write subagent_run_end marker into parent session (always, even on error).
        if let Some(ref raw) = parent_raw {
            let mut end = SessionEvent::new_runtime("subagent_run_end");
            end.data = Some(serde_json::json!({
                "tool_call_id": tool_call_id.clone(),
                "error": result.error.as_deref(),
            }));
            raw.append(end).await;
        }

        // Hand the child record back to the recording service so the
        // parent's recorder can drain it via `take_child_record` in
        // `after_tool_call`.
        if let Some((svc, id)) = &recording {
            svc.finalize_child_run(id);
        }

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

        let root_scope = Arc::new(alva_agent_context::scope::SpawnScopeImpl::root(
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

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use alva_kernel_abi::tool::schema::normalize_llm_tool_schema;

    /// Snapshot-style: print the normalized LLM-facing schema so we
    /// can eyeball it. Run with:
    /// `cargo test -p alva-app-core print_spawn_input_schema -- --nocapture`.
    #[test]
    fn print_spawn_input_schema() {
        let mut schema = serde_json::to_value(schemars::schema_for!(SpawnInput)).unwrap();
        normalize_llm_tool_schema(&mut schema);
        println!("{}", serde_json::to_string_pretty(&schema).unwrap());
    }

    #[test]
    fn spawn_input_schema_shape() {
        let mut schema = serde_json::to_value(schemars::schema_for!(SpawnInput)).unwrap();
        normalize_llm_tool_schema(&mut schema);

        assert_eq!(schema["type"], "object");
        assert!(schema["properties"]["task"].is_object());
        assert!(schema["properties"]["role"].is_object());
        assert!(schema["properties"]["tools"].is_object());
        assert_eq!(schema["properties"]["tools"]["type"], "array");

        // Descriptions survived from doc comments.
        assert!(schema["properties"]["task"]["description"]
            .as_str()
            .map(|s| s.contains("task"))
            .unwrap_or(false));
    }
}
