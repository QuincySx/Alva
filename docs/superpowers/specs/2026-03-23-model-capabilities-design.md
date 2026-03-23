# Model Capabilities Toolbox Design

## Goal

将 AI 模型能力（Embedding、Transcription、Speech、Image（含 edit）、Video、Reranking、Moderation）提升为框架级 trait，放在 `agent-types` 中与 `LanguageModel` 同级。Provider 作为能力工厂声明自己支持哪些能力，adapter 消化厂商差异。同时清理旧 V4 重类型（`ProviderOptions`、`ProviderMetadata`、`ProviderWarning`、`ProviderHeaders`）。

## 设计原则

参考 Vercel AI SDK V4 的全覆盖能力体系 + LangChain 的极简签名风格：

- **trait 定义不变的语义**：`embed = text → vector`，不管哪个厂商
- **Provider 声明能力组合**：OpenAI 有 7 种能力，DeepSeek 可能只有 1 种
- **Adapter 消化差异**：API 格式、认证方式、字段命名等全部在 adapter 内部处理
- **零传输泄露**：trait 方法签名不出现 HTTP headers、provider options 等传输层概念

## 架构分层

```
agent-types/                    ← Tier 1: trait 定义（能力语义）
  LanguageModel                 ← 已有
  EmbeddingModel                ← 新增
  TranscriptionModel            ← 新增
  SpeechModel                   ← 新增
  ImageModel                    ← 新增
  VideoModel                    ← 新增
  RerankingModel                ← 新增
  ModerationModel               ← 新增

srow-core/ports/provider/       ← Tier 4: Provider 工厂
  Provider trait                ← 扩展：7 个可选能力方法
  ProviderRegistry              ← 已有，加 7 个 shorthand 方法

srow-core/adapters/             ← Tier 4: 具体实现
  (后续实现 OpenAI-compat 等 adapter)
```

## 约定

- 所有 data struct 加 `#[derive(Debug, Clone, Serialize, Deserialize)]`
- 数量相关字段统一用 `usize`（Rust 惯例）
- `media_type` / `output_format` 等字符串字段应为 IANA 标准 MIME 类型，校验由 adapter 负责
- `Vec<u8>` 二进制字段（audio/video）默认 serde 序列化为整数数组；如需 base64 序列化可在 adapter 层用 `serde_bytes` 处理，不在 trait 层强制
- `RankEntry` 不包含 `document` 文本（同 LangChain `BaseCrossEncoder`），调用者需保留原始 `documents` 切片按 `index` 取值

## agent-types 新增 trait

### EmbeddingModel

```rust
// agent-types/src/embedding.rs

#[async_trait]
pub trait EmbeddingModel: Send + Sync {
    fn model_id(&self) -> &str;
    fn dimensions(&self) -> Option<usize>;
    fn max_embeddings_per_call(&self) -> Option<usize>;

    async fn embed(&self, texts: &[&str]) -> Result<EmbeddingResult, AgentError>;
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EmbeddingResult {
    pub embeddings: Vec<Vec<f32>>,
    pub usage: Option<EmbeddingUsage>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EmbeddingUsage {
    pub tokens: u32,
}
```

设计说明：
- `embed_documents` 和 `embed_query` 在 LangChain 中分开，但语义上相同（都是 text → vector）。Rust 中统一为 `embed(&[&str])` 即可，调用者传 1 条就是 query，传多条就是 documents。
- `dimensions()` 和 `max_embeddings_per_call()` 作为能力声明，跟 AI SDK 对齐。

### TranscriptionModel

```rust
// agent-types/src/transcription.rs

#[async_trait]
pub trait TranscriptionModel: Send + Sync {
    fn model_id(&self) -> &str;

    async fn transcribe(
        &self,
        audio: &[u8],
        config: &TranscriptionConfig,
    ) -> Result<TranscriptionResult, AgentError>;
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TranscriptionConfig {
    pub media_type: String,
    pub language: Option<String>,
    pub prompt: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TranscriptionResult {
    pub text: String,
    pub segments: Option<Vec<TranscriptionSegment>>,
    pub language: Option<String>,
    pub duration_seconds: Option<f64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TranscriptionSegment {
    pub text: String,
    pub start_seconds: f64,
    pub end_seconds: f64,
}
```

### SpeechModel

