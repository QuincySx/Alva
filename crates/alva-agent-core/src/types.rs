// INPUT:  alva_types (Message, ModelConfig, Tool, ToolCall, ToolContext, ToolResult, AgentMessage), serde, serde_json, crate::middleware::MiddlewareStack, alva_agent_context
// OUTPUT: AgentMessage, AgentContext, AgentState, AgentHooks, ToolCallDecision, ToolExecutionMode, HookFuture, ConvertToLlmFn
// POS:    Defines core value types and the hook-based configuration struct (AgentHooks) that drives the agent loop.
use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;

use alva_types::{
    Message, ModelConfig, Tool, ToolCall, ToolContext, ToolResult,
};

use crate::middleware::MiddlewareStack;

/// A boxed, pinned, Send future — used for async hook return types.
pub type HookFuture<T> = Pin<Box<dyn Future<Output = T> + Send>>;

// Re-export AgentMessage from alva-types (canonical definition lives there).
pub use alva_types::AgentMessage;

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
    pub session_id: String,
    pub system_prompt: String,
    pub messages: Vec<AgentMessage>,
    pub tools: Vec<Arc<dyn Tool>>,
    pub is_streaming: bool,
    pub model_config: ModelConfig,
    pub tool_context: Arc<dyn ToolContext>,
}

impl AgentState {
    pub fn new(session_id: impl Into<String>, system_prompt: String, model_config: ModelConfig) -> Self {
        Self {
            session_id: session_id.into(),
            system_prompt,
            messages: Vec::new(),
            tools: Vec::new(),
            is_streaming: false,
            model_config,
            tool_context: Arc::new(alva_types::EmptyToolContext),
        }
    }

    pub fn with_tool_context(
        session_id: impl Into<String>,
        system_prompt: String,
        model_config: ModelConfig,
        tool_context: Arc<dyn ToolContext>,
    ) -> Self {
        Self {
            session_id: session_id.into(),
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
// AgentHooks — the hook collection
// ---------------------------------------------------------------------------

/// Type aliases for the hook function signatures to keep things readable.
///
/// `convert_to_llm` is the *only* required hook — it converts the agent's
/// internal `AgentMessage` list into the `Message` slice the LLM expects.
pub type ConvertToLlmFn =
    Arc<dyn Fn(&AgentContext<'_>) -> Vec<Message> + Send + Sync>;

pub type BeforeToolCallFn =
    Arc<dyn Fn(&ToolCall, &AgentContext<'_>) -> ToolCallDecision + Send + Sync>;

pub type AfterToolCallFn =
    Arc<dyn Fn(&ToolCall, ToolResult, &AgentContext<'_>) -> ToolResult + Send + Sync>;

pub type GetSteeringMessagesFn =
    Arc<dyn Fn(&AgentContext<'_>) -> Vec<AgentMessage> + Send + Sync>;

pub type GetFollowUpMessagesFn =
    Arc<dyn Fn(&AgentContext<'_>) -> Vec<AgentMessage> + Send + Sync>;

pub struct AgentHooks {
    /// Required — turns agent messages into LLM messages.
    pub convert_to_llm: ConvertToLlmFn,

    /// Context management plugin — drives context lifecycle hooks.
    pub context_plugin: Arc<dyn alva_agent_context::ContextPlugin>,

    /// Context management SDK — operations interface for the plugin.
    pub context_sdk: Arc<dyn alva_agent_context::ContextPluginSDK>,

    /// Optional message store — turn-based conversation persistence.
    pub message_store: Option<Arc<dyn alva_agent_context::MessageStore>>,

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

    /// Async middleware stack (onion model). Runs alongside the sync hooks.
    pub middleware: MiddlewareStack,
}

impl AgentHooks {
    /// Create a config with only the required `convert_to_llm` hook.
    /// All optional hooks default to `None`, tool execution defaults to
    /// `Parallel`, and max iterations defaults to 100.
    ///
    /// Default context plugin: `RulesContextPlugin` (deterministic, zero-LLM-cost).
    /// Default context SDK: `ContextSDKImpl` backed by an in-memory `ContextStore`.
    pub fn new(convert_to_llm: ConvertToLlmFn) -> Self {
        let ctx_store = Arc::new(std::sync::Mutex::new(
            alva_agent_context::ContextStore::new(200_000, 180_000, "/tmp/alva-ctx".into())
        ));
        let sdk: Arc<dyn alva_agent_context::ContextPluginSDK> = Arc::new(
            alva_agent_context::ContextSDKImpl::new(ctx_store)
        );
        let plugin: Arc<dyn alva_agent_context::ContextPlugin> = Arc::new(
            alva_agent_context::RulesContextPlugin::default()
        );

        Self {
            convert_to_llm,
            context_plugin: plugin,
            context_sdk: sdk,
            message_store: None,
            before_tool_call: Vec::new(),
            after_tool_call: Vec::new(),
            get_steering_messages: Vec::new(),
            get_follow_up_messages: Vec::new(),
            tool_execution: ToolExecutionMode::Parallel,
            max_iterations: 100,
            middleware: MiddlewareStack::new(),
        }
    }
}
