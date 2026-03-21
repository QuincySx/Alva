// INPUT:  async_trait, super::types, super::errors
// OUTPUT: ImageModel (trait), ImageCallOptions, ImageResult
// POS:    Image generation model trait and associated types for Provider V4.
use async_trait::async_trait;
use super::types::*;
use super::errors::ProviderError;

/// Abstract image generation model interface (Provider V4 specification).
#[async_trait]
pub trait ImageModel: Send + Sync {
    /// The specification version this model implements.
    fn specification_version(&self) -> &str {
        "v4"
    }

    /// The provider identifier.
    fn provider(&self) -> &str;

    /// The model identifier (e.g. "dall-e-3").
    fn model_id(&self) -> &str;

    /// Generate images from a prompt.
    async fn do_generate(
        &self,
        options: ImageCallOptions,
    ) -> Result<ImageResult, ProviderError>;
}

/// Options for an image generation call.
pub struct ImageCallOptions {
    pub prompt: String,
    pub n: Option<u32>,
    pub size: Option<String>,
    pub aspect_ratio: Option<String>,
    pub style: Option<String>,
    pub provider_options: Option<ProviderOptions>,
    pub headers: Option<ProviderHeaders>,
}

/// The result of an image generation call.
pub struct ImageResult {
    pub images: Vec<GeneratedImage>,
    pub provider_metadata: Option<ProviderMetadata>,
    pub warnings: Vec<ProviderWarning>,
}

/// A single generated image.
pub struct GeneratedImage {
    /// Base64-encoded image data.
    pub data: String,
    /// MIME type of the image (e.g. "image/png").
    pub media_type: String,
}
