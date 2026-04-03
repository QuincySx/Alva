// INPUT:  async_trait, serde, serde_json, thiserror, chrono, tokio_util, alva_agent_bus
// OUTPUT: CancellationToken, ContentBlock, AgentError, Message, LanguageModel, ModelConfig, TokenCounter, StreamEvent, Tool, ToolCall, ToolExecutionContext, ToolOutput, Bus, BusHandle, BusWriter, BusEvent, BusPlugin, PluginRegistrar, StateCell, TokenBudgetExceeded, ContextCompacted, MemoryExtracted, ...
// POS:    Crate root — re-exports all shared types including bus coordination primitives and context bus events.
pub mod base;
// context is now at scope::context; re-export for backward compatibility
pub use scope::context;
pub mod constants;
pub mod model;
pub mod task;
pub mod token_estimation;
pub mod tool;
pub mod multimodal;
// backward-compatible re-exports for old module paths
pub use multimodal::embedding;
pub use multimodal::transcription;
pub use multimodal::speech;
pub use multimodal::image;
pub use multimodal::video;
pub use multimodal::reranking;
pub use multimodal::moderation;
pub mod provider;
// provider_test is now at provider::tests; re-export for backward compatibility
pub use provider::tests as provider_test;
pub mod scope;
pub mod session;
// tool_guard is now at tool::guard

pub use base::cancel::CancellationToken;
pub use base::content::ContentBlock;
pub use base::error::AgentError;
pub use base::message::{AgentMessage, Marker, Message, MessageRole, UsageMetadata};
pub use model::{LanguageModel, ModelConfig, TokenCounter};
pub use base::stream::StreamEvent;
pub use tool::{Tool, ToolCall, ToolDefinition, ToolFs, ToolFsDirEntry, ToolFsExecResult, ToolPermissionResult, ToolRegistry, SearchReadInfo};
pub use tool::execution::{MinimalExecutionContext, ProgressEvent, ToolContent, ToolExecutionContext, ToolOutput};
pub use task::{TaskType, TaskStatus, TaskState, generate_task_id, create_task_state};
pub use token_estimation::{TokenEstimator, SimpleTokenEstimator};
pub use embedding::{EmbeddingModel, EmbeddingResult, EmbeddingUsage};
pub use transcription::{
    TranscriptionConfig, TranscriptionModel, TranscriptionResult, TranscriptionSegment,
};
pub use speech::{SpeechConfig, SpeechModel, SpeechResult};
pub use image::{ImageConfig, ImageData, ImageEditConfig, ImageModel, ImageResult};
pub use video::{VideoConfig, VideoData, VideoModel, VideoResult};
pub use reranking::{RankEntry, RerankConfig, RerankResult, RerankingModel};
pub use moderation::{ModerationCategory, ModerationEntry, ModerationModel, ModerationResult};
pub use provider::{CredentialSource, StaticCredential, Provider, ProviderError, ProviderRegistry};
pub use scope::{ChildScopeConfig, ScopeError, ScopeId, ScopeSnapshot};
pub use session::{AgentSession, InMemorySession};

// Bus — cross-layer coordination
pub use alva_agent_bus::{Bus, BusHandle, BusWriter, BusEvent, BusPlugin, PluginRegistrar, StateCell};

// Context bus events
pub use scope::context::{TokenBudgetExceeded, ContextCompacted, MemoryExtracted};
