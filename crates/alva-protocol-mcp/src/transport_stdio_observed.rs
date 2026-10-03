// INPUT: Owned MCP stdio handles, optional host journal and process lifetime guard
// OUTPUT: ObservedOutput, continuous bounded stdout/stderr capture and observed send_frame
// POS: Native I/O mechanism only; host owns persistence, redaction and retrieval policy.
use super::{failure, FRAME_LIMIT};
use crate::{error::McpError, transport::McpIoObserver};
use serde_json::{json, Value};
use std::sync::{
    atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering},
    Arc, Mutex as SyncMutex,
};
use tokio::{
    io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader},
    process::{Child, ChildStderr, ChildStdin, ChildStdout},
    sync::{mpsc, Mutex},
    task::JoinHandle,
};

pub(super) type ProcessGuard = Arc<SyncMutex<Option<Child>>>;
pub(super) fn terminate_owned(process: &ProcessGuard, connected: &AtomicBool) -> Option<Child> {
    if let Ok(mut owned) = process.lock() {
        if let Some(mut child) = owned.take() {
            connected.store(false, Ordering::SeqCst);
            #[cfg(unix)]
            if let Some(pid) = child.id() {
                unsafe {
                    libc::kill(-(pid as i32), libc::SIGKILL);
                }
            }
            let _ = child.start_kill();
            // A late old reader sees None, so cannot kill a recycled PID or
            // overwrite the state published by a replacement connection.
            return Some(child);
        }
    }
    None
}
pub(super) async fn send_frame(
    input: &Arc<Mutex<ChildStdin>>,
    observer: Option<&Arc<dyn McpIoObserver>>,
    server: &str,
    value: Value,
) -> Result<(), McpError> {
    let mut bytes = serde_json::to_vec(&value).map_err(|_| failure("invalid outgoing JSON"))?;
    if bytes.len() > FRAME_LIMIT {
        return Err(failure("MCP frame exceeds 1 MiB"));
    }
    if let Some(observer) = observer {
        observer.record(server, "outgoing_attempt", value).await?;
    }
    bytes.push(b'\n');
    let mut input = input.lock().await;
    input
        .write_all(&bytes)
        .await
        .map_err(|_| failure("MCP stdin closed"))?;
    input.flush().await.map_err(|_| failure("MCP stdin closed"))
}
pub(super) async fn read_frame(
    output: &mut BufReader<ChildStdout>,
    observer: Option<&Arc<dyn McpIoObserver>>,
    server: &str,
) -> Result<Value, McpError> {
    let mut bytes = Vec::new();
    loop {
        let chunk = output
            .fill_buf()
            .await
            .map_err(|_| failure("MCP stdout unavailable"))?;
        if chunk.is_empty() {
            if !bytes.is_empty() {
                if let Some(observer) = observer {
                    observer
                        .record(
                            server,
                            "incomplete_stdout",
                            json!({"bytes":bytes,"complete":false}),
                        )
                        .await?;
                }
            }
            return Err(failure("MCP server closed stdout"));
        }
        let end = chunk.iter().position(|b| *b == b'\n').map(|i| i + 1);
        let length = end.unwrap_or(chunk.len());
        if bytes.len().saturating_add(length) > FRAME_LIMIT {
            if let Some(observer) = observer {
                observer
                    .record(
                        server,
                        "rejected_stdout",
                        json!({"prefix_bytes":bytes,"complete":false,"reason":"1 MiB frame limit"}),
                    )
                    .await?;
            }
            return Err(failure("MCP frame exceeds 1 MiB"));
        }
        bytes.extend_from_slice(&chunk[..length]);
        output.consume(length);
        if end.is_some() {
            break;
        }
    }
    let result = serde_json::from_slice::<Value>(&bytes);
    if let Some(observer) = observer {
        let frame = match &result {
            Ok(value) => value.clone(),
            Err(_) => json!({"invalid_json_bytes":bytes}),
        };
        observer.record(server, "incoming", frame).await?;
    }
    result.map_err(|_| failure("MCP emitted invalid JSON"))
}

