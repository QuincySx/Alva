// INPUT:  alva_kernel_abi (including scope::spawn + ProviderRegistry), alva_kernel_core::run_child, alva_agent_context::scope::SpawnScopeImpl, alva_agent_context::default_context_system, alva_agent_context::ContextHooksChain
// OUTPUT: AgentSpawnTool, create_agent_spawn_tool, SubAgentPlugin
// POS:    AI-driven sub-agent spawning — dynamic roles, optional per-spawn model, pluggable SpawnCommunication capabilities (blackboard etc).

//! Agent spawn plugin — the AI primitive for creating sub-agents.
//!
//! The LLM decides when to spawn, what role to give, optionally which
//! model to run the child with, and which communication capabilities (if
//! any) to attach — all via the `agent` tool's `SpawnInput`.
//!
//! Orchestration lives in the LLM's reasoning, not in code-level graph
//! definitions.
//!
//! Sub-agent events are recorded into the parent's session in real time
//! via a `ListenableInMemorySession` + a `ForwardToSession` listener.
//! Projection consumers (eval, debug) delimit each sub-run by matching
//! `subagent_run_start` / `subagent_run_end` marker events tagged with
//! the parent `tool_call_id`.

use std::sync::Arc;

use async_trait::async_trait;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use alva_kernel_abi::agent_session::{
    AgentSession, ListenableInMemorySession, SessionEvent, SessionEventListener,
};
use alva_kernel_abi::base::error::AgentError;
use alva_kernel_abi::context::{ContextHooks, ContextSystem};
use alva_kernel_abi::model::LanguageModel;
use alva_kernel_abi::scope::{ChildScopeConfig, ScopeError};
use alva_kernel_abi::tool::execution::{ToolExecutionContext, ToolOutput};
use alva_kernel_abi::tool::Tool;
use alva_kernel_abi::{
    OnChildComplete, ProviderRegistry, SpawnCommContext, SpawnCommHandle,
    SpawnCommunicationRegistry, SpawnResult,
};
use alva_kernel_core::run_child::{run_child_agent, ChildAgentParams};

use alva_agent_context::scope::SpawnScopeImpl;
use alva_agent_context::{default_context_system, ContextHooksChain};

use crate::extension::skills::agent_template_service::AgentTemplateService;
use crate::extension::skills::skill_domain::agent_template::{AgentModelConfig, AgentTemplate};

// ---------------------------------------------------------------------------
// AgentTemplateRegistry — named, predefined sub-agent profiles
// ---------------------------------------------------------------------------

/// Registry of named [`AgentTemplate`]s the spawn tool can instantiate by
/// name (the `agent_type` input), mirroring kimi-code's `subagent_type` →
/// profile model. Looked up off the bus (like `SpawnCommunicationRegistry`)
/// so it stays optional: when absent, spawning falls back to the dynamic
/// `role` + `tools` path unchanged.
pub trait AgentTemplateRegistry: Send + Sync {
    /// Fetch a template by its `name`.
    fn get(&self, name: &str) -> Option<Arc<AgentTemplate>>;
    /// All registered templates (used to build the `agent_type` enum and to
    /// surface each template's `description`/when-to-use to the parent).
    fn list(&self) -> Vec<Arc<AgentTemplate>>;
}

/// Simple in-memory [`AgentTemplateRegistry`]. The source of templates
/// (config files, defaults) is the caller's concern — this just holds them.
pub struct InMemoryAgentTemplateRegistry {
    templates: Vec<Arc<AgentTemplate>>,
}

impl InMemoryAgentTemplateRegistry {
    pub fn new(templates: Vec<AgentTemplate>) -> Self {
        Self {
            templates: templates.into_iter().map(Arc::new).collect(),
        }
    }
    pub fn is_empty(&self) -> bool {
        self.templates.is_empty()
    }
}

impl AgentTemplateRegistry for InMemoryAgentTemplateRegistry {
    fn get(&self, name: &str) -> Option<Arc<AgentTemplate>> {
        self.templates.iter().find(|t| t.name == name).cloned()
    }
    fn list(&self) -> Vec<Arc<AgentTemplate>> {
        self.templates.clone()
    }
}

