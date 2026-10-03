// INPUT: MCP transport/observer contracts and bounded Tokio channels
// OUTPUT: WorkerTransport, an owned serial I/O task with cancellation cleanup
// POS: Harness MCP worker; no secondary LLM, persistence policy or SDK-loop changes.
use alva_protocol_mcp::{
    error::McpError,
    transport::{McpIoObserver, McpTransport},
    types::McpToolInfo,
};
use async_trait::async_trait;
use serde_json::{json, Value};
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc,
};
use tokio::sync::{mpsc, oneshot};

enum Operation {
    Connect,
    List,
    Call(String, Value),
    Disconnect,
}
enum Reply {
    Done,
    Tools(Vec<McpToolInfo>),
    Result(Value),
}
struct Job {
    operation: Operation,
    reply: oneshot::Sender<Result<Reply, McpError>>,
}

pub struct WorkerTransport {
    jobs: mpsc::Sender<Job>,
    connected: Arc<AtomicBool>,
}
fn unavailable() -> McpError {
    McpError::Transport("MCP interaction worker unavailable".into())
}

impl WorkerTransport {
    pub fn new(
        mut transport: Box<dyn McpTransport>,
        observer: Arc<dyn McpIoObserver>,
        server: String,
        connected: Arc<AtomicBool>,
    ) -> Self {
        let (jobs, mut queue) = mpsc::channel::<Job>(1);
        let status = connected.clone();
        tokio::spawn(async move {
            while let Some(mut job) = queue.recv().await {
                let operation = match &job.operation {
                    Operation::Connect => "connect",
                    Operation::List => "list_tools",
                    Operation::Call(_, _) => "call_tool",
                    Operation::Disconnect => "disconnect",
                };
                // Dropping the caller's future closes this receiver. Drop the
                // pending RPC too: StdioTransport invalidates/reaps its session.
                let result = tokio::select! {
                    biased;
                    _ = job.reply.closed() => None,
                    value = async {
                        match job.operation {
                            Operation::Connect => transport.connect().await.map(|_| Reply::Done),
                            Operation::List => transport.list_tools().await.map(Reply::Tools),
                            Operation::Call(name, input) => transport.call_tool(&name, input).await.map(Reply::Result),
                            Operation::Disconnect => transport.disconnect().await.map(|_| Reply::Done),
                        }
                    } => Some(value),
                };
                let state = match &result {
                    None => "cancelled_acceptance_unknown",
                    Some(Ok(_)) => "completed",
                    Some(Err(_)) => "failed",
                };
                let recorded = observer
                    .record(
                        &server,
                        "worker_status",
                        json!({"operation":operation,"status":state,"error":result.as_ref().and_then(|r|r.as_ref().err()).map(ToString::to_string)}),
                    )
                    .await;
                match result {
                    Some(value) => {
                        let failed = recorded.is_err();
                        let _ = job.reply.send(recorded.and(value));
                        if failed {
                            break;
                        }
                    }
                    None => break,
                }
            }
            // Queue closure is owned transport lifetime, not a detached MCP
            // daemon. This path also handles cancellation and recorder failure.
            let _ = transport.disconnect().await;
            status.store(false, Ordering::SeqCst);
            let _ = observer
                .record(&server, "worker_status", json!({"status":"closed"}))
                .await;
        });
        Self { jobs, connected }
    }

    async fn submit(&self, operation: Operation) -> Result<Reply, McpError> {
        let (reply, result) = oneshot::channel();
        self.jobs
            .send(Job { operation, reply })
            .await
            .map_err(|_| unavailable())?;
        result.await.map_err(|_| unavailable())?
    }
}

