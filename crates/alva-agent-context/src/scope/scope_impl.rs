// INPUT:  Arc, Duration, LanguageModel, ChildScopeConfig, ScopeError, ScopeId, ScopeSnapshot, Tool, SessionTracker
// OUTPUT: SpawnScopeImpl
// POS:    Concrete SpawnScope implementation — pure lifecycle management (tree, depth, resources).
//         Communication (Blackboard, channels) is NOT managed here — it lives in plugins/middleware.

use std::sync::Arc;
use std::time::Duration;

use alva_kernel_abi::model::LanguageModel;
use alva_kernel_abi::scope::{ChildScopeConfig, ScopeError, ScopeId, ScopeSnapshot};
use alva_kernel_abi::tool::Tool;

use super::session_tracker::SessionTracker;

/// Concrete SpawnScope implementation.
///
/// Each instance represents one node in the spawn tree. Children are created
/// via `spawn_child()`, which shares the same SessionTracker and model
/// (so state is tracked globally across the tree).
///
/// Communication (Blackboard, channels, etc.) is NOT part of SpawnScope.
/// It is managed by plugins or middleware that hold their own state.
pub struct SpawnScopeImpl {
    id: ScopeId,
    parent_id: Option<ScopeId>,
    depth: u32,
    max_depth: u32,
    role: String,
    session_id: String,

    // Shared across entire tree (Arc)
    model: Arc<dyn LanguageModel>,
    parent_tools: Arc<Vec<Arc<dyn Tool>>>,
    session_tracker: Arc<SessionTracker>,

    // Per-scope config
    timeout: Duration,
    max_iterations: u32,
}

impl SpawnScopeImpl {
    /// Create the root scope (depth=0) that anchors the spawn tree.
    pub fn root(
        model: Arc<dyn LanguageModel>,
        tools: Vec<Arc<dyn Tool>>,
        timeout: Duration,
        max_iterations: u32,
        max_depth: u32,
    ) -> Self {
        let id = ScopeId::new();
        let session_tracker = Arc::new(SessionTracker::new());
        let session_id = session_tracker.create_root("root");

        Self {
            id,
            parent_id: None,
            depth: 0,
            max_depth,
            role: "root".to_string(),
            session_id,
            model,
            parent_tools: Arc::new(tools),
            session_tracker,
            timeout,
            max_iterations,
        }
    }

    /// Spawn a child scope with depth+1, sharing all Arc'd state.
    ///
    /// Fails with `ScopeError::DepthExceeded` if the new depth would exceed
    /// `max_depth`.
    pub async fn spawn_child(
        &self,
        config: ChildScopeConfig,
    ) -> Result<Arc<SpawnScopeImpl>, ScopeError> {
        let new_depth = self.depth + 1;
        if new_depth > self.max_depth {
            return Err(ScopeError::DepthExceeded {
                current: self.depth,
                max: self.max_depth,
            });
        }

        let child_id = ScopeId::new();

        let child_session_id =
            self.session_tracker.create_child(&self.session_id, &config.role);

        let child = SpawnScopeImpl {
            id: child_id,
            parent_id: Some(self.id.clone()),
            depth: new_depth,
            max_depth: self.max_depth,
            role: config.role,
            session_id: child_session_id,
            model: self.model.clone(),
            parent_tools: self.parent_tools.clone(),
            session_tracker: self.session_tracker.clone(),
            timeout: config.timeout.unwrap_or(self.timeout),
            max_iterations: config.max_iterations.unwrap_or(self.max_iterations),
        };

        Ok(Arc::new(child))
    }

    // ── Accessors ────────────────────────────────────────────────────────

    pub fn id(&self) -> &ScopeId {
        &self.id
    }

    pub fn parent_id(&self) -> Option<&ScopeId> {
        self.parent_id.as_ref()
    }

    pub fn depth(&self) -> u32 {
        self.depth
    }

    pub fn role(&self) -> &str {
        &self.role
    }

    pub fn session_id(&self) -> &str {
        &self.session_id
    }

    pub fn model(&self) -> Arc<dyn LanguageModel> {
        self.model.clone()
    }

    pub fn timeout(&self) -> Duration {
        self.timeout
    }

    pub fn max_iterations(&self) -> u32 {
        self.max_iterations
    }

    pub fn session_tracker(&self) -> &Arc<SessionTracker> {
        &self.session_tracker
    }

    // ── Tools ────────────────────────────────────────────────────────────