/// Build a dedicated [`LanguageModel`] for a sub-agent from its template's
/// [`AgentModelConfig`] (own model id, endpoint, provider kind, and key).
/// The key is taken from `api_key`, else read from `api_key_env`, else empty.
fn build_template_model(cfg: &AgentModelConfig) -> Arc<dyn LanguageModel> {
    use alva_llm_provider::{
        AnthropicProvider, GeminiProvider, OpenAIChatProvider, OpenAIResponsesProvider,
        ProviderConfig,
    };

    let api_key = cfg
        .api_key
        .clone()
        .filter(|s| !s.is_empty())
        .or_else(|| {
            cfg.api_key_env
                .as_ref()
                .and_then(|e| std::env::var(e).ok())
                .filter(|s| !s.is_empty())
        })
        .unwrap_or_default();

    let provider_config = ProviderConfig {
        api_key,
        model: cfg.model.clone(),
        base_url: cfg.base_url.clone().unwrap_or_default(),
        max_tokens: cfg.max_tokens.unwrap_or(4096),
        custom_headers: std::collections::HashMap::new(),
        kind: cfg.provider_kind.clone(),
    };

    match cfg.provider_kind.as_deref() {
        Some("anthropic") => Arc::new(AnthropicProvider::new(provider_config)),
        Some("openai-responses") => Arc::new(OpenAIResponsesProvider::new(provider_config)),
        Some("gemini") => Arc::new(GeminiProvider::new(provider_config)),
        // None / "openai-chat" / unknown → OpenAI Chat (broadest compat path).
        _ => Arc::new(OpenAIChatProvider::new(provider_config)),
    }
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

/// One communication capability to attach to a spawned sub-agent.
///
/// `kind` is runtime-registered on the bus via
/// `SpawnCommunicationRegistry`; the shape of `config` depends on the
/// kind (see each `SpawnCommunication::config_schema`).
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
struct CommSpec {
    /// Communication capability kind (e.g. "blackboard"). Must match a
    /// capability registered with the spawn comm registry.
    kind: String,
    /// Kind-specific config payload. See the capability's schema.
    #[serde(default)]
    config: Value,
}

/// Input arguments for the `agent` spawn tool.
///
/// `schemars` derives the JSON Schema from this struct's fields and
/// their doc comments — the tool's `parameters_schema` just calls
/// `schema_for!(SpawnInput)` and post-processes the result to inject
/// the runtime-dependent `tools.items.enum` list and the currently
/// registered `comms.items.properties.kind.enum` values (see
/// `AgentSpawnTool::apply_schema_overrides`).
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
struct SpawnInput {
    /// Complete task description for the sub-agent.
    task: String,
    /// Optional predefined agent template to spawn (see the `agent_type`
    /// enum + per-template "when to use" notes in this field's
    /// description). When set, the template supplies the system prompt and
    /// tool set, and `role`/`system_prompt`/`tools` below are only used as
    /// overrides/fallbacks. Leave unset to define the sub-agent ad-hoc.
    #[serde(default)]
    agent_type: Option<String>,
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
    /// Provider/model spec for this sub-agent (e.g.
    /// `anthropic/claude-haiku-4.5`). Leave empty (or unset) to inherit
    /// the parent's model. If no `ProviderRegistry` is registered on the
    /// bus, this field is ignored and the parent model is inherited.
    #[serde(default)]
    model: Option<String>,
    /// Communication capabilities to attach to this sub-agent. Each kind
    /// is a runtime-registered plugin (e.g. "blackboard"); config format
    /// depends on kind.
    #[serde(default)]
    comms: Vec<CommSpec>,
}

// ---------------------------------------------------------------------------
// AgentSpawnTool
// ---------------------------------------------------------------------------

#[derive(alva_kernel_abi::Tool)]
#[tool(
    name = "agent",
    description = "Spawn a sub-agent to handle a specific task. The sub-agent runs independently \
        with its own role and system prompt. Pick a subset of the parent's tools via 'tools'. \
        Optionally pick a different model via 'model' when a ProviderRegistry is available; \
        otherwise the parent model is inherited. \
        Attach communication capabilities (e.g. shared blackboard) via 'comms'. \
        Sub-agents can spawn further sub-agents up to the configured depth limit.",
    input = SpawnInput,
    manages_own_timeout,
    // Orchestrator: takes no scheduler lock. The spawned sub-agent runs inline
    // on this same task and executes its own tools (which take their own
    // locks). Holding the global read lock here would deadlock a child's
    // `serial-global` tool (e.g. execute_shell) requesting the global write.
    execution_mode = "coordinator",
)]
pub struct AgentSpawnTool {
    scope: Arc<SpawnScopeImpl>,
}

impl AgentSpawnTool {
    pub fn new(scope: Arc<SpawnScopeImpl>) -> Self {
        Self { scope }
    }
}

