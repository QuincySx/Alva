// INPUT:  subprocess::{SubprocessRuntime, ReadHalf, WriteHalf, ShutdownHandle},
//         protocol::{Request, Response, Notification, RpcError, methods, error_codes}
// OUTPUT: RpcDispatcher, HostHandler, NoopHostHandler, DispatchError
// POS:    Bounded bidirectional JSON-RPC dispatcher with cancellable pending calls and host concurrency limits.

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
use std::process::ExitStatus;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex as StdMutex};

use async_trait::async_trait;
use serde_json::Value;
use tokio::sync::{mpsc, oneshot, watch, Semaphore};
use tokio::task::{JoinHandle, JoinSet};

use crate::protocol::{error_codes, Notification, Request, RequestId, Response, RpcError};
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

struct DispatcherState {
    pending: HashMap<RequestId, oneshot::Sender<Response>>,
    transport_open: bool,
}

type SharedState = Arc<StdMutex<DispatcherState>>;

const DISPATCH_WRITE_QUEUE_CAPACITY: usize = 128;
const MAX_PENDING_REQUESTS: usize = 128;
const MAX_CONCURRENT_HOST_REQUESTS: usize = 32;
const MAX_LOGGED_FRAME_CHARS: usize = 2048;

struct PendingRequestGuard {
    id: RequestId,
    state: SharedState,
}

impl Drop for PendingRequestGuard {
    fn drop(&mut self) {
        if let Ok(mut state) = self.state.lock() {
            state.pending.remove(&self.id);
        }
    }
}

/// Bidirectional JSON-RPC dispatcher. See module docs for the model.
pub struct RpcDispatcher {
    name: String,
    next_id: AtomicU64,
    state: SharedState,
    write_tx: mpsc::Sender<String>,
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
        let state: SharedState = Arc::new(StdMutex::new(DispatcherState {
            pending: HashMap::new(),
            transport_open: true,
        }));
        let (write_tx, write_rx) = mpsc::channel::<String>(DISPATCH_WRITE_QUEUE_CAPACITY);
        let (shutdown_tx, shutdown_rx) = watch::channel(false);
        let host_request_limit = Arc::new(Semaphore::new(MAX_CONCURRENT_HOST_REQUESTS));

        let reader_handle = tokio::spawn(reader_loop(
            read_half,
            Arc::clone(&state),
            handler,
            write_tx.clone(),
            host_request_limit,
            shutdown_rx.clone(),
        ));
        let writer_handle = tokio::spawn(writer_loop(
            write_half,
            write_rx,
            Arc::clone(&state),
            shutdown_rx,
        ));

