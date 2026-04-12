// INPUT:  crate::checkpoint::CheckpointSaver, crate::compaction::CompactionConfig, crate::pregel::CompiledGraph, crate::retry::RetryConfig
// OUTPUT: pub struct AgentSession
// POS:    High-level session wrapper bundling a compiled graph with retry, compaction, and checkpointing.
//         Note: linear agent support removed — use run_agent directly for single-agent flows.

use crate::checkpoint::CheckpointSaver;
use crate::compaction::CompactionConfig;
use crate::pregel::CompiledGraph;
use crate::retry::RetryConfig;

/// High-level session wrapper that bundles an execution backend (compiled
/// graph) with orchestration features: retry, compaction, and checkpointing.
///
/// # Example
///
/// ```ignore
/// let session = AgentSession::from_graph(compiled_graph)
///     .with_retry(RetryConfig::default())
///     .with_checkpoint(Box::new(InMemoryCheckpointSaver::new()));
/// ```
pub struct AgentSession {
    _graph: CompiledGraph<serde_json::Value>,
    retry_config: Option<RetryConfig>,
    compaction_config: Option<CompactionConfig>,
    checkpoint_saver: Option<Box<dyn CheckpointSaver>>,
}

impl AgentSession {
    /// Create a session backed by a compiled graph.
    pub fn from_graph(graph: CompiledGraph<serde_json::Value>) -> Self {
        Self {
            _graph: graph,
            retry_config: None,
            compaction_config: None,
            checkpoint_saver: None,
        }
    }

    /// Enable retry with the given configuration.
    pub fn with_retry(mut self, config: RetryConfig) -> Self {
        self.retry_config = Some(config);
        self
    }

    /// Enable context compaction with the given configuration.
    pub fn with_compaction(mut self, config: CompactionConfig) -> Self {
        self.compaction_config = Some(config);
        self
    }

    /// Enable checkpointing with the given saver implementation.
    pub fn with_checkpoint(mut self, saver: Box<dyn CheckpointSaver>) -> Self {
        self.checkpoint_saver = Some(saver);
        self
    }
}
