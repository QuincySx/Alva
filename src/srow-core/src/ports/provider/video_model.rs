// INPUT:  async_trait, super::types, super::errors
// OUTPUT: VideoModel (trait), VideoCallOptions, VideoResult
// POS:    Video generation model trait and associated types for Provider V4.
use async_trait::async_trait;
use super::types::*;
use super::errors::ProviderError;

/// Abstract video generation model interface (Provider V4 specification).
#[async_trait]
pub trait VideoModel: Send + Sync {
    /// The specification version this model implements.
    fn specification_version(&self) -> &str {
        "v4"
    }

    /// The provider identifier.
    fn provider(&self) -> &str;

    /// The model identifier (e.g. "sora").
    fn model_id(&self) -> &str;

    /// Generate a video from a prompt.
    async fn do_generate(
        &self,
        options: VideoCallOptions,
    ) -> Result<VideoResult, ProviderError>;
}

/// Options for a video generation call.
pub struct VideoCallOptions {
    pub prompt: String,
    pub duration_seconds: Option<f64>,
    pub size: Option<String>,
    pub aspect_ratio: Option<String>,
    pub provider_options: Option<ProviderOptions>,
    pub headers: Option<ProviderHeaders>,
}

/// The result of a video generation call.
pub struct VideoResult {
    /// Raw video bytes.
    pub video: Vec<u8>,
    /// MIME type of the video (e.g. "video/mp4").
    pub media_type: String,
    pub provider_metadata: Option<ProviderMetadata>,
    pub warnings: Vec<ProviderWarning>,
}
