// Integration tests for SpawnScope multi-level spawn tree.
//
// Exercises: depth tracking, session tree structure, mark_completed,
// timeout/iteration inheritance, board sharing (via BoardRegistry, independent of scope).

use std::sync::Arc;
use std::time::Duration;

use alva_agent_scope::board_registry::BoardRegistry;
use alva_agent_scope::BoardMessage;
use alva_agent_scope::SpawnScopeImpl;
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

    let result = b.spawn_child(ChildScopeConfig::new("C")).await;
    match result {
        Err(ScopeError::DepthExceeded { current: 2, max: 2 }) => {}
        Err(ref e) => panic!("Expected DepthExceeded(current=2, max=2), got: {}", e),
        Ok(_) => panic!("Expected DepthExceeded error, but spawn succeeded"),
    }
}

// ── 2. Board sharing via BoardRegistry (independent of scope) ──────────
//
// BoardRegistry manages boards keyed by (scope_id, board_id).
// Siblings sharing the same parent scope_id + board_id get the same board.

#[tokio::test]
async fn sibling_board_sharing_via_registry() {
    let root = root_scope(3);
    let registry = Arc::new(BoardRegistry::new());

    let _a = root
        .spawn_child(ChildScopeConfig::new("A"))
        .await
        .unwrap();
    let _b = root
        .spawn_child(ChildScopeConfig::new("B"))
        .await
        .unwrap();

    // Both use root's scope ID as the key — siblings share the board
    let board_a = registry.get_or_create(root.id(), "proj").await;
    let board_b = registry.get_or_create(root.id(), "proj").await;

    board_a
        .post(BoardMessage::new("A", "hello from A"))
        .await;

    assert_eq!(
        board_b.message_count().await,
        1,
        "sibling B should see A's message"
    );
}

// ── 3. Board isolation across different parent scopes ──────────────────

#[tokio::test]
async fn cross_tree_board_isolation() {
    let root = root_scope(3);
    let registry = Arc::new(BoardRegistry::new());

    let b = root
        .spawn_child(ChildScopeConfig::new("B"))
        .await
        .unwrap();

    // Board under root's scope
    let board_root = registry.get_or_create(root.id(), "work").await;
    board_root.post(BoardMessage::new("A", "secret data")).await;

    // Board under B's scope — different key, different board
    let board_b = registry.get_or_create(b.id(), "work").await;

    assert_eq!(
        board_b.message_count().await,
        0,
        "B's board must NOT see root-scoped messages"
    );
}

// ── 4. Session tree structure ───────────────────────────────────────────

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

    let root_children = tracker.children_of(root.session_id());
    assert_eq!(root_children.len(), 2);
    assert!(root_children.contains(&a.session_id().to_string()));
    assert!(root_children.contains(&b.session_id().to_string()));

    let a_children = tracker.children_of(a.session_id());
    assert_eq!(a_children.len(), 1);
    assert_eq!(a_children[0], a1.session_id());

    let b_children = tracker.children_of(b.session_id());
    assert_eq!(b_children.len(), 0);

    assert_eq!(
        tracker.parent_of(a1.session_id()),
        Some(a.session_id().to_string())
    );

    let tree = tracker.tree_json(root.session_id());
    assert!(tree["children"].is_array());
    let top_children = tree["children"].as_array().unwrap();
    assert_eq!(top_children.len(), 2);

    let a_node = top_children
        .iter()
        .find(|n| n["role"].as_str() == Some("A"))
        .expect("should find A in tree");
    let a_sub_children = a_node["children"].as_array().unwrap();
    assert_eq!(a_sub_children.len(), 1);
    assert_eq!(a_sub_children[0]["role"].as_str(), Some("A1"));
}

// ── 5. Mark completed ──────────────────────────────────────────────────

#[tokio::test]
async fn mark_completed_propagates() {
    let root = root_scope(3);
    let tracker = root.session_tracker().clone();

    let worker = root
        .spawn_child(ChildScopeConfig::new("worker"))
        .await
        .unwrap();

    let snap_before = tracker.snapshot(worker.session_id());
    assert!(!snap_before.completed);

    worker.mark_completed("done");

    let snap_after = tracker.snapshot(worker.session_id());
    assert!(snap_after.completed);
    assert_eq!(snap_after.output_summary, Some("done".to_string()));

    let root_snap = tracker.snapshot(root.session_id());
    assert!(!root_snap.completed);
}

// ── 6. Timeout/iterations inheritance + override ────────────────────────

#[tokio::test]
async fn timeout_iterations_inheritance_and_override() {
    let root = root_scope(3);
    assert_eq!(root.timeout(), Duration::from_secs(60));
    assert_eq!(root.max_iterations(), 30);

    let inherited = root
        .spawn_child(ChildScopeConfig::new("inheritor"))
        .await
        .unwrap();
    assert_eq!(inherited.timeout(), Duration::from_secs(60));
    assert_eq!(inherited.max_iterations(), 30);

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

    let grandchild = overridden
        .spawn_child(ChildScopeConfig::new("grandchild"))
        .await
        .unwrap();
    assert_eq!(grandchild.timeout(), Duration::from_secs(10));
    assert_eq!(grandchild.max_iterations(), 5);
}

// ── 7. Snapshot fields ─────────────────────────────────────────────────

#[tokio::test]
async fn snapshot_across_tree() {
    let root = root_scope(3);

    let child = root
        .spawn_child(ChildScopeConfig::new("planner"))
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

    let root_snap = root.snapshot();
    assert_eq!(root_snap.depth, 0);
    assert_eq!(root_snap.role, "root");
    assert!(root_snap.parent_id.is_none());
    assert_eq!(root_snap.children_count, 1);

    let child_snap = child.snapshot();
    assert_eq!(child_snap.depth, 1);
    assert_eq!(child_snap.role, "planner");
    assert_eq!(child_snap.parent_id, Some(root.id().as_str().to_owned()));
    assert_eq!(child_snap.children_count, 2);
}

// ── 8. Model shared across tree ────────────────────────────────────────

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

// ── 9. Session tracker shared across tree ──────────────────────────────

#[tokio::test]
async fn shared_session_tracker_across_tree() {
    let root = root_scope(3);
    let child = root
        .spawn_child(ChildScopeConfig::new("worker"))
        .await
        .unwrap();

    assert!(
        Arc::ptr_eq(root.session_tracker(), child.session_tracker()),
        "session_tracker should be shared"
    );
}
