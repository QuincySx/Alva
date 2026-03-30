// INPUT:  std::collections::HashMap, std::future::Future, std::pin::Pin, alva_types::AgentError, crate::pregel::CompiledGraph
// OUTPUT: pub const START, pub const END, pub struct StateGraph, pub enum NodeResult, pub struct Send
// POS:    StateGraph builder with Send/Command-style dynamic routing support.

use std::collections::HashMap;
use std::future::Future;
use std::pin::Pin;

use alva_types::AgentError;

use crate::pregel::CompiledGraph;

pub const START: &str = "__start__";
pub const END: &str = "__end__";

pub(crate) type BoxFuture<T> = Pin<Box<dyn Future<Output = T> + Send>>;
pub(crate) type NodeFn<S> = Box<dyn Fn(S) -> BoxFuture<NodeResult<S>> + Send + Sync>;
pub(crate) type RouterFn<S> = Box<dyn Fn(&S) -> String + Send + Sync>;
pub(crate) type MergeFn<S> = Box<dyn Fn(S, Vec<S>) -> S + Send + Sync>;

pub(crate) enum Edge<S> {
    Direct { from: String, to: String },
    Conditional { from: String, router: RouterFn<S> },
}

// ---------------------------------------------------------------------------
// Send & NodeResult — dynamic routing primitives
// ---------------------------------------------------------------------------

/// Dynamic fan-out: route to a specific node with a specific state.
///
/// Returned by nodes via `NodeResult::Sends` to create parallel tasks
/// at runtime. Unlike static edges, the number and targets of Sends are
/// determined dynamically by the node's logic.
///
/// ```rust,ignore
/// // Node decides at runtime to fan out to 3 workers
/// NodeResult::Sends(items.into_iter().map(|item| {
///     SendTo { node: "process".into(), state: item_state }
/// }).collect())
/// ```
pub struct SendTo<S> {
    pub node: String,
    pub state: S,
}

/// What a node returns after execution.
///
/// - `Update(S)`: normal state update (backward compatible with simple nodes)
/// - `Sends(Vec<Send<S>>)`: dynamic fan-out to specific nodes with specific states
pub enum NodeResult<S> {
    /// Normal: return the updated state, continue via edges.
    Update(S),
    /// Dynamic routing: fan out to specific nodes, bypassing edge definitions.
    /// Each Send runs in parallel as a separate superstep task.
    Sends(Vec<SendTo<S>>),
}

/// Convenience: convert a raw state into `NodeResult::Update`.
impl<S> From<S> for NodeResult<S> {
    fn from(state: S) -> Self {
        NodeResult::Update(state)
    }
}

// ---------------------------------------------------------------------------
// StateGraph builder
// ---------------------------------------------------------------------------

/// Builder for constructing a state graph.
///
/// Add nodes (async functions that transform state) and edges (transitions
/// between nodes), then call `compile()` to produce an executable
/// `CompiledGraph`.
pub struct StateGraph<S> {
    nodes: HashMap<String, NodeFn<S>>,
    edges: Vec<Edge<S>>,
    entry_point: Option<String>,
    merge_fn: Option<MergeFn<S>>,
}

