// INPUT:  subprocess::{SubprocessRuntime, ReadHalf, WriteHalf, ShutdownHandle},
//         protocol::{Request, Response, Notification, RpcError, methods, error_codes}
// OUTPUT: RpcDispatcher, HostHandler, NoopHostHandler, DispatchError
// POS:    Phase 2 — bidirectional JSON-RPC dispatcher on top of a subprocess.

//! Bidirectional JSON-RPC 2.0 dispatcher for AEP plugins.
//!
//! Runs two background tasks on top of a split `SubprocessRuntime`:
//!
//! - a **reader** task that parses each inbound line as a JSON-RPC
//!   message, routes responses to pending host-originated calls,
//!   dispatches plugin-originated requests / notifications through a
//!   user-supplied [`HostHandler`], and logs parse errors
//! - a **writer** task that serialises outgoing messages and writes
//!   them to stdin one line at a time
//!
//! Host code calls [`RpcDispatcher::call`] to send a request and
//! await a response, or [`RpcDispatcher::notify`] for fire-and-forget
//! notifications. Shutdown is ordered: signal both tasks to stop,
//! join them, then drive the subprocess to exit.
//!
//! This module is **transport + dispatch only** — it does not know
//! anything about AEP event semantics. Payload validation (does this
//! `ExtensionAction` variant fit this event?) lives in Phase 3.

use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex as StdMutex};
use std::process::ExitStatus;

use async_trait::async_trait;
use serde_json::Value;
use tokio::sync::{mpsc, oneshot, watch};
use tokio::task::JoinHandle;

use crate::protocol::{
    error_codes, Notification, Request, RequestId, Response, RpcError,
};
use crate::subprocess::{ReadHalf, ShutdownHandle, SubprocessError, SubprocessRuntime, WriteHalf};

// ===========================================================
// Host handler trait — what plugins can call back on the host
// ===========================================================

/// Trait implemented by the host to respond to plugin-originated
/// requests and notifications (the `host/*` half of AEP).
///
/// Phase 2 only uses this for the smoke test, where a no-op handler
/// suffices. Phase 3 will wire the real implementation that backs
/// `host/log`, `host/state.*`, `host/memory.*`, etc.
#[async_trait]
pub trait HostHandler: Send + Sync + 'static {
    /// Handle a JSON-RPC request from the plugin. Return either a
    /// JSON value to be sent back as `result`, or an [`RpcError`]
    /// that becomes the response's `error` field.
    async fn handle_request(
        &self,
        method: String,
        params: Option<Value>,
    ) -> Result<Value, RpcError>;

    /// Handle a JSON-RPC notification from the plugin. No response
    /// is sent regardless of what this method returns.
    async fn handle_notification(&self, method: String, params: Option<Value>);
}

/// A handler that rejects every request with `METHOD_NOT_FOUND` and
/// drops every notification. Used by the smoke test and by crates
/// that only want the host → plugin half of the protocol.
pub struct NoopHostHandler;

#[async_trait]
impl HostHandler for NoopHostHandler {
    async fn handle_request(
        &self,
        method: String,
        _params: Option<Value>,
    ) -> Result<Value, RpcError> {
        tracing::debug!(method = %method, "NoopHostHandler: method not found");
        Err(RpcError::new(
            error_codes::METHOD_NOT_FOUND,
            format!("method '{}' not implemented on host", method),
        ))
    }

    async fn handle_notification(&self, method: String, _params: Option<Value>) {
        tracing::debug!(method = %method, "NoopHostHandler: notification dropped");
    }
}

// ===========================================================
// Dispatcher
// ===========================================================

type PendingMap = Arc<StdMutex<HashMap<RequestId, oneshot::Sender<Response>>>>;

/// Bidirectional JSON-RPC dispatcher. See module docs for the model.
pub struct RpcDispatcher {
    name: String,
    next_id: AtomicU64,
    pending: PendingMap,
    write_tx: mpsc::UnboundedSender<String>,
    shutdown_tx: watch::Sender<bool>,
    reader_handle: Option<JoinHandle<()>>,
    writer_handle: Option<JoinHandle<()>>,
    shutdown: Option<ShutdownHandle>,
}

impl RpcDispatcher {
    /// Spawn reader and writer tasks on top of a live subprocess.
    ///
    /// The subprocess is immediately split; the returned dispatcher
    /// owns everything needed to drive it until
    /// [`RpcDispatcher::shutdown`] is called.
    pub fn spawn(runtime: SubprocessRuntime, handler: Arc<dyn HostHandler>) -> Self {
        let name = runtime.name().to_string();
        let (read_half, write_half, shutdown) = runtime.split();
        let pending: PendingMap = Arc::new(StdMutex::new(HashMap::new()));
        let (write_tx, write_rx) = mpsc::unbounded_channel::<String>();
        let (shutdown_tx, shutdown_rx) = watch::channel(false);

        let reader_handle = tokio::spawn(reader_loop(
            read_half,
            Arc::clone(&pending),
            handler,
            write_tx.clone(),
            shutdown_rx.clone(),
        ));
        let writer_handle = tokio::spawn(writer_loop(write_half, write_rx, shutdown_rx));

        Self {
            name,
            next_id: AtomicU64::new(0),
            pending,
            write_tx,
            shutdown_tx,
            reader_handle: Some(reader_handle),
            writer_handle: Some(writer_handle),
            shutdown: Some(shutdown),
        }
    }