impl AgentSpawnTool {
    /// Inject runtime-dependent enums:
    /// - `tools.items.enum`: exact set of tool names the parent can hand
    ///   down (per-spawn, changes across agents). Sourced from the scope
    ///   this tool was built against — independent of the bus.
    /// - `comms.items.properties.kind.enum`: currently registered
    ///   `SpawnCommunication` kinds, read from the bus via
    ///   [`ToolSchemaContext::bus`]. Omitted entirely when no bus is wired
    ///   or no capabilities are registered (kind stays a free-form
    ///   string, and the executor still validates against the live
    ///   registry at call time).
    fn inject_dynamic_enums(
        &self,
        schema: &mut Value,
        comm_kinds: &[String],
        templates: &[Arc<AgentTemplate>],
    ) {
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

        // `agent_type` enum + per-template "when to use" guidance, so the
        // parent picks a predefined profile the way kimi-code surfaces
        // `subagent_type` choices. Omitted when no templates are registered
        // (the field stays an unconstrained optional string).
        if !templates.is_empty() {
            if let Some(at) = schema
                .pointer_mut("/properties/agent_type")
                .and_then(Value::as_object_mut)
            {
                at.insert(
                    "enum".into(),
                    Value::Array(
                        templates
                            .iter()
                            .map(|t| Value::String(t.name.clone()))
                            .collect(),
                    ),
                );
                let mut lines = vec![
                    "Predefined agent template to spawn. Available types (use when):"
                        .to_string(),
                ];
                for t in templates {
                    lines.push(format!("- {}: {}", t.name, t.description));
                }
                at.insert("description".into(), Value::String(lines.join("\n")));
            }
        }

        if !comm_kinds.is_empty() {
            if let Some(kind_schema) = schema
                .pointer_mut("/properties/comms/items/properties/kind")
                .and_then(Value::as_object_mut)
            {
                kind_schema.insert(
                    "enum".into(),
                    Value::Array(comm_kinds.iter().cloned().map(Value::String).collect()),
                );
            }
        }
    }

    /// Context-free fallback path. Hit when the provider generates a
    /// tool schema without a live [`ToolSchemaContext`] (e.g. an offline
    /// dump, a test that calls `parameters_schema()` directly). Only
    /// the scope-dependent `tools.items.enum` can be populated; the
    /// bus-dependent comm-kinds enum is skipped here and injected by
    /// the ctx-aware path instead.
    ///
    /// Picked up automatically by the `#[derive(Tool)]`-generated
    /// `parameters_schema`: it calls `self.apply_schema_overrides(...)`
    /// unqualified, which Rust's method resolution binds to this
    /// inherent method (winning over `Tool::apply_schema_overrides`'s
    /// trait default).
    fn apply_schema_overrides(&self, schema: &mut Value) {
        self.inject_dynamic_enums(schema, &[], &[]);
    }

    /// Context-aware path — invoked by the `#[derive(Tool)]`-generated
    /// `parameters_schema_with`. Same inherent-wins-over-trait pattern
    /// as `apply_schema_overrides`: defining this inherent method
    /// overrides the trait default that merely forwards to the
    /// context-free variant.
    ///
    /// Reads `SpawnCommunicationRegistry` off
    /// [`ToolSchemaContext::bus`] (if wired) and injects the full set of
    /// registered kind ids as a JSON-Schema `enum` on
    /// `comms.items.properties.kind`, so the LLM gets the precise live
    /// set of choices instead of an unconstrained string.
    fn apply_schema_overrides_with(
        &self,
        schema: &mut Value,
        ctx: &alva_kernel_abi::tool::schema::ToolSchemaContext,
    ) {
        let comm_kinds: Vec<String> = ctx
            .bus
            .and_then(|b| b.get::<dyn SpawnCommunicationRegistry>())
            .map(|reg| reg.list().iter().map(|c| c.kind().to_string()).collect())
            .unwrap_or_default();
        let templates: Vec<Arc<AgentTemplate>> = ctx
            .bus
            .and_then(|b| b.get::<dyn AgentTemplateRegistry>())
            .map(|reg| reg.list())
            .unwrap_or_default();
        self.inject_dynamic_enums(schema, &comm_kinds, &templates);
    }

