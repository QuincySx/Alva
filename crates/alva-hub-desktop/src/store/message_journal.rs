// INPUT: Positioned product messages and private Hub SQLite connection
// OUTPUT: Atomic message journal append and session-scoped cursor pages
// POS: Durable conversation fragments, independent of bounded turn summaries.

use rusqlite::{params, TransactionBehavior};
use serde::Serialize;

use super::{HubStore, PreparedTurn};
use crate::{MessagePosition, TimelineEvent, TimelineStatus};

const CHUNK_BYTES: usize = 16 * 1024;
const PAGE_SIZE: usize = 128;

#[derive(Debug, Serialize)]
pub struct MessageJournalRecord {
    pub sequence: i64,
    pub turn_id: String,
    pub attempt_status: String,
    pub event: serde_json::Value,
}

#[derive(Debug, Serialize)]
pub struct MessageJournalPage {
    pub records: Vec<MessageJournalRecord>,
    pub next_cursor: i64,
    pub through: i64,
    pub has_more: bool,
}

impl HubStore {
    pub(crate) fn append_message_event(
        &self,
        prepared: &PreparedTurn,
        event: &TimelineEvent,
    ) -> Result<(), String> {
        let Some(position) = event.message_position.as_ref() else {
            return Ok(());
        };
        if event.product_session_id != prepared.product_session_id
            || position.stream_id != prepared.attempt_id
        {
            return Err("message journal identity mismatch".into());
        }
        let text = event.message.as_deref().unwrap_or_default();
        if text.len() > 64 * 1024 * 1024 {
            return Err("message fragment exceeds 64 MiB".into());
        }
        let mut connection = self.lock()?;
        let transaction = connection
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(|_| "message journal unavailable")?;
        let running: bool = transaction.query_row(
            "SELECT EXISTS(SELECT 1 FROM engine_attempts WHERE attempt_id=?1 AND product_session_id=?2 AND status='running')",
            params![prepared.attempt_id, prepared.product_session_id], |row| row.get(0)
        ).map_err(|_| "message attempt unavailable")?;
        if !running {
            return Err("message attempt is no longer running".into());
        }
        let mut stored = event.clone();
        // A provider may emit only Completed(result), without preceding deltas.
        // Store its full text as positioned fragments plus one terminal marker.
        let stores_text = event.status == TimelineStatus::AssistantMessage || position.offset == 0;
        let mut offset = position.offset;
        if stores_text && !text.is_empty() {
            stored.status = TimelineStatus::AssistantMessage;
            let mut rest = text;
            while !rest.is_empty() {
                let mut end = rest.len().min(CHUNK_BYTES);
                while !rest.is_char_boundary(end) {
                    end -= 1;
                }
                let chunk = &rest[..end];
                stored.message = Some(chunk.to_string());
                stored.message_position = Some(MessagePosition {
                    stream_id: position.stream_id.clone(),
                    offset,
                });
                let json =
                    serde_json::to_string(&stored).map_err(|_| "message serialization failed")?;
                transaction
                    .execute(
                        "INSERT INTO message_journal(attempt_id,event_json) VALUES(?1,?2)",
                        params![prepared.attempt_id, json],
                    )
                    .map_err(|_| "message journal write failed")?;
                offset += chunk.encode_utf16().count();
                rest = &rest[end..];
            }
        }
        if event.status != TimelineStatus::AssistantMessage {
            stored.status = event.status.clone();
            stored.message = None;
            stored.message_position = Some(MessagePosition {
                stream_id: position.stream_id.clone(),
                offset,
            });
            transaction
                .execute(
                    "INSERT INTO message_journal(attempt_id,event_json) VALUES(?1,?2)",
                    params![
                        prepared.attempt_id,
                        serde_json::to_string(&stored)
                            .map_err(|_| "message serialization failed")?
                    ],
                )
                .map_err(|_| "message journal write failed")?;
        }
        transaction
            .commit()
            .map_err(|_| "message journal commit failed".to_string())
    }

