// INPUT:  crate::run::run_agent, crate::state::{AgentState, AgentConfig}, crate::event::AgentEvent,
//         alva_kernel_abi::{AgentError, AgentMessage, CancellationToken}
// OUTPUT: pub trait AgentLoop, pub struct DefaultAgentLoop
// POS:    The swappable loop-driver seam (ADR-0007). DefaultAgentLoop delegates to run_agent; any
//         harness can `dyn AgentLoop` and substitute its own loop without touching the kernel default.

use alva_kernel_abi::{AgentError, AgentMessage, CancellationToken};
use async_trait::async_trait;
use tokio::sync::mpsc;

use crate::event::AgentEvent;
use crate::run::run_agent;
use crate::state::{AgentConfig, AgentState};

/// The agent-loop driver seam: one `drive` action whose default implementation
/// is [`run_agent`]. Any harness (Hub, a third-party driver, or a test) can
/// `dyn AgentLoop` and substitute its own loop while the kernel defaults keep
/// working unchanged.
#[async_trait]
pub trait AgentLoop: Send + Sync {
    /// Drive one agent run: read context from `state`, call the model through
    /// `config`, execute tools, and stream every observable into `event_tx`.
    async fn drive(
        &self,
        state: &AgentState,
        config: &AgentConfig,
        cancel: CancellationToken,
        input: Vec<AgentMessage>,
        event_tx: mpsc::UnboundedSender<AgentEvent>,
    ) -> Result<(), AgentError>;
}

/// The stock loop: delegates `drive` straight to [`run_agent`].
#[derive(Debug, Default, Clone, Copy)]
pub struct DefaultAgentLoop;

#[async_trait]
impl AgentLoop for DefaultAgentLoop {
    async fn drive(
        &self,
        state: &AgentState,
        config: &AgentConfig,
        cancel: CancellationToken,
        input: Vec<AgentMessage>,
        event_tx: mpsc::UnboundedSender<AgentEvent>,
    ) -> Result<(), AgentError> {
        run_agent(state, config, cancel, input, event_tx).await
    }
}