    /// Core execution, with the input already deserialized. Called by
    /// the `#[derive(Tool)]`-generated `execute` after it parses the
    /// JSON input into `SpawnInput`.
    async fn execute_impl(
        &self,
        input: SpawnInput,
        ctx: &dyn ToolExecutionContext,
    ) -> Result<ToolOutput, AgentError> {
        // ── Resolve a predefined template (kimi-code `subagent_type`) ────
        // When `agent_type` is set, the named AgentTemplate supplies the
        // system prompt and tool whitelist; explicit `role`/`system_prompt`/
        // `tools` still win as overrides. NOTE: skills/MCP instantiation via
        // AgentTemplateService is a follow-up slice — this wires the prompt +
        // tool set, which is the core of the connection.
        let template = match input.agent_type.as_deref() {
            Some(name) if !name.is_empty() => {
                let Some(reg) = ctx.bus().and_then(|b| b.get::<dyn AgentTemplateRegistry>()) else {
                    return Ok(ToolOutput::error(format!(
                        "agent_type '{name}' requested but no AgentTemplateRegistry is configured."
                    )));
                };
                match reg.get(name) {
                    Some(t) => Some(t),
                    None => {
                        let available: Vec<String> =
                            reg.list().iter().map(|t| t.name.clone()).collect();
                        return Ok(ToolOutput::error(format!(
                            "unknown agent_type '{name}'; available: {available:?}"
                        )));
                    }
                }
            }
            _ => None,
        };

        // role: explicit non-empty role wins; else the template's name; else
        // whatever role string was passed (possibly empty).
        let role = match &template {
            Some(t) if input.role.is_empty() => t.name.clone(),
            _ => input.role.clone(),
        };

        // Instantiate the template via AgentTemplateService when it's on the
        // bus: this is what makes the template's `skills` take effect — the
        // returned `system_prompt` has the skill-injection block appended, and
        // `allowed_tools` reflects the template's MCP/tool whitelist. Falls
        // back to the template's raw fields when the service is absent (skills
        // component off) or instantiation fails.
        let instance = match &template {
            Some(t) => match ctx.bus().and_then(|b| b.get::<AgentTemplateService>()) {
                Some(svc) => match svc.instantiate(t).await {
                    Ok(inst) => Some(inst),
                    Err(e) => {
                        tracing::warn!(
                            agent_type = %t.name,
                            error = %e,
                            "template instantiation failed; using raw template fields"
                        );
                        None
                    }
                },
                None => None,
            },
            None => None,
        };

        let system_prompt = if !input.system_prompt.is_empty() {
            input.system_prompt.clone()
        } else if let Some(inst) = &instance {
            inst.system_prompt.clone()
        } else if let Some(t) = &template {
            t.system_prompt_base.clone()
        } else {
            format!("You are a {role} agent. Complete the task given to you.")
        };

        // tool names: explicit `tools` wins; else the instantiated whitelist
        // (template tools + any MCP tools); else the template's raw
        // allowed_tools (None = inherit all parent tools); else empty.
        let tool_names: Vec<String> = if !input.tools.is_empty() {
            input.tools.clone()
        } else if let Some(inst) = &instance {
            inst.allowed_tools
                .clone()
                .unwrap_or_else(|| self.scope.parent_tool_names())
        } else if let Some(t) = &template {
            t.allowed_tools
                .clone()
                .unwrap_or_else(|| self.scope.parent_tool_names())
        } else {
            Vec::new()
        };

        // Build child scope — spawn_child() enforces the depth limit.
        let child_config = ChildScopeConfig::new(&role).with_system_prompt(&system_prompt);

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

        // ── Model resolution ────────────────────────────────────────────
        // Priority: explicit per-spawn `model` (runtime, via ProviderRegistry)
        // > the template's own `[agent.model]` (config: own model + endpoint)
        // > the parent agent's model (inherited).
        let child_model: Arc<dyn LanguageModel> = match input.model.as_deref() {
            Some(spec) if !spec.is_empty() => {
                match ctx.bus().and_then(|b| b.get::<ProviderRegistry>()) {
                    Some(registry) => alva_host_native::model(spec, &registry).map_err(|e| {
                        AgentError::ToolError {
                            tool_name: "agent".into(),
                            message: format!("resolve model '{spec}': {e}"),
                        }
                    })?,
                    None => child_scope.model(),
                }
            }
            _ => match template.as_ref().and_then(|t| t.model.as_ref()) {
                Some(mc) if !mc.model.is_empty() => build_template_model(mc),
                _ => child_scope.model(),
            },
        };

        // ── Communication capabilities ──────────────────────────────────
        let registry = ctx
            .bus()
            .and_then(|b| b.get::<dyn SpawnCommunicationRegistry>());

        let mut comm_handles: Vec<SpawnCommHandle> = Vec::new();
        let mut child_hooks: Vec<Arc<dyn ContextHooks>> = Vec::new();

        for spec in &input.comms {
            let Some(ref reg) = registry else {
                return Ok(ToolOutput::error(format!(
                    "comms '{}' requested but no SpawnCommunicationRegistry on bus",
                    spec.kind
                )));
            };
            let Some(ch) = reg.get(&spec.kind) else {
                let available: Vec<String> =
                    reg.list().iter().map(|c| c.kind().to_string()).collect();
                return Ok(ToolOutput::error(format!(
                    "unknown communication kind '{}'; available: {:?}",
                    spec.kind, available
                )));
            };

            let comm_ctx = SpawnCommContext {
                parent_scope_id: self.scope.id().as_str(),
                parent_session_id: self.scope.session_id(),
                child_scope_id: child_scope.id().as_str(),
                child_session_id: child_scope.session_id(),
                role: &role,
                bus: ctx.bus(),
            };

            match ch.attach(&comm_ctx, spec.config.clone()).await {
                Ok(handle) => {
                    child_hooks.extend(handle.hooks.clone());
                    comm_handles.push(handle);
                }
                Err(e) => {
                    return Ok(ToolOutput::error(format!(
                        "attach comm '{}' failed: {}",
                        spec.kind, e
                    )));
                }
            }
        }

        // ── Child tool list ─────────────────────────────────────────────
        //
        // `tools_by_names` drops any `agent` entry from the whitelist so
        // the parent's own spawn tool (bound to parent scope, wrong depth)
        // doesn't end up in the list alongside ours. Without that,
        // dispatch's first-match find would route the child's recursive
        // spawn calls to the parent-scoped instance, creating siblings at
        // parent-depth instead of grandchildren at child-depth — silently
        // bypassing max_depth.
        let mut child_tools = child_scope.tools_by_names(&tool_names);
        child_tools.push(Arc::new(AgentSpawnTool {
            scope: child_scope.clone(),
        }));

        tracing::info!(
            sub_agent_task = %input.task,
            sub_agent_role = %role,
            agent_type = ?input.agent_type,
            depth = child_scope.depth(),
            parent_scope_id = %self.scope.id(),
            granted_tools = ?tool_names,
            tool_count = child_tools.len(),
            model_override = ?input.model,
            comm_kinds = ?input.comms.iter().map(|c| c.kind.as_str()).collect::<Vec<_>>(),
            "sub-agent spawned"
        );

        // Retrieve the raw parent session (bypassing emitter stamping) so we
        // can write start/end markers and forward child events directly.
        let parent_raw: Option<Arc<dyn AgentSession>> = ctx.session().map(|s| s.inner());

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
            child_session
                .subscribe(Arc::new(ForwardToSession {
                    target: raw.clone(),
                }))
                .await;
        }

