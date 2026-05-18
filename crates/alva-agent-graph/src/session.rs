// INPUT:  crate::checkpoint::CheckpointSaver, crate::compaction::CompactionConfig, crate::pregel::CompiledGraph, crate::retry::RetryConfig
// OUTPUT: pub struct GraphRun
// POS:    High-level session run wrapper bundling a compiled graph with retry, compaction, and checkpointing.
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
/// let session = GraphRun::from_graph(compiled_graph)
///     .with_retry(RetryConfig::default())
///     .with_checkpoint(Box::new(InMemoryCheckpointSaver::new()));
/// ```
pub struct GraphRun {
    _graph: CompiledGraph<serde_json::Value>,
    retry_config: Option<RetryConfig>,
    compaction_config: Option<CompactionConfig>,
    checkpoint_saver: Option<Box<dyn CheckpointSaver>>,
}

impl GraphRun {
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

#[cfg(test)]
mod tests {
    //! Tests for GraphRun builder.
    //!
    //! Two contracts, covered by two tests:
    //!
    //! 1. `from_graph` initial state — every optional capability is
    //!    None / opt-in. A regression that auto-defaulted e.g. retry
    //!    to Some would silently change failure semantics for every
    //!    caller. (Cannot be subsumed by the chain test, which sets
    //!    all three explicitly.)
    //!
    //! 2. Full 3-step builder chain — all three `with_*` setters
    //!    compose without clobbering each other. This catches the
    //!    silent-clobber regression for ALL setters in one shot;
    //!    individual `is_some` / `does_not_clobber` per-setter tests
    //!    are subsumed.
    //!
    //! `_graph` field is intentionally underscore-prefixed (held but
    //! not yet consumed by an `execute()` method).
    use super::*;
    use crate::checkpoint::InMemoryCheckpointSaver;
    use crate::pregel::CompiledGraph;
    use std::collections::HashMap;

    fn empty_compiled_graph() -> CompiledGraph<serde_json::Value> {
        CompiledGraph {
            nodes: HashMap::new(),
            edges: Vec::new(),
            entry_point: "start".to_string(),
            merge_fn: None,
        }
    }

    #[test]
    fn from_graph_initializes_all_optionals_to_none() {
        let run = GraphRun::from_graph(empty_compiled_graph());
        assert!(run.retry_config.is_none(), "retry must be opt-in");
        assert!(run.compaction_config.is_none(), "compaction must be opt-in");
        assert!(run.checkpoint_saver.is_none(), "checkpoint must be opt-in");
    }

    #[test]
    fn builder_chain_with_all_three_setters_composes_without_clobbering() {
        // Full chain: every `with_*` setter must set its own field
        // AND leave previously-set fields intact. A refactor that
        // e.g. recreated `self` from defaults inside a setter would
        // fail this test — and would silently break the documented
        // builder pattern used by GraphRun consumers.
        let saver: Box<dyn CheckpointSaver> = Box::new(InMemoryCheckpointSaver::new());
        let run = GraphRun::from_graph(empty_compiled_graph())
            .with_retry(RetryConfig::default())
            .with_compaction(CompactionConfig {
                max_tokens: 1000,
                keep_recent: 5,
                model: std::sync::Arc::new(NoopModel),
            })
            .with_checkpoint(saver);
        assert!(run.retry_config.is_some());
        assert!(run.compaction_config.is_some());
        assert!(run.checkpoint_saver.is_some());
    }

    // -- support for the chain test --------------------------------------

    use alva_kernel_abi::{
        AgentError, CompletionResponse, LanguageModel, Message, ModelConfig, StreamEvent, Tool,
    };
    use async_trait::async_trait;
    use futures_core::Stream;
    use std::pin::Pin;

    /// Stub model — only used to satisfy CompactionConfig::model.
    /// Mirrors the DummyModel pattern in compaction.rs::tests.
    struct NoopModel;

    #[async_trait]
    impl LanguageModel for NoopModel {
        fn model_id(&self) -> &str {
            "noop"
        }
        async fn complete(
            &self,
            _messages: &[Message],
            _tools: &[&dyn Tool],
            _config: &ModelConfig,
        ) -> Result<CompletionResponse, AgentError> {
            unimplemented!("NoopModel is a builder-test stub, not executed")
        }
        fn stream(
            &self,
            _messages: &[Message],
            _tools: &[&dyn Tool],
            _config: &ModelConfig,
        ) -> Pin<Box<dyn Stream<Item = StreamEvent> + Send>> {
            unimplemented!("NoopModel is a builder-test stub, not executed")
        }
    }
}
