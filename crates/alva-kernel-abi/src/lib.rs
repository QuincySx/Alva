// INPUT:  async_trait, serde, serde_json, thiserror, chrono, tokio_util, alva_kernel_bus
// OUTPUT: CancellationToken, ContentBlock, AgentError, Message, LanguageModel, ModelConfig, TokenCounter, StreamEvent, StreamMessageAccumulator, Tool, ToolCall, ToolExecutionContext, ToolOutput, ConversationLog, SessionStore, SessionRecord, Bus, BusHandle, BusWriter, BusEvent, BusPlugin, PluginRegistrar, StateCell, TokenBudgetExceeded, ContextCompacted, MemoryExtracted, ...
// POS:    内核契约层——循环怎么转的最小词汇：消息/工具/模型/取消/错误、
//         对话记录与会话存储、调度与上下文装配。agent 产品概念
//         （Task / Scope / Spawn / Phase / Analytics / Provider 注册表）在
//         alva-agent-abi（ticket 16 收口后内核不再承载）。
pub use alva_llm_wire::adapter;
pub use alva_llm_wire::{message_from_events, StreamMessageAccumulator, StreamMessageError};
pub mod agent_session;
pub mod base;
pub mod constants;
pub mod conversation_log;
pub mod model;
pub mod runtime;
pub mod scope;
pub mod session_record;
pub mod session_store;
pub mod tool;

pub use runtime::{timeout, NoopSleeper, Sleeper, TimeoutError};

pub use base::cancel::CancellationToken;
pub use base::content::ContentBlock;
pub use base::error::AgentError;
pub use base::message::{AgentMessage, Marker, Message, MessageRole, UsageMetadata};
pub use base::stream::StreamEvent;
pub use model::{CompletionResponse, LanguageModel, ModelConfig, ReasoningEffort, TokenCounter};
pub use tool::execution::{
    MinimalExecutionContext, ProgressEvent, ToolContent, ToolExecutionContext, ToolOutput,
};
pub use tool::scheduler::{
    normalize_path_string, resolve_against_workspace, ExecutionMode, LockMode, ResourceKey,
    Scheduling,
};
pub use tool::{
    tool_definition, EffectDescription, Tool, ToolCall, ToolDefinition, ToolFs, ToolFsDirEntry,
    ToolFsExecResult, ToolRegistry, ToolSchemaContext,
};

// Re-export proc macros from alva-macros. A derive macro and a trait
// with the same name can coexist: the trait lives in the type namespace,
// the derive in the macro namespace. `use alva_kernel_abi::Tool;` imports
// both, so users write `#[derive(Tool)]` and `impl Tool for ...` with
// a single import — same pattern as serde's Serialize/Deserialize.
#[doc(inline)]
pub use alva_macros::Tool;

// Discovery markers for `alva-bus-lint`. Identity attribute macros — the
// lint binary walks every `#[bus_cap]` trait in the workspace and enforces
// a cross-crate type-surface limit at the definition site. Re-exported
// from `alva-kernel-abi` so downstream crates don't need a direct
// `alva-macros` dep.
pub use agent_session::{
    AgentSession, ComponentDescriptor, EmitterKind, EventEmitter, EventMatch, EventQuery,
    PersistentSessions, ScopedSession, SessionError, SessionEvent, SessionEventListener,
    SessionMessage, StoreSessionCell,
};
#[doc(inline)]
pub use alva_macros::{bus_cap, bus_event};
pub use conversation_log::ConversationLog;
pub use session_record::{
    AppendReceipt, BlobReference, ConversationRecord, ExecutionRecord, ExtensionRecord,
    RecordCategory, RecordValue, SessionRecord, SessionRecordPayload,
    SESSION_RECORD_SCHEMA_VERSION,
};
pub use session_store::SessionStore;

// Bus — cross-layer coordination
pub use alva_kernel_bus::{
    Bus, BusEvent, BusHandle, BusPlugin, BusWriter, PluginRegistrar, StateCell,
};

// Context bus events
pub use scope::context::{ContextCompacted, MemoryExtracted, TokenBudgetExceeded};
