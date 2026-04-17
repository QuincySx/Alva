//! **AEP** — Alva Extension Protocol wire types.
//!
//! JSON-RPC 2.0 messages exchanged between host and plugin subprocess
//! over newline-delimited JSON on stdio. See `docs/aep.md` for the
//! full specification; this module is the Rust side of the contract.
//!
//! ## Layout
//!
//! - **Envelope** — [`Request`], [`Response`], [`Notification`],
//!   [`RpcError`]. These mirror JSON-RPC 2.0 exactly.
//! - **Method constants** — [`methods`] collects every method name
//!   the protocol defines, so dispatch code does not string-match.
//! - **Error codes** — [`error_codes`] constants for standard and
//!   AEP-specific errors.
//! - **Payloads** — typed `*Params` and `*Result` structs for each
//!   method. Phase 1 defines the handshake + one representative event
//!   + one representative host-api call; the rest will be added as
//!   phase 2/3 wire them up.

use serde::{Deserialize, Serialize};
use serde_json::Value;

/// AEP protocol version advertised in the handshake.
///
/// Bump via semver: minor for additive changes, major for breaking.
pub const PROTOCOL_VERSION: &str = "0.1.0";

/// Literal `"2.0"` string used in every JSON-RPC envelope.
pub const JSONRPC_VERSION: &str = "2.0";

/// JSON-RPC request / response id.
///
/// AEP v0.1 always uses string ids of the form `"h-<seq>"` (host
/// originated) or `"p-<seq>"` (plugin originated) to avoid id
/// collision on the bidirectional channel. Kept as `String` rather
/// than a struct so we can widen to numeric ids later without a
/// breaking change.
pub type RequestId = String;

// ===========================================================
// JSON-RPC 2.0 envelope
// ===========================================================

/// JSON-RPC 2.0 request (has an id, expects a response).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Request {
    pub jsonrpc: String,
    pub id: RequestId,
    pub method: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub params: Option<Value>,
}

impl Request {
    pub fn new(id: impl Into<RequestId>, method: impl Into<String>, params: Option<Value>) -> Self {
        Self {
            jsonrpc: JSONRPC_VERSION.to_string(),
            id: id.into(),
            method: method.into(),
            params,
        }
    }
}

/// JSON-RPC 2.0 notification (no id, no response expected).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Notification {
    pub jsonrpc: String,
    pub method: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub params: Option<Value>,
}

impl Notification {
    pub fn new(method: impl Into<String>, params: Option<Value>) -> Self {
        Self {
            jsonrpc: JSONRPC_VERSION.to_string(),
            method: method.into(),
            params,
        }
    }
}

/// JSON-RPC 2.0 response. Exactly one of `result` / `error` is set.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Response {
    pub jsonrpc: String,
    pub id: RequestId,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub result: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<RpcError>,
}

impl Response {
    pub fn ok(id: impl Into<RequestId>, result: Value) -> Self {
        Self {
            jsonrpc: JSONRPC_VERSION.to_string(),
            id: id.into(),
            result: Some(result),
            error: None,
        }
    }

    pub fn err(id: impl Into<RequestId>, error: RpcError) -> Self {
        Self {
            jsonrpc: JSONRPC_VERSION.to_string(),
            id: id.into(),
            result: None,
            error: Some(error),
        }
    }
}

/// JSON-RPC 2.0 error object.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RpcError {
    pub code: i32,
    pub message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub data: Option<Value>,
}

impl RpcError {
    pub fn new(code: i32, message: impl Into<String>) -> Self {
        Self { code, message: message.into(), data: None }
    }
}

// ===========================================================
// Method name constants
// ===========================================================

/// Every method name AEP defines, grouped by flow direction.
///
/// Dispatch code matches against these constants rather than bare
/// string literals — typos become compile errors and renames become
/// a single-file change.
pub mod methods {
    // ---- Lifecycle (host → plugin) ----
    pub const INITIALIZE: &str = "initialize";
    pub const INITIALIZED: &str = "initialized";
    pub const SHUTDOWN: &str = "shutdown";

    // ---- Tools (host → plugin, MCP-compatible) ----
    pub const TOOLS_LIST: &str = "tools/list";
    pub const TOOLS_CALL: &str = "tools/call";

    // ---- Extension events (host → plugin) ----
    pub const BEFORE_TOOL_CALL: &str = "extension/before_tool_call";
    pub const AFTER_TOOL_CALL: &str = "extension/after_tool_call";
    pub const ON_LLM_CALL_START: &str = "extension/on_llm_call_start";
    pub const ON_LLM_CALL_END: &str = "extension/on_llm_call_end";
    pub const ON_USER_MESSAGE: &str = "extension/on_user_message";
    pub const ON_AGENT_START: &str = "extension/on_agent_start";
    pub const ON_AGENT_END: &str = "extension/on_agent_end";

