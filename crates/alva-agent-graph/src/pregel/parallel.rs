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
