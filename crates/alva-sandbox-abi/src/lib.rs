// INPUT:  base64, serde
// OUTPUT: FETCH_PROXY_ABI_VERSION, fetch size limits, FetchHeader, FetchRequest, FetchResponse, FetchProxyResult
// POS:    Dependency-free versioned JSON contract shared by untrusted WASIp1 guests and the native sandbox host.

use serde::{Deserialize, Serialize};

/// Version of `alva:host/http::fetch(req_ptr, req_len)` JSON messages.
pub const FETCH_PROXY_ABI_VERSION: u32 = 1;
/// Maximum serialized request crossing from guest to host.
pub const MAX_FETCH_PROXY_REQUEST_BYTES: usize = 4 * 1024 * 1024;
/// Maximum request body accepted before any network operation.
pub const MAX_FETCH_REQUEST_BODY_BYTES: usize = 1024 * 1024;
/// Maximum decoded response body read by the host.
pub const MAX_FETCH_RESPONSE_BODY_BYTES: usize = 4 * 1024 * 1024;
/// Maximum serialized result allocated in guest linear memory.
pub const MAX_FETCH_PROXY_RESPONSE_BYTES: usize = 16 * 1024 * 1024;

/// One HTTP header. A vector preserves duplicates such as `set-cookie`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FetchHeader {
    pub name: String,
    pub value: String,
}

/// Guest-to-host fetch request.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FetchRequest {
    pub version: u32,
    pub method: String,
    pub url: String,
    pub headers: Vec<FetchHeader>,
    #[serde(with = "base64_body")]
    pub body: Vec<u8>,
}

impl FetchRequest {
    pub fn new(
        method: impl Into<String>,
        url: impl Into<String>,
        headers: Vec<FetchHeader>,
        body: Vec<u8>,
    ) -> Self {
        Self {
            version: FETCH_PROXY_ABI_VERSION,
            method: method.into(),
            url: url.into(),
            headers,
            body,
        }
    }

    pub fn has_supported_version(&self) -> bool {
        self.version == FETCH_PROXY_ABI_VERSION
    }
}

/// Successful host-to-guest HTTP response.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FetchResponse {
    pub version: u32,
    pub status: u16,
    pub headers: Vec<FetchHeader>,
    #[serde(with = "base64_body")]
    pub body: Vec<u8>,
}

impl FetchResponse {
    pub fn new(status: u16, headers: Vec<FetchHeader>, body: Vec<u8>) -> Self {
        Self {
            version: FETCH_PROXY_ABI_VERSION,
            status,
            headers,
            body,
        }
    }

    pub fn has_supported_version(&self) -> bool {
        self.version == FETCH_PROXY_ABI_VERSION
    }
}

/// Versioned result envelope. Policy and transport failures are data so the
/// guest can throw a catchable JavaScript exception; malformed ABI traffic
/// remains a host trap and never enters this envelope.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FetchProxyResult {
    pub version: u32,
    pub response: Option<FetchResponse>,
    pub error: Option<String>,
}

impl FetchProxyResult {
    pub fn success(response: FetchResponse) -> Self {
        Self {
            version: FETCH_PROXY_ABI_VERSION,
            response: Some(response),
            error: None,
        }
    }

    pub fn failure(error: impl Into<String>) -> Self {
        Self {
            version: FETCH_PROXY_ABI_VERSION,
            response: None,
            error: Some(error.into()),
        }
    }

    pub fn has_supported_version(&self) -> bool {
        self.version == FETCH_PROXY_ABI_VERSION
    }
}

mod base64_body {
    use base64::engine::general_purpose::STANDARD;
    use base64::Engine;
    use serde::{Deserialize, Deserializer, Serializer};

    pub fn serialize<S>(body: &[u8], serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_str(&STANDARD.encode(body))
    }

    pub fn deserialize<'de, D>(deserializer: D) -> Result<Vec<u8>, D::Error>
    where
        D: Deserializer<'de>,
    {
        let encoded = String::deserialize(deserializer)?;
        STANDARD.decode(encoded).map_err(serde::de::Error::custom)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn messages_carry_the_current_version() {
        let request = FetchRequest::new("GET", "https://example.com", Vec::new(), Vec::new());
        let response = FetchResponse::new(200, Vec::new(), b"ok".to_vec());
        let result = FetchProxyResult::success(response.clone());
        assert!(request.has_supported_version());
        assert!(response.has_supported_version());
        assert!(result.has_supported_version());
        assert_eq!(result.response, Some(response));
        assert!(result.error.is_none());
    }

    #[test]
    fn failure_has_no_ambiguous_success_payload() {
        let result = FetchProxyResult::failure("blocked");
        assert!(result.response.is_none());
        assert_eq!(result.error.as_deref(), Some("blocked"));
    }

    #[test]
    fn wire_shape_contains_no_policy_or_credentials() {
        let value = serde_json::to_value(FetchRequest::new(
            "GET",
            "https://example.com",
            Vec::new(),
            Vec::new(),
        ))
        .unwrap();
        let object = value.as_object().unwrap();
        assert_eq!(object.len(), 5);
        assert!(value.get("allowed_domains").is_none());
        assert!(value.get("api_key").is_none());
    }

    #[test]
    fn bodies_use_bounded_base64_wire_encoding() {
        let request =
            FetchRequest::new("POST", "https://example.com", Vec::new(), vec![0, 127, 255]);
        let json = serde_json::to_value(&request).unwrap();
        assert_eq!(json["body"], "AH//");
        assert_eq!(
            serde_json::from_value::<FetchRequest>(json).unwrap(),
            request
        );
    }
}
