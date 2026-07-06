// Session lifecycle commands (list/create/switch/delete, workspace mapping,
// raw event log / run record) plus the in-memory session cache loader and the
// `Vec<AgentMessage>` → `ChatEntry` projection. Split from `agent.rs`
// (PR-13a); command names are unchanged (= function names).

use std::sync::Arc;

use serde::Serialize;
use tauri::State;

use alva_kernel_abi::agent_session::{AgentSession, EventQuery};
use alva_kernel_abi::base::content::ContentBlock;
use alva_kernel_abi::base::message::{AgentMessage, Message, MessageRole};

use crate::sqlite_session::{SessionSummary, SqliteEvalSession, SqliteEvalSessionManager};

use super::{
    default_plugin_state, default_workspace_for, now_ms, AppState, SessionEntry, SessionInfo,
};

// ---------------------------------------------------------------------------
// Session cache helpers
// ---------------------------------------------------------------------------

fn summary_to_session_info(s: SessionSummary, manager: &SqliteEvalSessionManager) -> SessionInfo {
    let title = if s.preview.is_empty() {
        "New chat".to_string()
    } else {
        s.preview
    };
    let workspace_path = s
        .workspace_id
        .as_deref()
        .and_then(|wid| manager.get_workspace(wid))
        .map(|ws| ws.path);
    SessionInfo {
        id: s.session_id,
        title,
        created_at_ms: s.created_at as u64,
        updated_at_ms: s.created_at as u64,
        workspace_path,
    }
}

/// Create or find a workspace record for a path, and link it to the session.
fn link_workspace(manager: &SqliteEvalSessionManager, session_id: &str, path: &str) {
    let workspace_id = if let Some(existing) = manager.find_workspace_by_path(path) {
        existing.workspace_id
    } else {
        let id = format!(
            "ws-{:x}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_nanos()
        );
        manager.upsert_workspace(&crate::sqlite_session::StoredWorkspace {
            workspace_id: id.clone(),
            path: path.to_string(),
            permissions: "{}".into(),
            created_at: chrono::Utc::now().timestamp_millis(),
        });
        id
    };
    manager.set_session_workspace(session_id, &workspace_id);
}

/// Fast check: does this session already have an `eval_config_snapshot`
/// system event? Used when loading a session from disk to avoid appending
/// a duplicate snapshot on the next send.
async fn session_has_config_snapshot(session: &Arc<SqliteEvalSession>) -> bool {
    let events = session
        .query(&EventQuery {
            limit: usize::MAX,
            ..Default::default()
        })
        .await;
    events.iter().any(|m| {
        m.event.event_type == "system"
            && m.event
                .data
                .as_ref()
                .and_then(|d| d.get("type"))
                .and_then(|v| v.as_str())
                == Some("eval_config_snapshot")
    })
}

/// Ensure a session Arc is in the in-memory cache. Loads from db if it's
/// not already there. Returns the cached (or freshly-loaded) Arc plus a
/// best-effort `SessionInfo`.
pub(crate) async fn ensure_session_loaded(
    state: &State<'_, AppState>,
    id: &str,
) -> Result<Arc<SqliteEvalSession>, String> {
    {
        let sessions = state.sessions.read().await;
        if let Some(entry) = sessions.iter().find(|e| e.info.id == id) {
            return Ok(entry.session.clone());
        }
    }

    let loaded = state
        .session_manager
        .load_session(id)
        .await
        .ok_or_else(|| format!("session not found: {id}"))?;
    let snapshot_done = session_has_config_snapshot(&loaded).await;

    // Look up info from the db's sessions table so the title survives.
    let info = {
        let manager = state.session_manager.clone();
        let target = id.to_string();
        tokio::task::spawn_blocking(move || {
            let found = manager
                .list_sessions()
                .into_iter()
                .find(|s| s.session_id == target);
            found.map(|s| summary_to_session_info(s, &manager))
        })
        .await
        .ok()
        .flatten()
        .unwrap_or_else(|| SessionInfo {
            id: id.to_string(),
            title: "New chat".to_string(),
            created_at_ms: now_ms(),
            updated_at_ms: now_ms(),
            workspace_path: None,
        })
    };

    let mut sessions = state.sessions.write().await;
    // Guard against a concurrent insert racing us.
    if !sessions.iter().any(|e| e.info.id == id) {
        sessions.insert(
            0,
            SessionEntry {
                info,
                session: loaded.clone(),
                config_snapshot_appended: snapshot_done,
            },
        );
    }
    Ok(loaded)
}

