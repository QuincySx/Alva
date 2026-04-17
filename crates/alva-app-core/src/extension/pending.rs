//! Pending user-message injection — mid-run steering.
//!
//! External callers (UI, CLI, integration drivers) queue a "user said
//! this while you were working" message via `PendingService::add`. At
//! the next LLM call boundary, `PendingMiddleware` drains unread messages,
//! marks them as read, prepends a system notice, and appends each as a
//! user message to both the LLM request and the session log.
//!
//! **Not registered by default.** The `BaseAgent` builder does not add
//! this extension; callers that want mid-run steering opt in with
//! `.extension(Box::new(PendingExtension::new()))`. This keeps the
//! kernel free of steering-specific logic and lets downstream code
//! subscribe to the service directly without an automatic forwarding
//! layer.
//!
//! # Cancellation
//!
//! A queued message may be cancelled via `PendingService::cancel(uuid)`
//! if it has not yet been drained by the middleware. Once drained (i.e.
//! injected into an LLM call), it is logged in the session and cannot be
//! retracted.

use std::sync::{Arc, Mutex as StdMutex};

use async_trait::async_trait;

use alva_kernel_abi::{AgentMessage, Message};
use alva_kernel_core::middleware::{Middleware, MiddlewareError};
use alva_kernel_core::state::AgentState;

use super::{Extension, ExtensionContext, HostAPI};

/// A user message queued for injection at the next LLM call.
#[derive(Debug, Clone)]
pub struct PendingMessage {
    pub uuid: String,
    pub text: String,
    /// Unix epoch millis — when `add` returned.
    pub created_at: i64,
    /// `None` while unread. Set to the epoch millis of the injection
    /// when the middleware drains this message.
    pub read_at: Option<i64>,
}

/// Public surface for queuing / cancelling / inspecting pending messages.
/// Consumers grab this off the agent bus via `bus.get::<dyn PendingService>()`.
pub trait PendingService: Send + Sync {
    /// Queue a new message. Returns its uuid for later cancellation.
    fn add(&self, text: &str) -> String;

    /// Cancel a not-yet-injected message. Returns `true` if the message
    /// was unread and has been removed; `false` if the uuid is unknown
    /// or the message was already drained.
    fn cancel(&self, uuid: &str) -> bool;

    /// Snapshot of every message that hasn't been injected yet.
    fn list_unread(&self) -> Vec<PendingMessage>;

    /// Full history (including already-injected messages).
    fn list_all(&self) -> Vec<PendingMessage>;
}

struct PendingState {
    /// Every message ever added. `cancel` removes unread entries outright;
    /// read entries are retained so callers can inspect history.
    messages: Vec<PendingMessage>,
}

/// Default `PendingService` implementation — in-process, mutex-protected.
#[derive(Clone)]
pub struct PendingServiceImpl {
    inner: Arc<StdMutex<PendingState>>,
}

impl PendingServiceImpl {
    pub fn new() -> Self {
        Self {
            inner: Arc::new(StdMutex::new(PendingState {
                messages: Vec::new(),
            })),
        }
    }

    /// Atomically take every unread message and mark each as read.
    /// Used by the middleware's `before_llm_call` hook.
    fn drain_unread(&self) -> Vec<PendingMessage> {
        let now = chrono::Utc::now().timestamp_millis();
        let mut state = self.inner.lock().unwrap_or_else(|e| e.into_inner());
        let mut drained = Vec::new();
        for m in &mut state.messages {
            if m.read_at.is_none() {
                m.read_at = Some(now);
                drained.push(m.clone());
            }
        }
        drained
    }
}

impl Default for PendingServiceImpl {
    fn default() -> Self {
        Self::new()
    }
}

impl PendingService for PendingServiceImpl {
    fn add(&self, text: &str) -> String {
        let uuid = uuid::Uuid::new_v4().to_string();
        let now = chrono::Utc::now().timestamp_millis();
        let mut state = self.inner.lock().unwrap_or_else(|e| e.into_inner());
        state.messages.push(PendingMessage {
            uuid: uuid.clone(),
            text: text.to_string(),
            created_at: now,
            read_at: None,
        });
        uuid
    }

    fn cancel(&self, uuid: &str) -> bool {
        let mut state = self.inner.lock().unwrap_or_else(|e| e.into_inner());
        if let Some(pos) = state
            .messages
            .iter()
            .position(|m| m.uuid == uuid && m.read_at.is_none())
        {
            state.messages.remove(pos);
            true
        } else {
            false
        }
    }

