// INPUT: Real HubRuntime with successful source and fatal target fake engines.
// OUTPUT: Native resume identity used by a handoff retry before old EOF.
// POS: R90 acceptance probe for failure-native rollback consistency.
use std::{pin::Pin, sync::{Arc, Mutex, atomic::{AtomicUsize, Ordering}}};
use async_trait::async_trait;
use futures_util::{stream, Stream, StreamExt};
use alva_engine_runtime::{EngineRuntime, RuntimeRequest, RuntimeEvent, RuntimeError, RuntimeCapabilities, PermissionDecision};
use alva_hub_core::{HubRuntime, HubRequest};
struct Engine { fatal_first: bool, count: AtomicUsize, requests: Mutex<Vec<RuntimeRequest>> }
#[async_trait]
impl EngineRuntime for Engine {
    fn execute(&self, request: RuntimeRequest) -> Result<Pin<Box<dyn Stream<Item=RuntimeEvent> + Send>>, RuntimeError> {
        self.requests.lock().unwrap().push(request);
        let n = self.count.fetch_add(1, Ordering::SeqCst);
        let sid = format!("native-{n}");
        let first = RuntimeEvent::SessionStarted { session_id: sid.clone(), model: None, tools: vec![] };
        if self.fatal_first && n == 0 {
            Ok(Box::pin(stream::iter(vec![first, RuntimeEvent::Error { message: "fatal handoff".into(), recoverable: false }]).chain(stream::pending())))
        } else {
            Ok(Box::pin(stream::iter(vec![first, RuntimeEvent::Completed { session_id: sid, result: Some("done".into()), usage: None }])))
        }
    }
    async fn cancel(&self, _: &str) -> Result<(), RuntimeError> { Ok(()) }
    async fn respond_permission(&self, _: &str, _: &str, _: PermissionDecision) -> Result<(), RuntimeError> { Ok(()) }
    fn capabilities(&self) -> RuntimeCapabilities { RuntimeCapabilities { streaming: true, tool_control: false, permission_callback: false, resume: true, cancel: true } }
}
#[tokio::main]
async fn main() {
    let hub = HubRuntime::new();
    let source = Arc::new(Engine { fatal_first: false, count: AtomicUsize::new(0), requests: Mutex::new(vec![]) });
    let target = Arc::new(Engine { fatal_first: true, count: AtomicUsize::new(0), requests: Mutex::new(vec![]) });
    hub.register_engine("source", source).unwrap();
    hub.register_engine("target", target.clone()).unwrap();
    let initial: Vec<_> = hub.execute(HubRequest::new("source", RuntimeRequest::new("start"))).unwrap().collect().await;
    let pid = initial[0].product_session_id.clone();
    hub.switch_engine(&pid, "target").unwrap();
    let mut failed = hub.execute(HubRequest::new("target", RuntimeRequest::new("first")).resume(pid.clone()).with_handoff_context("UNIQUE_HANDOFF")).unwrap();
    failed.next().await.unwrap();
    failed.next().await.unwrap();
    let _: Vec<_> = hub.execute(HubRequest::new("target", RuntimeRequest::new("retry")).resume(pid)).unwrap().collect().await;
    let requests = target.requests.lock().unwrap();
    println!("retry marker count={}, retry native={:?}; expected None as after EOF rollback", requests[1].prompt.matches("UNIQUE_HANDOFF").count(), requests[1].resume_session);
    assert_eq!(requests[1].prompt.matches("UNIQUE_HANDOFF").count(), 1);
    assert!(requests[1].resume_session.is_none(), "failed first-handoff native must not be resumed merely because EOF is late");
}
