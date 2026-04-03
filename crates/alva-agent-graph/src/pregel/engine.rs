// INPUT:  CompiledGraph, InvokeConfig, GraphEvent from types, crate::graph::*
// OUTPUT: invoke(), invoke_with_config(), resolve_next_nodes()
// POS:    Pregel-style BSP superstep loop and edge resolution.

use alva_types::AgentError;

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
