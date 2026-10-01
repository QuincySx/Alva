// INPUT: Private Hub SQLite transaction, raw tool text, product redaction
// OUTPUT: ToolContentRef/ToolContentChunk and atomic tool-evidence persistence
// POS: Product-owned bounded full-text storage, never an SDK or filesystem API.

use rusqlite::{params, OptionalExtension, TransactionBehavior};
use serde::{Deserialize, Serialize};

use super::{
    bounded_text, turn::record_side_effect_in, HubStore, PreparedTurn, SideEffectEvidence,
};

const CHUNK_BYTES: usize = 16 * 1024;
// An explicit rejection, never silent truncation. Keep one tool result from
// exhausting the product database; the caller surfaces the persistence error.
const MAX_CONTENT_BYTES: usize = 64 * 1024 * 1024;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ToolContentRef {
    pub content_id: String,
    pub total_bytes: usize,
    pub chunk_count: usize,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{EngineTarget, PermissionProfile};

    fn fixture() -> (tempfile::TempDir, HubStore, PreparedTurn) {
        let root = tempfile::tempdir().unwrap();
        let store = HubStore::open(root.path().join("data/hub.db")).unwrap();
        let prepared = store
            .prepare_new_turn(
                "content-test",
                root.path(),
                "tools",
                &EngineTarget::for_engine("alva").unwrap(),
                PermissionProfile::WorkspaceWrite,
            )
            .unwrap();
        (root, store, prepared)
    }

    fn evidence() -> SideEffectEvidence {
        SideEffectEvidence {
            tool_call_id: "call-1".into(),
            tool_name: "read_file".into(),
            ..Default::default()
        }
    }

    #[test]
    fn unicode_full_text_roundtrips_across_reopen_without_entering_history_payload() {
        let (_root, store, prepared) = fixture();
        let raw = format!("{}\nEND", "🙂正文".repeat(12_000));
        let input = store
            .record_tool_side_effect(&prepared, evidence(), ToolContentKind::Input, "input")
            .unwrap();
        let output = store
            .record_tool_side_effect(&prepared, evidence(), ToolContentKind::Output, &raw)
            .unwrap();
        assert_eq!(output.tool_input_ref, input.tool_input_ref);
        let content = output.tool_output_ref.unwrap();
        assert!(content.chunk_count > 1);
        assert_eq!(content.total_bytes, raw.len());
        let database = store.path().to_owned();
        drop(store);
        let store = HubStore::open(database).unwrap();
        let mut text = String::new();
        for index in 0..content.chunk_count {
            let chunk = store
                .read_tool_content("content-test", &content.content_id, index)
                .unwrap()
                .unwrap();
            assert!(chunk.text.len() <= CHUNK_BYTES);
            text.push_str(&chunk.text);
        }
        assert_eq!(text, raw);
        let history = store.get_session("content-test").unwrap().unwrap();
        let saved = &history.turns[0].attempts[0].side_effects[0];
        assert_eq!(saved.tool_output_ref.as_ref(), Some(&content));
        assert!(saved.tool_output.as_ref().unwrap().chars().count() <= 2001);
        assert!(!serde_json::to_string(&history).unwrap().contains("END"));
        assert!(store
            .read_tool_content("other-session", &content.content_id, 0)
            .unwrap()
            .is_none());
        assert!(store
            .read_tool_content("content-test", &content.content_id, content.chunk_count)
            .unwrap()
            .is_none());
        assert!(store
            .read_tool_content("content-test", &content.content_id, usize::MAX)
            .unwrap()
            .is_none());
    }

    #[test]
    fn redaction_precedes_chunking_and_failure_rolls_back_both_tables() {
        let (_root, store, prepared) = fixture();
        let raw = format!(
            "{} api_key=synthetic-secret-only\nend",
            "x".repeat(CHUNK_BYTES - 16)
        );
        let saved = store
            .record_tool_side_effect(&prepared, evidence(), ToolContentKind::Output, &raw)
            .unwrap();
        let reference = saved.tool_output_ref.unwrap();
        let text = (0..reference.chunk_count)
            .map(|index| {
                store
                    .read_tool_content("content-test", &reference.content_id, index)
                    .unwrap()
                    .unwrap()
                    .text
            })
            .collect::<String>();
        assert!(!text.contains("synthetic-secret-only"));
        assert_eq!(text, crate::redact::redact_text(&raw));
        store
            .finish_turn(&prepared, None, Some("done"), None, None)
            .unwrap();
        assert!(store
            .record_tool_side_effect(&prepared, evidence(), ToolContentKind::Output, "late")
            .is_err());
        let count: usize = store
            .lock()
            .unwrap()
            .query_row("SELECT COUNT(*) FROM tool_contents", [], |row| row.get(0))
            .unwrap();
        assert_eq!(count, 1);
    }

    #[test]
    fn empty_content_is_readable_and_oversize_is_explicitly_rejected() {
        let (_root, store, prepared) = fixture();
        let saved = store
            .record_tool_side_effect(&prepared, evidence(), ToolContentKind::Output, "")
            .unwrap();
        let reference = saved.tool_output_ref.unwrap();
        assert_eq!(reference.chunk_count, 1);
        assert_eq!(
            store
                .read_tool_content("content-test", &reference.content_id, 0)
                .unwrap()
                .unwrap()
                .text,
            ""
        );
        let oversized = "x".repeat(MAX_CONTENT_BYTES + 1);
        let error = store
            .record_tool_side_effect(&prepared, evidence(), ToolContentKind::Output, &oversized)
            .unwrap_err();
        assert!(error.contains("64 MiB"));
        assert_eq!(
            store.get_session("content-test").unwrap().unwrap().turns[0].attempts[0].side_effects
                [0]
            .tool_output_ref
            .as_ref(),
            Some(&reference)
        );
    }

    #[test]
    fn schema_18_migration_keeps_legacy_previews_without_inventing_full_text() {
        let (_root, store, prepared) = fixture();
        let mut legacy = evidence();
        legacy.tool_output = Some("legacy preview".into());
        store.record_side_effect(&prepared, legacy).unwrap();
        let path = store.path().to_owned();
        drop(store);
        let connection = rusqlite::Connection::open(&path).unwrap();
        // Only this disposable fixture is rewound to the actual v18 schema.
        connection
            .execute_batch(
                "DROP TABLE tool_content_chunks; DROP TABLE tool_contents; PRAGMA user_version=18;",
            )
            .unwrap();
        drop(connection);
        let store = HubStore::open(path).unwrap();
        let detail = store.get_session("content-test").unwrap().unwrap();
        let effect = &detail.turns[0].attempts[0].side_effects[0];
        assert_eq!(effect.tool_output.as_deref(), Some("legacy preview"));
        assert!(effect.tool_output_ref.is_none());
        assert!(store
            .read_tool_content("content-test", "missing", 0)
            .unwrap()
            .is_none());
    }
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct ToolContentChunk {
    pub content_id: String,
    pub chunk_index: usize,
    pub chunk_count: usize,
    pub total_bytes: usize,
    pub text: String,
}

#[derive(Clone, Copy)]
pub(crate) enum ToolContentKind {
    Input,
    Output,
}

impl ToolContentKind {
    fn as_str(self) -> &'static str {
        match self {
            Self::Input => "input",
            Self::Output => "output",
        }
    }
}

