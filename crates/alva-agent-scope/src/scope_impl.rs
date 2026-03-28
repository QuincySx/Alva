// INPUT:  Arc, Duration, LanguageModel, ChildScopeConfig, ScopeError, ScopeId, ScopeSnapshot, Tool, BoardRegistry, SessionTracker, Blackboard
// OUTPUT: SpawnScopeImpl
// POS:    Concrete SpawnScope implementation — each instance is one node in the spawn tree.

use std::sync::Arc;
use std::time::Duration;

use alva_types::model::LanguageModel;
use alva_types::scope::{ChildScopeConfig, ScopeError, ScopeId, ScopeSnapshot};
use alva_types::tool::Tool;

use crate::board_registry::BoardRegistry;
use crate::session_tracker::SessionTracker;
use crate::blackboard::Blackboard;

/// Concrete SpawnScope implementation.
///
/// Each instance represents one node in the spawn tree. Children are created
/// via `spawn_child()`, which shares the same BoardRegistry, SessionTracker,
/// and model (so state is tracked globally across the tree).
///
/// Board isolation: when a child scope requests a board via `board()`, the
/// board is keyed under the **parent's** scope ID. This means siblings under
/// the same parent naturally share the same board instance.
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
    board_registry: Arc<BoardRegistry>,
    session_tracker: Arc<SessionTracker>,

    // Per-scope config
    board_id: Option<String>,
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
            board_registry: Arc::new(BoardRegistry::new()),
            session_tracker,
            board_id: None,
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

        // Register parent-child in board_registry
        self.board_registry.set_parent(&child_id, &self.id).await;

        // Register in session_tracker
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
            board_registry: self.board_registry.clone(),
            session_tracker: self.session_tracker.clone(),
            board_id: config.board_id,
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

    pub fn board_registry(&self) -> &Arc<BoardRegistry> {
        &self.board_registry
    }

    pub fn session_tracker(&self) -> &Arc<SessionTracker> {
        &self.session_tracker
    }

    // ── Tools ────────────────────────────────────────────────────────────

    /// Get the tools available to this scope.
    ///
    /// When `inherit` is true, parent tools are included but the "agent" tool
    /// is filtered out to avoid recursive self-spawning loops.
    pub fn tools(&self, inherit: bool) -> Vec<Arc<dyn Tool>> {
        if inherit {
            self.parent_tools
                .iter()
                .filter(|t| t.name() != "agent")
                .cloned()
                .collect()
        } else {
            Vec::new()
        }
    }

    // ── Blackboard ───────────────────────────────────────────────────────

    /// Get (or create) a board for the given `board_id`.
    ///
    /// Boards are scoped under the **parent's** scope ID so that siblings
    /// under the same parent share the same board instance. For the root
    /// scope (which has no parent), boards are scoped under its own ID.
    pub async fn board(&self, board_id: &str) -> Arc<Blackboard> {
        let scope_key = self.parent_id.as_ref().unwrap_or(&self.id);
        self.board_registry.get_or_create(scope_key, board_id).await
    }

    /// Get the parent scope's board for this scope's configured `board_id`.
    ///
    /// Returns `None` if no parent exists, no `board_id` is configured, or
    /// the parent has no board with that ID.
    pub async fn parent_board(&self) -> Option<Arc<Blackboard>> {
        let board_id = self.board_id.as_deref()?;
        let parent_id = self.parent_id.as_ref()?;
        // The parent's board is keyed under the *grandparent's* scope ID
        // (following the same rule), but parent_board_for looks at the
        // parent relationship stored in BoardRegistry, so we use the
        // parent_id directly.
        self.board_registry
            .parent_board_for(parent_id, board_id)
            .await
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
            board_id: self.board_id.clone(),
            session_id: self.session_id.clone(),
            children_count: session_snap.children_count,
            completed: session_snap.completed,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use alva_types::base::error::AgentError;
    use alva_types::base::message::Message;
    use alva_types::model::ModelConfig;
    use alva_types::base::stream::StreamEvent;
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
        ) -> Result<Message, AgentError> {
            Ok(Message::system("mock"))
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
    async fn siblings_share_board() {
        let root = test_root(3);
        let c1 = root
            .spawn_child(ChildScopeConfig::new("a").with_board("team"))
            .await
            .unwrap();
        let c2 = root
            .spawn_child(ChildScopeConfig::new("b").with_board("team"))
            .await
            .unwrap();

        // Both children's boards are keyed under the parent (root) scope ID,
        // so they share the same Blackboard instance.
        let board1 = c1.board("team").await;
        let board2 = c2.board("team").await;

        use crate::blackboard::BoardMessage;
        board1.post(BoardMessage::new("a", "hello")).await;
        assert_eq!(board2.message_count().await, 1);
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
    async fn tools_empty_when_not_inheriting() {
        let root = test_root(3);
        let tools = root.tools(false);
        assert!(tools.is_empty());
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
