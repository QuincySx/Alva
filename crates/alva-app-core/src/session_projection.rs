// INPUT:  alva_kernel_abi (ToolDefinition, ToolCall, ToolOutput, Message),
//         alva_kernel_abi::agent_session::SessionEvent
// OUTPUT: RunRecord, ConfigSnapshot, TurnRecord, LlmCallRecord, ToolCallRecord,
//         HookRecord, build_run_record
// POS:    Pure-function projection layer.  Takes a slice of SessionEvents
//         (the eval session's complete event log) and produces a RunRecord
//         suitable for JSON serialisation and the eval frontend.
//
//         Type definitions are kept field-name-identical to the originals in
//         recorder.rs so the frontend JS continues to work without changes.

use alva_kernel_abi::agent_session::SessionEvent;
use alva_kernel_abi::{Message, ToolCall, ToolDefinition, ToolOutput};
use serde::{Deserialize, Serialize};

// ===========================================================================
// Public types — field names must match the originals in recorder.rs exactly.
// ===========================================================================

/// Top-level record for a complete agent run.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RunRecord {
    pub config_snapshot: ConfigSnapshot,
    pub turns: Vec<TurnRecord>,
    pub total_duration_ms: u64,
    pub total_input_tokens: u64,
    pub total_output_tokens: u64,
    /// User-submitted messages, in event order. Each entry's
    /// `before_turn_number` is the turn_number of the first turn it kicked
    /// off (1-indexed); the inspector inserts the message block before
    /// that turn in the timeline. Messages that arrive after the last
    /// `iteration_start` get a `before_turn_number` one past the last
    /// turn so they render as a trailing pending prompt.
    #[serde(default)]
    pub user_messages: Vec<UserMessageRecord>,
}

/// One user prompt that was submitted into the run.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UserMessageRecord {
    pub before_turn_number: u32,
    pub text: String,
    pub timestamp_ms: u64,
}

/// Snapshot of the agent configuration at the start of the run.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConfigSnapshot {
    /// System prompt segments captured at run start. The kernel renders
    /// every segment except the last with `cache_control: ephemeral`
    /// (Anthropic). Inspector displays each segment so users can see
    /// the cache-boundary split.
    pub system_prompt: Vec<String>,
    pub model_id: String,
    pub tool_names: Vec<String>,
    /// Full tool definitions sent to the LLM (name + description + parameters schema).
    pub tool_definitions: Vec<ToolDefinition>,
    pub skill_names: Vec<String>,
    pub max_iterations: u32,
    #[serde(default)]
    pub extension_names: Vec<String>,
    #[serde(default)]
    pub middleware_names: Vec<String>,
}


/// Record for a single agent turn (one LLM call + zero or more tool calls).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TurnRecord {
    pub turn_number: u32,
    pub llm_call: LlmCallRecord,
    pub tool_calls: Vec<ToolCallRecord>,
    pub duration_ms: u64,
}

/// Details of a single LLM inference call.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LlmCallRecord {
    pub messages_sent: Vec<Message>,
    pub messages_sent_count: usize,
    pub response: Option<Message>,
    pub input_tokens: u32,
    pub output_tokens: u32,
    pub duration_ms: u64,
    /// One of `"end_turn"`, `"tool_use"`, `"max_tokens"`, or `"error"`.
    pub stop_reason: String,
    /// Error message if the agent ended with an error.
    pub error_message: Option<String>,
    pub middleware_hooks: Vec<HookRecord>,

    // ───────── Prompt-cache observability ─────────
    /// Anthropic `cache_creation_input_tokens` — fresh tokens written
    /// to the cache on this call (you pay for these). Surfaced from
    /// `response.usage`. `None` if the provider doesn't report it.
    #[serde(default)]
    pub cache_creation_input_tokens: Option<u32>,
    /// Anthropic `cache_read_input_tokens` — cached tokens reused
    /// (you DON'T pay full price for these — Anthropic discounts ~90%).
    /// Higher = better cache hit rate.
    #[serde(default)]
    pub cache_read_input_tokens: Option<u32>,

    // ───────── Per-turn config knobs (P2 marker fields) ─────────
    /// `true` when this call ran with `disable_tools` set — the
    /// request body had no `tools` field even though tools were
    /// registered.
    #[serde(default)]
    pub disable_tools: bool,
    /// Number of system-prompt segments sent on this call. > 1 means
    /// the cache boundary is in effect (stable + dynamic split).
    #[serde(default)]
    pub system_prompt_segments: u32,
    /// Number of tools actually sent (may be 0 if `disable_tools`
    /// active or no tools registered).
    #[serde(default)]
    pub tools_count_sent: u32,
    /// `true` when `ModelConfig.extra_body` was populated for this
    /// call (vendor-specific JSON merged into the body).
    #[serde(default)]
    pub provider_options_applied: bool,
}

/// Details of a single tool invocation within a turn.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolCallRecord {
    pub tool_call: ToolCall,
    pub result: Option<ToolOutput>,
    pub is_error: bool,
    pub duration_ms: u64,
    pub middleware_hooks: Vec<HookRecord>,
    /// Nested `RunRecord` for tools that spawn a full child agent run.
    ///
    // TODO: sub-run projection when sub-agent session linkage is wired (Phase 3+)
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sub_run: Option<Box<RunRecord>>,
}

/// Timing and outcome of a single middleware hook invocation.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HookRecord {
    pub middleware_name: String,
    pub hook: String,
    pub duration_ms: u64,
    pub outcome: String,
}

// ===========================================================================
// Helper: default RunRecord for crashes / empty runs
// ===========================================================================

fn empty_record() -> RunRecord {
    RunRecord {
        config_snapshot: ConfigSnapshot {
            system_prompt: Vec::new(),
            model_id: String::new(),
            tool_names: Vec::new(),
            tool_definitions: Vec::new(),
            skill_names: Vec::new(),
            max_iterations: 0,
            extension_names: Vec::new(),
            middleware_names: Vec::new(),
        },
        turns: Vec::new(),
        total_duration_ms: 0,
        total_input_tokens: 0,
        total_output_tokens: 0,
        user_messages: Vec::new(),
    }
}

// ===========================================================================
// Sub-agent range helpers
// ===========================================================================

/// Locate the slice of events that belongs to a specific sub-agent run,
/// delimited by `subagent_run_start` / `subagent_run_end` markers both
/// tagged with `tool_call_id`.
///
/// Returns the events strictly *between* the two markers (the markers
/// themselves are bookkeeping noise for the projection layer, not part of
/// the child run's own event log).
fn find_subagent_range(events: &[SessionEvent], tool_call_id: &str) -> Option<Vec<SessionEvent>> {
    let start_idx = events.iter().position(|e| {
        e.event_type == "subagent_run_start"
            && e.data
                .as_ref()
                .and_then(|d| d.get("tool_call_id"))
                .and_then(|v| v.as_str())
                == Some(tool_call_id)
    })?;

    let end_rel = events[start_idx + 1..].iter().position(|e| {
        e.event_type == "subagent_run_end"
            && e.data
                .as_ref()
                .and_then(|d| d.get("tool_call_id"))
                .and_then(|v| v.as_str())
                == Some(tool_call_id)
    })?;
    let end_idx = start_idx + 1 + end_rel;

    Some(events[start_idx + 1..end_idx].to_vec())
}

// ===========================================================================
// Main projection entry point
// ===========================================================================

/// Build a `RunRecord` from a complete session event log.
///
/// Tolerant of partial runs: if events are missing (e.g. a run that crashed
/// mid-way) the function returns whatever it could assemble with sensible
/// defaults for missing fields.
pub fn build_run_record(events: &[SessionEvent]) -> RunRecord {
    if events.is_empty() {
        return empty_record();
    }

    // -------------------------------------------------------------------
    // 1. Extract run_start event (first event with type "run_start")
    // -------------------------------------------------------------------
    let run_start = events
        .iter()
        .find(|e| e.event_type == "run_start");

    let max_iterations: u32 = run_start
        .and_then(|e| e.data.as_ref())
        .and_then(|d| d.get("max_iterations"))
        .and_then(|v| v.as_u64())
        .unwrap_or(0) as u32;

    let run_start_ts: i64 = run_start
        .map(|e| e.timestamp)
        .unwrap_or(0);

    // -------------------------------------------------------------------
    // 2. Extract eval_config_snapshot event
    //    Written by eval's create_run just after building the agent.
    //    It's a "system" event with data.type == "eval_config_snapshot".
    // -------------------------------------------------------------------
    let cfg_event = events.iter().find(|e| {
        e.event_type == "system"
            && e.data
                .as_ref()
                .and_then(|d| d.get("type"))
                .and_then(|t| t.as_str())
                == Some("eval_config_snapshot")
    });

    let config_snapshot = build_config_snapshot(cfg_event, max_iterations);

    // -------------------------------------------------------------------
    // 3. Walk events to group by iteration
    // -------------------------------------------------------------------
    let mut turns = build_turns(events);

    // -------------------------------------------------------------------
    // 3b. Attach sub_run records to tool calls that spawned sub-agents.
    //     We search the FULL event list for subagent_run_start/end markers
    //     (they live in the parent's stream, not within a single iteration).
    // -------------------------------------------------------------------
    for turn in &mut turns {
        for tool_call in &mut turn.tool_calls {
            let id = &tool_call.tool_call.id;
            if !id.is_empty() {
                if let Some(child_events) = find_subagent_range(events, id) {
                    let child_record = build_run_record(&child_events);
                    tool_call.sub_run = Some(Box::new(child_record));
                }
            }
        }
    }

    // -------------------------------------------------------------------
    // 4. Aggregate token counts from llm_call_end events
    // -------------------------------------------------------------------
    let mut total_input_tokens: u64 = 0;
    let mut total_output_tokens: u64 = 0;
    for event in events {
        if event.event_type == "llm_call_end" {
            if let Some(d) = &event.data {
                total_input_tokens +=
                    d.get("input_tokens").and_then(|v| v.as_u64()).unwrap_or(0);
                total_output_tokens +=
                    d.get("output_tokens").and_then(|v| v.as_u64()).unwrap_or(0);
            }
        }
    }

    // -------------------------------------------------------------------
    // 5. Compute total_duration_ms from run_start / run_end timestamps
    // -------------------------------------------------------------------
    let run_end_ts: i64 = events
        .iter()
        .rev()
        .find(|e| e.event_type == "run_end")
        .map(|e| e.timestamp)
        .unwrap_or_else(|| events.last().map(|e| e.timestamp).unwrap_or(0));

    let total_duration_ms = if run_end_ts > run_start_ts {
        (run_end_ts - run_start_ts) as u64
    } else {
        0
    };

    // -------------------------------------------------------------------
    // 6. Attach the error message from run_end to the last turn's llm_call
    // -------------------------------------------------------------------
    let run_error: Option<String> = events
        .iter()
        .rev()
        .find(|e| e.event_type == "run_end")
        .and_then(|e| e.data.as_ref())
        .and_then(|d| d.get("error"))
        .and_then(|v| v.as_str())
        .filter(|s| !s.is_empty())
        .map(|s| s.to_string());

    let mut turns = turns;
    if let Some(err) = run_error {
        if let Some(last) = turns.last_mut() {
            last.llm_call.error_message = Some(err);
        }
    }

    let user_messages = build_user_messages(events);

    RunRecord {
        config_snapshot,
        turns,
        total_duration_ms,
        total_input_tokens,
        total_output_tokens,
        user_messages,
    }
}

