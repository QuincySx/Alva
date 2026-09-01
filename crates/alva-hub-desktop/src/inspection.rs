// INPUT:  HubStore routing, Alva adapter repository, EngineInspector snapshots, stable adapter projection
// OUTPUT: HubInspectorService and stable HubInspectionSnapshot IPC DTOs
// POS:    L7 inspection bridge that validates product/native session ownership before projecting durable engine records

use std::cmp::Reverse;
use std::collections::{HashSet, VecDeque};
use std::path::Path;

use alva_engine_adapter_alva::{
    build_run_record, RepositoryInspector, RunRecord, SessionRepository,
};
use alva_engine_runtime::{
    ChildSessionLink, EngineInspector, InspectionCapabilities, InspectionSnapshot,
};
use alva_kernel_abi::{
    ExecutionRecord, RecordCategory, SessionEvent, SessionRecord, SessionRecordPayload,
};
use serde::Serialize;

use crate::store::{HubSessionDetail, HubStore, StoredBranch};

const MAX_INSPECTION_SESSIONS: usize = 256;

#[derive(Clone)]
pub struct HubInspectorService {
    repository: SessionRepository,
}

#[derive(Debug, Clone, Serialize)]
pub struct HubInspectionRecord {
    pub seq: u64,
    pub record_id: String,
    pub parent_record_id: Option<String>,
    pub timestamp: i64,
    pub category: String,
    pub type_name: String,
    pub emitter: serde_json::Value,
    pub payload: serde_json::Value,
}

#[derive(Debug, Clone, Serialize)]
pub struct HubInspectionSnapshot {
    pub product_session_id: String,
    pub branch_id: String,
    pub root_native_session_id: String,
    pub engine_id: String,
    pub native_session_id: String,
    pub parent_session_id: Option<String>,
    pub high_water_seq: u64,
    pub live: bool,
    pub capabilities: InspectionCapabilities,
    pub run: RunRecord,
    pub records: Vec<HubInspectionRecord>,
    pub children: Vec<ChildSessionLink>,
}

impl HubInspectorService {
    pub fn open(history_dir: &Path) -> Result<Self, String> {
        Ok(Self {
            repository: SessionRepository::open(history_dir.join("alva-agent-records.db"))?,
        })
    }

    pub(crate) fn repository(&self) -> SessionRepository {
        self.repository.clone()
    }

    pub async fn inspect(
        &self,
        store: &HubStore,
        product_session_id: &str,
        native_session_id: Option<&str>,
    ) -> Result<HubInspectionSnapshot, String> {
        let detail = store
            .get_session(product_session_id)?
            .ok_or_else(|| format!("Hub session '{product_session_id}' was not found"))?;
        let candidates = alva_roots(&detail);
        if candidates.is_empty() {
            return Err(format!(
                "{} exposes no full native Inspector records",
                detail.session.engine_id
            ));
        }

        let inspector = RepositoryInspector::new(self.repository.clone());
        let capabilities = inspector.inspection_capabilities();
        let requested = native_session_id.filter(|id| !id.trim().is_empty());
        let mut last_error = None;

        for branch in candidates {
            let root_native_session_id = branch
                .engine_session_id
                .as_deref()
                .expect("filtered Alva branch must have a native session id");
            match snapshot_in_tree(&inspector, root_native_session_id, requested).await {
                Ok(Some(snapshot)) => {
                    let live = detail.session.status == "running"
                        && detail.session.active_branch_id == branch.branch_id;
                    return Ok(project_snapshot(
                        product_session_id,
                        &branch.branch_id,
                        root_native_session_id,
                        live,
                        capabilities,
                        snapshot,
                    ));
                }
                Ok(None) => {}
                Err(error) => last_error = Some(error),
            }
        }

        if let Some(requested) = requested {
            Err(format!(
                "native Inspector session '{requested}' does not belong to Hub session '{product_session_id}'"
            ))
        } else {
            Err(last_error.unwrap_or_else(|| {
                format!("No durable Alva Inspector records exist for '{product_session_id}'")
            }))
        }
    }
}

fn alva_roots(detail: &HubSessionDetail) -> Vec<&StoredBranch> {
    let active_branch_id = detail.session.active_branch_id.as_str();
    let mut candidates = detail
        .branches
        .iter()
        .filter(|branch| {
            branch.engine_id == "alva"
                && branch
                    .engine_session_id
                    .as_deref()
                    .is_some_and(|id| !id.trim().is_empty())
        })
        .collect::<Vec<_>>();
    candidates
        .sort_by_key(|branch| Reverse((branch.branch_id == active_branch_id, branch.updated_at)));
    candidates
}

