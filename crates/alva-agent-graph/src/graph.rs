// INPUT:  std::collections::HashMap, std::future::Future, std::pin::Pin, alva_kernel_abi::AgentError, crate::pregel::CompiledGraph
// OUTPUT: pub const START, pub const END, pub struct StateGraph, pub enum NodeResult, pub struct Send
// POS:    StateGraph builder with Send/Command-style dynamic routing support.

use std::collections::HashMap;
use std::future::Future;
use std::pin::Pin;

use alva_kernel_abi::AgentError;

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

#[cfg(test)]
mod tests {
    //! Tests for StateGraph::compile() — the validation gate that
    //! produces a CompiledGraph (or a ConfigError for any of 5
    //! malformed-graph cases).
    //!
    //! Two contract families are pinned here:
    //!
    //! 1. **5 ConfigError paths** — each error message must include the
    //!    offending name so users can diagnose without guesswork:
    //!    - entry_point not set
    //!    - entry_point references an unknown node
    //!    - direct edge source unknown (and not START)
    //!    - direct edge target unknown (and not END)
    //!    - conditional edge source unknown (and not START)
    //!
    //! 2. **3 sentinel special-cases (silent contracts)** — `START` and
    //!    `END` bypass the registration check. A refactor that typo'd
    //!    the constant comparison (e.g., `!= "__start__"` instead of
    //!    `!= START`) would silently break every START/END edge — no
    //!    compile-time hint. Each sentinel-allow path gets its own pin.
    use super::*;
    use std::pin::Pin;

    type State = i32;

