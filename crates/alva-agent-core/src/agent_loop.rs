// INPUT:  alva_types (CancellationToken, ContentBlock, LanguageModel, Message, MessageRole, StreamEvent, ToolCall), tokio, tokio_stream, tracing, uuid, chrono, crate::middleware, crate::tool_executor, crate::types, crate::event
// OUTPUT: run_agent_loop (pub(crate))
// POS:    Double-loop agent execution — outer loop handles follow-ups, inner loop drives LLM + tool calls + steering, with middleware hooks at each stage.
use alva_types::{
    CancellationToken, ContentBlock, LanguageModel, Message, MessageRole, StreamEvent, ToolCall,
};
use tokio::sync::mpsc;
use tokio_stream::StreamExt;
use tracing::{debug, info, warn};

use crate::event::AgentEvent;
use crate::middleware::{Extensions, MiddlewareContext};
use crate::tool_executor::execute_tools;
use crate::types::{AgentHooks, AgentContext, AgentMessage, AgentState};

/// Run the double-loop agent execution algorithm.
///
/// * **Outer loop** — checks for follow-up messages after the inner loop
///   finishes.
/// * **Inner loop** — repeatedly calls the LLM, executes any requested tool
///   calls, injects steering messages, and continues until the model produces
///   a response with no tool calls.
///
/// The function mutates `state` in place and emits `AgentEvent`s through the
/// provided channel.
///
/// A single [`MiddlewareContext`] is created at the start and shared across
/// all middleware hook invocations so that [`Extensions`] set in
/// `on_agent_start` are visible to `before_llm_call`, `before_tool_call`, etc.
pub(crate) async fn run_agent_loop(
    state: &mut AgentState,
    model: &dyn LanguageModel,
    config: &AgentHooks,
    cancel: &CancellationToken,
    event_tx: &mpsc::UnboundedSender<AgentEvent>,
) -> Result<(), alva_types::AgentError> {
    let _ = event_tx.send(AgentEvent::AgentStart);

    // Turn tracking for MessageStore persistence.
    // The user message is expected to be the last element of state.messages when
    // run_agent_loop is called (pushed by the caller). We subtract 1 so the
    // turn slice includes it; if messages is empty we fall back to 0.
    let turn_start_msg_index = state.messages.len().saturating_sub(1);
    let turn_start_time = chrono::Utc::now().timestamp_millis();

    // Create a single MiddlewareContext that persists across the entire run.
    let mut mw_ctx = MiddlewareContext {
        session_id: state.session_id.clone(),
        system_prompt: state.system_prompt.clone(),
        messages: state.messages.clone(),
        extensions: Extensions::new(),
    };

    // Middleware: on_agent_start
    if !config.middleware.is_empty() {
        if let Err(e) = config.middleware.run_on_agent_start(&mut mw_ctx).await {
            warn!(error = %e, "middleware on_agent_start failed");
        }
    }

    // Context plugin: bootstrap + on_agent_start + maintain
    {
        let ctx_plugin = &config.context_plugin;
        let ctx_sdk = config.context_sdk.as_ref();
        if let Err(e) = ctx_plugin.bootstrap(ctx_sdk, &state.session_id).await {
            tracing::warn!("context plugin bootstrap: {}", e);
        }
        ctx_plugin.on_agent_start(ctx_sdk, &state.session_id).await;
        if let Err(e) = ctx_plugin.maintain(ctx_sdk, &state.session_id).await {
            tracing::warn!("context plugin maintain: {}", e);
        }
    }

    // Context plugin: on_user_message — get injections
    if let Some(last_msg) = state.messages.last() {
        let injections = config.context_plugin.on_user_message(
            config.context_sdk.as_ref(), &state.session_id, last_msg
        ).await;
        for inj in injections {
            match inj {
                alva_agent_context::Injection::Memory(facts) => {
                    if !facts.is_empty() {
                        let text = facts.iter().map(|f| format!("- {}", f.text)).collect::<Vec<_>>().join("\n");
                        state.system_prompt.push_str(&format!("\n\n<user_memory>\n{}\n</user_memory>", text));
                    }
                }
                alva_agent_context::Injection::Skill { name, content } => {
                    state.system_prompt.push_str(&format!("\n\n<skill name=\"{}\">\n{}\n</skill>", name, content));
                }
                alva_agent_context::Injection::RuntimeContext(data) => {
                    state.system_prompt.push_str(&format!("\n\n<runtime>\n{}\n</runtime>", data));
                }
                alva_agent_context::Injection::Message(msg) => {
                    state.messages.push(msg);
                }
            }
        }
    }

    // Context plugin: on_inject_file — NOT called here.
    // The on_inject_file hook is intended for upper layers (e.g. UI, CLI) to call
    // when injecting file content into state.messages. The caller should invoke
    // `plugin.on_inject_file(sdk, agent_id, path, content, tokens)` before adding
    // the file content message, and respect the returned InjectDecision.

    // Context plugin: on_inject_media — scan last user message for Image blocks.
    // For each image, call the plugin hook and apply the decision (Keep/Describe/Remove/Reject).
    if let Some(AgentMessage::Standard(last_msg)) = state.messages.last_mut() {
        if last_msg.role == MessageRole::User {
            let mut new_content = Vec::new();
            let mut modified = false;
            for block in last_msg.content.drain(..) {
                if let ContentBlock::Image { ref data, ref media_type } = block {
                    let size_bytes = data.len();
                    let estimated_tokens = (size_bytes + 3) / 4;
                    let source = alva_agent_context::MediaSource::UserMessage {
                        message_id: last_msg.id.clone(),
                    };
                    let decision = config.context_plugin.on_inject_media(
                        config.context_sdk.as_ref(),
                        &state.session_id,
                        media_type,
                        source,
                        size_bytes,
                        estimated_tokens,
                    ).await;
                    match decision {
                        alva_agent_context::InjectDecision::Allow(alva_agent_context::MediaAction::Keep) => {
                            new_content.push(block);
                        }
                        alva_agent_context::InjectDecision::Allow(alva_agent_context::MediaAction::Describe { description }) => {
                            new_content.push(ContentBlock::Text { text: description });
                            modified = true;
                        }
                        alva_agent_context::InjectDecision::Allow(alva_agent_context::MediaAction::Remove)
                        | alva_agent_context::InjectDecision::Allow(alva_agent_context::MediaAction::Externalize { .. }) => {
                            modified = true;
                            // Drop the image block.
                        }
                        alva_agent_context::InjectDecision::Reject { reason } => {
                            new_content.push(ContentBlock::Text {
                                text: format!("[Image rejected: {}]", reason),
                            });
                            modified = true;
                        }
                        alva_agent_context::InjectDecision::Modify(action) => {
                            match action {
                                alva_agent_context::MediaAction::Keep => {
                                    new_content.push(block);
                                }
                                alva_agent_context::MediaAction::Describe { description } => {
                                    new_content.push(ContentBlock::Text { text: description });
                                    modified = true;
                                }
                                alva_agent_context::MediaAction::Remove
                                | alva_agent_context::MediaAction::Externalize { .. } => {
                                    modified = true;
                                }
                            }
                        }
                        alva_agent_context::InjectDecision::Summarize { summary } => {
                            new_content.push(ContentBlock::Text { text: summary });
                            modified = true;
                        }
                    }
                } else {
                    new_content.push(block);
                }
            }
            if modified {
                debug!(msg_id = %last_msg.id, "on_inject_media: modified user message content");
            }
            last_msg.content = new_content;
        }
    }

    let result = run_agent_loop_inner(state, model, config, cancel, event_tx, &mut mw_ctx).await;

    // Persist turn to MessageStore.
    if let Some(store) = &config.message_store {
        let turn_messages: Vec<AgentMessage> = state.messages[turn_start_msg_index..].to_vec();
        if let Some((user_msg, agent_msgs)) = turn_messages.split_first() {
            let turn = alva_agent_context::Turn {
                index: store.turn_count(&state.session_id).await,
                user_message: user_msg.clone(),
                agent_messages: agent_msgs.to_vec(),
                started_at: turn_start_time,
                completed_at: Some(chrono::Utc::now().timestamp_millis()),
            };
            store.append_turn(&state.session_id, turn).await;
        }
    }

    // Context plugin: after_turn
    config.context_plugin.after_turn(config.context_sdk.as_ref(), &state.session_id).await;

    // Middleware: on_agent_end
    if !config.middleware.is_empty() {
        mw_ctx.messages = state.messages.clone();
        let err_str = match &result {
            Ok(()) => None,
            Err(e) => Some(e.to_string()),
        };
        if let Err(e) = config
            .middleware
            .run_on_agent_end(&mut mw_ctx, err_str.as_deref())
            .await
        {
            warn!(error = %e, "middleware on_agent_end failed");
        }
    }

    // Context plugin: on_agent_end
    {
        let error = match &result {
            Ok(()) => None,
            Err(e) => Some(e.to_string()),
        };
        config.context_plugin.on_agent_end(
            config.context_sdk.as_ref(), &state.session_id, error.as_deref()
        ).await;
    }

    match &result {
        Ok(()) => {
            let _ = event_tx.send(AgentEvent::AgentEnd { error: None });
        }
        Err(e) => {
            let _ = event_tx.send(AgentEvent::AgentEnd {
                error: Some(e.to_string()),
            });
        }
    }

    result
}

