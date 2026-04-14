//! Agent — SDK-level assembled agent handle.
//!
//! Produced by `AgentBuilder::build()`. Holds the wired-up `AgentState` +
//! `AgentConfig` + bus/extension-host bookkeeping. Runs the agent loop via
//! `alva_kernel_core::run_agent` when `.run()` is called.

use std::sync::Arc;
use tokio::sync::{mpsc, Mutex};

use alva_kernel_abi::{AgentError, AgentMessage, BusHandle, CancellationToken, Tool};
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
    /// Snapshot of the tools the agent was built with. Cached so callers
    /// can inspect tool definitions without locking `state`.
    pub(crate) tools: Vec<Arc<dyn Tool>>,
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

    /// Access the runtime extension host (event dispatch, command registry,
    /// agent binding for cancellation/pending messages).
    pub fn host(&self) -> &Arc<std::sync::RwLock<ExtensionHost>> {
        &self.host
    }

    /// Snapshot of the tools the agent was built with.
    pub fn tools(&self) -> &[Arc<dyn Tool>] {
        &self.tools
    }

    /// Access the agent config (read-only).
    pub fn config(&self) -> &AgentConfig {
        &self.config
    }

    /// Access the underlying state mutex (advanced — most callers should
    /// use `run`/`messages` accessors via a wrapping handle).
    pub fn state(&self) -> &Mutex<AgentState> {
        &self.state
    }
}