/// Walk events in order, pulling out every `user` event and tagging it
/// with the `turn_number` of the first iteration that follows. Iteration
/// numbering restarts each `run_start`, so we mirror `build_turns`'s
/// counter to stay aligned. A user event that has no following
/// iteration (cancelled / errored before any LLM call) attaches to one
/// past the last counted turn so it still surfaces in the UI as a
/// pending prompt.
fn build_user_messages(events: &[SessionEvent]) -> Vec<UserMessageRecord> {
    let mut out: Vec<UserMessageRecord> = Vec::new();
    let mut next_turn_number: u32 = 1;
    let mut pending: Vec<(String, u64)> = Vec::new();

    for ev in events {
        match ev.event_type.as_str() {
            "user" => {
                let text = ev
                    .message
                    .as_ref()
                    .map(|m| extract_text_from_content(&m.content))
                    .unwrap_or_default();
                pending.push((text, ev.timestamp as u64));
            }
            "iteration_start" => {
                let tn = next_turn_number;
                for (text, ts) in pending.drain(..) {
                    out.push(UserMessageRecord {
                        before_turn_number: tn,
                        text,
                        timestamp_ms: ts,
                    });
                }
                next_turn_number += 1;
            }
            _ => {}
        }
    }
    // Drain any remaining (no iteration after them) → tag them past
    // the end of the timeline so the inspector still surfaces them.
    if !pending.is_empty() {
        let tn = next_turn_number;
        for (text, ts) in pending.drain(..) {
            out.push(UserMessageRecord {
                before_turn_number: tn,
                text,
                timestamp_ms: ts,
            });
        }
    }
    out
}

/// Pull plain text out of a `SessionMessage.content` JSON value. Joins
/// every `type=="text"` block; ignores non-text blocks (images, etc.).
/// Falls back to a bare string if `content` isn't an array.
fn extract_text_from_content(content: &serde_json::Value) -> String {
    if let Some(arr) = content.as_array() {
        arr.iter()
            .filter_map(|b| {
                if b.get("type")?.as_str()? == "text" {
                    Some(b.get("text")?.as_str()?.to_string())
                } else {
                    None
                }
            })
            .collect::<Vec<_>>()
            .join(" ")
    } else if let Some(s) = content.as_str() {
        s.to_string()
    } else {
        String::new()
    }
}

// ===========================================================================
// Sub-projections
// ===========================================================================

/// Build the `ConfigSnapshot` from the `eval_config_snapshot` system event.
fn build_config_snapshot(
    cfg_event: Option<&SessionEvent>,
    fallback_max_iterations: u32,
) -> ConfigSnapshot {
    let data = match cfg_event.and_then(|e| e.data.as_ref()) {
        Some(d) => d,
        None => {
            return ConfigSnapshot {
                system_prompt: Vec::new(),
                model_id: String::new(),
                tool_names: Vec::new(),
                tool_definitions: Vec::new(),
                skill_names: Vec::new(),
                max_iterations: fallback_max_iterations,
                extension_names: Vec::new(),
                middleware_names: Vec::new(),
            };
        }
    };

    let system_prompt: Vec<String> = data
        .get("system_prompt")
        .and_then(|v| v.as_array())
        .map(|arr| {
            arr.iter()
                .filter_map(|v| v.as_str().map(|s| s.to_string()))
                .collect()
        })
        .unwrap_or_default();

    let model_id = data
        .get("model_id")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();

    let tool_names: Vec<String> = data
        .get("tool_names")
        .and_then(|v| v.as_array())
        .map(|arr| {
            arr.iter()
                .filter_map(|v| v.as_str().map(String::from))
                .collect()
        })
        .unwrap_or_default();

    let tool_definitions: Vec<ToolDefinition> = data
        .get("tool_definitions")
        .and_then(|v| serde_json::from_value(v.clone()).ok())
        .unwrap_or_default();

    let skill_names: Vec<String> = data
        .get("skill_names")
        .and_then(|v| v.as_array())
        .map(|arr| {
            arr.iter()
                .filter_map(|v| v.as_str().map(String::from))
                .collect()
        })
        .unwrap_or_default();

    let max_iterations = data
        .get("max_iterations")
        .and_then(|v| v.as_u64())
        .unwrap_or(fallback_max_iterations as u64) as u32;

    let extension_names: Vec<String> = data
        .get("extension_names")
        .and_then(|v| v.as_array())
        .map(|arr| {
            arr.iter()
                .filter_map(|v| v.as_str().map(String::from))
                .collect()
        })
        .unwrap_or_default();

    let middleware_names: Vec<String> = data
        .get("middleware_names")
        .and_then(|v| v.as_array())
        .map(|arr| {
            arr.iter()
                .filter_map(|v| v.as_str().map(String::from))
                .collect()
        })
        .unwrap_or_default();

    ConfigSnapshot {
        system_prompt,
        model_id,
        tool_names,
        tool_definitions,
        skill_names,
        max_iterations,
        extension_names,
        middleware_names,
    }
}

/// Group events into `TurnRecord`s by walking `iteration_start` →
/// `iteration_end` boundaries.
fn build_turns(events: &[SessionEvent]) -> Vec<TurnRecord> {
    let mut turns = Vec::new();
    let mut turn_number: u32 = 0;

    // Collect positions of iteration boundaries.
    let mut i = 0;
    while i < events.len() {
        if events[i].event_type == "iteration_start" {
            turn_number += 1;
            let iter_start_idx = i;
            let iter_start_ts = events[i].timestamp;
            let iteration_start_uuid = events[i].uuid.clone();

            // Find the matching iteration_end (parent_uuid == iteration_start_uuid)
            let iter_end_idx = events[iter_start_idx + 1..].iter().position(|e| {
                e.event_type == "iteration_end"
                    && e.parent_uuid.as_deref() == Some(&iteration_start_uuid)
            });
            let end_idx = iter_end_idx
                .map(|rel| iter_start_idx + 1 + rel)
                .unwrap_or(events.len() - 1);

            let iter_events = &events[iter_start_idx..=end_idx];
            let iter_end_ts = events[end_idx].timestamp;
            let duration_ms = if iter_end_ts > iter_start_ts {
                (iter_end_ts - iter_start_ts) as u64
            } else {
                0
            };

            let llm_call = build_llm_call_record(iter_events, turn_number);
            let tool_calls = build_tool_call_records(iter_events);

            turns.push(TurnRecord {
                turn_number,
                llm_call,
                tool_calls,
                duration_ms,
            });

            // Skip past the end of this iteration.
            i = end_idx + 1;
        } else {
            i += 1;
        }
    }

    turns
}

/// Build the `LlmCallRecord` for a single iteration's event slice.
fn build_llm_call_record(iter_events: &[SessionEvent], _turn_number: u32) -> LlmCallRecord {
    // Find llm_call_start and llm_call_end within this iteration.
    let llm_start = iter_events.iter().find(|e| e.event_type == "llm_call_start");
    let llm_end = iter_events.iter().find(|e| e.event_type == "llm_call_end");

    let llm_start_uuid = llm_start.map(|e| e.uuid.as_str()).unwrap_or("");
    let llm_start_ts = llm_start.map(|e| e.timestamp).unwrap_or(0);
    let llm_end_ts = llm_end.map(|e| e.timestamp).unwrap_or(llm_start_ts);
    let llm_duration_ms = if llm_end_ts > llm_start_ts {
        (llm_end_ts - llm_start_ts) as u64
    } else {
        0
    };

    let (input_tokens, output_tokens, cache_creation, cache_read) = llm_end
        .and_then(|e| e.data.as_ref())
        .map(|d| {
            let inp = d.get("input_tokens").and_then(|v| v.as_u64()).unwrap_or(0) as u32;
            let out = d.get("output_tokens").and_then(|v| v.as_u64()).unwrap_or(0) as u32;
            // Anthropic-only fields. `as_u64` returns None for null/missing
            // so we map to Option<u32>.
            let cache_creation = d
                .get("cache_creation_input_tokens")
                .and_then(|v| v.as_u64())
                .map(|n| n as u32);
            let cache_read = d
                .get("cache_read_input_tokens")
                .and_then(|v| v.as_u64())
                .map(|n| n as u32);
            (inp, out, cache_creation, cache_read)
        })
        .unwrap_or((0, 0, None, None));

    // Per-turn config knobs from llm_call_start.
    let llm_start_data = llm_start.and_then(|e| e.data.as_ref());
    let disable_tools = llm_start_data
        .and_then(|d| d.get("disable_tools"))
        .and_then(|v| v.as_bool())
        .unwrap_or(false);
    let system_prompt_segments = llm_start_data
        .and_then(|d| d.get("system_prompt_segments"))
        .and_then(|v| v.as_u64())
        .map(|n| n as u32)
        .unwrap_or(0);
    let tools_count_sent = llm_start_data
        .and_then(|d| d.get("tools_count_sent"))
        .and_then(|v| v.as_u64())
        .map(|n| n as u32)
        .unwrap_or(0);
    let provider_options_applied = llm_start_data
        .and_then(|d| d.get("provider_options_applied"))
        .and_then(|v| v.as_bool())
        .unwrap_or(false);

    // Find the assistant message event (emitted by append_message with event_type "assistant")
    // The data field holds the serialized AgentMessage; the message field has SessionMessage.
    let assistant_event = iter_events.iter().find(|e| {
        e.event_type == "assistant"
            && e.parent_uuid.as_deref() == Some(llm_start_uuid)
    }).or_else(|| {
        // fallback: any assistant event in this iteration
        iter_events.iter().find(|e| e.event_type == "assistant")
    });

    // Deserialize the assistant Message from data (AgentMessage::Standard).
    let response: Option<Message> = assistant_event
        .and_then(|e| e.data.as_ref())
        .and_then(|d| {
            // data holds AgentMessage serialized — try Standard variant directly
            if let Some(std_val) = d.get("Standard") {
                serde_json::from_value::<Message>(std_val.clone()).ok()
            } else {
                // Try direct deserialization as Message (older format)
                serde_json::from_value::<Message>(d.clone()).ok()
            }
        });

    // Determine stop_reason from the response.
    let stop_reason = match &response {
        Some(resp) => {
            let has_tool_use = resp.content.iter().any(|block| {
                matches!(block, alva_kernel_abi::ContentBlock::ToolUse { .. })
            });
            if has_tool_use {
                "tool_use".to_string()
            } else {
                "end_turn".to_string()
            }
        }
        None => "error".to_string(),
    };

    // messages_sent is read from the llm_call_start event's data["messages"] field,
    // which carries the full serialized Vec<Message> since the kernel fix.
    let messages_sent: Vec<Message> = llm_start
        .and_then(|e| e.data.as_ref())
        .and_then(|d| d.get("messages"))
        .and_then(|m| serde_json::from_value(m.clone()).ok())
        .unwrap_or_default();
    let messages_sent_count = messages_sent.len();

    LlmCallRecord {
        messages_sent,
        messages_sent_count,
        response,
        input_tokens,
        output_tokens,
        duration_ms: llm_duration_ms,
        stop_reason,
        error_message: None, // filled later from run_end
        middleware_hooks: Vec::new(),
        cache_creation_input_tokens: cache_creation,
        cache_read_input_tokens: cache_read,
        disable_tools,
        system_prompt_segments,
        tools_count_sent,
        provider_options_applied,
    }
}