async fn run_agent_loop_inner(
    state: &mut AgentState,
    model: &dyn LanguageModel,
    config: &AgentHooks,
    cancel: &CancellationToken,
    event_tx: &mpsc::UnboundedSender<AgentEvent>,
    mw_ctx: &mut MiddlewareContext,
) -> Result<(), alva_types::AgentError> {
    let mut iteration: u32 = 0;

    // ===== OUTER LOOP (follow-up) ==========================================
    'outer: loop {
        if cancel.is_cancelled() {
            return Err(alva_types::AgentError::Cancelled);
        }

        // ===== INNER LOOP (tool calls + steering) ==========================
        'inner: loop {
            iteration += 1;
            if iteration > config.max_iterations {
                return Err(alva_types::AgentError::MaxIterations(
                    config.max_iterations,
                ));
            }

            if cancel.is_cancelled() {
                return Err(alva_types::AgentError::Cancelled);
            }

            let _ = event_tx.send(AgentEvent::TurnStart);

            // 1. Build the messages to send to the LLM ---------------------

            // 1a. Sync actual token usage from state.messages into ContextStore
            // so that sdk.budget() returns real values instead of 0.
            {
                let used_tokens: usize = state.messages.iter().map(|m| {
                    match m {
                        AgentMessage::Standard(msg) => msg.content.iter().map(|b| b.estimated_tokens()).sum::<usize>(),
                        AgentMessage::Custom { data, .. } => data.to_string().len() / 4,
                    }
                }).sum();
                config.context_sdk.sync_external_usage(&state.session_id, used_tokens);
            }

            // 1b. Check budget and trigger compression if exceeded.
            //
            // NOTE: The snapshot passed to on_budget_exceeded only contains
            // the synthetic usage entry (from sync_external_usage), not real
            // conversation entries. Snapshot-based actions (RemoveByPriority,
            // ReplaceToolResult) are therefore limited. The primary effective
            // actions are SlidingWindow (applied here) and the three-strategy
            // compression in assemble() which operates on real messages.
            let budget = config.context_sdk.budget(&state.session_id);
            if budget.usage_ratio > 0.7 {
                let snapshot = config.context_sdk.snapshot(&state.session_id);
                let actions = config.context_plugin.on_budget_exceeded(
                    config.context_sdk.as_ref(),
                    &state.session_id,
                    &snapshot,
                ).await;

                for action in actions {
                    match action {
                        alva_agent_context::CompressAction::SlidingWindow { keep_recent } => {
                            if state.messages.len() > keep_recent {
                                let drop_count = state.messages.len() - keep_recent;
                                state.messages.drain(..drop_count);
                                debug!(drop_count, keep_recent, "budget: applied sliding window");
                            }
                        }
                        alva_agent_context::CompressAction::RemoveByPriority { .. } => {
                            // Priority-based removal operates on ContextStore entries.
                            // Since messages live in state.messages, this is a no-op
                            // here. Will activate once ContextStore is integrated with
                            // the real conversation. For now, assemble() handles all
                            // message-level compression.
                        }
                        alva_agent_context::CompressAction::ReplaceToolResult { message_id, summary } => {
                            // Replace matching tool result in state.messages by message id.
                            for msg in state.messages.iter_mut() {
                                if let AgentMessage::Standard(m) = msg {
                                    if m.id == message_id {
                                        m.content = vec![ContentBlock::Text { text: summary.clone() }];
                                        break;
                                    }
                                }
                            }
                        }
                        alva_agent_context::CompressAction::Summarize { range, hints } => {
                            // Resolve the MessageRange to indices on state.messages.
                            let msg_len = state.messages.len();
                            let from_idx = match &range.from {
                                alva_agent_context::MessageSelector::FromStart => 0,
                                alva_agent_context::MessageSelector::ByIndex(i) => (*i).min(msg_len),
                                alva_agent_context::MessageSelector::ById(id) => {
                                    state.messages.iter().position(|m| match m {
                                        AgentMessage::Standard(msg) => msg.id == *id,
                                        _ => false,
                                    }).unwrap_or(0)
                                }
                                alva_agent_context::MessageSelector::ToEnd => 0,
                            };
                            let to_idx = match &range.to {
                                alva_agent_context::MessageSelector::ToEnd => msg_len,
                                alva_agent_context::MessageSelector::ByIndex(i) => (*i).min(msg_len),
                                alva_agent_context::MessageSelector::ById(id) => {
                                    state.messages.iter().position(|m| match m {
                                        AgentMessage::Standard(msg) => msg.id == *id,
                                        _ => false,
                                    }).map(|i| i + 1).unwrap_or(msg_len)
                                }
                                alva_agent_context::MessageSelector::FromStart => msg_len,
                            };

                            if from_idx < to_idx && to_idx <= msg_len {
                                // Serialize the messages in the range to text for summarization.
                                let range_text: String = state.messages[from_idx..to_idx]
                                    .iter()
                                    .enumerate()
                                    .map(|(i, m)| {
                                        let role = match m {
                                            AgentMessage::Standard(msg) => format!("{:?}", msg.role),
                                            AgentMessage::Custom { .. } => "Custom".to_string(),
                                        };
                                        let text = match m {
                                            AgentMessage::Standard(msg) => msg.text_content(),
                                            AgentMessage::Custom { data, .. } => data.to_string(),
                                        };
                                        // Truncate individual message text to avoid blowing up the summary input
                                        let truncated = if text.len() > 2000 {
                                            format!("{}...[truncated]", &text[..2000])
                                        } else {
                                            text
                                        };
                                        format!("[{}] {}: {}", from_idx + i, role, truncated)
                                    })
                                    .collect::<Vec<_>>()
                                    .join("\n");

                                // Call SDK summarize with a 5-second timeout.
                                let summary_result = tokio::time::timeout(
                                    std::time::Duration::from_secs(5),
                                    config.context_sdk.summarize(
                                        &state.session_id,
                                        range.clone(),
                                        &hints,
                                    ),
                                ).await;

                                let summary_text = match summary_result {
                                    Ok(s) => s,
                                    Err(_) => {
                                        warn!("budget: summarize timed out, falling back to truncation");
                                        let truncated: String = range_text.chars().take(2000).collect();
                                        format!("{}\n\n[... summarization timed out, truncated]", truncated)
                                    }
                                };

                                // Replace the range with a single system message containing the summary.
                                let summary_msg = AgentMessage::Standard(alva_types::Message {
                                    id: uuid::Uuid::new_v4().to_string(),
                                    role: MessageRole::User,
                                    content: vec![ContentBlock::Text {
                                        text: format!(
                                            "<conversation_summary>\n{}\n</conversation_summary>",
                                            summary_text
                                        ),
                                    }],
                                    tool_call_id: None,
                                    usage: None,
                                    timestamp: chrono::Utc::now().timestamp_millis(),
                                });

                                // Drain the range and insert the summary message.
                                state.messages.drain(from_idx..to_idx);
                                state.messages.insert(from_idx, summary_msg);
                                debug!(
                                    from = from_idx,
                                    to = to_idx,
                                    "budget: applied LLM summarization, replaced {} messages with summary",
                                    to_idx - from_idx
                                );
                            } else {
                                debug!(from = from_idx, to = to_idx, len = msg_len, "budget: summarize range invalid, skipping");
                            }
                        }
                        alva_agent_context::CompressAction::Externalize { .. } => {
                            // File externalization not yet implemented.
                            debug!("budget: externalize action not yet implemented");
                        }
                    }
                }

                // Re-sync usage after compression.
                let used_tokens: usize = state.messages.iter().map(|m| {
                    match m {
                        AgentMessage::Standard(msg) => msg.content.iter().map(|b| b.estimated_tokens()).sum::<usize>(),
                        AgentMessage::Custom { data, .. } => data.to_string().len() / 4,
                    }
                }).sum();
                config.context_sdk.sync_external_usage(&state.session_id, used_tokens);
            }

            // 1c. Context plugin: on_inject_system_prompt — let plugin modify system prompt sections.
            // Default plugin returns sections unchanged (prompt-cache friendly).
            // Custom plugins may add, remove, or reorder sections.
            {
                let sections = vec![alva_agent_context::PromptSection {
                    id: "system".to_string(),
                    content: state.system_prompt.clone(),
                    priority: alva_agent_context::Priority::Critical,
                }];
                let modified_sections = config.context_plugin.on_inject_system_prompt(
                    config.context_sdk.as_ref(),
                    &state.session_id,
                    sections,
                ).await;
                state.system_prompt = modified_sections
                    .iter()
                    .map(|s| s.content.as_str())
                    .collect::<Vec<_>>()
                    .join("\n\n");
            }

            // 1d. Assemble context (plugin applies its own compression strategies).
            let budget = config.context_sdk.budget(&state.session_id);
            let context_messages = config.context_plugin.assemble(
                config.context_sdk.as_ref(),
                &state.session_id,
                state.messages.clone(),
                budget.budget_tokens,
            ).await;

            let convert_ctx = AgentContext {
                system_prompt: &state.system_prompt,
                messages: &context_messages,
                tools: &state.tools,
            };
            let mut llm_messages = (config.convert_to_llm)(&convert_ctx);

            // 1b. Middleware: before_llm_call --------------------------------
            if !config.middleware.is_empty() {
                mw_ctx.messages = state.messages.clone();
                if let Err(e) = config
                    .middleware
                    .run_before_llm_call(mw_ctx, &mut llm_messages)
                    .await
                {
                    warn!(error = %e, "middleware before_llm_call failed");
                }
            }

            // 2. Collect tool references for the model ----------------------
            let tool_refs: Vec<&dyn alva_types::Tool> =
                state.tools.iter().map(|t| t.as_ref()).collect();

            // 3. Call the model ---------------------------------------------
            debug!(
                model = model.model_id(),
                messages = llm_messages.len(),
                tools = tool_refs.len(),
                "calling LLM"
            );

            let mut assistant_message = if state.is_streaming {
                stream_llm_response(model, &llm_messages, &tool_refs, &state.model_config, event_tx).await?
            } else {
                model.complete(&llm_messages, &tool_refs, &state.model_config).await?
            };

            // 3b. Middleware: after_llm_call ---------------------------------
            if !config.middleware.is_empty() {
                mw_ctx.messages = state.messages.clone();
                if let Err(e) = config
                    .middleware
                    .run_after_llm_call(mw_ctx, &mut assistant_message)
                    .await
                {
                    warn!(error = %e, "middleware after_llm_call failed");
                }
            }

            let agent_msg = AgentMessage::Standard(assistant_message.clone());

            // 3c. Context plugin: on_llm_output
            config.context_plugin.on_llm_output(
                config.context_sdk.as_ref(), &state.session_id, &agent_msg
            ).await;

            // 4. Emit message events ----------------------------------------
            let _ = event_tx.send(AgentEvent::MessageStart {
                message: agent_msg.clone(),
            });
            let _ = event_tx.send(AgentEvent::MessageEnd {
                message: agent_msg.clone(),
            });

            // 5. Context plugin: ingest — let plugin decide keep/modify/skip
            let mut agent_msg = agent_msg;
            {
                let ingest_action = config.context_plugin.ingest(
                    config.context_sdk.as_ref(), &state.session_id, &mut agent_msg,
                ).await;
                match ingest_action {
                    alva_agent_context::IngestAction::Skip => {
                        // Plugin says skip — do NOT push to state.messages.
                        // But we still need to check for tool calls below.
                    }
                    alva_agent_context::IngestAction::Modify(new_msg) => {
                        state.messages.push(new_msg);
                    }
                    alva_agent_context::IngestAction::TagAndKeep { .. } => {
                        // TODO: apply priority tag to ContextStore entry when
                        // ContextStore is integrated with state.messages.
                        state.messages.push(agent_msg.clone());
                    }
                    alva_agent_context::IngestAction::Keep => {
                        state.messages.push(agent_msg.clone());
                    }
                }
            }

            // 6. Check for tool calls ---------------------------------------
            let tool_calls: Vec<ToolCall> = assistant_message
                .content
                .iter()
                .filter_map(|b| {
                    b.as_tool_use().map(|(id, name, input)| ToolCall {
                        id: id.to_owned(),
                        name: name.to_owned(),
                        arguments: input.clone(),
                    })
                })
                .collect();

            if tool_calls.is_empty() {
                // No tool calls — the model is done for this inner loop.
                let _ = event_tx.send(AgentEvent::TurnEnd);
                break 'inner;
            }

            // 7. Execute tools ----------------------------------------------
            let context = AgentContext {
                system_prompt: &state.system_prompt,
                messages: &state.messages,
                tools: &state.tools,
            };

            mw_ctx.messages = state.messages.clone();
            let results = execute_tools(
                &tool_calls,
                &state.tools,
                config,
                &context,
                cancel,
                event_tx,
                &state.tool_context,
                mw_ctx,
                &config.context_plugin,
                &config.context_sdk,
                &state.session_id,
            )
            .await;

            // 8. Push tool results as messages into state -------------------
            for (tc, result) in tool_calls.iter().zip(results.iter()) {
                let tool_msg = Message {
                    id: uuid::Uuid::new_v4().to_string(),
                    role: MessageRole::Tool,
                    content: vec![ContentBlock::ToolResult {
                        id: tc.id.clone(),
                        content: result.content.clone(),
                        is_error: result.is_error,
                    }],
                    tool_call_id: Some(tc.id.clone()),
                    usage: None,
                    timestamp: chrono::Utc::now().timestamp_millis(),
                };
                let mut tool_agent_msg = AgentMessage::Standard(tool_msg);

                // Context plugin: ingest — let plugin decide on tool results too.
                let ingest_action = config.context_plugin.ingest(
                    config.context_sdk.as_ref(), &state.session_id, &mut tool_agent_msg,
                ).await;
                match ingest_action {
                    alva_agent_context::IngestAction::Skip => {
                        // Plugin says skip this tool result.
                    }
                    alva_agent_context::IngestAction::Modify(new_msg) => {
                        state.messages.push(new_msg);
                    }
                    alva_agent_context::IngestAction::TagAndKeep { .. } => {
                        state.messages.push(tool_agent_msg);
                    }
                    alva_agent_context::IngestAction::Keep => {
                        state.messages.push(tool_agent_msg);
                    }
                }
            }

            let _ = event_tx.send(AgentEvent::TurnEnd);

            // 9. Steering messages ------------------------------------------
            let mut steering = Vec::new();
            for hook in &config.get_steering_messages {
                let ctx = AgentContext {
                    system_prompt: &state.system_prompt,
                    messages: &state.messages,
                    tools: &state.tools,
                };
                steering.extend(hook(&ctx));
            }
            if !steering.is_empty() {
                info!(count = steering.len(), "injecting steering messages");
                state.messages.extend(steering);
                continue 'inner;
            }

            // Tool results exist — we need another LLM call, so continue.
            continue 'inner;
        }
        // ===== END INNER LOOP ==============================================

        // 10. Follow-up messages --------------------------------------------
        let mut follow_ups = Vec::new();
        for hook in &config.get_follow_up_messages {
            let ctx = AgentContext {
                system_prompt: &state.system_prompt,
                messages: &state.messages,
                tools: &state.tools,
            };
            follow_ups.extend(hook(&ctx));
        }
        if !follow_ups.is_empty() {
            info!(count = follow_ups.len(), "injecting follow-up messages");
            state.messages.extend(follow_ups);
            continue 'outer;
        }

        // Nothing more to do.
        break 'outer;
    }
    // ===== END OUTER LOOP ==================================================

    Ok(())
}

