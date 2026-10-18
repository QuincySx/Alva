// INPUT: Real HubRuntime and a gated fake EngineRuntime.
// OUTPUT: Old cancel result versus new synchronously starting claim assertion.
// POS: R80 acceptance probe for the claim-to-RunHandle-registration window.
use std::{pin::Pin, sync::{Arc, Mutex, Condvar, atomic::{AtomicUsize, Ordering}}};
use async_trait::async_trait;
use futures_util::{stream, Stream, StreamExt};
use tokio::sync::Notify;
use alva_engine_runtime::{EngineRuntime, RuntimeRequest, RuntimeEvent, RuntimeError, RuntimeCapabilities, PermissionDecision};
use alva_hub_core::{HubRuntime, HubRequest, HubSessionStatus};

struct Engine {
    count: AtomicUsize,
    cancel_entered: Notify,
    cancel_release: Notify,
    execute_entered: Notify,
    execute_gate: (Mutex<bool>, Condvar),
}
#[async_trait]
impl EngineRuntime for Engine {
    fn execute(&self, _: RuntimeRequest) -> Result<Pin<Box<dyn Stream<Item=RuntimeEvent> + Send>>, RuntimeError> {
        let n = self.count.fetch_add(1, Ordering::SeqCst);
        if n == 1 {
            self.execute_entered.notify_one();
            let mut released = self.execute_gate.0.lock().unwrap();
            while !*released { released = self.execute_gate.1.wait(released).unwrap(); }
        }
        let mut events = vec![RuntimeEvent::SessionStarted { session_id: format!("native-{n}"), model: None, tools: vec![] }];
        if n == 0 { events.push(RuntimeEvent::Error { message: "fatal before EOF".into(), recoverable: false }); }
        Ok(Box::pin(stream::iter(events).chain(stream::pending())))
    }
    async fn cancel(&self, _: &str) -> Result<(), RuntimeError> {
        self.cancel_entered.notify_one();
        self.cancel_release.notified().await;
        Err(RuntimeError::Unsupported("cancel".into()))
    }
    async fn respond_permission(&self, _: &str, _: &str, _: PermissionDecision) -> Result<(), RuntimeError> { Ok(()) }
    fn capabilities(&self) -> RuntimeCapabilities { RuntimeCapabilities { streaming: true, tool_control: false, permission_callback: false, resume: true, cancel: true } }
}

#[tokio::main(flavor="multi_thread", worker_threads=2)]
async fn main() {
    let engine = Arc::new(Engine { count: AtomicUsize::new(0), cancel_entered: Notify::new(), cancel_release: Notify::new(), execute_entered: Notify::new(), execute_gate: (Mutex::new(false), Condvar::new()) });
    let hub = HubRuntime::new();
    hub.register_engine("test", engine.clone()).unwrap();
    let mut first = hub.execute(HubRequest::new("test", RuntimeRequest::new("first"))).unwrap();
    let pid = first.next().await.unwrap().product_session_id;
    first.next().await.unwrap(); // fatal, but old stream and RunHandle remain alive
    assert_eq!(hub.session(&pid).unwrap().status, HubSessionStatus::Failed);
    let old_cancel = { let hub = hub.clone(); let pid = pid.clone(); tokio::spawn(async move { hub.cancel(&pid).await }) };
    engine.cancel_entered.notified().await;
    let new_execute = { let hub = hub.clone(); let pid = pid.clone(); tokio::task::spawn_blocking(move || hub.execute(HubRequest::new("test", RuntimeRequest::new("second")).resume(pid))) };
    engine.execute_entered.notified().await; // new claim exists; new RunHandle does not
    assert_eq!(hub.session(&pid).unwrap().status, HubSessionStatus::Starting);
    engine.cancel_release.notify_one();
    old_cancel.await.unwrap().unwrap();
    let status = hub.session(&pid).unwrap().status;
    println!("new claim after stale cancel: {status:?}; expected Starting");
    // Always open the synchronous gate before asserting, so failure exits cleanly.
    *engine.execute_gate.0.lock().unwrap() = true;
    engine.execute_gate.1.notify_all();
    let _second = new_execute.await.unwrap().unwrap();
    assert_eq!(status, HubSessionStatus::Starting, "old RunGeneration must not release a newer session claim before its RunHandle registers");
}