// ---------------------------------------------------------------------------
// Chat projection type
// ---------------------------------------------------------------------------

/// A single rendered chat bubble in the Home chat area. Discriminated
/// union — the frontend renders each variant differently:
///
/// - `User` / `Assistant` / `System` → a plain bubble with text.
/// - `Thinking` → collapsible "思考" block.
/// - `ToolCall` → collapsible block with the tool name, arguments, and
///    (once the paired `tool_result` block arrives) its output + error flag.
/// - `Error` → red bubble for surfaced errors.
///
/// Projection from `Vec<AgentMessage>` happens in `messages_to_chat_entries`,
/// which walks blocks in order and merges `ToolUse` + `ToolResult` pairs
/// by their `tool_use_id`.
#[allow(dead_code)] // the Error variant is emitted from the frontend only
#[derive(Serialize, Clone)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ChatEntry {
    User {
        text: String,
    },
    Assistant {
        text: String,
    },
    System {
        text: String,
    },
    Thinking {
        text: String,
    },
    ToolCall {
        id: String,
        name: String,
        arguments: serde_json::Value,
        /// Flat text rendering of the tool output (from ToolResult). `None`
        /// means the tool is still running or the result hasn't landed.
        result: Option<String>,
        is_error: bool,
    },
    Error {
        text: String,
    },
}

// ---------------------------------------------------------------------------
// Commands — raw event log / run record
// ---------------------------------------------------------------------------

/// Return the full raw event log for a session as a flat JSON array.
/// Each entry is the serialized form of a `SessionEvent` (seq, uuid,
/// parent_uuid, timestamp, event_type, emitter, message, data). Used by
/// the Raw Events tab in Home — survives restart because it reads from
/// the persistent sqlite-backed session store.
#[tauri::command]
pub async fn list_session_events(
    state: State<'_, AppState>,
    id: String,
) -> Result<Vec<serde_json::Value>, String> {
    let session = ensure_session_loaded(&state, &id).await?;
    let matches = session
        .query(&EventQuery {
            limit: usize::MAX,
            ..Default::default()
        })
        .await;
    Ok(matches
        .into_iter()
        .map(|m| serde_json::to_value(&m.event).unwrap_or(serde_json::Value::Null))
        .collect())
}

#[tauri::command]
pub async fn get_session_record(
    state: State<'_, AppState>,
    id: String,
) -> Result<serde_json::Value, String> {
    let session = ensure_session_loaded(&state, &id).await?;

    let events: Vec<alva_kernel_abi::agent_session::SessionEvent> = session
        .query(&alva_kernel_abi::agent_session::EventQuery {
            limit: usize::MAX,
            ..Default::default()
        })
        .await
        .into_iter()
        .map(|m| m.event)
        .collect();

    let record = alva_app_core::session_projection::build_run_record(&events);
    serde_json::to_value(&record).map_err(|e| format!("serialize record: {e}"))
}

// ---------------------------------------------------------------------------
// Commands — session management
// ---------------------------------------------------------------------------

#[tauri::command]
pub async fn list_sessions(state: State<'_, AppState>) -> Result<Vec<SessionInfo>, String> {
    let manager = state.session_manager.clone();
    let summaries = tokio::task::spawn_blocking(move || {
        let sessions = manager.list_sessions();
        sessions
            .into_iter()
            .map(|s| summary_to_session_info(s, &manager))
            .collect::<Vec<_>>()
    })
    .await
    .map_err(|e| format!("list_sessions join: {e}"))?;
    Ok(summaries)
}

