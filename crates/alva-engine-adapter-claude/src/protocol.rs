// INPUT:  serde::{Deserialize, Serialize}, serde_json::Value
// OUTPUT: pub enum BridgeMessage, pub enum SdkMessage, pub struct SdkAssistantPayload, pub enum SdkContentBlock, pub struct SdkUsage, pub enum BridgeOutbound, pub enum BridgePermissionDecision
// POS:    Defines the JSON-line wire protocol types for stdin/stdout communication with the Node.js bridge.

use serde::Deserialize;
use serde_json::Value;

/// Messages received from the bridge script via stdout.
#[derive(Debug, Deserialize)]
#[serde(tag = "type")]
pub enum BridgeMessage {
    #[serde(rename = "sdk_message")]
    SdkMessage { message: SdkMessage },

    #[serde(rename = "permission_request")]
    PermissionRequest {
        request_id: String,
        tool_name: String,
        tool_input: Value,
    },

    #[serde(rename = "done")]
    Done,

    #[serde(rename = "error")]
    Error { message: String },
}

/// SDK message types we care about. Unknown types are silently ignored.
#[derive(Debug, Deserialize)]
#[serde(tag = "type")]
pub enum SdkMessage {
    #[serde(rename = "system")]
    System {
        subtype: Option<String>,
        session_id: Option<String>,
        model: Option<String>,
        tools: Option<Vec<String>>,
    },

    #[serde(rename = "assistant")]
    Assistant {
        uuid: Option<String>,
        session_id: Option<String>,
        message: Option<SdkAssistantPayload>,
    },

    #[serde(rename = "stream_event")]
    StreamEvent {
        uuid: Option<String>,
        event: Option<Value>,
    },

    #[serde(rename = "result")]
    Result {
        subtype: Option<String>,
        session_id: Option<String>,
        result: Option<String>,
        total_cost_usd: Option<f64>,
        duration_ms: Option<u64>,
        num_turns: Option<u32>,
        usage: Option<SdkUsage>,
    },

    #[serde(other)]
    Unknown,
}

#[derive(Debug, Deserialize)]
pub struct SdkAssistantPayload {
    pub content: Option<Vec<SdkContentBlock>>,
}

#[derive(Debug, Deserialize)]
#[serde(tag = "type")]
pub enum SdkContentBlock {
    #[serde(rename = "text")]
    Text { text: String },

    #[serde(rename = "thinking")]
    Thinking { thinking: String },

    #[serde(rename = "tool_use")]
    ToolUse {
        id: String,
        name: String,
        input: Value,
    },

    #[serde(rename = "tool_result")]
    ToolResult {
        tool_use_id: String,
        content: Option<String>,
        is_error: Option<bool>,
    },

    #[serde(other)]
    Other,
}

#[derive(Debug, Deserialize)]
pub struct SdkUsage {
    pub input_tokens: Option<u64>,
    pub output_tokens: Option<u64>,
}

/// Outbound messages sent to the bridge script via stdin.
#[derive(Debug, serde::Serialize)]
#[serde(tag = "type")]
pub enum BridgeOutbound {
    #[serde(rename = "permission_response")]
    PermissionResponse {
        request_id: String,
        decision: BridgePermissionDecision,
    },

    #[serde(rename = "cancel")]
    Cancel,

    #[serde(rename = "shutdown")]
    Shutdown,
}

#[derive(Debug, serde::Serialize)]
#[serde(tag = "behavior")]
pub enum BridgePermissionDecision {
    #[serde(rename = "allow")]
    Allow {
        #[serde(skip_serializing_if = "Option::is_none")]
        updated_input: Option<Value>,
    },
    #[serde(rename = "deny")]
    Deny { message: String },
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_system_init() {
        let json = r#"{"type":"sdk_message","message":{"type":"system","subtype":"init","session_id":"abc","model":"claude-sonnet-4-6","tools":["Read","Write"]}}"#;
        let msg: BridgeMessage = serde_json::from_str(json).unwrap();
        match msg {
            BridgeMessage::SdkMessage {
                message:
                    SdkMessage::System {
                        subtype,
                        session_id,
                        model,
                        tools,
                    },
            } => {
                assert_eq!(subtype.as_deref(), Some("init"));
                assert_eq!(session_id.as_deref(), Some("abc"));
                assert_eq!(model.as_deref(), Some("claude-sonnet-4-6"));
                assert_eq!(tools.as_ref().unwrap().len(), 2);
            }
            other => panic!("unexpected: {other:?}"),
        }
    }