// ===========================================================================
// Streaming helper
// ===========================================================================

/// Consumes the model's stream, emitting MessageUpdate events and accumulating
/// the final Message from deltas.
async fn stream_llm_response(
    model: &dyn LanguageModel,
    messages: &[Message],
    tools: &[&dyn alva_types::Tool],
    config: &alva_types::ModelConfig,
    event_tx: &mpsc::UnboundedSender<AgentEvent>,
) -> Result<Message, alva_types::AgentError> {
    let mut stream = model.stream(messages, tools, config);

    let mut text = String::new();
    let mut reasoning = String::new();
    let mut tool_call_accumulators: Vec<ToolCallAccumulator> = Vec::new();
    let mut usage = None;

    while let Some(event) = stream.next().await {
        match &event {
            StreamEvent::TextDelta { text: delta } => {
                text.push_str(delta);
            }
            StreamEvent::ReasoningDelta { text: delta } => {
                reasoning.push_str(delta);
            }
            StreamEvent::ToolCallDelta {
                id,
                name,
                arguments_delta,
            } => {
                if let Some(acc) = tool_call_accumulators.iter_mut().find(|a| a.id == *id) {
                    acc.arguments_json.push_str(arguments_delta);
                    if let Some(n) = name {
                        acc.name = n.clone();
                    }
                } else {
                    tool_call_accumulators.push(ToolCallAccumulator {
                        id: id.clone(),
                        name: name.clone().unwrap_or_default(),
                        arguments_json: arguments_delta.clone(),
                    });
                }
            }
            StreamEvent::Usage(u) => {
                usage = Some(u.clone());
            }
            StreamEvent::Error(e) => {
                return Err(alva_types::AgentError::LlmError(e.clone()));
            }
            _ => {} // Start, Done
        }

        // Build partial message from accumulated state so far
        let mut partial_content = Vec::new();
        if !text.is_empty() {
            partial_content.push(ContentBlock::Text { text: text.clone() });
        }
        if !reasoning.is_empty() {
            partial_content.push(ContentBlock::Reasoning { text: reasoning.clone() });
        }
        for acc in &tool_call_accumulators {
            let input: serde_json::Value = serde_json::from_str(&acc.arguments_json)
                .unwrap_or(serde_json::Value::String(acc.arguments_json.clone()));
            partial_content.push(ContentBlock::ToolUse {
                id: acc.id.clone(),
                name: acc.name.clone(),
                input,
            });
        }

        let partial_message = Message {
            id: String::new(),
            role: MessageRole::Assistant,
            content: partial_content,
            tool_call_id: None,
            usage: usage.clone(),
            timestamp: chrono::Utc::now().timestamp_millis(),
        };

        let _ = event_tx.send(AgentEvent::MessageUpdate {
            message: AgentMessage::Standard(partial_message),
            delta: event,
        });
    }

    // Build final message from accumulated deltas
    let mut content = Vec::new();
    if !text.is_empty() {
        content.push(ContentBlock::Text { text });
    }
    if !reasoning.is_empty() {
        content.push(ContentBlock::Reasoning { text: reasoning });
    }
    for acc in &tool_call_accumulators {
        let input: serde_json::Value = serde_json::from_str(&acc.arguments_json)
            .unwrap_or(serde_json::Value::String(acc.arguments_json.clone()));
        content.push(ContentBlock::ToolUse {
            id: acc.id.clone(),
            name: acc.name.clone(),
            input,
        });
    }

    Ok(Message {
        id: uuid::Uuid::new_v4().to_string(),
        role: MessageRole::Assistant,
        content,
        tool_call_id: None,
        usage,
        timestamp: chrono::Utc::now().timestamp_millis(),
    })
}

