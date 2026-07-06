// Inline tool-approval commands: resolve a pending "Allow / Reject" decision
// and snapshot the currently-pending approvals for UI rehydration. Split from
// `agent.rs` (PR-13a); command names are unchanged (= function names).

use tauri::{AppHandle, Emitter, State};

use alva_app_core::PermissionDecision;

use super::{AppState, PendingApproval};

/// User decision string from the inline approval bubble. Mirrors
/// `PermissionDecision`'s 4 variants.
fn parse_decision(s: &str) -> Result<PermissionDecision, String> {
    // Canonical parser lives on PermissionDecision (alva-agent-security) —
    // this was a drifting twin of the CLI's human-input parser until PR-9.
    PermissionDecision::parse_token(s)
}

/// Resolve a pending approval. Called by the inline "Allow / Reject"
/// bubble in the chat. Removes the entry from `pending_approvals`,
/// dispatches the decision into `BaseAgent::resolve_permission`, and
/// emits `approval_resolved` so any other webview tab can drop the
/// prompt from view.
#[tauri::command]
pub async fn respond_approval(
    app: AppHandle,
    state: State<'_, AppState>,
    request_id: String,
    decision: String,
) -> Result<(), String> {
    let parsed = parse_decision(&decision)?;
    // Resolve the agent BEFORE taking the pending entry out (D-7):
    // removing first and then failing any later step would drop the entry
    // on the floor — prompt gone from the UI, but the tool call still
    // parked on an unresolved oneshot forever.
    let agent = state
        .agent
        .read()
        .await
        .clone()
        .ok_or_else(|| "no agent built yet".to_string())?;

    let pa = state.pending_approvals.write().await.remove(&request_id);
    let Some(pa) = pa else {
        // Already answered (double-click, multi-window race) or cleared
        // by an agent rebuild. Idempotent no-op.
        tracing::debug!(
            request_id = %request_id,
            "respond_approval: no pending entry (already resolved or stale)"
        );
        return Ok(());
    };
    agent
        .resolve_permission(&pa.request_id, &pa.tool_name, parsed)
        .await;

    if let Err(e) = app.emit(
        "approval_resolved",
        serde_json::json!({ "request_id": pa.request_id }),
    ) {
        tracing::warn!(
            request_id = %pa.request_id,
            error = %e,
            "failed to emit approval_resolved; UI may show stuck Pending prompt",
        );
    }
    Ok(())
}

/// Snapshot of the currently-pending approvals. The frontend calls this
/// on mount / on session switch to rehydrate any prompts that arrived
/// before its event listener was attached.
#[tauri::command]
pub async fn list_pending_approvals(
    state: State<'_, AppState>,
) -> Result<Vec<PendingApproval>, String> {
    let pending = state.pending_approvals.read().await;
    Ok(pending.values().cloned().collect())
}
