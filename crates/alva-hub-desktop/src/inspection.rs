// INPUT:  HubStore routing, Alva adapter repository, EngineInspector snapshots, stable adapter projection
// OUTPUT: HubInspectorService with scoped read-only MCP journal queries and reference-only MCP snapshot DTOs
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

    /// Explicit UI inspection only: never constructs an Agent or MCP connection.
    pub async fn mcp_journal(
        &self,
        store: &HubStore,
        product_session_id: &str,
        native_session_id: &str,
        input: serde_json::Value,
    ) -> Result<serde_json::Value, String> {
        let unavailable = || "MCP journal unavailable in this session".to_string();
        if native_session_id.is_empty() || native_session_id.len() > 128 {
            return Err(unavailable());
        }
        let detail = store
            .get_session(product_session_id)
            .map_err(|_| unavailable())?
            .ok_or_else(unavailable)?;
        let roots: Vec<String> = alva_roots(&detail)
            .iter().filter_map(|branch| branch.engine_session_id.clone()).collect();
        let repository = self.repository.clone();
        let target = native_session_id.to_string();
        let authorized = tokio::task::spawn_blocking(move || {
            if !repository.contains(&target).map_err(|_| ())? {
                return Ok(false);
            }
            let mut current = target;
            let mut visited = HashSet::new();
            for _ in 0..MAX_INSPECTION_SESSIONS {
                if !visited.insert(current.clone()) {
                    return Err(());
                }
                if roots.contains(&current) {
                    return Ok(true);
                }
                let Some(parent) = repository.linked_parent(&current).map_err(|_| ())? else {
                    return Ok(false);
                };
                current = parent;
            }
            Err(())
        }).await;
        if !matches!(authorized, Ok(Ok(true))) {
            return Err(unavailable());
        }
        alva_engine_adapter_alva::ContextRetrievalPlugin::new(
            self.repository.clone(), native_session_id.into(),
        )
        .inspect_mcp_journal(input).await.map_err(|_| unavailable())
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
        .map(|record| {
            // Periodic Inspector snapshots must not ship entire MCP frames.
            // Explicit journal queries retrieve the selected verified text page.
            let payload = match &record.payload {
                SessionRecordPayload::Extension(extension)
                    if extension.namespace == "alva.mcp"
                        && record.type_name == "mcp_io"
                        && record.schema_version == 1 =>
                {
                    let label = |field: &str| {
                        extension.payload[field]
                            .as_str().unwrap_or("").chars().take(128).collect::<String>()
                    };
                    serde_json::json!({"mcp_journal_reference":{"record_id":record.record_id,
                        "server_id":label("server_id"),"direction":label("direction")}})
                }
                _ => serde_json::to_value(&record.payload).unwrap_or(serde_json::Value::Null),
            };
            HubInspectionRecord {
                seq: record.seq,
                record_id: record.record_id,
                parent_record_id: record.parent_record_id,
                timestamp: record.timestamp,
                category: match record.category {
                    RecordCategory::Conversation => "conversation",
                    RecordCategory::Execution => "execution",
                    RecordCategory::Extension => "extension",
                }.to_string(),
                type_name: record.type_name,
                emitter: serde_json::to_value(record.emitter).unwrap_or(serde_json::Value::Null),
                payload,
            }
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
    use alva_kernel_abi::{ConversationLog, EventEmitter, RecordValue, SessionEvent, SessionStore};

    fn bind_native_session(
        store: &HubStore,
        workspace: &Path,
        product: &str,
        engine: &str,
        native: &str,
    ) -> String {
        let target = crate::EngineTarget::for_engine(engine).unwrap();
        let prepared = store
            .prepare_new_turn(
                product,
                workspace,
                "Inspector fixture",
                &target,
                crate::PermissionProfile::WorkspaceWrite,
            )
            .unwrap();
        store.record_engine_session(&prepared, native).unwrap();
        store
            .finish_turn(&prepared, Some(native), Some("done"), None, None)
            .unwrap();
        prepared.branch_id
    }

    #[tokio::test]
    async fn mcp_journal_reads_actual_descendants_without_replay_or_full_snapshot_delivery() {
        let directory = tempfile::tempdir().unwrap();
        let store = HubStore::open(directory.path().join("hub.db")).unwrap();
        bind_native_session(&store, directory.path(), "product-a", "alva", "root");
        bind_native_session(&store, directory.path(), "product-b", "alva", "other");
        let service = HubInspectorService::open(directory.path()).unwrap();
        let repository = service.repository();
        let root = repository.open_session("root", None).await.unwrap();
        let child = root.create_child_session("run", "spawn").await.unwrap().unwrap();
        let grandchild = child.create_child_session("child-run", "grand-spawn").await.unwrap().unwrap();
        repository.open_session("fake", Some("root".into())).await.unwrap();
        let mut ids = Vec::new();
        for session in [root.clone() as std::sync::Arc<dyn alva_kernel_abi::AgentSession>, child.clone(), grandchild.clone()] {
            let record = SessionRecord::extension("alva.mcp", "mcp_io", 1, EventEmitter::runtime(), serde_json::json!({
                "server_id":"fixture","direction":"incoming","frame":RecordValue::Inline{value:serde_json::json!({"text":format!("{}own-{}", "x".repeat(100000), session.session_id())})}
            })).unwrap();
            ids.push(record.record_id.clone());
            session.append_records(vec![record]).await.unwrap();
        }
        let sessions = ["root".to_string(), child.session_id().to_string(), grandchild.session_id().to_string()];
        for (index, native) in sessions.iter().enumerate() {
            let page = service.mcp_journal(&store,"product-a",native,serde_json::json!({"action":"list"})).await.unwrap();
            assert_eq!(page["hits"][0]["record_id"],ids[index]);
            let text = service.mcp_journal(&store,"product-a",native,serde_json::json!({"action":"read","record_id":ids[index]})).await.unwrap();
            assert!(text["text"].as_str().unwrap().len() <= 2048);
            assert!(service.mcp_journal(&store,"product-b",native,serde_json::json!({"action":"list"})).await.is_err());
        }
        let count = repository.records("root").unwrap().len();
        assert!(service.mcp_journal(&store,"product-a","root",serde_json::json!({"action":"read","record_id":ids[1]})).await.is_err());
        assert!(service.mcp_journal(&store,"product-a","fake",serde_json::json!({"action":"list"})).await.is_err());
        assert!(service.mcp_journal(&store,"product-a","unknown",serde_json::json!({"action":"list"})).await.is_err());
        assert!(!repository.contains("unknown").unwrap());
        let snapshot = service.inspect(&store,"product-a",None).await.unwrap();
        assert!(serde_json::to_string(&snapshot).unwrap().len() < 10000);
        assert!(snapshot.records.iter().any(|record| record.payload.get("mcp_journal_reference").is_some()));
        assert_eq!(repository.records("root").unwrap().len(), count, "inspection never appends/replays calls");
        drop((root,child,grandchild,repository,service));
        let reopened = HubInspectorService::open(directory.path()).unwrap();
        assert_eq!(reopened.mcp_journal(&store,"product-a",&sessions[2],serde_json::json!({"action":"list"})).await.unwrap()["hits"][0]["record_id"],ids[2]);
    }

    #[tokio::test]
    async fn durable_inspection_reopens_recursive_records_with_product_ownership() {
        let directory = tempfile::tempdir().unwrap();
        let store = HubStore::open(directory.path().join("hub.db")).unwrap();
        let branch =
            bind_native_session(&store, directory.path(), "product-a", "alva", "native-root");
        bind_native_session(
            &store,
            directory.path(),
            "product-b",
            "alva",
            "unrelated-root",
        );
        let service = HubInspectorService::open(directory.path()).unwrap();
        let repository = service.repository();
        let root = repository.open_session("native-root", None).await.unwrap();
        root.append_records(vec![
            event(0, "run_start", 1_000, serde_json::json!({})),
            event(0, "iteration_start", 1_010, serde_json::json!({})),
            event(0, "llm_call_start", 1_020, serde_json::json!({})),
            event(
                0,
                "llm_stream_summary",
                1_060,
                serde_json::json!({"time_to_first_output_ms": 8}),
            ),
            event(
                0,
                "llm_call_end",
                1_070,
                serde_json::json!({"input_tokens": 3, "output_tokens": 2}),
            ),
            event(0, "iteration_end", 1_080, serde_json::json!({})),
            event(0, "run_end", 1_090, serde_json::json!({})),
        ])
        .await
        .unwrap();
        let child = root
            .create_child_session("root-run", "spawn-child")
            .await
            .unwrap()
            .unwrap();
        let child_id = child.session_id().to_string();
        child
            .append_records(vec![SessionRecord::execution(
                "tool_intent",
                EventEmitter::runtime(),
                ExecutionRecord::Intent {
                    operation_id: "read-1".into(),
                    operation: "read".into(),
                    input: RecordValue::Inline {
                        value: serde_json::json!({"path": "hello.txt"}),
                    },
                },
            )])
            .await
            .unwrap();
        let grandchild = child
            .create_child_session("child-run", "spawn-grandchild")
            .await
            .unwrap()
            .unwrap();
        let grandchild_id = grandchild.session_id().to_string();
        grandchild
            .append_records(vec![event(0, "run_start", 2_000, serde_json::json!({}))])
            .await
            .unwrap();
        let unrelated = repository
            .open_session("unrelated-root", None)
            .await
            .unwrap();
        unrelated
            .append_records(vec![event(0, "run_start", 3_000, serde_json::json!({}))])
            .await
            .unwrap();
        root.flush().await.unwrap();
        child.flush().await.unwrap();
        grandchild.flush().await.unwrap();
        let expected_root = repository.records("native-root").unwrap();
        let expected_child = repository.records(&child_id).unwrap();
        drop((root, child, grandchild, unrelated, repository, service));

        let reopened = HubInspectorService::open(directory.path()).unwrap();
        let snapshot = reopened.inspect(&store, "product-a", None).await.unwrap();
        assert_eq!(snapshot.branch_id, branch);
        assert_eq!(snapshot.native_session_id, "native-root");
        assert!(!snapshot.live);
        assert_eq!(snapshot.high_water_seq, expected_root.last().unwrap().seq);
        assert_eq!(snapshot.children.len(), 1);
        assert_eq!(snapshot.children[0].child_session_id, child_id);
        assert_eq!(snapshot.children[0].spawn_tool_call_id, "spawn-child");
        assert_eq!(
            snapshot.run.turns[0].llm_call.time_to_first_output_ms,
            Some(8)
        );
        assert_eq!(snapshot.records.len(), expected_root.len());
        for (actual, expected) in snapshot.records.iter().zip(&expected_root) {
            assert_eq!(actual.record_id, expected.record_id);
            assert_eq!(actual.seq, expected.seq);
            assert_eq!(
                actual.payload,
                serde_json::to_value(&expected.payload).unwrap()
            );
        }
        let snapshot = reopened
            .inspect(&store, "product-a", Some(&child_id))
            .await
            .unwrap();
        assert_eq!(snapshot.parent_session_id.as_deref(), Some("native-root"));
        assert_eq!(
            snapshot.records[0].payload,
            serde_json::to_value(&expected_child[0].payload).unwrap()
        );
        assert_eq!(snapshot.children[0].child_session_id, grandchild_id);
        let snapshot = reopened
            .inspect(&store, "product-a", Some(&grandchild_id))
            .await
            .unwrap();
        assert_eq!(
            snapshot.parent_session_id.as_deref(),
            Some(child_id.as_str())
        );
        assert_eq!(snapshot.root_native_session_id, "native-root");
        assert!(reopened
            .inspect(&store, "product-a", Some("unrelated-root"))
            .await
            .unwrap_err()
            .contains("does not belong"));
        assert!(reopened
            .inspect(&store, "product-b", Some(&child_id))
            .await
            .unwrap_err()
            .contains("does not belong"));
    }

    #[tokio::test]
    async fn missing_or_unsupported_inspection_never_creates_native_records() {
        let directory = tempfile::tempdir().unwrap();
        let store = HubStore::open(directory.path().join("hub.db")).unwrap();
        let service = HubInspectorService::open(directory.path()).unwrap();
        bind_native_session(
            &store,
            directory.path(),
            "missing-records",
            "alva",
            "not-created",
        );
        bind_native_session(
            &store,
            directory.path(),
            "unsupported",
            "claude",
            "claude-native",
        );
        assert!(service
            .inspect(&store, "missing-product", None)
            .await
            .is_err());
        assert!(service
            .inspect(&store, "missing-records", None)
            .await
            .is_err());
        assert!(service
            .inspect(&store, "unsupported", None)
            .await
            .unwrap_err()
            .contains("no full native Inspector"));
        assert!(!service.repository().contains("not-created").unwrap());
        assert!(!service.repository().contains("claude-native").unwrap());
    }

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