/// Build tool call records from tool_use / tool_result pairs in this
/// iteration's event slice.
fn build_tool_call_records(iter_events: &[SessionEvent]) -> Vec<ToolCallRecord> {
    let mut tool_calls = Vec::new();

    for event in iter_events {
        if event.event_type != "tool_use" {
            continue;
        }
        let data = match &event.data {
            Some(d) => d,
            None => continue,
        };

        let tool_name = data
            .get("tool_name")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();
        let tool_call_id = data
            .get("tool_call_id")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();

        let tool_use_uuid = event.uuid.as_str();
        let tool_use_ts = event.timestamp;

        // Find matching tool_result: the single event written by append_message,
        // linked via parent_uuid == tool_use_uuid. Its data holds the serialized
        // AgentMessage containing the ToolResult content block.
        let tool_result_event = iter_events.iter().find(|e| {
            e.event_type == "tool_result"
                && e.parent_uuid.as_deref() == Some(tool_use_uuid)
        });

        // duration_ms is derived from timestamp delta (both epoch millis).
        let duration_ms = tool_result_event
            .map(|e| {
                let result_ts = e.timestamp;
                if result_ts > tool_use_ts {
                    (result_ts - tool_use_ts) as u64
                } else {
                    0
                }
            })
            .unwrap_or(0);

        // is_error is extracted from the deserialized AgentMessage's ToolResult block.
        // tool_output is extracted the same way via the shared find_tool_output helper.
        let tool_output: Option<ToolOutput> = find_tool_output(iter_events, &tool_call_id);
        let is_error = tool_output.as_ref().map(|o| o.is_error).unwrap_or(false);

        // Build the ToolCall from the arguments in the assistant response.
        // The arguments are stored in the assistant message's content blocks.
        let tool_call = ToolCall {
            id: tool_call_id.clone(),
            name: tool_name,
            arguments: extract_tool_arguments(iter_events, &tool_call_id),
        };

        tool_calls.push(ToolCallRecord {
            tool_call,
            result: tool_output,
            is_error,
            duration_ms,
            middleware_hooks: Vec::new(),
            // sub_run is populated by build_run_record after build_turns returns,
            // using find_subagent_range over the full parent event stream.
            sub_run: None,
        });
    }

    tool_calls
}

/// Find the ToolOutput for a given tool_call_id by looking at the single
/// tool_result event (written by append_message) in the iteration's event slice.
/// Deserialize an event's `data` field as a `Message`.
///
/// `AgentMessage` uses `#[serde(tag = "kind")]` (internally-tagged), so
/// `AgentMessage::Standard(Message)` serializes flat — the Message fields
/// sit alongside `"kind":"Standard"` rather than under a `"Standard"` key.
/// We try the legacy externally-tagged shape first for backwards compat,
/// then fall back to parsing `data` directly as a `Message` (serde ignores
/// the extra `kind` field).
fn message_from_event_data(data: &serde_json::Value) -> Option<alva_kernel_abi::Message> {
    if let Some(std_val) = data.get("Standard") {
        if let Ok(m) = serde_json::from_value::<alva_kernel_abi::Message>(std_val.clone()) {
            return Some(m);
        }
    }
    serde_json::from_value::<alva_kernel_abi::Message>(data.clone()).ok()
}

/// Find the ToolOutput for a given tool_call_id by looking at the single
/// tool_result event (written by append_message) in the iteration's event slice.
fn find_tool_output(iter_events: &[SessionEvent], tool_call_id: &str) -> Option<ToolOutput> {
    for event in iter_events {
        if event.event_type != "tool_result" {
            continue;
        }
        let data = match &event.data {
            Some(d) => d,
            None => continue,
        };
        let msg = match message_from_event_data(data) {
            Some(m) => m,
            None => continue,
        };
        for block in &msg.content {
            if let alva_kernel_abi::ContentBlock::ToolResult { id, content, is_error } = block {
                if id == tool_call_id {
                    return Some(ToolOutput {
                        content: content.clone(),
                        is_error: *is_error,
                        details: None,
                    });
                }
            }
        }
    }
    None
}

/// Extract the tool arguments from the assistant message's ToolUse content block.
fn extract_tool_arguments(
    iter_events: &[SessionEvent],
    tool_call_id: &str,
) -> serde_json::Value {
    for event in iter_events {
        if event.event_type != "assistant" {
            continue;
        }
        let data = match &event.data {
            Some(d) => d,
            None => continue,
        };
        let msg = match message_from_event_data(data) {
            Some(m) => m,
            None => continue,
        };
        for block in &msg.content {
            if let alva_kernel_abi::ContentBlock::ToolUse { id, input, .. } = block {
                if id == tool_call_id {
                    return input.clone();
                }
            }
        }
    }
    serde_json::Value::Object(serde_json::Map::new())
}

#[cfg(test)]
mod tests {
    //! Tests for small pure helpers in session_projection.rs.
    //!
    //! Covers four private free-functions whose logic is independently
    //! testable without needing a full RunRecord fixture:
    //!   - extract_text_from_content (JSON content array → String)
    //!   - message_from_event_data (Standard-wrapped vs bare Message)
    //!   - find_tool_output (event-list search for tool_result)
    //!   - extract_tool_arguments (event-list search for ToolUse input)
    //!
    //! The bigger projection functions (build_run_record, build_turns,
    //! build_user_messages, build_config_snapshot, build_llm_call_record,
    //! build_tool_call_records) need full event-stream fixtures and are
    //! left for a follow-up loop.
    use super::*;
    use alva_kernel_abi::agent_session::{EventEmitter, SessionEvent};
    use serde_json::json;

    // -- extract_text_from_content -----------------------------------------

    #[test]
    fn extract_text_from_content_joins_text_blocks_with_space() {
        let v = json!([
            {"type": "text", "text": "hello"},
            {"type": "text", "text": "world"},
        ]);
        assert_eq!(extract_text_from_content(&v), "hello world");
    }

    #[test]
    fn extract_text_from_content_ignores_non_text_blocks() {
        // Skips image / tool_use blocks; preserves the text-block order.
        let v = json!([
            {"type": "image", "source": "..."},
            {"type": "text", "text": "kept"},
            {"type": "tool_use", "id": "x", "name": "n"},
        ]);
        assert_eq!(extract_text_from_content(&v), "kept");
    }

    #[test]
    fn extract_text_from_content_handles_bare_string() {
        // Some events store content as a string rather than an array
        let v = json!("just a string");
        assert_eq!(extract_text_from_content(&v), "just a string");
    }

    #[test]
    fn extract_text_from_content_returns_empty_for_other_types() {
        // Null / number / object → "" (graceful degradation)
        assert_eq!(extract_text_from_content(&json!(null)), "");
        assert_eq!(extract_text_from_content(&json!(42)), "");
        assert_eq!(extract_text_from_content(&json!({"foo": "bar"})), "");
    }

    #[test]
    fn extract_text_from_content_skips_text_block_missing_text_field() {
        // Defensive: type=="text" but no "text" → that block ignored,
        // others still collected (filter_map's `?` short-circuits).
        let v = json!([
            {"type": "text"},                   // no text field
            {"type": "text", "text": "ok"},
        ]);
        assert_eq!(extract_text_from_content(&v), "ok");
    }

    // -- message_from_event_data -------------------------------------------

    #[test]
    fn message_from_event_data_unwraps_standard_variant() {
        // Some persistence layers store AgentMessage::Standard tagged with
        // a "Standard" wrapper; the helper must unwrap it first.
        let inner_msg = json!({
            "id": "m1",
            "role": "user",
            "content": [{"type": "text", "text": "hi"}],
            "timestamp": 0,
        });
        let wrapped = json!({"Standard": inner_msg});
        let msg = message_from_event_data(&wrapped).expect("Standard wrapper should unwrap");
        assert_eq!(msg.id, "m1");
    }

    #[test]
    fn message_from_event_data_falls_back_to_bare_message() {
        // Older events / direct serialization put Message at the root.
        let bare = json!({
            "id": "m2",
            "role": "assistant",
            "content": [{"type": "text", "text": "hello"}],
            "timestamp": 0,
        });
        let msg = message_from_event_data(&bare).expect("bare Message should deserialize");
        assert_eq!(msg.id, "m2");
    }

    #[test]
    fn message_from_event_data_returns_none_for_invalid_json() {
        // Neither Standard-wrapped nor a valid bare Message → None
        let bogus = json!({"random": "garbage"});
        assert!(message_from_event_data(&bogus).is_none());
    }

    // -- find_tool_output / extract_tool_arguments shared fixtures ---------

    fn event(event_type: &str, data: serde_json::Value) -> SessionEvent {
        SessionEvent {
            seq: 0,
            uuid: format!("uuid-{}", event_type),
            parent_uuid: None,
            timestamp: 0,
            event_type: event_type.to_string(),
            emitter: EventEmitter::runtime(),
            message: None,
            data: Some(data),
        }
    }

    /// Builds a Standard-wrapped tool_result Message JSON with one
    /// `tool_result` content block. The inner `content` is a
    /// Vec<ToolContent> — one text block for simplicity.
    fn tool_result_event(tool_call_id: &str, result_text: &str, is_error: bool) -> SessionEvent {
        event(
            "tool_result",
            json!({"Standard": {
                "id": "msg-r",
                "role": "tool",
                "content": [{
                    "type": "tool_result",
                    "tool_use_id": tool_call_id,
                    "content": [{"type": "text", "text": result_text}],
                    "is_error": is_error,
                }],
                "timestamp": 0,
            }}),
        )
    }

