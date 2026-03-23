use std::sync::Arc;

use agent_types::{CancellationToken, Tool, ToolCall, ToolContext, ToolResult};
use tokio::sync::mpsc;
use tracing::{error, warn};

use crate::event::AgentEvent;
use crate::middleware::{Extensions, MiddlewareContext};
use crate::types::{
    AgentConfig, AgentContext, ToolCallDecision, ToolExecutionMode,
};

/// Build a [`MiddlewareContext`] from the sync `AgentContext` reference.
fn build_mw_ctx_from_context(context: &AgentContext<'_>, tool_context: &dyn ToolContext) -> MiddlewareContext {
    MiddlewareContext {
        session_id: tool_context.session_id().to_string(),
        system_prompt: context.system_prompt.to_string(),
        messages: context.messages.to_vec(),
        extensions: Extensions::new(),
    }
}

/// Execute a batch of tool calls, respecting the configured execution mode,
/// before/after hooks, and cancellation.
pub(crate) async fn execute_tools(
    tool_calls: &[ToolCall],
    tools: &[Arc<dyn Tool>],
    config: &AgentConfig,
    context: &AgentContext<'_>,
    cancel: &CancellationToken,
    event_tx: &mpsc::UnboundedSender<AgentEvent>,
    tool_context: &Arc<dyn ToolContext>,
) -> Vec<ToolResult> {
    match config.tool_execution {
        ToolExecutionMode::Parallel => {
            execute_parallel(tool_calls, tools, config, context, cancel, event_tx, tool_context).await
        }
        ToolExecutionMode::Sequential => {
            execute_sequential(tool_calls, tools, config, context, cancel, event_tx, tool_context).await
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
    tool_context: &Arc<dyn ToolContext>,
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

        // Middleware: before_tool_call (on the main task, before spawning) ----
        if !config.middleware.is_empty() {
            let mut mw_ctx = build_mw_ctx_from_context(context, tool_context.as_ref());
            if let Err(e) = config
                .middleware
                .run_before_tool_call(&mut mw_ctx, tc, tool_context.as_ref())
                .await
            {
                warn!(tool = %tc.name, error = %e, "middleware before_tool_call blocked");
                let blocked = ToolResult {
                    content: format!("Blocked by middleware: {e}"),
                    is_error: true,
                    details: None,
                };
                let tc_clone = tc.clone();
                let event_tx = event_tx.clone();
                let _ = event_tx.send(AgentEvent::ToolExecutionStart {
                    tool_call: tc_clone.clone(),
                });
                let _ = event_tx.send(AgentEvent::ToolExecutionEnd {
                    tool_call: tc_clone.clone(),
                    result: blocked.clone(),
                });
                join_set.spawn(async move { (tc_clone, blocked) });
                continue;
            }
        }

        // Find the tool ------------------------------------------------------
        let tool = tools.iter().find(|t| t.name() == tc.name).cloned();
        let tc_clone = tc.clone();
        let cancel_clone = cancel.clone();
        let event_tx_clone = event_tx.clone();
        let tool_ctx = tool_context.clone();

        let _ = event_tx.send(AgentEvent::ToolExecutionStart {
            tool_call: tc_clone.clone(),
        });

        join_set.spawn(async move {
            let result = match tool {
                Some(t) => match t.execute(tc_clone.arguments.clone(), &cancel_clone, tool_ctx.as_ref()).await {
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
        // Middleware: after_tool_call (on the main task).
        if !config.middleware.is_empty() {
            let mut mw_ctx = build_mw_ctx_from_context(context, tool_context.as_ref());
            if let Err(e) = config
                .middleware
                .run_after_tool_call(&mut mw_ctx, &tc, &mut result)
                .await
            {
                warn!(tool = %tc.name, error = %e, "middleware after_tool_call failed");
            }
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
    tool_context: &Arc<dyn ToolContext>,
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

        // Middleware: before_tool_call ----------------------------------------
        if !config.middleware.is_empty() {
            let mut mw_ctx = build_mw_ctx_from_context(context, tool_context.as_ref());
            if let Err(e) = config
                .middleware
                .run_before_tool_call(&mut mw_ctx, tc, tool_context.as_ref())
                .await
            {
                warn!(tool = %tc.name, error = %e, "middleware before_tool_call blocked");
                let blocked = ToolResult {
                    content: format!("Blocked by middleware: {e}"),
                    is_error: true,
                    details: None,
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

        let _ = event_tx.send(AgentEvent::ToolExecutionStart {
            tool_call: tc.clone(),
        });

        // Find & execute -----------------------------------------------------
        let tool = tools.iter().find(|t| t.name() == tc.name);
        let mut result = match tool {
            Some(t) => match t.execute(tc.arguments.clone(), cancel, tool_context.as_ref()).await {
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

        // Middleware: after_tool_call -----------------------------------------
        if !config.middleware.is_empty() {
            let mut mw_ctx = build_mw_ctx_from_context(context, tool_context.as_ref());
            if let Err(e) = config
                .middleware
                .run_after_tool_call(&mut mw_ctx, tc, &mut result)
                .await
            {
                warn!(tool = %tc.name, error = %e, "middleware after_tool_call failed");
            }
        }

        let _ = event_tx.send(AgentEvent::ToolExecutionEnd {
            tool_call: tc.clone(),
            result: result.clone(),
        });

        results.push(result);
    }

    results
}
