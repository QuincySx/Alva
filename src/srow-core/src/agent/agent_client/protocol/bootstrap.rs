use serde::{Deserialize, Serialize};

/// Sandbox level enumeration.
/// Matches Wukong's sandbox_level field.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SandboxLevel {
    /// No sandbox (development)
    None,
    /// Network sandbox (allow files, block network)
    Network,
    /// Full sandbox (macOS sandbox-exec, Sub-7)
    Full,
}

impl Default for SandboxLevel {
    fn default() -> Self {
        Self::None
    }
}

/// Model configuration -- tells external Agent which model to use.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModelConfig {
    pub provider: String,
    pub model: String,
    pub api_key: String,
    /// Override default base_url (proxy / private deployment)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub base_url: Option<String>,
    /// Max token count
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_tokens: Option<u32>,
}

/// Complete Bootstrap Payload.
/// Written to stdin immediately after spawn (one JSON line terminated by \n).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BootstrapPayload {
    /// Working directory (absolute path)
    pub workspace: String,
    /// Allowed root paths for file operation permission checks
    pub authorized_roots: Vec<String>,
    /// Sandbox level
    #[serde(default)]
    pub sandbox_level: SandboxLevel,
    /// Model configuration (external Agent uses this to call LLM)
    pub model_config: ModelConfig,
    /// Attachment paths (optional initial context files)
    #[serde(default)]
    pub attachment_paths: Vec<String>,
    /// Srow version (for external Agent compatibility checks)
    #[serde(default = "default_version")]
    pub srow_version: String,
}

fn default_version() -> String {
    env!("CARGO_PKG_VERSION").to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_sandbox_level_serde() {
        let json = r#""none""#;
        let level: SandboxLevel = serde_json::from_str(json).unwrap();
        assert_eq!(level, SandboxLevel::None);

        let json = r#""network""#;
        let level: SandboxLevel = serde_json::from_str(json).unwrap();
        assert_eq!(level, SandboxLevel::Network);

        let json = r#""full""#;
        let level: SandboxLevel = serde_json::from_str(json).unwrap();
        assert_eq!(level, SandboxLevel::Full);
    }

    #[test]
    fn test_bootstrap_payload_roundtrip() {
        let payload = BootstrapPayload {
            workspace: "/tmp/test".to_string(),
            authorized_roots: vec!["/tmp/test".to_string()],
            sandbox_level: SandboxLevel::None,
            model_config: ModelConfig {
                provider: "anthropic".to_string(),
                model: "claude-opus-4-5".to_string(),
                api_key: "sk-test".to_string(),
                base_url: None,
                max_tokens: Some(8192),
            },
            attachment_paths: vec![],
            srow_version: "0.1.0".to_string(),
        };

        let json = serde_json::to_string(&payload).unwrap();
        let deserialized: BootstrapPayload = serde_json::from_str(&json).unwrap();
        assert_eq!(deserialized.workspace, "/tmp/test");
        assert_eq!(deserialized.model_config.provider, "anthropic");
    }
}
