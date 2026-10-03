// INPUT: Catalog factory, durable sessions, real authored stdio fixture and mock model
// OUTPUT: Recursive managed MCP ownership, HITL, grant and restart regressions
// POS: Product-only integration evidence; no real accounts or external MCP service.
use crate::*;
use alva_app_core::extension::{mcp::McpSpawnPolicy, spawn_context::SpawnContextPolicy};
use alva_kernel_abi::{
    AgentMessage, CancellationToken, ExecutionRecord, Message, SessionRecordPayload,
    ToolExecutionContext,
};
use alva_test::{
    fixtures::{make_assistant_message, make_tool_call_message},
    mock_provider::MockLanguageModel,
};
use serde_json::{json, Value};
use std::{path::Path, sync::Arc, time::Duration};

// Authored fixture, not downloaded code. Each connection reports its own PID.
const SERVER: &str = r#"import json,sys,os
print('owned-mcp-pid='+str(os.getpid()),file=sys.stderr,flush=True)
for line in sys.stdin:
 r=json.loads(line)
 if 'id' not in r or 'method' not in r: continue
 if r['method']=='initialize':
  result={'protocolVersion':r['params']['protocolVersion'],'capabilities':{'tools':{}},'serverInfo':{'name':'fixture','version':'1'}}
 elif r['method']=='tools/list':
  result={'tools':[{'name':'echo','inputSchema':{'type':'object','properties':{'query':{'type':'string'}}}}]}
 else:
  result={'content':[{'type':'text','text':'x'*600000+'reply-'+r['params']['arguments']['query']},{'type':'image','data':'typed-media-must-not-load','mimeType':'image/png'}],'structuredContent':{'pid':os.getpid(),'cwd':os.getcwd(),'ambient_home':'HOME' in os.environ}}
 print(json.dumps({'jsonrpc':'2.0','id':r['id'],'result':result}),flush=True)
"#;
fn servers() -> Vec<McpServerConfig> {
    vec![McpServerConfig {
        id: "fixture".into(),
        display_name: "Fixture".into(),
        auto_connect: true,
        connect_timeout_secs: 5,
        transport: McpTransportConfig::Stdio {
            command: "/usr/bin/python3".into(),
            args: vec!["-u".into(), "-c".into(), SERVER.into()],
            env: Default::default(),
        },
    }]
}
async fn build(
    repository: &SessionRepository,
    cwd: &Path,
    model: Arc<MockLanguageModel>,
) -> BuiltAgent {
    let toggles = COMPONENTS
        .iter()
        .map(|c| (c.id.to_string(), c.id == "sub-agents"))
        .collect();
    let session = repository.open_session("root", None).await.unwrap();
    let request = alva_engine_runtime::RuntimeRequest {
        prompt: "fixture".into(),
        resume_session: None,
        system_prompt: None,
        working_directory: Some(cwd.to_path_buf()),
        options: Default::default(),
    };
    CatalogBaseAgentFactory::new(model, cwd)
        .with_toggles(toggles)
        .with_context_repository(repository.clone())
        .with_managed_mcp(servers())
        .build(&request, session)
        .await
        .unwrap()
}
async fn drive(mut built: BuiltAgent, decision: alva_app_core::PermissionDecision) -> usize {
    let mut approvals = built.approval_rx.take().unwrap();
    let mut events = built.agent.prompt_text("parent-only-visible").await;
    let mut count = 0;
    tokio::time::timeout(Duration::from_secs(10), async {
        loop { tokio::select! {
            Some(approval) = approvals.recv() => {
                assert_eq!(approval.tool_name,"mcp_runtime");
                assert!(built.agent.resolve_permission(&approval.request_id, &approval.tool_name, decision).await);
                count += 1;
            }
            event = events.recv() => if event.is_none() { break; },
        }}
    }).await.unwrap();
    count
}
fn children(repository: &SessionRepository, parent: &str) -> Vec<String> {
    repository
        .records(parent)
        .unwrap()
        .into_iter()
        .filter_map(|r| match r.payload {
            SessionRecordPayload::Execution(ExecutionRecord::ChildSessionLinked {
                child_session_id,
                ..
            }) => Some(child_session_id),
            _ => None,
        })
        .collect()
}
fn frames(repository: &SessionRepository, session: &str) -> Vec<Value> {
    repository
        .records(session)
        .unwrap()
        .iter()
        .filter_map(crate::mcp_journal::frame_value)
        .map(|v| repository.context_value(&v.unwrap()).unwrap())
        .collect()
}
async fn wait_closed(repository: &SessionRepository, id: &str) {
    tokio::time::timeout(Duration::from_secs(2), async {
        while !frames(repository, id)
            .iter()
            .any(|f| f["status"] == "closed")
        {
            tokio::task::yield_now().await;
        }
    })
    .await
    .unwrap();
}
struct OwnContext {
    id: String,
    cancel: CancellationToken,
}
impl ToolExecutionContext for OwnContext {
    fn session_id(&self) -> &str {
        &self.id
    }
    fn cancel_token(&self) -> &CancellationToken {
        &self.cancel
    }
    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
}
async fn read(
    repository: &SessionRepository,
    session: &str,
    input: Value,
) -> Result<String, alva_kernel_abi::AgentError> {
    let tool = ContextRetrievalPlugin::new(repository.clone(), session.into())
        .bind(session)
        .retrieval_tool;
    tool.execute(
        input,
        &OwnContext {
            id: session.into(),
            cancel: CancellationToken::new(),
        },
    )
    .await
    .map(|o| o.model_text())
}
fn call(query: &str) -> Message {
    make_tool_call_message(
        "mcp_runtime",
        json!({"action":"call_tool","server_id":"fixture","tool_name":"echo","arguments":{"query":query}}),
    )
}
fn spawn(with_mcp: bool) -> Message {
    make_tool_call_message(
        "agent",
        json!({"task":"perform own task","role":"worker","tools":if with_mcp { vec!["mcp_runtime"] } else { vec![] }}),
    )
}
fn search(query: &str) -> Message {
    make_tool_call_message(
        "context_retrieval",
        json!({"action":"search_history","kind":"mcp","query":query}),
    )
}