#[tauri::command]
pub async fn create_session(state: State<'_, AppState>) -> Result<SessionInfo, String> {
    let session = state.session_manager.create_session("").await;
    let id = session.session_id().to_string();
    let now = now_ms();

    // Save default plugin config for this session
    state
        .session_manager
        .set_plugin_config(&id, &default_plugin_state());

    // Auto-provision the default sandbox folder for this session and
    // persist it onto the sessions row so it survives restart + is visible
    // via list_sessions.
    let workspace = default_workspace_for(&id)?;
    let workspace_str = workspace.to_string_lossy().to_string();
    {
        let manager = state.session_manager.clone();
        let sid = id.clone();
        let ws = workspace_str.clone();
        tokio::task::spawn_blocking(move || link_workspace(&manager, &sid, &ws))
            .await
            .ok();
    }

    let info = SessionInfo {
        id: id.clone(),
        title: "New chat".to_string(),
        created_at_ms: now,
        updated_at_ms: now,
        workspace_path: Some(workspace_str),
    };

    {
        let mut sessions = state.sessions.write().await;
        sessions.insert(
            0,
            SessionEntry {
                info: info.clone(),
                session,
                config_snapshot_appended: false,
            },
        );
    }
    *state.active_session_id.write().await = Some(id);
    Ok(info)
}

#[tauri::command]
pub async fn switch_session(
    state: State<'_, AppState>,
    id: String,
) -> Result<Vec<ChatEntry>, String> {
    let session = ensure_session_loaded(&state, &id).await?;
    *state.active_session_id.write().await = Some(id);

    // If the agent exists, swap this session into it so the next send_message
    // continues the right thread. If the agent is not built yet, the swap
    // will happen inside ensure_agent on the first send_message.
    if let Some(agent) = state.agent.read().await.clone() {
        agent
            .swap_session(session.clone() as Arc<dyn AgentSession>)
            .await;
    }

    let agent_msgs = session.messages().await;
    let run_errors = collect_run_errors(&session).await;
    Ok(messages_to_chat_entries(agent_msgs, run_errors))
}

/// Walk the session's `run_end` events and pull out every non-null
/// `error` field. Returned in seq order; one entry per failed run. The
/// chat projection appends these as `ChatEntry::Error` so a session
/// that hit an `LLM error: invalid tool arguments ...` mid-stream still
/// shows the failure permanently in history (otherwise the AgentEnd
/// red bubble race-loses to switchSession's projection refresh).
async fn collect_run_errors(session: &Arc<SqliteEvalSession>) -> Vec<String> {
    let matches = session
        .query(&EventQuery {
            event_type: Some("run_end".into()),
            limit: usize::MAX,
            ..Default::default()
        })
        .await;
    matches
        .into_iter()
        .filter_map(|m| {
            m.event
                .data
                .as_ref()
                .and_then(|d| d.get("error"))
                .and_then(|e| e.as_str().map(|s| s.to_string()))
        })
        .collect()
}

/// Update a session's workspace path. Only allowed **before** the first
/// user message is sent — after that, the agent has already run against
/// the old path and changing it silently would corrupt tool results.
///
/// The frontend gates its own button on `messages.length === 0`; we also
/// check here as a defence in depth by counting events with
/// `event_type == "user"` on the session's log.
#[tauri::command]
pub async fn set_session_workspace(
    state: State<'_, AppState>,
    id: String,
    path: String,
) -> Result<(), String> {
    // Make sure the session exists and load its current events.
    let session = ensure_session_loaded(&state, &id).await?;
    let events = session
        .query(&EventQuery {
            limit: usize::MAX,
            ..Default::default()
        })
        .await;
    let has_started = events
        .iter()
        .any(|m| m.event.event_type == "user" || m.event.event_type == "iteration_start");
    if has_started {
        return Err("对话已开始,不能再修改工作目录。新建一个任务即可选择自己的路径。".into());
    }

    // Make sure the directory exists — picker returns existing paths but
    // Rust-side we also create on custom selection so ~/.alva/workspaces
    // layout stays consistent.
    std::fs::create_dir_all(&path).map_err(|e| format!("create workspace dir {path}: {e}"))?;

    // Persist to db.
    {
        let manager = state.session_manager.clone();
        let sid = id.clone();
        let p = path.clone();
        tokio::task::spawn_blocking(move || link_workspace(&manager, &sid, &p))
            .await
            .ok();
    }

    // Update in-memory cache.
    {
        let mut sessions = state.sessions.write().await;
        if let Some(entry) = sessions.iter_mut().find(|e| e.info.id == id) {
            entry.info.workspace_path = Some(path);
        }
    }

    Ok(())
}

