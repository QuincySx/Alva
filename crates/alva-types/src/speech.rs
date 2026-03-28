// INPUT:  async_trait, serde, crate::base::error::AgentError
// OUTPUT: pub trait SpeechModel, pub struct SpeechConfig, pub struct SpeechResult
// POS:    Trait and wire types for text-to-speech synthesis models.
use async_trait::async_trait;
use serde::{Deserialize, Serialize};

use crate::base::error::AgentError;

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
