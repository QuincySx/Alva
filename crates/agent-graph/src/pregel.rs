/// Pregel-style BSP execution engine for compiled graphs.

use std::collections::HashMap;

use agent_types::AgentError;

use crate::graph::{Edge, NodeFn, END};

/// A compiled, executable graph produced by [`StateGraph::compile`](crate::StateGraph::compile).
///
/// State flows through nodes sequentially; edges (direct or conditional)
/// determine the next node after each step. Execution terminates when the
/// current node resolves to `END` or when no outgoing edge is found.
pub struct CompiledGraph<S> {
    pub(crate) nodes: HashMap<String, NodeFn<S>>,
    pub(crate) edges: Vec<Edge<S>>,
    pub(crate) entry_point: String,
}

impl<S> std::fmt::Debug for CompiledGraph<S> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("CompiledGraph")
            .field("entry_point", &self.entry_point)
            .field("node_count", &self.nodes.len())
            .field("edge_count", &self.edges.len())
            .finish()
    }
}

impl<S: Send + 'static> CompiledGraph<S> {
    /// Execute the graph starting from the entry point.
    ///
    /// `input` is the initial state value; each node receives the current state
    /// and returns the (possibly modified) state. The final state is returned
    /// when execution reaches `END`.
    ///
    /// When multiple edges leave a single source node, the corresponding target
    /// nodes are executed concurrently as a BSP-style "superstep". The `Clone`
    /// bound is required so each parallel node can receive its own copy of the
    /// state.
    pub async fn invoke(&self, input: S) -> Result<S, AgentError>
    where
        S: Clone,
    {
        let mut state = input;
        let mut current_nodes = vec![self.entry_point.clone()];

        loop {
            // Filter out END
            current_nodes.retain(|n| n != END);
            if current_nodes.is_empty() {
                return Ok(state);
            }

            if current_nodes.len() == 1 {
                // Single node — execute directly (backward-compatible fast path)
                let node_name = &current_nodes[0];
                let node_fn = self.nodes.get(node_name).ok_or_else(|| {
                    AgentError::ConfigError(format!("Node not found: {}", node_name))
                })?;
                state = node_fn(state).await;
                current_nodes = self.resolve_next_nodes(&current_nodes[0], &state)?;
            } else {
                // Parallel superstep — execute all nodes concurrently
                let mut join_set = tokio::task::JoinSet::new();
                for node_name in &current_nodes {
                    let node_fn = self.nodes.get(node_name).ok_or_else(|| {
                        AgentError::ConfigError(format!("Node not found: {}", node_name))
                    })?;
                    let s = state.clone();
                    let fut = node_fn(s);
                    let name = node_name.clone();
                    join_set.spawn(async move { (name, fut.await) });
                }

                // Collect results — last result wins for now (simple merge)
                // TODO: integrate Channel system for proper state merging
                while let Some(Ok((_name, result))) = join_set.join_next().await {
                    state = result;
                }

                // Collect next nodes from all executed nodes
                let mut next = Vec::new();
                for node in &current_nodes {
                    next.extend(self.resolve_next_nodes(node, &state)?);
                }
                next.sort();
                next.dedup();
                current_nodes = next;
            }
        }
    }

    /// Determine the next node to execute given the current node and state.
    ///
    /// Resolution order:
    /// 1. Direct edges matching `current` (first match wins)
    /// 2. Conditional edges matching `current` (first match wins)
    /// 3. If no edge found, default to `END`
    fn resolve_next(&self, current: &str, state: &S) -> Result<String, AgentError> {
        for edge in &self.edges {
            match edge {
                Edge::Direct { from, to } if from == current => {
                    return Ok(to.clone());
                }
                Edge::Conditional { from, router } if from == current => {
                    return Ok(router(state));
                }
                _ => continue,
            }
        }
        // No edge found — default to END
        Ok(END.to_string())
    }

    /// Collect ALL next nodes from edges matching `current`.
    ///
    /// Unlike [`resolve_next()`](Self::resolve_next) which short-circuits on
    /// the first matching edge, this method gathers every target — enabling
    /// fan-out to multiple parallel nodes in a single superstep.
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
        use std::sync::Arc;

        let counter = Arc::new(AtomicU32::new(0));
        let mut graph = StateGraph::<serde_json::Value>::new();

        // Entry node (fan-out point)
        graph.add_node("entry", |s| Box::pin(async { s }));

        // Two parallel nodes
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

        // Fan-in node
        graph.add_node("merge", |s| Box::pin(async { s }));

        graph.set_entry_point("entry");
        // Two edges from entry = fan-out
        graph.add_edge("entry", "parallel_a");
        graph.add_edge("entry", "parallel_b");
        graph.add_edge("parallel_a", "merge");
        graph.add_edge("parallel_b", "merge");
        graph.add_edge("merge", END);

        let compiled = graph.compile().unwrap();
        let result = compiled.invoke(serde_json::json!({})).await.unwrap();

        // Both nodes executed
        assert_eq!(
            counter.load(Ordering::SeqCst),
            2,
            "both parallel nodes should execute"
        );
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
}