#[tokio::test]
async fn recursive_mcp_workers_have_own_logs_approvals_retrieval_and_restart() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("records.db");
    let repository = SessionRepository::open(&path).unwrap();
    let model = Arc::new(
        MockLanguageModel::new()
            .with_response(call("root-needle"))
            .with_response(spawn(true))
            .with_response(call("child-needle"))
            .with_response(search("reply-child-needle"))
            .with_response(spawn(true))
            .with_response(call("grand-needle"))
            .with_response(search("reply-grand-needle"))
            .with_response(make_assistant_message("grandchild done"))
            .with_response(make_assistant_message("child done"))
            .with_response(call("root-after-child"))
            .with_response(make_assistant_message("root done")),
    );
    let built = build(&repository, temp.path(), model.clone()).await;
    assert!(built.agent.bus().get::<dyn McpSpawnPolicy>().is_some());
    assert_eq!(
        drive(built, alva_app_core::PermissionDecision::AllowOnce).await,
        4
    );
    assert_eq!(model.calls().len(), 11);
    for call in model.calls() {
        let text = serde_json::to_string(&call).unwrap();
        assert!(text.len() < 30000, "raw MCP transcript entered context");
        assert!(!text.contains("typed-media-must-not-load"));
    }
    for index in 2..9 {
        let text = serde_json::to_string(&model.calls()[index]).unwrap();
        assert!(!text.contains("root-needle") && !text.contains("parent-only-visible"));
    }
    for index in 5..8 {
        assert!(!serde_json::to_string(&model.calls()[index])
            .unwrap()
            .contains("child-needle"));
    }
    for index in [1, 3, 6] {
        assert!(serde_json::to_string(&model.calls()[index])
            .unwrap()
            .contains("mcp_interaction_reference"));
    }
    for (index, needle) in [(4, "reply-child-needle"), (7, "reply-grand-needle")] {
        assert!(serde_json::to_string(&model.calls()[index])
            .unwrap()
            .contains(needle));
    }
    let child = children(&repository, "root").pop().unwrap();
    let grandchild = children(&repository, &child).pop().unwrap();
    let ids = ["root", child.as_str(), grandchild.as_str()];
    let mut pids = Vec::new();
    let mut references = Vec::new();
    for (id, needle) in ids
        .iter()
        .zip(["root-needle", "child-needle", "grand-needle"])
    {
        wait_closed(&repository, id).await;
        let frames = frames(&repository, id);
        let pid = frames
            .iter()
            .find_map(|f| f["result"]["structuredContent"]["pid"].as_u64())
            .unwrap();
        for result in frames
            .iter()
            .filter(|f| f["result"]["structuredContent"]["pid"].is_u64())
        {
            assert_eq!(
                result["result"]["structuredContent"]["cwd"],
                temp.path()
                    .canonicalize()
                    .unwrap()
                    .to_string_lossy()
                    .as_ref()
            );
            assert_eq!(result["result"]["structuredContent"]["ambient_home"], false);
        }
        pids.push(pid);
        assert!(frames.iter().any(|f| f["method"] == "initialize"));
        assert!(frames.iter().any(|f| f["text"]
            .as_str()
            .is_some_and(|t| t.contains("owned-mcp-pid="))));
        let rpc_queries: Vec<_> = frames
            .iter()
            .filter(|f| f["method"] == "tools/call")
            .map(|f| f["params"]["arguments"]["query"].as_str().unwrap())
            .collect();
        assert_eq!(
            rpc_queries,
            if *id == "root" {
                vec!["root-needle", "root-after-child"]
            } else {
                vec![needle]
            }
        );
        let hits: Value = serde_json::from_str(
            &read(
                &repository,
                id,
                json!({"action":"search_history","kind":"mcp","query":format!("reply-{needle}")}),
            )
            .await
            .unwrap(),
        )
        .unwrap();
        let source = hits["hits"][0]["record_id"].as_str().unwrap().to_string();
        references.push(source);
        assert!(read(&repository,id,json!({"action":"search_record","record_id":references.last(),"query":format!("reply-{needle}")})).await.unwrap().contains(needle));
    }
    assert_eq!(
        pids.iter().collect::<std::collections::HashSet<_>>().len(),
        3
    );
    for (index, id) in ids.iter().enumerate() {
        for (other, reference) in references.iter().enumerate() {
            if index != other {
                assert!(read(
                    &repository,
                    id,
                    json!({"action":"read","record_id":reference})
                )
                .await
                .is_err());
            }
        }
    }
    let reopened = SessionRepository::open(&path).unwrap();
    assert!(read(
        &reopened,
        &grandchild,
        json!({"action":"search_record","record_id":references[2],"query":"reply-grand-needle"})
    )
    .await
    .unwrap()
    .contains("reply-grand-needle"));
}