```rust
// agent-types/src/speech.rs

#[async_trait]
pub trait SpeechModel: Send + Sync {
    fn model_id(&self) -> &str;

    async fn synthesize(
        &self,
        text: &str,
        config: &SpeechConfig,
    ) -> Result<SpeechResult, AgentError>;
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SpeechConfig {
    pub voice: Option<String>,
    pub output_format: Option<String>,
    pub speed: Option<f32>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SpeechResult {
    pub audio: Vec<u8>,
    pub media_type: String,
}
```

### ImageModel

```rust
// agent-types/src/image.rs

#[async_trait]
pub trait ImageModel: Send + Sync {
    fn model_id(&self) -> &str;
    fn max_images_per_call(&self) -> Option<usize>;

    async fn generate(
        &self,
        prompt: &str,
        config: &ImageConfig,
    ) -> Result<ImageResult, AgentError>;

    /// Edit an existing image. Default: unsupported.
    async fn edit(
        &self,
        _image: &[u8],
        _prompt: &str,
        _config: &ImageEditConfig,
    ) -> Result<ImageResult, AgentError> {
        Err(AgentError::Other("image editing not supported".into()))
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ImageConfig {
    pub n: Option<u32>,
    pub size: Option<String>,
    pub aspect_ratio: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ImageEditConfig {
    pub mask: Option<Vec<u8>>,
    pub size: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ImageResult {
    pub images: Vec<ImageData>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ImageData {
    Base64(String),
    Bytes(Vec<u8>),
    Url(String),
}
```

### VideoModel

```rust
// agent-types/src/video.rs

#[async_trait]
pub trait VideoModel: Send + Sync {
    fn model_id(&self) -> &str;

    async fn generate(
        &self,
        prompt: &str,
        config: &VideoConfig,
    ) -> Result<VideoResult, AgentError>;
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VideoConfig {
    pub n: Option<u32>,
    pub duration_seconds: Option<f32>,
    pub size: Option<String>,
    pub aspect_ratio: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VideoResult {
    pub videos: Vec<VideoData>,
    pub media_type: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum VideoData {
    Base64(String),
    Bytes(Vec<u8>),
    Url(String),
}
```

### RerankingModel

```rust
// agent-types/src/reranking.rs

#[async_trait]
pub trait RerankingModel: Send + Sync {
    fn model_id(&self) -> &str;

    async fn rerank(
        &self,
        query: &str,
        documents: &[&str],
        config: &RerankConfig,
    ) -> Result<RerankResult, AgentError>;
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RerankConfig {
    pub top_n: Option<usize>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RerankResult {
    pub rankings: Vec<RankEntry>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RankEntry {
    pub index: usize,
    pub relevance_score: f64,
}
```

### ModerationModel

```rust
// agent-types/src/moderation.rs

#[async_trait]
pub trait ModerationModel: Send + Sync {
    fn model_id(&self) -> &str;

    async fn classify(
        &self,
        inputs: &[&str],
    ) -> Result<ModerationResult, AgentError>;
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModerationResult {
    pub results: Vec<ModerationEntry>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModerationEntry {
    pub flagged: bool,
    pub categories: Vec<ModerationCategory>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModerationCategory {
    pub name: String,
    pub flagged: bool,
    pub score: f64,
}
```

### agent-types/src/lib.rs 更新

```rust
pub mod embedding;
pub mod transcription;
pub mod speech;
pub mod image;
pub mod video;
pub mod reranking;
pub mod moderation;

pub use embedding::{EmbeddingModel, EmbeddingResult, EmbeddingUsage};
pub use transcription::{TranscriptionModel, TranscriptionConfig, TranscriptionResult, TranscriptionSegment};
pub use speech::{SpeechModel, SpeechConfig, SpeechResult};
pub use image::{ImageModel, ImageConfig, ImageEditConfig, ImageResult, ImageData};
pub use video::{VideoModel, VideoConfig, VideoResult, VideoData};
pub use reranking::{RerankingModel, RerankConfig, RerankResult, RankEntry};
pub use moderation::{ModerationModel, ModerationResult, ModerationEntry, ModerationCategory};
```

## Provider trait 扩展