    /// Names of all parent tools available to hand down to a child scope.
    ///
    /// Excludes the `agent` spawn tool. **Not** to prevent recursion
    /// (recursive spawning is allowed — `A → A' → A'' → …` up to
    /// `max_depth`). The exclusion is because the parent's `agent`
    /// tool instance is bound to the parent's scope (parent's depth),
    /// so if a child were to inherit and dispatch it, the child's
    /// "spawn a grandchild" call would actually create a sibling at
    /// parent-depth instead of a grandchild at child-depth — silently
    /// flattening the tree and bypassing `max_depth`. The child always
    /// receives a freshly-built `AgentSpawnTool` bound to its own
    /// `child_scope` (see `AgentSpawnTool::execute`).
    ///
    /// Used by `AgentSpawnTool::parameters_schema` to expose the valid
    /// tool names as an enum so the parent LLM knows what it can grant.
    pub fn parent_tool_names(&self) -> Vec<String> {
        self.parent_tools
            .iter()
            .filter(|t| t.name() != "agent")
            .map(|t| t.name().to_string())
            .collect()
    }

    /// Return the subset of parent tools whose names appear in `allowed`.
    ///
    /// The `agent` spawn tool is always excluded regardless of the
    /// whitelist — see `parent_tool_names` for the full explanation.
    /// In short: the parent's spawn tool is bound to the parent's
    /// scope, so inheriting it would corrupt depth tracking. The
    /// caller is expected to push a freshly-built `AgentSpawnTool`
    /// bound to the child scope after this call.
    ///
    /// Names that don't match any parent tool are silently skipped, so
    /// a mistyped or stale name from an LLM spawn call does not abort
    /// the sub-agent — it just runs with fewer tools.
    pub fn tools_by_names(&self, allowed: &[String]) -> Vec<Arc<dyn Tool>> {
        use std::collections::HashSet;
        let allowed: HashSet<&str> = allowed.iter().map(String::as_str).collect();
        self.parent_tools
            .iter()
            .filter(|t| t.name() != "agent" && allowed.contains(t.name()))
            .cloned()
            .collect()
    }

    // ── Lifecycle ────────────────────────────────────────────────────────

    /// Mark this scope's session as completed with the given output summary.
    pub fn mark_completed(&self, output: &str) {
        self.session_tracker.mark_completed(&self.session_id, output);
    }

