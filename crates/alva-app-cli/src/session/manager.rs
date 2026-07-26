// INPUT:  json_file_session::JsonFileAgentSession, serde, serde_json, std::fs
// OUTPUT: fallible JsonFileSessionManager, SessionSummary, eval_config_snapshot records
// POS:    CLI session directory manager — knows about .alva/sessions/ layout,
//         UUID/path validation, private atomic metadata, session construction,
//         and agent config snapshots.

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

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
    index_lock: Mutex<()>,
}

impl JsonFileSessionManager {
    /// Create a manager for the given workspace, ensuring the sessions
    /// directory exists.
    pub fn for_workspace(workspace: &Path) -> Result<Self, String> {
        let canonical_workspace = workspace.canonicalize().map_err(|error| {
            format!("cannot resolve workspace {}: {error}", workspace.display())
        })?;
        let alva_dir = canonical_workspace.join(".alva");
        ensure_workspace_dir(&alva_dir, &canonical_workspace)?;
        let sessions_dir = alva_dir.join("sessions");
        ensure_workspace_dir(&sessions_dir, &canonical_workspace)?;
        let index_path = sessions_dir.join("index.json");
        ensure_private_dir(&sessions_dir).map_err(|error| {
            format!(
                "failed to prepare sessions directory {}: {error}",
                sessions_dir.display()
            )
        })?;
        Ok(Self {
            sessions_dir,
            index_path,
            index_lock: Mutex::new(()),
        })
    }

    fn load_index(&self) -> Vec<SessionSummary> {
        if !self.index_path.exists() {
            return Vec::new();
        }
        if fs::symlink_metadata(&self.index_path)
            .map(|metadata| metadata.file_type().is_symlink() || !metadata.is_file())
            .unwrap_or(true)
        {
            return Vec::new();
        }
        fs::read_to_string(&self.index_path)
            .ok()
            .and_then(|s| serde_json::from_str(&s).ok())
            .map(|entries: Vec<SessionSummary>| {
                entries
                    .into_iter()
                    .filter(|entry| valid_session_id(&entry.session_id))
                    .collect()
            })
            .unwrap_or_default()
    }

