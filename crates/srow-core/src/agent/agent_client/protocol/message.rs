// INPUT:  serde, super::{content, lifecycle, permission, special, tool}
// OUTPUT: AcpInboundMessage, AcpOutboundMessage
// POS:    Top-level ACP protocol envelopes: 13 inbound event types and 5 outbound command types.
use serde::{Deserialize, Serialize};

use super::{
    content::ContentBlock,
    lifecycle::{ErrorData, FinishData, SystemMessageData, TaskCompleteData, TaskStartData},
    permission::{PermissionData, PermissionRequest},
    special::{PingPongData, PlanData},
    tool::{PostToolUseData, PreToolUseData, ToolCallData},
};

/// ACP protocol: External Agent -> Srow (read from stdout)
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "acp_event_type", rename_all = "snake_case")]
pub enum AcpInboundMessage {
    /// Session state update (usually contains ContentBlock list)
    SessionUpdate {
        session_id: String,
        #[serde(default)]
        content: Vec<ContentBlock>,
    },
    /// Single message content update (streaming delta)
    MessageUpdate {
        message_id: String,
        #[serde(default)]
        content: Vec<ContentBlock>,
    },
    /// External Agent requests permission (before tool execution)
    RequestPermission {
        request_id: String,
        data: PermissionRequest,
    },
    /// Task started
    TaskStart { data: TaskStartData },
    /// Task completed
    TaskComplete { data: TaskCompleteData },
    /// System message (log / status notification)
    SystemMessage { data: SystemMessageData },
    /// Finish data (contains final output summary)
    FinishData { data: FinishData },
    /// Error data
    ErrorData { data: ErrorData },
    /// Pre-tool-use notification
    PreToolUse { data: PreToolUseData },
    /// Post-tool-use notification
    PostToolUse { data: PostToolUseData },
    /// Tool call data (complete parameters)
    ToolCallData { data: ToolCallData },
    /// Agent execution plan (step list displayed to user)
    Plan { data: PlanData },
    /// Heartbeat
    #[serde(rename = "ping")]
    PingPong { data: PingPongData },
}

