// Migrated to agent_types::Message.
// This module is kept temporarily for ImageSource which has no equivalent in agent-types yet.

use serde::{Deserialize, Serialize};

/// Image source type — retained until agent-types adds Image support with source metadata.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ImageSource {
    Base64,
    Url,
}
