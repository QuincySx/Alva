// INPUT:  std::collections::HashMap, alva_kernel_abi::AgentError, crate::graph::*, crate::checkpoint::CheckpointSaver
// OUTPUT: pub struct CompiledGraph, pub enum GraphEvent, pub struct InvokeConfig
// POS:    Type definitions for the Pregel execution engine.

use std::collections::HashMap;
use std::sync::Arc;

use tokio::sync::mpsc;

use crate::checkpoint::CheckpointSaver;
use crate::graph::{Edge, MergeFn, NodeFn};

// ---------------------------------------------------------------------------
// GraphEvent — execution observability
// ---------------------------------------------------------------------------

/// Events emitted during graph execution for observability.
#[derive(Debug, Clone)]
pub enum GraphEvent {
    /// A new superstep is starting.
    SuperstepStart { step: u32 },
    /// A node is about to execute.
    NodeStart { node: String, step: u32 },
    /// A node has finished executing.
    NodeEnd { node: String, step: u32 },
    /// A superstep has completed (all parallel nodes done).
    SuperstepEnd { step: u32, nodes_executed: usize },
    /// State was checkpointed.
    Checkpoint { step: u32, checkpoint_id: String },
    /// Graph execution completed successfully.
    Completed { total_steps: u32 },
    /// A Send was emitted by a node for dynamic routing.
    SendEmitted { from_node: String, to_node: String, step: u32 },
}

// ---------------------------------------------------------------------------
// InvokeConfig — optional features for invoke
// ---------------------------------------------------------------------------

/// Configuration for `invoke_with_config()`.
///
/// Enables optional features: checkpointing and event streaming.
/// Use `InvokeConfig::default()` for bare execution (same as `invoke()`).
pub struct InvokeConfig {
    /// If set, state is checkpointed after each superstep.
    pub checkpoint: Option<Arc<dyn CheckpointSaver>>,
    /// If set, execution events are sent to this channel.
    pub event_tx: Option<mpsc::UnboundedSender<GraphEvent>>,
    /// Checkpoint ID prefix (default: "graph").
    pub checkpoint_id: String,
    /// Maximum supersteps before forced termination (default: 100).
    /// Prevents infinite loops from misconfigured graphs.
    pub max_steps: u32,
}

impl Default for InvokeConfig {
    fn default() -> Self {
        Self {
            checkpoint: None,
            event_tx: None,
            checkpoint_id: "graph".into(),
            max_steps: 100,
        }
    }
}

impl InvokeConfig {
    pub fn with_checkpoint(mut self, saver: Arc<dyn CheckpointSaver>) -> Self {
        self.checkpoint = Some(saver);
        self
    }

    pub fn with_events(mut self, tx: mpsc::UnboundedSender<GraphEvent>) -> Self {
        self.event_tx = Some(tx);
        self
    }

    pub fn with_checkpoint_id(mut self, id: impl Into<String>) -> Self {
        self.checkpoint_id = id.into();
        self
    }

    pub(crate) fn emit(&self, event: GraphEvent) {
        if let Some(tx) = &self.event_tx {
            let _ = tx.send(event);
        }
    }
}

// ---------------------------------------------------------------------------
// CompiledGraph
// ---------------------------------------------------------------------------

/// A compiled, executable graph produced by [`StateGraph::compile`](crate::StateGraph::compile).
///
/// Supports:
/// - Sequential and parallel (BSP) execution
/// - Dynamic routing via `Send` (from `NodeResult::Sends`)
/// - Checkpoint persistence after each superstep
/// - Event streaming for observability
pub struct CompiledGraph<S> {
    pub(crate) nodes: HashMap<String, NodeFn<S>>,
    pub(crate) edges: Vec<Edge<S>>,
    pub(crate) entry_point: String,
    pub(crate) merge_fn: Option<MergeFn<S>>,
}

impl<S> std::fmt::Debug for CompiledGraph<S> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("CompiledGraph")
            .field("entry_point", &self.entry_point)
            .field("node_count", &self.nodes.len())
            .field("edge_count", &self.edges.len())
            .field("has_merge_fn", &self.merge_fn.is_some())
            .finish()
    }
}

