// INPUT:  crate::run::run_agent, crate::state::{AgentState, AgentConfig}, crate::event::AgentEvent,
//         alva_kernel_abi::{AgentError, AgentMessage, CancellationToken}
// OUTPUT: pub trait AgentLoop, pub struct DefaultAgentLoop
// POS:    Swappable loop seam with source-compatible unbounded drive and additive bounded backpressure drive.

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

    /// Drive one run into a bounded event stream.
    ///
    /// Existing third-party loop implementations remain source-compatible:
    /// the default adapter forwards their legacy unbounded output into the
    /// bounded channel. Implementations that can apply backpressure at the
    /// producer should override this method; [`DefaultAgentLoop`] does so.
    async fn drive_bounded(
        &self,
        state: &AgentState,
        config: &AgentConfig,
        cancel: CancellationToken,
        input: Vec<AgentMessage>,
        event_tx: mpsc::Sender<AgentEvent>,
    ) -> Result<(), AgentError> {
        let (legacy_tx, mut legacy_rx) = mpsc::unbounded_channel();
        let drive = self.drive(state, config, cancel, input, legacy_tx);
        let forward = async move {
            while let Some(event) = legacy_rx.recv().await {
                if event_tx.send(event).await.is_err() {
                    break;
                }
            }
        };
        let (result, ()) = futures_util::future::join(drive, forward).await;
        result
    }
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

    async fn drive_bounded(
        &self,
        state: &AgentState,
        config: &AgentConfig,
        cancel: CancellationToken,
        input: Vec<AgentMessage>,
        event_tx: mpsc::Sender<AgentEvent>,
    ) -> Result<(), AgentError> {
        run_agent(state, config, cancel, input, event_tx).await
    }
}