#[tokio::test]
async fn recursive_mcp_cannot_restore_a_direct_parents_missing_grant() {
    let temp = tempfile::tempdir().unwrap();
    let repository = SessionRepository::open(temp.path().join("records.db")).unwrap();
    let model = Arc::new(
        MockLanguageModel::new()
            .with_response(spawn(false))
            .with_response(spawn(true))
            .with_response(call("must-not-execute"))
            .with_response(make_assistant_message("grandchild done"))
            .with_response(make_assistant_message("child done"))
            .with_response(make_assistant_message("root done")),
    );
    assert_eq!(
        drive(
            build(&repository, temp.path(), model.clone()).await,
            alva_app_core::PermissionDecision::AllowOnce
        )
        .await,
        // The existing security middleware checks dangerous names before the
        // executor rejects missing tools. Even approving this hallucinated
        // call must not restore a parent's missing grant or start a worker.
        1
    );
    assert_eq!(model.calls().len(), 6);
    for names in &model.tool_names_seen()[1..5] {
        assert!(!names.iter().any(|n| n == "mcp_runtime"));
    }
    let child = children(&repository, "root").pop().unwrap();
    let grandchild = children(&repository, &child).pop().unwrap();
    assert!(
        serde_json::to_string(&repository.records(&grandchild).unwrap())
            .unwrap()
            .contains("Tool not found: mcp_runtime")
    );
    assert!(frames(&repository, &child).is_empty() && frames(&repository, &grandchild).is_empty());
    assert!(!frames(&repository, "root")
        .iter()
        .any(|f| f["method"] == "tools/call"));
}

#[tokio::test]
async fn child_mcp_rejection_prevents_gateway_and_remote_execution() {
    let temp = tempfile::tempdir().unwrap();
    let repository = SessionRepository::open(temp.path().join("records.db")).unwrap();
    let model = Arc::new(
        MockLanguageModel::new()
            .with_response(spawn(true))
            .with_response(call("rejected-call"))
            .with_response(make_assistant_message("child done"))
            .with_response(make_assistant_message("root done")),
    );
    assert_eq!(
        drive(
            build(&repository, temp.path(), model.clone()).await,
            alva_app_core::PermissionDecision::RejectOnce
        )
        .await,
        1
    );
    let child = children(&repository, "root").pop().unwrap();
    wait_closed(&repository, &child).await;
    assert!(!frames(&repository, &child)
        .iter()
        .any(|f| f["method"] == "tools/call" || f["input"]["action"] == "call_tool"));
    assert_eq!(model.calls().len(), 4);
}

#[tokio::test]
async fn missing_child_native_authority_fails_before_mcp_startup() {
    let temp = tempfile::tempdir().unwrap();
    let repository = SessionRepository::open(temp.path().join("records.db")).unwrap();
    let policy = crate::mcp_journal::ManagedMcpSpawnPlugin {
        repository,
        servers: servers(),
        workspace: temp.path().into(),
    };
    let session = Arc::new(alva_agent_core::agent_session::ListenableInMemorySession::new());
    let error = policy
        .bind(session)
        .await
        .err()
        .expect("durable child authority required");
    assert!(error.to_string().contains("authority unavailable"));
}
