use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;

use agent_types::{
    Message, ModelConfig, Tool, ToolCall, ToolContext, ToolResult,
};
use serde::{Deserialize, Serialize};
use serde_json::Value;

/// A boxed, pinned, Send future — used for async hook return types.
pub type HookFuture<T> = Pin<Box<dyn Future<Output = T> + Send>>;

// ---------------------------------------------------------------------------
// AgentMessage
// ---------------------------------------------------------------------------

/// Wraps either a standard LLM message or a custom application-level message
/// that can flow through the agent event stream.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind")]
pub enum AgentMessage {
    Standard(Message),
    Custom {
        type_name: String,
        data: Value,
    },
}

// ---------------------------------------------------------------------------
// AgentContext — snapshot passed to hooks
// ---------------------------------------------------------------------------

pub struct AgentContext<'a> {
    pub system_prompt: &'a str,
    pub messages: &'a [AgentMessage],
    pub tools: &'a [Arc<dyn Tool>],
}

// ---------------------------------------------------------------------------
// Tool-call policy
// ---------------------------------------------------------------------------

#[derive(Debug, Clone)]
pub enum ToolCallDecision {
    Allow,
    Block { reason: String },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ToolExecutionMode {
    Parallel,
    Sequential,
}

// ---------------------------------------------------------------------------
// AgentState — mutable state carried through the loop
// ---------------------------------------------------------------------------

pub struct AgentState {
    pub system_prompt: String,
    pub messages: Vec<AgentMessage>,
    pub tools: Vec<Arc<dyn Tool>>,
    pub is_streaming: bool,
    pub model_config: ModelConfig,
    pub tool_context: Arc<dyn ToolContext>,
}

impl AgentState {
    pub fn new(system_prompt: String, model_config: ModelConfig) -> Self {
        Self {
            system_prompt,
            messages: Vec::new(),
            tools: Vec::new(),
            is_streaming: false,
            model_config,
            tool_context: Arc::new(agent_types::EmptyToolContext),
        }
    }

    pub fn with_tool_context(
        system_prompt: String,
        model_config: ModelConfig,
        tool_context: Arc<dyn ToolContext>,
    ) -> Self {
        Self {
            system_prompt,
            messages: Vec::new(),
            tools: Vec::new(),
            is_streaming: false,
            model_config,
            tool_context,
        }
    }
}

// ---------------------------------------------------------------------------
// AgentConfig — the hook collection
// ---------------------------------------------------------------------------

/// Type aliases for the hook function signatures to keep things readable.
///
/// `convert_to_llm` is the *only* required hook — it converts the agent's
/// internal `AgentMessage` list into the `Message` slice the LLM expects.
pub type ConvertToLlmFn =
    Arc<dyn Fn(&AgentContext<'_>) -> Vec<Message> + Send + Sync>;

pub type TransformContextFn =
    Arc<dyn Fn(&[AgentMessage]) -> Vec<AgentMessage> + Send + Sync>;

pub type BeforeToolCallFn =
    Arc<dyn Fn(&ToolCall, &AgentContext<'_>) -> ToolCallDecision + Send + Sync>;

pub type AfterToolCallFn =
    Arc<dyn Fn(&ToolCall, ToolResult, &AgentContext<'_>) -> ToolResult + Send + Sync>;

pub type GetSteeringMessagesFn =
    Arc<dyn Fn(&AgentContext<'_>) -> Vec<AgentMessage> + Send + Sync>;

pub type GetFollowUpMessagesFn =
    Arc<dyn Fn(&AgentContext<'_>) -> Vec<AgentMessage> + Send + Sync>;

pub struct AgentConfig {
    /// Required — turns agent messages into LLM messages.
    pub convert_to_llm: ConvertToLlmFn,

    /// Optional — rewrite context before it is sent to the model.
    pub transform_context: Option<TransformContextFn>,

    /// Composable — decide whether a tool call should proceed.
    /// First `Block` wins; if all return `Allow`, the call proceeds.
    pub before_tool_call: Vec<BeforeToolCallFn>,

    /// Composable — post-process a tool result before it re-enters the loop.
    /// Hooks are chained: each receives the result from the previous one.
    pub after_tool_call: Vec<AfterToolCallFn>,

    /// Composable — inject steering messages after tool results.
    /// Messages from all hooks are collected.
    pub get_steering_messages: Vec<GetSteeringMessagesFn>,

    /// Composable — inject follow-up messages after the inner loop completes.
    /// Messages from all hooks are collected.
    pub get_follow_up_messages: Vec<GetFollowUpMessagesFn>,

    /// How tools are executed when there are multiple calls.
    pub tool_execution: ToolExecutionMode,

    /// Guard against runaway loops.
    pub max_iterations: u32,
}

impl AgentConfig {
    /// Create a config with only the required `convert_to_llm` hook.
    /// All optional hooks default to `None`, tool execution defaults to
    /// `Parallel`, and max iterations defaults to 100.
    pub fn new(convert_to_llm: ConvertToLlmFn) -> Self {
        Self {
            convert_to_llm,
            transform_context: None,
            before_tool_call: Vec::new(),
            after_tool_call: Vec::new(),
            get_steering_messages: Vec::new(),
            get_follow_up_messages: Vec::new(),
            tool_execution: ToolExecutionMode::Parallel,
            max_iterations: 100,
        }
    }
}
