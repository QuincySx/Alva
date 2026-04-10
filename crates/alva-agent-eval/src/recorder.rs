//! Data model and middleware for recording every detail of an agent run.
//!
//! These types capture the full lifecycle of an agent execution:
//! configuration snapshot, per-turn LLM and tool call records,
//! middleware hook timings, and run-level aggregate totals.
//!
//! The `RecorderMiddleware` passively populates these structs during
//! a live agent run. It never fails — all hooks return `Ok(())`.

use std::sync::{Arc, Mutex};
use std::time::Instant;

use async_trait::async_trait;
use serde::Serialize;

use alva_agent_core::middleware::{Middleware, MiddlewareError, MiddlewarePriority};
use alva_agent_core::state::AgentState;
use alva_types::{ContentBlock, Message, ToolCall, ToolDefinition, ToolOutput};

// ---------------------------------------------------------------------------
// Run-level record
// ---------------------------------------------------------------------------

/// Top-level record for a complete agent run.
#[derive(Debug, Clone, Serialize)]
pub struct RunRecord {
    pub config_snapshot: ConfigSnapshot,
    pub turns: Vec<TurnRecord>,
    pub total_duration_ms: u64,
    pub total_input_tokens: u64,
    pub total_output_tokens: u64,
}

/// Snapshot of the agent configuration at the start of the run.
#[derive(Debug, Clone, Serialize)]
pub struct ConfigSnapshot {
    pub system_prompt: String,
    pub model_id: String,
    pub tool_names: Vec<String>,
    /// Full tool definitions sent to the LLM (name + description + parameters schema).
    pub tool_definitions: Vec<ToolDefinition>,
    pub skill_names: Vec<String>,
    pub max_iterations: u32,
}

// ---------------------------------------------------------------------------
// Per-turn records
// ---------------------------------------------------------------------------

/// Record for a single agent turn (one LLM call + zero or more tool calls).
#[derive(Debug, Clone, Serialize)]
pub struct TurnRecord {
    pub turn_number: u32,
    pub llm_call: LlmCallRecord,
    pub tool_calls: Vec<ToolCallRecord>,
    pub duration_ms: u64,
}

/// Details of a single LLM inference call.
#[derive(Debug, Clone, Serialize)]
pub struct LlmCallRecord {
    pub messages_sent: Vec<Message>,
    pub messages_sent_count: usize,
    pub response: Option<Message>,
    pub input_tokens: u32,
    pub output_tokens: u32,
    pub duration_ms: u64,
    /// One of `"end_turn"`, `"tool_use"`, `"max_tokens"`, or `"error"`.
    pub stop_reason: String,
    pub middleware_hooks: Vec<HookRecord>,
}

/// Details of a single tool invocation within a turn.
#[derive(Debug, Clone, Serialize)]
pub struct ToolCallRecord {
    pub tool_call: ToolCall,
    pub result: Option<ToolOutput>,
    pub is_error: bool,
    pub duration_ms: u64,
    pub middleware_hooks: Vec<HookRecord>,
}

// ---------------------------------------------------------------------------
// Middleware hook record
// ---------------------------------------------------------------------------

/// Timing and outcome of a single middleware hook invocation.
#[derive(Debug, Clone, Serialize)]
pub struct HookRecord {
    pub middleware_name: String,
    pub hook: String,
    pub duration_ms: u64,
    pub outcome: String,
}

// ---------------------------------------------------------------------------
// In-progress turn accumulator (private)
// ---------------------------------------------------------------------------

/// Accumulates data for a single turn while it is being built.
struct TurnBuild {
    turn_number: u32,
    turn_start: Instant,
    llm_messages_sent: Vec<Message>,
    llm_start: Option<Instant>,
    llm_response: Option<Message>,
    llm_input_tokens: u32,
    llm_output_tokens: u32,
    tool_calls: Vec<ToolCallRecord>,
}

// ---------------------------------------------------------------------------
// Recorder internal state (private)
// ---------------------------------------------------------------------------