        Self {
            name,
            next_id: AtomicU64::new(0),
            state,
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
    pub async fn call(&self, method: &str, params: Option<Value>) -> Result<Value, DispatchError> {
        let seq = self.next_id.fetch_add(1, Ordering::Relaxed);
        let id: RequestId = format!("h-{}", seq);
        let request = Request::new(id.clone(), method, params);
        let json = serde_json::to_string(&request)?;

        let (tx, rx) = oneshot::channel();
        {
            let mut state = self
                .state
                .lock()
                .map_err(|_| DispatchError::PendingPoisoned)?;
            if !state.transport_open {
                return Err(DispatchError::ChannelClosed);
            }
            if state.pending.len() >= MAX_PENDING_REQUESTS {
                return Err(DispatchError::Capacity(MAX_PENDING_REQUESTS));
            }
            state.pending.insert(id.clone(), tx);
        }
        let _pending_guard = PendingRequestGuard {
            id: id.clone(),
            state: Arc::clone(&self.state),
        };

        self.write_tx
            .send(json)
            .await
            .map_err(|_| DispatchError::ChannelClosed)?;

        let response = match rx.await {
            Ok(r) => r,
            Err(_) => {
                // oneshot sender dropped without sending — typically
                // means the reader task exited / subprocess died.
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
    pub async fn notify(&self, method: &str, params: Option<Value>) -> Result<(), DispatchError> {
        if !self
            .state
            .lock()
            .map_err(|_| DispatchError::PendingPoisoned)?
            .transport_open
        {
            return Err(DispatchError::ChannelClosed);
        }
        let notif = Notification::new(method, params);
        let json = serde_json::to_string(&notif)?;
        self.write_tx
            .send(json)
            .await
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
            if let Ok(mut state) = self.state.lock() {
                state.transport_open = false;
                state.pending.clear();
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
    state: SharedState,
    handler: Arc<dyn HostHandler>,
    write_tx: mpsc::Sender<String>,
    host_request_limit: Arc<Semaphore>,
    mut shutdown_rx: watch::Receiver<bool>,
) {
    let plugin_name = read_half.name().to_string();
    let mut host_tasks = JoinSet::new();
    loop {
        tokio::select! {
            biased;
            _ = shutdown_rx.changed() => {
                if *shutdown_rx.borrow() {
                    tracing::debug!(plugin = %plugin_name, "reader task: shutdown signalled");
                    break;
                }
            }
            joined = host_tasks.join_next(), if !host_tasks.is_empty() => {
                if let Some(Err(error)) = joined {
                    tracing::error!(
                        plugin = %plugin_name,
                        error = %error,
                        "plugin host callback task failed"
                    );
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
                            &state,
                            &handler,
                            &write_tx,
                            &host_request_limit,
                            &mut host_tasks,
                        )
                        .await
                        {
                            tracing::error!(
                                plugin = %plugin_name,
                                error = %e,
                                frame_preview = %frame_preview(&line),
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

    // Plugin-originated callbacks belong to this dispatcher. They must not
    // survive EOF, shutdown, or a transport failure and retain host state in
    // detached tasks.
    host_tasks.abort_all();
    while host_tasks.join_next().await.is_some() {}
    close_transport(&state);
}

async fn dispatch_incoming(
    plugin_name: &str,
    line: &str,
    state: &SharedState,
    handler: &Arc<dyn HostHandler>,
    write_tx: &mpsc::Sender<String>,
    host_request_limit: &Arc<Semaphore>,
    host_tasks: &mut JoinSet<()>,
) -> Result<(), serde_json::Error> {
    let value: Value = serde_json::from_str(line)?;

    let has_method = value.get("method").is_some();
    let has_id = value.get("id").is_some();
    let has_result_or_error = value.get("result").is_some() || value.get("error").is_some();

    if has_result_or_error && has_id {
        let response: Response = serde_json::from_value(value)?;
        let waiter = {
            let mut state = match state.lock() {
                Ok(state) => state,
                Err(_) => {
                    tracing::error!(plugin = %plugin_name, "pending map poisoned");
                    return Ok(());
                }
            };
            state.pending.remove(&response.id)
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
        let permit = match Arc::clone(host_request_limit).try_acquire_owned() {
            Ok(permit) => permit,
            Err(_) => {
                tracing::warn!(
                    plugin = %plugin_name,
                    limit = MAX_CONCURRENT_HOST_REQUESTS,
                    "rejecting plugin host request because concurrency limit is full"
                );
                let response = Response::err(
                    request.id,
                    RpcError::new(
                        error_codes::INTERNAL_ERROR,
                        "host request concurrency limit exceeded",
                    ),
                );
                if let Ok(json) = serde_json::to_string(&response) {
                    let _ = write_tx.send(json).await;
                }
                return Ok(());
            }
        };
        let handler = Arc::clone(handler);
        let write_tx = write_tx.clone();
        let plugin = plugin_name.to_string();
        host_tasks.spawn(async move {
            let _permit = permit;
            let id = request.id.clone();
            let result = handler.handle_request(request.method, request.params).await;
            let response = match result {
                Ok(value) => Response::ok(id, value),
                Err(err) => Response::err(id, err),
            };
            match serde_json::to_string(&response) {
                Ok(json) => {
                    if write_tx.send(json).await.is_err() {
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
        let Ok(permit) = Arc::clone(host_request_limit).try_acquire_owned() else {
            tracing::warn!(
                plugin = %plugin_name,
                limit = MAX_CONCURRENT_HOST_REQUESTS,
                "dropping plugin notification because host concurrency limit is full"
            );
            return Ok(());
        };
        let handler = Arc::clone(handler);
        host_tasks.spawn(async move {
            let _permit = permit;
            handler
                .handle_notification(notif.method, notif.params)
                .await;
        });
    } else {
        tracing::warn!(
            plugin = %plugin_name,
            frame_preview = %frame_preview(line),
            "unparseable JSON-RPC message — neither request, response, nor notification"
        );
    }

    Ok(())
}

async fn writer_loop(
    mut write_half: WriteHalf,
    mut write_rx: mpsc::Receiver<String>,
    state: SharedState,
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
    close_transport(&state);
}

fn close_transport(state: &SharedState) {
    if let Ok(mut state) = state.lock() {
        state.transport_open = false;
        state.pending.clear();
    }
}

fn frame_preview(frame: &str) -> String {
    let mut preview: String = frame.chars().take(MAX_LOGGED_FRAME_CHARS).collect();
    if preview.len() < frame.len() {
        preview.push('…');
    }
    preview
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

    #[error("dispatcher reached its {0}-request capacity")]
    Capacity(usize),

    #[error("shutdown already called")]
    ShutdownAlreadyCalled,
}

#[cfg(test)]
mod tests {
    //! Tests for NoopHostHandler + DispatchError silent contracts.
    //!
    //! 1. **NoopHostHandler**: returns RpcError with code =
    //!    METHOD_NOT_FOUND (-32601 per JSON-RPC 2.0) and a diagnostic
    //!    message that names the requested method. A silent change to
    //!    INTERNAL_ERROR (-32603) would mislead plugins into treating
    //!    the response as a host-side bug and triggering retry logic.
    //!
    //! 2. **DispatchError Display + From**: only contracts that aren't
    //!    pure thiserror-derive are pinned —
    //!    - Subprocess Display: chains inner Display through `{0}`
    //!      interpolation (Serialization uses the same pattern; one
    //!      representative test suffices)
    //!    - Rpc Display: non-standard `code={} message={}` format
    //!      pulling from inner RpcError fields — a refactor collapsing
    //!      to `{0}` would let RpcError's own Display take over and
    //!      silently change log lines that downstream alerting greps
    //!    - From<SubprocessError>: `#[from]` produces the named variant
    //!      (silent contract: rename would silently break `?` callers'
    //!      match arms; same applies to From<serde_json::Error> but
    //!      one representative test suffices)
    use super::*;
    use crate::protocol::error_codes;

    #[tokio::test]
    async fn noop_handler_request_returns_method_not_found_with_method_name_in_diagnostic() {
        // Two pins in one call: the JSON-RPC 2.0 wire code (-32601)
        // and the diagnostic includes the rejected method name so
        // plugin authors can identify which method was unimplemented
        // from logs without --verbose.
        let handler = NoopHostHandler;
        let err = handler
            .handle_request("host/some.specific.method".to_string(), None)
            .await
            .expect_err("noop must reject every method");
        assert_eq!(err.code, error_codes::METHOD_NOT_FOUND);
        assert_eq!(err.code, -32601, "JSON-RPC 2.0 spec value pin");
        assert!(
            err.message.contains("host/some.specific.method"),
            "diagnostic must include requested method: {}",
            err.message
        );
    }

    #[test]
    fn dispatcher_limits_are_bounded() {
        assert_eq!(DISPATCH_WRITE_QUEUE_CAPACITY, 128);
        assert_eq!(MAX_PENDING_REQUESTS, 128);
        assert_eq!(MAX_CONCURRENT_HOST_REQUESTS, 32);
    }

    #[test]
    fn dropping_pending_guard_removes_cancelled_request() {
        let state: SharedState = Arc::new(StdMutex::new(DispatcherState {
            pending: HashMap::new(),
            transport_open: true,
        }));
        let (tx, _rx) = oneshot::channel();
        state.lock().unwrap().pending.insert("h-1".to_string(), tx);

        {
            let _guard = PendingRequestGuard {
                id: "h-1".to_string(),
                state: Arc::clone(&state),
            };
            assert_eq!(state.lock().unwrap().pending.len(), 1);
        }

        assert!(state.lock().unwrap().pending.is_empty());
    }

    #[test]
    fn closing_transport_fails_all_pending_requests_atomically() {
        let state: SharedState = Arc::new(StdMutex::new(DispatcherState {
            pending: HashMap::new(),
            transport_open: true,
        }));
        let (tx, mut rx) = oneshot::channel();
        state.lock().unwrap().pending.insert("h-1".to_string(), tx);

        close_transport(&state);

        let state_guard = state.lock().unwrap();
        assert!(!state_guard.transport_open);
        assert!(state_guard.pending.is_empty());
        drop(state_guard);
        assert!(matches!(
            rx.try_recv(),
            Err(oneshot::error::TryRecvError::Closed)
        ));
    }

    #[test]
    fn dispatch_error_subprocess_display_chains_inner_message_through() {
        // Pin: `#[error("subprocess error: {0}")]` chains the wrapped
        // Display. A refactor dropping `{0}` would silently lose inner
        // diagnostic. Same pattern applies to Serialization; one
        // representative test covers the thiserror chain-through
        // behavior for both variants.
        let inner = SubprocessError::AlreadyClosed;
        let inner_msg = inner.to_string();
        let e = DispatchError::Subprocess(inner);
        let s = e.to_string();
        assert!(s.starts_with("subprocess error:"), "prefix missing: {s}");
        assert!(s.contains(&inner_msg), "inner subprocess message lost: {s}");
    }

    #[test]
    fn dispatch_error_rpc_display_uses_custom_code_and_message_format() {
        // CRITICAL: the Display attr is
        // `#[error("rpc error: code={} message={}", .0.code, .0.message)]`.
        // A refactor to `#[error("{0}")]` would let RpcError's own
        // Display take over and silently change log lines.
        let e = DispatchError::Rpc(RpcError::new(-32601, "method 'x' not found"));
        assert_eq!(
            e.to_string(),
            "rpc error: code=-32601 message=method 'x' not found"
        );
    }

    #[test]
    fn from_subprocess_error_produces_named_subprocess_variant() {
        // Pin: `#[from] SubprocessError` → DispatchError::Subprocess(_)
        // via `?`. A rename of the variant would silently break match
        // arms in scan/start paths. Same applies to From<serde_json::Error>;
        // one representative test covers the #[from] contract for both.
        let inner = SubprocessError::AlreadyClosed;
        let e: DispatchError = inner.into();
        match e {
            DispatchError::Subprocess(_) => {}
            other => panic!("expected Subprocess variant, got {other:?}"),
        }
    }
}
