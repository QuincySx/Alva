// INPUT:  CompiledGraph, InvokeConfig, GraphEvent from types, crate::graph::*
// OUTPUT: invoke(), invoke_with_config(), resolve_next_nodes()
// POS:    Pregel-style BSP superstep loop and edge resolution.

use alva_kernel_abi::AgentError;

use crate::graph::{Edge, NodeResult, END};

use super::types::{CompiledGraph, GraphEvent, InvokeConfig};

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

    /// Collect all next nodes from edges matching `current`.
    pub(crate) fn resolve_next_nodes(&self, current: &str, state: &S) -> Result<Vec<String>, AgentError> {
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

#[cfg(test)]
mod tests {
    //! Tests for `CompiledGraph::resolve_next_nodes` — the pure-sync
    //! edge-routing slice shared by the invoke loop and execute_sends.
    //!
    //! Three load-bearing contracts:
    //!
    //! 1. **END fallback when no edges match** — a node with no
    //!    outgoing edges is a leaf; resolve must return `vec![END]`
    //!    (NOT panic, NOT Err). A refactor that changed this fallback
    //!    would crash every otherwise-valid terminal node.
    //!
    //! 2. **Router receives `&state`, not a clone or `&from`** — the
    //!    Conditional router closure is user-defined and routinely
    //!    inspects state fields to decide the next branch. A typo
    //!    that called `router(&current)` instead of `router(state)`
    //!    would silently route every conditional edge identically.
    //!
    //! 3. **All matching edges accumulate, including mixed types** —
    //!    fan-out via multiple Direct edges + Conditional edges from
    //!    the same node is the static-routing fan-out primitive.
    use super::*;
    use crate::graph::Edge;
    use crate::pregel::CompiledGraph;
    use std::collections::HashMap;

    type State = i32;

    fn empty_graph() -> CompiledGraph<State> {
        CompiledGraph {
            nodes: HashMap::new(),
            edges: Vec::new(),
            entry_point: "n".to_string(),
            merge_fn: None,
        }
    }

    // -- END fallback ----------------------------------------------------

    #[test]
    fn no_matching_edges_returns_end_fallback() {
        // Leaf node contract: no outgoing edges → vec![END]. A change
        // to panic / Err / empty vec would crash every terminal node.
        let g = empty_graph();
        let next = g.resolve_next_nodes("leaf", &0).expect("must not error");
        assert_eq!(next, vec![END.to_string()]);
    }

    #[test]
    fn edges_for_other_nodes_do_not_match_returns_end_fallback() {
        // Pin: edges matching OTHER `from` values are ignored — they
        // don't accidentally route us. Verifies the `if from == current`
        // guard, not just edges.is_empty().
        let mut g = empty_graph();
        g.edges.push(Edge::Direct { from: "other".into(), to: "x".into() });
        g.edges.push(Edge::Direct { from: "another".into(), to: "y".into() });
        let next = g.resolve_next_nodes("me", &0).unwrap();
        assert_eq!(next, vec![END.to_string()], "no edge matches `me` → END fallback");
    }

    // -- Direct edges ----------------------------------------------------

    #[test]
    fn single_direct_edge_returns_target() {
        let mut g = empty_graph();
        g.edges.push(Edge::Direct { from: "a".into(), to: "b".into() });
        let next = g.resolve_next_nodes("a", &0).unwrap();
        assert_eq!(next, vec!["b".to_string()]);
    }

    #[test]
    fn multiple_direct_edges_from_same_source_collected_in_order() {
        // Pin: fan-out via several Direct edges from one source →
        // all targets collected in declaration order. resolve_next_nodes
        // itself does NOT sort/dedup (that's the caller's job, see
        // pregel/parallel.rs); a refactor that started sorting here
        // would silently change scheduling order in the single-send
        // fast path.
        let mut g = empty_graph();
        g.edges.push(Edge::Direct { from: "src".into(), to: "z".into() });
        g.edges.push(Edge::Direct { from: "src".into(), to: "a".into() });
        g.edges.push(Edge::Direct { from: "src".into(), to: "m".into() });
        let next = g.resolve_next_nodes("src", &0).unwrap();
        assert_eq!(
            next,
            vec!["z".to_string(), "a".to_string(), "m".to_string()],
            "must preserve declaration order (no sort/dedup at this layer)"
        );
    }

    #[test]
    fn end_as_direct_target_passes_through_verbatim() {
        // END is a sentinel literal, NOT special-cased in this layer —
        // it's just a string the layer above (invoke loop) recognises.
        // Pin: resolve_next_nodes treats it as any other target.
        let mut g = empty_graph();
        g.edges.push(Edge::Direct { from: "n".into(), to: END.into() });
        let next = g.resolve_next_nodes("n", &0).unwrap();
        assert_eq!(next, vec![END.to_string()]);
    }

    // -- Conditional edges -----------------------------------------------

    #[test]
    fn conditional_edge_router_is_called_with_state_reference() {
        // CRITICAL PIN: router receives the *state*, not the `current`
        // name or a clone. A refactor that passed e.g. `router(&current)`
        // would silently make every conditional edge route to the
        // same target regardless of state.
        let mut g = empty_graph();
        g.edges.push(Edge::Conditional {
            from: "n".into(),
            router: Box::new(|s: &State| format!("branch-{s}")),
        });
        let next = g.resolve_next_nodes("n", &42).unwrap();
        assert_eq!(next, vec!["branch-42".to_string()]);
    }

    #[test]
    fn mixed_direct_and_conditional_edges_all_collected() {
        // Pin: both Edge variants accumulate from the same source,
        // in declaration order. Useful for "default + override" routing
        // patterns.
        let mut g = empty_graph();
        g.edges.push(Edge::Direct { from: "n".into(), to: "static_first".into() });
        g.edges.push(Edge::Conditional {
            from: "n".into(),
            router: Box::new(|_| "dynamic_second".into()),
        });
        g.edges.push(Edge::Direct { from: "n".into(), to: "static_third".into() });
        let next = g.resolve_next_nodes("n", &0).unwrap();
        assert_eq!(
            next,
            vec![
                "static_first".to_string(),
                "dynamic_second".to_string(),
                "static_third".to_string(),
            ]
        );
    }

    #[test]
    fn unrelated_conditional_edges_do_not_invoke_router() {
        // Pin: if a Conditional edge's `from` doesn't match, its
        // router MUST NOT be called. A regression that swapped the
        // `if from == current` guard for unconditional invocation
        // could trigger user-side side effects in routers.
        use std::sync::atomic::{AtomicBool, Ordering};
        use std::sync::Arc;

        let called = Arc::new(AtomicBool::new(false));
        let called_inner = Arc::clone(&called);

        let mut g = empty_graph();
        g.edges.push(Edge::Conditional {
            from: "other".into(),
            router: Box::new(move |_| {
                called_inner.store(true, Ordering::SeqCst);
                "nope".to_string()
            }),
        });

        let next = g.resolve_next_nodes("me", &0).unwrap();
        assert_eq!(next, vec![END.to_string()]);
        assert!(
            !called.load(Ordering::SeqCst),
            "router for unrelated `from` MUST NOT be invoked"
        );
    }
}
