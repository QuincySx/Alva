// INPUT:  async_trait, serde, crate::base::error::AgentError
// OUTPUT: pub trait TranscriptionModel, pub struct TranscriptionConfig, pub struct TranscriptionResult, pub struct TranscriptionSegment
// POS:    Trait and wire types for audio transcription (speech-to-text) models.
use async_trait::async_trait;
use serde::{Deserialize, Serialize};

use crate::base::error::AgentError;

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