pub(super) struct ObservedOutput {
    pub receiver: mpsc::Receiver<Result<Value, McpError>>,
    pub pending: Arc<AtomicU64>,
    pub notifications: Arc<AtomicUsize>,
    reader: JoinHandle<()>,
    stderr: Option<JoinHandle<()>>,
}
impl Drop for ObservedOutput {
    fn drop(&mut self) {
        self.reader.abort();
        if let Some(task) = &self.stderr {
            task.abort();
        }
    }
}
impl ObservedOutput {
    pub fn new(
        mut output: BufReader<ChildStdout>,
        stderr: Option<ChildStderr>,
        input: Arc<Mutex<ChildStdin>>,
        observer: Arc<dyn McpIoObserver>,
        server: String,
        connected: Arc<AtomicBool>,
        pid: ProcessGuard,
    ) -> Self {
        let (sender, receiver) = mpsc::channel(1);
        let pending = Arc::new(AtomicU64::new(0));
        let notifications = Arc::new(AtomicUsize::new(0));
        let stderr = stderr.map(|mut stderr| {
            let (observer, server, connected, pid) = (
                observer.clone(),
                server.clone(),
                connected.clone(),
                pid.clone(),
            );
            tokio::spawn(async move {
                let mut bytes = [0_u8; 4096];
                loop {
                    match stderr.read(&mut bytes).await {
                        Ok(0) => break,
                        Ok(length) => {
                            let chunk = match std::str::from_utf8(&bytes[..length]) {
                                Ok(text) => json!({"text":text}),
                                Err(_) => json!({"bytes":&bytes[..length]}),
                            };
                            if observer.record(&server, "stderr", chunk).await.is_ok() {
                                continue;
                            }
                        }
                        Err(_) => {
                            let _ = observer
                                .record(&server, "stderr_error", json!({"status":"read_failed"}))
                                .await;
                        }
                    }
                    terminate_owned(&pid, &connected);
                    break;
                }
            })
        });
        let (request_id, counts) = (pending.clone(), notifications.clone());
        let reader = tokio::spawn(async move {
            let result:Result<(),McpError>=async {
                loop {
                    let frame=tokio::select! { biased; _=sender.closed()=>return Ok(()), frame=read_frame(&mut output,Some(&observer),&server)=>frame? };
                    if frame["jsonrpc"]!="2.0" { return Err(failure("invalid MCP envelope")); }
                    if let Some(method)=frame["method"].as_str() {
                        if request_id.load(Ordering::SeqCst)>0 && counts.fetch_add(1,Ordering::SeqCst)>=128 { return Err(failure("MCP notification limit exceeded")); }
                        if let Some(id)=frame.get("id") {
                            let response=if method=="ping" { json!({"jsonrpc":"2.0","id":id,"result":{}}) } else { json!({"jsonrpc":"2.0","id":id,"error":{"code":-32601,"message":"Unsupported client method"}}) };
                            send_frame(&input,Some(&observer),&server,response).await?;
                        }
                        continue;
                    }
                    let expected=request_id.load(Ordering::SeqCst);
                    if expected==0 || frame.get("id")!=Some(&json!(expected)) || request_id.compare_exchange(expected,0,Ordering::SeqCst,Ordering::SeqCst).is_err() { return Err(failure("unexpected MCP response identity")); }
                    sender.send(Ok(frame)).await.map_err(|_|failure("MCP response consumer closed"))?;
                }
            }.await;
            if let Err(error) = result {
                terminate_owned(&pid, &connected);
                let _ = observer
                    .record(
                        &server,
                        "reader_error",
                        json!({"status":"failed","error":error.to_string()}),
                    )
                    .await;
                let _ = sender.send(Err(error)).await;
            }
        });
        Self {
            receiver,
            pending,
            notifications,
            reader,
            stderr,
        }
    }
}
