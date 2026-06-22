// INPUT:  serde, serde_json, alva_kernel_abi::agent_session::{AgentSession, EventQuery}
// OUTPUT: PendingAction, ResolveStatus, pending_actions(),
//         EVENT_REQUIRES_ACTION, EVENT_REQUIRES_ACTION_RESOLVED
// POS:    HITL pending-action view derived from the session event log. The
//         security middleware appends `requires_action` events when a tool
//         needs human approval and `requires_action_resolved` events when
//         the wait ends (decision / cancel / timeout). Subscribers to
//         `AgentSession::subscribe_events` see these live; `pending_actions`
//         returns the snapshot of currently-unresolved entries. Equivalent
//         to Anthropic Managed Agents API's `session.status_idle.stop_reason
//         .event_ids[]`.

use serde::{Deserialize, Serialize};
use serde_json::Value;

use alva_kernel_abi::agent_session::{AgentSession, EventQuery};

/// SessionEvent.event_type for a freshly-emitted HITL request. The event's
/// `data` carries `action_type`, `request_id`, `tool_name`, `tool_call_id`,
/// `arguments`.
pub const EVENT_REQUIRES_ACTION: &str = "requires_action";

/// SessionEvent.event_type for the matching resolution. Its `parent_uuid`
/// points back at the `requires_action` event's uuid; `data` carries
/// `request_id` and `status` (allow_once / allow_always / reject_once /
/// reject_always / cancelled / timed_out / disconnected / no_handler).
pub const EVENT_REQUIRES_ACTION_RESOLVED: &str = "requires_action_resolved";

/// Snapshot of one pending HITL request. Derived from the session event log:
/// a `requires_action` event whose uuid does NOT appear in any
/// `requires_action_resolved` event's parent_uuid.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct PendingAction {
    /// uuid of the originating `requires_action` SessionEvent. Stable across
    /// reads — pass it back to subscribers so they can correlate snapshot
    /// entries with live stream events.
    pub event_uuid: String,

    /// PermissionManager request_id. Pass to
    /// `BaseAgent::resolve_permission(request_id, decision)` to resolve.
    pub request_id: String,

    /// What kind of HITL action this is. Today only `"tool_confirmation"`;
    /// future variants will cover plan-mode approval and custom tool result
    /// delivery as those paths are unified through `requires_action`.
    pub action_type: String,

    /// Tool name that triggered the request.
    pub tool_name: String,

    /// `ToolCall.id` of the originating call.
    pub tool_call_id: String,

    /// Tool arguments the LLM was about to run. May be useful to the UI for
    /// rendering an approval prompt.
    pub arguments: Value,

    /// Wall-clock epoch millis when the `requires_action` event was appended.
    pub requested_at: i64,
}

/// Final state recorded in the `requires_action_resolved` event's `data.status`
/// field. Maps 1:1 to PermissionDecision plus the non-decision outcomes
/// (cancel / timeout / disconnected / no_handler).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ResolveStatus {
    AllowOnce,
    AllowAlways,
    RejectOnce,
    RejectAlways,
    /// Run cancelled while the request was waiting.
    Cancelled,
    /// Approval timeout elapsed without a decision.
    TimedOut,
    /// The oneshot receiver returned an error (sender dropped).
    Disconnected,
    /// No `ApprovalNotifier` was registered on the bus, so the request
    /// could not be delivered to a human.
    NoHandler,
}

impl ResolveStatus {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::AllowOnce => "allow_once",
            Self::AllowAlways => "allow_always",
            Self::RejectOnce => "reject_once",
            Self::RejectAlways => "reject_always",
            Self::Cancelled => "cancelled",
            Self::TimedOut => "timed_out",
            Self::Disconnected => "disconnected",
            Self::NoHandler => "no_handler",
        }
    }
}

/// Scan the session event log and return one `PendingAction` for every
/// `requires_action` event that has no matching `requires_action_resolved`
/// child. The result is ordered by event seq ascending — the same order in
/// which the requests were emitted.
///
/// O(n) in event count. Safe to call from UI / status endpoints whenever a
/// snapshot is needed; subscribers that follow `subscribe_events` get the
/// stream form for free.
pub async fn pending_actions(session: &dyn AgentSession) -> Vec<PendingAction> {
    // limit=0 means "no cap" per EventQuery::query semantics. We need every
    // event because we have to scan for matched resolutions.
    let matches = session.query(&EventQuery::default()).await;

    let mut resolved: std::collections::HashSet<String> = std::collections::HashSet::new();
    for m in &matches {
        if m.event.event_type == EVENT_REQUIRES_ACTION_RESOLVED {
            if let Some(parent) = &m.event.parent_uuid {
                resolved.insert(parent.clone());
            }
        }
    }

    let mut pending = Vec::new();
    for m in &matches {
        if m.event.event_type != EVENT_REQUIRES_ACTION {
            continue;
        }
        if resolved.contains(&m.event.uuid) {
            continue;
        }
        let Some(data) = m.event.data.as_ref() else {
            continue;
        };
        let Some(action_type) = data.get("action_type").and_then(Value::as_str) else {
            continue;
        };
        let Some(request_id) = data.get("request_id").and_then(Value::as_str) else {
            continue;
        };
        let Some(tool_name) = data.get("tool_name").and_then(Value::as_str) else {
            continue;
        };
        let tool_call_id = data
            .get("tool_call_id")
            .and_then(Value::as_str)
            .unwrap_or("")
            .to_string();
        let arguments = data.get("arguments").cloned().unwrap_or(Value::Null);
        pending.push(PendingAction {
            event_uuid: m.event.uuid.clone(),
            request_id: request_id.to_string(),
            action_type: action_type.to_string(),
            tool_name: tool_name.to_string(),
            tool_call_id,
            arguments,
            requested_at: m.event.timestamp,
        });
    }
    pending
}

