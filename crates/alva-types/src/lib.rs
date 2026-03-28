// INPUT:  async_trait, serde, serde_json, thiserror, chrono, tokio_util
// OUTPUT: CancellationToken, ContentBlock, AgentError, Message, MessageRole, UsageMetadata, LanguageModel, ModelConfig, StreamEvent, Tool, ToolCall, ToolContext, LocalToolContext, EmptyToolContext, ToolDefinition, ToolRegistry, ToolResult, EmbeddingModel, TranscriptionModel, SpeechModel, ImageModel, VideoModel, RerankingModel, ModerationModel, Provider, ProviderRegistry
// POS:    Crate root — declares all type modules and re-exports their public items as the shared vocabulary for the agent ecosystem.
pub mod base;
// context is now at scope::context; re-export for backward compatibility
pub use scope::context;
pub mod model;
pub mod tool;
pub mod embedding;
pub mod transcription;
pub mod speech;
pub mod image;
pub mod video;
pub mod reranking;
pub mod moderation;
pub mod provider;
pub mod provider_test;
pub mod scope;
// tool_guard is now at tool::guard

pub use base::cancel::CancellationToken;
pub use base::content::ContentBlock;
pub use base::error::AgentError;
pub use base::message::{AgentMessage, Message, MessageRole, UsageMetadata};
pub use model::{LanguageModel, ModelConfig};
pub use base::stream::StreamEvent;
pub use tool::{EmptyToolContext, LocalToolContext, Tool, ToolCall, ToolContext, ToolDefinition, ToolFs, ToolFsDirEntry, ToolFsExecResult, ToolRegistry, ToolResult};
pub use embedding::{EmbeddingModel, EmbeddingResult, EmbeddingUsage};
pub use transcription::{
    TranscriptionConfig, TranscriptionModel, TranscriptionResult, TranscriptionSegment,
};
pub use speech::{SpeechConfig, SpeechModel, SpeechResult};
pub use image::{ImageConfig, ImageData, ImageEditConfig, ImageModel, ImageResult};
pub use video::{VideoConfig, VideoData, VideoModel, VideoResult};
pub use reranking::{RankEntry, RerankConfig, RerankResult, RerankingModel};
pub use moderation::{ModerationCategory, ModerationEntry, ModerationModel, ModerationResult};
pub use provider::{Provider, ProviderError, ProviderRegistry};
pub use scope::{ChildScopeConfig, ScopeError, ScopeId, ScopeSnapshot};
