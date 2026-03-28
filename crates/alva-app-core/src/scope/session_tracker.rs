// INPUT:  HashMap, Mutex, uuid, chrono, serde_json
// OUTPUT: SessionTracker, SessionSnapshot
// POS:    Tracks tree-structured session relationships across the spawn tree.

use std::collections::HashMap;
use std::sync::Mutex;

/// Tracks tree-structured session relationships across the spawn tree.
///
/// Each scope gets a unique session_id. Children link to parents,
/// forming a tree that can be traversed and serialized for debugging.
pub struct SessionTracker {
    sessions: Mutex<HashMap<String, SessionNode>>,
}

struct SessionNode {
    id: String,
    parent_id: Option<String>,
    role: String,
    children: Vec<String>,
    created_at: i64,
    completed: bool,
    output_summary: Option<String>,
}

/// Read-only snapshot of a session node for debugging/inspection.
#[derive(Debug, Clone, serde::Serialize)]
pub struct SessionSnapshot {
    pub id: String,
    pub parent_id: Option<String>,
    pub role: String,
    pub children_count: usize,
    pub completed: bool,
    pub output_summary: Option<String>,
}

impl SessionTracker {
    pub fn new() -> Self {
        Self {
            sessions: Mutex::new(HashMap::new()),
        }
    }

    /// Creates a root session for a workspace. Returns the session_id.
    pub fn create_root(&self, workspace: &str) -> String {
        let id = uuid::Uuid::new_v4().to_string();
        let node = SessionNode {
            id: id.clone(),
            parent_id: None,
            role: format!("root:{}", workspace),
            children: Vec::new(),
            created_at: chrono::Utc::now().timestamp_millis(),
            completed: false,
            output_summary: None,
        };
        let mut sessions = self.sessions.lock().unwrap();
        sessions.insert(id.clone(), node);
        id
    }

    /// Creates a child session under `parent_session_id` with the given role.
    /// Returns the new child's session_id.
    pub fn create_child(&self, parent_session_id: &str, role: &str) -> String {
        let id = uuid::Uuid::new_v4().to_string();
        let node = SessionNode {
            id: id.clone(),
            parent_id: Some(parent_session_id.to_owned()),
            role: role.to_owned(),
            children: Vec::new(),
            created_at: chrono::Utc::now().timestamp_millis(),
            completed: false,
            output_summary: None,
        };
        let mut sessions = self.sessions.lock().unwrap();
        sessions.insert(id.clone(), node);
        if let Some(parent) = sessions.get_mut(parent_session_id) {
            parent.children.push(id.clone());
        }
        id
    }

    /// Returns the parent's session_id, or None if root.
    pub fn parent_of(&self, session_id: &str) -> Option<String> {
        let sessions = self.sessions.lock().unwrap();
        sessions.get(session_id)?.parent_id.clone()
    }

    /// Lists the session_ids of all direct children.
    pub fn children_of(&self, session_id: &str) -> Vec<String> {
        let sessions = self.sessions.lock().unwrap();
        sessions
            .get(session_id)
            .map(|n| n.children.clone())
            .unwrap_or_default()
    }

    /// Marks a session as completed with an output summary.
    pub fn mark_completed(&self, session_id: &str, output_summary: &str) {
        let mut sessions = self.sessions.lock().unwrap();
        if let Some(node) = sessions.get_mut(session_id) {
            node.completed = true;
            node.output_summary = Some(output_summary.to_owned());
        }
    }

    /// Returns a debug snapshot of a single session node.
    pub fn snapshot(&self, session_id: &str) -> SessionSnapshot {
        let sessions = self.sessions.lock().unwrap();
        match sessions.get(session_id) {
            Some(node) => SessionSnapshot {
                id: node.id.clone(),
                parent_id: node.parent_id.clone(),
                role: node.role.clone(),
                children_count: node.children.len(),
                completed: node.completed,
                output_summary: node.output_summary.clone(),
            },
            None => SessionSnapshot {
                id: session_id.to_owned(),
                parent_id: None,
                role: String::new(),
                children_count: 0,
                completed: false,
                output_summary: None,
            },
        }
    }

    /// Serializes the entire tree rooted at `root_session_id` as JSON.
    pub fn tree_json(&self, root_session_id: &str) -> serde_json::Value {
        let sessions = self.sessions.lock().unwrap();
        Self::build_tree_json(&sessions, root_session_id)
    }

    fn build_tree_json(
        sessions: &HashMap<String, SessionNode>,
        node_id: &str,
    ) -> serde_json::Value {
        match sessions.get(node_id) {
            Some(node) => {
                let children: Vec<serde_json::Value> = node
                    .children
                    .iter()
                    .map(|child_id| Self::build_tree_json(sessions, child_id))
                    .collect();

                serde_json::json!({
                    "id": node.id,
                    "role": node.role,
                    "parent_id": node.parent_id,
                    "completed": node.completed,
                    "output_summary": node.output_summary,
                    "created_at": node.created_at,
                    "children": children,
                })
            }
            None => serde_json::json!(null),
        }
    }
}

impl Default for SessionTracker {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn root_session_has_no_parent() {
        let tracker = SessionTracker::new();
        let root = tracker.create_root("test-workspace");
        assert!(tracker.parent_of(&root).is_none());
    }

    #[test]
    fn child_session_links_to_parent() {
        let tracker = SessionTracker::new();
        let root = tracker.create_root("ws");
        let child = tracker.create_child(&root, "planner");
        assert_eq!(tracker.parent_of(&child), Some(root.clone()));
    }

    #[test]
    fn children_listed_under_parent() {
        let tracker = SessionTracker::new();
        let root = tracker.create_root("ws");
        let _c1 = tracker.create_child(&root, "planner");
        let _c2 = tracker.create_child(&root, "coder");
        let children = tracker.children_of(&root);
        assert_eq!(children.len(), 2);
    }

    #[test]
    fn grandchildren() {
        let tracker = SessionTracker::new();
        let root = tracker.create_root("ws");
        let child = tracker.create_child(&root, "planner");
        let grandchild = tracker.create_child(&child, "researcher");
        assert_eq!(tracker.parent_of(&grandchild), Some(child.clone()));
        assert_eq!(tracker.children_of(&child).len(), 1);
    }

    #[test]
    fn mark_completed() {
        let tracker = SessionTracker::new();
        let root = tracker.create_root("ws");
        let child = tracker.create_child(&root, "planner");
        tracker.mark_completed(&child, "spec written");
        let snap = tracker.snapshot(&child);
        assert!(snap.completed);
        assert_eq!(snap.output_summary, Some("spec written".into()));
    }

    #[test]
    fn snapshot_fields() {
        let tracker = SessionTracker::new();
        let root = tracker.create_root("ws");
        let _c1 = tracker.create_child(&root, "planner");
        let snap = tracker.snapshot(&root);
        assert_eq!(snap.children_count, 1);
        assert!(!snap.completed);
        assert!(snap.parent_id.is_none());
    }

    #[test]
    fn tree_json_serializes() {
        let tracker = SessionTracker::new();
        let root = tracker.create_root("ws");
        let c1 = tracker.create_child(&root, "planner");
        let _c2 = tracker.create_child(&root, "coder");
        let _gc = tracker.create_child(&c1, "researcher");

        let json = tracker.tree_json(&root);
        assert!(json["children"].is_array());
        assert_eq!(json["children"].as_array().unwrap().len(), 2);
    }
}
