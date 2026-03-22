use std::sync::Arc;

use agent_base::{CancellationToken, Tool, ToolCall, ToolResult};
use tokio::sync::mpsc;
use tracing::{error, warn};

use crate::event::AgentEvent;
use crate::types::{
    AgentConfig, AgentContext, ToolCallDecision, ToolExecutionMode,
};

/// Execute a batch of tool calls, respecting the configured execution mode,
/// before/after hooks, and cancellation.
pub(crate) async fn execute_tools(
    tool_calls: &[ToolCall],
    tools: &[Arc<dyn Tool>],
    config: &AgentConfig,
    context: &AgentContext<'_>,
    cancel: &CancellationToken,
    event_tx: &mpsc::UnboundedSender<AgentEvent>,
) -> Vec<ToolResult> {
    match config.tool_execution {
        ToolExecutionMode::Parallel => {
            execute_parallel(tool_calls, tools, config, context, cancel, event_tx).await
        }
        ToolExecutionMode::Sequential => {
            execute_sequential(tool_calls, tools, config, context, cancel, event_tx).await
        }
    }
}

// ---------------------------------------------------------------------------
// Parallel execution
// ---------------------------------------------------------------------------

async fn execute_parallel(
    tool_calls: &[ToolCall],
    tools: &[Arc<dyn Tool>],
    config: &AgentConfig,
    context: &AgentContext<'_>,
    cancel: &CancellationToken,
    event_tx: &mpsc::UnboundedSender<AgentEvent>,
) -> Vec<ToolResult> {
    use tokio::task::JoinSet;

    let mut join_set = JoinSet::new();

    for tc in tool_calls {
        // Pre-flight check ---------------------------------------------------
        if let Some(ref hook) = config.before_tool_call {
            match hook(tc, context) {
                ToolCallDecision::Allow => {}
                ToolCallDecision::Block { reason } => {
                    let blocked_result = ToolResult {
                        tool_call_id: tc.id.clone(),
                        content: format!("Tool call blocked: {reason}"),
                        is_error: true,
                        details: None,
                    };
                    let tc_clone = tc.clone();
                    let event_tx = event_tx.clone();
                    let after_hook = config.after_tool_call.clone();
                    let blocked_result = match after_hook {
                        Some(h) => h(&tc_clone, blocked_result, context),
                        None => blocked_result,
                    };
                    let _ = event_tx.send(AgentEvent::ToolExecutionStart {
                        tool_call: tc_clone.clone(),
                    });
                    let _ = event_tx.send(AgentEvent::ToolExecutionEnd {
                        tool_call: tc_clone.clone(),
                        result: blocked_result.clone(),
                    });
                    // We still need to push the result — spawn a trivial task.
                    join_set.spawn(async move { (tc_clone, blocked_result) });
                    continue;
                }
            }
        }

        // Find the tool ------------------------------------------------------
        let tool = tools.iter().find(|t| t.name() == tc.name).cloned();
        let tc_clone = tc.clone();
        let cancel_clone = cancel.clone();
        let event_tx_clone = event_tx.clone();

        let _ = event_tx.send(AgentEvent::ToolExecutionStart {
            tool_call: tc_clone.clone(),
        });

        join_set.spawn(async move {
            let result = match tool {
                Some(t) => match t.execute(tc_clone.arguments.clone(), &cancel_clone).await {
                    Ok(r) => r,
                    Err(e) => {
                        error!(tool = %tc_clone.name, error = %e, "tool execution failed");
                        ToolResult {
                            tool_call_id: tc_clone.id.clone(),
                            content: format!("Tool execution error: {e}"),
                            is_error: true,
                            details: None,
                        }
                    }
                },
                None => {
                    warn!(tool = %tc_clone.name, "tool not found");
                    ToolResult {
                        tool_call_id: tc_clone.id.clone(),
                        content: format!("Tool '{}' not found", tc_clone.name),
                        is_error: true,
                        details: None,
                    }
                }
            };

            // Note: after_tool_call hook cannot access AgentContext in the
            // spawned task (not Send). We apply it here with a captured clone
            // if the closure is Send+Sync (which it is by our type alias).
            // However, the context reference cannot be sent across tasks, so
            // we skip the after_hook in the spawned task and apply it after
            // the join below. For now, we return a pair.
            let _ = event_tx_clone.send(AgentEvent::ToolExecutionEnd {
                tool_call: tc_clone.clone(),
                result: result.clone(),
            });

            (tc_clone, result)
        });
    }

    let mut results = Vec::with_capacity(tool_calls.len());
    while let Some(Ok((tc, mut result))) = join_set.join_next().await {
        // Apply after_tool_call hook (on the main task where context lives).
        if let Some(ref hook) = config.after_tool_call {
            result = hook(&tc, result, context);
        }
        results.push(result);
    }
    results
}

// ---------------------------------------------------------------------------
// Sequential execution
// ---------------------------------------------------------------------------

async fn execute_sequential(
    tool_calls: &[ToolCall],
    tools: &[Arc<dyn Tool>],
    config: &AgentConfig,
    context: &AgentContext<'_>,
    cancel: &CancellationToken,
    event_tx: &mpsc::UnboundedSender<AgentEvent>,
) -> Vec<ToolResult> {
    let mut results = Vec::with_capacity(tool_calls.len());

    for tc in tool_calls {
        if cancel.is_cancelled() {
            results.push(ToolResult {
                tool_call_id: tc.id.clone(),
                content: "Cancelled".to_string(),
                is_error: true,
                details: None,
            });
            continue;
        }

        // Pre-flight check ---------------------------------------------------
        if let Some(ref hook) = config.before_tool_call {
            match hook(tc, context) {
                ToolCallDecision::Allow => {}
                ToolCallDecision::Block { reason } => {
                    let blocked = ToolResult {
                        tool_call_id: tc.id.clone(),
                        content: format!("Tool call blocked: {reason}"),
                        is_error: true,
                        details: None,
                    };
                    let blocked = match &config.after_tool_call {
                        Some(h) => h(tc, blocked, context),
                        None => blocked,
                    };
                    let _ = event_tx.send(AgentEvent::ToolExecutionStart {
                        tool_call: tc.clone(),
                    });
                    let _ = event_tx.send(AgentEvent::ToolExecutionEnd {
                        tool_call: tc.clone(),
                        result: blocked.clone(),
                    });
                    results.push(blocked);
                    continue;
                }
            }
        }

        let _ = event_tx.send(AgentEvent::ToolExecutionStart {
            tool_call: tc.clone(),
        });

        // Find & execute -----------------------------------------------------
        let tool = tools.iter().find(|t| t.name() == tc.name);
        let mut result = match tool {
            Some(t) => match t.execute(tc.arguments.clone(), cancel).await {
                Ok(r) => r,
                Err(e) => {
                    error!(tool = %tc.name, error = %e, "tool execution failed");
                    ToolResult {
                        tool_call_id: tc.id.clone(),
                        content: format!("Tool execution error: {e}"),
                        is_error: true,
                        details: None,
                    }
                }
            },
            None => {
                warn!(tool = %tc.name, "tool not found");
                ToolResult {
                    tool_call_id: tc.id.clone(),
                    content: format!("Tool '{}' not found", tc.name),
                    is_error: true,
                    details: None,
                }
            }
        };

        // Post-processing ----------------------------------------------------
        if let Some(ref hook) = config.after_tool_call {
            result = hook(tc, result, context);
        }

        let _ = event_tx.send(AgentEvent::ToolExecutionEnd {
            tool_call: tc.clone(),
            result: result.clone(),
        });

        results.push(result);
    }

    results
}
