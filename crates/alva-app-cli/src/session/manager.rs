// INPUT:  json_file_session::JsonFileAgentSession, serde, serde_json, std::fs
// OUTPUT: JsonFileSessionManager, SessionSummary, eval_config_snapshot records
// POS:    CLI session directory manager — knows about .alva/sessions/ layout,
//         index.json, JsonFileAgentSession construction, and agent config snapshots.

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use serde::{Deserialize, Serialize};

use alva_kernel_abi::agent_session::{AgentSession, EventQuery};

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
        if let Err(e) = fs::create_dir_all(&self.sessions_dir) {
            tracing::warn!(
                dir = ?self.sessions_dir,
                error = %e,
                "save_index: failed to create sessions dir; index write will likely also fail",
            );
        }
        match serde_json::to_string_pretty(entries) {
            Ok(json) => {
                if let Err(e) = fs::write(&self.index_path, &json) {
                    tracing::warn!(
                        path = ?self.index_path,
                        error = %e,
                        "save_index: failed to write index — session list may show stale state",
                    );
                }
            }
            Err(e) => {
                tracing::warn!(
                    error = %e,
                    entries = entries.len(),
                    "save_index: failed to serialize entries; index unchanged",
                );
            }
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
    pub fn refresh_summary(
        &self,
        session_id: &str,
        event_count: usize,
        preview_override: Option<&str>,
    ) {
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

    /// Append an `eval_config_snapshot` system event to the session if it
    /// doesn't already have one. Mirrors what Tauri's `ensure_agent` does on
    /// the first send_message — without it, RunRecord.config_snapshot stays
    /// empty (no system_prompt segments / no tool_definitions), and external
    /// tools can't see what the agent was actually configured with for the run.
    pub async fn append_config_snapshot_if_needed(
        &self,
        session: &Arc<JsonFileAgentSession>,
        agent: &alva_app_core::BaseAgent,
        model_id: &str,
    ) {
        let events = session
            .query(&EventQuery {
                limit: usize::MAX,
                ..Default::default()
            })
            .await;
        let already = events.iter().any(|m| {
            m.event.event_type == "system"
                && m.event
                    .data
                    .as_ref()
                    .and_then(|d| d.get("type"))
                    .and_then(|v| v.as_str())
                    == Some("eval_config_snapshot")
        });
        if already {
            return;
        }

        let tool_definitions = agent.tool_registry().definitions();
        let tool_names = agent.tool_names();
        let system_prompt_segments = agent.system_prompt_segments().await;
        let assembly = agent.assembly_snapshot();
        let plugin_names = agent.plugin_names();
        let middleware_names = agent.middleware_names();
        let direct_middleware_names = assembly.direct_middleware_names.clone();
        let snapshot = serde_json::json!({
            "type": "eval_config_snapshot",
            "system_prompt": system_prompt_segments,
            "model_id": model_id,
            "tool_names": tool_names,
            "tool_definitions": tool_definitions,
            "skill_names": Vec::<String>::new(),
            "max_iterations": 20u32,
            "plugin_names": plugin_names,
            "plugin_assembly": assembly.plugins,
            "middleware_names": middleware_names,
            "direct_middleware_names": direct_middleware_names,
        });
        let event = alva_kernel_abi::agent_session::SessionEvent::system(snapshot);
        // `AgentSession::append` returns `()` (infallible by trait
        // contract); concrete impls absorb their own I/O errors.
        session.append(event).await;
    }

    /// Build a structured `RunRecord` from the session's full event log and
    /// persist it next to `<session_id>.json` as `<session_id>.run.json`.
    /// This is the same projection Tauri builds on demand for its Inspector;
    /// CLI dumps it to disk so external tools can load the structured view
    /// without re-implementing the events → record reduction. Errors are
    /// logged but not propagated — the raw event log is still on disk and
    /// the run record can be rebuilt later.
    pub async fn write_run_record(&self, session: &Arc<JsonFileAgentSession>) {
        let session_id = session.session_id().to_string();
        let events: Vec<alva_kernel_abi::agent_session::SessionEvent> = session
            .query(&EventQuery {
                limit: usize::MAX,
                ..Default::default()
            })
            .await
            .into_iter()
            .map(|m| m.event)
            .collect();
        let record = alva_app_core::session_projection::build_run_record(&events);
        let path = self.sessions_dir.join(format!("{}.run.json", session_id));
        match serde_json::to_string_pretty(&record) {
            Ok(json) => {
                if let Err(e) = fs::write(&path, json) {
                    tracing::warn!(session_id = %session_id, error = %e, "write run record failed");
                }
            }
            Err(e) => {
                tracing::warn!(session_id = %session_id, error = %e, "serialize run record failed");
            }
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

#[cfg(test)]
mod tests {
    //! Tests for `truncate_preview`, the session-list display helper.
    //! Already correctly chars-based (unlike the byte-slice bugs the
    //! UTF-8 panic bus fixed in L56-L63); these tests pin down the
    //! 80-char limit + "..." marker contract.
    use super::*;
    use std::sync::Arc;

    #[test]
    fn truncate_preview_empty_string_returned_unchanged() {
        assert_eq!(truncate_preview(""), "");
    }

    #[test]
    fn truncate_preview_short_ascii_returned_unchanged_no_marker() {
        assert_eq!(truncate_preview("hello"), "hello");
    }

    #[test]
    fn truncate_preview_ascii_exact_80_chars_no_marker() {
        // s.chars().take(80).collect() returns the whole string when
        // s has exactly 80 chars → truncated.len() == s.len() → no marker.
        let s = "a".repeat(80);
        assert_eq!(truncate_preview(&s), s);
        assert!(
            !truncate_preview(&s).ends_with("..."),
            "no marker at exact boundary"
        );
    }

    #[test]
    fn truncate_preview_ascii_over_80_chars_truncates_with_marker() {
        let s = "a".repeat(120);
        let out = truncate_preview(&s);
        // First 80 chars + "..." = 83 bytes
        assert_eq!(out.len(), 83);
        assert!(out.ends_with("..."));
        let kept = out.strip_suffix("...").unwrap();
        assert_eq!(kept.len(), 80);
        assert!(kept.chars().all(|c| c == 'a'));
    }

    #[test]
    fn truncate_preview_cjk_counts_chars_not_bytes() {
        // 100 CJK chars = 300 bytes. chars().take(80) keeps 80 chars
        // = 240 bytes. 240 < 300 → marker added.
        let s = "中".repeat(100);
        assert_eq!(s.len(), 300);
        let out = truncate_preview(&s);
        assert!(out.ends_with("..."));
        let kept = out.strip_suffix("...").unwrap();
        // Should be 80 CJK chars × 3 bytes = 240 bytes
        assert_eq!(kept.chars().count(), 80);
        assert_eq!(kept.len(), 240);
    }

    #[test]
    fn truncate_preview_emoji_treated_as_one_char() {
        // 81 4-byte emojis = 324 bytes. chars().take(80) keeps 80 emojis
        // = 320 bytes. 320 < 324 → marker added.
        let s = "🦀".repeat(81);
        let out = truncate_preview(&s);
        assert!(out.ends_with("..."));
        let kept = out.strip_suffix("...").unwrap();
        assert_eq!(kept.chars().count(), 80, "exactly 80 emojis kept");
    }

    #[tokio::test]
    async fn config_snapshot_records_actual_plugins_and_middleware() {
        let tmp = tempfile::tempdir().unwrap();
        let manager = JsonFileSessionManager::for_workspace(tmp.path());
        let session = manager.create("snapshot test").await;
        let model = Arc::new(alva_llm_provider::OpenAIChatProvider::new(
            alva_llm_provider::ProviderConfig {
                api_key: "test-key".to_string(),
                model: "test-model".to_string(),
                base_url: "https://example.invalid/v1".to_string(),
                max_tokens: 128,
                custom_headers: Default::default(),
                kind: Some("openai-chat".to_string()),
            },
        ));

        let agent = alva_app_core::BaseAgent::builder()
            .workspace(tmp.path())
            .plugin(Box::new(alva_app_core::extension::CorePlugin))
            .middleware(Arc::new(
                alva_kernel_core::builtins::LoopDetectionMiddleware::new(),
            ))
            .build(model)
            .await
            .expect("agent builds");

        manager
            .append_config_snapshot_if_needed(&session, &agent, "test-model")
            .await;

        let events = session
            .query(&EventQuery {
                limit: usize::MAX,
                ..Default::default()
            })
            .await;
        let snapshot = events
            .iter()
            .find_map(|m| {
                let data = m.event.data.as_ref()?;
                (data.get("type").and_then(|v| v.as_str()) == Some("eval_config_snapshot"))
                    .then_some(data)
            })
            .expect("config snapshot event");

        let plugins = snapshot
            .get("plugin_names")
            .and_then(|v| v.as_array())
            .expect("plugin_names array");
        assert!(
            snapshot.get("extension_names").is_none(),
            "extension_names must not be emitted in the new snapshot schema"
        );
        assert!(
            plugins.iter().any(|v| v.as_str() == Some("core")),
            "actual plugin list should include core: {plugins:?}"
        );
        assert!(
            plugins.iter().any(|v| v.as_str() == Some("security")),
            "default security plugin should be recorded: {plugins:?}"
        );
        let plugin_assembly = snapshot
            .get("plugin_assembly")
            .and_then(|v| v.as_array())
            .expect("plugin_assembly array");
        assert!(
            plugin_assembly
                .iter()
                .any(|v| v.get("name").and_then(|n| n.as_str()) == Some("core")),
            "plugin assembly should include structured core contribution: {plugin_assembly:?}"
        );

        let middleware = snapshot
            .get("middleware_names")
            .and_then(|v| v.as_array())
            .expect("middleware_names array");
        let direct_middleware = snapshot
            .get("direct_middleware_names")
            .and_then(|v| v.as_array())
            .expect("direct_middleware_names array");
        assert!(
            !middleware.is_empty(),
            "middleware_names should record actual stack"
        );
        assert!(
            middleware
                .iter()
                .any(|v| v.as_str() == Some("builtins_loop_detection")),
            "explicit loop detection middleware should be recorded: {middleware:?}"
        );
        assert_eq!(
            direct_middleware,
            &[serde_json::json!("builtins_loop_detection")],
            "direct middleware should be attributed separately"
        );
    }
}