#[cfg(test)]
mod tests {
    use super::*;
    use alva_kernel_abi::agent_session::{
        EmitterKind, EventEmitter, InMemoryAgentSession, SessionEvent,
    };

    fn make_requires_action(request_id: &str, tool: &str, call_id: &str) -> SessionEvent {
        let mut event = SessionEvent::new_runtime(EVENT_REQUIRES_ACTION);
        event.emitter = EventEmitter {
            kind: EmitterKind::Middleware,
            id: "security".into(),
            instance: None,
        };
        event.data = Some(serde_json::json!({
            "action_type": "tool_confirmation",
            "request_id": request_id,
            "tool_name": tool,
            "tool_call_id": call_id,
            "arguments": { "command": "ls" },
        }));
        event
    }

    fn make_resolved(parent_uuid: &str, request_id: &str, status: &str) -> SessionEvent {
        let mut event = SessionEvent::new_runtime(EVENT_REQUIRES_ACTION_RESOLVED);
        event.emitter = EventEmitter {
            kind: EmitterKind::Middleware,
            id: "security".into(),
            instance: None,
        };
        event.parent_uuid = Some(parent_uuid.to_string());
        event.data = Some(serde_json::json!({
            "request_id": request_id,
            "status": status,
        }));
        event
    }

    #[tokio::test]
    async fn empty_session_returns_no_pending() {
        let session = InMemoryAgentSession::new();
        assert!(pending_actions(&session).await.is_empty());
    }

    #[tokio::test]
    async fn unresolved_requires_action_shows_up() {
        let session = InMemoryAgentSession::new();
        let req = make_requires_action("req-1", "execute_shell", "call-1");
        let uuid = req.uuid.clone();
        session.append(req).await;

        let pending = pending_actions(&session).await;
        assert_eq!(pending.len(), 1);
        assert_eq!(pending[0].event_uuid, uuid);
        assert_eq!(pending[0].request_id, "req-1");
        assert_eq!(pending[0].action_type, "tool_confirmation");
        assert_eq!(pending[0].tool_name, "execute_shell");
        assert_eq!(pending[0].tool_call_id, "call-1");
    }

    #[tokio::test]
    async fn resolved_requires_action_is_filtered_out() {
        let session = InMemoryAgentSession::new();
        let req = make_requires_action("req-1", "execute_shell", "call-1");
        let uuid = req.uuid.clone();
        session.append(req).await;
        session
            .append(make_resolved(&uuid, "req-1", "allow_once"))
            .await;

        assert!(pending_actions(&session).await.is_empty());
    }

    #[tokio::test]
    async fn mixed_pending_and_resolved() {
        let session = InMemoryAgentSession::new();

        let req1 = make_requires_action("req-1", "execute_shell", "call-1");
        let uuid1 = req1.uuid.clone();
        session.append(req1).await;

        let req2 = make_requires_action("req-2", "file_edit", "call-2");
        session.append(req2).await;

        session
            .append(make_resolved(&uuid1, "req-1", "reject_once"))
            .await;

        let pending = pending_actions(&session).await;
        assert_eq!(pending.len(), 1);
        assert_eq!(pending[0].request_id, "req-2");
        assert_eq!(pending[0].tool_name, "file_edit");
    }

    #[tokio::test]
    async fn pending_actions_are_ordered_by_seq() {
        let session = InMemoryAgentSession::new();
        for i in 0..5 {
            session
                .append(make_requires_action(
                    &format!("req-{i}"),
                    "execute_shell",
                    &format!("call-{i}"),
                ))
                .await;
        }
        let pending = pending_actions(&session).await;
        assert_eq!(pending.len(), 5);
        for (i, p) in pending.iter().enumerate() {
            assert_eq!(p.request_id, format!("req-{i}"));
        }
    }

    #[test]
    fn resolve_status_strings_match_expected() {
        assert_eq!(ResolveStatus::AllowOnce.as_str(), "allow_once");
        assert_eq!(ResolveStatus::AllowAlways.as_str(), "allow_always");
        assert_eq!(ResolveStatus::RejectOnce.as_str(), "reject_once");
        assert_eq!(ResolveStatus::RejectAlways.as_str(), "reject_always");
        assert_eq!(ResolveStatus::Cancelled.as_str(), "cancelled");
        assert_eq!(ResolveStatus::TimedOut.as_str(), "timed_out");
        assert_eq!(ResolveStatus::Disconnected.as_str(), "disconnected");
        assert_eq!(ResolveStatus::NoHandler.as_str(), "no_handler");
    }
}
