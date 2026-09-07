// INPUT:  crate::state::{AgentState, AgentConfig}, crate::event::{AgentEvent, AgentEventSender}, alva_kernel_abi::*
// OUTPUT: pub async fn run_agent(), pub trait AgentLoop, pub struct DefaultAgentLoop
// POS:    session-centric agent loop with bounded/unbounded event transport behind the
//         swappable AgentLoop driver interface (ADR-0007). Mid-run steering is NOT a kernel
//         concern — external callers inject messages by appending them to the session directly
//         (typically via an opt-in `PendingPlugin` middleware). Before each LLM call, tool
//         schemas are pre-baked against a `ToolSchemaContext` carrying the bus handle so
//         `Tool::parameters_schema_with` sees live runtime state.
use std::sync::Arc;

use crate::context_runtime::ContextRuntime;
use crate::event::{AgentEvent, AgentEventSender};
use crate::middleware::MiddlewareError;
use crate::session_events::emit_runtime_event;
use crate::state::{AgentConfig, AgentState};
use alva_kernel_abi::agent_session::{ComponentDescriptor, EmitterKind};
use alva_kernel_abi::tool::Tool;
use alva_kernel_abi::ConversationLog;
use alva_kernel_abi::{
    AgentError, AgentMessage, CancellationToken, ContentBlock, Message, MessageRole, ToolCall,
    UsageMetadata,
};
use sha2::{Digest, Sha256};

mod llm_call;
mod loop_impl;

use llm_call::{bake_tool_schemas, ActualLlmCall};
pub use loop_impl::{AgentLoop, DefaultAgentLoop};

/// Periodic session-flush cadence (AgentSession lifecycle contract point b):
/// flush every this-many loop iterations, or every [`FLUSH_INTERVAL`],
/// whichever comes first.
const FLUSH_EVERY_N_ITERATIONS: u32 = 10;
/// Wall-clock cap between periodic session flushes.
const FLUSH_INTERVAL: std::time::Duration = std::time::Duration::from_secs(30);

fn placeholder_assistant_message(message_id: &str) -> AgentMessage {
    AgentMessage::Standard(Message {
        id: message_id.to_string(),
        role: MessageRole::Assistant,
        content: vec![],
        tool_call_id: None,
        usage: None,
        timestamp: chrono::Utc::now().timestamp_millis(),
    })
}

/// Fold per-turn harness additions into the stable system-prompt bucket.
///
/// Convention (see `AgentConfig::system_prompt`): every entry except the last
/// is stable/cacheable; the last is dynamic. When there is only one segment
/// we treat it as the stable bucket and append in place so the additions sit
/// with the long-lived instructions rather than the volatile tail.
fn apply_turn_system_additions(system_prompt: &mut Vec<String>, additions: &[String]) {
    let joined = additions
        .iter()
        .map(String::as_str)
        .filter(|s| !s.is_empty())
        .collect::<Vec<_>>()
        .join("\n\n");
    if joined.is_empty() {
        return;
    }
    if system_prompt.is_empty() {
        system_prompt.push(joined);
        return;
    }
    if system_prompt.len() == 1 {
        system_prompt[0].push_str("\n\n");
        system_prompt[0].push_str(&joined);
        return;
    }
    // 2+ segments: last is dynamic, second-to-last is the stable bucket.
    let idx = system_prompt.len() - 2;
    system_prompt[idx].push_str("\n\n");
    system_prompt[idx].push_str(&joined);
}

fn update_framed_hash(hasher: &mut Sha256, value: &[u8]) {
    hasher.update((value.len() as u64).to_be_bytes());
    hasher.update(value);
}

fn sha256_hex(value: &[u8]) -> String {
    format!("{:x}", Sha256::digest(value))
}

