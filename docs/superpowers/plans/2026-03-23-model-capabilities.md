# Model Capabilities Toolbox Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add 7 model capability traits to `alva-types`, extend Provider/ProviderRegistry, and delete old V4 model files.

**Architecture:** Each trait gets its own file in `alva-types/src/`. Provider trait gains 7 optional methods with default "unsupported" returns. Old V4 model files in `alva-app-core/ports/provider/` are deleted along with `ProviderOptions`/`ProviderMetadata`/`ProviderWarning`/`ProviderHeaders`.

**Tech Stack:** Rust, async-trait, serde, alva-types, alva-app-core

**Spec:** `docs/superpowers/specs/2026-03-23-model-capabilities-design.md`

---

## File Structure

### Task 1: alva-types trait files (7 new files)
- Create: `crates/alva-types/src/embedding.rs`
- Create: `crates/alva-types/src/transcription.rs`
- Create: `crates/alva-types/src/speech.rs`
- Create: `crates/alva-types/src/image.rs`
- Create: `crates/alva-types/src/video.rs`
- Create: `crates/alva-types/src/reranking.rs`
- Create: `crates/alva-types/src/moderation.rs`
- Modify: `crates/alva-types/src/lib.rs`

### Task 2: Provider trait + ProviderRegistry extension
- Modify: `crates/alva-app-core/src/ports/provider/provider_registry.rs`

### Task 3: Old V4 cleanup
- Delete: `crates/alva-app-core/src/ports/provider/embedding_model.rs`
- Delete: `crates/alva-app-core/src/ports/provider/transcription_model.rs`
- Delete: `crates/alva-app-core/src/ports/provider/speech_model.rs`
- Delete: `crates/alva-app-core/src/ports/provider/image_model.rs`
- Delete: `crates/alva-app-core/src/ports/provider/video_model.rs`
- Delete: `crates/alva-app-core/src/ports/provider/reranking_model.rs`
- Delete: `crates/alva-app-core/src/ports/provider/middleware.rs`
- Modify: `crates/alva-app-core/src/ports/provider/types.rs` (delete or empty)
- Modify: `crates/alva-app-core/src/ports/provider/mod.rs`
- Modify: `crates/alva-app-core/src/lib.rs`

---

## Task 1: Add 7 Model Capability Traits to alva-types

**Files:**
- Create: `crates/alva-types/src/embedding.rs`
- Create: `crates/alva-types/src/transcription.rs`
- Create: `crates/alva-types/src/speech.rs`
- Create: `crates/alva-types/src/image.rs`
- Create: `crates/alva-types/src/video.rs`
- Create: `crates/alva-types/src/reranking.rs`
- Create: `crates/alva-types/src/moderation.rs`
- Modify: `crates/alva-types/src/lib.rs`

- [ ] **Step 1: Create `embedding.rs`**

Create `crates/alva-types/src/embedding.rs`:

```rust
use async_trait::async_trait;
use serde::{Deserialize, Serialize};

use crate::error::AgentError;

/// Interface for text embedding models.
///
/// Maps text to vectors (points in n-dimensional space). Similar texts
/// produce vectors that are close together. Pass one text for a query
/// embedding, or many for document embeddings.
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

- [ ] **Step 2: Create `transcription.rs`**

Create `crates/alva-types/src/transcription.rs`:

```rust
use async_trait::async_trait;
use serde::{Deserialize, Serialize};

use crate::error::AgentError;

