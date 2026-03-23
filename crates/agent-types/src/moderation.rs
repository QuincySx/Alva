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
