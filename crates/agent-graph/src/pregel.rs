/// Pregel-style BSP execution engine for compiled graphs.

use std::collections::HashMap;

use agent_base::AgentError;

use crate::graph::{Edge, NodeFn, END};

/// A compiled, executable graph produced by [`StateGraph::compile`](crate::StateGraph::compile).
///
/// State flows through nodes sequentially; edges (direct or conditional)
/// determine the next node after each step. Execution terminates when the
/// current node resolves to `END` or when no outgoing edge is found.
pub struct CompiledGraph {
    pub(crate) nodes: HashMap<String, NodeFn>,
    pub(crate) edges: Vec<Edge>,
    pub(crate) entry_point: String,
}

impl std::fmt::Debug for CompiledGraph {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("CompiledGraph")
            .field("entry_point", &self.entry_point)
            .field("node_count", &self.nodes.len())
            .field("edge_count", &self.edges.len())
            .finish()
    }
}

impl CompiledGraph {
    /// Execute the graph starting from the entry point.
    ///
    /// `input` is the initial state value; each node receives the current state
    /// and returns the (possibly modified) state. The final state is returned
    /// when execution reaches `END`.
    pub async fn invoke(
        &self,
        input: serde_json::Value,
    ) -> Result<serde_json::Value, AgentError> {
        let mut state = input;
        let mut current_node = self.entry_point.clone();

        loop {
            if current_node == END {
                return Ok(state);
            }

            // Execute current node
            let node_fn = self.nodes.get(&current_node).ok_or_else(|| {
                AgentError::ConfigError(format!("Node not found: {}", current_node))
            })?;

            state = node_fn(state).await;

            // Resolve next node
            current_node = self.resolve_next(&current_node, &state)?;
        }
    }

    /// Determine the next node to execute given the current node and state.
    ///
    /// Resolution order:
    /// 1. Direct edges matching `current` (first match wins)
    /// 2. Conditional edges matching `current` (first match wins)
    /// 3. If no edge found, default to `END`
    fn resolve_next(
        &self,
        current: &str,
        state: &serde_json::Value,
    ) -> Result<String, AgentError> {
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
        let graph = StateGraph::new();
        let err = graph.compile().unwrap_err();
        assert!(err.to_string().contains("Entry point not set"));
    }

    #[test]
    fn compile_fails_with_invalid_entry_point() {
        let mut graph = StateGraph::new();
        graph.set_entry_point("nonexistent");
        let err = graph.compile().unwrap_err();
        assert!(err.to_string().contains("does not exist"));
    }

    #[test]
    fn compile_fails_with_invalid_edge_target() {
        let mut graph = StateGraph::new();
        graph.add_node("a", |s| Box::pin(async { s }));
        graph.set_entry_point("a");
        graph.add_edge("a", "nonexistent");
        let err = graph.compile().unwrap_err();
        assert!(err.to_string().contains("Edge target"));
    }

    #[test]
    fn compile_fails_with_invalid_edge_source() {
        let mut graph = StateGraph::new();
        graph.add_node("a", |s| Box::pin(async { s }));
        graph.set_entry_point("a");
        graph.add_edge("nonexistent", "a");
        let err = graph.compile().unwrap_err();
        assert!(err.to_string().contains("Edge source"));
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
