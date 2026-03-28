// INPUT:  async_trait, serde, crate::base::error::AgentError
// OUTPUT: pub trait ModerationModel, pub struct ModerationResult, pub struct ModerationEntry, pub struct ModerationCategory
// POS:    Trait and result types for content moderation and safety classification models.
use async_trait::async_trait;
use serde::{Deserialize, Serialize};

use crate::base::error::AgentError;

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
