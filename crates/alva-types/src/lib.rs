// INPUT:  async_trait, serde, serde_json, thiserror, chrono, tokio_util
// OUTPUT: CancellationToken, ContentBlock, AgentError, Message, MessageRole, UsageMetadata, LanguageModel, ModelConfig, StreamEvent, Tool, ToolCall, ToolDefinition, ToolRegistry, ToolExecutionContext, ToolOutput, ToolContent, ProgressEvent, MinimalExecutionContext, EmbeddingModel, TranscriptionModel, SpeechModel, ImageModel, VideoModel, RerankingModel, ModerationModel, Provider, ProviderRegistry
// POS:    Crate root — declares all type modules and re-exports their public items as the shared vocabulary for the agent ecosystem.
pub mod base;
// context is now at scope::context; re-export for backward compatibility
pub use scope::context;
pub mod model;
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
pub use tool::{Tool, ToolCall, ToolDefinition, ToolFs, ToolFsDirEntry, ToolFsExecResult, ToolRegistry};
pub use tool::execution::{MinimalExecutionContext, ProgressEvent, ToolContent, ToolExecutionContext, ToolOutput};
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
pub use alva_agent_bus::{Bus, BusHandle, BusEvent, StateCell};