    #[test]
    fn test_parse_result_success() {
        let json = r#"{"type":"sdk_message","message":{"type":"result","subtype":"success","session_id":"abc","result":"done","total_cost_usd":0.05,"duration_ms":1200,"num_turns":3,"usage":{"input_tokens":100,"output_tokens":200}}}"#;
        let msg: BridgeMessage = serde_json::from_str(json).unwrap();
        match msg {
            BridgeMessage::SdkMessage {
                message:
                    SdkMessage::Result {
                        subtype,
                        total_cost_usd,
                        ..
                    },
            } => {
                assert_eq!(subtype.as_deref(), Some("success"));
                assert!((total_cost_usd.unwrap() - 0.05).abs() < f64::EPSILON);
            }
            other => panic!("unexpected: {other:?}"),
        }
    }

    #[test]
    fn test_parse_assistant_with_tool_use() {
        let json = r#"{"type":"sdk_message","message":{"type":"assistant","uuid":"u1","session_id":"s1","message":{"content":[{"type":"text","text":"hello"},{"type":"tool_use","id":"t1","name":"Read","input":{"file_path":"/tmp/test.rs"}}]}}}"#;
        let msg: BridgeMessage = serde_json::from_str(json).unwrap();
        match msg {
            BridgeMessage::SdkMessage {
                message:
                    SdkMessage::Assistant {
                        message: Some(payload),
                        ..
                    },
            } => {
                let blocks = payload.content.unwrap();
                assert_eq!(blocks.len(), 2);
                assert!(
                    matches!(&blocks[0], SdkContentBlock::Text { text } if text == "hello")
                );
                assert!(
                    matches!(&blocks[1], SdkContentBlock::ToolUse { name, .. } if name == "Read")
                );
            }
            other => panic!("unexpected: {other:?}"),
        }
    }

    #[test]
    fn test_parse_permission_request() {
        let json = r#"{"type":"permission_request","request_id":"r1","tool_name":"Bash","tool_input":{"command":"ls"}}"#;
        let msg: BridgeMessage = serde_json::from_str(json).unwrap();
        assert!(
            matches!(msg, BridgeMessage::PermissionRequest { request_id, .. } if request_id == "r1")
        );
    }

    #[test]
    fn test_parse_unknown_sdk_message() {
        let json = r#"{"type":"sdk_message","message":{"type":"some_future_type","data":123}}"#;
        let msg: BridgeMessage = serde_json::from_str(json).unwrap();
        assert!(matches!(
            msg,
            BridgeMessage::SdkMessage {
                message: SdkMessage::Unknown
            }
        ));
    }

    #[test]
    fn test_serialize_permission_response() {
        let msg = BridgeOutbound::PermissionResponse {
            request_id: "r1".into(),
            decision: BridgePermissionDecision::Allow {
                updated_input: None,
            },
        };
        let json = serde_json::to_string(&msg).unwrap();
        assert!(json.contains("permission_response"));
        assert!(json.contains("allow"));
    }

    // --- Real SDK output tests (captured from Claude Agent SDK v0.2.81) ---

    #[test]
    fn test_real_sdk_system_init() {
        let json = r#"{"type":"sdk_message","message":{"type":"system","subtype":"init","cwd":"/private/tmp/alva-sdk-test","session_id":"fcbc2425-ffaa-4c32-a9b4-38cd5800bb02","tools":["Task","Bash","Read","Edit","Write"],"mcp_servers":[],"model":"claude-opus-4-6[1m]","permissionMode":"plan","uuid":"c37263ae-e605-4f21-81e2-7029bce60371"}}"#;
        let msg: BridgeMessage = serde_json::from_str(json).unwrap();
        match msg {
            BridgeMessage::SdkMessage { message: SdkMessage::System { subtype, session_id, model, tools } } => {
                assert_eq!(subtype.as_deref(), Some("init"));
                assert_eq!(session_id.as_deref(), Some("fcbc2425-ffaa-4c32-a9b4-38cd5800bb02"));
                assert_eq!(model.as_deref(), Some("claude-opus-4-6[1m]"));
                assert_eq!(tools.unwrap().len(), 5);
            }
            other => panic!("unexpected: {other:?}"),
        }
    }