async fn snapshot_in_tree(
    inspector: &dyn EngineInspector,
    root_native_session_id: &str,
    requested: Option<&str>,
) -> Result<Option<InspectionSnapshot>, String> {
    let root = inspector
        .snapshot(root_native_session_id)
        .await
        .map_err(|error| error.to_string())?;
    let target = requested.unwrap_or(root_native_session_id);
    if target == root_native_session_id {
        return Ok(Some(root));
    }

    let mut pending = root
        .children
        .iter()
        .map(|child| child.child_session_id.clone())
        .collect::<VecDeque<_>>();
    let mut visited = HashSet::from([root_native_session_id.to_string()]);
    while let Some(session_id) = pending.pop_front() {
        if !visited.insert(session_id.clone()) {
            continue;
        }
        if visited.len() > MAX_INSPECTION_SESSIONS {
            return Err(format!(
                "Inspector session tree exceeds the {MAX_INSPECTION_SESSIONS}-session safety limit"
            ));
        }
        let snapshot = inspector
            .snapshot(&session_id)
            .await
            .map_err(|error| error.to_string())?;
        if session_id == target {
            return Ok(Some(snapshot));
        }
        pending.extend(
            snapshot
                .children
                .iter()
                .map(|child| child.child_session_id.clone()),
        );
    }
    Ok(None)
}

fn project_snapshot(
    product_session_id: &str,
    branch_id: &str,
    root_native_session_id: &str,
    live: bool,
    capabilities: InspectionCapabilities,
    snapshot: InspectionSnapshot,
) -> HubInspectionSnapshot {
    let events = snapshot
        .records
        .iter()
        .map(projection_event)
        .collect::<Vec<_>>();
    let run = build_run_record(&events);
    let records = snapshot
        .records
        .into_iter()
        .map(|record| HubInspectionRecord {
            seq: record.seq,
            record_id: record.record_id,
            parent_record_id: record.parent_record_id,
            timestamp: record.timestamp,
            category: match record.category {
                RecordCategory::Conversation => "conversation",
                RecordCategory::Execution => "execution",
                RecordCategory::Extension => "extension",
            }
            .to_string(),
            type_name: record.type_name,
            emitter: serde_json::to_value(record.emitter).unwrap_or(serde_json::Value::Null),
            payload: serde_json::to_value(record.payload).unwrap_or(serde_json::Value::Null),
        })
        .collect();
    HubInspectionSnapshot {
        product_session_id: product_session_id.to_string(),
        branch_id: branch_id.to_string(),
        root_native_session_id: root_native_session_id.to_string(),
        engine_id: snapshot.engine_id,
        native_session_id: snapshot.native_session_id,
        parent_session_id: snapshot.parent_session_id,
        high_water_seq: snapshot.high_water_seq,
        live,
        capabilities,
        run,
        records,
        children: snapshot.children,
    }
}

fn projection_event(record: &SessionRecord) -> SessionEvent {
    let mut event = record.clone().into_event();
    if let SessionRecordPayload::Execution(ExecutionRecord::Event { data }) = &record.payload {
        event.data = Some(data.clone());
    }
    event
}

#[cfg(test)]
mod tests {
    use super::*;
    use alva_kernel_abi::{EventEmitter, SessionEvent};

    fn event(seq: u64, kind: &str, timestamp: i64, data: serde_json::Value) -> SessionRecord {
        let mut event = SessionEvent::new_runtime(kind);
        event.seq = seq;
        event.timestamp = timestamp;
        event.emitter = EventEmitter::runtime();
        event.data = Some(data);
        SessionRecord::from_event(event)
    }

    #[test]
    fn projects_compact_stream_observation_from_typed_records() {
        let records = vec![
            event(1, "run_start", 1_000, serde_json::json!({})),
            event(2, "iteration_start", 1_010, serde_json::json!({})),
            event(3, "llm_call_start", 1_020, serde_json::json!({})),
            event(
                4,
                "llm_stream_summary",
                1_060,
                serde_json::json!({
                    "first_event_ms": 4,
                    "time_to_first_output_ms": 8,
                    "time_to_first_token_ms": 12,
                    "stream_duration_ms": 40,
                    "event_count": 7,
                    "visible_event_count": 5,
                    "transport": "stream",
                    "outcome": "completed"
                }),
            ),
            event(
                5,
                "llm_call_end",
                1_070,
                serde_json::json!({"input_tokens": 3, "output_tokens": 2}),
            ),
            event(6, "iteration_end", 1_080, serde_json::json!({})),
            event(7, "run_end", 1_090, serde_json::json!({})),
        ];
        let snapshot = InspectionSnapshot {
            engine_id: "alva".into(),
            native_session_id: "native-1".into(),
            parent_session_id: None,
            high_water_seq: 7,
            records,
            children: Vec::new(),
        };
        let projected = project_snapshot(
            "product-1",
            "branch-1",
            "native-1",
            false,
            InspectionCapabilities::full(),
            snapshot,
        );

        assert_eq!(projected.run.turns.len(), 1);
        let llm = &projected.run.turns[0].llm_call;
        assert_eq!(llm.time_to_first_output_ms, Some(8));
        assert_eq!(llm.stream_duration_ms, Some(40));
        assert_eq!(llm.stream_event_count, 7);
        assert_eq!(projected.records.len(), 7);
    }
}