#[cfg(test)]
mod tests {
    //! Tests for InvokeConfig — 5 tests covering 5 distinct contracts.
    //!
    //! 1. **Default state** — encodes safety posture: `max_steps=100`
    //!    is an infinite-loop guard (0 short-circuits every graph;
    //!    u32::MAX defeats the guard); `checkpoint_id="graph"` is the
    //!    literal directory key under which checkpoints are written
    //!    (silent rename orphans existing checkpoints); checkpoint /
    //!    event_tx None is the opt-in posture.
    //!
    //! 2. **Builder chain composes without clobbering** — the 3
    //!    `with_*` setters must all reach the final config; subsumes
    //!    per-setter `is_some` tests.
    //!
    //! 3. **emit() with no event_tx is no-op** — default callers don't
    //!    supply a channel; panic here would crash every bare invoke().
    //!
    //! 4. **emit() forwards events in FIFO order with fields intact**
    //!    — verifies both delivery AND ordering via field-pattern
    //!    matching; subsumes a separate single-forward test.
    //!
    //! 5. **emit() swallows send error when receiver dropped** —
    //!    `let _ = tx.send(...)` is deliberate; a subscriber
    //!    disconnect mid-graph must not crash the producer.
    use super::*;
    use crate::checkpoint::InMemoryCheckpointSaver;

    #[test]
    fn default_state_pins_all_safety_postures() {
        let cfg = InvokeConfig::default();
        assert!(cfg.checkpoint.is_none(), "default must be opt-in for checkpointing");
        assert!(cfg.event_tx.is_none(), "default must be opt-in for event streaming");
        assert_eq!(cfg.checkpoint_id, "graph", "literal key for checkpoint storage");
        assert_eq!(cfg.max_steps, 100, "infinite-loop guard literal");
    }

    #[test]
    fn builder_chain_with_all_three_setters_composes_without_clobbering() {
        let (tx, _rx) = mpsc::unbounded_channel::<GraphEvent>();
        let saver: Arc<dyn CheckpointSaver> = Arc::new(InMemoryCheckpointSaver::new());
        let cfg = InvokeConfig::default()
            .with_checkpoint(saver)
            .with_events(tx)
            .with_checkpoint_id("composed");
        assert!(cfg.checkpoint.is_some());
        assert!(cfg.event_tx.is_some());
        assert_eq!(cfg.checkpoint_id, "composed");
        assert_eq!(cfg.max_steps, 100, "max_steps default must survive builder chain");
    }

    #[test]
    fn emit_with_no_event_tx_is_noop_not_panic() {
        let cfg = InvokeConfig::default();
        cfg.emit(GraphEvent::SuperstepStart { step: 0 });
        cfg.emit(GraphEvent::Completed { total_steps: 5 });
        // If we got here without panic, the no-op contract holds.
    }

    #[tokio::test]
    async fn emit_forwards_multiple_events_in_fifo_order_with_fields_intact() {
        // Subsumes a separate single-forward test: the `matches!`
        // pattern matching on each received event verifies field
        // values (step=0, nodes_executed=2, total_steps=1) AND
        // ordering in one pass.
        let (tx, mut rx) = mpsc::unbounded_channel::<GraphEvent>();
        let cfg = InvokeConfig::default().with_events(tx);
        cfg.emit(GraphEvent::SuperstepStart { step: 0 });
        cfg.emit(GraphEvent::SuperstepEnd {
            step: 0,
            nodes_executed: 2,
        });
        cfg.emit(GraphEvent::Completed { total_steps: 1 });

        let e0 = rx.recv().await.expect("first event");
        assert!(matches!(e0, GraphEvent::SuperstepStart { step: 0 }));
        let e1 = rx.recv().await.expect("second event");
        assert!(matches!(
            e1,
            GraphEvent::SuperstepEnd {
                step: 0,
                nodes_executed: 2
            }
        ));
        let e2 = rx.recv().await.expect("third event");
        assert!(matches!(e2, GraphEvent::Completed { total_steps: 1 }));
    }

    #[tokio::test]
    async fn emit_silently_swallows_send_error_on_dropped_receiver() {
        let (tx, rx) = mpsc::unbounded_channel::<GraphEvent>();
        let cfg = InvokeConfig::default().with_events(tx);
        drop(rx);
        cfg.emit(GraphEvent::SuperstepStart { step: 0 });
        // If we got here without panic, the swallow-on-disconnect contract holds.
    }
}
