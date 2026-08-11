// INPUT:  serde, serde_json, AgentMessage, agent_session event identities
// OUTPUT: Versioned typed SessionRecord envelope, payloads, blob/link metadata, append receipt
// POS:    Canonical durable record vocabulary shared by AgentSession storage and Inspector readers

use serde::{Deserialize, Serialize};

use crate::agent_session::{EventEmitter, SessionEvent, SessionMessage};
use crate::AgentMessage;

pub const SESSION_RECORD_SCHEMA_VERSION: u32 = 1;

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum RecordCategory {
    Conversation,
    Execution,
    Extension,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct BlobReference {
    pub sha256: String,
    pub media_type: String,
    pub size_bytes: u64,
    pub preview: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "storage", rename_all = "snake_case")]
pub enum RecordValue {
    Inline { value: serde_json::Value },
    Blob { reference: BlobReference },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConversationRecord {
    pub role: String,
    pub content: RecordValue,
    pub agent_message: Option<AgentMessage>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub agent_message_blob: Option<BlobReference>,
    pub parent_entry_id: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ExecutionRecord {
    Event {
        data: serde_json::Value,
    },
    Intent {
        operation_id: String,
        operation: String,
        input: RecordValue,
    },
    Outcome {
        operation_id: String,
        success: bool,
        output: RecordValue,
    },
    ChildSessionLinked {
        child_session_id: String,
        parent_run_id: String,
        spawn_tool_call_id: String,
    },
    ConversationHeadMoved {
        entry_id: String,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExtensionRecord {
    pub namespace: String,
    pub payload: serde_json::Value,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "category", content = "payload", rename_all = "snake_case")]
pub enum SessionRecordPayload {
    Conversation(ConversationRecord),
    Execution(ExecutionRecord),
    Extension(ExtensionRecord),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionRecord {
    pub seq: u64,
    pub record_id: String,
    pub parent_record_id: Option<String>,
    pub timestamp: i64,
    pub emitter: EventEmitter,
    pub category: RecordCategory,
    pub type_name: String,
    pub schema_version: u32,
    pub payload: SessionRecordPayload,
}

impl SessionRecord {
    pub fn conversation(agent_message: AgentMessage, parent_record_id: Option<String>) -> Self {
        let (type_name, role, content) = match &agent_message {
            AgentMessage::Standard(message) => {
                let role = match &message.role {
                    crate::MessageRole::User => "user",
                    crate::MessageRole::Assistant => "assistant",
                    crate::MessageRole::System => "system",
                    crate::MessageRole::Tool => "tool",
                };
                let type_name = if matches!(&message.role, crate::MessageRole::Tool) {
                    "tool_result"
                } else {
                    role
                };
                (
                    type_name.to_string(),
                    role.to_string(),
                    serde_json::to_value(&message.content).unwrap_or(serde_json::Value::Null),
                )
            }
            AgentMessage::Steering(message) => (
                "user_steering".to_string(),
                "user".to_string(),
                serde_json::to_value(&message.content).unwrap_or(serde_json::Value::Null),
            ),
            AgentMessage::FollowUp(message) => (
                "system_followup".to_string(),
                "system".to_string(),
                serde_json::to_value(&message.content).unwrap_or(serde_json::Value::Null),
            ),
            AgentMessage::Marker(marker) => (
                "marker".to_string(),
                "marker".to_string(),
                serde_json::to_value(marker).unwrap_or(serde_json::Value::Null),
            ),
            AgentMessage::Extension { type_name, data } => (
                format!("extension:{type_name}"),
                "extension".to_string(),
                data.clone(),
            ),
        };
        let mut record = Self::new(
            RecordCategory::Conversation,
            type_name,
            EventEmitter::runtime(),
            SessionRecordPayload::Conversation(ConversationRecord {
                role,
                content: RecordValue::Inline { value: content },
                agent_message: Some(agent_message),
                agent_message_blob: None,
                parent_entry_id: parent_record_id.clone(),
            }),
        );
        record.parent_record_id = parent_record_id;
        record
    }

    pub fn execution(
        type_name: impl Into<String>,
        emitter: EventEmitter,
        payload: ExecutionRecord,
    ) -> Self {
        Self::new(
            RecordCategory::Execution,
            type_name,
            emitter,
            SessionRecordPayload::Execution(payload),
        )
    }

    pub fn extension(
        namespace: impl Into<String>,
        type_name: impl Into<String>,
        schema_version: u32,
        emitter: EventEmitter,
        payload: serde_json::Value,
    ) -> Result<Self, String> {
        let namespace = namespace.into();
        let type_name = type_name.into();
        if !namespace.contains('.') || type_name.trim().is_empty() || schema_version == 0 {
            return Err(
                "extension records require a dotted namespace, type name, and schema version"
                    .to_string(),
            );
        }
        Ok(Self {
            seq: 0,
            record_id: uuid::Uuid::new_v4().to_string(),
            parent_record_id: None,
            timestamp: chrono::Utc::now().timestamp_millis(),
            emitter,
            category: RecordCategory::Extension,
            type_name,
            schema_version,
            payload: SessionRecordPayload::Extension(ExtensionRecord { namespace, payload }),
        })
    }

    pub fn from_event(event: SessionEvent) -> Self {
        let payload = if let Some(message) = event.message.clone() {
            SessionRecordPayload::Conversation(ConversationRecord {
                role: message.role,
                content: RecordValue::Inline {
                    value: message.content,
                },
                agent_message: event
                    .data
                    .clone()
                    .and_then(|value| serde_json::from_value(value).ok()),
                agent_message_blob: None,
                parent_entry_id: event.parent_uuid.clone(),
            })
        } else if let Some(namespace) = event.event_type.strip_prefix("extension:") {
            SessionRecordPayload::Extension(ExtensionRecord {
                namespace: namespace.to_string(),
                payload: event.data.clone().unwrap_or(serde_json::Value::Null),
            })
        } else {
            SessionRecordPayload::Execution(ExecutionRecord::Event {
                data: event.data.clone().unwrap_or(serde_json::Value::Null),
            })
        };
        let category = match &payload {
            SessionRecordPayload::Conversation(_) => RecordCategory::Conversation,
            SessionRecordPayload::Execution(_) => RecordCategory::Execution,
            SessionRecordPayload::Extension(_) => RecordCategory::Extension,
        };
        Self {
            seq: event.seq,
            record_id: event.uuid,
            parent_record_id: event.parent_uuid,
            timestamp: event.timestamp,
            emitter: event.emitter,
            category,
            type_name: event.event_type,
            schema_version: SESSION_RECORD_SCHEMA_VERSION,
            payload,
        }
    }

    pub fn into_event(self) -> SessionEvent {
        let (message, data) = match &self.payload {
            SessionRecordPayload::Conversation(conversation) => {
                let content = match &conversation.content {
                    RecordValue::Inline { value } => value.clone(),
                    RecordValue::Blob { reference } => serde_json::json!({
                        "blob": reference,
                    }),
                };
                (
                    Some(SessionMessage {
                        role: conversation.role.clone(),
                        content,
                    }),
                    conversation
                        .agent_message
                        .as_ref()
                        .and_then(|message| serde_json::to_value(message).ok()),
                )
            }
            SessionRecordPayload::Execution(execution) => (
                None,
                Some(serde_json::to_value(execution).unwrap_or(serde_json::Value::Null)),
            ),
            SessionRecordPayload::Extension(extension) => (None, Some(extension.payload.clone())),
        };
        SessionEvent {
            seq: self.seq,
            uuid: self.record_id,
            parent_uuid: self.parent_record_id,
            timestamp: self.timestamp,
            event_type: self.type_name,
            emitter: self.emitter,
            message,
            data,
        }
    }

    fn new(
        category: RecordCategory,
        type_name: impl Into<String>,
        emitter: EventEmitter,
        payload: SessionRecordPayload,
    ) -> Self {
        Self {
            seq: 0,
            record_id: uuid::Uuid::new_v4().to_string(),
            parent_record_id: None,
            timestamp: chrono::Utc::now().timestamp_millis(),
            emitter,
            category,
            type_name: type_name.into(),
            schema_version: SESSION_RECORD_SCHEMA_VERSION,
            payload,
        }
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub struct AppendReceipt {
    pub first_seq: u64,
    pub last_seq: u64,
    pub count: usize,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unknown_extension_payload_roundtrips_without_kernel_enum_change() {
        let record = SessionRecord::extension(
            "com.example.plugin",
            "custom_observation",
            7,
            EventEmitter::runtime(),
            serde_json::json!({"future": true}),
        )
        .unwrap();
        let json = serde_json::to_vec(&record).unwrap();
        let restored: SessionRecord = serde_json::from_slice(&json).unwrap();
        assert_eq!(restored.schema_version, 7);
        assert!(matches!(
            restored.payload,
            SessionRecordPayload::Extension(_)
        ));
    }
}
