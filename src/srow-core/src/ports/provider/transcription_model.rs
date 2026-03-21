// INPUT:  async_trait, super::types, super::errors
// OUTPUT: TranscriptionModel (trait), TranscriptionCallOptions, TranscriptionResult
// POS:    Audio transcription (STT) model trait and associated types for Provider V4.
use async_trait::async_trait;
use super::types::*;
use super::errors::ProviderError;

/// Abstract audio transcription model interface (Provider V4 specification).
#[async_trait]
pub trait TranscriptionModel: Send + Sync {
    /// The specification version this model implements.
    fn specification_version(&self) -> &str {
        "v4"
    }

    /// The provider identifier.
    fn provider(&self) -> &str;

    /// The model identifier (e.g. "whisper-1").
    fn model_id(&self) -> &str;

    /// Transcribe audio to text.
    async fn do_transcribe(
        &self,
        options: TranscriptionCallOptions,
    ) -> Result<TranscriptionResult, ProviderError>;
}

/// Options for a transcription call.
pub struct TranscriptionCallOptions {
    /// Raw audio bytes.
    pub audio: Vec<u8>,
    /// MIME type of the audio (e.g. "audio/wav").
    pub media_type: String,
    pub language: Option<String>,
    pub prompt: Option<String>,
    pub provider_options: Option<ProviderOptions>,
    pub headers: Option<ProviderHeaders>,
}

/// The result of a transcription call.
pub struct TranscriptionResult {
    pub text: String,
    pub segments: Option<Vec<TranscriptionSegment>>,
    pub language: Option<String>,
    pub duration_seconds: Option<f64>,
    pub provider_metadata: Option<ProviderMetadata>,
    pub warnings: Vec<ProviderWarning>,
}

/// A time-aligned segment of transcribed text.
pub struct TranscriptionSegment {
    pub text: String,
    pub start_seconds: f64,
    pub end_seconds: f64,
}