    // ---- Host API (plugin → host) ----
    pub const HOST_LOG: &str = "host/log";
    pub const HOST_NOTIFY: &str = "host/notify";
    pub const HOST_REQUEST_APPROVAL: &str = "host/request_approval";
    pub const HOST_EMIT_METRIC: &str = "host/emit_metric";
    pub const HOST_STATE_GET_MESSAGES: &str = "host/state.get_messages";
    pub const HOST_STATE_GET_METADATA: &str = "host/state.get_metadata";
    pub const HOST_STATE_COUNT_TOKENS: &str = "host/state.count_tokens";
    pub const HOST_MEMORY_READ: &str = "host/memory.read";
    pub const HOST_MEMORY_WRITE: &str = "host/memory.write";
}

// ===========================================================
// Error codes
// ===========================================================

/// Standard JSON-RPC error codes plus AEP-specific extensions.
pub mod error_codes {
    // Standard JSON-RPC 2.0
    pub const PARSE_ERROR: i32 = -32700;
    pub const INVALID_REQUEST: i32 = -32600;
    pub const METHOD_NOT_FOUND: i32 = -32601;
    pub const INVALID_PARAMS: i32 = -32602;
    pub const INTERNAL_ERROR: i32 = -32603;

    // AEP-specific
    /// A capability handle has expired (the event scope it belonged
    /// to has already returned).
    pub const HANDLE_EXPIRED: i32 = -32000;
    /// The plugin called a host method it did not declare in its
    /// `requestedCapabilities` list. In v0.1 this is only produced
    /// in observation mode as a tag on the log line; v0.2 will
    /// return it as a real error.
    pub const CAPABILITY_DENIED: i32 = -32001;
    /// The plugin returned an `ExtensionAction` variant that is not
    /// valid for the event (e.g. `Block` on `on_agent_end`).
    pub const INVALID_ACTION: i32 = -32002;
}

// ===========================================================
// Handshake payloads — `initialize` method
// ===========================================================

/// Params for `initialize` (host → plugin).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct InitializeParams {
    pub protocol_version: String,
    pub host_info: HostInfo,
    pub host_capabilities: HostCapabilities,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HostInfo {
    pub name: String,
    pub version: String,
}

/// What the host offers to the plugin — the upper bound on what
/// events the plugin can subscribe to and what host APIs it can call.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HostCapabilities {
    /// State-reading scopes this host supports, e.g. `"messages"`,
    /// `"metadata"`, `"tool_calls"`.
    pub state_access: Vec<String>,
    /// Event method names the host will dispatch.
    pub events: Vec<String>,
    /// Host API method suffixes the plugin may call back, e.g.
    /// `"log"`, `"get_state"`, `"memory.write"`.
    pub host_api: Vec<String>,
}

/// Result of `initialize` (plugin → host).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct InitializeResult {
    pub protocol_version: String,
    pub plugin: PluginInfo,
    pub tools: Vec<ToolDef>,
    pub event_subscriptions: Vec<String>,
    pub requested_capabilities: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PluginInfo {
    pub name: String,
    pub version: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub author: Option<String>,
}

/// Tool declaration — mirrors MCP's `tools/list` shape so SDKs can
/// reuse existing MCP tooling.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ToolDef {
    pub name: String,
    pub description: String,
    /// JSON Schema for the tool's arguments.
    pub input_schema: Value,
}

// ===========================================================
// Event payloads — representative v1 shapes
// ===========================================================

/// Opaque handle identifying an `AgentState` snapshot valid only
/// for the lifetime of a single event request.
///
/// The plugin must pass this back when calling `host/state.*` and
/// **must not** store it past the event response — it is invalidated
/// as soon as the host sees the response for the event that issued
/// it.
pub type StateHandle = String;

/// Params for `extension/before_tool_call`.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BeforeToolCallParams {
    pub state_handle: StateHandle,
    pub tool_call: ToolCallWire,
}

/// Wire-format view of a tool call — a thin, serializable mirror of
/// the kernel's `ToolCall` type. Kept separate so changes to the
/// kernel type do not silently change the wire protocol.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolCallWire {
    pub id: String,
    pub name: String,
    pub arguments: Value,
}

