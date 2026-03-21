use std::sync::{Arc, Mutex};

use srow_core::ui_message::{UIMessage, UIMessagePart, UIMessageRole, ToolState};
use srow_core::ui_message_stream::{
    ChatStatus, FinishReason, UIMessageChunk,
};
use srow_core::ui_message_stream::processor::{process_ui_message_stream, UIMessageStreamUpdate};
use srow_core::error::ChatError;

use crate::transport::{ChatRequest, ChatTransport};
use crate::util::{AbortController, AbortHandle, SerialJobExecutor};

use super::chat_options::*;
use super::chat_state::ChatState;

use tokio::sync::mpsc;

/// Core chat orchestrator — drives the lifecycle of a single chat conversation.
///
/// Manages: user messages -> transport -> stream processing -> state updates.
///
/// Interior mutability: mutable state lives inside `Arc<Mutex<ChatInner<S>>>`.
/// All public methods take `&self`.
///
/// Thread safety: `ChatState` is NOT required to be `Send + Sync`, but the
/// `Mutex` wrapper ensures the outer `AbstractChat` can be shared. The lock
/// is held only briefly during reads/writes.
pub struct AbstractChat<S: ChatState> {
    inner: Arc<Mutex<ChatInner<S>>>,
    transport: Arc<dyn ChatTransport>,
    job_executor: SerialJobExecutor,
    generate_id: Arc<dyn Fn() -> String + Send + Sync>,
    #[allow(dead_code)]
    on_tool_call: Option<AsyncToolCallHandler>,
    on_finish: Option<Arc<dyn Fn(FinishInfo) + Send + Sync>>,
    on_error: Option<Arc<dyn Fn(ChatError) + Send + Sync>>,
    send_automatically_when: Option<Arc<dyn Fn(&UIMessage) -> bool + Send + Sync>>,
}

struct ChatInner<S: ChatState> {
    id: String,
    state: S,
    active_abort: Option<AbortController>,
}

