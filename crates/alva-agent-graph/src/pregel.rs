// INPUT:  std::collections::HashMap, alva_types::AgentError, crate::graph::*, crate::checkpoint::CheckpointSaver
// OUTPUT: pub struct CompiledGraph, pub enum GraphEvent, pub struct InvokeConfig
// POS:    Pregel-style BSP execution engine with checkpoint, streaming events, and Send-based dynamic routing.

use std::collections::HashMap;
use std::sync::Arc;

use alva_types::AgentError;
use tokio::sync::mpsc;

use crate::checkpoint::CheckpointSaver;
use crate::graph::{Edge, MergeFn, NodeFn, NodeResult, END};

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

    fn emit(&self, event: GraphEvent) {
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

impl<S: Send + 'static> CompiledGraph<S> {
    /// Execute the graph. Shorthand for `invoke_with_config` with defaults.
    pub async fn invoke(&self, input: S) -> Result<S, AgentError>
    where
        S: Clone + serde::Serialize,
    {
        self.invoke_with_config(input, InvokeConfig::default()).await
    }

    /// Execute the graph with optional checkpointing and event streaming.
    pub async fn invoke_with_config(
        &self,
        input: S,
        config: InvokeConfig,
    ) -> Result<S, AgentError>
    where
        S: Clone + serde::Serialize,
    {
        let mut state = input;
        let mut current_nodes = vec![self.entry_point.clone()];
        let mut step: u32 = 0;

        loop {
            current_nodes.retain(|n| n != END);
            if current_nodes.is_empty() {
                config.emit(GraphEvent::Completed { total_steps: step });
                return Ok(state);
            }

            step += 1;
            if step > config.max_steps {
                return Err(AgentError::Other(format!(
                    "Graph execution exceeded max_steps ({}) — possible infinite loop",
                    config.max_steps
                )));
            }
            config.emit(GraphEvent::SuperstepStart { step });

            let nodes_in_step = current_nodes.len();

            if current_nodes.len() == 1 {
                // Single node — fast path
                let node_name = &current_nodes[0];
                let node_fn = self.nodes.get(node_name).ok_or_else(|| {
                    AgentError::ConfigError(format!("Node not found: {}", node_name))
                })?;

                config.emit(GraphEvent::NodeStart {
                    node: node_name.clone(),
                    step,
                });

                let result = node_fn(state.clone()).await;

                config.emit(GraphEvent::NodeEnd {
                    node: node_name.clone(),
                    step,
                });

                match result {
                    NodeResult::Update(new_state) => {
                        state = new_state;
                        current_nodes = self.resolve_next_nodes(&current_nodes[0], &state)?;
                    }
                    NodeResult::Sends(sends) => {
                        // Dynamic routing: fan out to Send targets
                        for send in &sends {
                            config.emit(GraphEvent::SendEmitted {
                                from_node: current_nodes[0].clone(),
                                to_node: send.node.clone(),
                                step,
                            });
                        }
                        current_nodes = self.execute_sends(sends, &mut state, &config, step).await?;
                    }
                }
            } else {
                // Parallel superstep
                let base_state = state.clone();
                let mut join_set = tokio::task::JoinSet::new();

                for node_name in &current_nodes {
                    let node_fn = self.nodes.get(node_name).ok_or_else(|| {
                        AgentError::ConfigError(format!("Node not found: {}", node_name))
                    })?;

                    config.emit(GraphEvent::NodeStart {
                        node: node_name.clone(),
                        step,
                    });

                    let s = state.clone();
                    let fut = node_fn(s);
                    let name = node_name.clone();
                    join_set.spawn(async move { (name, fut.await) });
                }

                let mut updates = Vec::new();
                let mut all_sends: Vec<crate::graph::SendTo<S>> = Vec::new();

                while let Some(join_result) = join_set.join_next().await {
                    let (name, result) = join_result.map_err(|e| {
                        AgentError::Other(format!("Parallel node task failed: {e}"))
                    })?;

                    config.emit(GraphEvent::NodeEnd {
                        node: name.clone(),
                        step,
                    });

                    match result {
                        NodeResult::Update(s) => updates.push(s),
                        NodeResult::Sends(sends) => {
                            for send in &sends {
                                config.emit(GraphEvent::SendEmitted {
                                    from_node: name.clone(),
                                    to_node: send.node.clone(),
                                    step,
                                });
                            }
                            all_sends.extend(sends);
                        }
                    }
                }

                // Merge state updates
                if !updates.is_empty() {
                    state = match &self.merge_fn {
                        Some(merge) => merge(base_state, updates),
                        None => updates.into_iter().last().unwrap_or(base_state),
                    };
                }

                // Process Sends (they create the next round of nodes)
                if !all_sends.is_empty() {
                    current_nodes = self.execute_sends(all_sends, &mut state, &config, step).await?;
                } else {
                    // Collect next nodes from all executed nodes via edges
                    let mut next = Vec::new();
                    for node in &current_nodes {
                        next.extend(self.resolve_next_nodes(node, &state)?);
                    }
                    next.sort();
                    next.dedup();
                    current_nodes = next;
                }
            }

            config.emit(GraphEvent::SuperstepEnd {
                step,
                nodes_executed: nodes_in_step,
            });

            // Checkpoint after each superstep
            if let Some(saver) = &config.checkpoint {
                let cp_id = format!("{}-step-{}", config.checkpoint_id, step);
                let serialized = serde_json::to_value(&state).unwrap_or_default();
                if let Err(e) = saver.save(&cp_id, serialized).await {
                    tracing::warn!(step, error = %e, "checkpoint save failed");
                } else {
                    config.emit(GraphEvent::Checkpoint {
                        step,
                        checkpoint_id: cp_id,
                    });
                }
            }
        }
    }

    /// Execute Send targets in parallel, merge results, return next nodes.
    async fn execute_sends(
        &self,
        sends: Vec<crate::graph::SendTo<S>>,
        state: &mut S,
        config: &InvokeConfig,
        step: u32,
    ) -> Result<Vec<String>, AgentError>
    where
        S: Clone + serde::Serialize,
    {
        if sends.len() == 1 {
            // Single send — execute directly
            let send = sends.into_iter().next().unwrap();
            let node_fn = self.nodes.get(&send.node).ok_or_else(|| {
                AgentError::ConfigError(format!("Send target node not found: {}", send.node))
            })?;

            config.emit(GraphEvent::NodeStart {
                node: send.node.clone(),
                step,
            });

            let result = node_fn(send.state).await;

            config.emit(GraphEvent::NodeEnd {
                node: send.node.clone(),
                step,
            });

            match result {
                NodeResult::Update(new_state) => {
                    let next = self.resolve_next_nodes(&send.node, &new_state)?;
                    *state = new_state;
                    Ok(next)
                }
                NodeResult::Sends(more_sends) => {
                    // Recursive sends — execute them too
                    Box::pin(self.execute_sends(more_sends, state, config, step)).await
                }
            }
        } else {
            // Multiple sends — parallel execution
            let base_state = state.clone();
            let mut join_set = tokio::task::JoinSet::new();

            for send in sends {
                let node_fn = self.nodes.get(&send.node).ok_or_else(|| {
                    AgentError::ConfigError(format!("Send target node not found: {}", send.node))
                })?;

                config.emit(GraphEvent::NodeStart {
                    node: send.node.clone(),
                    step,
                });

                let fut = node_fn(send.state);
                let name = send.node;
                join_set.spawn(async move { (name, fut.await) });
            }

            let mut updates = Vec::new();
            let mut next_nodes = Vec::new();

            while let Some(join_result) = join_set.join_next().await {
                let (name, result) = join_result.map_err(|e| {
                    AgentError::Other(format!("Send task failed: {e}"))
                })?;

                config.emit(GraphEvent::NodeEnd {
                    node: name.clone(),
                    step,
                });

                match result {
                    NodeResult::Update(s) => {
                        next_nodes.extend(self.resolve_next_nodes(&name, &s)?);
                        updates.push(s);
                    }
                    NodeResult::Sends(_) => {
                        // Nested sends from parallel execution — not supported yet
                        tracing::warn!(node = %name, "nested Sends from parallel execution are dropped");
                    }
                }
            }

            if !updates.is_empty() {
                *state = match &self.merge_fn {
                    Some(merge) => merge(base_state, updates),
                    None => updates.into_iter().last().unwrap_or_else(|| state.clone()),
                };
            }

            next_nodes.sort();
            next_nodes.dedup();
            Ok(next_nodes)
        }
    }

    /// Collect all next nodes from edges matching `current`.
    fn resolve_next_nodes(&self, current: &str, state: &S) -> Result<Vec<String>, AgentError> {
        let mut targets = Vec::new();
        for edge in &self.edges {
            match edge {
                Edge::Direct { from, to } if from == current => {
                    targets.push(to.clone());
                }
                Edge::Conditional { from, router } if from == current => {
                    targets.push(router(state));
                }
                _ => continue,
            }
        }
        if targets.is_empty() {
            targets.push(END.to_string());
        }
        Ok(targets)
    }
}

