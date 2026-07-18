// INPUT:  AgentState, AgentConfig, run_agent, PluginHost, Tool
// OUTPUT: Agent, AgentAssemblySnapshot
// POS:    SDK-level assembled agent handle with runtime execution and build-time observability.
//! Agent — SDK-level assembled agent handle.
//!
//! Produced by `AgentBuilder::build()`. Holds the wired-up `AgentState` +
//! `AgentConfig` + bus/plugin-host bookkeeping. Runs the agent loop via
//! `alva_kernel_core::run_agent` when `.run()` is called.

use std::sync::Arc;

use serde::{Deserialize, Serialize};
use tokio::sync::{mpsc, Mutex, RwLock};

use alva_kernel_abi::{
    AgentError, AgentMessage, BusHandle, CancellationToken, ReasoningEffort, Tool,
};
use alva_kernel_core::event::AgentEvent;
use alva_kernel_core::run_agent;
use alva_kernel_core::state::{AgentConfig, AgentState};

use crate::extension::PluginHost;

/// Per-plugin build-time contribution metadata for observability.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct PluginAssemblySnapshot {
    pub name: String,
    pub description: String,
    pub registered_tool_names: Vec<String>,
    pub finalized_tool_names: Vec<String>,
    pub middleware_names: Vec<String>,
    #[serde(default)]
    pub phase_contribution_names: Vec<String>,
    pub command_names: Vec<String>,
    pub system_prompt_fragments: usize,
}

/// Build-time assembly metadata for observability.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct AgentAssemblySnapshot {
    /// Plugin names in registration order.
    pub plugin_names: Vec<String>,
    /// Middleware names in final sorted execution order.
    pub middleware_names: Vec<String>,
    /// Middleware names registered directly on AgentBuilder/BaseAgentBuilder,
    /// outside any plugin.
    #[serde(default)]
    pub direct_middleware_names: Vec<String>,
    /// Per-plugin contribution records in registration order.
    pub plugins: Vec<PluginAssemblySnapshot>,
}

/// A fully-assembled, ready-to-run agent.
///
/// Use `Agent::builder()` to construct one.
///
/// `config` is wrapped in a `RwLock` so per-turn overrides (e.g.
/// `reasoning_effort`) can be set between runs without rebuilding the
/// whole agent. Write lock is very brief (just flip one field); read
/// lock is held across the full `run_agent` call.
pub struct Agent {
    pub(crate) state: Mutex<AgentState>,
    pub(crate) config: RwLock<AgentConfig>,
    pub(crate) bus: BusHandle,
    pub(crate) host: Arc<std::sync::RwLock<PluginHost>>,
    /// Snapshot of the tools the agent was built with. Cached so callers
    /// can inspect tool definitions without locking `state`.
    pub(crate) tools: Vec<Arc<dyn Tool>>,
    pub(crate) assembly: AgentAssemblySnapshot,
}

impl Agent {
    /// Start building a new agent.
    pub fn builder() -> crate::agent_builder::AgentBuilder {
        crate::agent_builder::AgentBuilder::new()
    }

    /// Run one conversation turn. Returns a channel that streams
    /// `AgentEvent`s until the turn completes.
    ///
    /// `cancel` lets the caller interrupt the loop mid-turn.
    pub async fn run(
        &self,
        input: Vec<AgentMessage>,
        cancel: CancellationToken,
    ) -> Result<mpsc::UnboundedReceiver<AgentEvent>, AgentError> {
        let (tx, rx) = mpsc::unbounded_channel();
        let mut state = self.state.lock().await;
        let config = self.config.read().await;
        run_agent(&mut state, &*config, cancel, input, tx).await?;
        Ok(rx)
    }

    /// Set (or clear) the reasoning effort applied to every LLM call in
    /// the next run. Takes effect immediately for the next `run()` — a
    /// currently-running `run()` is unaffected (it holds a read guard).
    ///
    /// Per-provider translation happens inside each provider's request-
    /// body assembly; `None` means "don't send the field" (use provider
    /// default).
    pub async fn set_reasoning_effort(&self, effort: Option<ReasoningEffort>) {
        let mut config = self.config.write().await;
        config.model_config.reasoning_effort = effort;
    }

    /// Per-turn override of the provider-specific JSON pass-through
    /// (`ModelConfig::extra_body`). Same write-lock semantics as
    /// `set_reasoning_effort`: takes effect on the next `run()`. `None`
    /// or an empty map clears any previous override.
    pub async fn set_extra_body(&self, extra: Option<serde_json::Map<String, serde_json::Value>>) {
        let mut config = self.config.write().await;
        config.model_config.extra_body = match extra {
            Some(m) if !m.is_empty() => Some(m),
            _ => None,
        };
    }

    /// Per-turn override of `ModelConfig::disable_tools`. When set to
    /// `true`, the next `run()` skips ALL tool injection (state.tools
    /// stays as-is, but the provider sees `tools: []` → omits the
    /// field). Use when the active model doesn't support function
    /// calling.
    pub async fn set_disable_tools(&self, disabled: bool) {
        let mut config = self.config.write().await;
        config.model_config.disable_tools = disabled;
    }

    /// Per-turn manual tool allow-list. `Some(names)` restricts the next
    /// `run()` to exactly those tools; `None` clears the restriction. The
    /// agent's registered tool set is untouched, so a later turn can widen the
    /// selection again — this is the non-destructive counterpart to
    /// `retain_tools`, meant for a UI control that changes per turn.
    pub async fn set_allowed_tools(&self, allowed: Option<Vec<String>>) {
        let mut config = self.config.write().await;
        config.model_config.allowed_tools = allowed;
    }

    /// Access the bus for out-of-band communication (e.g. injecting
    /// steering messages, reading capability registrations).
    pub fn bus(&self) -> &BusHandle {
        &self.bus
    }

    /// Access the runtime plugin host (middleware + command registry,
    /// agent binding for cancellation/pending messages).
    pub fn host(&self) -> &Arc<std::sync::RwLock<PluginHost>> {
        &self.host
    }

    /// Snapshot of the tools the agent was built with.
    pub fn tools(&self) -> &[Arc<dyn Tool>] {
        &self.tools
    }

    /// Build-time plugin and middleware assembly metadata.
    pub fn assembly_snapshot(&self) -> AgentAssemblySnapshot {
        self.assembly.clone()
    }

    /// Access the agent config (read-only). Returns a read guard that
    /// dereferences to `&AgentConfig`. The guard is held across the
    /// caller's usage — keep it short if another turn might need to
    /// update config via `set_reasoning_effort`.
    pub async fn config(&self) -> tokio::sync::RwLockReadGuard<'_, AgentConfig> {
        self.config.read().await
    }

    /// Access the underlying state mutex (advanced — most callers should
    /// use `run`/`messages` accessors via a wrapping handle).
    pub fn state(&self) -> &Mutex<AgentState> {
        &self.state
    }
}