/// Fingerprint the provider-cache base without folding in the volatile system
/// tail or conversation history. Consumers can compare consecutive
/// `llm_call_start` events to distinguish a stable-prefix change from an
/// ordinary provider cache miss.
fn cache_base_fingerprints(
    provider: &str,
    model: &str,
    system_prompt: &[String],
    tools: &[Arc<dyn Tool>],
) -> Option<(String, String, usize)> {
    let stable_system_segments = match system_prompt.len() {
        0 => 0,
        1 => 1,
        count => count - 1,
    };

    let mut tools_hasher = Sha256::new();
    update_framed_hash(&mut tools_hasher, b"alva-tool-schema-fingerprint-v1");
    for tool in tools {
        let definition = serde_json::to_vec(&alva_kernel_abi::tool_definition(
            tool.as_ref(),
            &alva_kernel_abi::tool::schema::ToolSchemaContext::empty(),
        ))
        .map_err(|error| {
            tracing::warn!(tool = tool.name(), %error, "failed to fingerprint tool schema");
            error
        })
        .ok()?;
        update_framed_hash(&mut tools_hasher, &definition);
    }
    let tool_schema_fingerprint = format!("{:x}", tools_hasher.finalize());

    let mut base_hasher = Sha256::new();
    update_framed_hash(&mut base_hasher, b"alva-cache-base-fingerprint-v1");
    update_framed_hash(&mut base_hasher, provider.as_bytes());
    update_framed_hash(&mut base_hasher, model.as_bytes());
    for segment in system_prompt.iter().take(stable_system_segments) {
        update_framed_hash(&mut base_hasher, segment.as_bytes());
    }
    update_framed_hash(&mut base_hasher, tool_schema_fingerprint.as_bytes());
    let cache_base_fingerprint = format!("{:x}", base_hasher.finalize());

    Some((
        cache_base_fingerprint,
        tool_schema_fingerprint,
        stable_system_segments,
    ))
}

fn cache_outcome(usage: Option<&UsageMetadata>) -> &'static str {
    let Some(usage) = usage else {
        return "unreported";
    };
    match (
        usage.cache_creation_input_tokens,
        usage.cache_read_input_tokens,
    ) {
        (Some(write), Some(read)) if write > 0 && read > 0 => "hit_and_write",
        (_, Some(read)) if read > 0 => "hit",
        (Some(write), _) if write > 0 => "write",
        (Some(_), _) | (_, Some(_)) => "miss",
        (None, None) => "unreported",
    }
}

