// INPUT:  std::collections, serde, serde_json
// OUTPUT: ProviderHeaders, ProviderMetadata, ProviderOptions, ProviderWarning
// POS:    Foundational type aliases and warning enum for the Provider V4 type system.
use std::collections::HashMap;
use serde::{Deserialize, Serialize};

/// HTTP headers to send with provider API requests.
pub type ProviderHeaders = HashMap<String, String>;

/// Provider-specific metadata returned with responses.
/// Keyed by provider name, each containing an arbitrary JSON object.
pub type ProviderMetadata = HashMap<String, serde_json::Map<String, serde_json::Value>>;

/// Provider-specific options passed with requests.
/// Keyed by provider name, each containing an arbitrary JSON object.
pub type ProviderOptions = HashMap<String, serde_json::Map<String, serde_json::Value>>;

/// Warnings generated during provider operations.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "kebab-case")]
pub enum ProviderWarning {
    /// A requested feature is not supported by this provider.
    Unsupported {
        feature: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        details: Option<String>,
    },
    /// A requested feature is supported but may behave differently.
    Compatibility {
        feature: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        details: Option<String>,
    },
    /// A provider-specific warning.
    Other { message: String },
}
