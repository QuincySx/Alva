/// StateGraph builder — defines nodes and edges, then compiles into a `CompiledGraph`.

use std::collections::HashMap;
use std::future::Future;
use std::pin::Pin;

use agent_types::AgentError;

use crate::pregel::CompiledGraph;

pub const START: &str = "__start__";
pub const END: &str = "__end__";

pub(crate) type BoxFuture<T> = Pin<Box<dyn Future<Output = T> + Send>>;
pub(crate) type NodeFn<S> = Box<dyn Fn(S) -> BoxFuture<S> + Send + Sync>;
pub(crate) type RouterFn<S> = Box<dyn Fn(&S) -> String + Send + Sync>;

pub(crate) enum Edge<S> {
    Direct { from: String, to: String },
    Conditional { from: String, router: RouterFn<S> },
}

/// Builder for constructing a state graph.
///
/// Add nodes (async functions that transform state) and edges (transitions
/// between nodes), then call `compile()` to produce an executable
/// `CompiledGraph`.
pub struct StateGraph<S> {
    nodes: HashMap<String, NodeFn<S>>,
    edges: Vec<Edge<S>>,
    entry_point: Option<String>,
}

impl<S: Send + 'static> StateGraph<S> {
    /// Create a new, empty graph.
    pub fn new() -> Self {
        Self {
            nodes: HashMap::new(),
            edges: Vec::new(),
            entry_point: None,
        }
    }

    /// Register a node.
    ///
    /// `node` is an async function that receives the current state and returns
    /// the (possibly modified) state.
    pub fn add_node(
        &mut self,
        name: &str,
        node: impl Fn(S) -> BoxFuture<S> + Send + Sync + 'static,
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
    ///
    /// `router` inspects the current state and returns the name of the next
    /// node (or `END`).
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

    /// Validate the graph and produce a `CompiledGraph` ready for execution.
    ///
    /// Validation checks:
    /// - Entry point must be set and must reference an existing node.
    /// - All direct-edge targets must be existing nodes or `END`.
    /// - All direct-edge sources must be existing nodes or `START`.
    /// - Every registered node must be reachable (no orphans).
    pub fn compile(self) -> Result<CompiledGraph<S>, AgentError> {
        // 1. Entry point must be set
        let entry_point = self.entry_point.ok_or_else(|| {
            AgentError::ConfigError("Entry point not set".to_string())
        })?;

        // 2. Entry point must reference an existing node
        if !self.nodes.contains_key(&entry_point) {
            return Err(AgentError::ConfigError(format!(
                "Entry point '{}' does not exist as a node",
                entry_point
            )));
        }

        // 3. Validate edge targets and sources
        for edge in &self.edges {
            match edge {
                Edge::Direct { from, to } => {
                    // Source must be a known node or START
                    if from != START && !self.nodes.contains_key(from.as_str()) {
                        return Err(AgentError::ConfigError(format!(
                            "Edge source '{}' is not a registered node",
                            from
                        )));
                    }
                    // Target must be a known node or END
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

        // 4. Check for orphan nodes — every node must appear as either:
        //    - the entry point, OR
        //    - a direct-edge target, OR
        //    - potentially reachable via a conditional edge (we can't statically
        //      verify conditional targets, so we skip those).
        //    We collect all statically-reachable node names.
        let mut referenced_nodes = std::collections::HashSet::new();
        referenced_nodes.insert(entry_point.clone());
        for edge in &self.edges {
            match edge {
                Edge::Direct { to, .. } if to != END => {
                    referenced_nodes.insert(to.clone());
                }
                _ => {}
            }
        }

        for node_name in self.nodes.keys() {
            if !referenced_nodes.contains(node_name) {
                // The node may still be reachable via conditional edges, so
                // only warn via tracing rather than erroring, to avoid false
                // positives.
                tracing::warn!(
                    node = %node_name,
                    "Node is not statically referenced by any direct edge or entry point; \
                     it may only be reachable via conditional routing"
                );
            }
        }

        Ok(CompiledGraph {
            nodes: self.nodes,
            edges: self.edges,
            entry_point,
        })
    }
}

impl<S: Send + 'static> Default for StateGraph<S> {
    fn default() -> Self {
        Self::new()
    }
}