    /// Take a snapshot of this scope's state for debugging/logging.
    pub fn snapshot(&self) -> ScopeSnapshot {
        let session_snap = self.session_tracker.snapshot(&self.session_id);
        ScopeSnapshot {
            id: self.id.as_str().to_owned(),
            parent_id: self.parent_id.as_ref().map(|p| p.as_str().to_owned()),
            depth: self.depth,
            role: self.role.clone(),
            session_id: self.session_id.clone(),
            children_count: session_snap.children_count,
            completed: session_snap.completed,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use alva_kernel_abi::base::error::AgentError;
    use alva_kernel_abi::base::message::Message;
    use alva_kernel_abi::model::{CompletionResponse, ModelConfig};
    use alva_kernel_abi::base::stream::StreamEvent;
    use async_trait::async_trait;
    use futures::stream;
    use futures::Stream;
    use std::pin::Pin;

    struct MockModel;

    #[async_trait]
    impl LanguageModel for MockModel {
        async fn complete(
            &self,
            _messages: &[Message],
            _tools: &[&dyn Tool],
            _config: &ModelConfig,
        ) -> Result<CompletionResponse, AgentError> {
            Ok(CompletionResponse::from_message(Message::system("mock")))
        }

        fn stream(
            &self,
            _messages: &[Message],
            _tools: &[&dyn Tool],
            _config: &ModelConfig,
        ) -> Pin<Box<dyn Stream<Item = StreamEvent> + Send>> {
            Box::pin(stream::empty())
        }

        fn model_id(&self) -> &str {
            "mock"
        }
    }

    fn test_root(max_depth: u32) -> SpawnScopeImpl {
        SpawnScopeImpl::root(
            Arc::new(MockModel),
            vec![],
            Duration::from_secs(60),
            30,
            max_depth,
        )
    }

    #[tokio::test]
    async fn root_scope_properties() {
        let scope = test_root(3);
        assert_eq!(scope.depth(), 0);
        assert!(scope.parent_id().is_none());
        assert_eq!(scope.role(), "root");
        assert_eq!(scope.timeout(), Duration::from_secs(60));
        assert_eq!(scope.max_iterations(), 30);
        assert_eq!(scope.model().model_id(), "mock");
    }

    #[tokio::test]
    async fn spawn_child_increments_depth() {
        let root = test_root(3);
        let child = root
            .spawn_child(ChildScopeConfig::new("planner"))
            .await
            .unwrap();
        assert_eq!(child.depth(), 1);
        assert_eq!(child.parent_id(), Some(root.id()));
        assert_eq!(child.role(), "planner");
    }

    #[tokio::test]
    async fn depth_limit_enforced() {
        let root = test_root(2);
        let c1 = root
            .spawn_child(ChildScopeConfig::new("a"))
            .await
            .unwrap();
        let c2 = c1
            .spawn_child(ChildScopeConfig::new("b"))
            .await
            .unwrap();
        let result = c2.spawn_child(ChildScopeConfig::new("c")).await;
        assert!(matches!(result, Err(ScopeError::DepthExceeded { .. })));
    }

    #[tokio::test]
    async fn child_timeout_override() {
        let root = test_root(3);
        let child = root
            .spawn_child(ChildScopeConfig::new("fast").with_timeout(Duration::from_secs(10)))
            .await
            .unwrap();
        assert_eq!(child.timeout(), Duration::from_secs(10));
    }

    #[tokio::test]
    async fn child_inherits_timeout() {
        let root = test_root(3);
        let child = root
            .spawn_child(ChildScopeConfig::new("worker"))
            .await
            .unwrap();
        assert_eq!(child.timeout(), Duration::from_secs(60));
    }

    #[tokio::test]
    async fn mark_completed_updates_tracker() {
        let root = test_root(3);
        let child = root
            .spawn_child(ChildScopeConfig::new("worker"))
            .await
            .unwrap();
        child.mark_completed("done");
        let snap = root.session_tracker().snapshot(child.session_id());
        assert!(snap.completed);
        assert_eq!(snap.output_summary, Some("done".to_string()));
    }

    #[tokio::test]
    async fn session_tree_tracks_children() {
        let root = test_root(3);
        let _c1 = root
            .spawn_child(ChildScopeConfig::new("a"))
            .await
            .unwrap();
        let _c2 = root
            .spawn_child(ChildScopeConfig::new("b"))
            .await
            .unwrap();
        let children = root.session_tracker().children_of(root.session_id());
        assert_eq!(children.len(), 2);
    }

    #[tokio::test]
    async fn snapshot_works() {
        let root = test_root(3);
        let _c1 = root
            .spawn_child(ChildScopeConfig::new("worker"))
            .await
            .unwrap();
        let snap = root.snapshot();
        assert_eq!(snap.depth, 0);
        assert_eq!(snap.children_count, 1);
        assert!(!snap.completed);
        assert!(snap.parent_id.is_none());
        assert_eq!(snap.role, "root");
    }

    #[tokio::test]
    async fn child_inherits_max_iterations() {
        let root = test_root(3);
        let child = root
            .spawn_child(ChildScopeConfig::new("worker"))
            .await
            .unwrap();
        assert_eq!(child.max_iterations(), 30);
    }

    #[tokio::test]
    async fn child_overrides_max_iterations() {
        let root = test_root(3);
        let child = root
            .spawn_child(ChildScopeConfig::new("worker").with_max_iterations(10))
            .await
            .unwrap();
        assert_eq!(child.max_iterations(), 10);
    }

    #[tokio::test]
    async fn tools_by_names_empty_when_whitelist_empty() {
        let root = test_root(3);
        // `test_root` passes an empty parent_tools vec, so both the
        // names list and any whitelist resolve to empty.
        assert!(root.parent_tool_names().is_empty());
        assert!(root.tools_by_names(&[]).is_empty());
        assert!(root.tools_by_names(&["nonexistent".to_string()]).is_empty());
    }

    #[tokio::test]
    async fn grandchild_depth() {
        let root = test_root(5);
        let c1 = root
            .spawn_child(ChildScopeConfig::new("a"))
            .await
            .unwrap();
        let c2 = c1
            .spawn_child(ChildScopeConfig::new("b"))
            .await
            .unwrap();
        let c3 = c2
            .spawn_child(ChildScopeConfig::new("c"))
            .await
            .unwrap();
        assert_eq!(c3.depth(), 3);
        assert_eq!(c3.parent_id(), Some(c2.id()));
    }
}
