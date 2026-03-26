// INPUT:  alva_types (CancellationToken, Tool, ToolCall, ToolContext, ToolResult), tokio, tracing, crate::middleware, crate::event, crate::types (AgentHooks, AgentContext, ToolCallDecision, ToolExecutionMode), alva_agent_context
// OUTPUT: execute_tools (pub(crate))
// POS:    Executes tool-call batches in parallel or sequential mode, applying before/after hooks, context plugin hooks, and middleware at each call.
use std::sync::Arc;

use alva_types::{CancellationToken, Tool, ToolCall, ToolContext, ToolResult};
use tokio::sync::mpsc;
use tracing::{error, warn};

use crate::event::AgentEvent;
use crate::middleware::MiddlewareContext;
use crate::types::{
    AgentHooks, AgentContext, AgentMessage, ToolCallDecision, ToolExecutionMode,
};

/// Execute a batch of tool calls, respecting the configured execution mode,
/// before/after hooks, context plugin hooks, and cancellation.
pub(crate) async fn execute_tools(
    tool_calls: &[ToolCall],
    tools: &[Arc<dyn Tool>],
    config: &AgentHooks,
    context: &AgentContext<'_>,
    cancel: &CancellationToken,
    event_tx: &mpsc::UnboundedSender<AgentEvent>,
    tool_context: &Arc<dyn ToolContext>,
    mw_ctx: &mut MiddlewareContext,
    context_plugin: &Arc<dyn alva_agent_context::ContextPlugin>,
    context_sdk: &Arc<dyn alva_agent_context::ContextManagementSDK>,
    session_id: &str,
) -> Vec<ToolResult> {
    match config.tool_execution {
        ToolExecutionMode::Parallel => {
            execute_parallel(tool_calls, tools, config, context, cancel, event_tx, tool_context, mw_ctx, context_plugin, context_sdk, session_id).await
        }
        ToolExecutionMode::Sequential => {
            execute_sequential(tool_calls, tools, config, context, cancel, event_tx, tool_context, mw_ctx, context_plugin, context_sdk, session_id).await
        }
    }
}

// ---------------------------------------------------------------------------
// Parallel execution
// ---------------------------------------------------------------------------

