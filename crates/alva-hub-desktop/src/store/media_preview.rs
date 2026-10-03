// INPUT: Session-owned schema-24 media metadata/BLOBs and durable delivery receipts
// OUTPUT: MediaArtifactRef/Content/Page, MediaReceiptPage and read-only HubStore APIs
// POS: Product media inspection authority shared by Tauri and HTTP, never decoder or model execution.
use super::{media::MediaArtifact, HubStore};
use base64::Engine;
use rusqlite::{params, OptionalExtension};
use serde::Serialize;
use serde_json::Value;

#[derive(Debug, Serialize)]
pub struct MediaArtifactRef {
    pub artifact_id: String,
    pub source_attachment_id: String,
    pub name: String,
    pub total_bytes: usize,
    pub path: String,
    pub content_type: String,
    pub processing: Value,
}
#[derive(Debug, Serialize)]
pub struct MediaArtifactContent {
    #[serde(flatten)]
    pub reference: MediaArtifactRef,
    pub data_base64: String,
}
#[derive(Debug, Serialize)]
pub struct MediaArtifactPage {
    pub artifacts: Vec<MediaArtifactRef>,
    pub next_cursor: i64,
    pub has_more: bool,
}
#[derive(Debug, Serialize)]
pub struct MediaDeliveryReceipt {
    pub delivery_id: String,
    pub status: String,
    pub created_at: i64,
    pub updated_at: i64,
    pub selection: Value,
}
#[derive(Debug, Serialize)]
pub struct MediaReceiptPage {
    pub receipts: Vec<MediaDeliveryReceipt>,
    pub next_cursor: i64,
    pub through: i64,
    pub has_more: bool,
}

fn manifest(text: &str) -> Result<Value, String> {
    if text.len() > 16 * 1024 {
        return Err("Media metadata exceeds inspection budget".into());
    }
    let mut value: Value = serde_json::from_str(text).map_err(|_| "Media metadata unavailable")?;
    fn redact(value: &mut Value) {
        match value {
            Value::String(text) => *text = crate::redact::redact_text(text),
            Value::Array(items) => items.iter_mut().for_each(redact),
            Value::Object(items) => items.values_mut().for_each(redact),
            _ => {}
        }
    }
    redact(&mut value);
    Ok(value)
}
fn artifact_ref(
    id: String,
    source: &str,
    bytes: usize,
    path: String,
    kind: String,
    text: String,
) -> Result<MediaArtifactRef, String> {
    let processing = manifest(&text)?;
    let (name, maximum) = match kind.as_str() {
        "image/jpeg" => ("derived.jpg", 256 * 1024),
        "video/mp4" => ("derived.mp4", 8 * 1024 * 1024),
        _ => return Err("Media metadata unavailable".into()),
    };
    if bytes == 0
        || bytes > maximum
        || processing["artifact_id"] != id
        || processing["source_attachment_id"] != source
        || processing["output_bytes"] != bytes
        || processing["path"] != path
        || processing["content_type"] != kind
        || !processing["output_sha256"]
            .as_str()
            .is_some_and(|s| s.len() == 64 && s.bytes().all(|b| b.is_ascii_hexdigit()))
    {
        return Err("Media metadata unavailable".into());
    }
    Ok(MediaArtifactRef {
        artifact_id: id,
        source_attachment_id: source.into(),
        name: name.into(),
        total_bytes: bytes,
        path,
        content_type: kind,
        processing,
    })
}
fn source_owned(
    connection: &rusqlite::Connection,
    session: &str,
    attachment: &str,
) -> Result<(), String> {
    let owned:bool=connection.query_row("SELECT EXISTS(SELECT 1 FROM input_attachments a JOIN turns t ON t.turn_id=a.turn_id WHERE t.product_session_id=?1 AND a.attachment_id=?2)",params![session,attachment],|r|r.get(0)).map_err(|_|"Media unavailable")?;
    if owned {
        Ok(())
    } else {
        Err("Media unavailable".into())
    }
}