// ===========================================================================
// Tests
// ===========================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use crate::graph::StateGraph;
    use serde_json::json;

    #[tokio::test]
    async fn simple_linear_graph() {
        let mut graph = StateGraph::new();

        graph.add_node("step1", |state: serde_json::Value| {
            Box::pin(async move {
                let mut s = state;
                s["step1"] = json!(true);
                s
            })
        });

        graph.add_node("step2", |state: serde_json::Value| {
            Box::pin(async move {
                let mut s = state;
                s["step2"] = json!(true);
                s
            })
        });

        graph.set_entry_point("step1");
        graph.add_edge("step1", "step2");
        graph.add_edge("step2", END);

        let compiled = graph.compile().unwrap();
        let result = compiled.invoke(json!({})).await.unwrap();

        assert_eq!(result["step1"], true);
        assert_eq!(result["step2"], true);
    }

    #[tokio::test]
    async fn conditional_routing() {
        let mut graph = StateGraph::new();

        graph.add_node("router_node", |state| Box::pin(async { state }));
        graph.add_node("path_a", |state: serde_json::Value| {
            Box::pin(async move {
                let mut s = state;
                s["path"] = "a".into();
                s
            })
        });
        graph.add_node("path_b", |state: serde_json::Value| {
            Box::pin(async move {
                let mut s = state;
                s["path"] = "b".into();
                s
            })
        });

        graph.set_entry_point("router_node");
        graph.add_conditional_edge("router_node", |state| {
            if state
                .get("go_a")
                .and_then(|v| v.as_bool())
                .unwrap_or(false)
            {
                "path_a".to_string()
            } else {
                "path_b".to_string()
            }
        });
        graph.add_edge("path_a", END);
        graph.add_edge("path_b", END);

        let compiled = graph.compile().unwrap();

        let result = compiled.invoke(json!({"go_a": true})).await.unwrap();
        assert_eq!(result["path"], "a");

        let result = compiled.invoke(json!({"go_a": false})).await.unwrap();
        assert_eq!(result["path"], "b");
    }

    #[tokio::test]
    async fn single_node_no_explicit_edge_defaults_to_end() {
        let mut graph = StateGraph::new();
        graph.add_node("only", |state: serde_json::Value| {
            Box::pin(async move {
                let mut s = state;
                s["done"] = json!(true);
                s
            })
        });
        graph.set_entry_point("only");

        let compiled = graph.compile().unwrap();
        let result = compiled.invoke(json!({})).await.unwrap();
        assert_eq!(result["done"], true);
    }

    #[test]
    fn compile_fails_without_entry_point() {
        let graph = StateGraph::<serde_json::Value>::new();
        let err = graph.compile().unwrap_err();
        assert!(err.to_string().contains("Entry point not set"));
    }

    #[test]
    fn compile_fails_with_invalid_entry_point() {
        let mut graph = StateGraph::<serde_json::Value>::new();
        graph.set_entry_point("nonexistent");
        let err = graph.compile().unwrap_err();
        assert!(err.to_string().contains("does not exist"));
    }

    #[test]
    fn compile_fails_with_invalid_edge_target() {
        let mut graph = StateGraph::new();
        graph.add_node("a", |s: serde_json::Value| Box::pin(async { s }));
        graph.set_entry_point("a");
        graph.add_edge("a", "nonexistent");
        let err = graph.compile().unwrap_err();
        assert!(err.to_string().contains("Edge target"));
    }

    #[test]
    fn compile_fails_with_invalid_edge_source() {
        let mut graph = StateGraph::new();
        graph.add_node("a", |s: serde_json::Value| Box::pin(async { s }));
        graph.set_entry_point("a");
        graph.add_edge("nonexistent", "a");
        let err = graph.compile().unwrap_err();
        assert!(err.to_string().contains("Edge source"));
    }

    #[tokio::test]
    async fn parallel_fan_out_execution() {
        use std::sync::atomic::{AtomicU32, Ordering};

        let counter = Arc::new(AtomicU32::new(0));
        let mut graph = StateGraph::<serde_json::Value>::new();

        graph.add_node("entry", |s| Box::pin(async { s }));

        let c1 = counter.clone();
        graph.add_node("parallel_a", move |s: serde_json::Value| {
            let c = c1.clone();
            Box::pin(async move {
                c.fetch_add(1, Ordering::SeqCst);
                let mut s = s;
                s["a"] = serde_json::json!(true);
                s
            })
        });

        let c2 = counter.clone();
        graph.add_node("parallel_b", move |s: serde_json::Value| {
            let c = c2.clone();
            Box::pin(async move {
                c.fetch_add(1, Ordering::SeqCst);
                let mut s = s;
                s["b"] = serde_json::json!(true);
                s
            })
        });

        graph.add_node("merge", |s| Box::pin(async { s }));

        graph.set_entry_point("entry");
        graph.add_edge("entry", "parallel_a");
        graph.add_edge("entry", "parallel_b");
        graph.add_edge("parallel_a", "merge");
        graph.add_edge("parallel_b", "merge");
        graph.add_edge("merge", END);

        let compiled = graph.compile().unwrap();
        let _result = compiled.invoke(serde_json::json!({})).await.unwrap();

        assert_eq!(
            counter.load(Ordering::SeqCst),
            2,
            "both parallel nodes should execute"
        );
    }

    #[tokio::test]
    async fn parallel_fan_out_with_merge() {
        let mut graph = StateGraph::<serde_json::Value>::new();

        graph.add_node("entry", |s| Box::pin(async { s }));
        graph.add_node("add_a", |s: serde_json::Value| {
            Box::pin(async move {
                let mut s = s;
                s["a"] = serde_json::json!(true);
                s
            })
        });
        graph.add_node("add_b", |s: serde_json::Value| {
            Box::pin(async move {
                let mut s = s;
                s["b"] = serde_json::json!(true);
                s
            })
        });
        graph.add_node("final", |s| Box::pin(async { s }));

        graph.set_entry_point("entry");
        graph.add_edge("entry", "add_a");
        graph.add_edge("entry", "add_b");
        graph.add_edge("add_a", "final");
        graph.add_edge("add_b", "final");
        graph.add_edge("final", END);

        graph.set_merge(|base, outputs| {
            let mut merged = base;
            for output in outputs {
                if let (Some(m), Some(o)) = (merged.as_object_mut(), output.as_object()) {
                    for (k, v) in o {
                        m.insert(k.clone(), v.clone());
                    }
                }
            }
            merged
        });

        let compiled = graph.compile().unwrap();
        let result = compiled.invoke(serde_json::json!({})).await.unwrap();

        assert_eq!(result["a"], true);
        assert_eq!(result["b"], true);
    }

    #[tokio::test]
    async fn three_node_chain() {
        let mut graph = StateGraph::new();

        graph.add_node("a", |state: serde_json::Value| {
            Box::pin(async move {
                let mut s = state;
                s["order"] = json!("a");
                s
            })
        });
        graph.add_node("b", |state: serde_json::Value| {
            Box::pin(async move {
                let mut s = state;
                let prev = s["order"].as_str().unwrap_or("").to_string();
                s["order"] = json!(format!("{},b", prev));
                s
            })
        });
        graph.add_node("c", |state: serde_json::Value| {
            Box::pin(async move {
                let mut s = state;
                let prev = s["order"].as_str().unwrap_or("").to_string();
                s["order"] = json!(format!("{},c", prev));
                s
            })
        });

        graph.set_entry_point("a");
        graph.add_edge("a", "b");
        graph.add_edge("b", "c");
        graph.add_edge("c", END);

        let compiled = graph.compile().unwrap();
        let result = compiled.invoke(json!({})).await.unwrap();
        assert_eq!(result["order"], "a,b,c");
    }

    // -- New tests for Send, events, checkpoint --

    #[tokio::test]
    async fn send_dynamic_routing() {
        use crate::graph::{NodeResult, SendTo};

        let mut graph = StateGraph::<serde_json::Value>::new();

        // Router node uses Send to fan out dynamically
        graph.add_routing_node("router", |state: serde_json::Value| {
            Box::pin(async move {
                let items = state["items"].as_array().cloned().unwrap_or_default();
                let sends: Vec<SendTo<serde_json::Value>> = items
                    .into_iter()
                    .map(|item| SendTo {
                        node: "worker".into(),
                        state: json!({ "item": item }),
                    })
                    .collect();
                NodeResult::Sends(sends)
            })
        });

        graph.add_node("worker", |state: serde_json::Value| {
            Box::pin(async move {
                let mut s = state;
                let item = s["item"].clone();
                s["processed"] = json!(format!("done:{}", item));
                s
            })
        });

        graph.set_entry_point("router");
        graph.add_edge("worker", END);

        graph.set_merge(|_base, outputs| {
            let results: Vec<String> = outputs
                .iter()
                .filter_map(|o| o["processed"].as_str().map(String::from))
                .collect();
            json!({ "results": results })
        });

        let compiled = graph.compile().unwrap();
        let result = compiled
            .invoke(json!({ "items": [1, 2, 3] }))
            .await
            .unwrap();

        let results = result["results"].as_array().unwrap();
        assert_eq!(results.len(), 3);
    }

    #[tokio::test]
    async fn invoke_with_events() {
        let mut graph = StateGraph::new();
        graph.add_node("step1", |s: serde_json::Value| {
            Box::pin(async move {
                let mut s = s;
                s["done"] = json!(true);
                s
            })
        });
        graph.set_entry_point("step1");
        graph.add_edge("step1", END);

        let compiled = graph.compile().unwrap();

        let (tx, mut rx) = mpsc::unbounded_channel();
        let config = InvokeConfig::default().with_events(tx);

        let _result = compiled.invoke_with_config(json!({}), config).await.unwrap();

        let mut events = Vec::new();
        while let Ok(event) = rx.try_recv() {
            events.push(event);
        }

        assert!(events.iter().any(|e| matches!(e, GraphEvent::SuperstepStart { step: 1 })));
        assert!(events.iter().any(|e| matches!(e, GraphEvent::NodeStart { node, .. } if node == "step1")));
        assert!(events.iter().any(|e| matches!(e, GraphEvent::NodeEnd { node, .. } if node == "step1")));
        assert!(events.iter().any(|e| matches!(e, GraphEvent::Completed { .. })));
    }

    #[tokio::test]
    async fn invoke_with_checkpoint() {
        let mut graph = StateGraph::new();
        graph.add_node("s1", |s: serde_json::Value| {
            Box::pin(async move {
                let mut s = s;
                s["s1"] = json!(true);
                s
            })
        });
        graph.add_node("s2", |s: serde_json::Value| {
            Box::pin(async move {
                let mut s = s;
                s["s2"] = json!(true);
                s
            })
        });
        graph.set_entry_point("s1");
        graph.add_edge("s1", "s2");
        graph.add_edge("s2", END);

        let compiled = graph.compile().unwrap();

        let saver = Arc::new(crate::checkpoint::InMemoryCheckpointSaver::new());
        let config = InvokeConfig::default()
            .with_checkpoint(saver.clone())
            .with_checkpoint_id("test");

        let _result = compiled.invoke_with_config(json!({}), config).await.unwrap();

        // Should have 2 checkpoints (one per superstep)
        let ids = saver.list().await.unwrap();
        assert_eq!(ids.len(), 2);
        assert!(ids.iter().any(|id| id == "test-step-1"));
        assert!(ids.iter().any(|id| id == "test-step-2"));

        // Verify checkpoint content
        let cp2 = saver.load("test-step-2").await.unwrap().unwrap();
        assert_eq!(cp2["s1"], true);
        assert_eq!(cp2["s2"], true);
    }
}
