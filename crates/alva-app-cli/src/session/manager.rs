// INPUT:  json_file_session::JsonFileAgentSession, serde, serde_json, std::fs
// OUTPUT: JsonFileSessionManager, SessionSummary
// POS:    CLI session directory manager — knows about .alva/sessions/ layout,
//         index.json, and how to construct JsonFileAgentSession instances
//         pointing at the right files.

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use serde::{Deserialize, Serialize};

use alva_kernel_abi::agent_session::AgentSession;

use super::json_file_session::JsonFileAgentSession;

/// Metadata for a session in the index. One entry per session file.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionSummary {
    pub session_id: String,
    pub created_at: i64,
    pub updated_at: i64,
    pub preview: String,
    pub event_count: usize,
}

/// Manages a workspace's `.alva/sessions/` directory.
pub struct JsonFileSessionManager {
    sessions_dir: PathBuf,
    index_path: PathBuf,
}

impl JsonFileSessionManager {
    /// Create a manager for the given workspace, ensuring the sessions
    /// directory exists.
    pub fn for_workspace(workspace: &Path) -> Self {
        let sessions_dir = workspace.join(".alva").join("sessions");
        let index_path = sessions_dir.join("index.json");
        let _ = fs::create_dir_all(&sessions_dir);
        Self {
            sessions_dir,
            index_path,
        }
    }

    fn load_index(&self) -> Vec<SessionSummary> {
        if !self.index_path.exists() {
            return Vec::new();
        }
        fs::read_to_string(&self.index_path)
            .ok()
            .and_then(|s| serde_json::from_str(&s).ok())
            .unwrap_or_default()
    }

    fn save_index(&self, entries: &[SessionSummary]) {
        let _ = fs::create_dir_all(&self.sessions_dir);
        if let Ok(json) = serde_json::to_string_pretty(entries) {
            let _ = fs::write(&self.index_path, json);
        }
    }

    fn session_path(&self, session_id: &str) -> PathBuf {
        self.sessions_dir.join(format!("{}.json", session_id))
    }

    /// List all known sessions, most recently updated first.
    pub fn list(&self) -> Vec<SessionSummary> {
        let mut entries = self.load_index();
        entries.sort_by(|a, b| b.updated_at.cmp(&a.updated_at));
        entries
    }

    /// Return the session_id of the most recently updated session, if any.
    pub fn latest(&self) -> Option<String> {
        self.list().into_iter().next().map(|e| e.session_id)
    }

    /// Create a new session with the given preview text, returning a handle
    /// to a `JsonFileAgentSession` pointing at the new file. The index is
    /// updated immediately so the session shows up in `list()`.
    pub async fn create(&self, preview: &str) -> Arc<JsonFileAgentSession> {
        let now = chrono::Utc::now().timestamp_millis();
        // Create a temporary session just to get a random session_id.
        let temp_session = JsonFileAgentSession::new_at(PathBuf::new());
        let session_id = temp_session.session_id().to_string();
        drop(temp_session);

        let path = self.session_path(&session_id);

        // Construct a fresh session with the real path.
        let real_session = Arc::new(JsonFileAgentSession::with_id_at(
            path.clone(),
            session_id.clone(),
        ));
        // Force-persist the empty file so the session_id is visible on disk.
        let _ = real_session.flush().await;

        // Update the index.
        let mut index = self.load_index();
        index.push(SessionSummary {
            session_id: session_id.clone(),
            created_at: now,
            updated_at: now,
            preview: truncate_preview(preview),
            event_count: 0,
        });
        self.save_index(&index);

        real_session
    }

    /// Load an existing session by id. Returns `None` if the file does not exist.
    pub async fn load(&self, session_id: &str) -> Option<Arc<JsonFileAgentSession>> {
        let path = self.session_path(session_id);
        if !path.exists() {
            return None;
        }
        let session = Arc::new(JsonFileAgentSession::with_id_at(
            path,
            session_id.to_string(),
        ));
        if let Err(e) = session.restore().await {
            tracing::warn!(session_id, error = %e, "failed to restore session");
            return None;
        }
        Some(session)
    }

    /// Delete a session's file and remove it from the index.
    pub fn delete(&self, session_id: &str) {
        let path = self.session_path(session_id);
        let _ = fs::remove_file(&path);
        let mut index = self.load_index();
        index.retain(|e| e.session_id != session_id);
        self.save_index(&index);
    }

    /// Update a session's summary in the index after it's been written to
    /// disk (e.g. after event_count has grown).
    pub fn refresh_summary(&self, session_id: &str, event_count: usize, preview_override: Option<&str>) {
        let mut index = self.load_index();
        if let Some(entry) = index.iter_mut().find(|e| e.session_id == session_id) {
            entry.updated_at = chrono::Utc::now().timestamp_millis();
            entry.event_count = event_count;
            if let Some(p) = preview_override {
                entry.preview = truncate_preview(p);
            }
            self.save_index(&index);
        }
    }
}

fn truncate_preview(s: &str) -> String {
    let max = 80;
    let truncated: String = s.chars().take(max).collect();
    if truncated.len() < s.len() {
        format!("{}...", truncated)
    } else {
        truncated
    }
}
