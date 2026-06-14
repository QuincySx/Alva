// INPUT:  std::sync::Arc, async_trait, alva_agent_context::scope::{BlackboardCommunication, BoardRegistry}, alva_kernel_abi::SpawnCommunicationRegistry, crate::extension::{Plugin, Registrar, LateContext}
// OUTPUT: BlackboardCommPlugin
// POS:    Optional Plugin that registers a BlackboardCommunication into the SpawnCommunicationRegistry on the bus (in finalize/late phase) — opt-in wiring for sub-agent board sharing.

//! `BlackboardCommPlugin` — registers the Blackboard communication
//! capability with the sub-agent spawn system.
//!
//! Without this extension, a child agent cannot be spawned with
//! `comms: [{kind: "blackboard", ...}]` — `AgentSpawnTool` will report
//! "unknown communication kind". Users opt in by adding the extension to
//! their `BaseAgentBuilder`.

use std::sync::Arc;

use async_trait::async_trait;

use alva_agent_context::scope::{BlackboardCommunication, BoardRegistry};
use alva_kernel_abi::tool::Tool;
use alva_kernel_abi::SpawnCommunicationRegistry;

use crate::extension::{LateContext, Plugin, Registrar};

/// Registers the shared Blackboard as a `SpawnCommunication` kind.
///
/// Needs the `SpawnCommunicationRegistry` to already be on the bus, which
/// `BaseAgentBuilder::build()` provides by default.
pub struct BlackboardCommPlugin {
    board_registry: Arc<BoardRegistry>,
}

impl BlackboardCommPlugin {
    /// Create with a fresh in-process `BoardRegistry`.
    pub fn new() -> Self {
        Self {
            board_registry: Arc::new(BoardRegistry::new()),
        }
    }

    /// Create with an existing `BoardRegistry` (lets the caller share the
    /// same registry across multiple agents if needed).
    pub fn with_registry(registry: Arc<BoardRegistry>) -> Self {
        Self { board_registry: registry }
    }
}

impl Default for BlackboardCommPlugin {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl Plugin for BlackboardCommPlugin {
    fn name(&self) -> &str {
        "blackboard-comm"
    }

    fn description(&self) -> &str {
        "Registers the shared-Blackboard communication kind for sub-agent spawns."
    }

    // Nothing to provide at assembly time.
    async fn register(&self, _r: &Registrar) {}

    // Reads the `SpawnCommunicationRegistry` (provided by another plugin in
    // its `register()` phase) and self-registers the Blackboard capability —
    // late wiring, since reading another plugin's bus capability is only
    // safe after all `register()` calls have finished.
    async fn finalize(&self, cx: &LateContext) -> Vec<Arc<dyn Tool>> {
        let Some(registry) = cx.bus.get::<dyn SpawnCommunicationRegistry>() else {
            tracing::warn!(
                "blackboard-comm: SpawnCommunicationRegistry not present on bus; skipping registration"
            );
            return vec![];
        };
        registry.register(Arc::new(BlackboardCommunication::new(
            self.board_registry.clone(),
        )));
        vec![]
    }
}
