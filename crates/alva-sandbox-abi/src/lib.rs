// INPUT:  base64, serde
// OUTPUT: fetch/escalation/log/context ABI versions and limits, their request/result DTOs, AuditEvent, WasmEnvironmentContext
// POS:    Dependency-free versioned JSON contracts shared by untrusted WASIp1 guests and the native sandbox host.

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
/// Version of `alva:host/log::append(req_ptr, req_len)` JSON messages.
pub const LOG_PROXY_ABI_VERSION: u32 = 1;
/// Maximum serialized audit event crossing from guest to host.
pub const MAX_LOG_PROXY_REQUEST_BYTES: usize = 64 * 1024;
/// Version of `alva:host/escalation::execute(req_ptr, req_len)` JSON messages.
pub const ESCALATION_PROXY_ABI_VERSION: u32 = 1;
/// Maximum serialized escalation request crossing from guest to host.
pub const MAX_ESCALATION_PROXY_REQUEST_BYTES: usize = 64 * 1024;
/// Maximum serialized escalation result allocated in guest linear memory.
pub const MAX_ESCALATION_PROXY_RESPONSE_BYTES: usize = 16 * 1024 * 1024;
/// Shell-compatible exit code used when host policy rejects an escalation.
pub const ESCALATION_REJECTED_EXIT_CODE: i32 = 126;
/// Version of `alva:host/context::wasm_environment()` JSON messages.
pub const WASM_ENV_CONTEXT_ABI_VERSION: u32 = 1;
/// Maximum serialized environment context allocated in guest linear memory.
pub const MAX_WASM_ENV_CONTEXT_RESPONSE_BYTES: usize = 256 * 1024;

/// Host-parsed, fully expanded wasm environment skill delivered to the guest.
///
/// This DTO deliberately contains no host filesystem path: mount locations in
/// the body are guest-visible paths, while the live mount map stays in WASI
/// args and the guest's dynamic prompt preface.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WasmEnvironmentContext {
    pub version: u32,
    pub system_prompt: String,
}

impl WasmEnvironmentContext {
    pub fn new(system_prompt: impl Into<String>) -> Self {
        Self {
            version: WASM_ENV_CONTEXT_ABI_VERSION,
            system_prompt: system_prompt.into(),
        }
    }

    pub fn has_supported_version(&self) -> bool {
        self.version == WASM_ENV_CONTEXT_ABI_VERSION
    }
}

/// Guest-to-host request for execution outside the WASIp1 worker.
///
/// `cwd` is always in the guest namespace. The host must translate it through
/// the current job's grant table before policy checks or process execution.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct EscalationProxyRequest {
    pub version: u32,
    pub command: String,
    pub cwd: String,
    pub timeout_ms: u64,
}

impl EscalationProxyRequest {
    pub fn new(command: impl Into<String>, cwd: impl Into<String>, timeout_ms: u64) -> Self {
        Self {
            version: ESCALATION_PROXY_ABI_VERSION,
            command: command.into(),
            cwd: cwd.into(),
            timeout_ms,
        }
    }

    pub fn has_supported_version(&self) -> bool {
        self.version == ESCALATION_PROXY_ABI_VERSION
    }
}

/// Successful host execution result, including non-zero command exits.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct EscalationResponse {
    pub version: u32,
    pub stdout: String,
    pub stderr: String,
    pub exit_code: i32,
}

impl EscalationResponse {
    pub fn new(stdout: impl Into<String>, stderr: impl Into<String>, exit_code: i32) -> Self {
        Self {
            version: ESCALATION_PROXY_ABI_VERSION,
            stdout: stdout.into(),
            stderr: stderr.into(),
            exit_code,
        }
    }

    pub fn has_supported_version(&self) -> bool {
        self.version == ESCALATION_PROXY_ABI_VERSION
    }
}

/// Versioned escalation result envelope. Policy rejection is data rather than
/// a host trap so the worker can receive the reason and finish gracefully.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct EscalationProxyResult {
    pub version: u32,
    pub response: Option<EscalationResponse>,
    pub error: Option<String>,
}

impl EscalationProxyResult {
    pub fn success(response: EscalationResponse) -> Self {
        Self {
            version: ESCALATION_PROXY_ABI_VERSION,
            response: Some(response),
            error: None,
        }
    }

    pub fn failure(error: impl Into<String>) -> Self {
        Self {
            version: ESCALATION_PROXY_ABI_VERSION,
            response: None,
            error: Some(error.into()),
        }
    }

    pub fn has_supported_version(&self) -> bool {
        self.version == ESCALATION_PROXY_ABI_VERSION
    }
}

/// Guest-reported audit event. The host owns timestamps and persistence; the
/// open `kind` string reserves the same envelope for future elevation events.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AuditEvent {
    pub version: u32,
    pub kind: String,
    pub tool_call_id: String,
    pub tool_name: String,
    pub is_error: bool,
    pub result_summary: String,
}

impl AuditEvent {
    pub fn tool_call(
        tool_call_id: impl Into<String>,
        tool_name: impl Into<String>,
        is_error: bool,
        result_summary: impl Into<String>,
    ) -> Self {
        Self {
            version: LOG_PROXY_ABI_VERSION,
            kind: "tool_call".into(),
            tool_call_id: tool_call_id.into(),
            tool_name: tool_name.into(),
            is_error,
            result_summary: result_summary.into(),
        }
    }

    pub fn has_supported_version(&self) -> bool {
        self.version == LOG_PROXY_ABI_VERSION
    }
}

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
    fn wasm_environment_context_carries_only_versioned_prompt_text() {
        let context = WasmEnvironmentContext::new("## Skill: wasm-env");
        assert!(context.has_supported_version());
        let value = serde_json::to_value(context).unwrap();
        assert_eq!(value.as_object().unwrap().len(), 2);
        assert_eq!(value["system_prompt"], "## Skill: wasm-env");
        assert!(value.get("skill_dir").is_none());
        assert!(value.get("host_path").is_none());
    }

    #[test]
    fn escalation_messages_carry_version_and_unambiguous_envelopes() {
        let request = EscalationProxyRequest::new("cargo test --offline", "/workspace", 120_000);
        let response = EscalationResponse::new("ok", "", 0);
        let success = EscalationProxyResult::success(response.clone());
        let rejected = EscalationProxyResult::failure("headless Ask rejected once");

        assert!(request.has_supported_version());
        assert!(response.has_supported_version());
        assert!(success.has_supported_version());
        assert_eq!(success.response, Some(response));
        assert!(success.error.is_none());
        assert!(rejected.response.is_none());
        assert_eq!(
            rejected.error.as_deref(),
            Some("headless Ask rejected once")
        );
    }

    #[test]
    fn escalation_wire_contains_guest_cwd_but_no_host_policy() {
        let value = serde_json::to_value(EscalationProxyRequest::new(
            "cargo test",
            "/workspace",
            120_000,
        ))
        .unwrap();
        assert_eq!(value.as_object().unwrap().len(), 4);
        assert_eq!(value["cwd"], "/workspace");
        assert!(value.get("host_cwd").is_none());
        assert!(value.get("permission_mode").is_none());
        assert!(value.get("approved").is_none());
    }

    #[test]
    fn audit_event_carries_extensible_kind_and_current_version() {
        let event = AuditEvent::tool_call("call-1", "read_file", false, "read 12 bytes");
        assert!(event.has_supported_version());
        assert_eq!(event.kind, "tool_call");
        assert_eq!(event.tool_name, "read_file");
        assert_eq!(event.result_summary, "read 12 bytes");
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