/// agent loop — session-centric with middleware hooks.
///
/// Messages are never stored in local variables across iterations;
/// instead, every iteration reads the full history from `state.session`.
pub async fn run_agent<E>(
    state: &AgentState,
    config: &AgentConfig,
    cancel: CancellationToken,
    input: Vec<AgentMessage>,
    event_tx: E,
) -> Result<(), AgentError>
where
    E: Into<AgentEventSender>,
{
    let event_tx = event_tx.into();
    state.extensions.insert(cancel.clone());

    // 内核循环只依赖对话记录（ConversationLog）：这里把 harness 递进来的
    // 完整会话收窄成日志句柄，循环内所有读写都走这个更小的表面——查询、
    // 回滚、快照、订阅、压缩在类型层面就进不了内核。
    let log: std::sync::Arc<dyn ConversationLog> = state.session.clone();

    let agent_id = log.session_id().to_string();
    let mut context_runtime = ContextRuntime::new(agent_id.clone());

    // Emit AgentStart + run_start up front, BEFORE any fallible startup step.
    // This makes the terminal cleanup below unconditional: every run that
    // reaches this point gets exactly one paired terminal (AgentEnd +
    // run_end + on_agent_end + dispose), even if `on_agent_start` or
    // `input_committed` fails. Previously those paths `?`-returned and the
    // consumer just saw the event stream close with no AgentEnd.
    let _ = event_tx.send(AgentEvent::AgentStart).await;

    // --- Session skeleton: run_start ---
    let run_start_uuid = emit_runtime_event(
        &log,
        "run_start",
        None,
        Some(serde_json::json!({
            "agent_id": agent_id.clone(),
            "max_iterations": config.max_iterations,
        })),
    )
    .await?;

    // Fallible body: on the first error we stop running it but still fall
    // through to the finalizer. The `async` block scopes the borrows of
    // `state` / `context_runtime` so they are free again for cleanup.
    let mut result: Result<(), AgentError> = async {
        // 1. Lifecycle: on_agent_start
        config
            .middleware
            .run_on_agent_start(state)
            .await
            .map_err(MiddlewareError::into_agent_error)?;
        context_runtime.bootstrap(config).await;

        // --- Session skeleton: component_registry ---
        // Collect descriptors for every tool and middleware in this run.
        let mut components: Vec<ComponentDescriptor> = Vec::new();
        for tool in &state.tools {
            components.push(ComponentDescriptor {
                kind: EmitterKind::Tool,
                id: tool.name().to_string(),
                name: tool.name().to_string(),
            });
        }
        for mw_name in config.middleware.names() {
            components.push(ComponentDescriptor {
                kind: EmitterKind::Middleware,
                id: mw_name.clone(),
                name: mw_name,
            });
        }
        emit_runtime_event(
            &log,
            "component_registry",
            Some(run_start_uuid.clone()),
            Some(serde_json::json!({ "components": components })),
        )
        .await?;

        // 2. Store input messages in session + collect context injections.
        for msg in input {
            log.append_message(msg.clone(), None)
                .await
                .map_err(AgentError::from)?;
            config
                .middleware
                .run_input_committed(state, &msg)
                .await
                .map_err(MiddlewareError::into_agent_error)?;
            context_runtime.on_message(config, &msg).await;
        }

        // 3. Main loop
        run_loop(
            state,
            &log,
            config,
            &cancel,
            &event_tx,
            &run_start_uuid,
            &mut context_runtime,
        )
        .await
    }
    .await;

    let run_error: Option<String> = result.as_ref().err().map(|e| e.to_string());

    // 4. Lifecycle: on_agent_end
    if let Err(e) = config
        .middleware
        .run_on_agent_end(state, run_error.as_deref())
        .await
    {
        tracing::warn!(error = %e, "on_agent_end middleware failed");
    }

    context_runtime.dispose(config).await;

    // --- Session skeleton: run_end ---
    emit_runtime_event(
        &log,
        "run_end",
        Some(run_start_uuid.clone()),
        Some(serde_json::json!({
            "error": run_error,
        })),
    )
    .await
    .map_err(|e| tracing::warn!(error = %e, "run_end skeleton event failed"));

    // Durability flush at agent end (lifecycle contract point a): everything
    // appended this run — including run_end above — is persisted before the
    // caller observes completion. Preserve an earlier execution error, but a
    // flush failure must turn an otherwise-successful run into a failure.
    if let Err(e) = log.flush().await {
        if result.is_ok() {
            result = Err(AgentError::Other(format!(
                "session durability flush failed: {e}"
            )));
        } else {
            tracing::warn!(error = %e, "session flush also failed after run error");
        }
    }

    // 5. Emit AgentEnd
    let error = result.as_ref().err().map(|e| e.to_string());
    let _ = event_tx
        .send(AgentEvent::AgentEnd {
            error: error.clone(),
        })
        .await;

    result
}

