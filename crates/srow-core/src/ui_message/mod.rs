pub mod parts;
pub mod convert;

pub use parts::*;

use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct UIMessage {
    pub id: String,
    pub role: UIMessageRole,
    pub parts: Vec<UIMessagePart>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub metadata: Option<serde_json::Value>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum UIMessageRole {
    System,
    User,
    Assistant,
}