impl HubStore {
    /// Full redacted text, its preview, and the evidence reference commit
    /// together. Immutable content IDs keep already-emitted references valid.
    pub(crate) fn record_tool_side_effect(
        &self,
        prepared: &PreparedTurn,
        mut evidence: SideEffectEvidence,
        kind: ToolContentKind,
        raw: &str,
    ) -> Result<SideEffectEvidence, String> {
        if raw.len() > MAX_CONTENT_BYTES {
            return Err(
                "tool text exceeds the 64 MiB storage limit; full text was not saved".into(),
            );
        }
        // Redact the WHOLE document before chunking, including boundary-spanning secrets.
        let text = crate::redact::redact_text(raw);
        if text.len() > MAX_CONTENT_BYTES {
            return Err(
                "redacted tool text exceeds the 64 MiB storage limit; full text was not saved"
                    .into(),
            );
        }
        let mut chunks = Vec::new();
        let mut start = 0;
        while start < text.len() {
            let mut end = (start + CHUNK_BYTES).min(text.len());
            while !text.is_char_boundary(end) {
                end -= 1;
            }
            chunks.push(&text[start..end]);
            start = end;
        }
        if chunks.is_empty() {
            chunks.push("");
        }
        let content = ToolContentRef {
            content_id: uuid::Uuid::new_v4().to_string(),
            total_bytes: text.len(),
            chunk_count: chunks.len(),
        };
        let mut connection = self.lock()?;
        let transaction = connection
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(|error| format!("begin tool content transaction: {error}"))?;
        transaction.execute(
            "INSERT INTO tool_contents (content_id, attempt_id, tool_call_id, kind, total_bytes, chunk_count)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            params![content.content_id, prepared.attempt_id, evidence.tool_call_id, kind.as_str(), content.total_bytes, content.chunk_count],
        ).map_err(|error| format!("persist tool content metadata: {error}"))?;
        {
            let mut insert = transaction.prepare(
                "INSERT INTO tool_content_chunks (content_id, chunk_index, body) VALUES (?1, ?2, ?3)"
            ).map_err(|error| format!("prepare tool content chunks: {error}"))?;
            for (index, chunk) in chunks.iter().enumerate() {
                insert
                    .execute(params![content.content_id, index, chunk])
                    .map_err(|error| format!("persist tool content chunk: {error}"))?;
            }
        }
        match kind {
            ToolContentKind::Input => {
                evidence.tool_input = Some(bounded_text(&text));
                evidence.tool_input_ref = Some(content);
            }
            ToolContentKind::Output => {
                evidence.tool_output = Some(bounded_text(&text));
                evidence.tool_output_ref = Some(content);
            }
        }
        let evidence = record_side_effect_in(&transaction, prepared, evidence)?;
        transaction
            .commit()
            .map_err(|error| format!("commit tool content transaction: {error}"))?;
        Ok(evidence)
    }

    /// Session ownership is checked in SQL, including for guessed content IDs.
    /// Reads exactly one bounded chunk, not the entire document or history.
    pub fn read_tool_content(
        &self,
        product_session_id: &str,
        content_id: &str,
        chunk_index: usize,
    ) -> Result<Option<ToolContentChunk>, String> {
        // Prevent out-of-range integers from reaching SQLite's signed binding.
        if chunk_index > MAX_CONTENT_BYTES {
            return Ok(None);
        }
        self.lock()?
            .query_row(
                "SELECT c.total_bytes, c.chunk_count, ch.body
             FROM tool_contents c
             JOIN engine_attempts a ON a.attempt_id = c.attempt_id
             JOIN tool_content_chunks ch ON ch.content_id = c.content_id
             WHERE c.content_id = ?1 AND a.product_session_id = ?2 AND ch.chunk_index = ?3",
                params![content_id, product_session_id, chunk_index],
                |row| {
                    Ok(ToolContentChunk {
                        content_id: content_id.to_owned(),
                        chunk_index,
                        total_bytes: row.get(0)?,
                        chunk_count: row.get(1)?,
                        text: row.get(2)?,
                    })
                },
            )
            .optional()
            .map_err(|error| format!("read tool content chunk: {error}"))
    }
}