#[async_trait]
impl McpTransport for WorkerTransport {
    async fn connect(&mut self) -> Result<(), McpError> {
        self.submit(Operation::Connect).await.map(|_| ())
    }
    async fn disconnect(&mut self) -> Result<(), McpError> {
        self.submit(Operation::Disconnect).await.map(|_| ())
    }
    fn is_connected(&self) -> bool {
        self.connected.load(Ordering::SeqCst)
    }
    async fn list_tools(&self) -> Result<Vec<McpToolInfo>, McpError> {
        match self.submit(Operation::List).await? {
            Reply::Tools(value) => Ok(value),
            _ => Err(unavailable()),
        }
    }
    async fn call_tool(&self, name: &str, arguments: Value) -> Result<Value, McpError> {
        match self.submit(Operation::Call(name.into(), arguments)).await? {
            Reply::Result(value) => Ok(value),
            _ => Err(unavailable()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::sync::Mutex;
    #[derive(Default)]
    struct Recorder {
        frames: Mutex<Vec<Value>>,
        fail: bool,
    }
    #[async_trait]
    impl McpIoObserver for Recorder {
        async fn record(&self, _: &str, _: &str, frame: Value) -> Result<String, McpError> {
            if self.fail {
                return Err(unavailable());
            }
            self.frames.lock().await.push(frame);
            Ok("fixture-reference".into())
        }
    }
    struct PendingTransport {
        connected: Arc<AtomicBool>,
        dropped: Arc<AtomicBool>,
    }
    impl Drop for PendingTransport {
        fn drop(&mut self) {
            self.dropped.store(true, Ordering::SeqCst);
        }
    }
    #[async_trait]
    impl McpTransport for PendingTransport {
        async fn connect(&mut self) -> Result<(), McpError> {
            self.connected.store(true, Ordering::SeqCst);
            Ok(())
        }
        async fn disconnect(&mut self) -> Result<(), McpError> {
            self.connected.store(false, Ordering::SeqCst);
            Ok(())
        }
        fn is_connected(&self) -> bool {
            self.connected.load(Ordering::SeqCst)
        }
        async fn list_tools(&self) -> Result<Vec<McpToolInfo>, McpError> {
            Ok(vec![])
        }
        async fn call_tool(&self, _: &str, _: Value) -> Result<Value, McpError> {
            std::future::pending().await
        }
    }
    async fn wait_until_dropped(flag: &AtomicBool) {
        tokio::time::timeout(std::time::Duration::from_secs(2), async {
            while !flag.load(Ordering::SeqCst) {
                tokio::task::yield_now().await;
            }
        })
        .await
        .unwrap();
    }
    #[tokio::test]
    async fn cancelled_caller_records_unknown_and_releases_owned_transport() {
        let dropped = Arc::new(AtomicBool::new(false));
        let connected = Arc::new(AtomicBool::new(false));
        let recorder = Arc::new(Recorder::default());
        let mut worker = WorkerTransport::new(
            Box::new(PendingTransport {
                connected: connected.clone(),
                dropped: dropped.clone(),
            }),
            recorder.clone(),
            "fixture".into(),
            connected,
        );
        worker.connect().await.unwrap();
        assert!(worker.is_connected());
        assert!(tokio::time::timeout(
            std::time::Duration::from_millis(30),
            worker.call_tool("pending", json!({}))
        )
        .await
        .is_err());
        wait_until_dropped(&dropped).await;
        assert!(!worker.is_connected());
        assert!(recorder
            .frames
            .lock()
            .await
            .iter()
            .any(|frame| frame["status"] == "cancelled_acceptance_unknown"));
        assert!(worker.call_tool("no-replay", json!({})).await.is_err());
    }
    #[tokio::test]
    async fn recorder_failure_or_last_handle_drop_closes_the_worker() {
        for fail in [false, true] {
            let dropped = Arc::new(AtomicBool::new(false));
            let connected = Arc::new(AtomicBool::new(false));
            let mut worker = WorkerTransport::new(
                Box::new(PendingTransport {
                    connected: connected.clone(),
                    dropped: dropped.clone(),
                }),
                Arc::new(Recorder {
                    fail,
                    ..Default::default()
                }),
                "fixture".into(),
                connected,
            );
            assert_eq!(worker.connect().await.is_err(), fail);
            drop(worker);
            wait_until_dropped(&dropped).await;
        }
    }
}