impl HubStore {
    /// Four metadata records only; no derivative body or filesystem read.
    pub fn list_attachment_media(
        &self,
        session: &str,
        attachment: &str,
        after: i64,
    ) -> Result<MediaArtifactPage, String> {
        if after < 0 {
            return Err("Invalid media cursor".into());
        }
        let connection = self.lock()?;
        source_owned(&connection, session, attachment)?;
        let mut query=connection.prepare("SELECT rowid,artifact_id,length(body),relative_path,content_type,CASE WHEN length(CAST(processing_json AS BLOB))<=16384 THEN processing_json END FROM media_artifacts WHERE attachment_id=?1 AND rowid>?2 ORDER BY rowid LIMIT 5").map_err(|_|"Media index unavailable")?;
        let rows = query
            .query_map(params![attachment, after], |r| {
                Ok((
                    r.get::<_, i64>(0)?,
                    r.get::<_, String>(1)?,
                    r.get::<_, usize>(2)?,
                    r.get::<_, String>(3)?,
                    r.get::<_, String>(4)?,
                    r.get::<_, String>(5)?,
                ))
            })
            .map_err(|_| "Media index unavailable")?;
        let mut rows = rows
            .collect::<rusqlite::Result<Vec<_>>>()
            .map_err(|_| "Media index unavailable")?;
        let has_more = rows.len() > 4;
        rows.truncate(4);
        let next_cursor = rows.last().map(|r| r.0).unwrap_or(after);
        let artifacts = rows
            .into_iter()
            .map(|r| artifact_ref(r.1, attachment, r.2, r.3, r.4, r.5))
            .collect::<Result<Vec<_>, _>>()?;
        Ok(MediaArtifactPage {
            artifacts,
            next_cursor,
            has_more,
        })
    }
    /// Reads a hash-verified private BLOB, not its mutable workspace copy.
    pub fn read_media_artifact(
        &self,
        session: &str,
        attachment: &str,
        id: &str,
    ) -> Result<MediaArtifactContent, String> {
        let reference = {
            let connection = self.lock()?;
            source_owned(&connection, session, attachment)?;
            let row:(usize,String,String,String)=connection.query_row("SELECT length(body),relative_path,content_type,CASE WHEN length(CAST(processing_json AS BLOB))<=16384 THEN processing_json END FROM media_artifacts WHERE artifact_id=?1 AND attachment_id=?2",params![id,attachment],|r|Ok((r.get(0)?,r.get(1)?,r.get(2)?,r.get(3)?))).optional().map_err(|_|"Media unavailable")?.ok_or("Media unavailable")?;
            artifact_ref(id.into(), attachment, row.0, row.1, row.2, row.3)?
        };
        let MediaArtifact { bytes, .. } = self.media_artifact(session, id)?;
        Ok(MediaArtifactContent {
            reference,
            data_base64: base64::engine::general_purpose::STANDARD.encode(bytes),
        })
    }
    /// Five receipts at a fixed row high-water; status may advance while inspected.
    pub fn media_artifact_receipts(
        &self,
        session: &str,
        attachment: &str,
        id: &str,
        after: i64,
        through: Option<i64>,
    ) -> Result<MediaReceiptPage, String> {
        if after < 0 || through.is_some_and(|v| v < after) {
            return Err("Invalid media receipt cursor".into());
        }
        let connection = self.lock()?;
        source_owned(&connection, session, attachment)?;
        let owned:bool=connection.query_row("SELECT EXISTS(SELECT 1 FROM media_artifacts WHERE artifact_id=?1 AND attachment_id=?2)",params![id,attachment],|r|r.get(0)).map_err(|_|"Media unavailable")?;
        if !owned {
            return Err("Media unavailable".into());
        }
        let through = match through {
            Some(value) => value,
            None => connection.query_row("SELECT COALESCE(MAX(rowid),0) FROM media_deliveries WHERE product_session_id=?1",[session],|r|r.get(0)).map_err(|_|"Media receipts unavailable")?,
        };
        if after > through {
            return Err("Invalid media receipt cursor".into());
        }
        let mut query=connection.prepare("SELECT d.rowid,d.delivery_id,d.status,d.created_at,d.updated_at,CASE WHEN length(CAST(d.selection_json AS BLOB))<=16384 THEN d.selection_json END FROM media_deliveries d WHERE d.product_session_id=?1 AND d.rowid>?2 AND d.rowid<=?3 AND (length(CAST(d.selection_json AS BLOB))>16384 OR EXISTS(SELECT 1 FROM json_each(CASE WHEN length(CAST(d.selection_json AS BLOB))<=16384 THEN d.selection_json ELSE '{}' END,'$.artifact_ids') WHERE value=?4)) ORDER BY d.rowid LIMIT 6").map_err(|_|"Media receipts unavailable")?;
        let rows = query
            .query_map(params![session, after, through, id], |r| {
                Ok((
                    r.get::<_, i64>(0)?,
                    r.get::<_, String>(1)?,
                    r.get::<_, String>(2)?,
                    r.get::<_, i64>(3)?,
                    r.get::<_, i64>(4)?,
                    r.get::<_, String>(5)?,
                ))
            })
            .map_err(|_| "Media receipts unavailable")?;
        let mut rows = rows
            .collect::<rusqlite::Result<Vec<_>>>()
            .map_err(|_| "Media receipts unavailable")?;
        let has_more = rows.len() > 5;
        rows.truncate(5);
        let next_cursor = rows.last().map(|r| r.0).unwrap_or(after);
        let receipts = rows
            .into_iter()
            .map(|r| {
                Ok(MediaDeliveryReceipt {
                    delivery_id: r.1,
                    status: r.2,
                    created_at: r.3,
                    updated_at: r.4,
                    selection: manifest(&r.5)?,
                })
            })
            .collect::<Result<Vec<_>, String>>()?;
        Ok(MediaReceiptPage {
            receipts,
            next_cursor,
            through,
            has_more,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{EngineTarget, InputAttachment, PermissionProfile};
    #[test]
    fn metadata_is_bounded_and_redacted_without_corrupting_json() {
        let value = manifest(
            r#"{"reason":"token=fixture-only-value","nested":["<script>inert</script>"]}"#,
        )
        .unwrap();
        assert!(!value.to_string().contains("fixture-only-value"));
        assert_eq!(value["nested"][0], "<script>inert</script>");
        assert!(manifest(&format!(r#"{{"reason":"{}"}}"#, "x".repeat(16 * 1024))).is_err());
        assert!(manifest("not json").is_err());
    }
    #[test]
    fn inspection_is_paged_scoped_verified_and_durable_without_workspace_reads() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("data/hub.db");
        let store = HubStore::open(&path).unwrap();
        let turn = store
            .prepare_new_turn(
                "media",
                temp.path(),
                "task",
                &EngineTarget::for_engine("alva").unwrap(),
                PermissionProfile::WorkspaceWrite,
            )
            .unwrap();
        store
            .save_input_attachments(
                &turn,
                &[InputAttachment {
                    name: "source.txt".into(),
                    text: "original".into(),
                    data_base64: None,
                }],
            )
            .unwrap();
        let source_id = store.input_attachment_refs("media", &turn.turn_id).unwrap()[0]
            .attachment_id
            .clone();
        let source = store.media_source("media", &source_id).unwrap();
        let mut ids = Vec::new();
        for _ in 0..6 {
            let result=store.save_media_artifacts("media",&source,vec![(b"\xff\xd8\xffsynthetic".to_vec(),"image/jpeg".into(),serde_json::json!({"method":"fixture_only","actual_timestamp_seconds":0.6,"reason":"<script>inert</script>"}))]).unwrap();
            ids.push(result[0]["artifact_id"].as_str().unwrap().to_string());
        }
        let first = store.list_attachment_media("media", &source_id, 0).unwrap();
        assert_eq!(first.artifacts.len(), 4);
        assert!(first.has_more);
        assert_eq!(
            store
                .list_attachment_media("media", &source_id, first.next_cursor)
                .unwrap()
                .artifacts
                .len(),
            2
        );
        assert!(store.list_attachment_media("other", &source_id, 0).is_err());
        assert!(store
            .read_media_artifact("other", &source_id, &ids[0])
            .is_err());
        assert!(store
            .read_media_artifact("media", "wrong-source", &ids[0])
            .is_err());
        let reference = &first.artifacts[0];
        std::fs::write(temp.path().join(&reference.path), b"changed workspace copy").unwrap();
        let preview = store
            .read_media_artifact("media", &source_id, &ids[0])
            .unwrap();
        assert_eq!(
            base64::engine::general_purpose::STANDARD
                .decode(preview.data_base64)
                .unwrap(),
            b"\xff\xd8\xffsynthetic"
        );
        for i in 0..7 {
            let plan = store
                .prepare_media_delivery(
                    "media",
                    "root",
                    &format!("call-{i}"),
                    std::slice::from_ref(&ids[0]),
                    "task",
                    "route",
                )
                .unwrap();
            store
                .transition_media_delivery(
                    "media",
                    "root",
                    plan["delivery_id"].as_str().unwrap(),
                    "prepared",
                    "not_submitted_cancelled",
                )
                .unwrap();
        }
        let receipts = store
            .media_artifact_receipts("media", &source_id, &ids[0], 0, None)
            .unwrap();
        assert_eq!(receipts.receipts.len(), 5);
        assert!(receipts.has_more);
        let plan = store
            .prepare_media_delivery(
                "media",
                "root",
                "late-call",
                std::slice::from_ref(&ids[0]),
                "late",
                "route",
            )
            .unwrap();
        store
            .submit_media_delivery(
                "media",
                "root",
                plan["delivery_id"].as_str().unwrap(),
                "actual-model",
                "backend",
            )
            .unwrap();
        let next = store
            .media_artifact_receipts(
                "media",
                &source_id,
                &ids[0],
                receipts.next_cursor,
                Some(receipts.through),
            )
            .unwrap();
        assert_eq!(next.receipts.len(), 2);
        assert!(!next.has_more);
        assert!(store
            .media_artifact_receipts("other", &source_id, &ids[0], 0, None)
            .is_err());
        assert!(store
            .media_artifact_receipts("media", &source_id, &ids[0], 1, Some(0))
            .is_err());
        assert!(store
            .media_artifact_receipts("media", &source_id, &ids[0], i64::MAX, None)
            .is_err());
        drop(store);
        let reopened = HubStore::open(path).unwrap();
        let receipts = reopened
            .media_artifact_receipts("media", &source_id, &ids[0], next.next_cursor, None)
            .unwrap();
        assert_eq!(receipts.receipts[0].status, "interrupted_unknown");
        assert_eq!(
            receipts.receipts[0].selection["actual_model_id"],
            "actual-model"
        );
        assert!(reopened
            .read_media_artifact("media", &source_id, &ids[0])
            .is_ok());
        reopened
            .lock()
            .unwrap()
            .execute(
                "UPDATE media_artifacts SET body=?1 WHERE artifact_id=?2",
                params![b"corrupt".to_vec(), ids[0]],
            )
            .unwrap();
        assert!(reopened
            .read_media_artifact("media", &source_id, &ids[0])
            .is_err());
        let oversized = serde_json::json!({"reason":"x".repeat(16 * 1024)}).to_string();
        reopened
            .lock()
            .unwrap()
            .execute(
                "UPDATE media_artifacts SET processing_json=?1 WHERE artifact_id=?2",
                params![oversized, ids[0]],
            )
            .unwrap();
        assert!(reopened
            .list_attachment_media("media", &source_id, 0)
            .is_err());
        assert!(reopened
            .read_media_artifact("media", &source_id, &ids[0])
            .is_err());
        reopened
            .lock()
            .unwrap()
            .execute("UPDATE media_deliveries SET selection_json=?1", [oversized])
            .unwrap();
        assert!(reopened
            .media_artifact_receipts("media", &source_id, &ids[0], 0, None)
            .is_err());
    }
}