    #[test]
    fn test_real_sdk_stream_event_text_delta() {
        let json = r#"{"type":"sdk_message","message":{"type":"stream_event","event":{"type":"content_block_delta","index":0,"delta":{"type":"text_delta","text":"Four"}},"session_id":"fcbc2425-ffaa-4c32-a9b4-38cd5800bb02","parent_tool_use_id":null,"uuid":"b4919f8d-2934-4098-8479-0202b32e4035"}}"#;
        let msg: BridgeMessage = serde_json::from_str(json).unwrap();
        match msg {
            BridgeMessage::SdkMessage { message: SdkMessage::StreamEvent { uuid, event } } => {
                assert_eq!(uuid.as_deref(), Some("b4919f8d-2934-4098-8479-0202b32e4035"));
                let delta = event.unwrap();
                assert_eq!(delta["delta"]["text"].as_str(), Some("Four"));
            }
            other => panic!("unexpected: {other:?}"),
        }
    }

    #[test]
    fn test_real_sdk_assistant_message() {
        let json = r#"{"type":"sdk_message","message":{"type":"assistant","message":{"model":"claude-opus-4-6","id":"msg_014BqHc8aZMV26WyHcXqJ1bq","type":"message","role":"assistant","content":[{"type":"text","text":"Four."}],"stop_reason":null,"usage":{"input_tokens":3,"output_tokens":1}},"parent_tool_use_id":null,"session_id":"fcbc2425-ffaa-4c32-a9b4-38cd5800bb02","uuid":"e6944592-7141-4f55-b996-686576cdb540"}}"#;
        let msg: BridgeMessage = serde_json::from_str(json).unwrap();
        match msg {
            BridgeMessage::SdkMessage { message: SdkMessage::Assistant { uuid, message, .. } } => {
                assert_eq!(uuid.as_deref(), Some("e6944592-7141-4f55-b996-686576cdb540"));
                let content = message.unwrap().content.unwrap();
                assert_eq!(content.len(), 1);
                assert!(matches!(&content[0], SdkContentBlock::Text { text } if text == "Four."));
            }
            other => panic!("unexpected: {other:?}"),
        }
    }

    #[test]
    fn test_real_sdk_result_success() {
        let json = r#"{"type":"sdk_message","message":{"type":"result","subtype":"success","is_error":false,"duration_ms":3589,"num_turns":1,"result":"Four.","session_id":"fcbc2425-ffaa-4c32-a9b4-38cd5800bb02","total_cost_usd":0.03144475,"usage":{"input_tokens":3,"output_tokens":5},"uuid":"34efce21-30a9-4a28-9ecf-9dc367ed279d"}}"#;
        let msg: BridgeMessage = serde_json::from_str(json).unwrap();
        match msg {
            BridgeMessage::SdkMessage { message: SdkMessage::Result { subtype, result, total_cost_usd, duration_ms, num_turns, .. } } => {
                assert_eq!(subtype.as_deref(), Some("success"));
                assert_eq!(result.as_deref(), Some("Four."));
                assert!((total_cost_usd.unwrap() - 0.03144475).abs() < 1e-8);
                assert_eq!(duration_ms, Some(3589));
                assert_eq!(num_turns, Some(1));
            }
            other => panic!("unexpected: {other:?}"),
        }
    }

    #[test]
    fn test_real_sdk_rate_limit_falls_to_unknown() {
        let json = r#"{"type":"sdk_message","message":{"type":"rate_limit_event","rate_limit_info":{"status":"allowed"},"uuid":"1e1d33fe-e786-4723-929a-7f951e87c4ae"}}"#;
        let msg: BridgeMessage = serde_json::from_str(json).unwrap();
        assert!(matches!(msg, BridgeMessage::SdkMessage { message: SdkMessage::Unknown }));
    }
}
