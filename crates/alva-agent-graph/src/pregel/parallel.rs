// INPUT:  CompiledGraph, InvokeConfig, GraphEvent from types
// OUTPUT: execute_sends() — JoinSet fan-out logic for Send-based dynamic routing
// POS:    Parallel execution of Send targets with merge and recursive handling.

use alva_kernel_abi::AgentError;

use crate::graph::NodeResult;

use super::types::{CompiledGraph, GraphEvent, InvokeConfig};

impl<S: Send + 'static> CompiledGraph<S> {
    /// Execute Send targets in parallel, merge results, return next nodes.
    pub(crate) async fn execute_sends(
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
}

#[cfg(test)]
mod tests {
    //! Tests for `CompiledGraph::execute_sends`.
    //!
    //! Two structurally different branches share one entry point:
    //! - `sends.len() == 1` → fast path, executes inline, recurses on
    //!   nested `NodeResult::Sends`.
    //! - `sends.len() != 1` (0 or ≥2) → JoinSet parallel fan-out, merges
    //!   via `merge_fn` (or last-update fallback), sorts + dedups next
    //!   nodes, **silently drops nested Sends** (logs `warn!` only).
    //!
    //! That asymmetry is the load-bearing pin: a refactor that
    //! "unified" the two branches into recursion-or-drop everywhere
    //! would silently change runtime behavior for either single- or
    //! multi-send graphs.
    use super::*;
    use crate::graph::{Edge, SendTo};
    use crate::pregel::{CompiledGraph, InvokeConfig};
    use std::collections::HashMap;
    use tokio::sync::mpsc;

    type State = i32;
    type Node = crate::graph::NodeFn<State>;

    // Helpers ----------------------------------------------------------------

    /// Wrap an async-returning closure into a NodeFn.
    fn node_fn<F>(f: F) -> Node
    where
        F: Fn(State) -> NodeResult<State> + Send + Sync + 'static,
    {
        Box::new(move |s: State| {
            let r = f(s);
            Box::pin(async move { r })
        })
    }

    fn empty_graph() -> CompiledGraph<State> {
        CompiledGraph {
            nodes: HashMap::new(),
            edges: Vec::new(),
            entry_point: "n1".to_string(),
            merge_fn: None,
        }
    }

    // -- Single send (fast path) -----------------------------------------

    #[tokio::test]
    async fn single_send_executes_node_and_updates_state() {
        // Pin: single-send path actually invokes the node function
        // and writes the returned state back to the caller's `&mut state`.
        let mut g = empty_graph();
        g.nodes
            .insert("worker".to_string(), node_fn(|s| NodeResult::Update(s + 10)));
        // edge worker → END will be created implicitly by resolve_next_nodes
        // (no outgoing edge → returns vec![END]).
        let cfg = InvokeConfig::default();
        let mut state: State = 5;
        let next = g
            .execute_sends(
                vec![SendTo {
                    node: "worker".into(),
                    state: 42,
                }],
                &mut state,
                &cfg,
                0,
            )
            .await
            .expect("single send must succeed");
        assert_eq!(state, 52, "node received 42, returned 52, written to state");
        assert_eq!(next, vec![crate::graph::END.to_string()]);
    }

    #[tokio::test]
    async fn single_send_missing_target_returns_config_error() {
        // Pin: target lookup miss MUST surface as ConfigError, NOT panic
        // or silently no-op (otherwise a typo in a routing node's
        // `SendTo` target would silently swallow that branch).
        let g = empty_graph();
        let cfg = InvokeConfig::default();
        let mut state: State = 0;
        let err = g
            .execute_sends(
                vec![SendTo {
                    node: "nope".into(),
                    state: 0,
                }],
                &mut state,
                &cfg,
                0,
            )
            .await
            .expect_err("missing target must error");
        let msg = err.to_string();
        assert!(
            msg.contains("Send target node not found") && msg.contains("nope"),
            "error must name the missing target: {msg}"
        );
    }