/// Inner loop extracted so we can capture the error cleanly for lifecycle hooks.
///
/// Double-loop structure:
/// - **Outer loop**: processes follow-up messages after the inner loop finishes naturally.
/// - **Inner loop**: LLM calls + tool execution + steering injection.
async fn run_loop(
    state: &AgentState,
    log: &std::sync::Arc<dyn ConversationLog>,
    config: &AgentConfig,
    cancel: &CancellationToken,
    event_tx: &AgentEventSender,
    run_start_uuid: &str,
    context_runtime: &mut ContextRuntime,
) -> Result<(), AgentError> {
    let mut total_iterations: u32 = 0;
    let mut _session_input_tokens: u64 = 0;
    let mut _session_output_tokens: u64 = 0;
    // Durability cadence promised by the AgentSession lifecycle contract:
    // flush every N iterations or every INTERVAL, whichever comes first.
    let mut last_flush = web_time::Instant::now();

    // The run loop: LLM call → tool execution, iterating until the model
    // stops requesting tools. Follow-up / steering moved to PendingPlugin,
    // so there is no longer an outer conversation loop — callers re-invoke
    // run_agent for the next user turn.
    loop {
        if cancel.is_cancelled() {
            return Err(AgentError::Cancelled);
        }
        if total_iterations >= config.max_iterations {
            tracing::warn!(
                max_iterations = config.max_iterations,
                "agent loop exhausted max_iterations without finishing"
            );
            return Err(AgentError::MaxIterations(config.max_iterations));
        }
        total_iterations += 1;

        // Periodic durability flush (lifecycle contract point b): bound how
        // long a permanent backend write failure can stay hidden. Checked at
        // the iteration boundary so it never interrupts a turn mid-flight.
        if total_iterations.is_multiple_of(FLUSH_EVERY_N_ITERATIONS)
            || last_flush.elapsed() >= FLUSH_INTERVAL
        {
            log.flush().await.map_err(|e| {
                AgentError::Other(format!("periodic session durability flush failed: {e}"))
            })?;
            last_flush = web_time::Instant::now();
        }

        // Session skeleton: iteration boundary
        let iteration_start_uuid = emit_runtime_event(
            &log,
            "iteration_start",
            Some(run_start_uuid.to_string()),
            Some(serde_json::json!({ "iteration": total_iterations })),
        )
        .await?;

        // Emit TurnStart
        let _ = event_tx.send(AgentEvent::TurnStart).await;
        let turn_start = web_time::Instant::now();

        // 3a. Get messages from session (optionally windowed)
        let session_messages = if config.context_window > 0 {
            state.session.recent_messages(config.context_window).await?
        } else {
            state.session.messages().await?
        };

        let mut system_prompt_buf: Vec<String> = config.system_prompt.clone();
        // Per-turn harness additions (UI-selected skill bodies, etc.) go into
        // the stable bucket so they sit with cacheable instructions rather
        // than the volatile RuntimeInject tail. Applied on a clone only —
        // AgentConfig::system_prompt itself is not mutated.
        apply_turn_system_additions(
            &mut system_prompt_buf,
            &config.model_config.turn_system_additions,
        );
        let working_messages = context_runtime
            .prepare_llm_context(config, &mut system_prompt_buf, session_messages)
            .await;

        // 3b. Build LLM messages: [system_prompt segments] + working_messages
        //
        //     We push ONE Message::system per segment. Adapters decide
        //     how to render — Anthropic emits a `system: [{...}]`
        //     block array with `cache_control: ephemeral` on every
        //     segment except the last (cacheable); OpenAI / Gemini
        //     concat into one string (auto-prefix-cached).
        let mut llm_messages = Vec::new();
        let total_segments = system_prompt_buf.len();
        let total_chars: usize = system_prompt_buf.iter().map(|s| s.len()).sum();
        if total_chars > 0 {
            tracing::debug!(
                segments = total_segments,
                total_chars,
                "llm_call: system_prompt"
            );
        }
        for segment in &system_prompt_buf {
            if !segment.is_empty() {
                llm_messages.push(Message::system(segment));
            }
        }
        for msg in &working_messages {
            // Only Standard messages are sent to the LLM.
            // Steering and FollowUp are normalized to Standard before
            // entering the session (see delegate injection below).
            if let AgentMessage::Standard(m) = msg {
                llm_messages.push(m.clone());
            }
        }

        // 3c. Middleware: before_llm_call
        config
            .middleware
            .run_before_llm_call(state, &mut llm_messages)
            .await
            .map_err(MiddlewareError::into_agent_error)?;

        // 3d. Emit MessageStart before the LLM call
        let message_id = uuid::Uuid::new_v4().to_string();
        let placeholder_msg = placeholder_assistant_message(&message_id);
        let _ = event_tx
            .send(AgentEvent::MessageStart {
                message: placeholder_msg.clone(),
            })
            .await;

        // 3e. Call LLM through wrap_llm_call middleware chain.
        //
        // Pre-bake each tool's JSON Schema against a `ToolSchemaContext`
        // that carries the config bus. Tools that override
        // `Tool::parameters_schema_with` (e.g. `AgentSpawnTool`
        // pulling registered `SpawnCommunication` kinds off the bus)
        // observe the live runtime state here; tools that don't just
        // see their usual static schema cached once per turn.
        //
        // `model_config.disable_tools = true` short-circuits to an
        // empty list — used when the user (or runtime probe) has
        // marked the active model as not supporting function
        // calling. The provider then omits the `tools` field from
        // the request entirely (no empty array; AMP / pi-mono
        // behavior).
        let baked_tools = if config.model_config.disable_tools {
            tracing::debug!("model_config.disable_tools=true; skipping all tool injection");
            Vec::new()
        } else if let Some(allowed) = &config.model_config.allowed_tools {
            // Per-turn manual allow-list: present only the selected tools. The
            // model never sees the others, so it cannot call them — the UI's
            // "only these tools this turn" promise is enforced here rather than
            // being a no-op log line.
            let selected: Vec<Arc<dyn Tool>> = state
                .tools
                .iter()
                .filter(|t| allowed.iter().any(|name| name == t.name()))
                .cloned()
                .collect();
            bake_tool_schemas(&selected, config.bus.as_ref())
        } else {
            bake_tool_schemas(&state.tools, config.bus.as_ref())
        };

        // Record the final provider-bound request only after all preparation
        // middleware has run. This keeps replay/audit faithful when middleware
        // compacts, injects, or redacts messages.
        let segment_hashes: Vec<String> = system_prompt_buf
            .iter()
            .map(|segment| {
                sha256_hex(segment.as_bytes())
                    .chars()
                    .take(16)
                    .collect::<String>()
            })
            .collect();
        let cache_fingerprints = cache_base_fingerprints(
            state.model.provider_id(),
            state.model.model_id(),
            &system_prompt_buf,
            &baked_tools,
        );
        let cache_base_fingerprint = cache_fingerprints
            .as_ref()
            .map(|(fingerprint, _, _)| fingerprint);
        let tool_schema_fingerprint = cache_fingerprints
            .as_ref()
            .map(|(_, fingerprint, _)| fingerprint);
        let stable_system_prompt_segments = cache_fingerprints
            .as_ref()
            .map(|(_, _, count)| *count)
            .unwrap_or(0);
        let provider_options_applied = config
            .model_config
            .extra_body
            .as_ref()
            .is_some_and(|options| !options.is_empty());
        let llm_start_uuid = emit_runtime_event(
            &log,
            "llm_call_start",
            Some(iteration_start_uuid.clone()),
            Some(serde_json::json!({
                "iteration": total_iterations,
                "provider": state.model.provider_id(),
                "model": state.model.model_id(),
                "message_count": llm_messages.len(),
                "messages": llm_messages,
                "system_prompt_segments": system_prompt_buf.len(),
                "stable_system_prompt_segments": stable_system_prompt_segments,
                "system_prompt_segment_hashes": segment_hashes,
                "cache_base_fingerprint_version": 1,
                "cache_base_fingerprint": cache_base_fingerprint,
                "tool_schema_fingerprint": tool_schema_fingerprint,
                "disable_tools": config.model_config.disable_tools,
                "tools_count_sent": baked_tools.len(),
                "provider_options_applied": provider_options_applied,
            })),
        )
        .await?;
        let llm_call_started_at = web_time::Instant::now();

        let actual_call = ActualLlmCall {
            model: state.model.clone(),
            tools: baked_tools,
            model_config: config.model_config.clone(),
            event_tx: event_tx.clone(),
            message_id,
            session: log.clone(),
            llm_call_id: llm_start_uuid.clone(),
            cancel: cancel.clone(),
        };
        let mut response = match config
            .middleware
            .run_wrap_llm_call(state, llm_messages, Arc::new(actual_call))
            .await
        {
            Ok(response) => response,
            Err(error) => {
                let agent_error = error.into_agent_error();
                let _ = event_tx
                    .send(AgentEvent::MessageError {
                        message: placeholder_msg.clone(),
                        error: agent_error.to_string(),
                    })
                    .await;
                return Err(agent_error);
            }
        };

        // 3f. Middleware: after_llm_call
        if let Err(error) = config
            .middleware
            .run_after_llm_call(state, &mut response)
            .await
        {
            let agent_error = error.into_agent_error();
            let _ = event_tx
                .send(AgentEvent::MessageError {
                    message: AgentMessage::Standard(response.clone()),
                    error: agent_error.to_string(),
                })
                .await;
            return Err(agent_error);
        }

        // Session skeleton: llm_call_end.
        // Carries token usage for this turn — basic in/out counts plus
        // cache stats for providers that report them (e.g. Anthropic).
        // Absent fields (e.g. OpenAI cache) serialize as null, not 0,
        // so consumers can distinguish "unknown" from "zero".
        let cache_metrics_reported = response.usage.as_ref().is_some_and(|usage| {
            usage.cache_creation_input_tokens.is_some() || usage.cache_read_input_tokens.is_some()
        });
        emit_runtime_event(
            &log,
            "llm_call_end",
            Some(llm_start_uuid.clone()),
            Some(serde_json::json!({
                "input_tokens": response.usage.as_ref().map(|u| u.input_tokens).unwrap_or(0),
                "output_tokens": response.usage.as_ref().map(|u| u.output_tokens).unwrap_or(0),
                "cache_creation_input_tokens": response.usage.as_ref().and_then(|u| u.cache_creation_input_tokens),
                "cache_read_input_tokens": response.usage.as_ref().and_then(|u| u.cache_read_input_tokens),
                "cache_metrics_reported": cache_metrics_reported,
                "cache_outcome": cache_outcome(response.usage.as_ref()),
            })),
        )
        .await?;

        // Analytics: emit a structured LlmCall event for telemetry sinks.
        // No-op if no AnalyticsSink is registered on the bus.
        if let Some(sink) = config
            .bus
            .as_ref()
            .and_then(|b| b.get::<dyn alva_kernel_abi::AnalyticsSink>())
        {
            let usage = response.usage.as_ref();
            sink.record(alva_kernel_abi::AnalyticsEvent::LlmCall {
                session_id: state.session.session_id().to_string(),
                provider: state.model.provider_id().to_string(),
                model: state.model.model_id().to_string(),
                input_tokens: usage.map(|u| u.input_tokens).unwrap_or(0),
                output_tokens: usage.map(|u| u.output_tokens).unwrap_or(0),
                cache_read: usage.and_then(|u| u.cache_read_input_tokens).unwrap_or(0),
                cache_write: usage
                    .and_then(|u| u.cache_creation_input_tokens)
                    .unwrap_or(0),
                cost_usd: 0.0,
                latency_ms: llm_call_started_at.elapsed().as_millis() as u64,
                // The analytics ABI field is std::time::SystemTime, whose
                // `now()` panics on wasm32. Take "now" from web_time and
                // rebase it onto the std type — SystemTime ARITHMETIC is
                // wasm-safe, only ::now() is not. On native web_time is a
                // re-export of std, so this is byte-identical there.
                ts: std::time::SystemTime::UNIX_EPOCH
                    + web_time::SystemTime::now()
                        .duration_since(web_time::SystemTime::UNIX_EPOCH)
                        .unwrap_or_default(),
            });
        }

        // 3g. Store response in session + fire ContextHooks::on_message
        let response_msg = AgentMessage::Standard(response.clone());
        log.append_message(response_msg.clone(), Some(llm_start_uuid.clone()))
            .await?;
        context_runtime.on_message(config, &response_msg).await;

        // 3h. Track token usage from this turn
        if let Some(ref usage) = response.usage {
            _session_input_tokens += usage.input_tokens as u64;
            _session_output_tokens += usage.output_tokens as u64;
        }

        // 3i. Emit MessageEnd with the complete response
        // (MessageStart was emitted before the LLM call; MessageUpdate
        // events were emitted during streaming inside ActualLlmCall.)
        let agent_msg = AgentMessage::Standard(response.clone());
        let _ = event_tx
            .send(AgentEvent::MessageEnd { message: agent_msg })
            .await;

        // 3i. Extract tool_calls from response
        let tool_calls: Vec<ToolCall> = response
            .content
            .iter()
            .filter_map(|block| {
                if let ContentBlock::ToolUse { id, name, input } = block {
                    Some(ToolCall {
                        id: id.clone(),
                        name: name.clone(),
                        arguments: input.clone(),
                    })
                } else {
                    None
                }
            })
            .collect();

        // 3j. If no tool_calls, emit TurnEnd and break inner (natural finish — check follow-ups)
        if tool_calls.is_empty() {
            tracing::info!(
                turn = total_iterations,
                duration_ms = turn_start.elapsed().as_millis() as u64,
                tool_calls = 0,
                "turn completed (no tool calls)"
            );
            let _ = event_tx.send(AgentEvent::TurnEnd).await;
            context_runtime.after_turn(config).await;
            emit_runtime_event(
                &log,
                "iteration_end",
                Some(iteration_start_uuid.clone()),
                None,
            )
            .await?;
            break;
        }

        let committed_tools = crate::tool_batch::ToolBatchCoordinator::new()
            .execute_batch(
                state,
                &log,
                config,
                cancel.clone(),
                &tool_calls,
                llm_start_uuid.clone(),
                event_tx.clone(),
            )
            .await?;
        for committed in committed_tools {
            context_runtime.on_message(config, &committed.message).await;
        }

        // 3l. Emit TurnEnd + ContextHooks::after_turn
        tracing::info!(
            turn = total_iterations,
            duration_ms = turn_start.elapsed().as_millis() as u64,
            tool_calls = tool_calls.len(),
            "turn completed (with tool calls)"
        );
        let _ = event_tx.send(AgentEvent::TurnEnd).await;
        context_runtime.after_turn(config).await;

        // Session skeleton: end of iteration (tool-calls path).
        // Any mid-run user interjection lands via an extension that
        // runs at `before_llm_call`, not here.
        emit_runtime_event(
            &log,
            "iteration_end",
            Some(iteration_start_uuid.clone()),
            None,
        )
        .await?;
    }

    // The loop ended with no more tool calls → agent run is done. Callers
    // that want to "continue the conversation" invoke `run_agent` again
    // with the next user input.

    Ok(())
}