/// Ask the OS to open a session's workspace folder in the native file
/// manager (Finder on macOS, Explorer on Windows, xdg-open on Linux).
#[tauri::command]
pub async fn open_session_workspace(state: State<'_, AppState>, id: String) -> Result<(), String> {
    // Look up the path from the db (source of truth) so newly-created or
    // just-switched sessions work even if the cache is stale.
    let manager = state.session_manager.clone();
    let target_id = id.clone();
    let path: String =
        tokio::task::spawn_blocking(move || manager.get_session_workspace_path(&target_id))
            .await
            .map_err(|e| format!("join error: {e}"))?
            .ok_or_else(|| format!("session {id} has no workspace set"))?;

    // If somehow missing (user deleted externally), recreate to avoid a
    // confusing "no such directory" popup.
    std::fs::create_dir_all(&path).map_err(|e| format!("ensure workspace dir {path}: {e}"))?;

    opener::open(&path).map_err(|e| format!("open folder {path}: {e}"))?;
    Ok(())
}

#[tauri::command]
pub async fn delete_session(state: State<'_, AppState>, id: String) -> Result<(), String> {
    // Snapshot the workspace path BEFORE we drop the session row — once
    // it's gone the workspace_id ↔ path linkage is unrecoverable from
    // the cache. Only the on-disk folder cleanup branches on this; the
    // `workspaces` table row is left alone (multiple sessions may share
    // a user-picked path via link_workspace's de-dup, and the row is
    // cheap to keep).
    let workspace_path: Option<String> = {
        let manager = state.session_manager.clone();
        let target = id.clone();
        tokio::task::spawn_blocking(move || manager.get_session_workspace_path(&target))
            .await
            .ok()
            .flatten()
    };

    // Delete from the db first — source of truth.
    let manager = state.session_manager.clone();
    let target = id.clone();
    match tokio::task::spawn_blocking(move || manager.delete_session(&target)).await {
        Ok(true) => {} // happy path: row removed
        Ok(false) => {
            tracing::debug!(
                session_id = %id,
                "delete_session: row not present in DB (already gone or never persisted)",
            );
        }
        Err(e) => {
            // JoinError (panic in the blocking task or runtime shutdown).
            // We still proceed with in-memory + workspace cleanup below
            // because the user's intent is "delete this session" — but a
            // stale DB row may resurrect on next launch, so log loudly so
            // the operator can correlate "ghost session reappears" reports.
            tracing::warn!(
                session_id = %id,
                error = %e,
                "delete_session task failed; DB row may still exist and session could reappear on next launch",
            );
        }
    }

    // Drop any cached entry.
    {
        let mut sessions = state.sessions.write().await;
        sessions.retain(|e| e.info.id != id);
    }

    // Workspace folder cleanup, two-rule policy:
    //  1) User-picked path  ⇒ NEVER delete (it's the user's real project
    //     directory; could be a git repo, could be Documents/, blowing it
    //     away would be a data-loss bug).
    //  2) Auto-allocated default `~/.alva/workspaces/{session_id}` ⇒
    //     safe to remove — Alva owns it, only this session lived there.
    //
    // We compare the stored path against `default_workspace_for(id)`
    // exactly (same `~/.alva/workspaces/{session_id}` shape) to decide.
    if let Some(ws) = workspace_path {
        let default = default_workspace_for(&id).ok();
        let ws_path = std::path::PathBuf::from(&ws);
        let is_default = default
            .as_ref()
            .map(|d| paths_match(d, &ws_path))
            .unwrap_or(false);
        if is_default {
            let to_remove = ws_path.clone();
            let removed =
                tokio::task::spawn_blocking(move || std::fs::remove_dir_all(&to_remove)).await;
            match removed {
                Ok(Ok(())) => tracing::info!(
                    session_id = %id,
                    path = %ws_path.display(),
                    "deleted auto-allocated workspace folder"
                ),
                Ok(Err(e)) if e.kind() == std::io::ErrorKind::NotFound => {
                    tracing::debug!(
                        path = %ws_path.display(),
                        "workspace folder already gone, nothing to do"
                    );
                }
                Ok(Err(e)) => tracing::warn!(
                    error = %e,
                    path = %ws_path.display(),
                    "failed to remove auto-allocated workspace folder"
                ),
                Err(e) => tracing::warn!(error = %e, "remove_dir_all join failed"),
            }
        } else {
            tracing::info!(
                session_id = %id,
                path = %ws_path.display(),
                "preserving user-picked workspace folder on session delete"
            );
        }
    }

    let mut active = state.active_session_id.write().await;
    if active.as_deref() == Some(&id) {
        // Pick the most recent remaining session from the db.
        let manager = state.session_manager.clone();
        let next: Option<String> = tokio::task::spawn_blocking(move || {
            manager
                .list_sessions()
                .into_iter()
                .next()
                .map(|s| s.session_id)
        })
        .await
        .ok()
        .flatten();
        *active = next;
    }
    Ok(())
}