    #[tokio::test]
    async fn single_send_with_nested_sends_recurses_via_box_pin() {
        // CRITICAL CONTRAST PIN: in the single-send branch, nested Sends
        // are RECURSIVELY EXECUTED (via Box::pin to allow async recursion).
        // In the multi-send branch the same nested Sends are DROPPED
        // (warn-only). This test pins the single-send recursive behavior.
        let mut g = empty_graph();
        g.nodes.insert(
            "router".to_string(),
            node_fn(|_| {
                NodeResult::Sends(vec![SendTo {
                    node: "leaf".into(),
                    state: 100,
                }])
            }),
        );
        g.nodes
            .insert("leaf".to_string(), node_fn(|s| NodeResult::Update(s + 1)));
        let cfg = InvokeConfig::default();
        let mut state: State = -1;
        let next = g
            .execute_sends(
                vec![SendTo {
                    node: "router".into(),
                    state: 0,
                }],
                &mut state,
                &cfg,
                0,
            )
            .await
            .expect("nested single send must recurse");
        assert_eq!(state, 101, "leaf produced 101 after recursion");
        assert_eq!(next, vec![crate::graph::END.to_string()]);
    }

    // -- Multi send (JoinSet path) ---------------------------------------

    #[tokio::test]
    async fn multi_send_with_merge_fn_combines_updates() {
        // Pin: when merge_fn is set, it MUST be invoked with the base
        // state + the Vec of all parallel updates. Sum-merge here makes
        // ordering noise irrelevant.
        let mut g = empty_graph();
        g.nodes
            .insert("a".to_string(), node_fn(|s| NodeResult::Update(s + 1)));
        g.nodes
            .insert("b".to_string(), node_fn(|s| NodeResult::Update(s + 10)));
        // edges so resolve_next_nodes returns something deterministic
        g.edges.push(Edge::Direct { from: "a".into(), to: "end_a".into() });
        g.edges.push(Edge::Direct { from: "b".into(), to: "end_b".into() });
        g.merge_fn = Some(Box::new(|base, updates: Vec<State>| {
            // base + sum of deltas (each update was base + N, recompute deltas)
            let total: State = updates.iter().map(|u| u - base).sum();
            base + total
        }));
        let cfg = InvokeConfig::default();
        let mut state: State = 1000;
        let mut next = g
            .execute_sends(
                vec![
                    SendTo { node: "a".into(), state: 1000 },
                    SendTo { node: "b".into(), state: 1000 },
                ],
                &mut state,
                &cfg,
                0,
            )
            .await
            .expect("multi-send with merge must succeed");
        assert_eq!(state, 1011, "merge_fn must combine: 1000 + 1 + 10");
        next.sort();
        assert_eq!(next, vec!["end_a".to_string(), "end_b".to_string()]);
    }

    #[tokio::test]
    async fn multi_send_without_merge_fn_uses_last_update_fallback() {
        // Pin: without merge_fn the code path takes `updates.into_iter()
        // .last().unwrap_or_else(|| state.clone())`. That fallback is a
        // documented contract — silent change to first-update or to
        // panic would break every Sends-without-merge graph.
        //
        // Cannot pin exact value (JoinSet completion order is
        // non-deterministic) — instead pin that the final state is ONE
        // of the parallel outputs (NOT base, NOT a merge, NOT a panic).
        let mut g = empty_graph();
        g.nodes
            .insert("a".to_string(), node_fn(|_| NodeResult::Update(111)));
        g.nodes
            .insert("b".to_string(), node_fn(|_| NodeResult::Update(222)));
        // merge_fn is None
        let cfg = InvokeConfig::default();
        let mut state: State = 0;
        g.execute_sends(
            vec![
                SendTo { node: "a".into(), state: 0 },
                SendTo { node: "b".into(), state: 0 },
            ],
            &mut state,
            &cfg,
            0,
        )
        .await
        .expect("multi-send no-merge must succeed");
        assert!(
            state == 111 || state == 222,
            "fallback must surface ONE of the parallel updates (got {state})"
        );
    }

