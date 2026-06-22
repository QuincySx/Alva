// INPUT:  async_trait, serde_json, std::sync::Arc, crate::{BusHandle, context::ContextHooks}
// OUTPUT: SpawnCommunication, SpawnCommContext, SpawnCommHandle, SpawnCommError, OnChildComplete, SpawnResult, SpawnCommunicationRegistry
// POS:    Communication-capability trait contract attached to sub-agents at spawn time (pluggable alternative to hardcoded Blackboard wiring).

//! Spawn-time communication plugin contract.
//!
//! `AgentSpawnTool` used to hardcode a `board: Option<String>` field in its
//! input. The `SpawnCommunication` trait replaces that with a pluggable
//! mechanism: any "communication capability" (shared blackboard, handoff
//! recap, callback channel, parent watch, shared workspace, …) can be
//! registered against a kind name, and the LLM picks which ones to attach
//! per spawn via the `comms: [{kind, config}]` input field.
//!
//! Lifecycle:
//! 1. LLM emits `comms: [{kind: "blackboard", config: {board_id: "team-x"}}]`.
//! 2. `AgentSpawnTool::execute` looks up `SpawnCommunicationRegistry` on the
//!    bus and, for each entry, fetches the matching `SpawnCommunication`.
//! 3. It calls `attach(SpawnCommContext, config)` which returns a
//!    [`SpawnCommHandle`] — the hooks to mount on the child's `ContextSystem`
//!    plus an optional `on_complete` callback fired after the child run.
//! 4. The child agent runs with the hooks installed; when it finishes,
//!    each `on_complete` is invoked with the result.

use std::sync::Arc;

use async_trait::async_trait;
use serde_json::Value;

use crate::context::ContextHooks;
use crate::BusHandle;

// ---------------------------------------------------------------------------
// SpawnCommContext
// ---------------------------------------------------------------------------

/// Context passed to `SpawnCommunication::attach` describing the
/// parent/child scope + session identifiers and the active bus handle.
///
/// Borrowed for the duration of a single `attach()` call — implementations
/// must clone any data they need to retain.
pub struct SpawnCommContext<'a> {
    pub parent_scope_id: &'a str,
    pub parent_session_id: &'a str,
    pub child_scope_id: &'a str,
    pub child_session_id: &'a str,
    pub role: &'a str,
    pub bus: Option<&'a BusHandle>,
}

// ---------------------------------------------------------------------------
// SpawnResult
// ---------------------------------------------------------------------------

/// Minimal abi-layer view of a completed child agent run.
///
/// Kept deliberately sparse (no references to `run_child`) so the
/// `OnChildComplete` callback trait can live in `alva-kernel-abi` without
/// creating a dependency cycle with `alva-kernel-core`.
#[derive(Clone, Debug)]
pub struct SpawnResult {
    /// The collected text output.
    pub text: String,
    /// Whether the agent encountered an error.
    pub is_error: bool,
    /// Error message, if any.
    pub error: Option<String>,
}

// ---------------------------------------------------------------------------
// OnChildComplete
// ---------------------------------------------------------------------------

/// Callback invoked after a child agent run completes (successfully or not).
///
/// Used by communication plugins that need to react to the child's final
/// output — e.g. BlackboardCommunication posts the child's text back to
/// the shared board as an `Artifact` message.
#[async_trait]
pub trait OnChildComplete: Send + Sync {
    async fn call(&self, result: &SpawnResult);
}

// ---------------------------------------------------------------------------
// SpawnCommHandle
// ---------------------------------------------------------------------------

/// What a `SpawnCommunication::attach()` call returns to `AgentSpawnTool`.
///
/// - `hooks`: `ContextHooks` implementations to mount on the child's
///   `ContextSystem`. Multiple plugins can contribute hooks to the same
///   spawn (they'll all be chained via `ContextHooksChain`).
/// - `on_complete`: optional post-run callback receiving the child's
///   final [`SpawnResult`].
pub struct SpawnCommHandle {
    pub hooks: Vec<Arc<dyn ContextHooks>>,
    pub on_complete: Option<Arc<dyn OnChildComplete>>,
}

