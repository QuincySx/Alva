// INPUT:  async_trait, serde, crate::base::error::AgentError
// OUTPUT: pub trait ImageModel, pub struct ImageConfig, pub struct ImageEditConfig, pub struct ImageResult, pub enum ImageData
// POS:    Trait and wire types for image generation and editing models.
use async_trait::async_trait;
use serde::{Deserialize, Serialize};

use crate::base::error::AgentError;

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
