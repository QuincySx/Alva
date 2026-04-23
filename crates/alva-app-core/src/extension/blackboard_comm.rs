// INPUT:  std::sync::Arc, async_trait, alva_agent_context::scope::{BlackboardCommunication, BoardRegistry}, alva_kernel_abi::SpawnCommunicationRegistry, crate::extension::{Extension, ExtensionContext}
// OUTPUT: BlackboardCommExtension
// POS:    Optional Extension that registers a BlackboardCommunication into the SpawnCommunicationRegistry on the bus — opt-in wiring for sub-agent board sharing.

//! `BlackboardCommExtension` — registers the Blackboard communication
//! capability with the sub-agent spawn system.
//!
//! Without this extension, a child agent cannot be spawned with
//! `comms: [{kind: "blackboard", ...}]` — `AgentSpawnTool` will report
//! "unknown communication kind". Users opt in by adding the extension to
//! their `BaseAgentBuilder`.

use std::sync::Arc;

use async_trait::async_trait;

use alva_agent_context::scope::{BlackboardCommunication, BoardRegistry};
use alva_kernel_abi::SpawnCommunicationRegistry;

use crate::extension::{Extension, ExtensionContext};

/// Registers the shared Blackboard as a `SpawnCommunication` kind.
///
/// Needs the `SpawnCommunicationRegistry` to already be on the bus, which
/// `BaseAgentBuilder::build()` provides by default.
pub struct BlackboardCommExtension {
    board_registry: Arc<BoardRegistry>,
}

impl BlackboardCommExtension {
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

impl Default for BlackboardCommExtension {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl Extension for BlackboardCommExtension {
    fn name(&self) -> &str {
        "blackboard-comm"
    }

    fn description(&self) -> &str {
        "Registers the shared-Blackboard communication kind for sub-agent spawns."
    }

    async fn configure(&self, ctx: &ExtensionContext) {
        let Some(registry) = ctx.bus.get::<dyn SpawnCommunicationRegistry>() else {
            tracing::warn!(
                "blackboard-comm: SpawnCommunicationRegistry not present on bus; skipping registration"
            );
            return;
        };
        registry.register(Arc::new(BlackboardCommunication::new(
            self.board_registry.clone(),
        )));
    }
}