/// Mutable state held inside the recorder middleware.
struct RecorderState {
    config_snapshot: Option<ConfigSnapshot>,
    turns: Vec<TurnRecord>,
    current_turn: Option<TurnBuild>,
    run_start: Instant,
}

// ---------------------------------------------------------------------------
// RecorderMiddleware
// ---------------------------------------------------------------------------

/// Passive middleware that records every detail of an agent run.
///
/// Create one per run, attach it to the middleware stack, then call
/// [`take_record()`](Self::take_record) after the run completes to
/// extract the full [`RunRecord`].
pub struct RecorderMiddleware {
    state: Arc<Mutex<RecorderState>>,
}

impl RecorderMiddleware {
    /// Create a new recorder. The clock starts when the middleware is created,
    /// but `run_start` is reset in `on_agent_start` for accuracy.
    pub fn new() -> Self {
        Self {
            state: Arc::new(Mutex::new(RecorderState {
                config_snapshot: None,
                turns: Vec::new(),
                current_turn: None,
                run_start: Instant::now(),
            })),
        }
    }

    /// Pre-fill config fields that are only available to the caller (not via AgentState).
    /// Call this before the agent run starts.
    pub fn set_config(&self, system_prompt: String, max_iterations: u32, skill_names: Vec<String>) {
        let mut s = self.state.lock().unwrap();
        if let Some(ref mut snap) = s.config_snapshot {
            snap.system_prompt = system_prompt;
            snap.max_iterations = max_iterations;
            snap.skill_names = skill_names;
        } else {
            s.config_snapshot = Some(ConfigSnapshot {
                system_prompt,
                model_id: String::new(),
                tool_names: vec![],
                tool_definitions: vec![],
                skill_names,
                max_iterations,
            });
        }
    }

    /// Extract the completed [`RunRecord`], computing aggregate totals.
    ///
    /// This drains the internal state — calling it a second time yields
    /// an empty record with zero totals.
    pub fn take_record(&self) -> RunRecord {
        let mut s = self.state.lock().unwrap();

        // Finalize any in-progress turn (should already be done by on_agent_end,
        // but be defensive).
        Self::finalize_current_turn(&mut s);

        let total_duration_ms = s.run_start.elapsed().as_millis() as u64;

        let total_input_tokens: u64 = s
            .turns
            .iter()
            .map(|t| t.llm_call.input_tokens as u64)
            .sum();
        let total_output_tokens: u64 = s
            .turns
            .iter()
            .map(|t| t.llm_call.output_tokens as u64)
            .sum();

        let config_snapshot = s.config_snapshot.take().unwrap_or(ConfigSnapshot {
            system_prompt: String::new(),
            model_id: String::new(),
            tool_names: Vec::new(),
            tool_definitions: Vec::new(),
            skill_names: Vec::new(),
            max_iterations: 0,
        });

        let turns = std::mem::take(&mut s.turns);

        RunRecord {
            config_snapshot,
            turns,
            total_duration_ms,
            total_input_tokens,
            total_output_tokens,
        }
    }

    /// Finalize the current in-progress turn and push it to the turns vec.
    fn finalize_current_turn(s: &mut RecorderState) {
        if let Some(tb) = s.current_turn.take() {
            let duration_ms = tb.turn_start.elapsed().as_millis() as u64;

            let llm_duration_ms = tb
                .llm_start
                .map(|start| start.elapsed().as_millis() as u64)
                .unwrap_or(0);

            // Determine stop_reason from the response content blocks.
            let stop_reason = match &tb.llm_response {
                Some(resp) => {
                    let has_tool_use = resp
                        .content
                        .iter()
                        .any(|block| matches!(block, ContentBlock::ToolUse { .. }));
                    if has_tool_use {
                        "tool_use".to_string()
                    } else {
                        "end_turn".to_string()
                    }
                }
                None => "error".to_string(),
            };

            let llm_call = LlmCallRecord {
                messages_sent_count: tb.llm_messages_sent.len(),
                messages_sent: tb.llm_messages_sent,
                response: tb.llm_response,
                input_tokens: tb.llm_input_tokens,
                output_tokens: tb.llm_output_tokens,
                duration_ms: llm_duration_ms,
                stop_reason,
                middleware_hooks: Vec::new(),
            };

            s.turns.push(TurnRecord {
                turn_number: tb.turn_number,
                llm_call,
                tool_calls: tb.tool_calls,
                duration_ms,
            });
        }
    }
}

