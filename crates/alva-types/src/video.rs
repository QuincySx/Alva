// INPUT:  async_trait, serde, crate::error::AgentError
// OUTPUT: pub trait VideoModel, pub struct VideoConfig, pub struct VideoResult, pub enum VideoData
// POS:    Trait and wire types for video generation models.
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