        // ── ContextSystem for hooks (only when any comm attached hooks) ─
        let context_system: Option<Arc<ContextSystem>> = if child_hooks.is_empty() {
            None
        } else {
            let mut cs = default_context_system();
            let chain: Arc<dyn ContextHooks> = Arc::new(ContextHooksChain::new(child_hooks));
            cs.set_hooks(chain);
            Some(Arc::new(cs))
        };

        // Run child agent using the shared helper, supplying the listenable session.
        let result = run_child_agent(ChildAgentParams {
            model: child_model,
            tools: child_tools,
            system_prompt: if system_prompt.is_empty() {
                Vec::new()
            } else {
                vec![system_prompt]
            },
            task: input.task.clone(),
            max_iterations: child_scope.max_iterations(),
            timeout: child_scope.timeout(),
            parent_session_id: Some(self.scope.session_id().to_string()),
            // Derive the child's cancel token from the parent's so a cancel
            // on the parent run reaches the sub-agent. A fresh token here
            // would leave the child disconnected — a child blocked in a
            // cooperative-cancel tool (e.g. `sleep`) would never stop. The
            // token clones share one watch channel (latching), so this also
            // covers a cancel that races ahead of the child spawning.
            cancel: ctx.cancel_token().clone(),
            model_config: None,
            context_window: 0,
            workspace: ctx.workspace().map(|p| p.to_path_buf()),
            bus: ctx.bus().cloned(),
            sleeper: None,
            session: Some(child_session as Arc<dyn AgentSession>),
            context_system,
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

        tracing::info!(
            sub_agent_role = %role,
            depth = child_scope.depth(),
            parent_scope_id = %self.scope.id(),
            output_len = result.text.len(),
            success = !result.is_error,
            error = result.error.as_deref().unwrap_or(""),
            "sub-agent completed"
        );

        // Fire comm on_complete callbacks. They get a lightweight
        // `SpawnResult` (abi-layer view, decoupled from ChildAgentOutput).
        if !comm_handles.is_empty() {
            let spawn_result = SpawnResult {
                text: result.text.clone(),
                is_error: result.is_error,
                error: result.error.clone(),
            };
            for handle in comm_handles {
                if let Some(cb) = handle.on_complete {
                    let _: Arc<dyn OnChildComplete> = cb.clone();
                    cb.call(&spawn_result).await;
                }
            }
        }

        if result.is_error {
            Ok(ToolOutput::error(format!(
                "[Agent '{}' error: {}]\n{}",
                role,
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

use crate::extension::{LateContext, Plugin, Registrar};

/// Sub-agent spawning via the `agent` tool.
///
/// Uses `finalize()` because it needs the final tool list and model to
/// construct the `SpawnScopeImpl` root scope.
pub struct SubAgentPlugin {
    max_depth: u32,
    /// Predefined sub-agent templates exposed via the `agent_type` input.
    /// Empty (the default) keeps the dynamic-only spawn behavior.
    templates: Vec<AgentTemplate>,
}

impl SubAgentPlugin {
    pub fn new(max_depth: u32) -> Self {
        Self {
            max_depth,
            templates: Vec::new(),
        }
    }

    /// Attach predefined [`AgentTemplate`]s so the parent can spawn them by
    /// name via `agent_type`. The templates are published on the bus as an
    /// [`AgentTemplateRegistry`] at registration time.
    pub fn with_templates(mut self, templates: Vec<AgentTemplate>) -> Self {
        self.templates = templates;
        self
    }
}

#[async_trait]
impl Plugin for SubAgentPlugin {
    fn name(&self) -> &str {
        "sub-agents"
    }
    fn description(&self) -> &str {
        "Sub-agent spawning via the agent tool"
    }

    // Publish the template registry (if any) so the spawn tool can resolve
    // `agent_type` off the bus. The spawn tool itself is wired late.
    async fn register(&self, r: &Registrar) {
        if !self.templates.is_empty() {
            let registry: Arc<dyn AgentTemplateRegistry> =
                Arc::new(InMemoryAgentTemplateRegistry::new(self.templates.clone()));
            r.provide::<dyn AgentTemplateRegistry>(registry);
        }
    }

    async fn finalize(&self, ctx: &LateContext) -> Vec<Arc<dyn Tool>> {
        // Build a clean tool list without any placeholder agent tool
        let tools_without_agent: Vec<Arc<dyn Tool>> = ctx
            .tools
            .iter()
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
    use alva_kernel_abi::base::cancel::CancellationToken;
    use alva_kernel_abi::tool::schema::{normalize_llm_tool_schema, ToolSchemaContext};
    use alva_kernel_abi::{Bus, BusHandle};
    use alva_test::fixtures::make_assistant_message;
    use alva_test::mock_provider::MockLanguageModel;
    use std::time::Duration;

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

        // New fields survived the derive.
        assert!(schema["properties"]["model"].is_object());
        assert!(schema["properties"]["comms"].is_object());
        assert_eq!(schema["properties"]["comms"]["type"], "array");

        // Descriptions survived from doc comments.
        assert!(schema["properties"]["task"]["description"]
            .as_str()
            .map(|s| s.contains("task"))
            .unwrap_or(false));
    }

    // A minimal stub model so we can build a root `SpawnScopeImpl` without
    // pulling in a real LLM provider.
    struct StubModel;
    #[async_trait]
    impl LanguageModel for StubModel {
        async fn complete(
            &self,
            _messages: &[alva_kernel_abi::Message],
            _tools: &[&dyn Tool],
            _config: &alva_kernel_abi::ModelConfig,
        ) -> Result<alva_kernel_abi::CompletionResponse, AgentError> {
            Err(AgentError::LlmError("stub".into()))
        }
        fn stream(
            &self,
            _messages: &[alva_kernel_abi::Message],
            _tools: &[&dyn Tool],
            _config: &alva_kernel_abi::ModelConfig,
        ) -> std::pin::Pin<Box<dyn futures::Stream<Item = alva_kernel_abi::StreamEvent> + Send>>
        {
            Box::pin(futures::stream::empty())
        }
        fn model_id(&self) -> &str {
            "stub"
        }
    }

    // Minimal `SpawnCommunication` implementations so the registry has
    // something to list when we fetch it off the bus.
    struct DummyComm {
        kind: String,
    }
    #[async_trait]
    impl alva_kernel_abi::SpawnCommunication for DummyComm {
        fn kind(&self) -> &str {
            &self.kind
        }
        fn description(&self) -> &str {
            "dummy comm for tests"
        }
        async fn attach(
            &self,
            _ctx: &alva_kernel_abi::SpawnCommContext<'_>,
            _config: serde_json::Value,
        ) -> Result<alva_kernel_abi::SpawnCommHandle, alva_kernel_abi::SpawnCommError> {
            Ok(alva_kernel_abi::SpawnCommHandle::empty())
        }
    }

    // In-memory `SpawnCommunicationRegistry` — the default
    // implementation lives in app-core extension wiring; we only need
    // list/register/get semantics here.
    struct TestRegistry {
        inner: std::sync::Mutex<Vec<Arc<dyn alva_kernel_abi::SpawnCommunication>>>,
    }
    impl TestRegistry {
        fn new() -> Self {
            Self {
                inner: std::sync::Mutex::new(Vec::new()),
            }
        }
    }
    impl SpawnCommunicationRegistry for TestRegistry {
        fn register(&self, ch: Arc<dyn alva_kernel_abi::SpawnCommunication>) {
            self.inner.lock().unwrap().push(ch);
        }
        fn get(&self, kind: &str) -> Option<Arc<dyn alva_kernel_abi::SpawnCommunication>> {
            self.inner
                .lock()
                .unwrap()
                .iter()
                .find(|c| c.kind() == kind)
                .cloned()
        }
        fn list(&self) -> Vec<Arc<dyn alva_kernel_abi::SpawnCommunication>> {
            self.inner.lock().unwrap().clone()
        }
    }

    fn build_spawn_tool() -> AgentSpawnTool {
        build_spawn_tool_with_model(Arc::new(StubModel))
    }

    fn build_spawn_tool_with_model(model: Arc<dyn LanguageModel>) -> AgentSpawnTool {
        let scope = Arc::new(SpawnScopeImpl::root(
            model,
            Vec::new(),
            Duration::from_secs(60),
            1,
            1,
        ));
        AgentSpawnTool::new(scope)
    }

    struct TestExecutionContext {
        cancel: CancellationToken,
        bus: Option<BusHandle>,
    }

    impl ToolExecutionContext for TestExecutionContext {
        fn cancel_token(&self) -> &CancellationToken {
            &self.cancel
        }
        fn session_id(&self) -> &str {
            "test-session"
        }
        fn bus(&self) -> Option<&BusHandle> {
            self.bus.as_ref()
        }
        fn as_any(&self) -> &dyn std::any::Any {
            self
        }
    }

    /// Context-free path (the old behavior): without a bus we can't
    /// list live comm kinds, so `comms.items.properties.kind` has no
    /// `enum` — the string stays free-form and the executor validates
    /// against the registry at call time.
    #[test]
    fn parameters_schema_without_ctx_omits_comm_enum() {
        let tool = build_spawn_tool();
        let schema = tool.parameters_schema();
        let kind = &schema["properties"]["comms"]["items"]["properties"]["kind"];
        assert_eq!(kind["type"], "string");
        assert!(kind.get("enum").is_none());
    }

    /// Ctx-aware path: with a `ToolSchemaContext` whose bus has a
    /// populated `SpawnCommunicationRegistry`, the generated schema
    /// carries the live set of kind ids as a JSON-Schema `enum`.
    #[test]
    fn parameters_schema_with_ctx_injects_comm_enum() {
        let bus = Bus::new();
        let writer = bus.writer();
        let reg: Arc<dyn SpawnCommunicationRegistry> = Arc::new(TestRegistry::new());
        reg.register(Arc::new(DummyComm {
            kind: "blackboard".into(),
        }));
        reg.register(Arc::new(DummyComm {
            kind: "handoff".into(),
        }));
        writer.provide::<dyn SpawnCommunicationRegistry>(reg);
        let handle = bus.handle();

        let tool = build_spawn_tool();
        let ctx = ToolSchemaContext::with_bus(&handle);
        let schema = tool.parameters_schema_with(&ctx);

        let kind = &schema["properties"]["comms"]["items"]["properties"]["kind"];
        let enum_vals = kind["enum"]
            .as_array()
            .expect("comms.kind.enum should be populated from the bus registry")
            .iter()
            .map(|v| v.as_str().unwrap().to_string())
            .collect::<Vec<_>>();
        assert!(enum_vals.contains(&"blackboard".to_string()));
        assert!(enum_vals.contains(&"handoff".to_string()));
        assert_eq!(enum_vals.len(), 2);
    }

    #[tokio::test]
    async fn execute_with_model_field_without_provider_registry_inherits_parent_model() {
        let model = MockLanguageModel::new().with_response(make_assistant_message("child done"));
        let tool = build_spawn_tool_with_model(Arc::new(model));
        let bus = Bus::new();
        let ctx = TestExecutionContext {
            cancel: CancellationToken::new(),
            bus: Some(bus.handle()),
        };

        let out = tool
            .execute(
                serde_json::json!({
                    "task": "say child done",
                    "role": "summarizer",
                    "model": "deepseek-v4-flash"
                }),
                &ctx,
            )
            .await
            .expect("model override without ProviderRegistry should fall back");

        assert!(
            !out.is_error,
            "fallback should not error: {}",
            out.model_text()
        );
        assert!(
            out.model_text().contains("child done"),
            "child output should come from parent model fallback: {}",
            out.model_text()
        );
    }

    // ── AgentTemplate ↔ AgentSpawnTool connection ─────────────────────

    use crate::extension::skills::skill_domain::agent_template::AgentTemplate;

    fn make_template(name: &str, prompt: &str, tools: Option<Vec<String>>) -> AgentTemplate {
        AgentTemplate {
            id: name.to_string(),
            name: name.to_string(),
            description: format!("Use {name} when you need a {name}."),
            system_prompt_base: prompt.to_string(),
            skills: Default::default(),
            mcp_servers: Default::default(),
            allowed_tools: tools,
            max_iterations: None,
            model: None,
        }
    }

    #[test]
    fn build_template_model_uses_config_model_and_key_env() {
        use crate::extension::skills::skill_domain::agent_template::AgentModelConfig;
        // SAFETY: single-threaded test; sets then reads one env var.
        std::env::set_var("ALVA_TEST_SUBAGENT_KEY", "sk-test");
        let cfg = AgentModelConfig {
            model: "qwen3.5".into(),
            base_url: Some("https://example.test/v1".into()),
            provider_kind: Some("openai-chat".into()),
            api_key: None,
            api_key_env: Some("ALVA_TEST_SUBAGENT_KEY".into()),
            max_tokens: Some(2048),
        };
        let model = build_template_model(&cfg);
        assert_eq!(model.model_id(), "qwen3.5");
        std::env::remove_var("ALVA_TEST_SUBAGENT_KEY");
    }

    #[test]
    fn in_memory_registry_get_and_list() {
        let reg = InMemoryAgentTemplateRegistry::new(vec![
            make_template("a", "p", None),
            make_template("b", "p", None),
        ]);
        assert!(reg.get("a").is_some());
        assert!(reg.get("missing").is_none());
        assert_eq!(reg.list().len(), 2);
    }

    /// With templates on the bus, the schema exposes an `agent_type` enum
    /// plus each template's "when to use" description — the kimi-code
    /// `subagent_type` surfacing.
    #[test]
    fn schema_injects_agent_type_enum_and_when_to_use() {
        let bus = Bus::new();
        let reg: Arc<dyn AgentTemplateRegistry> = Arc::new(InMemoryAgentTemplateRegistry::new(vec![
            make_template("video", "You watch videos.", Some(vec!["understand_video".into()])),
            make_template("coder", "You write code.", None),
        ]));
        bus.writer().provide::<dyn AgentTemplateRegistry>(reg);
        let handle = bus.handle();

        let tool = build_spawn_tool();
        let ctx = ToolSchemaContext::with_bus(&handle);
        let schema = tool.parameters_schema_with(&ctx);

        let at = &schema["properties"]["agent_type"];
        let enum_vals = at["enum"]
            .as_array()
            .expect("agent_type.enum populated from registry")
            .iter()
            .map(|v| v.as_str().unwrap().to_string())
            .collect::<Vec<_>>();
        assert!(enum_vals.contains(&"video".to_string()));
        assert!(enum_vals.contains(&"coder".to_string()));
        assert!(
            at["description"].as_str().unwrap().contains("when you need"),
            "agent_type description should list per-template when-to-use"
        );
    }

    #[tokio::test]
    async fn agent_type_without_registry_errors() {
        let tool = build_spawn_tool();
        let bus = Bus::new();
        let ctx = TestExecutionContext {
            cancel: CancellationToken::new(),
            bus: Some(bus.handle()),
        };
        let out = tool
            .execute(
                serde_json::json!({ "task": "t", "role": "", "agent_type": "video" }),
                &ctx,
            )
            .await
            .expect("resolves to an error output");
        assert!(out.is_error);
        assert!(out.model_text().contains("no AgentTemplateRegistry"));
    }

    #[tokio::test]
    async fn unknown_agent_type_lists_available() {
        let tool = build_spawn_tool();
        let bus = Bus::new();
        bus.writer().provide::<dyn AgentTemplateRegistry>(Arc::new(
            InMemoryAgentTemplateRegistry::new(vec![make_template("coder", "p", None)]),
        ));
        let ctx = TestExecutionContext {
            cancel: CancellationToken::new(),
            bus: Some(bus.handle()),
        };
        let out = tool
            .execute(
                serde_json::json!({ "task": "t", "role": "", "agent_type": "nope" }),
                &ctx,
            )
            .await
            .expect("resolves to an error output");
        assert!(out.is_error);
        assert!(out.model_text().contains("unknown agent_type"));
        assert!(out.model_text().contains("coder"));
    }

    /// Happy path: spawning by `agent_type` resolves the template and runs
    /// the child (against the parent's mock model) without error.
    #[tokio::test]
    async fn spawn_by_agent_type_runs_via_template() {
        let model = MockLanguageModel::new().with_response(make_assistant_message("child done"));
        let tool = build_spawn_tool_with_model(Arc::new(model));
        let bus = Bus::new();
        bus.writer().provide::<dyn AgentTemplateRegistry>(Arc::new(
            InMemoryAgentTemplateRegistry::new(vec![make_template(
                "video",
                "You watch videos.",
                None,
            )]),
        ));
        let ctx = TestExecutionContext {
            cancel: CancellationToken::new(),
            bus: Some(bus.handle()),
        };
        let out = tool
            .execute(
                serde_json::json!({ "task": "watch it", "role": "", "agent_type": "video" }),
                &ctx,
            )
            .await
            .expect("spawn by agent_type should run");
        assert!(!out.is_error, "should not error: {}", out.model_text());
        assert!(out.model_text().contains("child done"));
    }
}