async fn execute_parallel(
    tool_calls: &[ToolCall],
    tools: &[Arc<dyn Tool>],
    config: &AgentHooks,
    context: &AgentContext<'_>,
    cancel: &CancellationToken,
    event_tx: &mpsc::UnboundedSender<AgentEvent>,
    tool_context: &Arc<dyn ToolContext>,
    mw_ctx: &mut MiddlewareContext,
    context_plugin: &Arc<dyn alva_agent_context::ContextPlugin>,
    context_sdk: &Arc<dyn alva_agent_context::ContextManagementSDK>,
    session_id: &str,
) -> Vec<ToolResult> {
    use tokio::task::JoinSet;

    let mut join_set = JoinSet::new();

    for tc in tool_calls {
        // Context plugin: before_tool_call -----------------------------------
        let plugin_action = context_plugin.before_tool_call(
            context_sdk.as_ref(), session_id, &tc.name, &tc.arguments,
        ).await;
        match plugin_action {
            alva_agent_context::ToolCallAction::Block { reason } => {
                let blocked_result = ToolResult {
                    content: format!("Tool call blocked by context plugin: {reason}"),
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
                    result: blocked_result.clone(),
                });
                join_set.spawn(async move { (tc_clone, blocked_result) });
                continue;
            }
            alva_agent_context::ToolCallAction::AllowWithWarning { warning } => {
                tracing::warn!(tool = %tc.name, warning = %warning, "context plugin warning");
            }
            alva_agent_context::ToolCallAction::Allow => {}
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
            if let Err(e) = config
                .middleware
                .run_before_tool_call(mw_ctx, tc, tool_context.as_ref())
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
            if let Err(e) = config
                .middleware
                .run_after_tool_call(mw_ctx, &tc, &mut result)
                .await
            {
                warn!(tool = %tc.name, error = %e, "middleware after_tool_call failed");
            }
        }
        // Context plugin: after_tool_call
        {
            let result_msg = AgentMessage::Standard(alva_types::Message {
                id: String::new(),
                role: alva_types::MessageRole::Tool,
                content: vec![alva_types::ContentBlock::Text { text: result.content.clone() }],
                tool_call_id: Some(tc.id.clone()),
                usage: None,
                timestamp: chrono::Utc::now().timestamp_millis(),
            });
            let result_tokens = alva_agent_context::store::estimate_tokens(&result.content);
            let action = context_plugin.after_tool_call(
                context_sdk.as_ref(), session_id, &tc.name, &result_msg, result_tokens,
            ).await;
            match action {
                alva_agent_context::ToolResultAction::Truncate { max_lines } => {
                    let truncated: String = result.content.lines().take(max_lines).collect::<Vec<_>>().join("\n");
                    if truncated.len() < result.content.len() {
                        result.content = format!("{}\n[... truncated to {} lines]", truncated, max_lines);
                    }
                }
                alva_agent_context::ToolResultAction::Replace { summary } => {
                    result.content = summary;
                }
                alva_agent_context::ToolResultAction::Externalize { path } => {
                    result.content = format!("[result externalized to {}]", path);
                }
                alva_agent_context::ToolResultAction::Keep => {}
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
    config: &AgentHooks,
    context: &AgentContext<'_>,
    cancel: &CancellationToken,
    event_tx: &mpsc::UnboundedSender<AgentEvent>,
    tool_context: &Arc<dyn ToolContext>,
    mw_ctx: &mut MiddlewareContext,
    context_plugin: &Arc<dyn alva_agent_context::ContextPlugin>,
    context_sdk: &Arc<dyn alva_agent_context::ContextManagementSDK>,
    session_id: &str,
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

        // Context plugin: before_tool_call -----------------------------------
        let plugin_action = context_plugin.before_tool_call(
            context_sdk.as_ref(), session_id, &tc.name, &tc.arguments,
        ).await;
        match plugin_action {
            alva_agent_context::ToolCallAction::Block { reason } => {
                let blocked = ToolResult {
                    content: format!("Tool call blocked by context plugin: {reason}"),
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
            alva_agent_context::ToolCallAction::AllowWithWarning { warning } => {
                tracing::warn!(tool = %tc.name, warning = %warning, "context plugin warning");
            }
            alva_agent_context::ToolCallAction::Allow => {}
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
            if let Err(e) = config
                .middleware
                .run_before_tool_call(mw_ctx, tc, tool_context.as_ref())
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
            if let Err(e) = config
                .middleware
                .run_after_tool_call(mw_ctx, tc, &mut result)
                .await
            {
                warn!(tool = %tc.name, error = %e, "middleware after_tool_call failed");
            }
        }

        // Context plugin: after_tool_call ------------------------------------
        {
            let result_msg = AgentMessage::Standard(alva_types::Message {
                id: String::new(),
                role: alva_types::MessageRole::Tool,
                content: vec![alva_types::ContentBlock::Text { text: result.content.clone() }],
                tool_call_id: Some(tc.id.clone()),
                usage: None,
                timestamp: chrono::Utc::now().timestamp_millis(),
            });
            let result_tokens = alva_agent_context::store::estimate_tokens(&result.content);
            let action = context_plugin.after_tool_call(
                context_sdk.as_ref(), session_id, &tc.name, &result_msg, result_tokens,
            ).await;
            match action {
                alva_agent_context::ToolResultAction::Truncate { max_lines } => {
                    let truncated: String = result.content.lines().take(max_lines).collect::<Vec<_>>().join("\n");
                    if truncated.len() < result.content.len() {
                        result.content = format!("{}\n[... truncated to {} lines]", truncated, max_lines);
                    }
                }
                alva_agent_context::ToolResultAction::Replace { summary } => {
                    result.content = summary;
                }
                alva_agent_context::ToolResultAction::Externalize { path } => {
                    result.content = format!("[result externalized to {}]", path);
                }
                alva_agent_context::ToolResultAction::Keep => {}
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
