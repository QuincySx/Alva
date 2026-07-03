// INPUT:  alva_kernel_core::AgentEvent, tokio mpsc
// OUTPUT: pub fn collect_events, ran_tool, tool_result_for, agent_ended_cleanly
// POS:    Event-stream assertion helpers shared by every e2e/capability suite.
//         Formerly copy-pasted into four app-core test files (and about to be
//         a fifth copy in the CLI) — this is the single home.

use alva_kernel_core::AgentEvent;

/// Drain all events until `AgentEnd` (inclusive). Returns the full list.
///
/// Callers should wrap this in `tokio::time::timeout(...)` when a hang is a
/// plausible failure mode — a missing AgentEnd otherwise blocks the test
/// forever instead of failing it.
pub async fn collect_events(
    mut rx: tokio::sync::mpsc::UnboundedReceiver<AgentEvent>,
) -> Vec<AgentEvent> {
    let mut events = Vec::new();
    while let Some(event) = rx.recv().await {
        let is_end = matches!(event, AgentEvent::AgentEnd { .. });
        events.push(event);
        if is_end {
            break;
        }
    }
    events
}

/// Did a `ToolExecutionEnd` for `tool_name` appear in the stream?
pub fn ran_tool(events: &[AgentEvent], tool_name: &str) -> bool {
    events.iter().any(
        |e| matches!(e, AgentEvent::ToolExecutionEnd { tool_call, .. } if tool_call.name == tool_name),
    )
}

/// First `ToolExecutionEnd` result for `tool_name`. Panics if the tool never
/// ran — the caller asserts it was supposed to.
pub fn tool_result_for(events: &[AgentEvent], tool_name: &str) -> alva_kernel_abi::ToolOutput {
    events
        .iter()
        .find_map(|e| match e {
            AgentEvent::ToolExecutionEnd { tool_call, result } if tool_call.name == tool_name => {
                Some(result.clone())
            }
            _ => None,
        })
        .unwrap_or_else(|| panic!("no ToolExecutionEnd for `{tool_name}` in event stream"))
}

/// Did the run end with `AgentEnd {{ error: None }}`?
pub fn agent_ended_cleanly(events: &[AgentEvent]) -> bool {
    events
        .iter()
        .any(|e| matches!(e, AgentEvent::AgentEnd { error: None }))
}
