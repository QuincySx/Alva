// INPUT:  std::collections::HashMap, tokio::sync::Mutex, Arc, ScopeId, Blackboard
// OUTPUT: BoardRegistry
// POS:    Manages Blackboard instances scoped to SpawnScope IDs with visibility rules.

use std::collections::HashMap;
use std::sync::Arc;

use tokio::sync::Mutex;

use alva_kernel_abi::scope::ScopeId;
use super::blackboard::Blackboard;

/// Manages Blackboard instances scoped to SpawnScope IDs.
///
/// # Visibility rules
///
/// 1. Same scope + same board_id → same Blackboard instance (siblings share)
/// 2. Different scope (no parent relationship) → different Blackboard even with same board_id (isolated)
/// 3. Child scope can read parent scope's board via `parent_board_for()` (upward read-only)
/// 4. Children's internal boards are invisible to parent (downward private)
pub struct BoardRegistry {
    /// (scope_id_str, board_id) → Blackboard
    boards: Mutex<HashMap<(String, String), Arc<Blackboard>>>,
    /// child_scope_id → parent_scope_id
    parents: Mutex<HashMap<String, String>>,
}

impl BoardRegistry {
    pub fn new() -> Self {
        Self {
            boards: Mutex::new(HashMap::new()),
            parents: Mutex::new(HashMap::new()),
        }
    }

    /// Register a parent-child relationship.
    pub async fn set_parent(&self, child: &ScopeId, parent: &ScopeId) {
        let mut parents = self.parents.lock().await;
        parents.insert(child.as_str().to_owned(), parent.as_str().to_owned());
    }

    /// Get or create a board for a scope. Same scope + same board_id = same instance.
    pub async fn get_or_create(&self, scope_id: &ScopeId, board_id: &str) -> Arc<Blackboard> {
        let mut boards = self.boards.lock().await;
        let key = (scope_id.as_str().to_owned(), board_id.to_owned());
        boards
            .entry(key)
            .or_insert_with(|| Arc::new(Blackboard::new()))
            .clone()
    }

    /// Get the parent scope's board (for read-only access from a child).
    /// Returns None if no parent or parent has no board with that ID.
    pub async fn parent_board_for(
        &self,
        child_scope_id: &ScopeId,
        board_id: &str,
    ) -> Option<Arc<Blackboard>> {
        let parents = self.parents.lock().await;
        let parent_id = parents.get(child_scope_id.as_str())?;
        let key = (parent_id.clone(), board_id.to_owned());
        drop(parents);

        let boards = self.boards.lock().await;
        boards.get(&key).cloned()
    }
}

impl Default for BoardRegistry {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::scope::blackboard::BoardMessage;

    #[tokio::test]
    async fn same_scope_shares_board() {
        let reg = BoardRegistry::new();
        let scope_a = ScopeId::new();

        let b1 = reg.get_or_create(&scope_a, "team-1").await;
        let b2 = reg.get_or_create(&scope_a, "team-1").await;

        b1.post(BoardMessage::new("a", "hello")).await;
        assert_eq!(b2.message_count().await, 1);
    }

    #[tokio::test]
    async fn different_board_id_different_board() {
        let reg = BoardRegistry::new();
        let scope = ScopeId::new();

        let b1 = reg.get_or_create(&scope, "team-1").await;
        let b2 = reg.get_or_create(&scope, "team-2").await;

        b1.post(BoardMessage::new("a", "hello")).await;
        assert_eq!(b2.message_count().await, 0);
    }

    #[tokio::test]
    async fn different_scope_isolated() {
        let reg = BoardRegistry::new();
        let scope_a = ScopeId::new();
        let scope_b = ScopeId::new();

        let b1 = reg.get_or_create(&scope_a, "work").await;
        let b2 = reg.get_or_create(&scope_b, "work").await;

        b1.post(BoardMessage::new("a", "hello")).await;
        assert_eq!(b2.message_count().await, 0);
    }

    #[tokio::test]
    async fn child_reads_parent_board() {
        let reg = BoardRegistry::new();
        let parent = ScopeId::new();
        let child = ScopeId::new();

        reg.set_parent(&child, &parent).await;

        let parent_board = reg.get_or_create(&parent, "team").await;
        parent_board.post(BoardMessage::new("boss", "task")).await;

        let view = reg.parent_board_for(&child, "team").await;
        assert!(view.is_some());
        assert_eq!(view.unwrap().message_count().await, 1);
    }

    #[tokio::test]
    async fn no_parent_returns_none() {
        let reg = BoardRegistry::new();
        let orphan = ScopeId::new();
        let view = reg.parent_board_for(&orphan, "team").await;
        assert!(view.is_none());
    }

    #[tokio::test]
    async fn parent_cannot_see_child_board() {
        let reg = BoardRegistry::new();
        let parent = ScopeId::new();
        let child = ScopeId::new();

        reg.set_parent(&child, &parent).await;

        let child_board = reg.get_or_create(&child, "internal").await;
        child_board
            .post(BoardMessage::new("child", "secret"))
            .await;

        // Parent has no board named "internal"
        let parent_view = reg.get_or_create(&parent, "internal").await;
        // This is a DIFFERENT board instance (parent's own "internal", not child's)
        assert_eq!(parent_view.message_count().await, 0);
    }
}
