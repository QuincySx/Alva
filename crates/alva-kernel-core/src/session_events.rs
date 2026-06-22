use alva_kernel_abi::agent_session::{AgentSession, SessionEvent};

/// Append a runtime-emitted event to the session. The emitter is always
/// `EventEmitter::runtime()`; callers set event_type, parent_uuid, and data.
/// Returns the event uuid so later events can use it as their causal parent.
pub(crate) async fn emit_runtime_event(
    session: &std::sync::Arc<dyn AgentSession>,
    event_type: &str,
    parent_uuid: Option<String>,
    data: Option<serde_json::Value>,
) -> String {
    let mut event = SessionEvent::new_runtime(event_type);
    event.parent_uuid = parent_uuid;
    event.data = data;
    let uuid = event.uuid.clone();
    session.append(event).await;
    uuid
}
