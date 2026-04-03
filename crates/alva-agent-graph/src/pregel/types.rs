// INPUT:  std::collections::HashMap, alva_types::AgentError, crate::graph::*, crate::checkpoint::CheckpointSaver
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