/// Interface for audio transcription (ASR) models.
///
/// Converts audio bytes to text with optional time-aligned segments.
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
    /// IANA media type of the audio (e.g. "audio/wav", "audio/mp3").
    pub media_type: String,
    /// BCP-47 language hint (e.g. "en", "zh").
    pub language: Option<String>,
    /// Context prompt to guide recognition.
    pub prompt: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TranscriptionResult {
    pub text: String,
    /// Time-aligned segments, if the model supports them.
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

- [ ] **Step 3: Create `speech.rs`**

Create `crates/alva-types/src/speech.rs`:

```rust
use async_trait::async_trait;
use serde::{Deserialize, Serialize};

use crate::error::AgentError;

/// Interface for text-to-speech (TTS) models.
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
    /// IANA media type for output (e.g. "audio/mp3", "audio/opus").
    pub output_format: Option<String>,
    pub speed: Option<f32>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SpeechResult {
    pub audio: Vec<u8>,
    /// IANA media type of the audio.
    pub media_type: String,
}
```

- [ ] **Step 4: Create `image.rs`**

Create `crates/alva-types/src/image.rs`:

```rust
use async_trait::async_trait;
use serde::{Deserialize, Serialize};

use crate::error::AgentError;

/// Interface for image generation and editing models.
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
    /// e.g. "1024x1024"
    pub size: Option<String>,
    /// e.g. "16:9"
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

- [ ] **Step 5: Create `video.rs`**

Create `crates/alva-types/src/video.rs`:

```rust
use async_trait::async_trait;
use serde::{Deserialize, Serialize};

use crate::error::AgentError;

/// Interface for video generation models.
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
    /// e.g. "1920x1080"
    pub size: Option<String>,
    /// e.g. "16:9"
    pub aspect_ratio: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VideoResult {
    pub videos: Vec<VideoData>,
    /// IANA media type (e.g. "video/mp4").
    pub media_type: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum VideoData {
    Base64(String),
    Bytes(Vec<u8>),
    Url(String),
}
```

- [ ] **Step 6: Create `reranking.rs`**

Create `crates/alva-types/src/reranking.rs`:

```rust
use async_trait::async_trait;
use serde::{Deserialize, Serialize};

use crate::error::AgentError;

/// Interface for reranking models.
///
/// Given a query and a list of documents, returns relevance scores.
/// Caller keeps the original documents slice and uses `index` to look up.
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
    /// Index into the original documents slice.
    pub index: usize,
    pub relevance_score: f64,
}
```

- [ ] **Step 7: Create `moderation.rs`**

Create `crates/alva-types/src/moderation.rs`:

```rust
use async_trait::async_trait;
use serde::{Deserialize, Serialize};

use crate::error::AgentError;

/// Interface for content moderation / safety classification models.
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

- [ ] **Step 8: Update `lib.rs` with new modules and re-exports**

In `crates/alva-types/src/lib.rs`, add module declarations and re-exports. The file currently contains:

```rust
pub mod cancel;
pub mod content;
pub mod error;
pub mod message;
pub mod model;
pub mod stream;
pub mod tool;
```

Add after `pub mod tool;`:

```rust
pub mod embedding;
pub mod transcription;
pub mod speech;
pub mod image;
pub mod video;
pub mod reranking;
pub mod moderation;
```

Add after the existing `pub use tool::...` line:

```rust
pub use embedding::{EmbeddingModel, EmbeddingResult, EmbeddingUsage};
pub use transcription::{
    TranscriptionConfig, TranscriptionModel, TranscriptionResult, TranscriptionSegment,
};
pub use speech::{SpeechConfig, SpeechModel, SpeechResult};
pub use image::{ImageConfig, ImageData, ImageEditConfig, ImageModel, ImageResult};
pub use video::{VideoConfig, VideoData, VideoModel, VideoResult};
pub use reranking::{RankEntry, RerankConfig, RerankResult, RerankingModel};
pub use moderation::{ModerationCategory, ModerationEntry, ModerationModel, ModerationResult};
```

- [ ] **Step 9: Build alva-types to verify**

Run: `cd /Users/smallraw/Development/QuincyWork/srow-agent && cargo build -p alva-types`

Expected: Compiles with no errors.

- [ ] **Step 10: Run all alva-types tests**

Run: `cd /Users/smallraw/Development/QuincyWork/srow-agent && cargo test -p alva-types`

Expected: All existing tests pass. (New traits have no tests — they are pure trait definitions.)

- [ ] **Step 11: Commit**

```bash
git add crates/alva-types/src/embedding.rs crates/alva-types/src/transcription.rs crates/alva-types/src/speech.rs crates/alva-types/src/image.rs crates/alva-types/src/video.rs crates/alva-types/src/reranking.rs crates/alva-types/src/moderation.rs crates/alva-types/src/lib.rs
git commit -m "feat(alva-types): add 7 model capability traits

Adds EmbeddingModel, TranscriptionModel, SpeechModel, ImageModel (with
edit), VideoModel, RerankingModel, and ModerationModel traits alongside
the existing LanguageModel. All traits follow the same minimal pattern:
model_id() + one async capability method + Config/Result structs."
```

---

## Task 2: Extend Provider Trait and ProviderRegistry

**Files:**
- Modify: `crates/alva-app-core/src/ports/provider/provider_registry.rs`

- [ ] **Step 1: Add alva-types imports**

In `crates/alva-app-core/src/ports/provider/provider_registry.rs`, update the `use alva_types::LanguageModel;` import to:

```rust
use alva_types::{
    EmbeddingModel, ImageModel, LanguageModel, ModerationModel, RerankingModel, SpeechModel,
    TranscriptionModel, VideoModel,
};
```

- [ ] **Step 2: Add 7 optional methods to Provider trait**

After the existing `language_model()` method in the `Provider` trait, add:

```rust
    fn embedding_model(
        &self,
        _model_id: &str,
    ) -> Result<Arc<dyn EmbeddingModel>, ProviderError> {
        Err(ProviderError::UnsupportedFunctionality("embedding".into()))
    }

    fn transcription_model(
        &self,
        _model_id: &str,
    ) -> Result<Arc<dyn TranscriptionModel>, ProviderError> {
        Err(ProviderError::UnsupportedFunctionality(
            "transcription".into(),
        ))
    }

    fn speech_model(
        &self,
        _model_id: &str,
    ) -> Result<Arc<dyn SpeechModel>, ProviderError> {
        Err(ProviderError::UnsupportedFunctionality("speech".into()))
    }

    fn image_model(
        &self,
        _model_id: &str,
    ) -> Result<Arc<dyn ImageModel>, ProviderError> {
        Err(ProviderError::UnsupportedFunctionality("image".into()))
    }

    fn video_model(
        &self,
        _model_id: &str,
    ) -> Result<Arc<dyn VideoModel>, ProviderError> {
        Err(ProviderError::UnsupportedFunctionality("video".into()))
    }

    fn reranking_model(
        &self,
        _model_id: &str,
    ) -> Result<Arc<dyn RerankingModel>, ProviderError> {
        Err(ProviderError::UnsupportedFunctionality("reranking".into()))
    }

    fn moderation_model(
        &self,
        _model_id: &str,
    ) -> Result<Arc<dyn ModerationModel>, ProviderError> {
        Err(ProviderError::UnsupportedFunctionality(
            "moderation".into(),
        ))
    }
```

- [ ] **Step 3: Add 7 shorthand methods to ProviderRegistry**

After the existing `language_model()` method in `impl ProviderRegistry`, add 7 methods following the same pattern. Example for embedding (repeat for all 7):

```rust
    pub fn embedding_model(
        &self,
        provider_id: &str,
        model_id: &str,
    ) -> Result<Arc<dyn EmbeddingModel>, ProviderError> {
        let provider = self.providers.get(provider_id).ok_or_else(|| {
            ProviderError::NoSuchModel {
                model_id: format!("{provider_id}:{model_id}"),
                model_type: "embedding".to_string(),
            }
        })?;
        provider.embedding_model(model_id)
    }

    pub fn transcription_model(
        &self,
        provider_id: &str,
        model_id: &str,
    ) -> Result<Arc<dyn TranscriptionModel>, ProviderError> {
        let provider = self.providers.get(provider_id).ok_or_else(|| {
            ProviderError::NoSuchModel {
                model_id: format!("{provider_id}:{model_id}"),
                model_type: "transcription".to_string(),
            }
        })?;
        provider.transcription_model(model_id)
    }

    pub fn speech_model(
        &self,
        provider_id: &str,
        model_id: &str,
    ) -> Result<Arc<dyn SpeechModel>, ProviderError> {
        let provider = self.providers.get(provider_id).ok_or_else(|| {
            ProviderError::NoSuchModel {
                model_id: format!("{provider_id}:{model_id}"),
                model_type: "speech".to_string(),
            }
        })?;
        provider.speech_model(model_id)
    }

    pub fn image_model(
        &self,
        provider_id: &str,
        model_id: &str,
    ) -> Result<Arc<dyn ImageModel>, ProviderError> {
        let provider = self.providers.get(provider_id).ok_or_else(|| {
            ProviderError::NoSuchModel {
                model_id: format!("{provider_id}:{model_id}"),
                model_type: "image".to_string(),
            }
        })?;
        provider.image_model(model_id)
    }

    pub fn video_model(
        &self,
        provider_id: &str,
        model_id: &str,
    ) -> Result<Arc<dyn VideoModel>, ProviderError> {
        let provider = self.providers.get(provider_id).ok_or_else(|| {
            ProviderError::NoSuchModel {
                model_id: format!("{provider_id}:{model_id}"),
                model_type: "video".to_string(),
            }
        })?;
        provider.video_model(model_id)
    }

    pub fn reranking_model(
        &self,
        provider_id: &str,
        model_id: &str,
    ) -> Result<Arc<dyn RerankingModel>, ProviderError> {
        let provider = self.providers.get(provider_id).ok_or_else(|| {
            ProviderError::NoSuchModel {
                model_id: format!("{provider_id}:{model_id}"),
                model_type: "reranking".to_string(),
            }
        })?;
        provider.reranking_model(model_id)
    }

    pub fn moderation_model(
        &self,
        provider_id: &str,
        model_id: &str,
    ) -> Result<Arc<dyn ModerationModel>, ProviderError> {
        let provider = self.providers.get(provider_id).ok_or_else(|| {
            ProviderError::NoSuchModel {
                model_id: format!("{provider_id}:{model_id}"),
                model_type: "moderation".to_string(),
            }
        })?;
        provider.moderation_model(model_id)
    }
```

- [ ] **Step 4: Run alva-app-core tests**

Run: `cd /Users/smallraw/Development/QuincyWork/srow-agent && cargo test -p alva-app-core`

Expected: All tests pass. Existing `MockProvider` only implements `language_model()` — the 7 new methods have defaults, so no breakage.

- [ ] **Step 5: Run workspace check**

Run: `cd /Users/smallraw/Development/QuincyWork/srow-agent && cargo check 2>&1 | tail -5`

Expected: No new errors.

- [ ] **Step 6: Commit**

```bash
git add crates/alva-app-core/src/ports/provider/provider_registry.rs
git commit -m "feat(alva-app-core): extend Provider trait with 7 model capability methods

Provider now supports embedding_model(), transcription_model(),
speech_model(), image_model(), video_model(), reranking_model(), and
moderation_model(). All default to UnsupportedFunctionality so existing
Provider impls are unaffected. ProviderRegistry gains matching shorthand
methods."
```

---

## Task 3: Delete Old V4 Model Files and Clean Up

**Files:**
- Delete: `crates/alva-app-core/src/ports/provider/embedding_model.rs`
- Delete: `crates/alva-app-core/src/ports/provider/transcription_model.rs`
- Delete: `crates/alva-app-core/src/ports/provider/speech_model.rs`
- Delete: `crates/alva-app-core/src/ports/provider/image_model.rs`
- Delete: `crates/alva-app-core/src/ports/provider/video_model.rs`
- Delete: `crates/alva-app-core/src/ports/provider/reranking_model.rs`
- Delete: `crates/alva-app-core/src/ports/provider/middleware.rs`
- Modify: `crates/alva-app-core/src/ports/provider/types.rs`
- Modify: `crates/alva-app-core/src/ports/provider/mod.rs`
- Modify: `crates/alva-app-core/src/lib.rs`

- [ ] **Step 1: Delete the 7 old model files + middleware**

```bash
cd /Users/smallraw/Development/QuincyWork/srow-agent
rm crates/alva-app-core/src/ports/provider/embedding_model.rs
rm crates/alva-app-core/src/ports/provider/transcription_model.rs
rm crates/alva-app-core/src/ports/provider/speech_model.rs
rm crates/alva-app-core/src/ports/provider/image_model.rs
rm crates/alva-app-core/src/ports/provider/video_model.rs
rm crates/alva-app-core/src/ports/provider/reranking_model.rs
rm crates/alva-app-core/src/ports/provider/middleware.rs
```

- [ ] **Step 2: Empty `types.rs`**

Replace `crates/alva-app-core/src/ports/provider/types.rs` with:

```rust
// V4 provider types (ProviderOptions, ProviderMetadata, ProviderHeaders,
// ProviderWarning) have been removed. Model capability traits are now
// defined in alva-types. This module is kept as a placeholder in case
// provider-level shared types are needed in the future.
```

- [ ] **Step 3: Update `mod.rs`**

Replace `crates/alva-app-core/src/ports/provider/mod.rs` with:

```rust
pub mod types;
pub mod errors;
pub mod provider_registry;

pub use errors::*;
pub use provider_registry::{Provider, ProviderRegistry};
```

- [ ] **Step 4: Check `lib.rs` for stale re-exports**

In `crates/alva-app-core/src/lib.rs`, check for any re-exports of the deleted V4 types (`EmbeddingModel`, `ImageModel`, `SpeechModel`, `TranscriptionModel`, `VideoModel`, `RerankingModel` from `ports::provider`). If any exist, remove them. The current `lib.rs` only re-exports `Provider` and `ProviderRegistry` from the provider module, so no changes should be needed.

- [ ] **Step 5: Build workspace**

Run: `cd /Users/smallraw/Development/QuincyWork/srow-agent && cargo build 2>&1 | head -30`

Expected: Compiles. If any file still references the deleted V4 types, fix the remaining imports.

- [ ] **Step 6: Run all tests**

Run: `cd /Users/smallraw/Development/QuincyWork/srow-agent && cargo test 2>&1 | tail -20`

Expected: All tests pass.

- [ ] **Step 7: Commit**

```bash
git add -A crates/alva-app-core/src/ports/provider/
git commit -m "refactor(alva-app-core): delete old V4 model traits and provider types

Removes embedding_model.rs, transcription_model.rs, speech_model.rs,
image_model.rs, video_model.rs, reranking_model.rs, middleware.rs, and
the ProviderOptions/ProviderMetadata/ProviderHeaders/ProviderWarning
types from types.rs. All model capability traits are now defined in
alva-types."
```
