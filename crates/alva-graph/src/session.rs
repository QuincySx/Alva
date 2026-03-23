use alva_core::Agent;

use crate::checkpoint::CheckpointSaver;
use crate::compaction::CompactionConfig;
use crate::pregel::CompiledGraph;
use crate::retry::RetryConfig;

/// The underlying execution backend for a session.
#[allow(dead_code)]
enum SessionKind {
    /// A single-agent linear loop.
    Linear(Agent),
    /// A multi-node graph execution.
    Graph(CompiledGraph<serde_json::Value>),
}

/// High-level session wrapper that bundles an execution backend (linear
/// agent or compiled graph) with orchestration features: retry, compaction,
/// and checkpointing.
///
/// # Example
///
/// ```ignore
/// let session = AgentSession::from_agent(agent)
///     .with_retry(RetryConfig::default())
///     .with_checkpoint(Box::new(InMemoryCheckpointSaver::new()));
/// ```
pub struct AgentSession {
    _kind: SessionKind,
    retry_config: Option<RetryConfig>,
    compaction_config: Option<CompactionConfig>,
    checkpoint_saver: Option<Box<dyn CheckpointSaver>>,
}

impl AgentSession {
    /// Create a session backed by a single linear agent.
    pub fn from_agent(agent: Agent) -> Self {
        Self {
            _kind: SessionKind::Linear(agent),
            retry_config: None,
            compaction_config: None,
            checkpoint_saver: None,
        }
    }

    /// Create a session backed by a compiled graph.
    pub fn from_graph(graph: CompiledGraph<serde_json::Value>) -> Self {
        Self {
            _kind: SessionKind::Graph(graph),
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
