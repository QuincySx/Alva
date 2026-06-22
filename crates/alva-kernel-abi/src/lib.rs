// INPUT:  async_trait, serde, serde_json, thiserror, chrono, tokio_util, alva_kernel_bus
// OUTPUT: CancellationToken, ContentBlock, AgentError, Message, LanguageModel, ModelConfig, TokenCounter, StreamEvent, Tool, ToolCall, ToolExecutionContext, ToolOutput, Bus, BusHandle, BusWriter, BusEvent, BusPlugin, PluginRegistrar, StateCell, TokenBudgetExceeded, ContextCompacted, MemoryExtracted, SpawnCommunication, SpawnCommContext, SpawnCommHandle, SpawnCommError, SpawnCommunicationRegistry, OnChildComplete, SpawnResult, ...
// POS:    Crate root — re-exports all shared types including bus coordination primitives, context bus events, and spawn-time communication plugin contract.
pub use alva_llm_wire::adapter;
pub mod analytics;
pub mod base;
pub mod diagnostic;
// context is now at scope::context; re-export for backward compatibility
pub use scope::context;
pub mod constants;
pub mod model;
pub mod multimodal;
pub mod phase;
pub mod task;
pub mod token_estimation;
pub mod tool;
// backward-compatible re-exports for old module paths
pub use multimodal::embedding;
pub use multimodal::image;
pub use multimodal::moderation;
pub use multimodal::reranking;
pub use multimodal::speech;
pub use multimodal::transcription;
pub use multimodal::video;
pub mod provider;
// provider_test is now at provider::tests; re-export for backward compatibility
pub use provider::tests as provider_test;
pub mod agent_session;
pub mod runtime;
pub mod scope;
// tool_guard is now at tool::guard

pub use analytics::{AnalyticsEvent, AnalyticsSink, NoopAnalyticsSink};
pub use diagnostic::{Diagnostic, Severity};

pub use runtime::{timeout, NoopSleeper, Sleeper, TimeoutError};

pub use base::cancel::CancellationToken;
pub use base::content::ContentBlock;
pub use base::error::AgentError;
pub use base::message::{AgentMessage, Marker, Message, MessageRole, UsageMetadata};
pub use base::stream::StreamEvent;
pub use model::{CompletionResponse, LanguageModel, ModelConfig, ReasoningEffort, TokenCounter};
pub use phase::{Phase, PhaseEffect};
pub use tool::execution::{
    MinimalExecutionContext, ProgressEvent, ToolContent, ToolExecutionContext, ToolOutput,
};
pub use tool::scheduler::{ExecutionMode, LockMode, ResourceKey, ToolLockGuards, ToolLockRegistry};
pub use tool::{
    SearchReadInfo, Tool, ToolCall, ToolDefinition, ToolFs, ToolFsDirEntry, ToolFsExecResult,
    ToolPermissionResult, ToolRegistry, ToolSchemaContext,
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
    InMemoryAgentSession, ListenableInMemorySession, ScopedSession, SessionError, SessionEvent,
    SessionEventListener, SessionMessage,
};
#[doc(inline)]
pub use alva_macros::{bus_cap, bus_event};
pub use embedding::{EmbeddingModel, EmbeddingResult, EmbeddingUsage};
pub use image::{ImageConfig, ImageData, ImageEditConfig, ImageModel, ImageResult};
pub use moderation::{ModerationCategory, ModerationEntry, ModerationModel, ModerationResult};
pub use provider::{CredentialSource, Provider, ProviderError, ProviderRegistry, StaticCredential};
pub use reranking::{RankEntry, RerankConfig, RerankResult, RerankingModel};
pub use scope::spawn::{
    OnChildComplete, SpawnCommContext, SpawnCommError, SpawnCommHandle, SpawnCommunication,
    SpawnCommunicationRegistry, SpawnResult,
};
pub use scope::{ChildScopeConfig, ScopeError, ScopeId, ScopeSnapshot};
pub use speech::{SpeechConfig, SpeechModel, SpeechResult};
pub use task::{create_task_state, generate_task_id, TaskState, TaskStatus, TaskType};
pub use token_estimation::{SimpleTokenEstimator, TokenEstimator};
pub use transcription::{
    TranscriptionConfig, TranscriptionModel, TranscriptionResult, TranscriptionSegment,
};
pub use video::{VideoConfig, VideoData, VideoModel, VideoResult};

// Bus — cross-layer coordination
pub use alva_kernel_bus::{
    Bus, BusEvent, BusHandle, BusPlugin, BusWriter, PluginRegistrar, StateCell,
};

// Context bus events
pub use scope::context::{ContextCompacted, MemoryExtracted, TokenBudgetExceeded};