    /// Plugin name (for logging).
    pub fn name(&self) -> &str {
        &self.name
    }

    /// Send a JSON-RPC request and await the response.
    pub async fn call(
        &self,
        method: &str,
        params: Option<Value>,
    ) -> Result<Value, DispatchError> {
        let seq = self.next_id.fetch_add(1, Ordering::Relaxed);
        let id: RequestId = format!("h-{}", seq);
        let request = Request::new(id.clone(), method, params);
        let json = serde_json::to_string(&request)?;

        let (tx, rx) = oneshot::channel();
        {
            let mut pending = self
                .pending
                .lock()
                .map_err(|_| DispatchError::PendingPoisoned)?;
            pending.insert(id.clone(), tx);
        }

        self.write_tx
            .send(json)
            .map_err(|_| DispatchError::ChannelClosed)?;

        let response = match rx.await {
            Ok(r) => r,
            Err(_) => {
                // oneshot sender dropped without sending — typically
                // means the reader task exited / subprocess died.
                let mut pending = self
                    .pending
                    .lock()
                    .map_err(|_| DispatchError::PendingPoisoned)?;
                pending.remove(&id);
                return Err(DispatchError::ChannelClosed);
            }
        };

        if let Some(err) = response.error {
            Err(DispatchError::Rpc(err))
        } else if let Some(result) = response.result {
            Ok(result)
        } else {
            Err(DispatchError::MalformedResponse)
        }
    }

    /// Send a JSON-RPC notification. Does not await a response.
    pub async fn notify(
        &self,
        method: &str,
        params: Option<Value>,
    ) -> Result<(), DispatchError> {
        let notif = Notification::new(method, params);
        let json = serde_json::to_string(&notif)?;
        self.write_tx
            .send(json)
            .map_err(|_| DispatchError::ChannelClosed)
    }

    /// Shut down the dispatcher and the underlying subprocess.
    ///
    /// Order: signal both tasks → join writer (drops stdin → child
    /// sees EOF) → join reader (drops its write_tx clone) → reap the
    /// child with the grace-period escalation from
    /// [`ShutdownHandle::shutdown`].
    pub async fn shutdown(mut self) -> Result<ExitStatus, DispatchError> {
        // Fire the shutdown signal.
        let _ = self.shutdown_tx.send(true);

        // Drop our write_tx so the writer task sees no more senders
        // once the reader task also drops its clone.
        let writer_handle = self
            .writer_handle
            .take()
            .ok_or(DispatchError::ShutdownAlreadyCalled)?;
        let reader_handle = self
            .reader_handle
            .take()
            .ok_or(DispatchError::ShutdownAlreadyCalled)?;
        let shutdown = self
            .shutdown
            .take()
            .ok_or(DispatchError::ShutdownAlreadyCalled)?;

        // Join writer first so stdin closes cleanly.
        match writer_handle.await {
            Ok(()) => {}
            Err(e) if e.is_cancelled() => {}
            Err(e) => tracing::warn!(plugin = %self.name, error = %e, "writer task join failed"),
        }

        // Join reader — once stdin is closed, the plugin exits, the
        // stdout stream EOFs, and reader_loop breaks out.
        match reader_handle.await {
            Ok(()) => {}
            Err(e) if e.is_cancelled() => {}
            Err(e) => tracing::warn!(plugin = %self.name, error = %e, "reader task join failed"),
        }

        // Fail any still-pending callers.
        {
            if let Ok(mut pending) = self.pending.lock() {
                pending.clear();
            }
        }

        Ok(shutdown.shutdown().await?)
    }
}

impl Drop for RpcDispatcher {
    fn drop(&mut self) {
        if self.shutdown.is_some() {
            tracing::warn!(
                plugin = %self.name,
                "RpcDispatcher dropped without calling shutdown(); child may leak until task cleanup"
            );
            // shutdown_tx fires on drop; child has kill_on_drop=true
            // so it will be killed when `shutdown` drops.
            let _ = self.shutdown_tx.send(true);
        }
    }
}

// ===========================================================
// Task loops
// ===========================================================