/// Path equality robust to symlinks / `..` segments / trailing slashes.
/// Falls back to literal compare when canonicalize fails (e.g. the path
/// no longer exists, which is fine — `default_workspace_for` always
/// `create_dir_all`s, but the user-picked side may have been moved).
fn paths_match(a: &std::path::Path, b: &std::path::Path) -> bool {
    let ca = std::fs::canonicalize(a).ok();
    let cb = std::fs::canonicalize(b).ok();
    match (ca, cb) {
        (Some(ca), Some(cb)) => ca == cb,
        _ => a == b,
    }
}

// ---------------------------------------------------------------------------
// AgentMessage → ChatMessage projection
// ---------------------------------------------------------------------------

/// Project a session's full `Vec<AgentMessage>` history into an ordered
/// list of `ChatEntry` variants ready for the Home chat renderer.
///
/// The tricky piece: `ToolUse` and `ToolResult` content blocks land in
/// *different* messages (the assistant's reply vs the subsequent user/tool
/// turn). We track tool_use_id → `ChatEntry::ToolCall` index while walking
/// and patch `result` + `is_error` when we later see the matching
/// `ToolResult` block. That preserves 1:1 ordering *and* co-locates the
/// result with its call in the output list.
fn messages_to_chat_entries(msgs: Vec<AgentMessage>, run_errors: Vec<String>) -> Vec<ChatEntry> {
    let mut entries: Vec<ChatEntry> = Vec::new();
    let mut tool_call_indices: std::collections::HashMap<String, usize> =
        std::collections::HashMap::new();

    for am in msgs {
        let msg: Message = match am {
            AgentMessage::Standard(m) | AgentMessage::Steering(m) | AgentMessage::FollowUp(m) => m,
            // Marker / Extension variants aren't chat bubbles.
            _ => continue,
        };

        let role = msg.role.clone();

        for block in msg.content {
            match block {
                ContentBlock::Text { text } if !text.is_empty() => match role {
                    MessageRole::User => entries.push(ChatEntry::User { text }),
                    MessageRole::Assistant => entries.push(ChatEntry::Assistant { text }),
                    MessageRole::System | MessageRole::Tool => {
                        entries.push(ChatEntry::System { text })
                    }
                },
                ContentBlock::Reasoning { text, .. } if !text.is_empty() => {
                    entries.push(ChatEntry::Thinking { text });
                }
                ContentBlock::ToolUse { id, name, input } => {
                    let idx = entries.len();
                    entries.push(ChatEntry::ToolCall {
                        id: id.clone(),
                        name,
                        arguments: input,
                        result: None,
                        is_error: false,
                    });
                    tool_call_indices.insert(id, idx);
                }
                ContentBlock::ToolResult {
                    id,
                    content,
                    is_error,
                } => {
                    // Flatten the ToolContent list to a single string for
                    // display — each ToolContent knows its own model_string.
                    let flat = content
                        .iter()
                        .map(|c| c.to_model_string())
                        .collect::<Vec<_>>()
                        .join("\n");
                    if let Some(&idx) = tool_call_indices.get(&id) {
                        if let Some(ChatEntry::ToolCall {
                            result,
                            is_error: e,
                            ..
                        }) = entries.get_mut(idx)
                        {
                            *result = Some(flat);
                            *e = is_error;
                        }
                    }
                }
                _ => {}
            }
        }
    }

    // Run-end errors appended at the tail. Imperfect interleave (if the
    // user retried multiple times, errors stack at the bottom) but
    // ensures failures don't get silently dropped by the AgentEnd-vs-
    // switchSession race in Home.tsx. Each error is prefixed so the
    // user sees the structural cause at a glance.
    for err in run_errors {
        entries.push(ChatEntry::Error {
            text: format!("agent error: {err}"),
        });
    }

    entries
}
