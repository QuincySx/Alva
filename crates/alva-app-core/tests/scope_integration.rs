// Integration tests for SpawnScope multi-level spawn tree.
//
// Exercises: depth tracking, board sharing, board isolation,
// session tree structure, mark_completed, timeout/iteration inheritance.

use std::sync::Arc;
use std::time::Duration;

use alva_app_core::plugins::blackboard::BoardMessage;
use alva_app_core::scope::SpawnScopeImpl;
use alva_types::base::error::AgentError;
use alva_types::base::message::Message;
use alva_types::model::{LanguageModel, ModelConfig};
use alva_types::scope::{ChildScopeConfig, ScopeError};
use alva_types::base::stream::StreamEvent;
use alva_types::tool::Tool;
use async_trait::async_trait;
use futures::Stream;
use std::pin::Pin;

// ── Mock model ──────────────────────────────────────────────────────────

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
        Box::pin(futures::stream::empty())
    }

    fn model_id(&self) -> &str {
        "mock"
    }
}

fn root_scope(max_depth: u32) -> SpawnScopeImpl {
    SpawnScopeImpl::root(
        Arc::new(MockModel),
        vec![],
        Duration::from_secs(60),
        30,
        max_depth,
    )
}

// ── 1. Multi-level depth tracking ───────────────────────────────────────
//
// Root (depth 0) → A (depth 1) → B (depth 2) → C REFUSED (depth 3 > max 2)

#[tokio::test]
async fn multi_level_depth_tracking() {
    let root = root_scope(2);
    assert_eq!(root.depth(), 0);

    let a = root
        .spawn_child(ChildScopeConfig::new("A"))
        .await
        .unwrap();
    assert_eq!(a.depth(), 1);
    assert_eq!(a.parent_id(), Some(root.id()));

    let b = a.spawn_child(ChildScopeConfig::new("B")).await.unwrap();
    assert_eq!(b.depth(), 2);
    assert_eq!(b.parent_id(), Some(a.id()));

    // Depth 3 should be refused (max_depth=2)
    let result = b.spawn_child(ChildScopeConfig::new("C")).await;
    match result {
        Err(ScopeError::DepthExceeded { current: 2, max: 2 }) => {} // expected
        Err(ref e) => panic!("Expected DepthExceeded(current=2, max=2), got: {}", e),
        Ok(_) => panic!("Expected DepthExceeded error, but spawn succeeded"),
    }
}

// ── 2. Sibling board sharing ────────────────────────────────────────────
//
// Root → spawn A (board="proj") + spawn B (board="proj")
// A posts to board → B sees it (same parent, same board_id)

#[tokio::test]
async fn sibling_board_sharing() {
    let root = root_scope(3);

    let a = root
        .spawn_child(ChildScopeConfig::new("A").with_board("proj"))
        .await
        .unwrap();
    let b = root
        .spawn_child(ChildScopeConfig::new("B").with_board("proj"))
        .await
        .unwrap();

    // Both children key their board under root's scope ID, so they share.
    let board_a = a.board("proj").await;
    let board_b = b.board("proj").await;

    board_a
        .post(BoardMessage::new("A", "hello from A"))
        .await;

    assert_eq!(
        board_b.message_count().await,
        1,
        "sibling B should see A's message"
    );

    let msgs = board_b.all_messages().await;
    assert_eq!(msgs[0].from, "A");
    assert_eq!(msgs[0].content, "hello from A");
}

// ── 3. Child reads parent board (upward visibility) ─────────────────────
//
// Root creates board "team" and posts a message.
// Root → spawn middle → spawn worker (board="team")
//
// worker.board("team") returns the board keyed under middle's scope ID.
// middle.board("team") also keys under root's scope ID (middle's parent).
// So worker's board("team") keys under middle.id while root's board("team") keys under root.id.
//
// To test upward visibility via parent_board(), we use a 3-level tree:
// middle's board() keys under root.id. worker's parent_board() looks up
// the board that middle would see, which is keyed under root.id via the
// parent chain.

#[tokio::test]
async fn child_reads_parent_board_upward_visibility() {
    let root = root_scope(5);

    // Root creates a board and posts a message.
    let root_board = root.board("team").await;
    root_board
        .post(BoardMessage::new("root", "team announcement"))
        .await;

    // Spawn middle child — middle's board("team") also keys under root.id
    // (because middle.parent_id = root.id).
    let middle = root
        .spawn_child(ChildScopeConfig::new("middle").with_board("team"))
        .await
        .unwrap();

    // Middle can see root's board through board()
    let middle_board = middle.board("team").await;
    assert_eq!(
        middle_board.message_count().await,
        1,
        "middle should see root's message via shared parent key"
    );

    // Now spawn worker under middle, with board="team".
    let worker = middle
        .spawn_child(ChildScopeConfig::new("worker").with_board("team"))
        .await
        .unwrap();

    // worker.board("team") keys under middle.id (worker's parent).
    // This is a different board than root's.
    let worker_board = worker.board("team").await;
    assert_eq!(
        worker_board.message_count().await,
        0,
        "worker's board is keyed under middle.id, separate from root's"
    );

    // worker.parent_board() should reach middle's board, which is keyed
    // under root.id — and contains root's announcement.
    let parent_view = worker.parent_board().await;
    assert!(
        parent_view.is_some(),
        "worker should be able to read parent's board"
    );
    let pboard = parent_view.unwrap();
    assert_eq!(
        pboard.message_count().await,
        1,
        "parent board should have root's message"
    );
    let msgs = pboard.all_messages().await;
    assert_eq!(msgs[0].content, "team announcement");
}

