// INPUT:  crate::run::run_agent, crate::state::{AgentState, AgentConfig}, crate::event::AgentEvent,
//         alva_kernel_abi::{AgentError, AgentMessage, CancellationToken}
// OUTPUT: pub trait AgentLoop, pub struct DefaultAgentLoop
// POS:    The swappable loop seam — ONE drive signature taking the event
//         transport (ADR-0007 revision, ticket 11). The old dual-signature
//         shape (unbounded `drive` + bounded `drive_bounded` plus a default
//         adapter forwarding the former into the latter) paid a compatibility
//         tax for third-party implementors that never existed; the transport
//         choice now lives in [`AgentEventSender`], so a single signature
//         covers both the lossless-unbounded SDK path and the
//         bounded-backpressure production path.

use alva_kernel_abi::{AgentError, AgentMessage, CancellationToken};
use async_trait::async_trait;

use crate::event::{AgentEvent, AgentEventSender};
use crate::run::run_agent;
use crate::state::{AgentConfig, AgentState};

/// The agent-loop driver seam: one `drive` action whose default implementation
/// is [`run_agent`]. Any harness (Hub, a third-party driver, or a test) can
/// `dyn AgentLoop` and substitute its own loop while the kernel defaults keep
/// working unchanged.
///
/// 阶段级钩子（"决定下一步"、"决定怎么排这批工具"）被明确拒绝进入这个
/// seam——那会把循环内部结构变成公开契约，才是真正锁死内核演化的做法。
/// seam 保持"给你状态和配置，你还我事件流"这个粗粒度。
#[async_trait]
pub trait AgentLoop: Send + Sync {
    /// Drive one agent run: read context from `state`, call the model through
    /// `config`, execute tools, and stream every observable into `event_tx`.
    ///
    /// The transport carries the backpressure policy: [`AgentEventSender::Bounded`]
    /// applies producer backpressure (lifecycle/message events wait for
    /// capacity; only synchronous tool progress may drop), while
    /// [`AgentEventSender::Unbounded`] keeps the lossless SDK semantics.
    async fn drive(
        &self,
        state: &AgentState,
        config: &AgentConfig,
        cancel: CancellationToken,
        input: Vec<AgentMessage>,
        event_tx: AgentEventSender,
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
        event_tx: AgentEventSender,
    ) -> Result<(), AgentError> {
        run_agent(state, config, cancel, input, event_tx).await
    }
}