async fn reader_loop(
    mut read_half: ReadHalf,
    pending: PendingMap,
    handler: Arc<dyn HostHandler>,
    write_tx: mpsc::UnboundedSender<String>,
    mut shutdown_rx: watch::Receiver<bool>,
) {
    let plugin_name = read_half.name().to_string();
    loop {
        tokio::select! {
            biased;
            _ = shutdown_rx.changed() => {
                if *shutdown_rx.borrow() {
                    tracing::debug!(plugin = %plugin_name, "reader task: shutdown signalled");
                    break;
                }
            }
            result = read_half.read_message() => {
                match result {
                    Ok(None) => {
                        tracing::debug!(plugin = %plugin_name, "reader task: stdout EOF");
                        break;
                    }
                    Ok(Some(line)) => {
                        if let Err(e) = dispatch_incoming(
                            &plugin_name,
                            &line,
                            &pending,
                            &handler,
                            &write_tx,
                        )
                        .await
                        {
                            tracing::error!(
                                plugin = %plugin_name,
                                error = %e,
                                line = %line,
                                "reader task: dispatch failed"
                            );
                        }
                    }
                    Err(e) => {
                        tracing::error!(
                            plugin = %plugin_name,
                            error = %e,
                            "reader task: read error"
                        );
                        break;
                    }
                }
            }
        }
    }
}

async fn dispatch_incoming(
    plugin_name: &str,
    line: &str,
    pending: &PendingMap,
    handler: &Arc<dyn HostHandler>,
    write_tx: &mpsc::UnboundedSender<String>,
) -> Result<(), serde_json::Error> {
    let value: Value = serde_json::from_str(line)?;

    let has_method = value.get("method").is_some();
    let has_id = value.get("id").is_some();
    let has_result_or_error =
        value.get("result").is_some() || value.get("error").is_some();

    if has_result_or_error && has_id {
        let response: Response = serde_json::from_value(value)?;
        let waiter = {
            let mut pending = match pending.lock() {
                Ok(p) => p,
                Err(_) => {
                    tracing::error!(plugin = %plugin_name, "pending map poisoned");
                    return Ok(());
                }
            };
            pending.remove(&response.id)
        };
        match waiter {
            Some(tx) => {
                let _ = tx.send(response);
            }
            None => {
                tracing::warn!(
                    plugin = %plugin_name,
                    id = %response.id,
                    "received response for unknown request id"
                );
            }
        }
    } else if has_method && has_id {
        // Plugin-originated request.
        let request: Request = serde_json::from_value(value)?;
        let handler = Arc::clone(handler);
        let write_tx = write_tx.clone();
        let plugin = plugin_name.to_string();
        tokio::spawn(async move {
            let id = request.id.clone();
            let result = handler.handle_request(request.method, request.params).await;
            let response = match result {
                Ok(value) => Response::ok(id, value),
                Err(err) => Response::err(id, err),
            };
            match serde_json::to_string(&response) {
                Ok(json) => {
                    if write_tx.send(json).is_err() {
                        tracing::warn!(
                            plugin = %plugin,
                            "could not send response: writer channel closed"
                        );
                    }
                }
                Err(e) => tracing::error!(
                    plugin = %plugin,
                    error = %e,
                    "failed to serialize host response"
                ),
            }
        });
    } else if has_method {
        // Notification.
        let notif: Notification = serde_json::from_value(value)?;
        let handler = Arc::clone(handler);
        tokio::spawn(async move {
            handler.handle_notification(notif.method, notif.params).await;
        });
    } else {
        tracing::warn!(
            plugin = %plugin_name,
            line = %line,
            "unparseable JSON-RPC message — neither request, response, nor notification"
        );
    }

    Ok(())
}

async fn writer_loop(
    mut write_half: WriteHalf,
    mut write_rx: mpsc::UnboundedReceiver<String>,
    mut shutdown_rx: watch::Receiver<bool>,
) {
    let plugin_name = write_half.name().to_string();
    loop {
        tokio::select! {
            biased;
            _ = shutdown_rx.changed() => {
                if *shutdown_rx.borrow() {
                    tracing::debug!(plugin = %plugin_name, "writer task: shutdown signalled");
                    break;
                }
            }
            cmd = write_rx.recv() => {
                match cmd {
                    Some(line) => {
                        if let Err(e) = write_half.write_message(&line).await {
                            tracing::error!(
                                plugin = %plugin_name,
                                error = %e,
                                "writer task: write error"
                            );
                            break;
                        }
                    }
                    None => {
                        tracing::debug!(plugin = %plugin_name, "writer task: channel closed");
                        break;
                    }
                }
            }
        }
    }
    // write_half drops here -> stdin closes.
}

// ===========================================================
// Error
// ===========================================================

#[derive(Debug, thiserror::Error)]
pub enum DispatchError {
    #[error("dispatcher channel closed")]
    ChannelClosed,

    #[error("subprocess error: {0}")]
    Subprocess(#[from] SubprocessError),

    #[error("rpc error: code={} message={}", .0.code, .0.message)]
    Rpc(RpcError),

    #[error("serialization error: {0}")]
    Serialization(#[from] serde_json::Error),

    #[error("response had neither result nor error")]
    MalformedResponse,

    #[error("pending-request map mutex was poisoned")]
    PendingPoisoned,

    #[error("shutdown already called")]
    ShutdownAlreadyCalled,
}