    /// Trivial async passthrough node used for shape-only tests.
    fn passthrough() -> impl Fn(State) -> Pin<Box<dyn std::future::Future<Output = State> + Send>>
           + Send + Sync + 'static {
        |s: State| Box::pin(async move { s })
    }

    // -- new() + Default -------------------------------------------------

    #[test]
    fn new_initializes_empty_state() {
        let g: StateGraph<State> = StateGraph::new();
        assert!(g.nodes.is_empty());
        assert!(g.edges.is_empty());
        assert!(g.entry_point.is_none());
        assert!(g.merge_fn.is_none());
    }

    #[test]
    fn default_delegates_to_new_with_same_empty_state() {
        // Pin: Default MUST call ::new() — if someone breaks the
        // delegation (e.g. fills different defaults), graph behavior
        // diverges between `let g = StateGraph::new()` and
        // `let g: StateGraph<_> = Default::default()`.
        let g: StateGraph<State> = StateGraph::default();
        assert!(g.nodes.is_empty());
        assert!(g.edges.is_empty());
        assert!(g.entry_point.is_none());
        assert!(g.merge_fn.is_none());
    }

    // -- 5 ConfigError paths --------------------------------------------

    #[test]
    fn compile_fails_when_entry_point_not_set() {
        let g: StateGraph<State> = StateGraph::new();
        let err = g.compile().expect_err("missing entry must error");
        let msg = err.to_string();
        assert!(
            msg.contains("Entry point not set"),
            "diagnostic must say entry-point-not-set: {msg}"
        );
    }

    #[test]
    fn compile_fails_when_entry_point_node_unregistered_includes_name() {
        // Diagnostic must include the offending name — a refactor that
        // dropped `{}` from the format string would surface "does not
        // exist as a node" garbage to users with no clue which name was wrong.
        let mut g: StateGraph<State> = StateGraph::new();
        g.set_entry_point("ghost");
        let err = g.compile().expect_err("missing entry-target must error");
        let msg = err.to_string();
        assert!(msg.contains("does not exist as a node"), "{msg}");
        assert!(msg.contains("ghost"), "must name the missing entry: {msg}");
    }

    #[test]
    fn compile_fails_when_direct_edge_source_unknown_includes_name() {
        let mut g: StateGraph<State> = StateGraph::new();
        g.add_node("real", passthrough());
        g.set_entry_point("real");
        g.add_edge("phantom", "real"); // source unknown, not START
        let err = g.compile().expect_err("unknown source must error");
        let msg = err.to_string();
        assert!(msg.contains("Edge source"), "{msg}");
        assert!(msg.contains("phantom"), "must name the unknown source: {msg}");
    }

    #[test]
    fn compile_fails_when_direct_edge_target_unknown_includes_name() {
        let mut g: StateGraph<State> = StateGraph::new();
        g.add_node("real", passthrough());
        g.set_entry_point("real");
        g.add_edge("real", "missing"); // target unknown, not END
        let err = g.compile().expect_err("unknown target must error");
        let msg = err.to_string();
        assert!(msg.contains("Edge target"), "{msg}");
        assert!(msg.contains("missing"), "must name the unknown target: {msg}");
    }

    #[test]
    fn compile_fails_when_conditional_edge_source_unknown_includes_name() {
        let mut g: StateGraph<State> = StateGraph::new();
        g.add_node("real", passthrough());
        g.set_entry_point("real");
        g.add_conditional_edge("phantom_cond", |_| END.to_string());
        let err = g.compile().expect_err("unknown conditional source must error");
        let msg = err.to_string();
        assert!(msg.contains("Conditional edge source"), "{msg}");
        assert!(
            msg.contains("phantom_cond"),
            "must name the unknown conditional source: {msg}"
        );
    }

    // -- 3 sentinel special-cases (silent contract pins) -----------------

    #[test]
    fn compile_allows_direct_edge_from_start_sentinel() {
        // PIN: START is special — it MUST NOT be required to exist as
        // a registered node. A refactor that typo'd `!= START` would
        // silently break every "START → first_node" edge.
        let mut g: StateGraph<State> = StateGraph::new();
        g.add_node("first", passthrough());
        g.set_entry_point("first");
        g.add_edge(START, "first");
        let compiled = g.compile().expect("START source must be allowed");
        assert_eq!(compiled.entry_point, "first");
    }

    #[test]
    fn compile_allows_direct_edge_to_end_sentinel() {
        // PIN: END is special — it MUST NOT be required to exist as
        // a registered node. A refactor that typo'd `!= END` would
        // silently break every "last_node → END" edge.
        let mut g: StateGraph<State> = StateGraph::new();
        g.add_node("last", passthrough());
        g.set_entry_point("last");
        g.add_edge("last", END);
        g.compile().expect("END target must be allowed");
    }

    #[test]
    fn compile_allows_conditional_edge_from_start_sentinel() {
        // PIN: START works as a conditional source too — distinct
        // code path (Edge::Conditional vs Edge::Direct) so pinned
        // separately. A refactor that fixed one and missed the other
        // would silently break conditional routing from START.
        let mut g: StateGraph<State> = StateGraph::new();
        g.add_node("first", passthrough());
        g.set_entry_point("first");
        g.add_conditional_edge(START, |_| "first".to_string());
        g.compile().expect("START as conditional source must be allowed");
    }

    // -- Happy path ------------------------------------------------------

    #[test]
    fn compile_happy_path_returns_compiled_graph_preserving_entry_and_counts() {
        let mut g: StateGraph<State> = StateGraph::new();
        g.add_node("a", passthrough());
        g.add_node("b", passthrough());
        g.set_entry_point("a");
        g.add_edge("a", "b");
        g.add_edge("b", END);
        let compiled = g.compile().expect("valid graph must compile");
        assert_eq!(compiled.entry_point, "a");
        assert_eq!(compiled.nodes.len(), 2);
        assert_eq!(compiled.edges.len(), 2);
        assert!(compiled.merge_fn.is_none(), "no merge_fn set → none on output");
    }

    #[test]
    fn compile_preserves_merge_fn_when_set() {
        // Pin: set_merge → compile carries merge_fn through to the
        // CompiledGraph. A refactor that forgot to forward it would
        // silently break parallel-execution semantics (executor would
        // fall back to last-update instead of using the merge).
        let mut g: StateGraph<State> = StateGraph::new();
        g.add_node("a", passthrough());
        g.set_entry_point("a");
        g.set_merge(|base, _updates: Vec<State>| base);
        let compiled = g.compile().expect("merged graph must compile");
        assert!(compiled.merge_fn.is_some(), "set_merge must round-trip through compile");
    }
}