// ── 4. Cross-tree isolation ─────────────────────────────────────────────
//
// Root → spawn A (board="work")
// Root → spawn B
// B → spawn C (board="work")
//
// A's board("work") keys under root.id.
// C's board("work") keys under B.id.
// They must NOT share boards (different parent scopes).

#[tokio::test]
async fn cross_tree_isolation() {
    let root = root_scope(3);

    let a = root
        .spawn_child(ChildScopeConfig::new("A").with_board("work"))
        .await
        .unwrap();

    let b = root
        .spawn_child(ChildScopeConfig::new("B"))
        .await
        .unwrap();

    let c = b
        .spawn_child(ChildScopeConfig::new("C").with_board("work"))
        .await
        .unwrap();

    // A's board is keyed under root.id
    let board_a = a.board("work").await;
    board_a.post(BoardMessage::new("A", "secret data")).await;

    // C's board is keyed under B.id (C's parent)
    let board_c = c.board("work").await;

    assert_eq!(
        board_c.message_count().await,
        0,
        "C must NOT see A's messages — different parent scope keys"
    );

    // Verify A still has its message
    assert_eq!(board_a.message_count().await, 1);

    // Post from C's side
    board_c.post(BoardMessage::new("C", "C's data")).await;

    // A should not see C's message
    assert_eq!(
        board_a.message_count().await,
        1,
        "A must NOT see C's messages"
    );
    assert_eq!(board_c.message_count().await, 1);
}

// ── 5. Session tree structure ───────────────────────────────────────────
//
// Root → spawn A → spawn A1
// Root → spawn B
//
// session_tracker shows: root has 2 children (A, B), A has 1 child (A1)
// tree_json serializes the full tree.

#[tokio::test]
async fn session_tree_structure() {
    let root = root_scope(3);
    let tracker = root.session_tracker().clone();

    let a = root
        .spawn_child(ChildScopeConfig::new("A"))
        .await
        .unwrap();
    let a1 = a
        .spawn_child(ChildScopeConfig::new("A1"))
        .await
        .unwrap();
    let b = root
        .spawn_child(ChildScopeConfig::new("B"))
        .await
        .unwrap();

    // Root has 2 children
    let root_children = tracker.children_of(root.session_id());
    assert_eq!(root_children.len(), 2, "root should have 2 children");
    assert!(root_children.contains(&a.session_id().to_string()));
    assert!(root_children.contains(&b.session_id().to_string()));

    // A has 1 child (A1)
    let a_children = tracker.children_of(a.session_id());
    assert_eq!(a_children.len(), 1, "A should have 1 child");
    assert_eq!(a_children[0], a1.session_id());

    // B has no children
    let b_children = tracker.children_of(b.session_id());
    assert_eq!(b_children.len(), 0, "B should have no children");

    // A1's parent is A
    assert_eq!(
        tracker.parent_of(a1.session_id()),
        Some(a.session_id().to_string())
    );

    // tree_json serializes the full tree
    let tree = tracker.tree_json(root.session_id());
    assert!(tree["children"].is_array());
    let top_children = tree["children"].as_array().unwrap();
    assert_eq!(top_children.len(), 2);

    // Find A's subtree and verify it has A1
    let a_node = top_children
        .iter()
        .find(|n| n["role"].as_str() == Some("A"))
        .expect("should find A in tree");
    let a_sub_children = a_node["children"].as_array().unwrap();
    assert_eq!(a_sub_children.len(), 1);
    assert_eq!(a_sub_children[0]["role"].as_str(), Some("A1"));
}

// ── 6. Mark completed propagates ────────────────────────────────────────
//
// Root → spawn worker
// worker.mark_completed("done")
// session_tracker.snapshot(worker.session_id()) shows completed=true

#[tokio::test]
async fn mark_completed_propagates() {
    let root = root_scope(3);
    let tracker = root.session_tracker().clone();

    let worker = root
        .spawn_child(ChildScopeConfig::new("worker"))
        .await
        .unwrap();

    // Not completed yet
    let snap_before = tracker.snapshot(worker.session_id());
    assert!(!snap_before.completed);
    assert!(snap_before.output_summary.is_none());

    // Mark completed
    worker.mark_completed("done");

    // Now completed
    let snap_after = tracker.snapshot(worker.session_id());
    assert!(snap_after.completed);
    assert_eq!(snap_after.output_summary, Some("done".to_string()));

    // Root itself is NOT completed
    let root_snap = tracker.snapshot(root.session_id());
    assert!(!root_snap.completed);

    // Verify via scope's own snapshot method
    let worker_scope_snap = worker.snapshot();
    assert!(worker_scope_snap.completed);
    assert_eq!(worker_scope_snap.role, "worker");
    assert_eq!(worker_scope_snap.depth, 1);
}