```rust
// srow-core/src/ports/provider/provider_registry.rs

use std::sync::Arc;
use agent_types::{
    LanguageModel, EmbeddingModel, TranscriptionModel,
    SpeechModel, ImageModel, VideoModel, RerankingModel, ModerationModel,
};

pub trait Provider: Send + Sync {
    fn id(&self) -> &str;

    fn language_model(&self, model_id: &str)
        -> Result<Arc<dyn LanguageModel>, ProviderError>;

    fn embedding_model(&self, _model_id: &str)
        -> Result<Arc<dyn EmbeddingModel>, ProviderError> {
        Err(ProviderError::UnsupportedFunctionality("embedding".into()))
    }

    fn transcription_model(&self, _model_id: &str)
        -> Result<Arc<dyn TranscriptionModel>, ProviderError> {
        Err(ProviderError::UnsupportedFunctionality("transcription".into()))
    }

    fn speech_model(&self, _model_id: &str)
        -> Result<Arc<dyn SpeechModel>, ProviderError> {
        Err(ProviderError::UnsupportedFunctionality("speech".into()))
    }

    fn image_model(&self, _model_id: &str)
        -> Result<Arc<dyn ImageModel>, ProviderError> {
        Err(ProviderError::UnsupportedFunctionality("image".into()))
    }

    fn video_model(&self, _model_id: &str)
        -> Result<Arc<dyn VideoModel>, ProviderError> {
        Err(ProviderError::UnsupportedFunctionality("video".into()))
    }

    fn reranking_model(&self, _model_id: &str)
        -> Result<Arc<dyn RerankingModel>, ProviderError> {
        Err(ProviderError::UnsupportedFunctionality("reranking".into()))
    }

    fn moderation_model(&self, _model_id: &str)
        -> Result<Arc<dyn ModerationModel>, ProviderError> {
        Err(ProviderError::UnsupportedFunctionality("moderation".into()))
    }
}
```

ProviderRegistry 同步扩展 7 个 shorthand 方法（`embedding_model(provider_id, model_id)` 等），模式与现有 `language_model()` 完全一致。

## 旧 V4 清理

### 删除

| 文件 | 原因 |
|------|------|
| `srow-core/ports/provider/embedding_model.rs` | 被 `agent_types::EmbeddingModel` 替代 |
| `srow-core/ports/provider/transcription_model.rs` | 被 `agent_types::TranscriptionModel` 替代 |
| `srow-core/ports/provider/speech_model.rs` | 被 `agent_types::SpeechModel` 替代 |
| `srow-core/ports/provider/image_model.rs` | 被 `agent_types::ImageModel` 替代 |
| `srow-core/ports/provider/video_model.rs` | 被 `agent_types::VideoModel` 替代 |
| `srow-core/ports/provider/reranking_model.rs` | 被 `agent_types::RerankingModel` 替代 |
| `srow-core/ports/provider/middleware.rs` | 空占位，暂无实现 |

### 精简 types.rs

`ProviderOptions`、`ProviderMetadata`、`ProviderHeaders` 从 `types.rs` 中移除。`ProviderWarning` 移除。

如果 `types.rs` 变空，删除文件。如果还有其他类型被引用，保留需要的部分。

### 更新 mod.rs

移除已删除模块的 `pub mod` 声明和 `pub use` re-export。

## 实现注意事项

- 更新 `agent-types/src/AGENTS.md` 的业务域清单，加入 6 个新模块
- `agent-types` 的 `Cargo.toml` 无需新增依赖（`async-trait`、`serde` 已有）
- `srow-core` 的 `lib.rs` re-export 中无旧 V4 模型类型引用，删除不会影响外部

## 不在范围内

- OpenAI-compat adapter 的具体实现（下一个 spec）
- Streaming 支持（Speech/Video 可能需要，但第一版不做）
- Multimodal embedding（Gemini Embedding 2 支持图片+音频+视频 embedding，留作后续扩展）

## 参考

- [Vercel AI SDK V4 Provider Spec](https://ai-sdk.dev/providers/community-providers/custom-providers) — 7 种模型能力全覆盖
- [LangChain Embeddings](https://docs.langchain.com/oss/python/integrations/embeddings) — 极简 `embed_documents` / `embed_query`
- [LangChain BaseCrossEncoder](https://github.com/langchain-ai/langchain/blob/main/libs/core/langchain_core/cross_encoders.py) — 极简 `score(text_pairs)`