impl SpawnCommHandle {
    pub fn empty() -> Self {
        Self {
            hooks: Vec::new(),
            on_complete: None,
        }
    }

    pub fn with_hooks(hooks: Vec<Arc<dyn ContextHooks>>) -> Self {
        Self {
            hooks,
            on_complete: None,
        }
    }

    pub fn with_on_complete(mut self, cb: Arc<dyn OnChildComplete>) -> Self {
        self.on_complete = Some(cb);
        self
    }
}

// ---------------------------------------------------------------------------
// SpawnCommError
// ---------------------------------------------------------------------------

/// Errors that a `SpawnCommunication::attach` may return.
#[derive(Debug)]
pub enum SpawnCommError {
    /// The config JSON did not match the plugin's expected schema.
    InvalidConfig(String),
    /// The plugin could not initialize the capability for other reasons.
    AttachFailed(String),
}

impl std::fmt::Display for SpawnCommError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            SpawnCommError::InvalidConfig(msg) => write!(f, "invalid config: {msg}"),
            SpawnCommError::AttachFailed(msg) => write!(f, "attach failed: {msg}"),
        }
    }
}

impl std::error::Error for SpawnCommError {}

// ---------------------------------------------------------------------------
// SpawnCommunication
// ---------------------------------------------------------------------------

/// A communication capability that can be attached to a sub-agent at spawn.
///
/// Implementations are registered with a [`SpawnCommunicationRegistry`] that
/// lives on the bus. The LLM chooses which ones to attach per spawn by
/// listing `{kind, config}` tuples in the spawn tool input.
#[async_trait]
pub trait SpawnCommunication: Send + Sync {
    /// Kind identifier — matches the `kind` field in the spawn tool input.
    fn kind(&self) -> &str;

    /// One-line description exposed to the LLM via schema metadata.
    fn description(&self) -> &str;

    /// JSON schema for this kind's `config` payload. The default is an
    /// open object (`{"type":"object"}`); override to constrain fields.
    fn config_schema(&self) -> Value {
        serde_json::json!({ "type": "object" })
    }

    /// Called just before the child agent starts. Returns hooks to install
    /// plus an optional on-complete callback.
    async fn attach(
        &self,
        ctx: &SpawnCommContext<'_>,
        config: Value,
    ) -> Result<SpawnCommHandle, SpawnCommError>;
}

// ---------------------------------------------------------------------------
// SpawnCommunicationRegistry
// ---------------------------------------------------------------------------

/// Bus Capability: registry of spawn-time communication plugins
/// (blackboard, handoff, RPC, …).
///
/// **Provider**: `SpawnCommRegistryPlugin::register`
/// (`alva-app-core/src/extension/spawn_comm_registry.rs`). Opt-in — the
/// outer app must register the plugin. No built-in default.
/// **Consumers**: `AgentSpawnTool` (enumerates available `kind`s for
/// its JSON-Schema, resolves a plugin by kind at spawn time);
/// `BlackboardCommPlugin` pulls the registry in its own `finalize`
/// to register itself as a communication plugin.
/// **Why bus**: the registry is populated from multiple plugins at
/// different times — spawn-comm registry extension provides the empty
/// registry, other extensions (blackboard, etc.) mutate it in their own
/// `configure` step. Constructor injection can't express that ordering
/// cleanly; bus-backed late injection is exactly the right shape.
#[crate::bus_cap]
pub trait SpawnCommunicationRegistry: Send + Sync {
    /// Add a capability. Idempotency/overwrite policy is implementation-defined.
    fn register(&self, ch: Arc<dyn SpawnCommunication>);

    /// Look up by kind.
    fn get(&self, kind: &str) -> Option<Arc<dyn SpawnCommunication>>;

    /// Snapshot of all registered capabilities (order unspecified).
    fn list(&self) -> Vec<Arc<dyn SpawnCommunication>>;
}
