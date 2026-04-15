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
}

/// Snapshot of the agent configuration at the start of the run.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConfigSnapshot {
    pub system_prompt: String,
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
            system_prompt: String::new(),
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
    }
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
    let turns = build_turns(events);

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

    RunRecord {
        config_snapshot,
        turns,
        total_duration_ms,
        total_input_tokens,
        total_output_tokens,
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
                system_prompt: String::new(),
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

    let system_prompt = data
        .get("system_prompt")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();

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

    let (input_tokens, output_tokens) = llm_end
        .and_then(|e| e.data.as_ref())
        .map(|d| {
            let inp = d.get("input_tokens").and_then(|v| v.as_u64()).unwrap_or(0) as u32;
            let out = d.get("output_tokens").and_then(|v| v.as_u64()).unwrap_or(0) as u32;
            (inp, out)
        })
        .unwrap_or((0, 0));

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

    // messages_sent is not stored in skeleton events — leave as empty Vec.
    // The frontend only shows response content and token counts for the
    // LlmCallRecord, so this doesn't impact UX.
    LlmCallRecord {
        messages_sent: Vec::new(),
        messages_sent_count: 0,
        response,
        input_tokens,
        output_tokens,
        duration_ms: llm_duration_ms,
        stop_reason,
        error_message: None, // filled later from run_end
        middleware_hooks: Vec::new(),
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

        // Find matching tool_result (parent_uuid == this tool_use uuid)
        let tool_result_event = iter_events.iter().find(|e| {
            e.event_type == "tool_result"
                && e.parent_uuid.as_deref() == Some(tool_use_uuid)
        });

        let (is_error, duration_ms) = tool_result_event
            .and_then(|e| e.data.as_ref())
            .map(|d| {
                let is_err = d.get("is_error").and_then(|v| v.as_bool()).unwrap_or(false);
                let dur = d.get("duration_ms").and_then(|v| v.as_u64()).unwrap_or(0);
                (is_err, dur)
            })
            .unwrap_or((false, 0));

        // Find the tool_result message event (event_type "tool_result" from append_message).
        // This is a different event from the skeleton tool_result — the message
        // event is the one with a message.content field (the actual tool output).
        // Kernel emits it via append_message right after the skeleton tool_result.
        // event_type == "tool_result" AND message is Some AND parent_uuid != tool_use_uuid
        // (since the message event is unparented — see run.rs line 872: None).
        // We identify it by finding a tool_result event with a message whose
        // content contains the tool_call_id.
        let tool_output: Option<ToolOutput> = find_tool_output(iter_events, &tool_call_id);

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
            // TODO: sub-run projection when sub-agent session linkage is wired (Phase 3+)
            sub_run: None,
        });
    }

    tool_calls
}

/// Find the ToolOutput for a given tool_call_id by looking at tool_result
/// message events in the iteration's event slice.
fn find_tool_output(iter_events: &[SessionEvent], tool_call_id: &str) -> Option<ToolOutput> {
    // Look for an event whose data (AgentMessage::Standard) has role Tool
    // and content containing a ToolResult block with matching id.
    for event in iter_events {
        if event.event_type != "tool_result" {
            continue;
        }
        let data = match &event.data {
            Some(d) => d,
            None => continue,
        };
        // Try to find AgentMessage::Standard with role Tool
        let msg = if let Some(std_val) = data.get("Standard") {
            serde_json::from_value::<alva_kernel_abi::Message>(std_val.clone()).ok()
        } else {
            None
        };
        if let Some(msg) = msg {
            // Check if this message has a ToolResult block with our tool_call_id
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
    }
    None
}

/// Extract the tool arguments from the assistant message's ToolUse content block.
fn extract_tool_arguments(
    iter_events: &[SessionEvent],
    tool_call_id: &str,
) -> serde_json::Value {
    // Find the assistant event and look for a ToolUse block with this id.
    for event in iter_events {
        if event.event_type != "assistant" {
            continue;
        }
        let data = match &event.data {
            Some(d) => d,
            None => continue,
        };
        let msg: Option<alva_kernel_abi::Message> = if let Some(std_val) = data.get("Standard") {
            serde_json::from_value::<alva_kernel_abi::Message>(std_val.clone()).ok()
        } else {
            None
        };
        if let Some(msg) = msg {
            for block in &msg.content {
                if let alva_kernel_abi::ContentBlock::ToolUse { id, input, .. } = block {
                    if id == tool_call_id {
                        return input.clone();
                    }
                }
            }
        }
    }
    serde_json::Value::Object(serde_json::Map::new())
}
