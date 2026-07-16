// INPUT:  crate::{Message, ModelConfig, StreamEvent, ToolDefinition}, serde
// OUTPUT: LLM proxy ABI constants plus LlmProxyRequest and LlmProxyResponse DTOs
// POS:    Versioned, size-bounded JSON contract shared by WASIp1 guests and native hosts.

use serde::{Deserialize, Serialize};

use crate::{Message, ModelConfig, StreamEvent, ToolDefinition};

/// Version of the blocking `alva:host/llm::llm_complete` JSON contract.
pub const LLM_PROXY_ABI_VERSION: u32 = 1;
/// Maximum serialized request accepted across the guest-to-host boundary.
pub const MAX_LLM_PROXY_REQUEST_BYTES: usize = 4 * 1024 * 1024;
/// Maximum serialized response accepted across the host-to-guest boundary.
pub const MAX_LLM_PROXY_RESPONSE_BYTES: usize = 16 * 1024 * 1024;

/// Owned request DTO so the exact same type is serialized by the guest and
/// deserialized by the host.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LlmProxyRequest {
    pub version: u32,
    pub messages: Vec<Message>,
    pub tools: Vec<ToolDefinition>,
    pub config: ModelConfig,
}

impl LlmProxyRequest {
    pub fn new(messages: Vec<Message>, tools: Vec<ToolDefinition>, config: ModelConfig) -> Self {
        Self {
            version: LLM_PROXY_ABI_VERSION,
            messages,
            tools,
            config,
        }
    }

    pub fn has_supported_version(&self) -> bool {
        self.version == LLM_PROXY_ABI_VERSION
    }
}

/// Owned response DTO returned through the packed pointer/length ABI.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LlmProxyResponse {
    pub version: u32,
    pub events: Vec<StreamEvent>,
}

impl LlmProxyResponse {
    pub fn new(events: Vec<StreamEvent>) -> Self {
        Self {
            version: LLM_PROXY_ABI_VERSION,
            events,
        }
    }

    pub fn has_supported_version(&self) -> bool {
        self.version == LLM_PROXY_ABI_VERSION
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn request_and_response_carry_the_current_version() {
        let request = LlmProxyRequest::new(Vec::new(), Vec::new(), ModelConfig::default());
        let response = LlmProxyResponse::new(vec![StreamEvent::Done]);
        assert_eq!(request.version, LLM_PROXY_ABI_VERSION);
        assert_eq!(response.version, LLM_PROXY_ABI_VERSION);
        assert!(request.has_supported_version());
        assert!(response.has_supported_version());
    }

    #[test]
    fn stale_versions_are_rejected_by_both_sides() {
        let mut request = LlmProxyRequest::new(Vec::new(), Vec::new(), ModelConfig::default());
        let mut response = LlmProxyResponse::new(Vec::new());
        request.version += 1;
        response.version += 1;
        assert!(!request.has_supported_version());
        assert!(!response.has_supported_version());
    }

    #[test]
    fn request_wire_shape_has_no_credential_channel() {
        let request = LlmProxyRequest::new(Vec::new(), Vec::new(), ModelConfig::default());
        let json = serde_json::to_value(request).unwrap();
        let object = json.as_object().unwrap();
        assert_eq!(object.len(), 4);
        for field in ["version", "messages", "tools", "config"] {
            assert!(object.contains_key(field));
        }
        assert!(json.get("api_key").is_none());
        assert!(json.get("headers").is_none());
    }
}