    pub fn message_journal_page(
        &self,
        session: &str,
        after: i64,
        through: Option<i64>,
    ) -> Result<MessageJournalPage, String> {
        if after < 0 || through.is_some_and(|value| value < after) {
            return Err("invalid message cursor".into());
        }
        let connection = self.lock()?;
        let maximum: i64 = connection.query_row(
            "SELECT COALESCE(MAX(j.sequence),0) FROM message_journal j JOIN engine_attempts a ON a.attempt_id=j.attempt_id WHERE a.product_session_id=?1",
            [session], |row| row.get(0)).map_err(|_| "message cursor unavailable")?;
        let through = through.unwrap_or(maximum).min(maximum).max(after);
        let mut query = connection.prepare(
            "SELECT j.sequence,a.turn_id,a.status,j.event_json FROM message_journal j JOIN engine_attempts a ON a.attempt_id=j.attempt_id
             WHERE a.product_session_id=?1 AND j.sequence>=?2 AND j.sequence<=?3 ORDER BY j.sequence LIMIT ?4"
        ).map_err(|_| "message reader unavailable")?;
        let rows = query
            .query_map(
                params![session, after, through, (PAGE_SIZE + 1) as i64],
                |row| {
                    Ok((
                        row.get::<_, i64>(0)?,
                        row.get::<_, String>(1)?,
                        row.get::<_, String>(2)?,
                        row.get::<_, String>(3)?,
                    ))
                },
            )
            .map_err(|_| "message page unavailable")?;
        let mut records = Vec::new();
        for row in rows {
            let (sequence, turn_id, attempt_status, json) =
                row.map_err(|_| "message record unavailable")?;
            records.push(MessageJournalRecord {
                sequence,
                turn_id,
                attempt_status,
                event: serde_json::from_str(&json).map_err(|_| "invalid durable message")?,
            });
        }
        let has_more = records.len() > PAGE_SIZE;
        records.truncate(PAGE_SIZE);
        let next_cursor = records
            .last()
            .map(|record| record.sequence)
            .unwrap_or(after);
        Ok(MessageJournalPage {
            records,
            next_cursor,
            through,
            has_more,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{EngineTarget, PermissionProfile};

    #[test]
    fn full_unicode_messages_survive_reopen_and_cursor_pages_are_session_scoped() {
        let temp = tempfile::tempdir().unwrap();
        let database = temp.path().join("data/hub.db");
        let store = HubStore::open(&database).unwrap();
        let prepared = store
            .prepare_new_turn(
                "messages",
                temp.path(),
                "test",
                &EngineTarget::for_engine("alva").unwrap(),
                PermissionProfile::WorkspaceWrite,
            )
            .unwrap();
        let text = "🙂全文".repeat(400_000);
        let event = TimelineEvent {
            status: TimelineStatus::Completed,
            engine_id: "alva".into(),
            product_session_id: "messages".into(),
            created_at: 1,
            message: Some(text.clone()),
            item_id: Some("native".into()),
            activity: None,
            permission_request: None,
            message_position: Some(MessagePosition {
                stream_id: prepared.attempt_id.clone(),
                offset: 0,
            }),
        };
        store.append_message_event(&prepared, &event).unwrap();
        drop(store);
        let store = HubStore::open(&database).unwrap();
        let mut cursor = 0;
        let mut reconstructed = String::new();
        let mut through = None;
        let mut pages = 0;
        loop {
            let page = store
                .message_journal_page("messages", cursor, through)
                .unwrap();
            assert!(page.records.len() <= PAGE_SIZE);
            for record in &page.records {
                assert_eq!(record.attempt_status, "interrupted");
                if record.sequence > cursor {
                    reconstructed.push_str(record.event["message"].as_str().unwrap_or_default());
                }
            }
            cursor = page.next_cursor;
            through = Some(page.through);
            pages += 1;
            if !page.has_more {
                break;
            }
        }
        assert!(pages > 1);
        assert_eq!(reconstructed, text);
        assert!(store
            .message_journal_page("other", 0, None)
            .unwrap()
            .records
            .is_empty());
        // Boundary replay observes a later terminal/crash repair even when
        // no new fragment was appended; the client deduplicates by position.
        assert_eq!(
            store
                .message_journal_page("messages", cursor, None)
                .unwrap()
                .records
                .len(),
            1
        );
        assert!(store.append_message_event(&prepared, &event).is_err());
    }
}