    /// Builds a Standard-wrapped assistant Message JSON with one
    /// `tool_use` content block.
    fn assistant_tool_use_event(tool_call_id: &str, input: serde_json::Value) -> SessionEvent {
        event(
            "assistant",
            json!({"Standard": {
                "id": "msg-a",
                "role": "assistant",
                "content": [{
                    "type": "tool_use",
                    "id": tool_call_id,
                    "name": "some_tool",
                    "input": input,
                }],
                "timestamp": 0,
            }}),
        )
    }

    // -- find_tool_output --------------------------------------------------

    #[test]
    fn find_tool_output_returns_match_for_correct_id() {
        let events = vec![
            tool_result_event("call-1", "first", false),
            tool_result_event("call-2", "second", false),
        ];
        let out = find_tool_output(&events, "call-2").expect("call-2 should match");
        assert_eq!(out.content.len(), 1, "exactly one ToolContent block");
        assert_eq!(out.content[0].as_text(), Some("second"));
        assert!(!out.is_error);
    }

    #[test]
    fn find_tool_output_preserves_is_error_flag() {
        let events = vec![tool_result_event("call-x", "ERR", true)];
        let out = find_tool_output(&events, "call-x").expect("should find");
        assert!(out.is_error);
    }

    #[test]
    fn find_tool_output_returns_none_for_unknown_id() {
        let events = vec![tool_result_event("call-1", "only", false)];
        assert!(find_tool_output(&events, "call-missing").is_none());
    }

    #[test]
    fn find_tool_output_skips_non_tool_result_events() {
        // The same ID appears in an `assistant` event but the helper
        // is `event_type != "tool_result"` filtered — must not match.
        let events = vec![
            assistant_tool_use_event("call-1", json!({})),
            // No actual tool_result follows
        ];
        assert!(find_tool_output(&events, "call-1").is_none());
    }

    #[test]
    fn find_tool_output_handles_empty_event_list() {
        assert!(find_tool_output(&[], "anything").is_none());
    }

    // -- extract_tool_arguments --------------------------------------------

    #[test]
    fn extract_tool_arguments_returns_input_for_matching_id() {
        let events = vec![assistant_tool_use_event(
            "call-42",
            json!({"path": "/etc/passwd", "max_lines": 10}),
        )];
        let args = extract_tool_arguments(&events, "call-42");
        assert_eq!(args["path"], "/etc/passwd");
        assert_eq!(args["max_lines"], 10);
    }

    #[test]
    fn extract_tool_arguments_empty_object_when_id_not_found() {
        let events = vec![assistant_tool_use_event("other", json!({"x": 1}))];
        let args = extract_tool_arguments(&events, "missing");
        assert!(args.is_object());
        assert_eq!(args.as_object().unwrap().len(), 0);
    }

    #[test]
    fn extract_tool_arguments_skips_non_assistant_events() {
        // A tool_result event shouldn't be searched (helper filters on
        // event_type == "assistant"). Empty object expected.
        let events = vec![tool_result_event("call-1", "result", false)];
        let args = extract_tool_arguments(&events, "call-1");
        assert_eq!(args.as_object().unwrap().len(), 0);
    }

    #[test]
    fn extract_tool_arguments_handles_empty_event_list() {
        let args = extract_tool_arguments(&[], "id");
        assert!(args.is_object());
        assert!(args.as_object().unwrap().is_empty());
    }

    // -- build_config_snapshot --------------------------------------------

    #[test]
    fn build_config_snapshot_none_returns_defaults_with_fallback_max_iter() {
        let s = build_config_snapshot(None, 42);
        assert!(s.system_prompt.is_empty());
        assert_eq!(s.model_id, "");
        assert!(s.tool_names.is_empty());
        assert!(s.tool_definitions.is_empty());
        assert!(s.skill_names.is_empty());
        assert_eq!(s.max_iterations, 42, "fallback must be respected when no event");
        assert!(s.extension_names.is_empty());
        assert!(s.middleware_names.is_empty());
    }

    #[test]
    fn build_config_snapshot_event_without_data_field_returns_defaults() {
        // Event present but data field is None — the `.and_then(...)` chain
        // takes the same branch as None event.
        let mut ev = event("config_snapshot", json!({}));
        ev.data = None;
        let s = build_config_snapshot(Some(&ev), 7);
        assert_eq!(s.max_iterations, 7, "fallback used when event has no data");
        assert_eq!(s.model_id, "");
    }

    #[test]
    fn build_config_snapshot_full_data_populates_all_fields() {
        let ev = event(
            "config_snapshot",
            json!({
                "system_prompt": ["L0", "L1", "L2-dyn"],
                "model_id": "claude-sonnet-4-6",
                "tool_names": ["read_file", "edit"],
                "tool_definitions": [
                    {"name": "read_file", "description": "Reads", "parameters": {"type": "object"}},
                    {"name": "edit", "description": "Edits", "parameters": {"type": "object"}},
                ],
                "skill_names": ["debug", "test"],
                "max_iterations": 25,
                "extension_names": ["memory", "security"],
                "middleware_names": ["loop_detection"],
            }),
        );
        let s = build_config_snapshot(Some(&ev), 99);
        assert_eq!(s.system_prompt, vec!["L0", "L1", "L2-dyn"]);
        assert_eq!(s.model_id, "claude-sonnet-4-6");
        assert_eq!(s.tool_names, vec!["read_file", "edit"]);
        assert_eq!(s.tool_definitions.len(), 2);
        assert_eq!(s.tool_definitions[0].name, "read_file");
        assert_eq!(s.tool_definitions[1].name, "edit");
        assert_eq!(s.skill_names, vec!["debug", "test"]);
        assert_eq!(s.max_iterations, 25, "explicit value should beat fallback");
        assert_eq!(s.extension_names, vec!["memory", "security"]);
        assert_eq!(s.middleware_names, vec!["loop_detection"]);
    }

    #[test]
    fn build_config_snapshot_partial_data_only_populates_present_fields() {
        // Only model_id and skill_names present — everything else defaults.
        let ev = event(
            "config_snapshot",
            json!({"model_id": "gpt-5", "skill_names": ["s1"]}),
        );
        let s = build_config_snapshot(Some(&ev), 10);
        assert_eq!(s.model_id, "gpt-5");
        assert_eq!(s.skill_names, vec!["s1"]);
        assert!(s.system_prompt.is_empty());
        assert!(s.tool_names.is_empty());
        assert!(s.tool_definitions.is_empty());
        assert!(s.extension_names.is_empty());
        assert!(s.middleware_names.is_empty());
        assert_eq!(s.max_iterations, 10, "missing → fallback");
    }

    #[test]
    fn build_config_snapshot_max_iterations_falls_back_when_missing() {
        // Other fields present, but no max_iterations → fallback wins.
        let ev = event(
            "config_snapshot",
            json!({"model_id": "x", "tool_names": ["t"]}),
        );
        let s = build_config_snapshot(Some(&ev), 33);
        assert_eq!(s.max_iterations, 33);
    }

    #[test]
    fn build_config_snapshot_wrong_type_fields_fall_back_to_defaults() {
        // Defensive: a corrupted event with wrong-typed fields shouldn't
        // panic. Strings-where-arrays-expected → empty vec; non-number
        // max_iterations → fallback.
        let ev = event(
            "config_snapshot",
            json!({
                "system_prompt": "not-an-array",
                "tool_names": 42,
                "skill_names": null,
                "max_iterations": "oops",
                "extension_names": {"not": "an array"},
                "middleware_names": true,
            }),
        );
        let s = build_config_snapshot(Some(&ev), 11);
        assert!(s.system_prompt.is_empty());
        assert!(s.tool_names.is_empty());
        assert!(s.skill_names.is_empty());
        assert!(s.extension_names.is_empty());
        assert!(s.middleware_names.is_empty());
        assert_eq!(s.max_iterations, 11, "non-numeric → fallback");
    }

    #[test]
    fn build_config_snapshot_string_arrays_filter_out_non_string_values() {
        // Per the `.filter_map(|v| v.as_str().map(...))` pattern: mixed-type
        // arrays should keep only the string entries.
        let ev = event(
            "config_snapshot",
            json!({"tool_names": ["valid-1", 42, null, true, "valid-2"]}),
        );
        let s = build_config_snapshot(Some(&ev), 0);
        assert_eq!(s.tool_names, vec!["valid-1", "valid-2"]);
    }

    // -- build_user_messages ----------------------------------------------

    /// Build a "user" event whose SessionMessage carries `text` as a single
    /// `[{"type":"text", "text":...}]` content block at `timestamp`.
    fn user_event(text: &str, timestamp_ms: i64) -> SessionEvent {
        use alva_kernel_abi::agent_session::SessionMessage;
        SessionEvent {
            seq: 0,
            uuid: format!("u-{}", text),
            parent_uuid: None,
            timestamp: timestamp_ms,
            event_type: "user".to_string(),
            emitter: EventEmitter::runtime(),
            message: Some(SessionMessage {
                role: "user".to_string(),
                content: json!([{"type": "text", "text": text}]),
            }),
            data: None,
        }
    }

    /// Build a bare iteration_start marker event.
    fn iter_start_event() -> SessionEvent {
        event("iteration_start", json!({}))
    }

    #[test]
    fn build_user_messages_empty_events_returns_empty() {
        assert!(build_user_messages(&[]).is_empty());
    }

