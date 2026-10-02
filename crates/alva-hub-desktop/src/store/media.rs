// INPUT: Private attachment BLOBs, saved session cwd, derived bytes and delivery identities
// OUTPUT: Verified media sources/artifacts and atomic scoped delivery receipts
// POS: Hub product media authority; originals remain immutable, paths never authorize reads.
use super::{input_attachments::attachment_directory, now_unix_ms, HubStore};
use cap_fs_ext::{FollowSymlinks, OpenOptionsFollowExt};
use cap_std::fs::OpenOptions;
use rusqlite::{params, OptionalExtension};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::{io::Write, path::PathBuf};

pub(crate) fn sha(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}
pub(crate) struct MediaSource {
    pub id: String,
    pub bytes: Vec<u8>,
    pub content_type: String,
    pub hash: String,
    pub workspace: PathBuf,
}
pub(crate) struct MediaArtifact {
    pub bytes: Vec<u8>,
    pub content_type: String,
    pub manifest: Value,
}

impl HubStore {
    pub(crate) fn media_source(&self, session: &str, id: &str) -> Result<MediaSource, String> {
        let connection = self.lock()?;
        let (bytes,kind,manifest,workspace):(Vec<u8>,String,String,String)=connection.query_row(
            "SELECT a.body,a.content_type,a.processing_json,h.workspace FROM input_attachments a JOIN turns t ON t.turn_id=a.turn_id JOIN hub_sessions h ON h.product_session_id=t.product_session_id WHERE t.product_session_id=?1 AND a.attachment_id=?2 AND a.total_bytes<=20971520",
            params![session,id],|r|Ok((r.get(0)?,r.get(1)?,r.get(2)?,r.get(3)?))).optional().map_err(|_|"Media source unavailable")?.ok_or("Media source unavailable")?;
        let manifest: Value =
            serde_json::from_str(&manifest).map_err(|_| "Media source corrupt")?;
        let hash = sha(&bytes);
        if bytes.len() > 20 * 1024 * 1024 || manifest["source_sha256"] != hash {
            return Err("Media source corrupt".into());
        }
        Ok(MediaSource {
            id: id.into(),
            bytes,
            content_type: kind,
            hash,
            workspace: workspace.into(),
        })
    }
    pub(crate) fn media_snapshot(
        &self,
        session: &str,
        source: &MediaSource,
    ) -> Result<std::fs::File, String> {
        validate_session(session)?;
        let folder = format!(
            ".alva/sessions/{session}/media-jobs/{}",
            uuid::Uuid::new_v4()
        );
        let directory = attachment_directory(&source.workspace, &folder)?;
        let mut options = OpenOptions::new();
        options
            .read(true)
            .write(true)
            .create_new(true)
            .follow(FollowSymlinks::No);
        #[cfg(unix)]
        {
            use cap_std::fs::OpenOptionsExt;
            options.mode(0o600);
        }
        let mut file = directory
            .open_with("source", &options)
            .map_err(|_| "Media snapshot unavailable")?;
        file.write_all(&source.bytes)
            .map_err(|_| "Media snapshot unavailable")?;
        file.sync_all().map_err(|_| "Media snapshot unavailable")?;
        let mut read_options = OpenOptions::new();
        read_options.read(true).follow(FollowSymlinks::No);
        let readonly = directory
            .open_with("source", &read_options)
            .map_err(|_| "Read-only media snapshot unavailable")?;
        drop(file);
        // Only this new processing snapshot is unlinked, not an uploaded original.
        // The decoder consumes its already-open descriptor; no retained duplicate.
        directory
            .remove_file("source")
            .map_err(|_| "Media snapshot cannot be unlinked")?;
        Ok(readonly.into_std())
    }
    pub(crate) fn save_media_artifacts(
        &self,
        session: &str,
        source: &MediaSource,
        outputs: Vec<(Vec<u8>, String, Value)>,
    ) -> Result<Vec<Value>, String> {
        validate_session(session)?;
        if outputs.is_empty() || outputs.len() > 4 {
            return Err("Invalid media output count".into());
        }
        let mut connection = self.lock()?;
        let transaction = connection
            .transaction()
            .map_err(|_| "Media storage unavailable")?;
        let count: usize = transaction
            .query_row(
                "SELECT COUNT(*) FROM media_artifacts WHERE attachment_id=?1",
                [&source.id],
                |r| r.get(0),
            )
            .map_err(|_| "Media storage unavailable")?;
        if count + outputs.len() > 64 {
            return Err("Attachment already has 64 derived artifacts".into());
        }
        let owned:bool=transaction.query_row("SELECT EXISTS(SELECT 1 FROM input_attachments a JOIN turns t ON t.turn_id=a.turn_id WHERE t.product_session_id=?1 AND a.attachment_id=?2)",params![session,source.id],|r|r.get(0)).map_err(|_|"Media ownership unavailable")?;
        if !owned {
            return Err("Media source unavailable".into());
        }
        let mut results = Vec::new();
        for (bytes, content_type, mut manifest) in outputs {
            let maximum = if content_type == "image/jpeg" {
                256 * 1024
            } else if content_type == "video/mp4" {
                8 * 1024 * 1024
            } else {
                return Err("Invalid media output type".into());
            };
            if bytes.is_empty() || bytes.len() > maximum {
                return Err("Media output exceeds byte budget".into());
            }
            let id = uuid::Uuid::new_v4().to_string();
            let folder = format!(".alva/sessions/{session}/media/{id}");
            let filename = if content_type == "image/jpeg" {
                "derived.jpg"
            } else {
                "derived.mp4"
            };
            let path = format!("{folder}/{filename}");
            let directory = attachment_directory(&source.workspace, &folder)?;
            let mut options = OpenOptions::new();
            options
                .write(true)
                .create_new(true)
                .follow(FollowSymlinks::No);
            #[cfg(unix)]
            {
                use cap_std::fs::OpenOptionsExt;
                options.mode(0o600);
            }
            let mut file = directory
                .open_with(filename, &options)
                .map_err(|_| "Media output file unavailable")?;
            file.write_all(&bytes)
                .map_err(|_| "Media output file unavailable")?;
            file.sync_all()
                .map_err(|_| "Media output file unavailable")?;
            manifest["source_attachment_id"] = json!(source.id);
            manifest["source_sha256"] = json!(source.hash);
            manifest["source_bytes"] = json!(source.bytes.len());
            manifest["artifact_id"] = json!(id);
            manifest["output_sha256"] = json!(sha(&bytes));
            manifest["output_bytes"] = json!(bytes.len());
            manifest["content_type"] = json!(content_type);
            manifest["path"] = json!(path);
            manifest["original_preserved"] = json!(true);
            manifest["delivery_scope"] = json!("derivation_only_not_model_delivery");
            transaction.execute("INSERT INTO media_artifacts(artifact_id,attachment_id,body,content_type,relative_path,processing_json,created_at) VALUES(?1,?2,?3,?4,?5,?6,?7)",params![id,source.id,bytes,content_type,path,manifest.to_string(),now_unix_ms()]).map_err(|_|"Media storage unavailable")?;
            results.push(json!({"artifact_id":id,"content_type":content_type,"bytes":bytes.len(),"path":path,"actual_timestamp_seconds":manifest["actual_timestamp_seconds"],"method":manifest["method"],"delivery":"not_sent_by_derivation"}));
        }
        transaction
            .commit()
            .map_err(|_| "Media storage unavailable")?;
        Ok(results)
    }
    pub(crate) fn media_artifact(&self, session: &str, id: &str) -> Result<MediaArtifact, String> {
        let (bytes,content_type,path,manifest):(Vec<u8>,String,String,String)=self.lock()?.query_row("SELECT m.body,m.content_type,m.relative_path,m.processing_json FROM media_artifacts m JOIN input_attachments a ON a.attachment_id=m.attachment_id JOIN turns t ON t.turn_id=a.turn_id WHERE t.product_session_id=?1 AND m.artifact_id=?2 AND length(m.body)<=8388608",params![session,id],|r|Ok((r.get(0)?,r.get(1)?,r.get(2)?,r.get(3)?))).optional().map_err(|_|"Media artifact unavailable")?.ok_or("Media artifact unavailable")?;
        let manifest: Value =
            serde_json::from_str(&manifest).map_err(|_| "Media artifact corrupt")?;
        if manifest["output_sha256"] != sha(&bytes)
            || manifest["output_bytes"] != bytes.len()
            || manifest["artifact_id"] != id
            || manifest["path"] != path
            || manifest["content_type"] != content_type
        {
            return Err("Media artifact corrupt".into());
        }
        Ok(MediaArtifact {
            bytes,
            content_type,
            manifest,
        })
    }
    pub(crate) fn media_index(
        &self,
        session: &str,
        attachment: &str,
        after: i64,
    ) -> Result<Value, String> {
        if after < 0 {
            return Err("Invalid media cursor".into());
        }
        let connection = self.lock()?;
        let mut query=connection.prepare("SELECT m.rowid,m.artifact_id,m.content_type,m.relative_path,length(m.body) FROM media_artifacts m JOIN input_attachments a ON a.attachment_id=m.attachment_id JOIN turns t ON t.turn_id=a.turn_id WHERE t.product_session_id=?1 AND a.attachment_id=?2 AND m.rowid>?3 ORDER BY m.rowid LIMIT 5").map_err(|_|"Media index unavailable")?;
        let mut rows=query.query_map(params![session,attachment,after],|r|Ok((r.get::<_,i64>(0)?,json!({"artifact_id":r.get::<_,String>(1)?,"content_type":r.get::<_,String>(2)?,"path":r.get::<_,String>(3)?,"bytes":r.get::<_,usize>(4)?})))).map_err(|_|"Media index unavailable")?.collect::<rusqlite::Result<Vec<_>>>().map_err(|_|"Media index unavailable")?;
        let has_more = rows.len() > 4;
        rows.truncate(4);
        let next = rows.last().map(|r| r.0).unwrap_or(after);
        Ok(
            json!({"artifacts":rows.into_iter().map(|r|r.1).collect::<Vec<_>>(),"has_more":has_more,"next_cursor":next}),
        )
    }
    pub(crate) fn prepare_media_delivery(
        &self,
        session: &str,
        native: &str,
        call: &str,
        ids: &[String],
        reason: &str,
        route: &str,
    ) -> Result<Value, String> {
        if ids.is_empty()
            || ids.len() > 2
            || reason.is_empty()
            || reason.chars().count() > 200
            || ids.len() != ids.iter().collect::<std::collections::HashSet<_>>().len()
        {
            return Err("Select 1–2 distinct images and a 1–200 character reason".into());
        }
        let mut artifacts = Vec::new();
        for id in ids {
            let artifact = self.media_artifact(session, id)?;
            if artifact.content_type != "image/jpeg" || artifact.bytes.len() > 256 * 1024 {
                return Err("Only bounded derived JPEG images may be selected, not original images or video".into());
            }
            artifacts.push(json!({"artifact_id":id,"source_attachment_id":artifact.manifest["source_attachment_id"],"source_sha256":artifact.manifest["source_sha256"],"output_sha256":artifact.manifest["output_sha256"],"output_bytes":artifact.bytes.len(),"actual_timestamp_seconds":artifact.manifest["actual_timestamp_seconds"],"method":artifact.manifest["method"]}));
        }
        let id = uuid::Uuid::new_v4().to_string();
        let now = now_unix_ms();
        let plan = json!({"delivery_id":id,"artifact_ids":ids,"artifacts":artifacts,"reason":reason,"model_route":route,"image_capability":"declared_by_product_route","originals_sent":false,"only_selected_derivatives":true});
        let connection = self.lock()?;
        let count: usize = connection
            .query_row(
                "SELECT COUNT(*) FROM media_deliveries WHERE native_session_id=?1",
                [native],
                |r| r.get(0),
            )
            .map_err(|_| "Media receipt unavailable")?;
        if count >= 128 {
            return Err("Native session already has 128 media delivery selections".into());
        }
        let pending:usize=connection.query_row("SELECT COALESCE(SUM(json_array_length(selection_json,'$.artifact_ids')),0) FROM media_deliveries WHERE native_session_id=?1 AND status='prepared'",[native],|r|r.get(0)).map_err(|_|"Media receipt unavailable")?;
        if pending + ids.len() > 2 {
            return Err("Already selected images for the next request; wait for their receipt before selecting more".into());
        }
        connection.execute("INSERT INTO media_deliveries(delivery_id,product_session_id,native_session_id,tool_call_id,selection_json,status,created_at,updated_at) VALUES(?1,?2,?3,?4,?5,'prepared',?6,?6)",params![id,session,native,call,plan.to_string(),now]).map_err(|_|"Media receipt unavailable")?;
        Ok(
            json!({"delivery_id":id,"status":"prepared_not_sent","artifact_ids":ids,"instruction":"Only this explicit selection may be attached to the next model request; use receipt to inspect actual outcome."}),
        )
    }
    pub(crate) fn media_receipt(
        &self,
        session: &str,
        native: &str,
        id: &str,
    ) -> Result<(Value, String, String), String> {
        let (text,status,call):(String,String,String)=self.lock()?.query_row("SELECT selection_json,status,tool_call_id FROM media_deliveries WHERE product_session_id=?1 AND native_session_id=?2 AND delivery_id=?3",params![session,native,id],|r|Ok((r.get(0)?,r.get(1)?,r.get(2)?))).optional().map_err(|_|"Media receipt unavailable")?.ok_or("Media receipt unavailable")?;
        let mut plan: Value = serde_json::from_str(&text).map_err(|_| "Media receipt corrupt")?;
        plan["status"] = json!(status);
        plan["outcome_notice"]=json!("submitted means a model request was attempted, not guaranteed provider acceptance. response_received is not proof of visual understanding. Errors/interruption can leave acceptance unknown; selections are not automatically replayed.");
        Ok((plan, status, call))
    }
    pub(crate) fn transition_media_delivery(
        &self,
        session: &str,
        native: &str,
        id: &str,
        from: &str,
        to: &str,
    ) -> Result<(), String> {
        let changed=self.lock()?.execute("UPDATE media_deliveries SET status=?1,updated_at=?2 WHERE product_session_id=?3 AND native_session_id=?4 AND delivery_id=?5 AND status=?6",params![to,now_unix_ms(),session,native,id,from]).map_err(|_|"Media receipt unavailable")?;
        if changed != 1 {
            return Err("Media selection changed; no automatic replay".into());
        }
        Ok(())
    }
    pub(crate) fn submit_media_delivery(
        &self,
        session: &str,
        native: &str,
        id: &str,
        model: &str,
        provider: &str,
    ) -> Result<(), String> {
        let changed=self.lock()?.execute("UPDATE media_deliveries SET status='submitted',updated_at=?1,selection_json=json_set(selection_json,'$.actual_model_id',?2,'$.provider_backend',?3) WHERE product_session_id=?4 AND native_session_id=?5 AND delivery_id=?6 AND status='prepared'",params![now_unix_ms(),model,provider,session,native,id]).map_err(|_|"Media receipt unavailable")?;
        if changed != 1 {
            return Err("Media selection changed; no automatic replay".into());
        }
        Ok(())
    }
    pub(crate) fn settle_media_deliveries(
        &self,
        session: &str,
        native: &str,
    ) -> Result<(), String> {
        self.lock()?.execute("UPDATE media_deliveries SET status=CASE WHEN status='submitted' THEN 'interrupted_unknown' ELSE 'not_submitted_before_run_end' END,updated_at=?1 WHERE product_session_id=?2 AND native_session_id=?3 AND status IN ('prepared','submitted')",params![now_unix_ms(),session,native]).map_err(|_|"Media receipts could not be settled")?;
        Ok(())
    }
}
fn validate_session(session: &str) -> Result<(), String> {
    if session.is_empty()
        || session.len() > 128
        || !session
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
    {
        return Err("Invalid media session identity".into());
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{EngineTarget, InputAttachment, PermissionProfile};
    fn fixture() -> (tempfile::TempDir, HubStore, MediaSource) {
        let temp = tempfile::tempdir().unwrap();
        let store = HubStore::open(temp.path().join("data/hub.db")).unwrap();
        let turn = store
            .prepare_new_turn(
                "media",
                temp.path(),
                "media",
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
        let id = store.input_attachment_refs("media", &turn.turn_id).unwrap()[0]
            .attachment_id
            .clone();
        let source = store.media_source("media", &id).unwrap();
        (temp, store, source)
    }
    fn artifact(store: &HubStore, source: &MediaSource) -> String {
        store
            .save_media_artifacts(
                "media",
                source,
                vec![(
                    b"\xff\xd8\xff synthetic-owned-output".to_vec(),
                    "image/jpeg".into(),
                    json!({"method":"test_only","actual_timestamp_seconds":0.6}),
                )],
            )
            .unwrap()[0]["artifact_id"]
            .as_str()
            .unwrap()
            .into()
    }
    #[test]
    fn scoped_receipts_snapshot_hashes_cancel_and_repair_without_replay() {
        let (temp, store, source) = fixture();
        let artifact = artifact(&store, &source);
        assert!(store.media_source("other", &source.id).is_err());
        let first = store
            .prepare_media_delivery(
                "media",
                "root",
                "call-1",
                std::slice::from_ref(&artifact),
                "task",
                "route",
            )
            .unwrap();
        let id = first["delivery_id"].as_str().unwrap();
        let (receipt, _, _) = store.media_receipt("media", "root", id).unwrap();
        assert_eq!(receipt["artifacts"][0]["source_sha256"], source.hash);
        assert_eq!(receipt["artifacts"][0]["actual_timestamp_seconds"], 0.6);
        assert!(store.media_receipt("other", "root", id).is_err());
        assert!(store.media_receipt("media", "child", id).is_err());
        store
            .transition_media_delivery("media", "root", id, "prepared", "not_submitted_cancelled")
            .unwrap();
        assert!(store
            .transition_media_delivery("media", "root", id, "prepared", "submitted")
            .is_err());
        let submitted = store
            .prepare_media_delivery(
                "media",
                "root",
                "call-2",
                std::slice::from_ref(&artifact),
                "task",
                "route",
            )
            .unwrap();
        store
            .transition_media_delivery(
                "media",
                "root",
                submitted["delivery_id"].as_str().unwrap(),
                "prepared",
                "submitted",
            )
            .unwrap();
        let pending = store
            .prepare_media_delivery(
                "media",
                "root",
                "call-3",
                std::slice::from_ref(&artifact),
                "task",
                "route",
            )
            .unwrap();
        drop(store);
        let store = HubStore::open(temp.path().join("data/hub.db")).unwrap();
        assert_eq!(
            store
                .media_receipt("media", "root", submitted["delivery_id"].as_str().unwrap())
                .unwrap()
                .1,
            "interrupted_unknown"
        );
        assert_eq!(
            store
                .media_receipt("media", "root", pending["delivery_id"].as_str().unwrap())
                .unwrap()
                .1,
            "not_submitted_interrupted"
        );
        assert_eq!(
            store.media_artifact("media", &artifact).unwrap().manifest["original_preserved"],
            true
        );
    }
    #[test]
    fn corrupt_artifacts_size_limits_and_schema_23_migration_fail_closed() {
        let (_temp, store, source) = fixture();
        let id = artifact(&store, &source);
        store
            .lock()
            .unwrap()
            .execute(
                "UPDATE media_artifacts SET body=?1 WHERE artifact_id=?2",
                params![b"changed".to_vec(), id],
            )
            .unwrap();
        assert!(store.media_artifact("media", &id).is_err());
        assert!(store
            .prepare_media_delivery("media", "root", "call", &[id], "task", "route")
            .is_err());
        assert!(store
            .save_media_artifacts(
                "media",
                &source,
                vec![(vec![0; 256 * 1024 + 1], "image/jpeg".into(), json!({}))]
            )
            .is_err());
        store
            .lock()
            .unwrap()
            .execute_batch(
                "DROP TABLE media_deliveries; DROP TABLE media_artifacts; PRAGMA user_version=23;",
            )
            .unwrap();
        let path = store.path().to_path_buf();
        drop(store);
        let store = HubStore::open(path).unwrap();
        assert_eq!(
            store.media_source("media", &source.id).unwrap().bytes,
            b"original"
        );
        assert_eq!(
            store.media_index("media", &source.id, 0).unwrap()["artifacts"],
            json!([])
        );
    }
}