    fn save_index(&self, entries: &[SessionSummary]) {
        if let Err(e) = ensure_private_dir(&self.sessions_dir) {
            tracing::warn!(
                dir = ?self.sessions_dir,
                error = %e,
                "save_index: failed to create sessions dir; index write will likely also fail",
            );
        }
        match serde_json::to_string_pretty(entries) {
            Ok(json) => {
                if let Err(e) = atomic_write_private(&self.index_path, json.as_bytes()) {
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

    fn session_path(&self, session_id: &str) -> Option<PathBuf> {
        valid_session_id(session_id).then(|| self.sessions_dir.join(format!("{}.json", session_id)))
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

        let path = self
            .session_path(&session_id)
            .expect("internally generated session id must be a canonical UUID");

        // Construct a fresh session with the real path.
        let real_session = Arc::new(JsonFileAgentSession::with_id_at(
            path.clone(),
            session_id.clone(),
        ));
        // Force-persist the empty file so the session_id is visible on disk.
        let _ = real_session.flush().await;

        // Update the index.
        let _guard = self.index_lock.lock().unwrap_or_else(|e| e.into_inner());
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
        let path = self.session_path(session_id)?;
        if fs::symlink_metadata(&path)
            .map(|metadata| metadata.file_type().is_symlink() || !metadata.is_file())
            .unwrap_or(true)
        {
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
        let Some(path) = self.session_path(session_id) else {
            return;
        };
        let _ = fs::remove_file(&path);
        let _guard = self.index_lock.lock().unwrap_or_else(|e| e.into_inner());
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
        if !valid_session_id(session_id) {
            return;
        }
        let _guard = self.index_lock.lock().unwrap_or_else(|e| e.into_inner());
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
        max_iterations: u32,
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
            "max_iterations": max_iterations,
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
        if !valid_session_id(&session_id) {
            tracing::warn!(%session_id, "refusing to write run record with invalid session id");
            return;
        }
        let path = self.sessions_dir.join(format!("{}.run.json", session_id));
        match serde_json::to_string_pretty(&record) {
            Ok(json) => {
                if let Err(e) = atomic_write_private(&path, json.as_bytes()) {
                    tracing::warn!(session_id = %session_id, error = %e, "write run record failed");
                }
            }
            Err(e) => {
                tracing::warn!(session_id = %session_id, error = %e, "serialize run record failed");
            }
        }
    }
}

fn valid_session_id(session_id: &str) -> bool {
    uuid::Uuid::parse_str(session_id)
        .map(|id| id.to_string() == session_id)
        .unwrap_or(false)
}

fn ensure_private_dir(path: &Path) -> std::io::Result<()> {
    match fs::symlink_metadata(path) {
        Ok(metadata) if metadata.file_type().is_symlink() => {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                format!("refusing symlinked private directory: {}", path.display()),
            ));
        }
        Ok(metadata) if !metadata.is_dir() => {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                format!("private path is not a directory: {}", path.display()),
            ));
        }
        Ok(_) => {}
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => fs::create_dir_all(path)?,
        Err(error) => return Err(error),
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt as _;
        fs::set_permissions(path, fs::Permissions::from_mode(0o700))?;
    }
    Ok(())
}

fn ensure_workspace_dir(path: &Path, canonical_workspace: &Path) -> Result<(), String> {
    if let Ok(metadata) = fs::symlink_metadata(path) {
        if metadata.file_type().is_symlink() {
            return Err(format!(
                "refusing symlinked session directory component: {}",
                path.display()
            ));
        }
        if !metadata.is_dir() {
            return Err(format!(
                "session path is not a directory: {}",
                path.display()
            ));
        }
    } else {
        fs::create_dir(path)
            .map_err(|error| format!("create session directory {}: {error}", path.display()))?;
    }
    let canonical = path
        .canonicalize()
        .map_err(|error| format!("resolve session directory {}: {error}", path.display()))?;
    if !canonical.starts_with(canonical_workspace) {
        return Err(format!(
            "session directory resolves outside workspace: {}",
            path.display()
        ));
    }
    Ok(())
}

fn atomic_write_private(path: &Path, contents: &[u8]) -> std::io::Result<()> {
    use std::io::Write as _;

    let parent = path.parent().ok_or_else(|| {
        std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            "session metadata file must have a parent directory",
        )
    })?;
    ensure_private_dir(parent)?;
    let file_name = path
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("session.json");
    let temp_path = parent.join(format!(
        ".{file_name}.{}.tmp",
        uuid::Uuid::new_v4().simple()
    ));
    let result = (|| {
        let mut options = fs::OpenOptions::new();
        options.write(true).create_new(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt as _;
            options.mode(0o600);
        }
        let mut file = options.open(&temp_path)?;
        file.write_all(contents)?;
        file.sync_all()?;
        drop(file);
        #[cfg(windows)]
        if path.exists() {
            fs::remove_file(path)?;
        }
        fs::rename(&temp_path, path)
    })();
    if result.is_err() {
        let _ = fs::remove_file(&temp_path);
    }
    result
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
        let manager = JsonFileSessionManager::for_workspace(tmp.path()).unwrap();
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
            .append_config_snapshot_if_needed(&session, &agent, "test-model", 7)
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
        assert_eq!(
            snapshot.get("max_iterations").and_then(|v| v.as_u64()),
            Some(7),
            "snapshot must record the invocation's actual turn budget"
        );
    }

    #[tokio::test]
    async fn invalid_session_id_cannot_escape_workspace_on_load_or_delete() {
        let tmp = tempfile::tempdir().unwrap();
        let manager = JsonFileSessionManager::for_workspace(tmp.path()).unwrap();
        let victim = tmp.path().join("victim.json");
        std::fs::write(&victim, "must survive").unwrap();

        assert!(manager.load("../../victim").await.is_none());
        manager.delete("../../victim");
        assert_eq!(std::fs::read_to_string(victim).unwrap(), "must survive");
    }

    #[cfg(unix)]
    #[test]
    fn manager_creates_private_sessions_directory() {
        use std::os::unix::fs::PermissionsExt as _;

        let tmp = tempfile::tempdir().unwrap();
        let manager = JsonFileSessionManager::for_workspace(tmp.path()).unwrap();
        assert_eq!(
            std::fs::metadata(manager.sessions_dir)
                .unwrap()
                .permissions()
                .mode()
                & 0o777,
            0o700
        );
    }

    #[cfg(unix)]
    #[test]
    fn manager_rejects_symlinked_alva_directory() {
        use std::os::unix::fs::symlink;

        let workspace = tempfile::tempdir().unwrap();
        let outside = tempfile::tempdir().unwrap();
        symlink(outside.path(), workspace.path().join(".alva")).unwrap();

        let error = JsonFileSessionManager::for_workspace(workspace.path())
            .err()
            .expect("symlinked metadata directory must be rejected");
        assert!(error.contains("symlinked session directory"), "{error}");
        assert!(
            !outside.path().join("sessions").exists(),
            "manager must not create files through the symlink"
        );
    }
}