// ── 7. Timeout/iterations inheritance + override ────────────────────────
//
// Root (timeout=60s, max_iter=30) → spawn child (no override) → inherits 60s, 30
// Root → spawn child (timeout=10s, max_iter=5) → gets 10s, 5

#[tokio::test]
async fn timeout_iterations_inheritance_and_override() {
    let root = root_scope(3);

    // Verify root defaults
    assert_eq!(root.timeout(), Duration::from_secs(60));
    assert_eq!(root.max_iterations(), 30);

    // Child with no override inherits parent values
    let inherited = root
        .spawn_child(ChildScopeConfig::new("inheritor"))
        .await
        .unwrap();
    assert_eq!(inherited.timeout(), Duration::from_secs(60));
    assert_eq!(inherited.max_iterations(), 30);

    // Child with explicit overrides
    let overridden = root
        .spawn_child(
            ChildScopeConfig::new("overrider")
                .with_timeout(Duration::from_secs(10))
                .with_max_iterations(5),
        )
        .await
        .unwrap();
    assert_eq!(overridden.timeout(), Duration::from_secs(10));
    assert_eq!(overridden.max_iterations(), 5);

    // Grandchild inherits from its parent (the overridden child), not root
    let grandchild = overridden
        .spawn_child(ChildScopeConfig::new("grandchild"))
        .await
        .unwrap();
    assert_eq!(
        grandchild.timeout(),
        Duration::from_secs(10),
        "grandchild should inherit overridden parent's timeout"
    );
    assert_eq!(
        grandchild.max_iterations(),
        5,
        "grandchild should inherit overridden parent's max_iterations"
    );

    // Grandchild can further override
    let gc_override = overridden
        .spawn_child(
            ChildScopeConfig::new("gc_override")
                .with_timeout(Duration::from_secs(3))
                .with_max_iterations(2),
        )
        .await
        .unwrap();
    assert_eq!(gc_override.timeout(), Duration::from_secs(3));
    assert_eq!(gc_override.max_iterations(), 2);
}

// ── Additional: snapshot fields are correct across tree ──────────────────

#[tokio::test]
async fn snapshot_across_tree() {
    let root = root_scope(3);

    let child = root
        .spawn_child(ChildScopeConfig::new("planner").with_board("design"))
        .await
        .unwrap();

    let _gc1 = child
        .spawn_child(ChildScopeConfig::new("researcher"))
        .await
        .unwrap();
    let _gc2 = child
        .spawn_child(ChildScopeConfig::new("writer"))
        .await
        .unwrap();

    // Root snapshot
    let root_snap = root.snapshot();
    assert_eq!(root_snap.depth, 0);
    assert_eq!(root_snap.role, "root");
    assert!(root_snap.parent_id.is_none());
    assert!(root_snap.board_id.is_none());
    assert_eq!(root_snap.children_count, 1); // only "planner"
    assert!(!root_snap.completed);

    // Child snapshot
    let child_snap = child.snapshot();
    assert_eq!(child_snap.depth, 1);
    assert_eq!(child_snap.role, "planner");
    assert_eq!(child_snap.parent_id, Some(root.id().as_str().to_owned()));
    assert_eq!(child_snap.board_id, Some("design".to_string()));
    assert_eq!(child_snap.children_count, 2); // researcher + writer
    assert!(!child_snap.completed);
}

// ── Additional: model is shared across the tree ─────────────────────────

#[tokio::test]
async fn model_shared_across_tree() {
    let root = root_scope(3);
    let child = root
        .spawn_child(ChildScopeConfig::new("worker"))
        .await
        .unwrap();
    let grandchild = child
        .spawn_child(ChildScopeConfig::new("sub"))
        .await
        .unwrap();

    assert_eq!(root.model().model_id(), "mock");
    assert_eq!(child.model().model_id(), "mock");
    assert_eq!(grandchild.model().model_id(), "mock");
}

// ── Additional: board_registry and session_tracker are shared ───────────

#[tokio::test]
async fn shared_registries_across_tree() {
    let root = root_scope(3);
    let child = root
        .spawn_child(ChildScopeConfig::new("worker"))
        .await
        .unwrap();

    // Same board_registry instance (Arc identity)
    assert!(
        Arc::ptr_eq(root.board_registry(), child.board_registry()),
        "board_registry should be shared"
    );

    // Same session_tracker instance (Arc identity)
    assert!(
        Arc::ptr_eq(root.session_tracker(), child.session_tracker()),
        "session_tracker should be shared"
    );
}
