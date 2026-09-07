use alva_kernel_abi::agent_session::SessionEvent;
use alva_kernel_abi::ConversationLog;

/// Append a runtime-emitted event to the conversation log. The emitter is
/// always `EventEmitter::runtime()`; callers set event_type, parent_uuid, and
/// data. Returns the event uuid so later events can use it as their causal
/// parent. Append failures surface to the caller — the kernel loop maps them
/// onto `AgentError::Session` and aborts the run rather than silently
/// skipping skeleton bookkeeping.
pub(crate) async fn emit_runtime_event(
    session: &std::sync::Arc<dyn ConversationLog>,
    event_type: &str,
    parent_uuid: Option<String>,
    data: Option<serde_json::Value>,
) -> Result<String, alva_kernel_abi::agent_session::SessionError> {
    let mut event = SessionEvent::new_runtime(event_type);
    event.parent_uuid = parent_uuid;
    event.data = data;
    let uuid = event.uuid.clone();
    session.append(event).await?;
    Ok(uuid)
}
