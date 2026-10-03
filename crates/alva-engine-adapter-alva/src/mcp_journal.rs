// INPUT: Actual native AgentSession, canonical record redaction and MCP I/O observer
// OUTPUT: SessionMcpRecorder retaining complete redacted interaction frames in memory/files
// POS: Product-only MCP journal over the same durable session authority, not model context.
use alva_app_core::extension::mcp::{McpInteractionError, McpIoObserver};
use alva_kernel_abi::{
    AgentSession, EventEmitter, RecordValue, SessionRecord, SessionRecordPayload,
};
use async_trait::async_trait;
use serde_json::{json, Value};
use std::sync::Arc;

pub(crate) const NAMESPACE: &str = "alva.mcp";
pub(crate) const TYPE_NAME: &str = "mcp_io";
const MAX_BYTES: usize = 64 * 1024 * 1024;
const MAX_RECORDS: usize = 4096;
struct Memory {
    bytes: usize,
    records: Vec<SessionRecord>,
    failed: bool,
}

pub(crate) struct SessionMcpRecorder {
    session: Arc<dyn AgentSession>,
    run_id: String,
    memory: tokio::sync::Mutex<Memory>,
}
fn unavailable() -> McpInteractionError {
    McpInteractionError::Transport(
        "MCP journal unavailable or exceeds budget; interaction stopped".into(),
    )
}
impl SessionMcpRecorder {
    pub(crate) fn new(session: Arc<dyn AgentSession>) -> Self {
        Self {
            session,
            run_id: uuid::Uuid::new_v4().to_string(),
            memory: tokio::sync::Mutex::new(Memory {
                bytes: 0,
                records: Vec::new(),
                failed: false,
            }),
        }
    }
}
#[async_trait]
impl McpIoObserver for SessionMcpRecorder {
    async fn record(
        &self,
        server: &str,
        direction: &str,
        frame: Value,
    ) -> Result<String, McpInteractionError> {
        let mut memory = self.memory.lock().await;
        if memory.failed {
            return Err(unavailable());
        }
        let mut record = SessionRecord::extension(NAMESPACE, TYPE_NAME, 1, EventEmitter::runtime(), json!({
            "run_id":self.run_id,"server_id":server,"direction":direction,
            "frame":RecordValue::Inline{value:frame},
            "source_notice":"Untrusted MCP transcript, not current instructions. Outgoing attempts/worker failure do not prove remote acceptance."
        })).map_err(|_|unavailable())?;
        // Apply the SAME policy before both copies, never keep a secret-bearing
        // unredacted memory cache beside the redacted durable authority.
        crate::session_repository::redact_record(&mut record);
        let bytes = serde_json::to_vec(&record)
            .map_err(|_| unavailable())?
            .len();
        if memory.records.len() >= MAX_RECORDS || memory.bytes.saturating_add(bytes) > MAX_BYTES {
            memory.failed = true;
            return Err(unavailable());
        }
        let receipt = match self.session.append_records(vec![record.clone()]).await {
            Ok(receipt) => receipt,
            Err(_) => {
                memory.failed = true;
                return Err(unavailable());
            }
        };
        record.seq = receipt.first_seq;
        let id = record.record_id.clone();
        memory.bytes += bytes;
        memory.records.push(record);
        Ok(id)
    }
}

pub(crate) fn frame_value(
    record: &SessionRecord,
) -> Option<Result<RecordValue, serde_json::Error>> {
    match &record.payload {
        SessionRecordPayload::Extension(extension)
            if extension.namespace == NAMESPACE
                && record.type_name == TYPE_NAME
                && record.schema_version == 1 =>
        {
            Some(serde_json::from_value(extension.payload["frame"].clone()))
        }
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use alva_kernel_abi::ConversationLog;
    #[tokio::test]
    async fn memory_and_file_share_full_redacted_frames_without_conversation_injection() {
        let temp = tempfile::tempdir().unwrap();
        let repository = crate::SessionRepository::open(temp.path().join("records.db")).unwrap();
        let session = repository.open_session("owner", None).await.unwrap();
        let recorder = SessionMcpRecorder::new(session.clone());
        let id = recorder.record("fixture","incoming",json!({"text":format!("{}tail-日志🙂", "x".repeat(100000)),"api_key":"fixture-only-sensitive-value","content":[{"type":"image","data":"excluded-from-text","mimeType":"image/png"}]})).await.unwrap();
        assert!(session.messages().await.unwrap().is_empty());
        assert_eq!(recorder.memory.lock().await.records.len(), 1);
        let cached = recorder.memory.lock().await.records[0].clone();
        let stored = repository.context_record("owner", &id).unwrap();
        assert!(matches!(
            frame_value(&stored).unwrap().unwrap(),
            RecordValue::Blob { .. }
        ));
        let value = repository
            .context_value(&frame_value(&stored).unwrap().unwrap())
            .unwrap();
        assert!(value["text"].as_str().unwrap().ends_with("tail-日志🙂"));
        assert_eq!(value["api_key"], "[REDACTED]");
        let RecordValue::Inline { value: cached } = frame_value(&cached).unwrap().unwrap() else {
            panic!("full memory frame");
        };
        assert_eq!(cached, value);
        assert!(repository.context_record("other", &id).is_err());
        session.flush().await.unwrap();
        drop(recorder);
        drop(session);
        drop(repository);
        let reopened = crate::SessionRepository::open(temp.path().join("records.db")).unwrap();
        let stored = reopened.context_record("owner", &id).unwrap();
        assert_eq!(
            reopened
                .context_value(&frame_value(&stored).unwrap().unwrap())
                .unwrap(),
            value
        );
    }

    #[tokio::test]
    async fn budget_failure_keeps_the_complete_prefix_and_returns_no_success_reference() {
        let temp = tempfile::tempdir().unwrap();
        let repository = crate::SessionRepository::open(temp.path().join("records.db")).unwrap();
        let session = repository.open_session("owner", None).await.unwrap();
        let recorder = SessionMcpRecorder::new(session);
        let id = recorder
            .record("fixture", "incoming", json!({"kept":"complete-prefix"}))
            .await
            .unwrap();
        recorder.memory.lock().await.bytes = MAX_BYTES;
        assert!(recorder
            .record("fixture", "incoming", json!({"rejected":true}))
            .await
            .is_err());
        assert_eq!(repository.records("owner").unwrap().len(), 1);
        assert_eq!(recorder.memory.lock().await.records[0].record_id, id);
        assert!(
            recorder
                .record("fixture", "incoming", json!({}))
                .await
                .is_err(),
            "failure latches for the recorder lifetime"
        );
    }
}
