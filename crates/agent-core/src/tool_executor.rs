use std::sync::Arc;

use agent_types::{CancellationToken, Tool, ToolCall, ToolResult};
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
        let mut decision = ToolCallDecision::Allow;
        for hook in &config.before_tool_call {
            decision = hook(tc, context);
            if matches!(decision, ToolCallDecision::Block { .. }) {
                break;
            }
        }
        match decision {
            ToolCallDecision::Allow => {}
            ToolCallDecision::Block { reason } => {
                let mut blocked_result = ToolResult {
                    content: format!("Tool call blocked: {reason}"),
                    is_error: true,
                    details: None,
                };
                let tc_clone = tc.clone();
                let event_tx = event_tx.clone();
                for hook in &config.after_tool_call {
                    blocked_result = hook(&tc_clone, blocked_result, context);
                }
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
                            content: format!("Tool execution error: {e}"),
                            is_error: true,
                            details: None,
                        }
                    }
                },
                None => {
                    warn!(tool = %tc_clone.name, "tool not found");
                    ToolResult {
                        content: format!("Tool '{}' not found", tc_clone.name),
                        is_error: true,
                        details: None,
                    }
                }
            };

            // Design note: after_tool_call hooks run on the main task after
            // all parallel tools complete (see loop below). This is intentional —
            // hooks process individual tool results and don't need to see
            // results from sibling tools. AgentContext stays on the main task
            // to avoid Send requirements on borrowed references.
            let _ = event_tx_clone.send(AgentEvent::ToolExecutionEnd {
                tool_call: tc_clone.clone(),
                result: result.clone(),
            });

            (tc_clone, result)
        });
    }

    let mut results = Vec::with_capacity(tool_calls.len());
    while let Some(Ok((tc, mut result))) = join_set.join_next().await {
        // Apply after_tool_call hooks (on the main task where context lives).
        for hook in &config.after_tool_call {
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
                content: "Cancelled".to_string(),
                is_error: true,
                details: None,
            });
            continue;
        }

        // Pre-flight check ---------------------------------------------------
        let mut decision = ToolCallDecision::Allow;
        for hook in &config.before_tool_call {
            decision = hook(tc, context);
            if matches!(decision, ToolCallDecision::Block { .. }) {
                break;
            }
        }
        match decision {
            ToolCallDecision::Allow => {}
            ToolCallDecision::Block { reason } => {
                let mut blocked = ToolResult {
                    content: format!("Tool call blocked: {reason}"),
                    is_error: true,
                    details: None,
                };
                for hook in &config.after_tool_call {
                    blocked = hook(tc, blocked, context);
                }
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
                        content: format!("Tool execution error: {e}"),
                        is_error: true,
                        details: None,
                    }
                }
            },
            None => {
                warn!(tool = %tc.name, "tool not found");
                ToolResult {
                    content: format!("Tool '{}' not found", tc.name),
                    is_error: true,
                    details: None,
                }
            }
        };

        // Post-processing ----------------------------------------------------
        for hook in &config.after_tool_call {
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