/// ACP protocol: Srow -> External Agent (written to stdin)
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum AcpOutboundMessage {
    /// User prompt (start task)
    Prompt {
        content: String,
        /// Optional: continuation prompt (resume previous task)
        #[serde(skip_serializing_if = "Option::is_none")]
        resume: Option<bool>,
    },
    /// Permission response (responding to RequestPermission)
    PermissionResponse {
        request_id: String,
        data: PermissionData,
    },
    /// Cancel current task
    Cancel,
    /// Shutdown Agent (graceful exit)
    Shutdown,
    /// Heartbeat response
    #[serde(rename = "pong")]
    Pong { id: String },
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agent::agent_client::protocol::lifecycle::TaskFinishReason;
    use crate::agent::agent_client::protocol::permission::PermissionOption;

    #[test]
    fn test_inbound_task_complete_serde() {
        let json = r#"{
            "acp_event_type": "task_complete",
            "data": {
                "task_id": "t-001",
                "finish_reason": "complete",
                "summary": "Done"
            }
        }"#;
        let msg: AcpInboundMessage = serde_json::from_str(json).unwrap();
        match msg {
            AcpInboundMessage::TaskComplete { data } => {
                assert_eq!(data.task_id, "t-001");
                assert_eq!(data.finish_reason, TaskFinishReason::Complete);
                assert_eq!(data.summary, Some("Done".to_string()));
            }
            other => panic!("expected TaskComplete, got {:?}", other),
        }
    }

    #[test]
    fn test_inbound_task_start_serde() {
        let json = r#"{
            "acp_event_type": "task_start",
            "data": {
                "task_id": "t-002",
                "description": "Refactoring main.rs"
            }
        }"#;
        let msg: AcpInboundMessage = serde_json::from_str(json).unwrap();
        assert!(matches!(msg, AcpInboundMessage::TaskStart { .. }));
    }

    #[test]
    fn test_inbound_session_update_serde() {
        let json = r#"{
            "acp_event_type": "session_update",
            "session_id": "s-001",
            "content": [
                {"type": "text", "text": "Hello world", "is_delta": false}
            ]
        }"#;
        let msg: AcpInboundMessage = serde_json::from_str(json).unwrap();
        match msg {
            AcpInboundMessage::SessionUpdate {
                session_id,
                content,
            } => {
                assert_eq!(session_id, "s-001");
                assert_eq!(content.len(), 1);
            }
            other => panic!("expected SessionUpdate, got {:?}", other),
        }
    }

    #[test]
    fn test_inbound_request_permission_serde() {
        let json = r#"{
            "acp_event_type": "request_permission",
            "request_id": "rp-001",
            "data": {
                "description": "Execute rm -rf /tmp/test",
                "risk_level": "high",
                "tool_name": "execute_shell",
                "tool_input_summary": "rm -rf /tmp/test"
            }
        }"#;
        let msg: AcpInboundMessage = serde_json::from_str(json).unwrap();
        assert!(matches!(msg, AcpInboundMessage::RequestPermission { .. }));
    }

    #[test]
    fn test_inbound_ping_serde() {
        let json = r#"{
            "acp_event_type": "ping",
            "data": {
                "id": "pg-001",
                "timestamp_ms": 1700000000000
            }
        }"#;
        let msg: AcpInboundMessage = serde_json::from_str(json).unwrap();
        match msg {
            AcpInboundMessage::PingPong { data } => {
                assert_eq!(data.id, "pg-001");
            }
            other => panic!("expected PingPong, got {:?}", other),
        }
    }

    #[test]
    fn test_inbound_error_data_serde() {
        let json = r#"{
            "acp_event_type": "error_data",
            "data": {
                "code": "RATE_LIMIT",
                "message": "Rate limited",
                "recoverable": true
            }
        }"#;
        let msg: AcpInboundMessage = serde_json::from_str(json).unwrap();
        match msg {
            AcpInboundMessage::ErrorData { data } => {
                assert_eq!(data.code, "RATE_LIMIT");
                assert_eq!(data.recoverable, Some(true));
            }
            other => panic!("expected ErrorData, got {:?}", other),
        }
    }

    #[test]
    fn test_outbound_prompt_serde() {
        let msg = AcpOutboundMessage::Prompt {
            content: "Write a function".to_string(),
            resume: None,
        };
        let json = serde_json::to_string(&msg).unwrap();
        assert!(json.contains(r#""type":"prompt""#));
        assert!(json.contains("Write a function"));
        assert!(!json.contains("resume"));
    }

    #[test]
    fn test_outbound_permission_response_serde() {
        let msg = AcpOutboundMessage::PermissionResponse {
            request_id: "req-001".to_string(),
            data: crate::agent::agent_client::protocol::permission::PermissionData {
                option: PermissionOption::AllowOnce,
                reason: None,
            },
        };
        let json = serde_json::to_string(&msg).unwrap();
        assert!(json.contains("allow_once"));
        assert!(json.contains("req-001"));
    }

    #[test]
    fn test_outbound_cancel_serde() {
        let msg = AcpOutboundMessage::Cancel;
        let json = serde_json::to_string(&msg).unwrap();
        assert!(json.contains(r#""type":"cancel""#));
    }

    #[test]
    fn test_outbound_shutdown_serde() {
        let msg = AcpOutboundMessage::Shutdown;
        let json = serde_json::to_string(&msg).unwrap();
        assert!(json.contains(r#""type":"shutdown""#));
    }

    #[test]
    fn test_outbound_pong_serde() {
        let msg = AcpOutboundMessage::Pong {
            id: "pg-001".to_string(),
        };
        let json = serde_json::to_string(&msg).unwrap();
        assert!(json.contains(r#""type":"pong""#));
        assert!(json.contains("pg-001"));
    }
}
