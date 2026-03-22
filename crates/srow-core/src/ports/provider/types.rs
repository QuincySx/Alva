// INPUT:  std::collections, serde, serde_json
// OUTPUT: ProviderHeaders, ProviderMetadata, ProviderOptions, ProviderWarning
// POS:    Foundational type aliases and warning enum for the Provider V4 type system.
//         Kept during migration because embedding/image/speech/video/reranking models depend on these.
use std::collections::HashMap;
use serde::{Deserialize, Serialize};

/// HTTP headers to send with provider API requests.
pub type ProviderHeaders = HashMap<String, String>;

/// Provider-specific metadata returned with responses.
pub type ProviderMetadata = HashMap<String, serde_json::Map<String, serde_json::Value>>;

/// Provider-specific options passed with requests.
pub type ProviderOptions = HashMap<String, serde_json::Map<String, serde_json::Value>>;

/// Warnings generated during provider operations.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "kebab-case")]
pub enum ProviderWarning {
    Unsupported {
        feature: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        details: Option<String>,
    },
    Compatibility {
        feature: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        details: Option<String>,
    },
    Other { message: String },
}