struct ToolCallAccumulator {
    id: String,
    name: String,
    arguments_json: String,
}

// ===========================================================================
// Tests
// ===========================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use alva_types::*;
    use async_trait::async_trait;
    use std::pin::Pin;
    use std::sync::Arc;

    // -----------------------------------------------------------------------
    // Mock model that returns a simple text response with no tool calls
    // -----------------------------------------------------------------------
    struct MockModel;

    #[async_trait]
    impl LanguageModel for MockModel {
        async fn complete(
            &self,
            _messages: &[Message],
            _tools: &[&dyn Tool],
            _config: &ModelConfig,
        ) -> Result<Message, AgentError> {
            Ok(Message {
                id: "mock-msg-1".to_string(),
                role: MessageRole::Assistant,
                content: vec![ContentBlock::Text {
                    text: "Hello from mock model!".to_string(),
                }],
                tool_call_id: None,
                usage: Some(UsageMetadata {
                    input_tokens: 10,
                    output_tokens: 5,
                    total_tokens: 15,
                }),
                timestamp: chrono::Utc::now().timestamp_millis(),
            })
        }

        fn stream(
            &self,
            _messages: &[Message],
            _tools: &[&dyn Tool],
            _config: &ModelConfig,
        ) -> Pin<Box<dyn futures_core::Stream<Item = StreamEvent> + Send>> {
            // Return an empty stream using async_stream-style manual impl.
            struct EmptyStream;
            impl futures_core::Stream for EmptyStream {
                type Item = StreamEvent;
                fn poll_next(
                    self: Pin<&mut Self>,
                    _cx: &mut std::task::Context<'_>,
                ) -> std::task::Poll<Option<Self::Item>> {
                    std::task::Poll::Ready(None)
                }
            }
            // SAFETY: EmptyStream has no state, trivially Send.
            unsafe impl Send for EmptyStream {}
            Box::pin(EmptyStream)
        }

        fn model_id(&self) -> &str {
            "mock-model"
        }
    }

    // -----------------------------------------------------------------------
    // Helper: default convert_to_llm — just extract Standard messages
    // -----------------------------------------------------------------------
    fn default_convert_to_llm(ctx: &AgentContext<'_>) -> Vec<Message> {
        let mut result = vec![Message::system(ctx.system_prompt)];
        for m in ctx.messages {
            if let AgentMessage::Standard(msg) = m {
                result.push(msg.clone());
            }
        }
        result
    }

    // -----------------------------------------------------------------------
    // Test: simple text response with no tool calls
    // -----------------------------------------------------------------------
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn test_simple_text_response() {
        let model = MockModel;
        let config = AgentHooks::new(Arc::new(default_convert_to_llm));
        let cancel = CancellationToken::new();
        let (event_tx, mut event_rx) = mpsc::unbounded_channel();

        let mut state = AgentState::new(
            "test-session",
            "You are a test assistant.".to_string(),
            ModelConfig::default(),
        );

        // Seed a user message.
        state
            .messages
            .push(AgentMessage::Standard(Message::user("Hi")));

        let result =
            run_agent_loop(&mut state, &model, &config, &cancel, &event_tx)
                .await;

        assert!(result.is_ok(), "agent loop should succeed");

        // Collect all events.
        drop(event_tx); // close channel so recv terminates
        let mut events = Vec::new();
        while let Some(ev) = event_rx.recv().await {
            events.push(ev);
        }

        // Verify expected event sequence.
        assert!(
            matches!(events.first(), Some(AgentEvent::AgentStart)),
            "first event should be AgentStart"
        );
        assert!(
            matches!(events.last(), Some(AgentEvent::AgentEnd { error: None })),
            "last event should be AgentEnd with no error"
        );

        // Should contain a MessageEnd with the assistant response.
        let has_message_end = events.iter().any(|e| {
            matches!(e, AgentEvent::MessageEnd { message: AgentMessage::Standard(m) } if m.text_content() == "Hello from mock model!")
        });
        assert!(has_message_end, "should have MessageEnd with model response");

        // State should now have 2 messages: the user message + assistant.
        assert_eq!(state.messages.len(), 2);
    }

    // -----------------------------------------------------------------------
    // Test: cancellation stops the loop
    // -----------------------------------------------------------------------
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn test_cancellation() {
        let model = MockModel;
        let config = AgentHooks::new(Arc::new(default_convert_to_llm));
        let cancel = CancellationToken::new();
        cancel.cancel(); // pre-cancel

        let (event_tx, _event_rx) = mpsc::unbounded_channel();
        let mut state = AgentState::new(
            "test-session",
            "test".to_string(),
            ModelConfig::default(),
        );
        state
            .messages
            .push(AgentMessage::Standard(Message::user("Hi")));

        let result =
            run_agent_loop(&mut state, &model, &config, &cancel, &event_tx)
                .await;

        assert!(
            matches!(result, Err(AgentError::Cancelled)),
            "should return Cancelled"
        );
    }

    // -----------------------------------------------------------------------
    // Test: streaming text response
    // -----------------------------------------------------------------------
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn test_streaming_text_response() {
        // Create a model that implements stream() returning TextDelta events
        struct StreamingMockModel;

        #[async_trait]
        impl LanguageModel for StreamingMockModel {
            async fn complete(
                &self,
                _messages: &[Message],
                _tools: &[&dyn Tool],
                _config: &ModelConfig,
            ) -> Result<Message, AgentError> {
                panic!("should use stream path, not complete")
            }

            fn stream(
                &self,
                _messages: &[Message],
                _tools: &[&dyn Tool],
                _config: &ModelConfig,
            ) -> Pin<Box<dyn futures_core::Stream<Item = StreamEvent> + Send>> {
                Box::pin(tokio_stream::iter(vec![
                    StreamEvent::Start,
                    StreamEvent::TextDelta {
                        text: "Hello ".into(),
                    },
                    StreamEvent::TextDelta {
                        text: "world!".into(),
                    },
                    StreamEvent::Done,
                ]))
            }

            fn model_id(&self) -> &str {
                "streaming-mock"
            }
        }

        let config = AgentHooks::new(Arc::new(default_convert_to_llm));
        let cancel = CancellationToken::new();
        let (event_tx, mut event_rx) = mpsc::unbounded_channel();

        let mut state = AgentState::new("test-session", "test".to_string(), ModelConfig::default());
        state.is_streaming = true;
        state
            .messages
            .push(AgentMessage::Standard(Message::user("Hi")));

        let result = run_agent_loop(
            &mut state,
            &StreamingMockModel,
            &config,
            &cancel,
            &event_tx,
        )
        .await;
        assert!(result.is_ok(), "agent loop should succeed");

        drop(event_tx);
        let mut got_update = false;
        let mut got_message_end = false;
        while let Some(ev) = event_rx.recv().await {
            if matches!(ev, AgentEvent::MessageUpdate { .. }) {
                got_update = true;
            }
            if let AgentEvent::MessageEnd {
                message: AgentMessage::Standard(m),
            } = &ev
            {
                if m.text_content() == "Hello world!" {
                    got_message_end = true;
                }
            }
        }
        assert!(got_update, "should have received MessageUpdate events");
        assert!(
            got_message_end,
            "should have MessageEnd with accumulated text"
        );

        // State should have the accumulated message
        assert_eq!(state.messages.len(), 2); // user + assistant
    }

    // -----------------------------------------------------------------------
    // Test: streaming emits partial messages (not placeholders) in MessageUpdate
    // -----------------------------------------------------------------------
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn test_streaming_emits_partial_message() {
        struct PartialStreamModel;

        #[async_trait]
        impl LanguageModel for PartialStreamModel {
            async fn complete(
                &self,
                _messages: &[Message],
                _tools: &[&dyn Tool],
                _config: &ModelConfig,
            ) -> Result<Message, AgentError> {
                panic!("should use stream path")
            }

            fn stream(
                &self,
                _messages: &[Message],
                _tools: &[&dyn Tool],
                _config: &ModelConfig,
            ) -> Pin<Box<dyn futures_core::Stream<Item = StreamEvent> + Send>> {
                Box::pin(tokio_stream::iter(vec![
                    StreamEvent::Start,
                    StreamEvent::TextDelta { text: "Hello ".into() },
                    StreamEvent::TextDelta { text: "world!".into() },
                    StreamEvent::Done,
                ]))
            }

            fn model_id(&self) -> &str {
                "partial-stream-mock"
            }
        }

        let config = AgentHooks::new(Arc::new(default_convert_to_llm));
        let cancel = CancellationToken::new();
        let (event_tx, mut event_rx) = mpsc::unbounded_channel();

        let mut state = AgentState::new("test-session", "test".to_string(), ModelConfig::default());
        state.is_streaming = true;
        state.messages.push(AgentMessage::Standard(Message::user("Hi")));

        let result = run_agent_loop(
            &mut state,
            &PartialStreamModel,
            &config,
            &cancel,
            &event_tx,
        )
        .await;
        assert!(result.is_ok());

        drop(event_tx);
        let mut partial_texts = Vec::new();
        while let Some(ev) = event_rx.recv().await {
            if let AgentEvent::MessageUpdate {
                message: AgentMessage::Standard(m),
                ..
            } = &ev
            {
                partial_texts.push(m.text_content());
            }
        }

        // After "Hello " delta, partial should contain "Hello "
        // After "world!" delta, partial should contain "Hello world!"
        assert!(
            partial_texts.iter().any(|t| t == "Hello "),
            "should have partial with 'Hello ', got: {:?}",
            partial_texts
        );
        assert!(
            partial_texts.iter().any(|t| t == "Hello world!"),
            "should have partial with 'Hello world!', got: {:?}",
            partial_texts
        );
    }

    // -----------------------------------------------------------------------
    // Test: simple text response using alva-test MockLanguageModel + fixtures
    // -----------------------------------------------------------------------
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn test_text_response_with_alva_test_mock() {
        use alva_test::mock_provider::MockLanguageModel;
        use alva_test::fixtures::{make_assistant_message, make_user_message};

        let response = make_assistant_message("Hello from alva-test mock!");
        let model = MockLanguageModel::new().with_response(response);

        let config = AgentHooks::new(Arc::new(default_convert_to_llm));
        let cancel = CancellationToken::new();
        let (event_tx, mut event_rx) = mpsc::unbounded_channel();

        let mut state = AgentState::new(
            "test-session",
            "You are a test assistant.".to_string(),
            ModelConfig::default(),
        );
        state
            .messages
            .push(AgentMessage::Standard(make_user_message("Hi")));

        let result =
            run_agent_loop(&mut state, &model, &config, &cancel, &event_tx).await;

        assert!(result.is_ok(), "agent loop should succeed");

        // Collect all events.
        drop(event_tx);
        let mut events = Vec::new();
        while let Some(ev) = event_rx.recv().await {
            events.push(ev);
        }

        // Verify the expected event sequence.
        assert!(
            matches!(events.first(), Some(AgentEvent::AgentStart)),
            "first event should be AgentStart"
        );
        assert!(
            matches!(events.last(), Some(AgentEvent::AgentEnd { error: None })),
            "last event should be AgentEnd with no error"
        );

        // Should contain TurnStart and TurnEnd.
        let has_turn_start = events.iter().any(|e| matches!(e, AgentEvent::TurnStart));
        let has_turn_end = events.iter().any(|e| matches!(e, AgentEvent::TurnEnd));
        assert!(has_turn_start, "should have TurnStart event");
        assert!(has_turn_end, "should have TurnEnd event");

        // Should contain a MessageStart and MessageEnd with the assistant response.
        let has_message_start = events.iter().any(|e| {
            matches!(e, AgentEvent::MessageStart { message: AgentMessage::Standard(m) } if m.text_content() == "Hello from alva-test mock!")
        });
        let has_message_end = events.iter().any(|e| {
            matches!(e, AgentEvent::MessageEnd { message: AgentMessage::Standard(m) } if m.text_content() == "Hello from alva-test mock!")
        });
        assert!(has_message_start, "should have MessageStart with model response");
        assert!(has_message_end, "should have MessageEnd with model response");

        // State should now have 2 messages: the user message + assistant.
        assert_eq!(state.messages.len(), 2);

        // Verify the mock recorded exactly one call.
        let calls = model.calls();
        assert_eq!(calls.len(), 1, "model should have been called exactly once");
        // The call should contain the system prompt + the user message.
        assert_eq!(calls[0].len(), 2, "call should have system + user message");
    }

    // -----------------------------------------------------------------------
    // Test: model error propagates correctly using alva-test MockLanguageModel
    // -----------------------------------------------------------------------
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn test_model_error_with_alva_test_mock() {
        use alva_test::mock_provider::MockLanguageModel;
        use alva_test::fixtures::make_user_message;

        let model = MockLanguageModel::new()
            .with_error(AgentError::LlmError("boom from mock".into()));

        let config = AgentHooks::new(Arc::new(default_convert_to_llm));
        let cancel = CancellationToken::new();
        let (event_tx, mut event_rx) = mpsc::unbounded_channel();

        let mut state = AgentState::new(
            "test-session",
            "test".to_string(),
            ModelConfig::default(),
        );
        state
            .messages
            .push(AgentMessage::Standard(make_user_message("Hi")));

        let result =
            run_agent_loop(&mut state, &model, &config, &cancel, &event_tx).await;

        assert!(result.is_err(), "agent loop should fail when model errors");
        let err_str = result.unwrap_err().to_string();
        assert!(
            err_str.contains("boom from mock"),
            "error should contain the mock error message, got: {err_str}"
        );

        // Events should still have AgentStart and AgentEnd (with error).
        drop(event_tx);
        let mut events = Vec::new();
        while let Some(ev) = event_rx.recv().await {
            events.push(ev);
        }

        assert!(
            matches!(events.first(), Some(AgentEvent::AgentStart)),
            "first event should be AgentStart"
        );
        assert!(
            matches!(events.last(), Some(AgentEvent::AgentEnd { error: Some(_) })),
            "last event should be AgentEnd with an error"
        );
    }

    // -----------------------------------------------------------------------
    // Test: streaming with alva-test MockLanguageModel
    // -----------------------------------------------------------------------
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn test_streaming_with_alva_test_mock() {
        use alva_test::mock_provider::MockLanguageModel;
        use alva_test::fixtures::make_user_message;

        let model = MockLanguageModel::new().with_stream_events(vec![
            StreamEvent::Start,
            StreamEvent::TextDelta {
                text: "Streamed ".into(),
            },
            StreamEvent::TextDelta {
                text: "response!".into(),
            },
            StreamEvent::Done,
        ]);

        let config = AgentHooks::new(Arc::new(default_convert_to_llm));
        let cancel = CancellationToken::new();
        let (event_tx, mut event_rx) = mpsc::unbounded_channel();

        let mut state = AgentState::new("test-session", "test".to_string(), ModelConfig::default());
        state.is_streaming = true;
        state
            .messages
            .push(AgentMessage::Standard(make_user_message("Hi")));

        let result =
            run_agent_loop(&mut state, &model, &config, &cancel, &event_tx).await;
        assert!(result.is_ok(), "agent loop should succeed");

        drop(event_tx);
        let mut got_update = false;
        let mut got_message_end = false;
        while let Some(ev) = event_rx.recv().await {
            if matches!(ev, AgentEvent::MessageUpdate { .. }) {
                got_update = true;
            }
            if let AgentEvent::MessageEnd {
                message: AgentMessage::Standard(m),
            } = &ev
            {
                if m.text_content() == "Streamed response!" {
                    got_message_end = true;
                }
            }
        }
        assert!(got_update, "should have received MessageUpdate events");
        assert!(
            got_message_end,
            "should have MessageEnd with accumulated streamed text"
        );

        // State should have the accumulated message.
        assert_eq!(state.messages.len(), 2); // user + assistant
    }

    // -----------------------------------------------------------------------
    // Test: tool call round-trip using alva-test MockLanguageModel + MockTool
    // -----------------------------------------------------------------------
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn test_tool_call_round_trip_with_alva_test_mocks() {
        use alva_test::mock_provider::MockLanguageModel;
        use alva_test::mock_tool::MockTool;
        use alva_test::fixtures::make_user_message;

        // First call: model requests a tool call.
        let tool_call_response = Message {
            id: "msg-tc".to_string(),
            role: MessageRole::Assistant,
            content: vec![ContentBlock::ToolUse {
                id: "call-1".to_string(),
                name: "greet".to_string(),
                input: serde_json::json!({"name": "World"}),
            }],
            tool_call_id: None,
            usage: None,
            timestamp: 0,
        };
        // Second call: model returns final text after receiving tool result.
        let final_response = Message {
            id: "msg-final".to_string(),
            role: MessageRole::Assistant,
            content: vec![ContentBlock::Text {
                text: "Greeting sent!".to_string(),
            }],
            tool_call_id: None,
            usage: None,
            timestamp: 0,
        };

        let model = MockLanguageModel::new()
            .with_response(tool_call_response)
            .with_response(final_response);

        let mock_tool = MockTool::new("greet").with_result(alva_types::ToolResult {
            content: "Hello, World!".into(),
            is_error: false,
            details: None,
        });

        let config = AgentHooks::new(Arc::new(default_convert_to_llm));
        let cancel = CancellationToken::new();
        let (event_tx, mut event_rx) = mpsc::unbounded_channel();

        let mut state = AgentState::new("test-session", "test".to_string(), ModelConfig::default());
        state.tools.push(Arc::new(mock_tool.clone()));
        state
            .messages
            .push(AgentMessage::Standard(make_user_message("Say hello")));

        let result =
            run_agent_loop(&mut state, &model, &config, &cancel, &event_tx).await;
        assert!(result.is_ok(), "agent loop should succeed");

        drop(event_tx);
        let mut events = Vec::new();
        while let Some(ev) = event_rx.recv().await {
            events.push(ev);
        }

        // Should have tool execution events.
        let has_tool_exec_start = events
            .iter()
            .any(|e| matches!(e, AgentEvent::ToolExecutionStart { .. }));
        let has_tool_exec_end = events
            .iter()
            .any(|e| matches!(e, AgentEvent::ToolExecutionEnd { .. }));
        assert!(has_tool_exec_start, "should have ToolExecutionStart event");
        assert!(has_tool_exec_end, "should have ToolExecutionEnd event");

        // Should have the final text response.
        let has_final = events.iter().any(|e| {
            matches!(e, AgentEvent::MessageEnd { message: AgentMessage::Standard(m) } if m.text_content() == "Greeting sent!")
        });
        assert!(has_final, "should have MessageEnd with final response");

        // Model should have been called twice.
        let calls = model.calls();
        assert_eq!(calls.len(), 2, "model should have been called twice (tool call + final)");

        // MockTool should have been called once.
        let tool_calls = mock_tool.calls();
        assert_eq!(tool_calls.len(), 1, "tool should have been called once");
        assert_eq!(
            tool_calls[0],
            serde_json::json!({"name": "World"}),
            "tool should have received the correct input"
        );

        // State: user + assistant(tool_call) + tool_result + assistant(final)
        assert_eq!(state.messages.len(), 4);
    }

    // -----------------------------------------------------------------------
    // Test: MiddlewareContext Extensions persist across hooks
    // -----------------------------------------------------------------------
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn test_middleware_extensions_persist_across_hooks() {
        use crate::middleware::{Middleware, MiddlewareContext, MiddlewareError};
        use async_trait::async_trait;

        #[derive(Debug)]
        struct Budget(u32);

        struct PersistenceMiddleware {
            observed: Arc<std::sync::Mutex<Option<u32>>>,
        }

        #[async_trait]
        impl Middleware for PersistenceMiddleware {
            async fn on_agent_start(
                &self,
                ctx: &mut MiddlewareContext,
            ) -> Result<(), MiddlewareError> {
                ctx.extensions.insert(Budget(999));
                Ok(())
            }
            async fn before_llm_call(
                &self,
                ctx: &mut MiddlewareContext,
                _msgs: &mut Vec<Message>,
            ) -> Result<(), MiddlewareError> {
                if let Some(b) = ctx.extensions.get::<Budget>() {
                    *self.observed.lock().unwrap() = Some(b.0);
                }
                Ok(())
            }
        }

        let observed = Arc::new(std::sync::Mutex::new(None));
        let mw = PersistenceMiddleware {
            observed: observed.clone(),
        };

        let mut config = AgentHooks::new(Arc::new(default_convert_to_llm));
        config.middleware.push(Arc::new(mw));

        let cancel = CancellationToken::new();
        let (event_tx, _event_rx) = mpsc::unbounded_channel();
        let mut state = AgentState::new("test-session", "test".to_string(), ModelConfig::default());
        state
            .messages
            .push(AgentMessage::Standard(Message::user("Hi")));

        let _ = run_agent_loop(&mut state, &MockModel, &config, &cancel, &event_tx).await;

        assert_eq!(
            *observed.lock().unwrap(),
            Some(999),
            "Extensions should persist from on_agent_start to before_llm_call"
        );
    }
}