#[async_trait]
impl Middleware for RecorderMiddleware {
    fn name(&self) -> &str {
        "eval_recorder"
    }

    fn priority(&self) -> i32 {
        MiddlewarePriority::OBSERVATION + 100
    }

    async fn on_agent_start(&self, state: &mut AgentState) -> Result<(), MiddlewareError> {
        let mut s = self.state.lock().unwrap();
        s.run_start = Instant::now();

        // Merge runtime info (model_id, tool_names, tool_definitions) into the config snapshot.
        // The caller may have pre-filled system_prompt/max_iterations via set_config().
        let model_id = state.model.model_id().to_string();
        let tool_names: Vec<String> = state.tools.iter().map(|t| t.name().to_string()).collect();
        let tool_definitions: Vec<ToolDefinition> = state.tools.iter().map(|t| t.definition()).collect();

        if let Some(ref mut snap) = s.config_snapshot {
            snap.model_id = model_id;
            snap.tool_names = tool_names;
            snap.tool_definitions = tool_definitions;
        } else {
            s.config_snapshot = Some(ConfigSnapshot {
                system_prompt: String::new(),
                model_id,
                tool_names,
                tool_definitions,
                skill_names: Vec::new(),
                max_iterations: 0,
            });
        }

        Ok(())
    }

    async fn on_agent_end(
        &self,
        _state: &mut AgentState,
        _error: Option<&str>,
    ) -> Result<(), MiddlewareError> {
        let mut s = self.state.lock().unwrap();
        Self::finalize_current_turn(&mut s);
        Ok(())
    }

    async fn before_llm_call(
        &self,
        _state: &mut AgentState,
        messages: &mut Vec<Message>,
    ) -> Result<(), MiddlewareError> {
        let mut s = self.state.lock().unwrap();

        // Finalize any previous turn that was not yet closed
        // (e.g., a turn with no tool calls).
        Self::finalize_current_turn(&mut s);

        let turn_number = s.turns.len() as u32 + 1;

        s.current_turn = Some(TurnBuild {
            turn_number,
            turn_start: Instant::now(),
            llm_messages_sent: messages.clone(),
            llm_start: Some(Instant::now()),
            llm_response: None,
            llm_input_tokens: 0,
            llm_output_tokens: 0,
            tool_calls: Vec::new(),
        });

        Ok(())
    }

    async fn after_llm_call(
        &self,
        _state: &mut AgentState,
        response: &mut Message,
    ) -> Result<(), MiddlewareError> {
        let mut s = self.state.lock().unwrap();

        if let Some(ref mut tb) = s.current_turn {
            tb.llm_response = Some(response.clone());

            // Extract token usage from the response.
            if let Some(ref usage) = response.usage {
                tb.llm_input_tokens = usage.input_tokens;
                tb.llm_output_tokens = usage.output_tokens;
            }
        }

        Ok(())
    }

    async fn after_tool_call(
        &self,
        _state: &mut AgentState,
        tool_call: &ToolCall,
        result: &mut ToolOutput,
    ) -> Result<(), MiddlewareError> {
        let mut s = self.state.lock().unwrap();

        if let Some(ref mut tb) = s.current_turn {
            let is_error = result.is_error;
            tb.tool_calls.push(ToolCallRecord {
                tool_call: tool_call.clone(),
                result: Some(result.clone()),
                is_error,
                duration_ms: 0, // individual tool timing not available at this hook
                middleware_hooks: Vec::new(),
            });
        }

        Ok(())
    }
}