/// The action a plugin returns from an event handler, telling the
/// host how to proceed.
///
/// Not every variant is legal for every event — e.g. `Block` makes
/// no sense for `on_agent_end`. The host validates on dispatch and
/// returns [`error_codes::INVALID_ACTION`] when a plugin picks one
/// that does not fit.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "action", rename_all = "snake_case")]
pub enum ExtensionAction {
    /// Proceed normally.
    Continue,
    /// Reject the operation that triggered the event.
    Block { reason: String },
    /// Rewrite the triggering operation's arguments before it runs.
    Modify { modified_arguments: Value },
    /// Skip execution entirely and use this result instead.
    ReplaceResult { result: Value },
    /// Rewrite the LLM-bound messages list before sending.
    ModifyMessages { messages: Value },
    /// Rewrite the LLM response before the agent sees it.
    ModifyResponse { response: Value },
    /// Rewrite a completed tool call's result.
    ModifyResult { result: Value },
}

// ===========================================================
// Host API payloads — representative v1 shapes
// ===========================================================

/// Params for `host/log`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HostLogParams {
    pub level: LogLevel,
    pub message: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub fields: Option<Value>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum LogLevel {
    Trace,
    Debug,
    Info,
    Warn,
    Error,
}

// ===========================================================
// Tests — ensure the wire format matches the spec byte-for-byte
// ===========================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn request_wire_format() {
        let req = Request::new("h-1", methods::INITIALIZE, Some(serde_json::json!({"x": 1})));
        let s = serde_json::to_string(&req).unwrap();
        let v: Value = serde_json::from_str(&s).unwrap();
        assert_eq!(v["jsonrpc"], "2.0");
        assert_eq!(v["id"], "h-1");
        assert_eq!(v["method"], "initialize");
        assert_eq!(v["params"]["x"], 1);
    }

    #[test]
    fn response_ok_and_err_have_exactly_one_payload_field() {
        let ok = Response::ok("h-1", serde_json::json!({"ok": true}));
        let ok_v: Value = serde_json::to_value(&ok).unwrap();
        assert!(ok_v.get("result").is_some());
        assert!(ok_v.get("error").is_none());

        let err = Response::err("h-1", RpcError::new(error_codes::METHOD_NOT_FOUND, "nope"));
        let err_v: Value = serde_json::to_value(&err).unwrap();
        assert!(err_v.get("result").is_none());
        assert!(err_v.get("error").is_some());
    }

    #[test]
    fn initialize_params_camel_case() {
        let p = InitializeParams {
            protocol_version: PROTOCOL_VERSION.to_string(),
            host_info: HostInfo {
                name: "alva".into(),
                version: "0.1.0".into(),
            },
            host_capabilities: HostCapabilities {
                state_access: vec!["messages".into()],
                events: vec![methods::BEFORE_TOOL_CALL.into()],
                host_api: vec!["log".into()],
            },
        };
        let v = serde_json::to_value(&p).unwrap();
        // snake_case Rust → camelCase JSON
        assert!(v.get("protocolVersion").is_some());
        assert!(v.get("hostInfo").is_some());
        assert!(v.get("hostCapabilities").is_some());
        assert!(v["hostCapabilities"].get("stateAccess").is_some());
        assert!(v["hostCapabilities"].get("hostApi").is_some());
    }

    #[test]
    fn extension_action_serializes_as_tagged_union() {
        let a = ExtensionAction::Block {
            reason: "dangerous".into(),
        };
        let v = serde_json::to_value(&a).unwrap();
        assert_eq!(v["action"], "block");
        assert_eq!(v["reason"], "dangerous");

        let c = ExtensionAction::Continue;
        let cv = serde_json::to_value(&c).unwrap();
        assert_eq!(cv["action"], "continue");

        // Round-trip
        let parsed: ExtensionAction = serde_json::from_value(v).unwrap();
        match parsed {
            ExtensionAction::Block { reason } => assert_eq!(reason, "dangerous"),
            _ => panic!("wrong variant"),
        }
    }

    #[test]
    fn before_tool_call_params_shape() {
        let p = BeforeToolCallParams {
            state_handle: "s-7".into(),
            tool_call: ToolCallWire {
                id: "call_abc123".into(),
                name: "shell".into(),
                arguments: serde_json::json!({"command": "rm -rf /"}),
            },
        };
        let v = serde_json::to_value(&p).unwrap();
        assert_eq!(v["stateHandle"], "s-7");
        assert_eq!(v["toolCall"]["name"], "shell");
        assert_eq!(v["toolCall"]["arguments"]["command"], "rm -rf /");
    }

    #[test]
    fn host_log_params_shape() {
        let p = HostLogParams {
            level: LogLevel::Warn,
            message: "blocking rm -rf".into(),
            fields: Some(serde_json::json!({"tool_call_id": "call_abc123"})),
        };
        let v = serde_json::to_value(&p).unwrap();
        assert_eq!(v["level"], "warn");
        assert_eq!(v["fields"]["tool_call_id"], "call_abc123");
    }
}
