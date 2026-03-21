// INPUT:  async_trait, super::types, super::errors
// OUTPUT: SpeechModel (trait), SpeechCallOptions, SpeechResult
// POS:    Speech synthesis (TTS) model trait and associated types for Provider V4.
use async_trait::async_trait;
use super::types::*;
use super::errors::ProviderError;

/// Abstract speech synthesis model interface (Provider V4 specification).
#[async_trait]
pub trait SpeechModel: Send + Sync {
    /// The specification version this model implements.
    fn specification_version(&self) -> &str {
        "v4"
    }

    /// The provider identifier.
    fn provider(&self) -> &str;

    /// The model identifier (e.g. "tts-1").
    fn model_id(&self) -> &str;

    /// Generate speech audio from text.
    async fn do_generate(
        &self,
        options: SpeechCallOptions,
    ) -> Result<SpeechResult, ProviderError>;
}

/// Options for a speech synthesis call.
pub struct SpeechCallOptions {
    pub text: String,
    pub voice: Option<String>,
    pub output_format: Option<String>,
    pub speed: Option<f32>,
    pub provider_options: Option<ProviderOptions>,
    pub headers: Option<ProviderHeaders>,
}

/// The result of a speech synthesis call.
pub struct SpeechResult {
    /// Raw audio bytes.
    pub audio: Vec<u8>,
    /// MIME type of the audio (e.g. "audio/mpeg").
    pub media_type: String,
    pub provider_metadata: Option<ProviderMetadata>,
    pub warnings: Vec<ProviderWarning>,
}