#[cfg(test)]
mod run_helpers_tests {
    use std::sync::Arc;

    use alva_kernel_abi::tool::execution::{ToolExecutionContext, ToolOutput};
    use alva_kernel_abi::{AgentError, Tool, UsageMetadata};
    use async_trait::async_trait;

    use super::{apply_turn_system_additions, cache_base_fingerprints, cache_outcome};

    struct FingerprintTool;

    #[async_trait]
    impl Tool for FingerprintTool {
        fn name(&self) -> &str {
            "echo"
        }

        fn description(&self) -> &str {
            "Echo a value"
        }

        fn parameters_schema(
            &self,
            _ctx: &alva_kernel_abi::tool::schema::ToolSchemaContext,
        ) -> serde_json::Value {
            serde_json::json!({
                "type": "object",
                "properties": { "value": { "type": "string" } },
                "required": ["value"]
            })
        }

        async fn execute(
            &self,
            _input: serde_json::Value,
            _ctx: &dyn ToolExecutionContext,
        ) -> Result<ToolOutput, AgentError> {
            Ok(ToolOutput::text("ok"))
        }
    }

    #[test]
    fn empty_additions_leave_prompt_untouched() {
        let mut prompt = vec!["base".into()];
        apply_turn_system_additions(&mut prompt, &[]);
        assert_eq!(prompt, vec!["base".to_string()]);
    }

