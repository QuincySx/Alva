//! File-based session storage — JSON files under `.alva/sessions/`.
//!
//! Layout:
//!   .alva/
//!   └── sessions/
//!       ├── index.json          ← session metadata list
//!       └── {session_id}.json   ← conversation messages

use std::fs;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use alva_types::AgentMessage;

/// Metadata for one session.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionMeta {
    pub id: String,
    pub created_at: i64,
    pub updated_at: i64,
    pub message_count: usize,
    /// First user prompt (truncated, for display in list).
    pub summary: String,
}

/// File-based session store scoped to a working directory.
pub struct SessionStore {
    sessions_dir: PathBuf,
    index_path: PathBuf,
}

impl SessionStore {
    /// Create a store for the given workspace directory.
    /// Creates `.alva/sessions/` if needed.
    pub fn for_workspace(workspace: &Path) -> Self {
        let sessions_dir = workspace.join(".alva").join("sessions");
        let index_path = sessions_dir.join("index.json");
        Self {
            sessions_dir,
            index_path,
        }
    }

    fn ensure_dir(&self) {
        let _ = fs::create_dir_all(&self.sessions_dir);
    }

    // ── Index operations ────────────────────────────────────────────

    fn load_index(&self) -> Vec<SessionMeta> {
        if !self.index_path.exists() {
            return Vec::new();
        }
        fs::read_to_string(&self.index_path)
            .ok()
            .and_then(|s| serde_json::from_str(&s).ok())
            .unwrap_or_default()
    }

    fn save_index(&self, index: &[SessionMeta]) {
        self.ensure_dir();
        let json = serde_json::to_string_pretty(index).unwrap_or_default();
        let _ = fs::write(&self.index_path, json);
    }

    // ── Public API ──────────────────────────────────────────────────

    /// List all sessions, most recent first.
    pub fn list(&self) -> Vec<SessionMeta> {
        let mut index = self.load_index();
        index.sort_by(|a, b| b.updated_at.cmp(&a.updated_at));
        index
    }

    /// Create a new session and return its ID.
    pub fn create(&self, summary: &str) -> String {
        let id = uuid::Uuid::new_v4().to_string();
        let now = chrono::Utc::now().timestamp_millis();

        let meta = SessionMeta {
            id: id.clone(),
            created_at: now,
            updated_at: now,
            message_count: 0,
            summary: truncate(summary, 80),
        };

        let mut index = self.load_index();
        index.push(meta);
        self.save_index(&index);

        // Create empty messages file
        self.save_messages(&id, &[]);

        id
    }

    /// Get the most recent session ID (if any).
    pub fn latest(&self) -> Option<String> {
        self.list().first().map(|m| m.id.clone())
    }

    /// Save conversation messages for a session.
    pub fn save_messages(&self, session_id: &str, messages: &[AgentMessage]) {
        self.ensure_dir();
        let path = self.sessions_dir.join(format!("{}.json", session_id));
        let json = serde_json::to_string_pretty(messages).unwrap_or_default();
        let _ = fs::write(path, json);

        // Update index
        let mut index = self.load_index();
        if let Some(meta) = index.iter_mut().find(|m| m.id == session_id) {
            meta.updated_at = chrono::Utc::now().timestamp_millis();
            meta.message_count = messages.len();
            // Update summary from first user message if empty
            if meta.summary.is_empty() {
                if let Some(first_user) = messages.iter().find_map(|m| {
                    if let AgentMessage::Standard(msg) = m {
                        if msg.role == alva_types::MessageRole::User {
                            return Some(msg.text_content());
                        }
                    }
                    None
                }) {
                    meta.summary = truncate(&first_user, 80);
                }
            }
        }
        self.save_index(&index);
    }

    /// Load conversation messages for a session.
    pub fn load_messages(&self, session_id: &str) -> Vec<AgentMessage> {
        let path = self.sessions_dir.join(format!("{}.json", session_id));
        if !path.exists() {
            return Vec::new();
        }
        fs::read_to_string(path)
            .ok()
            .and_then(|s| serde_json::from_str(&s).ok())
            .unwrap_or_default()
    }

    /// Delete a session.
    #[allow(dead_code)]
    pub fn delete(&self, session_id: &str) {
        let path = self.sessions_dir.join(format!("{}.json", session_id));
        let _ = fs::remove_file(path);

        let mut index = self.load_index();
        index.retain(|m| m.id != session_id);
        self.save_index(&index);
    }
}

fn truncate(s: &str, max: usize) -> String {
    let truncated: String = s.chars().take(max).collect();
    if truncated.len() < s.len() {
        format!("{}...", truncated)
    } else {
        truncated
    }
}
