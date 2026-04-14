//! Agent — SDK-level assembled agent handle.
//!
//! Produced by `AgentBuilder::build()`. Holds the wired-up `AgentState` +
//! `AgentConfig` + bus/extension-host bookkeeping. Runs the agent loop via
//! `alva_kernel_core::run_agent` when `.run()` is called.

use std::sync::Arc;
use tokio::sync::{mpsc, Mutex};

use alva_kernel_abi::{AgentError, AgentMessage, BusHandle, CancellationToken};
use alva_kernel_core::event::AgentEvent;
use alva_kernel_core::run_agent;
use alva_kernel_core::state::{AgentConfig, AgentState};

use crate::extension::ExtensionHost;

/// A fully-assembled, ready-to-run agent.
///
/// Use `Agent::builder()` to construct one.
pub struct Agent {
    pub(crate) state: Mutex<AgentState>,
    pub(crate) config: AgentConfig,
    pub(crate) bus: BusHandle,
    pub(crate) host: Arc<std::sync::RwLock<ExtensionHost>>,
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
        run_agent(&mut state, &self.config, cancel, input, tx).await?;
        Ok(rx)
    }

    /// Access the bus for out-of-band communication (e.g. injecting
    /// steering messages, reading capability registrations).
    pub fn bus(&self) -> &BusHandle {
        &self.bus
    }
}