    #[test]
    fn single_segment_appends_in_place() {
        let mut prompt = vec!["base".into()];
        apply_turn_system_additions(&mut prompt, &["skill-a".into(), "skill-b".into()]);
        assert_eq!(prompt, vec!["base\n\nskill-a\n\nskill-b".to_string()]);
    }

    #[test]
    fn multi_segment_appends_to_stable_bucket_not_dynamic_tail() {
        let mut prompt = vec!["stable".into(), "dynamic".into()];
        apply_turn_system_additions(&mut prompt, &["skill".into()]);
        assert_eq!(
            prompt,
            vec!["stable\n\nskill".to_string(), "dynamic".to_string()]
        );
    }

    #[test]
    fn empty_prompt_receives_additions_as_first_segment() {
        let mut prompt = Vec::new();
        apply_turn_system_additions(&mut prompt, &["skill".into()]);
        assert_eq!(prompt, vec!["skill".to_string()]);
    }

    #[test]
    fn cache_base_ignores_dynamic_system_tail() {
        let tools: Vec<Arc<dyn Tool>> = vec![Arc::new(FingerprintTool)];
        let first = cache_base_fingerprints(
            "anthropic",
            "claude-test",
            &["stable".into(), "dynamic-a".into()],
            &tools,
        )
        .unwrap();
        let second = cache_base_fingerprints(
            "anthropic",
            "claude-test",
            &["stable".into(), "dynamic-b".into()],
            &tools,
        )
        .unwrap();

        assert_eq!(first, second);
        assert_eq!(first.2, 1);
    }