impl<S: ChatState + Send + 'static> AbstractChat<S> {
    /// Create a new `AbstractChat` from initialization parameters.
    pub fn new(init: ChatInit<S>) -> Self {
        let generate_id: Arc<dyn Fn() -> String + Send + Sync> = match init.generate_id {
            Some(f) => Arc::from(f),
            None => Arc::new(|| uuid::Uuid::new_v4().to_string()),
        };

        let mut state = init.state;

        // If initial_messages were provided, set them on state.
        if !init.initial_messages.is_empty() {
            state.set_messages(init.initial_messages);
        }

        let inner = ChatInner {
            id: init.id,
            state,
            active_abort: None,
        };

        Self {
            inner: Arc::new(Mutex::new(inner)),
            transport: Arc::from(init.transport),
            job_executor: SerialJobExecutor::new(&init.runtime_handle),
            generate_id,
            on_tool_call: init.on_tool_call,
            on_finish: init.on_finish.map(|f| Arc::from(f)),
            on_error: init.on_error.map(|f| Arc::from(f)),
            send_automatically_when: init.send_automatically_when.map(|f| Arc::from(f)),
        }
    }

    // -----------------------------------------------------------------------
    // Public API — all &self
    // -----------------------------------------------------------------------

    /// Send a user message and begin a request to the AI.
    pub async fn send_message(&self, parts: Vec<UIMessagePart>, options: SendOptions) {
        let msg = UIMessage {
            id: (self.generate_id)(),
            role: UIMessageRole::User,
            parts,
            metadata: options.metadata,
        };

        {
            let mut inner = self.inner.lock().unwrap();
            inner.state.push_message(msg);
            inner.state.notify_messages_changed();
        }

        self.make_request().await;
    }

    /// Regenerate the last assistant response — removes the last assistant
    /// message and re-runs the request.
    pub async fn regenerate(&self, _options: RegenerateOptions) {
        {
            let mut inner = self.inner.lock().unwrap();
            // Pop the last message if it's an assistant message.
            if let Some(last) = inner.state.messages().last().cloned() {
                if last.role == UIMessageRole::Assistant {
                    inner.state.pop_message();
                    inner.state.notify_messages_changed();
                }
            }
        }

        self.make_request().await;
    }

    /// Stop the current in-flight request.
    pub async fn stop(&self) {
        let mut inner = self.inner.lock().unwrap();
        if let Some(controller) = inner.active_abort.take() {
            controller.abort();
        }
        inner.state.set_status(ChatStatus::Ready);
        inner.state.notify_status_changed();
    }

    /// Resume a previously interrupted stream (reconnect).
    pub async fn resume_stream(&self) {
        let chat_id = {
            let inner = self.inner.lock().unwrap();
            inner.id.clone()
        };

        match self.transport.reconnect(&chat_id).await {
            Ok(Some(stream)) => {
                let inner = self.inner.clone();
                let generate_id = self.generate_id.clone();
                let on_finish = self.on_finish.clone();
                let on_error = self.on_error.clone();

                // Create an abort controller for the resume.
                let (controller, abort_handle) = AbortController::new();
                {
                    let mut inner_guard = inner.lock().unwrap();
                    inner_guard.active_abort = Some(controller);
                }

                self.job_executor
                    .run(async move {
                        Self::consume_stream(
                            inner,
                            stream,
                            abort_handle,
                            generate_id,
                            on_finish,
                            on_error,
                        )
                        .await;
                    })
                    .await;
            }
            Ok(None) => {
                // No stream to resume.
            }
            Err(e) => {
                let mut inner = self.inner.lock().unwrap();
                let err = ChatError::Transport(e.to_string());
                inner.state.set_error(Some(err.clone()));
                inner.state.set_status(ChatStatus::Error);
                inner.state.notify_error_changed();
                inner.state.notify_status_changed();
                if let Some(on_error) = &self.on_error {
                    on_error(err);
                }
            }
        }
    }

    /// Add tool output for a tool call. Optionally triggers auto-send if all
    /// tool parts are in a terminal state and `send_automatically_when` returns true.
    pub async fn add_tool_output(&self, tool_call_id: &str, output: serde_json::Value) {
        let should_auto_send = {
            let mut inner = self.inner.lock().unwrap();
            let messages = inner.state.messages();

            // Find the message containing this tool call and update it.
            let mut updated_messages = messages;
            let mut found = false;

            for msg in updated_messages.iter_mut().rev() {
                for part in msg.parts.iter_mut() {
                    if let UIMessagePart::Tool {
                        id,
                        output: ref mut tool_output,
                        state: ref mut tool_state,
                        ..
                    } = part
                    {
                        if id == tool_call_id {
                            *tool_output = Some(output.clone());
                            *tool_state = ToolState::OutputAvailable;
                            found = true;
                            break;
                        }
                    }
                }
                if found {
                    break;
                }
            }

            if found {
                inner.state.set_messages(updated_messages.clone());
                inner.state.notify_messages_changed();
            }

            // Check if we should auto-send: all Tool parts in last assistant
            // message must be in a terminal state.
            if let Some(send_when) = &self.send_automatically_when {
                if let Some(last_msg) = updated_messages.last() {
                    if last_msg.role == UIMessageRole::Assistant {
                        let all_terminal = last_msg.parts.iter().all(|p| match p {
                            UIMessagePart::Tool { state, .. } => matches!(
                                state,
                                ToolState::OutputAvailable
                                    | ToolState::OutputError
                                    | ToolState::OutputDenied
                            ),
                            _ => true,
                        });
                        all_terminal && send_when(last_msg)
                    } else {
                        false
                    }
                } else {
                    false
                }
            } else {
                false
            }
        };

        if should_auto_send {
            self.make_request().await;
        }
    }

    /// Record that the user approved or denied a tool call.
    pub fn add_tool_approval_response(&self, tool_call_id: &str, approved: bool) {
        let mut inner = self.inner.lock().unwrap();
        let mut messages = inner.state.messages();

        let new_state = if approved {
            ToolState::ApprovalResponded
        } else {
            ToolState::OutputDenied
        };

        let mut found = false;
        for msg in messages.iter_mut().rev() {
            for part in msg.parts.iter_mut() {
                if let UIMessagePart::Tool {
                    id,
                    state: ref mut tool_state,
                    ..
                } = part
                {
                    if id == tool_call_id {
                        *tool_state = new_state.clone();
                        found = true;
                        break;
                    }
                }
            }
            if found {
                break;
            }
        }

        if found {
            inner.state.set_messages(messages);
            inner.state.notify_messages_changed();
        }
    }

    /// Clear the current error and reset status to Ready.
    pub fn clear_error(&self) {
        let mut inner = self.inner.lock().unwrap();
        inner.state.set_error(None);
        inner.state.set_status(ChatStatus::Ready);
        inner.state.notify_error_changed();
        inner.state.notify_status_changed();
    }

    /// Read state inside the lock — the closure receives an immutable reference.
    pub fn with_state<R>(&self, f: impl FnOnce(&S) -> R) -> R {
        let inner = self.inner.lock().unwrap();
        f(&inner.state)
    }

    /// Get the chat ID.
    pub fn id(&self) -> String {
        self.inner.lock().unwrap().id.clone()
    }

    // -----------------------------------------------------------------------
    // Internal
    // -----------------------------------------------------------------------

    /// Build and send a request through the transport, then consume the stream.
    async fn make_request(&self) {
        let inner = self.inner.clone();
        let transport = self.transport.clone();
        let generate_id = self.generate_id.clone();
        let on_finish = self.on_finish.clone();
        let on_error = self.on_error.clone();

        self.job_executor
            .run(async move {
                // 1. Create AbortController, set status to Submitted.
                let (controller, abort_handle) = AbortController::new();
                {
                    let mut inner = inner.lock().unwrap();
                    inner.active_abort = Some(controller);
                    inner.state.set_status(ChatStatus::Submitted);
                    inner.state.notify_status_changed();
                }

                // 2. Snapshot messages and chat_id.
                let (messages, chat_id) = {
                    let inner = inner.lock().unwrap();
                    (inner.state.messages(), inner.id.clone())
                };

                // 3. Call transport.
                let request = ChatRequest {
                    chat_id,
                    messages,
                    abort_handle: abort_handle.clone(),
                };

                let stream = match transport.send_messages(request).await {
                    Ok(s) => s,
                    Err(e) => {
                        let mut inner = inner.lock().unwrap();
                        let err = ChatError::Transport(e.to_string());
                        inner.state.set_error(Some(err.clone()));
                        inner.state.set_status(ChatStatus::Error);
                        inner.state.notify_error_changed();
                        inner.state.notify_status_changed();
                        inner.active_abort = None;
                        if let Some(on_error) = &on_error {
                            on_error(err);
                        }
                        return;
                    }
                };

                Self::consume_stream(inner, stream, abort_handle, generate_id, on_finish, on_error)
                    .await;
            })
            .await;
    }

    /// Consume a chunk stream, building up the assistant message and updating state.
    ///
    /// The `abort_handle` is checked during update consumption. When aborted, the
    /// stream processor task is cancelled and state is set back to Ready.
    async fn consume_stream(
        inner: Arc<Mutex<ChatInner<S>>>,
        stream: std::pin::Pin<
            Box<
                dyn futures::Stream<Item = Result<UIMessageChunk, srow_core::error::StreamError>>
                    + Send,
            >,
        >,
        mut abort_handle: AbortHandle,
        generate_id: Arc<dyn Fn() -> String + Send + Sync>,
        on_finish: Option<Arc<dyn Fn(FinishInfo) + Send + Sync>>,
        on_error: Option<Arc<dyn Fn(ChatError) + Send + Sync>>,
    ) {
        // Create initial assistant message.
        let assistant_msg = UIMessage {
            id: (generate_id)(),
            role: UIMessageRole::Assistant,
            parts: vec![],
            metadata: None,
        };

        // Create update channel and process stream.
        let (update_tx, mut update_rx) = mpsc::unbounded_channel();

        let process_handle = tokio::spawn(async move {
            process_ui_message_stream(stream, assistant_msg, update_tx).await
        });

        // Consume updates, racing with abort signal.
        let mut final_state = None;
        let mut aborted = false;

        loop {
            tokio::select! {
                update = update_rx.recv() => {
                    match update {
                        Some(UIMessageStreamUpdate::FirstWrite(msg)) => {
                            let mut inner = inner.lock().unwrap();
                            inner.state.set_status(ChatStatus::Streaming);
                            inner.state.push_message(msg);
                            inner.state.notify_status_changed();
                            inner.state.notify_messages_changed();
                        }
                        Some(UIMessageStreamUpdate::MessageChanged(msg)) => {
                            let mut inner = inner.lock().unwrap();
                            let len = inner.state.messages().len();
                            if len > 0 {
                                inner.state.replace_message(len - 1, msg);
                                inner.state.notify_messages_changed();
                            }
                        }
                        Some(UIMessageStreamUpdate::Finished {
                            message,
                            finish_reason,
                            usage,
                        }) => {
                            final_state = Some((message, finish_reason, usage));
                        }
                        None => {
                            // Channel closed — processor finished.
                            break;
                        }
                    }
                }
                _ = abort_handle.cancelled() => {
                    // Abort requested — cancel the processor task.
                    process_handle.abort();
                    aborted = true;
                    break;
                }
            }
        }

        if aborted {
            // Status is already set to Ready by stop(). Just clean up.
            let mut inner = inner.lock().unwrap();
            inner.active_abort = None;
            return;
        }

        // Wait for processor to complete (if not already).
        let result = process_handle.await;

        // Update final status.
        {
            let mut inner = inner.lock().unwrap();
            inner.active_abort = None;
            inner.state.set_status(ChatStatus::Ready);
            inner.state.notify_status_changed();
        }

        // Call on_finish callback.
        if let Some((message, finish_reason, usage)) = final_state {
            if let Some(on_finish) = &on_finish {
                on_finish(FinishInfo {
                    message,
                    finish_reason: finish_reason.unwrap_or(FinishReason::Stop),
                    usage,
                });
            }
        }

        // Handle processor error.
        if let Ok(Err(e)) = result {
            let mut inner = inner.lock().unwrap();
            let err = ChatError::Stream(e.to_string());
            inner.state.set_error(Some(err.clone()));
            inner.state.set_status(ChatStatus::Error);
            inner.state.notify_error_changed();
            inner.state.notify_status_changed();
            if let Some(on_error) = &on_error {
                on_error(err);
            }
        }
    }
}