    fn list_unread(&self) -> Vec<PendingMessage> {
        let state = self.inner.lock().unwrap_or_else(|e| e.into_inner());
        state
            .messages
            .iter()
            .filter(|m| m.read_at.is_none())
            .cloned()
            .collect()
    }

    fn list_all(&self) -> Vec<PendingMessage> {
        let state = self.inner.lock().unwrap_or_else(|e| e.into_inner());
        state.messages.clone()
    }
}

/// Middleware that injects unread pending messages into each LLM call.
struct PendingMiddleware {
    svc: Arc<PendingServiceImpl>,
}

#[async_trait]
impl Middleware for PendingMiddleware {
    fn name(&self) -> &str {
        "pending_messages"
    }

    async fn before_llm_call(
        &self,
        state: &mut AgentState,
        messages: &mut Vec<Message>,
    ) -> Result<(), MiddlewareError> {
        let unread = self.svc.drain_unread();
        if unread.is_empty() {
            return Ok(());
        }

        // Frame the injection with a system notice so the model understands
        // these messages arrived out of band rather than as a natural next
        // turn. Then add each pending as a user message, and also append to
        // the session log so downstream observers see them on the same
        // channel as any other user message.
        let notice = format!(
            "[The user interjected while you were working. {} new message(s) follow.]",
            unread.len()
        );
        messages.push(Message::system(notice));
        for m in &unread {
            messages.push(Message::user(&m.text));
            state
                .session
                .append_message(
                    AgentMessage::Standard(Message::user(&m.text)),
                    None,
                )
                .await;
        }
        Ok(())
    }
}

/// The opt-in Extension. Publishes `Arc<dyn PendingService>` on the bus
/// and registers a `PendingMiddleware` that drains unread messages at
/// the start of each LLM call.
pub struct PendingExtension {
    svc: Arc<PendingServiceImpl>,
}

impl PendingExtension {
    pub fn new() -> Self {
        Self {
            svc: Arc::new(PendingServiceImpl::new()),
        }
    }

    /// Direct handle for callers that hold the extension. Normally
    /// consumers fetch the service from the bus instead.
    pub fn service(&self) -> Arc<dyn PendingService> {
        self.svc.clone() as Arc<dyn PendingService>
    }
}

impl Default for PendingExtension {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl Extension for PendingExtension {
    fn name(&self) -> &str {
        "pending_messages"
    }

    fn description(&self) -> &str {
        "Inject out-of-band user messages at the next LLM call boundary"
    }

    fn activate(&self, api: &HostAPI) {
        api.middleware(Arc::new(PendingMiddleware {
            svc: self.svc.clone(),
        }));
    }

    async fn configure(&self, ctx: &ExtensionContext) {
        ctx.bus_writer
            .provide::<dyn PendingService>(self.svc.clone() as Arc<dyn PendingService>);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn add_returns_uuid_and_lists_unread() {
        let svc = PendingServiceImpl::new();
        let id = svc.add("hello");
        assert!(!id.is_empty());
        let unread = svc.list_unread();
        assert_eq!(unread.len(), 1);
        assert_eq!(unread[0].uuid, id);
        assert_eq!(unread[0].text, "hello");
        assert!(unread[0].read_at.is_none());
    }

    #[test]
    fn cancel_removes_unread() {
        let svc = PendingServiceImpl::new();
        let id = svc.add("oops");
        assert!(svc.cancel(&id));
        assert!(svc.list_unread().is_empty());
        assert!(svc.list_all().is_empty());
    }

    #[test]
    fn cancel_unknown_uuid_returns_false() {
        let svc = PendingServiceImpl::new();
        svc.add("a");
        assert!(!svc.cancel("nonexistent"));
        assert_eq!(svc.list_unread().len(), 1);
    }

    #[test]
    fn drain_marks_read_and_keeps_history() {
        let svc = PendingServiceImpl::new();
        svc.add("first");
        svc.add("second");

        let drained = svc.drain_unread();
        assert_eq!(drained.len(), 2);

        // After drain: nothing unread, but list_all still shows them.
        assert!(svc.list_unread().is_empty());
        let all = svc.list_all();
        assert_eq!(all.len(), 2);
        assert!(all.iter().all(|m| m.read_at.is_some()));
    }

    #[test]
    fn cancel_after_drain_returns_false() {
        let svc = PendingServiceImpl::new();
        let id = svc.add("x");
        let _ = svc.drain_unread();
        assert!(!svc.cancel(&id), "cannot cancel a message that was already drained");
    }

    #[test]
    fn drain_is_idempotent_on_empty() {
        let svc = PendingServiceImpl::new();
        svc.add("x");
        let _ = svc.drain_unread();
        let again = svc.drain_unread();
        assert!(again.is_empty());
    }
}