    #[test]
    fn cache_base_changes_with_stable_system_or_model() {
        let tools: Vec<Arc<dyn Tool>> = vec![Arc::new(FingerprintTool)];
        let baseline = cache_base_fingerprints(
            "anthropic",
            "claude-test",
            &["stable-a".into(), "dynamic".into()],
            &tools,
        )
        .unwrap();
        let changed_system = cache_base_fingerprints(
            "anthropic",
            "claude-test",
            &["stable-b".into(), "dynamic".into()],
            &tools,
        )
        .unwrap();
        let changed_model = cache_base_fingerprints(
            "anthropic",
            "claude-other",
            &["stable-a".into(), "dynamic".into()],
            &tools,
        )
        .unwrap();

        assert_ne!(baseline.0, changed_system.0);
        assert_ne!(baseline.0, changed_model.0);
    }

    #[test]
    fn cache_outcome_distinguishes_provider_observations() {
        let usage = |write, read| UsageMetadata {
            cache_creation_input_tokens: write,
            cache_read_input_tokens: read,
            ..Default::default()
        };

        assert_eq!(cache_outcome(None), "unreported");
        assert_eq!(cache_outcome(Some(&usage(None, None))), "unreported");
        assert_eq!(cache_outcome(Some(&usage(Some(0), Some(0)))), "miss");
        assert_eq!(cache_outcome(Some(&usage(Some(3), Some(0)))), "write");
        assert_eq!(cache_outcome(Some(&usage(Some(0), Some(5)))), "hit");
        assert_eq!(
            cache_outcome(Some(&usage(Some(3), Some(5)))),
            "hit_and_write"
        );
    }
}