    #[tokio::test]
    async fn multi_send_next_nodes_sorted_and_deduped() {
        // Pin: next_nodes from N parallel sends → sort+dedup. A
        // refactor that dropped either operation would surface as:
        //  - sorting: caller sees random scheduling order per run
        //  - dedup: same target enqueued N times → wasted re-execution
        let mut g = empty_graph();
        g.nodes
            .insert("a".to_string(), node_fn(|s| NodeResult::Update(s)));
        g.nodes
            .insert("b".to_string(), node_fn(|s| NodeResult::Update(s)));
        // both fan out to the SAME target — must dedup to 1 entry
        g.edges.push(Edge::Direct { from: "a".into(), to: "shared".into() });
        g.edges.push(Edge::Direct { from: "b".into(), to: "shared".into() });
        let cfg = InvokeConfig::default();
        let mut state: State = 0;
        let next = g
            .execute_sends(
                vec![
                    SendTo { node: "a".into(), state: 0 },
                    SendTo { node: "b".into(), state: 0 },
                ],
                &mut state,
                &cfg,
                0,
            )
            .await
            .unwrap();
        assert_eq!(next, vec!["shared".to_string()], "dedup collapsed both routes");
    }

    #[tokio::test]
    async fn multi_send_nested_sends_are_dropped_not_recursed() {
        // CRITICAL ASYMMETRY PIN: in the parallel branch, a node
        // returning NodeResult::Sends is `tracing::warn!`-logged and
        // DROPPED — NOT recursed (compare with the single-send branch
        // which DOES recurse via Box::pin). The comment "nested Sends
        // from parallel execution are dropped" is the load-bearing
        // contract; a future refactor that tried to support it
        // recursively would deadlock the JoinSet.
        let mut g = empty_graph();
        g.nodes.insert(
            "router".to_string(),
            node_fn(|_| {
                NodeResult::Sends(vec![SendTo {
                    node: "would_be_recursed".into(),
                    state: 999,
                }])
            }),
        );
        g.nodes
            .insert("worker".to_string(), node_fn(|s| NodeResult::Update(s + 5)));
        g.edges.push(Edge::Direct {
            from: "worker".into(),
            to: "end_worker".into(),
        });
        // NOTE: router has no outgoing edge defined; since its update
        // path is skipped (Sends → drop), it contributes nothing to
        // next_nodes (resolve_next_nodes never called on it).
        let cfg = InvokeConfig::default();
        let mut state: State = 0;
        let next = g
            .execute_sends(
                vec![
                    SendTo { node: "router".into(), state: 0 },
                    SendTo { node: "worker".into(), state: 0 },
                ],
                &mut state,
                &cfg,
                0,
            )
            .await
            .expect("multi-send with one nested-Sends must NOT panic");
        // worker's update reaches state via fallback (no merge, single update)
        assert_eq!(state, 5, "worker update applied; router's Sends were dropped");
        assert_eq!(
            next,
            vec!["end_worker".to_string()],
            "router contributed no next_nodes (its Sends were dropped)"
        );
    }

    #[tokio::test]
    async fn multi_send_emits_node_start_and_end_events_per_target() {
        // Pin: each Send target gets BOTH a NodeStart and a NodeEnd
        // event in the parallel branch. A refactor that dropped either
        // emit would silently break observability subscribers.
        let mut g = empty_graph();
        g.nodes
            .insert("a".to_string(), node_fn(|s| NodeResult::Update(s)));
        g.nodes
            .insert("b".to_string(), node_fn(|s| NodeResult::Update(s)));
        let (tx, mut rx) = mpsc::unbounded_channel::<GraphEvent>();
        let cfg = InvokeConfig::default().with_events(tx);
        let mut state: State = 0;
        g.execute_sends(
            vec![
                SendTo { node: "a".into(), state: 0 },
                SendTo { node: "b".into(), state: 0 },
            ],
            &mut state,
            &cfg,
            7,
        )
        .await
        .unwrap();

        let mut starts: Vec<String> = Vec::new();
        let mut ends: Vec<String> = Vec::new();
        while let Ok(ev) = rx.try_recv() {
            match ev {
                GraphEvent::NodeStart { node, step } => {
                    assert_eq!(step, 7);
                    starts.push(node);
                }
                GraphEvent::NodeEnd { node, step } => {
                    assert_eq!(step, 7);
                    ends.push(node);
                }
                other => panic!("unexpected event: {other:?}"),
            }
        }
        starts.sort();
        ends.sort();
        assert_eq!(starts, vec!["a".to_string(), "b".to_string()]);
        assert_eq!(ends, vec!["a".to_string(), "b".to_string()]);
    }
}