    #[test]
    fn build_user_messages_single_user_before_iter_gets_turn_1() {
        let events = vec![user_event("hi", 100), iter_start_event()];
        let out = build_user_messages(&events);
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].text, "hi");
        assert_eq!(out[0].before_turn_number, 1, "first iter → turn 1");
        assert_eq!(out[0].timestamp_ms, 100, "timestamp preserved");
    }

    #[test]
    fn build_user_messages_two_users_before_one_iter_both_get_turn_1() {
        // The pending queue flushes ALL accumulated users at the next
        // iteration_start, so both should map to before_turn_number=1.
        let events = vec![
            user_event("first", 100),
            user_event("second", 200),
            iter_start_event(),
        ];
        let out = build_user_messages(&events);
        assert_eq!(out.len(), 2);
        assert_eq!(out[0].before_turn_number, 1);
        assert_eq!(out[1].before_turn_number, 1);
        assert_eq!(out[0].text, "first");
        assert_eq!(out[1].text, "second");
    }

    #[test]
    fn build_user_messages_users_interleaved_with_iters_increment_turn_number() {
        // User → iter → user → iter — first user maps to turn 1, second to turn 2.
        let events = vec![
            user_event("u1", 10),
            iter_start_event(),
            user_event("u2", 20),
            iter_start_event(),
        ];
        let out = build_user_messages(&events);
        assert_eq!(out.len(), 2);
        assert_eq!(out[0].before_turn_number, 1, "first user → turn 1");
        assert_eq!(out[1].before_turn_number, 2, "second user → turn 2");
    }

    #[test]
    fn build_user_messages_user_after_last_iter_uses_past_end_turn_number() {
        // User → iter → user with NO trailing iter — the trailing user
        // gets `next_turn_number` which is 1+last_seen = 2 (it points
        // one past the last actual turn so the Inspector shows it as
        // a pending prompt at the tail of the timeline).
        let events = vec![
            user_event("first", 10),
            iter_start_event(),
            user_event("trailing", 99),
        ];
        let out = build_user_messages(&events);
        assert_eq!(out.len(), 2);
        assert_eq!(out[0].before_turn_number, 1);
        assert_eq!(out[1].before_turn_number, 2, "trailing user → past last turn");
        assert_eq!(out[1].text, "trailing");
    }

    #[test]
    fn build_user_messages_lone_user_no_iter_gets_turn_1() {
        // Brand new run: user typed something, no iteration_start fired
        // yet. `next_turn_number` is still 1 → it goes there.
        let events = vec![user_event("typed", 50)];
        let out = build_user_messages(&events);
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].before_turn_number, 1);
    }

    #[test]
    fn build_user_messages_ignores_unrelated_event_types() {
        // Non-{user, iteration_start} events must not affect counter or
        // produce records. Mix in assistant + tool_result for safety.
        let events = vec![
            event("assistant", json!({})),
            user_event("real", 10),
            event("tool_result", json!({})),
            event("config_snapshot", json!({})),
            iter_start_event(),
        ];
        let out = build_user_messages(&events);
        assert_eq!(out.len(), 1, "only the one user event should be projected");
        assert_eq!(out[0].text, "real");
        assert_eq!(out[0].before_turn_number, 1);
    }

    // -- find_subagent_range ---------------------------------------------

    fn subagent_start(tool_call_id: &str) -> SessionEvent {
        event("subagent_run_start", json!({"tool_call_id": tool_call_id}))
    }

    fn subagent_end(tool_call_id: &str) -> SessionEvent {
        event("subagent_run_end", json!({"tool_call_id": tool_call_id}))
    }

    #[test]
    fn find_subagent_range_empty_events_returns_none() {
        assert!(find_subagent_range(&[], "anything").is_none());
    }

    #[test]
    fn find_subagent_range_no_matching_start_returns_none() {
        // Events present but none is a subagent_run_start with our id.
        let events = vec![
            event("assistant", json!({})),
            subagent_start("other-id"),
            subagent_end("other-id"),
        ];
        assert!(find_subagent_range(&events, "wanted-id").is_none());
    }

    #[test]
    fn find_subagent_range_start_without_matching_end_returns_none() {
        // Start exists but no end with the same id follows.
        let events = vec![
            subagent_start("id-1"),
            event("assistant", json!({})),
            subagent_end("different-id"),
        ];
        assert!(find_subagent_range(&events, "id-1").is_none());
    }

    #[test]
    fn find_subagent_range_returns_inner_events_excluding_markers() {
        let inner = event("assistant", json!({"role": "inner"}));
        let events = vec![
            event("user", json!({})),
            subagent_start("call-1"),
            inner.clone(),
            event("tool_result", json!({})),
            subagent_end("call-1"),
            event("assistant", json!({"role": "after"})),
        ];
        let range = find_subagent_range(&events, "call-1").expect("should find");
        assert_eq!(range.len(), 2, "2 events between markers");
        assert_eq!(range[0].event_type, "assistant");
        assert_eq!(range[0].data.as_ref().unwrap()["role"], "inner");
        assert_eq!(range[1].event_type, "tool_result");
    }

    #[test]
    fn find_subagent_range_returns_empty_vec_when_markers_are_adjacent() {
        // start immediately followed by end → range = []  (Some, not None)
        let events = vec![subagent_start("c"), subagent_end("c")];
        let range = find_subagent_range(&events, "c").expect("should return Some(empty)");
        assert!(range.is_empty(), "no events between adjacent markers");
    }

    #[test]
    fn find_subagent_range_isolates_concurrent_subagents_by_id() {
        // Two subagent runs with different ids in flight at once. Asking
        // for "a" must return only events bracketed by a's markers,
        // ignoring b's traffic — but the actual function uses sequential
        // event order, so for an INNER b-run nested inside an a-run, the
        // a-range will include b's start/end events as opaque events.
        // This test pins down that behavior: nested b markers ARE returned
        // as part of a's inner range (they're just events).
        let events = vec![
            subagent_start("a"),
            event("assistant", json!({"who": "a-inner"})),
            subagent_start("b"),
            event("assistant", json!({"who": "b-inner"})),
            subagent_end("b"),
            subagent_end("a"),
        ];
        let a_range = find_subagent_range(&events, "a").expect("a should match");
        assert_eq!(a_range.len(), 4, "a's range includes b's markers + b's inner");

        let b_range = find_subagent_range(&events, "b").expect("b should match");
        assert_eq!(b_range.len(), 1);
        assert_eq!(b_range[0].data.as_ref().unwrap()["who"], "b-inner");
    }

    #[test]
    fn find_subagent_range_skips_events_with_missing_data_field() {
        // Defensive: an event with the right type but no data must not
        // match — and must not crash on the .and_then() chain.
        let mut start_no_data = subagent_start("c");
        start_no_data.data = None;
        let events = vec![start_no_data, subagent_end("c")];
        assert!(
            find_subagent_range(&events, "c").is_none(),
            "start without data field shouldn't match"
        );
    }

    // -- build_turns ------------------------------------------------------

    /// Build an iteration_start with the given uuid and timestamp.
    /// The uuid must be unique per iteration so iteration_end can match
    /// it via parent_uuid.
    fn iter_start(uuid: &str, ts: i64) -> SessionEvent {
        SessionEvent {
            seq: 0,
            uuid: uuid.to_string(),
            parent_uuid: None,
            timestamp: ts,
            event_type: "iteration_start".to_string(),
            emitter: EventEmitter::runtime(),
            message: None,
            data: None,
        }
    }

    /// Build an iteration_end whose parent_uuid points at the matching
    /// iteration_start. Without the parent link, build_turns falls
    /// through to its "no matching end" fallback path.
    fn iter_end(parent_start_uuid: &str, ts: i64) -> SessionEvent {
        SessionEvent {
            seq: 0,
            uuid: format!("end-of-{}", parent_start_uuid),
            parent_uuid: Some(parent_start_uuid.to_string()),
            timestamp: ts,
            event_type: "iteration_end".to_string(),
            emitter: EventEmitter::runtime(),
            message: None,
            data: None,
        }
    }

    #[test]
    fn build_turns_empty_events_returns_empty() {
        assert!(build_turns(&[]).is_empty());
    }

    #[test]
    fn build_turns_single_complete_iteration_emits_one_turn_with_duration() {
        let events = vec![
            iter_start("S1", 100),
            iter_end("S1", 250),
        ];
        let turns = build_turns(&events);
        assert_eq!(turns.len(), 1);
        assert_eq!(turns[0].turn_number, 1);
        assert_eq!(turns[0].duration_ms, 150, "end_ts - start_ts");
        // llm_call / tool_calls are populated by sub-fns; for our
        // minimal iter-only fixture they should be at their defaults.
        assert_eq!(turns[0].tool_calls.len(), 0);
    }

    #[test]
    fn build_turns_two_iterations_increment_turn_number() {
        let events = vec![
            iter_start("S1", 100),
            iter_end("S1", 150),
            iter_start("S2", 200),
            iter_end("S2", 350),
        ];
        let turns = build_turns(&events);
        assert_eq!(turns.len(), 2);
        assert_eq!(turns[0].turn_number, 1);
        assert_eq!(turns[1].turn_number, 2);
        assert_eq!(turns[0].duration_ms, 50);
        assert_eq!(turns[1].duration_ms, 150);
    }

    #[test]
    fn build_turns_iteration_without_matching_end_falls_back_to_last_event() {
        // No iteration_end at all → end_idx = events.len() - 1.
        // Duration = last_event.ts - start.ts.
        let events = vec![
            iter_start("S1", 100),
            // some unrelated trailing event with ts=400
            event("assistant", json!({})),
        ];
        let mut tail = events;
        // Set timestamp on the trailing event explicitly
        tail[1].timestamp = 400;
        let turns = build_turns(&tail);
        assert_eq!(turns.len(), 1, "still emits turn even when unclosed");
        assert_eq!(turns[0].turn_number, 1);
        assert_eq!(turns[0].duration_ms, 300, "uses last event ts as end");
    }

    #[test]
    fn build_turns_end_ts_before_start_ts_yields_zero_duration() {
        // Defensive: clock skew / replayed events with reversed times.
        let events = vec![
            iter_start("S1", 500),
            iter_end("S1", 100), // end before start!
        ];
        let turns = build_turns(&events);
        assert_eq!(turns.len(), 1);
        assert_eq!(turns[0].duration_ms, 0, "negative range → 0, not underflow");
    }

    #[test]
    fn build_turns_wrong_parent_uuid_end_does_not_match() {
        // iteration_end with WRONG parent_uuid is NOT picked → fallback
        // to events.len()-1 (which is the bad end itself, so duration
        // becomes end.ts - start.ts using the unmatched-end's timestamp).
        // The test pins down that parent_uuid is strictly checked, not
        // just event_type.
        let events = vec![
            iter_start("S1", 100),
            SessionEvent {
                seq: 0,
                uuid: "rogue-end".to_string(),
                parent_uuid: Some("DIFFERENT-PARENT".to_string()),
                timestamp: 999,
                event_type: "iteration_end".to_string(),
                emitter: EventEmitter::runtime(),
                message: None,
                data: None,
            },
        ];
        let turns = build_turns(&events);
        assert_eq!(turns.len(), 1);
        // Fallback uses last event (which is the rogue end), so duration
        // = 999 - 100 = 899 — this confirms it didn't pair, just used
        // length-1 fallback.
        assert_eq!(turns[0].duration_ms, 899);
    }

    #[test]
    fn build_turns_skips_non_iteration_events_at_top_level() {
        // user / assistant / etc. events outside any iteration must
        // not produce TurnRecords and must not affect the counter.
        let events = vec![
            event("user", json!({})),
            event("config_snapshot", json!({})),
            iter_start("S1", 100),
            iter_end("S1", 200),
            event("user", json!({})),
            iter_start("S2", 300),
            iter_end("S2", 400),
        ];
        let turns = build_turns(&events);
        assert_eq!(turns.len(), 2);
        assert_eq!(turns[0].turn_number, 1);
        assert_eq!(turns[1].turn_number, 2);
    }

    // -- build_tool_call_records ------------------------------------------

    /// Build a `tool_use` event (note: different from the `assistant`
    /// event that carries a ToolUse content block — `build_tool_call_records`
    /// iterates `event_type == "tool_use"` events specifically and reads
    /// `data.tool_name` / `data.tool_call_id` strings).
    fn tool_use_event(uuid: &str, tool_name: &str, tool_call_id: &str, ts: i64) -> SessionEvent {
        SessionEvent {
            seq: 0,
            uuid: uuid.to_string(),
            parent_uuid: None,
            timestamp: ts,
            event_type: "tool_use".to_string(),
            emitter: EventEmitter::runtime(),
            message: None,
            data: Some(json!({
                "tool_name": tool_name,
                "tool_call_id": tool_call_id,
            })),
        }
    }

    /// Build a tool_result event linked to its tool_use via parent_uuid,
    /// with a Standard-wrapped tool_result content block at a given ts.
    fn tool_result_event_linked(
        parent_tool_use_uuid: &str,
        tool_call_id: &str,
        result_text: &str,
        is_error: bool,
        ts: i64,
    ) -> SessionEvent {
        let mut ev = tool_result_event(tool_call_id, result_text, is_error);
        ev.parent_uuid = Some(parent_tool_use_uuid.to_string());
        ev.timestamp = ts;
        ev
    }

    #[test]
    fn build_tool_call_records_empty_returns_empty() {
        assert!(build_tool_call_records(&[]).is_empty());
    }

    #[test]
    fn build_tool_call_records_skips_non_tool_use_events() {
        // user / assistant / tool_result / iteration_start must not produce records
        let events = vec![
            event("user", json!({})),
            event("assistant", json!({})),
            event("tool_result", json!({})),
            event("iteration_start", json!({})),
        ];
        assert!(build_tool_call_records(&events).is_empty());
    }

    #[test]
    fn build_tool_call_records_skips_tool_use_without_data() {
        let mut ev = tool_use_event("u1", "read_file", "c1", 100);
        ev.data = None;
        assert!(build_tool_call_records(&[ev]).is_empty());
    }

    #[test]
    fn build_tool_call_records_single_tool_use_with_matching_result() {
        let events = vec![
            tool_use_event("uu-1", "read_file", "call-1", 100),
            tool_result_event_linked("uu-1", "call-1", "file contents", false, 350),
        ];
        let records = build_tool_call_records(&events);
        assert_eq!(records.len(), 1);
        let r = &records[0];
        assert_eq!(r.tool_call.name, "read_file");
        assert_eq!(r.tool_call.id, "call-1");
        assert_eq!(r.duration_ms, 250, "result_ts - use_ts");
        assert!(!r.is_error);
        let out = r.result.as_ref().expect("result should be Some");
        assert_eq!(out.content[0].as_text(), Some("file contents"));
        assert!(r.middleware_hooks.is_empty());
        assert!(r.sub_run.is_none());
    }

    #[test]
    fn build_tool_call_records_no_matching_result_yields_none_and_zero_duration() {
        let events = vec![tool_use_event("uu-1", "edit", "call-x", 100)];
        let records = build_tool_call_records(&events);
        assert_eq!(records.len(), 1);
        assert_eq!(records[0].duration_ms, 0, "no result → 0 duration");
        assert!(records[0].result.is_none());
        assert!(!records[0].is_error, "no result → not an error");
    }

    #[test]
    fn build_tool_call_records_reversed_timestamps_yield_zero_duration() {
        // Defensive: tool_result somehow earlier than tool_use (clock skew /
        // replay). Must not underflow.
        let events = vec![
            tool_use_event("uu-1", "edit", "c1", 500),
            tool_result_event_linked("uu-1", "c1", "ok", false, 100),
        ];
        let records = build_tool_call_records(&events);
        assert_eq!(records[0].duration_ms, 0);
    }

    #[test]
    fn build_tool_call_records_propagates_is_error_flag() {
        let events = vec![
            tool_use_event("uu-1", "edit", "c1", 100),
            tool_result_event_linked("uu-1", "c1", "boom", true, 200),
        ];
        let records = build_tool_call_records(&events);
        assert!(records[0].is_error, "is_error should flow through from ToolOutput");
        let out = records[0].result.as_ref().unwrap();
        assert!(out.is_error);
    }

    #[test]
    fn build_tool_call_records_arguments_pulled_from_assistant_event() {
        // extract_tool_arguments searches `assistant` events for a ToolUse
        // content block matching tool_call_id. Without one, arguments is
        // an empty object. Add one and verify it populates.
        let events_no_args = vec![
            tool_use_event("uu-1", "read_file", "call-1", 100),
            tool_result_event_linked("uu-1", "call-1", "x", false, 200),
        ];
        let records_no_args = build_tool_call_records(&events_no_args);
        assert_eq!(records_no_args[0].tool_call.arguments.as_object().unwrap().len(), 0);

        let events_with_args = vec![
            assistant_tool_use_event("call-1", json!({"path": "/etc"})),
            tool_use_event("uu-1", "read_file", "call-1", 100),
            tool_result_event_linked("uu-1", "call-1", "x", false, 200),
        ];
        let records_with_args = build_tool_call_records(&events_with_args);
        assert_eq!(records_with_args[0].tool_call.arguments["path"], "/etc");
    }

    #[test]
    fn build_tool_call_records_preserves_event_order_for_multiple_tool_uses() {
        let events = vec![
            tool_use_event("uu-1", "read", "c1", 100),
            tool_result_event_linked("uu-1", "c1", "r1", false, 150),
            tool_use_event("uu-2", "edit", "c2", 200),
            tool_result_event_linked("uu-2", "c2", "r2", false, 280),
        ];
        let records = build_tool_call_records(&events);
        assert_eq!(records.len(), 2);
        assert_eq!(records[0].tool_call.name, "read");
        assert_eq!(records[0].tool_call.id, "c1");
        assert_eq!(records[1].tool_call.name, "edit");
        assert_eq!(records[1].tool_call.id, "c2");
    }

    #[test]
    fn build_tool_call_records_defaults_missing_name_and_id_to_empty_string() {
        // Defensive: tool_use event with empty data {} → name = "", id = ""
        // (still emits a record so the call shows up in the UI even if
        // metadata is missing; better than silently dropping it).
        let mut ev = tool_use_event("uu-1", "_unused", "_unused", 100);
        ev.data = Some(json!({}));
        let records = build_tool_call_records(&[ev]);
        assert_eq!(records.len(), 1);
        assert_eq!(records[0].tool_call.name, "");
        assert_eq!(records[0].tool_call.id, "");
    }

    // -- build_llm_call_record --------------------------------------------

    /// Build an llm_call_start event with a given uuid + timestamp and
    /// an optional `data` JSON for the per-turn config knobs / messages.
    fn llm_call_start(uuid: &str, ts: i64, data: serde_json::Value) -> SessionEvent {
        SessionEvent {
            seq: 0,
            uuid: uuid.to_string(),
            parent_uuid: None,
            timestamp: ts,
            event_type: "llm_call_start".to_string(),
            emitter: EventEmitter::runtime(),
            message: None,
            data: Some(data),
        }
    }

    /// Build an llm_call_end event with token usage data.
    fn llm_call_end(ts: i64, data: serde_json::Value) -> SessionEvent {
        SessionEvent {
            seq: 0,
            uuid: "llm-end-uuid".to_string(),
            parent_uuid: None,
            timestamp: ts,
            event_type: "llm_call_end".to_string(),
            emitter: EventEmitter::runtime(),
            message: None,
            data: Some(data),
        }
    }

    /// Build an assistant event whose data field carries a
    /// Standard-wrapped Message with the given content blocks.
    fn assistant_event_linked(
        parent_uuid: Option<&str>,
        content_blocks: serde_json::Value,
    ) -> SessionEvent {
        SessionEvent {
            seq: 0,
            uuid: "assistant-msg".to_string(),
            parent_uuid: parent_uuid.map(String::from),
            timestamp: 0,
            event_type: "assistant".to_string(),
            emitter: EventEmitter::runtime(),
            message: None,
            data: Some(json!({"Standard": {
                "id": "msg-a",
                "role": "assistant",
                "content": content_blocks,
                "timestamp": 0,
            }})),
        }
    }

    #[test]
    fn build_llm_call_record_empty_events_returns_all_defaults() {
        let r = build_llm_call_record(&[], 1);
        assert!(r.messages_sent.is_empty());
        assert_eq!(r.messages_sent_count, 0);
        assert!(r.response.is_none());
        assert_eq!(r.input_tokens, 0);
        assert_eq!(r.output_tokens, 0);
        assert_eq!(r.duration_ms, 0);
        assert_eq!(r.stop_reason, "error", "no response → 'error'");
        assert!(r.error_message.is_none());
        assert!(r.middleware_hooks.is_empty());
        assert!(r.cache_creation_input_tokens.is_none());
        assert!(r.cache_read_input_tokens.is_none());
        assert!(!r.disable_tools);
        assert_eq!(r.system_prompt_segments, 0);
        assert_eq!(r.tools_count_sent, 0);
        assert!(!r.provider_options_applied);
    }

    #[test]
    fn build_llm_call_record_complete_happy_path() {
        // Full LLM call: start + end + assistant response with Text only.
        let events = vec![
            llm_call_start(
                "S-uu",
                100,
                json!({
                    "disable_tools": true,
                    "system_prompt_segments": 3,
                    "tools_count_sent": 12,
                    "provider_options_applied": true,
                    "messages": [{
                        "id": "msg-u",
                        "role": "user",
                        "content": [{"type": "text", "text": "hi"}],
                        "timestamp": 0,
                    }],
                }),
            ),
            llm_call_end(
                500,
                json!({
                    "input_tokens": 1500,
                    "output_tokens": 400,
                    "cache_creation_input_tokens": 800,
                    "cache_read_input_tokens": 200,
                }),
            ),
            assistant_event_linked(
                Some("S-uu"),
                json!([{"type": "text", "text": "answer"}]),
            ),
        ];
        let r = build_llm_call_record(&events, 1);
        assert_eq!(r.messages_sent.len(), 1);
        assert_eq!(r.messages_sent[0].id, "msg-u");
        assert_eq!(r.messages_sent_count, 1);
        assert_eq!(r.input_tokens, 1500);
        assert_eq!(r.output_tokens, 400);
        assert_eq!(r.cache_creation_input_tokens, Some(800));
        assert_eq!(r.cache_read_input_tokens, Some(200));
        assert_eq!(r.duration_ms, 400, "end_ts - start_ts");
        assert_eq!(r.stop_reason, "end_turn", "Text only response");
        assert!(r.disable_tools);
        assert_eq!(r.system_prompt_segments, 3);
        assert_eq!(r.tools_count_sent, 12);
        assert!(r.provider_options_applied);
        assert!(r.response.is_some());
    }

    #[test]
    fn build_llm_call_record_tool_use_response_sets_tool_use_stop_reason() {
        let events = vec![
            llm_call_start("S", 100, json!({})),
            llm_call_end(200, json!({"input_tokens": 0, "output_tokens": 0})),
            assistant_event_linked(
                Some("S"),
                json!([
                    {"type": "text", "text": "let me run a tool"},
                    {"type": "tool_use", "id": "tc-1", "name": "read", "input": {}},
                ]),
            ),
        ];
        let r = build_llm_call_record(&events, 1);
        assert_eq!(r.stop_reason, "tool_use", "ToolUse in content → 'tool_use'");
    }

    #[test]
    fn build_llm_call_record_no_response_sets_error_stop_reason() {
        // start + end but no assistant event at all → stop_reason = "error"
        let events = vec![
            llm_call_start("S", 100, json!({})),
            llm_call_end(200, json!({"input_tokens": 0, "output_tokens": 0})),
        ];
        let r = build_llm_call_record(&events, 1);
        assert_eq!(r.stop_reason, "error");
        assert!(r.response.is_none());
    }

    #[test]
    fn build_llm_call_record_missing_cache_token_fields_yield_none() {
        // Non-Anthropic providers omit cache_*_tokens → must stay None,
        // NOT default to Some(0).
        let events = vec![
            llm_call_start("S", 100, json!({})),
            llm_call_end(200, json!({"input_tokens": 100, "output_tokens": 50})),
        ];
        let r = build_llm_call_record(&events, 1);
        assert_eq!(r.input_tokens, 100);
        assert_eq!(r.output_tokens, 50);
        assert!(r.cache_creation_input_tokens.is_none(), "missing → None not Some(0)");
        assert!(r.cache_read_input_tokens.is_none());
    }

    #[test]
    fn build_llm_call_record_no_end_event_duration_is_zero() {
        // llm_call_start present, llm_call_end missing → end_ts defaults
        // to start_ts → duration_ms = 0; tokens default to 0.
        let events = vec![llm_call_start("S", 100, json!({}))];
        let r = build_llm_call_record(&events, 1);
        assert_eq!(r.duration_ms, 0);
        assert_eq!(r.input_tokens, 0);
        assert_eq!(r.output_tokens, 0);
    }

    #[test]
    fn build_llm_call_record_reversed_timestamps_yield_zero_duration() {
        // Defensive: clock skew / replay. end_ts < start_ts → 0, no underflow.
        let events = vec![
            llm_call_start("S", 500, json!({})),
            llm_call_end(100, json!({})),
        ];
        let r = build_llm_call_record(&events, 1);
        assert_eq!(r.duration_ms, 0);
    }

    #[test]
    fn build_llm_call_record_assistant_event_picked_by_parent_uuid_first() {
        // Two assistant events: one with NO parent_uuid (rogue), one
        // properly linked to llm_call_start. Must pick the linked one.
        let unrelated = assistant_event_linked(
            None,
            json!([{"type": "text", "text": "ROGUE"}]),
        );
        let linked = assistant_event_linked(
            Some("S"),
            json!([{"type": "text", "text": "LINKED"}]),
        );
        let events = vec![
            llm_call_start("S", 100, json!({})),
            unrelated,
            linked,
            llm_call_end(200, json!({})),
        ];
        let r = build_llm_call_record(&events, 1);
        let resp = r.response.expect("should pick the linked assistant");
        assert_eq!(resp.text_content(), "LINKED", "parent_uuid match wins over earlier rogue");
    }

    #[test]
    fn build_llm_call_record_falls_back_to_any_assistant_if_no_parent_match() {
        // Linked path fails (assistant has wrong parent_uuid) → falls back
        // to the first assistant event in the iteration.
        let mismatched = assistant_event_linked(
            Some("DIFFERENT-PARENT"),
            json!([{"type": "text", "text": "FALLBACK"}]),
        );
        let events = vec![
            llm_call_start("S", 100, json!({})),
            mismatched,
        ];
        let r = build_llm_call_record(&events, 1);
        let resp = r.response.expect("fallback to any assistant");
        assert_eq!(resp.text_content(), "FALLBACK");
    }

    #[test]
    fn build_llm_call_record_messages_sent_invalid_array_defaults_to_empty() {
        // Defensive: "messages" field present but wrong shape → empty vec,
        // not panic. Also tests messages_sent_count consistency.
        let events = vec![llm_call_start(
            "S",
            100,
            json!({"messages": "not-an-array"}),
        )];
        let r = build_llm_call_record(&events, 1);
        assert!(r.messages_sent.is_empty());
        assert_eq!(r.messages_sent_count, 0);
    }

    // -- empty_record + build_run_record (top-level integration) ----------

    /// Build a run_start event with optional max_iterations + timestamp.
    fn run_start_event(ts: i64, max_iter: u32) -> SessionEvent {
        event(
            "run_start",
            json!({"max_iterations": max_iter}),
        )
        .pipe(|mut e| {
            e.event_type = "run_start".to_string();
            e.timestamp = ts;
            e
        })
    }

    /// Build a run_end event with optional error string. ts is used for
    /// total_duration_ms = run_end_ts - run_start_ts.
    fn run_end_event(ts: i64, error: Option<&str>) -> SessionEvent {
        let mut data = serde_json::Map::new();
        if let Some(err) = error {
            data.insert("error".to_string(), json!(err));
        }
        let mut ev = event("run_end", serde_json::Value::Object(data));
        ev.timestamp = ts;
        ev
    }

    /// Build an eval_config_snapshot system event.
    fn eval_config_event(model_id: &str) -> SessionEvent {
        event(
            "system",
            json!({
                "type": "eval_config_snapshot",
                "model_id": model_id,
            }),
        )
    }

    // Tiny helper trait so the mutating closure in run_start_event reads
    // cleanly. (Avoids std::mem::take noise.)
    trait Pipe: Sized {
        fn pipe<R>(self, f: impl FnOnce(Self) -> R) -> R {
            f(self)
        }
    }
    impl<T> Pipe for T {}

    #[test]
    fn empty_record_returns_zeroed_runrecord() {
        let r = empty_record();
        assert!(r.config_snapshot.system_prompt.is_empty());
        assert_eq!(r.config_snapshot.model_id, "");
        assert!(r.config_snapshot.tool_names.is_empty());
        assert!(r.config_snapshot.tool_definitions.is_empty());
        assert!(r.config_snapshot.skill_names.is_empty());
        assert_eq!(r.config_snapshot.max_iterations, 0);
        assert!(r.config_snapshot.extension_names.is_empty());
        assert!(r.config_snapshot.middleware_names.is_empty());
        assert!(r.turns.is_empty());
        assert_eq!(r.total_duration_ms, 0);
        assert_eq!(r.total_input_tokens, 0);
        assert_eq!(r.total_output_tokens, 0);
        assert!(r.user_messages.is_empty());
    }

    #[test]
    fn build_run_record_empty_events_matches_empty_record() {
        let r = build_run_record(&[]);
        let empty = empty_record();
        assert_eq!(r.total_duration_ms, empty.total_duration_ms);
        assert_eq!(r.total_input_tokens, empty.total_input_tokens);
        assert_eq!(r.total_output_tokens, empty.total_output_tokens);
        assert!(r.turns.is_empty());
        assert!(r.user_messages.is_empty());
        assert_eq!(r.config_snapshot.max_iterations, 0);
    }

    #[test]
    fn build_run_record_single_turn_complete_run_populates_top_level_fields() {
        // Full session: run_start → eval_config → user → iter_start →
        // llm_call_start → llm_call_end → assistant → iter_end → run_end.
        let events = vec![
            run_start_event(100, 25),
            eval_config_event("claude-test"),
            user_event("hi", 105),
            iter_start("ITER-1", 110),
            llm_call_start("S1", 115, json!({"messages": []})),
            llm_call_end(180, json!({"input_tokens": 500, "output_tokens": 200})),
            assistant_event_linked(Some("S1"), json!([{"type": "text", "text": "answer"}])),
            iter_end("ITER-1", 190),
            run_end_event(200, None),
        ];
        let r = build_run_record(&events);
        // config_snapshot populated from eval_config_event
        assert_eq!(r.config_snapshot.model_id, "claude-test");
        // max_iterations: build_config_snapshot reads it from cfg_event.data
        // (cfg_event doesn't have max_iterations → falls back to run_start's 25)
        assert_eq!(r.config_snapshot.max_iterations, 25);
        // turns populated by build_turns
        assert_eq!(r.turns.len(), 1);
        assert_eq!(r.turns[0].turn_number, 1);
        // token aggregation
        assert_eq!(r.total_input_tokens, 500);
        assert_eq!(r.total_output_tokens, 200);
        // duration = run_end_ts(200) - run_start_ts(100)
        assert_eq!(r.total_duration_ms, 100);
        // user_messages collected
        assert_eq!(r.user_messages.len(), 1);
        assert_eq!(r.user_messages[0].text, "hi");
        assert_eq!(r.user_messages[0].before_turn_number, 1);
        // no error → no error_message on last turn
        assert!(r.turns[0].llm_call.error_message.is_none());
    }

    #[test]
    fn build_run_record_aggregates_tokens_across_multiple_llm_calls() {
        // 2 iterations, each with its own llm_call_end carrying tokens.
        // Verify SUM, not last-wins.
        let events = vec![
            run_start_event(100, 0),
            iter_start("I1", 110),
            llm_call_end(120, json!({"input_tokens": 100, "output_tokens": 50})),
            iter_end("I1", 130),
            iter_start("I2", 140),
            llm_call_end(150, json!({"input_tokens": 300, "output_tokens": 75})),
            iter_end("I2", 160),
        ];
        let r = build_run_record(&events);
        assert_eq!(r.total_input_tokens, 400, "100 + 300");
        assert_eq!(r.total_output_tokens, 125, "50 + 75");
    }

    #[test]
    fn build_run_record_attaches_error_from_run_end_to_last_turn() {
        let events = vec![
            run_start_event(100, 0),
            iter_start("I1", 110),
            iter_end("I1", 130),
            iter_start("I2", 140),
            iter_end("I2", 160),
            run_end_event(200, Some("max_iterations reached")),
        ];
        let r = build_run_record(&events);
        assert_eq!(r.turns.len(), 2);
        // first turn has no error
        assert!(r.turns[0].llm_call.error_message.is_none());
        // LAST turn picks up the run_end error
        assert_eq!(
            r.turns[1].llm_call.error_message,
            Some("max_iterations reached".to_string()),
            "error attached to last turn only"
        );
    }

    #[test]
    fn build_run_record_empty_error_string_does_not_attach() {
        // Defensive: run_end with empty error must not poison the
        // last-turn error_message. The .filter(|s| !s.is_empty()) guard
        // in build_run_record exists for exactly this case.
        let events = vec![
            run_start_event(100, 0),
            iter_start("I1", 110),
            iter_end("I1", 130),
            run_end_event(200, Some("")),
        ];
        let r = build_run_record(&events);
        assert!(
            r.turns[0].llm_call.error_message.is_none(),
            "empty error string should be filtered, not set"
        );
    }

    // -- build_run_record: sub_run recursion ------------------------------

    /// Helper: assemble events for a parent run that contains ONE tool_call
    /// inside iteration I1. The tool_call has `tool_call_id`. Sub-agent
    /// markers and child_events go at the top level (after iter_end) per
    /// the function's comment: "they live in the parent's stream, not
    /// within a single iteration."
    fn parent_with_tool_call(
        tool_call_id: &str,
        subagent_inner: Vec<SessionEvent>,
        with_markers: bool,
    ) -> Vec<SessionEvent> {
        let mut events = vec![
            run_start_event(100, 0),
            iter_start("I1", 110),
            tool_use_event("uu-1", "delegate", tool_call_id, 115),
            tool_result_event_linked("uu-1", tool_call_id, "child done", false, 200),
            iter_end("I1", 210),
        ];
        if with_markers {
            events.push(subagent_start(tool_call_id));
            events.extend(subagent_inner);
            events.push(subagent_end(tool_call_id));
        }
        events.push(run_end_event(300, None));
        events
    }

    #[test]
    fn build_run_record_no_subagent_markers_leaves_sub_run_none() {
        // Tool call exists but NO matching subagent_run_start/end pair.
        // sub_run must stay None.
        let events = parent_with_tool_call("call-1", vec![], false);
        let r = build_run_record(&events);
        assert_eq!(r.turns.len(), 1);
        assert_eq!(r.turns[0].tool_calls.len(), 1);
        assert!(
            r.turns[0].tool_calls[0].sub_run.is_none(),
            "no markers → sub_run stays None"
        );
    }

    #[test]
    fn build_run_record_matching_markers_attach_sub_run() {
        // Marker pair present, child range carries a minimal sub-run
        // (run_start + run_end) — sub_run should be populated, recursive
        // build_run_record returns a child RunRecord with its own duration.
        let child_inner = vec![
            run_start_event(220, 0),
            run_end_event(260, None),
        ];
        let events = parent_with_tool_call("call-1", child_inner, true);
        let r = build_run_record(&events);

        let tc = &r.turns[0].tool_calls[0];
        let sub = tc.sub_run.as_ref().expect("sub_run must be Some");
        // child's total_duration_ms = 260 - 220 = 40
        assert_eq!(sub.total_duration_ms, 40, "child run_start/run_end drives child duration");
        // Child has no iterations of its own → empty turns
        assert!(sub.turns.is_empty(), "child has no iter events → no turns");
    }

    #[test]
    fn build_run_record_empty_subagent_range_yields_empty_recursive_record() {
        // Adjacent markers (no child events between) → child_events is
        // empty → recursive build_run_record(empty) → empty_record()
        // (via the `if events.is_empty()` early return at line 215).
        let events = parent_with_tool_call("call-1", vec![], true);
        let r = build_run_record(&events);
        let sub = r.turns[0].tool_calls[0]
            .sub_run
            .as_ref()
            .expect("Some for adjacent markers (empty range)");
        // Should mirror empty_record() in all the integration-visible fields.
        assert_eq!(sub.total_duration_ms, 0);
        assert_eq!(sub.total_input_tokens, 0);
        assert_eq!(sub.total_output_tokens, 0);
        assert!(sub.turns.is_empty());
    }

    #[test]
    fn build_run_record_multiple_tool_calls_each_get_their_own_sub_run() {
        // Two tool_calls in the same turn, each with its own marker pair
        // and distinct child events. Each must attach its OWN sub_run,
        // not share or cross-contaminate.
        let events = vec![
            run_start_event(100, 0),
            iter_start("I1", 110),
            tool_use_event("uu-a", "delegate", "call-A", 115),
            tool_result_event_linked("uu-a", "call-A", "a done", false, 130),
            tool_use_event("uu-b", "delegate", "call-B", 140),
            tool_result_event_linked("uu-b", "call-B", "b done", false, 160),
            iter_end("I1", 170),
            // Markers + child for A: 50ms child run
            subagent_start("call-A"),
            run_start_event(200, 0),
            run_end_event(250, None),
            subagent_end("call-A"),
            // Markers + child for B: 100ms child run
            subagent_start("call-B"),
            run_start_event(260, 0),
            run_end_event(360, None),
            subagent_end("call-B"),
            run_end_event(400, None),
        ];
        let r = build_run_record(&events);
        assert_eq!(r.turns.len(), 1);
        assert_eq!(r.turns[0].tool_calls.len(), 2);

        let a_sub = r.turns[0].tool_calls[0].sub_run.as_ref().expect("A");
        let b_sub = r.turns[0].tool_calls[1].sub_run.as_ref().expect("B");
        assert_eq!(a_sub.total_duration_ms, 50, "A's child = 250-200");
        assert_eq!(b_sub.total_duration_ms, 100, "B's child = 360-260");
    }

    #[test]
    fn build_run_record_tool_call_with_empty_id_skips_sub_run_lookup() {
        // The `if !id.is_empty()` guard at line 265 prevents lookup with
        // an empty id (which would scan for subagent markers with id "").
        // Set up a tool_use with empty tool_call_id — even if marker
        // events with empty id somehow exist, sub_run must stay None.
        let mut events = vec![
            run_start_event(100, 0),
            iter_start("I1", 110),
        ];
        // tool_use with empty tool_call_id
        let mut empty_id_tu = tool_use_event("uu-x", "delegate", "_unused", 115);
        empty_id_tu.data = Some(json!({"tool_name": "delegate", "tool_call_id": ""}));
        events.push(empty_id_tu);
        events.push(iter_end("I1", 200));
        // Even if rogue markers with empty id existed, guard would skip
        events.push(subagent_start(""));
        events.push(subagent_end(""));
        events.push(run_end_event(300, None));

        let r = build_run_record(&events);
        let tc = &r.turns[0].tool_calls[0];
        assert_eq!(tc.tool_call.id, "", "empty id preserved on the record");
        assert!(tc.sub_run.is_none(), "empty id must not trigger sub_run lookup");
    }

    #[test]
    fn build_run_record_total_duration_falls_back_to_last_event_when_no_run_end() {
        // No run_end event → run_end_ts = last event's timestamp.
        let events = vec![
            run_start_event(100, 0),
            iter_start("I1", 110),
            iter_end("I1", 350),
        ];
        let r = build_run_record(&events);
        // last event's ts = 350; run_start_ts = 100 → duration = 250
        assert_eq!(r.total_duration_ms, 250);
    }

    #[test]
    fn build_llm_call_record_wrong_type_config_knobs_default_safely() {
        // Defensive: corrupted llm_call_start.data → all knobs use defaults.
        let events = vec![llm_call_start(
            "S",
            100,
            json!({
                "disable_tools": "not-a-bool",
                "system_prompt_segments": "not-a-number",
                "tools_count_sent": null,
                "provider_options_applied": 42,
            }),
        )];
        let r = build_llm_call_record(&events, 1);
        assert!(!r.disable_tools, "non-bool → false");
        assert_eq!(r.system_prompt_segments, 0, "non-number → 0");
        assert_eq!(r.tools_count_sent, 0, "null → 0");
        assert!(!r.provider_options_applied, "non-bool → false");
    }

    #[test]
    fn build_tool_call_records_pairs_results_by_parent_uuid_not_position() {
        // Two tool_uses with their tool_results in reversed order.
        // Pairing must be by parent_uuid, not by adjacency.
        let events = vec![
            tool_use_event("uu-A", "read", "cA", 100),
            tool_use_event("uu-B", "edit", "cB", 110),
            tool_result_event_linked("uu-B", "cB", "B-result", false, 200),
            tool_result_event_linked("uu-A", "cA", "A-result", false, 300),
        ];
        let records = build_tool_call_records(&events);
        assert_eq!(records.len(), 2);
        let a = &records[0];
        assert_eq!(a.tool_call.id, "cA");
        assert_eq!(a.duration_ms, 200, "A: 300 - 100");
        assert_eq!(a.result.as_ref().unwrap().content[0].as_text(), Some("A-result"));
        let b = &records[1];
        assert_eq!(b.tool_call.id, "cB");
        assert_eq!(b.duration_ms, 90, "B: 200 - 110");
        assert_eq!(b.result.as_ref().unwrap().content[0].as_text(), Some("B-result"));
    }

    #[test]
    fn build_turns_only_pairs_with_first_matching_end_after_start() {
        // If a second iteration_end for the same parent_uuid appears
        // later (shouldn't happen, but defensive), only the FIRST one
        // after start is used as the boundary. The second is skipped
        // past via `i = end_idx + 1` and won't be paired with anything.
        let events = vec![
            iter_start("S1", 100),
            iter_end("S1", 200),
            iter_end("S1", 300), // duplicate — should be ignored
        ];
        let turns = build_turns(&events);
        assert_eq!(turns.len(), 1, "duplicate end is skipped past, not re-paired");
        assert_eq!(turns[0].duration_ms, 100);
    }

    #[test]
    fn find_subagent_range_skips_events_with_non_string_tool_call_id() {
        // Defensive: tool_call_id stored as a number → as_str() returns
        // None → that event doesn't match.
        let events = vec![
            event("subagent_run_start", json!({"tool_call_id": 42})),
            event("subagent_run_end", json!({"tool_call_id": 42})),
        ];
        assert!(
            find_subagent_range(&events, "42").is_none(),
            "non-string tool_call_id must not match the string '42'"
        );
    }

    #[test]
    fn build_user_messages_handles_missing_message_field_with_empty_text() {
        // Defensive: a malformed user event without `message` should
        // still appear in the output with empty text (rather than
        // panic or be silently dropped).
        let mut ev = user_event("ignored", 0);
        ev.message = None;
        let events = vec![ev, iter_start_event()];
        let out = build_user_messages(&events);
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].text, "");
        assert_eq!(out[0].before_turn_number, 1);
    }

    #[test]
    fn build_config_snapshot_invalid_tool_definitions_silently_default_to_empty() {
        // tool_definitions uses serde_json::from_value, not filter_map →
        // entire field discards on any element that doesn't deserialize.
        // Verify the from_value().ok() branch falls back to empty
        // rather than panicking on malformed input.
        let ev = event(
            "config_snapshot",
            json!({"tool_definitions": [{"missing": "required-fields"}]}),
        );
        let s = build_config_snapshot(Some(&ev), 0);
        assert!(
            s.tool_definitions.is_empty(),
            "from_value failure should yield empty, not panic"
        );
    }
}