impl<S: Send + 'static> StateGraph<S> {
    pub fn new() -> Self {
        Self {
            nodes: HashMap::new(),
            edges: Vec::new(),
            entry_point: None,
            merge_fn: None,
        }
    }

    /// Register a node with a simple `Fn(S) -> Future<S>` signature.
    ///
    /// This is the backward-compatible API. The closure is internally wrapped
    /// to return `NodeResult::Update(S)`.
    pub fn add_node(
        &mut self,
        name: &str,
        node: impl Fn(S) -> BoxFuture<S> + Send + Sync + 'static,
    ) {
        let wrapped = move |s: S| -> BoxFuture<NodeResult<S>> {
            let fut = node(s);
            Box::pin(async move { NodeResult::Update(fut.await) })
        };
        self.nodes.insert(name.to_string(), Box::new(wrapped));
    }

    /// Register a node that can return `NodeResult` for dynamic routing.
    ///
    /// Use this when the node needs to:
    /// - Fan out to specific nodes with specific states (`NodeResult::Sends`)
    /// - Choose its next target dynamically without pre-defined conditional edges
    pub fn add_routing_node(
        &mut self,
        name: &str,
        node: impl Fn(S) -> BoxFuture<NodeResult<S>> + Send + Sync + 'static,
    ) {
        self.nodes.insert(name.to_string(), Box::new(node));
    }

    /// Add a direct (unconditional) edge from one node to another.
    pub fn add_edge(&mut self, from: &str, to: &str) {
        self.edges.push(Edge::Direct {
            from: from.to_string(),
            to: to.to_string(),
        });
    }

    /// Add a conditional edge whose target is determined at runtime by `router`.
    pub fn add_conditional_edge(
        &mut self,
        from: &str,
        router: impl Fn(&S) -> String + Send + Sync + 'static,
    ) {
        self.edges.push(Edge::Conditional {
            from: from.to_string(),
            router: Box::new(router),
        });
    }

    /// Set the node that execution begins at.
    pub fn set_entry_point(&mut self, name: &str) {
        self.entry_point = Some(name.to_string());
    }

    /// Set a merge function for combining parallel node outputs.
    ///
    /// When multiple nodes execute in a parallel superstep, each receives a
    /// clone of the current state. The merge function receives the original
    /// base state and a `Vec` of all node outputs, and must return a single
    /// combined state.
    pub fn set_merge(
        &mut self,
        merge: impl Fn(S, Vec<S>) -> S + Send + Sync + 'static,
    ) {
        self.merge_fn = Some(Box::new(merge));
    }

    /// Validate the graph and produce a `CompiledGraph` ready for execution.
    pub fn compile(self) -> Result<CompiledGraph<S>, AgentError> {
        let entry_point = self.entry_point.ok_or_else(|| {
            AgentError::ConfigError("Entry point not set".to_string())
        })?;

        if !self.nodes.contains_key(&entry_point) {
            return Err(AgentError::ConfigError(format!(
                "Entry point '{}' does not exist as a node",
                entry_point
            )));
        }

        for edge in &self.edges {
            match edge {
                Edge::Direct { from, to } => {
                    if from != START && !self.nodes.contains_key(from.as_str()) {
                        return Err(AgentError::ConfigError(format!(
                            "Edge source '{}' is not a registered node",
                            from
                        )));
                    }
                    if to != END && !self.nodes.contains_key(to.as_str()) {
                        return Err(AgentError::ConfigError(format!(
                            "Edge target '{}' is not a registered node",
                            to
                        )));
                    }
                }
                Edge::Conditional { from, .. } => {
                    if from != START && !self.nodes.contains_key(from.as_str()) {
                        return Err(AgentError::ConfigError(format!(
                            "Conditional edge source '{}' is not a registered node",
                            from
                        )));
                    }
                }
            }
        }

        let mut referenced_nodes = std::collections::HashSet::new();
        referenced_nodes.insert(entry_point.clone());
        for edge in &self.edges {
            if let Edge::Direct { to, .. } = edge {
                if to != END {
                    referenced_nodes.insert(to.clone());
                }
            }
        }

        for node_name in self.nodes.keys() {
            if !referenced_nodes.contains(node_name) {
                tracing::warn!(
                    node = %node_name,
                    "Node is not statically referenced by any direct edge or entry point; \
                     it may only be reachable via conditional routing or Send"
                );
            }
        }

        Ok(CompiledGraph {
            nodes: self.nodes,
            edges: self.edges,
            entry_point,
            merge_fn: self.merge_fn,
        })
    }
}

impl<S: Send + 'static> Default for StateGraph<S> {
    fn default() -> Self {
        Self::new()
    }
}
